//! Token bucket rate limiter.
//!
//! This module provides a rate limiter based on the token bucket algorithm,
//! useful for controlling the rate at which items enter the pipeline.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Configuration for the rate limiter.
#[derive(Debug, Clone)]
pub struct RateLimiterConfig {
    /// Maximum number of tokens in the bucket.
    pub capacity: u64,
    /// Rate at which tokens are refilled (tokens per second).
    pub refill_rate: f64,
    /// Initial number of tokens.
    pub initial_tokens: u64,
}

impl Default for RateLimiterConfig {
    fn default() -> Self {
        Self {
            capacity: 100,
            refill_rate: 1000.0, // 1000 tokens/sec
            initial_tokens: 100,
        }
    }
}

impl RateLimiterConfig {
    /// Creates a new rate limiter configuration.
    #[must_use]
    pub fn new(rate_per_second: f64) -> Self {
        Self {
            capacity: (rate_per_second * 0.1) as u64, // 100ms worth of burst
            refill_rate: rate_per_second,
            initial_tokens: (rate_per_second * 0.1) as u64,
        }
    }

    /// Sets the bucket capacity.
    #[must_use]
    pub fn capacity(mut self, capacity: u64) -> Self {
        self.capacity = capacity;
        self
    }

    /// Sets the refill rate.
    #[must_use]
    pub fn refill_rate(mut self, rate: f64) -> Self {
        self.refill_rate = rate;
        self
    }

    /// Sets the initial number of tokens.
    #[must_use]
    pub fn initial_tokens(mut self, tokens: u64) -> Self {
        self.initial_tokens = tokens;
        self
    }
}

/// A token bucket rate limiter.
///
/// Allows a burst of up to `capacity` items, then limits to `refill_rate`
/// items per second.
///
/// # Example
///
/// ```rust
/// use sensor_bridge::backpressure::{RateLimiter, RateLimiterConfig};
///
/// // 1000 items per second with burst of 100
/// let limiter = RateLimiter::new(RateLimiterConfig::new(1000.0).capacity(100));
///
/// if limiter.try_acquire() {
///     // Process item
/// } else {
///     // Rate limited
/// }
/// ```
pub struct RateLimiter {
    config: RateLimiterConfig,
    /// Available tokens (stored as fixed-point with 32 fractional bits).
    tokens_fp: AtomicU64,
    /// Last update time in nanoseconds since creation.
    last_update_ns: AtomicU64,
    /// Creation time.
    created: Instant,
    /// Number of acquisitions.
    acquired_count: AtomicU64,
    /// Number of rejections.
    rejected_count: AtomicU64,
}

impl RateLimiter {
    /// Fixed-point scale factor.
    const FP_SCALE: u64 = 1 << 32;

    /// Creates a new rate limiter with the given configuration.
    #[must_use]
    pub fn new(config: RateLimiterConfig) -> Self {
        let initial_tokens_fp = config.initial_tokens.saturating_mul(Self::FP_SCALE);
        Self {
            config,
            tokens_fp: AtomicU64::new(initial_tokens_fp),
            last_update_ns: AtomicU64::new(0),
            created: Instant::now(),
            acquired_count: AtomicU64::new(0),
            rejected_count: AtomicU64::new(0),
        }
    }

    /// Creates a new rate limiter with a simple rate limit.
    #[must_use]
    pub fn with_rate(rate_per_second: f64) -> Self {
        Self::new(RateLimiterConfig::new(rate_per_second))
    }

    /// Tries to acquire a token.
    ///
    /// Returns `true` if a token was acquired, `false` if rate limited.
    #[inline]
    pub fn try_acquire(&self) -> bool {
        self.try_acquire_n(1)
    }

    /// Tries to acquire multiple tokens.
    pub fn try_acquire_n(&self, n: u64) -> bool {
        self.refill();

        let needed_fp = n.saturating_mul(Self::FP_SCALE);

        loop {
            let current = self.tokens_fp.load(Ordering::Relaxed);
            if current < needed_fp {
                self.rejected_count.fetch_add(1, Ordering::Relaxed);
                return false;
            }

            let new_tokens = current - needed_fp;
            if self
                .tokens_fp
                .compare_exchange_weak(current, new_tokens, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                self.acquired_count.fetch_add(1, Ordering::Relaxed);
                return true;
            }
        }
    }

    /// Waits until a token is available (blocking).
    pub fn acquire(&self) {
        while !self.try_acquire() {
            std::thread::sleep(Duration::from_micros(100));
        }
    }

    /// Waits until a token is available or timeout.
    ///
    /// Returns `true` if a token was acquired, `false` if timeout.
    pub fn acquire_timeout(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;

        while Instant::now() < deadline {
            if self.try_acquire() {
                return true;
            }
            std::thread::sleep(Duration::from_micros(100));
        }

        false
    }

