//! Stage worker loop and execution logic.
//!
//! This module provides the core execution loop for pipeline stages,
//! handling input processing, metrics collection, and lifecycle management.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::channel::{Receiver, RecvError, Sender};
use crate::metrics::{Counter, LatencyHistogram};
use crate::stage::Stage;

use super::stage_handle::{StageCommand, StageControl, StageState};

/// Metrics for a single pipeline stage.
#[derive(Debug)]
pub struct StageMetrics {
    /// Number of items received.
    input_count: Counter,
    /// Number of items output.
    output_count: Counter,
    /// Number of items filtered (dropped by stage logic).
    filtered_count: Counter,
    /// Number of items dropped due to backpressure.
    dropped_count: Counter,
    /// Processing latency histogram.
    latency: LatencyHistogram,
    /// Total processing time in nanoseconds.
    total_processing_ns: AtomicU64,
    /// Last processing time in nanoseconds.
    last_processing_ns: AtomicU64,
}

impl StageMetrics {
    /// Creates new stage metrics.
    #[must_use]
    pub fn new() -> Self {
        Self {
            input_count: Counter::new(),
            output_count: Counter::new(),
            filtered_count: Counter::new(),
            dropped_count: Counter::new(),
            latency: LatencyHistogram::new(),
            total_processing_ns: AtomicU64::new(0),
            last_processing_ns: AtomicU64::new(0),
        }
    }

    /// Records processing of an input item.
    #[inline]
    pub fn record_input(&self) {
        self.input_count.increment();
    }

    /// Records an output item.
    #[inline]
    pub fn record_output(&self) {
        self.output_count.increment();
    }

    /// Records a filtered (dropped by stage) item.
    #[inline]
    pub fn record_filtered(&self) {
        self.filtered_count.increment();
    }

    /// Records a dropped item due to backpressure.
    #[inline]
    pub fn record_dropped(&self) {
        self.dropped_count.increment();
    }

    /// Records processing latency.
    #[inline]
    pub fn record_latency(&self, latency_ns: u64) {
        self.latency.record(latency_ns);
        self.total_processing_ns
            .fetch_add(latency_ns, Ordering::Relaxed);
        self.last_processing_ns.store(latency_ns, Ordering::Relaxed);
    }

    /// Returns the input count.
    #[inline]
    #[must_use]
    pub fn input_count(&self) -> u64 {
        self.input_count.get()
    }

    /// Returns the output count.
    #[inline]
    #[must_use]
    pub fn output_count(&self) -> u64 {
        self.output_count.get()
    }

    /// Returns the filtered count.
    #[inline]
    #[must_use]
    pub fn filtered_count(&self) -> u64 {
        self.filtered_count.get()
    }

    /// Returns the dropped count.
    #[inline]
    #[must_use]
    pub fn dropped_count(&self) -> u64 {
        self.dropped_count.get()
    }

    /// Returns the latency histogram.
    #[must_use]
    pub fn latency(&self) -> &LatencyHistogram {
        &self.latency
    }

    /// Returns the total processing time in nanoseconds.
    #[inline]
    #[must_use]
    pub fn total_processing_ns(&self) -> u64 {
        self.total_processing_ns.load(Ordering::Relaxed)
    }

    /// Returns the last processing time in nanoseconds.
    #[inline]
    #[must_use]
    pub fn last_processing_ns(&self) -> u64 {
        self.last_processing_ns.load(Ordering::Relaxed)
    }

    /// Returns a snapshot of the metrics.
    #[must_use]
    pub fn snapshot(&self) -> StageMetricsSnapshot {
        StageMetricsSnapshot {
            input_count: self.input_count(),
            output_count: self.output_count(),
            filtered_count: self.filtered_count(),
            dropped_count: self.dropped_count(),
            latency: self.latency.snapshot(),
            total_processing_ns: self.total_processing_ns(),
        }
    }

    /// Resets all metrics.
    pub fn reset(&self) {
        self.input_count.reset();
        self.output_count.reset();
        self.filtered_count.reset();
        self.dropped_count.reset();
        self.latency.reset();
        self.total_processing_ns.store(0, Ordering::Relaxed);
        self.last_processing_ns.store(0, Ordering::Relaxed);
    }
}

impl Default for StageMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// A snapshot of stage metrics.
#[derive(Debug, Clone)]
pub struct StageMetricsSnapshot {
    /// Number of items received.
    pub input_count: u64,
    /// Number of items output.
    pub output_count: u64,
    /// Number of items filtered.
    pub filtered_count: u64,
    /// Number of items dropped.
    pub dropped_count: u64,
    /// Latency statistics.
    pub latency: crate::metrics::LatencySnapshot,
    /// Total processing time in nanoseconds.
    pub total_processing_ns: u64,
}

impl StageMetricsSnapshot {
    /// Returns the throughput (output items per input items).
    #[must_use]
    pub fn throughput_ratio(&self) -> f64 {
        if self.input_count == 0 {
            0.0
        } else {
            self.output_count as f64 / self.input_count as f64
        }
    }

