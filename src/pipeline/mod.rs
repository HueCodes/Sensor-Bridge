//! Pipeline construction and execution.
//!
//! This module provides:
//! - [`PipelineBuilder`]: Ergonomic pipeline construction
//! - [`PipelineRunner`]: Single-threaded pipeline execution
//! - [`spawn_pipeline`]: Multi-threaded pipeline execution
//!
//! # Building a Pipeline
//!
//! ```rust
//! use sensor_pipeline::pipeline::PipelineBuilder;
//!
//! let pipeline = PipelineBuilder::new()
//!     .map(|x: i32| x * 2)
//!     .filter(|x| *x > 10)
//!     .build();
//! ```
//!
//! # Running a Pipeline
//!
//! ```rust
//! use sensor_pipeline::buffer::RingBuffer;
//! use sensor_pipeline::pipeline::{PipelineBuilder, PipelineRunner};
//! use sensor_pipeline::stage::Map;
//!
//! let buffer: RingBuffer<i32, 16> = RingBuffer::new();
//! let (producer, consumer) = buffer.split();
//!
//! // Push data
//! producer.push(1).unwrap();
//! producer.push(2).unwrap();
//!
//! // Process data
//! let mut runner = PipelineRunner::new(consumer, Map::new(|x: i32| x * 2));
//! assert_eq!(runner.poll(), Some(2));
//! assert_eq!(runner.poll(), Some(4));
//! ```

mod builder;
mod runner;

pub use builder::PipelineBuilder;
pub use runner::{BatchOptions, PipelineRunner, PipelineStats};

#[cfg(feature = "std")]
pub use builder::{PipelineConfig, SensorPipeline};

#[cfg(feature = "std")]
pub use runner::{spawn_pipeline, PipelineHandle};
