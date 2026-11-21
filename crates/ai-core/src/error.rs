//! Unified error model for the AI Core framework.
//!
//! Every fallible public API of this crate returns [`CoreResult<T>`], i.e. a
//! `Result<T, CoreError>`. [`CoreError`] serves three purposes:
//!
//! 1. **Stable classification** — [`CoreError::kind`] maps every variant onto an
//!    [`ErrorKind`], which is also exposed through [`CoreError::error_code`] as a
//!    stable machine-readable string (`CORE_TIMEOUT`, ...). Host applications can
//!    branch on the kind without matching on unstable variant payloads.
//! 2. **Recoverability** — [`CoreError::is_retryable`] answers whether the failed
//!    operation may be attempted again (timeouts, transient I/O, provider-side
//!    throttling) or whether it is a permanent failure (validation, auth, ...).
//! 3. **Root-cause preservation** — underlying errors are kept as
//!    [`std::error::Error::source`] instead of being flattened into a string, so
//!    callers can walk the chain while the `Display` output stays free of secrets.
//!
//! # Secret hygiene
//!
//! This type never formats a credential on its own. Variants carry short,
//! operational messages that are expected to be built from already-redacted
//! values (see [`crate::secret`]). Root causes come from third-party crates and
//! must not be trusted, so:
//!
//! * `Display` never interpolates a root cause, except for [`CoreError::Io`],
//!   whose message is sanitized once, when the conversion happens.
//! * `Debug` renders the root cause through [`crate::secret::sanitize_log_text`],
//!   which masks bearer tokens, provider-style keys (`sk-...`) and sensitive
//!   `key=value` pairs.
//! * The untouched error object stays reachable through
//!   [`std::error::Error::source`] for local debugging.
//!
//! Phase 1 defines the variants required by the foundation layer and *reserves*
//! the ones needed later (provider, auth, capability, token budget, tool, storage)
//! so that Phase 2 can extend behaviour without breaking the public contract.

use std::error::Error as StdError;
use std::fmt;
use std::time::Duration;

/// Convenience alias used by every fallible API of this crate.
pub type CoreResult<T> = std::result::Result<T, CoreError>;

/// Boxed, thread-safe, `'static` error used to retain a root cause without
/// committing to a concrete source type.
pub type BoxError = Box<dyn StdError + Send + Sync + 'static>;

/// Broad, stable classification of a [`CoreError`].
///
/// The enum is `#[non_exhaustive]`: future phases may add kinds, therefore
/// consumers must always keep a wildcard arm when matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Invalid configuration (loading, parsing, semantic validation).
    Config,
    /// A caller-supplied argument violated the documented contract.
    InvalidArgument,
    /// The operation did not finish within its deadline.
    Timeout,
    /// The operation was cancelled through a cancellation signal.
    Cancelled,
    /// Filesystem or other I/O failure.
    Io,
    /// Serialization or deserialization failure.
    Serialization,
    /// A requested resource does not exist.
    NotFound,
    /// Concurrency-related failure (task join failure, poisoned state, ...).
    Concurrency,
    /// Internal invariant violation; indicates a bug rather than bad input.
    Internal,
    /// The runtime is shutting down and refuses new work.
    Shutdown,
    /// Upstream AI provider failure.
    Provider,
    /// Authentication/authorization failure.
    Authentication,
    /// The provider/model does not support the required capability.
    CapabilityUnsupported,
    /// Token/context budget exceeded.
    TokenLimitExceeded,
    /// Tool invocation failure.
    Tool,
    /// AI memory/metadata/cache storage failure.
    Storage,
}

