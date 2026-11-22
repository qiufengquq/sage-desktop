//! Typed configuration, validation and immutable snapshots.
//!
//! # Design
//!
//! * [`AiCoreConfig`] is a plain, `Serialize`/`Deserialize`, value-typed struct
//!   composed of focused sub-structs. It never contains credentials: secrets stay
//!   in [`crate::secret::SecretString`] values owned by the provider layer.
//! * Validation is explicit ([`AiCoreConfig::validate`]) and returns
//!   [`CoreError::Config`] with a `field: reason` message.
//! * [`ConfigSnapshot`] is an immutable, reference-counted view of a validated
//!   configuration plus its [`ConfigVersion`]. [`ConfigStore`] holds the current
//!   snapshot behind an `Arc`; readers clone the `Arc` under a very short read
//!   lock (no allocation, no contention beyond the pointer copy) and then work
//!   lock-free.
//!
//! # Phase 2 hook (hot reload)
//!
//! Applying a new configuration always *creates a new snapshot* and bumps the
//! version; existing holders keep the old `Arc` untouched. That is exactly the
//! semantic required by "in-flight requests keep the configuration they started
//! with, new requests read the new one": a request grabs
//! [`ConfigStore::snapshot`] once at its start and passes it down.
//!
//! # Locking rules
//!
//! [`ConfigStore`] uses a `std::sync::RwLock` **only** to swap/clone the `Arc`.
//! The guard never crosses an `.await`, and no user code runs while it is held.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::provider::config::{ModelSpec, ProviderConfig, ProviderId};
use crate::storage::StorageLayout;

/// Default deadline for a whole AI request.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// Default deadline for establishing an upstream connection.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Default maximum silence while a stream is open.
pub const DEFAULT_STREAMING_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// Default grace period granted to background tasks during shutdown.
pub const DEFAULT_SHUTDOWN_GRACE: Duration = Duration::from_secs(10);

/// Upper bound accepted for any timeout (1 hour).
pub const MAX_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// Maximum accepted value for [`ConcurrencyConfig::max_concurrent_requests`].
pub const MAX_CONCURRENT_REQUESTS: usize = 1024;
/// Maximum accepted value for [`ConcurrencyConfig::max_blocking_tasks`].
pub const MAX_BLOCKING_TASKS: usize = 4096;

/// Default capacity of the usage write queue.
pub const DEFAULT_USAGE_BUFFER_CAPACITY: usize = 1024;
/// Maximum accepted value for [`UsageConfig::buffer_capacity`].
pub const MAX_USAGE_BUFFER_CAPACITY: usize = 65_536;
/// Default number of usage records written per transaction.
pub const DEFAULT_USAGE_BATCH_SIZE: usize = 128;
/// Maximum accepted value for [`UsageConfig::batch_size`].
pub const MAX_USAGE_BATCH_SIZE: usize = 4096;
/// Default delay before a partial usage batch is flushed.
pub const DEFAULT_USAGE_FLUSH_INTERVAL: Duration = Duration::from_secs(2);

/// Default input-token budget of one assembled context.
pub const DEFAULT_CONTEXT_MAX_INPUT_TOKENS: usize = 8192;
/// Default output tokens reserved out of the context budget.
pub const DEFAULT_CONTEXT_RESERVE_TOKENS: usize = 1024;
/// Maximum accepted value for [`ContextConfig::max_input_tokens`].
pub const MAX_CONTEXT_INPUT_TOKENS: usize = 2_000_000;

/// Default number of stored messages replayed per session.
pub const DEFAULT_MEMORY_HISTORY_LIMIT: usize = 200;
/// Maximum accepted value for [`MemoryConfig::history_limit`].
pub const MAX_MEMORY_HISTORY_LIMIT: usize = 10_000;

/// Default lifetime of a cache entry.
pub const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(300);
/// Default upper bound on stored cache entries.
pub const DEFAULT_CACHE_MAX_ENTRIES: usize = 4096;
/// Maximum accepted value for [`CacheConfig::max_entries`].
pub const MAX_CACHE_MAX_ENTRIES: usize = 1_000_000;

