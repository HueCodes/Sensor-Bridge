//! Signal processing filters for sensor data.
//!
//! This module provides common filters used in robotics:
//! - [`MovingAverage`]: Simple moving average with O(1) update
//! - [`ExponentialMovingAverage`]: Exponential smoothing
//! - [`KalmanFilter1D`]: 1D Kalman filter for state estimation
//!
//! All filters implement [`Stage`], allowing them to be composed in pipelines.

use core::ops::{Add, Div, Mul, Sub};

use super::Stage;

/// Trait for values that can be used in numeric filters.
///
/// This is automatically implemented for types that support the required operations.
pub trait Filterable:
    Copy
    + Default
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<f32, Output = Self>
    + Div<f32, Output = Self>
{
}

impl<T> Filterable for T where
    T: Copy
        + Default
        + Add<Output = T>
        + Sub<Output = T>
        + Mul<f32, Output = T>
        + Div<f32, Output = T>
{
}

/// A simple moving average filter with O(1) update complexity.
///
/// Maintains a fixed-size sliding window and computes the average of all
/// values in the window. Uses a running sum for O(1) updates.
///
/// # Window Behavior
///
/// - Before the window is full, returns the average of available samples
/// - After the window is full, each new sample replaces the oldest
///
/// # Example
///
/// ```rust
/// use sensor_bridge::stage::{Stage, MovingAverage};
///
/// let mut filter: MovingAverage<f32, 3> = MovingAverage::new();
///
/// assert_eq!(filter.process(3.0), Some(3.0));  // [3]     avg = 3
/// assert_eq!(filter.process(6.0), Some(4.5));  // [3,6]   avg = 4.5
/// assert_eq!(filter.process(9.0), Some(6.0));  // [3,6,9] avg = 6
/// assert_eq!(filter.process(12.0), Some(9.0)); // [6,9,12] avg = 9
/// ```
#[derive(Debug, Clone)]
pub struct MovingAverage<T, const WINDOW: usize> {
    buffer: [T; WINDOW],
    sum: T,
    index: usize,
    count: usize,
}

impl<T: Filterable, const WINDOW: usize> MovingAverage<T, WINDOW> {
    /// Creates a new moving average filter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buffer: [T::default(); WINDOW],
            sum: T::default(),
            index: 0,
            count: 0,
        }
    }

    /// Returns the current average, or `None` if no samples have been added.
    #[must_use]
    pub fn current(&self) -> Option<T> {
        if self.count == 0 {
            None
        } else {
            Some(self.sum / self.count as f32)
        }
    }

    /// Returns the number of samples in the window.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Returns `true` if no samples have been added.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Returns `true` if the window is full.
    #[must_use]
    pub const fn is_full(&self) -> bool {
        self.count >= WINDOW
    }
}

impl<T: Filterable, const WINDOW: usize> Default for MovingAverage<T, WINDOW> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Filterable + Send, const WINDOW: usize> Stage for MovingAverage<T, WINDOW> {
    type Input = T;
    type Output = T;

    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        if self.count >= WINDOW {
            // Window is full, subtract the oldest value
            self.sum = self.sum - self.buffer[self.index];
        } else {
            self.count += 1;
        }

        // Add new value
        self.buffer[self.index] = input;
        self.sum = self.sum + input;
        self.index = (self.index + 1) % WINDOW;

        Some(self.sum / self.count as f32)
    }

    fn reset(&mut self) {
        self.buffer = [T::default(); WINDOW];
        self.sum = T::default();
        self.index = 0;
        self.count = 0;
    }
}

/// An exponential moving average (EMA) filter.
///
/// The EMA gives more weight to recent observations while still considering
/// older data. It's computationally efficient (O(1) per sample) and doesn't
/// require storing historical values.
///
/// # Formula
///
/// `EMA_t = α * x_t + (1 - α) * EMA_{t-1}`
///
/// Where:
/// - `α` (alpha) is the smoothing factor (0 < α ≤ 1)
/// - Higher α = more responsive to recent changes
/// - Lower α = smoother output
///
/// # Example
///
/// ```rust
/// use sensor_bridge::stage::{Stage, ExponentialMovingAverage};
///
/// // High alpha (0.9) = responsive
/// let mut responsive: ExponentialMovingAverage<f32> = ExponentialMovingAverage::new(0.9);
///
/// // Low alpha (0.1) = smooth
/// let mut smooth: ExponentialMovingAverage<f32> = ExponentialMovingAverage::new(0.1);
/// ```
#[derive(Debug, Clone)]
pub struct ExponentialMovingAverage<T> {
    alpha: f32,
    value: Option<T>,
}

