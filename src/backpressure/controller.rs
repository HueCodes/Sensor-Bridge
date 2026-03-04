//! Adaptive backpressure controller.
//!
//! This module provides an adaptive controller that adjusts quality/sampling
//! based on queue utilization to prevent overload.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use super::strategy::BackpressureStats;

/// Quality level for adaptive degradation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum QualityLevel {
    /// Full quality - accept all items.
    Full = 0,
    /// High quality - minor reduction.
    High = 1,
    /// Medium quality - moderate reduction.
    Medium = 2,
    /// Low quality - significant reduction.
    Low = 3,
    /// Minimal quality - accept only essential items.
    Minimal = 4,
}

impl QualityLevel {
    /// Returns the sampling rate for this quality level.
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        match self {
            Self::Full => 1,
            Self::High => 2,
            Self::Medium => 4,
            Self::Low => 8,
            Self::Minimal => 16,
        }
    }

    /// Creates a quality level from a numeric value.
    #[must_use]
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Full,
            1 => Self::High,
            2 => Self::Medium,
            3 => Self::Low,
            _ => Self::Minimal,
        }
    }
}

/// An adaptive controller that adjusts quality based on load.
///
/// Uses high and low water marks to determine when to degrade quality.
/// When utilization exceeds the high water mark, quality degrades.
/// When utilization falls below the low water mark, quality improves.
///
/// # Example
///
/// ```rust
/// use sensor_bridge::backpressure::{AdaptiveController, QualityLevel};
///
/// let controller = AdaptiveController::builder()
///     .high_water_mark(0.8)
///     .low_water_mark(0.5)
///     .build();
///
/// // Under normal load
/// assert!(controller.should_accept(0.3));
///
/// // Under high load - may drop items
/// let utilization = 0.9;
/// for _ in 0..100 {
///     controller.update(utilization);
/// }
/// // Quality may have degraded
/// ```
pub struct AdaptiveController {
    /// High water mark (utilization threshold to trigger degradation).
    high_water_mark: f32,
    /// Low water mark (utilization threshold to restore quality).
    low_water_mark: f32,
    /// Current quality level.
    quality_level: AtomicU32,
    /// Sampler for current quality level.
    sample_counter: AtomicU64,
    /// Hysteresis counter to prevent rapid oscillation.
    hysteresis_counter: AtomicU32,
    /// Hysteresis threshold.
    hysteresis_threshold: u32,
    /// Statistics.
    stats: BackpressureStats,
}

