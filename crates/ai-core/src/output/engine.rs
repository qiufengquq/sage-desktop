//! Output engine: the only place that knows how a provider answer becomes a
//! core event stream.
//!
//! ```text
//! ProviderRuntime ──AiResponse/AiStream──> OutputEngine ──OutputStream──> host
//! ```

use std::sync::Arc;

use crate::error::CoreResult;
use crate::output::event::{OutputOutcome, OutputToolCall};
use crate::output::stream::OutputStream;
use crate::output::structured::parse_structured;
use crate::provider::router::{CallContext, ProviderRuntime};
use crate::provider::types::{AiRequest, ResponseFormat};

/// Converts provider answers into the framework's output contract.
///
/// The engine owns nothing but the runtime handle, so it can be shared
/// (`Arc<OutputEngine>`) or re-created cheaply.
#[derive(Debug, Clone)]
pub struct OutputEngine {
    runtime: Arc<ProviderRuntime>,
}

impl OutputEngine {
    /// Builds an engine over an existing provider runtime.
    #[must_use]
    pub fn new(runtime: Arc<ProviderRuntime>) -> Self {
        Self { runtime }
    }

    /// The runtime used for provider calls.
    #[must_use]
    pub fn runtime(&self) -> &ProviderRuntime {
        &self.runtime
    }

    /// Runs a non-streaming call and normalizes the answer.
    ///
    /// # Errors
    ///
    /// Propagates provider errors unchanged, and returns
    /// [`CoreError::Serialization`](crate::CoreError::Serialization) /
    /// [`CoreError::InvalidArgument`](crate::CoreError::InvalidArgument) when a
    /// requested JSON contract is not satisfied.
    pub async fn complete(
        &self,
        request: AiRequest,
        context: &CallContext,
    ) -> CoreResult<OutputOutcome> {
        let format = request.response_format().clone();
        let response = self.runtime.complete(request, context).await?;

        let tool_calls = response
            .tool_calls()
            .iter()
            .enumerate()
            .map(|(index, call)| OutputToolCall {
                index: u16::try_from(index).unwrap_or(u16::MAX),
                call_id: call.id.clone(),
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            })
            .collect();

        let mut outcome = OutputOutcome {
            text: response.content().to_owned(),
            reasoning: response.reasoning().unwrap_or_default().to_owned(),
            structured: None,
            tool_calls,
            usage: response.usage().cloned(),
            finish_reason: Some(response.finish_reason()),
            interrupted: false,
            error: None,
        };

        if !matches!(format, ResponseFormat::Text) {
            outcome.structured = Some(parse_structured(&outcome.text, &format)?);
        }
        Ok(outcome)
    }

    /// Starts a streaming call.
    ///
    /// Cancellation is observed at the next poll: the resulting stream emits a
    /// redacted `Error` event and a `Completed { interrupted: true }` event that
    /// still carries everything generated so far.
    ///
    /// # Errors
    ///
    /// Propagates routing/connection errors from the provider runtime.
    pub async fn stream(
        &self,
        request: AiRequest,
        context: &CallContext,
    ) -> CoreResult<OutputStream> {
        let format = request.response_format().clone();
        let stream = self.runtime.stream(request, context).await?;
        Ok(OutputStream::new(stream, context.cancel().clone(), format))
    }
}
