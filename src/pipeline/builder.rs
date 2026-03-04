//! Pipeline builder for ergonomic pipeline construction.
//!
//! The builder pattern allows constructing complex pipelines with a fluent API.

use crate::buffer::RingBuffer;
use crate::stage::{Chain, Filter, FilterMap, Inspect, Map, Stage};

/// A pipeline builder that constructs processing pipelines.
///
/// # Example
///
/// ```rust
/// use sensor_bridge::pipeline::PipelineBuilder;
///
/// let pipeline = PipelineBuilder::new()
///     .map(|x: i32| x * 2)
///     .filter(|x| *x > 10)
///     .map(|x| x.to_string())
///     .build();
/// ```
#[derive(Debug)]
pub struct PipelineBuilder<S> {
    stage: S,
}

/// Marker type for an empty pipeline builder.
#[derive(Debug, Default, Clone, Copy)]
pub struct Empty;

impl PipelineBuilder<Empty> {
    /// Creates a new empty pipeline builder.
    #[must_use]
    pub const fn new() -> Self {
        Self { stage: Empty }
    }

    /// Adds a mapping stage as the first stage.
    #[must_use]
    pub fn map<I, O, F>(self, f: F) -> PipelineBuilder<Map<I, O, F>>
    where
        F: FnMut(I) -> O + Send,
        I: Send,
        O: Send,
    {
        PipelineBuilder { stage: Map::new(f) }
    }

    /// Adds a filter stage as the first stage.
    #[must_use]
    pub fn filter<T, F>(self, predicate: F) -> PipelineBuilder<Filter<T, F>>
    where
        F: FnMut(&T) -> bool + Send,
        T: Send,
    {
        PipelineBuilder {
            stage: Filter::new(predicate),
        }
    }

    /// Adds a filter-map stage as the first stage.
    #[must_use]
    pub fn filter_map<I, O, F>(self, f: F) -> PipelineBuilder<FilterMap<I, O, F>>
    where
        F: FnMut(I) -> Option<O> + Send,
        I: Send,
        O: Send,
    {
        PipelineBuilder {
            stage: FilterMap::new(f),
        }
    }

    /// Starts with a custom stage.
    #[must_use]
    pub fn stage<S: Stage>(self, stage: S) -> PipelineBuilder<S> {
        PipelineBuilder { stage }
    }
}

impl Default for PipelineBuilder<Empty> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: Stage> PipelineBuilder<S> {
    /// Adds a mapping stage to the pipeline.
    #[must_use]
    pub fn map<O, F>(self, f: F) -> PipelineBuilder<Chain<S, Map<S::Output, O, F>>>
    where
        F: FnMut(S::Output) -> O + Send,
        S::Output: Send,
        O: Send,
    {
        PipelineBuilder {
            stage: Chain::new(self.stage, Map::new(f)),
        }
    }

    /// Adds a filter stage to the pipeline.
    #[must_use]
    pub fn filter<F>(self, predicate: F) -> PipelineBuilder<Chain<S, Filter<S::Output, F>>>
    where
        F: FnMut(&S::Output) -> bool + Send,
        S::Output: Send,
    {
        PipelineBuilder {
            stage: Chain::new(self.stage, Filter::new(predicate)),
        }
    }

    /// Adds a filter-map stage to the pipeline.
    #[must_use]
    pub fn filter_map<O, F>(self, f: F) -> PipelineBuilder<Chain<S, FilterMap<S::Output, O, F>>>
    where
        F: FnMut(S::Output) -> Option<O> + Send,
        S::Output: Send,
        O: Send,
    {
        PipelineBuilder {
            stage: Chain::new(self.stage, FilterMap::new(f)),
        }
    }

    /// Adds an inspect stage for debugging/logging.
    #[must_use]
    pub fn inspect<F>(self, f: F) -> PipelineBuilder<Chain<S, Inspect<S::Output, F>>>
    where
        F: FnMut(&S::Output) + Send,
        S::Output: Send,
    {
        PipelineBuilder {
            stage: Chain::new(self.stage, Inspect::new(f)),
        }
    }

