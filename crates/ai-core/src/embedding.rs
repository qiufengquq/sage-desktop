//! Embedding pipeline: cache-first provider calls, optionally remembered.
//!
//! # Responsibility
//!
//! Tie the three embedding-related pieces together without hiding any of them:
//! [`ProviderRuntime::embed`] produces the vectors, [`CacheStore`] may serve a
//! previous answer, and [`MemoryStore`] can store the vectors next to the
//! messages they describe.
//!
//! # Boundary
//!
//! The framework does not decide *when* a text is worth embedding and does not
//! pick a model; the host does. This module only removes the repeated plumbing:
//! one normalized cache key, one degradation policy, one alignment check.
//!
//! # Cache key
//!
//! The key covers the model spec, the requested dimensions and every input text
//! (separator-delimited). Changing a single character of a single input is a
//! different entry, and two calls that differ only in their trace ids share one.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cache::{CacheKey, CacheKind, CacheLookup, CacheStore};
use crate::error::{CoreError, CoreResult};
use crate::memory::MemoryStore;
use crate::provider::config::ProviderId;
use crate::provider::response::{EmbeddingResponse, UsageReport};
use crate::provider::router::{CallContext, ProviderRuntime};
use crate::provider::types::EmbeddingRequest;

/// Payload stored in the cache for one embedding batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct CachedVectors {
    provider: String,
    model: String,
    vectors: Vec<Vec<f32>>,
    usage: Option<UsageReport>,
}

/// Cache-first wrapper around [`ProviderRuntime::embed`].
#[derive(Debug, Clone)]
pub struct EmbeddingPipeline {
    runtime: Arc<ProviderRuntime>,
    cache: Arc<CacheStore>,
    ttl: Duration,
}

