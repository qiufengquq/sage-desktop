//! End-to-end tests: memory → context → provider → tool loop, plus the cache.
//!
//! These exercise the Phase 6–8 modules through their public API only, on the
//! same isolated storage layout a host would derive from `AiCoreConfig`.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use ai_core::cache::{CacheKind, CacheLookup, CacheStore};
use ai_core::context::{
    ContextAssembler, ContextAssembly, ContextPolicy, HeuristicEstimator, TokenEstimator,
};
use ai_core::embedding::EmbeddingPipeline;
use ai_core::memory::{MemoryAppend, MemoryRole, MemoryStore};
use ai_core::output::{CachedOutputEngine, OutputEngine, RoundLoop, RoundStopReason};
use ai_core::provider::response::ToolCall;
use ai_core::provider::{
    AdapterFactory, AiRequest, CallContext, EmbeddingRequest, Message, MockAdapter,
    ModelCapabilities, ModelConfig, ProviderConfig, ProviderId, ProviderProtocol, ProviderRuntime,
    ToolSpec,
};
use ai_core::storage::StorageLayout;
use ai_core::tool::{Tool, ToolExecutor, ToolFuture, ToolRegistry};
use ai_core::{AiCoreConfig, ConfigStore, SessionId};

fn layout() -> (tempfile::TempDir, StorageLayout) {
    let directory = tempfile::tempdir().expect("temp dir");
    let layout = StorageLayout::derive(directory.path().join("ai-core")).expect("layout");
    (directory, layout)
}

fn provider() -> ProviderConfig {
    ProviderConfig::new(
        ProviderId::new("mock").expect("provider id"),
        ProviderProtocol::OpenAiCompatible,
    )
    .with_api_key("test-key")
    .with_model(ModelConfig::new("mock-1"))
}

fn engine(mock: &MockAdapter) -> Arc<OutputEngine> {
    let mut config = AiCoreConfig::default();
    config.providers.insert(provider().id.clone(), provider());
    let store = Arc::new(ConfigStore::new(config).expect("config"));
    let factory = AdapterFactory::new().expect("factory");
    factory.bind(mock.provider_id().clone(), Arc::new(mock.clone()));
    let runtime = ProviderRuntime::with_factory(store, factory).expect("runtime");
    Arc::new(OutputEngine::new(Arc::new(runtime)))
}

/// Tool that returns its arguments unchanged.
struct EchoTool {
    spec: ToolSpec,
}

