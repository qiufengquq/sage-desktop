//! End-to-end tests for Phase 3: provider attempts → usage recorder → SQLite.
//!
//! Every test drives the real router (with a mock adapter) so the recorded
//! numbers are produced by the same code path a host application uses.

use std::sync::Arc;
use std::time::Duration;

use ai_core::ids::SessionId;
use ai_core::provider::{
    AdapterFactory, AiRequest, AttemptObserver, CallContext, FinishReason, MockAdapter,
    MockFailure, ModelConfig, ProviderConfig, ProviderId, ProviderProtocol, ProviderRuntime,
    RequestStatus, RetryPolicy, UsageReport,
};
use ai_core::usage::{UsageGroupBy, UsageQuery, UsageRecorder, UsageStore, USAGE_DB_FILE};
use ai_core::{AiCoreConfig, ConfigStore};

struct Harness {
    _directory: tempfile::TempDir,
    runtime: ProviderRuntime,
    recorder: Arc<UsageRecorder>,
}

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

async fn harness(providers: Vec<ProviderConfig>, mocks: &[MockAdapter]) -> Harness {
    let directory = tempfile::tempdir().unwrap();
    let mut config = AiCoreConfig::default();
    config.storage.root = directory.path().join("ai-core-data");
    for provider in providers {
        config.providers.insert(provider.id.clone(), provider);
    }
    let layout = config.storage_layout().unwrap();
    let recorder = Arc::new(
        UsageRecorder::start(&layout, &config.usage)
            .await
            .unwrap()
            .unwrap(),
    );
    let store = Arc::new(ConfigStore::new(config).unwrap());
    let factory = AdapterFactory::new().unwrap();
    for mock in mocks {
        factory.bind(mock.provider_id().clone(), Arc::new(mock.clone()));
    }
    let runtime = ProviderRuntime::with_factory(store, factory)
        .unwrap()
        .with_observer(Arc::clone(&recorder) as Arc<dyn AttemptObserver>);
    Harness {
        _directory: directory,
        runtime,
        recorder,
    }
}

async fn total(harness: &Harness, query: UsageQuery) -> ai_core::usage::UsageAggregate {
    harness.recorder.flush().await.unwrap();
    harness.recorder.store().query(&query).unwrap()
}

#[tokio::test]
async fn complete_records_actual_tokens_traffic_and_correlation() {
    let provider = provider("mock").with_model(model("mock-1"));
    let mock = MockAdapter::new(provider.id.clone()).respond_detailed(
        "hello",
        FinishReason::Stop,
        Some(UsageReport {
            prompt_tokens: Some(12),
            completion_tokens: Some(3),
            total_tokens: Some(15),
            ..UsageReport::default()
        }),
    );
    let harness = harness(vec![provider], std::slice::from_ref(&mock)).await;

    let session = SessionId::new();
    let context = CallContext::generate()
        .with_session(session)
        .with_feature("vocabulary.lookup");
    let response = harness
        .runtime
        .complete(AiRequest::user_prompt("hi"), &context)
        .await
        .unwrap();
    assert_eq!(response.content(), "hello");

    let aggregate = total(&harness, UsageQuery::default()).await;
    let summary = &aggregate.total;
    assert_eq!(summary.attempts, 1);
    assert_eq!(summary.requests, 1);
    assert_eq!(summary.success, 1);
    assert_eq!(summary.input_tokens, 12);
    assert_eq!(summary.output_tokens, 3);
    assert_eq!(summary.total_tokens, 15);
    assert_eq!(summary.actual_tokens, 15);
    assert_eq!(summary.estimated_tokens, 0);

    // Traffic is counted in bytes and never mixed with tokens.
    assert_eq!(summary.request_bytes, "hi".len() as u64);
    assert_eq!(summary.response_bytes, "hello".len() as u64);
    assert_eq!(summary.total_bytes(), 7);
    assert_ne!(summary.total_bytes(), u64::from(15_u32));

    let by_feature = total(
        &harness,
        UsageQuery {
            feature: Some("vocabulary.lookup".to_owned()),
            group_by: UsageGroupBy::Provider,
            ..UsageQuery::default()
        },
    )
    .await;
    assert_eq!(by_feature.total.attempts, 1);
    assert_eq!(by_feature.buckets.len(), 1);
    assert_eq!(by_feature.buckets[0].key, "mock");

    let by_session = total(
        &harness,
        UsageQuery {
            session_id: Some(session),
            ..UsageQuery::default()
        },
    )
    .await;
    assert_eq!(by_session.total.attempts, 1);

    // Request-level view: one attempt, provider-measured tokens, no error.
    let request_id = response.request_id().unwrap();
    let attempts = harness.recorder.store().attempts_for(request_id).unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].error_code, None);
    assert_eq!(attempts[0].model_id.as_deref(), Some("mock-1"));
    assert_eq!(
        attempts[0].tokens.map(|tokens| tokens.source),
        Some(ai_core::usage::TokenSource::Actual)
    );
}

