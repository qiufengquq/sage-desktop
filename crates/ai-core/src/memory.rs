//! AI-owned, long-lived conversation memory.
//!
//! # Responsibility
//!
//! Persist the durable part of a conversation — one row per message, grouped by
//! session — and expose the explicit read/prune interface the context assembler
//! replays from.
//!
//! Retrieval is full-text: a `memory_messages_fts` FTS5 table indexes message
//! content through triggers, so [`MemoryStore::search`] answers with bm25
//! relevance while writes, prunes and cascades keep the index consistent without
//! extra bookkeeping. A per-session `summary` column stores a host-generated
//! compaction summary (the framework never calls a model on its own).
//!
//! Similarity search is exact and dependency-free: a host stores vectors with
//! [`MemoryStore::store_embedding`] (the framework never calls an embeddings
//! endpoint on its own) and [`MemoryStore::search_similar`] scans the matching
//! rows with cosine similarity. The scan is linear, which is honest for a
//! conversation-sized table; a host with millions of rows should keep a real
//! vector index outside the framework.
//!
//! The two retrievals answer different questions: FTS matches words, vectors
//! match meaning. Neither replaces the other.
//!
//! # Boundaries
//!
//! * The database lives in its own area ([`StorageArea::Memory`] →
//!   `ai-memory/memory.sqlite3`) and is never the client's business database and
//!   never the cache: memory is durable and backed up, cache is disposable.
//! * Stored rows contain private user content, so nothing here logs payloads —
//!   only session identifiers, counts and durations.
//! * `rusqlite` is synchronous: async callers use the `*_async` variants, which
//!   run the same code through [`run_blocking`].
//!
//! # Schema
//!
//! `PRAGMA user_version` records the schema version; migrations run inside a
//! transaction so an interrupted upgrade rolls back atomically.
//!
//! [`run_blocking`]: crate::async_utils::run_blocking

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::SystemTime;

use rusqlite::{params, Connection, OptionalExtension, Row as SqlRow};

use crate::async_utils::run_blocking_result;
use crate::error::{CoreError, CoreResult, StorageArea};
use crate::ids::SessionId;
use crate::storage::{system_time_from_millis, system_time_millis, StorageLayout};

/// File name of the memory database inside `ai-memory/`.
pub const MEMORY_DB_FILE: &str = "memory.sqlite3";
/// Schema version this build creates and understands.
pub const MEMORY_SCHEMA_VERSION: i64 = 3;

const SCHEMA_V1: &str = r#"
CREATE TABLE memory_sessions (
    session_id     TEXT    PRIMARY KEY,
    created_at_ms  INTEGER NOT NULL,
    updated_at_ms  INTEGER NOT NULL,
    message_count  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE memory_messages (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id    TEXT    NOT NULL
                  REFERENCES memory_sessions (session_id) ON DELETE CASCADE,
    role          TEXT    NOT NULL,
    content       TEXT    NOT NULL,
    tool_call_id  TEXT,
    created_at_ms INTEGER NOT NULL
);

CREATE INDEX idx_memory_messages_session ON memory_messages (session_id, id);
CREATE INDEX idx_memory_messages_created ON memory_messages (created_at_ms);
"#;

/// v2: full-text index over message content plus a per-session summary.
///
/// The FTS table is *external content* over `memory_messages`, so the index
/// stores no duplicate payload; triggers keep it in sync for inserts, updates
/// and deletes (including the ones performed by a cascading session delete).
const SCHEMA_V2: &str = r#"
ALTER TABLE memory_sessions ADD COLUMN summary TEXT;

CREATE VIRTUAL TABLE memory_messages_fts USING fts5(
    content,
    content='memory_messages',
    content_rowid='id',
    tokenize='unicode61'
);

CREATE TRIGGER memory_messages_fts_ai AFTER INSERT ON memory_messages BEGIN
    INSERT INTO memory_messages_fts (rowid, content) VALUES (new.id, new.content);
END;

CREATE TRIGGER memory_messages_fts_ad AFTER DELETE ON memory_messages BEGIN
    INSERT INTO memory_messages_fts (memory_messages_fts, rowid, content)
    VALUES ('delete', old.id, old.content);
END;

CREATE TRIGGER memory_messages_fts_au AFTER UPDATE ON memory_messages BEGIN
    INSERT INTO memory_messages_fts (memory_messages_fts, rowid, content)
    VALUES ('delete', old.id, old.content);
    INSERT INTO memory_messages_fts (rowid, content) VALUES (new.id, new.content);
END;
"#;

/// v3: host-supplied embedding vectors, one row per (message, model).
///
/// The framework never generates the vectors itself (that needs an embeddings
/// call); it only stores and compares them. A cascading message/session delete
/// removes its rows, so a pruned turn never leaves an orphan vector behind.
const SCHEMA_V3: &str = r#"
CREATE TABLE memory_embeddings (
    message_id    INTEGER NOT NULL
                  REFERENCES memory_messages (id) ON DELETE CASCADE,
    model         TEXT    NOT NULL,
    dimensions    INTEGER NOT NULL,
    vector        BLOB    NOT NULL,
    created_at_ms INTEGER NOT NULL,
    PRIMARY KEY (message_id, model)
);

CREATE INDEX idx_memory_embeddings_model ON memory_embeddings (model, dimensions);
"#;

const MESSAGE_COLUMNS: &str = "id, session_id, role, content, tool_call_id, created_at_ms";
/// Same columns, qualified for the join against the FTS index.
const MESSAGE_COLUMNS_PREFIXED: &str =
    "m.id, m.session_id, m.role, m.content, m.tool_call_id, m.created_at_ms";
const SESSION_COLUMNS: &str = "session_id, created_at_ms, updated_at_ms, message_count, summary";

/// Turns free text into an FTS5 expression built only from quoted tokens.
///
/// Punctuation, `*`, `NEAR`, `-` and `OR` therefore never reach the query parser,
/// which is what keeps a user-typed string from turning into a syntax error.
fn fts_expression(raw: &str) -> Option<String> {
    let tokens: Vec<String> = raw
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect();
    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" AND "))
    }
}

