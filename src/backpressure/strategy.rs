//! Backpressure strategies for handling buffer overflow.
//!
//! Different strategies for what to do when a buffer is full.

use std::sync::atomic::{AtomicU64, Ordering};

/// Strategy for handling backpressure when buffers are full.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackpressureStrategy {
    /// Block until space is available.
    /// Good for batch processing where all data must be processed.
    Block,

    /// Drop the newest item (the one being added).
    /// Preserves order and older data.
    #[default]
    DropNewest,

    /// Drop the oldest item to make room for the new one.
    /// Prioritizes the most recent data.
    DropOldest,

    /// Sample items - only accept every Nth item.
    /// Useful for reducing data rate while maintaining coverage.
    Sample {
        /// Sample rate (1 = keep all, 2 = keep every other, etc.)
        rate: u32,
    },

    /// Adaptive strategy that adjusts based on load.
    /// Starts with Block and progressively drops more under pressure.
    Adaptive,
}

impl BackpressureStrategy {
    /// Creates a sampling strategy.
    #[must_use]
    pub const fn sample(rate: u32) -> Self {
        let rate = if rate < 1 { 1 } else { rate };
        Self::Sample { rate }
    }

    /// Returns whether this strategy may block.
    #[must_use]
    pub const fn may_block(&self) -> bool {
        matches!(self, Self::Block)
    }

    /// Returns whether this strategy may drop items.
    #[must_use]
    pub const fn may_drop(&self) -> bool {
        matches!(
            self,
            Self::DropNewest | Self::DropOldest | Self::Sample { .. } | Self::Adaptive
        )
    }
}

/// Tracks statistics for backpressure decisions.
#[derive(Debug)]
pub struct BackpressureStats {
    /// Number of items accepted.
    accepted: AtomicU64,
    /// Number of items dropped.
    dropped: AtomicU64,
    /// Number of times blocked.
    blocked: AtomicU64,
    /// Number of times sampled out.
    sampled_out: AtomicU64,
}

impl BackpressureStats {
    /// Creates new backpressure statistics.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            accepted: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            blocked: AtomicU64::new(0),
            sampled_out: AtomicU64::new(0),
        }
    }

    /// Records an accepted item.
    #[inline]
    pub fn record_accepted(&self) {
        self.accepted.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a dropped item.
    #[inline]
    pub fn record_dropped(&self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a blocked operation.
    #[inline]
    pub fn record_blocked(&self) {
        self.blocked.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a sampled-out item.
    #[inline]
    pub fn record_sampled_out(&self) {
        self.sampled_out.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns the number of accepted items.
    #[must_use]
    pub fn accepted(&self) -> u64 {
        self.accepted.load(Ordering::Relaxed)
    }

    /// Returns the number of dropped items.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Returns the number of blocked operations.
    #[must_use]
    pub fn blocked(&self) -> u64 {
        self.blocked.load(Ordering::Relaxed)
    }

    /// Returns the number of sampled-out items.
    #[must_use]
    pub fn sampled_out(&self) -> u64 {
        self.sampled_out.load(Ordering::Relaxed)
    }

    /// Returns the total number of items seen.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.accepted() + self.dropped() + self.sampled_out()
    }

    /// Returns the drop rate (dropped / total).
    #[must_use]
    pub fn drop_rate(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            0.0
        } else {
            (self.dropped() + self.sampled_out()) as f64 / total as f64
        }
    }

    /// Returns a snapshot of the statistics.
    #[must_use]
    pub fn snapshot(&self) -> BackpressureStatsSnapshot {
        BackpressureStatsSnapshot {
            accepted: self.accepted(),
            dropped: self.dropped(),
            blocked: self.blocked(),
            sampled_out: self.sampled_out(),
        }
    }

    /// Resets all statistics.
    pub fn reset(&self) {
        self.accepted.store(0, Ordering::Relaxed);
        self.dropped.store(0, Ordering::Relaxed);
        self.blocked.store(0, Ordering::Relaxed);
        self.sampled_out.store(0, Ordering::Relaxed);
    }
}

impl Default for BackpressureStats {
    fn default() -> Self {
        Self::new()
    }
}

/// A snapshot of backpressure statistics.
#[derive(Debug, Clone)]
pub struct BackpressureStatsSnapshot {
    /// Number of items accepted.
    pub accepted: u64,
    /// Number of items dropped.
    pub dropped: u64,
    /// Number of times blocked.
    pub blocked: u64,
    /// Number of items sampled out.
    pub sampled_out: u64,
}

impl BackpressureStatsSnapshot {
    /// Returns the total items processed.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.accepted + self.dropped + self.sampled_out
    }

    /// Returns the acceptance rate.
    #[must_use]
    pub fn acceptance_rate(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            1.0
        } else {
            self.accepted as f64 / total as f64
        }
    }
}

