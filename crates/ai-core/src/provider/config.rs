//! Configuration of AI providers, their models, retry budgets and capabilities.
//!
//! These types are deliberately **leaf** types: they only depend on
//! [`crate::error`] and [`crate::secret`]. [`crate::config::AiCoreConfig`] embeds
//! them (so the provider configuration travels inside the immutable
//! [`crate::ConfigSnapshot`]), while the provider *runtime* modules
//! (`registry`, `router`, `adapters`) depend on `crate::config`. That keeps the
//! dependency graph acyclic at the leaf-module level.
//!
//! # Secrets
//!
//! [`ProviderConfig::api_key`] and every value of
//! [`ProviderConfig::extra_headers`] are [`SecretString`] values: their `Debug`
//! rendering is `[REDACTED]`, they are **never** serialized back out (so a config
//! dump cannot leak a credential), and the plaintext is only readable through the
//! explicitly named `expose_secret()`.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::secret::SecretString;

/// Maximum accepted length of a provider identifier.
pub const MAX_PROVIDER_ID_LEN: usize = 64;
/// Maximum accepted length of a model identifier.
pub const MAX_MODEL_ID_LEN: usize = 128;
/// Shortest accepted retry backoff.
pub const MIN_BACKOFF: Duration = Duration::from_millis(1);
/// Longest accepted retry backoff between two attempts.
pub const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// Maximum number of attempts per provider, the first try included.
pub const MAX_ATTEMPTS: u32 = 5;
/// Upper bound for a provider-level timeout override (1 hour).
pub const MAX_PROVIDER_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// Strongly typed provider identifier (the registry key and the routing handle).
///
/// Valid identifiers are 1..=64 ASCII characters from `[A-Za-z0-9._-]` and may
/// not start or end with `.`. The value is validated on construction and on
/// deserialization, so an invalid id cannot enter the system silently.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProviderId(String);

impl ProviderId {
    /// Validates and wraps a provider identifier.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] when the value is empty, too long
    /// or contains unsupported characters.
    pub fn new(value: impl Into<String>) -> CoreResult<Self> {
        let value = value.into();
        validate_provider_id(&value)?;
        Ok(Self(value))
    }

    /// Borrowed string form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Parses a provider identifier.
    ///
    /// # Errors
    ///
    /// See [`ProviderId::new`].
    pub fn parse(value: &str) -> CoreResult<Self> {
        Self::new(value)
    }
}

fn validate_provider_id(value: &str) -> CoreResult<()> {
    if value.trim().is_empty() {
        return Err(CoreError::invalid_argument(
            "provider_id",
            "must not be empty",
        ));
    }
    if value.len() > MAX_PROVIDER_ID_LEN {
        return Err(CoreError::invalid_argument(
            "provider_id",
            format!("must not exceed {MAX_PROVIDER_ID_LEN} characters"),
        ));
    }
    if value.starts_with('.') || value.ends_with('.') {
        return Err(CoreError::invalid_argument(
            "provider_id",
            "must not start or end with `.`",
        ));
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(CoreError::invalid_argument(
            "provider_id",
            "must contain only ASCII alphanumerics, `-`, `_` or `.`",
        ));
    }
    Ok(())
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ProviderId {
    type Err = CoreError;

    fn from_str(value: &str) -> CoreResult<Self> {
        Self::new(value)
    }
}

impl AsRef<str> for ProviderId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<ProviderId> for String {
    fn from(value: ProviderId) -> Self {
        value.0
    }
}

impl TryFrom<String> for ProviderId {
    type Error = CoreError;

    fn try_from(value: String) -> CoreResult<Self> {
        Self::new(value)
    }
}

/// Wire protocol spoken by a provider.
///
/// The enum is `#[non_exhaustive]`: a new protocol is added by implementing
/// [`crate::provider::ProviderAdapter`] and extending this enum, without touching
/// the registry or router.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[non_exhaustive]
pub enum ProviderProtocol {
    /// `POST {base_url}/chat/completions` with the OpenAI chat-completions shape.
    ///
    /// Covers OpenAI, DeepSeek and any self-hosted OpenAI-compatible endpoint.
    #[default]
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
    /// `POST {base_url}/messages` with the Anthropic Messages shape.
    #[serde(rename = "anthropic")]
    Anthropic,
}

impl ProviderProtocol {
    /// Stable lowercase label used in logs, errors and configuration.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "openai-compatible",
            Self::Anthropic => "anthropic",
        }
    }

    /// Default API base URL for this protocol.
    #[must_use]
    pub const fn default_base_url(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "https://api.openai.com/v1",
            Self::Anthropic => "https://api.anthropic.com/v1",
        }
    }
}