/// Default number of model rounds in one tool loop.
pub const DEFAULT_TOOL_MAX_ROUNDS: u32 = 4;
/// Maximum accepted value for [`ToolConfig::max_rounds`].
pub const MAX_TOOL_ROUNDS: u32 = 32;
/// Default deadline of one tool call.
pub const DEFAULT_TOOL_CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Validates a single timeout setting.
fn validate_timeout(field: &str, value: Duration) -> CoreResult<()> {
    if value.is_zero() {
        return Err(CoreError::config_field(field, "must be greater than zero"));
    }
    if value > MAX_TIMEOUT {
        return Err(CoreError::config_field(
            field,
            format!("must not exceed {MAX_TIMEOUT:?}"),
        ));
    }
    Ok(())
}

/// Validates a bounded parallelism setting.
fn validate_parallelism(field: &str, value: usize, max: usize) -> CoreResult<()> {
    if value == 0 {
        return Err(CoreError::config_field(field, "must be greater than zero"));
    }
    if value > max {
        return Err(CoreError::config_field(
            field,
            format!("must not exceed {max}"),
        ));
    }
    Ok(())
}

/// Timeout budget of the framework.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TimeoutsConfig {
    /// Deadline for one complete AI request (including retries).
    pub request: Duration,
    /// Deadline for establishing a connection to an upstream provider.
    pub connect: Duration,
    /// Maximum silence tolerated while a stream is open.
    pub streaming_idle: Duration,
    /// Grace period for background tasks after shutdown was requested.
    pub shutdown_grace: Duration,
}

impl Default for TimeoutsConfig {
    fn default() -> Self {
        Self {
            request: DEFAULT_REQUEST_TIMEOUT,
            connect: DEFAULT_CONNECT_TIMEOUT,
            streaming_idle: DEFAULT_STREAMING_IDLE_TIMEOUT,
            shutdown_grace: DEFAULT_SHUTDOWN_GRACE,
        }
    }
}

impl TimeoutsConfig {
    /// Validates every timeout.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] when a value is zero or exceeds
    /// [`MAX_TIMEOUT`].
    pub fn validate(&self) -> CoreResult<()> {
        validate_timeout("timeouts.request", self.request)?;
        validate_timeout("timeouts.connect", self.connect)?;
        validate_timeout("timeouts.streaming_idle", self.streaming_idle)?;
        validate_timeout("timeouts.shutdown_grace", self.shutdown_grace)
    }
}

/// Parallelism limits used by the scheduler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConcurrencyConfig {
    /// Maximum number of AI requests executing at the same time.
    pub max_concurrent_requests: usize,
    /// Maximum number of blocking jobs dispatched to `spawn_blocking`.
    pub max_blocking_tasks: usize,
}

impl Default for ConcurrencyConfig {
    fn default() -> Self {
        Self {
            max_concurrent_requests: 4,
            max_blocking_tasks: 8,
        }
    }
}

impl ConcurrencyConfig {
    /// Validates the parallelism limits.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] when a limit is zero or above the documented
    /// maximum.
    pub fn validate(&self) -> CoreResult<()> {
        validate_parallelism(
            "concurrency.max_concurrent_requests",
            self.max_concurrent_requests,
            MAX_CONCURRENT_REQUESTS,
        )?;
        validate_parallelism(
            "concurrency.max_blocking_tasks",
            self.max_blocking_tasks,
            MAX_BLOCKING_TASKS,
        )
    }
}

/// Usage accounting settings.
///
/// Token counts and network traffic are recorded by the provider router into the
/// AI-owned metadata database; this section only controls the write pipeline,
/// never what the numbers mean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UsageConfig {
    /// Whether the router reports finished attempts to the accounting store.
    ///
    /// Disabling this only stops *reporting*; requests keep working unchanged.
    pub enabled: bool,
    /// Capacity of the in-memory queue between requests and the writer task.
    ///
    /// A full queue drops the record and increments a counter instead of slowing
    /// down the request path.
    pub buffer_capacity: usize,
    /// Maximum number of records written in one transaction.
    pub batch_size: usize,
    /// Maximum time a record may wait in the buffer before being flushed.
    pub flush_interval: Duration,
}