impl std::fmt::Display for BackpressureStatsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "accepted={} dropped={} sampled_out={} blocked={} rate={:.1}%",
            self.accepted,
            self.dropped,
            self.sampled_out,
            self.blocked,
            self.acceptance_rate() * 100.0
        )
    }
}

/// A sampler that accepts every Nth item.
#[allow(dead_code)]
#[derive(Debug)]
pub struct Sampler {
    rate: u32,
    counter: AtomicU64,
}

#[allow(dead_code)]
impl Sampler {
    /// Creates a new sampler with the given rate.
    ///
    /// Rate of 1 accepts all items, rate of 2 accepts every other item, etc.
    #[must_use]
    pub fn new(rate: u32) -> Self {
        Self {
            rate: rate.max(1),
            counter: AtomicU64::new(0),
        }
    }

    /// Checks if the current item should be accepted.
    #[inline]
    pub fn should_accept(&self) -> bool {
        let count = self.counter.fetch_add(1, Ordering::Relaxed);
        count % (self.rate as u64) == 0
    }

    /// Returns the sampling rate.
    #[must_use]
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Sets the sampling rate.
    pub fn set_rate(&mut self, rate: u32) {
        self.rate = rate.max(1);
    }

    /// Resets the counter.
    pub fn reset(&self) {
        self.counter.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backpressure_strategy_properties() {
        assert!(BackpressureStrategy::Block.may_block());
        assert!(!BackpressureStrategy::DropNewest.may_block());

        assert!(!BackpressureStrategy::Block.may_drop());
        assert!(BackpressureStrategy::DropNewest.may_drop());
        assert!(BackpressureStrategy::DropOldest.may_drop());
        assert!(BackpressureStrategy::Sample { rate: 2 }.may_drop());
    }

    #[test]
    fn test_backpressure_stats() {
        let stats = BackpressureStats::new();

        stats.record_accepted();
        stats.record_accepted();
        stats.record_dropped();
        stats.record_blocked();

        assert_eq!(stats.accepted(), 2);
        assert_eq!(stats.dropped(), 1);
        assert_eq!(stats.blocked(), 1);
        assert_eq!(stats.total(), 3);
    }

    #[test]
    fn test_backpressure_stats_drop_rate() {
        let stats = BackpressureStats::new();

        for _ in 0..80 {
            stats.record_accepted();
        }
        for _ in 0..20 {
            stats.record_dropped();
        }

        assert!((stats.drop_rate() - 0.2).abs() < 0.01);
    }

    #[test]
    fn test_sampler() {
        let sampler = Sampler::new(4);

        // Should accept every 4th item (0, 4, 8, ...)
        let accepted: Vec<bool> = (0..12).map(|_| sampler.should_accept()).collect();

        assert_eq!(
            accepted,
            vec![true, false, false, false, true, false, false, false, true, false, false, false]
        );
    }

    #[test]
    fn test_sampler_rate_one() {
        let sampler = Sampler::new(1);

        // Should accept all items
        for _ in 0..10 {
            assert!(sampler.should_accept());
        }
    }

    #[test]
    fn test_sampler_rate_zero_becomes_one() {
        let sampler = Sampler::new(0);
        assert_eq!(sampler.rate(), 1);
    }

    #[test]
    fn test_stats_snapshot() {
        let stats = BackpressureStats::new();
        stats.record_accepted();
        stats.record_dropped();

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.accepted, 1);
        assert_eq!(snapshot.dropped, 1);
        assert_eq!(snapshot.total(), 2);
        assert!((snapshot.acceptance_rate() - 0.5).abs() < 0.01);
    }
}
