//! Non-blocking write pipeline between the request path and the store.
//!
//! # Why a queue
//!
//! Accounting must never slow down or fail an AI request. [`UsageRecorder`] owns
//! a bounded in-memory queue drained by a background task that writes batches in
//! one transaction. If the queue is full the record is dropped and counted; the
//! request path never blocks and never sees an accounting error.
//!
//! # Lifetime
//!
//! * Records are flushed when the batch is full, when the flush interval elapses,
//!   on [`UsageRecorder::flush`] and when the last handle is dropped.
//! * [`UsageRecorder::shutdown`] flushes and awaits the writer task, which is what
//!   tests and application shutdown should use.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::async_utils::run_blocking_result;
use crate::config::UsageConfig;
use crate::error::{CoreError, CoreResult};
use crate::provider::router::{AttemptObserver, AttemptReport};
use crate::storage::StorageLayout;
use crate::usage::record::UsageRecord;
use crate::usage::store::UsageStore;

/// Message accepted by the writer task.
enum Command {
    Record(Box<UsageRecord>),
    Flush(oneshot::Sender<CoreResult<()>>),
}

/// Buffered usage writer, usable directly as an [`AttemptObserver`].
pub struct UsageRecorder {
    sender: mpsc::Sender<Command>,
    store: Arc<UsageStore>,
    dropped: Arc<AtomicU64>,
    written: Arc<AtomicU64>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for UsageRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UsageRecorder")
            .field("store", &self.store.path().display().to_string())
            .field("written", &self.written())
            .field("dropped", &self.dropped())
            .finish()
    }
}

impl UsageRecorder {
    /// Opens the store described by `layout` and starts the writer task.
    ///
    /// Returns `Ok(None)` when accounting is disabled in `config`, so hosts can
    /// wire the result unconditionally.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] for an invalid pipeline configuration and
    /// [`CoreError::Storage`] when the database cannot be opened.
    pub async fn start(layout: &StorageLayout, config: &UsageConfig) -> CoreResult<Option<Self>> {
        config.validate()?;
        if !config.enabled {
            return Ok(None);
        }
        let store = Arc::new(UsageStore::open_in_layout_async(layout).await?);
        Self::spawn(store, config).map(Some)
    }

    /// Starts the writer task over an already open store.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] for an invalid configuration and
    /// [`CoreError::Internal`] when called outside a Tokio runtime.
    pub fn spawn(store: Arc<UsageStore>, config: &UsageConfig) -> CoreResult<Self> {
        config.validate()?;
        let handle = tokio::runtime::Handle::try_current().map_err(|_| {
            CoreError::internal("the usage recorder must be created inside a Tokio runtime")
        })?;

        let (sender, receiver) = mpsc::channel(config.buffer_capacity);
        let dropped = Arc::new(AtomicU64::new(0));
        let written = Arc::new(AtomicU64::new(0));
        let worker = handle.spawn(run_worker(
            receiver,
            Arc::clone(&store),
            config.batch_size,
            config.flush_interval,
            Arc::clone(&written),
        ));

        Ok(Self {
            sender,
            store,
            dropped,
            written,
            worker: Mutex::new(Some(worker)),
        })
    }

    /// The store backing this recorder (useful for queries).
    #[must_use]
    pub fn store(&self) -> &Arc<UsageStore> {
        &self.store
    }

    /// Records written to the database so far.
    #[must_use]
    pub fn written(&self) -> u64 {
        self.written.load(Ordering::Relaxed)
    }

