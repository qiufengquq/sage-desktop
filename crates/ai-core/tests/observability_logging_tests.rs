//! Log output checks: trace identifiers must be present, secrets must not be.

mod common;

use ai_core::config::LogLevel;
use ai_core::observability::{
    child_span, record_trace_context, request_span, to_tracing_level, TraceContext,
};
use ai_core::secret::{redact_for_log, SecretString};
use ai_core::SessionId;

#[test]
fn trace_identifiers_are_logged_while_secrets_are_redacted() {
    let context = TraceContext::generate().with_session(SessionId::new());
    let trace_id = context.trace_id().to_string();

    let logs = common::capture_logs(|| {
        let span = request_span("provider.call", &context);
        let _entered = span.enter();

        let api_key = SecretString::new("sk-live-super-secret-value");
        let authorization = redact_for_log("authorization", "Bearer sk-live-super-secret-value");

        tracing::info!(
            trace_id = %context.trace_id(),
            request_id = %context.request_id(),
            api_key = %api_key,
            api_key_debug = ?api_key,
            authorization = %authorization,
            "calling provider"
        );
    });

    let output = logs.contents();
    assert!(
        !output.is_empty(),
        "the subscriber must have captured output"
    );
    assert!(output.contains(&trace_id), "trace id must be logged");
    assert!(output.contains("provider.call"));
    assert!(output.contains("[REDACTED]"));
    assert!(
        !output.contains("sk-live-super-secret-value"),
        "a secret leaked into the log output: {output}"
    );
}

#[test]
fn span_metadata_declares_all_trace_fields() {
    // Span metadata only exists while a subscriber is installed; `capture_logs`
    // provides a thread-scoped one (never a global).
    common::capture_logs(|| {
        let context = TraceContext::generate().with_session(SessionId::new());
        let span = request_span("memory.search", &context);
        assert!(!span.is_disabled());

        let metadata = span.metadata().expect("span must have metadata");
        for field in ["operation", "trace_id", "request_id", "session_id"] {
            assert!(
                metadata.fields().field(field).is_some(),
                "field `{field}` must be declared so it can be recorded later"
            );
        }
    });
}

#[test]
fn child_spans_keep_the_trace_context() {
    common::capture_logs(|| {
        let context = TraceContext::generate();
        let parent = request_span("root", &context);
        let child = child_span("step", &context, &parent);

        assert!(!child.is_disabled());
        let metadata = child.metadata().expect("metadata");
        assert!(metadata.fields().field("trace_id").is_some());

        // Late recording must be allowed (fields were declared as empty).
        record_trace_context(&child, &context.child_request());
    });
}

#[test]
fn log_levels_map_onto_tracing_levels() {
    assert_eq!(to_tracing_level(LogLevel::Error), tracing::Level::ERROR);
    assert_eq!(to_tracing_level(LogLevel::Warn), tracing::Level::WARN);
    assert_eq!(to_tracing_level(LogLevel::Info), tracing::Level::INFO);
    assert_eq!(to_tracing_level(LogLevel::Debug), tracing::Level::DEBUG);
    assert_eq!(to_tracing_level(LogLevel::Trace), tracing::Level::TRACE);
}

#[test]
fn sensitive_keys_are_masked_before_logging() {
    let logs = common::capture_logs(|| {
        let pairs = [
            ("Authorization", "Bearer sk-live-abc123"),
            ("x-api-key", "sk-live-abc123"),
            ("model", "gpt-4o-mini"),
        ];
        for (key, value) in pairs {
            tracing::info!(key = key, value = %redact_for_log(key, value), "header");
        }
    });

    let output = logs.contents();
    assert!(output.contains("gpt-4o-mini"), "non-sensitive values stay");
    assert!(
        !output.contains("sk-live-abc123"),
        "credentials must be masked"
    );
}

#[test]
fn framework_spans_do_not_require_a_global_subscriber() {
    // Creating and recording spans with no subscriber installed must be a no-op,
    // never a panic and never a global side effect.
    let context = TraceContext::generate();
    let span = request_span("noop", &context);
    span.in_scope(|| {
        tracing::debug!("event without subscriber");
    });
}