impl EmbeddingPipeline {
    /// Builds the pipeline with an explicit entry lifetime.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] when `ttl` is zero.
    pub fn new(
        runtime: Arc<ProviderRuntime>,
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
            runtime,
            cache,
            ttl,
        })
    }

    /// The wrapped runtime.
    #[must_use]
    pub fn runtime(&self) -> &ProviderRuntime {
        &self.runtime
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

    /// The cache key of an embedding request.
    #[must_use]
    pub fn cache_key(request: &EmbeddingRequest) -> CacheKey {
        let model = request
            .model()
            .map(|spec| format!("{}/{}", spec.provider().as_str(), spec.model()))
            .unwrap_or_else(|| "default".to_owned());
        let dimensions = request
            .dimensions()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "default".to_owned());

        let mut parts: Vec<String> = vec![
            "ai-core.embedding.v1".to_owned(),
            model,
            dimensions,
            request.len().to_string(),
        ];
        parts.extend(request.inputs().iter().cloned());
        let borrowed: Vec<&str> = parts.iter().map(String::as_str).collect();
        CacheKey::from_parts(&borrowed)
    }

    /// Embeds a batch, serving it from the cache when a valid entry exists.
    ///
    /// # Errors
    ///
    /// Propagates validation, capability, token-budget and transport errors from
    /// the runtime. Cache failures are never surfaced: they degrade to a direct
    /// provider call.
    pub async fn embed(
        &self,
        request: EmbeddingRequest,
        context: &CallContext,
    ) -> CoreResult<EmbeddingResponse> {
        let key = Self::cache_key(&request);
        if let Some(response) = self.cached(&key) {
            tracing::debug!("served cached embedding vectors");
            return Ok(response);
        }

        let response = self.runtime.embed(request, context).await?;
        self.store(&key, &response);
        Ok(response)
    }

    /// Stores the vectors of `response` next to their messages.
    ///
    /// `message_ids` must align with the returned vectors (same order, same
    /// count); a mismatch is rejected instead of pairing the wrong text with a
    /// vector. Returns how many rows were written.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] for a count mismatch and
    /// [`CoreError::Storage`] when a row cannot be written (for example because
    /// the message id does not exist).
    pub fn remember(
        store: &MemoryStore,
        message_ids: &[i64],
        response: &EmbeddingResponse,
    ) -> CoreResult<usize> {
        if message_ids.len() != response.len() {
            return Err(CoreError::invalid_argument(
                "embedding.message_ids",
                format!(
                    "has {} entries but the response holds {} vectors",
                    message_ids.len(),
                    response.len()
                ),
            ));
        }
        for (message_id, vector) in message_ids.iter().zip(response.vectors()) {
            store.store_embedding(*message_id, response.model_id(), vector)?;
        }
        Ok(message_ids.len())
    }

    fn cached(&self, key: &CacheKey) -> Option<EmbeddingResponse> {
        match self.cache.lookup(key) {
            CacheLookup::Hit(entry) => {
                match serde_json::from_str::<CachedVectors>(&entry.payload) {
                    Ok(cached) => {
                        let provider = ProviderId::new(cached.provider).ok()?;
                        let mut response = EmbeddingResponse::new(provider, cached.model)
                            .with_vectors(cached.vectors);
                        if let Some(usage) = cached.usage {
                            response = response.with_usage(usage);
                        }
                        Some(response)
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, "discarded an unreadable embedding cache entry");
                        let _ = self.cache.invalidate(key);
                        None
                    }
                }
            }
            CacheLookup::Miss => None,
            CacheLookup::Unavailable(error) => {
                tracing::warn!(
                    code = error.error_code(),
                    "embedding cache degraded; falling back to the provider"
                );
                None
            }
        }
    }

    fn store(&self, key: &CacheKey, response: &EmbeddingResponse) {
        let payload = CachedVectors {
            provider: response.provider_id().to_string(),
            model: response.model_id().to_owned(),
            vectors: response.vectors().to_vec(),
            usage: response.usage().cloned(),
        };
        let payload = match serde_json::to_string(&payload) {
            Ok(payload) => payload,
            Err(error) => {
                tracing::warn!(error = %error, "failed to encode an embedding cache entry");
                return;
            }
        };
        if let Err(error) = self.cache.put(key, CacheKind::Embedding, payload, self.ttl) {
            tracing::warn!(
                code = error.error_code(),
                "failed to store an embedding cache entry; continuing uncached"
            );
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use crate::cache::CacheStore;
    use crate::config::ConfigStore;
    use crate::memory::{MemoryAppend, MemoryRole};
    use crate::provider::config::{
        ModelCapabilities, ModelConfig, ProviderConfig, ProviderProtocol,
    };
    use crate::provider::{AdapterFactory, MockAdapter};
    use crate::AiCoreConfig;
    use crate::SessionId;

    fn provider_config() -> ProviderConfig {
        ProviderConfig::new(
            ProviderId::new("mock").unwrap(),
            ProviderProtocol::OpenAiCompatible,
        )
        .with_api_key("test-key")
        .with_model(
            ModelConfig::new("mock-embed").with_capabilities(ModelCapabilities::embedding_only()),
        )
    }

    fn runtime(mock: &MockAdapter) -> Arc<ProviderRuntime> {
        let mut config = AiCoreConfig::default();
        config
            .providers
            .insert(provider_config().id.clone(), provider_config());
        let store = Arc::new(ConfigStore::new(config).unwrap());
        let factory = AdapterFactory::new().unwrap();
        factory.bind(mock.provider_id().clone(), Arc::new(mock.clone()));
        Arc::new(ProviderRuntime::with_factory(store, factory).unwrap())
    }

    fn mock() -> MockAdapter {
        MockAdapter::new(ProviderId::new("mock").unwrap())
    }

    fn cache() -> Arc<CacheStore> {
        Arc::new(CacheStore::open(":memory:").unwrap())
    }

    #[tokio::test]
    async fn a_repeated_batch_is_served_from_the_cache() {
        let mock = mock().respond_embeddings(vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
        let pipeline =
            EmbeddingPipeline::new(runtime(&mock), cache(), Duration::from_secs(60)).unwrap();
        let command = CallContext::generate();
        let request = EmbeddingRequest::new(["apple", "pear"]).unwrap();

        let first = pipeline.embed(request.clone(), &command).await.unwrap();
        let second = pipeline.embed(request, &command).await.unwrap();

        assert_eq!(first.vectors(), second.vectors());
        assert_eq!(first.len(), 2);
        assert_eq!(first.dimensions(), Some(2));
        assert_eq!(
            mock.call_count(),
            1,
            "the second batch must come from cache"
        );
        assert_eq!(pipeline.cache().len().unwrap(), 1);
    }

    #[tokio::test]
    async fn a_different_input_is_a_different_key() {
        let mock = mock()
            .respond_embeddings(vec![vec![1.0]])
            .respond_embeddings(vec![vec![2.0]]);
        let pipeline =
            EmbeddingPipeline::new(runtime(&mock), cache(), Duration::from_secs(60)).unwrap();
        let command = CallContext::generate();

        pipeline
            .embed(EmbeddingRequest::single("apple").unwrap(), &command)
            .await
            .unwrap();
        let second = pipeline
            .embed(EmbeddingRequest::single("pear").unwrap(), &command)
            .await
            .unwrap();

        assert_eq!(second.vectors()[0], vec![2.0]);
        assert_eq!(mock.call_count(), 2);
        assert_ne!(
            EmbeddingPipeline::cache_key(&EmbeddingRequest::single("apple").unwrap()),
            EmbeddingPipeline::cache_key(&EmbeddingRequest::single("pear").unwrap())
        );
        assert_ne!(
            EmbeddingPipeline::cache_key(&EmbeddingRequest::single("apple").unwrap()),
            EmbeddingPipeline::cache_key(
                &EmbeddingRequest::single("apple")
                    .unwrap()
                    .with_dimensions(64)
            )
        );
    }

    #[tokio::test]
    async fn vectors_can_be_remembered_and_found_again() {
        let mock = mock().respond_embeddings(vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
        let pipeline =
            EmbeddingPipeline::new(runtime(&mock), cache(), Duration::from_secs(60)).unwrap();
        let store = MemoryStore::open(":memory:").unwrap();
        let session = SessionId::new();
        let apple = store
            .append(session, MemoryAppend::new(MemoryRole::User, "apple"))
            .unwrap();
        let pear = store
            .append(session, MemoryAppend::new(MemoryRole::User, "pear"))
            .unwrap();

        let response = pipeline
            .embed(
                EmbeddingRequest::new(["apple", "pear"]).unwrap(),
                &CallContext::generate(),
            )
            .await
            .unwrap();
        let written = EmbeddingPipeline::remember(&store, &[apple.id, pear.id], &response).unwrap();
        assert_eq!(written, 2);

        let hits = store
            .search_similar(session, response.model_id(), &[0.9, 0.1], Some(1))
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].message.id, apple.id);

        // Misaligned ids are rejected instead of pairing the wrong vectors.
        assert!(EmbeddingPipeline::remember(&store, &[apple.id], &response).is_err());
    }

    #[tokio::test]
    async fn a_model_without_the_embedding_capability_is_rejected() {
        let mock = mock().respond_embeddings(vec![vec![1.0]]);
        let mut config = AiCoreConfig::default();
        // Default capabilities do not declare `embedding`.
        let provider = ProviderConfig::new(
            ProviderId::new("mock").unwrap(),
            ProviderProtocol::OpenAiCompatible,
        )
        .with_api_key("test-key")
        .with_model(ModelConfig::new("chat-only"));
        config.providers.insert(provider.id.clone(), provider);
        let store = Arc::new(ConfigStore::new(config).unwrap());
        let factory = AdapterFactory::new().unwrap();
        factory.bind(mock.provider_id().clone(), Arc::new(mock.clone()));
        let runtime = Arc::new(ProviderRuntime::with_factory(store, factory).unwrap());

        let error = runtime
            .embed(
                EmbeddingRequest::single("apple").unwrap(),
                &CallContext::generate(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.kind(), crate::ErrorKind::CapabilityUnsupported);
        assert_eq!(mock.call_count(), 0, "the gate must run before the network");
    }

    #[tokio::test]
    async fn a_retryable_embedding_failure_is_retried() {
        let mock = mock()
            .fail(crate::provider::MockFailure::RateLimited)
            .respond_embeddings(vec![vec![1.0]]);
        let pipeline =
            EmbeddingPipeline::new(runtime(&mock), cache(), Duration::from_secs(60)).unwrap();

        let response = pipeline
            .embed(
                EmbeddingRequest::single("apple").unwrap(),
                &CallContext::generate(),
            )
            .await
            .unwrap();

        assert_eq!(response.attempts(), 2);
        assert_eq!(mock.call_count(), 2);
        assert_eq!(response.vectors(), &[vec![1.0]]);
    }

    #[test]
    fn zero_ttl_is_rejected() {
        let mock = mock();
        let error = EmbeddingPipeline::new(runtime(&mock), cache(), Duration::ZERO).unwrap_err();
        assert_eq!(error.kind(), crate::ErrorKind::InvalidArgument);
    }

    #[test]
    fn corrupted_entries_degrade_to_a_miss() {
        let mock = mock();
        let pipeline =
            EmbeddingPipeline::new(runtime(&mock), cache(), Duration::from_secs(60)).unwrap();
        let request = EmbeddingRequest::single("apple").unwrap();
        let key = EmbeddingPipeline::cache_key(&request);
        pipeline
            .cache()
            .put(
                &key,
                CacheKind::Embedding,
                "not json",
                Duration::from_secs(60),
            )
            .unwrap();

        assert!(pipeline.cached(&key).is_none());
        assert!(pipeline.cache().is_empty().unwrap());
    }
}