    /// Returns the filter rate (filtered / input).
    #[must_use]
    pub fn filter_rate(&self) -> f64 {
        if self.input_count == 0 {
            0.0
        } else {
            self.filtered_count as f64 / self.input_count as f64
        }
    }

    /// Returns the drop rate (dropped / input).
    #[must_use]
    pub fn drop_rate(&self) -> f64 {
        if self.input_count == 0 {
            0.0
        } else {
            self.dropped_count as f64 / self.input_count as f64
        }
    }

    /// Returns the average processing time in nanoseconds.
    #[must_use]
    pub fn avg_processing_ns(&self) -> f64 {
        if self.input_count == 0 {
            0.0
        } else {
            self.total_processing_ns as f64 / self.input_count as f64
        }
    }
}

/// The main stage worker loop.
///
/// This function runs in a dedicated thread for each pipeline stage.
/// It receives items from the input channel, processes them through
/// the stage, and sends results to the output channel.
///
/// # Arguments
///
/// * `stage` - The stage to run
/// * `input` - Receiver for input items
/// * `output` - Sender for output items
/// * `control` - Shared control state for commands
/// * `metrics` - Shared metrics collection
pub fn stage_worker<S>(
    mut stage: S,
    input: Receiver<S::Input>,
    output: Sender<S::Output>,
    control: Arc<StageControl>,
    metrics: Arc<StageMetrics>,
) where
    S: Stage,
    S::Input: Send,
    S::Output: Send,
{
    control.set_state(StageState::Running);

    // Receive timeout - short enough for responsive shutdown but long enough
    // to avoid busy-waiting
    let recv_timeout = Duration::from_micros(100);

    loop {
        // Check for shutdown
        if control.is_shutdown() {
            break;
        }

        // Handle pause state
        if control.is_paused() {
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }

        // Check for reset command
        if control.command() == StageCommand::Reset {
            stage.reset();
            metrics.reset();
            control.set_command(StageCommand::Run);
        }

        // Try to receive an item
        match input.recv_timeout(recv_timeout) {
            Ok(item) => {
                metrics.record_input();
                let start = Instant::now();

                // Process the item
                if let Some(result) = stage.process(item) {
                    // Try to send the result
                    if output.send_or_drop(result) {
                        metrics.record_output();
                    } else {
                        metrics.record_dropped();
                    }
                } else {
                    metrics.record_filtered();
                }

                let elapsed_ns = start.elapsed().as_nanos() as u64;
                metrics.record_latency(elapsed_ns);
            }
            Err(RecvError::Empty) => {
                // No input available - try tick
                if let Some(result) = stage.tick() {
                    if output.send_or_drop(result) {
                        metrics.record_output();
                    } else {
                        metrics.record_dropped();
                    }
                }
            }
            Err(RecvError::Disconnected) => {
                // Input channel closed - flush and exit
                break;
            }
        }
    }

    // Flush any remaining buffered data
    control.set_state(StageState::Stopping);
    if let Some(result) = stage.flush() {
        let _ = output.send(result);
        metrics.record_output();
    }

    control.set_state(StageState::Stopped);
}

/// Configuration for the stage worker.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Timeout for receiving input items.
    pub recv_timeout: Duration,
    /// Whether to collect detailed metrics.
    pub collect_metrics: bool,
    /// Batch size for processing (if supported by stage).
    pub batch_size: usize,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            recv_timeout: Duration::from_micros(100),
            collect_metrics: true,
            batch_size: 1,
        }
    }
}

impl WorkerConfig {
    /// Creates a new worker configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the receive timeout.
    #[must_use]
    pub fn recv_timeout(mut self, timeout: Duration) -> Self {
        self.recv_timeout = timeout;
        self
    }

    /// Enables or disables metrics collection.
    #[must_use]
    pub fn collect_metrics(mut self, collect: bool) -> Self {
        self.collect_metrics = collect;
        self
    }

    /// Sets the batch size.
    #[must_use]
    pub fn batch_size(mut self, size: usize) -> Self {
        self.batch_size = size;
        self
    }
}