/// Encodes a vector as little-endian `f32` components.
fn encode_vector(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Decodes a stored blob back into components.
fn decode_vector(bytes: &[u8]) -> CoreResult<Vec<f32>> {
    if bytes.len() % 4 != 0 {
        return Err(CoreError::storage(
            StorageArea::Memory,
            format!(
                "stored embedding has {} bytes, which is not a whole number of f32 components",
                bytes.len()
            ),
            None,
        ));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

/// Cosine similarity, or `None` when the shapes differ or a vector is zero.
fn cosine_similarity(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.len() != right.len() || left.is_empty() {
        return None;
    }
    let mut dot = 0.0_f32;
    let mut left_norm = 0.0_f32;
    let mut right_norm = 0.0_f32;
    for (a, b) in left.iter().zip(right) {
        if !a.is_finite() || !b.is_finite() {
            return None;
        }
        dot += a * b;
        left_norm += a * a;
        right_norm += b * b;
    }
    if left_norm == 0.0 || right_norm == 0.0 {
        return None;
    }
    let score = dot / (left_norm.sqrt() * right_norm.sqrt());
    score.is_finite().then_some(score)
}

/// Validates and converts a search limit.
fn search_limit(limit: Option<usize>) -> CoreResult<i64> {
    if limit == Some(0) {
        return Err(CoreError::invalid_argument(
            "memory.limit",
            "must be greater than zero",
        ));
    }
    Ok(i64::try_from(limit.unwrap_or(usize::MAX)).unwrap_or(i64::MAX))
}

/// Author of a stored message.
///
/// Deliberately independent of the provider layer so memory stays a storage
/// boundary; the context assembler performs the mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryRole {
    /// System instruction.
    System,
    /// End-user input.
    User,
    /// Model output.
    Assistant,
    /// Tool result, usually correlated with `tool_call_id`.
    Tool,
}

impl MemoryRole {
    /// Stable database label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }

    /// Parses a stored label, returning `None` for an unknown value.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "system" => Some(Self::System),
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            "tool" => Some(Self::Tool),
            _ => None,
        }
    }
}

impl fmt::Display for MemoryRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for MemoryRole {
    type Err = CoreError;

    fn from_str(value: &str) -> CoreResult<Self> {
        Self::from_label(value.trim()).ok_or_else(|| {
            CoreError::invalid_argument(
                "memory.role",
                format!("unknown role `{value}` (expected system, user, assistant or tool)"),
            )
        })
    }
}

/// One message to append to a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryAppend {
    /// Author of the message.
    pub role: MemoryRole,
    /// Message text. May be empty for an assistant turn that only calls tools.
    pub content: String,
    /// Call id a [`MemoryRole::Tool`] message answers.
    pub tool_call_id: Option<String>,
}

impl MemoryAppend {
    /// Builds a text message.
    #[must_use]
    pub fn new(role: MemoryRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_call_id: None,
        }
    }

    /// Correlates the message with the tool call it answers.
    #[must_use]
    pub fn with_tool_call_id(mut self, call_id: impl Into<String>) -> Self {
        self.tool_call_id = Some(call_id.into());
        self
    }
}

/// A persisted message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryMessage {
    /// Monotonic row id, usable as a stable cursor.
    pub id: i64,
    /// Owning session.
    pub session_id: SessionId,
    /// Author of the message.
    pub role: MemoryRole,
    /// Message text.
    pub content: String,
    /// Call id this message answers, when it is a tool result.
    pub tool_call_id: Option<String>,
    /// Insertion timestamp.
    pub created_at: SystemTime,
}

/// Per-session counters, without loading the messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySession {
    /// Session identifier.
    pub session_id: SessionId,
    /// First insertion timestamp.
    pub created_at: SystemTime,
    /// Last insertion or prune timestamp.
    pub updated_at: SystemTime,
    /// Number of stored messages.
    pub message_count: u64,
    /// Host-generated compaction summary, when the host wrote one.
    pub summary: Option<String>,
}

/// A message and its similarity to a query vector.
#[derive(Debug, Clone, PartialEq)]
pub struct SimilarMessage {
    /// The matched message.
    pub message: MemoryMessage,
    /// Cosine similarity in `[-1, 1]`; higher is more similar.
    pub score: f32,
}

/// SQLite-backed conversation memory.
///
/// The type owns its connection, so it is shared as `Arc<MemoryStore>` rather
/// than cloned; every method is blocking, with `*_async` siblings for callers on
/// the async runtime.
pub struct MemoryStore {
    path: PathBuf,
    connection: Mutex<Connection>,
}

impl fmt::Debug for MemoryStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryStore")
            .field("path", &self.path)
            .finish()
    }
}

