//! Usage and network-traffic accounting.
//!
//! # Two independent dimensions
//!
//! | dimension | type | unit | source |
//! |-----------|------|------|--------|
//! | tokens | [`TokenUsage`] | tokens | provider (`Actual`) or local estimator (`Estimated`) |
//! | traffic | [`TrafficStats`] | bytes | adapter transport counters |
//!
//! They are deliberately separate types and separate columns: a byte count is
//! never derived from tokens, and a token count is never derived from bytes.
//!
//! # Where the data lives
//!
//! Records are stored in a SQLite database inside
//! [`StorageLayout::metadata`](crate::storage::StorageLayout::metadata)
//! (`ai-metadata/usage.sqlite3`). The AI-owned accounting database must never be
//! merged with the client business database.
//!
//! # Pipeline
//!
//! ```text
//! ProviderRuntime  --AttemptReport-->  UsageRecorder  --batch-->  UsageStore
//!    (request path)                     (bounded queue)            (SQLite)
//! ```
//!
//! * The router emits one [`AttemptReport`](crate::provider::AttemptReport) per
//!   finished attempt, including failed ones.
//! * [`UsageRecorder`] buffers reports in a bounded channel and writes batches on
//!   the blocking pool. A full queue drops records instead of blocking requests.
//! * [`UsageStore`] owns the schema, migrations and the aggregation queries.
//!
//! # Privacy
//!
//! A [`UsageRecord`] contains identifiers, counters, durations and a redacted
//! error summary. Prompt text, completion text and credentials are never stored
//! or logged by this module.
//!
//! # Phase 4 handoff
//!
//! Streaming counters settle through the [`AiStream`](crate::provider::AiStream)
//! completion hook: it fires on the terminal event, on an error item and when the
//! handle is dropped early. Phase 4 can refine this by emitting
//! [`StreamEvent::Usage`](crate::provider::StreamEvent::Usage) updates as they
//! arrive, while the final settlement keeps using the same record shape.

pub mod record;
pub mod recorder;
pub mod store;

pub use self::record::{
    TokenSource, TokenUsage, TrafficStats, UsageAggregate, UsageBucket, UsageGroupBy, UsageQuery,
    UsageRecord, UsageSummary, MAX_ERROR_CODE_LEN, MAX_ERROR_SUMMARY_LEN, MAX_FEATURE_LEN,
    MAX_MODEL_ID_LEN, MAX_QUERY_LIMIT,
};
pub use self::recorder::UsageRecorder;
pub use self::store::{UsageStore, USAGE_DB_FILE, USAGE_SCHEMA_VERSION};