impl fmt::Display for ProviderProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ProviderProtocol {
    type Err = CoreError;

    fn from_str(value: &str) -> CoreResult<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "openai-compatible" | "openai" | "openai_compatible" => Ok(Self::OpenAiCompatible),
            "anthropic" => Ok(Self::Anthropic),
            other => Err(CoreError::invalid_argument(
                "provider_protocol",
                format!("unsupported protocol `{other}`"),
            )),
        }
    }
}

/// Retry budget of a single provider.
///
/// Backoff is deterministic exponential (the first retry waits `initial_backoff`,
/// then the delay doubles up to `max_backoff`) with no jitter: integration tests
/// and incident timelines stay reproducible. A host that wants jitter can size
/// the policy accordingly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RetryPolicy {
    /// Total number of attempts, including the first one (1 = no retry).
    pub max_attempts: u32,
    /// Backoff before the second attempt.
    pub initial_backoff: Duration,
    /// Upper bound of the exponential backoff.
    pub max_backoff: Duration,
    /// Whether a timeout may be retried (opt-in: it doubles the worst-case
    /// latency, so it is disabled by default).
    pub retry_on_timeout: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 2,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(2),
            retry_on_timeout: false,
        }
    }
}

impl RetryPolicy {
    /// A policy that never retries.
    #[must_use]
    pub fn none() -> Self {
        Self {
            max_attempts: 1,
            ..Self::default()
        }
    }

    /// Validates the retry budget.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] when the attempt count or the backoff window
    /// is out of range.
    pub fn validate(&self, field_prefix: &str) -> CoreResult<()> {
        if self.max_attempts == 0 || self.max_attempts > MAX_ATTEMPTS {
            return Err(CoreError::config_field(
                format!("{field_prefix}.max_attempts"),
                format!("must be between 1 and {MAX_ATTEMPTS}"),
            ));
        }
        if self.initial_backoff < MIN_BACKOFF {
            return Err(CoreError::config_field(
                format!("{field_prefix}.initial_backoff"),
                format!("must be at least {MIN_BACKOFF:?}"),
            ));
        }
        if self.initial_backoff > self.max_backoff {
            return Err(CoreError::config_field(
                format!("{field_prefix}.initial_backoff"),
                "must not exceed `max_backoff`",
            ));
        }
        if self.max_backoff > MAX_BACKOFF {
            return Err(CoreError::config_field(
                format!("{field_prefix}.max_backoff"),
                format!("must not exceed {MAX_BACKOFF:?}"),
            ));
        }
        Ok(())
    }

    /// Backoff to wait before attempt number `attempt` (1-based).
    ///
    /// The first attempt has no wait; the wait before attempt `n >= 2` is
    /// `initial_backoff * 2^(n-2)`, capped at `max_backoff`. Passing `n + 1`
    /// after attempt `n` failed therefore yields the first retry delay
    /// `initial_backoff`.
    #[must_use]
    pub fn backoff_for(&self, attempt: u32) -> Duration {
        if attempt <= 1 {
            return Duration::ZERO;
        }
        let exponent = attempt.saturating_sub(2).min(16);
        let factor = 1_u32 << exponent;
        self.initial_backoff
            .saturating_mul(factor)
            .min(self.max_backoff)
    }
}