impl MemoryStore {
    /// Opens (creating if needed) the memory database at `path` and migrates it.
    ///
    /// Passing `":memory:"` opens a private in-process database, which is useful
    /// for tests and for hosts that explicitly do not want persistence.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Storage`] with [`StorageArea::Memory`] when the
    /// directory cannot be created, the database cannot be opened, or the stored
    /// schema is newer than this build supports.
    pub fn open(path: impl Into<PathBuf>) -> CoreResult<Self> {
        let path = path.into();
        if path.as_os_str().is_empty() {
            return Err(CoreError::invalid_argument(
                "memory.path",
                "must not be empty",
            ));
        }
        let in_memory = path == Path::new(IN_MEMORY_PATH);
        if !in_memory {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    CoreError::storage(
                        StorageArea::Memory,
                        format!("failed to create `{}`", parent.display()),
                        Some(Box::new(error)),
                    )
                })?;
            }
        }

        let mut connection = Connection::open(&path)
            .map_err(|error| memory_error("failed to open the memory database", error))?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;\n\
                 PRAGMA synchronous = NORMAL;\n\
                 PRAGMA busy_timeout = 5000;\n\
                 PRAGMA foreign_keys = ON;",
            )
            .map_err(|error| memory_error("failed to configure the memory database", error))?;
        migrate(&mut connection)?;
        Ok(Self {
            path,
            connection: Mutex::new(connection),
        })
    }

    /// Opens the memory database inside a storage layout's `ai-memory/` area.
    ///
    /// # Errors
    ///
    /// See [`MemoryStore::open`].
    pub fn open_in_layout(layout: &StorageLayout) -> CoreResult<Self> {
        Self::open(layout.memory().join(MEMORY_DB_FILE))
    }

    /// Opens the memory database on the blocking pool.
    ///
    /// # Errors
    ///
    /// See [`MemoryStore::open`], plus [`CoreError::Concurrency`] when the
    /// blocking task cannot be joined.
    pub async fn open_in_layout_async(layout: &StorageLayout) -> CoreResult<Self> {
        let layout = layout.clone();
        run_blocking_result("memory.open", move || Self::open_in_layout(&layout)).await
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
            .map_err(|error| memory_error("failed to read the memory schema version", error))
    }

    /// Appends one message, creating the session row on first use.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Storage`] when the transaction fails.
    pub fn append(&self, session: SessionId, message: MemoryAppend) -> CoreResult<MemoryMessage> {
        let now = system_time_millis("memory.created_at", SystemTime::now())?;
        let mut connection = self.connection();
        let transaction = connection
            .transaction()
            .map_err(|error| memory_error("failed to start a memory transaction", error))?;

        transaction
            .execute(
                "INSERT INTO memory_sessions (session_id, created_at_ms, updated_at_ms, message_count) \
                 VALUES (?1, ?2, ?2, 0) ON CONFLICT (session_id) DO NOTHING",
                params![session.to_string(), now],
            )
            .map_err(|error| memory_error("failed to create the memory session", error))?;
        transaction
            .execute(
                "INSERT INTO memory_messages (session_id, role, content, tool_call_id, created_at_ms) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    session.to_string(),
                    message.role.as_str(),
                    message.content,
                    message.tool_call_id,
                    now
                ],
            )
            .map_err(|error| memory_error("failed to append a memory message", error))?;
        let id = transaction.last_insert_rowid();
        refresh_session(&transaction, session, now)?;
        transaction
            .commit()
            .map_err(|error| memory_error("failed to commit a memory message", error))?;

        Ok(MemoryMessage {
            id,
            session_id: session,
            role: message.role,
            content: message.content,
            tool_call_id: message.tool_call_id,
            created_at: system_time_from_millis(now),
        })
    }

    /// Appends one message on the blocking pool.
    ///
    /// # Errors
    ///
    /// See [`MemoryStore::append`].
    pub async fn append_async(
        self: Arc<Self>,
        session: SessionId,
        message: MemoryAppend,
    ) -> CoreResult<MemoryMessage> {
        run_blocking_result("memory.append", move || self.append(session, message)).await
    }

    /// Loads the newest `limit` messages of a session, oldest first.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] when `limit` is zero, and
    /// [`CoreError::Storage`] when the rows cannot be read or decoded.
    pub fn history(
        &self,
        session: SessionId,
        limit: Option<usize>,
    ) -> CoreResult<Vec<MemoryMessage>> {
        if limit == Some(0) {
            return Err(CoreError::invalid_argument(
                "memory.limit",
                "must be greater than zero",
            ));
        }
        let limit = i64::try_from(limit.unwrap_or(usize::MAX)).unwrap_or(i64::MAX);
        let connection = self.connection();
        let mut statement = connection
            .prepare(&format!(
                "SELECT {MESSAGE_COLUMNS} FROM memory_messages \
                 WHERE session_id = ?1 ORDER BY id DESC LIMIT ?2"
            ))
            .map_err(|error| memory_error("failed to prepare the memory history", error))?;
        let mut rows = statement
            .query(params![session.to_string(), limit])
            .map_err(|error| memory_error("failed to read the memory history", error))?;

        let mut messages = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|error| memory_error("failed to advance the memory history", error))?
        {
            messages.push(message_from_row(row)?);
        }
        // The query reads newest first for the LIMIT to be meaningful; replay
        // order is chronological.
        messages.reverse();
        Ok(messages)
    }

    /// Loads session history on the blocking pool.
    ///
    /// # Errors
    ///
    /// See [`MemoryStore::history`].
    pub async fn history_async(
        self: Arc<Self>,
        session: SessionId,
        limit: Option<usize>,
    ) -> CoreResult<Vec<MemoryMessage>> {
        run_blocking_result("memory.history", move || self.history(session, limit)).await
    }

    /// Number of messages stored for a session.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Storage`] when the query fails.
    pub fn message_count(&self, session: SessionId) -> CoreResult<u64> {
        let connection = self.connection();
        let count: Option<i64> = connection
            .query_row(
                "SELECT message_count FROM memory_sessions WHERE session_id = ?1",
                params![session.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| memory_error("failed to read the memory session", error))?;
        Ok(count
            .map(|value| u64::try_from(value).unwrap_or(0))
            .unwrap_or(0))
    }

    /// Session counters, newest activity first.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Storage`] when the rows cannot be read or decoded.
    pub fn sessions(&self) -> CoreResult<Vec<MemorySession>> {
        let connection = self.connection();
        let mut statement = connection
            .prepare(&format!(
                "SELECT {SESSION_COLUMNS} FROM memory_sessions ORDER BY updated_at_ms DESC"
            ))
            .map_err(|error| memory_error("failed to prepare the memory sessions", error))?;
        let mut rows = statement
            .query([])
            .map_err(|error| memory_error("failed to read the memory sessions", error))?;

        let mut sessions = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|error| memory_error("failed to advance the memory sessions", error))?
        {
            sessions.push(
                session_from_row(row)
                    .map_err(|error| memory_error("failed to decode a memory session", error))?,
            );
        }
        Ok(sessions)
    }

    /// One session's counters, when it exists.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Storage`] when the row cannot be read or decoded.
    pub fn session(&self, session: SessionId) -> CoreResult<Option<MemorySession>> {
        let connection = self.connection();
        connection
            .query_row(
                &format!("SELECT {SESSION_COLUMNS} FROM memory_sessions WHERE session_id = ?1"),
                params![session.to_string()],
                session_from_row,
            )
            .optional()
            .map_err(|error| memory_error("failed to read the memory session", error))
    }

    /// Full-text search inside one session, most relevant first.
    ///
    /// The query is reduced to its alphanumeric tokens and ANDed as quoted
    /// phrases, so punctuation never turns into FTS syntax. An empty query (or
    /// one with no usable token) returns no hit instead of every row.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] when `limit` is zero and
    /// [`CoreError::Storage`] when the index cannot be queried.
    pub fn search(
        &self,
        session: SessionId,
        query: &str,
        limit: Option<usize>,
    ) -> CoreResult<Vec<MemoryMessage>> {
        let Some(expression) = fts_expression(query) else {
            return Ok(Vec::new());
        };
        let limit = search_limit(limit)?;
        let connection = self.connection();
        let mut statement = connection
            .prepare(&format!(
                "SELECT {MESSAGE_COLUMNS_PREFIXED} FROM memory_messages_fts \
                 JOIN memory_messages m ON m.id = memory_messages_fts.rowid \
                 WHERE memory_messages_fts MATCH ?1 AND m.session_id = ?2 \
                 ORDER BY rank, m.id DESC LIMIT ?3"
            ))
            .map_err(|error| memory_error("failed to prepare the memory search", error))?;
        let mut rows = statement
            .query(params![expression, session.to_string(), limit])
            .map_err(|error| memory_error("failed to search memory", error))?;

        let mut messages = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|error| memory_error("failed to advance the memory search", error))?
        {
            messages.push(message_from_row(row)?);
        }
        Ok(messages)
    }

    /// Full-text search across every session, most relevant first.
    ///
    /// # Errors
    ///
    /// See [`MemoryStore::search`].
    pub fn search_all(&self, query: &str, limit: Option<usize>) -> CoreResult<Vec<MemoryMessage>> {
        let Some(expression) = fts_expression(query) else {
            return Ok(Vec::new());
        };
        let limit = search_limit(limit)?;
        let connection = self.connection();
        let mut statement = connection
            .prepare(&format!(
                "SELECT {MESSAGE_COLUMNS_PREFIXED} FROM memory_messages_fts \
                 JOIN memory_messages m ON m.id = memory_messages_fts.rowid \
                 WHERE memory_messages_fts MATCH ?1 \
                 ORDER BY rank, m.id DESC LIMIT ?2"
            ))
            .map_err(|error| memory_error("failed to prepare the memory search", error))?;
        let mut rows = statement
            .query(params![expression, limit])
            .map_err(|error| memory_error("failed to search memory", error))?;

        let mut messages = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|error| memory_error("failed to advance the memory search", error))?
        {
            messages.push(message_from_row(row)?);
        }
        Ok(messages)
    }

    /// Searches one session on the blocking pool.
    ///
    /// # Errors
    ///
    /// See [`MemoryStore::search`].
    pub async fn search_async(
        self: Arc<Self>,
        session: SessionId,
        query: String,
        limit: Option<usize>,
    ) -> CoreResult<Vec<MemoryMessage>> {
        run_blocking_result("memory.search", move || self.search(session, &query, limit)).await
    }

    /// Stores (or replaces) the vector of one message for one embedding model.
    ///
    /// Vectors are host-supplied: the framework does not call an embeddings
    /// endpoint on its own, so a host can mix models, dimensions or even a local
    /// encoder. Non-finite components are rejected because they would make every
    /// cosine score meaningless.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] for an empty model name, an empty
    /// vector or a non-finite component, and [`CoreError::Storage`] when the row
    /// cannot be written (including an unknown message id).
    pub fn store_embedding(&self, message_id: i64, model: &str, vector: &[f32]) -> CoreResult<()> {
        let model = model.trim();
        if model.is_empty() {
            return Err(CoreError::invalid_argument(
                "memory.embedding.model",
                "must not be empty",
            ));
        }
        if vector.is_empty() {
            return Err(CoreError::invalid_argument(
                "memory.embedding.vector",
                "must contain at least one component",
            ));
        }
        if let Some(index) = vector.iter().position(|value| !value.is_finite()) {
            return Err(CoreError::invalid_argument(
                format!("memory.embedding.vector.{index}"),
                "must be a finite number",
            ));
        }
        let dimensions = i64::try_from(vector.len()).map_err(|_| {
            CoreError::invalid_argument("memory.embedding.vector", "is too wide to store")
        })?;
        let now = system_time_millis("memory.created_at", SystemTime::now())?;
        let connection = self.connection();
        connection
            .execute(
                "INSERT INTO memory_embeddings (message_id, model, dimensions, vector, created_at_ms) \
                 VALUES (?1, ?2, ?3, ?4, ?5) \
                 ON CONFLICT (message_id, model) DO UPDATE SET \
                    dimensions = excluded.dimensions, \
                    vector = excluded.vector, \
                    created_at_ms = excluded.created_at_ms",
                params![
                    message_id,
                    model,
                    dimensions,
                    encode_vector(vector),
                    now
                ],
            )
            .map(|_| ())
            .map_err(|error| memory_error("failed to store a memory embedding", error))
    }

    /// Stores a vector on the blocking pool.
    ///
    /// # Errors
    ///
    /// See [`MemoryStore::store_embedding`].
    pub async fn store_embedding_async(
        self: Arc<Self>,
        message_id: i64,
        model: String,
        vector: Vec<f32>,
    ) -> CoreResult<()> {
        run_blocking_result("memory.embedding", move || {
            self.store_embedding(message_id, &model, &vector)
        })
        .await
    }

    /// Number of stored vectors, across every session and model.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Storage`] when the query fails.
    pub fn embedding_count(&self) -> CoreResult<u64> {
        let connection = self.connection();
        connection
            .query_row("SELECT COUNT(*) FROM memory_embeddings", [], |row| {
                row.get::<_, i64>(0)
            })
            .map(|count| u64::try_from(count).unwrap_or(0))
            .map_err(|error| memory_error("failed to count memory embeddings", error))
    }

    /// Nearest neighbours of `query` inside one session, most similar first.
    ///
    /// Comparison is a brute-force cosine scan over the vectors of `model` whose
    /// width matches the query. That is exact and dependency-free; it is also
    /// documented as linear, so a host with millions of rows should keep a real
    /// vector index outside the framework.
    ///
    /// Ties are broken by ascending message id, which keeps the result
    /// deterministic.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] for an empty/non-finite query and a
    /// zero limit, and [`CoreError::Storage`] when the scan fails.
    pub fn search_similar(
        &self,
        session: SessionId,
        model: &str,
        query: &[f32],
        limit: Option<usize>,
    ) -> CoreResult<Vec<SimilarMessage>> {
        let model = model.trim();
        if model.is_empty() {
            return Err(CoreError::invalid_argument(
                "memory.embedding.model",
                "must not be empty",
            ));
        }
        if query.is_empty() {
            return Err(CoreError::invalid_argument(
                "memory.embedding.query",
                "must contain at least one component",
            ));
        }
        if let Some(index) = query.iter().position(|value| !value.is_finite()) {
            return Err(CoreError::invalid_argument(
                format!("memory.embedding.query.{index}"),
                "must be a finite number",
            ));
        }
        let limit = search_limit(limit)?;
        let dimensions = i64::try_from(query.len()).map_err(|_| {
            CoreError::invalid_argument("memory.embedding.query", "is too wide to compare")
        })?;

        let connection = self.connection();
        let mut statement = connection
            .prepare(&format!(
                "SELECT {MESSAGE_COLUMNS_PREFIXED}, e.vector FROM memory_embeddings e \
                 JOIN memory_messages m ON m.id = e.message_id \
                 WHERE m.session_id = ?1 AND e.model = ?2 AND e.dimensions = ?3"
            ))
            .map_err(|error| memory_error("failed to prepare the memory similarity scan", error))?;
        let mut rows = statement
            .query(params![session.to_string(), model, dimensions])
            .map_err(|error| memory_error("failed to scan memory embeddings", error))?;

        let mut matches = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|error| memory_error("failed to advance the memory similarity scan", error))?
        {
            let message = message_from_row(row)?;
            let stored: Vec<u8> = column(row, 6)?;
            let Ok(stored) = decode_vector(&stored) else {
                // A row written by a different build is unusable; skip it rather
                // than failing the whole search.
                continue;
            };
            let Some(score) = cosine_similarity(query, &stored) else {
                continue;
            };
            matches.push(SimilarMessage { message, score });
        }

        matches.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.message.id.cmp(&right.message.id))
        });
        matches.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        Ok(matches)
    }

    /// Nearest neighbours on the blocking pool.
    ///
    /// # Errors
    ///
    /// See [`MemoryStore::search_similar`].
    pub async fn search_similar_async(
        self: Arc<Self>,
        session: SessionId,
        model: String,
        query: Vec<f32>,
        limit: Option<usize>,
    ) -> CoreResult<Vec<SimilarMessage>> {
        run_blocking_result("memory.similar", move || {
            self.search_similar(session, &model, &query, limit)
        })
        .await
    }

    /// Stores (or clears, with `None`) the session's compaction summary.
    ///
    /// The summary is generated by the host — usually by another model round over
    /// the turns that were pruned — and is replayed by
    /// [`ContextAssembler::from_memory_with_summary`]. Writing a summary never
    /// prunes anything.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Storage`] when the update fails.
    ///
    /// [`ContextAssembler::from_memory_with_summary`]: crate::context::ContextAssembler::from_memory_with_summary
    pub fn set_summary(&self, session: SessionId, summary: Option<&str>) -> CoreResult<bool> {
        let normalized = summary.map(str::trim).filter(|text| !text.is_empty());
        let now = system_time_millis("memory.updated_at", SystemTime::now())?;
        let connection = self.connection();
        connection
            .execute(
                "UPDATE memory_sessions SET summary = ?2, updated_at_ms = ?3 \
                 WHERE session_id = ?1",
                params![session.to_string(), normalized, now],
            )
            .map(|updated| updated > 0)
            .map_err(|error| memory_error("failed to store the memory summary", error))
    }

    /// Stores a summary on the blocking pool.
    ///
    /// # Errors
    ///
    /// See [`MemoryStore::set_summary`].
    pub async fn set_summary_async(
        self: Arc<Self>,
        session: SessionId,
        summary: Option<String>,
    ) -> CoreResult<bool> {
        run_blocking_result("memory.summary", move || {
            self.set_summary(session, summary.as_deref())
        })
        .await
    }

    /// Deletes every message created before `cutoff` in one session.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] for a pre-epoch cutoff and
    /// [`CoreError::Storage`] when the delete fails.
    pub fn prune_before(&self, session: SessionId, cutoff: SystemTime) -> CoreResult<u64> {
        let cutoff_ms = system_time_millis("memory.cutoff", cutoff)?;
        self.prune(session, "created_at_ms < ?2", cutoff_ms)
    }

    /// Keeps only the newest `keep` messages of a session.
    ///
    /// This is the retention policy a host applies after a session grows past
    /// its budget. `keep == 0` is rejected: clearing a session is
    /// [`MemoryStore::delete_session`]'s job.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] when `keep` is zero and
    /// [`CoreError::Storage`] when the delete fails.
    pub fn keep_last(&self, session: SessionId, keep: usize) -> CoreResult<u64> {
        if keep == 0 {
            return Err(CoreError::invalid_argument(
                "memory.keep",
                "must be greater than zero; use delete_session to clear a session",
            ));
        }
        let keep = i64::try_from(keep).unwrap_or(i64::MAX);
        self.prune(
            session,
            "id NOT IN (SELECT id FROM memory_messages WHERE session_id = ?1 \
             ORDER BY id DESC LIMIT ?2)",
            keep,
        )
    }

    /// Deletes a session and its messages.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Storage`] when the delete fails.
    pub fn delete_session(&self, session: SessionId) -> CoreResult<bool> {
        let connection = self.connection();
        connection
            .execute(
                "DELETE FROM memory_sessions WHERE session_id = ?1",
                params![session.to_string()],
            )
            .map(|deleted| deleted > 0)
            .map_err(|error| memory_error("failed to delete the memory session", error))
    }

    fn prune(&self, session: SessionId, condition: &str, value: i64) -> CoreResult<u64> {
        let mut connection = self.connection();
        let transaction = connection
            .transaction()
            .map_err(|error| memory_error("failed to start a memory prune", error))?;
        let deleted = transaction
            .execute(
                &format!("DELETE FROM memory_messages WHERE session_id = ?1 AND {condition}"),
                params![session.to_string(), value],
            )
            .map_err(|error| memory_error("failed to prune memory messages", error))?;
        let now = system_time_millis("memory.updated_at", SystemTime::now())?;
        refresh_session(&transaction, session, now)?;
        transaction
            .commit()
            .map_err(|error| memory_error("failed to commit the memory prune", error))?;
        Ok(u64::try_from(deleted).unwrap_or(u64::MAX))
    }

    fn connection(&self) -> MutexGuard<'_, Connection> {
        match self.connection.lock() {
            Ok(guard) => guard,
            // The connection stays usable after a panic; recovering keeps memory
            // from turning an unrelated panic into a permanent outage.
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

/// Path marker for a private in-process database.
const IN_MEMORY_PATH: &str = ":memory:";

/// Recomputes a session's message count and touch timestamp.
fn refresh_session(
    transaction: &rusqlite::Transaction<'_>,
    session: SessionId,
    now: i64,
) -> CoreResult<()> {
    transaction
        .execute(
            "UPDATE memory_sessions SET \
                message_count = (SELECT COUNT(*) FROM memory_messages WHERE session_id = ?1), \
                updated_at_ms = ?2 \
             WHERE session_id = ?1",
            params![session.to_string(), now],
        )
        .map(|_| ())
        .map_err(|error| memory_error("failed to update the memory session", error))
}

/// Applies the schema migrations described by `PRAGMA user_version`.
fn migrate(connection: &mut Connection) -> CoreResult<()> {
    let current: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| memory_error("failed to read the memory schema version", error))?;
    if current > MEMORY_SCHEMA_VERSION {
        return Err(CoreError::storage(
            StorageArea::Memory,
            format!(
                "memory database schema v{current} is newer than the supported v{MEMORY_SCHEMA_VERSION}"
            ),
            None,
        ));
    }
    if current == MEMORY_SCHEMA_VERSION {
        return Ok(());
    }

    let transaction = connection
        .transaction()
        .map_err(|error| memory_error("failed to start the memory migration", error))?;
    if current < 1 {
        transaction
            .execute_batch(SCHEMA_V1)
            .map_err(|error| memory_error("failed to apply the memory schema", error))?;
    }
    if current < 2 {
        transaction
            .execute_batch(SCHEMA_V2)
            .map_err(|error| memory_error("failed to apply the memory search schema", error))?;
        // Backfill the index for rows written by v1 (a no-op on a fresh database).
        transaction
            .execute_batch(
                "INSERT INTO memory_messages_fts (memory_messages_fts) VALUES ('rebuild')",
            )
            .map_err(|error| memory_error("failed to build the memory search index", error))?;
    }
    if current < 3 {
        transaction
            .execute_batch(SCHEMA_V3)
            .map_err(|error| memory_error("failed to apply the memory embedding schema", error))?;
    }
    transaction
        .pragma_update(None, "user_version", MEMORY_SCHEMA_VERSION)
        .map_err(|error| memory_error("failed to record the memory schema version", error))?;
    transaction
        .commit()
        .map_err(|error| memory_error("failed to commit the memory migration", error))
}

