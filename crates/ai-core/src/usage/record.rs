//! Usage and traffic records: the value model written to the accounting store.
//!
//! # Two separate dimensions
//!
//! * [`TokenUsage`] counts **tokens**. Every value carries a [`TokenSource`] so a
//!   provider-measured number can never be confused with a local estimate.
//! * [`TrafficStats`] counts **bytes** on the wire. It is never derived from, and
//!   never summed with, token counts.
//!
//! # Privacy
//!
//! A record holds identifiers, counters and a redacted error summary. Prompt and
//! completion text are never part of this structure.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::{CoreError, CoreResult};
use crate::ids::{RequestId, SessionId, TraceId};
use crate::provider::config::ProviderId;
use crate::provider::response::{EstimatedTokens, RequestStatus, TransportStats, UsageReport};
use crate::provider::router::AttemptReport;

/// Maximum length of a caller-defined feature label.
pub const MAX_FEATURE_LEN: usize = 64;
/// Maximum length of a stored model identifier.
pub const MAX_MODEL_ID_LEN: usize = 128;
/// Maximum length of a stored error code.
pub const MAX_ERROR_CODE_LEN: usize = 64;
/// Maximum length of a stored (already redacted) error summary.
pub const MAX_ERROR_SUMMARY_LEN: usize = 256;
/// Upper bound accepted for [`UsageQuery::limit`].
pub const MAX_QUERY_LIMIT: u32 = 1_000;

/// Origin of the token numbers of one attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TokenSource {
    /// Reported by the provider (API usage object).
    #[default]
    Actual,
    /// Computed locally by the framework's character-based estimator.
    Estimated,
}

impl TokenSource {
    /// Stable lowercase label used in storage.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Actual => "actual",
            Self::Estimated => "estimated",
        }
    }

    /// Parses the label produced by [`Self::as_str`].
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "actual" => Some(Self::Actual),
            "estimated" => Some(Self::Estimated),
            _ => None,
        }
    }
}

impl std::fmt::Display for TokenSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Token accounting of one attempt.
///
/// The `source` field is part of the contract: an [`TokenSource::Estimated`]
/// value is a heuristic and must be presented to users as an estimate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenUsage {
    /// Input/prompt tokens.
    pub input: u32,
    /// Generated tokens.
    pub output: u32,
    /// Total tokens (`input + output` when the provider omitted it).
    pub total: u32,
    /// Reasoning/thinking tokens, when reported.
    pub reasoning: Option<u32>,
    /// Tokens served from a provider cache, when reported.
    pub cached: Option<u32>,
    /// Whether these numbers are measured or estimated.
    pub source: TokenSource,
}

impl TokenUsage {
    /// Builds token usage from a provider-reported [`UsageReport`].
    ///
    /// Returns `None` when the report carries no numeric counters at all: a
    /// `raw`-only payload must not be turned into a fake `Actual` zero.
    #[must_use]
    pub fn from_provider(report: &UsageReport) -> Option<Self> {
        let has_numbers = report.prompt_tokens.is_some()
            || report.completion_tokens.is_some()
            || report.total_tokens.is_some()
            || report.reasoning_tokens.is_some()
            || report.cached_tokens.is_some();
        if !has_numbers {
            return None;
        }
        let input = report.prompt_tokens.unwrap_or(0);
        let output = report.completion_tokens.unwrap_or(0);
        let total = report
            .total_tokens
            .unwrap_or_else(|| input.saturating_add(output));
        Some(Self {
            input,
            output,
            total,
            reasoning: report.reasoning_tokens,
            cached: report.cached_tokens,
            source: TokenSource::Actual,
        })
    }

    /// Builds token usage from local estimates.
    ///
    /// Returns `None` when both estimates are zero, which would only add noise.
    #[must_use]
    pub fn from_estimate(estimate: EstimatedTokens) -> Option<Self> {
        let total = estimate.input.saturating_add(estimate.output);
        if total == 0 {
            return None;
        }
        Some(Self {
            input: estimate.input,
            output: estimate.output,
            total,
            reasoning: None,
            cached: None,
            source: TokenSource::Estimated,
        })
    }
}

/// Network traffic of one attempt, in bytes.
///
/// Kept separate from [`TokenUsage`] on purpose: bytes and tokens measure
/// different things and must never be added together.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct TrafficStats {
    /// Serialized request body bytes sent upstream.
    pub request_bytes: u64,
    /// Response bytes received downstream (including framing).
    pub response_bytes: u64,
}

impl TrafficStats {
    /// Builds a traffic record from its two counters.
    #[must_use]
    pub const fn new(request_bytes: u64, response_bytes: u64) -> Self {
        Self {
            request_bytes,
            response_bytes,
        }
    }

