//! Request routing: snapshot binding, capability gate, retries and fallbacks.
//!
//! # Snapshot binding
//!
//! Every call resolves **one** configuration snapshot and **one** registry up
//! front (`registry_for`), then passes references to that registry down through
//! all attempts. A configuration hot reload therefore cannot tear a request apart:
//! it publishes a new snapshot, the next request picks it up, the in-flight one
//! finishes on the snapshot it started with.
//!
//! # Budget
//!
//! A request has one overall deadline (`request.timeout` or
//! `timeouts.request`). Each attempt gets `min(provider.timeout_override,
//! remaining budget)`, so retries and fallbacks can never exceed the caller's
//! budget. Expiry is reported as [`CoreError::Timeout`], cancellation as
//! [`CoreError::Cancelled`] — never as a silent success.
//!
//! # Retry / fallback rules
//!
//! * retry only when the error is retryable (and, for timeouts, only when
//!   [`RetryPolicy::retry_on_timeout`] is enabled);
//! * back off with the provider's deterministic exponential policy, cancellable;
//! * after the retries are exhausted, move to the next model in
//!   `ModelConfig::fallbacks` for *transient* provider failures, authentication
//!   failures and timeouts;
//! * capability, token-budget, validation and cancellation errors never fall back
//!   because retrying them elsewhere would mask a configuration bug.

use std::fmt;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};

use crate::async_utils::{run_with_limits, CancelToken};
use crate::config::{ConfigSnapshot, ConfigStore};
use crate::error::{CoreError, CoreResult, ErrorKind};
use crate::ids::{RequestId, SessionId, TraceId};
use crate::observability::{record_trace_context, Instrument, TraceContext};
use crate::provider::adapter::{AdapterCall, EmbeddingCall};
use crate::provider::capabilities::{
    estimate_embedding_input_tokens, estimate_input_tokens, estimate_text_tokens, Capability,
    RequestRequirements,
};
use crate::provider::config::{ProviderId, RetryPolicy};
use crate::provider::factory::AdapterFactory;
use crate::provider::registry::{ModelDescriptor, ProviderRecord, ProviderRegistry};
use crate::provider::response::{
    AiResponse, AiStream, EmbeddingResponse, EstimatedTokens, RequestStatus, StreamTermination,
    TransportStats, UsageReport,
};
use crate::provider::types::{AiRequest, EmbeddingRequest};
use crate::secret::sanitize_log_text;

/// Trace/cancellation handle of one logical call, shared by all attempts.
#[derive(Debug, Clone)]
pub struct CallContext {
    trace: TraceContext,
    cancel: CancelToken,
    session_id: Option<SessionId>,
    feature: Option<String>,
}

impl CallContext {
    /// Builds a context from an existing trace and cancellation token.
    #[must_use]
    pub fn new(trace: TraceContext, cancel: CancelToken) -> Self {
        Self {
            trace,
            cancel,
            session_id: None,
            feature: None,
        }
    }

    /// Creates a fresh trace and cancellation token.
    #[must_use]
    pub fn generate() -> Self {
        Self::new(TraceContext::generate(), CancelToken::new())
    }

    /// Attaches the session this call belongs to.
    ///
    /// The session id is correlation metadata only: it is recorded in usage
    /// statistics, never used for authorization.
    #[must_use]
    pub fn with_session(mut self, session_id: SessionId) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Attaches a caller-defined feature label (e.g. `vocabulary.lookup`).
    ///
    /// The label is truncated to [`crate::usage::MAX_FEATURE_LEN`] characters and
    /// control characters are rejected by the usage store when the record is
    /// written.
    #[must_use]
    pub fn with_feature(mut self, feature: impl Into<String>) -> Self {
        self.feature = Some(feature.into());
        self
    }

    /// Trace context of the logical call.
    #[must_use]
    pub fn trace(&self) -> &TraceContext {
        &self.trace
    }

    /// Cancellation token of the logical call.
    #[must_use]
    pub fn cancel(&self) -> &CancelToken {
        &self.cancel
    }

    /// Session this call belongs to, when the host supplied one.
    #[must_use]
    pub fn session_id(&self) -> Option<SessionId> {
        self.session_id
    }

    /// Caller-defined feature label, when supplied.
    #[must_use]
    pub fn feature(&self) -> Option<&str> {
        self.feature.as_deref()
    }

    /// Whether the caller already requested cancellation.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

impl Default for CallContext {
    fn default() -> Self {
        Self::generate()
    }
}

/// Overall budget of one call.
#[derive(Debug, Clone, Copy)]
struct Deadline {
    budget: Duration,
    expires_at: Instant,
}

impl Deadline {
    fn new(budget: Duration) -> Self {
        Self {
            budget,
            expires_at: Instant::now() + budget,
        }
    }

