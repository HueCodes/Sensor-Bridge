//! Per-stage metrics tracking.
//!
//! This module provides comprehensive metrics for individual pipeline stages,
//! including latency histograms, jitter tracking, and throughput measurement.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use super::counter::Counter;
use super::jitter::JitterTracker;
use super::latency::LatencyHistogram;

/// Comprehensive metrics for a single pipeline stage.
#[derive(Debug)]
pub struct StageMetricsCollector {
    /// Stage name for identification.
    name: String,
    /// Number of items received.
    input_count: Counter,
    /// Number of items output.
    output_count: Counter,
    /// Number of items filtered (dropped by stage logic).
    filtered_count: Counter,
    /// Number of items dropped due to backpressure.
    dropped_count: Counter,
    /// Number of errors encountered.
    error_count: Counter,
    /// Processing latency histogram.
    latency: LatencyHistogram,
    /// Jitter (variance) tracker.
    jitter: JitterTracker,
    /// Total bytes processed (if applicable).
    bytes_processed: AtomicU64,
    /// Time of first item processed.
    first_item_time: AtomicU64,
    /// Time of last item processed.
    last_item_time: AtomicU64,
    /// Creation time.
    created_at: Instant,
}

impl StageMetricsCollector {
    /// Creates a new stage metrics collector.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            input_count: Counter::new(),
            output_count: Counter::new(),
            filtered_count: Counter::new(),
            dropped_count: Counter::new(),
            error_count: Counter::new(),
            latency: LatencyHistogram::new(),
            jitter: JitterTracker::new(),
            bytes_processed: AtomicU64::new(0),
            first_item_time: AtomicU64::new(0),
            last_item_time: AtomicU64::new(0),
            created_at: Instant::now(),
        }
    }

    /// Returns the stage name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Records an input item.
    #[inline]
    pub fn record_input(&self) {
        self.input_count.increment();
        let now = self.created_at.elapsed().as_nanos() as u64;
        self.first_item_time
            .compare_exchange(0, now, Ordering::Relaxed, Ordering::Relaxed)
            .ok();
        self.last_item_time.store(now, Ordering::Relaxed);
    }

    /// Records an output item with its processing latency.
    #[inline]
    pub fn record_output(&self, latency_ns: u64) {
        self.output_count.increment();
        self.latency.record(latency_ns);
        self.jitter.record(latency_ns);
    }

    /// Records a filtered item.
    #[inline]
    pub fn record_filtered(&self) {
        self.filtered_count.increment();
    }

    /// Records a dropped item.
    #[inline]
    pub fn record_dropped(&self) {
        self.dropped_count.increment();
    }

    /// Records an error.
    #[inline]
    pub fn record_error(&self) {
        self.error_count.increment();
    }

    /// Records bytes processed.
    #[inline]
    pub fn record_bytes(&self, bytes: u64) {
        self.bytes_processed.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Returns the input count.
    #[must_use]
    pub fn input_count(&self) -> u64 {
        self.input_count.get()
    }

    /// Returns the output count.
    #[must_use]
    pub fn output_count(&self) -> u64 {
        self.output_count.get()
    }

    /// Returns the filtered count.
    #[must_use]
    pub fn filtered_count(&self) -> u64 {
        self.filtered_count.get()
    }

    /// Returns the dropped count.
    #[must_use]
    pub fn dropped_count(&self) -> u64 {
        self.dropped_count.get()
    }

    /// Returns the error count.
    #[must_use]
    pub fn error_count(&self) -> u64 {
        self.error_count.get()
    }

    /// Returns the latency histogram.
    #[must_use]
    pub fn latency(&self) -> &LatencyHistogram {
        &self.latency
    }

    /// Returns the jitter tracker.
    #[must_use]
    pub fn jitter(&self) -> &JitterTracker {
        &self.jitter
    }

    /// Returns the total bytes processed.
    #[must_use]
    pub fn bytes_processed(&self) -> u64 {
        self.bytes_processed.load(Ordering::Relaxed)
    }

    /// Returns the elapsed time since creation.
    #[must_use]
    pub fn elapsed(&self) -> std::time::Duration {
        self.created_at.elapsed()
    }

    /// Returns the throughput in items per second.
    #[must_use]
    pub fn throughput(&self) -> f64 {
        let elapsed = self.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.output_count() as f64 / elapsed
        } else {
            0.0
        }
    }

    /// Returns the bytes throughput in bytes per second.
    #[must_use]
    pub fn bytes_throughput(&self) -> f64 {
        let elapsed = self.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.bytes_processed() as f64 / elapsed
        } else {
            0.0
        }
    }

    /// Returns a snapshot of all metrics.
    #[must_use]
    pub fn snapshot(&self) -> StageMetricsSnapshot {
        StageMetricsSnapshot {
            name: self.name.clone(),
            input_count: self.input_count(),
            output_count: self.output_count(),
            filtered_count: self.filtered_count(),
            dropped_count: self.dropped_count(),
            error_count: self.error_count(),
            latency: self.latency.snapshot(),
            jitter: self.jitter.snapshot(),
            bytes_processed: self.bytes_processed(),
            elapsed_secs: self.elapsed().as_secs_f64(),
        }
    }

    /// Resets all metrics.
    pub fn reset(&self) {
        self.input_count.reset();
        self.output_count.reset();
        self.filtered_count.reset();
        self.dropped_count.reset();
        self.error_count.reset();
        self.latency.reset();
        self.jitter.reset();
        self.bytes_processed.store(0, Ordering::Relaxed);
        self.first_item_time.store(0, Ordering::Relaxed);
        self.last_item_time.store(0, Ordering::Relaxed);
    }
}