impl Default for UsageConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            buffer_capacity: DEFAULT_USAGE_BUFFER_CAPACITY,
            batch_size: DEFAULT_USAGE_BATCH_SIZE,
            flush_interval: DEFAULT_USAGE_FLUSH_INTERVAL,
        }
    }
}

impl UsageConfig {
    /// Validates the buffer, batch and flush settings.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] when a limit is out of range.
    pub fn validate(&self) -> CoreResult<()> {
        if self.buffer_capacity == 0 || self.buffer_capacity > MAX_USAGE_BUFFER_CAPACITY {
            return Err(CoreError::config_field(
                "usage.buffer_capacity",
                format!("must be between 1 and {MAX_USAGE_BUFFER_CAPACITY}"),
            ));
        }
        if self.batch_size == 0 || self.batch_size > MAX_USAGE_BATCH_SIZE {
            return Err(CoreError::config_field(
                "usage.batch_size",
                format!("must be between 1 and {MAX_USAGE_BATCH_SIZE}"),
            ));
        }
        validate_timeout("usage.flush_interval", self.flush_interval)
    }
}

/// Context assembly budget.
///
/// The assembler never sends more than `max_input_tokens - reserve_output_tokens`
/// estimated tokens of history, and always keeps the system instructions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContextConfig {
    /// Estimated input-token ceiling for one assembled context.
    pub max_input_tokens: usize,
    /// Tokens reserved for the model's own output inside that ceiling.
    pub reserve_output_tokens: usize,
    /// Whether dropped turns are replaced by a one-line omission marker.
    pub emit_omission_marker: bool,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_input_tokens: DEFAULT_CONTEXT_MAX_INPUT_TOKENS,
            reserve_output_tokens: DEFAULT_CONTEXT_RESERVE_TOKENS,
            emit_omission_marker: true,
        }
    }
}

impl ContextConfig {
    /// Validates the budget.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] when the ceiling is zero or above
    /// [`MAX_CONTEXT_INPUT_TOKENS`], or when the reserve does not leave room for
    /// at least one input token.
    pub fn validate(&self) -> CoreResult<()> {
        if self.max_input_tokens == 0 || self.max_input_tokens > MAX_CONTEXT_INPUT_TOKENS {
            return Err(CoreError::config_field(
                "context.max_input_tokens",
                format!("must be between 1 and {MAX_CONTEXT_INPUT_TOKENS}"),
            ));
        }
        if self.reserve_output_tokens >= self.max_input_tokens {
            return Err(CoreError::config_field(
                "context.reserve_output_tokens",
                "must leave at least one input token of the context budget",
            ));
        }
        Ok(())
    }
}

/// AI-owned conversation memory settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MemoryConfig {
    /// Whether the framework replays memory into context by default.
    ///
    /// Disabling this never deletes data: it only stops automatic replay.
    pub enabled: bool,
    /// Maximum number of stored messages loaded per session.
    pub history_limit: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            history_limit: DEFAULT_MEMORY_HISTORY_LIMIT,
        }
    }
}

impl MemoryConfig {
    /// Validates the replay limit.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] when the limit is zero or above
    /// [`MAX_MEMORY_HISTORY_LIMIT`].
    pub fn validate(&self) -> CoreResult<()> {
        if self.history_limit == 0 || self.history_limit > MAX_MEMORY_HISTORY_LIMIT {
            return Err(CoreError::config_field(
                "memory.history_limit",
                format!("must be between 1 and {MAX_MEMORY_HISTORY_LIMIT}"),
            ));
        }
        Ok(())
    }
}