    fn remaining(&self) -> CoreResult<Duration> {
        let now = Instant::now();
        if now >= self.expires_at {
            return Err(CoreError::timeout("provider.request", self.budget));
        }
        Ok(self.expires_at - now)
    }
}

/// One model the router may call, in priority order.
#[derive(Debug)]
struct PlannedEntry {
    descriptor: Arc<ModelDescriptor>,
}

/// Primary model plus its compatible fallbacks, in order.
#[derive(Debug)]
struct PlannedCall {
    entries: Vec<PlannedEntry>,
}

/// Everything an attempt loop needs, all borrowed from one snapshot.
#[derive(Debug, Clone, Copy)]
struct Execution<'a> {
    registry: &'a ProviderRegistry,
    plan: &'a PlannedCall,
    request: &'a AiRequest,
    context: &'a CallContext,
    trace: &'a TraceContext,
    deadline: Deadline,
    streaming_idle: Duration,
}

/// One finished provider attempt, as observed by an [`AttemptObserver`].
///
/// The report is deliberately number-only: it carries identifiers, counters and
/// a **sanitized** error summary, never prompt or completion text. Token counts
/// come either from the provider (`usage`) or from the local estimator
/// (`estimated_tokens`); the two are never merged into a single number here.
#[derive(Debug, Clone, PartialEq)]
pub struct AttemptReport {
    /// Request identifier shared by every attempt of the logical call.
    pub request_id: RequestId,
    /// Trace identifier of the logical call.
    pub trace_id: TraceId,
    /// Session the call belongs to, when supplied by the host.
    pub session_id: Option<SessionId>,
    /// Caller-defined feature label, when supplied.
    pub feature: Option<String>,
    /// Provider that served the attempt, when one was reached.
    pub provider_id: Option<ProviderId>,
    /// Wire model id of the attempt, when one was reached.
    pub model_id: Option<String>,
    /// 1-based attempt counter across retries and fallbacks.
    pub attempt: u32,
    /// Whether the attempt opened a stream.
    pub streaming: bool,
    /// Wall-clock time at which the attempt started.
    pub started_at: SystemTime,
    /// Duration of the attempt (until settlement, for streams).
    pub latency: Duration,
    /// Outcome classification of the attempt.
    pub status: RequestStatus,
    /// Stable error code (`CORE_*`), when the attempt failed.
    pub error_code: Option<String>,
    /// Redacted error summary, when the attempt failed.
    pub error_message: Option<String>,
    /// Usage reported by the provider (never synthesized locally).
    pub usage: Option<UsageReport>,
    /// Locally estimated token counts, used only when `usage` is absent.
    pub estimated_tokens: Option<EstimatedTokens>,
    /// Byte counters of the attempt.
    pub transport: TransportStats,
}

/// Receives one report per finished provider attempt.
///
/// # Contract
///
/// * The router calls [`AttemptObserver::observe_attempt`] synchronously on the
///   request path, so implementations must not block: buffer the report and
///   return.
/// * A failing observer must never fail the request; swallow storage errors and
///   log a redacted warning instead.
/// * Reports may arrive from several tasks concurrently, so implementations
///   must be `Send + Sync`.
pub trait AttemptObserver: Send + Sync {
    /// Records one finished attempt, successful or not.
    fn observe_attempt(&self, report: &AttemptReport);
}

/// Optional observer slot with a `Debug` implementation that never prints it.
#[derive(Clone, Default)]
struct ObserverSlot(Option<Arc<dyn AttemptObserver>>);

impl ObserverSlot {
    fn get(&self) -> Option<Arc<dyn AttemptObserver>> {
        self.0.clone()
    }
}

impl fmt::Debug for ObserverSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObserverSlot")
            .field("attached", &self.0.is_some())
            .finish()
    }
}

/// Everything needed to build an [`AttemptReport`] without borrowing the error.
struct AttemptReportInput {
    provider_id: Option<ProviderId>,
    model_id: Option<String>,
    attempt: u32,
    streaming: bool,
    started_at: SystemTime,
    latency: Duration,
    status: RequestStatus,
    error_code: Option<&'static str>,
    error_message: Option<String>,
    usage: Option<UsageReport>,
    estimated_tokens: Option<EstimatedTokens>,
    transport: TransportStats,
}

/// Executes AI requests against the provider configuration snapshot.
///
/// The runtime is cheap to clone by reference (`Arc<ProviderRuntime>`) and safe to
/// share across tasks: the registry is swapped under a short write lock that is
/// never held across an `.await`.
#[derive(Debug)]
pub struct ProviderRuntime {
    config: Arc<ConfigStore>,
    factory: AdapterFactory,
    registry: RwLock<Arc<ProviderRegistry>>,
    observer: ObserverSlot,
}

