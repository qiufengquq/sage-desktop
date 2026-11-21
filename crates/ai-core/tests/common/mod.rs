//! Shared test helpers: log capture, temp-directory fixtures and trait assertions.
//!
//! Every helper here is hermetic: no network, no environment variables that carry
//! credentials, no shared global state between tests.

#![allow(dead_code)]

use std::io::Write;
use std::sync::{Arc, Mutex};

use ai_core::storage::StorageLayout;
use ai_core::AiCoreConfig;
use tempfile::TempDir;

/// In-memory writer used to assert what a `tracing` subscriber emitted.
#[derive(Clone, Default)]
pub struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

impl CapturedLogs {
    /// Returns everything written so far as UTF-8 (lossy).
    pub fn contents(&self) -> String {
        let buffer = match self.0.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        String::from_utf8_lossy(&buffer).into_owned()
    }
}

impl Write for CapturedLogs {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let mut target = match self.0.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        target.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedLogs {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Runs `body` with a thread-local subscriber that records into a buffer.
///
/// The subscriber is installed with `tracing::subscriber::with_default`, i.e. it
/// is scoped to this thread and cannot conflict with other tests (the library
/// never installs a global subscriber).
pub fn capture_logs<F: FnOnce()>(body: F) -> CapturedLogs {
    let logs = CapturedLogs::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(logs.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .finish();
    tracing::subscriber::with_default(subscriber, body);
    logs
}

/// Temporary storage fixture: an isolated root plus the derived layout.
pub struct TempStorage {
    dir: TempDir,
    layout: StorageLayout,
}

impl TempStorage {
    /// Creates a fresh temporary root and derives the storage layout.
    pub fn new() -> Self {
        let dir = TempDir::new().expect("temporary directory must be creatable");
        let layout = StorageLayout::derive(dir.path()).expect("temp root must be a valid layout");
        Self { dir, layout }
    }

    /// The temporary root directory.
    pub fn root(&self) -> &std::path::Path {
        self.dir.path()
    }

    /// The derived layout below the temporary root.
    pub fn layout(&self) -> &StorageLayout {
        &self.layout
    }

    /// A configuration whose storage root points at this fixture.
    pub fn config(&self) -> AiCoreConfig {
        let mut config = AiCoreConfig::default();
        config.storage.root = self.dir.path().to_path_buf();
        config
    }
}

impl Default for TempStorage {
    fn default() -> Self {
        Self::new()
    }
}

/// Asserts that `T` can cross thread and task boundaries.
pub fn assert_send_sync_static<T: Send + Sync + 'static>() {}

/// Asserts that `T` is cheap to copy (identifiers).
pub fn assert_copy<T: Copy>() {}
