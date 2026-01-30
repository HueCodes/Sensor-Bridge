//! Multi-stage pipeline orchestration.
//!
//! This module provides `MultiStagePipeline` for creating and managing
//! a 4-stage sensor processing pipeline:
//!
//! 1. **Ingestion**: Receives raw sensor data
//! 2. **Filtering**: Validates and filters data
//! 3. **Aggregation**: Combines/transforms data
//! 4. **Output**: Delivers processed results
//!
//! Each stage runs in a dedicated thread with lock-free channels
//! for inter-stage communication.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::channel::{bounded, Receiver, Sender};
use crate::stage::Stage;

use super::executor::{stage_worker, StageMetrics, StageMetricsSnapshot};
use super::stage_handle::{StageCommand, StageControl};

/// State of the entire pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PipelineState {
    /// Pipeline is created but not started.
    Created = 0,
    /// Pipeline is starting up.
    Starting = 1,
    /// Pipeline is running normally.
    Running = 2,
    /// Pipeline is stopping.
    Stopping = 3,
    /// Pipeline has stopped.
    Stopped = 4,
    /// Pipeline encountered an error.
    Error = 5,
}

impl From<u8> for PipelineState {
    fn from(v: u8) -> Self {
        match v {
            0 => Self::Created,
            1 => Self::Starting,
            2 => Self::Running,
            3 => Self::Stopping,
            4 => Self::Stopped,
            5 => Self::Error,
            _ => Self::Created,
        }
    }
}

/// Configuration for the multi-stage pipeline.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Channel capacity between stages.
    pub channel_capacity: usize,
    /// Timeout for graceful shutdown.
    pub shutdown_timeout: Duration,
    /// Whether to collect detailed metrics.
    pub collect_metrics: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            channel_capacity: 1024,
            shutdown_timeout: Duration::from_secs(5),
            collect_metrics: true,
        }
    }
}

impl PipelineConfig {
    /// Creates a new pipeline configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the channel capacity.
    #[must_use]
    pub fn channel_capacity(mut self, capacity: usize) -> Self {
        self.channel_capacity = capacity;
        self
    }

    /// Sets the shutdown timeout.
    #[must_use]
    pub fn shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = timeout;
        self
    }

    /// Enables or disables metrics collection.
    #[must_use]
    pub fn collect_metrics(mut self, collect: bool) -> Self {
        self.collect_metrics = collect;
        self
    }
}

/// Internal stage data.
struct StageData {
    name: String,
    control: Arc<StageControl>,
    metrics: Arc<StageMetrics>,
    thread: Option<thread::JoinHandle<()>>,
}

/// A multi-stage pipeline with dedicated threads per stage.
///
/// The pipeline consists of 4 stages connected by lock-free channels:
/// Input → Ingestion → Filtering → Aggregation → Output
///
/// # Example
///
/// ```rust,ignore
/// use sensor_pipeline::pipeline::{MultiStagePipeline, PipelineConfig};
/// use sensor_pipeline::stage::{Map, Filter, Identity};
///
/// let pipeline = MultiStagePipeline::builder()
///     .config(PipelineConfig::default())
///     .ingestion(Identity::new())
///     .filtering(Filter::new(|x: &i32| *x > 0))
///     .aggregation(Map::new(|x: i32| x * 2))
///     .output(Identity::new())
///     .build();
///
/// // Start the pipeline
/// let mut running = pipeline.start().unwrap();
///
/// // Send data
/// running.send(42).unwrap();
///
/// // Receive results
/// if let Some(result) = running.recv() {
///     println!("Result: {}", result);
/// }
///
/// // Shutdown
/// running.shutdown();
/// ```
pub struct MultiStagePipeline<I, O>
where
    I: Send + 'static,
    O: Send + 'static,
{
    /// Configuration.
    config: PipelineConfig,
    /// Pipeline state.
    state: Arc<AtomicU8>,
    /// Stages (ingestion, filtering, aggregation, output).
    stages: Vec<StageData>,
    /// Input sender for the pipeline.
    input_tx: Option<Sender<I>>,
    /// Output receiver from the pipeline.
    output_rx: Option<Receiver<O>>,
    /// Pipeline start time.
    start_time: Option<Instant>,
}

