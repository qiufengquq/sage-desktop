//! SQLite persistence for usage records.
//!
//! # Where the database lives
//!
//! Always [`StorageLayout::metadata`]: AI-owned bookkeeping must never share a
//! file with the client business database. The default file name is
//! [`USAGE_DB_FILE`].
//!
//! # Blocking and concurrency
//!
//! `rusqlite` is synchronous, so every public method here is blocking. Async
//! callers use the `*_async` variants, which run the same code on the blocking
//! pool. A single connection guarded by a `Mutex` serialises writes inside the
//! process; WAL mode plus `busy_timeout` keeps external readers from failing.
//!
//! # Schema
//!
//! The schema version is stored in `PRAGMA user_version` and migrated inside a
//! transaction, so an interrupted upgrade rolls back atomically.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, Row as SqlRow};
use std::sync::{Mutex, MutexGuard};

use crate::async_utils::run_blocking_result;
use crate::error::{CoreError, CoreResult, StorageArea};
use crate::ids::{RequestId, SessionId, TraceId};
use crate::provider::config::ProviderId;
use crate::provider::response::RequestStatus;
use crate::storage::StorageLayout;
use crate::usage::record::{
    system_time_millis, TokenSource, TokenUsage, TrafficStats, UsageAggregate, UsageBucket,
    UsageGroupBy, UsageQuery, UsageRecord, UsageSummary,
};

/// File name of the usage database inside `ai-metadata/`.
pub const USAGE_DB_FILE: &str = "usage.sqlite3";
/// Schema version this build creates and understands.
pub const USAGE_SCHEMA_VERSION: i64 = 1;

const SCHEMA_V1: &str = r#"
CREATE TABLE usage_records (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id      TEXT    NOT NULL,
    trace_id        TEXT    NOT NULL,
    session_id      TEXT,
    feature         TEXT,
    provider_id     TEXT,
    model_id        TEXT,
    attempt         INTEGER NOT NULL,
    started_at_ms   INTEGER NOT NULL,
    latency_ms      INTEGER NOT NULL,
    status          TEXT    NOT NULL,
    error_code      TEXT,
    error_message   TEXT,
    token_input     INTEGER,
    token_output    INTEGER,
    token_total     INTEGER,
    token_reasoning INTEGER,
    token_cached    INTEGER,
    token_source    TEXT,
    request_bytes   INTEGER NOT NULL,
    response_bytes  INTEGER NOT NULL
);

CREATE INDEX idx_usage_started_at ON usage_records (started_at_ms);
CREATE INDEX idx_usage_request    ON usage_records (request_id);
CREATE INDEX idx_usage_provider   ON usage_records (provider_id, started_at_ms);
CREATE INDEX idx_usage_model      ON usage_records (model_id, started_at_ms);
CREATE INDEX idx_usage_feature    ON usage_records (feature, started_at_ms);
CREATE INDEX idx_usage_session    ON usage_records (session_id, started_at_ms);
CREATE INDEX idx_usage_status     ON usage_records (status, started_at_ms);
"#;

const RECORD_COLUMNS: &str = "\
request_id, trace_id, session_id, feature, provider_id, model_id, attempt, \
started_at_ms, latency_ms, status, error_code, error_message, \
token_input, token_output, token_total, token_reasoning, token_cached, token_source, \
request_bytes, response_bytes";

const INSERT_SQL: &str = "\
INSERT INTO usage_records (
    request_id, trace_id, session_id, feature, provider_id, model_id, attempt,
    started_at_ms, latency_ms, status, error_code, error_message,
    token_input, token_output, token_total, token_reasoning, token_cached, token_source,
    request_bytes, response_bytes
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)";

/// Aggregated columns shared by the flat and grouped queries.
const SUMMARY_COLUMNS: &str = "\
COUNT(*) AS attempts,
COUNT(DISTINCT request_id) AS requests,
COALESCE(SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END), 0) AS success,
COALESCE(SUM(CASE WHEN status = 'degraded' THEN 1 ELSE 0 END), 0) AS degraded,
COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0) AS failed,
COALESCE(SUM(CASE WHEN status = 'cancelled' THEN 1 ELSE 0 END), 0) AS cancelled,
COALESCE(SUM(CASE WHEN status = 'timeout' THEN 1 ELSE 0 END), 0) AS timeouts,
COALESCE(SUM(token_input), 0) AS input_tokens,
COALESCE(SUM(token_output), 0) AS output_tokens,
COALESCE(SUM(token_total), 0) AS total_tokens,
COALESCE(SUM(CASE WHEN token_source = 'actual' THEN token_total ELSE 0 END), 0) AS actual_tokens,
COALESCE(SUM(CASE WHEN token_source = 'estimated' THEN token_total ELSE 0 END), 0) AS estimated_tokens,
COALESCE(SUM(token_reasoning), 0) AS reasoning_tokens,
COALESCE(SUM(token_cached), 0) AS cached_tokens,
COALESCE(SUM(request_bytes), 0) AS request_bytes,
COALESCE(SUM(response_bytes), 0) AS response_bytes,
COALESCE(SUM(latency_ms), 0) AS latency_total_ms,
COALESCE(MAX(latency_ms), 0) AS latency_max_ms";

