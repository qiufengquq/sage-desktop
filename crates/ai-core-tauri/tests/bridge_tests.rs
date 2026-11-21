//! Adapter tests: event mapping, command errors and stream forwarding.
//!
//! No Tauri runtime, no webview: the adapter is a pure mapping layer, so it can
//! be verified with a recording emitter.

use std::sync::{Arc, Mutex};

use ai_core::output::OutputEngine;
use ai_core::provider::{
    AdapterFactory, AiRequest, CallContext, MockAdapter, MockFailure, ModelConfig, ProviderConfig,
    ProviderId, ProviderProtocol, ProviderRuntime,
};
use ai_core::{AiCoreConfig, ConfigStore, CoreError};
use ai_core_tauri::{
    complete, drive_stream, encode, event_name, outcome_error, start_stream, CommandError,
    EventEmitter, EventEnvelope, StreamCommand, EVENT_PREFIX,
};

#[derive(Default)]
struct RecordingEmitter {
    envelopes: Mutex<Vec<EventEnvelope>>,
}

impl RecordingEmitter {
    fn names(&self) -> Vec<String> {
        self.envelopes
            .lock()
            .expect("emitter mutex is not poisoned")
            .iter()
            .map(|envelope| envelope.event.clone())
            .collect()
    }

    fn payloads(&self) -> Vec<serde_json::Value> {
        self.envelopes
            .lock()
            .expect("emitter mutex is not poisoned")
            .iter()
            .map(|envelope| envelope.payload.clone())
            .collect()
    }
}

impl EventEmitter for RecordingEmitter {
    fn emit(&self, envelope: EventEnvelope) {
        self.envelopes
            .lock()
            .expect("emitter mutex is not poisoned")
            .push(envelope);
    }
}

fn provider() -> ProviderConfig {
    ProviderConfig::new(
        ProviderId::new("mock").unwrap(),
        ProviderProtocol::OpenAiCompatible,
    )
    .with_api_key("test-key")
    .with_model(ModelConfig::new("mock-1"))
}

fn engine(mock: &MockAdapter) -> OutputEngine {
    let mut config = AiCoreConfig::default();
    config.providers.insert(provider().id.clone(), provider());
    let store = Arc::new(ConfigStore::new(config).unwrap());
    let factory = AdapterFactory::new().unwrap();
    factory.bind(mock.provider_id().clone(), Arc::new(mock.clone()));
    let runtime = ProviderRuntime::with_factory(store, factory).unwrap();
    OutputEngine::new(Arc::new(runtime))
}

fn mock() -> MockAdapter {
    MockAdapter::new(ProviderId::new("mock").unwrap())
}

#[test]
fn events_map_to_namespaced_envelopes() {
    let event = ai_core::output::CoreOutputEvent::TextDelta {
        text: "hello".to_owned(),
    };
    let envelope = encode(&event).unwrap();
    assert_eq!(envelope.event, format!("{EVENT_PREFIX}text-delta"));
    assert_eq!(envelope.payload["type"], "text_delta");
    assert_eq!(envelope.payload["text"], "hello");
    assert_eq!(event_name(&event), "text-delta");
}

#[test]
fn tool_execution_maps_to_its_own_event() {
    let event = ai_core::output::CoreOutputEvent::ToolCallExecuted {
        index: 0,
        call_id: "call_1".to_owned(),
        name: "echo".to_owned(),
        success: true,
        output: "{\"word\":\"apple\"}".to_owned(),
        duration_ms: 3,
    };
    let envelope = encode(&event).unwrap();
    assert_eq!(event_name(&event), "tool-call-executed");
    assert_eq!(envelope.event, format!("{EVENT_PREFIX}tool-call-executed"));
    assert_eq!(envelope.payload["type"], "tool_call_executed");
    assert_eq!(envelope.payload["call_id"], "call_1");
    assert_eq!(envelope.payload["success"], true);
    assert_eq!(envelope.payload["duration_ms"], 3);
}

#[test]
fn command_errors_keep_the_code_and_redact_credentials() {
    let error = CoreError::authentication("rejected key sk-live-supersecret");
    let command = CommandError::from_core(&error);
    assert_eq!(command.code, "CORE_AUTHENTICATION");
    assert!(!command.retryable);
    assert!(!command.message.contains("sk-live-supersecret"));
    assert!(command.to_string().starts_with("CORE_AUTHENTICATION: "));
}

#[tokio::test]
async fn streaming_forwards_ordered_envelopes() {
    let mock = mock().stream_chunks(["he", "llo"]);
    let engine = engine(&mock);
    let emitter = RecordingEmitter::default();
    let command = StreamCommand::new(CallContext::generate());

    let outcome = start_stream(&engine, AiRequest::user_prompt("hi"), &command, &emitter)
        .await
        .unwrap();

    let names = emitter.names();
    assert_eq!(names.first().map(String::as_str), Some("ai-core://started"));
    assert_eq!(
        names
            .iter()
            .filter(|name| *name == "ai-core://text-delta")
            .count(),
        2
    );
    assert_eq!(
        names
            .iter()
            .filter(|name| *name == "ai-core://completed")
            .count(),
        1
    );
    assert!(names.iter().any(|name| name == "ai-core://usage-updated"));

    let payloads = emitter.payloads();
    assert_eq!(payloads[0]["streaming"], true);
    assert_eq!(payloads[1]["text"], "he");
    assert_eq!(
        payloads.last().map(|payload| payload["type"].as_str()),
        Some(Some("completed"))
    );

    assert!(outcome.is_complete());
    assert_eq!(outcome.text, "hello");
    assert!(outcome_error(&outcome).is_none());
}

