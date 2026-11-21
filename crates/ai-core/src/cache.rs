//! Disposable response/embedding cache.
//!
//! # Responsibility
//!
//! Memoize provider answers and embeddings keyed by a hash of the *redacted*
//! request shape, with an explicit TTL and explicit invalidation.
//!
//! # Boundaries
//!
//! * Cache has a **different lifetime than memory**: entries may be evicted at
//!   any moment and are never the source of truth for conversation state. The
//!   two live in separate directories ([`StorageArea::Cache`] vs
//!   [`StorageArea::Memory`]) and never share tables.
//! * Keys are content hashes of the normalized request, so they cannot embed
//!   credentials. The hash here is FNV-1a (fast, deterministic, **not**
//!   collision-resistant) — a collision can only produce a cache miss or a wrong
//!   hit for an identical request shape, never a security boundary.
//! * A cache failure is a *degradation*, not a failure: [`CacheStore::lookup`]
//!   returns [`CacheLookup::Unavailable`] so the caller continues without the
//!   cache instead of treating the failure as a miss or a hit.
//! * A cache hit never fabricates correlation: the caller keeps its own
//!   trace/request identifiers, only the payload is reused.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use rusqlite::{params, Connection, OptionalExtension, Row as SqlRow};

use crate::async_utils::run_blocking_result;
use crate::error::{CoreError, CoreResult, StorageArea};
use crate::storage::{system_time_from_millis, system_time_millis, StorageLayout};

/// File name of the cache database inside `ai-cache/`.
pub const CACHE_DB_FILE: &str = "cache.sqlite3";
/// Schema version this build creates and understands.
pub const CACHE_SCHEMA_VERSION: i64 = 1;

/// Separator inserted between hashed parts, so `["ab", "c"]` and `["a", "bc"]`
/// produce different keys.
const KEY_SEPARATOR: u8 = 0x1f;
/// First FNV-1a 64-bit offset basis.
const FNV_OFFSET_A: u64 = 0xcbf2_9ce4_8422_2325;
/// Second (independently seeded) offset basis, giving a 128-bit key.
const FNV_OFFSET_B: u64 = 0x8422_2325_cbf2_9ce4;
/// FNV-1a 64-bit prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Handle of a private in-process database.
const IN_MEMORY_PATH: &str = ":memory:";

const SCHEMA_V1: &str = r#"
CREATE TABLE cache_entries (
    key           TEXT    PRIMARY KEY,
    kind          TEXT    NOT NULL,
    payload       TEXT    NOT NULL,
    created_at_ms INTEGER NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    hits          INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_cache_expires ON cache_entries (expires_at_ms);
CREATE INDEX idx_cache_created ON cache_entries (created_at_ms);
"#;

const ENTRY_COLUMNS: &str = "key, kind, payload, created_at_ms, expires_at_ms, hits";

/// What a cache entry holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CacheKind {
    /// A complete provider response payload.
    Response,
    /// An embedding vector payload.
    Embedding,
}

impl CacheKind {
    /// Stable database label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Response => "response",
            Self::Embedding => "embedding",
        }
    }

    /// Parses a stored label, returning `None` for an unknown value.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "response" => Some(Self::Response),
            "embedding" => Some(Self::Embedding),
            _ => None,
        }
    }
}

impl fmt::Display for CacheKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for CacheKind {
    type Err = CoreError;

    fn from_str(value: &str) -> CoreResult<Self> {
        Self::from_label(value.trim()).ok_or_else(|| {
            CoreError::invalid_argument(
                "cache.kind",
                format!("unknown kind `{value}` (expected response or embedding)"),
            )
        })
    }
}

/// 128-bit content hash identifying a cache entry.
///
/// Built with [`CacheKey::from_parts`] from the normalized, credential-free
/// parts of a request. The hex form is what the database stores.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CacheKey(String);

impl CacheKey {
    /// Hashes the given parts, which are separator-delimited and order-sensitive.
    ///
    /// Callers must pass the *normalized* request shape (model, redacted
    /// messages, temperature, response format, ...) and never a credential.
    #[must_use]
    pub fn from_parts(parts: &[&str]) -> Self {
        let high = fnv1a64(parts, FNV_OFFSET_A);
        let low = fnv1a64(parts, FNV_OFFSET_B);
        Self(format!("{high:016x}{low:016x}"))
    }

