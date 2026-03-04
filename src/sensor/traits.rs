//! Sensor trait and common sensor abstractions.
//!
//! This module defines the core [`Sensor`] trait that all sensor implementations
//! must implement, providing a common interface for sampling data with timestamps.

use crate::error::Result;
use crate::timestamp::Timestamped;

/// A sensor that produces timestamped readings.
///
/// This trait abstracts over any sensor type (IMU, LIDAR, GPS, encoder, etc.)
/// and provides a common interface for the pipeline to consume data.
///
/// # Example
///
/// ```rust
/// use sensor_bridge::sensor::{Sensor, ImuReading};
/// use sensor_bridge::timestamp::Timestamped;
/// use sensor_bridge::error::Result;
///
/// struct MyImu {
///     // hardware handle
/// }
///
/// impl Sensor for MyImu {
///     type Reading = ImuReading;
///
///     fn sample(&mut self) -> Result<Timestamped<Self::Reading>> {
///         // Read from hardware and timestamp
///         todo!()
///     }
///
///     fn name(&self) -> &str {
///         "my_imu"
///     }
///
///     fn sample_rate_hz(&self) -> f64 {
///         1000.0
///     }
/// }
/// ```
pub trait Sensor: Send {
    /// The type of data this sensor produces.
    type Reading: Send;

    /// Takes a single sample from the sensor.
    ///
    /// This should be non-blocking for real-time sensors. If the sensor
    /// requires time to produce a reading, implement a state machine
    /// or use async.
    ///
    /// # Errors
    ///
    /// Returns an error if the sensor fails to produce a reading.
    fn sample(&mut self) -> Result<Timestamped<Self::Reading>>;

    /// Returns the human-readable name of this sensor.
    fn name(&self) -> &str;

    /// Returns the expected sample rate in Hz.
    ///
    /// This is used for monitoring and may be 0 for event-driven sensors.
    fn sample_rate_hz(&self) -> f64;

    /// Returns `true` if the sensor is ready to produce samples.
    fn is_ready(&self) -> bool {
        true
    }

    /// Initializes the sensor if needed.
    ///
    /// # Errors
    ///
    /// Returns an error if initialization fails.
    fn initialize(&mut self) -> Result<()> {
        Ok(())
    }

    /// Resets the sensor state.
    ///
    /// # Errors
    ///
    /// Returns an error if reset fails.
    fn reset(&mut self) -> Result<()> {
        Ok(())
    }

    /// Shuts down the sensor cleanly.
    fn shutdown(&mut self) {}
}

/// Extension trait for sensors that support configuration.
pub trait ConfigurableSensor: Sensor {
    /// The configuration type for this sensor.
    type Config;

    /// Applies configuration to the sensor.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration cannot be applied.
    fn configure(&mut self, config: Self::Config) -> Result<()>;

    /// Returns the current configuration.
    fn config(&self) -> &Self::Config;
}

/// Extension trait for sensors that support calibration.
pub trait CalibratableSensor: Sensor {
    /// The calibration data type.
    type Calibration;

    /// Performs calibration and returns the calibration data.
    ///
    /// # Errors
    ///
    /// Returns an error if calibration fails.
    fn calibrate(&mut self) -> Result<Self::Calibration>;

    /// Applies calibration data to the sensor.
    ///
    /// # Errors
    ///
    /// Returns an error if the calibration cannot be applied.
    fn apply_calibration(&mut self, calibration: &Self::Calibration) -> Result<()>;
}

/// A wrapper that adapts a function into a sensor.
///
/// This is useful for creating sensors from closures or simple functions.
///
/// # Example
///
/// ```rust
/// use sensor_bridge::sensor::{FnSensor, Sensor};
/// use sensor_bridge::timestamp::{Timestamped, MonotonicClock};
/// use sensor_bridge::error::Result;
///
/// let clock = MonotonicClock::new();
/// let mut counter = 0u64;
///
/// let sensor = FnSensor::new("counter", 100.0, move || {
///     counter += 1;
///     Ok(counter)
/// });
/// ```
#[cfg(feature = "std")]
pub struct FnSensor<T, F>
where
    F: FnMut() -> Result<T>,
{
    name: &'static str,
    sample_rate: f64,
    clock: crate::timestamp::MonotonicClock,
    sample_fn: F,
}

#[cfg(feature = "std")]
impl<T, F> FnSensor<T, F>
where
    F: FnMut() -> Result<T>,
{
    /// Creates a new function-based sensor.
    pub fn new(name: &'static str, sample_rate_hz: f64, sample_fn: F) -> Self {
        Self {
            name,
            sample_rate: sample_rate_hz,
            clock: crate::timestamp::MonotonicClock::new(),
            sample_fn,
        }
    }
}

#[cfg(feature = "std")]
impl<T: Send, F: FnMut() -> Result<T> + Send> Sensor for FnSensor<T, F> {
    type Reading = T;

    fn sample(&mut self) -> Result<Timestamped<Self::Reading>> {
        let data = (self.sample_fn)()?;
        Ok(self.clock.stamp(data))
    }

    fn name(&self) -> &str {
        self.name
    }

    fn sample_rate_hz(&self) -> f64 {
        self.sample_rate
    }
}

/// Metadata about a sensor for introspection.
#[derive(Debug, Clone)]
pub struct SensorInfo {
    /// Human-readable name of the sensor.
    pub name: String,
    /// Expected sample rate in Hz.
    pub sample_rate_hz: f64,
    /// Type of sensor (e.g., "IMU", "LIDAR", "GPS").
    pub sensor_type: String,
    /// Additional key-value metadata.
    #[cfg(feature = "std")]
    pub metadata: std::collections::HashMap<String, String>,
}

impl SensorInfo {
    /// Creates new sensor info with the given name and type.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        sensor_type: impl Into<String>,
        sample_rate_hz: f64,
    ) -> Self {
        Self {
            name: name.into(),
            sample_rate_hz,
            sensor_type: sensor_type.into(),
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Adds metadata to the sensor info.
    #[cfg(feature = "std")]
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}
