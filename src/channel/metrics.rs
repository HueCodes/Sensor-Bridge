//! Channel metrics tracking.
//!
//! Provides atomic counters for tracking channel throughput, queue depth,
//! and other performance metrics without adding lock contention.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Metrics for a single channel.
///
/// All operations are lock-free and use atomic operations for thread safety.
/// The metrics are designed to have minimal overhead on the hot path.
#[derive(Debug)]
pub struct ChannelMetrics {
    /// Total items sent through the channel.
    sent: AtomicU64,
    /// Total items received from the channel.
    received: AtomicU64,
    /// Items dropped due to backpressure.
    dropped: AtomicU64,
    /// Current approximate queue depth.
    current_depth: AtomicUsize,
    /// Maximum observed queue depth (high water mark).
    max_depth: AtomicUsize,
    /// Channel capacity.
    capacity: usize,
}

impl ChannelMetrics {
    /// Creates new channel metrics with the given capacity.
    #[must_use]
    pub const fn new(capacity: usize) -> Self {
        Self {
            sent: AtomicU64::new(0),
            received: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            current_depth: AtomicUsize::new(0),
            max_depth: AtomicUsize::new(0),
            capacity,
        }
    }

    /// Records a successful send operation.
    #[inline]
    pub fn record_send(&self) {
        self.sent.fetch_add(1, Ordering::Relaxed);
        let new_depth = self.current_depth.fetch_add(1, Ordering::Relaxed) + 1;

        // Update max depth if necessary (relaxed ordering is fine here)
        let mut max = self.max_depth.load(Ordering::Relaxed);
        while new_depth > max {
            match self.max_depth.compare_exchange_weak(
                max,
                new_depth,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(current) => max = current,
            }
        }
    }

    /// Records a successful receive operation.
    #[inline]
    pub fn record_receive(&self) {
        self.received.fetch_add(1, Ordering::Relaxed);
        // Saturating sub to handle race conditions where depth could go negative
        let _ = self.current_depth.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |d| {
            Some(d.saturating_sub(1))
        });
    }

    /// Records a dropped item due to backpressure.
    #[inline]
    pub fn record_drop(&self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns the total number of items sent.
    #[inline]
    #[must_use]
    pub fn sent(&self) -> u64 {
        self.sent.load(Ordering::Relaxed)
    }

    /// Returns the total number of items received.
    #[inline]
    #[must_use]
    pub fn received(&self) -> u64 {
        self.received.load(Ordering::Relaxed)
    }

    /// Returns the total number of dropped items.
    #[inline]
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Returns the current approximate queue depth.
    #[inline]
    #[must_use]
    pub fn current_depth(&self) -> usize {
        self.current_depth.load(Ordering::Relaxed)
    }

    /// Returns the maximum observed queue depth.
    #[inline]
    #[must_use]
    pub fn max_depth(&self) -> usize {
        self.max_depth.load(Ordering::Relaxed)
    }

    /// Returns the channel capacity.
    #[inline]
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the current utilization as a percentage (0-100).
    #[inline]
    #[must_use]
    pub fn utilization(&self) -> u8 {
        if self.capacity == usize::MAX {
            0 // Unbounded channel
        } else {
            let depth = self.current_depth.load(Ordering::Relaxed);
            ((depth * 100) / self.capacity).min(100) as u8
        }
    }

    /// Returns whether the channel is under backpressure.
    ///
    /// A channel is considered under backpressure when utilization exceeds 80%.
    #[inline]
    #[must_use]
    pub fn is_under_pressure(&self) -> bool {
        self.utilization() >= 80
    }

    /// Resets all counters to zero.
    pub fn reset(&self) {
        self.sent.store(0, Ordering::Relaxed);
        self.received.store(0, Ordering::Relaxed);
        self.dropped.store(0, Ordering::Relaxed);
        self.current_depth.store(0, Ordering::Relaxed);
        self.max_depth.store(0, Ordering::Relaxed);
    }

    /// Returns a snapshot of all metrics.
    #[must_use]
    pub fn snapshot(&self) -> ChannelMetricsSnapshot {
        ChannelMetricsSnapshot {
            sent: self.sent(),
            received: self.received(),
            dropped: self.dropped(),
            current_depth: self.current_depth(),
            max_depth: self.max_depth(),
            capacity: self.capacity,
        }
    }
}

/// A point-in-time snapshot of channel metrics.
#[derive(Debug, Clone, Copy)]
pub struct ChannelMetricsSnapshot {
    /// Total items sent.
    pub sent: u64,
    /// Total items received.
    pub received: u64,
    /// Total items dropped.
    pub dropped: u64,
    /// Current queue depth.
    pub current_depth: usize,
    /// Maximum queue depth observed.
    pub max_depth: usize,
    /// Channel capacity.
    pub capacity: usize,
}

impl ChannelMetricsSnapshot {
    /// Returns the throughput (items received).
    #[inline]
    #[must_use]
    pub fn throughput(&self) -> u64 {
        self.received
    }

    /// Returns the drop rate as a percentage.
    #[inline]
    #[must_use]
    pub fn drop_rate(&self) -> f64 {
        if self.sent == 0 {
            0.0
        } else {
            (self.dropped as f64 / self.sent as f64) * 100.0
        }
    }

    /// Returns the utilization as a percentage.
    #[inline]
    #[must_use]
    pub fn utilization(&self) -> f64 {
        if self.capacity == usize::MAX {
            0.0
        } else {
            (self.current_depth as f64 / self.capacity as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_basic() {
        let metrics = ChannelMetrics::new(100);

        assert_eq!(metrics.sent(), 0);
        assert_eq!(metrics.received(), 0);
        assert_eq!(metrics.current_depth(), 0);

        metrics.record_send();
        metrics.record_send();
        metrics.record_send();

        assert_eq!(metrics.sent(), 3);
        assert_eq!(metrics.current_depth(), 3);
        assert_eq!(metrics.max_depth(), 3);

        metrics.record_receive();
        assert_eq!(metrics.received(), 1);
        assert_eq!(metrics.current_depth(), 2);
        assert_eq!(metrics.max_depth(), 3); // Max should not decrease
    }

    #[test]
    fn test_metrics_dropped() {
        let metrics = ChannelMetrics::new(10);

        metrics.record_drop();
        metrics.record_drop();

        assert_eq!(metrics.dropped(), 2);
    }

    #[test]
    fn test_metrics_utilization() {
        let metrics = ChannelMetrics::new(100);

        for _ in 0..80 {
            metrics.record_send();
        }

        assert_eq!(metrics.utilization(), 80);
        assert!(metrics.is_under_pressure());

        for _ in 0..30 {
            metrics.record_receive();
        }

        assert_eq!(metrics.utilization(), 50);
        assert!(!metrics.is_under_pressure());
    }

    #[test]
    fn test_metrics_snapshot() {
        let metrics = ChannelMetrics::new(100);

        metrics.record_send();
        metrics.record_send();
        metrics.record_receive();
        metrics.record_drop();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.sent, 2);
        assert_eq!(snapshot.received, 1);
        assert_eq!(snapshot.dropped, 1);
        assert_eq!(snapshot.current_depth, 1);
    }

    #[test]
    fn test_metrics_reset() {
        let metrics = ChannelMetrics::new(100);

        metrics.record_send();
        metrics.record_receive();
        metrics.record_drop();

        metrics.reset();

        assert_eq!(metrics.sent(), 0);
        assert_eq!(metrics.received(), 0);
        assert_eq!(metrics.dropped(), 0);
        assert_eq!(metrics.current_depth(), 0);
    }
}