    /// Rebuilds a key from its stored hex form.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] unless `value` is exactly 32
    /// lowercase hexadecimal characters.
    pub fn parse(value: &str) -> CoreResult<Self> {
        let valid = value.len() == 32
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !valid {
            return Err(CoreError::invalid_argument(
                "cache.key",
                "expected 32 lowercase hexadecimal characters",
            ));
        }
        Ok(Self(value.to_owned()))
    }

    /// Hex representation used for storage and comparison.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CacheKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for CacheKey {
    type Err = CoreError;

    fn from_str(value: &str) -> CoreResult<Self> {
        Self::parse(value)
    }
}

/// FNV-1a over every part, separated so concatenation is unambiguous.
fn fnv1a64(parts: &[&str], basis: u64) -> u64 {
    let mut hash = basis;
    for part in parts {
        for byte in part.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        hash ^= u64::from(KEY_SEPARATOR);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// A stored cache entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    /// Content hash of the request shape.
    pub key: CacheKey,
    /// What the payload holds.
    pub kind: CacheKind,
    /// Opaque payload (serialized response, embedding vector, ...).
    pub payload: String,
    /// Insertion timestamp.
    pub created_at: SystemTime,
    /// Expiry timestamp; entries are never stored without one.
    pub expires_at: SystemTime,
    /// Number of lookups served.
    pub hits: u64,
}

impl CacheEntry {
    /// Whether the entry is past its expiry at `now`.
    #[must_use]
    pub fn is_expired_at(&self, now: SystemTime) -> bool {
        now >= self.expires_at
    }
}

/// Outcome of a cache lookup.
///
/// `Unavailable` is deliberately distinct from `Miss`: a broken cache must be
/// reported as a degradation, never silently treated as "not cached".
#[derive(Debug)]
pub enum CacheLookup {
    /// The entry was found and is still valid.
    Hit(CacheEntry),
    /// No entry exists for the key.
    Miss,
    /// The cache could not be consulted; the caller should continue uncached.
    Unavailable(CoreError),
}

impl CacheLookup {
    /// Consumes the outcome, yielding the entry of a hit.
    #[must_use]
    pub fn hit(self) -> Option<CacheEntry> {
        match self {
            Self::Hit(entry) => Some(entry),
            Self::Miss | Self::Unavailable(_) => None,
        }
    }

    /// Whether the lookup hit a valid entry.
    #[must_use]
    pub fn is_hit(&self) -> bool {
        matches!(self, Self::Hit(_))
    }

    /// Whether the cache was unreachable.
    #[must_use]
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }

    /// Borrowed payload of a hit.
    #[must_use]
    pub fn payload(&self) -> Option<&str> {
        match self {
            Self::Hit(entry) => Some(&entry.payload),
            Self::Miss | Self::Unavailable(_) => None,
        }
    }
}

/// SQLite-backed, disposable cache.
///
/// The type owns its connection and is shared as `Arc<CacheStore>`; every method
/// is blocking, with `*_async` siblings that hop onto the blocking pool.
pub struct CacheStore {
    path: PathBuf,
    connection: Mutex<Connection>,
}

impl fmt::Debug for CacheStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CacheStore")
            .field("path", &self.path)
            .finish()
    }
}