/// Disposable response/embedding cache settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CacheConfig {
    /// Whether lookups are attempted at all.
    ///
    /// Disabling the cache keeps the database intact; it only bypasses it.
    pub enabled: bool,
    /// Lifetime applied to entries written without an explicit TTL.
    pub default_ttl: Duration,
    /// Upper bound on stored entries; the oldest are evicted first.
    pub max_entries: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_ttl: DEFAULT_CACHE_TTL,
            max_entries: DEFAULT_CACHE_MAX_ENTRIES,
        }
    }
}

impl CacheConfig {
    /// Validates the TTL and entry bound.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] when the TTL is zero or above
    /// [`MAX_TIMEOUT`], or when `max_entries` is zero or above
    /// [`MAX_CACHE_MAX_ENTRIES`].
    pub fn validate(&self) -> CoreResult<()> {
        validate_timeout("cache.default_ttl", self.default_ttl)?;
        if self.max_entries == 0 || self.max_entries > MAX_CACHE_MAX_ENTRIES {
            return Err(CoreError::config_field(
                "cache.max_entries",
                format!("must be between 1 and {MAX_CACHE_MAX_ENTRIES}"),
            ));
        }
        Ok(())
    }
}

/// Tool-loop settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ToolConfig {
    /// Maximum number of model rounds in one tool loop (1 = no tool round).
    pub max_rounds: u32,
    /// Deadline of one tool call.
    pub call_timeout: Duration,
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            max_rounds: DEFAULT_TOOL_MAX_ROUNDS,
            call_timeout: DEFAULT_TOOL_CALL_TIMEOUT,
        }
    }
}

impl ToolConfig {
    /// Validates the round limit and the call deadline.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] when `max_rounds` is zero or above
    /// [`MAX_TOOL_ROUNDS`], or when the deadline is out of range.
    pub fn validate(&self) -> CoreResult<()> {
        if self.max_rounds == 0 || self.max_rounds > MAX_TOOL_ROUNDS {
            return Err(CoreError::config_field(
                "tools.max_rounds",
                format!("must be between 1 and {MAX_TOOL_ROUNDS}"),
            ));
        }
        validate_timeout("tools.call_timeout", self.call_timeout)
    }
}

/// Location of the AI-owned storage areas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StorageConfig {
    /// Root directory; every AI area is created below it.
    pub root: PathBuf,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            root: PathBuf::from("ai-core-data"),
        }
    }
}

impl StorageConfig {
    /// Validates the root path and derives the isolated layout.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] when the root is empty or contains `.`/`..`
    /// components.
    pub fn layout(&self) -> CoreResult<StorageLayout> {
        StorageLayout::derive(self.root.clone())
    }
}

/// Verbosity of the built-in log filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Only failures.
    Error,
    /// Failures and recoverable degradation.
    Warn,
    /// Default operational verbosity.
    #[default]
    Info,
    /// Development diagnostics (never enabled in production by default).
    Debug,
    /// Full tracing output.
    Trace,
}

impl LogLevel {
    /// Lowercase label, usable directly in an `EnvFilter` directive.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for LogLevel {
    type Err = CoreError;

    fn from_str(value: &str) -> CoreResult<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "error" => Ok(Self::Error),
            "warn" | "warning" => Ok(Self::Warn),
            "info" => Ok(Self::Info),
            "debug" => Ok(Self::Debug),
            "trace" => Ok(Self::Trace),
            other => Err(CoreError::invalid_argument(
                "log_level",
                format!("unknown level `{other}`"),
            )),
        }
    }
}

/// Observability-related settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ObservabilityConfig {
    /// Logical service name attached to spans and events.
    pub service_name: String,
    /// Default log filter level.
    pub level: LogLevel,
    /// Redaction switch.
    ///
    /// Reserved for forward compatibility: Phase 1 refuses to start with
    /// redaction disabled, see [`Self::validate`].
    pub redact_sensitive: bool,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            service_name: "sage-ai-core".to_owned(),
            level: LogLevel::Info,
            redact_sensitive: true,
        }
    }
}

