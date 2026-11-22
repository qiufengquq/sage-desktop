//! End-to-end tests of the provider runtime.
//!
//! Everything runs against [`MockAdapter`]: no network, no credentials, but the
//! full routing path (registry, capability gate, retries, fallbacks, budget,
//! cancellation, hot reload, streaming) is exercised.

use std::sync::Arc;
use std::time::Duration;

use ai_core::async_utils::CancelToken;
use ai_core::observability::TraceContext;
use ai_core::provider::{
    AdapterFactory, AiRequest, CallContext, FinishReason, MockAdapter, MockFailure,
    ModelCapabilities, ModelConfig, ProviderConfig, ProviderId, ProviderProtocol, ProviderRuntime,
    RetryPolicy,
};
use ai_core::{AiCoreConfig, ConfigStore, CoreError, ErrorKind};

fn provider(id: &str) -> ProviderConfig {
    ProviderConfig::new(
        ProviderId::new(id).unwrap(),
        ProviderProtocol::OpenAiCompatible,
    )
    .with_api_key("test-key")
}

fn model(id: &str) -> ModelConfig {
    ModelConfig::new(id)
}

/// A request pinned to `<provider>/<model>`.
///
/// Without pinning, `resolve(None)` picks the default model of the first enabled
/// provider in *alphabetical* order, which would hide the fallback path.
fn pinned_request(spec: &str) -> AiRequest {
    AiRequest::user_prompt("hi").with_model(spec.parse().unwrap())
}

fn config_with(providers: Vec<ProviderConfig>) -> AiCoreConfig {
    let mut config = AiCoreConfig::default();
    for provider in providers {
        config.providers.insert(provider.id.clone(), provider);
    }
    config
}

/// Builds a runtime whose factory pins every mock to its provider id.
fn runtime_with(
    config: AiCoreConfig,
    mocks: &[MockAdapter],
) -> (ProviderRuntime, Arc<ConfigStore>) {
    let store = Arc::new(ConfigStore::new(config).unwrap());
    let factory = AdapterFactory::new().unwrap();
    for mock in mocks {
        factory.bind(mock.provider_id().clone(), Arc::new(mock.clone()));
    }
    let runtime = ProviderRuntime::with_factory(Arc::clone(&store), factory).unwrap();
    (runtime, store)
}

#[tokio::test]
async fn complete_routes_to_the_bound_adapter_and_annotates_the_response() {
    let provider = provider("mock").with_model(model("mock-1"));
    let mock = MockAdapter::new(provider.id.clone()).respond_text("hello");
    let (runtime, _store) = runtime_with(config_with(vec![provider]), std::slice::from_ref(&mock));

    let context = CallContext::generate();
    let response = runtime
        .complete(AiRequest::user_prompt("hi"), &context)
        .await
        .unwrap();

    assert_eq!(response.content(), "hello");
    assert_eq!(response.provider_id().as_str(), "mock");
    assert_eq!(response.model_id(), "mock-1");
    assert_eq!(response.attempts(), 1);
    assert_eq!(response.trace_id(), Some(context.trace().trace_id()));

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].message_count, 1);
    assert!(!calls[0].streaming);
    assert_eq!(calls[0].trace_id, context.trace().trace_id());
}

#[tokio::test]
async fn retryable_failures_are_retried_within_the_budget() {
    let provider = provider("mock")
        .with_model(model("mock-1"))
        .with_retry(RetryPolicy {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(2),
            retry_on_timeout: false,
        });
    let mock = MockAdapter::new(provider.id.clone())
        .fail(MockFailure::RateLimited)
        .respond_text("second attempt");
    let (runtime, _store) = runtime_with(config_with(vec![provider]), std::slice::from_ref(&mock));

    let response = runtime
        .complete(AiRequest::user_prompt("hi"), &CallContext::generate())
        .await
        .unwrap();

    assert_eq!(response.content(), "second attempt");
    assert_eq!(response.attempts(), 2);
    assert_eq!(mock.call_count(), 2);
}

#[tokio::test]
async fn permanent_failures_are_not_retried_nor_routed_to_a_fallback() {
    let primary = provider("primary").with_model(model("p1").with_fallbacks(["backup/b1"]));
    let backup = provider("backup").with_model(model("b1"));
    let primary_mock = MockAdapter::new(primary.id.clone()).fail(MockFailure::NonRetryable);
    let backup_mock = MockAdapter::new(backup.id.clone()).respond_text("backup answer");
    let (runtime, _store) = runtime_with(
        config_with(vec![primary, backup]),
        &[primary_mock.clone(), backup_mock.clone()],
    );

    let error = runtime
        .complete(pinned_request("primary/p1"), &CallContext::generate())
        .await
        .unwrap_err();

    assert_eq!(error.kind(), ErrorKind::Provider);
    assert!(!error.is_retryable());
    assert_eq!(primary_mock.call_count(), 1);
    assert_eq!(
        backup_mock.call_count(),
        0,
        "a 400 must not trigger a fallback"
    );
}