    /// Chains with a custom stage.
    #[must_use]
    pub fn then<S2>(self, next: S2) -> PipelineBuilder<Chain<S, S2>>
    where
        S2: Stage<Input = S::Output>,
    {
        PipelineBuilder {
            stage: Chain::new(self.stage, next),
        }
    }

    /// Builds the final pipeline.
    #[must_use]
    pub fn build(self) -> S {
        self.stage
    }
}

/// Configuration for a sensor pipeline.
#[derive(Debug, Clone)]
#[cfg(feature = "std")]
pub struct PipelineConfig {
    /// Name of the pipeline for logging/metrics.
    pub name: String,
    /// Size of the input ring buffer (must be power of 2).
    pub buffer_size: usize,
    /// Whether to track latency metrics.
    pub track_latency: bool,
    /// Whether to track drop counts.
    pub track_drops: bool,
}

#[cfg(feature = "std")]
impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            name: String::from("pipeline"),
            buffer_size: 1024,
            track_latency: true,
            track_drops: true,
        }
    }
}

#[cfg(feature = "std")]
impl PipelineConfig {
    /// Creates a new pipeline configuration with the given name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Sets the buffer size.
    #[must_use]
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        assert!(
            size > 0 && (size & (size - 1)) == 0,
            "Buffer size must be power of 2"
        );
        self.buffer_size = size;
        self
    }

    /// Enables or disables latency tracking.
    #[must_use]
    pub fn with_latency_tracking(mut self, enabled: bool) -> Self {
        self.track_latency = enabled;
        self
    }

    /// Enables or disables drop counting.
    #[must_use]
    pub fn with_drop_tracking(mut self, enabled: bool) -> Self {
        self.track_drops = enabled;
        self
    }
}

/// A complete sensor pipeline with buffer and processing stage.
#[cfg(feature = "std")]
pub struct SensorPipeline<T, S, const N: usize>
where
    S: Stage<Input = T>,
{
    /// The ring buffer for incoming data.
    pub buffer: RingBuffer<T, N>,
    /// The processing stage.
    pub stage: S,
    /// Pipeline configuration.
    pub config: PipelineConfig,
}

#[cfg(feature = "std")]
impl<T, S, const N: usize> SensorPipeline<T, S, N>
where
    S: Stage<Input = T>,
{
    /// Creates a new sensor pipeline.
    #[must_use]
    pub fn new(stage: S, config: PipelineConfig) -> Self {
        Self {
            buffer: RingBuffer::new(),
            stage,
            config,
        }
    }

    /// Creates with default configuration.
    #[must_use]
    pub fn with_default_config(stage: S) -> Self {
        Self::new(stage, PipelineConfig::default())
    }

    /// Returns the pipeline name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.config.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_basic() {
        let mut pipeline = PipelineBuilder::new()
            .map(|x: i32| x * 2)
            .filter(|x| *x > 5)
            .map(|x| x + 1)
            .build();

        assert_eq!(pipeline.process(1), None); // 2, filtered
        assert_eq!(pipeline.process(3), Some(7)); // 6 + 1
        assert_eq!(pipeline.process(10), Some(21)); // 20 + 1
    }

    #[test]
    fn test_builder_with_custom_stage() {
        use crate::stage::MovingAverage;

        let mut pipeline = PipelineBuilder::new()
            .stage(MovingAverage::<f32, 3>::new())
            .map(|x| x * 2.0)
            .build();

        assert!((pipeline.process(3.0).unwrap() - 6.0).abs() < 0.01);
    }

    #[test]
    fn test_builder_filter_map() {
        let mut pipeline = PipelineBuilder::new()
            .filter_map(|x: i32| if x > 0 { Some(x as u32) } else { None })
            .map(|x| x * 2)
            .build();

        assert_eq!(pipeline.process(-1), None);
        assert_eq!(pipeline.process(5), Some(10));
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_pipeline_config() {
        let config = PipelineConfig::new("test")
            .with_buffer_size(2048)
            .with_latency_tracking(true);

        assert_eq!(config.name, "test");
        assert_eq!(config.buffer_size, 2048);
        assert!(config.track_latency);
    }
}
