//! Provider abstraction — adapters, registry, routing and normalization.
//!
//! # Responsibility
//!
//! Translate a provider-agnostic AI request into exactly one upstream call and
//! normalize the answer back. The layer is split so every concern can be tested
//! in isolation:
//!
//! | module | responsibility |
//! |--------|----------------|
//! | [`types`] | provider-agnostic request/response vocabulary (`AiRequest`, `Message`, ...) |
//! | [`config`] | `ProviderId`, `ProviderConfig`, `ModelConfig`, `RetryPolicy` and their validation |
//! | [`capabilities`] | capability descriptors and token estimation (`RequestRequirements`) |
//! | [`adapter`] | the object-safe [`ProviderAdapter`] contract |
//! | [`adapters`] | concrete wire protocols (OpenAI-compatible, Anthropic, mock) |
//! | `http` (crate-private) | shared HTTP plumbing, status→error mapping and SSE decoding |
//! | [`registry`] | immutable per-snapshot index of providers and models |
//! | [`factory`] | adapter construction, connection reuse and explicit bindings |
//! | [`router`] | [`ProviderRuntime`]: snapshot binding, capability gate, retries, fallbacks, budget |
//! | [`response`] | normalized responses, embeddings, stream events and accumulation |
//!
//! # Boundaries
//!
//! * [`CoreError::Authentication`] covers missing/invalid credentials;
//!   [`CoreError::CapabilityUnsupported`] covers "this model cannot do that".
//!   Embedding calls require the `embedding` capability and are gated before any
//!   transport work, exactly like streaming or tool calling.
//! * Credentials are handed over as [`SecretString`] and unwrapped with
//!   `expose_secret()` **only** at the transport edge.
//! * Every outbound call derives its own [`TraceContext::child_request`] so a
//!   single user action can fan out into several provider calls inside one trace.
//! * In-flight requests keep the [`ConfigSnapshot`] they started with: the router
//!   resolves the snapshot and the [`ProviderRegistry`] once per call and passes
//!   them down, so a hot reload never tears a request apart.
//! * No `reqwest` type appears in the public API (the `http` module is
//!   `pub(crate)`), so
//!   the transport can be replaced without a breaking change.
//!
//! # Typical usage
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use ai_core::provider::{CallContext, ProviderRuntime};
//! use ai_core::{AiCoreConfig, ConfigStore};
//!
//! # async fn demo() -> ai_core::CoreResult<()> {
//! let store = Arc::new(ConfigStore::new(AiCoreConfig::default())?);
//! let runtime = ProviderRuntime::new(store)?;
//! let context = CallContext::generate();
//!
//! let request = ai_core::provider::AiRequest::default();
//! let response = runtime.complete(request, &context).await?;
//! assert!(!response.model_id().is_empty());
//! # Ok(())
//! # }
//! ```
//!
//! [`CoreError::Authentication`]: crate::CoreError::Authentication
//! [`CoreError::CapabilityUnsupported`]: crate::CoreError::CapabilityUnsupported
//! [`SecretString`]: crate::SecretString
//! [`TraceContext::child_request`]: crate::observability::TraceContext::child_request
//! [`ConfigSnapshot`]: crate::ConfigSnapshot

pub mod adapter;
pub mod adapters;
pub mod capabilities;
pub mod config;
pub mod factory;
pub(crate) mod http;
pub mod registry;
pub mod response;
pub mod router;
pub mod types;

pub use self::adapter::{AdapterCall, BoxFuture, EmbeddingCall, ProviderAdapter};
pub use self::adapters::{
    AnthropicAdapter, MockAdapter, MockFailure, OpenAiCompatibleAdapter, RecordedCall,
};
pub use self::capabilities::{
    check_capabilities, estimate_embedding_input_tokens, estimate_input_tokens,
    estimate_text_tokens, Capability, RequestRequirements,
};
pub use self::config::{
    ModelCapabilities, ModelConfig, ModelSpec, ProviderConfig, ProviderId, ProviderProtocol,
    RetryPolicy,
};
pub use self::factory::AdapterFactory;
pub use self::registry::{ModelDescriptor, ModelRegistry, ProviderRecord, ProviderRegistry};
pub use self::response::{
    AiResponse, AiStream, EmbeddingResponse, EstimatedTokens, FinishReason, RequestStatus,
    StreamEvent, StreamOutcome, StreamSummary, StreamTermination, ToolCall, TransportStats,
    UsageReport,
};
pub use self::router::{AttemptObserver, AttemptReport, CallContext, ProviderRuntime};
pub use self::types::{
    AiRequest, ContentPart, EmbeddingRequest, Message, ResponseFormat, Role, ToolSpec,
    MAX_EMBEDDING_INPUTS,
};