impl ProviderRuntime {
    /// Builds a runtime from a configuration store.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] when the current snapshot cannot be turned
    /// into a registry (invalid provider, missing adapter for a protocol).
    pub fn new(config: Arc<ConfigStore>) -> CoreResult<Self> {
        Self::with_factory(config, AdapterFactory::new()?)
    }

    /// Builds a runtime with a caller-provided [`AdapterFactory`].
    ///
    /// This is the entry point for tests (mock adapters) and for hosts that pin a
    /// custom adapter to a provider id.
    ///
    /// # Errors
    ///
    /// See [`ProviderRuntime::new`].
    pub fn with_factory(config: Arc<ConfigStore>, factory: AdapterFactory) -> CoreResult<Self> {
        let snapshot = config.snapshot();
        let registry = Arc::new(ProviderRegistry::from_snapshot(&snapshot, &factory)?);
        Ok(Self {
            config,
            factory,
            registry: RwLock::new(registry),
            observer: ObserverSlot::default(),
        })
    }

    /// Attaches an [`AttemptObserver`], typically the usage recorder.
    ///
    /// Reporting is best-effort: the observer cannot fail a request and a missing
    /// observer simply disables statistics.
    #[must_use]
    pub fn with_observer(mut self, observer: Arc<dyn AttemptObserver>) -> Self {
        self.observer = ObserverSlot(Some(observer));
        self
    }

    /// The configuration store backing this runtime.
    #[must_use]
    pub fn config_store(&self) -> &Arc<ConfigStore> {
        &self.config
    }

    /// The adapter factory (use it to bind mock or custom adapters).
    #[must_use]
    pub fn factory(&self) -> &AdapterFactory {
        &self.factory
    }

    /// Current configuration snapshot.
    #[must_use]
    pub fn snapshot(&self) -> Arc<ConfigSnapshot> {
        self.config.snapshot()
    }

    /// Registry currently published (may lag behind the store by one reload).
    #[must_use]
    pub fn registry(&self) -> Arc<ProviderRegistry> {
        match self.registry.read() {
            Ok(guard) => Arc::clone(&guard),
            Err(poisoned) => Arc::clone(&poisoned.into_inner()),
        }
    }

    /// Rebuilds the registry if the configuration version changed.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] when the new snapshot cannot be turned into a
    /// registry; the previous registry keeps serving requests in that case.
    pub fn refresh(&self) -> CoreResult<Arc<ProviderRegistry>> {
        let snapshot = self.snapshot();
        self.registry_for(&snapshot)
    }

    /// Returns the registry matching `snapshot`, rebuilding it once if needed.
    fn registry_for(&self, snapshot: &ConfigSnapshot) -> CoreResult<Arc<ProviderRegistry>> {
        {
            let current = match self.registry.read() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if current.version() == snapshot.version() {
                return Ok(Arc::clone(&current));
            }
        }

        let mut current = match self.registry.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if current.version() == snapshot.version() {
            return Ok(Arc::clone(&current));
        }
        let rebuilt = Arc::new(ProviderRegistry::from_snapshot(snapshot, &self.factory)?);
        *current = Arc::clone(&rebuilt);
        Ok(rebuilt)
    }

    /// Executes a non-streaming request against the latest configuration.
    ///
    /// # Errors
    ///
    /// Validation, capability, token-budget, timeout, cancellation and provider
    /// errors are returned as [`CoreError`].
    pub async fn complete(
        &self,
        request: AiRequest,
        context: &CallContext,
    ) -> CoreResult<AiResponse> {
        let snapshot = self.snapshot();
        self.complete_with_snapshot(&snapshot, request, context)
            .await
    }

    /// Executes a non-streaming request against an explicit snapshot.
    ///
    /// Use this to pin several calls to one configuration version (batch jobs,
    /// multi-step flows) so they cannot observe a reload midway.
    ///
    /// # Errors
    ///
    /// See [`ProviderRuntime::complete`].
    pub async fn complete_with_snapshot(
        &self,
        snapshot: &ConfigSnapshot,
        request: AiRequest,
        context: &CallContext,
    ) -> CoreResult<AiResponse> {
        request.validate()?;
        if context.is_cancelled() {
            return Err(CoreError::cancelled("provider.complete"));
        }
        let registry = self.registry_for(snapshot)?;
        let plan = build_plan(&registry, &request, false)?;
        let deadline = Deadline::new(
            request
                .timeout()
                .unwrap_or(snapshot.config().timeouts.request),
        );
        let trace = context.trace().child_request();
        let execution = Execution {
            registry: &registry,
            plan: &plan,
            request: &request,
            context,
            trace: &trace,
            deadline,
            streaming_idle: snapshot.config().timeouts.streaming_idle,
        };
        self.execute_complete(&execution).await
    }