    /// Records dropped because the queue was full or the writer had stopped.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Enqueues a record without waiting.
    ///
    /// Returns `false` when the record was dropped, which is the only outcome
    /// possible on a saturated queue: accounting never applies backpressure to
    /// the request path.
    pub fn try_record(&self, record: UsageRecord) -> bool {
        match self.sender.try_send(Command::Record(Box::new(record))) {
            Ok(()) => true,
            Err(_) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }

    /// Enqueues a record, waiting for capacity when the queue is full.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Internal`] when the writer task is no longer running.
    pub async fn record(&self, record: UsageRecord) -> CoreResult<()> {
        self.sender
            .send(Command::Record(Box::new(record)))
            .await
            .map_err(|_| {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                CoreError::internal("the usage writer task is not running")
            })
    }

    /// Flushes every buffered record and waits for the write to finish.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Storage`] when the batch cannot be written, or
    /// [`CoreError::Internal`] when the writer task has stopped.
    pub async fn flush(&self) -> CoreResult<()> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::Flush(sender))
            .await
            .map_err(|_| CoreError::internal("the usage writer task is not running"))?;
        receiver
            .await
            .map_err(|_| CoreError::internal("the usage writer task stopped before flushing"))?
    }

    /// Flushes pending records and awaits the writer task.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Internal`] when the writer task panicked or was
    /// cancelled.
    pub async fn shutdown(self) -> CoreResult<()> {
        let Self {
            sender,
            worker,
            store: _,
            dropped: _,
            written: _,
        } = self;
        drop(sender);

        let handle = match worker.lock() {
            Ok(mut guard) => guard.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(handle) = handle {
            handle.await.map_err(|error| {
                CoreError::internal(format!("the usage writer task failed: {error}"))
            })?;
        }
        Ok(())
    }
}

impl AttemptObserver for UsageRecorder {
    fn observe_attempt(&self, report: &AttemptReport) {
        let record = UsageRecord::from_attempt(report);
        if let Err(error) = record.validate() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(error = %error, "dropping an invalid usage record");
            return;
        }
        if !self.try_record(record) {
            tracing::debug!("usage queue is full; dropping a record");
        }
    }
}

