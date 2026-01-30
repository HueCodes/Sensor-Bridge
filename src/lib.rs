//! # Sensor Pipeline
//!
//! A real-time sensor data pipeline library with lock-free data structures
//! designed for robotics applications.
//!
//! ## Features
//!
//! - **Lock-free SPSC ring buffer**: Wait-free single-producer, single-consumer queue
//!   with proper cache-line padding to prevent false sharing
//! - **Zero-copy data handling**: Minimize allocations and copies in hot paths
//! - **Composable pipeline stages**: Filter, transform, and fuse sensor data
//! - **Timestamp synchronization**: Align data from multiple sensors
//! - **Configurable backpressure**: Handle slow consumers gracefully
//! - **Metrics and observability**: Track latency percentiles and dropped samples
//!
//! ## Example
//!
//! ```rust,no_run
//! use sensor_pipeline::{
//!     buffer::RingBuffer,
//!     timestamp::Timestamped,
//!     sensor::ImuReading,
//! };
//!
//! // Create a ring buffer for IMU data
//! let buffer: RingBuffer<Timestamped<ImuReading>, 1024> = RingBuffer::new();
//! let (producer, consumer) = buffer.split();
//!
//! // Producer pushes data
//! // Consumer pops and processes
//! ```

#![forbid(unsafe_op_in_unsafe_fn)]
#![deny(missing_docs)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(clippy::module_name_repetitions)]
#![cfg_attr(not(feature = "std"), no_std)]

pub mod buffer;
pub mod error;
pub mod metrics;
pub mod pipeline;
pub mod sensor;
pub mod stage;
pub mod timestamp;

#[cfg(feature = "std")]
pub mod backpressure;
#[cfg(feature = "std")]
pub mod channel;
#[cfg(feature = "std")]
pub mod zero_copy;

// Re-export commonly used types at crate root
pub use buffer::{Consumer, Producer, RingBuffer};
pub use error::{PipelineError, Result};
pub use sensor::{ImuReading, Sensor};
pub use stage::Stage;
pub use timestamp::Timestamped;

// Re-export channel types
#[cfg(feature = "std")]
pub use channel::{bounded, unbounded, Receiver, Sender};

// Re-export pipeline types
#[cfg(feature = "std")]
pub use pipeline::{MultiStagePipeline, PipelineState, SimplePipelineBuilder};

// Re-export backpressure types
#[cfg(feature = "std")]
pub use backpressure::{AdaptiveController, BackpressureStrategy, RateLimiter};

// Re-export zero-copy types
#[cfg(feature = "std")]
pub use zero_copy::{BufferPool, ObjectPool, SharedData};

// Re-export metrics types
#[cfg(feature = "std")]
pub use metrics::{Dashboard, JitterTracker, PerformanceTargets};