    /// Opens a streaming request against the latest configuration.
    ///
    /// Retries and fallbacks are applied **before** the first event is delivered;
    /// once the stream is established, mid-stream failures surface as `Err` items
    /// on the stream (see [`AiStream`]).
    ///
    /// # Errors
    ///
    /// See [`ProviderRuntime::complete`].
    pub async fn stream(&self, request: AiRequest, context: &CallContext) -> CoreResult<AiStream> {
        let snapshot = self.snapshot();
        self.stream_with_snapshot(&snapshot, request, context).await
    }

    /// Opens a streaming request against an explicit snapshot.
    ///
    /// # Errors
    ///
    /// See [`ProviderRuntime::stream`].
    pub async fn stream_with_snapshot(
        &self,
        snapshot: &ConfigSnapshot,
        request: AiRequest,
        context: &CallContext,
    ) -> CoreResult<AiStream> {
        request.validate()?;
        if context.is_cancelled() {
            return Err(CoreError::cancelled("provider.stream"));
        }
        let registry = self.registry_for(snapshot)?;
        let plan = build_plan(&registry, &request, true)?;
        let deadline = Deadline::new(
            request
                .timeout()
                .unwrap_or(snapshot.config().timeouts.request),
        );
        let trace = context.trace().child_request();
        let execution = Execution {
            registry: &registry,
            plan: &plan,
            request: &request,
            context,
            trace: &trace,
            deadline,
            streaming_idle: snapshot.config().timeouts.streaming_idle,
        };
        self.execute_stream(&execution).await
    }

    /// Executes an embedding request against the latest configuration.
    ///
    /// Embeddings share the chat path's rules: one snapshot per call, capability
    /// gate before the network, per-attempt budget, retries and compatible
    /// fallbacks.
    ///
    /// # Errors
    ///
    /// [`CoreError::CapabilityUnsupported`] when the target model does not declare
    /// the `embedding` capability, [`CoreError::TokenLimitExceeded`] when the batch
    /// does not fit the context window, plus the transport errors of the attempt.
    pub async fn embed(
        &self,
        request: EmbeddingRequest,
        context: &CallContext,
    ) -> CoreResult<EmbeddingResponse> {
        let snapshot = self.snapshot();
        self.embed_with_snapshot(&snapshot, request, context).await
    }

    /// Executes an embedding request against an explicit snapshot.
    ///
    /// # Errors
    ///
    /// See [`ProviderRuntime::embed`].
    pub async fn embed_with_snapshot(
        &self,
        snapshot: &ConfigSnapshot,
        request: EmbeddingRequest,
        context: &CallContext,
    ) -> CoreResult<EmbeddingResponse> {
        request.validate()?;
        if context.is_cancelled() {
            return Err(CoreError::cancelled("provider.embed"));
        }
        let registry = self.registry_for(snapshot)?;
        let plan = build_embedding_plan(&registry, &request)?;
        let deadline = Deadline::new(
            request
                .timeout()
                .unwrap_or(snapshot.config().timeouts.request),
        );
        let trace = context.trace().child_request();
        self.execute_embedding(&registry, &plan, &request, context, &trace, deadline)
            .await
    }

    async fn execute_embedding(
        &self,
        registry: &ProviderRegistry,
        plan: &PlannedCall,
        request: &EmbeddingRequest,
        context: &CallContext,
        trace: &TraceContext,
        deadline: Deadline,
    ) -> CoreResult<EmbeddingResponse> {
        let mut attempts = 0_u32;
        let mut last_error: Option<CoreError> = None;
        let estimated_input = estimate_embedding_input_tokens(request);

        for (index, entry) in plan.entries.iter().enumerate() {
            let record = self.require_record(registry, entry)?;
            let policy = &record.config().retry;
            let max_attempts = policy.max_attempts.max(1);
            let has_fallback = index + 1 < plan.entries.len();

            for attempt in 1..=max_attempts {
                if context.is_cancelled() {
                    return Err(CoreError::cancelled("provider.embed"));
                }
                let remaining = deadline.remaining()?;
                let limit = record
                    .config()
                    .timeout_override
                    .map_or(remaining, |override_limit| override_limit.min(remaining));

                let span = attempt_span("provider.embed", &entry.descriptor, attempt, trace);
                let call = EmbeddingCall {
                    request,
                    provider: record.config(),
                    model: entry.descriptor.model(),
                    trace,
                    cancel: context.cancel(),
                };
                let adapter = Arc::clone(record.adapter());
                let started_at = SystemTime::now();
                let started = Instant::now();
                let outcome =
                    run_with_limits("provider.embed", limit, context.cancel(), async move {
                        adapter.embed(call).await
                    })
                    .instrument(span)
                    .await;
                attempts += 1;
                let latency = started.elapsed();

                match outcome {
                    Ok(Ok(mut response)) => {
                        response.set_attempts(attempts);
                        response.set_latency(latency);
                        response.set_correlation(trace.trace_id(), trace.request_id());
                        let mut input = attempt_base(entry, attempts, false, started_at, latency);
                        input.status = if attempts == 1 {
                            RequestStatus::Success
                        } else {
                            RequestStatus::Degraded
                        };
                        input.usage = response.usage().cloned();
                        input.estimated_tokens = Some(EstimatedTokens::new(estimated_input, 0));
                        input.transport = response.transport();
                        self.emit_attempt(context, trace, input);
                        return Ok(response);
                    }
                    Ok(Err(error)) | Err(error) => {
                        let mut input = attempt_base(entry, attempts, false, started_at, latency);
                        input.status = status_for(&error);
                        input.error_code = Some(error.error_code());
                        input.error_message = Some(sanitize_log_text(&error.to_string()));
                        input.estimated_tokens = Some(EstimatedTokens::new(estimated_input, 0));
                        self.emit_attempt(context, trace, input);
                        let retry_allowed = should_retry(&error, policy) && attempt < max_attempts;
                        if retry_allowed {
                            tracing::warn!(
                                provider = %entry.descriptor.provider_id(),
                                model = %entry.descriptor.model_id(),
                                attempt,
                                error = %error,
                                "embedding attempt failed; retrying"
                            );
                            wait_backoff(policy, attempt, context, deadline).await?;
                            last_error = Some(error);
                            continue;
                        }
                        if has_fallback && should_fallback(&error) {
                            last_error = Some(error);
                            break;
                        }
                        return Err(error);
                    }
                }
            }
        }

        Err(last_error
            .unwrap_or_else(|| CoreError::provider("<none>", "no provider was attempted", false)))
    }