impl AdaptiveController {
    /// Creates a new adaptive controller with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            high_water_mark: 0.8,
            low_water_mark: 0.5,
            quality_level: AtomicU32::new(QualityLevel::Full as u32),
            sample_counter: AtomicU64::new(0),
            hysteresis_counter: AtomicU32::new(0),
            hysteresis_threshold: 10,
            stats: BackpressureStats::new(),
        }
    }

    /// Creates a builder for configuring the controller.
    #[must_use]
    pub fn builder() -> AdaptiveControllerBuilder {
        AdaptiveControllerBuilder::new()
    }

    /// Returns the current quality level.
    #[must_use]
    pub fn quality(&self) -> QualityLevel {
        QualityLevel::from_u8(self.quality_level.load(Ordering::Relaxed) as u8)
    }

    /// Checks if an item should be accepted based on current quality.
    #[inline]
    pub fn should_accept(&self, utilization: f32) -> bool {
        self.update(utilization);

        let quality = self.quality();
        let rate = quality.sample_rate();

        if rate == 1 {
            self.stats.record_accepted();
            return true;
        }

        let count = self.sample_counter.fetch_add(1, Ordering::Relaxed);
        let accepted = count % (rate as u64) == 0;

        if accepted {
            self.stats.record_accepted();
        } else {
            self.stats.record_sampled_out();
        }

        accepted
    }

    /// Updates the quality level based on current utilization.
    pub fn update(&self, utilization: f32) {
        let current = self.quality_level.load(Ordering::Relaxed);

        if utilization > self.high_water_mark {
            // Increase degradation
            let counter = self.hysteresis_counter.fetch_add(1, Ordering::Relaxed);
            if counter >= self.hysteresis_threshold && current < QualityLevel::Minimal as u32 {
                self.quality_level.store(current + 1, Ordering::Relaxed);
                self.hysteresis_counter.store(0, Ordering::Relaxed);
            }
        } else if utilization < self.low_water_mark {
            // Decrease degradation
            let counter = self.hysteresis_counter.fetch_add(1, Ordering::Relaxed);
            if counter >= self.hysteresis_threshold && current > QualityLevel::Full as u32 {
                self.quality_level.store(current - 1, Ordering::Relaxed);
                self.hysteresis_counter.store(0, Ordering::Relaxed);
            }
        } else {
            // In between - reset hysteresis
            self.hysteresis_counter.store(0, Ordering::Relaxed);
        }
    }

    /// Forces a specific quality level.
    pub fn set_quality(&self, level: QualityLevel) {
        self.quality_level.store(level as u32, Ordering::Relaxed);
        self.hysteresis_counter.store(0, Ordering::Relaxed);
    }

    /// Returns the statistics.
    #[must_use]
    pub fn stats(&self) -> &BackpressureStats {
        &self.stats
    }

    /// Returns the high water mark.
    #[must_use]
    pub fn high_water_mark(&self) -> f32 {
        self.high_water_mark
    }

    /// Returns the low water mark.
    #[must_use]
    pub fn low_water_mark(&self) -> f32 {
        self.low_water_mark
    }

    /// Resets the controller to initial state.
    pub fn reset(&self) {
        self.quality_level
            .store(QualityLevel::Full as u32, Ordering::Relaxed);
        self.sample_counter.store(0, Ordering::Relaxed);
        self.hysteresis_counter.store(0, Ordering::Relaxed);
        self.stats.reset();
    }
}

impl Default for AdaptiveController {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for `AdaptiveController`.
pub struct AdaptiveControllerBuilder {
    high_water_mark: f32,
    low_water_mark: f32,
    hysteresis_threshold: u32,
}

impl AdaptiveControllerBuilder {
    /// Creates a new builder with default values.
    #[must_use]
    pub fn new() -> Self {
        Self {
            high_water_mark: 0.8,
            low_water_mark: 0.5,
            hysteresis_threshold: 10,
        }
    }

    /// Sets the high water mark.
    #[must_use]
    pub fn high_water_mark(mut self, mark: f32) -> Self {
        self.high_water_mark = mark.clamp(0.0, 1.0);
        self
    }

    /// Sets the low water mark.
    #[must_use]
    pub fn low_water_mark(mut self, mark: f32) -> Self {
        self.low_water_mark = mark.clamp(0.0, 1.0);
        self
    }

    /// Sets the hysteresis threshold.
    #[must_use]
    pub fn hysteresis_threshold(mut self, threshold: u32) -> Self {
        self.hysteresis_threshold = threshold;
        self
    }

    /// Builds the controller.
    #[must_use]
    pub fn build(self) -> AdaptiveController {
        assert!(
            self.low_water_mark < self.high_water_mark,
            "low water mark must be less than high water mark"
        );

        AdaptiveController {
            high_water_mark: self.high_water_mark,
            low_water_mark: self.low_water_mark,
            quality_level: AtomicU32::new(QualityLevel::Full as u32),
            sample_counter: AtomicU64::new(0),
            hysteresis_counter: AtomicU32::new(0),
            hysteresis_threshold: self.hysteresis_threshold,
            stats: BackpressureStats::new(),
        }
    }
}

impl Default for AdaptiveControllerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quality_level() {
        assert_eq!(QualityLevel::Full.sample_rate(), 1);
        assert_eq!(QualityLevel::High.sample_rate(), 2);
        assert_eq!(QualityLevel::Medium.sample_rate(), 4);
        assert_eq!(QualityLevel::Low.sample_rate(), 8);
        assert_eq!(QualityLevel::Minimal.sample_rate(), 16);
    }

