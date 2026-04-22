//! IMU (Inertial Measurement Unit) sensor types.
//!
//! This module provides data types for IMU sensors, which typically measure:
//! - Linear acceleration (accelerometer)
//! - Angular velocity (gyroscope)
//! - Optionally: magnetic field (magnetometer)

use core::ops::{Add, Mul, Sub};

/// A 3D vector for sensor data.
///
/// Uses `#[repr(C)]` for predictable memory layout, enabling zero-copy
/// operations when interfacing with hardware or FFI.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[repr(C)]
pub struct Vec3 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
    /// Z component.
    pub z: f32,
}

impl Vec3 {
    /// Creates a new 3D vector.
    #[inline]
    #[must_use]
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Creates a zero vector.
    #[inline]
    #[must_use]
    pub const fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0)
    }

    /// Returns the vector as an array.
    #[inline]
    #[must_use]
    pub const fn as_array(&self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }

    /// Creates a vector from an array.
    #[inline]
    #[must_use]
    pub const fn from_array(arr: [f32; 3]) -> Self {
        Self::new(arr[0], arr[1], arr[2])
    }

    /// Computes the squared magnitude of the vector.
    #[inline]
    #[must_use]
    pub fn magnitude_squared(&self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    /// Computes the magnitude (length) of the vector.
    #[inline]
    #[must_use]
    pub fn magnitude(&self) -> f32 {
        crate::math::sqrt_f32(self.magnitude_squared())
    }

    /// Returns a normalized (unit length) version of this vector.
    ///
    /// Returns zero vector if the magnitude is zero.
    #[inline]
    #[must_use]
    pub fn normalized(&self) -> Self {
        let mag = self.magnitude();
        if mag > f32::EPSILON {
            Self::new(self.x / mag, self.y / mag, self.z / mag)
        } else {
            Self::zero()
        }
    }

    /// Computes the dot product with another vector.
    #[inline]
    #[must_use]
    pub fn dot(&self, other: &Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Computes the cross product with another vector.
    #[inline]
    #[must_use]
    pub fn cross(&self, other: &Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }
}

impl Add for Vec3 {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl Sub for Vec3 {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl Mul<f32> for Vec3 {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

impl From<[f32; 3]> for Vec3 {
    fn from(arr: [f32; 3]) -> Self {
        Self::from_array(arr)
    }
}

impl From<Vec3> for [f32; 3] {
    fn from(v: Vec3) -> Self {
        v.as_array()
    }
}

/// A 6-DOF IMU reading containing accelerometer and gyroscope data.
///
/// This is the most common IMU configuration, measuring:
/// - 3-axis linear acceleration (m/s²)
/// - 3-axis angular velocity (rad/s)
///
/// Uses `#[repr(C)]` for predictable memory layout, enabling zero-copy
/// operations and direct memory mapping from hardware buffers.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[repr(C)]
pub struct ImuReading {
    /// Linear acceleration in m/s² (body frame).
    pub accel: Vec3,
    /// Angular velocity in rad/s (body frame).
    pub gyro: Vec3,
}

impl ImuReading {
    /// Creates a new IMU reading.
    #[inline]
    #[must_use]
    pub const fn new(accel: Vec3, gyro: Vec3) -> Self {
        Self { accel, gyro }
    }

    /// Creates a new IMU reading from raw arrays.
    #[inline]
    #[must_use]
    pub const fn from_arrays(accel: [f32; 3], gyro: [f32; 3]) -> Self {
        Self {
            accel: Vec3::from_array(accel),
            gyro: Vec3::from_array(gyro),
        }
    }

    /// Creates a zero reading (no acceleration or rotation).
    #[inline]
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            accel: Vec3::zero(),
            gyro: Vec3::zero(),
        }
    }

    /// Returns the accelerometer data as an array.
    #[inline]
    #[must_use]
    pub const fn accel_array(&self) -> [f32; 3] {
        self.accel.as_array()
    }

    /// Returns the gyroscope data as an array.
    #[inline]
    #[must_use]
    pub const fn gyro_array(&self) -> [f32; 3] {
        self.gyro.as_array()
    }

    /// Computes the total acceleration magnitude.
    #[inline]
    #[must_use]
    pub fn accel_magnitude(&self) -> f32 {
        self.accel.magnitude()
    }

    /// Computes the total angular velocity magnitude.
    #[inline]
    #[must_use]
    pub fn gyro_magnitude(&self) -> f32 {
        self.gyro.magnitude()
    }
}

/// A 9-DOF IMU reading with magnetometer data.
///
/// Extends the 6-DOF reading with:
/// - 3-axis magnetic field (μT or gauss, depending on sensor)
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[repr(C)]
pub struct Imu9DofReading {
    /// Linear acceleration in m/s² (body frame).
    pub accel: Vec3,
    /// Angular velocity in rad/s (body frame).
    pub gyro: Vec3,
    /// Magnetic field strength (sensor-dependent units).
    pub mag: Vec3,
}

impl Imu9DofReading {
    /// Creates a new 9-DOF IMU reading.
    #[inline]
    #[must_use]
    pub const fn new(accel: Vec3, gyro: Vec3, mag: Vec3) -> Self {
        Self { accel, gyro, mag }
    }

    /// Extracts the 6-DOF (accel + gyro) portion.
    #[inline]
    #[must_use]
    pub const fn to_6dof(&self) -> ImuReading {
        ImuReading {
            accel: self.accel,
            gyro: self.gyro,
        }
    }
}

impl From<Imu9DofReading> for ImuReading {
    fn from(reading: Imu9DofReading) -> Self {
        reading.to_6dof()
    }
}

/// Calibration data for an IMU sensor.
///
/// Contains bias and scale factors for calibrating raw sensor readings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImuCalibration {
    /// Accelerometer bias (offset to subtract).
    pub accel_bias: Vec3,
    /// Accelerometer scale factors.
    pub accel_scale: Vec3,
    /// Gyroscope bias (offset to subtract).
    pub gyro_bias: Vec3,
    /// Gyroscope scale factors.
    pub gyro_scale: Vec3,
}

impl Default for ImuCalibration {
    fn default() -> Self {
        Self {
            accel_bias: Vec3::zero(),
            accel_scale: Vec3::new(1.0, 1.0, 1.0),
            gyro_bias: Vec3::zero(),
            gyro_scale: Vec3::new(1.0, 1.0, 1.0),
        }
    }
}

impl ImuCalibration {
    /// Applies calibration to a raw IMU reading.
    #[must_use]
    pub fn apply(&self, raw: &ImuReading) -> ImuReading {
        ImuReading {
            accel: Vec3::new(
                (raw.accel.x - self.accel_bias.x) * self.accel_scale.x,
                (raw.accel.y - self.accel_bias.y) * self.accel_scale.y,
                (raw.accel.z - self.accel_bias.z) * self.accel_scale.z,
            ),
            gyro: Vec3::new(
                (raw.gyro.x - self.gyro_bias.x) * self.gyro_scale.x,
                (raw.gyro.y - self.gyro_bias.y) * self.gyro_scale.y,
                (raw.gyro.z - self.gyro_bias.z) * self.gyro_scale.z,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vec3_operations() {
        let v1 = Vec3::new(1.0, 2.0, 3.0);
        let v2 = Vec3::new(4.0, 5.0, 6.0);

        // Addition
        let sum = v1 + v2;
        assert_eq!(sum, Vec3::new(5.0, 7.0, 9.0));

        // Subtraction
        let diff = v2 - v1;
        assert_eq!(diff, Vec3::new(3.0, 3.0, 3.0));

        // Scalar multiplication
        let scaled = v1 * 2.0;
        assert_eq!(scaled, Vec3::new(2.0, 4.0, 6.0));

        // Dot product
        let dot = v1.dot(&v2);
        assert!((dot - 32.0).abs() < f32::EPSILON);

        // Magnitude
        let v = Vec3::new(3.0, 4.0, 0.0);
        assert!((v.magnitude() - 5.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_vec3_cross_product() {
        let x = Vec3::new(1.0, 0.0, 0.0);
        let y = Vec3::new(0.0, 1.0, 0.0);
        let z = x.cross(&y);
        assert!((z.x).abs() < f32::EPSILON);
        assert!((z.y).abs() < f32::EPSILON);
        assert!((z.z - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_imu_reading() {
        let reading = ImuReading::from_arrays([1.0, 2.0, 3.0], [0.1, 0.2, 0.3]);
        assert_eq!(reading.accel, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(reading.gyro, Vec3::new(0.1, 0.2, 0.3));
    }

    #[test]
    fn test_imu_calibration() {
        let raw = ImuReading::from_arrays([1.0, 2.0, 3.0], [0.1, 0.2, 0.3]);
        let cal = ImuCalibration {
            accel_bias: Vec3::new(0.1, 0.1, 0.1),
            accel_scale: Vec3::new(1.0, 1.0, 1.0),
            gyro_bias: Vec3::new(0.01, 0.01, 0.01),
            gyro_scale: Vec3::new(1.0, 1.0, 1.0),
        };

        let calibrated = cal.apply(&raw);
        assert!((calibrated.accel.x - 0.9).abs() < f32::EPSILON);
        assert!((calibrated.gyro.x - 0.09).abs() < f32::EPSILON);
    }

    #[test]
    fn test_repr_c_layout() {
        // Verify that our structs have predictable sizes for zero-copy
        assert_eq!(core::mem::size_of::<Vec3>(), 12); // 3 * f32
        assert_eq!(core::mem::size_of::<ImuReading>(), 24); // 2 * Vec3
        assert_eq!(core::mem::size_of::<Imu9DofReading>(), 36); // 3 * Vec3
    }
}
