//! Data-race, deadlock and snapshot-consistency checks.
//!
//! Locks and channels are exercised with explicit deadlines, so a regression that
//! deadlocks fails the test instead of hanging the suite.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use ai_core::observability::TraceContext;
use ai_core::{AiCoreConfig, ConfigStore, ConfigVersion, RequestId};

const DEADLOCK_GUARD: Duration = Duration::from_secs(15);

fn config_with_latency(millis: u64) -> AiCoreConfig {
    let mut config = AiCoreConfig::default();
    let budget = Duration::from_millis(millis);
    // Two correlated fields: a torn snapshot would make them disagree.
    config.timeouts.request = budget;
    config.timeouts.connect = budget;
    config
}

#[test]
fn snapshot_reads_stay_consistent_while_a_writer_publishes() {
    let store = Arc::new(ConfigStore::new(config_with_latency(100)).expect("valid"));
    let stop = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = mpsc::channel();

    let readers: Vec<_> = (0..4)
        .map(|_| {
            let store = Arc::clone(&store);
            let stop = Arc::clone(&stop);
            let sender = sender.clone();
            std::thread::spawn(move || {
                let mut reads = 0_usize;
                while !stop.load(Ordering::Relaxed) {
                    let snapshot = store.snapshot();
                    let timeouts = &snapshot.config().timeouts;
                    assert_eq!(
                        timeouts.request,
                        timeouts.connect,
                        "torn snapshot observed at version {}",
                        snapshot.version()
                    );
                    assert!(snapshot.version() >= ConfigVersion::INITIAL);
                    reads += 1;
                }
                sender.send(reads).ok();
            })
        })
        .collect();

    for step in 1..=200_u64 {
        store
            .apply(config_with_latency(100 + step))
            .expect("valid update");
    }
    stop.store(true, Ordering::Relaxed);
    drop(sender);

    let mut total_reads = 0_usize;
    for _ in 0..readers.len() {
        total_reads += receiver
            .recv_timeout(DEADLOCK_GUARD)
            .expect("readers must not deadlock");
    }
    assert!(total_reads > 0, "readers must have observed snapshots");

    for reader in readers {
        reader.join().expect("reader thread must not panic");
    }
    assert_eq!(store.version(), ConfigVersion::new(1 + 200));
}

#[test]
fn concurrent_writers_assign_unique_sequential_versions() {
    let store = Arc::new(ConfigStore::new(config_with_latency(100)).expect("valid"));

    let writers: Vec<_> = (0..4_u64)
        .map(|writer| {
            let store = Arc::clone(&store);
            std::thread::spawn(move || {
                let mut versions = Vec::new();
                for iteration in 0..50_u64 {
                    let config = config_with_latency(100 + writer * 100 + iteration);
                    versions.push(store.apply(config).expect("valid update").version().get());
                }
                versions
            })
        })
        .collect();

    let mut all_versions = Vec::new();
    for writer in writers {
        all_versions.extend(writer.join().expect("writer must not panic"));
    }
    all_versions.sort_unstable();

    assert_eq!(all_versions.len(), 200);
    assert_eq!(all_versions.first().copied(), Some(2));
    assert_eq!(all_versions.last().copied(), Some(201));

    let unique: HashSet<u64> = all_versions.iter().copied().collect();
    assert_eq!(unique.len(), 200, "versions must never repeat");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn async_readers_may_hold_snapshots_across_await_points() {
    let store = Arc::new(ConfigStore::new(config_with_latency(100)).expect("valid"));

    let mut readers = Vec::new();
    for _ in 0..8 {
        let store = Arc::clone(&store);
        readers.push(tokio::spawn(async move {
            for _ in 0..200 {
                let snapshot = store.snapshot();
                tokio::task::yield_now().await;
                let timeouts = &snapshot.config().timeouts;
                assert_eq!(timeouts.request, timeouts.connect);
            }
        }));
    }

    let writer = tokio::spawn({
        let store = Arc::clone(&store);
        async move {
            for step in 1..=100_u64 {
                store
                    .apply(config_with_latency(100 + step))
                    .expect("valid update");
                tokio::task::yield_now().await;
            }
        }
    });

    let joined = tokio::time::timeout(DEADLOCK_GUARD, async {
        for reader in readers {
            reader.await.expect("reader task must not panic");
        }
        writer.await.expect("writer task must not panic");
    })
    .await;

    joined.expect("reading snapshots across await points must not deadlock");
    assert_eq!(store.version(), ConfigVersion::new(101));
}

#[test]
fn identifiers_and_trace_contexts_are_unique_under_contention() {
    let handles: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                let mut requests = Vec::with_capacity(250);
                let mut traces = Vec::with_capacity(250);
                for _ in 0..250 {
                    let context = TraceContext::generate();
                    requests.push(context.request_id());
                    traces.push(context.trace_id());
                }
                (requests, traces)
            })
        })
        .collect();

    let mut all_requests = HashSet::new();
    let mut all_traces = HashSet::new();
    for handle in handles {
        let (requests, traces) = handle.join().expect("thread must not panic");
        for request in requests {
            assert!(all_requests.insert(request), "duplicate RequestId");
        }
        for trace in traces {
            assert!(all_traces.insert(trace), "duplicate TraceId");
        }
    }
    assert_eq!(all_requests.len(), 2_000);
    assert_eq!(all_traces.len(), 2_000);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn identifier_generation_is_unique_across_tasks() {
    let mut tasks = Vec::new();
    for _ in 0..16 {
        tasks.push(tokio::spawn(async {
            let mut ids = HashSet::new();
            for _ in 0..100 {
                assert!(ids.insert(RequestId::new()));
                tokio::task::yield_now().await;
            }
            ids.len()
        }));
    }

    let mut total = 0_usize;
    for task in tasks {
        total += task.await.expect("task must not panic");
    }
    assert_eq!(total, 1_600);
}
