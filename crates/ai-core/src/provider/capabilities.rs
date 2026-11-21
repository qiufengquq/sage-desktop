//! Capability and token-budget validation.
//!
//! Validation runs **before** a request is dispatched, so an unsupported request
//! never reaches the network and never costs money. Two classes of problems are
//! detected:
//!
//! * a required capability is not declared by the target model →
//!   [`CoreError::CapabilityUnsupported`] (carries provider, model and capability);
//! * the estimated input plus the requested output does not fit the declared
//!   context window, or the requested output exceeds the model maximum →
//!   [`CoreError::TokenLimitExceeded`].
//!
//! Token counts are a documented **heuristic** (≈4 characters per token, a
//! constant per image, plus message/tool overhead). It exists to catch gross
//! misconfiguration; a real tokenizer belongs to the context subsystem and can
//! replace [`estimate_input_tokens`] later without changing the checks.

use std::fmt;

use crate::error::{CoreError, CoreResult};
use crate::provider::config::{ModelCapabilities, ProviderId};
use crate::provider::types::{AiRequest, ContentPart, EmbeddingRequest};

/// Heuristic weight of one image part, in tokens.
pub const IMAGE_TOKEN_ESTIMATE: u32 = 1_024;
/// Characters per token used by the estimator.
pub const CHARS_PER_TOKEN: u32 = 4;
/// Per-message overhead of chat framing.
pub const MESSAGE_OVERHEAD_TOKENS: u32 = 4;

/// A capability a request may require from a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    /// Server-sent streaming.
    Streaming,
    /// Function/tool calling.
    ToolCalling,
    /// JSON / JSON-schema constrained output.
    StructuredOutput,
    /// Image input.
    Vision,
    /// Embedding generation.
    Embedding,
    /// Extended reasoning output.
    Reasoning,
}

