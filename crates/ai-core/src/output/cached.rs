//! Cache-first non-streaming calls.
//!
//! # Responsibility
//!
//! Wrap [`OutputEngine`] with the disposable [`CacheStore`]: a settled answer of a
//! *normalized* request is served from the cache instead of the provider, and a
//! miss falls through to the engine and stores what came back.
//!
//! # What the key contains
//!
//! [`CacheKey`] is derived from the request shape only — model, sampling
//! parameters, stop sequences, response format, tool declarations and every
//! message (role, text, tool ids, image URLs). Correlation data (trace/request
//! ids), credentials and the request timeout are *excluded*: two logically
//! identical requests hit the same entry, and a key can never leak a secret.
//!
//! # What is never cached
//!
//! * Interrupted answers (errors, timeouts, cancellations) — they are partial.
//! * Answers that request tool calls, unless
//!   [`CachedOutputEngine::with_tool_calls`] opts in: replaying a tool call skips
//!   the tool run, which is only sound when the tool is known to be pure.
//!
//! # Failure policy
//!
//! A broken or unreadable cache is a **degradation**: the call proceeds to the
//! provider, a write failure is logged and ignored, and the caller never sees a
//! cache error as its own result. Only an actual provider failure is returned.

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cache::{CacheKey, CacheKind, CacheLookup, CacheStore};
use crate::error::{CoreError, CoreResult};
use crate::output::engine::OutputEngine;
use crate::output::event::{OutputOutcome, OutputToolCall};
use crate::output::structured::StructuredOutput;
use crate::provider::response::{FinishReason, UsageReport};
use crate::provider::router::CallContext;
use crate::provider::types::{AiRequest, Message};

/// Payload stored in the cache for one settled answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct CachedOutcome {
    text: String,
    reasoning: String,
    structured: Option<StructuredOutput>,
    tool_calls: Vec<OutputToolCall>,
    usage: Option<UsageReport>,
    finish_reason: Option<FinishReason>,
}

impl CachedOutcome {
    fn from_outcome(outcome: &OutputOutcome) -> Self {
        Self {
            text: outcome.text.clone(),
            reasoning: outcome.reasoning.clone(),
            structured: outcome.structured.clone(),
            tool_calls: outcome.tool_calls.clone(),
            usage: outcome.usage.clone(),
            finish_reason: outcome.finish_reason,
        }
    }

    fn into_outcome(self) -> OutputOutcome {
        OutputOutcome {
            text: self.text,
            reasoning: self.reasoning,
            structured: self.structured,
            tool_calls: self.tool_calls,
            usage: self.usage,
            finish_reason: self.finish_reason,
            // A cached answer is by construction a complete, non-interrupted one.
            interrupted: false,
            error: None,
        }
    }
}

/// Cache-first wrapper around one [`OutputEngine`].
#[derive(Debug, Clone)]
pub struct CachedOutputEngine {
    engine: Arc<OutputEngine>,
    cache: Arc<CacheStore>,
    ttl: Duration,
    cache_tool_calls: bool,
}

impl CachedOutputEngine {
    /// Builds the wrapper with an explicit entry lifetime.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] when `ttl` is zero.
    pub fn new(
        engine: Arc<OutputEngine>,
        cache: Arc<CacheStore>,
        ttl: Duration,
    ) -> CoreResult<Self> {
        if ttl.is_zero() {
            return Err(CoreError::invalid_argument(
                "cache.ttl",
                "must be greater than zero",
            ));
        }
        Ok(Self {
            engine,
            cache,
            ttl,
            cache_tool_calls: false,
        })
    }

    /// Also caches answers that request tool calls.
    ///
    /// Only enable this when the registered tools are pure: a cached call skips
    /// execution entirely.
    #[must_use]
    pub fn with_tool_calls(mut self, allow: bool) -> Self {
        self.cache_tool_calls = allow;
        self
    }

    /// The wrapped engine.
    #[must_use]
    pub fn engine(&self) -> &OutputEngine {
        &self.engine
    }

    /// The underlying cache.
    #[must_use]
    pub fn cache(&self) -> &CacheStore {
        &self.cache
    }

    /// Configured entry lifetime.
    #[must_use]
    pub const fn ttl(&self) -> Duration {
        self.ttl
    }

    /// The cache key of a request.
    ///
    /// Exposed so a host can invalidate exactly one logical answer after
    /// changing something outside the request shape (for example a tool result
    /// it knows to be stale).
    #[must_use]
    pub fn cache_key(request: &AiRequest) -> CacheKey {
        CacheKey::from_parts(&["ai-core.response.v1", &normalized_request(request)])
    }