    /// Sum of both directions.
    #[must_use]
    pub const fn total(self) -> u64 {
        self.request_bytes.saturating_add(self.response_bytes)
    }

    /// Converts the transport counters reported by a provider adapter.
    #[must_use]
    pub const fn from_transport(transport: TransportStats) -> Self {
        Self {
            request_bytes: transport.request_bytes,
            response_bytes: transport.response_bytes,
        }
    }
}

/// One finished provider attempt, ready to be persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRecord {
    /// Request identifier shared by every attempt of a logical call.
    pub request_id: RequestId,
    /// Trace identifier of the logical call.
    pub trace_id: TraceId,
    /// Session the call belongs to.
    pub session_id: Option<SessionId>,
    /// Caller-defined feature label (for example `vocabulary.lookup`).
    pub feature: Option<String>,
    /// Provider that served the attempt.
    pub provider_id: Option<ProviderId>,
    /// Wire model identifier.
    pub model_id: Option<String>,
    /// 1-based attempt counter across retries and fallbacks.
    pub attempt: u32,
    /// Wall-clock start time.
    pub started_at: SystemTime,
    /// Duration of the attempt.
    pub latency: Duration,
    /// Outcome classification.
    pub status: RequestStatus,
    /// Stable error code (`CORE_*`), when the attempt failed.
    pub error_code: Option<String>,
    /// Redacted error summary, when the attempt failed.
    pub error_message: Option<String>,
    /// Token counts, measured or estimated.
    pub tokens: Option<TokenUsage>,
    /// Byte counters.
    pub traffic: TrafficStats,
}

impl UsageRecord {
    /// Converts an [`AttemptReport`] into a persistable record.
    ///
    /// Provider-measured usage always wins; the local estimate is used only when
    /// the provider reported nothing, and the [`TokenSource`] keeps that visible.
    #[must_use]
    pub fn from_attempt(report: &AttemptReport) -> Self {
        let tokens = report
            .usage
            .as_ref()
            .and_then(TokenUsage::from_provider)
            .or_else(|| report.estimated_tokens.and_then(TokenUsage::from_estimate));
        Self {
            request_id: report.request_id,
            trace_id: report.trace_id,
            session_id: report.session_id,
            feature: report
                .feature
                .as_deref()
                .map(|value| truncate(value, MAX_FEATURE_LEN)),
            provider_id: report.provider_id.clone(),
            model_id: report
                .model_id
                .as_deref()
                .map(|value| truncate(value, MAX_MODEL_ID_LEN)),
            attempt: report.attempt,
            started_at: report.started_at,
            latency: report.latency,
            status: report.status,
            error_code: report
                .error_code
                .as_deref()
                .map(|value| truncate(value, MAX_ERROR_CODE_LEN)),
            error_message: report
                .error_message
                .as_deref()
                .map(|value| truncate(value, MAX_ERROR_SUMMARY_LEN)),
            tokens,
            traffic: TrafficStats::from_transport(report.transport),
        }
    }

    /// Validates the record before it is written.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] when the attempt counter is zero or
    /// a text field exceeds its documented bound.
    pub fn validate(&self) -> CoreResult<()> {
        if self.attempt == 0 {
            return Err(CoreError::invalid_argument(
                "usage.attempt",
                "must be 1 or greater",
            ));
        }
        validate_label("usage.feature", self.feature.as_deref(), MAX_FEATURE_LEN)?;
        validate_label("usage.model_id", self.model_id.as_deref(), MAX_MODEL_ID_LEN)?;
        validate_label(
            "usage.error_code",
            self.error_code.as_deref(),
            MAX_ERROR_CODE_LEN,
        )?;
        validate_label(
            "usage.error_message",
            self.error_message.as_deref(),
            MAX_ERROR_SUMMARY_LEN,
        )?;
        Ok(())
    }

    /// Milliseconds since the Unix epoch, the storage representation of
    /// [`Self::started_at`].
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] for timestamps before the epoch,
    /// which SQLite cannot represent sensibly.
    pub fn started_at_millis(&self) -> CoreResult<i64> {
        system_time_millis("usage.started_at", self.started_at)
    }

    /// Latency in milliseconds, saturating at [`i64::MAX`].
    #[must_use]
    pub fn latency_millis(&self) -> i64 {
        i64::try_from(self.latency.as_millis()).unwrap_or(i64::MAX)
    }
}

/// Grouping dimension of an aggregation query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum UsageGroupBy {
    /// A single summary over all matching rows.
    #[default]
    Total,
    /// One bucket per provider.
    Provider,
    /// One bucket per model.
    Model,
    /// One bucket per caller-defined feature label.
    Feature,
    /// One bucket per session.
    Session,
    /// One bucket per UTC day (`YYYY-MM-DD`).
    Day,
}

