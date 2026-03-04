//! Mock sensor implementations for testing.
//!
//! These mock sensors generate synthetic data that mimics real sensor behavior,
//! useful for testing and development without physical hardware.

use crate::error::Result;
use crate::sensor::imu::{ImuReading, Vec3};
use crate::sensor::Sensor;
use crate::timestamp::{MonotonicClock, Timestamped};

/// Configuration for mock sensor noise generation.
#[derive(Debug, Clone, Copy)]
pub struct NoiseConfig {
    /// Standard deviation of Gaussian noise.
    pub std_dev: f32,
    /// Random walk bias drift rate.
    pub bias_drift: f32,
    /// Probability of a spike/outlier (0.0-1.0).
    pub spike_probability: f32,
    /// Magnitude of spikes when they occur.
    pub spike_magnitude: f32,
}

impl Default for NoiseConfig {
    fn default() -> Self {
        Self {
            std_dev: 0.01,
            bias_drift: 0.0001,
            spike_probability: 0.0,
            spike_magnitude: 0.0,
        }
    }
}

impl NoiseConfig {
    /// Creates a noise-free configuration.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            std_dev: 0.0,
            bias_drift: 0.0,
            spike_probability: 0.0,
            spike_magnitude: 0.0,
        }
    }

    /// Creates a low-noise configuration (typical for good sensors).
    #[must_use]
    pub const fn low() -> Self {
        Self {
            std_dev: 0.005,
            bias_drift: 0.00001,
            spike_probability: 0.0,
            spike_magnitude: 0.0,
        }
    }

    /// Creates a high-noise configuration (typical for cheap sensors).
    #[must_use]
    pub const fn high() -> Self {
        Self {
            std_dev: 0.1,
            bias_drift: 0.001,
            spike_probability: 0.01,
            spike_magnitude: 1.0,
        }
    }
}

/// A simple linear congruential generator for deterministic "random" values.
///
/// This is intentionally simple and deterministic for reproducible tests.
/// NOT cryptographically secure - only for testing.
#[derive(Debug, Clone)]
pub struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    /// Creates a new RNG with the given seed.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self {
            state: seed.wrapping_add(1), // Avoid zero state
        }
    }

    /// Generates the next random u64.
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        // LCG parameters from Numerical Recipes
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    /// Generates a random f32 in [0, 1).
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Generates a random f32 in [-1, 1).
    #[inline]
    pub fn next_f32_signed(&mut self) -> f32 {
        self.next_f32() * 2.0 - 1.0
    }

    /// Generates approximate Gaussian noise using Box-Muller (simplified).
    pub fn next_gaussian(&mut self) -> f32 {
        // Use sum of uniforms approximation for speed
        let sum: f32 = (0..12).map(|_| self.next_f32()).sum();
        sum - 6.0 // Approximate N(0,1)
    }
}

impl Default for SimpleRng {
    fn default() -> Self {
        Self::new(42)
    }
}

/// A mock IMU sensor that generates synthetic data.
///
/// Can be configured to simulate:
/// - Stationary sensor (constant gravity reading)
/// - Constant rotation
/// - Sinusoidal motion
/// - Custom motion patterns
/// - Various noise profiles
#[cfg(feature = "std")]
pub struct MockImu {
    name: &'static str,
    sample_rate_hz: f64,
    clock: MonotonicClock,
    rng: SimpleRng,

    // Configuration
    /// Base acceleration (e.g., gravity when stationary).
    pub base_accel: Vec3,
    /// Base angular velocity.
    pub base_gyro: Vec3,
    /// Noise configuration for accelerometer.
    pub accel_noise: NoiseConfig,
    /// Noise configuration for gyroscope.
    pub gyro_noise: NoiseConfig,

    // State for motion simulation
    motion_phase: f64,
    motion_frequency: f64,
    motion_amplitude: Vec3,

    // Bias state for random walk
    accel_bias: Vec3,
    gyro_bias: Vec3,
}

#[cfg(feature = "std")]
impl MockImu {
    /// Creates a new mock IMU with the given sample rate.
    #[must_use]
    pub fn new(sample_rate_hz: f64) -> Self {
        Self {
            name: "mock_imu",
            sample_rate_hz,
            clock: MonotonicClock::new(),
            rng: SimpleRng::default(),
            base_accel: Vec3::new(0.0, 0.0, -9.81), // Gravity in Z-down frame
            base_gyro: Vec3::zero(),
            accel_noise: NoiseConfig::low(),
            gyro_noise: NoiseConfig::low(),
            motion_phase: 0.0,
            motion_frequency: 0.0,
            motion_amplitude: Vec3::zero(),
            accel_bias: Vec3::zero(),
            gyro_bias: Vec3::zero(),
        }
    }

