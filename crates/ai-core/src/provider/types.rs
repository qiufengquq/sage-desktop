//! Provider-agnostic request contract.
//!
//! Everything here is free of vendor vocabulary: adapters translate these types
//! into `/chat/completions`, `/messages`, ... and translate the answers back.
//! That keeps the rest of the framework independent of any single provider and
//! makes switching providers a routing decision instead of a code change.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::error::{CoreError, CoreResult};
use crate::provider::config::ModelSpec;
use crate::provider::response::ToolCall;

/// Author of a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    /// Instruction that precedes the conversation.
    System,
    /// End-user input.
    User,
    /// Previous model output replayed into the context.
    Assistant,
    /// Result of a tool call (tool execution itself is Phase 4).
    Tool,
}

impl Role {
    /// Lowercase wire label (`system`, `user`, ...).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One piece of message content.
///
/// Text and images are the only modalities Phase 2 needs; tool-call payloads are
/// represented on the response side ([`crate::provider::ToolCall`]) and by
/// [`Role::Tool`] messages on the request side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentPart {
    /// Plain text.
    Text(String),
    /// Image referenced by URL (or `data:` URI).
    Image {
        /// Image location.
        url: String,
    },
}

impl ContentPart {
    /// Text content, if this part is text.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::Image { .. } => None,
        }
    }

    /// Image URL, if this part is an image.
    #[must_use]
    pub fn as_image_url(&self) -> Option<&str> {
        match self {
            Self::Image { url } => Some(url),
            Self::Text(_) => None,
        }
    }
}

/// One conversation message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    role: Role,
    parts: Vec<ContentPart>,
    tool_call_id: Option<String>,
    tool_calls: Vec<ToolCall>,
}

impl Message {
    /// Builds a message from `role` and a single text part.
    pub fn text(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            parts: vec![ContentPart::Text(content.into())],
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    /// System instruction message.
    pub fn system(content: impl Into<String>) -> Self {
        Self::text(Role::System, content)
    }

    /// User message.
    pub fn user(content: impl Into<String>) -> Self {
        Self::text(Role::User, content)
    }

    /// Assistant message (usually replayed context).
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::text(Role::Assistant, content)
    }

    /// Tool result message without a call correlation id.
    ///
    /// Prefer [`Message::tool_result`] for real provider round-trips: OpenAI and
    /// Anthropic both require the result to point back at the call it answers.
    pub fn tool(content: impl Into<String>) -> Self {
        Self::text(Role::Tool, content)
    }

    /// Tool result message correlated with the call it answers.
    #[must_use]
    pub fn tool_result(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        let mut message = Self::text(Role::Tool, content);
        message.tool_call_id = Some(call_id.into());
        message
    }

    /// Assistant message replaying model text together with its tool calls.
    pub fn assistant_tool_calls(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        let mut message = Self::text(Role::Assistant, content);
        message.tool_calls = tool_calls;
        message
    }

    /// Call id this tool message answers, when correlated.
    #[must_use]
    pub fn tool_call_id(&self) -> Option<&str> {
        self.tool_call_id.as_deref()
    }

    /// Tool calls requested by an assistant message.
    #[must_use]
    pub fn tool_calls(&self) -> &[ToolCall] {
        &self.tool_calls
    }

    /// Author of the message.
    #[must_use]
    pub fn role(&self) -> Role {
        self.role
    }

    /// Content parts, in order.
    #[must_use]
    pub fn parts(&self) -> &[ContentPart] {
        &self.parts
    }

    /// Appends a content part.
    pub fn push_part(&mut self, part: ContentPart) {
        self.parts.push(part);
    }

    /// Builder form of [`Message::push_part`].
    #[must_use]
    pub fn with_part(mut self, part: ContentPart) -> Self {
        self.parts.push(part);
        self
    }

    /// Builder form for an image attachment.
    #[must_use]
    pub fn with_image(mut self, url: impl Into<String>) -> Self {
        self.parts.push(ContentPart::Image { url: url.into() });
        self
    }

    /// Whether the message carries an image part.
    #[must_use]
    pub fn has_image(&self) -> bool {
        self.parts.iter().any(|part| part.as_image_url().is_some())
    }

    /// Concatenation of all text parts (used for token estimation and logging).
    #[must_use]
    pub fn text_content(&self) -> String {
        let mut buffer = String::new();
        for part in &self.parts {
            if let Some(text) = part.as_text() {
                buffer.push_str(text);
            }
        }
        buffer
    }
}

/// Declaration of a callable function.
///
/// The provider layer only *declares* tools, so the capability check can reject
/// models that cannot call them; validating arguments and executing the call
/// belongs to [`crate::tool`], and replaying the result belongs to
/// [`crate::output::RoundLoop`].
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

