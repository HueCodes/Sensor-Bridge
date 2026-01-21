//! Latency measurement and histogram tracking.
//!
//! Provides lock-free latency tracking with percentile calculations.

use core::sync::atomic::{AtomicU64, Ordering};

/// A histogram for tracking latency distributions.
///
/// Uses fixed buckets for O(1) updates and lock-free operations.
/// Designed for nanosecond-scale measurements.
///
/// # Bucket Layout
///
/// The histogram uses logarithmic buckets:
/// - Buckets 0-9: 0-9 ns (1ns resolution)
/// - Buckets 10-19: 10-99 ns (10ns resolution)
/// - Buckets 20-29: 100-999 ns (100ns resolution)
/// - And so on...
///
/// This provides good resolution for fast operations while still
/// capturing slow outliers.
#[derive(Debug)]
pub struct LatencyHistogram {
    /// Histogram buckets.
    buckets: [AtomicU64; Self::NUM_BUCKETS],
    /// Total number of samples.
    count: AtomicU64,
    /// Sum of all samples (for mean calculation).
    sum: AtomicU64,
    /// Minimum value seen.
    min: AtomicU64,
    /// Maximum value seen.
    max: AtomicU64,
}

impl LatencyHistogram {
    /// Number of buckets in the histogram.
    const NUM_BUCKETS: usize = 64;

    /// Creates a new empty histogram.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buckets: core::array::from_fn(|_| AtomicU64::new(0)),
            count: AtomicU64::new(0),
            sum: AtomicU64::new(0),
            min: AtomicU64::new(u64::MAX),
            max: AtomicU64::new(0),
        }
    }

    /// Records a latency value in nanoseconds.
    #[inline]
    pub fn record(&self, latency_ns: u64) {
        let bucket = Self::bucket_for(latency_ns);
        self.buckets[bucket].fetch_add(1, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
        self.sum.fetch_add(latency_ns, Ordering::Relaxed);

        // Update min/max (using compare-and-swap loops)
        let mut current_min = self.min.load(Ordering::Relaxed);
        while latency_ns < current_min {
            match self.min.compare_exchange_weak(
                current_min,
                latency_ns,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current_min = actual,
            }
        }

        let mut current_max = self.max.load(Ordering::Relaxed);
        while latency_ns > current_max {
            match self.max.compare_exchange_weak(
                current_max,
                latency_ns,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current_max = actual,
            }
        }
    }

    /// Maps a latency value to a bucket index.
    #[inline]
    fn bucket_for(latency_ns: u64) -> usize {
        if latency_ns == 0 {
            return 0;
        }

        // Use leading zeros to determine the order of magnitude
        let leading = latency_ns.leading_zeros() as usize;
        let magnitude = 63 - leading; // log2(latency_ns)

        // Map to bucket: roughly 10 buckets per order of magnitude
        let bucket = magnitude.saturating_mul(10) / 3;
        bucket.min(Self::NUM_BUCKETS - 1)
    }

    /// Maps a bucket index back to an approximate latency value.
    #[inline]
    fn latency_for_bucket(bucket: usize) -> u64 {
        if bucket == 0 {
            return 0;
        }

        // Inverse of bucket_for
        let magnitude = (bucket * 3) / 10;
        1u64 << magnitude
    }

    /// Returns the total number of samples.
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Returns the mean latency in nanoseconds.
    #[must_use]
    pub fn mean(&self) -> f64 {
        let count = self.count.load(Ordering::Relaxed);
        if count == 0 {
            return 0.0;
        }
        self.sum.load(Ordering::Relaxed) as f64 / count as f64
    }

    /// Returns the minimum latency in nanoseconds.
    #[must_use]
    pub fn min(&self) -> Option<u64> {
        let min = self.min.load(Ordering::Relaxed);
        if min == u64::MAX {
            None
        } else {
            Some(min)
        }
    }

    /// Returns the maximum latency in nanoseconds.
    #[must_use]
    pub fn max(&self) -> Option<u64> {
        let max = self.max.load(Ordering::Relaxed);
        if max == 0 && self.count.load(Ordering::Relaxed) == 0 {
            None
        } else {
            Some(max)
        }
    }

    /// Returns the approximate percentile value.
    ///
    /// # Arguments
    ///
    /// * `p` - Percentile (0.0 to 1.0), e.g., 0.99 for p99
    #[must_use]
    pub fn percentile(&self, p: f64) -> u64 {
        assert!((0.0..=1.0).contains(&p), "Percentile must be between 0 and 1");

        let count = self.count.load(Ordering::Relaxed);
        if count == 0 {
            return 0;
        }

        let target = ((count as f64) * p).ceil() as u64;
        let mut cumulative = 0u64;

        for (i, bucket) in self.buckets.iter().enumerate() {
            cumulative += bucket.load(Ordering::Relaxed);
            if cumulative >= target {
                return Self::latency_for_bucket(i);
            }
        }

        // If we get here, return the max bucket value
        Self::latency_for_bucket(Self::NUM_BUCKETS - 1)
    }

    /// Returns the p50 (median) latency.
    #[must_use]
    pub fn p50(&self) -> u64 {
        self.percentile(0.50)
    }

    /// Returns the p95 latency.
    #[must_use]
    pub fn p95(&self) -> u64 {
        self.percentile(0.95)
    }

    /// Returns the p99 latency.
    #[must_use]
    pub fn p99(&self) -> u64 {
        self.percentile(0.99)
    }

    /// Returns the p999 latency.
    #[must_use]
    pub fn p999(&self) -> u64 {
        self.percentile(0.999)
    }

    /// Resets all statistics.
    pub fn reset(&self) {
        for bucket in &self.buckets {
            bucket.store(0, Ordering::Relaxed);
        }
        self.count.store(0, Ordering::Relaxed);
        self.sum.store(0, Ordering::Relaxed);
        self.min.store(u64::MAX, Ordering::Relaxed);
        self.max.store(0, Ordering::Relaxed);
    }

    /// Returns a snapshot of the histogram data.
    #[must_use]
    pub fn snapshot(&self) -> LatencySnapshot {
        LatencySnapshot {
            count: self.count(),
            mean_ns: self.mean(),
            min_ns: self.min(),
            max_ns: self.max(),
            p50_ns: self.p50(),
            p95_ns: self.p95(),
            p99_ns: self.p99(),
            p999_ns: self.p999(),
        }
    }
}