#[tokio::test]
async fn authentication_failures_fall_back_to_the_declared_model() {
    let primary = provider("primary")
        .with_model(model("p1").with_fallbacks(["backup/b1"]))
        .with_retry(RetryPolicy::none());
    let backup = provider("backup").with_model(model("b1"));
    let primary_mock = MockAdapter::new(primary.id.clone()).fail(MockFailure::AuthRejected);
    let backup_mock = MockAdapter::new(backup.id.clone()).respond_text("from the fallback");
    let (runtime, _store) = runtime_with(
        config_with(vec![primary, backup]),
        &[primary_mock.clone(), backup_mock.clone()],
    );

    let response = runtime
        .complete(pinned_request("primary/p1"), &CallContext::generate())
        .await
        .unwrap();

    assert_eq!(response.content(), "from the fallback");
    assert_eq!(response.provider_id().as_str(), "backup");
    assert_eq!(response.model_id(), "b1");
    assert_eq!(response.attempts(), 2);
    assert_eq!(primary_mock.call_count(), 1);
    assert_eq!(backup_mock.call_count(), 1);
}

#[tokio::test]
async fn cancellation_short_circuits_before_any_upstream_call() {
    let provider = provider("mock").with_model(model("mock-1"));
    let mock = MockAdapter::new(provider.id.clone()).respond_text("never sent");
    let (runtime, _store) = runtime_with(config_with(vec![provider]), std::slice::from_ref(&mock));

    let token = CancelToken::new();
    token.cancel();
    let context = CallContext::new(TraceContext::generate(), token);

    let error = runtime
        .complete(AiRequest::user_prompt("hi"), &context)
        .await
        .unwrap_err();

    assert!(error.is_cancelled());
    assert_eq!(mock.call_count(), 0);
}

#[tokio::test]
async fn unsupported_streaming_is_rejected_before_routing() {
    let provider =
        provider("mock").with_model(model("mock-1").with_capabilities(ModelCapabilities {
            streaming: false,
            ..ModelCapabilities::default()
        }));
    let mock = MockAdapter::new(provider.id.clone()).stream_chunks(["never"]);
    let (runtime, _store) = runtime_with(config_with(vec![provider]), std::slice::from_ref(&mock));

    let error = runtime
        .stream(AiRequest::user_prompt("hi"), &CallContext::generate())
        .await
        .unwrap_err();

    assert_eq!(error.kind(), ErrorKind::CapabilityUnsupported);
    assert_eq!(mock.call_count(), 0);
}

#[tokio::test]
async fn provider_timeout_override_bounds_a_single_attempt() {
    let mut provider = provider("mock").with_model(model("mock-1"));
    provider.timeout_override = Some(Duration::from_millis(20));
    let mock = MockAdapter::new(provider.id.clone())
        .with_delay(Duration::from_secs(30))
        .respond_text("too late");
    let (runtime, _store) = runtime_with(config_with(vec![provider]), std::slice::from_ref(&mock));

    let error = runtime
        .complete(AiRequest::user_prompt("hi"), &CallContext::generate())
        .await
        .unwrap_err();

    assert!(error.is_timeout(), "unexpected error: {error}");
}

#[tokio::test]
async fn streaming_retries_before_the_first_event_then_accumulates() {
    let provider = provider("mock")
        .with_model(model("mock-1"))
        .with_retry(RetryPolicy {
            max_attempts: 2,
            initial_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(2),
            retry_on_timeout: false,
        });
    let mock = MockAdapter::new(provider.id.clone())
        .fail(MockFailure::ServerError)
        .stream_chunks(["he", "llo"]);
    let (runtime, _store) = runtime_with(config_with(vec![provider]), std::slice::from_ref(&mock));

    let context = CallContext::generate();
    let stream = runtime
        .stream(AiRequest::user_prompt("hi"), &context)
        .await
        .unwrap();
    let outcome = stream.finish().await.unwrap();

    assert_eq!(outcome.text, "hello");
    assert_eq!(outcome.finish_reason, Some(FinishReason::Stop));
    assert_eq!(mock.call_count(), 2);
}

#[tokio::test]
async fn a_pinned_snapshot_keeps_routing_to_the_old_configuration() {
    let first = provider("mock").with_model(model("first-model"));
    let mock = MockAdapter::new(first.id.clone());
    let (runtime, store) = runtime_with(config_with(vec![first]), std::slice::from_ref(&mock));
    let pinned = runtime.snapshot();

    let second = provider("mock").with_model(model("second-model"));
    store.apply(config_with(vec![second])).unwrap();

    let context = CallContext::generate();
    let fresh = runtime
        .complete(AiRequest::user_prompt("hi"), &context)
        .await
        .unwrap();
    let stale = runtime
        .complete_with_snapshot(&pinned, AiRequest::user_prompt("hi"), &context)
        .await
        .unwrap();

    assert_eq!(fresh.model_id(), "second-model");
    assert_eq!(stale.model_id(), "first-model");
    assert!(runtime.refresh().unwrap().version() >= store.version());
    assert_eq!(
        mock.calls()
            .into_iter()
            .map(|call| call.model)
            .collect::<Vec<_>>(),
        vec!["second-model".to_owned(), "first-model".to_owned()]
    );
}

#[tokio::test]
async fn empty_configuration_reports_a_configuration_error() {
    let store = Arc::new(ConfigStore::new(AiCoreConfig::default()).unwrap());
    let runtime = ProviderRuntime::new(store).unwrap();

    let error = runtime
        .complete(AiRequest::user_prompt("hi"), &CallContext::generate())
        .await
        .unwrap_err();

    assert_eq!(error.kind(), ErrorKind::Config);
    assert!(matches!(error, CoreError::Config { .. }));
}