/// Declared capabilities and limits of a model.
///
/// Capabilities are part of the *contract*: a request that needs an undeclared
/// capability is rejected with [`CoreError::CapabilityUnsupported`] instead of
/// being sent and failing upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModelCapabilities {
    /// Server-sent streaming output.
    pub streaming: bool,
    /// Function/tool calling.
    pub tool_calling: bool,
    /// JSON mode / JSON-schema constrained output.
    pub structured_output: bool,
    /// Image inputs.
    pub vision: bool,
    /// Embedding models (not used by the Phase 2 chat path; declared so the
    /// registry can answer routing questions).
    pub embedding: bool,
    /// Extended reasoning / thinking output.
    pub reasoning: bool,
    /// Total context window in tokens (input + output).
    pub context_window: u32,
    /// Maximum tokens the model may generate in one response.
    pub max_output_tokens: u32,
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        Self {
            streaming: true,
            tool_calling: false,
            structured_output: false,
            vision: false,
            embedding: false,
            reasoning: false,
            context_window: 8_192,
            max_output_tokens: 2_048,
        }
    }
}

impl ModelCapabilities {
    /// Capabilities of a dedicated embedding model: embeddings only.
    ///
    /// Chat, streaming and tool calling are off because such a model has no chat
    /// endpoint; `context_window` is the largest batch it accepts.
    ///
    /// `max_output_tokens` is `1` rather than `0`: the configuration validator
    /// requires a positive budget, and an embedding model never spends it.
    #[must_use]
    pub const fn embedding_only() -> Self {
        Self {
            streaming: false,
            tool_calling: false,
            structured_output: false,
            vision: false,
            embedding: true,
            reasoning: false,
            context_window: 8_192,
            max_output_tokens: 1,
        }
    }

    /// Validates the numeric limits.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] when a limit is zero, absurdly large or when
    /// `max_output_tokens` exceeds the context window.
    pub fn validate(&self, field_prefix: &str) -> CoreResult<()> {
        if self.context_window == 0 {
            return Err(CoreError::config_field(
                format!("{field_prefix}.context_window"),
                "must be greater than zero",
            ));
        }
        if self.context_window > 10_000_000 {
            return Err(CoreError::config_field(
                format!("{field_prefix}.context_window"),
                "must not exceed 10000000 tokens",
            ));
        }
        if self.max_output_tokens == 0 {
            return Err(CoreError::config_field(
                format!("{field_prefix}.max_output_tokens"),
                "must be greater than zero",
            ));
        }
        if self.max_output_tokens > self.context_window {
            return Err(CoreError::config_field(
                format!("{field_prefix}.max_output_tokens"),
                "must not exceed `context_window`",
            ));
        }
        Ok(())
    }
}

/// One model exposed by a provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModelConfig {
    /// Wire model identifier, e.g. `gpt-4o-mini` or `claude-3-5-sonnet-latest`.
    pub id: String,
    /// Human readable label (defaults to [`ModelConfig::id`] when empty).
    pub display_name: String,
    /// Whether the model may be routed to.
    pub enabled: bool,
    /// Declared capabilities and limits.
    pub capabilities: ModelCapabilities,
    /// Ordered fallback targets (`"provider/model"`) used when this model keeps
    /// failing with a retryable / authentication error.
    pub fallbacks: Vec<String>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            display_name: String::new(),
            enabled: true,
            capabilities: ModelCapabilities::default(),
            fallbacks: Vec::new(),
        }
    }
}

impl ModelConfig {
    /// Builds a model with default capabilities.
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            display_name: id.clone(),
            id,
            ..Self::default()
        }
    }

    /// Replaces the declared capabilities.
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: ModelCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Disables the model for routing.
    #[must_use]
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Appends fallback targets.
    #[must_use]
    pub fn with_fallbacks<I, S>(mut self, fallbacks: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.fallbacks.extend(fallbacks.into_iter().map(Into::into));
        self
    }

    /// Validates the model entry.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] for an invalid id, capabilities or fallback
    /// syntax.
    pub fn validate(&self, field_prefix: &str) -> CoreResult<()> {
        validate_model_id(&self.id)
            .map_err(|reason| CoreError::config_field(format!("{field_prefix}.id"), reason))?;
        self.capabilities
            .validate(&format!("{field_prefix}.capabilities"))?;
        for (index, fallback) in self.fallbacks.iter().enumerate() {
            ModelSpec::from_str(fallback).map_err(|error| {
                CoreError::config_field(
                    format!("{field_prefix}.fallbacks[{index}]"),
                    error.to_string(),
                )
            })?;
        }
        Ok(())
    }
}