impl EchoTool {
    fn new() -> Self {
        Self {
            spec: ToolSpec::new(
                "echo",
                "echoes its arguments",
                json!({"type": "object", "properties": {}}),
            )
            .expect("tool spec"),
        }
    }
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

fn executor() -> ToolExecutor {
    let mut registry = ToolRegistry::new();
    registry
        .register(Arc::new(EchoTool::new()))
        .expect("register tool");
    ToolExecutor::new(Arc::new(registry), Duration::from_secs(1))
}

#[test]
fn memory_feeds_a_budgeted_context() {
    let (_directory, layout) = layout();
    let store = MemoryStore::open_in_layout(&layout).expect("memory store");
    let session = SessionId::new();

    store
        .append(
            session,
            MemoryAppend::new(MemoryRole::System, "you are terse"),
        )
        .expect("append system");
    for turn in 0..40 {
        store
            .append(
                session,
                MemoryAppend::new(MemoryRole::User, format!("question {turn}")),
            )
            .expect("append user");
    }

    let assembler =
        ContextAssembler::new(ContextPolicy::new(120, 20).expect("policy")).expect("assembler");
    let assembly = assembler
        .from_memory(&store, session, Some(100))
        .expect("assemble");

    assert!(assembly.is_compacted());
    assert!(assembly.message_count() > 1);
    assert_eq!(assembly.messages[0].role(), ai_core::provider::Role::System);
    let last = assembly.messages.last().expect("last message");
    assert_eq!(last.text_content(), "question 39");
    assert!(assembly.estimated_input_tokens <= assembly.message_count() * 40);
}

#[test]
fn a_custom_estimator_controls_the_budget() {
    struct FixedEstimator;

    impl TokenEstimator for FixedEstimator {
        fn estimate(&self, text: &str) -> usize {
            if text.is_empty() {
                0
            } else {
                100
            }
        }
    }

    let history = [
        Message::user("one"),
        Message::user("two"),
        Message::user("three"),
    ];
    let policy = ContextPolicy::new(160, 16).expect("policy");
    let heuristic = ContextAssembler::new(policy)
        .expect("assembler")
        .assemble(&history);
    let strict = ContextAssembler::new(policy)
        .expect("assembler")
        .with_estimator(Arc::new(FixedEstimator))
        .assemble(&history);

    assert_eq!(heuristic.dropped_messages, 0);
    assert!(strict.dropped_messages > 0);
    assert!(strict.message_count() < heuristic.message_count());
    assert_eq!(HeuristicEstimator::default().estimate("abcd"), 1);
}

#[test]
fn cache_entries_survive_a_reopen_and_respect_their_ttl() {
    let (_directory, layout) = layout();
    let key = ai_core::cache::CacheKey::from_parts(&["mock", "mock-1", "prompt"]);
    {
        let store = CacheStore::open_in_layout(&layout).expect("cache");
        store
            .put(
                &key,
                CacheKind::Response,
                "{\"text\":\"cached\"}",
                Duration::from_secs(60),
            )
            .expect("put");
        assert_eq!(store.lookup(&key).payload(), Some("{\"text\":\"cached\"}"));
    }

    let reopened = CacheStore::open_in_layout(&layout).expect("cache");
    match reopened.lookup(&key) {
        CacheLookup::Hit(entry) => {
            assert_eq!(entry.payload, "{\"text\":\"cached\"}");
            assert_eq!(entry.hits, 2);
            assert_eq!(entry.kind, CacheKind::Response);
        }
        other => panic!("expected a hit, got {other:?}"),
    }
    assert_ne!(
        layout.cache(),
        layout.memory(),
        "cache and memory must never share a directory"
    );
    assert!(reopened.path().starts_with(layout.cache()));
}

#[test]
fn memory_search_and_summary_feed_the_context() {
    let (_directory, layout) = layout();
    let store = MemoryStore::open_in_layout(&layout).expect("memory store");
    let session = SessionId::new();

    store
        .append(
            session,
            MemoryAppend::new(MemoryRole::User, "the apple pie recipe"),
        )
        .expect("append");
    store
        .append(
            session,
            MemoryAppend::new(MemoryRole::Assistant, "use cinnamon"),
        )
        .expect("append");

    let hits = store.search(session, "apple", None).expect("search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].content, "the apple pie recipe");
    assert!(store.search_all("cinnamon", None).expect("search").len() == 1);

    store
        .set_summary(session, Some("the user asked about apple pie"))
        .expect("summary");

    let assembler =
        ContextAssembler::new(ContextPolicy::new(400, 32).expect("policy")).expect("assembler");
    let assembly = assembler
        .from_memory_with_summary(&store, session, Some(10))
        .expect("assemble");
    assert_eq!(assembly.message_count(), 3);
    assert!(assembly.messages[0].text_content().contains("apple pie"));
    assert_eq!(assembly.messages[0].role(), ai_core::provider::Role::System);
}

#[tokio::test]
async fn embeddings_are_cached_and_searchable_after_a_reopen() {
    let (_directory, layout) = layout();
    let cache = Arc::new(CacheStore::open_in_layout(&layout).expect("cache"));
    let store = MemoryStore::open_in_layout(&layout).expect("memory");

    let provider = ProviderConfig::new(
        ProviderId::new("mock").expect("provider"),
        ProviderProtocol::OpenAiCompatible,
    )
    .with_api_key("test-key")
    .with_model(
        ModelConfig::new("mock-embed").with_capabilities(ModelCapabilities::embedding_only()),
    );
    let mut config = AiCoreConfig::default();
    config.providers.insert(provider.id.clone(), provider);
    let config_store = Arc::new(ConfigStore::new(config).expect("config"));
    let mock = MockAdapter::new(ProviderId::new("mock").expect("provider"))
        .respond_embeddings(vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
    let factory = AdapterFactory::new().expect("factory");
    factory.bind(mock.provider_id().clone(), Arc::new(mock.clone()));
    let runtime = Arc::new(ProviderRuntime::with_factory(config_store, factory).expect("runtime"));

    let session = SessionId::new();
    let apple = store
        .append(session, MemoryAppend::new(MemoryRole::User, "apple"))
        .expect("append");
    let pear = store
        .append(session, MemoryAppend::new(MemoryRole::User, "pear"))
        .expect("append");

    let pipeline = EmbeddingPipeline::new(
        Arc::clone(&runtime),
        Arc::clone(&cache),
        Duration::from_secs(60),
    )
    .expect("pipeline");
    let request = EmbeddingRequest::new(["apple", "pear"]).expect("request");
    let command = CallContext::generate();

    let response = pipeline
        .embed(request.clone(), &command)
        .await
        .expect("embed");
    assert_eq!(response.len(), 2);
    assert_eq!(response.dimensions(), Some(2));
    EmbeddingPipeline::remember(&store, &[apple.id, pear.id], &response).expect("remember");

    // Second batch is served from the cache.
    let again = pipeline.embed(request, &command).await.expect("cached");
    assert_eq!(again.vectors(), response.vectors());
    assert_eq!(mock.call_count(), 1);

    let hits = store
        .search_similar(session, response.model_id(), &[0.9, 0.1], Some(1))
        .expect("similar");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].message.id, apple.id);
    assert!(hits[0].score > 0.99);

    // The cached payload survives a reopen of the cache database.
    let reopened = CacheStore::open_in_layout(&layout).expect("cache");
    let key = EmbeddingPipeline::cache_key(&EmbeddingRequest::new(["apple", "pear"]).unwrap());
    assert!(matches!(reopened.lookup(&key), CacheLookup::Hit(_)));
}

#[tokio::test]
async fn a_cache_first_engine_skips_the_second_provider_call() {
    let (_directory, layout) = layout();
    let cache = Arc::new(CacheStore::open_in_layout(&layout).expect("cache"));
    let mock =
        MockAdapter::new(ProviderId::new("mock").expect("provider")).respond_text("cached answer");
    let cached_engine =
        CachedOutputEngine::new(engine(&mock), Arc::clone(&cache), Duration::from_secs(60))
            .expect("cached engine");
    let command = CallContext::generate();

    let first = cached_engine
        .complete(AiRequest::user_prompt("hi"), &command)
        .await
        .expect("first");
    let second = cached_engine
        .complete(AiRequest::user_prompt("hi"), &command)
        .await
        .expect("second");

    assert_eq!(first.text, "cached answer");
    assert_eq!(second.text, "cached answer");
    assert_eq!(mock.call_count(), 1);
    assert_eq!(cache.len().expect("len"), 1);

    let reopened = CacheStore::open_in_layout(&layout).expect("cache");
    let key = CachedOutputEngine::cache_key(&AiRequest::user_prompt("hi"));
    assert!(matches!(reopened.lookup(&key), CacheLookup::Hit(_)));
}

#[tokio::test]
async fn a_tool_round_is_replayed_and_the_answer_is_stored() {
    let (_directory, layout) = layout();
    let store = MemoryStore::open_in_layout(&layout).expect("memory store");
    let session = SessionId::new();
    store
        .append(session, MemoryAppend::new(MemoryRole::System, "be brief"))
        .expect("append system");
    store
        .append(
            session,
            MemoryAppend::new(MemoryRole::User, "what is an apple?"),
        )
        .expect("append user");

    let assembler =
        ContextAssembler::new(ContextPolicy::new(400, 32).expect("policy")).expect("assembler");
    let ContextAssembly { messages, .. } = assembler
        .from_memory(&store, session, Some(20))
        .expect("assemble");
    let context_len = messages.len();

    let mock = MockAdapter::new(ProviderId::new("mock").expect("provider"))
        .respond_tool_calls(
            "",
            vec![ToolCall {
                id: "call_1".to_owned(),
                name: "echo".to_owned(),
                arguments: "{\"word\":\"apple\"}".to_owned(),
            }],
        )
        .respond_text("an apple is a fruit");

    let engine = engine(&mock);
    let runner = RoundLoop::new(engine, executor());
    let mut events = Vec::new();

    let outcome = runner
        .run(
            AiRequest::new(messages),
            &CallContext::generate(),
            |event| events.push(event.clone()),
        )
        .await
        .expect("round loop");

    assert!(outcome.is_complete());
    assert_eq!(outcome.stop_reason, RoundStopReason::Completed);
    assert_eq!(outcome.final_outcome.text, "an apple is a fruit");
    assert_eq!(outcome.executions.len(), 1);
    assert!(outcome.executions[0].is_success());
    assert_eq!(events.len(), 1, "one execution event per tool call");
    assert_eq!(mock.calls().len(), 2);
    assert!(
        mock.calls()[1].message_count >= context_len + 2,
        "the tool exchange must be replayed to the model"
    );

    store
        .append(
            session,
            MemoryAppend::new(MemoryRole::Assistant, outcome.final_outcome.text.clone()),
        )
        .expect("append answer");
    let history = store.history(session, None).expect("history");
    assert_eq!(history.len(), 3);
    assert_eq!(history[2].content, "an apple is a fruit");
}

#[tokio::test]
async fn an_exhausted_tool_loop_reports_the_reason() {
    let call = ToolCall {
        id: "call_1".to_owned(),
        name: "echo".to_owned(),
        arguments: "{}".to_owned(),
    };
    let mock = MockAdapter::new(ProviderId::new("mock").expect("provider"))
        .respond_tool_calls("", vec![call.clone()])
        .respond_tool_calls("", vec![call]);

    let engine = engine(&mock);
    let runner = RoundLoop::new(engine, executor())
        .with_max_rounds(1)
        .expect("budget");

    let outcome = runner
        .run(
            AiRequest::user_prompt("loop forever"),
            &CallContext::generate(),
            |_| {},
        )
        .await
        .expect("round loop");

    assert!(outcome.is_exhausted());
    assert!(!outcome.is_complete());
    assert_eq!(outcome.round_count(), 1);
    assert_eq!(outcome.executions.len(), 1);
}