impl ErrorKind {
    /// Stable, lowercase, machine friendly label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::InvalidArgument => "invalid_argument",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::Io => "io",
            Self::Serialization => "serialization",
            Self::NotFound => "not_found",
            Self::Concurrency => "concurrency",
            Self::Internal => "internal",
            Self::Shutdown => "shutdown",
            Self::Provider => "provider",
            Self::Authentication => "authentication",
            Self::CapabilityUnsupported => "capability_unsupported",
            Self::TokenLimitExceeded => "token_limit_exceeded",
            Self::Tool => "tool",
            Self::Storage => "storage",
        }
    }

    /// Default recoverability for this kind.
    ///
    /// Variants that carry a per-instance decision (such as
    /// [`CoreError::Provider`]) override this in [`CoreError::is_retryable`].
    #[must_use]
    pub const fn default_retryable(self) -> bool {
        matches!(
            self,
            Self::Timeout | Self::Io | Self::Concurrency | Self::Provider
        )
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Logical storage area.
///
/// Declared next to [`CoreError::Storage`] so that the error layer stays a leaf
/// dependency; [`crate::storage`] re-exports it for path/layout APIs.
///
/// The three areas are deliberately isolated: AI memory (long-lived, must never
/// share the client business database), AI metadata (indexes/statistics) and
/// cache (disposable) have different lifetimes and backup/privacy requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StorageArea {
    /// Long-lived AI memory store (future vector/FTS index).
    Memory,
    /// AI-owned metadata (usage accounting, bookkeeping).
    Metadata,
    /// Disposable cache; may be purged at any time.
    Cache,
}

impl StorageArea {
    /// Stable directory name used below the configured storage root.
    #[must_use]
    pub const fn dir_name(self) -> &'static str {
        match self {
            Self::Memory => "ai-memory",
            Self::Metadata => "ai-metadata",
            Self::Cache => "ai-cache",
        }
    }
}

impl fmt::Display for StorageArea {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.dir_name())
    }
}

/// The unified error type of the AI Core framework.
///
/// The enum is `#[non_exhaustive]` so additional variants (provider, tool,
/// memory, ...) can be introduced in later phases without a breaking change.
///
/// # `Debug` and log safety
///
/// `Debug` is implemented manually: it prints the stable error code, the
/// human-readable message and a **sanitized** rendering of the root cause.
/// Third-party error strings are not trusted (they may embed a URL with a token,
/// an `Authorization` header or an API key), so they pass through
/// [`crate::secret::sanitize_log_text`] before being formatted. Structured access
/// to the untouched chain remains available through
/// [`std::error::Error::source`].
#[derive(thiserror::Error)]
#[non_exhaustive]
pub enum CoreError {
    /// Invalid configuration, or a configuration value rejected by validation.
    #[error("configuration error: {message}")]
    Config {
        /// Human readable, secret-free description (usually `field: reason`).
        message: String,
        /// Optional root cause retained for diagnostics.
        #[source]
        source: Option<BoxError>,
    },

    /// A caller-supplied argument is outside the documented contract.
    #[error("invalid argument `{field}`: {message}")]
    InvalidArgument {
        /// Name of the offending argument or configuration field.
        field: String,
        /// Why the value was rejected.
        message: String,
    },

    /// The operation exceeded its deadline.
    #[error("operation `{operation}` timed out after {timeout_ms} ms")]
    Timeout {
        /// Static operation label, safe to log.
        operation: &'static str,
        /// Configured deadline in milliseconds.
        timeout_ms: u64,
    },

    /// The operation was cancelled before completing.
    #[error("operation `{operation}` was cancelled")]
    Cancelled {
        /// Static operation label, safe to log.
        operation: &'static str,
    },

    /// I/O failure (filesystem, pipes, ...).
    ///
    /// The rendered message is a *sanitized* copy of the operating-system text,
    /// produced once when the error is converted; the untouched error stays
    /// available through [`std::error::Error::source`].
    #[error("i/o error: {message}")]
    Io {
        /// Sanitized description of the underlying failure.
        message: String,
        /// Underlying I/O error; retained as the error source.
        #[source]
        source: std::io::Error,
    },

    /// Serialization or deserialization failure.
    #[error("serialization error: {message}")]
    Serialization {
        /// Secret-free description of what failed.
        message: String,
        /// Optional root cause (e.g. a `serde_json` error).
        #[source]
        source: Option<BoxError>,
    },

    /// A requested resource does not exist.
    #[error("not found: {resource}")]
    NotFound {
        /// Identifier of the missing resource (never a credential).
        resource: String,
    },