impl<T: Filterable> ExponentialMovingAverage<T> {
    /// Creates a new EMA filter with the given smoothing factor.
    ///
    /// # Panics
    ///
    /// Panics if alpha is not in (0, 1].
    #[must_use]
    pub fn new(alpha: f32) -> Self {
        assert!(
            alpha > 0.0 && alpha <= 1.0,
            "Alpha must be in (0, 1], got {alpha}"
        );
        Self { alpha, value: None }
    }

    /// Creates an EMA filter from a time constant.
    ///
    /// The time constant τ determines how quickly the filter responds.
    /// After τ time units, the step response reaches ~63.2% of the final value.
    ///
    /// `alpha = dt / (τ + dt)` where `dt` is the sample period.
    #[must_use]
    pub fn from_time_constant(tau: f32, sample_rate_hz: f32) -> Self {
        let dt = 1.0 / sample_rate_hz;
        let alpha = dt / (tau + dt);
        Self::new(alpha)
    }

    /// Creates an EMA filter with a cutoff frequency.
    ///
    /// The cutoff frequency fc is where the filter attenuates by 3dB.
    #[must_use]
    pub fn from_cutoff_frequency(cutoff_hz: f32, sample_rate_hz: f32) -> Self {
        let dt = 1.0 / sample_rate_hz;
        let rc = 1.0 / (2.0 * core::f32::consts::PI * cutoff_hz);
        let alpha = dt / (rc + dt);
        Self::new(alpha)
    }

    /// Returns the current filtered value.
    #[must_use]
    pub fn current(&self) -> Option<T> {
        self.value
    }

    /// Returns the smoothing factor.
    #[must_use]
    pub const fn alpha(&self) -> f32 {
        self.alpha
    }
}

impl<T: Filterable + Send> Stage for ExponentialMovingAverage<T> {
    type Input = T;
    type Output = T;

    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        let new_value = match self.value {
            Some(prev) => {
                // EMA formula: alpha * input + (1 - alpha) * prev
                input * self.alpha + prev * (1.0 - self.alpha)
            }
            None => input, // First sample
        };

        self.value = Some(new_value);
        Some(new_value)
    }

    fn reset(&mut self) {
        self.value = None;
    }
}

/// A simple 1D Kalman filter for state estimation.
///
/// The Kalman filter is optimal for linear systems with Gaussian noise.
/// This implementation handles a 1D state with:
/// - Process noise (Q): Uncertainty in the model
/// - Measurement noise (R): Uncertainty in the sensor
///
/// # Example
///
/// ```rust
/// use sensor_bridge::stage::{Stage, KalmanFilter1D};
///
/// let mut filter = KalmanFilter1D::new(
///     0.0,   // Initial estimate
///     1.0,   // Initial error covariance
///     0.01,  // Process noise (Q)
///     0.1,   // Measurement noise (R)
/// );
///
/// // Feed in noisy measurements
/// let estimate = filter.process(1.2);
/// ```
#[derive(Debug, Clone)]
pub struct KalmanFilter1D {
    /// Current state estimate.
    estimate: f64,
    /// Current error covariance (uncertainty).
    error_covariance: f64,
    /// Process noise covariance (Q).
    process_noise: f64,
    /// Measurement noise covariance (R).
    measurement_noise: f64,
}

impl KalmanFilter1D {
    /// Creates a new 1D Kalman filter.
    ///
    /// # Arguments
    ///
    /// * `initial_estimate` - Initial state estimate
    /// * `initial_error` - Initial error covariance (uncertainty)
    /// * `process_noise` - Process noise covariance (Q)
    /// * `measurement_noise` - Measurement noise covariance (R)
    #[must_use]
    pub fn new(
        initial_estimate: f64,
        initial_error: f64,
        process_noise: f64,
        measurement_noise: f64,
    ) -> Self {
        Self {
            estimate: initial_estimate,
            error_covariance: initial_error,
            process_noise,
            measurement_noise,
        }
    }

    /// Creates a Kalman filter with default parameters.
    ///
    /// Good starting point for many applications.
    #[must_use]
    pub fn default_params() -> Self {
        Self::new(0.0, 1.0, 0.01, 0.1)
    }

    /// Returns the current state estimate.
    #[must_use]
    pub const fn estimate(&self) -> f64 {
        self.estimate
    }

    /// Returns the current error covariance (uncertainty).
    #[must_use]
    pub const fn error_covariance(&self) -> f64 {
        self.error_covariance
    }

    /// Returns the Kalman gain from the last update.
    ///
    /// The gain indicates how much the filter trusts the measurement vs the prediction.
    /// - Gain close to 1: Trusts measurement more
    /// - Gain close to 0: Trusts prediction more
    #[must_use]
    pub fn kalman_gain(&self) -> f64 {
        self.error_covariance / (self.error_covariance + self.measurement_noise)
    }