impl CacheStore {
    /// Opens (creating if needed) the cache database at `path` and migrates it.
    ///
    /// Passing `":memory:"` opens a private in-process database, which is useful
    /// for tests.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Storage`] with [`StorageArea::Cache`] when the
    /// directory cannot be created, the database cannot be opened, or the stored
    /// schema is newer than this build supports.
    pub fn open(path: impl Into<PathBuf>) -> CoreResult<Self> {
        let path = path.into();
        if path.as_os_str().is_empty() {
            return Err(CoreError::invalid_argument(
                "cache.path",
                "must not be empty",
            ));
        }
        if path != Path::new(IN_MEMORY_PATH) {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    CoreError::storage(
                        StorageArea::Cache,
                        format!("failed to create `{}`", parent.display()),
                        Some(Box::new(error)),
                    )
                })?;
            }
        }

        let mut connection = Connection::open(&path)
            .map_err(|error| cache_error("failed to open the cache database", error))?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;\n\
                 PRAGMA synchronous = NORMAL;\n\
                 PRAGMA busy_timeout = 5000;",
            )
            .map_err(|error| cache_error("failed to configure the cache database", error))?;
        migrate(&mut connection)?;
        Ok(Self {
            path,
            connection: Mutex::new(connection),
        })
    }

    /// Opens the cache database inside a storage layout's `ai-cache/` area.
    ///
    /// # Errors
    ///
    /// See [`CacheStore::open`].
    pub fn open_in_layout(layout: &StorageLayout) -> CoreResult<Self> {
        Self::open(layout.cache().join(CACHE_DB_FILE))
    }

    /// Opens the cache database on the blocking pool.
    ///
    /// # Errors
    ///
    /// See [`CacheStore::open`], plus [`CoreError::Concurrency`] when the blocking
    /// task cannot be joined.
    pub async fn open_in_layout_async(layout: &StorageLayout) -> CoreResult<Self> {
        let layout = layout.clone();
        run_blocking_result("cache.open", move || Self::open_in_layout(&layout)).await
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
            .map_err(|error| cache_error("failed to read the cache schema version", error))
    }

    /// Stores (or replaces) an entry that expires after `ttl`.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] when `ttl` is zero and
    /// [`CoreError::Storage`] when the write fails.
    pub fn put(
        &self,
        key: &CacheKey,
        kind: CacheKind,
        payload: impl Into<String>,
        ttl: Duration,
    ) -> CoreResult<CacheEntry> {
        if ttl.is_zero() {
            return Err(CoreError::invalid_argument(
                "cache.ttl",
                "must be greater than zero",
            ));
        }
        let payload = payload.into();
        let created_at = SystemTime::now();
        let created_at_ms = system_time_millis("cache.created_at", created_at)?;
        let expires_at = created_at.checked_add(ttl).ok_or_else(|| {
            CoreError::invalid_argument("cache.ttl", "moves the expiry past the supported range")
        })?;
        let expires_at_ms = system_time_millis("cache.expires_at", expires_at)?;

        let connection = self.connection();
        connection
            .execute(
                "INSERT INTO cache_entries (key, kind, payload, created_at_ms, expires_at_ms, hits) \
                 VALUES (?1, ?2, ?3, ?4, ?5, 0) \
                 ON CONFLICT (key) DO UPDATE SET \
                    kind = excluded.kind, \
                    payload = excluded.payload, \
                    created_at_ms = excluded.created_at_ms, \
                    expires_at_ms = excluded.expires_at_ms, \
                    hits = 0",
                params![
                    key.as_str(),
                    kind.as_str(),
                    payload,
                    created_at_ms,
                    expires_at_ms
                ],
            )
            .map_err(|error| cache_error("failed to store a cache entry", error))?;

        Ok(CacheEntry {
            key: key.clone(),
            kind,
            payload,
            created_at,
            expires_at,
            hits: 0,
        })
    }

    /// Stores an entry on the blocking pool.
    ///
    /// # Errors
    ///
    /// See [`CacheStore::put`].
    pub async fn put_async(
        self: Arc<Self>,
        key: CacheKey,
        kind: CacheKind,
        payload: String,
        ttl: Duration,
    ) -> CoreResult<CacheEntry> {
        run_blocking_result("cache.put", move || self.put(&key, kind, payload, ttl)).await
    }

    /// Looks an entry up without ever failing the caller.
    ///
    /// A hit bumps the entry's counter; an expired entry is deleted and reported
    /// as a miss.
    #[must_use]
    pub fn lookup(&self, key: &CacheKey) -> CacheLookup {
        match self.get(key) {
            Ok(Some(entry)) => CacheLookup::Hit(entry),
            Ok(None) => CacheLookup::Miss,
            Err(error) => {
                tracing::warn!(
                    code = error.error_code(),
                    "cache lookup degraded; continuing without the cache"
                );
                CacheLookup::Unavailable(error)
            }
        }
    }

    /// Looks an entry up on the blocking pool.
    #[must_use]
    pub async fn lookup_async(self: Arc<Self>, key: CacheKey) -> CacheLookup {
        match run_blocking_result("cache.lookup", move || self.get(&key)).await {
            Ok(Some(entry)) => CacheLookup::Hit(entry),
            Ok(None) => CacheLookup::Miss,
            Err(error) => {
                tracing::warn!(
                    code = error.error_code(),
                    "cache lookup degraded; continuing without the cache"
                );
                CacheLookup::Unavailable(error)
            }
        }
    }

    /// Reads an entry, deleting it when expired.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Storage`] when the database fails.
    pub fn get(&self, key: &CacheKey) -> CoreResult<Option<CacheEntry>> {
        let connection = self.connection();
        let entry = connection
            .query_row(
                &format!("SELECT {ENTRY_COLUMNS} FROM cache_entries WHERE key = ?1"),
                params![key.as_str()],
                entry_from_row,
            )
            .optional()
            .map_err(|error| cache_error("failed to read a cache entry", error))?;

        let Some(entry) = entry else {
            return Ok(None);
        };
        if entry.is_expired_at(SystemTime::now()) {
            // Lazy eviction: the row is useless from now on and would otherwise
            // linger until the next purge.
            connection
                .execute(
                    "DELETE FROM cache_entries WHERE key = ?1",
                    params![key.as_str()],
                )
                .map_err(|error| cache_error("failed to evict an expired entry", error))?;
            return Ok(None);
        }

        connection
            .execute(
                "UPDATE cache_entries SET hits = hits + 1 WHERE key = ?1",
                params![key.as_str()],
            )
            .map_err(|error| cache_error("failed to record a cache hit", error))?;
        Ok(Some(CacheEntry {
            hits: entry.hits.saturating_add(1),
            ..entry
        }))
    }

    /// Deletes one entry.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Storage`] when the delete fails.
    pub fn invalidate(&self, key: &CacheKey) -> CoreResult<bool> {
        let connection = self.connection();
        connection
            .execute(
                "DELETE FROM cache_entries WHERE key = ?1",
                params![key.as_str()],
            )
            .map(|deleted| deleted > 0)
            .map_err(|error| cache_error("failed to invalidate a cache entry", error))
    }

    /// Deletes every expired entry, returning how many were removed.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] for a pre-epoch cutoff and
    /// [`CoreError::Storage`] when the delete fails.
    pub fn purge_expired(&self, now: SystemTime) -> CoreResult<u64> {
        let now_ms = system_time_millis("cache.now", now)?;
        let connection = self.connection();
        connection
            .execute(
                "DELETE FROM cache_entries WHERE expires_at_ms <= ?1",
                params![now_ms],
            )
            .map(|deleted| u64::try_from(deleted).unwrap_or(u64::MAX))
            .map_err(|error| cache_error("failed to purge the cache", error))
    }

    /// Drops all entries, returning how many were removed.
    ///
    /// The cache is disposable by contract, so a host may call this at any time.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Storage`] when the delete fails.
    pub fn clear(&self) -> CoreResult<u64> {
        let connection = self.connection();
        connection
            .execute("DELETE FROM cache_entries", [])
            .map(|deleted| u64::try_from(deleted).unwrap_or(u64::MAX))
            .map_err(|error| cache_error("failed to clear the cache", error))
    }

    /// Number of stored entries, expired ones included.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Storage`] when the query fails.
    pub fn len(&self) -> CoreResult<u64> {
        let connection = self.connection();
        connection
            .query_row("SELECT COUNT(*) FROM cache_entries", [], |row| {
                row.get::<_, i64>(0)
            })
            .map(|count| u64::try_from(count).unwrap_or(0))
            .map_err(|error| cache_error("failed to count cache entries", error))
    }

    /// Whether the cache holds no entry.
    ///
    /// # Errors
    ///
    /// See [`CacheStore::len`].
    pub fn is_empty(&self) -> CoreResult<bool> {
        self.len().map(|count| count == 0)
    }

    /// Evicts the oldest entries so at most `max_entries` remain.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] when `max_entries` is zero and
    /// [`CoreError::Storage`] when the delete fails.
    pub fn evict_oldest(&self, max_entries: usize) -> CoreResult<u64> {
        if max_entries == 0 {
            return Err(CoreError::invalid_argument(
                "cache.max_entries",
                "must be greater than zero",
            ));
        }
        let keep = i64::try_from(max_entries).unwrap_or(i64::MAX);
        let connection = self.connection();
        connection
            .execute(
                "DELETE FROM cache_entries WHERE key NOT IN (\
                    SELECT key FROM cache_entries ORDER BY created_at_ms DESC, key DESC LIMIT ?1\
                 )",
                params![keep],
            )
            .map(|deleted| u64::try_from(deleted).unwrap_or(u64::MAX))
            .map_err(|error| cache_error("failed to evict cache entries", error))
    }

    fn connection(&self) -> MutexGuard<'_, Connection> {
        match self.connection.lock() {
            Ok(guard) => guard,
            // A poisoned lock only means another thread panicked; the connection
            // itself is still usable.
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

/// Applies the schema migrations described by `PRAGMA user_version`.
fn migrate(connection: &mut Connection) -> CoreResult<()> {
    let current: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| cache_error("failed to read the cache schema version", error))?;
    if current > CACHE_SCHEMA_VERSION {
        return Err(CoreError::storage(
            StorageArea::Cache,
            format!(
                "cache database schema v{current} is newer than the supported v{CACHE_SCHEMA_VERSION}"
            ),
            None,
        ));
    }
    if current == CACHE_SCHEMA_VERSION {
        return Ok(());
    }

    let transaction = connection
        .transaction()
        .map_err(|error| cache_error("failed to start the cache migration", error))?;
    transaction
        .execute_batch(SCHEMA_V1)
        .map_err(|error| cache_error("failed to apply the cache schema", error))?;
    transaction
        .pragma_update(None, "user_version", CACHE_SCHEMA_VERSION)
        .map_err(|error| cache_error("failed to record the cache schema version", error))?;
    transaction
        .commit()
        .map_err(|error| cache_error("failed to commit the cache migration", error))
}

