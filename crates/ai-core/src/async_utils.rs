//! Async execution, cancellation and lifecycle primitives.
//!
//! # Execution rules
//!
//! The framework follows the Tokio contract everywhere:
//!
//! * **Never hold a blocking lock across `.await`.** [`CancelToken`] and
//!   [`crate::ConfigStore`] take their (short) locks in synchronous functions and
//!   release them before returning; no guard ever survives into a future.
//! * **Never run blocking work on the async scheduler.** Filesystem access,
//!   SQLite calls, CPU-heavy tokenization or embedding post-processing must go
//!   through [`run_blocking`] / [`run_blocking_result`], which dispatch to
//!   `tokio::task::spawn_blocking` and translate a join failure into a
//!   [`CoreError::Concurrency`] instead of panicking the caller.
//! * **Cancellation and timeouts are explicit outcomes.** [`with_timeout`],
//!   [`with_cancellation`] and [`run_with_limits`] return
//!   [`CoreError::Timeout`] / [`CoreError::Cancelled`]; a cancelled future is
//!   dropped (releasing its resources) rather than silently reported as success.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::{CoreError, CoreResult};

/// A cloneable cancellation signal shared by a task tree.
///
/// `CancelToken` is a thin, allocation-light wrapper around a
/// [`tokio::sync::watch`] channel: triggering it wakes *all* waiters, and every
/// clone observes the same state. Cloning is cheap (`Arc` bump), which makes it
/// the natural value to pass down into provider calls, streaming loops and
/// memory indexing jobs.
///
/// # Example
///
/// ```
/// use ai_core::async_utils::CancelToken;
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// let token = CancelToken::new();
/// let waiter = token.clone();
/// let handle = tokio::spawn(async move {
///     waiter.cancelled().await;
///     42
/// });
/// token.cancel();
/// assert_eq!(handle.await.unwrap(), 42);
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct CancelToken {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    sender: watch::Sender<bool>,
}

impl CancelToken {
    /// Creates a fresh, untriggered token.
    #[must_use]
    pub fn new() -> Self {
        // The receiver is intentionally dropped: `subscribe()` is only used on
        // demand by waiters, and `watch` keeps the current value regardless of
        // how many receivers exist.
        let (sender, _receiver) = watch::channel(false);
        Self {
            inner: Arc::new(Inner { sender }),
        }
    }

    /// Triggers cancellation; all current and future waiters observe it.
    ///
    /// Triggering twice is a no-op and never fails the caller.
    pub fn cancel(&self) {
        // `send_replace` (not `send`) is required: `send` refuses to store the
        // value when no receiver is currently subscribed, which would let a token
        // cancelled before any waiter subscribed stay "not cancelled" forever.
        self.inner.sender.send_replace(true);
    }

    /// Whether cancellation has already been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        *self.inner.sender.borrow()
    }

    /// Completes once cancellation is requested.
    ///
    /// Returns immediately if the token was already cancelled, so a waiter that
    /// subscribes late can never hang.
    pub async fn cancelled(&self) {
        let mut receiver = self.inner.sender.subscribe();
        if *receiver.borrow() {
            return;
        }
        loop {
            // `Err` means the sender disappeared (only possible when every token
            // clone is gone, i.e. never while `self` is borrowed); treat it as a
            // terminal state so waiters cannot hang.
            if receiver.changed().await.is_err() {
                return;
            }
            if *receiver.borrow() {
                return;
            }
        }
    }

    /// Creates an independently triggerable child token.
    ///
    /// Phase 1 keeps the semantics deliberately simple: the child is a clone of
    /// the shared state. Finer-grained per-task cancellation can be layered on
    /// top in Phase 2 without changing this API.
    #[must_use]
    pub fn child(&self) -> Self {
        self.clone()
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

/// Awaits `future`, failing with [`CoreError::Timeout`] once `limit` elapses.
///
/// The inner future is dropped on timeout, which releases its resources (sockets,
/// file handles, buffer guards).
///
/// # Errors
///
/// [`CoreError::Timeout`] when the deadline is exceeded.
pub async fn with_timeout<F>(
    operation: &'static str,
    limit: Duration,
    future: F,
) -> CoreResult<F::Output>
where
    F: Future,
{
    match tokio::time::timeout(limit, future).await {
        Ok(output) => Ok(output),
        Err(_) => Err(CoreError::timeout(operation, limit)),
    }
}

/// Awaits `future`, failing with [`CoreError::Cancelled`] if `token` is triggered.
///
/// Cancellation is checked with priority (`biased` select), so a token that is
/// already triggered prevents the future from being polled at all.
///
/// # Errors
///
/// [`CoreError::Cancelled`] when the token is (or becomes) triggered first.
pub async fn with_cancellation<F>(
    operation: &'static str,
    token: &CancelToken,
    future: F,
) -> CoreResult<F::Output>
where
    F: Future,
{
    tokio::select! {
        biased;
        () = token.cancelled() => Err(CoreError::cancelled(operation)),
        output = future => Ok(output),
    }
}

/// Applies cancellation **and** a deadline to `future`.
///
/// This is the primitive provider calls, streaming loops and memory indexing jobs
/// are expected to use: it enforces the configured timeout while honouring
/// shutdown.
///
/// # Errors
///
/// [`CoreError::Cancelled`] or [`CoreError::Timeout`], whichever happens first.
pub async fn run_with_limits<F>(
    operation: &'static str,
    limit: Duration,
    token: &CancelToken,
    future: F,
) -> CoreResult<F::Output>
where
    F: Future,
{
    tokio::select! {
        biased;
        () = token.cancelled() => Err(CoreError::cancelled(operation)),
        result = tokio::time::timeout(limit, future) => match result {
            Ok(output) => Ok(output),
            Err(_) => Err(CoreError::timeout(operation, limit)),
        },
    }
}

/// Runs a blocking closure on the Tokio blocking pool and awaits its result.
///
/// Use this for filesystem access, SQLite, CPU-heavy work and any third-party
/// API that is not async. The closure must be `Send + 'static`, and the value it
/// returns must be `Send`; this is what keeps a blocking task from touching
/// non-`Send` async state.
///
/// # Errors
///
/// [`CoreError::Concurrency`] if the task panicked, or [`CoreError::Cancelled`]
/// if the runtime cancelled the task. Panic payloads are deliberately *not*
/// surfaced: they may contain user data.
pub async fn run_blocking<F, T>(operation: &'static str, task: F) -> CoreResult<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(task).await {
        Ok(value) => Ok(value),
        Err(error) => Err(join_error(operation, &error)),
    }
}