    /// Failure caused by concurrent execution: task join failure, poisoned
    /// lock, losing a compare-and-swap race, ...
    #[error("concurrency error: {message}")]
    Concurrency {
        /// Secret-free description of the failure.
        message: String,
    },

    /// Internal invariant violation, i.e. a bug in the framework or host.
    #[error("internal error: {message}")]
    Internal {
        /// Secret-free description of the failure.
        message: String,
    },

    /// The runtime is shutting down and refuses to accept new work.
    #[error("ai core runtime is shutting down")]
    Shutdown,

    /// Failure reported by an AI provider.
    #[error("provider `{provider}` failed: {message}")]
    Provider {
        /// Provider identifier (`openai`, `deepseek`, ...).
        provider: String,
        /// Secret-free, redacted description.
        message: String,
        /// Whether the same request may be retried.
        retryable: bool,
        /// Optional root cause (transport error, ...).
        #[source]
        source: Option<BoxError>,
    },

    /// Credentials missing, invalid or rejected by the provider.
    #[error("authentication failed: {message}")]
    Authentication {
        /// Secret-free description; must never contain the credential itself.
        message: String,
    },

    /// The requested provider/model does not support a required capability.
    ///
    /// Carries the routing context so the host can point at the exact
    /// configuration entry instead of showing a generic "unsupported" message.
    #[error(
        "capability `{capability}` is not supported by model `{model}` (provider `{provider}`)"
    )]
    CapabilityUnsupported {
        /// Provider identifier.
        provider: String,
        /// Model identifier.
        model: String,
        /// Capability name, e.g. `streaming` or `tool_calling`.
        capability: String,
    },

    /// Token or context budget exceeded (declared limit or provider-reported).
    #[error("token limit exceeded: {message}")]
    TokenLimitExceeded {
        /// Secret-free description including the offending budget if known.
        message: String,
    },

    /// Tool invocation failure.
    #[error("tool `{tool}` failed: {message}")]
    Tool {
        /// Tool name.
        tool: String,
        /// Secret-free description.
        message: String,
    },

    /// Failure of an AI-owned storage area.
    #[error("storage error in `{area}`: {message}")]
    Storage {
        /// Which isolated storage area failed.
        area: StorageArea,
        /// Secret-free description.
        message: String,
        /// Optional root cause.
        #[source]
        source: Option<BoxError>,
    },
}