    async fn execute_complete(&self, execution: &Execution<'_>) -> CoreResult<AiResponse> {
        let Execution {
            registry,
            plan,
            request,
            context,
            trace,
            deadline,
            streaming_idle,
        } = *execution;
        let mut attempts = 0_u32;
        let mut last_error: Option<CoreError> = None;

        for (index, entry) in plan.entries.iter().enumerate() {
            let record = self.require_record(registry, entry)?;
            let policy = &record.config().retry;
            let max_attempts = policy.max_attempts.max(1);
            let has_fallback = index + 1 < plan.entries.len();

            for attempt in 1..=max_attempts {
                if context.is_cancelled() {
                    return Err(CoreError::cancelled("provider.complete"));
                }
                let remaining = deadline.remaining()?;
                let limit = record
                    .config()
                    .timeout_override
                    .map_or(remaining, |override_limit| override_limit.min(remaining));

                let span = attempt_span("provider.complete", &entry.descriptor, attempt, trace);
                let call = AdapterCall {
                    request,
                    provider: record.config(),
                    model: entry.descriptor.model(),
                    trace,
                    cancel: context.cancel(),
                    streaming_idle,
                };
                let adapter = Arc::clone(record.adapter());
                let started_at = SystemTime::now();
                let started = Instant::now();
                let outcome =
                    run_with_limits("provider.complete", limit, context.cancel(), async move {
                        adapter.complete(call).await
                    })
                    .instrument(span)
                    .await;
                attempts += 1;
                let latency = started.elapsed();

                match outcome {
                    // `run_with_limits` wraps the adapter result: the outer layer
                    // carries timeout/cancellation, the inner one provider errors.
                    Ok(Ok(mut response)) => {
                        response.set_attempts(attempts);
                        response.set_latency(latency);
                        response.set_correlation(trace.trace_id(), trace.request_id());
                        let mut input = attempt_base(entry, attempts, false, started_at, latency);
                        input.status = if attempts == 1 {
                            RequestStatus::Success
                        } else {
                            RequestStatus::Degraded
                        };
                        input.usage = response.usage().cloned();
                        input.estimated_tokens = Some(EstimatedTokens::new(
                            estimate_input_tokens(request),
                            estimate_text_tokens(response.content()),
                        ));
                        input.transport = response.transport();
                        self.emit_attempt(context, trace, input);
                        return Ok(response);
                    }
                    Ok(Err(error)) | Err(error) => {
                        let mut input = attempt_base(entry, attempts, false, started_at, latency);
                        input.status = status_for(&error);
                        input.error_code = Some(error.error_code());
                        input.error_message = Some(sanitize_log_text(&error.to_string()));
                        input.estimated_tokens =
                            Some(EstimatedTokens::new(estimate_input_tokens(request), 0));
                        self.emit_attempt(context, trace, input);
                        let retry_allowed = should_retry(&error, policy) && attempt < max_attempts;
                        if retry_allowed {
                            tracing::warn!(
                                provider = %entry.descriptor.provider_id(),
                                model = %entry.descriptor.model_id(),
                                attempt,
                                error = %error,
                                "provider attempt failed; retrying"
                            );
                            wait_backoff(policy, attempt, context, deadline).await?;
                            last_error = Some(error);
                            continue;
                        }
                        if has_fallback && should_fallback(&error) {
                            tracing::warn!(
                                provider = %entry.descriptor.provider_id(),
                                model = %entry.descriptor.model_id(),
                                error = %error,
                                "provider unavailable; trying the next fallback"
                            );
                            last_error = Some(error);
                            break;
                        }
                        return Err(error);
                    }
                }
            }
        }

        Err(last_error
            .unwrap_or_else(|| CoreError::provider("<none>", "no provider was attempted", false)))
    }

