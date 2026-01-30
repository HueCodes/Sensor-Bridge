//! Zero-copy optimizations for the sensor pipeline.
//!
//! This module provides utilities for minimizing allocations and data copies
//! in the hot path of the pipeline:
//!
//! - [`SharedData`]: Arc-wrapped data for zero-copy sharing between stages
//! - [`ObjectPool`]: Pre-allocated object pool with RAII guards
//! - [`BufferPool`]: Pool for variable-size byte buffers
//!
//! # Performance Goal
//!
//! These utilities target <10 allocations per 1000 items processed,
//! significantly reducing GC pressure and improving latency consistency.
//!
//! # Example
//!
//! ```rust,ignore
//! use sensor_pipeline::zero_copy::{ObjectPool, PoolGuard};
//!
//! // Create a pool of sensor readings
//! let pool = ObjectPool::new(|| SensorReading::default(), 100);
//!
//! // Acquire an object from the pool (no allocation)
//! let mut reading = pool.acquire();
//! reading.timestamp = 12345;
//! reading.value = 42.0;
//!
//! // Object is returned to pool when guard is dropped
//! drop(reading);
//! ```

mod shared_data;
mod object_pool;
mod buffer_pool;

pub use shared_data::SharedData;
pub use object_pool::{ObjectPool, PoolGuard, PooledObject};
pub use buffer_pool::{BufferPool, PooledBuffer};
