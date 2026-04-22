//! Coordinate transform and unit conversion stages.
//!
//! This module provides stages for transforming sensor data between
//! coordinate frames and converting between units.

use crate::sensor::{ImuReading, Vec3};

use super::Stage;

/// A 3x3 rotation matrix for coordinate transforms.
///
/// Uses row-major ordering: `matrix[row][col]`
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct RotationMatrix {
    /// The 3x3 matrix data in row-major order.
    pub data: [[f32; 3]; 3],
}

impl RotationMatrix {
    /// Creates a new rotation matrix from row-major data.
    #[must_use]
    pub const fn new(data: [[f32; 3]; 3]) -> Self {
        Self { data }
    }

    /// Creates an identity matrix (no rotation).
    #[must_use]
    pub const fn identity() -> Self {
        Self {
            data: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }

    /// Creates a rotation matrix for rotation around the X axis.
    #[must_use]
    pub fn rotation_x(angle_rad: f32) -> Self {
        let (sin, cos) = crate::math::sin_cos_f32(angle_rad);
        Self {
            data: [[1.0, 0.0, 0.0], [0.0, cos, -sin], [0.0, sin, cos]],
        }
    }

    /// Creates a rotation matrix for rotation around the Y axis.
    #[must_use]
    pub fn rotation_y(angle_rad: f32) -> Self {
        let (sin, cos) = crate::math::sin_cos_f32(angle_rad);
        Self {
            data: [[cos, 0.0, sin], [0.0, 1.0, 0.0], [-sin, 0.0, cos]],
        }
    }

    /// Creates a rotation matrix for rotation around the Z axis.
    #[must_use]
    pub fn rotation_z(angle_rad: f32) -> Self {
        let (sin, cos) = crate::math::sin_cos_f32(angle_rad);
        Self {
            data: [[cos, -sin, 0.0], [sin, cos, 0.0], [0.0, 0.0, 1.0]],
        }
    }

    /// Creates a rotation matrix from Euler angles (ZYX order).
    ///
    /// This is the most common convention for robotics (yaw-pitch-roll).
    #[must_use]
    pub fn from_euler_zyx(yaw: f32, pitch: f32, roll: f32) -> Self {
        let rz = Self::rotation_z(yaw);
        let ry = Self::rotation_y(pitch);
        let rx = Self::rotation_x(roll);

        // R = Rz * Ry * Rx
        rz.multiply(&ry).multiply(&rx)
    }

    /// Multiplies this matrix by another.
    #[must_use]
    #[allow(clippy::needless_range_loop)]
    pub fn multiply(&self, other: &Self) -> Self {
        let mut result = [[0.0f32; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                for k in 0..3 {
                    result[i][j] += self.data[i][k] * other.data[k][j];
                }
            }
        }
        Self { data: result }
    }

    /// Returns the transpose of this matrix.
    ///
    /// For rotation matrices, the transpose equals the inverse.
    #[must_use]
    pub fn transpose(&self) -> Self {
        Self {
            data: [
                [self.data[0][0], self.data[1][0], self.data[2][0]],
                [self.data[0][1], self.data[1][1], self.data[2][1]],
                [self.data[0][2], self.data[1][2], self.data[2][2]],
            ],
        }
    }

    /// Transforms a vector by this rotation matrix.
    #[must_use]
    pub fn transform(&self, v: Vec3) -> Vec3 {
        Vec3::new(
            self.data[0][0] * v.x + self.data[0][1] * v.y + self.data[0][2] * v.z,
            self.data[1][0] * v.x + self.data[1][1] * v.y + self.data[1][2] * v.z,
            self.data[2][0] * v.x + self.data[2][1] * v.y + self.data[2][2] * v.z,
        )
    }
}

impl Default for RotationMatrix {
    fn default() -> Self {
        Self::identity()
    }
}

/// A stage that transforms 3D vectors using a rotation matrix.
#[derive(Debug, Clone)]
pub struct VectorTransform {
    rotation: RotationMatrix,
}

impl VectorTransform {
    /// Creates a new vector transform stage.
    #[must_use]
    pub const fn new(rotation: RotationMatrix) -> Self {
        Self { rotation }
    }

