//! Timestamp utilities for sensor data.
//!
//! This module provides:
//! - [`Timestamped<T>`]: A wrapper that adds monotonic timestamps and sequence numbers to any data
//! - [`MonotonicClock`]: A clock that guarantees monotonically increasing timestamps
//!
//! # Monotonicity Guarantees
//!
//! The [`MonotonicClock`] ensures that timestamps never go backwards, even across system sleep
//! events or NTP adjustments. This is critical for sensor fusion and data ordering.

use core::sync::atomic::{AtomicU64, Ordering};

/// A timestamped wrapper for sensor data.
///
/// This wrapper adds:
/// - A monotonic timestamp in nanoseconds since the clock's epoch
/// - A sequence number for detecting dropped samples
///
/// # Example
///
/// ```rust
/// use sensor_bridge::timestamp::{Timestamped, MonotonicClock};
/// use sensor_bridge::sensor::ImuReading;
///
/// let clock = MonotonicClock::new();
/// let reading = ImuReading::default();
/// let timestamped = clock.stamp(reading);
///
/// assert!(timestamped.timestamp_ns > 0 || timestamped.timestamp_ns == 0);
/// assert_eq!(timestamped.seq, 0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Timestamped<T> {
    /// Monotonic nanoseconds since clock epoch.
    pub timestamp_ns: u64,
    /// Sequence number for drop detection.
    /// Increments for each sample from the same source.
    pub seq: u64,
    /// The actual sensor data.
    pub data: T,
}

impl<T> Timestamped<T> {
    /// Creates a new timestamped value with the given timestamp and sequence number.
    #[inline]
    #[must_use]
    pub const fn new(data: T, timestamp_ns: u64, seq: u64) -> Self {
        Self {
            timestamp_ns,
            seq,
            data,
        }
    }

    /// Maps the inner data to a new type.
    #[inline]
    pub fn map<U, F>(self, f: F) -> Timestamped<U>
    where
        F: FnOnce(T) -> U,
    {
        Timestamped {
            timestamp_ns: self.timestamp_ns,
            seq: self.seq,
            data: f(self.data),
        }
    }

    /// Returns a reference to the inner data.
    #[inline]
    #[must_use]
    pub const fn as_ref(&self) -> Timestamped<&T> {
        Timestamped {
            timestamp_ns: self.timestamp_ns,
            seq: self.seq,
            data: &self.data,
        }
    }

    /// Calculates the time difference in nanoseconds between this and another timestamped value.
    ///
    /// Returns `None` if `other` has a later timestamp (negative difference).
    #[inline]
    #[must_use]
    pub fn time_since(&self, other: &Timestamped<T>) -> Option<u64> {
        self.timestamp_ns.checked_sub(other.timestamp_ns)
    }

    /// Checks if this sample was dropped relative to an expected sequence number.
    ///
    /// Returns the number of dropped samples (0 if none were dropped).
    #[inline]
    #[must_use]
    pub const fn dropped_since(&self, expected_seq: u64) -> u64 {
        self.seq.saturating_sub(expected_seq)
    }
}

impl<T: Default> Default for Timestamped<T> {
    fn default() -> Self {
        Self {
            timestamp_ns: 0,
            seq: 0,
            data: T::default(),
        }
    }
}

/// A monotonic clock for timestamping sensor data.
///
/// This clock guarantees that timestamps are always monotonically increasing,
/// even if the system clock jumps backwards due to NTP sync or sleep.
///
/// # Thread Safety
///
/// The clock is `Send + Sync` and can be shared between threads. Each call to
/// [`now_ns()`](Self::now_ns) or [`stamp()`](Self::stamp) returns a timestamp
/// that is guaranteed to be >= all previous timestamps from this clock.
///
/// # Example
///
/// ```rust
/// use sensor_bridge::timestamp::MonotonicClock;
///
/// let clock = MonotonicClock::new();
/// let t1 = clock.now_ns();
/// let t2 = clock.now_ns();
/// assert!(t2 >= t1);
/// ```
pub struct MonotonicClock {
    /// The last timestamp returned, used to ensure monotonicity.
    last_timestamp: AtomicU64,
    /// Sequence counter for stamped values.
    sequence: AtomicU64,
    /// The epoch (start time) in platform-specific units.
    #[cfg(feature = "std")]
    epoch: std::time::Instant,
}