fn message_from_row(row: &SqlRow<'_>) -> CoreResult<MemoryMessage> {
    let id: i64 = column(row, 0)?;
    let session_id: String = column(row, 1)?;
    let role: String = column(row, 2)?;
    let content: String = column(row, 3)?;
    let tool_call_id: Option<String> = column(row, 4)?;
    let created_at_ms: i64 = column(row, 5)?;

    let role = MemoryRole::from_label(&role).ok_or_else(|| {
        CoreError::storage(
            StorageArea::Memory,
            format!("unknown memory role `{role}`"),
            None,
        )
    })?;
    Ok(MemoryMessage {
        id,
        session_id: SessionId::parse(&session_id)?,
        role,
        content,
        tool_call_id,
        created_at: system_time_from_millis(created_at_ms),
    })
}

fn session_from_row(row: &SqlRow<'_>) -> rusqlite::Result<MemorySession> {
    let session_id: String = row.get(0)?;
    let created_at_ms: i64 = row.get(1)?;
    let updated_at_ms: i64 = row.get(2)?;
    let message_count: i64 = row.get(3)?;
    let summary: Option<String> = row.get(4)?;
    Ok(MemorySession {
        // A corrupt id is a storage fault, surfaced through the `rusqlite` error
        // channel so the caller sees one `CoreError::Storage`.
        session_id: SessionId::parse(&session_id).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        created_at: system_time_from_millis(created_at_ms),
        updated_at: system_time_from_millis(updated_at_ms),
        message_count: u64::try_from(message_count).unwrap_or(0),
        summary,
    })
}