impl ToolSpec {
    /// Builds a tool declaration from a name, a description and a JSON schema.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] when the name is empty or the
    /// schema is not a JSON object.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> CoreResult<Self> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(CoreError::invalid_argument(
                "tool.name",
                "must not be empty",
            ));
        }
        if !parameters.is_object() {
            return Err(CoreError::invalid_argument(
                "tool.parameters",
                "must be a JSON schema object",
            ));
        }
        Ok(Self {
            name,
            description: description.into(),
            parameters,
        })
    }

    /// Function name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Function description.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// JSON schema of the arguments.
    #[must_use]
    pub fn parameters(&self) -> &serde_json::Value {
        &self.parameters
    }
}

/// Requested output format.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ResponseFormat {
    /// Free-form text (default).
    #[default]
    Text,
    /// Provider-native JSON mode (valid JSON, schema unspecified).
    JsonObject,
    /// JSON constrained by a schema.
    JsonSchema {
        /// Schema name reported to the provider.
        name: String,
        /// JSON schema.
        schema: serde_json::Value,
    },
}

impl ResponseFormat {
    /// Whether the format constrains the output beyond plain text.
    #[must_use]
    pub fn is_structured(&self) -> bool {
        !matches!(self, Self::Text)
    }
}

/// A provider-agnostic generation request.
///
/// Use the builder methods ([`AiRequest::with_model`], ...) and pass the value to
/// [`crate::provider::ProviderRuntime::complete`] or
/// [`crate::provider::ProviderRuntime::stream`], which validate it before any
/// network call is made.
#[derive(Debug, Clone)]
pub struct AiRequest {
    model: Option<ModelSpec>,
    messages: Vec<Message>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    max_output_tokens: Option<u32>,
    stop: Vec<String>,
    tools: Vec<ToolSpec>,
    response_format: ResponseFormat,
    extra_body: BTreeMap<String, serde_json::Value>,
    timeout: Option<Duration>,
}

impl Default for AiRequest {
    fn default() -> Self {
        Self {
            model: None,
            messages: Vec::new(),
            temperature: None,
            top_p: None,
            max_output_tokens: None,
            stop: Vec::new(),
            tools: Vec::new(),
            response_format: ResponseFormat::Text,
            extra_body: BTreeMap::new(),
            timeout: None,
        }
    }
}