    /// Creates an identity transform (no change).
    #[must_use]
    pub const fn identity() -> Self {
        Self::new(RotationMatrix::identity())
    }

    /// Creates a transform from Euler angles.
    #[must_use]
    pub fn from_euler_zyx(yaw: f32, pitch: f32, roll: f32) -> Self {
        Self::new(RotationMatrix::from_euler_zyx(yaw, pitch, roll))
    }

    /// Sets the rotation matrix.
    pub fn set_rotation(&mut self, rotation: RotationMatrix) {
        self.rotation = rotation;
    }
}

impl Stage for VectorTransform {
    type Input = Vec3;
    type Output = Vec3;

    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        Some(self.rotation.transform(input))
    }

    fn reset(&mut self) {}
}

/// A stage that transforms IMU readings to a different coordinate frame.
///
/// Applies the same rotation to both acceleration and angular velocity.
#[derive(Debug, Clone)]
pub struct ImuTransform {
    rotation: RotationMatrix,
}

impl ImuTransform {
    /// Creates a new IMU transform stage.
    #[must_use]
    pub const fn new(rotation: RotationMatrix) -> Self {
        Self { rotation }
    }

    /// Creates an identity transform.
    #[must_use]
    pub const fn identity() -> Self {
        Self::new(RotationMatrix::identity())
    }

    /// Creates a transform from Euler angles.
    #[must_use]
    pub fn from_euler_zyx(yaw: f32, pitch: f32, roll: f32) -> Self {
        Self::new(RotationMatrix::from_euler_zyx(yaw, pitch, roll))
    }

    /// Common transform: NED (North-East-Down) to ENU (East-North-Up).
    #[must_use]
    pub fn ned_to_enu() -> Self {
        // NED to ENU: swap X/Y and negate Z
        Self::new(RotationMatrix::new([
            [0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, -1.0],
        ]))
    }

    /// Common transform: ENU to NED.
    #[must_use]
    pub fn enu_to_ned() -> Self {
        // Same operation, it's its own inverse
        Self::ned_to_enu()
    }
}

impl Stage for ImuTransform {
    type Input = ImuReading;
    type Output = ImuReading;

    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        Some(ImuReading::new(
            self.rotation.transform(input.accel),
            self.rotation.transform(input.gyro),
        ))
    }

    fn reset(&mut self) {}
}

/// Unit conversion factors for common sensor units.
pub mod units {
    /// Degrees to radians.
    pub const DEG_TO_RAD: f32 = core::f32::consts::PI / 180.0;
    /// Radians to degrees.
    pub const RAD_TO_DEG: f32 = 180.0 / core::f32::consts::PI;
    /// G (standard gravity) to m/s².
    pub const G_TO_MS2: f32 = 9.80665;
    /// m/s² to G.
    pub const MS2_TO_G: f32 = 1.0 / 9.80665;
    /// Milligauss to Tesla.
    pub const MILLIGAUSS_TO_TESLA: f32 = 1e-7;
    /// Tesla to milligauss.
    pub const TESLA_TO_MILLIGAUSS: f32 = 1e7;
}

/// A stage that scales a scalar value by a constant factor.
#[derive(Debug, Clone)]
pub struct Scale<T> {
    factor: f32,
    _phantom: core::marker::PhantomData<T>,
}

impl<T> Scale<T> {
    /// Creates a new scaling stage.
    #[must_use]
    pub const fn new(factor: f32) -> Self {
        Self {
            factor,
            _phantom: core::marker::PhantomData,
        }
    }
}

impl<T: core::ops::Mul<f32, Output = T> + Send> Stage for Scale<T> {
    type Input = T;
    type Output = T;

    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        Some(input * self.factor)
    }

    fn reset(&mut self) {}
}

/// A stage that applies bias correction (subtracts a constant offset).
#[derive(Debug, Clone)]
pub struct BiasCorrection<T> {
    bias: T,
}

impl<T: Clone> BiasCorrection<T> {
    /// Creates a new bias correction stage.
    pub fn new(bias: T) -> Self {
        Self { bias }
    }

    /// Updates the bias value.
    pub fn set_bias(&mut self, bias: T) {
        self.bias = bias;
    }
}