    /// Runs a call, serving a cached answer when one is still valid.
    ///
    /// # Errors
    ///
    /// Propagates provider/routing errors and JSON contract violations from the
    /// engine. Cache failures are never surfaced.
    pub async fn complete(
        &self,
        request: AiRequest,
        context: &CallContext,
    ) -> CoreResult<OutputOutcome> {
        let key = Self::cache_key(&request);
        if let Some(outcome) = self.cached(&key) {
            tracing::debug!("served a cached answer");
            return Ok(outcome);
        }

        let outcome = self.engine.complete(request, context).await?;
        self.store(&key, &outcome);
        Ok(outcome)
    }

    /// Looks the key up, treating any cache problem as a miss.
    fn cached(&self, key: &CacheKey) -> Option<OutputOutcome> {
        match self.cache.lookup(key) {
            CacheLookup::Hit(entry) => {
                match serde_json::from_str::<CachedOutcome>(&entry.payload) {
                    Ok(cached) => Some(cached.into_outcome()),
                    Err(error) => {
                        // A payload written by an older/newer build is unusable;
                        // drop it so the next call does not pay for it again.
                        tracing::warn!(
                            error = %error,
                            "discarded an unreadable cache entry"
                        );
                        let _ = self.cache.invalidate(key);
                        None
                    }
                }
            }
            CacheLookup::Miss => None,
            CacheLookup::Unavailable(error) => {
                tracing::warn!(
                    code = error.error_code(),
                    "cache lookup degraded; falling back to the provider"
                );
                None
            }
        }
    }

    /// Stores a settled answer, ignoring (and logging) a write failure.
    fn store(&self, key: &CacheKey, outcome: &OutputOutcome) {
        if outcome.interrupted || (outcome.has_tool_calls() && !self.cache_tool_calls) {
            return;
        }
        let payload = match serde_json::to_string(&CachedOutcome::from_outcome(outcome)) {
            Ok(payload) => payload,
            Err(error) => {
                tracing::warn!(error = %error, "failed to encode a cache entry");
                return;
            }
        };
        if let Err(error) = self.cache.put(key, CacheKind::Response, payload, self.ttl) {
            tracing::warn!(
                code = error.error_code(),
                "failed to store a cache entry; continuing uncached"
            );
        }
    }
}

/// Builds the credential-free, order-stable description hashed into the key.
fn normalized_request(request: &AiRequest) -> String {
    let mut normalized = String::new();
    match request.model() {
        Some(model) => {
            let _ = write!(
                normalized,
                "model={}/{};",
                model.provider().as_str(),
                model.model()
            );
        }
        None => normalized.push_str("model=default;"),
    }
    let _ = write!(
        normalized,
        "temperature={:?};top_p={:?};max_output={:?};",
        request.temperature(),
        request.top_p(),
        request.max_output_tokens()
    );
    for stop in request.stop() {
        let _ = write!(normalized, "stop={stop}\u{1f};");
    }
    normalized.push_str(&format!("format={:?};", request.response_format()));
    for tool in request.tools() {
        let _ = write!(
            normalized,
            "tool={}:{}:{};",
            tool.name(),
            tool.description(),
            tool.parameters()
        );
    }
    for (key, value) in request.extra_body() {
        let _ = write!(normalized, "extra={key}={value};");
    }
    for message in request.messages() {
        normalized.push_str(&message_key(message));
    }
    normalized
}

