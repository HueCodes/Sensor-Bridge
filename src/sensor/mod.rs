//! Sensor abstractions and implementations.
//!
//! This module provides:
//! - [`Sensor`] trait: The core interface for all sensor types
//! - [`ImuReading`], [`Vec3`]: Types for IMU data
//! - [`LidarPoint2D`], [`LidarScan2D`]: Types for LIDAR data
//! - Mock implementations for testing
//!
//! # Implementing a Sensor
//!
//! To create a new sensor, implement the [`Sensor`] trait:
//!
//! ```rust
//! use sensor_pipeline::sensor::{Sensor, ImuReading};
//! use sensor_pipeline::timestamp::Timestamped;
//! use sensor_pipeline::error::Result;
//!
//! struct MyImu;
//!
//! impl Sensor for MyImu {
//!     type Reading = ImuReading;
//!
//!     fn sample(&mut self) -> Result<Timestamped<Self::Reading>> {
//!         todo!("Read from hardware")
//!     }
//!
//!     fn name(&self) -> &str { "my_imu" }
//!     fn sample_rate_hz(&self) -> f64 { 1000.0 }
//! }
//! ```

mod imu;
mod lidar;
mod traits;

#[cfg(feature = "std")]
mod mock;

// Re-export main types
pub use imu::{ImuCalibration, ImuReading, Imu9DofReading, Vec3};
pub use lidar::{LidarPoint2D, LidarPoint3D, LidarScanFixed};
pub use traits::{CalibratableSensor, ConfigurableSensor, Sensor, SensorInfo};

#[cfg(feature = "std")]
pub use lidar::LidarScan2D;

#[cfg(feature = "std")]
pub use mock::{MockImu, NoiseConfig, SequenceSensor, SimpleRng};

#[cfg(feature = "std")]
pub use traits::FnSensor;
