//! Aggregated pipeline metrics.
//!
//! This module provides end-to-end metrics aggregation for the entire pipeline,
//! combining metrics from individual stages to give a holistic view.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use super::jitter::JitterTracker;
use super::latency::LatencyHistogram;
use super::stage_metrics::{StageMetricsCollector, StageMetricsSnapshot};

/// Aggregated metrics for the entire pipeline.
#[derive(Debug)]
pub struct PipelineMetricsAggregator {
    /// Individual stage metrics.
    stages: Vec<Arc<StageMetricsCollector>>,
    /// End-to-end latency histogram.
    end_to_end_latency: LatencyHistogram,
    /// End-to-end jitter tracker.
    end_to_end_jitter: JitterTracker,
    /// Total items that entered the pipeline.
    total_input: AtomicU64,
    /// Total items that exited the pipeline.
    total_output: AtomicU64,
    /// Pipeline start time.
    start_time: Instant,
}

impl PipelineMetricsAggregator {
    /// Creates a new pipeline metrics aggregator.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stages: Vec::new(),
            end_to_end_latency: LatencyHistogram::new(),
            end_to_end_jitter: JitterTracker::new(),
            total_input: AtomicU64::new(0),
            total_output: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    /// Adds a stage to track.
    pub fn add_stage(&mut self, stage: Arc<StageMetricsCollector>) {
        self.stages.push(stage);
    }

    /// Records an item entering the pipeline.
    #[inline]
    pub fn record_input(&self) {
        self.total_input.fetch_add(1, Ordering::Relaxed);
    }

    /// Records an item exiting the pipeline with end-to-end latency.
    #[inline]
    pub fn record_output(&self, latency_ns: u64) {
        self.total_output.fetch_add(1, Ordering::Relaxed);
        self.end_to_end_latency.record(latency_ns);
        self.end_to_end_jitter.record(latency_ns);
    }

    /// Returns the total input count.
    #[must_use]
    pub fn total_input(&self) -> u64 {
        self.total_input.load(Ordering::Relaxed)
    }

    /// Returns the total output count.
    #[must_use]
    pub fn total_output(&self) -> u64 {
        self.total_output.load(Ordering::Relaxed)
    }

    /// Returns the end-to-end latency histogram.
    #[must_use]
    pub fn end_to_end_latency(&self) -> &LatencyHistogram {
        &self.end_to_end_latency
    }

    /// Returns the end-to-end jitter tracker.
    #[must_use]
    pub fn end_to_end_jitter(&self) -> &JitterTracker {
        &self.end_to_end_jitter
    }

    /// Returns snapshots of all stage metrics.
    #[must_use]
    pub fn stage_snapshots(&self) -> Vec<StageMetricsSnapshot> {
        self.stages.iter().map(|s| s.snapshot()).collect()
    }

    /// Returns the overall pipeline throughput in items per second.
    #[must_use]
    pub fn throughput(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.total_output() as f64 / elapsed
        } else {
            0.0
        }
    }

    /// Returns the elapsed time since pipeline start.
    #[must_use]
    pub fn elapsed(&self) -> std::time::Duration {
        self.start_time.elapsed()
    }

    /// Returns a complete snapshot of pipeline metrics.
    #[must_use]
    pub fn snapshot(&self) -> PipelineMetricsSnapshot {
        PipelineMetricsSnapshot {
            stages: self.stage_snapshots(),
            end_to_end_latency: self.end_to_end_latency.snapshot(),
            end_to_end_jitter: self.end_to_end_jitter.snapshot(),
            total_input: self.total_input(),
            total_output: self.total_output(),
            elapsed_secs: self.start_time.elapsed().as_secs_f64(),
        }
    }

    /// Resets all metrics.
    pub fn reset(&self) {
        for stage in &self.stages {
            stage.reset();
        }
        self.end_to_end_latency.reset();
        self.end_to_end_jitter.reset();
        self.total_input.store(0, Ordering::Relaxed);
        self.total_output.store(0, Ordering::Relaxed);
    }
}

impl Default for PipelineMetricsAggregator {
    fn default() -> Self {
        Self::new()
    }
}

/// A snapshot of complete pipeline metrics.
#[derive(Debug, Clone)]
pub struct PipelineMetricsSnapshot {
    /// Per-stage metrics.
    pub stages: Vec<StageMetricsSnapshot>,
    /// End-to-end latency statistics.
    pub end_to_end_latency: super::latency::LatencySnapshot,
    /// End-to-end jitter statistics.
    pub end_to_end_jitter: super::jitter::JitterSnapshot,
    /// Total items input to pipeline.
    pub total_input: u64,
    /// Total items output from pipeline.
    pub total_output: u64,
    /// Elapsed time in seconds.
    pub elapsed_secs: f64,
}

