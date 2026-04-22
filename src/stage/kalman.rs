//! Constant-velocity Kalman filter for 1-D state tracking.
//!
//! This is a classic 2-state linear Kalman filter tracking `[position,
//! velocity]` along a single axis, with scalar position measurements. It is
//! the right size of Kalman for sensor-bridge: small enough to be readable
//! without a linear-algebra dependency, large enough to demonstrate the
//! full predict / update cycle, and general enough that a caller can stack
//! three instances to track 3-D position from three independent scalar
//! measurements.
//!
//! For a full 6-DoF INS (3-D position + 3-D velocity from accelerometer
//! integration), enable the `dsp` feature in a future release and bring in
//! `nalgebra` — at 6 states the hand-rolled matrix algebra starts to cost
//! more than it saves. See `docs/ROADMAP.md`.

use crate::math;

use super::Stage;

/// Tunable parameters for [`KalmanFilter2D`].
///
/// `process_noise` scales the acceleration uncertainty you want to
/// tolerate before the filter re-weights toward measurements; a small
/// value produces smooth, high-inertia estimates. `measurement_noise` is
/// the expected variance of the sensor reading itself.
#[derive(Debug, Clone, Copy)]
pub struct KalmanConfig {
    /// Acceleration variance used to grow `P` during the predict step.
    pub process_noise: f32,
    /// Variance of the scalar position measurement, used in the Kalman gain.
    pub measurement_noise: f32,
    /// Initial position estimate.
    pub initial_position: f32,
    /// Initial velocity estimate.
    pub initial_velocity: f32,
}

impl Default for KalmanConfig {
    fn default() -> Self {
        Self {
            process_noise: 1.0,
            measurement_noise: 0.25,
            initial_position: 0.0,
            initial_velocity: 0.0,
        }
    }
}

/// Single position + velocity measurement into / out of the filter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KalmanMeasurement {
    /// Observed position.
    pub position: f32,
    /// Elapsed time since the previous update.
    pub dt_s: f32,
}

/// Posterior estimate produced by one predict/update cycle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KalmanEstimate {
    /// Position posterior.
    pub position: f32,
    /// Velocity posterior.
    pub velocity: f32,
    /// `sqrt(P[0,0])` — standard deviation of the position estimate.
    pub position_stddev: f32,
}

/// Two-state constant-velocity Kalman filter.
///
/// State vector `x = [position, velocity]` with transition matrix
/// `F = [[1, dt], [0, 1]]`, control-free, scalar measurement
/// `z = position`. Covariances are stored as their three independent
/// components to avoid an external linear-algebra dependency.
///
/// # Example
///
/// ```rust
/// use sensor_bridge::stage::{KalmanConfig, KalmanFilter2D, KalmanMeasurement, Stage};
///
/// let mut kf = KalmanFilter2D::new(KalmanConfig::default());
/// let est = kf
///     .process(KalmanMeasurement { position: 1.0, dt_s: 0.1 })
///     .unwrap();
/// assert!(est.position_stddev > 0.0);
/// ```
#[derive(Debug, Clone)]
pub struct KalmanFilter2D {
    position: f32,
    velocity: f32,
    // Covariance P is symmetric: [[p00, p01], [p01, p11]]
    p00: f32,
    p01: f32,
    p11: f32,
    process_noise: f32,
    measurement_noise: f32,
}

impl KalmanFilter2D {
    /// Creates a filter seeded with `config`. Initial covariance is
    /// identity scaled by `config.measurement_noise`, a reasonable
    /// "don't know much yet" starting belief.
    #[must_use]
    pub fn new(config: KalmanConfig) -> Self {
        Self {
            position: config.initial_position,
            velocity: config.initial_velocity,
            p00: config.measurement_noise,
            p01: 0.0,
            p11: config.measurement_noise,
            process_noise: config.process_noise,
            measurement_noise: config.measurement_noise,
        }
    }