impl MonotonicClock {
    /// Creates a new monotonic clock with the current time as epoch.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn new() -> Self {
        Self {
            last_timestamp: AtomicU64::new(0),
            sequence: AtomicU64::new(0),
            epoch: std::time::Instant::now(),
        }
    }

    /// Creates a new monotonic clock (no_std version).
    ///
    /// In no_std mode, you must provide timestamps externally via [`stamp_with_time()`](Self::stamp_with_time).
    #[cfg(not(feature = "std"))]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            last_timestamp: AtomicU64::new(0),
            sequence: AtomicU64::new(0),
        }
    }

    /// Returns the current monotonic timestamp in nanoseconds.
    ///
    /// The timestamp is guaranteed to be >= all previous timestamps from this clock.
    #[cfg(feature = "std")]
    #[inline]
    pub fn now_ns(&self) -> u64 {
        let raw_ns = self.epoch.elapsed().as_nanos() as u64;

        // Ensure monotonicity with atomic fetch_max
        // This handles the rare case where Instant::elapsed() might return
        // a smaller value due to platform quirks
        loop {
            let last = self.last_timestamp.load(Ordering::Acquire);
            let new_ts = raw_ns.max(last);

            // Try to update if we have a new maximum
            if new_ts == last {
                return last;
            }

            match self.last_timestamp.compare_exchange_weak(
                last,
                new_ts,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return new_ts,
                Err(_) => continue, // Retry if another thread updated
            }
        }
    }

    /// Returns the current monotonic timestamp in nanoseconds (no_std version).
    ///
    /// In no_std mode, this returns the last recorded timestamp.
    /// Use [`record_time()`](Self::record_time) to update it from an external source.
    #[cfg(not(feature = "std"))]
    #[inline]
    pub fn now_ns(&self) -> u64 {
        self.last_timestamp.load(Ordering::Acquire)
    }

    /// Records an external timestamp, ensuring monotonicity.
    ///
    /// This is useful in no_std environments where you get timestamps from
    /// hardware timers or other sources.
    ///
    /// Returns the actual timestamp used (may be higher than input if input was stale).
    #[inline]
    pub fn record_time(&self, timestamp_ns: u64) -> u64 {
        loop {
            let last = self.last_timestamp.load(Ordering::Acquire);
            let new_ts = timestamp_ns.max(last);

            if new_ts == last {
                return last;
            }

            match self.last_timestamp.compare_exchange_weak(
                last,
                new_ts,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return new_ts,
                Err(_) => continue,
            }
        }
    }

    /// Wraps data with the current timestamp and increments the sequence number.
    #[cfg(feature = "std")]
    #[inline]
    pub fn stamp<T>(&self, data: T) -> Timestamped<T> {
        let timestamp_ns = self.now_ns();
        let seq = self.sequence.fetch_add(1, Ordering::Relaxed);
        Timestamped::new(data, timestamp_ns, seq)
    }

    /// Wraps data with a provided timestamp and increments the sequence number.
    ///
    /// The timestamp is adjusted to ensure monotonicity.
    #[inline]
    pub fn stamp_with_time<T>(&self, data: T, timestamp_ns: u64) -> Timestamped<T> {
        let actual_ts = self.record_time(timestamp_ns);
        let seq = self.sequence.fetch_add(1, Ordering::Relaxed);
        Timestamped::new(data, actual_ts, seq)
    }

    /// Returns the current sequence number without incrementing it.
    #[inline]
    #[must_use]
    pub fn current_seq(&self) -> u64 {
        self.sequence.load(Ordering::Relaxed)
    }

    /// Resets the sequence counter to zero.
    ///
    /// This is useful when restarting a sensor or pipeline.
    #[inline]
    pub fn reset_seq(&self) {
        self.sequence.store(0, Ordering::Relaxed);
    }
}

#[cfg(feature = "std")]
impl Default for MonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

// Safety: MonotonicClock uses only atomic operations for shared state
unsafe impl Sync for MonotonicClock {}
unsafe impl Send for MonotonicClock {}

