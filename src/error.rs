//! Error types for the sensor pipeline.

use core::fmt;

/// Result type alias for pipeline operations.
pub type Result<T> = core::result::Result<T, PipelineError>;

/// Errors that can occur in the sensor pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineError {
    /// The ring buffer is full and cannot accept more data.
    BufferFull,
    /// The ring buffer is empty.
    BufferEmpty,
    /// A sensor failed to produce a reading.
    SensorError(SensorError),
    /// A pipeline stage failed to process data.
    StageError(&'static str),
    /// Timestamp synchronization failed.
    TimestampError(&'static str),
    /// The pipeline has been shut down.
    Shutdown,
}

/// Errors specific to sensor operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SensorError {
    /// The sensor is not initialized.
    NotInitialized,
    /// The sensor failed to read data.
    ReadFailed,
    /// The sensor is disconnected.
    Disconnected,
    /// The sensor returned invalid data.
    InvalidData,
    /// The sensor timed out.
    Timeout,
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferFull => write!(f, "ring buffer is full"),
            Self::BufferEmpty => write!(f, "ring buffer is empty"),
            Self::SensorError(e) => write!(f, "sensor error: {e}"),
            Self::StageError(msg) => write!(f, "stage error: {msg}"),
            Self::TimestampError(msg) => write!(f, "timestamp error: {msg}"),
            Self::Shutdown => write!(f, "pipeline has been shut down"),
        }
    }
}

impl fmt::Display for SensorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInitialized => write!(f, "sensor not initialized"),
            Self::ReadFailed => write!(f, "failed to read from sensor"),
            Self::Disconnected => write!(f, "sensor disconnected"),
            Self::InvalidData => write!(f, "sensor returned invalid data"),
            Self::Timeout => write!(f, "sensor read timed out"),
        }
    }
}

impl From<SensorError> for PipelineError {
    fn from(err: SensorError) -> Self {
        Self::SensorError(err)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for PipelineError {}

#[cfg(feature = "std")]
impl std::error::Error for SensorError {}