    #[test]
    fn test_quality_level_ordering() {
        assert!(QualityLevel::Full < QualityLevel::High);
        assert!(QualityLevel::High < QualityLevel::Medium);
        assert!(QualityLevel::Medium < QualityLevel::Low);
        assert!(QualityLevel::Low < QualityLevel::Minimal);
    }

    #[test]
    fn test_adaptive_controller_default() {
        let controller = AdaptiveController::new();
        assert_eq!(controller.quality(), QualityLevel::Full);
        assert_eq!(controller.high_water_mark(), 0.8);
        assert_eq!(controller.low_water_mark(), 0.5);
    }

    #[test]
    fn test_adaptive_controller_builder() {
        let controller = AdaptiveController::builder()
            .high_water_mark(0.9)
            .low_water_mark(0.6)
            .hysteresis_threshold(5)
            .build();

        assert_eq!(controller.high_water_mark(), 0.9);
        assert_eq!(controller.low_water_mark(), 0.6);
    }

    #[test]
    fn test_adaptive_controller_degradation() {
        let controller = AdaptiveController::builder()
            .high_water_mark(0.8)
            .low_water_mark(0.5)
            .hysteresis_threshold(5)
            .build();

        // Simulate high load
        for _ in 0..10 {
            controller.update(0.9);
        }

        assert!(controller.quality() > QualityLevel::Full);
    }

    #[test]
    fn test_adaptive_controller_recovery() {
        let controller = AdaptiveController::builder()
            .high_water_mark(0.8)
            .low_water_mark(0.5)
            .hysteresis_threshold(5)
            .build();

        // Degrade quality
        controller.set_quality(QualityLevel::Medium);

        // Simulate low load
        for _ in 0..10 {
            controller.update(0.3);
        }

        assert!(controller.quality() < QualityLevel::Medium);
    }

    #[test]
    fn test_adaptive_controller_hysteresis() {
        let controller = AdaptiveController::builder()
            .high_water_mark(0.8)
            .low_water_mark(0.5)
            .hysteresis_threshold(10)
            .build();

        // Not enough updates to trigger degradation
        for _ in 0..5 {
            controller.update(0.9);
        }

        assert_eq!(controller.quality(), QualityLevel::Full);

        // Continue until threshold
        for _ in 0..10 {
            controller.update(0.9);
        }

        // Now should have degraded
        assert!(controller.quality() > QualityLevel::Full);
    }

    #[test]
    fn test_adaptive_controller_should_accept() {
        let controller = AdaptiveController::new();

        // At full quality, should accept all
        for _ in 0..10 {
            assert!(controller.should_accept(0.3));
        }

        // Set to medium quality (sample rate 4)
        controller.set_quality(QualityLevel::Medium);

        // Should accept roughly 1/4
        // Use utilization in the middle range (between low and high water marks)
        // so quality level doesn't change
        let mut accepted = 0;
        for _ in 0..100 {
            if controller.should_accept(0.65) {
                accepted += 1;
            }
        }

        // Due to sampling, should be around 25
        assert!((20..=30).contains(&accepted), "accepted {accepted}");
    }

    #[test]
    fn test_adaptive_controller_stats() {
        let controller = AdaptiveController::new();

        for _ in 0..100 {
            controller.should_accept(0.3);
        }

        assert_eq!(controller.stats().accepted(), 100);
    }

    #[test]
    fn test_adaptive_controller_reset() {
        let controller = AdaptiveController::new();

        controller.set_quality(QualityLevel::Low);
        for _ in 0..10 {
            controller.should_accept(0.5);
        }

        controller.reset();

        assert_eq!(controller.quality(), QualityLevel::Full);
        assert_eq!(controller.stats().accepted(), 0);
    }

    #[test]
    #[should_panic(expected = "low water mark must be less than high water mark")]
    fn test_builder_invalid_water_marks() {
        let _ = AdaptiveController::builder()
            .high_water_mark(0.5)
            .low_water_mark(0.8)
            .build();
    }
}