impl UsageGroupBy {
    /// Stable lowercase label, also used by hosts to build query endpoints.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Total => "total",
            Self::Provider => "provider",
            Self::Model => "model",
            Self::Feature => "feature",
            Self::Session => "session",
            Self::Day => "day",
        }
    }
}

/// Filters and grouping of an aggregation query.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageQuery {
    /// Inclusive lower bound on the attempt start time.
    pub from: Option<SystemTime>,
    /// Inclusive upper bound on the attempt start time.
    pub to: Option<SystemTime>,
    /// Only this provider.
    pub provider_id: Option<ProviderId>,
    /// Only this model.
    pub model_id: Option<String>,
    /// Only this feature label.
    pub feature: Option<String>,
    /// Only this session.
    pub session_id: Option<SessionId>,
    /// Only this request.
    pub request_id: Option<RequestId>,
    /// Only attempts with this status.
    pub status: Option<RequestStatus>,
    /// Grouping dimension.
    pub group_by: UsageGroupBy,
    /// Maximum number of buckets; ignored for [`UsageGroupBy::Total`].
    pub limit: Option<u32>,
}

impl UsageQuery {
    /// Validates the time range, filters and bucket limit.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] when `from > to`, when a text
    /// filter is empty, or when `limit` is zero or above [`MAX_QUERY_LIMIT`].
    pub fn validate(&self) -> CoreResult<()> {
        if let (Some(from), Some(to)) = (self.from, self.to) {
            if from > to {
                return Err(CoreError::invalid_argument(
                    "usage.range",
                    "`from` must not be later than `to`",
                ));
            }
        }
        for (field, value) in [
            ("usage.model_id", self.model_id.as_deref()),
            ("usage.feature", self.feature.as_deref()),
        ] {
            if value.is_some_and(|value| value.trim().is_empty()) {
                return Err(CoreError::invalid_argument(field, "must not be empty"));
            }
        }
        if let Some(limit) = self.limit {
            if limit == 0 || limit > MAX_QUERY_LIMIT {
                return Err(CoreError::invalid_argument(
                    "usage.limit",
                    format!("must be between 1 and {MAX_QUERY_LIMIT}"),
                ));
            }
        }
        Ok(())
    }
}

/// Aggregated counters over a set of usage records.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageSummary {
    /// Number of attempts (rows).
    pub attempts: u64,
    /// Number of distinct logical requests.
    pub requests: u64,
    /// Attempts that succeeded on their first try.
    pub success: u64,
    /// Attempts that succeeded after a retry or fallback.
    pub degraded: u64,
    /// Attempts that failed permanently.
    pub failed: u64,
    /// Attempts cancelled by the caller.
    pub cancelled: u64,
    /// Attempts that exceeded their deadline.
    pub timeout: u64,
    /// Sum of input tokens.
    pub input_tokens: u64,
    /// Sum of output tokens.
    pub output_tokens: u64,
    /// Sum of total tokens.
    pub total_tokens: u64,
    /// Total tokens that were provider-measured.
    pub actual_tokens: u64,
    /// Total tokens that were locally estimated.
    pub estimated_tokens: u64,
    /// Sum of reasoning tokens.
    pub reasoning_tokens: u64,
    /// Sum of cached tokens.
    pub cached_tokens: u64,
    /// Sum of uploaded request bytes.
    pub request_bytes: u64,
    /// Sum of downloaded response bytes.
    pub response_bytes: u64,
    /// Sum of attempt latencies in milliseconds.
    pub latency_total_ms: u64,
    /// Highest attempt latency in milliseconds.
    pub latency_max_ms: u64,
}

impl UsageSummary {
    /// Total bytes in both directions.
    ///
    /// Bytes are intentionally kept apart from [`Self::total_tokens`].
    #[must_use]
    pub const fn total_bytes(&self) -> u64 {
        self.request_bytes.saturating_add(self.response_bytes)
    }

    /// Number of attempts that did not produce output.
    #[must_use]
    pub const fn unsuccessful(&self) -> u64 {
        self.failed
            .saturating_add(self.cancelled)
            .saturating_add(self.timeout)
    }
}

/// One bucket of a grouped aggregation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageBucket {
    /// Provider id, model id, feature label, session id or `YYYY-MM-DD`.
    pub key: String,
    /// Counters for this bucket.
    pub summary: UsageSummary,
}

/// Result of an aggregation query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageAggregate {
    /// Summary over every matching row, regardless of grouping.
    pub total: UsageSummary,
    /// Per-group summaries, empty for [`UsageGroupBy::Total`].
    pub buckets: Vec<UsageBucket>,
}