impl PipelineMetricsSnapshot {
    /// Returns the overall throughput in items per second.
    #[must_use]
    pub fn throughput(&self) -> f64 {
        if self.elapsed_secs > 0.0 {
            self.total_output as f64 / self.elapsed_secs
        } else {
            0.0
        }
    }

    /// Returns the overall drop rate.
    #[must_use]
    pub fn drop_rate(&self) -> f64 {
        if self.total_input == 0 {
            0.0
        } else {
            1.0 - (self.total_output as f64 / self.total_input as f64)
        }
    }

    /// Returns the p99 end-to-end latency in microseconds.
    #[must_use]
    pub fn p99_latency_us(&self) -> f64 {
        self.end_to_end_latency.p99_us()
    }

    /// Returns the jitter (standard deviation) in microseconds.
    #[must_use]
    pub fn jitter_us(&self) -> f64 {
        self.end_to_end_jitter.std_dev_us()
    }

    /// Returns the jitter in milliseconds.
    #[must_use]
    pub fn jitter_ms(&self) -> f64 {
        self.end_to_end_jitter.std_dev_ms()
    }

    /// Checks if the pipeline meets performance targets.
    pub fn check_targets(&self, targets: &PerformanceTargets) -> TargetCheck {
        TargetCheck {
            throughput_ok: self.throughput() >= targets.min_throughput,
            latency_ok: self.p99_latency_us() <= targets.max_p99_latency_us,
            jitter_ok: self.jitter_ms() <= targets.max_jitter_ms,
            actual_throughput: self.throughput(),
            actual_p99_latency_us: self.p99_latency_us(),
            actual_jitter_ms: self.jitter_ms(),
        }
    }
}

impl std::fmt::Display for PipelineMetricsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Pipeline Metrics:")?;
        writeln!(f, "  Throughput: {:.1} items/sec", self.throughput())?;
        writeln!(
            f,
            "  Total: {} in, {} out",
            self.total_input, self.total_output
        )?;
        writeln!(f, "  Drop rate: {:.2}%", self.drop_rate() * 100.0)?;
        writeln!(f, "  End-to-end p99: {:.1}us", self.p99_latency_us())?;
        writeln!(f, "  Jitter: {:.2}ms", self.jitter_ms())?;
        writeln!(f, "  Elapsed: {:.2}s", self.elapsed_secs)?;
        writeln!(f)?;
        writeln!(f, "Per-stage:")?;
        for stage in &self.stages {
            writeln!(f, "  {}", stage)?;
        }
        Ok(())
    }
}

/// Performance targets for the pipeline.
#[derive(Debug, Clone)]
pub struct PerformanceTargets {
    /// Minimum throughput in items per second.
    pub min_throughput: f64,
    /// Maximum p99 latency in microseconds.
    pub max_p99_latency_us: f64,
    /// Maximum jitter (std dev) in milliseconds.
    pub max_jitter_ms: f64,
}

impl Default for PerformanceTargets {
    fn default() -> Self {
        Self {
            min_throughput: 10_000.0,     // 10K items/sec
            max_p99_latency_us: 10_000.0, // 10ms = 10,000us
            max_jitter_ms: 2.0,           // 2ms std dev
        }
    }
}

impl PerformanceTargets {
    /// Creates new performance targets.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the minimum throughput.
    #[must_use]
    pub fn min_throughput(mut self, throughput: f64) -> Self {
        self.min_throughput = throughput;
        self
    }

    /// Sets the maximum p99 latency in microseconds.
    #[must_use]
    pub fn max_p99_latency_us(mut self, latency_us: f64) -> Self {
        self.max_p99_latency_us = latency_us;
        self
    }

    /// Sets the maximum p99 latency in milliseconds.
    #[must_use]
    pub fn max_p99_latency_ms(mut self, latency_ms: f64) -> Self {
        self.max_p99_latency_us = latency_ms * 1000.0;
        self
    }

    /// Sets the maximum jitter in milliseconds.
    #[must_use]
    pub fn max_jitter_ms(mut self, jitter_ms: f64) -> Self {
        self.max_jitter_ms = jitter_ms;
        self
    }
}

/// Result of checking performance against targets.
#[derive(Debug, Clone)]
pub struct TargetCheck {
    /// Whether throughput meets target.
    pub throughput_ok: bool,
    /// Whether latency meets target.
    pub latency_ok: bool,
    /// Whether jitter meets target.
    pub jitter_ok: bool,
    /// Actual throughput.
    pub actual_throughput: f64,
    /// Actual p99 latency in microseconds.
    pub actual_p99_latency_us: f64,
    /// Actual jitter in milliseconds.
    pub actual_jitter_ms: f64,
}

impl TargetCheck {
    /// Returns whether all targets are met.
    #[must_use]
    pub fn all_ok(&self) -> bool {
        self.throughput_ok && self.latency_ok && self.jitter_ok
    }
}