fn validate_model_id(value: &str) -> Result<(), &'static str> {
    if value.trim().is_empty() {
        return Err("must not be empty");
    }
    if value.len() > MAX_MODEL_ID_LEN {
        return Err("is too long");
    }
    if value.chars().any(char::is_whitespace) {
        return Err("must not contain whitespace");
    }
    if value.chars().any(char::is_control) {
        return Err("must not contain control characters");
    }
    Ok(())
}

/// Explicit `provider/model` pair used to address a model across providers.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ModelSpec {
    provider: ProviderId,
    model: String,
}

impl ModelSpec {
    /// Builds a spec from a provider id and a model id.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] when the model id is not valid.
    pub fn new(provider: ProviderId, model: impl Into<String>) -> CoreResult<Self> {
        let model = model.into();
        validate_model_id(&model)
            .map_err(|reason| CoreError::invalid_argument("model_id", reason))?;
        Ok(Self { provider, model })
    }

    /// Builds a spec from identifiers that are already validated.
    ///
    /// Used by the registry, which only ever holds validated model ids.
    pub(crate) fn new_unchecked(provider: ProviderId, model: String) -> Self {
        Self { provider, model }
    }

    /// Provider part.
    #[must_use]
    pub fn provider(&self) -> &ProviderId {
        &self.provider
    }

    /// Model part.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }
}

impl fmt::Display for ModelSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.provider, self.model)
    }
}

impl FromStr for ModelSpec {
    type Err = CoreError;

    /// Parses `provider/model`.
    ///
    /// The first `/` separates the provider from the model id, so a model id that
    /// itself contains `/` (e.g. `meta-llama/llama-3-8b`) is supported.
    fn from_str(value: &str) -> CoreResult<Self> {
        let value = value.trim();
        let Some((provider, model)) = value.split_once('/') else {
            return Err(CoreError::invalid_argument(
                "model_spec",
                "expected `provider/model`",
            ));
        };
        Self::new(ProviderId::new(provider)?, model)
    }
}

impl TryFrom<String> for ModelSpec {
    type Error = CoreError;

    fn try_from(value: String) -> CoreResult<Self> {
        Self::from_str(&value)
    }
}

impl From<ModelSpec> for String {
    fn from(value: ModelSpec) -> Self {
        value.to_string()
    }
}

/// Configuration of one AI provider and the models it exposes.
///
/// # Serialization
///
/// `Deserialize` is fully supported (hosts load provider definitions from their
/// own configuration files), but `Serialize` deliberately omits
/// [`ProviderConfig::api_key`] and [`ProviderConfig::extra_headers`]: writing a
/// credential into a config dump or a log snapshot must be impossible. Keys are
/// expected to come from the OS keychain / environment at load time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProviderConfig {
    /// Registry key. Must match the key used in
    /// [`crate::config::AiCoreConfig::providers`].
    pub id: ProviderId,
    /// Human readable name shown in the UI.
    pub display_name: String,
    /// Wire protocol used by the adapter.
    pub protocol: ProviderProtocol,
    /// API base URL, e.g. `https://api.deepseek.com/v1`.
    pub base_url: String,
    /// Credential; `[REDACTED]` in `Debug`, never serialized.
    #[serde(skip_serializing)]
    pub api_key: Option<SecretString>,
    /// Allows an enabled provider to run without a key (local inference servers).
    pub allow_unauthenticated: bool,
    /// Whether the provider may be routed to.
    pub enabled: bool,
    /// Models exposed by this provider.
    pub models: Vec<ModelConfig>,
    /// Default model id; falls back to the first enabled model.
    pub default_model: Option<String>,
    /// Per-provider timeout, overriding `timeouts.request` for one attempt.
    pub timeout_override: Option<Duration>,
    /// Retry budget used for this provider.
    pub retry: RetryPolicy,
    /// Additional request headers; values are `[REDACTED]` and never serialized.
    #[serde(skip_serializing)]
    pub extra_headers: BTreeMap<String, SecretString>,
    /// Additional JSON merged into the request body (lowest precedence).
    pub extra_body: BTreeMap<String, serde_json::Value>,
}