/// [`run_blocking`] for closures that already return a [`CoreResult`].
///
/// # Errors
///
/// Propagates the closure's error, or produces the same join errors as
/// [`run_blocking`].
pub async fn run_blocking_result<F, T>(operation: &'static str, task: F) -> CoreResult<T>
where
    F: FnOnce() -> CoreResult<T> + Send + 'static,
    T: Send + 'static,
{
    run_blocking(operation, task).await?
}

fn join_error(operation: &'static str, error: &tokio::task::JoinError) -> CoreError {
    if error.is_cancelled() {
        CoreError::cancelled(operation)
    } else {
        CoreError::concurrency(format!("blocking task `{operation}` did not complete"))
    }
}

/// Coordinates an orderly shutdown of the framework's task tree.
///
/// The signal wraps a [`CancelToken`] plus the configured grace period. Hosts
/// create one instance at startup, hand [`ShutdownSignal::token`] to long-lived
/// tasks and either call [`ShutdownSignal::wait_for_os_signal`] themselves or
/// install a listener with [`ShutdownSignal::spawn_os_signal_listener`].
///
/// # Example
///
/// ```no_run
/// use std::time::Duration;
/// use ai_core::async_utils::ShutdownSignal;
///
/// # async fn run() -> ai_core::CoreResult<()> {
/// let shutdown = ShutdownSignal::new(Duration::from_secs(5));
/// let worker_token = shutdown.token();
/// tokio::spawn(async move {
///     worker_token.cancelled().await;
///     // release resources here
/// });
/// shutdown.wait_for_os_signal().await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct ShutdownSignal {
    token: CancelToken,
    grace: Duration,
}

impl ShutdownSignal {
    /// Creates a signal that grants `grace` to background tasks once triggered.
    #[must_use]
    pub fn new(grace: Duration) -> Self {
        Self {
            token: CancelToken::new(),
            grace,
        }
    }

    /// The cancellation token to distribute to long-lived tasks.
    #[must_use]
    pub fn token(&self) -> CancelToken {
        self.token.clone()
    }

    /// Configured grace period.
    #[must_use]
    pub const fn grace(&self) -> Duration {
        self.grace
    }

