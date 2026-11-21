//! Event names and envelopes handed to the host.

use ai_core::error::CoreError;
use ai_core::output::CoreOutputEvent;
use ai_core::CoreResult;
use serde::{Deserialize, Serialize};

/// Prefix of every event emitted by this adapter.
///
/// Keeping a namespaced name (`ai-core://text-delta`) prevents collisions with
/// the host's own events and makes listeners easy to scope.
pub const EVENT_PREFIX: &str = "ai-core://";

/// What the host forwards: an event name plus its JSON payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// Fully qualified event name, e.g. `ai-core://text-delta`.
    pub event: String,
    /// Event payload, already serialized (the host emits it as-is).
    pub payload: serde_json::Value,
}

/// Stable event name of a core output event (without the prefix).
#[must_use]
pub fn event_name(event: &CoreOutputEvent) -> &'static str {
    match event {
        CoreOutputEvent::Started { .. } => "started",
        CoreOutputEvent::TextDelta { .. } => "text-delta",
        CoreOutputEvent::ReasoningDelta { .. } => "reasoning-delta",
        CoreOutputEvent::ToolCallStarted { .. } => "tool-call-started",
        CoreOutputEvent::ToolCallDelta { .. } => "tool-call-delta",
        CoreOutputEvent::ToolCallCompleted { .. } => "tool-call-completed",
        CoreOutputEvent::ToolCallExecuted { .. } => "tool-call-executed",
        CoreOutputEvent::StructuredOutput { .. } => "structured-output",
        CoreOutputEvent::UsageUpdated { .. } => "usage-updated",
        CoreOutputEvent::Error { .. } => "error",
        CoreOutputEvent::Completed { .. } => "completed",
    }
}

/// Serializes an event into a host-ready envelope.
///
/// # Errors
///
/// Returns [`CoreError::Serialization`] if the event cannot be encoded, which
/// would indicate a programming error rather than bad user data.
pub fn encode(event: &CoreOutputEvent) -> CoreResult<EventEnvelope> {
    let payload = serde_json::to_value(event).map_err(|error| {
        CoreError::serialization_with_source("failed to encode an output event", error)
    })?;
    Ok(EventEnvelope {
        event: format!("{EVENT_PREFIX}{}", event_name(event)),
        payload,
    })
}
