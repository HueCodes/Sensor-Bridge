//! Jitter tracking using Welford's online algorithm.
//!
//! This module provides lock-free jitter (variance) calculation for latency
//! measurements, useful for assessing timing consistency in real-time systems.
//!
//! # Welford's Algorithm
//!
//! Welford's algorithm computes running mean and variance in a numerically
//! stable way with O(1) memory. For each new value x:
//!
//! ```text
//! n += 1
//! delta = x - mean
//! mean += delta / n
//! delta2 = x - mean
//! M2 += delta * delta2
//! variance = M2 / (n - 1)
//! ```
//!
//! # Thread Safety
//!
//! The implementation uses atomic operations for lock-free updates.
//! Due to the nature of Welford's algorithm requiring multiple dependent
//! updates, we use a compare-and-swap loop to ensure consistency.

use core::sync::atomic::{AtomicU64, Ordering};

/// A lock-free jitter tracker using Welford's online algorithm.
///
/// Tracks running mean and variance of latency measurements for calculating
/// standard deviation (jitter).
///
/// # Example
///
/// ```rust
/// use sensor_bridge::metrics::JitterTracker;
///
/// let tracker = JitterTracker::new();
///
/// tracker.record(1000);
/// tracker.record(1100);
/// tracker.record(900);
/// tracker.record(1050);
///
/// println!("Mean: {:.2} ns", tracker.mean());
/// println!("Std Dev: {:.2} ns", tracker.std_dev());
/// ```
#[derive(Debug)]
pub struct JitterTracker {
    /// Number of samples.
    count: AtomicU64,
    /// Running mean (stored as fixed-point with 32 fractional bits).
    mean_fp: AtomicU64,
    /// Sum of squared differences from mean (M2), stored as fixed-point.
    m2_fp: AtomicU64,
    /// Minimum value observed.
    min: AtomicU64,
    /// Maximum value observed.
    max: AtomicU64,
}

impl JitterTracker {
    /// Fixed-point fractional bits for mean/variance storage.
    const FP_BITS: u32 = 32;
    const FP_SCALE: f64 = (1u64 << Self::FP_BITS) as f64;