impl Capability {
    /// Stable lowercase label used in errors and logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Streaming => "streaming",
            Self::ToolCalling => "tool_calling",
            Self::StructuredOutput => "structured_output",
            Self::Vision => "vision",
            Self::Embedding => "embedding",
            Self::Reasoning => "reasoning",
        }
    }

    /// Whether `capabilities` declares this capability.
    #[must_use]
    pub const fn is_supported_by(self, capabilities: &ModelCapabilities) -> bool {
        match self {
            Self::Streaming => capabilities.streaming,
            Self::ToolCalling => capabilities.tool_calling,
            Self::StructuredOutput => capabilities.structured_output,
            Self::Vision => capabilities.vision,
            Self::Embedding => capabilities.embedding,
            Self::Reasoning => capabilities.reasoning,
        }
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a request needs from a model, derived from the request itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestRequirements {
    /// The call will consume a streaming response.
    pub streaming: bool,
    /// The request declares tools.
    pub tool_calling: bool,
    /// The request asks for structured output.
    pub structured_output: bool,
    /// The request contains an image part.
    pub vision: bool,
    /// Estimated input tokens.
    pub estimated_input_tokens: u32,
    /// Requested output tokens (model default when unset).
    pub requested_output_tokens: u32,
}

impl RequestRequirements {
    /// Derives the requirements of a request.
    ///
    /// `requested_output_tokens` falls back to the model's declared maximum when
    /// the request does not specify a budget; the caller can refine it with
    /// [`RequestRequirements::clamp_output_tokens`].
    #[must_use]
    pub fn from_request(request: &AiRequest, streaming: bool) -> Self {
        Self {
            streaming,
            tool_calling: request.requires_tool_calling(),
            structured_output: request.response_format().is_structured(),
            vision: request.requires_vision(),
            estimated_input_tokens: estimate_input_tokens(request),
            requested_output_tokens: request.max_output_tokens().unwrap_or(0),
        }
    }

    /// Uses the model maximum when the request did not ask for a specific budget.
    #[must_use]
    pub fn clamp_output_tokens(mut self, capabilities: &ModelCapabilities) -> Self {
        if self.requested_output_tokens == 0 {
            self.requested_output_tokens = capabilities.max_output_tokens;
        }
        self
    }

    /// Whether the request requires `capability`.
    #[must_use]
    pub const fn requires(&self, capability: Capability) -> bool {
        match capability {
            Capability::Streaming => self.streaming,
            Capability::ToolCalling => self.tool_calling,
            Capability::StructuredOutput => self.structured_output,
            Capability::Vision => self.vision,
            // Phase 2 only issues chat/completion requests; embedding and
            // reasoning are declared capabilities used for model selection.
            Capability::Embedding | Capability::Reasoning => false,
        }
    }

    /// Capabilities required by this request that `capabilities` does not declare.
    #[must_use]
    pub fn mismatches(&self, capabilities: &ModelCapabilities) -> Vec<Capability> {
        [
            Capability::Streaming,
            Capability::ToolCalling,
            Capability::StructuredOutput,
            Capability::Vision,
        ]
        .into_iter()
        .filter(|capability| {
            self.requires(*capability) && !capability.is_supported_by(capabilities)
        })
        .collect()
    }
}

/// Estimates the token count of a bare text fragment.
///
/// No per-message overhead is added: this is the building block used for
/// prompt parts and for estimating generated output from its length. The result
/// is always an **estimate** and must never be reported as provider-measured
/// usage.
#[must_use]
pub fn estimate_text_tokens(text: &str) -> u32 {
    let characters = u64::try_from(text.chars().count()).unwrap_or(u64::MAX);
    u32::try_from(characters.div_ceil(u64::from(CHARS_PER_TOKEN))).unwrap_or(u32::MAX)
}

/// Estimates the input token count of a request.
///
/// See the module documentation: this is a coarse heuristic, not a tokenizer.
#[must_use]
pub fn estimate_input_tokens(request: &AiRequest) -> u32 {
    let mut total: u64 = 0;
    for message in request.messages() {
        total += u64::from(MESSAGE_OVERHEAD_TOKENS);
        for part in message.parts() {
            match part {
                ContentPart::Text(text) => total += u64::from(estimate_text_tokens(text)),
                ContentPart::Image { .. } => total += u64::from(IMAGE_TOKEN_ESTIMATE),
            }
        }
    }
    for tool in request.tools() {
        let schema = serde_json::to_string(tool.parameters()).unwrap_or_default();
        let characters = u64::try_from(schema.chars().count()).unwrap_or(u64::MAX);
        total += characters.div_ceil(u64::from(CHARS_PER_TOKEN));
    }
    u32::try_from(total).unwrap_or(u32::MAX)
}

/// Estimates the input tokens of an embedding batch.
///
/// Embedding providers bill input tokens only, so there is no output component
/// and no message framing overhead.
#[must_use]
pub fn estimate_embedding_input_tokens(request: &EmbeddingRequest) -> u32 {
    let total: u64 = request
        .inputs()
        .iter()
        .map(|text| u64::from(estimate_text_tokens(text)))
        .sum();
    u32::try_from(total).unwrap_or(u32::MAX)
}

/// Validates a request against a model's declared capabilities and limits.
///
/// # Errors
///
/// * [`CoreError::CapabilityUnsupported`] when a required capability is missing;
/// * [`CoreError::TokenLimitExceeded`] when input/output do not fit.
pub fn check_capabilities(
    provider: &ProviderId,
    model: &str,
    capabilities: &ModelCapabilities,
    requirements: &RequestRequirements,
) -> CoreResult<()> {
    let mismatches = requirements.mismatches(capabilities);
    if let Some(capability) = mismatches.first() {
        return Err(CoreError::capability_unsupported(
            provider.to_string(),
            model.to_owned(),
            capability.as_str(),
        ));
    }

    let requested_output = requirements.requested_output_tokens;
    if requested_output > capabilities.max_output_tokens {
        return Err(CoreError::token_limit(format!(
            "provider=`{provider}` model=`{model}` requested max_output_tokens={requested_output} \
             exceeds the declared maximum of {}",
            capabilities.max_output_tokens
        )));
    }

    let total = u64::from(requirements.estimated_input_tokens) + u64::from(requested_output);
    if total > u64::from(capabilities.context_window) {
        return Err(CoreError::token_limit(format!(
            "provider=`{provider}` model=`{model}` estimated input ({} tokens) plus requested output \
             ({requested_output} tokens) exceeds the context window of {} tokens",
            requirements.estimated_input_tokens, capabilities.context_window
        )));
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::provider::types::{Message, ResponseFormat};

    fn capabilities() -> ModelCapabilities {
        ModelCapabilities {
            streaming: true,
            tool_calling: true,
            structured_output: true,
            vision: false,
            embedding: false,
            reasoning: true,
            context_window: 8_000,
            max_output_tokens: 1_000,
        }
    }

    fn provider() -> ProviderId {
        ProviderId::new("mock").unwrap()
    }

    #[test]
    fn estimator_scales_with_text_length() {
        let short = AiRequest::user_prompt("hello");
        let long = AiRequest::user_prompt("a".repeat(4_000));
        assert!(estimate_input_tokens(&long) > estimate_input_tokens(&short));
        assert!(estimate_input_tokens(&short) >= MESSAGE_OVERHEAD_TOKENS);
    }

    #[test]
    fn missing_capability_is_reported_with_context() {
        let mut capabilities = capabilities();
        capabilities.streaming = false;

        let request = AiRequest::user_prompt("hi");
        let requirements = RequestRequirements::from_request(&request, true);
        let error = check_capabilities(&provider(), "mock-1", &capabilities, &requirements)
            .expect_err("streaming must be rejected");
        let message = error.to_string();
        assert!(message.contains("streaming"));
        assert!(message.contains("mock-1"));
        assert!(message.contains("mock"));
        assert!(matches!(error, CoreError::CapabilityUnsupported { .. }));
    }

    #[test]
    fn vision_and_structured_output_are_checked() {
        let capabilities = capabilities();
        let request =
            AiRequest::new([Message::user("describe").with_image("https://example.com/a.png")])
                .with_response_format(ResponseFormat::JsonObject);
        let requirements = RequestRequirements::from_request(&request, false);
        assert_eq!(
            requirements.mismatches(&capabilities),
            vec![Capability::Vision]
        );
    }

    #[test]
    fn output_budget_above_model_maximum_is_rejected() {
        let capabilities = capabilities();
        let request = AiRequest::user_prompt("hi").with_max_output_tokens(5_000);
        let requirements =
            RequestRequirements::from_request(&request, false).clamp_output_tokens(&capabilities);
        let error = check_capabilities(&provider(), "mock-1", &capabilities, &requirements)
            .expect_err("must exceed the output maximum");
        assert!(matches!(error, CoreError::TokenLimitExceeded { .. }));
        assert!(error.to_string().contains("max_output_tokens"));
    }

    #[test]
    fn context_window_is_enforced() {
        let mut capabilities = capabilities();
        capabilities.context_window = 100;
        capabilities.max_output_tokens = 100;
        let request = AiRequest::user_prompt("x".repeat(2_000));
        let requirements =
            RequestRequirements::from_request(&request, false).clamp_output_tokens(&capabilities);
        let error = check_capabilities(&provider(), "mock-1", &capabilities, &requirements)
            .expect_err("must exceed the context window");
        assert!(matches!(error, CoreError::TokenLimitExceeded { .. }));
        assert!(error.to_string().contains("context window"));
    }

    #[test]
    fn compatible_requests_pass() {
        let capabilities = capabilities();
        let request = AiRequest::user_prompt("hi").with_max_output_tokens(256);
        let requirements =
            RequestRequirements::from_request(&request, true).clamp_output_tokens(&capabilities);
        check_capabilities(&provider(), "mock-1", &capabilities, &requirements).unwrap();
    }
}
