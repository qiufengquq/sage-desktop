//! Output Engine — the framework's single output contract.
//!
//! # Responsibility
//!
//! Turn normalized provider answers into [`CoreOutputEvent`]s that any host can
//! render, and make "incomplete" distinguishable from "complete":
//!
//! | module | responsibility |
//! |--------|----------------|
//! | [`event`] | [`CoreOutputEvent`], [`OutputOutcome`], [`OutputError`] |
//! | [`stream`] | [`OutputStream`]: delta assembly, cancellation, partial preservation |
//! | [`structured`] | JSON parsing plus a documented JSON-Schema subset validator |
//! | [`engine`] | [`OutputEngine`]: non-streaming call and streaming entry point |
//! | [`rounds`] | [`RoundLoop`]: model round → tool execution → replayed round |
//!
//! # Guarantees
//!
//! * Exactly one [`CoreOutputEvent::Completed`] per call.
//! * A cancellation, timeout or provider error emits a redacted
//!   [`CoreOutputEvent::Error`] and then a completion with `interrupted = true`
//!   that still carries all text produced so far — nothing is silently dropped.
//! * [`OutputStream`] never holds a lock across an `.await`; it is a plain
//!   `Stream` the host can poll at its own pace (backpressure is inherited from
//!   the provider channel).
//! * No Tauri, UI or transport type appears here. The `ai-core-tauri` crate maps
//!   these values to command/event payloads; the host performs the final
//!   `emit` call.
//!
//! # Tool calls across rounds
//!
//! [`CoreOutputEvent::ToolCallStarted`] / [`CoreOutputEvent::ToolCallCompleted`]
//! describe the *model side* of a tool call (id, name, fully assembled JSON
//! arguments). Execution belongs to the tool subsystem ([`crate::tool`]): it
//! validates the arguments, runs the registered tool under the caller's deadline
//! and cancellation token, and reports each result as a
//! [`CoreOutputEvent::ToolCallExecuted`] carrying the output and the duration.
//! Replaying those results into the next provider round is [`RoundLoop`]'s job,
//! which keeps this engine emitting exactly one `Completed` per *model round* —
//! the loop, not the engine, decides how many rounds a logical answer takes.

pub mod cached;
pub mod engine;
pub mod event;
pub mod rounds;
pub mod stream;
pub mod structured;

pub use self::cached::CachedOutputEngine;
pub use self::engine::OutputEngine;
pub use self::event::{CoreOutputEvent, OutputError, OutputOutcome, OutputToolCall};
pub use self::rounds::{Round, RoundLoop, RoundOutcome, RoundStopReason};
pub use self::stream::OutputStream;
pub use self::structured::{parse_structured, validate_json, StructuredOutput};