    async fn execute_stream(&self, execution: &Execution<'_>) -> CoreResult<AiStream> {
        let Execution {
            registry,
            plan,
            request,
            context,
            trace,
            deadline,
            streaming_idle,
        } = *execution;
        let mut attempts = 0_u32;
        let mut last_error: Option<CoreError> = None;

        for (index, entry) in plan.entries.iter().enumerate() {
            let record = self.require_record(registry, entry)?;
            let policy = &record.config().retry;
            let max_attempts = policy.max_attempts.max(1);
            let has_fallback = index + 1 < plan.entries.len();

            for attempt in 1..=max_attempts {
                if context.is_cancelled() {
                    return Err(CoreError::cancelled("provider.stream"));
                }
                let remaining = deadline.remaining()?;
                let limit = record
                    .config()
                    .timeout_override
                    .map_or(remaining, |override_limit| override_limit.min(remaining));

                let span = attempt_span("provider.stream", &entry.descriptor, attempt, trace);
                let call = AdapterCall {
                    request,
                    provider: record.config(),
                    model: entry.descriptor.model(),
                    trace,
                    cancel: context.cancel(),
                    streaming_idle,
                };
                let adapter = Arc::clone(record.adapter());
                let started_at = SystemTime::now();
                let started = Instant::now();
                let outcome =
                    run_with_limits("provider.stream", limit, context.cancel(), async move {
                        adapter.stream(call).await
                    })
                    .instrument(span)
                    .await;
                attempts += 1;

                match outcome {
                    Ok(Ok(mut stream)) => {
                        stream.set_correlation(trace.trace_id(), trace.request_id());
                        self.attach_stream_settlement(
                            &mut stream,
                            StreamSettlement {
                                context,
                                trace,
                                attempt: attempts,
                                started_at,
                                started,
                                estimated_input_tokens: estimate_input_tokens(request),
                            },
                        );
                        return Ok(stream);
                    }
                    Ok(Err(error)) | Err(error) => {
                        let mut input =
                            attempt_base(entry, attempts, true, started_at, started.elapsed());
                        input.status = status_for(&error);
                        input.error_code = Some(error.error_code());
                        input.error_message = Some(sanitize_log_text(&error.to_string()));
                        input.estimated_tokens =
                            Some(EstimatedTokens::new(estimate_input_tokens(request), 0));
                        self.emit_attempt(context, trace, input);
                        let retry_allowed = should_retry(&error, policy) && attempt < max_attempts;
                        if retry_allowed {
                            wait_backoff(policy, attempt, context, deadline).await?;
                            last_error = Some(error);
                            continue;
                        }
                        if has_fallback && should_fallback(&error) {
                            last_error = Some(error);
                            break;
                        }
                        return Err(error);
                    }
                }
            }
        }

        Err(last_error
            .unwrap_or_else(|| CoreError::provider("<none>", "no provider was attempted", false)))
    }

    fn require_record<'a>(
        &self,
        registry: &'a ProviderRegistry,
        entry: &PlannedEntry,
    ) -> CoreResult<&'a ProviderRecord> {
        registry
            .provider(entry.descriptor.provider_id())
            .ok_or_else(|| {
                CoreError::internal(format!(
                    "provider `{}` disappeared from the registry",
                    entry.descriptor.provider_id()
                ))
            })
    }

    /// Delivers one attempt report, if an observer is attached.
    fn emit_attempt(&self, context: &CallContext, trace: &TraceContext, input: AttemptReportInput) {
        let Some(observer) = self.observer.get() else {
            return;
        };
        observer.observe_attempt(&AttemptReport {
            request_id: trace.request_id(),
            trace_id: trace.trace_id(),
            session_id: context.session_id(),
            feature: context.feature().map(str::to_owned),
            provider_id: input.provider_id,
            model_id: input.model_id,
            attempt: input.attempt,
            streaming: input.streaming,
            started_at: input.started_at,
            latency: input.latency,
            status: input.status,
            error_code: input.error_code.map(str::to_owned),
            error_message: input.error_message,
            usage: input.usage,
            estimated_tokens: input.estimated_tokens,
            transport: input.transport,
        });
    }

    /// Arms the stream completion hook so token/traffic counters settle when the
    /// consumer finishes, hits an error or drops the handle.
    fn attach_stream_settlement(&self, stream: &mut AiStream, settlement: StreamSettlement<'_>) {
        let Some(observer) = self.observer.get() else {
            return;
        };
        let StreamSettlement {
            context,
            trace,
            attempt,
            started_at,
            started,
            estimated_input_tokens,
        } = settlement;
        let request_id = trace.request_id();
        let trace_id = trace.trace_id();
        let session_id = context.session_id();
        let feature = context.feature().map(str::to_owned);
        stream.set_completion_hook(Box::new(move |summary| {
            observer.observe_attempt(&AttemptReport {
                request_id,
                trace_id,
                session_id,
                feature,
                provider_id: Some(summary.provider_id),
                model_id: Some(summary.model_id),
                attempt,
                streaming: true,
                started_at,
                latency: started.elapsed(),
                status: stream_status(summary.termination, attempt),
                error_code: None,
                error_message: None,
                usage: summary.usage,
                estimated_tokens: Some(EstimatedTokens::new(
                    estimated_input_tokens,
                    summary.estimated_output_tokens,
                )),
                transport: summary.transport,
            });
        }));
    }
}

