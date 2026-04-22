#![doc = include_str!("../README.md")]
//!
//! ## Getting Started
//!
//! Add to your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! sensor-bridge = "0.1"
//! ```
//!
//! Build a 4-stage pipeline:
//!
//! ```rust,no_run
//! use sensor_bridge::pipeline::{MultiStagePipelineBuilder, PipelineConfig};
//! use sensor_bridge::stage::{Filter, Identity, Map};
//! use std::time::Duration;
//!
//! let mut pipeline = MultiStagePipelineBuilder::<i32, i32, _, _, _, _>::new()
//!     .config(PipelineConfig::default().channel_capacity(1024))
//!     .ingestion(Map::new(|x: i32| x + 1))
//!     .filtering(Filter::new(|x: &i32| *x > 0))
//!     .aggregation(Map::new(|x: i32| x * 2))
//!     .output(Identity::new())
//!     .build();
//!
//! pipeline.send(5).unwrap();
//! std::thread::sleep(Duration::from_millis(10));
//! let result = pipeline.try_recv();
//!
//! pipeline.shutdown();
//! pipeline.join().unwrap();
//! ```
//!
//! Or use the low-level ring buffer directly:
//!
//! ```rust,no_run
//! use sensor_bridge::{
//!     buffer::RingBuffer,
//!     timestamp::Timestamped,
//!     sensor::ImuReading,
//! };
//!
//! let buffer: RingBuffer<Timestamped<ImuReading>, 1024> = RingBuffer::new();
//! let (producer, consumer) = buffer.split();
//! // producer.push(item) / consumer.pop()
//! ```

#![forbid(unsafe_op_in_unsafe_fn)]
#![deny(missing_docs)]
#![warn(clippy::all)]
#![allow(
    clippy::module_name_repetitions,
    clippy::type_complexity,
    // Pedantic lints opted into selectively:
)]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod math;

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