    /// Returns the current position estimate.
    #[must_use]
    pub const fn position(&self) -> f32 {
        self.position
    }

    /// Returns the current velocity estimate.
    #[must_use]
    pub const fn velocity(&self) -> f32 {
        self.velocity
    }
}

impl Stage for KalmanFilter2D {
    type Input = KalmanMeasurement;
    type Output = KalmanEstimate;

    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        let dt = input.dt_s;
        // Predict step.
        //   x' = F x  where F = [[1, dt], [0, 1]]
        //   P' = F P Fᵀ + Q
        // For a constant-velocity model, the canonical Q from an
        // unmodeled acceleration variance q is:
        //   Q = q * [[dt^4/4, dt^3/2], [dt^3/2, dt^2]]
        self.position += self.velocity * dt;

        let p00_pred = self.p00 + 2.0 * dt * self.p01 + dt * dt * self.p11;
        let p01_pred = self.p01 + dt * self.p11;
        let p11_pred = self.p11;

        let dt2 = dt * dt;
        let dt3 = dt2 * dt;
        let dt4 = dt2 * dt2;
        let q = self.process_noise;
        let p00 = p00_pred + q * dt4 / 4.0;
        let p01 = p01_pred + q * dt3 / 2.0;
        let p11 = p11_pred + q * dt2;

        // Update step with scalar measurement z = position.
        //   y = z - H x     H = [1, 0]
        //   S = H P Hᵀ + R = P[0,0] + R
        //   K = P Hᵀ / S
        //   x = x + K y
        //   P = (I - K H) P
        let y = input.position - self.position;
        let s = p00 + self.measurement_noise;
        let k0 = p00 / s;
        let k1 = p01 / s;

        self.position += k0 * y;
        self.velocity += k1 * y;

        self.p00 = (1.0 - k0) * p00;
        self.p01 = (1.0 - k0) * p01;
        self.p11 = p11 - k1 * p01;

        Some(KalmanEstimate {
            position: self.position,
            velocity: self.velocity,
            position_stddev: math::sqrt_f32(self.p00.max(0.0)),
        })
    }

    fn reset(&mut self) {
        self.position = 0.0;
        self.velocity = 0.0;
        self.p00 = self.measurement_noise;
        self.p01 = 0.0;
        self.p11 = self.measurement_noise;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_constant_position_converges_to_measurement() {
        let mut kf = KalmanFilter2D::new(KalmanConfig {
            process_noise: 0.01,
            measurement_noise: 0.1,
            ..Default::default()
        });
        let mut last = 0.0;
        for _ in 0..200 {
            let est = kf
                .process(KalmanMeasurement {
                    position: 5.0,
                    dt_s: 0.01,
                })
                .unwrap();
            last = est.position;
        }
        assert!((last - 5.0).abs() < 0.05, "got {last}");
        // Velocity should converge toward zero.
        assert!(kf.velocity().abs() < 0.5);
    }

    #[test]
    fn tracks_constant_velocity_estimates_velocity() {
        let mut kf = KalmanFilter2D::new(KalmanConfig {
            process_noise: 0.01,
            measurement_noise: 0.01,
            ..Default::default()
        });
        let dt = 0.01f32;
        let v = 2.0f32;
        let mut t = 0.0f32;
        for _ in 0..500 {
            t += dt;
            kf.process(KalmanMeasurement {
                position: v * t,
                dt_s: dt,
            });
        }
        assert!((kf.velocity() - v).abs() < 0.05, "got {}", kf.velocity());
    }

    #[test]
    fn covariance_stays_finite_and_nonneg() {
        let mut kf = KalmanFilter2D::new(KalmanConfig::default());
        for i in 0..1000 {
            let est = kf
                .process(KalmanMeasurement {
                    position: (i as f32) * 0.01,
                    dt_s: 0.01,
                })
                .unwrap();
            assert!(est.position_stddev.is_finite());
            assert!(est.position_stddev >= 0.0);
        }
    }
}
