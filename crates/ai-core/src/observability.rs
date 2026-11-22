//! Structured logging, trace context propagation and log-safe redaction.
//!
//! The crate is a **library**: it never installs a global `tracing` subscriber on
//! its own, because that would fight the host application and make tests
//! order-dependent. Instead it provides
//!
//! * [`TraceContext`] — trace/request/session identifiers carried through a call
//!   chain and recorded on spans.
//! * [`request_span`] / [`child_span`] — helpers that build spans with the trace
//!   fields declared, plus [`record_trace_context`] to fill them in.
//! * [`SecretString`] (re-exported): its `Display` and `Debug` both print
//!   `[REDACTED]`, so `event!(Level::INFO, api_key = %secret)` (or `?secret`) is
//!   safe by construction.
//! * `redact_for_log` / `is_sensitive_key` — key-driven masking of arbitrary
//!   key/value pairs (HTTP headers, JSON bodies).
//!
//! ## Why secrets cannot be logged by accident
//!
//! `tracing`'s `field::Value` trait is sealed, which means a wrapper type cannot
//! implement it; `event!(Level::INFO, api_key = secret)` therefore does not
//! compile for [`SecretString`]. The developer must explicitly choose `%secret`
//! (Display) or `?secret` (Debug), and both are redacted. The only way to emit the
//! real value is a visible `expose_secret()` call, which is easy to spot in review.
//! * An optional, feature-gated `subscriber` module (cargo feature `subscriber`)
//!   for hosts that do want the framework to configure `tracing_subscriber` once
//!   at startup.
//!
//! # Propagation
//!
//! Create one [`TraceContext`] per user action, attach it to the root span, then
//! pass `span` down (`.instrument(span)` / `span.in_scope(...)`) or derive a
//! child context with [`TraceContext::child_request`] for every outbound provider
//! call. Log events emitted inside those spans automatically carry the IDs.

use crate::config::LogLevel;
use crate::ids::{RequestId, SessionId, TraceId};

pub use crate::secret::{
    is_sensitive_key, mask_prefix, redact, redact_for_log, sanitize_log_text, SecretString,
    REDACTED,
};
pub use tracing::Instrument;

/// Span field name carrying the [`TraceId`].
pub const TRACE_ID_FIELD: &str = "trace_id";
/// Span field name carrying the [`RequestId`].
pub const REQUEST_ID_FIELD: &str = "request_id";
/// Span field name carrying the [`SessionId`].
pub const SESSION_ID_FIELD: &str = "session_id";
/// Span field name carrying the logical operation label.
pub const OPERATION_FIELD: &str = "operation";

/// Name of the root span created for one logical AI operation.
const REQUEST_SPAN_NAME: &str = "ai_core.request";
/// Name of a nested span created for one step of a request.
const STEP_SPAN_NAME: &str = "ai_core.step";

/// Identifiers that travel with one logical AI operation.
///
/// `Copy` and immutable: handing it to a child task cannot corrupt the parent's
/// view, and it is cheap enough to embed in every log event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TraceContext {
    trace_id: TraceId,
    request_id: RequestId,
    session_id: Option<SessionId>,
}

impl TraceContext {
    /// Builds a context from explicit identifiers.
    #[must_use]
    pub const fn new(trace_id: TraceId, request_id: RequestId) -> Self {
        Self {
            trace_id,
            request_id,
            session_id: None,
        }
    }

    /// Generates a fresh trace with its first request id.
    #[must_use]
    pub fn generate() -> Self {
        Self::new(TraceId::new(), RequestId::new())
    }

    /// Attaches the client session.
    #[must_use]
    pub const fn with_session(mut self, session_id: SessionId) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// The trace this context belongs to.
    #[must_use]
    pub const fn trace_id(&self) -> TraceId {
        self.trace_id
    }

    /// The current request identifier.
    #[must_use]
    pub const fn request_id(&self) -> RequestId {
        self.request_id
    }

    /// The optional session identifier.
    #[must_use]
    pub const fn session_id(&self) -> Option<SessionId> {
        self.session_id
    }

    /// Derives a context for an outbound call: same trace/session, new request id.
    #[must_use]
    pub fn child_request(&self) -> Self {
        Self {
            trace_id: self.trace_id,
            request_id: RequestId::new(),
            session_id: self.session_id,
        }
    }
}

impl Default for TraceContext {
    fn default() -> Self {
        Self::generate()
    }
}

