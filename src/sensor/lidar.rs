//! LIDAR sensor types.
//!
//! This module provides data types for LIDAR (Light Detection and Ranging) sensors,
//! which measure distance using laser pulses.

/// A single LIDAR point in 2D polar coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[repr(C)]
pub struct LidarPoint2D {
    /// Distance to the detected point in meters.
    pub range: f32,
    /// Angle in radians from the sensor's forward direction.
    pub angle: f32,
    /// Intensity of the return signal (0.0-1.0 normalized).
    pub intensity: f32,
}

impl LidarPoint2D {
    /// Creates a new 2D LIDAR point.
    #[inline]
    #[must_use]
    pub const fn new(range: f32, angle: f32, intensity: f32) -> Self {
        Self {
            range,
            angle,
            intensity,
        }
    }

    /// Converts to Cartesian coordinates (x, y).
    #[inline]
    #[must_use]
    pub fn to_cartesian(&self) -> (f32, f32) {
        let (sin, cos) = self.angle.sin_cos();
        (self.range * cos, self.range * sin)
    }

    /// Returns true if this is a valid measurement (finite range > 0).
    #[inline]
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.range.is_finite() && self.range > 0.0
    }
}

/// A single LIDAR point in 3D spherical coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[repr(C)]
pub struct LidarPoint3D {
    /// Distance to the detected point in meters.
    pub range: f32,
    /// Azimuth angle in radians (horizontal rotation).
    pub azimuth: f32,
    /// Elevation angle in radians (vertical tilt).
    pub elevation: f32,
    /// Intensity of the return signal (0.0-1.0 normalized).
    pub intensity: f32,
}

impl LidarPoint3D {
    /// Creates a new 3D LIDAR point.
    #[inline]
    #[must_use]
    pub const fn new(range: f32, azimuth: f32, elevation: f32, intensity: f32) -> Self {
        Self {
            range,
            azimuth,
            elevation,
            intensity,
        }
    }

    /// Converts to Cartesian coordinates (x, y, z).
    #[inline]
    #[must_use]
    pub fn to_cartesian(&self) -> (f32, f32, f32) {
        let (sin_az, cos_az) = self.azimuth.sin_cos();
        let (sin_el, cos_el) = self.elevation.sin_cos();
        let r_cos_el = self.range * cos_el;
        (r_cos_el * cos_az, r_cos_el * sin_az, self.range * sin_el)
    }

    /// Returns true if this is a valid measurement.
    #[inline]
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.range.is_finite() && self.range > 0.0
    }
}

/// A complete 2D LIDAR scan (360° or partial).
///
/// This type uses a boxed slice for the points, which is allocated once
/// and reused. This keeps allocations out of the hot path after initialization.
#[cfg(feature = "std")]
#[derive(Debug, Clone)]
pub struct LidarScan2D {
    /// The scan points.
    pub points: Box<[LidarPoint2D]>,
    /// Number of valid points in the scan.
    pub valid_count: usize,
    /// Minimum angle in radians.
    pub angle_min: f32,
    /// Maximum angle in radians.
    pub angle_max: f32,
    /// Angular increment between points in radians.
    pub angle_increment: f32,
    /// Minimum valid range in meters.
    pub range_min: f32,
    /// Maximum valid range in meters.
    pub range_max: f32,
}

#[cfg(feature = "std")]
impl LidarScan2D {
    /// Creates a new LIDAR scan with pre-allocated storage.
    #[must_use]
    pub fn with_capacity(
        num_points: usize,
        angle_min: f32,
        angle_max: f32,
        range_min: f32,
        range_max: f32,
    ) -> Self {
        let angle_increment = if num_points > 1 {
            (angle_max - angle_min) / (num_points - 1) as f32
        } else {
            0.0
        };

        Self {
            points: vec![LidarPoint2D::default(); num_points].into_boxed_slice(),
            valid_count: 0,
            angle_min,
            angle_max,
            angle_increment,
            range_min,
            range_max,
        }
    }