    /// Updates the filter with a new measurement.
    ///
    /// Returns the updated state estimate.
    pub fn update(&mut self, measurement: f64) -> f64 {
        // Prediction step (with no control input, state stays the same)
        // P_predicted = P + Q
        let predicted_covariance = self.error_covariance + self.process_noise;

        // Update step
        // K = P_predicted / (P_predicted + R)
        let kalman_gain = predicted_covariance / (predicted_covariance + self.measurement_noise);

        // x = x + K * (measurement - x)
        self.estimate += kalman_gain * (measurement - self.estimate);

        // P = (1 - K) * P_predicted
        self.error_covariance = (1.0 - kalman_gain) * predicted_covariance;

        self.estimate
    }

    /// Sets the process noise covariance.
    pub fn set_process_noise(&mut self, q: f64) {
        self.process_noise = q;
    }

    /// Sets the measurement noise covariance.
    pub fn set_measurement_noise(&mut self, r: f64) {
        self.measurement_noise = r;
    }
}

impl Stage for KalmanFilter1D {
    type Input = f64;
    type Output = f64;

    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        Some(self.update(input))
    }

    fn reset(&mut self) {
        self.estimate = 0.0;
        self.error_covariance = 1.0;
    }
}

/// A low-pass filter using a simple first-order IIR design.
///
/// Attenuates high-frequency noise while passing low-frequency signals.
/// This is essentially an EMA with a cutoff frequency specification.
#[derive(Debug, Clone)]
pub struct LowPassFilter<T> {
    ema: ExponentialMovingAverage<T>,
}

impl<T: Filterable> LowPassFilter<T> {
    /// Creates a new low-pass filter with the given cutoff frequency.
    ///
    /// # Arguments
    ///
    /// * `cutoff_hz` - The 3dB cutoff frequency
    /// * `sample_rate_hz` - The sample rate of the input signal
    #[must_use]
    pub fn new(cutoff_hz: f32, sample_rate_hz: f32) -> Self {
        Self {
            ema: ExponentialMovingAverage::from_cutoff_frequency(cutoff_hz, sample_rate_hz),
        }
    }

    /// Returns the current filtered value.
    #[must_use]
    pub fn current(&self) -> Option<T> {
        self.ema.current()
    }
}

impl<T: Filterable + Send> Stage for LowPassFilter<T> {
    type Input = T;
    type Output = T;

    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        self.ema.process(input)
    }

    fn reset(&mut self) {
        self.ema.reset();
    }
}

/// A high-pass filter that removes low-frequency components.
///
/// Useful for removing DC bias or slow drift from sensor readings.
#[derive(Debug, Clone)]
pub struct HighPassFilter<T> {
    alpha: f32,
    prev_input: Option<T>,
    prev_output: Option<T>,
}

impl<T: Filterable> HighPassFilter<T> {
    /// Creates a new high-pass filter with the given cutoff frequency.
    #[must_use]
    pub fn new(cutoff_hz: f32, sample_rate_hz: f32) -> Self {
        let dt = 1.0 / sample_rate_hz;
        let rc = 1.0 / (2.0 * core::f32::consts::PI * cutoff_hz);
        let alpha = rc / (rc + dt);

        Self {
            alpha,
            prev_input: None,
            prev_output: None,
        }
    }

    /// Returns the current filtered value.
    #[must_use]
    pub fn current(&self) -> Option<T> {
        self.prev_output
    }
}

impl<T: Filterable + Send> Stage for HighPassFilter<T> {
    type Input = T;
    type Output = T;

    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        let output = match (self.prev_input, self.prev_output) {
            (Some(prev_in), Some(prev_out)) => {
                // y[n] = alpha * (y[n-1] + x[n] - x[n-1])
                (prev_out + input - prev_in) * self.alpha
            }
            _ => input, // First sample, pass through
        };

        self.prev_input = Some(input);
        self.prev_output = Some(output);
        Some(output)
    }

    fn reset(&mut self) {
        self.prev_input = None;
        self.prev_output = None;
    }
}

/// A median filter that removes spike noise.
///
/// Outputs the median of the last N samples. Effective at removing
/// outliers/spikes while preserving edges better than averaging filters.
#[derive(Debug, Clone)]
pub struct MedianFilter<T, const WINDOW: usize> {
    buffer: [T; WINDOW],
    sorted: [T; WINDOW],
    index: usize,
    count: usize,
}

impl<T: Copy + Default + PartialOrd, const WINDOW: usize> MedianFilter<T, WINDOW> {
    /// Creates a new median filter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buffer: [T::default(); WINDOW],
            sorted: [T::default(); WINDOW],
            index: 0,
            count: 0,
        }
    }

    fn compute_median(&mut self) -> T {
        // Copy current values to sorted buffer
        let len = self.count.min(WINDOW);
        self.sorted[..len].copy_from_slice(&self.buffer[..len]);

        // Simple insertion sort (efficient for small N)
        for i in 1..len {
            let key = self.sorted[i];
            let mut j = i;
            while j > 0 && self.sorted[j - 1] > key {
                self.sorted[j] = self.sorted[j - 1];
                j -= 1;
            }
            self.sorted[j] = key;
        }

        // Return median
        self.sorted[len / 2]
    }
}