impl AiRequest {
    /// Builds a request from messages.
    pub fn new(messages: impl IntoIterator<Item = Message>) -> Self {
        Self {
            messages: messages.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Builds a single-turn user request.
    pub fn user_prompt(prompt: impl Into<String>) -> Self {
        Self::new([Message::user(prompt)])
    }

    /// Pins the request to an explicit `provider/model` pair.
    ///
    /// Without this, the registry resolves the default provider/model of the
    /// configuration snapshot the request runs against.
    #[must_use]
    pub fn with_model(mut self, model: ModelSpec) -> Self {
        self.model = Some(model);
        self
    }

    /// Sampling temperature.
    #[must_use]
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Nucleus sampling probability.
    #[must_use]
    pub fn with_top_p(mut self, top_p: f32) -> Self {
        self.top_p = Some(top_p);
        self
    }

    /// Maximum number of generated tokens.
    #[must_use]
    pub fn with_max_output_tokens(mut self, tokens: u32) -> Self {
        self.max_output_tokens = Some(tokens);
        self
    }

    /// Adds stop sequences.
    #[must_use]
    pub fn with_stop<I, S>(mut self, stop: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.stop.extend(stop.into_iter().map(Into::into));
        self
    }

    /// Declares callable tools.
    #[must_use]
    pub fn with_tools(mut self, tools: impl IntoIterator<Item = ToolSpec>) -> Self {
        self.tools.extend(tools);
        self
    }

    /// Appends one message (used by the tool loop to replay rounds).
    pub fn push_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    /// Builder form of [`AiRequest::push_message`].
    #[must_use]
    pub fn with_message(mut self, message: Message) -> Self {
        self.messages.push(message);
        self
    }

    /// Requests a specific output format.
    #[must_use]
    pub fn with_response_format(mut self, format: ResponseFormat) -> Self {
        self.response_format = format;
        self
    }

    /// Merges an extra JSON field into the request body.
    ///
    /// Request-level extras override provider-level
    /// [`crate::provider::ProviderConfig::extra_body`].
    #[must_use]
    pub fn with_extra_body(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.extra_body.insert(key.into(), value);
        self
    }

    /// Overrides the overall request budget (all attempts and fallbacks included).
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Requested model, when explicitly pinned.
    #[must_use]
    pub fn model(&self) -> Option<&ModelSpec> {
        self.model.as_ref()
    }

    /// Conversation messages.
    #[must_use]
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Sampling temperature.
    #[must_use]
    pub fn temperature(&self) -> Option<f32> {
        self.temperature
    }

    /// Nucleus sampling probability.
    #[must_use]
    pub fn top_p(&self) -> Option<f32> {
        self.top_p
    }

    /// Maximum generated tokens.
    #[must_use]
    pub fn max_output_tokens(&self) -> Option<u32> {
        self.max_output_tokens
    }

    /// Stop sequences.
    #[must_use]
    pub fn stop(&self) -> &[String] {
        &self.stop
    }

    /// Declared tools.
    #[must_use]
    pub fn tools(&self) -> &[ToolSpec] {
        &self.tools
    }

    /// Requested output format.
    #[must_use]
    pub fn response_format(&self) -> &ResponseFormat {
        &self.response_format
    }

    /// Extra body fields.
    #[must_use]
    pub fn extra_body(&self) -> &BTreeMap<String, serde_json::Value> {
        &self.extra_body
    }

    /// Request budget override.
    #[must_use]
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Whether the request declares tools.
    #[must_use]
    pub fn requires_tool_calling(&self) -> bool {
        !self.tools.is_empty()
    }

    /// Whether the request carries an image part.
    #[must_use]
    pub fn requires_vision(&self) -> bool {
        self.messages.iter().any(Message::has_image)
    }

    /// Validates the request before any routing happens.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] for an empty conversation, an
    /// out-of-range sampling parameter, a duplicate tool name or an invalid
    /// budget.
    pub fn validate(&self) -> CoreResult<()> {
        if self.messages.is_empty() {
            return Err(CoreError::invalid_argument(
                "messages",
                "must contain at least one message",
            ));
        }
        if self.messages.iter().all(|message| {
            message.parts().iter().all(|part| match part {
                ContentPart::Text(text) => text.trim().is_empty(),
                ContentPart::Image { .. } => false,
            })
        }) {
            return Err(CoreError::invalid_argument(
                "messages",
                "must contain some non-empty content",
            ));
        }
        for message in &self.messages {
            if message.role() == Role::Tool && message.tool_call_id().is_none() {
                return Err(CoreError::invalid_argument(
                    "messages",
                    "tool result messages must carry a tool_call_id",
                ));
            }
            if !message.tool_calls().is_empty() && message.role() != Role::Assistant {
                return Err(CoreError::invalid_argument(
                    "messages",
                    "only assistant messages may carry tool calls",
                ));
            }
        }
        if let Some(temperature) = self.temperature {
            if !temperature.is_finite() || !(0.0..=2.0).contains(&temperature) {
                return Err(CoreError::invalid_argument(
                    "temperature",
                    "must be a finite value between 0.0 and 2.0",
                ));
            }
        }
        if let Some(top_p) = self.top_p {
            if !top_p.is_finite() || !(0.0..=1.0).contains(&top_p) {
                return Err(CoreError::invalid_argument(
                    "top_p",
                    "must be a finite value between 0.0 and 1.0",
                ));
            }
        }
        if let Some(tokens) = self.max_output_tokens {
            if tokens == 0 {
                return Err(CoreError::invalid_argument(
                    "max_output_tokens",
                    "must be greater than zero",
                ));
            }
        }
        for (index, tool) in self.tools.iter().enumerate() {
            if self.tools[..index]
                .iter()
                .any(|other| other.name() == tool.name())
            {
                return Err(CoreError::invalid_argument(
                    format!("tools[{index}].name"),
                    format!("duplicate tool `{}`", tool.name()),
                ));
            }
        }
        for (index, stop) in self.stop.iter().enumerate() {
            if stop.is_empty() {
                return Err(CoreError::invalid_argument(
                    format!("stop[{index}]"),
                    "must not be empty",
                ));
            }
        }
        if let Some(timeout) = self.timeout {
            if timeout.is_zero() {
                return Err(CoreError::invalid_argument(
                    "timeout",
                    "must be greater than zero",
                ));
            }
        }
        Ok(())
    }
}

/// A provider-agnostic embedding request.
///
/// An embedding call has no messages, no tools and no output budget: it maps a
/// batch of texts to one vector each. The model is optional and falls back to the
/// routed default, exactly like [`AiRequest::model`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EmbeddingRequest {
    model: Option<ModelSpec>,
    inputs: Vec<String>,
    dimensions: Option<u32>,
    timeout: Option<Duration>,
}

impl EmbeddingRequest {
    /// Builds a request from a batch of texts.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] when the batch is empty, larger
    /// than [`MAX_EMBEDDING_INPUTS`], or contains an empty text.
    pub fn new<I, S>(inputs: I) -> CoreResult<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let inputs: Vec<String> = inputs.into_iter().map(Into::into).collect();
        let request = Self {
            model: None,
            inputs,
            dimensions: None,
            timeout: None,
        };
        request.validate()?;
        Ok(request)
    }

