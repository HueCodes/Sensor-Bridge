//! Pipeline configuration types.
//!
//! This module provides configuration structures for the multi-stage pipeline,
//! allowing fine-grained control over buffer sizes, thread priorities, and
//! backpressure behavior.

use core::time::Duration;

/// Configuration for a single pipeline stage.
#[derive(Debug, Clone)]
pub struct StageConfig {
    /// Name of the stage for identification and metrics.
    pub name: String,
    /// Size of the input queue (number of items).
    pub queue_size: usize,
    /// Whether to enable detailed metrics collection.
    pub enable_metrics: bool,
    /// Timeout for blocking operations.
    pub timeout: Option<Duration>,
}

impl Default for StageConfig {
    fn default() -> Self {
        Self {
            name: "unnamed".to_string(),
            queue_size: 1024,
            enable_metrics: true,
            timeout: Some(Duration::from_millis(100)),
        }
    }
}

impl StageConfig {
    /// Creates a new stage configuration with the given name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Sets the queue size.
    #[must_use]
    pub fn with_queue_size(mut self, size: usize) -> Self {
        self.queue_size = size;
        self
    }

    /// Sets whether metrics collection is enabled.
    #[must_use]
    pub fn with_metrics(mut self, enabled: bool) -> Self {
        self.enable_metrics = enabled;
        self
    }

    /// Sets the operation timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Backpressure strategy when queues are full.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackpressureStrategy {
    /// Block until space is available.
    #[default]
    Block,
    /// Drop the incoming item.
    DropNewest,
    /// Drop the oldest item in the queue.
    DropOldest,
    /// Sample: keep 1 in N items.
    Sample(u32),
}

/// Configuration for backpressure handling.
#[derive(Debug, Clone)]
pub struct BackpressureConfig {
    /// The strategy to use when queues are full.
    pub strategy: BackpressureStrategy,
    /// High water mark (percentage 0-100) to start applying backpressure.
    pub high_water_mark: u8,
    /// Low water mark (percentage 0-100) to stop applying backpressure.
    pub low_water_mark: u8,
    /// Maximum time to wait before dropping (for Block strategy).
    pub max_wait: Duration,
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            strategy: BackpressureStrategy::Block,
            high_water_mark: 80,
            low_water_mark: 50,
            max_wait: Duration::from_millis(10),
        }
    }
}

impl BackpressureConfig {
    /// Creates a new backpressure configuration with the given strategy.
    #[must_use]
    pub fn new(strategy: BackpressureStrategy) -> Self {
        Self {
            strategy,
            ..Default::default()
        }
    }

    /// Sets the water marks.
    #[must_use]
    pub fn with_water_marks(mut self, high: u8, low: u8) -> Self {
        self.high_water_mark = high.min(100);
        self.low_water_mark = low.min(high);
        self
    }

    /// Sets the maximum wait time.
    #[must_use]
    pub fn with_max_wait(mut self, duration: Duration) -> Self {
        self.max_wait = duration;
        self
    }
}

/// Configuration for the entire multi-stage pipeline.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Configuration for the ingestion stage.
    pub ingestion: StageConfig,
    /// Configuration for the filtering stage.
    pub filtering: StageConfig,
    /// Configuration for the aggregation stage.
    pub aggregation: StageConfig,
    /// Configuration for the output stage.
    pub output: StageConfig,
    /// Global backpressure configuration.
    pub backpressure: BackpressureConfig,
    /// Whether to enable the metrics dashboard.
    pub enable_dashboard: bool,
    /// Metrics reporting interval.
    pub metrics_interval: Duration,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            ingestion: StageConfig::new("ingestion").with_queue_size(2048),
            filtering: StageConfig::new("filtering").with_queue_size(1024),
            aggregation: StageConfig::new("aggregation").with_queue_size(512),
            output: StageConfig::new("output").with_queue_size(256),
            backpressure: BackpressureConfig::default(),
            enable_dashboard: false,
            metrics_interval: Duration::from_millis(100),
        }
    }
}

