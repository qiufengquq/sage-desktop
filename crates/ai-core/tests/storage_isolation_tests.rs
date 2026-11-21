//! Temporary-directory isolation for the three AI storage areas.
//!
//! These tests never touch the real filesystem outside `tempfile` directories and
//! never depend on the network, credentials or the client application.

mod common;

use std::fs;
use std::sync::{Arc, Barrier};

use ai_core::storage::{StorageLayout, AI_CACHE_DIR, AI_MEMORY_DIR, AI_METADATA_DIR};
use ai_core::StorageArea;

#[test]
fn layout_creates_three_isolated_directories() {
    let storage = common::TempStorage::new();
    let layout = storage.layout();

    layout
        .create_directories()
        .expect("directories must be created");

    for area in [
        StorageArea::Memory,
        StorageArea::Metadata,
        StorageArea::Cache,
    ] {
        let path = layout.path_for(area);
        assert!(path.is_dir(), "{} must exist", path.display());
        assert!(path.starts_with(storage.root()));
    }

    let cache_entry = layout.cache().join("entry.json");
    fs::write(&cache_entry, "{}").expect("cache write");
    assert!(cache_entry.exists());
    assert!(
        !layout.memory().join("entry.json").exists(),
        "cache content must not appear in the memory area"
    );
}

#[tokio::test]
async fn asynchronous_directory_creation_uses_the_blocking_pool() {
    let storage = common::TempStorage::new();
    let layout = storage.layout().clone();
    layout
        .create_directories_async()
        .await
        .expect("async creation must succeed");

    assert!(layout.memory().is_dir());
    assert!(layout.metadata().is_dir());
    assert!(layout.cache().is_dir());
}

#[test]
fn area_directory_names_are_distinct() {
    assert_ne!(AI_MEMORY_DIR, AI_METADATA_DIR);
    assert_ne!(AI_MEMORY_DIR, AI_CACHE_DIR);
    assert_ne!(AI_METADATA_DIR, AI_CACHE_DIR);
    assert_eq!(StorageArea::Cache.dir_name(), AI_CACHE_DIR);
}

#[test]
fn invalid_roots_are_rejected_without_touching_the_filesystem() {
    for root in ["", "a/../b", "./c"] {
        let error = StorageLayout::derive(root).expect_err("root must be rejected");
        assert!(error.to_string().contains("storage.root"));
    }
}

#[test]
fn deriving_twice_produces_equal_layouts() {
    let storage = common::TempStorage::new();
    let first = StorageLayout::derive(storage.root()).expect("valid");
    let second = StorageLayout::derive(storage.root()).expect("valid");
    assert_eq!(first, second);
    assert_eq!(first, *storage.layout());
}

#[test]
fn parallel_temp_directories_do_not_interfere() {
    const WORKERS: usize = 16;
    let barrier = Arc::new(Barrier::new(WORKERS));

    let workers: Vec<_> = (0..WORKERS)
        .map(|worker| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let storage = common::TempStorage::new();
                storage
                    .layout()
                    .create_directories()
                    .expect("directories must be created");

                let marker = storage
                    .layout()
                    .cache()
                    .join(format!("worker-{worker}.txt"));
                let payload = format!("worker-{worker}");
                fs::write(&marker, &payload).expect("write marker");

                barrier.wait();

                let observed = fs::read_to_string(&marker).expect("read marker");
                assert_eq!(observed, payload, "another worker must not overwrite us");

                let root = storage.root().to_path_buf();
                // Only this worker's own file exists in its own cache directory.
                let entries: Vec<_> = fs::read_dir(storage.layout().cache())
                    .expect("cache dir readable")
                    .filter_map(Result::ok)
                    .map(|entry| entry.file_name())
                    .collect();
                assert_eq!(entries.len(), 1);

                root
            })
        })
        .collect();

    let mut roots = Vec::new();
    for worker in workers {
        roots.push(worker.join().expect("worker must not panic"));
    }

    roots.sort();
    roots.dedup();
    assert_eq!(roots.len(), WORKERS, "temporary roots must be unique");
}