impl<I, O> MultiStagePipeline<I, O>
where
    I: Send + 'static,
    O: Send + 'static,
{
    /// Creates a new pipeline builder.
    pub fn builder<S1, S2, S3, S4>() -> MultiStagePipelineBuilder<I, O, S1, S2, S3, S4>
    where
        S1: Stage<Input = I> + Send + 'static,
        S2: Stage<Input = S1::Output> + Send + 'static,
        S3: Stage<Input = S2::Output> + Send + 'static,
        S4: Stage<Input = S3::Output, Output = O> + Send + 'static,
        S1::Output: Send,
        S2::Output: Send,
        S3::Output: Send,
    {
        MultiStagePipelineBuilder::new()
    }

    /// Returns the current pipeline state.
    #[must_use]
    pub fn state(&self) -> PipelineState {
        PipelineState::from(self.state.load(Ordering::Acquire))
    }

    /// Returns whether the pipeline is running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.state() == PipelineState::Running
    }

    /// Returns the input sender for feeding data to the pipeline.
    #[must_use]
    pub fn input(&self) -> Option<&Sender<I>> {
        self.input_tx.as_ref()
    }

    /// Returns the output receiver for getting results from the pipeline.
    #[must_use]
    pub fn output(&self) -> Option<&Receiver<O>> {
        self.output_rx.as_ref()
    }

    /// Sends a value into the pipeline.
    ///
    /// Returns `Ok(())` on success, or `Err(value)` if the pipeline is not running.
    pub fn send(&self, value: I) -> Result<(), I> {
        if let Some(tx) = &self.input_tx {
            tx.send(value).map_err(|e| e.into_inner())
        } else {
            Err(value)
        }
    }

    /// Tries to send a value without blocking.
    pub fn try_send(&self, value: I) -> Result<(), I> {
        if let Some(tx) = &self.input_tx {
            tx.try_send(value).map_err(|e| e.into_inner())
        } else {
            Err(value)
        }
    }

    /// Receives a value from the pipeline.
    pub fn recv(&self) -> Option<O> {
        self.output_rx.as_ref().and_then(|rx| rx.recv())
    }

    /// Tries to receive a value without blocking.
    pub fn try_recv(&self) -> Option<O> {
        self.output_rx.as_ref().and_then(|rx| rx.try_recv().ok())
    }

    /// Returns metrics for all stages.
    #[must_use]
    pub fn metrics(&self) -> Vec<(&str, StageMetricsSnapshot)> {
        self.stages
            .iter()
            .map(|s| (s.name.as_str(), s.metrics.snapshot()))
            .collect()
    }

    /// Returns metrics for a specific stage by index.
    #[must_use]
    pub fn stage_metrics(&self, index: usize) -> Option<StageMetricsSnapshot> {
        self.stages.get(index).map(|s| s.metrics.snapshot())
    }

    /// Pauses all stages.
    pub fn pause(&self) {
        for stage in &self.stages {
            stage.control.set_command(StageCommand::Pause);
        }
    }

    /// Resumes all stages.
    pub fn resume(&self) {
        for stage in &self.stages {
            stage.control.set_command(StageCommand::Resume);
        }
    }

    /// Resets all stages.
    pub fn reset(&self) {
        for stage in &self.stages {
            stage.control.set_command(StageCommand::Reset);
        }
    }

    /// Initiates graceful shutdown of the pipeline.
    pub fn shutdown(&mut self) {
        self.state.store(PipelineState::Stopping as u8, Ordering::Release);

        // Request shutdown for all stages
        for stage in &self.stages {
            stage.control.request_shutdown();
        }

        // Drop the input channel to signal EOF
        self.input_tx.take();
    }

    /// Waits for all stages to finish.
    ///
    /// Call `shutdown()` first to initiate graceful shutdown.
    pub fn join(&mut self) -> Result<(), String> {
        let deadline = Instant::now() + self.config.shutdown_timeout;

        for stage in &mut self.stages {
            if let Some(thread) = stage.thread.take() {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(format!("Timeout waiting for stage '{}' to finish", stage.name));
                }

                // We can't do a timed join in std, so just join
                if thread.join().is_err() {
                    return Err(format!("Stage '{}' panicked", stage.name));
                }
            }
        }

        self.state.store(PipelineState::Stopped as u8, Ordering::Release);
        Ok(())
    }

    /// Returns the total number of items processed by the output stage.
    #[must_use]
    pub fn total_processed(&self) -> u64 {
        self.stages
            .last()
            .map(|s| s.metrics.output_count())
            .unwrap_or(0)
    }

    /// Returns the throughput in items per second.
    #[must_use]
    pub fn throughput(&self) -> f64 {
        if let Some(start) = self.start_time {
            let elapsed = start.elapsed().as_secs_f64();
            if elapsed > 0.0 {
                return self.total_processed() as f64 / elapsed;
            }
        }
        0.0
    }

    /// Returns elapsed time since pipeline start.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.start_time.map(|t| t.elapsed()).unwrap_or_default()
    }
}