/// Records the trace identifiers on `span`.
///
/// The fields must have been declared on the span (the helpers in this module do
/// that); recording an undeclared field is a no-op in `tracing`.
pub fn record_trace_context(span: &tracing::Span, context: &TraceContext) {
    span.record(TRACE_ID_FIELD, tracing::field::display(context.trace_id));
    span.record(
        REQUEST_ID_FIELD,
        tracing::field::display(context.request_id),
    );
    if let Some(session_id) = context.session_id {
        span.record(SESSION_ID_FIELD, tracing::field::display(session_id));
    }
}

/// Creates the root span of one logical AI operation.
#[must_use]
pub fn request_span(operation: &'static str, context: &TraceContext) -> tracing::Span {
    let span = tracing::info_span!(
        REQUEST_SPAN_NAME,
        operation = operation,
        trace_id = tracing::field::Empty,
        request_id = tracing::field::Empty,
        session_id = tracing::field::Empty,
    );
    record_trace_context(&span, context);
    span
}

/// Creates a nested span parented by `parent`, sharing the same trace context.
#[must_use]
pub fn child_span(
    operation: &'static str,
    context: &TraceContext,
    parent: &tracing::Span,
) -> tracing::Span {
    let span = tracing::info_span!(
        parent: parent.id(),
        STEP_SPAN_NAME,
        operation = operation,
        trace_id = tracing::field::Empty,
        request_id = tracing::field::Empty,
        session_id = tracing::field::Empty,
    );
    record_trace_context(&span, context);
    span
}

/// Maps the configured [`LogLevel`] onto `tracing`'s level type.
#[must_use]
pub const fn to_tracing_level(level: LogLevel) -> tracing::Level {
    match level {
        LogLevel::Error => tracing::Level::ERROR,
        LogLevel::Warn => tracing::Level::WARN,
        LogLevel::Info => tracing::Level::INFO,
        LogLevel::Debug => tracing::Level::DEBUG,
        LogLevel::Trace => tracing::Level::TRACE,
    }
}

/// Feature-gated helper that installs a process-wide subscriber.
///
/// Enabled with the `subscriber` cargo feature. Library consumers that already
/// own their logging setup simply ignore this module.
#[cfg(feature = "subscriber")]
pub mod subscriber {
    use tracing_subscriber::EnvFilter;

    use crate::config::LogLevel;
    use crate::{CoreError, CoreResult};

    /// Installs a `tracing_subscriber` formatting subscriber if none is set.
    ///
    /// The filter is taken from `RUST_LOG` when present and falls back to `level`.
    /// Calling this more than once is a no-op, so it is safe in tests and in hosts
    /// that initialize logging from several entry points.
    ///
    /// # Errors
    ///
    /// [`CoreError::Internal`] if a subscriber was already installed by other
    /// means and the installation therefore failed.
    pub fn try_init_global(level: LogLevel) -> CoreResult<()> {
        if tracing::dispatcher::has_been_set() {
            return Ok(());
        }
        let filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level.as_str()));
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .try_init()
            .map_err(|error| {
                CoreError::internal(format!("failed to install tracing subscriber: {error}"))
            })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn generated_contexts_are_unique() {
        let first = TraceContext::generate();
        let second = TraceContext::generate();
        assert_ne!(first.trace_id(), second.trace_id());
        assert_ne!(first.request_id(), second.request_id());
        assert_eq!(first.session_id(), None);
    }

    #[test]
    fn child_request_keeps_trace_and_session() {
        let session = SessionId::new();
        let parent = TraceContext::generate().with_session(session);
        let child = parent.child_request();
        assert_eq!(child.trace_id(), parent.trace_id());
        assert_eq!(child.session_id(), Some(session));
        assert_ne!(child.request_id(), parent.request_id());
    }

    #[test]
    fn levels_map_to_tracing_levels() {
        assert_eq!(to_tracing_level(LogLevel::Error), tracing::Level::ERROR);
        assert_eq!(to_tracing_level(LogLevel::Trace), tracing::Level::TRACE);
    }

    #[test]
    fn spans_can_be_created_without_a_subscriber() {
        // Without a subscriber `tracing` returns a disabled, metadata-less span (a
        // cheap no-op). Building it and recording fields must still succeed and
        // must never install a global subscriber.
        let context = TraceContext::generate();
        let root = request_span("test.operation", &context);
        let child = child_span("test.step", &context, &root);
        record_trace_context(&root, &context.child_request());
        assert!(child.is_disabled(), "no subscriber means a no-op span");
        assert!(!tracing::dispatcher::has_been_set());
    }
}