    /// Sets the sensor name.
    #[must_use]
    pub fn with_name(mut self, name: &'static str) -> Self {
        self.name = name;
        self
    }

    /// Sets the base (static) acceleration.
    #[must_use]
    pub fn with_base_accel(mut self, accel: Vec3) -> Self {
        self.base_accel = accel;
        self
    }

    /// Sets the base (static) angular velocity.
    #[must_use]
    pub fn with_base_gyro(mut self, gyro: Vec3) -> Self {
        self.base_gyro = gyro;
        self
    }

    /// Configures sinusoidal motion.
    #[must_use]
    pub fn with_sinusoidal_motion(mut self, frequency: f64, amplitude: Vec3) -> Self {
        self.motion_frequency = frequency;
        self.motion_amplitude = amplitude;
        self
    }

    /// Sets the noise configuration.
    #[must_use]
    pub fn with_noise(mut self, accel_noise: NoiseConfig, gyro_noise: NoiseConfig) -> Self {
        self.accel_noise = accel_noise;
        self.gyro_noise = gyro_noise;
        self
    }

    /// Sets the RNG seed for reproducible results.
    #[must_use]
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.rng = SimpleRng::new(seed);
        self
    }

    fn generate_noise_value(&mut self, std_dev: f32, spike_prob: f32, spike_mag: f32) -> f32 {
        let mut noise = 0.0f32;

        // Gaussian noise
        if std_dev > 0.0 {
            noise += self.rng.next_gaussian() * std_dev;
        }

        // Spike/outlier
        if spike_prob > 0.0 && self.rng.next_f32() < spike_prob {
            noise += self.rng.next_f32_signed() * spike_mag;
        }

        noise
    }

    fn generate_bias_delta(&mut self, drift: f32) -> Vec3 {
        if drift > 0.0 {
            Vec3::new(
                self.rng.next_f32_signed() * drift,
                self.rng.next_f32_signed() * drift,
                self.rng.next_f32_signed() * drift,
            )
        } else {
            Vec3::zero()
        }
    }
}

#[cfg(feature = "std")]
impl Sensor for MockImu {
    type Reading = ImuReading;

    fn sample(&mut self) -> Result<Timestamped<Self::Reading>> {
        // Update motion phase
        let dt = 1.0 / self.sample_rate_hz;
        self.motion_phase += self.motion_frequency * dt * core::f64::consts::TAU;

        // Calculate motion contribution
        let phase_sin = self.motion_phase.sin() as f32;
        let motion_accel = self.motion_amplitude * phase_sin;

        // Copy noise config values to avoid borrow issues
        let accel_drift = self.accel_noise.bias_drift;
        let gyro_drift = self.gyro_noise.bias_drift;
        let accel_std = self.accel_noise.std_dev;
        let accel_spike_prob = self.accel_noise.spike_probability;
        let accel_spike_mag = self.accel_noise.spike_magnitude;
        let gyro_std = self.gyro_noise.std_dev;
        let gyro_spike_prob = self.gyro_noise.spike_probability;
        let gyro_spike_mag = self.gyro_noise.spike_magnitude;

        // Update biases (random walk)
        let accel_bias_delta = self.generate_bias_delta(accel_drift);
        let gyro_bias_delta = self.generate_bias_delta(gyro_drift);
        self.accel_bias = self.accel_bias + accel_bias_delta;
        self.gyro_bias = self.gyro_bias + gyro_bias_delta;

        // Generate noise values
        let accel_noise_x = self.generate_noise_value(accel_std, accel_spike_prob, accel_spike_mag);
        let accel_noise_y = self.generate_noise_value(accel_std, accel_spike_prob, accel_spike_mag);
        let accel_noise_z = self.generate_noise_value(accel_std, accel_spike_prob, accel_spike_mag);
        let gyro_noise_x = self.generate_noise_value(gyro_std, gyro_spike_prob, gyro_spike_mag);
        let gyro_noise_y = self.generate_noise_value(gyro_std, gyro_spike_prob, gyro_spike_mag);
        let gyro_noise_z = self.generate_noise_value(gyro_std, gyro_spike_prob, gyro_spike_mag);

        // Generate reading
        let accel = Vec3::new(
            self.base_accel.x + motion_accel.x + self.accel_bias.x + accel_noise_x,
            self.base_accel.y + motion_accel.y + self.accel_bias.y + accel_noise_y,
            self.base_accel.z + motion_accel.z + self.accel_bias.z + accel_noise_z,
        );

        let gyro = Vec3::new(
            self.base_gyro.x + self.gyro_bias.x + gyro_noise_x,
            self.base_gyro.y + self.gyro_bias.y + gyro_noise_y,
            self.base_gyro.z + self.gyro_bias.z + gyro_noise_z,
        );

        let reading = ImuReading::new(accel, gyro);
        Ok(self.clock.stamp(reading))
    }