/// A snapshot of stage metrics.
#[derive(Debug, Clone)]
pub struct StageMetricsSnapshot {
    /// Stage name.
    pub name: String,
    /// Number of items received.
    pub input_count: u64,
    /// Number of items output.
    pub output_count: u64,
    /// Number of items filtered.
    pub filtered_count: u64,
    /// Number of items dropped.
    pub dropped_count: u64,
    /// Number of errors.
    pub error_count: u64,
    /// Latency statistics.
    pub latency: super::latency::LatencySnapshot,
    /// Jitter statistics.
    pub jitter: super::jitter::JitterSnapshot,
    /// Total bytes processed.
    pub bytes_processed: u64,
    /// Elapsed time in seconds.
    pub elapsed_secs: f64,
}

impl StageMetricsSnapshot {
    /// Returns the throughput ratio (output / input).
    #[must_use]
    pub fn throughput_ratio(&self) -> f64 {
        if self.input_count == 0 {
            0.0
        } else {
            self.output_count as f64 / self.input_count as f64
        }
    }

    /// Returns the items per second.
    #[must_use]
    pub fn items_per_second(&self) -> f64 {
        if self.elapsed_secs > 0.0 {
            self.output_count as f64 / self.elapsed_secs
        } else {
            0.0
        }
    }

    /// Returns the filter rate.
    #[must_use]
    pub fn filter_rate(&self) -> f64 {
        if self.input_count == 0 {
            0.0
        } else {
            self.filtered_count as f64 / self.input_count as f64
        }
    }

    /// Returns the drop rate.
    #[must_use]
    pub fn drop_rate(&self) -> f64 {
        if self.input_count == 0 {
            0.0
        } else {
            self.dropped_count as f64 / self.input_count as f64
        }
    }

    /// Returns the error rate.
    #[must_use]
    pub fn error_rate(&self) -> f64 {
        if self.input_count == 0 {
            0.0
        } else {
            self.error_count as f64 / self.input_count as f64
        }
    }
}

impl std::fmt::Display for StageMetricsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: in={} out={} filtered={} dropped={} errors={} p99={:.1}us jitter={:.1}us",
            self.name,
            self.input_count,
            self.output_count,
            self.filtered_count,
            self.dropped_count,
            self.error_count,
            self.latency.p99_us(),
            self.jitter.std_dev_us()
        )
    }
}

