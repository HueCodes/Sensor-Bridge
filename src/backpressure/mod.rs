//! Backpressure and flow control for the sensor pipeline.
//!
//! This module provides mechanisms for handling overload conditions gracefully:
//!
//! - [`BackpressureStrategy`]: Different strategies for handling full buffers
//! - [`AdaptiveController`]: Dynamic quality degradation under load
//! - [`RateLimiter`]: Token bucket rate limiting
//!
//! # Example
//!
//! ```rust,ignore
//! use sensor_pipeline::backpressure::{AdaptiveController, BackpressureStrategy};
//!
//! let controller = AdaptiveController::new()
//!     .high_water_mark(0.8)
//!     .low_water_mark(0.5)
//!     .build();
//!
//! // Check if we should accept an item based on queue utilization
//! if controller.should_accept(queue_utilization) {
//!     // Process item
//! } else {
//!     // Drop or defer item
//! }
//! ```

mod strategy;
mod controller;
mod rate_limiter;

pub use strategy::BackpressureStrategy;
pub use controller::{AdaptiveController, AdaptiveControllerBuilder, QualityLevel};
pub use rate_limiter::{RateLimiter, RateLimiterConfig};