impl Default for LatencyHistogram {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: All internal state uses atomics
unsafe impl Sync for LatencyHistogram {}
unsafe impl Send for LatencyHistogram {}

/// A point-in-time snapshot of latency statistics.
#[derive(Debug, Clone)]
pub struct LatencySnapshot {
    /// Total sample count.
    pub count: u64,
    /// Mean latency in nanoseconds.
    pub mean_ns: f64,
    /// Minimum latency in nanoseconds.
    pub min_ns: Option<u64>,
    /// Maximum latency in nanoseconds.
    pub max_ns: Option<u64>,
    /// p50 (median) latency in nanoseconds.
    pub p50_ns: u64,
    /// p95 latency in nanoseconds.
    pub p95_ns: u64,
    /// p99 latency in nanoseconds.
    pub p99_ns: u64,
    /// p999 latency in nanoseconds.
    pub p999_ns: u64,
}

impl LatencySnapshot {
    /// Returns the mean latency in microseconds.
    #[must_use]
    pub fn mean_us(&self) -> f64 {
        self.mean_ns / 1000.0
    }

    /// Returns the p99 latency in microseconds.
    #[must_use]
    pub fn p99_us(&self) -> f64 {
        self.p99_ns as f64 / 1000.0
    }
}

#[cfg(feature = "std")]
impl std::fmt::Display for LatencySnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "count={} mean={:.1}ns p50={}ns p95={}ns p99={}ns p999={}ns min={:?}ns max={:?}ns",
            self.count,
            self.mean_ns,
            self.p50_ns,
            self.p95_ns,
            self.p99_ns,
            self.p999_ns,
            self.min_ns,
            self.max_ns
        )
    }
}

/// A timer that automatically records its duration when dropped.
#[cfg(feature = "std")]
pub struct ScopedTimer<'a> {
    histogram: &'a LatencyHistogram,
    start: std::time::Instant,
}

#[cfg(feature = "std")]
impl<'a> ScopedTimer<'a> {
    /// Creates a new scoped timer.
    #[must_use]
    pub fn new(histogram: &'a LatencyHistogram) -> Self {
        Self {
            histogram,
            start: std::time::Instant::now(),
        }
    }

    /// Returns the elapsed time so far without recording.
    #[must_use]
    pub fn elapsed_ns(&self) -> u64 {
        self.start.elapsed().as_nanos() as u64
    }
}

#[cfg(feature = "std")]
impl Drop for ScopedTimer<'_> {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed().as_nanos() as u64;
        self.histogram.record(elapsed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_histogram_basic() {
        let hist = LatencyHistogram::new();

        hist.record(100);
        hist.record(200);
        hist.record(300);

        assert_eq!(hist.count(), 3);
        assert!((hist.mean() - 200.0).abs() < 0.01);
        assert_eq!(hist.min(), Some(100));
        assert_eq!(hist.max(), Some(300));
    }

    #[test]
    fn test_histogram_percentiles() {
        let hist = LatencyHistogram::new();

        // Record values 1-100
        for i in 1..=100 {
            hist.record(i);
        }

        // p50 should be approximately 50 (bucket approximation means this is rough)
        let p50 = hist.p50();
        assert!(p50 >= 8 && p50 <= 128, "p50 was {p50}");

        // p99 should be approximately 99
        let p99 = hist.p99();
        assert!(p99 >= 32 && p99 <= 256, "p99 was {p99}");
    }

    #[test]
    fn test_histogram_empty() {
        let hist = LatencyHistogram::new();

        assert_eq!(hist.count(), 0);
        assert_eq!(hist.mean(), 0.0);
        assert_eq!(hist.min(), None);
        assert_eq!(hist.max(), None);
        assert_eq!(hist.p99(), 0);
    }

    #[test]
    fn test_histogram_reset() {
        let hist = LatencyHistogram::new();

        hist.record(100);
        hist.record(200);
        assert_eq!(hist.count(), 2);

        hist.reset();
        assert_eq!(hist.count(), 0);
        assert_eq!(hist.min(), None);
    }

    #[test]
    fn test_bucket_mapping() {
        // Test that bucket mapping is roughly logarithmic
        assert!(LatencyHistogram::bucket_for(1) < LatencyHistogram::bucket_for(10));
        assert!(LatencyHistogram::bucket_for(10) < LatencyHistogram::bucket_for(100));
        assert!(LatencyHistogram::bucket_for(100) < LatencyHistogram::bucket_for(1000));
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_scoped_timer() {
        let hist = LatencyHistogram::new();

        {
            let _timer = ScopedTimer::new(&hist);
            std::thread::sleep(std::time::Duration::from_micros(100));
        }

        assert_eq!(hist.count(), 1);
        // Should be at least 100us = 100,000ns
        assert!(hist.min().unwrap() >= 50_000);
    }

    #[test]
    fn test_snapshot() {
        let hist = LatencyHistogram::new();
        hist.record(1000);
        hist.record(2000);

        let snap = hist.snapshot();
        assert_eq!(snap.count, 2);
        assert!((snap.mean_ns - 1500.0).abs() < 0.01);
    }
}
