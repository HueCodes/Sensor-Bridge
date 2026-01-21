//! Metrics collection and observability.
//!
//! This module provides lock-free metrics collection for monitoring
//! pipeline performance:
//!
//! - [`LatencyHistogram`]: Track latency distributions with percentiles
//! - [`Counter`]: Simple atomic counters
//! - [`PipelineCounters`]: Pre-defined counters for pipeline metrics
//!
//! # Example
//!
//! ```rust
//! use sensor_pipeline::metrics::{LatencyHistogram, Counter, PipelineCounters};
//!
//! // Track latency
//! let latency = LatencyHistogram::new();
//! latency.record(1000); // 1000 ns
//! println!("p99: {}ns", latency.p99());
//!
//! // Count events
//! let counter = Counter::new();
//! counter.increment();
//!
//! // Pipeline metrics
//! let metrics = PipelineCounters::new();
//! metrics.received.increment();
//! metrics.processed.increment();
//! ```

mod counter;
mod latency;

pub use counter::{Counter, PipelineCounterSnapshot, PipelineCounters};
pub use latency::{LatencyHistogram, LatencySnapshot};

#[cfg(feature = "std")]
pub use counter::RateCounter;

#[cfg(feature = "std")]
pub use latency::ScopedTimer;

/// Aggregate metrics for the entire pipeline.
#[derive(Debug)]
pub struct PipelineMetrics {
    /// Counters for events.
    pub counters: PipelineCounters,
    /// Latency histogram for end-to-end processing time.
    pub latency: LatencyHistogram,
}

impl PipelineMetrics {
    /// Creates a new metrics collection.
    #[must_use]
    pub fn new() -> Self {
        Self {
            counters: PipelineCounters::new(),
            latency: LatencyHistogram::new(),
        }
    }

    /// Records a successfully processed item with its latency.
    #[inline]
    pub fn record_success(&self, latency_ns: u64) {
        self.counters.processed.increment();
        self.latency.record(latency_ns);
    }

    /// Records a filtered item.
    #[inline]
    pub fn record_filtered(&self) {
        self.counters.filtered.increment();
    }

    /// Records a dropped item.
    #[inline]
    pub fn record_dropped(&self) {
        self.counters.dropped.increment();
    }

    /// Records an error.
    #[inline]
    pub fn record_error(&self) {
        self.counters.errors.increment();
    }

    /// Records a received item.
    #[inline]
    pub fn record_received(&self) {
        self.counters.received.increment();
    }

    /// Resets all metrics.
    pub fn reset(&self) {
        self.counters.reset();
        self.latency.reset();
    }

    /// Returns a snapshot of all metrics.
    #[must_use]
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            counters: self.counters.snapshot(),
            latency: self.latency.snapshot(),
        }
    }
}

impl Default for PipelineMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// A point-in-time snapshot of all pipeline metrics.
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    /// Counter values.
    pub counters: PipelineCounterSnapshot,
    /// Latency statistics.
    pub latency: LatencySnapshot,
}

#[cfg(feature = "std")]
impl std::fmt::Display for MetricsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Counters: {}", self.counters)?;
        write!(f, "Latency: {}", self.latency)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_metrics() {
        let metrics = PipelineMetrics::new();

        metrics.record_received();
        metrics.record_received();
        metrics.record_success(1000);
        metrics.record_filtered();

        let snap = metrics.snapshot();
        assert_eq!(snap.counters.received, 2);
        assert_eq!(snap.counters.processed, 1);
        assert_eq!(snap.counters.filtered, 1);
        assert_eq!(snap.latency.count, 1);
    }
}
