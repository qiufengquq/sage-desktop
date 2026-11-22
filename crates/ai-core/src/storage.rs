//! Storage path boundaries.
//!
//! The AI Core must never share a SQLite file with the client business database,
//! and the three AI-owned areas have different lifetimes:
//!
//! | area | content | lifetime |
//! |------|---------|----------|
//! | [`StorageArea::Memory`] | long-lived AI memory (future vector/FTS index) | permanent, backed up |
//! | [`StorageArea::Metadata`] | AI-owned bookkeeping (usage, indexes) | long-lived |
//! | [`StorageArea::Cache`] | disposable responses/embeddings | may be purged at any time |
//!
//! [`StorageLayout`] derives the three sub-directories from a single root and
//! validates them, so cross-area mixing (cache rows inside the memory database)
//! shows up as a path-design error rather than a silent runtime corruption.
//!
//! Phase 1 only *locates* directories; no schema, migration or query code exists
//! yet. Creating directories is available through [`StorageLayout::create_directories`]
//! (blocking) and [`StorageLayout::create_directories_async`] (isolated on the
//! blocking pool).

use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::{CoreError, CoreResult, StorageArea};

/// Name of the AI memory directory below the storage root.
pub const AI_MEMORY_DIR: &str = StorageArea::Memory.dir_name();
/// Name of the AI metadata directory below the storage root.
pub const AI_METADATA_DIR: &str = StorageArea::Metadata.dir_name();
/// Name of the disposable cache directory below the storage root.
pub const AI_CACHE_DIR: &str = StorageArea::Cache.dir_name();

/// Immutable set of paths that keeps the three AI storage areas apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageLayout {
    root: PathBuf,
    memory: PathBuf,
    metadata: PathBuf,
    cache: PathBuf,
}

impl StorageLayout {
    /// Derives and validates the layout for `root`.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] when `root` is empty, relative with `.`/`..`
    /// components, or otherwise not usable as a stable base directory.
    pub fn derive(root: impl Into<PathBuf>) -> CoreResult<Self> {
        let root = root.into();
        validate_root(&root)?;
        let layout = Self {
            memory: root.join(AI_MEMORY_DIR),
            metadata: root.join(AI_METADATA_DIR),
            cache: root.join(AI_CACHE_DIR),
            root,
        };
        layout.validate()?;
        Ok(layout)
    }

    /// The configured root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Directory of the long-lived AI memory store.
    #[must_use]
    pub fn memory(&self) -> &Path {
        &self.memory
    }

    /// Directory of AI-owned metadata.
    #[must_use]
    pub fn metadata(&self) -> &Path {
        &self.metadata
    }

    /// Directory of disposable cache data.
    #[must_use]
    pub fn cache(&self) -> &Path {
        &self.cache
    }

    /// Resolves the directory of a logical storage area.
    #[must_use]
    pub fn path_for(&self, area: StorageArea) -> &Path {
        match area {
            StorageArea::Memory => &self.memory,
            StorageArea::Metadata => &self.metadata,
            StorageArea::Cache => &self.cache,
        }
    }

    /// Re-checks the layout invariants.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] if a derived path escapes the root or two
    /// areas resolve to the same directory.
    pub fn validate(&self) -> CoreResult<()> {
        validate_root(&self.root)?;
        let areas = [
            StorageArea::Memory,
            StorageArea::Metadata,
            StorageArea::Cache,
        ];
        for (index, first) in areas.iter().enumerate() {
            let first_path = self.path_for(*first);
            if !first_path.starts_with(&self.root) {
                return Err(CoreError::config_field(
                    "storage.root",
                    format!("{first} must live below the storage root"),
                ));
            }
            for second in &areas[index + 1..] {
                if first_path == self.path_for(*second) {
                    return Err(CoreError::config_field(
                        "storage.root",
                        format!("{first} and {second} must not share a directory"),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Creates all three directories, including missing parents.
    ///
    /// This performs blocking I/O; async callers should prefer
    /// [`Self::create_directories_async`].
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Storage`] wrapping the underlying I/O error.
    pub fn create_directories(&self) -> CoreResult<()> {
        for area in [
            StorageArea::Memory,
            StorageArea::Metadata,
            StorageArea::Cache,
        ] {
            let path = self.path_for(area);
            std::fs::create_dir_all(path).map_err(|source| {
                CoreError::storage(
                    area,
                    format!("failed to create `{}`", path.display()),
                    Some(Box::new(source)),
                )
            })?;
        }
        Ok(())
    }

    /// Creates all three directories on the blocking thread pool.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Storage`] on I/O failure, or
    /// [`CoreError::Concurrency`] if the blocking task could not be joined.
    pub async fn create_directories_async(&self) -> CoreResult<()> {
        let layout = self.clone();
        crate::async_utils::run_blocking("storage.create_directories", move || {
            layout.create_directories()
        })
        .await?
    }
}

/// Converts a [`SystemTime`] into Unix milliseconds for storage.
///
/// Shared by the AI-owned databases so a timestamp written by the memory store
/// and one written by the cache are encoded identically.
///
/// # Errors
///
/// Returns [`CoreError::InvalidArgument`] for a pre-epoch timestamp, which would
/// silently become a huge positive value otherwise.
pub(crate) fn system_time_millis(field: &str, value: SystemTime) -> CoreResult<i64> {
    let since_epoch = value.duration_since(UNIX_EPOCH).map_err(|_| {
        CoreError::invalid_argument(field, "must not be earlier than the Unix epoch")
    })?;
    Ok(i64::try_from(since_epoch.as_millis()).unwrap_or(i64::MAX))
}

/// Rebuilds a [`SystemTime`] from Unix milliseconds, saturating at the epoch.
pub(crate) fn system_time_from_millis(value: i64) -> SystemTime {
    UNIX_EPOCH
        .checked_add(Duration::from_millis(u64::try_from(value).unwrap_or(0)))
        .unwrap_or(UNIX_EPOCH)
}

/// Validates a storage root path without touching the filesystem.
fn validate_root(root: &Path) -> CoreResult<()> {
    if root.as_os_str().is_empty() {
        return Err(CoreError::config_field("storage.root", "must not be empty"));
    }
    for component in root.components() {
        match component {
            Component::ParentDir => {
                return Err(CoreError::config_field(
                    "storage.root",
                    "must not contain `..` components",
                ));
            }
            Component::CurDir => {
                return Err(CoreError::config_field(
                    "storage.root",
                    "must not contain `.` components",
                ));
            }
            Component::Normal(_) | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn derives_isolated_children() {
        let layout = StorageLayout::derive("data/ai").unwrap();
        assert_eq!(layout.memory(), Path::new("data/ai/ai-memory"));
        assert_eq!(layout.metadata(), Path::new("data/ai/ai-metadata"));
        assert_eq!(layout.cache(), Path::new("data/ai/ai-cache"));
        assert_ne!(layout.memory(), layout.cache());
        assert_eq!(
            layout.path_for(StorageArea::Metadata),
            Path::new("data/ai/ai-metadata")
        );
    }

    #[test]
    fn rejects_empty_and_escaping_roots() {
        assert!(StorageLayout::derive("").is_err());
        assert!(StorageLayout::derive("data/../other").is_err());
        assert!(StorageLayout::derive("./data").is_err());
        assert!(StorageLayout::derive("/abs/data").is_ok());
    }
}