    fn name(&self) -> &str {
        self.name
    }

    fn sample_rate_hz(&self) -> f64 {
        self.sample_rate_hz
    }
}

/// A mock sensor that produces a configurable sequence of values.
///
/// Useful for testing specific scenarios with known data.
#[cfg(feature = "std")]
pub struct SequenceSensor<T> {
    name: &'static str,
    clock: MonotonicClock,
    values: Vec<T>,
    index: usize,
    repeat: bool,
}

#[cfg(feature = "std")]
impl<T: Clone> SequenceSensor<T> {
    /// Creates a new sequence sensor with the given values.
    pub fn new(name: &'static str, values: Vec<T>) -> Self {
        Self {
            name,
            clock: MonotonicClock::new(),
            values,
            index: 0,
            repeat: false,
        }
    }

    /// Configures the sensor to repeat the sequence.
    #[must_use]
    pub fn with_repeat(mut self, repeat: bool) -> Self {
        self.repeat = repeat;
        self
    }

    /// Resets the sequence to the beginning.
    pub fn reset(&mut self) {
        self.index = 0;
        self.clock.reset_seq();
    }
}

#[cfg(feature = "std")]
impl<T: Clone + Send> Sensor for SequenceSensor<T> {
    type Reading = T;

    fn sample(&mut self) -> Result<Timestamped<Self::Reading>> {
        if self.values.is_empty() {
            return Err(crate::error::SensorError::ReadFailed.into());
        }

        let value = self.values[self.index].clone();
        self.index += 1;

        if self.index >= self.values.len() {
            if self.repeat {
                self.index = 0;
            } else {
                self.index = self.values.len() - 1; // Stay at last value
            }
        }

        Ok(self.clock.stamp(value))
    }

    fn name(&self) -> &str {
        self.name
    }

    fn sample_rate_hz(&self) -> f64 {
        0.0 // Event-driven
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn test_simple_rng_deterministic() {
        let mut rng1 = SimpleRng::new(12345);
        let mut rng2 = SimpleRng::new(12345);

        for _ in 0..100 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    #[test]
    fn test_simple_rng_distribution() {
        let mut rng = SimpleRng::new(42);

        // Generate 1000 values and check they're in range
        for _ in 0..1000 {
            let v = rng.next_f32();
            assert!((0.0..1.0).contains(&v));
        }
    }

    #[test]
    fn test_mock_imu_stationary() {
        let mut imu = MockImu::new(1000.0).with_noise(NoiseConfig::none(), NoiseConfig::none());

        let sample = imu.sample().expect("sample failed");

        // Should read approximately gravity
        assert!((sample.data.accel.z - (-9.81)).abs() < 0.01);
        assert!(sample.data.gyro.x.abs() < 0.01);
    }

    #[test]
    fn test_mock_imu_constant_rotation() {
        let mut imu = MockImu::new(1000.0)
            .with_noise(NoiseConfig::none(), NoiseConfig::none())
            .with_base_gyro(Vec3::new(0.0, 0.0, 1.0)); // 1 rad/s around Z

        let sample = imu.sample().expect("sample failed");
        assert!((sample.data.gyro.z - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_mock_imu_sequence_numbers() {
        let mut imu = MockImu::new(1000.0);

        let s1 = imu.sample().expect("sample failed");
        let s2 = imu.sample().expect("sample failed");
        let s3 = imu.sample().expect("sample failed");

        assert_eq!(s1.seq, 0);
        assert_eq!(s2.seq, 1);
        assert_eq!(s3.seq, 2);
    }

    #[test]
    fn test_sequence_sensor() {
        let values = vec![1, 2, 3, 4, 5];
        let mut sensor = SequenceSensor::new("test", values);

        assert_eq!(sensor.sample().unwrap().data, 1);
        assert_eq!(sensor.sample().unwrap().data, 2);
        assert_eq!(sensor.sample().unwrap().data, 3);
        assert_eq!(sensor.sample().unwrap().data, 4);
        assert_eq!(sensor.sample().unwrap().data, 5);
        // Should stay at last value
        assert_eq!(sensor.sample().unwrap().data, 5);
    }

    #[test]
    fn test_sequence_sensor_repeat() {
        let values = vec![1, 2, 3];
        let mut sensor = SequenceSensor::new("test", values).with_repeat(true);

        assert_eq!(sensor.sample().unwrap().data, 1);
        assert_eq!(sensor.sample().unwrap().data, 2);
        assert_eq!(sensor.sample().unwrap().data, 3);
        assert_eq!(sensor.sample().unwrap().data, 1); // Wrapped
        assert_eq!(sensor.sample().unwrap().data, 2);
    }
}