    /// Creates a standard 360° scan with the given number of points.
    #[must_use]
    pub fn full_rotation(num_points: usize, range_max: f32) -> Self {
        Self::with_capacity(
            num_points,
            0.0,
            core::f32::consts::TAU, // 2π
            0.05,                   // 5cm minimum
            range_max,
        )
    }

    /// Returns an iterator over valid points only.
    pub fn valid_points(&self) -> impl Iterator<Item = &LidarPoint2D> {
        self.points.iter().filter(|p| p.is_valid())
    }

    /// Counts the number of valid points in the scan.
    #[must_use]
    pub fn count_valid(&self) -> usize {
        self.points.iter().filter(|p| p.is_valid()).count()
    }
}

/// A simplified LIDAR scan using fixed-size arrays.
///
/// This version is `no_std` compatible and has no allocations,
/// but is limited to a fixed number of points.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct LidarScanFixed<const N: usize> {
    /// Range values in meters.
    pub ranges: [f32; N],
    /// Intensity values (0.0-1.0).
    pub intensities: [f32; N],
    /// Minimum angle in radians.
    pub angle_min: f32,
    /// Angular increment in radians.
    pub angle_increment: f32,
}

impl<const N: usize> LidarScanFixed<N> {
    /// Creates a new fixed-size scan with zeros.
    #[must_use]
    pub const fn new(angle_min: f32, angle_increment: f32) -> Self {
        Self {
            ranges: [0.0; N],
            intensities: [0.0; N],
            angle_min,
            angle_increment,
        }
    }

    /// Returns the angle for a given index.
    #[inline]
    #[must_use]
    pub fn angle_at(&self, index: usize) -> f32 {
        self.angle_min + (index as f32) * self.angle_increment
    }

    /// Returns the point at the given index.
    #[must_use]
    pub fn point_at(&self, index: usize) -> LidarPoint2D {
        LidarPoint2D {
            range: self.ranges[index],
            angle: self.angle_at(index),
            intensity: self.intensities[index],
        }
    }
}

impl<const N: usize> Default for LidarScanFixed<N> {
    fn default() -> Self {
        Self::new(0.0, core::f32::consts::TAU / N as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lidar_point_2d_cartesian() {
        // Point straight ahead
        let p = LidarPoint2D::new(1.0, 0.0, 1.0);
        let (x, y) = p.to_cartesian();
        assert!((x - 1.0).abs() < f32::EPSILON);
        assert!(y.abs() < f32::EPSILON);

        // Point to the left (90°)
        let p = LidarPoint2D::new(1.0, core::f32::consts::FRAC_PI_2, 1.0);
        let (x, y) = p.to_cartesian();
        assert!(x.abs() < 1e-6);
        assert!((y - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_lidar_point_validity() {
        assert!(LidarPoint2D::new(1.0, 0.0, 1.0).is_valid());
        assert!(!LidarPoint2D::new(0.0, 0.0, 1.0).is_valid());
        assert!(!LidarPoint2D::new(-1.0, 0.0, 1.0).is_valid());
        assert!(!LidarPoint2D::new(f32::INFINITY, 0.0, 1.0).is_valid());
        assert!(!LidarPoint2D::new(f32::NAN, 0.0, 1.0).is_valid());
    }

    #[test]
    fn test_fixed_scan() {
        let scan: LidarScanFixed<360> = LidarScanFixed::default();
        assert_eq!(scan.ranges.len(), 360);

        // Check angle calculation
        let angle_inc = core::f32::consts::TAU / 360.0;
        assert!((scan.angle_increment - angle_inc).abs() < f32::EPSILON);
        assert!((scan.angle_at(180) - core::f32::consts::PI).abs() < 1e-5);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_lidar_scan_2d() {
        let scan = LidarScan2D::full_rotation(360, 30.0);
        assert_eq!(scan.points.len(), 360);
        assert_eq!(scan.range_max, 30.0);
    }
}