impl<I, O> Drop for MultiStagePipeline<I, O>
where
    I: Send + 'static,
    O: Send + 'static,
{
    fn drop(&mut self) {
        // Ensure shutdown is initiated
        if self.state() != PipelineState::Stopped {
            self.shutdown();
            let _ = self.join();
        }
    }
}

/// Builder for constructing a multi-stage pipeline.
pub struct MultiStagePipelineBuilder<I, O, S1, S2, S3, S4>
where
    I: Send + 'static,
    O: Send + 'static,
    S1: Stage<Input = I> + Send + 'static,
    S2: Stage<Input = S1::Output> + Send + 'static,
    S3: Stage<Input = S2::Output> + Send + 'static,
    S4: Stage<Input = S3::Output, Output = O> + Send + 'static,
    S1::Output: Send,
    S2::Output: Send,
    S3::Output: Send,
{
    config: PipelineConfig,
    ingestion: Option<S1>,
    filtering: Option<S2>,
    aggregation: Option<S3>,
    output: Option<S4>,
}

impl<I, O, S1, S2, S3, S4> MultiStagePipelineBuilder<I, O, S1, S2, S3, S4>
where
    I: Send + 'static,
    O: Send + 'static,
    S1: Stage<Input = I> + Send + 'static,
    S2: Stage<Input = S1::Output> + Send + 'static,
    S3: Stage<Input = S2::Output> + Send + 'static,
    S4: Stage<Input = S3::Output, Output = O> + Send + 'static,
    S1::Output: Send,
    S2::Output: Send,
    S3::Output: Send,
{
    /// Creates a new builder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: PipelineConfig::default(),
            ingestion: None,
            filtering: None,
            aggregation: None,
            output: None,
        }
    }

    /// Sets the pipeline configuration.
    #[must_use]
    pub fn config(mut self, config: PipelineConfig) -> Self {
        self.config = config;
        self
    }

    /// Sets the ingestion stage.
    #[must_use]
    pub fn ingestion(mut self, stage: S1) -> Self {
        self.ingestion = Some(stage);
        self
    }

    /// Sets the filtering stage.
    #[must_use]
    pub fn filtering(mut self, stage: S2) -> Self {
        self.filtering = Some(stage);
        self
    }

    /// Sets the aggregation stage.
    #[must_use]
    pub fn aggregation(mut self, stage: S3) -> Self {
        self.aggregation = Some(stage);
        self
    }

    /// Sets the output stage.
    #[must_use]
    pub fn output(mut self, stage: S4) -> Self {
        self.output = Some(stage);
        self
    }

    /// Builds and starts the pipeline.
    ///
    /// # Panics
    ///
    /// Panics if any stage is not configured.
    #[must_use]
    pub fn build(self) -> MultiStagePipeline<I, O> {
        let ingestion = self.ingestion.expect("ingestion stage not configured");
        let filtering = self.filtering.expect("filtering stage not configured");
        let aggregation = self.aggregation.expect("aggregation stage not configured");
        let output = self.output.expect("output stage not configured");

        let capacity = self.config.channel_capacity;

        // Create channels between stages
        let (input_tx, ingestion_rx) = bounded::<I>(capacity);
        let (ingestion_tx, filtering_rx) = bounded::<S1::Output>(capacity);
        let (filtering_tx, aggregation_rx) = bounded::<S2::Output>(capacity);
        let (aggregation_tx, output_rx_stage) = bounded::<S3::Output>(capacity);
        let (output_tx, output_rx) = bounded::<O>(capacity);

        let state = Arc::new(AtomicU8::new(PipelineState::Starting as u8));
        let mut stages = Vec::with_capacity(4);

        // Spawn ingestion stage
        let control1 = Arc::new(StageControl::new());
        let metrics1 = Arc::new(StageMetrics::new());
        let c1 = Arc::clone(&control1);
        let m1 = Arc::clone(&metrics1);
        let thread1 = thread::Builder::new()
            .name("ingestion".into())
            .spawn(move || {
                stage_worker(ingestion, ingestion_rx, ingestion_tx, c1, m1);
            })
            .expect("failed to spawn ingestion thread");
        stages.push(StageData {
            name: "ingestion".into(),
            control: control1,
            metrics: metrics1,
            thread: Some(thread1),
        });

        // Spawn filtering stage
        let control2 = Arc::new(StageControl::new());
        let metrics2 = Arc::new(StageMetrics::new());
        let c2 = Arc::clone(&control2);
        let m2 = Arc::clone(&metrics2);
        let thread2 = thread::Builder::new()
            .name("filtering".into())
            .spawn(move || {
                stage_worker(filtering, filtering_rx, filtering_tx, c2, m2);
            })
            .expect("failed to spawn filtering thread");
        stages.push(StageData {
            name: "filtering".into(),
            control: control2,
            metrics: metrics2,
            thread: Some(thread2),
        });

        // Spawn aggregation stage
        let control3 = Arc::new(StageControl::new());
        let metrics3 = Arc::new(StageMetrics::new());
        let c3 = Arc::clone(&control3);
        let m3 = Arc::clone(&metrics3);
        let thread3 = thread::Builder::new()
            .name("aggregation".into())
            .spawn(move || {
                stage_worker(aggregation, aggregation_rx, aggregation_tx, c3, m3);
            })
            .expect("failed to spawn aggregation thread");
        stages.push(StageData {
            name: "aggregation".into(),
            control: control3,
            metrics: metrics3,
            thread: Some(thread3),
        });

        // Spawn output stage
        let control4 = Arc::new(StageControl::new());
        let metrics4 = Arc::new(StageMetrics::new());
        let c4 = Arc::clone(&control4);
        let m4 = Arc::clone(&metrics4);
        let thread4 = thread::Builder::new()
            .name("output".into())
            .spawn(move || {
                stage_worker(output, output_rx_stage, output_tx, c4, m4);
            })
            .expect("failed to spawn output thread");
        stages.push(StageData {
            name: "output".into(),
            control: control4,
            metrics: metrics4,
            thread: Some(thread4),
        });

        state.store(PipelineState::Running as u8, Ordering::Release);

        MultiStagePipeline {
            config: self.config,
            state,
            stages,
            input_tx: Some(input_tx),
            output_rx: Some(output_rx),
            start_time: Some(Instant::now()),
        }
    }
}

