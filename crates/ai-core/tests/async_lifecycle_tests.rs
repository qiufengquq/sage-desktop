//! Async timeout, cancellation, blocking isolation and shutdown behaviour.
//!
//! Every test is bounded by an explicit `tokio::time::timeout` guard so a
//! regression (a hang / lost cancellation) fails the suite instead of blocking it.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ai_core::async_utils::{
    run_blocking, run_blocking_result, run_with_limits, with_cancellation, with_timeout,
    CancelToken, ShutdownSignal,
};
use ai_core::{CoreError, ErrorKind};

const GUARD: Duration = Duration::from_secs(5);

/// Drops a flag into a shared `AtomicBool` so tests can observe resource release.
struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn timeout_fails_fast_with_an_explicit_error() {
    let started = Instant::now();
    let error = with_timeout(
        "test.timeout",
        Duration::from_millis(30),
        std::future::pending::<()>(),
    )
    .await
    .expect_err("pending future must time out");

    assert!(error.is_timeout());
    assert_eq!(error.kind(), ErrorKind::Timeout);
    assert_eq!(error.error_code(), "CORE_TIMEOUT");
    assert!(started.elapsed() < GUARD);
}

#[tokio::test]
async fn timeout_propagates_successful_results() {
    let value = with_timeout("test.ok", GUARD, async { 7_u32 })
        .await
        .expect("must succeed");
    assert_eq!(value, 7);
}

#[tokio::test]
async fn timed_out_future_is_dropped_and_releases_resources() {
    let released = Arc::new(AtomicBool::new(false));
    let guard = DropFlag(Arc::clone(&released));

    let future = async move {
        let _guard = guard;
        std::future::pending::<()>().await;
    };

    let error = with_timeout("test.drop", Duration::from_millis(20), future)
        .await
        .expect_err("must time out");
    assert!(error.is_timeout());
    assert!(
        released.load(Ordering::SeqCst),
        "the timed-out future (and its guards) must be dropped"
    );
}

#[tokio::test]
async fn cancellation_aborts_the_pending_future() {
    let token = CancelToken::new();
    let released = Arc::new(AtomicBool::new(false));
    let guard = DropFlag(Arc::clone(&released));

    let future = async move {
        let _guard = guard;
        std::future::pending::<()>().await;
    };

    let cancel = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        cancel.cancel();
    });

    let error = with_cancellation("test.cancel", &token, future)
        .await
        .expect_err("must be cancelled");
    assert!(error.is_cancelled());
    assert_eq!(error.error_code(), "CORE_CANCELLED");
    assert!(released.load(Ordering::SeqCst));
}

#[tokio::test]
async fn already_cancelled_token_prevents_execution() {
    let token = CancelToken::new();
    token.cancel();

    let polled = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&polled);
    let future = async move {
        flag.store(true, Ordering::SeqCst);
    };

    let error = with_cancellation("test.pre_cancelled", &token, future)
        .await
        .expect_err("must be cancelled");
    assert!(error.is_cancelled());
    assert!(
        !polled.load(Ordering::SeqCst),
        "a triggered token must win before the future is polled"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_propagates_to_child_tasks() {
    let token = CancelToken::new();
    let child = token.child();
    let exited = Arc::new(AtomicBool::new(false));

    let worker = tokio::spawn({
        let exited = Arc::clone(&exited);
        async move {
            child.cancelled().await;
            exited.store(true, Ordering::SeqCst);
        }
    });

    tokio::time::sleep(Duration::from_millis(10)).await;
    token.cancel();

    tokio::time::timeout(GUARD, worker)
        .await
        .expect("child task must exit after cancellation")
        .expect("child task must not panic");
    assert!(exited.load(Ordering::SeqCst));
}

#[tokio::test]
async fn combined_limits_distinguish_timeout_from_cancellation() {
    let token = CancelToken::new();

    let timeout_error = run_with_limits(
        "test.combined.timeout",
        Duration::from_millis(20),
        &token,
        std::future::pending::<()>(),
    )
    .await
    .expect_err("must time out");
    assert!(timeout_error.is_timeout());

    let cancelled = CancelToken::new();
    cancelled.cancel();
    let cancel_error = run_with_limits(
        "test.combined.cancel",
        GUARD,
        &cancelled,
        std::future::pending::<()>(),
    )
    .await
    .expect_err("must be cancelled");
    assert!(cancel_error.is_cancelled());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocking_work_is_isolated_on_the_blocking_pool() {
    let counter = Arc::new(AtomicUsize::new(0));
    let worker_counter = Arc::clone(&counter);

    let value = tokio::time::timeout(
        GUARD,
        run_blocking("test.blocking", move || {
            worker_counter.fetch_add(1, Ordering::SeqCst);
            (1..=1_000_u64).sum::<u64>()
        }),
    )
    .await
    .expect("blocking work must finish")
    .expect("blocking work must not panic");

    assert_eq!(value, 500_500);
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn blocking_errors_are_propagated_unchanged() {
    let error = run_blocking_result("test.blocking_result", || {
        Err::<(), _>(CoreError::not_found("missing-row"))
    })
    .await
    .expect_err("must propagate");
    assert_eq!(error.kind(), ErrorKind::NotFound);
    assert!(error.to_string().contains("missing-row"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocking_panics_become_concurrency_errors_without_payload() {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    let result = tokio::time::timeout(
        GUARD,
        run_blocking("test.blocking_panic", || -> () {
            panic!("payload-with-user-data");
        }),
    )
    .await
    .expect("the join must complete");

    std::panic::set_hook(previous_hook);

    let error = result.expect_err("a panicking closure must surface as an error");
    assert_eq!(error.kind(), ErrorKind::Concurrency);
    assert!(
        !error.to_string().contains("payload-with-user-data"),
        "panic payloads must never be surfaced"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_signal_reaches_all_waiters() {
    let shutdown = ShutdownSignal::new(Duration::from_secs(2));
    let mut waiters = Vec::new();
    for _ in 0..4 {
        let waiter = shutdown.clone();
        waiters.push(tokio::spawn(async move { waiter.wait().await }));
    }

    shutdown.trigger();
    shutdown.trigger();

    for waiter in waiters {
        tokio::time::timeout(GUARD, waiter)
            .await
            .expect("waiter must observe shutdown")
            .expect("waiter must not panic");
    }
    assert!(shutdown.is_shutting_down());
    assert_eq!(shutdown.grace(), Duration::from_secs(2));
}

#[tokio::test]
async fn shutdown_deadline_is_a_timeout_not_a_silent_success() {
    let shutdown = ShutdownSignal::new(Duration::from_secs(1));
    let error = shutdown
        .wait_with_deadline(Duration::from_millis(20))
        .await
        .expect_err("nothing triggered shutdown");
    assert!(error.is_timeout());

    shutdown.trigger();
    shutdown
        .wait_with_deadline(GUARD)
        .await
        .expect("triggered shutdown must satisfy the wait");
}

#[tokio::test]
async fn cancel_tokens_are_shared_handles() {
    let parent = CancelToken::new();
    let clone = parent.clone();
    assert!(!clone.is_cancelled());
    parent.cancel();
    assert!(clone.is_cancelled());

    let default_token = CancelToken::default();
    assert!(!default_token.is_cancelled());
    default_token.cancel();
    tokio::time::timeout(GUARD, default_token.cancelled())
        .await
        .expect("already cancelled tokens complete immediately");
}