impl ObservabilityConfig {
    /// Validates the service name and enforces the redaction policy.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] when the service name is empty, too long or
    /// contains unsupported characters, or when redaction was disabled.
    pub fn validate(&self) -> CoreResult<()> {
        let name = self.service_name.trim();
        if name.is_empty() {
            return Err(CoreError::config_field(
                "observability.service_name",
                "must not be empty",
            ));
        }
        if name.len() > 64 {
            return Err(CoreError::config_field(
                "observability.service_name",
                "must not exceed 64 characters",
            ));
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        {
            return Err(CoreError::config_field(
                "observability.service_name",
                "must contain only ASCII alphanumerics, `-`, `_` or `.`",
            ));
        }
        if !self.redact_sensitive {
            return Err(CoreError::config_field(
                "observability.redact_sensitive",
                "disabling redaction is not supported",
            ));
        }
        Ok(())
    }
}

/// Root configuration of the AI Core framework.
///
/// Construct it directly, or start from [`Default`] and adjust fields, then call
/// [`Self::validate`] (or hand it to [`ConfigStore::new`], which validates).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct AiCoreConfig {
    /// Timeout budget.
    pub timeouts: TimeoutsConfig,
    /// Parallelism limits.
    pub concurrency: ConcurrencyConfig,
    /// Storage boundaries.
    pub storage: StorageConfig,
    /// Observability policy.
    pub observability: ObservabilityConfig,
    /// AI providers keyed by [`ProviderId`].
    ///
    /// Phase 2 addition. The map travels inside the immutable
    /// [`ConfigSnapshot`], which is what gives "in-flight requests keep the
    /// provider set they started with" for free: a provider is never mutated in
    /// place, a new snapshot is published instead.
    pub providers: BTreeMap<ProviderId, ProviderConfig>,
    /// Usage accounting pipeline settings.
    ///
    /// Phase 3 addition. The database itself always lives under
    /// `ai-metadata/`, next to — never inside — the client business database.
    pub usage: UsageConfig,
    /// Context assembly budget.
    ///
    /// Phase 6 addition.
    pub context: ContextConfig,
    /// Long-lived AI conversation memory.
    ///
    /// Phase 6 addition. The database always lives under `ai-memory/`.
    pub memory: MemoryConfig,
    /// Disposable response/embedding cache.
    ///
    /// Phase 7 addition. The database always lives under `ai-cache/`.
    pub cache: CacheConfig,
    /// Tool-loop limits.
    ///
    /// Phase 8 addition.
    pub tools: ToolConfig,
}

impl AiCoreConfig {
    /// Validates every section.
    ///
    /// # Errors
    ///
    /// Returns the first [`CoreError::Config`] produced by the section
    /// validators.
    pub fn validate(&self) -> CoreResult<()> {
        self.timeouts.validate()?;
        self.concurrency.validate()?;
        self.observability.validate()?;
        self.storage.layout()?;
        self.usage.validate()?;
        self.context.validate()?;
        self.memory.validate()?;
        self.cache.validate()?;
        self.tools.validate()?;
        self.validate_providers()
    }