impl CoreError {
    /// Builds a [`CoreError::Config`] from a secret-free message.
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config {
            message: message.into(),
            source: None,
        }
    }

    /// Builds a [`CoreError::Config`] that additionally retains a root cause.
    pub fn config_with_source(message: impl Into<String>, source: impl Into<BoxError>) -> Self {
        Self::Config {
            message: message.into(),
            source: Some(source.into()),
        }
    }

    /// Builds a configuration error for a specific field.
    ///
    /// The produced message is `"<field>: <reason>"`, which keeps validation
    /// diagnostics actionable without exposing values that may be sensitive.
    pub fn config_field(field: impl Into<String>, reason: impl fmt::Display) -> Self {
        let field = field.into();
        Self::Config {
            message: format!("{field}: {reason}"),
            source: None,
        }
    }

    /// Builds a [`CoreError::InvalidArgument`].
    pub fn invalid_argument(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::InvalidArgument {
            field: field.into(),
            message: message.into(),
        }
    }

    /// Builds a [`CoreError::Timeout`] from an operation label and deadline.
    #[must_use]
    pub fn timeout(operation: &'static str, limit: Duration) -> Self {
        Self::Timeout {
            operation,
            timeout_ms: u64::try_from(limit.as_millis()).unwrap_or(u64::MAX),
        }
    }

    /// Builds a [`CoreError::Cancelled`].
    #[must_use]
    pub fn cancelled(operation: &'static str) -> Self {
        Self::Cancelled { operation }
    }

    /// Builds a [`CoreError::Serialization`] from a secret-free message.
    pub fn serialization(message: impl Into<String>) -> Self {
        Self::Serialization {
            message: message.into(),
            source: None,
        }
    }

    /// Builds a [`CoreError::Serialization`] that retains a root cause.
    pub fn serialization_with_source(
        message: impl Into<String>,
        source: impl Into<BoxError>,
    ) -> Self {
        Self::Serialization {
            message: message.into(),
            source: Some(source.into()),
        }
    }

    /// Builds a [`CoreError::NotFound`].
    pub fn not_found(resource: impl Into<String>) -> Self {
        Self::NotFound {
            resource: resource.into(),
        }
    }

    /// Builds a [`CoreError::Concurrency`].
    pub fn concurrency(message: impl Into<String>) -> Self {
        Self::Concurrency {
            message: message.into(),
        }
    }

    /// Builds a [`CoreError::Internal`].
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    /// Builds a [`CoreError::CapabilityUnsupported`] with routing context.
    pub fn capability_unsupported(
        provider: impl Into<String>,
        model: impl Into<String>,
        capability: impl Into<String>,
    ) -> Self {
        Self::CapabilityUnsupported {
            provider: provider.into(),
            model: model.into(),
            capability: capability.into(),
        }
    }

    /// Builds a [`CoreError::CapabilityUnsupported`] without routing context.
    ///
    /// Prefer [`CoreError::capability_unsupported`]; this constructor exists for
    /// call sites that only know the capability name.
    pub fn unsupported_capability(capability: impl Into<String>) -> Self {
        Self::capability_unsupported("<unspecified>", "<unspecified>", capability)
    }

    /// Builds a [`CoreError::Provider`].
    pub fn provider(
        provider: impl Into<String>,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self::Provider {
            provider: provider.into(),
            message: message.into(),
            retryable,
            source: None,
        }
    }

    /// Builds a [`CoreError::Provider`] that retains the transport root cause.
    pub fn provider_with_source(
        provider: impl Into<String>,
        message: impl Into<String>,
        retryable: bool,
        source: impl Into<BoxError>,
    ) -> Self {
        Self::Provider {
            provider: provider.into(),
            message: message.into(),
            retryable,
            source: Some(source.into()),
        }
    }

    /// Builds a [`CoreError::Authentication`].
    pub fn authentication(message: impl Into<String>) -> Self {
        Self::Authentication {
            message: message.into(),
        }
    }

    /// Builds a [`CoreError::Tool`] describing a tool-level failure.
    pub fn tool(tool: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Tool {
            tool: tool.into(),
            message: message.into(),
        }
    }

    /// Builds a [`CoreError::TokenLimitExceeded`].
    pub fn token_limit(message: impl Into<String>) -> Self {
        Self::TokenLimitExceeded {
            message: message.into(),
        }
    }

    /// Builds a [`CoreError::Storage`] with an optional root cause.
    pub fn storage(
        area: StorageArea,
        message: impl Into<String>,
        source: Option<BoxError>,
    ) -> Self {
        Self::Storage {
            area,
            message: message.into(),
            source,
        }
    }

    /// Classification of this error.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::Config { .. } => ErrorKind::Config,
            Self::InvalidArgument { .. } => ErrorKind::InvalidArgument,
            Self::Timeout { .. } => ErrorKind::Timeout,
            Self::Cancelled { .. } => ErrorKind::Cancelled,
            Self::Io { .. } => ErrorKind::Io,
            Self::Serialization { .. } => ErrorKind::Serialization,
            Self::NotFound { .. } => ErrorKind::NotFound,
            Self::Concurrency { .. } => ErrorKind::Concurrency,
            Self::Internal { .. } => ErrorKind::Internal,
            Self::Shutdown => ErrorKind::Shutdown,
            Self::Provider { .. } => ErrorKind::Provider,
            Self::Authentication { .. } => ErrorKind::Authentication,
            Self::CapabilityUnsupported { .. } => ErrorKind::CapabilityUnsupported,
            Self::TokenLimitExceeded { .. } => ErrorKind::TokenLimitExceeded,
            Self::Tool { .. } => ErrorKind::Tool,
            Self::Storage { .. } => ErrorKind::Storage,
        }
    }

    /// Stable machine-readable error code (`CORE_TIMEOUT`, ...).
    ///
    /// Hosts may surface this to UI/telemetry without depending on the enum
    /// layout. The values are part of the public contract.
    #[must_use]
    pub const fn error_code(&self) -> &'static str {
        match self.kind() {
            ErrorKind::Config => "CORE_CONFIG",
            ErrorKind::InvalidArgument => "CORE_INVALID_ARGUMENT",
            ErrorKind::Timeout => "CORE_TIMEOUT",
            ErrorKind::Cancelled => "CORE_CANCELLED",
            ErrorKind::Io => "CORE_IO",
            ErrorKind::Serialization => "CORE_SERIALIZATION",
            ErrorKind::NotFound => "CORE_NOT_FOUND",
            ErrorKind::Concurrency => "CORE_CONCURRENCY",
            ErrorKind::Internal => "CORE_INTERNAL",
            ErrorKind::Shutdown => "CORE_SHUTDOWN",
            ErrorKind::Provider => "CORE_PROVIDER",
            ErrorKind::Authentication => "CORE_AUTHENTICATION",
            ErrorKind::CapabilityUnsupported => "CORE_CAPABILITY_UNSUPPORTED",
            ErrorKind::TokenLimitExceeded => "CORE_TOKEN_LIMIT",
            ErrorKind::Tool => "CORE_TOOL",
            ErrorKind::Storage => "CORE_STORAGE",
        }
    }

    /// Whether the same operation may be retried.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Io { source, .. } => matches!(
                source.kind(),
                std::io::ErrorKind::Interrupted
                    | std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
            ),
            Self::Provider { retryable, .. } => *retryable,
            other => other.kind().default_retryable(),
        }
    }

    /// Whether this error represents a cancellation.
    #[must_use]
    pub const fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled { .. })
    }

    /// Whether this error represents a timeout.
    #[must_use]
    pub const fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout { .. })
    }
}