/// A row prepared for insertion (all values already owned and checked).
struct PendingRow {
    request_id: String,
    trace_id: String,
    session_id: Option<String>,
    feature: Option<String>,
    provider_id: Option<String>,
    model_id: Option<String>,
    attempt: i64,
    started_at_ms: i64,
    latency_ms: i64,
    status: &'static str,
    error_code: Option<String>,
    error_message: Option<String>,
    token_input: Option<i64>,
    token_output: Option<i64>,
    token_total: Option<i64>,
    token_reasoning: Option<i64>,
    token_cached: Option<i64>,
    token_source: Option<&'static str>,
    request_bytes: i64,
    response_bytes: i64,
}

impl PendingRow {
    fn from_record(record: &UsageRecord) -> CoreResult<Self> {
        record.validate()?;
        let (token_input, token_output, token_total) = match &record.tokens {
            Some(tokens) => (
                Some(i64::from(tokens.input)),
                Some(i64::from(tokens.output)),
                Some(i64::from(tokens.total)),
            ),
            None => (None, None, None),
        };
        Ok(Self {
            request_id: record.request_id.to_string(),
            trace_id: record.trace_id.to_string(),
            session_id: record.session_id.map(|id| id.to_string()),
            feature: record.feature.clone(),
            provider_id: record.provider_id.as_ref().map(ToString::to_string),
            model_id: record.model_id.clone(),
            attempt: i64::from(record.attempt),
            started_at_ms: record.started_at_millis()?,
            latency_ms: record.latency_millis(),
            status: record.status.as_str(),
            error_code: record.error_code.clone(),
            error_message: record.error_message.clone(),
            token_input,
            token_output,
            token_total,
            token_reasoning: record
                .tokens
                .as_ref()
                .and_then(|tokens| tokens.reasoning)
                .map(i64::from),
            token_cached: record
                .tokens
                .as_ref()
                .and_then(|tokens| tokens.cached)
                .map(i64::from),
            token_source: record.tokens.as_ref().map(|tokens| tokens.source.as_str()),
            request_bytes: i64::try_from(record.traffic.request_bytes).unwrap_or(i64::MAX),
            response_bytes: i64::try_from(record.traffic.response_bytes).unwrap_or(i64::MAX),
        })
    }

    fn execute(&self, statement: &mut rusqlite::Statement<'_>) -> CoreResult<()> {
        statement
            .execute(params![
                self.request_id,
                self.trace_id,
                self.session_id.as_deref(),
                self.feature.as_deref(),
                self.provider_id.as_deref(),
                self.model_id.as_deref(),
                self.attempt,
                self.started_at_ms,
                self.latency_ms,
                self.status,
                self.error_code.as_deref(),
                self.error_message.as_deref(),
                self.token_input,
                self.token_output,
                self.token_total,
                self.token_reasoning,
                self.token_cached,
                self.token_source,
                self.request_bytes,
                self.response_bytes,
            ])
            .map(|_| ())
            .map_err(|error| storage_error("failed to insert a usage record", error))
    }
}

/// SQLite-backed usage accounting store.
///
/// Cloning is intentional at the `Arc` level only: the type itself is not
/// `Clone`, because it owns the connection.
pub struct UsageStore {
    path: PathBuf,
    connection: Mutex<Connection>,
}

impl fmt::Debug for UsageStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UsageStore")
            .field("path", &self.path)
            .finish()
    }
}

