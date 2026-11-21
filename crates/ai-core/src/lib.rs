//! # `ai-core` — the AI Core Framework kernel
//!
//! A reusable, provider-agnostic, UI-agnostic foundation for AI features. The
//! crate currently ships the substrate (error model, typed configuration with
//! immutable snapshots, observability, cancellation/timeout/lifecycle
//! primitives, storage boundaries), the provider runtime (adapters, routing,
//! retries, streaming), usage/traffic accounting with SQLite persistence, the
//! output engine (unified events, partial-output preservation, structured JSON
//! validation, multi-round tool loop), the tool subsystem (declarations,
//! registry, schema-checked and bounded execution), durable conversation memory
//! (`ai-memory/`), budgeted context assembly and a disposable response cache
//! (`ai-cache/`). Host-specific bridging lives in the separate `ai-core-tauri`
//! crate, which depends on this one — never the reverse.
//!
//! ## Module map
//!
//! | module | responsibility | status | depends on |
//! |--------|----------------|--------|------------|
//! | [`error`] | `CoreError` / `ErrorKind` / `CoreResult`, classification & sources | implemented | [`secret`] |
//! | [`ids`] | `RequestId`, `TraceId`, `SessionId` newtypes | implemented | [`error`] |
//! | [`secret`] | `SecretString` and key-based redaction | implemented | — |
//! | [`storage`] | isolated memory/metadata/cache path layout | implemented | [`error`], [`async_utils`] |
//! | [`config`] | typed config, validation, immutable snapshots | implemented | [`error`], [`storage`] |
//! | [`observability`] | trace context, span helpers, redaction bridge | implemented | [`ids`], [`secret`], [`config`] |
//! | [`async_utils`] | timeout, cancellation, blocking isolation, shutdown | implemented | [`error`] |
//! | [`provider`] | adapters, registry, routing, retries, fallbacks, streaming | implemented (Phase 2) | [`config`], [`error`], [`observability`] |
//! | [`usage`] | token & traffic accounting, SQLite persistence, aggregation | implemented (Phase 3) | [`provider`], [`storage`], [`async_utils`] |
//! | [`output`] | unified events, stream assembly, structured output, tool-round loop, cache-first calls | implemented (Phase 4, rounds and caching in Phase 8) | [`provider`], [`error`], [`tool`], [`cache`] |
//! | [`tool`] | tool declarations, registry, argument validation, bounded execution | implemented (Phase 5) | [`output`], [`provider`], [`async_utils`], [`secret`] |
//! | [`memory`] | durable conversation memory in `ai-memory/`, FTS5 + vector retrieval, summaries | implemented (Phase 6–9) | [`storage`] |
//! | [`embedding`] | cache-first embedding batches, stored into memory | implemented (Phase 9) | [`provider`], [`cache`], [`memory`] |
//! | [`context`] | budgeted context assembly, tool-exchange-safe trimming, summary replay | implemented (Phase 7) | [`memory`], [`provider`], [`config`] |
//! | [`cache`] | disposable response/embedding cache in `ai-cache/` | implemented (Phase 8) | [`storage`] |
//!
//! The dependency direction stays acyclic: `error` is a leaf, `provider` depends
//! on configuration/observability, `usage` consumes provider reports through the
//! `AttemptObserver` trait, and the output/tool/context layers sit above both.
//! `context` is the only module that reads [`memory`]: nothing queries the
//! memory or cache databases directly, and neither shares a file with the client
//! business database.
//!
//! ## Deliberate non-goals (current phases)
//!
//! * No `tauri::` type, no Tauri command/event, no frontend assumption. The host
//!   (Tauri adapter) is expected to depend on this crate, never the reverse.
//! * No provider SDK: providers are spoken to over plain HTTP (OpenAI-compatible
//!   and Anthropic protocols), and tests use a mock adapter.
//! * No Python, no sidecar: the ML ecosystem can be reached in a later phase via
//!   a separate process over an explicit IPC boundary.
//! * The AI memory/usage databases are *not* the client's business SQLite
//!   database; see [`storage`] for the enforced separation.
//!
//! ## Minimal usage
//!
//! ```
//! use std::time::Duration;
//!
//! use ai_core::async_utils::{run_with_limits, CancelToken};
//! use ai_core::observability::{request_span, Instrument, TraceContext};
//! use ai_core::{AiCoreConfig, ConfigStore};
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> ai_core::CoreResult<()> {
//! let store = ConfigStore::new(AiCoreConfig::default())?;
//! let snapshot = store.snapshot();
//!
//! let context = TraceContext::generate();
//! let span = request_span("demo.operation", &context);
//! let token = CancelToken::new();
//!
//! let value = run_with_limits(
//!     "demo.operation",
//!     snapshot.config().timeouts.request,
//!     &token,
//!     async { 21_u32 * 2 },
//! )
//! .instrument(span)
//! .await;
//!
//! assert_eq!(value?, 42);
//! assert_eq!(snapshot.version().get(), 1);
//! assert_eq!(Duration::from_secs(60), snapshot.config().timeouts.request);
//! # Ok(())
//! # }
//! ```
//!
//! ## Guarantees checked in CI
//!
//! * `cargo check --workspace --all-targets`
//! * `cargo test --workspace`
//! * `cargo clippy --workspace --all-targets --all-features -- -D warnings`
//! * `cargo fmt --check`
//!
//! Public types that cross task or thread boundaries are `Send + Sync + 'static`
//! (asserted in `tests/public_api_tests.rs`).

#![deny(missing_docs)]
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod async_utils;
pub mod cache;
pub mod config;
pub mod context;
pub mod embedding;
pub mod error;
pub mod ids;
pub mod memory;
pub mod observability;
pub mod output;
pub mod provider;
pub mod secret;
pub mod storage;
pub mod tool;
pub mod usage;

pub use crate::cache::{CacheEntry, CacheKey, CacheKind, CacheLookup, CacheStore};
pub use crate::config::{AiCoreConfig, ConfigSnapshot, ConfigStore, ConfigVersion};
pub use crate::context::{
    ContextAssembler, ContextAssembly, ContextPolicy, HeuristicEstimator, TokenEstimator,
};
pub use crate::embedding::EmbeddingPipeline;
pub use crate::error::{CoreError, CoreResult, ErrorKind, StorageArea};
pub use crate::ids::{RequestId, SessionId, TraceId};
pub use crate::memory::{
    MemoryAppend, MemoryMessage, MemoryRole, MemorySession, MemoryStore, SimilarMessage,
};
pub use crate::output::CachedOutputEngine;
pub use crate::secret::SecretString;
pub use crate::storage::StorageLayout;
pub use crate::tool::{Tool, ToolExecution, ToolExecutor, ToolRegistry};