// `serde_json::Value` cannot represent NaN (`Number::from_f64(NaN)` returns
// `None`), so structural equality is reflexive and `Eq` is sound here.
impl Eq for ProviderConfig {}

impl Default for ProviderConfig {
    /// Empty placeholder: `id` is empty and therefore invalid. Deserialization
    /// uses it for missing fields; [`ProviderConfig::validate`] rejects it.
    fn default() -> Self {
        Self {
            id: ProviderId(String::new()),
            display_name: String::new(),
            protocol: ProviderProtocol::default(),
            base_url: ProviderProtocol::default().default_base_url().to_owned(),
            api_key: None,
            allow_unauthenticated: false,
            enabled: true,
            models: Vec::new(),
            default_model: None,
            timeout_override: None,
            retry: RetryPolicy::default(),
            extra_headers: BTreeMap::new(),
            extra_body: BTreeMap::new(),
        }
    }
}

impl ProviderConfig {
    /// Builds a provider definition with the protocol's default base URL.
    pub fn new(id: ProviderId, protocol: ProviderProtocol) -> Self {
        Self {
            display_name: id.to_string(),
            base_url: protocol.default_base_url().to_owned(),
            protocol,
            id,
            ..Self::default()
        }
    }

    /// Overrides the base URL (trailing slashes are tolerated).
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Attaches an API key.
    #[must_use]
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(SecretString::new(api_key));
        self
    }

    /// Adds a model to the provider.
    #[must_use]
    pub fn with_model(mut self, model: ModelConfig) -> Self {
        self.models.push(model);
        self
    }

    /// Sets the default model id.
    #[must_use]
    pub fn with_default_model(mut self, model_id: impl Into<String>) -> Self {
        self.default_model = Some(model_id.into());
        self
    }

    /// Replaces the retry budget.
    #[must_use]
    pub fn with_retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// Marks the provider as disabled.
    #[must_use]
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Adds a custom request header.
    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_headers
            .insert(name.into(), SecretString::new(value));
        self
    }

    /// Full endpoint URL for a protocol path (`chat/completions`, `messages`, ...).
    #[must_use]
    pub fn endpoint(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    /// Looks up one of the provider's models by wire id.
    #[must_use]
    pub fn model(&self, model_id: &str) -> Option<&ModelConfig> {
        self.models.iter().find(|model| model.id == model_id)
    }

    /// Models that are enabled for routing.
    pub fn enabled_models(&self) -> impl Iterator<Item = &ModelConfig> {
        self.models.iter().filter(|model| model.enabled)
    }

    /// Resolves the provider's default model: the configured one, else the first
    /// enabled model.
    #[must_use]
    pub fn default_model_config(&self) -> Option<&ModelConfig> {
        match self.default_model.as_deref() {
            Some(id) => self.model(id),
            None => self.enabled_models().next(),
        }
    }

    /// Validates the provider definition.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] with a `providers.<id>.<field>` message when
    /// the id, name, base URL, credentials, models, default model, timeout
    /// override or retry budget are invalid.
    pub fn validate(&self) -> CoreResult<()> {
        let prefix = if self.id.as_str().is_empty() {
            "providers.<unnamed>".to_owned()
        } else {
            format!("providers.{}", self.id)
        };

        validate_provider_id(self.id.as_str())
            .map_err(|error| CoreError::config_field(format!("{prefix}.id"), error.to_string()))?;

        if self.display_name.trim().is_empty() {
            return Err(CoreError::config_field(
                format!("{prefix}.display_name"),
                "must not be empty",
            ));
        }
        validate_base_url(&self.base_url)
            .map_err(|reason| CoreError::config_field(format!("{prefix}.base_url"), reason))?;

        if let Some(key) = &self.api_key {
            if key.is_empty() {
                return Err(CoreError::config_field(
                    format!("{prefix}.api_key"),
                    "must not be empty when present",
                ));
            }
        }
        if self.enabled && !self.allow_unauthenticated && self.api_key.is_none() {
            return Err(CoreError::config_field(
                format!("{prefix}.api_key"),
                "is required for an enabled provider",
            ));
        }

        if self.models.is_empty() {
            return Err(CoreError::config_field(
                format!("{prefix}.models"),
                "must declare at least one model",
            ));
        }
        for (index, model) in self.models.iter().enumerate() {
            model.validate(&format!("{prefix}.models[{index}]"))?;
            if self.models[..index]
                .iter()
                .any(|other| other.id == model.id)
            {
                return Err(CoreError::config_field(
                    format!("{prefix}.models[{index}].id"),
                    format!("duplicate model id `{}`", model.id),
                ));
            }
        }

        if let Some(default_model) = &self.default_model {
            let model = self.model(default_model).ok_or_else(|| {
                CoreError::config_field(
                    format!("{prefix}.default_model"),
                    format!("`{default_model}` is not declared in `models`"),
                )
            })?;
            if !model.enabled {
                return Err(CoreError::config_field(
                    format!("{prefix}.default_model"),
                    format!("`{default_model}` is disabled"),
                ));
            }
        }
        if self.default_model.is_none() && self.enabled_models().next().is_none() {
            return Err(CoreError::config_field(
                format!("{prefix}.models"),
                "an enabled provider needs at least one enabled model",
            ));
        }

        if let Some(timeout) = self.timeout_override {
            if timeout.is_zero() || timeout > MAX_PROVIDER_TIMEOUT {
                return Err(CoreError::config_field(
                    format!("{prefix}.timeout_override"),
                    format!("must be greater than zero and at most {MAX_PROVIDER_TIMEOUT:?}"),
                ));
            }
        }

        self.retry.validate(&format!("{prefix}.retry"))?;

        for name in self.extra_headers.keys() {
            if name.trim().is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            {
                return Err(CoreError::config_field(
                    format!("{prefix}.extra_headers"),
                    format!("`{name}` is not a valid header name"),
                ));
            }
        }
        Ok(())
    }
}