/// Booking data captured when a stream is established and settled on its end.
struct StreamSettlement<'a> {
    context: &'a CallContext,
    trace: &'a TraceContext,
    attempt: u32,
    started_at: SystemTime,
    started: Instant,
    estimated_input_tokens: u32,
}

/// Builds the identifier part of an attempt report with neutral defaults.
fn attempt_base(
    entry: &PlannedEntry,
    attempt: u32,
    streaming: bool,
    started_at: SystemTime,
    latency: Duration,
) -> AttemptReportInput {
    AttemptReportInput {
        provider_id: Some(entry.descriptor.provider_id().clone()),
        model_id: Some(entry.descriptor.model_id().to_owned()),
        attempt,
        streaming,
        started_at,
        latency,
        status: RequestStatus::Success,
        error_code: None,
        error_message: None,
        usage: None,
        estimated_tokens: None,
        transport: TransportStats::default(),
    }
}

/// Maps a failed attempt onto its status classification.
fn status_for(error: &CoreError) -> RequestStatus {
    if error.is_cancelled() {
        RequestStatus::Cancelled
    } else if error.is_timeout() {
        RequestStatus::Timeout
    } else {
        RequestStatus::Failed
    }
}

/// Maps a stream termination onto the attempt status.
fn stream_status(termination: StreamTermination, attempt: u32) -> RequestStatus {
    match termination {
        StreamTermination::Completed => {
            if attempt == 1 {
                RequestStatus::Success
            } else {
                RequestStatus::Degraded
            }
        }
        StreamTermination::Failed => RequestStatus::Failed,
        StreamTermination::Aborted => RequestStatus::Cancelled,
    }
}

/// Resolves the primary model and its compatible fallbacks.
fn build_plan(
    registry: &ProviderRegistry,
    request: &AiRequest,
    streaming: bool,
) -> CoreResult<PlannedCall> {
    let primary = registry.resolve(request.model())?;
    let primary_requirements = RequestRequirements::from_request(request, streaming)
        .clamp_output_tokens(primary.capabilities());
    // The primary model is a hard requirement: an unsupported capability is a
    // configuration bug that must surface, not be papered over by a fallback.
    registry.check_requirements(&primary, &primary_requirements)?;

    let mut entries = vec![PlannedEntry {
        descriptor: Arc::clone(&primary),
    }];
    // `fallbacks_for` already validates every candidate against the same
    // requirements, so the plan only needs to remember the order.
    entries.extend(
        registry
            .fallbacks_for(&primary, &primary_requirements)
            .into_iter()
            .map(|descriptor| PlannedEntry { descriptor }),
    );
    Ok(PlannedCall { entries })
}

/// Resolves the embedding model and its embedding-capable fallbacks.
fn build_embedding_plan(
    registry: &ProviderRegistry,
    request: &EmbeddingRequest,
) -> CoreResult<PlannedCall> {
    let primary = registry.resolve(request.model())?;
    // `RequestRequirements` cannot express "embeddings" (it describes chat
    // requests), so the gate is explicit and runs before any transport work.
    if !Capability::Embedding.is_supported_by(primary.capabilities()) {
        return Err(CoreError::capability_unsupported(
            primary.provider_id().to_string(),
            primary.model_id().to_owned(),
            Capability::Embedding.as_str(),
        ));
    }

    let requirements = RequestRequirements {
        streaming: false,
        tool_calling: false,
        structured_output: false,
        vision: false,
        estimated_input_tokens: estimate_embedding_input_tokens(request),
        requested_output_tokens: 0,
    };
    registry.check_requirements(&primary, &requirements)?;

    let mut entries = vec![PlannedEntry {
        descriptor: Arc::clone(&primary),
    }];
    entries.extend(
        registry
            .fallbacks_for(&primary, &requirements)
            .into_iter()
            .filter(|descriptor| Capability::Embedding.is_supported_by(descriptor.capabilities()))
            .map(|descriptor| PlannedEntry { descriptor }),
    );
    Ok(PlannedCall { entries })
}

