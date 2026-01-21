//! Lock-free buffer implementations for the sensor pipeline.
//!
//! This module provides the core data structures for high-performance,
//! lock-free data transfer between pipeline stages.
//!
//! # Ring Buffer
//!
//! The [`RingBuffer`] is a single-producer, single-consumer (SPSC) queue with:
//! - Wait-free operations (bounded worst-case execution time)
//! - Cache-line padding to prevent false sharing
//! - Zero-copy design for maximum performance
//!
//! # Usage
//!
//! ```rust
//! use sensor_pipeline::buffer::{RingBuffer, CachePadded};
//!
//! // Create a buffer for 1024 elements
//! let buffer: RingBuffer<u32, 1024> = RingBuffer::new();
//!
//! // Split into producer and consumer handles
//! let (producer, consumer) = buffer.split();
//!
//! // Producer pushes data
//! producer.push(42).unwrap();
//!
//! // Consumer pops data
//! assert_eq!(consumer.pop(), Some(42));
//! ```

mod cache_padded;
mod ring;
mod split;

pub use cache_padded::{CachePadded, CACHE_LINE_SIZE};
pub use ring::{Consumer, Producer, RingBuffer};

#[cfg(feature = "std")]
pub use split::{BackpressurePolicy, BackpressureProducer, OwnedConsumer, OwnedProducer};
