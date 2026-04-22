//! Complementary filter for sensor fusion.
//!
//! A complementary filter blends a noisy low-frequency reference signal
//! (typically accelerometer-derived tilt) with an integrated high-frequency
//! signal (typically gyro rate). The classic form is:
//!
//! ```text
//! theta_k = alpha * (theta_{k-1} + gyro_rate * dt) + (1 - alpha) * accel_angle
//! ```
//!
//! With `alpha ≈ 0.98`, the filter trusts the gyro for fast motion but
//! corrects the drift using the accelerometer when the platform is near
//! steady state. It is O(1) per sample and has no tuning matrices to fit,
//! which is why it is still the default IMU fusion choice on flight
//! controllers and lightweight robotics stacks.

use super::Stage;

/// Per-sample input for [`ComplementaryFilter`].
///
/// `accel_angle_rad` is the tilt inferred from gravity (e.g. `atan2` over
/// two accelerometer axes). `gyro_rate_rad_s` is the body-frame angular
/// velocity about the same axis. `dt_s` is the elapsed time since the
/// previous sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComplementaryInput {
    /// Accelerometer-derived tilt in radians.
    pub accel_angle_rad: f32,
    /// Gyroscope rate in rad/s.
    pub gyro_rate_rad_s: f32,
    /// Sample period in seconds.
    pub dt_s: f32,
}

/// Scalar complementary filter fusing a gyro rate with an accel-derived
/// angle.
///
/// Use one instance per axis. For a 3D IMU the typical setup is: one
/// filter on pitch (rotation about body-Y) and one on roll
/// (rotation about body-X). Yaw is intentionally excluded — gravity does
/// not observe it, so a complementary filter cannot bound yaw drift;
/// that needs a magnetometer.
///
/// # Example
///
/// ```rust
/// use sensor_bridge::stage::{ComplementaryFilter, ComplementaryInput, Stage};
///
/// let mut pitch = ComplementaryFilter::new(0.98);
/// let input = ComplementaryInput {
///     accel_angle_rad: 0.10,
///     gyro_rate_rad_s: 0.02,
///     dt_s: 0.01,
/// };
/// let estimate = pitch.process(input).unwrap();
/// assert!(estimate.abs() < 0.2);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct ComplementaryFilter {
    alpha: f32,
    angle_rad: f32,
    initialized: bool,
}

impl ComplementaryFilter {
    /// Creates a filter with the given blend factor `alpha`.
    ///
    /// `alpha` should lie in `(0, 1)`; `0.98` is a safe default for IMU
    /// use at 100 Hz. Higher values trust the gyro more and reject more
    /// accelerometer noise; lower values converge to the accelerometer
    /// estimate faster.
    ///
    /// # Panics
    ///
    /// Panics if `alpha` is not in `(0, 1)`.
    #[must_use]
    pub fn new(alpha: f32) -> Self {
        assert!(
            alpha > 0.0 && alpha < 1.0,
            "alpha must be in (0, 1), got {alpha}"
        );
        Self {
            alpha,
            angle_rad: 0.0,
            initialized: false,
        }
    }

    /// Returns the blend factor.
    #[must_use]
    pub const fn alpha(&self) -> f32 {
        self.alpha
    }

    /// Returns the most recently produced angle estimate in radians.
    #[must_use]
    pub const fn angle_rad(&self) -> f32 {
        self.angle_rad
    }
}

impl Default for ComplementaryFilter {
    fn default() -> Self {
        Self::new(0.98)
    }
}

impl Stage for ComplementaryFilter {
    type Input = ComplementaryInput;
    type Output = f32;

    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        if !self.initialized {
            self.angle_rad = input.accel_angle_rad;
            self.initialized = true;
        } else {
            let gyro_integrated = self.angle_rad + input.gyro_rate_rad_s * input.dt_s;
            self.angle_rad =
                self.alpha * gyro_integrated + (1.0 - self.alpha) * input.accel_angle_rad;
        }
        Some(self.angle_rad)
    }

    fn reset(&mut self) {
        self.angle_rad = 0.0;
        self.initialized = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sample_seeds_with_accel() {
        let mut f = ComplementaryFilter::new(0.98);
        let out = f
            .process(ComplementaryInput {
                accel_angle_rad: 0.5,
                gyro_rate_rad_s: 10.0,
                dt_s: 0.01,
            })
            .unwrap();
        assert_eq!(out, 0.5);
    }

    #[test]
    fn high_alpha_trusts_gyro_in_short_term() {
        // With alpha = 0.99 and the accel reading held steady at 0, a
        // constant 1 rad/s gyro should integrate roughly linearly over
        // a short horizon.
        let mut f = ComplementaryFilter::new(0.99);
        f.process(ComplementaryInput {
            accel_angle_rad: 0.0,
            gyro_rate_rad_s: 0.0,
            dt_s: 0.01,
        });
        for _ in 0..10 {
            f.process(ComplementaryInput {
                accel_angle_rad: 0.0,
                gyro_rate_rad_s: 1.0,
                dt_s: 0.01,
            });
        }
        // 10 steps of 0.01s @ 1 rad/s ≈ 0.1 rad, attenuated by the
        // repeated (1 - alpha) pull back toward 0.
        let v = f.angle_rad();
        assert!(v > 0.07 && v < 0.1, "got {v}");
    }

    #[test]
    fn low_alpha_converges_to_accel() {
        // With alpha = 0.01 the filter is basically the accel input.
        let mut f = ComplementaryFilter::new(0.01);
        f.process(ComplementaryInput {
            accel_angle_rad: 0.5,
            gyro_rate_rad_s: 0.0,
            dt_s: 0.01,
        });
        let out = f
            .process(ComplementaryInput {
                accel_angle_rad: 0.3,
                gyro_rate_rad_s: 0.0,
                dt_s: 0.01,
            })
            .unwrap();
        assert!((out - 0.3).abs() < 0.01, "got {out}");
    }
}
