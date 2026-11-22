//! Compile-time contract checks for the public API surface.

use std::collections::HashMap;
use std::error::Error as StdError;

use ai_core::async_utils::{CancelToken, ShutdownSignal};
use ai_core::cache::{CacheEntry, CacheKey, CacheKind, CacheLookup, CacheStore};
use ai_core::config::{AiCoreConfig, ConfigSnapshot, ConfigStore, ConfigVersion, LogLevel};
use ai_core::context::{ContextAssembler, ContextAssembly, ContextPolicy, HeuristicEstimator};
use ai_core::embedding::EmbeddingPipeline;
use ai_core::memory::{
    MemoryAppend, MemoryMessage, MemoryRole, MemorySession, MemoryStore, SimilarMessage,
};
use ai_core::observability::TraceContext;
use ai_core::output::{CachedOutputEngine, Round, RoundLoop, RoundOutcome, RoundStopReason};
use ai_core::provider::{EmbeddingRequest, EmbeddingResponse};
use ai_core::secret::SecretString;
use ai_core::storage::StorageLayout;
use ai_core::tool::{ToolExecution, ToolExecutor, ToolRegistry};
use ai_core::{CoreError, CoreResult, ErrorKind, RequestId, SessionId, StorageArea, TraceId};

fn assert_send_sync_static<T: Send + Sync + 'static>() {}
fn assert_copy_hash_eq<T: Copy + Eq + std::hash::Hash>() {}
fn assert_value_type<T: Clone + PartialEq + Eq + std::fmt::Debug>() {}
fn assert_std_error<T: StdError + Send + Sync + 'static>() {}

#[test]
fn cross_task_types_are_send_sync_static() {
    assert_send_sync_static::<CoreError>();
    assert_send_sync_static::<ErrorKind>();
    assert_send_sync_static::<CoreResult<()>>();
    assert_send_sync_static::<AiCoreConfig>();
    assert_send_sync_static::<ConfigSnapshot>();
    assert_send_sync_static::<ConfigStore>();
    assert_send_sync_static::<ConfigVersion>();
    assert_send_sync_static::<LogLevel>();
    assert_send_sync_static::<StorageLayout>();
    assert_send_sync_static::<StorageArea>();
    assert_send_sync_static::<TraceContext>();
    assert_send_sync_static::<CancelToken>();
    assert_send_sync_static::<ShutdownSignal>();
    assert_send_sync_static::<SecretString>();
    assert_send_sync_static::<RequestId>();
    assert_send_sync_static::<TraceId>();
    assert_send_sync_static::<SessionId>();
    assert_send_sync_static::<ToolRegistry>();
    assert_send_sync_static::<ToolExecutor>();
    assert_send_sync_static::<ToolExecution>();
    assert_send_sync_static::<MemoryStore>();
    assert_send_sync_static::<CacheStore>();
    assert_send_sync_static::<CacheKey>();
    assert_send_sync_static::<CacheEntry>();
    assert_send_sync_static::<CacheLookup>();
    assert_send_sync_static::<ContextAssembler>();
    assert_send_sync_static::<ContextAssembly>();
    assert_send_sync_static::<RoundLoop>();
    assert_send_sync_static::<RoundOutcome>();
    assert_send_sync_static::<Round>();
    assert_send_sync_static::<RoundStopReason>();
    assert_send_sync_static::<CachedOutputEngine>();
    assert_send_sync_static::<EmbeddingPipeline>();
    assert_send_sync_static::<EmbeddingRequest>();
    assert_send_sync_static::<EmbeddingResponse>();
    assert_send_sync_static::<SimilarMessage>();
}

#[test]
fn identifiers_are_copy_hashable_keys() {
    assert_copy_hash_eq::<RequestId>();
    assert_copy_hash_eq::<TraceId>();
    assert_copy_hash_eq::<SessionId>();
    assert_copy_hash_eq::<ConfigVersion>();

    let mut map: HashMap<RequestId, &str> = HashMap::new();
    let id = RequestId::new();
    map.insert(id, "value");
    assert_eq!(map.get(&id).copied(), Some("value"));
}

#[test]
fn configuration_types_behave_as_values() {
    assert_value_type::<AiCoreConfig>();
    assert_value_type::<ConfigVersion>();
    assert_value_type::<StorageLayout>();
    assert_value_type::<TraceContext>();
    assert_value_type::<ToolExecution>();
    assert_value_type::<MemoryAppend>();
    assert_value_type::<MemoryMessage>();
    assert_value_type::<MemorySession>();
    assert_value_type::<CacheEntry>();
    assert_value_type::<ContextPolicy>();
    assert_value_type::<ContextAssembly>();
    assert_value_type::<CacheKind>();
    assert_value_type::<HeuristicEstimator>();
    assert_copy_hash_eq::<MemoryRole>();
}

#[test]
fn error_type_implements_the_standard_traits() {
    assert_std_error::<CoreError>();

    let error = CoreError::internal("boom");
    let boxed: Box<dyn StdError + Send + Sync> = Box::new(error);
    assert!(boxed.to_string().contains("boom"));
}

#[test]
fn result_alias_matches_std_result() {
    let ok: CoreResult<u8> = Ok(1);
    let mapped: Result<u16, CoreError> = ok.map(u16::from);
    assert_eq!(mapped.ok(), Some(1));

    let failure: CoreResult<()> = Err(CoreError::Shutdown);
    assert_eq!(
        failure.err().map(|error| error.kind()),
        Some(ErrorKind::Shutdown)
    );
}