impl UsageStore {
    /// Opens (creating if needed) the usage database at `path` and migrates it.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Storage`] with [`StorageArea::Metadata`] when the
    /// directory cannot be created, the database cannot be opened, or the schema
    /// is newer than this build supports.
    pub fn open(path: impl Into<PathBuf>) -> CoreResult<Self> {
        let path = path.into();
        if path.as_os_str().is_empty() {
            return Err(CoreError::invalid_argument(
                "usage.path",
                "must not be empty",
            ));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                CoreError::storage(
                    StorageArea::Metadata,
                    format!("failed to create `{}`", parent.display()),
                    Some(Box::new(error)),
                )
            })?;
        }

        let mut connection = Connection::open(&path)
            .map_err(|error| storage_error("failed to open the usage database", error))?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;\n\
                 PRAGMA synchronous = NORMAL;\n\
                 PRAGMA busy_timeout = 5000;\n\
                 PRAGMA foreign_keys = ON;",
            )
            .map_err(|error| storage_error("failed to configure the usage database", error))?;
        migrate(&mut connection)?;
        Ok(Self {
            path,
            connection: Mutex::new(connection),
        })
    }

    /// Opens the usage database inside a storage layout's `ai-metadata/` area.
    ///
    /// # Errors
    ///
    /// See [`UsageStore::open`].
    pub fn open_in_layout(layout: &StorageLayout) -> CoreResult<Self> {
        Self::open(layout.metadata().join(USAGE_DB_FILE))
    }

    /// Opens the store on the blocking pool.
    ///
    /// # Errors
    ///
    /// See [`UsageStore::open`], plus [`CoreError::Concurrency`] when the blocking
    /// task cannot be joined.
    pub async fn open_in_layout_async(layout: &StorageLayout) -> CoreResult<Self> {
        let layout = layout.clone();
        run_blocking_result("usage.open", move || Self::open_in_layout(&layout)).await
    }

    /// Path of the database file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Schema version currently stored in the database.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Storage`] when the pragma cannot be read.
    pub fn schema_version(&self) -> CoreResult<i64> {
        let connection = self.connection();
        connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|error| storage_error("failed to read the usage schema version", error))
    }

    /// Inserts one record.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] for an invalid record and
    /// [`CoreError::Storage`] for a database failure.
    pub fn insert(&self, record: &UsageRecord) -> CoreResult<()> {
        self.insert_many(std::slice::from_ref(record)).map(|_| ())
    }

    /// Inserts a batch of records in a single transaction.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] when any record is invalid; nothing
    /// is written in that case.
    pub fn insert_many(&self, records: &[UsageRecord]) -> CoreResult<usize> {
        if records.is_empty() {
            return Ok(0);
        }
        let pending = records
            .iter()
            .map(PendingRow::from_record)
            .collect::<CoreResult<Vec<_>>>()?;

        let mut connection = self.connection();
        let transaction = connection
            .transaction()
            .map_err(|error| storage_error("failed to start a usage transaction", error))?;
        {
            let mut statement = transaction
                .prepare(INSERT_SQL)
                .map_err(|error| storage_error("failed to prepare the usage insert", error))?;
            for row in &pending {
                row.execute(&mut statement)?;
            }
        }
        transaction
            .commit()
            .map_err(|error| storage_error("failed to commit usage records", error))?;
        Ok(pending.len())
    }

    /// Inserts a batch on the blocking pool.
    ///
    /// # Errors
    ///
    /// See [`UsageStore::insert_many`].
    pub async fn insert_many_async(
        self: Arc<Self>,
        records: Vec<UsageRecord>,
    ) -> CoreResult<usize> {
        run_blocking_result("usage.insert", move || self.insert_many(&records)).await
    }

    /// Number of stored records.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Storage`] when the query fails.
    pub fn count(&self) -> CoreResult<u64> {
        let connection = self.connection();
        connection
            .query_row("SELECT COUNT(*) FROM usage_records", [], |row| {
                row.get::<_, i64>(0)
            })
            .map(|value| u64::try_from(value).unwrap_or(0))
            .map_err(|error| storage_error("failed to count usage records", error))
    }

    /// Runs an aggregation query.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] for an invalid query and
    /// [`CoreError::Storage`] when the database fails.
    pub fn query(&self, query: &UsageQuery) -> CoreResult<UsageAggregate> {
        query.validate()?;
        let (filter, values) = build_filter(query)?;
        let connection = self.connection();

        let total = read_summary(&connection, &filter, &values)?;
        let buckets = match group_expression(query.group_by) {
            None => Vec::new(),
            Some(expression) => {
                read_buckets(&connection, expression, &filter, &values, query.limit)?
            }
        };
        Ok(UsageAggregate { total, buckets })
    }

    /// Runs an aggregation query on the blocking pool.
    ///
    /// # Errors
    ///
    /// See [`UsageStore::query`].
    pub async fn query_async(self: Arc<Self>, query: UsageQuery) -> CoreResult<UsageAggregate> {
        run_blocking_result("usage.query", move || self.query(&query)).await
    }

    /// Lists matching attempts, newest first.
    ///
    /// This is the detail view behind a dashboard table or an export; use
    /// [`UsageStore::query`] when only counters are needed.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] for an invalid query and
    /// [`CoreError::Storage`] when the rows cannot be read.
    pub fn records(&self, query: &UsageQuery) -> CoreResult<Vec<UsageRecord>> {
        query.validate()?;
        let (filter, mut values) = build_filter(query)?;
        let limit_index = values.len() + 1;
        values.push(Value::Integer(i64::from(query.limit.unwrap_or(100))));
        let sql = format!(
            "SELECT {RECORD_COLUMNS} FROM usage_records{filter} \
             ORDER BY started_at_ms DESC, id DESC LIMIT ?{limit_index}"
        );

        let connection = self.connection();
        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| storage_error("failed to prepare the usage listing", error))?;
        let mut rows = statement
            .query(params_from_iter(values.iter()))
            .map_err(|error| storage_error("failed to read the usage listing", error))?;

        let mut records = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|error| storage_error("failed to advance the usage listing", error))?
        {
            records.push(record_from_row(row)?);
        }
        Ok(records)
    }

    /// Lists matching attempts on the blocking pool.
    ///
    /// # Errors
    ///
    /// See [`UsageStore::records`].
    pub async fn records_async(self: Arc<Self>, query: UsageQuery) -> CoreResult<Vec<UsageRecord>> {
        run_blocking_result("usage.list", move || self.records(&query)).await
    }

    /// Loads every stored attempt of one logical request, ordered by attempt.
    ///
    /// This is the request-level view: aggregates answer "how much", this answers
    /// "what happened" (one row per retry or fallback).
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Storage`] when the rows cannot be read or decoded.
    pub fn attempts_for(&self, request_id: RequestId) -> CoreResult<Vec<UsageRecord>> {
        let connection = self.connection();
        let mut statement = connection
            .prepare(&format!(
                "SELECT {RECORD_COLUMNS} FROM usage_records \
                 WHERE request_id = ?1 ORDER BY attempt ASC, id ASC"
            ))
            .map_err(|error| storage_error("failed to prepare the request lookup", error))?;
        let mut rows = statement
            .query(params![request_id.to_string()])
            .map_err(|error| storage_error("failed to read the request attempts", error))?;

        let mut records = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|error| storage_error("failed to advance the request attempts", error))?
        {
            records.push(record_from_row(row)?);
        }
        Ok(records)
    }

    /// Loads one request's attempts on the blocking pool.
    ///
    /// # Errors
    ///
    /// See [`UsageStore::attempts_for`].
    pub async fn attempts_for_async(
        self: Arc<Self>,
        request_id: RequestId,
    ) -> CoreResult<Vec<UsageRecord>> {
        run_blocking_result("usage.request", move || self.attempts_for(request_id)).await
    }

    /// Deletes records that started before `cutoff`.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] for pre-epoch cutoffs and
    /// [`CoreError::Storage`] for a database failure.
    pub fn purge_before(&self, cutoff: SystemTime) -> CoreResult<u64> {
        let cutoff_ms = system_time_millis("usage.cutoff", cutoff)?;
        let connection = self.connection();
        connection
            .execute(
                "DELETE FROM usage_records WHERE started_at_ms < ?1",
                params![cutoff_ms],
            )
            .map(|deleted| u64::try_from(deleted).unwrap_or(u64::MAX))
            .map_err(|error| storage_error("failed to purge usage records", error))
    }

    fn connection(&self) -> MutexGuard<'_, Connection> {
        match self.connection.lock() {
            Ok(guard) => guard,
            // The connection itself stays usable after a panic; recovering keeps
            // statistics from turning an unrelated panic into a permanent outage.
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

/// Applies the schema migrations described by `PRAGMA user_version`.
fn migrate(connection: &mut Connection) -> CoreResult<()> {
    let current: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| storage_error("failed to read the usage schema version", error))?;
    if current > USAGE_SCHEMA_VERSION {
        return Err(CoreError::storage(
            StorageArea::Metadata,
            format!(
                "usage database schema v{current} is newer than the supported v{USAGE_SCHEMA_VERSION}"
            ),
            None,
        ));
    }
    if current == USAGE_SCHEMA_VERSION {
        return Ok(());
    }

    let transaction = connection
        .transaction()
        .map_err(|error| storage_error("failed to start the usage migration", error))?;
    transaction
        .execute_batch(SCHEMA_V1)
        .map_err(|error| storage_error("failed to apply the usage schema", error))?;
    transaction
        .pragma_update(None, "user_version", USAGE_SCHEMA_VERSION)
        .map_err(|error| storage_error("failed to record the usage schema version", error))?;
    transaction
        .commit()
        .map_err(|error| storage_error("failed to commit the usage migration", error))
}

/// Builds the `WHERE` clause and its positional values.
fn build_filter(query: &UsageQuery) -> CoreResult<(String, Vec<Value>)> {
    let mut clauses: Vec<&str> = Vec::new();
    let mut values: Vec<Value> = Vec::new();

    if let Some(from) = query.from {
        clauses.push("started_at_ms >= ?");
        values.push(Value::Integer(system_time_millis("usage.from", from)?));
    }
    if let Some(to) = query.to {
        clauses.push("started_at_ms <= ?");
        values.push(Value::Integer(system_time_millis("usage.to", to)?));
    }
    if let Some(provider) = &query.provider_id {
        clauses.push("provider_id = ?");
        values.push(Value::Text(provider.to_string()));
    }
    if let Some(model) = &query.model_id {
        clauses.push("model_id = ?");
        values.push(Value::Text(model.clone()));
    }
    if let Some(feature) = &query.feature {
        clauses.push("feature = ?");
        values.push(Value::Text(feature.clone()));
    }
    if let Some(session) = query.session_id {
        clauses.push("session_id = ?");
        values.push(Value::Text(session.to_string()));
    }
    if let Some(request) = query.request_id {
        clauses.push("request_id = ?");
        values.push(Value::Text(request.to_string()));
    }
    if let Some(status) = query.status {
        clauses.push("status = ?");
        values.push(Value::Text(status.as_str().to_owned()));
    }

    let filter = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    Ok((filter, values))
}

/// SQL expression producing the bucket key, or `None` for a flat summary.
fn group_expression(group_by: UsageGroupBy) -> Option<&'static str> {
    match group_by {
        UsageGroupBy::Total => None,
        UsageGroupBy::Provider => Some("COALESCE(provider_id, '(unknown)')"),
        UsageGroupBy::Model => Some("COALESCE(model_id, '(unknown)')"),
        UsageGroupBy::Feature => Some("COALESCE(feature, '(unattributed)')"),
        UsageGroupBy::Session => Some("COALESCE(session_id, '(none)')"),
        UsageGroupBy::Day => Some("strftime('%Y-%m-%d', started_at_ms / 1000, 'unixepoch')"),
    }
}

fn read_summary(
    connection: &Connection,
    filter: &str,
    values: &[Value],
) -> CoreResult<UsageSummary> {
    let sql = format!("SELECT {SUMMARY_COLUMNS} FROM usage_records{filter}");
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| storage_error("failed to prepare the usage summary", error))?;
    let mut rows = statement
        .query(params_from_iter(values.iter()))
        .map_err(|error| storage_error("failed to read the usage summary", error))?;
    match rows
        .next()
        .map_err(|error| storage_error("failed to advance the usage summary", error))?
    {
        Some(row) => summary_from_row(row, 0)
            .map_err(|error| storage_error("failed to decode the usage summary", error)),
        None => Ok(UsageSummary::default()),
    }
}

fn read_buckets(
    connection: &Connection,
    expression: &str,
    filter: &str,
    values: &[Value],
    limit: Option<u32>,
) -> CoreResult<Vec<UsageBucket>> {
    let limit = i64::from(limit.unwrap_or(100));
    let sql = format!(
        "SELECT {expression} AS bucket, {SUMMARY_COLUMNS} FROM usage_records{filter} \
         GROUP BY bucket ORDER BY bucket LIMIT ?{}",
        values.len() + 1
    );
    let mut bound = values.to_vec();
    bound.push(Value::Integer(limit));

    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| storage_error("failed to prepare the usage buckets", error))?;
    let mut rows = statement
        .query(params_from_iter(bound.iter()))
        .map_err(|error| storage_error("failed to read the usage buckets", error))?;

    let mut buckets = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|error| storage_error("failed to advance the usage buckets", error))?
    {
        let key: String = row
            .get(0)
            .map_err(|error| storage_error("failed to decode a usage bucket key", error))?;
        let summary = summary_from_row(row, 1)
            .map_err(|error| storage_error("failed to decode a usage bucket", error))?;
        buckets.push(UsageBucket { key, summary });
    }
    Ok(buckets)
}

/// Decodes one stored attempt back into a [`UsageRecord`].
fn record_from_row(row: &SqlRow<'_>) -> CoreResult<UsageRecord> {
    let request_id: String = column_value(row, 0)?;
    let trace_id: String = column_value(row, 1)?;
    let session_id: Option<String> = column_value(row, 2)?;
    let feature: Option<String> = column_value(row, 3)?;
    let provider_id: Option<String> = column_value(row, 4)?;
    let model_id: Option<String> = column_value(row, 5)?;
    let attempt: i64 = column_value(row, 6)?;
    let started_at_ms: i64 = column_value(row, 7)?;
    let latency_ms: i64 = column_value(row, 8)?;
    let status: String = column_value(row, 9)?;
    let error_code: Option<String> = column_value(row, 10)?;
    let error_message: Option<String> = column_value(row, 11)?;
    let token_input: Option<i64> = column_value(row, 12)?;
    let token_output: Option<i64> = column_value(row, 13)?;
    let token_total: Option<i64> = column_value(row, 14)?;
    let token_reasoning: Option<i64> = column_value(row, 15)?;
    let token_cached: Option<i64> = column_value(row, 16)?;
    let token_source: Option<String> = column_value(row, 17)?;
    let request_bytes: i64 = column_value(row, 18)?;
    let response_bytes: i64 = column_value(row, 19)?;

    let status = RequestStatus::from_label(&status).ok_or_else(|| {
        CoreError::storage(
            StorageArea::Metadata,
            format!("unknown request status `{status}` in the usage database"),
            None,
        )
    })?;
    let provider_id = match provider_id {
        Some(value) => Some(ProviderId::new(value)?),
        None => None,
    };

    Ok(UsageRecord {
        request_id: RequestId::parse(&request_id)?,
        trace_id: TraceId::parse(&trace_id)?,
        session_id: match session_id {
            Some(value) => Some(SessionId::parse(&value)?),
            None => None,
        },
        feature,
        provider_id,
        model_id,
        attempt: u32::try_from(attempt.max(1)).unwrap_or(u32::MAX),
        started_at: UNIX_EPOCH
            .checked_add(Duration::from_millis(
                u64::try_from(started_at_ms).unwrap_or(0),
            ))
            .unwrap_or(UNIX_EPOCH),
        latency: Duration::from_millis(u64::try_from(latency_ms).unwrap_or(0)),
        status,
        error_code,
        error_message,
        tokens: token_usage(
            token_input,
            token_output,
            token_total,
            token_reasoning,
            token_cached,
            token_source.as_deref(),
        ),
        traffic: TrafficStats::new(
            u64::try_from(request_bytes).unwrap_or(0),
            u64::try_from(response_bytes).unwrap_or(0),
        ),
    })
}

fn column_value<T: rusqlite::types::FromSql>(row: &SqlRow<'_>, index: usize) -> CoreResult<T> {
    row.get(index)
        .map_err(|error| storage_error("failed to decode a usage record", error))
}

/// Rebuilds token usage from nullable columns.
///
/// A row without any token column yields `None`; when the source label is
/// missing the value is treated as an estimate, so a corrupt row can never be
/// presented as provider-measured usage.
fn token_usage(
    input: Option<i64>,
    output: Option<i64>,
    total: Option<i64>,
    reasoning: Option<i64>,
    cached: Option<i64>,
    source: Option<&str>,
) -> Option<TokenUsage> {
    if input.is_none() && output.is_none() && total.is_none() {
        return None;
    }
    let input = input.map(token_count).unwrap_or(0);
    let output = output.map(token_count).unwrap_or(0);
    Some(TokenUsage {
        input,
        output,
        total: total
            .map(token_count)
            .unwrap_or_else(|| input.saturating_add(output)),
        reasoning: reasoning.map(token_count),
        cached: cached.map(token_count),
        source: source
            .and_then(TokenSource::from_label)
            .unwrap_or(TokenSource::Estimated),
    })
}

fn token_count(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

fn summary_from_row(row: &SqlRow<'_>, offset: usize) -> rusqlite::Result<UsageSummary> {
    Ok(UsageSummary {
        attempts: column(row, offset)?,
        requests: column(row, offset + 1)?,
        success: column(row, offset + 2)?,
        degraded: column(row, offset + 3)?,
        failed: column(row, offset + 4)?,
        cancelled: column(row, offset + 5)?,
        timeout: column(row, offset + 6)?,
        input_tokens: column(row, offset + 7)?,
        output_tokens: column(row, offset + 8)?,
        total_tokens: column(row, offset + 9)?,
        actual_tokens: column(row, offset + 10)?,
        estimated_tokens: column(row, offset + 11)?,
        reasoning_tokens: column(row, offset + 12)?,
        cached_tokens: column(row, offset + 13)?,
        request_bytes: column(row, offset + 14)?,
        response_bytes: column(row, offset + 15)?,
        latency_total_ms: column(row, offset + 16)?,
        latency_max_ms: column(row, offset + 17)?,
    })
}

fn column(row: &SqlRow<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    Ok(u64::try_from(value).unwrap_or(0))
}

fn storage_error(message: &str, error: rusqlite::Error) -> CoreError {
    CoreError::storage(StorageArea::Metadata, message, Some(Box::new(error)))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;
    use crate::ids::{RequestId, SessionId, TraceId};
    use crate::provider::config::ProviderId;
    use crate::provider::response::{RequestStatus, TransportStats};
    use crate::usage::record::{TokenSource, UsageGroupBy};
    use crate::usage::{TokenUsage, TrafficStats};

    fn layout() -> (tempfile::TempDir, StorageLayout) {
        let directory = tempfile::tempdir().unwrap();
        let layout = StorageLayout::derive(directory.path().join("ai-core")).unwrap();
        (directory, layout)
    }

    fn record(
        provider: &str,
        model: &str,
        feature: &str,
        status: RequestStatus,
        started_at: SystemTime,
    ) -> UsageRecord {
        UsageRecord {
            request_id: RequestId::new(),
            trace_id: TraceId::new(),
            session_id: Some(SessionId::new()),
            feature: Some(feature.to_owned()),
            provider_id: Some(ProviderId::new(provider).unwrap()),
            model_id: Some(model.to_owned()),
            attempt: 1,
            started_at,
            latency: Duration::from_millis(12),
            status,
            error_code: None,
            error_message: None,
            tokens: Some(TokenUsage {
                input: 10,
                output: 5,
                total: 15,
                reasoning: None,
                cached: None,
                source: TokenSource::Actual,
            }),
            traffic: TrafficStats::from_transport(TransportStats {
                request_bytes: 100,
                response_bytes: 400,
            }),
        }
    }

    #[test]
    fn creates_and_migrates_the_schema() {
        let (_directory, layout) = layout();
        let store = UsageStore::open_in_layout(&layout).unwrap();
        assert_eq!(store.schema_version().unwrap(), USAGE_SCHEMA_VERSION);
        assert_eq!(store.count().unwrap(), 0);
        assert!(store.path().starts_with(layout.metadata()));
    }

    #[test]
    fn reopened_store_keeps_history() {
        let (_directory, layout) = layout();
        {
            let store = UsageStore::open_in_layout(&layout).unwrap();
            store
                .insert(&record(
                    "openai",
                    "gpt-4o-mini",
                    "lookup",
                    RequestStatus::Success,
                    SystemTime::now(),
                ))
                .unwrap();
            assert_eq!(store.count().unwrap(), 1);
        }
        let reopened = UsageStore::open_in_layout(&layout).unwrap();
        assert_eq!(reopened.count().unwrap(), 1);
        assert_eq!(reopened.schema_version().unwrap(), USAGE_SCHEMA_VERSION);
    }

    #[test]
    fn aggregation_separates_tokens_traffic_and_status() {
        let (_directory, layout) = layout();
        let store = UsageStore::open_in_layout(&layout).unwrap();
        let now = SystemTime::now();

        store
            .insert_many(&[
                record(
                    "openai",
                    "gpt-4o-mini",
                    "lookup",
                    RequestStatus::Success,
                    now,
                ),
                record(
                    "openai",
                    "gpt-4o-mini",
                    "lookup",
                    RequestStatus::Degraded,
                    now,
                ),
                record("anthropic", "claude", "chat", RequestStatus::Failed, now),
            ])
            .unwrap();

        let aggregate = store.query(&UsageQuery::default()).unwrap();
        let total = aggregate.total;
        assert_eq!(total.attempts, 3);
        assert_eq!(total.requests, 3);
        assert_eq!(total.success, 1);
        assert_eq!(total.degraded, 1);
        assert_eq!(total.failed, 1);
        assert_eq!(total.total_tokens, 45);
        assert_eq!(total.actual_tokens, 45);
        assert_eq!(total.estimated_tokens, 0);
        assert_eq!(total.request_bytes, 300);
        assert_eq!(total.response_bytes, 1_200);
        assert_eq!(total.total_bytes(), 1_500);
        assert!(aggregate.buckets.is_empty());
    }

    #[test]
    fn estimated_and_actual_tokens_stay_separate() {
        let (_directory, layout) = layout();
        let store = UsageStore::open_in_layout(&layout).unwrap();
        let now = SystemTime::now();

        let mut estimated = record("mock", "m", "lookup", RequestStatus::Success, now);
        estimated.tokens = Some(TokenUsage {
            input: 4,
            output: 2,
            total: 6,
            reasoning: None,
            cached: None,
            source: TokenSource::Estimated,
        });
        let actual = record("mock", "m", "lookup", RequestStatus::Success, now);
        store.insert_many(&[estimated, actual]).unwrap();

        let total = store.query(&UsageQuery::default()).unwrap().total;
        assert_eq!(total.total_tokens, 21);
        assert_eq!(total.actual_tokens, 15);
        assert_eq!(total.estimated_tokens, 6);
    }

    #[test]
    fn groups_by_provider_model_feature_session_and_day() {
        let (_directory, layout) = layout();
        let store = UsageStore::open_in_layout(&layout).unwrap();
        // 2024-01-01T00:00:00Z and the following day.
        let day_one = UNIX_EPOCH + Duration::from_millis(1_704_067_200_000);
        let day_two = day_one + Duration::from_secs(86_400);

        let mut first = record("openai", "gpt", "lookup", RequestStatus::Success, day_one);
        first.session_id = Some(SessionId::new());
        let session = first.session_id;
        let mut second = record(
            "anthropic",
            "claude",
            "chat",
            RequestStatus::Success,
            day_two,
        );
        second.session_id = session;
        store.insert_many(&[first, second]).unwrap();

        for (group_by, expected) in [
            (UsageGroupBy::Provider, vec!["anthropic", "openai"]),
            (UsageGroupBy::Model, vec!["claude", "gpt"]),
            (UsageGroupBy::Feature, vec!["chat", "lookup"]),
            (UsageGroupBy::Day, vec!["2024-01-01", "2024-01-02"]),
        ] {
            let query = UsageQuery {
                group_by,
                ..UsageQuery::default()
            };
            let aggregate = store.query(&query).unwrap();
            assert_eq!(aggregate.total.attempts, 2);
            let keys: Vec<&str> = aggregate
                .buckets
                .iter()
                .map(|bucket| bucket.key.as_str())
                .collect();
            assert_eq!(keys, expected, "unexpected buckets for {group_by:?}");
        }

        let query = UsageQuery {
            group_by: UsageGroupBy::Session,
            session_id: session,
            ..UsageQuery::default()
        };
        let aggregate = store.query(&query).unwrap();
        assert_eq!(aggregate.buckets.len(), 1);
        assert_eq!(aggregate.buckets[0].summary.attempts, 2);
    }

    #[test]
    fn filters_by_status_and_request() {
        let (_directory, layout) = layout();
        let store = UsageStore::open_in_layout(&layout).unwrap();
        let now = SystemTime::now();
        let ok = record("openai", "gpt", "lookup", RequestStatus::Success, now);
        let failed = record("openai", "gpt", "lookup", RequestStatus::Timeout, now);
        let request_id = failed.request_id;
        store.insert_many(&[ok, failed]).unwrap();

        let by_status = UsageQuery {
            status: Some(RequestStatus::Timeout),
            ..UsageQuery::default()
        };
        assert_eq!(store.query(&by_status).unwrap().total.attempts, 1);

        let by_request = UsageQuery {
            request_id: Some(request_id),
            ..UsageQuery::default()
        };
        let aggregate = store.query(&by_request).unwrap();
        assert_eq!(aggregate.total.attempts, 1);
        assert_eq!(aggregate.total.timeout, 1);
        assert_eq!(aggregate.total.unsuccessful(), 1);
    }

    #[test]
    fn purges_old_records_only() {
        let (_directory, layout) = layout();
        let store = UsageStore::open_in_layout(&layout).unwrap();
        let old = UNIX_EPOCH + Duration::from_millis(1_704_067_200_000);
        let new = old + Duration::from_secs(86_400 * 30);
        store
            .insert_many(&[
                record("openai", "gpt", "lookup", RequestStatus::Success, old),
                record("openai", "gpt", "lookup", RequestStatus::Success, new),
            ])
            .unwrap();

        let deleted = store
            .purge_before(old + Duration::from_secs(86_400))
            .unwrap();
        assert_eq!(deleted, 1);
        assert_eq!(store.count().unwrap(), 1);
    }

    #[test]
    fn concurrent_writers_serialise_without_losing_rows() {
        let (_directory, layout) = layout();
        let store = Arc::new(UsageStore::open_in_layout(&layout).unwrap());
        let mut handles = Vec::new();
        for worker in 0..8 {
            let store = Arc::clone(&store);
            handles.push(std::thread::spawn(move || {
                for _ in 0..25 {
                    let provider = if worker % 2 == 0 { "openai" } else { "mock" };
                    store
                        .insert(&record(
                            provider,
                            "gpt",
                            "concurrent",
                            RequestStatus::Success,
                            SystemTime::now(),
                        ))
                        .unwrap();
                }
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(store.count().unwrap(), 200);
        assert_eq!(
            store.query(&UsageQuery::default()).unwrap().total.attempts,
            200
        );
    }

    #[test]
    fn request_lookup_returns_attempts_in_order() {
        let (_directory, layout) = layout();
        let store = UsageStore::open_in_layout(&layout).unwrap();
        let now = SystemTime::now();

        let first = record("openai", "gpt", "lookup", RequestStatus::Failed, now);
        let request_id = first.request_id;
        let mut second = first.clone();
        second.attempt = 2;
        second.status = RequestStatus::Degraded;
        second.error_code = Some("CORE_PROVIDER".to_owned());
        let mut third = first.clone();
        third.request_id = RequestId::new();
        store.insert_many(&[first, second, third]).unwrap();

        let attempts = store.attempts_for(request_id).unwrap();
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].attempt, 1);
        assert_eq!(attempts[1].attempt, 2);
        assert_eq!(attempts[1].status, RequestStatus::Degraded);
        assert_eq!(attempts[1].error_code.as_deref(), Some("CORE_PROVIDER"));
        assert_eq!(
            attempts[0].tokens.map(|tokens| tokens.source),
            Some(TokenSource::Actual)
        );
        assert_eq!(attempts[0].traffic.total(), 500);
        assert_eq!(attempts[0].request_id, request_id);
    }

    #[test]
    fn invalid_records_abort_the_whole_batch() {
        let (_directory, layout) = layout();
        let store = UsageStore::open_in_layout(&layout).unwrap();
        let valid = record(
            "openai",
            "gpt",
            "lookup",
            RequestStatus::Success,
            SystemTime::now(),
        );
        let mut invalid = valid.clone();
        invalid.attempt = 0;

        assert!(store.insert_many(&[valid, invalid]).is_err());
        assert_eq!(store.count().unwrap(), 0);
    }
}