    /// Refills tokens based on elapsed time.
    fn refill(&self) {
        let now_ns = self.created.elapsed().as_nanos() as u64;
        let last_ns = self.last_update_ns.swap(now_ns, Ordering::Relaxed);

        if now_ns <= last_ns {
            return;
        }

        let elapsed_ns = now_ns - last_ns;
        let elapsed_secs = elapsed_ns as f64 / 1_000_000_000.0;

        // Calculate tokens to add
        let tokens_to_add = (self.config.refill_rate * elapsed_secs) as u64;
        if tokens_to_add == 0 {
            return;
        }

        let tokens_to_add_fp = tokens_to_add.saturating_mul(Self::FP_SCALE);
        let max_tokens_fp = self.config.capacity.saturating_mul(Self::FP_SCALE);

        // Add tokens, clamping to capacity
        let _ = self
            .tokens_fp
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.saturating_add(tokens_to_add_fp).min(max_tokens_fp))
            });
    }

    /// Returns the current number of available tokens.
    #[must_use]
    pub fn available_tokens(&self) -> f64 {
        self.refill();
        self.tokens_fp.load(Ordering::Relaxed) as f64 / Self::FP_SCALE as f64
    }

    /// Returns the number of successful acquisitions.
    #[must_use]
    pub fn acquired_count(&self) -> u64 {
        self.acquired_count.load(Ordering::Relaxed)
    }

    /// Returns the number of rejections.
    #[must_use]
    pub fn rejected_count(&self) -> u64 {
        self.rejected_count.load(Ordering::Relaxed)
    }

    /// Returns the rejection rate.
    #[must_use]
    pub fn rejection_rate(&self) -> f64 {
        let acquired = self.acquired_count();
        let rejected = self.rejected_count();
        let total = acquired + rejected;

        if total == 0 {
            0.0
        } else {
            rejected as f64 / total as f64
        }
    }

    /// Returns the configured rate.
    #[must_use]
    pub fn rate(&self) -> f64 {
        self.config.refill_rate
    }

    /// Returns the bucket capacity.
    #[must_use]
    pub fn capacity(&self) -> u64 {
        self.config.capacity
    }

    /// Resets the limiter to initial state.
    pub fn reset(&self) {
        let initial_tokens_fp = self.config.initial_tokens.saturating_mul(Self::FP_SCALE);
        self.tokens_fp.store(initial_tokens_fp, Ordering::Relaxed);
        self.last_update_ns.store(0, Ordering::Relaxed);
        self.acquired_count.store(0, Ordering::Relaxed);
        self.rejected_count.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_rate_limiter_config() {
        let config = RateLimiterConfig::new(1000.0)
            .capacity(200)
            .initial_tokens(50);

        assert_eq!(config.capacity, 200);
        assert_eq!(config.initial_tokens, 50);
        assert_eq!(config.refill_rate, 1000.0);
    }

    #[test]
    fn test_rate_limiter_basic() {
        let limiter = RateLimiter::new(
            RateLimiterConfig::default()
                .capacity(10)
                .initial_tokens(10)
                .refill_rate(1000.0),
        );

        // Should be able to acquire initial tokens
        for _ in 0..10 {
            assert!(limiter.try_acquire());
        }

        // Should be rejected now
        assert!(!limiter.try_acquire());
    }

    #[test]
    fn test_rate_limiter_refill() {
        let limiter = RateLimiter::new(
            RateLimiterConfig::default()
                .capacity(10)
                .initial_tokens(0)
                .refill_rate(10000.0), // 10000/sec = 1 per 0.1ms
        );

        // Initially empty
        assert!(!limiter.try_acquire());

        // Wait for refill
        thread::sleep(Duration::from_millis(10));

        // Should have some tokens now
        assert!(limiter.try_acquire());
    }

    #[test]
    fn test_rate_limiter_burst() {
        let limiter = RateLimiter::new(
            RateLimiterConfig::default()
                .capacity(100)
                .initial_tokens(100)
                .refill_rate(10.0),
        );

        // Burst: can acquire many quickly
        for _ in 0..100 {
            assert!(limiter.try_acquire());
        }

        // Now limited
        assert!(!limiter.try_acquire());
    }

    #[test]
    fn test_rate_limiter_acquire_n() {
        let limiter = RateLimiter::new(
            RateLimiterConfig::default()
                .capacity(10)
                .initial_tokens(10)
                .refill_rate(1000.0),
        );

        // Acquire 5 tokens
        assert!(limiter.try_acquire_n(5));
        assert!(limiter.try_acquire_n(5));
        assert!(!limiter.try_acquire_n(1));
    }

    #[test]
    fn test_rate_limiter_stats() {
        let limiter = RateLimiter::new(
            RateLimiterConfig::default()
                .capacity(5)
                .initial_tokens(5)
                .refill_rate(0.0),
        );

        for _ in 0..10 {
            limiter.try_acquire();
        }

        assert_eq!(limiter.acquired_count(), 5);
        assert_eq!(limiter.rejected_count(), 5);
        assert!((limiter.rejection_rate() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_rate_limiter_reset() {
        let limiter = RateLimiter::new(
            RateLimiterConfig::default()
                .capacity(10)
                .initial_tokens(10)
                .refill_rate(0.0),
        );

        // Use all tokens
        for _ in 0..10 {
            limiter.try_acquire();
        }
        assert!(!limiter.try_acquire());

        // Reset
        limiter.reset();

        // Should have tokens again
        assert!(limiter.try_acquire());
    }

    #[test]
    fn test_rate_limiter_with_rate() {
        let limiter = RateLimiter::with_rate(1000.0);
        assert_eq!(limiter.rate(), 1000.0);
    }

    #[test]
    fn test_rate_limiter_concurrent() {
        let limiter = std::sync::Arc::new(RateLimiter::new(
            RateLimiterConfig::default()
                .capacity(1000)
                .initial_tokens(1000)
                .refill_rate(10000.0),
        ));

        let mut handles = vec![];

        for _ in 0..4 {
            let limiter = limiter.clone();
            handles.push(thread::spawn(move || {
                let mut acquired = 0;
                for _ in 0..1000 {
                    if limiter.try_acquire() {
                        acquired += 1;
                    }
                }
                acquired
            }));
        }

        let total: u32 = handles.into_iter().map(|h| h.join().unwrap()).sum();

        // Should have acquired roughly the capacity
        assert!(total >= 1000, "total acquired: {}", total);
    }
}
