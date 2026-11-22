//! Host adapter: `ai-core` output events → Tauri command/event payloads.
//!
//! # Why this is a separate crate
//!
//! `ai-core` must stay reusable from any Rust process (CLI, server, another
//! desktop shell). Therefore it knows nothing about `tauri`, and this crate holds
//! the only host-specific glue:
//!
//! ```text
//! ai-core ──CoreOutputEvent──> ai-core-tauri ──EventEnvelope──> AppHandle::emit
//!    ^                                                              |
//!    └──────────────────── commands (start / cancel) ───────────────┘
//! ```
//!
//! # No `tauri` dependency, on purpose
//!
//! The crate does not link Tauri: it produces a serializable [`EventEnvelope`]
//! plus an [`EventEmitter`] trait. The host implements the trait with one line —
//! `app.emit(&envelope.event, envelope.payload)` — which keeps compilation fast,
//! the core crate transport-free, and the mapping unit-testable without a webview.
//!
//! # Wiring sketch
//!
//! ```no_run
//! # use std::sync::Arc;
//! use ai_core::output::OutputEngine;
//! use ai_core::provider::CallContext;
//! use ai_core_tauri::{drive_stream, EventEmitter, EventEnvelope, StreamCommand};
//!
//! struct AppHandle;
//! impl EventEmitter for AppHandle {
//!     fn emit(&self, envelope: EventEnvelope) {
//!         // In a Tauri command: `app.emit(&envelope.event, envelope.payload)`.
//!         let _ = envelope;
//!     }
//! }
//!
//! # async fn run(engine: &OutputEngine, request: ai_core::provider::AiRequest) -> ai_core::CoreResult<()> {
//! let command = StreamCommand::new(CallContext::generate());
//! let stream = engine.stream(request, command.context()).await?;
//! let outcome = drive_stream(stream, &AppHandle).await;
//! assert!(!outcome.interrupted || outcome.error.is_some());
//! # Ok(())
//! # }
//! ```
//!
//! # Tool events
//!
//! Tool calls arrive as `ai-core://tool-call-started` / `-delta` / `-completed`
//! envelopes (model side) followed by `ai-core://tool-call-executed` (framework
//! side) once the tool subsystem has run the call; the mapping stays a pure
//! name/payload translation, so the adapter never needs to know about tools.
//! [`drive_rounds`] wires a multi-round tool loop into the same emitter.
//!
//! # Context, memory and cache
//!
//! Those stay in `ai-core`: memory/context/cache values are plain Rust types the
//! command layer builds per request and never serialize to the frontend. Only
//! the resulting events cross this crate's boundary.

pub mod bridge;
pub mod error;
pub mod event;

pub use self::bridge::{
    complete, drive_rounds, drive_stream, outcome_error, start_stream, EventEmitter, StreamCommand,
};
pub use self::error::CommandError;
pub use self::event::{encode, event_name, EventEnvelope, EVENT_PREFIX};