#[tokio::test]
async fn retry_then_success_records_each_attempt_of_one_request() {
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
        .respond_text("ok");
    let harness = harness(vec![provider], std::slice::from_ref(&mock)).await;

    let response = harness
        .runtime
        .complete(AiRequest::user_prompt("hi"), &CallContext::generate())
        .await
        .unwrap();
    assert_eq!(response.attempts(), 2);

    let aggregate = total(&harness, UsageQuery::default()).await;
    assert_eq!(aggregate.total.attempts, 2);
    assert_eq!(aggregate.total.requests, 1);
    assert_eq!(aggregate.total.failed, 1);
    assert_eq!(aggregate.total.degraded, 1);
    assert_eq!(aggregate.total.success, 0);

    // The failed attempt still consumed an estimated prompt, and it is marked as
    // an estimate rather than a provider measurement.
    assert!(aggregate.total.estimated_tokens > 0);
    assert_eq!(aggregate.total.actual_tokens, 0);
}

#[tokio::test]
async fn missing_provider_usage_falls_back_to_a_clearly_marked_estimate() {
    let provider = provider("mock").with_model(model("mock-1"));
    let mock = MockAdapter::new(provider.id.clone()).respond_text("hello world");
    let harness = harness(vec![provider], std::slice::from_ref(&mock)).await;

    harness
        .runtime
        .complete(AiRequest::user_prompt("hi"), &CallContext::generate())
        .await
        .unwrap();

    let summary = total(&harness, UsageQuery::default()).await.total;
    assert!(summary.total_tokens > 0);
    assert_eq!(summary.estimated_tokens, summary.total_tokens);
    assert_eq!(summary.actual_tokens, 0);
}

#[tokio::test]
async fn streaming_settles_usage_on_the_terminal_event() {
    let provider = provider("mock").with_model(model("mock-1"));
    let mock = MockAdapter::new(provider.id.clone()).stream_chunks(["he", "llo"]);
    let harness = harness(vec![provider], std::slice::from_ref(&mock)).await;

    let stream = harness
        .runtime
        .stream(AiRequest::user_prompt("hi"), &CallContext::generate())
        .await
        .unwrap();
    let outcome = stream.finish().await.unwrap();
    assert_eq!(outcome.text, "hello");

    let summary = total(&harness, UsageQuery::default()).await.total;
    assert_eq!(summary.attempts, 1);
    assert_eq!(summary.success, 1);
    assert_eq!(summary.actual_tokens, 6);
    assert_eq!(summary.input_tokens, 1);
    assert_eq!(summary.response_bytes, 5);
    assert_eq!(summary.request_bytes, 2);
}

#[tokio::test]
async fn dropping_a_stream_still_records_a_cancelled_attempt() {
    let provider = provider("mock").with_model(model("mock-1"));
    let mock = MockAdapter::new(provider.id.clone()).stream_chunks(["partial"]);
    let harness = harness(vec![provider], std::slice::from_ref(&mock)).await;

    let stream = harness
        .runtime
        .stream(AiRequest::user_prompt("hi"), &CallContext::generate())
        .await
        .unwrap();
    drop(stream);

    let summary = total(&harness, UsageQuery::default()).await.total;
    assert_eq!(summary.attempts, 1);
    assert_eq!(summary.cancelled, 1);
    assert_eq!(summary.unsuccessful(), 1);
}

#[tokio::test]
async fn concurrent_requests_are_all_recorded() {
    let provider = provider("mock").with_model(model("mock-1"));
    let mock = MockAdapter::new(provider.id.clone()).respond_text("ok");
    let harness = harness(vec![provider], std::slice::from_ref(&mock)).await;
    let recorder = Arc::clone(&harness.recorder);
    let runtime = Arc::new(harness.runtime);

    let mut tasks = Vec::new();
    for _ in 0..8 {
        let runtime = Arc::clone(&runtime);
        tasks.push(tokio::spawn(async move {
            runtime
                .complete(AiRequest::user_prompt("hi"), &CallContext::generate())
                .await
        }));
    }
    for task in tasks {
        task.await.unwrap().unwrap();
    }

    recorder.flush().await.unwrap();
    let summary = recorder
        .store()
        .query(&UsageQuery::default())
        .unwrap()
        .total;
    assert_eq!(summary.attempts, 8);
    assert_eq!(summary.requests, 8);
}