#[tokio::test]
async fn a_tool_loop_forwards_execution_envelopes() {
    use std::time::Duration;

    use ai_core::output::RoundLoop;
    use ai_core::provider::response::ToolCall;
    use ai_core::tool::{Tool, ToolExecutor, ToolFuture, ToolRegistry};
    use ai_core_tauri::drive_rounds;
    use serde_json::{json, Value};

    struct EchoTool {
        spec: ai_core::provider::ToolSpec,
    }

    impl Tool for EchoTool {
        fn name(&self) -> &str {
            self.spec.name()
        }

        fn description(&self) -> &str {
            self.spec.description()
        }

        fn parameters(&self) -> Value {
            self.spec.parameters().clone()
        }

        fn invoke<'a>(&'a self, arguments: &'a Value) -> ToolFuture<'a> {
            Box::pin(async move { Ok(arguments.clone()) })
        }
    }

    let mock = mock().respond_tool_calls(
        "",
        vec![ToolCall {
            id: "call_1".to_owned(),
            name: "echo".to_owned(),
            arguments: "{\"q\":\"apple\"}".to_owned(),
        }],
    );
    let mut registry = ToolRegistry::new();
    registry
        .register(Arc::new(EchoTool {
            spec: ai_core::provider::ToolSpec::new(
                "echo",
                "echoes its arguments",
                json!({"type": "object"}),
            )
            .unwrap(),
        }))
        .unwrap();
    let runner = RoundLoop::new(
        Arc::new(engine(&mock)),
        ToolExecutor::new(Arc::new(registry), Duration::from_secs(1)),
    );
    let emitter = RecordingEmitter::default();
    let command = StreamCommand::new(CallContext::generate());
    let request = AiRequest::user_prompt("hi");

    let outcome = drive_rounds(&runner, request, &command, &emitter)
        .await
        .unwrap();

    assert!(outcome.is_complete());
    assert!(!outcome.is_exhausted());
    let names = emitter.names();
    assert_eq!(
        names
            .iter()
            .filter(|name| *name == "ai-core://tool-call-executed")
            .count(),
        1
    );
    let payloads = emitter.payloads();
    assert_eq!(payloads[0]["success"], true);
    assert_eq!(payloads[0]["name"], "echo");
    // The scripted mock answers with its canned text on the second round.
    assert_eq!(outcome.round_count(), 2);
}

#[tokio::test]
async fn cancel_command_surfaces_the_cancellation_code() {
    let mock = mock().stream_chunks(["never"]);
    let engine = engine(&mock);
    let emitter = RecordingEmitter::default();
    let command = StreamCommand::new(CallContext::generate());
    command.cancel();
    assert!(command.is_cancelled());

    let error = start_stream(&engine, AiRequest::user_prompt("hi"), &command, &emitter)
        .await
        .unwrap_err();
    assert_eq!(error.code, "CORE_CANCELLED");
    assert!(emitter.names().is_empty());
}

#[tokio::test]
async fn interrupted_stream_still_returns_partial_text_and_an_error() {
    let mock = mock().stream_then_fail(["par"], MockFailure::ServerError);
    let engine = engine(&mock);
    let emitter = RecordingEmitter::default();
    let command = StreamCommand::new(CallContext::generate());

    let outcome = start_stream(&engine, AiRequest::user_prompt("hi"), &command, &emitter)
        .await
        .unwrap();

    assert!(outcome.interrupted);
    assert_eq!(outcome.text, "par");
    let error = outcome_error(&outcome).unwrap();
    assert_eq!(error.code, "CORE_PROVIDER");
    assert!(emitter.names().iter().any(|name| name == "ai-core://error"));
    assert!(emitter
        .names()
        .iter()
        .any(|name| name == "ai-core://completed"));
}

#[tokio::test]
async fn non_streaming_completion_maps_success_and_failure() {
    let happy_mock = mock().respond_text("hello");
    let happy_engine = engine(&happy_mock);
    let command = StreamCommand::new(CallContext::generate());

    let outcome = complete(&happy_engine, AiRequest::user_prompt("hi"), &command)
        .await
        .unwrap();
    assert_eq!(outcome.text, "hello");
    assert!(outcome.is_complete());

    // A non-retryable failure avoids consuming the default retry budget, so the
    // command surface sees exactly one provider error.
    let failing_mock = mock().fail(MockFailure::NonRetryable);
    let failing_engine = engine(&failing_mock);
    let error = complete(&failing_engine, AiRequest::user_prompt("hi"), &command)
        .await
        .unwrap_err();
    assert_eq!(error.code, "CORE_PROVIDER");
    assert!(!error.message.contains("test-key"));
}

#[tokio::test]
async fn drive_stream_is_usable_without_the_command_wrapper() {
    let mock = mock().stream_chunks(["x"]);
    let engine = engine(&mock);
    let emitter = RecordingEmitter::default();
    let stream = engine
        .stream(AiRequest::user_prompt("hi"), &CallContext::generate())
        .await
        .unwrap();

    let outcome = drive_stream(stream, &emitter).await;
    assert_eq!(outcome.text, "x");
    assert_eq!(
        emitter.names().last().map(String::as_str),
        Some("ai-core://completed")
    );
}