    /// Builds a single-text request.
    ///
    /// # Errors
    ///
    /// See [`EmbeddingRequest::new`].
    pub fn single(input: impl Into<String>) -> CoreResult<Self> {
        Self::new([input])
    }

    /// Routes the call to an explicit model.
    #[must_use]
    pub fn with_model(mut self, model: ModelSpec) -> Self {
        self.model = Some(model);
        self
    }

    /// Asks the provider for a specific vector width, when it supports it.
    #[must_use]
    pub fn with_dimensions(mut self, dimensions: u32) -> Self {
        self.dimensions = Some(dimensions);
        self
    }

    /// Overrides the configured request deadline.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Explicit model, if any.
    #[must_use]
    pub fn model(&self) -> Option<&ModelSpec> {
        self.model.as_ref()
    }

    /// Texts to embed, in input order.
    #[must_use]
    pub fn inputs(&self) -> &[String] {
        &self.inputs
    }

    /// Requested vector width.
    #[must_use]
    pub const fn dimensions(&self) -> Option<u32> {
        self.dimensions
    }

    /// Requested deadline override.
    #[must_use]
    pub const fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Number of texts in the batch.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inputs.len()
    }

    /// Whether the batch is empty (never valid).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty()
    }

    /// Re-checks the request invariants.
    ///
    /// # Errors
    ///
    /// See [`EmbeddingRequest::new`], plus [`CoreError::InvalidArgument`] when a
    /// dimension override or the timeout is zero.
    pub fn validate(&self) -> CoreResult<()> {
        if self.inputs.is_empty() {
            return Err(CoreError::invalid_argument(
                "embedding.inputs",
                "must contain at least one text",
            ));
        }
        if self.inputs.len() > MAX_EMBEDDING_INPUTS {
            return Err(CoreError::invalid_argument(
                "embedding.inputs",
                format!("must contain at most {MAX_EMBEDDING_INPUTS} texts"),
            ));
        }
        if let Some(index) = self.inputs.iter().position(|input| input.trim().is_empty()) {
            return Err(CoreError::invalid_argument(
                format!("embedding.inputs.{index}"),
                "must not be empty",
            ));
        }
        if self.dimensions == Some(0) {
            return Err(CoreError::invalid_argument(
                "embedding.dimensions",
                "must be greater than zero",
            ));
        }
        if self.timeout.is_some_and(|timeout| timeout.is_zero()) {
            return Err(CoreError::invalid_argument(
                "embedding.timeout",
                "must be greater than zero",
            ));
        }
        Ok(())
    }
}

/// Upper bound on the number of texts in one embedding batch.
pub const MAX_EMBEDDING_INPUTS: usize = 2_048;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn messages_carry_text_and_images() {
        let message = Message::user("hello").with_image("https://example.com/a.png");
        assert_eq!(message.role(), Role::User);
        assert!(message.has_image());
        assert_eq!(message.text_content(), "hello");
        assert_eq!(message.parts().len(), 2);
    }

    #[test]
    fn request_validation_rejects_bad_input() {
        assert!(AiRequest::user_prompt("hi").validate().is_ok());
        assert!(AiRequest::new(Vec::new()).validate().is_err());
        assert!(AiRequest::user_prompt("   ").validate().is_err());
        assert!(AiRequest::user_prompt("hi")
            .with_temperature(3.0)
            .validate()
            .is_err());
        assert!(AiRequest::user_prompt("hi")
            .with_top_p(f32::NAN)
            .validate()
            .is_err());
        assert!(AiRequest::user_prompt("hi")
            .with_max_output_tokens(0)
            .validate()
            .is_err());
        assert!(AiRequest::user_prompt("hi")
            .with_timeout(Duration::ZERO)
            .validate()
            .is_err());
    }

    #[test]
    fn duplicate_tools_are_rejected() {
        let schema = serde_json::json!({"type": "object"});
        let tool = ToolSpec::new("lookup", "look up a word", schema.clone()).unwrap();
        let duplicate = ToolSpec::new("lookup", "again", schema).unwrap();
        let request = AiRequest::user_prompt("hi").with_tools([tool, duplicate]);
        assert!(request.validate().is_err());
        assert!(request.requires_tool_calling());
    }

    #[test]
    fn empty_tool_schema_is_rejected() {
        assert!(ToolSpec::new("t", "d", serde_json::json!("not-an-object")).is_err());
        assert!(ToolSpec::new("", "d", serde_json::json!({})).is_err());
    }

    #[test]
    fn response_formats_flag_structured_output() {
        assert!(!ResponseFormat::Text.is_structured());
        assert!(ResponseFormat::JsonObject.is_structured());
        assert!(ResponseFormat::JsonSchema {
            name: "s".to_owned(),
            schema: serde_json::json!({"type": "object"})
        }
        .is_structured());
    }
}