    /// Whether shutdown has been requested.
    #[must_use]
    pub fn is_shutting_down(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Requests shutdown; idempotent.
    pub fn trigger(&self) {
        self.token.cancel();
    }

    /// Completes once shutdown has been requested.
    pub async fn wait(&self) {
        self.token.cancelled().await;
    }

    /// Waits for `Ctrl+C`, then requests shutdown.
    ///
    /// # Errors
    ///
    /// [`CoreError::Io`] if the OS signal handler cannot be installed.
    pub async fn wait_for_os_signal(&self) -> CoreResult<()> {
        tokio::signal::ctrl_c().await?;
        self.trigger();
        Ok(())
    }

    /// Spawns a background task waiting for `Ctrl+C`.
    ///
    /// # Errors
    ///
    /// [`CoreError::Internal`] if called outside a Tokio runtime (spawning would
    /// otherwise panic).
    pub fn spawn_os_signal_listener(&self) -> CoreResult<JoinHandle<()>> {
        let handle: Handle = Handle::try_current().map_err(|_| {
            CoreError::internal("a Tokio runtime is required to spawn the shutdown listener")
        })?;
        let signal = self.clone();
        Ok(handle.spawn(async move {
            if let Err(error) = signal.wait_for_os_signal().await {
                tracing::warn!(
                    error = %error,
                    "failed to install shutdown signal listener"
                );
            }
        }))
    }

    /// Waits until shutdown, but gives up after `limit` and returns
    /// [`CoreError::Timeout`] so a caller can force-terminate remaining work.
    ///
    /// # Errors
    ///
    /// [`CoreError::Timeout`] when `limit` elapses first.
    pub async fn wait_with_deadline(&self, limit: Duration) -> CoreResult<()> {
        with_timeout("shutdown.wait", limit, self.wait()).await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn timeout_returns_explicit_error() {
        let result = with_timeout(
            "test.timeout",
            Duration::from_millis(20),
            tokio::time::sleep(Duration::from_secs(30)),
        )
        .await;
        let error = result.unwrap_err();
        assert!(error.is_timeout());
        assert_eq!(error.kind(), crate::ErrorKind::Timeout);
        assert!(error.to_string().contains("test.timeout"));
    }

    #[tokio::test]
    async fn timeout_passes_through_success() {
        let value = with_timeout("test.ok", Duration::from_secs(1), async { 7_u8 })
            .await
            .unwrap();
        assert_eq!(value, 7);
    }

    #[tokio::test]
    async fn cancellation_wakes_waiters() {
        let token = CancelToken::new();
        let waiter = token.clone();
        let handle = tokio::spawn(async move {
            waiter.cancelled().await;
            "woke"
        });

        token.cancel();
        let joined = tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(joined, "woke");
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn late_waiter_returns_immediately() {
        let token = CancelToken::new();
        token.cancel();
        tokio::time::timeout(Duration::from_millis(50), token.cancelled())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn cancellation_aborts_long_running_future() {
        let token = CancelToken::new();
        let trigger = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            trigger.cancel();
        });

        let error = with_cancellation(
            "test.cancel",
            &token,
            tokio::time::sleep(Duration::from_secs(30)),
        )
        .await
        .unwrap_err();
        assert!(error.is_cancelled());
    }

    #[tokio::test]
    async fn combined_limits_prefer_cancellation() {
        let token = CancelToken::new();
        token.cancel();
        let error = run_with_limits("test.combined", Duration::from_secs(30), &token, async {})
            .await
            .unwrap_err();
        assert!(error.is_cancelled());
    }

    #[tokio::test]
    async fn combined_limits_still_enforce_timeout() {
        let token = CancelToken::new();
        let error = run_with_limits(
            "test.combined_timeout",
            Duration::from_millis(10),
            &token,
            tokio::time::sleep(Duration::from_secs(30)),
        )
        .await
        .unwrap_err();
        assert!(error.is_timeout());
    }

    #[tokio::test]
    async fn blocking_work_runs_off_runtime() {
        let sum = run_blocking("test.blocking", || (1..=100).sum::<u64>())
            .await
            .unwrap();
        assert_eq!(sum, 5050);

        let failure = run_blocking_result("test.blocking_err", || {
            Err::<(), _>(CoreError::not_found("demo"))
        })
        .await
        .unwrap_err();
        assert_eq!(failure.kind(), crate::ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn blocking_panic_is_translated_to_concurrency_error() {
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let result = run_blocking("test.blocking_panic", || -> () { panic!("boom") }).await;
        std::panic::set_hook(previous_hook);

        let error = result.unwrap_err();
        assert_eq!(error.kind(), crate::ErrorKind::Concurrency);
        // The panic payload must not be surfaced.
        assert!(!error.to_string().contains("boom"));
    }

    #[tokio::test]
    async fn shutdown_signal_triggers_waiters() {
        let shutdown = ShutdownSignal::new(Duration::from_secs(1));
        assert!(!shutdown.is_shutting_down());
        let waiter = shutdown.clone();
        let handle = tokio::spawn(async move { waiter.wait().await });

        shutdown.trigger();
        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .unwrap()
            .unwrap();
        assert!(shutdown.is_shutting_down());
        assert_eq!(shutdown.grace(), Duration::from_secs(1));
    }

    #[tokio::test]
    async fn shutdown_deadline_is_enforced() {
        let shutdown = ShutdownSignal::new(Duration::from_secs(1));
        let error = shutdown
            .wait_with_deadline(Duration::from_millis(10))
            .await
            .unwrap_err();
        assert!(error.is_timeout());
    }
}