/// A timer that records latency when dropped.
pub struct StageTimer<'a> {
    metrics: &'a StageMetricsCollector,
    start: Instant,
    recorded: bool,
}

impl<'a> StageTimer<'a> {
    /// Creates a new stage timer.
    #[must_use]
    pub fn new(metrics: &'a StageMetricsCollector) -> Self {
        Self {
            metrics,
            start: Instant::now(),
            recorded: false,
        }
    }

    /// Records the output with elapsed time and prevents double recording.
    pub fn record_output(mut self) {
        let elapsed = self.start.elapsed().as_nanos() as u64;
        self.metrics.record_output(elapsed);
        self.recorded = true;
    }

    /// Records a filtered item and prevents double recording.
    pub fn record_filtered(mut self) {
        self.metrics.record_filtered();
        self.recorded = true;
    }

    /// Returns the elapsed time without recording.
    #[must_use]
    pub fn elapsed_ns(&self) -> u64 {
        self.start.elapsed().as_nanos() as u64
    }
}

impl Drop for StageTimer<'_> {
    fn drop(&mut self) {
        // If not explicitly recorded, record as dropped
        if !self.recorded {
            self.metrics.record_dropped();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_stage_metrics_collector() {
        let metrics = StageMetricsCollector::new("test");

        assert_eq!(metrics.name(), "test");
        assert_eq!(metrics.input_count(), 0);
        assert_eq!(metrics.output_count(), 0);

        metrics.record_input();
        metrics.record_input();
        metrics.record_output(1000);
        metrics.record_filtered();
        metrics.record_dropped();
        metrics.record_error();
        metrics.record_bytes(100);

        assert_eq!(metrics.input_count(), 2);
        assert_eq!(metrics.output_count(), 1);
        assert_eq!(metrics.filtered_count(), 1);
        assert_eq!(metrics.dropped_count(), 1);
        assert_eq!(metrics.error_count(), 1);
        assert_eq!(metrics.bytes_processed(), 100);
    }

    #[test]
    fn test_stage_metrics_snapshot() {
        let metrics = StageMetricsCollector::new("test");

        for _ in 0..100 {
            metrics.record_input();
        }
        for _ in 0..80 {
            metrics.record_output(1000);
        }
        for _ in 0..10 {
            metrics.record_filtered();
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.input_count, 100);
        assert_eq!(snapshot.output_count, 80);
        assert!((snapshot.throughput_ratio() - 0.8).abs() < 0.01);
        assert!((snapshot.filter_rate() - 0.1).abs() < 0.01);
    }

    #[test]
    fn test_stage_timer() {
        let metrics = StageMetricsCollector::new("test");

        {
            let timer = StageTimer::new(&metrics);
            thread::sleep(Duration::from_micros(100));
            timer.record_output();
        }

        assert_eq!(metrics.output_count(), 1);
        assert!(metrics.latency().count() == 1);
    }

    #[test]
    fn test_stage_timer_dropped() {
        let metrics = StageMetricsCollector::new("test");

        {
            let _timer = StageTimer::new(&metrics);
            // Timer dropped without explicit record
        }

        assert_eq!(metrics.dropped_count(), 1);
    }

    #[test]
    fn test_throughput() {
        let metrics = StageMetricsCollector::new("test");

        for _ in 0..1000 {
            metrics.record_output(100);
        }

        thread::sleep(Duration::from_millis(100));

        let throughput = metrics.throughput();
        assert!(throughput > 0.0);
    }

    #[test]
    fn test_reset() {
        let metrics = StageMetricsCollector::new("test");

        metrics.record_input();
        metrics.record_output(1000);
        metrics.record_bytes(100);

        assert_eq!(metrics.input_count(), 1);

        metrics.reset();

        assert_eq!(metrics.input_count(), 0);
        assert_eq!(metrics.output_count(), 0);
        assert_eq!(metrics.bytes_processed(), 0);
    }
}