/// Validates an API base URL without pulling in a URL parser.
///
/// Rules: `http`/`https` scheme, a non-empty host, no whitespace, no fragment and
/// no embedded credentials (`https://user:token@host`).
fn validate_base_url(value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("must not be empty".to_owned());
    }
    if trimmed != value {
        return Err("must not contain leading or trailing whitespace".to_owned());
    }
    let rest = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .ok_or_else(|| "must start with `http://` or `https://`".to_owned())?;
    if rest.is_empty() {
        return Err("has no host".to_owned());
    }
    if rest.contains(char::is_whitespace) {
        return Err("must not contain whitespace".to_owned());
    }
    if rest.contains('#') {
        return Err("must not contain a fragment".to_owned());
    }
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return Err("has no host".to_owned());
    }
    if authority.contains('@') {
        return Err("must not embed credentials".to_owned());
    }
    if authority.starts_with(':') {
        return Err("has no host".to_owned());
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn provider_ids_are_validated() {
        assert!(ProviderId::new("openai").is_ok());
        assert!(ProviderId::new("deep-seek_2.local").is_ok());
        for invalid in [
            "",
            "  ",
            "has space",
            "sl/ash",
            ".leading",
            "trailing.",
            "中文",
        ] {
            assert!(ProviderId::new(invalid).is_err(), "`{invalid}` must fail");
        }
    }

    #[test]
    fn model_spec_round_trips() {
        let spec = ModelSpec::from_str("openai/gpt-4o-mini").unwrap();
        assert_eq!(spec.provider().as_str(), "openai");
        assert_eq!(spec.model(), "gpt-4o-mini");
        assert_eq!(spec.to_string(), "openai/gpt-4o-mini");

        let nested = ModelSpec::from_str("openrouter/meta-llama/llama-3-8b").unwrap();
        assert_eq!(nested.model(), "meta-llama/llama-3-8b");

        assert!(ModelSpec::from_str("gpt-4o-mini").is_err());
        assert!(ModelSpec::from_str("openai/").is_err());
    }

    #[test]
    fn backoff_is_exponential_and_capped() {
        let policy = RetryPolicy {
            max_attempts: 5,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_millis(500),
            retry_on_timeout: false,
        };
        assert_eq!(policy.backoff_for(1), Duration::ZERO);
        assert_eq!(policy.backoff_for(2), Duration::from_millis(100));
        assert_eq!(policy.backoff_for(3), Duration::from_millis(200));
        assert_eq!(policy.backoff_for(4), Duration::from_millis(400));
        assert_eq!(policy.backoff_for(5), Duration::from_millis(500));
        assert_eq!(policy.backoff_for(99), Duration::from_millis(500));
    }

    #[test]
    fn retry_policy_bounds_are_enforced() {
        let mut policy = RetryPolicy::default();
        policy.validate("retry").unwrap();

        policy.max_attempts = 0;
        assert!(policy.validate("retry").is_err());

        policy.max_attempts = MAX_ATTEMPTS + 1;
        assert!(policy.validate("retry").is_err());

        policy.max_attempts = 2;
        policy.initial_backoff = Duration::from_secs(5);
        policy.max_backoff = Duration::from_secs(1);
        assert!(policy.validate("retry").is_err());
    }

    #[test]
    fn base_urls_are_validated() {
        assert!(validate_base_url("https://api.openai.com/v1").is_ok());
        assert!(validate_base_url("http://127.0.0.1:8080/v1").is_ok());
        for invalid in [
            "",
            "api.openai.com",
            "ftp://host",
            "https://",
            "https://user:pass@host/v1",
            "https://host/v1#frag",
            " https://host",
        ] {
            assert!(
                validate_base_url(invalid).is_err(),
                "`{invalid}` must be rejected"
            );
        }
    }

    #[test]
    fn provider_configuration_is_validated() {
        let provider = ProviderConfig::new(
            ProviderId::new("openai").unwrap(),
            ProviderProtocol::OpenAiCompatible,
        )
        .with_api_key("sk-test")
        .with_model(ModelConfig::new("gpt-4o-mini"))
        .with_default_model("gpt-4o-mini");

        provider.validate().unwrap();
        assert_eq!(
            provider.endpoint("chat/completions"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(provider.default_model_config().unwrap().id, "gpt-4o-mini");

        let no_key = ProviderConfig::new(
            ProviderId::new("openai").unwrap(),
            ProviderProtocol::OpenAiCompatible,
        );
        assert!(no_key.validate().is_err());

        let missing_default = provider.clone().with_default_model("unknown");
        assert!(missing_default.validate().is_err());

        let mut duplicate = provider.clone();
        duplicate.models.push(ModelConfig::new("gpt-4o-mini"));
        assert!(duplicate.validate().is_err());
    }

    #[test]
    fn disabled_provider_may_omit_the_key() {
        let provider = ProviderConfig::new(
            ProviderId::new("local").unwrap(),
            ProviderProtocol::OpenAiCompatible,
        )
        .disabled()
        .with_model(ModelConfig::new("local-model"));
        provider.validate().unwrap();
    }

    #[test]
    fn api_key_never_serializes() {
        let provider = ProviderConfig::new(
            ProviderId::new("openai").unwrap(),
            ProviderProtocol::OpenAiCompatible,
        )
        .with_api_key("sk-super-secret-value")
        .with_header("x-api-key", "sk-header-secret")
        .with_model(ModelConfig::new("gpt-4o-mini"));

        let json = serde_json::to_string(&provider).unwrap();
        assert!(!json.contains("sk-super-secret-value"));
        assert!(!json.contains("sk-header-secret"));
        assert!(!json.contains("api_key"));
        assert!(!format!("{provider:?}").contains("sk-super-secret-value"));

        let restored: ProviderConfig = serde_json::from_str(&json).unwrap();
        assert!(restored.api_key.is_none());
    }
}