impl PipelineConfig {
    /// Creates a new pipeline configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a configuration optimized for low latency.
    #[must_use]
    pub fn low_latency() -> Self {
        Self {
            ingestion: StageConfig::new("ingestion")
                .with_queue_size(256)
                .with_timeout(Some(Duration::from_micros(100))),
            filtering: StageConfig::new("filtering")
                .with_queue_size(128)
                .with_timeout(Some(Duration::from_micros(100))),
            aggregation: StageConfig::new("aggregation")
                .with_queue_size(64)
                .with_timeout(Some(Duration::from_micros(100))),
            output: StageConfig::new("output")
                .with_queue_size(32)
                .with_timeout(Some(Duration::from_micros(100))),
            backpressure: BackpressureConfig::new(BackpressureStrategy::DropNewest)
                .with_water_marks(70, 40),
            enable_dashboard: false,
            metrics_interval: Duration::from_millis(50),
        }
    }

    /// Creates a configuration optimized for high throughput.
    #[must_use]
    pub fn high_throughput() -> Self {
        Self {
            ingestion: StageConfig::new("ingestion")
                .with_queue_size(8192)
                .with_timeout(Some(Duration::from_millis(100))),
            filtering: StageConfig::new("filtering")
                .with_queue_size(4096)
                .with_timeout(Some(Duration::from_millis(100))),
            aggregation: StageConfig::new("aggregation")
                .with_queue_size(2048)
                .with_timeout(Some(Duration::from_millis(100))),
            output: StageConfig::new("output")
                .with_queue_size(1024)
                .with_timeout(Some(Duration::from_millis(100))),
            backpressure: BackpressureConfig::new(BackpressureStrategy::Block)
                .with_water_marks(90, 70),
            enable_dashboard: false,
            metrics_interval: Duration::from_millis(200),
        }
    }

    /// Sets the ingestion stage configuration.
    #[must_use]
    pub fn with_ingestion(mut self, config: StageConfig) -> Self {
        self.ingestion = config;
        self
    }

    /// Sets the filtering stage configuration.
    #[must_use]
    pub fn with_filtering(mut self, config: StageConfig) -> Self {
        self.filtering = config;
        self
    }

    /// Sets the aggregation stage configuration.
    #[must_use]
    pub fn with_aggregation(mut self, config: StageConfig) -> Self {
        self.aggregation = config;
        self
    }

    /// Sets the output stage configuration.
    #[must_use]
    pub fn with_output(mut self, config: StageConfig) -> Self {
        self.output = config;
        self
    }

    /// Sets the backpressure configuration.
    #[must_use]
    pub fn with_backpressure(mut self, config: BackpressureConfig) -> Self {
        self.backpressure = config;
        self
    }

    /// Enables or disables the metrics dashboard.
    #[must_use]
    pub fn with_dashboard(mut self, enabled: bool) -> Self {
        self.enable_dashboard = enabled;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = PipelineConfig::default();
        assert_eq!(config.ingestion.queue_size, 2048);
        assert_eq!(config.filtering.queue_size, 1024);
        assert!(config.ingestion.enable_metrics);
    }

    #[test]
    fn test_low_latency_config() {
        let config = PipelineConfig::low_latency();
        assert!(config.ingestion.queue_size < 1024);
        assert_eq!(config.backpressure.strategy, BackpressureStrategy::DropNewest);
    }

    #[test]
    fn test_high_throughput_config() {
        let config = PipelineConfig::high_throughput();
        assert!(config.ingestion.queue_size >= 8192);
        assert_eq!(config.backpressure.strategy, BackpressureStrategy::Block);
    }

    #[test]
    fn test_builder_pattern() {
        let config = PipelineConfig::new()
            .with_ingestion(StageConfig::new("custom_ingestion").with_queue_size(4096))
            .with_backpressure(BackpressureConfig::new(BackpressureStrategy::DropOldest))
            .with_dashboard(true);

        assert_eq!(config.ingestion.name, "custom_ingestion");
        assert_eq!(config.ingestion.queue_size, 4096);
        assert_eq!(config.backpressure.strategy, BackpressureStrategy::DropOldest);
        assert!(config.enable_dashboard);
    }
}