/// Calculates the difference between two timestamps in nanoseconds.
///
/// Returns `None` if `earlier` is actually later than `later`.
#[inline]
#[must_use]
pub const fn timestamp_diff(earlier: u64, later: u64) -> Option<u64> {
    if later >= earlier {
        Some(later - earlier)
    } else {
        None
    }
}

/// Checks if two timestamps are within a tolerance window.
///
/// This is useful for sensor fusion where data from different sensors
/// needs to be aligned within a time tolerance.
#[inline]
#[must_use]
pub const fn timestamps_within_tolerance(t1: u64, t2: u64, tolerance_ns: u64) -> bool {
    t1.abs_diff(t2) <= tolerance_ns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timestamped_creation() {
        let data = 42u32;
        let ts = Timestamped::new(data, 1000, 5);
        assert_eq!(ts.data, 42);
        assert_eq!(ts.timestamp_ns, 1000);
        assert_eq!(ts.seq, 5);
    }

    #[test]
    fn test_timestamped_map() {
        let ts = Timestamped::new(10u32, 1000, 0);
        let mapped = ts.map(|x| x * 2);
        assert_eq!(mapped.data, 20);
        assert_eq!(mapped.timestamp_ns, 1000);
        assert_eq!(mapped.seq, 0);
    }

    #[test]
    fn test_time_since() {
        let ts1 = Timestamped::new((), 1000, 0);
        let ts2 = Timestamped::new((), 1500, 1);

        assert_eq!(ts2.time_since(&ts1), Some(500));
        assert_eq!(ts1.time_since(&ts2), None);
    }

    #[test]
    fn test_dropped_since() {
        let ts = Timestamped::new((), 0, 10);
        assert_eq!(ts.dropped_since(10), 0);
        assert_eq!(ts.dropped_since(5), 5);
        assert_eq!(ts.dropped_since(15), 0);
    }

    #[test]
    fn test_timestamp_diff() {
        assert_eq!(timestamp_diff(100, 200), Some(100));
        assert_eq!(timestamp_diff(200, 100), None);
        assert_eq!(timestamp_diff(100, 100), Some(0));
    }

    #[test]
    fn test_timestamps_within_tolerance() {
        assert!(timestamps_within_tolerance(100, 105, 10));
        assert!(timestamps_within_tolerance(105, 100, 10));
        assert!(!timestamps_within_tolerance(100, 120, 10));
    }

    #[cfg(feature = "std")]
    mod std_tests {
        use super::*;

        #[test]
        fn test_monotonic_clock_monotonicity() {
            let clock = MonotonicClock::new();
            let mut prev = clock.now_ns();

            for _ in 0..1000 {
                let curr = clock.now_ns();
                assert!(curr >= prev, "Clock went backwards: {} -> {}", prev, curr);
                prev = curr;
            }
        }

        #[test]
        fn test_monotonic_clock_sequence() {
            let clock = MonotonicClock::new();

            let ts1 = clock.stamp(1u32);
            let ts2 = clock.stamp(2u32);
            let ts3 = clock.stamp(3u32);

            assert_eq!(ts1.seq, 0);
            assert_eq!(ts2.seq, 1);
            assert_eq!(ts3.seq, 2);
            assert!(ts2.timestamp_ns >= ts1.timestamp_ns);
            assert!(ts3.timestamp_ns >= ts2.timestamp_ns);
        }

        #[test]
        fn test_monotonic_clock_reset_seq() {
            let clock = MonotonicClock::new();
            let _ = clock.stamp(1u32);
            let _ = clock.stamp(2u32);

            clock.reset_seq();

            let ts = clock.stamp(3u32);
            assert_eq!(ts.seq, 0);
        }

        #[test]
        fn test_record_time_monotonicity() {
            let clock = MonotonicClock::new();

            let t1 = clock.record_time(1000);
            let t2 = clock.record_time(500); // Earlier time should not go back
            let t3 = clock.record_time(2000);

            assert_eq!(t1, 1000);
            assert!(t2 >= 1000); // Should not go backwards
            assert_eq!(t3, 2000);
        }
    }
}