fn column<T: rusqlite::types::FromSql>(row: &SqlRow<'_>, index: usize) -> CoreResult<T> {
    row.get(index)
        .map_err(|error| memory_error("failed to decode a memory row", error))
}

fn memory_error(message: &str, error: rusqlite::Error) -> CoreError {
    CoreError::storage(StorageArea::Memory, message, Some(Box::new(error)))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    fn layout() -> (tempfile::TempDir, StorageLayout) {
        let directory = tempfile::tempdir().unwrap();
        let layout = StorageLayout::derive(directory.path().join("ai-core")).unwrap();
        (directory, layout)
    }

    fn store() -> MemoryStore {
        MemoryStore::open(IN_MEMORY_PATH).unwrap()
    }

    #[test]
    fn creates_and_migrates_the_schema() {
        let (_directory, layout) = layout();
        let store = MemoryStore::open_in_layout(&layout).unwrap();
        assert_eq!(store.schema_version().unwrap(), MEMORY_SCHEMA_VERSION);
        assert!(store.path().starts_with(layout.memory()));
        assert!(store.sessions().unwrap().is_empty());
    }

    #[test]
    fn reopened_store_keeps_history() {
        let (_directory, layout) = layout();
        let session = SessionId::new();
        {
            let store = MemoryStore::open_in_layout(&layout).unwrap();
            store
                .append(session, MemoryAppend::new(MemoryRole::User, "hello"))
                .unwrap();
        }
        let reopened = MemoryStore::open_in_layout(&layout).unwrap();
        let history = reopened.history(session, None).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content, "hello");
        assert_eq!(history[0].role, MemoryRole::User);
    }

    #[test]
    fn history_is_chronological_and_limited_to_the_newest_rows() {
        let store = store();
        let session = SessionId::new();
        for turn in 0..5 {
            store
                .append(
                    session,
                    MemoryAppend::new(MemoryRole::User, format!("m{turn}")),
                )
                .unwrap();
        }
        let all = store.history(session, None).unwrap();
        assert_eq!(all.len(), 5);
        assert_eq!(all[4].content, "m4");

        let newest = store.history(session, Some(2)).unwrap();
        assert_eq!(
            newest
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>(),
            vec!["m3", "m4"]
        );
        assert!(newest.windows(2).all(|pair| pair[0].id < pair[1].id));
        assert_eq!(store.message_count(session).unwrap(), 5);
    }

    #[test]
    fn sessions_are_isolated() {
        let store = store();
        let first = SessionId::new();
        let second = SessionId::new();
        store
            .append(first, MemoryAppend::new(MemoryRole::User, "one"))
            .unwrap();
        store
            .append(second, MemoryAppend::new(MemoryRole::User, "two"))
            .unwrap();

        assert_eq!(store.history(first, None).unwrap().len(), 1);
        assert_eq!(store.history(second, None).unwrap()[0].content, "two");
        assert_eq!(store.sessions().unwrap().len(), 2);
        assert_eq!(
            store.session(first).unwrap().map(|s| s.message_count),
            Some(1)
        );
        assert!(store.session(SessionId::new()).unwrap().is_none());
    }

    #[test]
    fn tool_results_keep_their_call_id() {
        let store = store();
        let session = SessionId::new();
        let stored = store
            .append(
                session,
                MemoryAppend::new(MemoryRole::Tool, "{\"word\":\"apple\"}")
                    .with_tool_call_id("call_1"),
            )
            .unwrap();
        assert_eq!(stored.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(
            store.history(session, None).unwrap()[0]
                .tool_call_id
                .as_deref(),
            Some("call_1")
        );
    }

    #[test]
    fn keep_last_prunes_and_recounts() {
        let store = store();
        let session = SessionId::new();
        for turn in 0..4 {
            store
                .append(
                    session,
                    MemoryAppend::new(MemoryRole::Assistant, format!("a{turn}")),
                )
                .unwrap();
        }

        let deleted = store.keep_last(session, 2).unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(store.message_count(session).unwrap(), 2);
        let remaining = store.history(session, None).unwrap();
        assert_eq!(remaining[0].content, "a2");
        assert_eq!(remaining[1].content, "a3");
        assert!(store.keep_last(session, 0).is_err());
    }

    #[test]
    fn prune_before_uses_the_cutoff() {
        let store = store();
        let session = SessionId::new();
        store
            .append(session, MemoryAppend::new(MemoryRole::User, "old"))
            .unwrap();

        // A cutoff in the past matches nothing.
        let past = UNIX_EPOCH + Duration::from_secs(86_400);
        assert_eq!(store.prune_before(session, past).unwrap(), 0);
        assert_eq!(store.message_count(session).unwrap(), 1);

        // A cutoff in the future matches every stored row.
        let future = SystemTime::now() + Duration::from_secs(60);
        assert_eq!(store.prune_before(session, future).unwrap(), 1);
        assert!(store.history(session, None).unwrap().is_empty());
        assert_eq!(store.message_count(session).unwrap(), 0);
    }

    #[test]
    fn delete_session_cascades() {
        let store = store();
        let session = SessionId::new();
        store
            .append(session, MemoryAppend::new(MemoryRole::User, "bye"))
            .unwrap();

        assert!(store.delete_session(session).unwrap());
        assert!(store.history(session, None).unwrap().is_empty());
        assert_eq!(store.message_count(session).unwrap(), 0);
        assert!(!store.delete_session(session).unwrap());
    }

    #[test]
    fn epoch_cutoffs_are_rejected() {
        let Some(pre_epoch) = UNIX_EPOCH.checked_sub(Duration::from_secs(1)) else {
            // Not every platform can represent a timestamp before 1970; the check
            // is unreachable there.
            return;
        };
        let store = store();
        let result = store.prune_before(SessionId::new(), pre_epoch);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().kind(),
            crate::ErrorKind::InvalidArgument
        );
    }

    #[test]
    fn full_text_search_scopes_to_a_session() {
        let store = store();
        let first = SessionId::new();
        let second = SessionId::new();
        store
            .append(
                first,
                MemoryAppend::new(MemoryRole::User, "apple pie recipe"),
            )
            .unwrap();
        store
            .append(
                first,
                MemoryAppend::new(MemoryRole::User, "banana bread recipe"),
            )
            .unwrap();
        store
            .append(
                second,
                MemoryAppend::new(MemoryRole::User, "apple orchard visit"),
            )
            .unwrap();

        let hits = store.search(first, "apple", None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].content, "apple pie recipe");

        // Case-insensitive, multi-token AND.
        assert_eq!(store.search(first, "APPLE PIE", None).unwrap().len(), 1);
        assert!(store
            .search(first, "apple banana", None)
            .unwrap()
            .is_empty());

        let all = store.search_all("apple", None).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(store.search_all("apple", Some(1)).unwrap().len(), 1);
    }

    #[test]
    fn search_cannot_be_broken_by_fts_syntax() {
        let store = store();
        let session = SessionId::new();
        store
            .append(session, MemoryAppend::new(MemoryRole::User, "hello world"))
            .unwrap();

        for query in ["\" OR 1=1 --", "NEAR(", "*", "-", "a:b", "\"unclosed"] {
            let result = store.search(session, query, None);
            assert!(result.is_ok(), "`{query}` must not fail: {result:?}");
        }
        assert!(store.search(session, "   ", None).unwrap().is_empty());
        assert!(store.search_all("!!!", None).unwrap().is_empty());
        assert!(store.search(session, "hello", Some(0)).is_err());
    }

    #[test]
    fn pruning_and_deleting_keep_the_index_in_sync() {
        let store = store();
        let session = SessionId::new();
        store
            .append(
                session,
                MemoryAppend::new(MemoryRole::User, "obsolete apple"),
            )
            .unwrap();
        store
            .append(session, MemoryAppend::new(MemoryRole::User, "fresh banana"))
            .unwrap();
        assert_eq!(store.search(session, "apple", None).unwrap().len(), 1);

        assert_eq!(store.keep_last(session, 1).unwrap(), 1);
        assert!(store.search(session, "apple", None).unwrap().is_empty());
        assert_eq!(store.search(session, "banana", None).unwrap().len(), 1);

        store.delete_session(session).unwrap();
        assert!(store.search_all("banana", None).unwrap().is_empty());
    }

    #[test]
    fn summaries_round_trip_and_clear() {
        let store = store();
        let session = SessionId::new();
        store
            .append(session, MemoryAppend::new(MemoryRole::User, "hi"))
            .unwrap();
        assert_eq!(store.session(session).unwrap().unwrap().summary, None);

        assert!(store
            .set_summary(session, Some("  user greeted  "))
            .unwrap());
        assert_eq!(
            store.session(session).unwrap().unwrap().summary.as_deref(),
            Some("user greeted")
        );

        // A blank summary clears the column instead of storing whitespace.
        assert!(store.set_summary(session, Some("   ")).unwrap());
        assert_eq!(store.session(session).unwrap().unwrap().summary, None);
        assert!(!store.set_summary(SessionId::new(), Some("x")).unwrap());
    }

    #[test]
    fn a_v1_database_is_migrated_and_indexed() {
        let (_directory, layout) = layout();
        layout.create_directories().unwrap();
        let path = layout.memory().join(MEMORY_DB_FILE);
        let session = SessionId::new();
        {
            let connection = Connection::open(&path).unwrap();
            connection.execute_batch(SCHEMA_V1).unwrap();
            connection.pragma_update(None, "user_version", 1).unwrap();
            connection
                .execute(
                    "INSERT INTO memory_sessions (session_id, created_at_ms, updated_at_ms, message_count) \
                     VALUES (?1, 1, 1, 1)",
                    params![session.to_string()],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO memory_messages (session_id, role, content, tool_call_id, created_at_ms) \
                     VALUES (?1, 'user', 'legacy apple note', NULL, 1)",
                    params![session.to_string()],
                )
                .unwrap();
        }

        let store = MemoryStore::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), MEMORY_SCHEMA_VERSION);
        let hits = store.search(session, "legacy", None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].content, "legacy apple note");
        assert_eq!(store.session(session).unwrap().unwrap().summary, None);
    }

    #[test]
    fn embeddings_are_stored_and_ranked_by_cosine_similarity() {
        let store = store();
        let session = SessionId::new();
        let apple = store
            .append(session, MemoryAppend::new(MemoryRole::User, "apple pie"))
            .unwrap();
        let car = store
            .append(session, MemoryAppend::new(MemoryRole::User, "car engine"))
            .unwrap();
        store
            .store_embedding(apple.id, "text-embedding-3-small", &[1.0, 0.0])
            .unwrap();
        store
            .store_embedding(car.id, "text-embedding-3-small", &[0.0, 1.0])
            .unwrap();
        assert_eq!(store.embedding_count().unwrap(), 2);

        let hits = store
            .search_similar(session, "text-embedding-3-small", &[0.9, 0.1], None)
            .unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].message.id, apple.id);
        assert!(hits[0].score > hits[1].score);
        assert!(hits[0].score > 0.99);

        // Model and width must match the stored rows.
        assert!(store
            .search_similar(session, "other-model", &[1.0, 0.0], None)
            .unwrap()
            .is_empty());
        assert!(store
            .search_similar(session, "text-embedding-3-small", &[1.0, 0.0, 0.0], None)
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .search_similar(session, "text-embedding-3-small", &[1.0, 0.0], Some(1))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn similarity_search_is_scoped_to_the_session() {
        let store = store();
        let first = SessionId::new();
        let second = SessionId::new();
        let stored = store
            .append(first, MemoryAppend::new(MemoryRole::User, "private"))
            .unwrap();
        store.store_embedding(stored.id, "m", &[1.0, 0.0]).unwrap();

        assert_eq!(
            store
                .search_similar(second, "m", &[1.0, 0.0], None)
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            store
                .search_similar(first, "m", &[1.0, 0.0], None)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn storing_a_vector_replaces_it_and_rejects_bad_input() {
        let store = store();
        let session = SessionId::new();
        let message = store
            .append(session, MemoryAppend::new(MemoryRole::User, "note"))
            .unwrap();

        store.store_embedding(message.id, "m", &[1.0, 0.0]).unwrap();
        store.store_embedding(message.id, "m", &[0.0, 1.0]).unwrap();
        assert_eq!(store.embedding_count().unwrap(), 1);
        let hits = store
            .search_similar(session, "m", &[0.0, 1.0], None)
            .unwrap();
        assert!((hits[0].score - 1.0).abs() < 1e-6);

        assert!(store.store_embedding(message.id, "m", &[]).is_err());
        assert!(store.store_embedding(message.id, "m", &[f32::NAN]).is_err());
        assert!(store.store_embedding(message.id, "  ", &[1.0]).is_err());
        assert!(store
            .store_embedding(message.id + 1_000, "m", &[1.0])
            .is_err());

        assert!(store.search_similar(session, "m", &[], None).is_err());
        assert!(store
            .search_similar(session, "m", &[f32::INFINITY], None)
            .is_err());
        assert!(store
            .search_similar(session, "m", &[1.0, 0.0], Some(0))
            .is_err());
    }

    #[test]
    fn deleting_a_session_drops_its_vectors() {
        let store = store();
        let session = SessionId::new();
        let message = store
            .append(session, MemoryAppend::new(MemoryRole::User, "bye"))
            .unwrap();
        store.store_embedding(message.id, "m", &[1.0]).unwrap();
        assert_eq!(store.embedding_count().unwrap(), 1);

        store.delete_session(session).unwrap();
        assert_eq!(store.embedding_count().unwrap(), 0);
    }

    #[tokio::test]
    async fn async_variants_run_on_the_blocking_pool() {
        let (_directory, layout) = layout();
        let store = Arc::new(MemoryStore::open_in_layout(&layout).unwrap());
        let session = SessionId::new();
        store
            .clone()
            .append_async(session, MemoryAppend::new(MemoryRole::User, "hi"))
            .await
            .unwrap();
        let history = store.history_async(session, Some(10)).await.unwrap();
        assert_eq!(history.len(), 1);
    }
}