/// A batch-processing stage worker for higher throughput.
///
/// Collects items into batches before processing to reduce per-item overhead.
#[allow(dead_code)]
pub fn batch_stage_worker<S>(
    mut stage: S,
    input: Receiver<S::Input>,
    output: Sender<S::Output>,
    control: Arc<StageControl>,
    metrics: Arc<StageMetrics>,
    batch_size: usize,
) where
    S: Stage,
    S::Input: Send,
    S::Output: Send,
{
    control.set_state(StageState::Running);

    let recv_timeout = Duration::from_micros(100);
    let mut batch: Vec<S::Input> = Vec::with_capacity(batch_size);

    loop {
        if control.is_shutdown() {
            break;
        }

        if control.is_paused() {
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }

        // Collect a batch
        batch.clear();
        let _batch_start = Instant::now();

        while batch.len() < batch_size {
            match input.recv_timeout(recv_timeout) {
                Ok(item) => {
                    batch.push(item);
                }
                Err(RecvError::Empty) => {
                    // No more items immediately available
                    break;
                }
                Err(RecvError::Disconnected) => {
                    // Process remaining batch and exit
                    control.set_state(StageState::Stopping);
                    break;
                }
            }
        }

        // Process the batch
        if !batch.is_empty() {
            let process_start = Instant::now();

            for item in batch.drain(..) {
                metrics.record_input();

                if let Some(result) = stage.process(item) {
                    if output.send_or_drop(result) {
                        metrics.record_output();
                    } else {
                        metrics.record_dropped();
                    }
                } else {
                    metrics.record_filtered();
                }
            }

            let elapsed_ns = process_start.elapsed().as_nanos() as u64;
            metrics.record_latency(elapsed_ns);
        } else if control.state() != StageState::Stopping {
            // No items in batch, try tick
            if let Some(result) = stage.tick() {
                if output.send_or_drop(result) {
                    metrics.record_output();
                }
            }
        } else {
            // Stopping state with empty batch - exit loop
            break;
        }
    }

    // Flush
    if let Some(result) = stage.flush() {
        let _ = output.send(result);
        metrics.record_output();
    }

    control.set_state(StageState::Stopped);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::Map;
    use std::thread;

    #[test]
    fn test_stage_metrics() {
        let metrics = StageMetrics::new();

        metrics.record_input();
        metrics.record_input();
        metrics.record_output();
        metrics.record_filtered();
        metrics.record_dropped();
        metrics.record_latency(1000);

        assert_eq!(metrics.input_count(), 2);
        assert_eq!(metrics.output_count(), 1);
        assert_eq!(metrics.filtered_count(), 1);
        assert_eq!(metrics.dropped_count(), 1);
        assert_eq!(metrics.total_processing_ns(), 1000);
    }

    #[test]
    fn test_stage_metrics_snapshot() {
        let metrics = StageMetrics::new();

        for _ in 0..100 {
            metrics.record_input();
        }
        for _ in 0..80 {
            metrics.record_output();
        }
        for _ in 0..10 {
            metrics.record_filtered();
        }
        for _ in 0..10 {
            metrics.record_dropped();
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.input_count, 100);
        assert_eq!(snapshot.output_count, 80);
        assert!((snapshot.throughput_ratio() - 0.8).abs() < 0.01);
        assert!((snapshot.filter_rate() - 0.1).abs() < 0.01);
        assert!((snapshot.drop_rate() - 0.1).abs() < 0.01);
    }

    #[test]
    fn test_stage_worker() {
        let stage = Map::new(|x: i32| x * 2);
        let control = Arc::new(StageControl::new());
        let metrics = Arc::new(StageMetrics::new());

        let (input_tx, input_rx) = crate::channel::bounded(16);
        let (output_tx, output_rx) = crate::channel::bounded(16);

        let control_clone = Arc::clone(&control);
        let metrics_clone = Arc::clone(&metrics);

        let worker = thread::spawn(move || {
            stage_worker(stage, input_rx, output_tx, control_clone, metrics_clone);
        });

        // Send some data
        for i in 0..10 {
            input_tx.send(i).unwrap();
        }

        // Wait for processing
        thread::sleep(Duration::from_millis(50));

        // Receive results
        let mut results = vec![];
        while let Ok(v) = output_rx.try_recv() {
            results.push(v);
        }

        assert_eq!(results.len(), 10);
        for (i, v) in results.iter().enumerate() {
            assert_eq!(*v, (i as i32) * 2);
        }

        // Shutdown
        control.request_shutdown();
        drop(input_tx);
        worker.join().unwrap();

        assert_eq!(metrics.input_count(), 10);
        assert_eq!(metrics.output_count(), 10);
    }

    #[test]
    fn test_stage_worker_filtering() {
        use crate::stage::Filter;

        let stage = Filter::new(|x: &i32| *x > 5);
        let control = Arc::new(StageControl::new());
        let metrics = Arc::new(StageMetrics::new());

        let (input_tx, input_rx) = crate::channel::bounded(16);
        let (output_tx, _output_rx) = crate::channel::bounded(16);

        let control_clone = Arc::clone(&control);
        let metrics_clone = Arc::clone(&metrics);

        let worker = thread::spawn(move || {
            stage_worker(stage, input_rx, output_tx, control_clone, metrics_clone);
        });

        // Send data, some will be filtered
        for i in 0..10 {
            input_tx.send(i).unwrap();
        }

        thread::sleep(Duration::from_millis(50));

        control.request_shutdown();
        drop(input_tx);
        worker.join().unwrap();

        assert_eq!(metrics.input_count(), 10);
        assert_eq!(metrics.output_count(), 4); // 6, 7, 8, 9
        assert_eq!(metrics.filtered_count(), 6); // 0, 1, 2, 3, 4, 5
    }

    #[test]
    fn test_worker_config() {
        let config = WorkerConfig::new()
            .recv_timeout(Duration::from_millis(10))
            .collect_metrics(true)
            .batch_size(32);

        assert_eq!(config.recv_timeout, Duration::from_millis(10));
        assert!(config.collect_metrics);
        assert_eq!(config.batch_size, 32);
    }
}
