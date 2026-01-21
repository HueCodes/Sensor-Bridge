//! Atomic counters for metrics collection.
//!
//! Provides lock-free counters for tracking events and samples.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::buffer::CachePadded;

/// A simple atomic counter.
///
/// Provides lock-free increment and read operations.
#[derive(Debug)]
pub struct Counter {
    value: AtomicU64,
}

impl Counter {
    /// Creates a new counter initialized to zero.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            value: AtomicU64::new(0),
        }
    }

    /// Creates a counter with an initial value.
    #[must_use]
    pub const fn with_value(initial: u64) -> Self {
        Self {
            value: AtomicU64::new(initial),
        }
    }

    /// Increments the counter by 1.
    #[inline]
    pub fn increment(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the counter by a specified amount.
    #[inline]
    pub fn add(&self, amount: u64) {
        self.value.fetch_add(amount, Ordering::Relaxed);
    }

    /// Returns the current value.
    #[inline]
    #[must_use]
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    /// Resets the counter to zero and returns the previous value.
    #[inline]
    pub fn reset(&self) -> u64 {
        self.value.swap(0, Ordering::Relaxed)
    }

    /// Sets the counter to a specific value.
    #[inline]
    pub fn set(&self, value: u64) {
        self.value.store(value, Ordering::Relaxed);
    }
}

impl Default for Counter {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: Uses atomic operations
unsafe impl Sync for Counter {}
unsafe impl Send for Counter {}

/// A rate counter that tracks events per unit time.
///
/// Useful for measuring throughput or event rates.
#[cfg(feature = "std")]
#[derive(Debug)]
pub struct RateCounter {
    count: AtomicU64,
    start_time: std::time::Instant,
}

#[cfg(feature = "std")]
impl RateCounter {
    /// Creates a new rate counter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            start_time: std::time::Instant::now(),
        }
    }

    /// Increments the event count.
    #[inline]
    pub fn increment(&self) {
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments by a specified amount.
    #[inline]
    pub fn add(&self, amount: u64) {
        self.count.fetch_add(amount, Ordering::Relaxed);
    }

    /// Returns the current count.
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Returns the elapsed time in seconds.
    #[must_use]
    pub fn elapsed_secs(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }

    /// Returns the current rate (events per second).
    #[must_use]
    pub fn rate(&self) -> f64 {
        let elapsed = self.elapsed_secs();
        if elapsed > 0.0 {
            self.count.load(Ordering::Relaxed) as f64 / elapsed
        } else {
            0.0
        }
    }

    /// Resets the counter and timer.
    pub fn reset(&mut self) {
        self.count.store(0, Ordering::Relaxed);
        self.start_time = std::time::Instant::now();
    }
}

#[cfg(feature = "std")]
impl Default for RateCounter {
    fn default() -> Self {
        Self::new()
    }
}

/// A set of counters for tracking pipeline metrics.
///
/// Uses cache-line padding to prevent false sharing when
/// different counters are updated from different threads.
#[derive(Debug)]
pub struct PipelineCounters {
    /// Number of items received.
    pub received: CachePadded<Counter>,
    /// Number of items processed successfully.
    pub processed: CachePadded<Counter>,
    /// Number of items filtered/dropped by stages.
    pub filtered: CachePadded<Counter>,
    /// Number of items dropped due to buffer full.
    pub dropped: CachePadded<Counter>,
    /// Number of errors encountered.
    pub errors: CachePadded<Counter>,
}

impl PipelineCounters {
    /// Creates a new set of pipeline counters.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            received: CachePadded::new(Counter::new()),
            processed: CachePadded::new(Counter::new()),
            filtered: CachePadded::new(Counter::new()),
            dropped: CachePadded::new(Counter::new()),
            errors: CachePadded::new(Counter::new()),
        }
    }

    /// Returns a snapshot of all counter values.
    #[must_use]
    pub fn snapshot(&self) -> PipelineCounterSnapshot {
        PipelineCounterSnapshot {
            received: self.received.get().get(),
            processed: self.processed.get().get(),
            filtered: self.filtered.get().get(),
            dropped: self.dropped.get().get(),
            errors: self.errors.get().get(),
        }
    }

    /// Resets all counters to zero.
    pub fn reset(&self) {
        self.received.reset();
        self.processed.reset();
        self.filtered.reset();
        self.dropped.reset();
        self.errors.reset();
    }
}