fn should_retry(error: &CoreError, policy: &RetryPolicy) -> bool {
    if error.is_cancelled() {
        return false;
    }
    if error.is_timeout() {
        return policy.retry_on_timeout;
    }
    error.is_retryable()
}

/// Whether another model may succeed where this one failed.
///
/// Only *transient* provider failures, authentication failures and timeouts may
/// switch model. A permanent provider error (HTTP 4xx that is not a rate limit),
/// a capability mismatch, a token-budget overflow, an invalid request or a
/// cancellation would fail the same way everywhere and must surface instead of
/// being masked by a fallback.
fn should_fallback(error: &CoreError) -> bool {
    match error.kind() {
        ErrorKind::Provider => error.is_retryable(),
        ErrorKind::Authentication | ErrorKind::Timeout => true,
        _ => false,
    }
}

/// Waits for the policy backoff, bounded by the remaining budget and cancellation.
async fn wait_backoff(
    policy: &RetryPolicy,
    attempt: u32,
    context: &CallContext,
    deadline: Deadline,
) -> CoreResult<()> {
    let backoff = policy.backoff_for(attempt + 1);
    if backoff.is_zero() {
        return Ok(());
    }
    let limit = deadline.remaining()?;
    run_with_limits(
        "provider.retry_backoff",
        limit,
        context.cancel(),
        tokio::time::sleep(backoff),
    )
    .await?;
    Ok(())
}

fn attempt_span(
    operation: &'static str,
    descriptor: &ModelDescriptor,
    attempt: u32,
    trace: &TraceContext,
) -> tracing::Span {
    let span = tracing::info_span!(
        "provider.attempt",
        operation = operation,
        provider = %descriptor.provider_id(),
        model = %descriptor.model_id(),
        attempt = attempt,
        trace_id = tracing::field::Empty,
        request_id = tracing::field::Empty,
        session_id = tracing::field::Empty,
    );
    record_trace_context(&span, trace);
    span
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::provider::config::{ModelConfig, ProviderConfig, ProviderId, ProviderProtocol};

    fn descriptor(fallbacks: Vec<&str>) -> ModelDescriptor {
        let mut model = ModelConfig::new("m");
        model.fallbacks = fallbacks.into_iter().map(str::to_owned).collect();
        ModelDescriptor::new(ProviderId::new("p").unwrap(), model)
    }

    #[test]
    fn only_transient_failures_trigger_fallbacks() {
        assert!(should_fallback(&CoreError::provider("p", "503", true)));
        assert!(should_fallback(&CoreError::authentication("401")));
        assert!(should_fallback(&CoreError::timeout(
            "op",
            Duration::from_millis(1)
        )));
        // Permanent client errors fail the same way on every model.
        assert!(!should_fallback(&CoreError::provider("p", "400", false)));
        assert!(!should_fallback(&CoreError::capability_unsupported(
            "p", "m", "vision"
        )));
        assert!(!should_fallback(&CoreError::token_limit("too long")));
        assert!(!should_fallback(&CoreError::cancelled("op")));
        assert!(!should_fallback(&CoreError::invalid_argument("f", "m")));
    }

    #[test]
    fn retry_policy_gates_timeouts() {
        let strict = RetryPolicy {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(2),
            retry_on_timeout: false,
        };
        let timeout = CoreError::timeout("op", Duration::from_millis(1));
        assert!(!should_retry(&timeout, &strict));
        assert!(should_retry(
            &CoreError::provider("p", "503", true),
            &strict
        ));
        assert!(!should_retry(
            &CoreError::provider("p", "400", false),
            &strict
        ));
        assert!(!should_retry(&CoreError::cancelled("op"), &strict));

        let lenient = RetryPolicy {
            retry_on_timeout: true,
            ..strict
        };
        assert!(should_retry(&timeout, &lenient));
    }

    #[test]
    fn deadline_reports_timeout_when_exhausted() {
        let deadline = Deadline::new(Duration::from_millis(0));
        let error = deadline.remaining().unwrap_err();
        assert!(error.is_timeout());
    }

    #[test]
    fn plan_uses_the_resolved_primary_first() {
        let descriptor = descriptor(vec![]);
        assert_eq!(descriptor.model_id(), "m");
        assert_eq!(descriptor.provider_id(), &ProviderId::new("p").unwrap());
        assert_eq!(
            ProviderProtocol::default(),
            ProviderProtocol::OpenAiCompatible
        );
        assert_eq!(
            ProviderConfig::new(
                ProviderId::new("p").unwrap(),
                ProviderProtocol::OpenAiCompatible
            )
            .with_model(ModelConfig::new("m"))
            .models
            .len(),
            1
        );
    }
}