/// One message's contribution to the key.
fn message_key(message: &Message) -> String {
    let mut key = String::new();
    let _ = write!(key, "msg={}:", message.role().as_str());
    for part in message.parts() {
        match part {
            crate::provider::types::ContentPart::Text(text) => {
                let _ = write!(key, "text={text}\u{1f}");
            }
            crate::provider::types::ContentPart::Image { url } => {
                let _ = write!(key, "image={url}\u{1f}");
            }
        }
    }
    if let Some(call_id) = message.tool_call_id() {
        let _ = write!(key, "call_id={call_id};");
    }
    for call in message.tool_calls() {
        let _ = write!(key, "call={}:{}:{};", call.id, call.name, call.arguments);
    }
    key.push(';');
    key
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::cache::CacheStore;
    use crate::config::ConfigStore;
    use crate::provider::config::{ModelConfig, ProviderConfig, ProviderId, ProviderProtocol};
    use crate::provider::response::ToolCall;
    use crate::provider::{AdapterFactory, MockAdapter, ProviderRuntime};
    use crate::AiCoreConfig;

    fn provider_config() -> ProviderConfig {
        ProviderConfig::new(
            ProviderId::new("mock").unwrap(),
            ProviderProtocol::OpenAiCompatible,
        )
        .with_api_key("test-key")
        .with_model(ModelConfig::new("mock-1"))
    }

    fn engine(mock: &MockAdapter) -> Arc<OutputEngine> {
        let mut config = AiCoreConfig::default();
        config
            .providers
            .insert(provider_config().id.clone(), provider_config());
        let store = Arc::new(ConfigStore::new(config).unwrap());
        let factory = AdapterFactory::new().unwrap();
        factory.bind(mock.provider_id().clone(), Arc::new(mock.clone()));
        let runtime = ProviderRuntime::with_factory(store, factory).unwrap();
        Arc::new(OutputEngine::new(Arc::new(runtime)))
    }

    fn cache() -> Arc<CacheStore> {
        Arc::new(CacheStore::open(":memory:").unwrap())
    }

    fn cached(mock: &MockAdapter) -> CachedOutputEngine {
        CachedOutputEngine::new(engine(mock), cache(), Duration::from_secs(60)).unwrap()
    }

    fn call(id: &str) -> ToolCall {
        ToolCall {
            id: id.to_owned(),
            name: "echo".to_owned(),
            arguments: "{}".to_owned(),
        }
    }

    #[tokio::test]
    async fn a_repeated_request_is_served_from_the_cache() {
        let mock = MockAdapter::new(ProviderId::new("mock").unwrap()).respond_text("hello");
        let cached_engine = cached(&mock);
        let command = CallContext::generate();

        let first = cached_engine
            .complete(AiRequest::user_prompt("hi"), &command)
            .await
            .unwrap();
        let second = cached_engine
            .complete(AiRequest::user_prompt("hi"), &command)
            .await
            .unwrap();

        assert_eq!(first.text, "hello");
        assert_eq!(second.text, "hello");
        assert_eq!(second.finish_reason, first.finish_reason);
        assert_eq!(
            mock.call_count(),
            1,
            "the second call must not reach the provider"
        );
        assert_eq!(cached_engine.cache().len().unwrap(), 1);
    }

    #[tokio::test]
    async fn a_different_prompt_is_a_different_key() {
        let mock = MockAdapter::new(ProviderId::new("mock").unwrap()).respond_text("a");
        let cached_engine = cached(&mock);
        let command = CallContext::generate();

        cached_engine
            .complete(AiRequest::user_prompt("one"), &command)
            .await
            .unwrap();
        cached_engine
            .complete(AiRequest::user_prompt("two"), &command)
            .await
            .unwrap();
        assert_eq!(mock.call_count(), 2);
        assert_eq!(cached_engine.cache().len().unwrap(), 2);
    }

    #[tokio::test]
    async fn tool_call_answers_are_not_cached_by_default() {
        let mock = MockAdapter::new(ProviderId::new("mock").unwrap())
            .respond_tool_calls("", vec![call("call_1")])
            .respond_tool_calls("", vec![call("call_2")]);
        let cached_engine = cached(&mock);
        let command = CallContext::generate();

        let first = cached_engine
            .complete(AiRequest::user_prompt("hi"), &command)
            .await
            .unwrap();
        assert!(first.has_tool_calls());
        cached_engine
            .complete(AiRequest::user_prompt("hi"), &command)
            .await
            .unwrap();
        assert_eq!(mock.call_count(), 2);
        assert!(cached_engine.cache().is_empty().unwrap());
    }

    #[tokio::test]
    async fn tool_call_answers_can_be_cached_explicitly() {
        let mock = MockAdapter::new(ProviderId::new("mock").unwrap())
            .respond_tool_calls("", vec![call("call_1")]);
        let cached_engine =
            CachedOutputEngine::new(engine(&mock), cache(), Duration::from_secs(60))
                .unwrap()
                .with_tool_calls(true);
        let command = CallContext::generate();

        cached_engine
            .complete(AiRequest::user_prompt("hi"), &command)
            .await
            .unwrap();
        let second = cached_engine
            .complete(AiRequest::user_prompt("hi"), &command)
            .await
            .unwrap();
        assert!(second.has_tool_calls());
        assert_eq!(mock.call_count(), 1);
    }

    #[test]
    fn keys_include_the_request_shape_only() {
        let request = AiRequest::user_prompt("hi");
        let same = AiRequest::user_prompt("hi");
        let other = AiRequest::user_prompt("hi").with_temperature(0.5);

        assert_eq!(
            CachedOutputEngine::cache_key(&request),
            CachedOutputEngine::cache_key(&same)
        );
        assert_ne!(
            CachedOutputEngine::cache_key(&request),
            CachedOutputEngine::cache_key(&other)
        );
    }

    #[test]
    fn zero_ttl_is_rejected() {
        let mock = MockAdapter::new(ProviderId::new("mock").unwrap());
        let error = CachedOutputEngine::new(engine(&mock), cache(), Duration::ZERO).unwrap_err();
        assert_eq!(error.kind(), crate::ErrorKind::InvalidArgument);
    }

    #[test]
    fn corrupted_entries_degrade_to_a_miss() {
        let mock = MockAdapter::new(ProviderId::new("mock").unwrap());
        let cached_engine = cached(&mock);
        let request = AiRequest::user_prompt("hi");
        let key = CachedOutputEngine::cache_key(&request);
        cached_engine
            .cache()
            .put(
                &key,
                CacheKind::Response,
                "not json",
                Duration::from_secs(60),
            )
            .unwrap();

        assert!(cached_engine.cached(&key).is_none());
        assert!(cached_engine.cache().is_empty().unwrap());
    }
}