fn entry_from_row(row: &SqlRow<'_>) -> rusqlite::Result<CacheEntry> {
    let key: String = row.get(0)?;
    let kind: String = row.get(1)?;
    let payload: String = row.get(2)?;
    let created_at_ms: i64 = row.get(3)?;
    let expires_at_ms: i64 = row.get(4)?;
    let hits: i64 = row.get(5)?;

    Ok(CacheEntry {
        key: CacheKey(key),
        kind: CacheKind::from_label(&kind)
            .ok_or_else(|| invalid_label(1, format!("unknown cache kind `{kind}`")))?,
        payload,
        created_at: system_time_from_millis(created_at_ms),
        expires_at: system_time_from_millis(expires_at_ms),
        hits: u64::try_from(hits).unwrap_or(0),
    })
}

/// Builds a `FromSqlConversionFailure` carrying the offending reason.
fn invalid_label(index: usize, reason: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, reason.into())
}

fn cache_error(message: &str, error: rusqlite::Error) -> CoreError {
    CoreError::storage(StorageArea::Cache, message, Some(Box::new(error)))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn layout() -> (tempfile::TempDir, StorageLayout) {
        let directory = tempfile::tempdir().unwrap();
        let layout = StorageLayout::derive(directory.path().join("ai-core")).unwrap();
        (directory, layout)
    }

    fn store() -> CacheStore {
        CacheStore::open(IN_MEMORY_PATH).unwrap()
    }

    fn key(label: &str) -> CacheKey {
        CacheKey::from_parts(&["openai", "gpt-4o-mini", label])
    }

    #[test]
    fn creates_and_migrates_the_schema() {
        let (_directory, layout) = layout();
        let store = CacheStore::open_in_layout(&layout).unwrap();
        assert_eq!(store.schema_version().unwrap(), CACHE_SCHEMA_VERSION);
        assert!(store.path().starts_with(layout.cache()));
        assert!(store.is_empty().unwrap());
    }

    #[test]
    fn keys_are_deterministic_and_separator_safe() {
        assert_eq!(key("a"), key("a"));
        assert_ne!(key("a"), key("b"));
        assert_ne!(
            CacheKey::from_parts(&["ab", "c"]),
            CacheKey::from_parts(&["a", "bc"])
        );
        let key = key("a");
        assert_eq!(key.as_str().len(), 32);
        assert_eq!(CacheKey::parse(key.as_str()).unwrap(), key);
        assert!(CacheKey::parse("not-a-key").is_err());
        assert!(CacheKey::parse(&key.as_str().to_uppercase()).is_err());
    }

    #[test]
    fn put_then_lookup_hits_and_counts() {
        let store = store();
        let key = key("hello");
        store
            .put(
                &key,
                CacheKind::Response,
                "{\"text\":\"hi\"}",
                Duration::from_secs(60),
            )
            .unwrap();

        let lookup = store.lookup(&key);
        assert!(lookup.is_hit());
        assert_eq!(lookup.payload(), Some("{\"text\":\"hi\"}"));

        let second = store.get(&key).unwrap().unwrap();
        assert_eq!(second.hits, 2);
        assert_eq!(second.kind, CacheKind::Response);
    }

    #[test]
    fn missing_keys_are_a_plain_miss() {
        let store = store();
        let lookup = store.lookup(&key("absent"));
        assert!(matches!(lookup, CacheLookup::Miss));
        assert!(!lookup.is_unavailable());
        assert!(lookup.payload().is_none());
    }

    #[test]
    fn expired_entries_are_evicted_lazily() {
        let store = store();
        let key = key("expiring");
        store
            .put(
                &key,
                CacheKind::Embedding,
                "[0.1,0.2]",
                Duration::from_millis(1),
            )
            .unwrap();
        std::thread::sleep(Duration::from_millis(5));

        assert!(matches!(store.lookup(&key), CacheLookup::Miss));
        assert_eq!(store.len().unwrap(), 0);
    }

    #[test]
    fn putting_again_replaces_the_payload_and_resets_hits() {
        let store = store();
        let key = key("replace");
        store
            .put(&key, CacheKind::Response, "one", Duration::from_secs(60))
            .unwrap();
        assert!(store.lookup(&key).is_hit());
        store
            .put(&key, CacheKind::Response, "two", Duration::from_secs(60))
            .unwrap();
        let entry = store.get(&key).unwrap().unwrap();
        assert_eq!(entry.payload, "two");
        assert_eq!(entry.hits, 1);
    }

    #[test]
    fn zero_ttl_is_rejected() {
        let store = store();
        let error = store
            .put(&key("x"), CacheKind::Response, "x", Duration::ZERO)
            .unwrap_err();
        assert_eq!(error.kind(), crate::ErrorKind::InvalidArgument);
    }

    #[test]
    fn invalidate_and_clear_remove_entries() {
        let store = store();
        let first = key("one");
        let second = key("two");
        store
            .put(&first, CacheKind::Response, "1", Duration::from_secs(60))
            .unwrap();
        store
            .put(&second, CacheKind::Response, "2", Duration::from_secs(60))
            .unwrap();

        assert!(store.invalidate(&first).unwrap());
        assert!(!store.invalidate(&first).unwrap());
        assert_eq!(store.len().unwrap(), 1);
        assert_eq!(store.clear().unwrap(), 1);
        assert!(store.is_empty().unwrap());
    }

    #[test]
    fn purge_expired_only_removes_expired_rows() {
        let store = store();
        store
            .put(
                &key("short"),
                CacheKind::Response,
                "s",
                Duration::from_millis(1),
            )
            .unwrap();
        store
            .put(
                &key("long"),
                CacheKind::Response,
                "l",
                Duration::from_secs(3600),
            )
            .unwrap();
        std::thread::sleep(Duration::from_millis(5));

        assert_eq!(store.purge_expired(SystemTime::now()).unwrap(), 1);
        assert_eq!(store.len().unwrap(), 1);
        assert!(matches!(store.lookup(&key("long")), CacheLookup::Hit(_)));
    }

    #[test]
    fn eviction_keeps_the_newest_entries() {
        let store = store();
        for label in ["a", "b", "c"] {
            store
                .put(
                    &key(label),
                    CacheKind::Response,
                    label,
                    Duration::from_secs(60),
                )
                .unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(store.evict_oldest(2).unwrap(), 1);
        assert_eq!(store.len().unwrap(), 2);
        assert!(matches!(store.lookup(&key("a")), CacheLookup::Miss));
        assert!(matches!(store.lookup(&key("c")), CacheLookup::Hit(_)));
        assert!(store.evict_oldest(0).is_err());
    }

    #[test]
    fn reopened_store_keeps_entries() {
        let (_directory, layout) = layout();
        let key = key("persisted");
        {
            let store = CacheStore::open_in_layout(&layout).unwrap();
            store
                .put(&key, CacheKind::Response, "value", Duration::from_secs(60))
                .unwrap();
        }
        let reopened = CacheStore::open_in_layout(&layout).unwrap();
        assert_eq!(reopened.lookup(&key).payload(), Some("value"));
    }

    #[tokio::test]
    async fn async_variants_run_on_the_blocking_pool() {
        let (_directory, layout) = layout();
        let store = Arc::new(CacheStore::open_in_layout(&layout).unwrap());
        let key = key("async");
        store
            .clone()
            .put_async(
                key.clone(),
                CacheKind::Response,
                "value".to_owned(),
                Duration::from_secs(60),
            )
            .await
            .unwrap();
        let lookup = store.lookup_async(key).await;
        assert_eq!(lookup.payload(), Some("value"));
    }
}