impl<T: Copy + Default + PartialOrd, const WINDOW: usize> Default for MedianFilter<T, WINDOW> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy + Default + PartialOrd + Send, const WINDOW: usize> Stage for MedianFilter<T, WINDOW> {
    type Input = T;
    type Output = T;

    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        self.buffer[self.index] = input;
        self.index = (self.index + 1) % WINDOW;
        if self.count < WINDOW {
            self.count += 1;
        }

        Some(self.compute_median())
    }

    fn reset(&mut self) {
        self.buffer = [T::default(); WINDOW];
        self.index = 0;
        self.count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_moving_average_basic() {
        let mut filter: MovingAverage<f32, 3> = MovingAverage::new();

        assert_eq!(filter.process(3.0), Some(3.0));
        assert_eq!(filter.process(6.0), Some(4.5));
        assert_eq!(filter.process(9.0), Some(6.0));
        assert_eq!(filter.process(12.0), Some(9.0)); // Window slides
    }

    #[test]
    fn test_moving_average_reset() {
        let mut filter: MovingAverage<f32, 3> = MovingAverage::new();

        filter.process(10.0);
        filter.process(20.0);
        assert!(filter.len() == 2);

        filter.reset();
        assert!(filter.is_empty());
        assert_eq!(filter.process(5.0), Some(5.0));
    }

    #[test]
    fn test_ema_basic() {
        let mut filter: ExponentialMovingAverage<f32> = ExponentialMovingAverage::new(0.5);

        // First sample is passed through
        assert_eq!(filter.process(10.0), Some(10.0));
        // Second: 0.5 * 20 + 0.5 * 10 = 15
        assert_eq!(filter.process(20.0), Some(15.0));
        // Third: 0.5 * 30 + 0.5 * 15 = 22.5
        assert_eq!(filter.process(30.0), Some(22.5));
    }

    #[test]
    fn test_ema_time_constant() {
        // With tau = 1s at 10Hz, alpha should be approximately 0.0909
        let filter = ExponentialMovingAverage::<f32>::from_time_constant(1.0, 10.0);
        let alpha = filter.alpha();
        assert!((alpha - 0.0909).abs() < 0.001);
    }

    #[test]
    fn test_kalman_basic() {
        let mut filter = KalmanFilter1D::new(0.0, 1.0, 0.01, 0.1);

        // Feed in constant measurements, estimate should converge
        for _ in 0..100 {
            filter.update(10.0);
        }

        assert!((filter.estimate() - 10.0).abs() < 0.1);
    }

    #[test]
    fn test_kalman_noisy_measurements() {
        let mut filter = KalmanFilter1D::new(0.0, 1.0, 0.01, 0.1);

        // Simulate noisy measurements around 5.0
        let measurements = [5.1, 4.9, 5.2, 4.8, 5.0, 5.1, 4.9, 5.0, 5.1, 4.9];
        for &m in &measurements {
            filter.update(m);
        }

        // Estimate should be close to 5.0
        assert!((filter.estimate() - 5.0).abs() < 0.5);
    }

    #[test]
    fn test_low_pass_filter() {
        let mut filter: LowPassFilter<f32> = LowPassFilter::new(10.0, 100.0);

        // Step response
        for _ in 0..50 {
            filter.process(1.0);
        }

        // Should be close to 1.0 after settling
        assert!((filter.current().unwrap() - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_high_pass_removes_dc() {
        let mut filter: HighPassFilter<f32> = HighPassFilter::new(0.1, 100.0);

        // Feed constant value (DC component)
        for _ in 0..1000 {
            filter.process(10.0);
        }

        // DC should be removed, output near zero
        assert!(filter.current().unwrap().abs() < 0.1);
    }

    #[test]
    fn test_median_filter_removes_spikes() {
        let mut filter: MedianFilter<f32, 5> = MedianFilter::new();

        // Normal values
        assert!(filter.process(1.0).is_some());
        assert!(filter.process(1.0).is_some());
        assert!(filter.process(1.0).is_some());

        // Spike!
        let result = filter.process(100.0);

        // Median should still be 1.0 (spike is filtered)
        assert!((result.unwrap() - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_filters_implement_stage() {
        fn assert_stage<T: Stage>() {}

        assert_stage::<MovingAverage<f32, 10>>();
        assert_stage::<ExponentialMovingAverage<f32>>();
        assert_stage::<KalmanFilter1D>();
        assert_stage::<LowPassFilter<f32>>();
        assert_stage::<HighPassFilter<f32>>();
        assert_stage::<MedianFilter<f32, 5>>();
    }
}
