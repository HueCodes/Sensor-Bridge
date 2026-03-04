//! Pipeline processing stages.
//!
//! This module provides composable stages for processing sensor data:
//!
//! - [`Stage`] trait: Core abstraction for data processing
//! - [`Chain`]: Compose stages sequentially
//! - Signal filters: [`MovingAverage`], [`ExponentialMovingAverage`], [`KalmanFilter1D`]
//! - Transforms: [`VectorTransform`], [`ImuTransform`]
//! - Fusion: [`TimestampSync`], [`Fuse`]
//!
//! # Composing Stages
//!
//! Stages can be composed using the [`Chain`] combinator or the [`StageExt`] trait:
//!
//! ```rust
//! use sensor_bridge::stage::{Stage, StageExt, Map, Filter};
//!
//! let mut pipeline = Map::new(|x: i32| x * 2)
//!     .filter(|x| *x > 10)
//!     .map(|x| x + 1);
//!
//! assert_eq!(pipeline.process(3), None);   // 6, filtered
//! assert_eq!(pipeline.process(6), Some(13)); // 12 + 1
//! ```

mod filter;
mod fusion;
mod traits;
mod transform;

// Re-export all stage types
pub use filter::{
    ExponentialMovingAverage, Filterable, HighPassFilter, KalmanFilter1D, LowPassFilter,
    MedianFilter, MovingAverage,
};
pub use fusion::{Fuse, SyncInput, SyncedPair, TimestampBuffer, TimestampSync, WeightedAverage};
pub use traits::{Chain, Filter, FilterMap, Identity, Inspect, Map, Stage, StageExt};
pub use transform::{
    units, BiasCorrection, ImuTransform, ImuUnitConversion, RotationMatrix, Scale, VectorTransform,
};