impl std::fmt::Display for TargetCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ok = |b: bool| if b { "OK" } else { "FAIL" };
        write!(
            f,
            "Throughput: {:.1} items/sec [{}], P99 Latency: {:.1}us [{}], Jitter: {:.2}ms [{}]",
            self.actual_throughput,
            ok(self.throughput_ok),
            self.actual_p99_latency_us,
            ok(self.latency_ok),
            self.actual_jitter_ms,
            ok(self.jitter_ok)
        )
    }
}

/// A helper for tracking end-to-end latency of items through the pipeline.
pub struct LatencyTracker {
    metrics: Arc<PipelineMetricsAggregator>,
}

impl LatencyTracker {
    /// Creates a new latency tracker.
    #[must_use]
    pub fn new(metrics: Arc<PipelineMetricsAggregator>) -> Self {
        Self { metrics }
    }

    /// Starts tracking latency for an item.
    #[must_use]
    pub fn start(&self) -> LatencyToken {
        self.metrics.record_input();
        LatencyToken {
            metrics: Arc::clone(&self.metrics),
            start: Instant::now(),
            recorded: false,
        }
    }
}

/// A token for tracking an item's end-to-end latency.
pub struct LatencyToken {
    metrics: Arc<PipelineMetricsAggregator>,
    start: Instant,
    recorded: bool,
}

impl LatencyToken {
    /// Records the item as successfully processed.
    pub fn complete(mut self) {
        let latency = self.start.elapsed().as_nanos() as u64;
        self.metrics.record_output(latency);
        self.recorded = true;
    }

    /// Returns the current elapsed time without recording.
    #[must_use]
    pub fn elapsed_ns(&self) -> u64 {
        self.start.elapsed().as_nanos() as u64
    }
}

impl Drop for LatencyToken {
    fn drop(&mut self) {
        // If not recorded, the item was lost
        // We don't record it as output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_metrics_aggregator() {
        let mut aggregator = PipelineMetricsAggregator::new();

        let stage1 = Arc::new(StageMetricsCollector::new("stage1"));
        let stage2 = Arc::new(StageMetricsCollector::new("stage2"));

        aggregator.add_stage(Arc::clone(&stage1));
        aggregator.add_stage(Arc::clone(&stage2));

        // Simulate processing
        for _ in 0..100 {
            aggregator.record_input();
            stage1.record_input();
            stage1.record_output(1000);
            stage2.record_input();
            stage2.record_output(500);
            aggregator.record_output(1500);
        }

        assert_eq!(aggregator.total_input(), 100);
        assert_eq!(aggregator.total_output(), 100);

        let snapshot = aggregator.snapshot();
        assert_eq!(snapshot.stages.len(), 2);
        assert_eq!(snapshot.total_input, 100);
        assert_eq!(snapshot.total_output, 100);
    }

    #[test]
    fn test_performance_targets() {
        let targets = PerformanceTargets::new()
            .min_throughput(5000.0)
            .max_p99_latency_ms(5.0)
            .max_jitter_ms(1.0);

        assert_eq!(targets.min_throughput, 5000.0);
        assert_eq!(targets.max_p99_latency_us, 5000.0);
        assert_eq!(targets.max_jitter_ms, 1.0);
    }

    #[test]
    fn test_target_check() {
        let snapshot = PipelineMetricsSnapshot {
            stages: vec![],
            end_to_end_latency: super::super::latency::LatencySnapshot {
                count: 100,
                mean_ns: 5000.0,
                min_ns: Some(1000),
                max_ns: Some(10000),
                p50_ns: 5000,
                p95_ns: 8000,
                p99_ns: 9000,
                p999_ns: 9500,
            },
            end_to_end_jitter: super::super::jitter::JitterSnapshot {
                count: 100,
                mean_ns: 5000.0,
                std_dev_ns: 1_000_000.0, // 1ms
                min_ns: Some(1000),
                max_ns: Some(10000),
            },
            total_input: 1000,
            total_output: 1000,
            elapsed_secs: 0.1,
        };

        let targets = PerformanceTargets::new()
            .min_throughput(5000.0)
            .max_p99_latency_us(10.0)
            .max_jitter_ms(2.0);

        let check = snapshot.check_targets(&targets);
        assert!(check.throughput_ok); // 1000/0.1 = 10000 > 5000
        assert!(check.latency_ok); // 9us < 10us
        assert!(check.jitter_ok); // 1ms < 2ms
        assert!(check.all_ok());
    }

    #[test]
    fn test_latency_tracker() {
        let metrics = Arc::new(PipelineMetricsAggregator::new());
        let tracker = LatencyTracker::new(Arc::clone(&metrics));

        {
            let token = tracker.start();
            std::thread::sleep(std::time::Duration::from_micros(100));
            token.complete();
        }

        assert_eq!(metrics.total_input(), 1);
        assert_eq!(metrics.total_output(), 1);
        assert!(metrics.end_to_end_latency().count() == 1);
    }
}
