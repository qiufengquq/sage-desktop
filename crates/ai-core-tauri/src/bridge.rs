//! Command layer: start, stream, cancel and map results.

use ai_core::output::{OutputEngine, OutputOutcome, OutputStream, RoundLoop, RoundOutcome};
use ai_core::provider::types::AiRequest;
use ai_core::provider::CallContext;

use crate::error::CommandError;
use crate::event::{encode, EventEnvelope};

/// Receives envelopes in order; the host emits them to the frontend.
///
/// Implementations must not block: they are called once per event on the task
/// driving the stream.
pub trait EventEmitter: Send + Sync {
    /// Forwards one envelope (e.g. `app.emit(&envelope.event, envelope.payload)`).
    fn emit(&self, envelope: EventEnvelope);
}

/// Command-side handle of one streaming request.
///
/// Holding the [`CallContext`] is what makes cancellation a command: the host
/// calls [`StreamCommand::cancel`] from a `cancel_ai_stream` Tauri command and
/// the running stream stops at its next poll.
#[derive(Debug, Clone)]
pub struct StreamCommand {
    context: CallContext,
}

impl StreamCommand {
    /// Wraps a caller context.
    #[must_use]
    pub fn new(context: CallContext) -> Self {
        Self { context }
    }

    /// The context handed to the output engine.
    #[must_use]
    pub fn context(&self) -> &CallContext {
        &self.context
    }

    /// Requests cancellation of the in-flight call.
    pub fn cancel(&self) {
        self.context.cancel().cancel();
    }

    /// Whether cancellation was already requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.context.is_cancelled()
    }
}

/// Drives a stream to completion, forwarding every event to `emitter`.
///
/// The returned outcome is always produced, even when the call was cancelled or
/// failed: `outcome.text` holds the partial answer and `outcome.error` explains
/// why it stopped. Use [`outcome_error`] to turn that into a command error after
/// the partial text has been delivered.
pub async fn drive_stream(stream: OutputStream, emitter: &dyn EventEmitter) -> OutputOutcome {
    stream
        .run(|event| {
            if let Ok(envelope) = encode(event) {
                emitter.emit(envelope);
            }
        })
        .await
}

/// Starts and drives a streaming call.
///
/// # Errors
///
/// Returns a [`CommandError`] when the call cannot even be routed (unknown model,
/// capability mismatch, missing credential). Errors that happen *after* the
/// stream opened are delivered as events, not as an `Err`.
pub async fn start_stream(
    engine: &OutputEngine,
    request: AiRequest,
    command: &StreamCommand,
    emitter: &dyn EventEmitter,
) -> Result<OutputOutcome, CommandError> {
    let stream = engine
        .stream(request, command.context())
        .await
        .map_err(|error| CommandError::from_core(&error))?;
    Ok(drive_stream(stream, emitter).await)
}

/// Runs a non-streaming call and maps failures to a command error.
///
/// # Errors
///
/// Any core error, including JSON contract violations.
pub async fn complete(
    engine: &OutputEngine,
    request: AiRequest,
    command: &StreamCommand,
) -> Result<OutputOutcome, CommandError> {
    engine
        .complete(request, command.context())
        .await
        .map_err(|error| CommandError::from_core(&error))
}

/// Runs a multi-round tool loop, forwarding every event to `emitter`.
///
/// The loop emits `ai-core://tool-call-executed` envelopes as each tool settles;
/// the returned [`RoundOutcome`] also carries the per-round outcomes and the stop
/// reason, so a command can report "rounds exhausted" to the frontend instead of
/// pretending the answer is final.
///
/// # Errors
///
/// Provider/routing failures are mapped to a [`CommandError`]. A failed *tool* is
/// not an error: it is replayed to the model and reported as an event.
pub async fn drive_rounds(
    runner: &RoundLoop,
    request: AiRequest,
    command: &StreamCommand,
    emitter: &dyn EventEmitter,
) -> Result<RoundOutcome, CommandError> {
    runner
        .run(request, command.context(), |event| {
            if let Ok(envelope) = encode(event) {
                emitter.emit(envelope);
            }
        })
        .await
        .map_err(|error| CommandError::from_core(&error))
}

/// Extracts the command error of an interrupted outcome, if any.
#[must_use]
pub fn outcome_error(outcome: &OutputOutcome) -> Option<CommandError> {
    outcome.error.as_ref().map(CommandError::from_output)
}