impl<T: core::ops::Sub<Output = T> + Clone + Send> Stage for BiasCorrection<T> {
    type Input = T;
    type Output = T;

    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        Some(input - self.bias.clone())
    }

    fn reset(&mut self) {}
}

/// A stage that converts IMU readings from raw units to standard units.
///
/// Handles common unit conversions:
/// - Acceleration: G to m/s²
/// - Angular velocity: deg/s to rad/s
#[derive(Debug, Clone)]
pub struct ImuUnitConversion {
    accel_scale: f32,
    gyro_scale: f32,
}

impl ImuUnitConversion {
    /// Creates a new IMU unit conversion stage.
    #[must_use]
    pub const fn new(accel_scale: f32, gyro_scale: f32) -> Self {
        Self {
            accel_scale,
            gyro_scale,
        }
    }

    /// Creates a conversion from G and deg/s to m/s² and rad/s.
    #[must_use]
    pub fn g_degs_to_si() -> Self {
        Self::new(units::G_TO_MS2, units::DEG_TO_RAD)
    }

    /// Creates a conversion from m/s² and rad/s to G and deg/s.
    #[must_use]
    pub fn si_to_g_degs() -> Self {
        Self::new(units::MS2_TO_G, units::RAD_TO_DEG)
    }
}

impl Stage for ImuUnitConversion {
    type Input = ImuReading;
    type Output = ImuReading;

    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        Some(ImuReading::new(
            input.accel * self.accel_scale,
            input.gyro * self.gyro_scale,
        ))
    }

    fn reset(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f32::consts::FRAC_PI_2;

    #[test]
    fn test_rotation_identity() {
        let r = RotationMatrix::identity();
        let v = Vec3::new(1.0, 2.0, 3.0);
        let result = r.transform(v);
        assert!((result.x - v.x).abs() < 1e-6);
        assert!((result.y - v.y).abs() < 1e-6);
        assert!((result.z - v.z).abs() < 1e-6);
    }

    #[test]
    fn test_rotation_z_90_degrees() {
        let r = RotationMatrix::rotation_z(FRAC_PI_2);
        let v = Vec3::new(1.0, 0.0, 0.0);
        let result = r.transform(v);

        // X axis should become Y axis
        assert!(result.x.abs() < 1e-6);
        assert!((result.y - 1.0).abs() < 1e-6);
        assert!(result.z.abs() < 1e-6);
    }

    #[test]
    fn test_rotation_transpose_is_inverse() {
        let r = RotationMatrix::from_euler_zyx(0.5, 0.3, 0.1);
        let rt = r.transpose();
        let product = r.multiply(&rt);

        // Should be close to identity
        for i in 0..3 {
            for j in 0..3 {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!((product.data[i][j] - expected).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn test_vector_transform_stage() {
        let mut stage = VectorTransform::from_euler_zyx(0.0, 0.0, FRAC_PI_2);
        let result = stage.process(Vec3::new(1.0, 0.0, 0.0)).unwrap();

        // 90° roll: X stays, Y becomes -Z, Z becomes Y
        assert!((result.x - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_imu_transform_stage() {
        let mut stage = ImuTransform::ned_to_enu();
        let reading = ImuReading::from_arrays([1.0, 2.0, 3.0], [0.1, 0.2, 0.3]);
        let result = stage.process(reading).unwrap();

        // NED to ENU swaps X/Y and negates Z
        assert!((result.accel.x - 2.0).abs() < 1e-6);
        assert!((result.accel.y - 1.0).abs() < 1e-6);
        assert!((result.accel.z - (-3.0)).abs() < 1e-6);
    }

    #[test]
    fn test_scale_stage() {
        let mut stage: Scale<f32> = Scale::new(2.0);
        assert_eq!(stage.process(5.0), Some(10.0));
    }

    #[test]
    fn test_imu_unit_conversion() {
        let mut stage = ImuUnitConversion::g_degs_to_si();

        let reading = ImuReading::from_arrays([1.0, 0.0, 0.0], [180.0, 0.0, 0.0]);
        let result = stage.process(reading).unwrap();

        // 1G ≈ 9.8 m/s²
        assert!((result.accel.x - units::G_TO_MS2).abs() < 0.01);
        // 180 deg/s ≈ π rad/s
        assert!((result.gyro.x - core::f32::consts::PI).abs() < 0.01);
    }
}