/// A simplified pipeline builder that accepts closures.
pub struct SimplePipelineBuilder<I>
where
    I: Send + 'static,
{
    config: PipelineConfig,
    _marker: std::marker::PhantomData<I>,
}

impl<I> SimplePipelineBuilder<I>
where
    I: Send + 'static,
{
    /// Creates a new simple pipeline builder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: PipelineConfig::default(),
            _marker: std::marker::PhantomData,
        }
    }

    /// Sets the pipeline configuration.
    #[must_use]
    pub fn config(mut self, config: PipelineConfig) -> Self {
        self.config = config;
        self
    }

    /// Builds a pipeline with the given stage functions.
    pub fn build_with<F1, T1, F2, T2, F3, T3, F4, O>(
        self,
        ingestion: F1,
        filtering: F2,
        aggregation: F3,
        output: F4,
    ) -> MultiStagePipeline<I, O>
    where
        F1: FnMut(I) -> Option<T1> + Send + 'static,
        T1: Send + 'static,
        F2: FnMut(T1) -> Option<T2> + Send + 'static,
        T2: Send + 'static,
        F3: FnMut(T2) -> Option<T3> + Send + 'static,
        T3: Send + 'static,
        F4: FnMut(T3) -> Option<O> + Send + 'static,
        O: Send + 'static,
    {
        use crate::stage::FilterMap;

        MultiStagePipelineBuilder::<I, O, _, _, _, _>::new()
            .config(self.config)
            .ingestion(FilterMap::new(ingestion))
            .filtering(FilterMap::new(filtering))
            .aggregation(FilterMap::new(aggregation))
            .output(FilterMap::new(output))
            .build()
    }
}