    /// Validates the provider section, including cross-provider references.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] when a provider is invalid, when its declared
    /// id does not match its registry key, or when a fallback target does not
    /// resolve to a declared model.
    pub fn validate_providers(&self) -> CoreResult<()> {
        for (id, provider) in &self.providers {
            if &provider.id != id {
                return Err(CoreError::config_field(
                    format!("providers.{id}.id"),
                    format!("`{}` does not match its registry key", provider.id),
                ));
            }
            provider.validate()?;

            for (index, model) in provider.models.iter().enumerate() {
                for (fallback_index, fallback) in model.fallbacks.iter().enumerate() {
                    let spec = ModelSpec::from_str(fallback).map_err(|error| {
                        CoreError::config_field(
                            format!("providers.{id}.models[{index}].fallbacks[{fallback_index}]"),
                            error.to_string(),
                        )
                    })?;
                    let target = self.providers.get(spec.provider()).ok_or_else(|| {
                        CoreError::config_field(
                            format!("providers.{id}.models[{index}].fallbacks[{fallback_index}]"),
                            format!("unknown provider `{}`", spec.provider()),
                        )
                    })?;
                    if target.model(spec.model()).is_none() {
                        return Err(CoreError::config_field(
                            format!("providers.{id}.models[{index}].fallbacks[{fallback_index}]"),
                            format!(
                                "provider `{}` has no model `{}`",
                                spec.provider(),
                                spec.model()
                            ),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Looks up a configured provider.
    #[must_use]
    pub fn provider(&self, id: &ProviderId) -> Option<&ProviderConfig> {
        self.providers.get(id)
    }

    /// Providers that are enabled for routing, in registry (alphabetical) order.
    pub fn enabled_providers(&self) -> impl Iterator<Item = &ProviderConfig> {
        self.providers.values().filter(|provider| provider.enabled)
    }

    /// Validates and returns `self`, for use in builder chains.
    ///
    /// # Errors
    ///
    /// See [`Self::validate`].
    pub fn validated(self) -> CoreResult<Self> {
        self.validate()?;
        Ok(self)
    }

    /// Derives the validated storage layout.
    ///
    /// # Errors
    ///
    /// See [`StorageConfig::layout`].
    pub fn storage_layout(&self) -> CoreResult<StorageLayout> {
        self.storage.layout()
    }
}

/// Monotonically increasing configuration version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConfigVersion(u64);

impl ConfigVersion {
    /// Version assigned to the first snapshot.
    pub const INITIAL: Self = Self(1);

    /// Wraps a raw version number.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the raw version number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for ConfigVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

impl FromStr for ConfigVersion {
    type Err = CoreError;

    fn from_str(value: &str) -> CoreResult<Self> {
        let digits = value.trim().trim_start_matches('v');
        digits
            .parse::<u64>()
            .map(Self)
            .map_err(|_| CoreError::invalid_argument("config_version", "expected `v<u64>`"))
    }
}

/// Immutable, reference-counted configuration snapshot.
///
/// Cloning is a single atomic increment, so the snapshot can be handed to every
/// in-flight request without copying the configuration.
#[derive(Debug, Clone)]
pub struct ConfigSnapshot {
    version: ConfigVersion,
    created_at: SystemTime,
    config: Arc<AiCoreConfig>,
}

impl ConfigSnapshot {
    fn new(version: ConfigVersion, config: AiCoreConfig) -> Self {
        Self {
            version,
            created_at: SystemTime::now(),
            config: Arc::new(config),
        }
    }

    /// Version of this snapshot.
    #[must_use]
    pub const fn version(&self) -> ConfigVersion {
        self.version
    }

    /// Creation timestamp, used to detect stale snapshots.
    #[must_use]
    pub const fn created_at(&self) -> SystemTime {
        self.created_at
    }

    /// Borrowed view of the configuration.
    #[must_use]
    pub fn config(&self) -> &AiCoreConfig {
        &self.config
    }

    /// Shared handle to the configuration, for passing into long-lived tasks.
    #[must_use]
    pub fn shared_config(&self) -> Arc<AiCoreConfig> {
        Arc::clone(&self.config)
    }

    /// Time elapsed since the snapshot was created (saturating at zero).
    #[must_use]
    pub fn age(&self) -> Duration {
        SystemTime::now()
            .duration_since(self.created_at)
            .unwrap_or(Duration::ZERO)
    }
}

/// Holds the current configuration snapshot and hands out read-only views.
///
/// # Concurrency
///
/// * Readers take a short `RwLock` read lock and clone one `Arc`.
/// * Writers validate **before** acquiring the write lock, so a rejected update
///   never blocks readers and never modifies state.
/// * Killing a reader task while it holds the guard is impossible: the guard is
///   never held across an `.await`.
#[derive(Debug)]
pub struct ConfigStore {
    current: RwLock<Arc<ConfigSnapshot>>,
    next_version: AtomicU64,
}

impl ConfigStore {
    /// Creates a store with an initial validated configuration.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] if `config` fails validation.
    pub fn new(config: AiCoreConfig) -> CoreResult<Self> {
        config.validate()?;
        Ok(Self {
            current: RwLock::new(Arc::new(ConfigSnapshot::new(
                ConfigVersion::INITIAL,
                config,
            ))),
            next_version: AtomicU64::new(ConfigVersion::INITIAL.get() + 1),
        })
    }

    /// Clones the current snapshot handle.
    #[must_use]
    pub fn snapshot(&self) -> Arc<ConfigSnapshot> {
        let guard = match self.current.read() {
            Ok(guard) => guard,
            // A poisoned lock only means another thread panicked while swapping
            // the pointer; the `Arc` itself is still valid, so recover instead of
            // propagating a panic.
            Err(poisoned) => poisoned.into_inner(),
        };
        Arc::clone(&guard)
    }

    /// Returns the underlying configuration of the current snapshot.
    #[must_use]
    pub fn config(&self) -> Arc<AiCoreConfig> {
        self.snapshot().shared_config()
    }

    /// Version of the current snapshot.
    #[must_use]
    pub fn version(&self) -> ConfigVersion {
        self.snapshot().version()
    }

    /// Validates `config` and publishes it as a new snapshot.
    ///
    /// Existing holders of older snapshots are unaffected.
    ///
    /// # Errors
    ///
    /// * [`CoreError::Config`] when `config` fails validation; the store keeps
    ///   serving the previous snapshot.
    /// * [`CoreError::Internal`] when the version counter is exhausted.
    pub fn apply(&self, config: AiCoreConfig) -> CoreResult<Arc<ConfigSnapshot>> {
        // Validate before taking the write lock: rejection must never block
        // readers and must never consume a version number.
        config.validate()?;

        let mut guard = match self.current.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        // Assign the version while holding the lock so that publication order and
        // version order can never diverge, even with concurrent writers.
        let previous = self.next_version.fetch_add(1, Ordering::SeqCst);
        if previous == u64::MAX {
            // Unreachable in practice (2^64 snapshots); reported explicitly
            // instead of silently wrapping to zero.
            return Err(CoreError::internal("configuration version space exhausted"));
        }
        let version = ConfigVersion::new(previous);

        let snapshot = Arc::new(ConfigSnapshot::new(version, config));
        *guard = Arc::clone(&snapshot);
        drop(guard);
        Ok(snapshot)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        let config = AiCoreConfig::default();
        config.validate().unwrap();
        assert_eq!(config.timeouts.request, DEFAULT_REQUEST_TIMEOUT);
        assert_eq!(config.observability.level, LogLevel::Info);
        assert!(config.observability.redact_sensitive);
    }

    #[test]
    fn zero_timeout_is_rejected() {
        let mut config = AiCoreConfig::default();
        config.timeouts.request = Duration::ZERO;
        let error = config.validate().unwrap_err();
        assert_eq!(error.kind(), crate::ErrorKind::Config);
        assert!(error.to_string().contains("timeouts.request"));
    }

    #[test]
    fn excessive_timeout_is_rejected() {
        let mut config = AiCoreConfig::default();
        config.timeouts.connect = MAX_TIMEOUT + Duration::from_secs(1);
        assert!(config.validate().is_err());
    }

    #[test]
    fn parallelism_bounds_are_enforced() {
        let mut config = AiCoreConfig::default();
        config.concurrency.max_concurrent_requests = 0;
        assert!(config.validate().is_err());

        config.concurrency.max_concurrent_requests = 4;
        config.concurrency.max_blocking_tasks = MAX_BLOCKING_TASKS + 1;
        assert!(config.validate().is_err());
    }

    #[test]
    fn invalid_service_names_are_rejected() {
        for name in ["", "   ", "has space", "slashes/bad"] {
            let mut config = AiCoreConfig::default();
            config.observability.service_name = name.to_owned();
            assert!(config.validate().is_err(), "`{name}` should be rejected");
        }
    }

    #[test]
    fn disabling_redaction_is_rejected() {
        let mut config = AiCoreConfig::default();
        config.observability.redact_sensitive = false;
        assert!(config.validate().is_err());
    }

    #[test]
    fn store_versions_are_monotonic_and_snapshots_are_stable() {
        let store = ConfigStore::new(AiCoreConfig::default()).unwrap();
        let first = store.snapshot();
        assert_eq!(first.version(), ConfigVersion::INITIAL);

        let mut updated = AiCoreConfig::default();
        updated.timeouts.request = Duration::from_secs(5);
        let second = store.apply(updated).unwrap();

        assert_eq!(second.version(), ConfigVersion::new(2));
        assert_eq!(first.version(), ConfigVersion::INITIAL);
        assert_eq!(first.config().timeouts.request, DEFAULT_REQUEST_TIMEOUT);
        assert_eq!(second.config().timeouts.request, Duration::from_secs(5));
        assert_eq!(store.version(), ConfigVersion::new(2));
    }

    #[test]
    fn rejected_updates_do_not_change_the_store() {
        let store = ConfigStore::new(AiCoreConfig::default()).unwrap();
        let mut invalid = AiCoreConfig::default();
        invalid.timeouts.request = Duration::ZERO;
        assert!(store.apply(invalid).is_err());
        assert_eq!(store.version(), ConfigVersion::INITIAL);
    }

    #[test]
    fn context_budget_must_leave_input_room() {
        let mut config = AiCoreConfig::default();
        config.context.max_input_tokens = 0;
        assert!(config.validate().is_err());

        config.context.max_input_tokens = 100;
        config.context.reserve_output_tokens = 100;
        assert!(config.validate().is_err());

        config.context.reserve_output_tokens = 0;
        config.validate().unwrap();
    }

    #[test]
    fn memory_and_cache_limits_are_bounded() {
        let mut config = AiCoreConfig::default();
        config.memory.history_limit = 0;
        assert!(config.validate().is_err());
        config.memory.history_limit = MAX_MEMORY_HISTORY_LIMIT + 1;
        assert!(config.validate().is_err());
        config.memory.history_limit = DEFAULT_MEMORY_HISTORY_LIMIT;

        config.cache.default_ttl = Duration::ZERO;
        assert!(config.validate().is_err());
        config.cache.default_ttl = DEFAULT_CACHE_TTL;
        config.cache.max_entries = 0;
        assert!(config.validate().is_err());
        config.cache.max_entries = DEFAULT_CACHE_MAX_ENTRIES;
        config.validate().unwrap();
    }

    #[test]
    fn tool_loop_limits_are_bounded() {
        let mut config = AiCoreConfig::default();
        config.tools.max_rounds = 0;
        assert!(config.validate().is_err());
        config.tools.max_rounds = MAX_TOOL_ROUNDS + 1;
        assert!(config.validate().is_err());
        config.tools.max_rounds = 1;
        config.tools.call_timeout = MAX_TIMEOUT + Duration::from_secs(1);
        assert!(config.validate().is_err());
        config.tools.call_timeout = DEFAULT_TOOL_CALL_TIMEOUT;
        config.validate().unwrap();
    }

    #[test]
    fn config_serde_round_trip() {
        let config = AiCoreConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let restored: AiCoreConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, restored);
    }

    #[test]
    fn config_version_parsing() {
        assert_eq!(
            ConfigVersion::from_str("v7").unwrap(),
            ConfigVersion::new(7)
        );
        assert_eq!(ConfigVersion::from_str("7").unwrap(), ConfigVersion::new(7));
        assert!(ConfigVersion::from_str("vX").is_err());
        assert_eq!(ConfigVersion::new(3).to_string(), "v3");
    }
}
