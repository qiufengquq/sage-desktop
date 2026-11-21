//! Unified output events.
//!
//! One vocabulary for every transport: a host renders the same event stream
//! whether the answer arrived as a single response or as a token stream. Events
//! carry identifiers and payloads only — no Tauri, no UI, no transport type.

use serde::{Deserialize, Serialize};

use crate::ids::{RequestId, TraceId};
use crate::output::structured::StructuredOutput;
use crate::provider::config::ProviderId;
use crate::provider::response::{FinishReason, UsageReport};

/// Event emitted by the output engine.
///
/// # Ordering contract
///
/// 1. [`CoreOutputEvent::Started`] is always first.
/// 2. Deltas follow in the order the provider produced them.
/// 3. [`CoreOutputEvent::ToolCallStarted`] precedes the deltas of the same call,
///    and [`CoreOutputEvent::ToolCallCompleted`] closes it once the model has
///    finished emitting its arguments. Execution results follow as
///    [`CoreOutputEvent::ToolCallExecuted`], one per call, still before the
///    terminal `Completed`.
/// 4. Any number of [`CoreOutputEvent::UsageUpdated`] events may appear.
/// 5. Exactly one terminal event: [`CoreOutputEvent::Completed`]. An
///    [`CoreOutputEvent::Error`] may appear immediately before it; the completion
///    then reports `interrupted = true` and still carries the partial text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreOutputEvent {
    /// The call was accepted and routed to a provider.
    Started {
        /// Request identifier, when the request went through the router.
        request_id: Option<RequestId>,
        /// Trace identifier, when available.
        trace_id: Option<TraceId>,
        /// Provider serving the call.
        provider_id: ProviderId,
        /// Wire model identifier.
        model_id: String,
        /// Whether the caller should expect incremental deltas.
        streaming: bool,
    },
    /// Incremental answer text.
    TextDelta {
        /// Newly generated text fragment.
        text: String,
    },
    /// Incremental reasoning/thinking text.
    ReasoningDelta {
        /// Newly generated reasoning fragment.
        text: String,
    },
    /// The model started emitting a tool call.
    ToolCallStarted {
        /// Position of the call in the response.
        index: u16,
        /// Provider-assigned call id.
        call_id: String,
        /// Tool name.
        name: String,
    },
    /// Incremental tool-call arguments (JSON fragment, not necessarily valid yet).
    ToolCallDelta {
        /// Position of the call in the response.
        index: u16,
        /// Argument fragment.
        arguments_delta: String,
    },
    /// The model finished emitting a tool call; arguments are complete.
    ///
    /// This is the *model-side* completion. Execution results are reported
    /// separately as [`CoreOutputEvent::ToolCallExecuted`].
    ToolCallCompleted {
        /// Position of the call in the response.
        index: u16,
        /// Provider-assigned call id, echoed back when submitting the result.
        call_id: String,
        /// Tool name.
        name: String,
        /// Fully assembled JSON argument string.
        arguments: String,
    },
    /// A tool call was executed by the tool engine.
    ///
    /// `output` is the serialized tool result on success, or a redacted,
    /// structured error message on failure. Phase 5 addition.
    ToolCallExecuted {
        /// Position of the call in the response.
        index: u16,
        /// Provider-assigned call id.
        call_id: String,
        /// Tool name.
        name: String,
        /// Whether the tool completed successfully.
        success: bool,
        /// Serialized result or error message.
        output: String,
        /// Execution duration in milliseconds.
        duration_ms: u64,
    },
    /// A validated structured (JSON) output was produced.
    StructuredOutput {
        /// Raw text, parsed value and validation metadata.
        output: StructuredOutput,
    },
    /// The provider reported token usage for the call.
    UsageUpdated {
        /// Provider-measured usage (never a local estimate).
        usage: UsageReport,
    },
    /// A failure occurred; `message` is already redacted.
    Error {
        /// Stable `CORE_*` error code.
        code: String,
        /// Redacted human-readable message.
        message: String,
        /// Whether the same call may be retried.
        retryable: bool,
    },
    /// Terminal event of the call.
    Completed {
        /// Why the model stopped, when a terminal provider event was seen.
        finish_reason: Option<FinishReason>,
        /// Full accumulated answer text (partial when interrupted).
        text: String,
        /// Full accumulated reasoning text.
        reasoning: String,
        /// Final usage, when the provider reported one.
        usage: Option<UsageReport>,
        /// `true` when the call ended because of an error, timeout or cancellation.
        interrupted: bool,
    },
}

impl CoreOutputEvent {
    /// Whether this is the terminal event of the stream.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed { .. })
    }

    /// Whether this event carries generated text.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::TextDelta { text } => Some(text),
            _ => None,
        }
    }
}

/// One fully assembled tool call, as reported by the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputToolCall {
    /// Position in the response.
    pub index: u16,
    /// Provider-assigned call id.
    pub call_id: String,
    /// Tool name.
    pub name: String,
    /// Complete JSON argument string.
    pub arguments: String,
}

/// Redacted error attached to an output outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputError {
    /// Stable `CORE_*` error code.
    pub code: String,
    /// Redacted message (never contains prompts, completions or credentials).
    pub message: String,
    /// Whether a retry could help.
    pub retryable: bool,
}

impl OutputError {
    /// Builds a redacted error payload from a [`CoreError`](crate::CoreError).
    #[must_use]
    pub fn from_error(error: &crate::CoreError) -> Self {
        Self {
            code: error.error_code().to_owned(),
            message: crate::secret::sanitize_log_text(&error.to_string()),
            retryable: error.is_retryable(),
        }
    }
}

/// Result of driving an output stream to its end.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputOutcome {
    /// Accumulated answer text (partial when `interrupted`).
    pub text: String,
    /// Accumulated reasoning text.
    pub reasoning: String,
    /// Structured output, when JSON was requested and validation succeeded.
    pub structured: Option<StructuredOutput>,
    /// Tool calls emitted by the model.
    pub tool_calls: Vec<OutputToolCall>,
    /// Final usage reported by the provider.
    pub usage: Option<UsageReport>,
    /// Provider finish reason.
    pub finish_reason: Option<FinishReason>,
    /// `true` when the call ended early (error, timeout or cancellation).
    pub interrupted: bool,
    /// Redacted error that interrupted the call.
    pub error: Option<OutputError>,
}

impl OutputOutcome {
    /// Whether the call produced a complete, non-interrupted answer.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        !self.interrupted
    }

    /// Whether the model requested at least one tool call.
    #[must_use]
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }
}