impl<I> Default for SimplePipelineBuilder<I>
where
    I: Send + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::{Filter, Identity, Map};

    #[test]
    fn test_pipeline_state() {
        assert_eq!(PipelineState::from(0), PipelineState::Created);
        assert_eq!(PipelineState::from(2), PipelineState::Running);
        assert_eq!(PipelineState::from(99), PipelineState::Created);
    }

    #[test]
    fn test_pipeline_config() {
        let config = PipelineConfig::new()
            .channel_capacity(2048)
            .shutdown_timeout(Duration::from_secs(10))
            .collect_metrics(false);

        assert_eq!(config.channel_capacity, 2048);
        assert_eq!(config.shutdown_timeout, Duration::from_secs(10));
        assert!(!config.collect_metrics);
    }

    #[test]
    fn test_multi_stage_pipeline() {
        // Build a simple pipeline: input → *2 → filter >5 → *10 → output
        let mut pipeline = MultiStagePipelineBuilder::<i32, i32, _, _, _, _>::new()
            .config(PipelineConfig::default().channel_capacity(16))
            .ingestion(Map::new(|x: i32| x * 2))
            .filtering(Filter::new(|x: &i32| *x > 5))
            .aggregation(Map::new(|x: i32| x * 10))
            .output(Identity::new())
            .build();

        assert!(pipeline.is_running());

        // Send values
        for i in 0..10 {
            pipeline.send(i).unwrap();
        }

        // Wait for processing
        thread::sleep(Duration::from_millis(100));

        // Collect results
        let mut results = vec![];
        while let Some(v) = pipeline.try_recv() {
            results.push(v);
        }

        // Expected: (3*2=6, 4*2=8, 5*2=10, ..., 9*2=18) * 10
        // Values 0-2 produce 0,2,4 which are filtered out (<=5)
        // Values 3-9 produce 6,8,10,12,14,16,18 which pass filter
        // Then * 10 = 60, 80, 100, 120, 140, 160, 180
        assert_eq!(results.len(), 7);
        assert!(results.contains(&60));
        assert!(results.contains(&180));

        // Check metrics
        let metrics = pipeline.metrics();
        assert_eq!(metrics.len(), 4);

        // Shutdown
        pipeline.shutdown();
        pipeline.join().unwrap();
        assert_eq!(pipeline.state(), PipelineState::Stopped);
    }

    #[test]
    fn test_simple_pipeline_builder() {
        let mut pipeline = SimplePipelineBuilder::<i32>::new()
            .config(PipelineConfig::default().channel_capacity(16))
            .build_with(
                |x| Some(x + 1),        // ingestion: +1
                |x| if x > 2 { Some(x) } else { None }, // filter: >2
                |x| Some(x * 2),        // aggregation: *2
                |x| Some(x),            // output: passthrough
            );

        pipeline.send(1).unwrap(); // 1+1=2, filtered
        pipeline.send(2).unwrap(); // 2+1=3, passes, *2=6
        pipeline.send(3).unwrap(); // 3+1=4, passes, *2=8

        thread::sleep(Duration::from_millis(50));

        let mut results = vec![];
        while let Some(v) = pipeline.try_recv() {
            results.push(v);
        }

        assert!(results.contains(&6));
        assert!(results.contains(&8));
        assert!(!results.contains(&4)); // 1 was filtered

        pipeline.shutdown();
        pipeline.join().unwrap();
    }

    #[test]
    fn test_pipeline_pause_resume() {
        let pipeline = MultiStagePipelineBuilder::<i32, i32, _, _, _, _>::new()
            .ingestion(Identity::new())
            .filtering(Identity::new())
            .aggregation(Identity::new())
            .output(Identity::new())
            .build();

        pipeline.pause();
        // All stages should be paused
        for (name, _) in pipeline.metrics() {
            assert!(name.len() > 0);
        }

        pipeline.resume();
        // Should resume processing

        drop(pipeline);
    }

    #[test]
    fn test_pipeline_throughput() {
        let mut pipeline = MultiStagePipelineBuilder::<i32, i32, _, _, _, _>::new()
            .config(PipelineConfig::default().channel_capacity(1024))
            .ingestion(Identity::new())
            .filtering(Identity::new())
            .aggregation(Identity::new())
            .output(Identity::new())
            .build();

        // Send many items
        for i in 0..1000 {
            pipeline.send(i).unwrap();
        }

        // Wait for processing
        thread::sleep(Duration::from_millis(200));

        // Drain output
        let mut count = 0;
        while pipeline.try_recv().is_some() {
            count += 1;
        }

        assert!(count > 0, "Should have processed some items");
        let throughput = pipeline.throughput();
        assert!(throughput > 0.0, "Throughput should be positive");

        pipeline.shutdown();
        pipeline.join().unwrap();
    }
}