impl Default for PipelineCounters {
    fn default() -> Self {
        Self::new()
    }
}

/// A snapshot of pipeline counter values.
#[derive(Debug, Clone, Default)]
pub struct PipelineCounterSnapshot {
    /// Number of items received.
    pub received: u64,
    /// Number of items processed.
    pub processed: u64,
    /// Number of items filtered.
    pub filtered: u64,
    /// Number of items dropped.
    pub dropped: u64,
    /// Number of errors.
    pub errors: u64,
}

impl PipelineCounterSnapshot {
    /// Returns the drop rate (dropped / received).
    #[must_use]
    pub fn drop_rate(&self) -> f64 {
        if self.received == 0 {
            0.0
        } else {
            self.dropped as f64 / self.received as f64
        }
    }

    /// Returns the filter rate (filtered / processed).
    #[must_use]
    pub fn filter_rate(&self) -> f64 {
        if self.processed == 0 {
            0.0
        } else {
            self.filtered as f64 / self.processed as f64
        }
    }

    /// Returns the error rate (errors / received).
    #[must_use]
    pub fn error_rate(&self) -> f64 {
        if self.received == 0 {
            0.0
        } else {
            self.errors as f64 / self.received as f64
        }
    }
}

#[cfg(feature = "std")]
impl std::fmt::Display for PipelineCounterSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "received={} processed={} filtered={} dropped={} errors={}",
            self.received, self.processed, self.filtered, self.dropped, self.errors
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter_basic() {
        let counter = Counter::new();
        assert_eq!(counter.get(), 0);

        counter.increment();
        assert_eq!(counter.get(), 1);

        counter.add(5);
        assert_eq!(counter.get(), 6);
    }

    #[test]
    fn test_counter_reset() {
        let counter = Counter::with_value(10);
        let prev = counter.reset();
        assert_eq!(prev, 10);
        assert_eq!(counter.get(), 0);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_rate_counter() {
        let counter = RateCounter::new();
        counter.add(1000);

        // Count should be correct
        assert_eq!(counter.count(), 1000);

        // Rate might be 0 or very high depending on timing
        // Just check it doesn't panic and is >= 0
        let rate = counter.rate();
        assert!(rate >= 0.0, "rate should be non-negative");
    }

    #[test]
    fn test_pipeline_counters() {
        let counters = PipelineCounters::new();

        counters.received.increment();
        counters.received.increment();
        counters.processed.increment();
        counters.filtered.increment();

        let snapshot = counters.snapshot();
        assert_eq!(snapshot.received, 2);
        assert_eq!(snapshot.processed, 1);
        assert_eq!(snapshot.filtered, 1);
        assert_eq!(snapshot.dropped, 0);
    }

    #[test]
    fn test_counter_snapshot_rates() {
        let snapshot = PipelineCounterSnapshot {
            received: 100,
            processed: 90,
            filtered: 5,
            dropped: 10,
            errors: 1,
        };

        assert!((snapshot.drop_rate() - 0.1).abs() < 0.01);
        assert!((snapshot.filter_rate() - (5.0 / 90.0)).abs() < 0.01);
        assert!((snapshot.error_rate() - 0.01).abs() < 0.001);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_counter_concurrent() {
        use std::sync::Arc;
        use std::thread;

        let counter = Arc::new(Counter::new());
        let mut handles = vec![];

        for _ in 0..4 {
            let c = Arc::clone(&counter);
            handles.push(thread::spawn(move || {
                for _ in 0..1000 {
                    c.increment();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(counter.get(), 4000);
    }
}