/// Truncates `value` to at most `max` characters (never splitting a UTF-8
/// scalar) and strips control characters.
fn truncate(value: &str, max: usize) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(max)
        .collect()
}

fn validate_label(field: &str, value: Option<&str>, max: usize) -> CoreResult<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.chars().count() > max {
        return Err(CoreError::invalid_argument(
            field,
            format!("must not exceed {max} characters"),
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(CoreError::invalid_argument(
            field,
            "must not contain control characters",
        ));
    }
    Ok(())
}

/// Converts a [`SystemTime`] into Unix milliseconds.
pub(crate) fn system_time_millis(field: &str, value: SystemTime) -> CoreResult<i64> {
    let since_epoch = value.duration_since(UNIX_EPOCH).map_err(|_| {
        CoreError::invalid_argument(field, "must not be earlier than the Unix epoch")
    })?;
    Ok(i64::try_from(since_epoch.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn report(prompt: Option<u32>, completion: Option<u32>, total: Option<u32>) -> UsageReport {
        UsageReport {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: total,
            ..UsageReport::default()
        }
    }

    #[test]
    fn provider_tokens_are_marked_actual_and_total_is_derived() {
        let usage = TokenUsage::from_provider(&report(Some(10), Some(5), None)).unwrap();
        assert_eq!(usage.source, TokenSource::Actual);
        assert_eq!(usage.total, 15);
        assert_eq!(usage.total, usage.input + usage.output);
    }

    #[test]
    fn raw_only_reports_do_not_become_fake_actual_usage() {
        let raw_only = UsageReport {
            raw: Some(serde_json::json!({"anything": true})),
            ..UsageReport::default()
        };
        assert!(TokenUsage::from_provider(&raw_only).is_none());
        assert!(TokenUsage::from_provider(&UsageReport::default()).is_none());
    }

    #[test]
    fn local_estimates_are_marked_estimated() {
        let usage = TokenUsage::from_estimate(EstimatedTokens::new(8, 4)).unwrap();
        assert_eq!(usage.source, TokenSource::Estimated);
        assert_eq!(usage.total, 12);
        assert!(TokenUsage::from_estimate(EstimatedTokens::default()).is_none());
    }

    #[test]
    fn traffic_is_separate_from_tokens() {
        let traffic = TrafficStats::from_transport(TransportStats {
            request_bytes: 120,
            response_bytes: 480,
        });
        assert_eq!(traffic.total(), 600);
        assert_eq!(traffic.request_bytes, 120);

        let summary = UsageSummary {
            total_tokens: 42,
            request_bytes: 120,
            response_bytes: 480,
            ..UsageSummary::default()
        };
        assert_eq!(summary.total_tokens, 42);
        assert_eq!(summary.total_bytes(), 600);
    }

    #[test]
    fn query_validation_rejects_bad_ranges_and_limits() {
        let now = SystemTime::now();
        let query = UsageQuery {
            from: Some(now + Duration::from_secs(1)),
            to: Some(now),
            ..UsageQuery::default()
        };
        assert!(query.validate().is_err());

        let query = UsageQuery {
            limit: Some(0),
            ..UsageQuery::default()
        };
        assert!(query.validate().is_err());

        let query = UsageQuery {
            limit: Some(MAX_QUERY_LIMIT + 1),
            ..UsageQuery::default()
        };
        assert!(query.validate().is_err());

        let query = UsageQuery {
            feature: Some("   ".to_owned()),
            ..UsageQuery::default()
        };
        assert!(query.validate().is_err());

        assert!(UsageQuery::default().validate().is_ok());
    }

    #[test]
    fn labels_are_truncated_and_control_characters_removed() {
        let long = format!("a\n{}", "b".repeat(MAX_FEATURE_LEN * 2));
        let truncated = truncate(&long, MAX_FEATURE_LEN);
        assert_eq!(truncated.chars().count(), MAX_FEATURE_LEN);
        assert!(!truncated.contains('\n'));
    }

    #[test]
    fn pre_epoch_timestamps_are_rejected() {
        let record = UsageRecord {
            request_id: RequestId::new(),
            trace_id: TraceId::new(),
            session_id: None,
            feature: None,
            provider_id: None,
            model_id: None,
            attempt: 1,
            started_at: UNIX_EPOCH - Duration::from_secs(1),
            latency: Duration::from_millis(1),
            status: RequestStatus::Success,
            error_code: None,
            error_message: None,
            tokens: None,
            traffic: TrafficStats::default(),
        };
        assert!(record.started_at_millis().is_err());

        let zero_attempt = UsageRecord {
            attempt: 0,
            ..record
        };
        assert!(zero_attempt.validate().is_err());
    }
}