    /// Creates a new jitter tracker.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            mean_fp: AtomicU64::new(0),
            m2_fp: AtomicU64::new(0),
            min: AtomicU64::new(u64::MAX),
            max: AtomicU64::new(0),
        }
    }

    /// Records a latency measurement in nanoseconds.
    ///
    /// Updates the running mean and variance using Welford's algorithm.
    /// This operation is lock-free but may spin under high contention.
    #[inline]
    pub fn record(&self, latency_ns: u64) {
        // Update min/max atomically
        Self::update_min(&self.min, latency_ns);
        Self::update_max(&self.max, latency_ns);

        // Welford's algorithm with CAS loop for atomicity
        // We need to update count, mean, and m2 together
        loop {
            let old_count = self.count.load(Ordering::Relaxed);
            let old_mean_fp = self.mean_fp.load(Ordering::Relaxed);
            let old_m2_fp = self.m2_fp.load(Ordering::Relaxed);

            let new_count = old_count + 1;
            let old_mean = Self::from_fixed_point(old_mean_fp);
            let x = latency_ns as f64;

            // delta = x - mean
            let delta = x - old_mean;

            // mean += delta / n
            let new_mean = old_mean + delta / (new_count as f64);

            // delta2 = x - new_mean
            let delta2 = x - new_mean;

            // M2 += delta * delta2
            let old_m2 = Self::from_fixed_point(old_m2_fp);
            let new_m2 = old_m2 + delta * delta2;

            let new_mean_fp = Self::to_fixed_point(new_mean);
            let new_m2_fp = Self::to_fixed_point(new_m2);

            // Try to update all values atomically
            // We use count as the "lock" - if it changed, retry
            if self
                .count
                .compare_exchange_weak(old_count, new_count, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                // Count was successfully incremented, update mean and m2
                self.mean_fp.store(new_mean_fp, Ordering::Release);
                self.m2_fp.store(new_m2_fp, Ordering::Release);
                break;
            }
            // CAS failed, another thread updated - retry with new values
            core::hint::spin_loop();
        }
    }

    /// Returns the number of samples recorded.
    #[inline]
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Returns the running mean in nanoseconds.
    #[inline]
    #[must_use]
    pub fn mean(&self) -> f64 {
        Self::from_fixed_point(self.mean_fp.load(Ordering::Relaxed))
    }

    /// Returns the sample variance in nanoseconds squared.
    ///
    /// Uses Bessel's correction (n-1 denominator) for unbiased estimation.
    #[inline]
    #[must_use]
    pub fn variance(&self) -> f64 {
        let count = self.count.load(Ordering::Relaxed);
        if count < 2 {
            return 0.0;
        }
        let m2 = Self::from_fixed_point(self.m2_fp.load(Ordering::Relaxed));
        m2 / (count - 1) as f64
    }

    /// Returns the sample standard deviation (jitter) in nanoseconds.
    #[inline]
    #[must_use]
    pub fn std_dev(&self) -> f64 {
        crate::math::sqrt_f64(self.variance())
    }

    /// Returns the standard deviation in microseconds.
    #[inline]
    #[must_use]
    pub fn std_dev_us(&self) -> f64 {
        self.std_dev() / 1000.0
    }

    /// Returns the standard deviation in milliseconds.
    #[inline]
    #[must_use]
    pub fn std_dev_ms(&self) -> f64 {
        self.std_dev() / 1_000_000.0
    }

    /// Returns the minimum value observed.
    #[inline]
    #[must_use]
    pub fn min(&self) -> Option<u64> {
        let min = self.min.load(Ordering::Relaxed);
        if min == u64::MAX {
            None
        } else {
            Some(min)
        }
    }

    /// Returns the maximum value observed.
    #[inline]
    #[must_use]
    pub fn max(&self) -> Option<u64> {
        let max = self.max.load(Ordering::Relaxed);
        if max == 0 && self.count.load(Ordering::Relaxed) == 0 {
            None
        } else {
            Some(max)
        }
    }

    /// Returns the range (max - min) of observed values.
    #[inline]
    #[must_use]
    pub fn range(&self) -> Option<u64> {
        match (self.min(), self.max()) {
            (Some(min), Some(max)) => Some(max - min),
            _ => None,
        }
    }

    /// Returns a snapshot of the current statistics.
    #[must_use]
    pub fn snapshot(&self) -> JitterSnapshot {
        JitterSnapshot {
            count: self.count(),
            mean_ns: self.mean(),
            std_dev_ns: self.std_dev(),
            min_ns: self.min(),
            max_ns: self.max(),
        }
    }

    /// Resets all statistics.
    pub fn reset(&self) {
        self.count.store(0, Ordering::Relaxed);
        self.mean_fp.store(0, Ordering::Relaxed);
        self.m2_fp.store(0, Ordering::Relaxed);
        self.min.store(u64::MAX, Ordering::Relaxed);
        self.max.store(0, Ordering::Relaxed);
    }

    /// Converts a floating-point value to fixed-point representation.
    #[inline]
    fn to_fixed_point(value: f64) -> u64 {
        // Clamp to prevent overflow
        let clamped = value.clamp(0.0, (u64::MAX >> Self::FP_BITS) as f64);
        (clamped * Self::FP_SCALE) as u64
    }

    /// Converts a fixed-point value back to floating-point.
    #[inline]
    fn from_fixed_point(fp: u64) -> f64 {
        fp as f64 / Self::FP_SCALE
    }

    /// Atomically updates the minimum value.
    #[inline]
    fn update_min(min: &AtomicU64, value: u64) {
        let mut current = min.load(Ordering::Relaxed);
        while value < current {
            match min.compare_exchange_weak(current, value, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    /// Atomically updates the maximum value.
    #[inline]
    fn update_max(max: &AtomicU64, value: u64) {
        let mut current = max.load(Ordering::Relaxed);
        while value > current {
            match max.compare_exchange_weak(current, value, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }
}

impl Default for JitterTracker {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: All operations use atomics
unsafe impl Sync for JitterTracker {}
unsafe impl Send for JitterTracker {}

/// A point-in-time snapshot of jitter statistics.
#[derive(Debug, Clone)]
pub struct JitterSnapshot {
    /// Number of samples.
    pub count: u64,
    /// Mean latency in nanoseconds.
    pub mean_ns: f64,
    /// Standard deviation (jitter) in nanoseconds.
    pub std_dev_ns: f64,
    /// Minimum latency in nanoseconds.
    pub min_ns: Option<u64>,
    /// Maximum latency in nanoseconds.
    pub max_ns: Option<u64>,
}

impl JitterSnapshot {
    /// Returns the mean in microseconds.
    #[must_use]
    pub fn mean_us(&self) -> f64 {
        self.mean_ns / 1000.0
    }

    /// Returns the standard deviation in microseconds.
    #[must_use]
    pub fn std_dev_us(&self) -> f64 {
        self.std_dev_ns / 1000.0
    }

    /// Returns the standard deviation in milliseconds.
    #[must_use]
    pub fn std_dev_ms(&self) -> f64 {
        self.std_dev_ns / 1_000_000.0
    }

    /// Returns the coefficient of variation (std_dev / mean).
    ///
    /// This is a normalized measure of dispersion - lower is more consistent.
    #[must_use]
    pub fn coefficient_of_variation(&self) -> f64 {
        if self.mean_ns == 0.0 {
            0.0
        } else {
            self.std_dev_ns / self.mean_ns
        }
    }
}

#[cfg(feature = "std")]
impl std::fmt::Display for JitterSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "n={} mean={:.1}ns std_dev={:.1}ns cv={:.3} min={:?}ns max={:?}ns",
            self.count,
            self.mean_ns,
            self.std_dev_ns,
            self.coefficient_of_variation(),
            self.min_ns,
            self.max_ns
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jitter_basic() {
        let tracker = JitterTracker::new();

        assert_eq!(tracker.count(), 0);
        assert_eq!(tracker.mean(), 0.0);
        assert_eq!(tracker.std_dev(), 0.0);
    }

    #[test]
    fn test_jitter_single_sample() {
        let tracker = JitterTracker::new();
        tracker.record(1000);

        assert_eq!(tracker.count(), 1);
        assert!((tracker.mean() - 1000.0).abs() < 0.1);
        assert_eq!(tracker.std_dev(), 0.0); // Need 2+ samples for std dev
    }

    #[test]
    fn test_jitter_known_values() {
        let tracker = JitterTracker::new();

        // Record known values: [2, 4, 4, 4, 5, 5, 7, 9]
        // Mean = 40/8 = 5
        // Variance = ((2-5)^2 + (4-5)^2*3 + (5-5)^2*2 + (7-5)^2 + (9-5)^2) / 7
        //          = (9 + 3 + 0 + 4 + 16) / 7 = 32/7 ≈ 4.571
        // Std Dev ≈ 2.138
        for v in [2, 4, 4, 4, 5, 5, 7, 9] {
            tracker.record(v);
        }

        assert_eq!(tracker.count(), 8);
        assert!(
            (tracker.mean() - 5.0).abs() < 0.01,
            "mean = {}",
            tracker.mean()
        );
        assert!(
            (tracker.variance() - 4.571).abs() < 0.1,
            "variance = {}",
            tracker.variance()
        );
        assert!(
            (tracker.std_dev() - 2.138).abs() < 0.1,
            "std_dev = {}",
            tracker.std_dev()
        );
    }

    #[test]
    fn test_jitter_min_max() {
        let tracker = JitterTracker::new();

        assert_eq!(tracker.min(), None);
        assert_eq!(tracker.max(), None);

        tracker.record(100);
        assert_eq!(tracker.min(), Some(100));
        assert_eq!(tracker.max(), Some(100));

        tracker.record(50);
        tracker.record(200);
        assert_eq!(tracker.min(), Some(50));
        assert_eq!(tracker.max(), Some(200));
        assert_eq!(tracker.range(), Some(150));
    }

    #[test]
    fn test_jitter_reset() {
        let tracker = JitterTracker::new();

        tracker.record(100);
        tracker.record(200);
        tracker.record(300);

        assert_eq!(tracker.count(), 3);

        tracker.reset();

        assert_eq!(tracker.count(), 0);
        assert_eq!(tracker.mean(), 0.0);
        assert_eq!(tracker.std_dev(), 0.0);
        assert_eq!(tracker.min(), None);
        assert_eq!(tracker.max(), None);
    }

    #[test]
    fn test_jitter_snapshot() {
        let tracker = JitterTracker::new();

        tracker.record(1000);
        tracker.record(2000);
        tracker.record(3000);

        let snapshot = tracker.snapshot();
        assert_eq!(snapshot.count, 3);
        assert!((snapshot.mean_ns - 2000.0).abs() < 0.1);
    }

    #[test]
    fn test_jitter_constant_values() {
        let tracker = JitterTracker::new();

        // All same values should have zero variance
        for _ in 0..100 {
            tracker.record(1000);
        }

        assert_eq!(tracker.count(), 100);
        assert!((tracker.mean() - 1000.0).abs() < 0.1);
        assert!(tracker.std_dev() < 0.1, "std_dev = {}", tracker.std_dev());
    }

    #[test]
    fn test_jitter_large_values() {
        let tracker = JitterTracker::new();

        // Test with microsecond-scale values
        tracker.record(1_000_000); // 1ms
        tracker.record(1_100_000); // 1.1ms
        tracker.record(900_000); // 0.9ms

        assert_eq!(tracker.count(), 3);
        let mean = tracker.mean();
        assert!((mean - 1_000_000.0).abs() < 1000.0, "mean = {}", mean);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_jitter_concurrent() {
        use std::sync::Arc;
        use std::thread;

        let tracker = Arc::new(JitterTracker::new());
        let mut handles = vec![];

        for t in 0..4 {
            let tracker = Arc::clone(&tracker);
            handles.push(thread::spawn(move || {
                for i in 0..1000 {
                    tracker.record((t * 1000 + i) as u64);
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(tracker.count(), 4000);
        // Mean should be approximately (0+999+1000+1999+2000+2999+3000+3999)/2 ≈ 1999.5
        // but with interleaving it's (0..4000).sum() / 4000 = 1999.5
        let mean = tracker.mean();
        assert!(
            (mean - 1999.5).abs() < 100.0,
            "mean = {}, expected ~1999.5",
            mean
        );
    }

    #[test]
    fn test_coefficient_of_variation() {
        let snapshot = JitterSnapshot {
            count: 100,
            mean_ns: 1000.0,
            std_dev_ns: 100.0,
            min_ns: Some(800),
            max_ns: Some(1200),
        };

        assert!((snapshot.coefficient_of_variation() - 0.1).abs() < 0.001);
    }
}
