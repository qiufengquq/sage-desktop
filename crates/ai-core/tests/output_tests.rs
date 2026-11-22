//! Phase 4 end-to-end tests: provider runtime → output engine → core events.

use std::sync::Arc;

use ai_core::async_utils::CancelToken;
use ai_core::output::{CoreOutputEvent, OutputEngine};
use ai_core::provider::{
    AdapterFactory, AiRequest, CallContext, FinishReason, MockAdapter, MockFailure,
    ModelCapabilities, ModelConfig, ProviderConfig, ProviderId, ProviderProtocol, ProviderRuntime,
    ResponseFormat,
};
use ai_core::{AiCoreConfig, ConfigStore, ErrorKind};

fn provider() -> ProviderConfig {
    ProviderConfig::new(
        ProviderId::new("mock").unwrap(),
        ProviderProtocol::OpenAiCompatible,
    )
    .with_api_key("test-key")
    .with_model(
        ModelConfig::new("mock-1").with_capabilities(ModelCapabilities {
            structured_output: true,
            ..ModelCapabilities::default()
        }),
    )
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

fn words_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["word", "level"],
        "properties": {
            "word": { "type": "string" },
            "level": { "type": "integer" }
        },
        "additionalProperties": false
    })
}

fn json_request() -> AiRequest {
    AiRequest::user_prompt("give me a word").with_response_format(ResponseFormat::JsonSchema {
        name: "word_card".to_owned(),
        schema: words_schema(),
    })
}

#[tokio::test]
async fn stream_emits_ordered_events_and_accumulates_text() {
    let mock =
        MockAdapter::new(ProviderId::new("mock").unwrap()).stream_chunks(["he", "llo", " world"]);
    let engine = engine(&mock);

    let stream = engine
        .stream(AiRequest::user_prompt("hi"), &CallContext::generate())
        .await
        .unwrap();
    let mut events = Vec::new();
    let outcome = stream.run(|event| events.push(event.clone())).await;

    assert!(matches!(events[0], CoreOutputEvent::Started { .. }));
    let deltas: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            CoreOutputEvent::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, vec!["he", "llo", " world"]);
    assert!(events
        .iter()
        .any(|event| matches!(event, CoreOutputEvent::UsageUpdated { .. })));
    assert!(matches!(
        events.last(),
        Some(CoreOutputEvent::Completed {
            interrupted: false,
            ..
        })
    ));
    assert_eq!(outcome.text, "hello world");
    assert_eq!(outcome.finish_reason, Some(FinishReason::Stop));
    assert!(outcome.is_complete());
    assert!(outcome.error.is_none());
}

#[tokio::test]
async fn cancellation_mid_stream_returns_partial_output() {
    let mock = MockAdapter::new(ProviderId::new("mock").unwrap()).stream_chunks(["a", "b", "c"]);
    let engine = engine(&mock);

    let token = CancelToken::new();
    let context = CallContext::new(
        ai_core::observability::TraceContext::generate(),
        token.clone(),
    );
    let stream = engine
        .stream(AiRequest::user_prompt("hi"), &context)
        .await
        .unwrap();

    let mut events = Vec::new();
    let mut seen_delta = false;
    let outcome = Box::pin(stream.run(|event| {
        events.push(event.clone());
        if matches!(event, CoreOutputEvent::TextDelta { .. }) && !seen_delta {
            seen_delta = true;
            token.cancel();
        }
    }))
    .await;

    assert!(outcome.interrupted);
    assert_eq!(outcome.text, "a");
    assert_eq!(
        outcome.error.as_ref().map(|error| error.code.as_str()),
        Some("CORE_CANCELLED")
    );
    assert!(matches!(
        events.last(),
        Some(CoreOutputEvent::Completed {
            interrupted: true,
            ..
        })
    ));
}

#[tokio::test]
async fn provider_error_mid_stream_preserves_partial_text() {
    let mock = MockAdapter::new(ProviderId::new("mock").unwrap())
        .stream_then_fail(["partial"], MockFailure::ServerError);
    let engine = engine(&mock);

    let outcome = engine
        .stream(AiRequest::user_prompt("hi"), &CallContext::generate())
        .await
        .unwrap()
        .drain()
        .await;

    assert!(outcome.interrupted);
    assert_eq!(outcome.text, "partial");
    assert_eq!(
        outcome.error.as_ref().map(|error| error.code.as_str()),
        Some("CORE_PROVIDER")
    );
}

#[tokio::test]
async fn valid_structured_output_is_parsed_and_marked_validated() {
    let mock = MockAdapter::new(ProviderId::new("mock").unwrap()).respond_detailed(
        "{\"word\":\"apple\",\"level\":2}",
        FinishReason::Stop,
        None,
    );
    let engine = engine(&mock);

    let outcome = engine
        .complete(json_request(), &CallContext::generate())
        .await
        .unwrap();

    let structured = outcome.structured.unwrap();
    assert!(structured.validated);
    assert_eq!(structured.schema_name.as_deref(), Some("word_card"));
    assert_eq!(structured.value["word"], "apple");
    assert_eq!(structured.raw, "{\"word\":\"apple\",\"level\":2}");
}

#[tokio::test]
async fn schema_violation_is_a_clear_error_for_complete() {
    let mock = MockAdapter::new(ProviderId::new("mock").unwrap())
        .respond_text("{\"word\":\"apple\",\"level\":\"two\"}");
    let engine = engine(&mock);

    let error = engine
        .complete(json_request(), &CallContext::generate())
        .await
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidArgument);
    assert!(error.to_string().contains("output.schema"));
}

#[tokio::test]
async fn invalid_json_is_reported_as_serialization() {
    let mock = MockAdapter::new(ProviderId::new("mock").unwrap()).respond_text("not json");
    let engine = engine(&mock);

    let error = engine
        .complete(json_request(), &CallContext::generate())
        .await
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Serialization);
}

#[tokio::test]
async fn streaming_schema_violation_emits_error_but_keeps_text() {
    let mock = MockAdapter::new(ProviderId::new("mock").unwrap())
        .stream_chunks(["{\"word\":", "\"apple\"}"]);
    let engine = engine(&mock);

    let mut events = Vec::new();
    let outcome = engine
        .stream(json_request(), &CallContext::generate())
        .await
        .unwrap()
        .run(|event| events.push(event.clone()))
        .await;

    assert!(outcome.structured.is_none());
    assert_eq!(outcome.text, "{\"word\":\"apple\"}");
    assert!(outcome
        .error
        .as_ref()
        .is_some_and(|error| error.code == "CORE_INVALID_ARGUMENT"));
    assert!(events
        .iter()
        .any(|event| matches!(event, CoreOutputEvent::Error { .. })));
}