impl From<std::io::Error> for CoreError {
    /// Converts an I/O error, sanitizing its text for display.
    ///
    /// The original error object is preserved as the error source, so callers can
    /// still inspect `source().kind()` and the untouched message.
    fn from(source: std::io::Error) -> Self {
        let message = crate::secret::sanitize_log_text(&source.to_string());
        Self::Io { message, source }
    }
}

impl fmt::Debug for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("CoreError");
        debug.field("code", &self.error_code());
        debug.field("message", &format_args!("{self}"));
        if let Some(source) = self.source() {
            debug.field("source", &SanitizedSource(source));
        }
        debug.finish()
    }
}

/// Formats an arbitrary root cause with credential-like substrings masked.
struct SanitizedSource<'a>(&'a dyn StdError);

impl fmt::Debug for SanitizedSource<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&crate::secret::sanitize_log_text(&self.0.to_string()))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_codes_are_stable() {
        let err = CoreError::timeout("test", Duration::from_millis(1500));
        assert_eq!(err.kind(), ErrorKind::Timeout);
        assert_eq!(err.error_code(), "CORE_TIMEOUT");
        assert!(err.is_retryable());
        assert!(err.is_timeout());
        assert!(!err.is_cancelled());
        assert!(err.to_string().contains("1500 ms"));
    }

    #[test]
    fn io_source_is_preserved() {
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err = CoreError::from(io);
        assert_eq!(err.kind(), ErrorKind::Io);
        assert!(!err.is_retryable());
        assert!(err.source().is_some());
    }

    #[test]
    fn config_field_message_contains_field() {
        let err = CoreError::config_field("timeouts.request", "must be > 0");
        assert_eq!(err.kind(), ErrorKind::Config);
        assert!(err.to_string().contains("timeouts.request"));
        assert!(err.to_string().contains("must be > 0"));
    }

    #[test]
    fn storage_errors_are_not_retryable_by_default() {
        let err = CoreError::storage(StorageArea::Cache, "purge failed", None);
        assert_eq!(err.kind(), ErrorKind::Storage);
        assert_eq!(err.error_code(), "CORE_STORAGE");
        assert!(!err.is_retryable());
        assert!(err.to_string().contains("ai-cache"));
    }
}