#[tokio::test]
async fn grouping_by_provider_and_model_separates_traffic() {
    let alpha = provider("alpha").with_model(model("a1"));
    let beta = provider("beta").with_model(model("b1"));
    let alpha_mock = MockAdapter::new(alpha.id.clone()).respond_text("a");
    let beta_mock = MockAdapter::new(beta.id.clone()).respond_text("bb");
    let harness = harness(vec![alpha, beta], &[alpha_mock.clone(), beta_mock.clone()]).await;

    harness
        .runtime
        .complete(
            AiRequest::user_prompt("hi").with_model("alpha/a1".parse().unwrap()),
            &CallContext::generate(),
        )
        .await
        .unwrap();
    harness
        .runtime
        .complete(
            AiRequest::user_prompt("hi").with_model("beta/b1".parse().unwrap()),
            &CallContext::generate(),
        )
        .await
        .unwrap();

    harness.recorder.flush().await.unwrap();
    let aggregate = harness
        .recorder
        .store()
        .query(&UsageQuery {
            group_by: UsageGroupBy::Provider,
            ..UsageQuery::default()
        })
        .unwrap();

    let keys: Vec<&str> = aggregate
        .buckets
        .iter()
        .map(|bucket| bucket.key.as_str())
        .collect();
    assert_eq!(keys, vec!["alpha", "beta"]);
    assert_eq!(aggregate.buckets[0].summary.response_bytes, 1);
    assert_eq!(aggregate.buckets[1].summary.response_bytes, 2);
    assert_eq!(aggregate.total.response_bytes, 3);
    assert_eq!(aggregate.total.attempts, 2);
}

#[tokio::test]
async fn history_survives_reopening_the_database() {
    let provider = provider("mock").with_model(model("mock-1"));
    let mock = MockAdapter::new(provider.id.clone()).respond_text("ok");
    let harness = harness(vec![provider], std::slice::from_ref(&mock)).await;

    harness
        .runtime
        .complete(AiRequest::user_prompt("hi"), &CallContext::generate())
        .await
        .unwrap();
    harness.recorder.flush().await.unwrap();
    let path = harness.recorder.store().path().to_owned();
    assert!(path.ends_with(USAGE_DB_FILE));
    drop(harness);

    let reopened = UsageStore::open(&path).unwrap();
    assert_eq!(reopened.count().unwrap(), 1);
    let summary = reopened.query(&UsageQuery::default()).unwrap().total;
    assert_eq!(summary.attempts, 1);
    assert_eq!(summary.success, 1);
}

#[tokio::test]
async fn cancelled_call_before_the_first_attempt_records_nothing() {
    let provider = provider("mock").with_model(model("mock-1"));
    let mock = MockAdapter::new(provider.id.clone()).respond_text("never sent");
    let harness = harness(vec![provider], std::slice::from_ref(&mock)).await;

    let context = CallContext::generate();
    context.cancel().cancel();
    assert!(harness
        .runtime
        .complete(AiRequest::user_prompt("hi"), &context)
        .await
        .is_err());

    let summary = total(&harness, UsageQuery::default()).await.total;
    assert_eq!(summary.attempts, 0);
    assert_eq!(mock.call_count(), 0);
}

#[tokio::test]
async fn failed_attempts_expose_a_sanitized_error_code() {
    let provider = provider("mock").with_model(model("mock-1"));
    let mock = MockAdapter::new(provider.id.clone()).fail(MockFailure::AuthRejected);
    let harness = harness(vec![provider], std::slice::from_ref(&mock)).await;

    let error = harness
        .runtime
        .complete(AiRequest::user_prompt("hi"), &CallContext::generate())
        .await
        .unwrap_err();
    assert_eq!(error.kind(), ai_core::ErrorKind::Authentication);

    let query = UsageQuery {
        status: Some(RequestStatus::Failed),
        ..UsageQuery::default()
    };
    let summary = total(&harness, query.clone()).await.total;
    assert_eq!(summary.attempts, 1);
    assert_eq!(summary.failed, 1);

    // The failure detail keeps a stable error code and never the credential.
    let records = harness.recorder.store().records(&query).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].error_code.as_deref(),
        Some("CORE_AUTHENTICATION")
    );
    assert_eq!(records[0].status, RequestStatus::Failed);
    let message = records[0].error_message.clone().unwrap_or_default();
    assert!(!message.contains("test-key"));
    assert!(!message.contains("sk-"));
}
