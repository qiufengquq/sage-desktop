//! The `ProviderAdapter` contract.
//!
//! An adapter knows exactly one *wire protocol* (OpenAI chat-completions,
//! Anthropic messages, ...): it builds the HTTP body, injects authentication,
//! parses the answer and decodes the streaming frames. Everything above it
//! (registry, router, retry, capability checks) is protocol-agnostic.
//!
//! # Object safety
//!
//! The trait is object safe (`Arc<dyn ProviderAdapter>` is used by the registry)
//! because the async methods return a boxed [`BoxFuture`] instead of using
//! `async fn` in the trait. Boxing costs one allocation per call, which is
//! negligible compared to a network round-trip, and it keeps the registry
//! homogeneous over protocols.
//!
//! # Implementing a new provider
//!
//! 1. Add a variant to [`crate::provider::ProviderProtocol`].
//! 2. Implement this trait for the new adapter (build the body, inject the key in
//!    the smallest possible scope via `expose_secret()`, map HTTP statuses with
//!    the crate-private `http::status_error` / `http::network_error` helpers,
//!    sanitize every message).
//! 3. Register it with [`crate::provider::AdapterFactory::bind`] (or extend the
//!    factory's protocol match).
//!
//! No other module needs to change: routing, retries, fallbacks, capability
//! validation and observability are shared.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use crate::async_utils::CancelToken;
use crate::observability::TraceContext;
use crate::provider::config::{ModelConfig, ProviderConfig, ProviderProtocol};
use crate::provider::response::{AiResponse, AiStream, EmbeddingResponse};
use crate::provider::types::{AiRequest, EmbeddingRequest};
use crate::CoreResult;

/// Boxed, `Send` future used by the object-safe adapter methods.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Everything an adapter needs for one upstream call.
///
/// All references point into data owned by the caller (the request and the
/// configuration snapshot), so an adapter can only observe the snapshot the
/// request resolved against — never a newer one.
#[derive(Debug)]
pub struct AdapterCall<'a> {
    /// The provider-agnostic request.
    pub request: &'a AiRequest,
    /// Provider configuration (endpoint, key, headers, extras).
    pub provider: &'a ProviderConfig,
    /// Resolved model configuration (capabilities, wire id).
    pub model: &'a ModelConfig,
    /// Trace context of this upstream call.
    pub trace: &'a TraceContext,
    /// Cancellation signal; adapters must stop reading when it fires.
    pub cancel: &'a CancelToken,
    /// Maximum silence tolerated inside an open stream.
    pub streaming_idle: Duration,
}

/// Everything an adapter needs for one embedding call.
///
/// Same ownership rules as [`AdapterCall`]: every reference points into data the
/// caller keeps alive for the duration of the call.
#[derive(Debug)]
pub struct EmbeddingCall<'a> {
    /// Provider-agnostic embedding request.
    pub request: &'a EmbeddingRequest,
    /// Provider configuration (endpoint, key, headers, extras).
    pub provider: &'a ProviderConfig,
    /// Resolved model configuration (capabilities, wire id).
    pub model: &'a ModelConfig,
    /// Trace context of this upstream call.
    pub trace: &'a TraceContext,
    /// Cancellation signal.
    pub cancel: &'a CancelToken,
}

/// Protocol-specific implementation of an AI provider.
pub trait ProviderAdapter: Send + Sync {
    /// Wire protocol implemented by this adapter.
    fn protocol(&self) -> ProviderProtocol;

    /// Short label used in logs and span fields.
    fn label(&self) -> &str {
        "provider-adapter"
    }

    /// Executes one non-streaming request.
    fn complete<'a>(&'a self, call: AdapterCall<'a>) -> BoxFuture<'a, CoreResult<AiResponse>>;

    /// Opens a streaming request.
    ///
    /// The returned stream must emit [`crate::provider::StreamEvent::Started`]
    /// first and end with either `Finished` or an `Err` item.
    fn stream<'a>(&'a self, call: AdapterCall<'a>) -> BoxFuture<'a, CoreResult<AiStream>>;

    /// Executes one embedding request.
    ///
    /// Protocols without an embeddings endpoint keep this default, which reports
    /// [`crate::CoreError::CapabilityUnsupported`] instead of a transport error —
    /// the router normally rejects that combination before reaching the network,
    /// so this is the last line of defence.
    fn embed<'a>(
        &'a self,
        call: EmbeddingCall<'a>,
    ) -> BoxFuture<'a, CoreResult<EmbeddingResponse>> {
        Box::pin(async move {
            Err(crate::CoreError::capability_unsupported(
                call.provider.id.to_string(),
                call.model.id.clone(),
                crate::provider::capabilities::Capability::Embedding.as_str(),
            ))
        })
    }
}