/// Drains the queue, writing batches in a single transaction.
async fn run_worker(
    mut receiver: mpsc::Receiver<Command>,
    store: Arc<UsageStore>,
    batch_size: usize,
    flush_interval: Duration,
    written: Arc<AtomicU64>,
) {
    let mut buffer: Vec<UsageRecord> = Vec::with_capacity(batch_size);
    let mut ticker = tokio::time::interval(flush_interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // The first tick of `interval` completes immediately; consume it so the
    // periodic flush starts one interval from now.
    ticker.tick().await;

    loop {
        tokio::select! {
            command = receiver.recv() => match command {
                Some(Command::Record(record)) => {
                    buffer.push(*record);
                    if buffer.len() >= batch_size {
                        // A failed batch is logged and dropped: retrying forever
                        // would keep a poisoned record in front of the queue.
                        let _ = flush(&store, &mut buffer, &written).await;
                    }
                }
                Some(Command::Flush(acknowledge)) => {
                    let result = flush(&store, &mut buffer, &written).await;
                    let _ = acknowledge.send(result);
                }
                None => {
                    let _ = flush(&store, &mut buffer, &written).await;
                    break;
                }
            },
            _ = ticker.tick() => {
                if !buffer.is_empty() {
                    let _ = flush(&store, &mut buffer, &written).await;
                }
            }
        }
    }
}

/// Writes the buffered records on the blocking pool.
async fn flush(
    store: &Arc<UsageStore>,
    buffer: &mut Vec<UsageRecord>,
    written: &AtomicU64,
) -> CoreResult<()> {
    if buffer.is_empty() {
        return Ok(());
    }
    let batch = std::mem::take(buffer);
    let count = batch.len();
    let store = Arc::clone(store);
    match run_blocking_result("usage.write", move || store.insert_many(&batch)).await {
        Ok(inserted) => {
            written.fetch_add(
                u64::try_from(inserted).unwrap_or(u64::MAX),
                Ordering::Relaxed,
            );
            Ok(())
        }
        Err(error) => {
            tracing::warn!(
                error = %error,
                records = count,
                "failed to persist usage records"
            );
            Err(error)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;
    use crate::ids::{RequestId, TraceId};
    use crate::provider::config::ProviderId;
    use crate::provider::response::{RequestStatus, TransportStats, UsageReport};
    use crate::usage::record::{TrafficStats, UsageQuery, UsageRecord};
    use crate::usage::TokenUsage;

    fn layout() -> (tempfile::TempDir, StorageLayout) {
        let directory = tempfile::tempdir().unwrap();
        let layout = StorageLayout::derive(directory.path().join("ai-core")).unwrap();
        (directory, layout)
    }

    fn record(feature: &str) -> UsageRecord {
        UsageRecord {
            request_id: RequestId::new(),
            trace_id: TraceId::new(),
            session_id: None,
            feature: Some(feature.to_owned()),
            provider_id: Some(ProviderId::new("mock").unwrap()),
            model_id: Some("mock-1".to_owned()),
            attempt: 1,
            started_at: SystemTime::now(),
            latency: Duration::from_millis(3),
            status: RequestStatus::Success,
            error_code: None,
            error_message: None,
            tokens: TokenUsage::from_provider(&UsageReport {
                prompt_tokens: Some(7),
                completion_tokens: Some(3),
                ..UsageReport::default()
            }),
            traffic: TrafficStats::from_transport(TransportStats {
                request_bytes: 64,
                response_bytes: 256,
            }),
        }
    }

    #[tokio::test]
    async fn buffered_records_are_flushed_and_queryable() {
        let (_directory, layout) = layout();
        let config = UsageConfig {
            enabled: true,
            buffer_capacity: 16,
            batch_size: 4,
            flush_interval: Duration::from_millis(10),
        };
        let recorder = UsageRecorder::start(&layout, &config)
            .await
            .unwrap()
            .unwrap();

        for _ in 0..6 {
            recorder.record(record("lookup")).await.unwrap();
        }
        recorder.flush().await.unwrap();

        assert_eq!(recorder.written(), 6);
        assert_eq!(recorder.dropped(), 0);
        let aggregate = recorder.store().query(&UsageQuery::default()).unwrap();
        assert_eq!(aggregate.total.attempts, 6);
        assert_eq!(aggregate.total.total_tokens, 60);
        assert_eq!(aggregate.total.request_bytes, 384);

        recorder.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn disabled_accounting_yields_no_recorder() {
        let (_directory, layout) = layout();
        let config = UsageConfig {
            enabled: false,
            ..UsageConfig::default()
        };
        assert!(UsageRecorder::start(&layout, &config)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn shutdown_flushes_pending_records() {
        let (_directory, layout) = layout();
        let config = UsageConfig {
            enabled: true,
            buffer_capacity: 32,
            batch_size: 32,
            flush_interval: Duration::from_secs(30),
        };
        let recorder = UsageRecorder::start(&layout, &config)
            .await
            .unwrap()
            .unwrap();
        recorder.record(record("chat")).await.unwrap();

        let store = Arc::clone(recorder.store());
        recorder.shutdown().await.unwrap();
        assert_eq!(store.count().unwrap(), 1);
    }

    #[tokio::test]
    async fn observer_maps_attempt_reports_into_records() {
        let (_directory, layout) = layout();
        let config = UsageConfig::default();
        let recorder = Arc::new(
            UsageRecorder::start(&layout, &config)
                .await
                .unwrap()
                .unwrap(),
        );

        let report = AttemptReport {
            request_id: RequestId::new(),
            trace_id: TraceId::new(),
            session_id: None,
            feature: Some("vocabulary.lookup".to_owned()),
            provider_id: Some(ProviderId::new("mock").unwrap()),
            model_id: Some("mock-1".to_owned()),
            attempt: 2,
            streaming: false,
            started_at: SystemTime::now(),
            latency: Duration::from_millis(11),
            status: RequestStatus::Degraded,
            error_code: None,
            error_message: None,
            usage: None,
            estimated_tokens: Some(crate::provider::response::EstimatedTokens::new(20, 10)),
            transport: TransportStats {
                request_bytes: 10,
                response_bytes: 20,
            },
        };
        recorder.observe_attempt(&report);
        recorder.flush().await.unwrap();

        let aggregate = recorder.store().query(&UsageQuery::default()).unwrap();
        assert_eq!(aggregate.total.attempts, 1);
        assert_eq!(aggregate.total.degraded, 1);
        assert_eq!(aggregate.total.estimated_tokens, 30);
        assert_eq!(aggregate.total.actual_tokens, 0);
        assert_eq!(aggregate.buckets.len(), 0);

        Arc::try_unwrap(recorder)
            .map_err(|_| ())
            .expect("recorder should be uniquely owned")
            .shutdown()
            .await
            .unwrap();
    }
}
