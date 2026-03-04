//! Pipeline execution and thread management.
//!
//! This module provides utilities for running pipelines in threads
//! and managing their lifecycle.

#[cfg(feature = "std")]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(feature = "std")]
use std::sync::Arc;
#[cfg(feature = "std")]
use std::thread::{self, JoinHandle};

use crate::buffer::Consumer;
use crate::stage::Stage;

/// Statistics from a pipeline run.
#[derive(Debug, Clone, Default)]
pub struct PipelineStats {
    /// Total number of items processed.
    pub processed_count: u64,
    /// Number of items that produced output.
    pub output_count: u64,
    /// Number of items filtered/dropped by stages.
    pub filtered_count: u64,
    /// Number of times the consumer found the buffer empty.
    pub empty_polls: u64,
}

impl PipelineStats {
    /// Returns the filter rate (filtered / processed).
    #[must_use]
    pub fn filter_rate(&self) -> f64 {
        if self.processed_count == 0 {
            0.0
        } else {
            self.filtered_count as f64 / self.processed_count as f64
        }
    }

    /// Returns the throughput (processed / second) given elapsed time.
    #[must_use]
    pub fn throughput(&self, elapsed_secs: f64) -> f64 {
        if elapsed_secs <= 0.0 {
            0.0
        } else {
            self.processed_count as f64 / elapsed_secs
        }
    }
}

/// A running pipeline that processes data from a ring buffer.
///
/// This is designed for single-threaded use within a consumer thread.
pub struct PipelineRunner<'a, T, S, const N: usize>
where
    S: Stage<Input = T>,
{
    consumer: Consumer<'a, T, N>,
    stage: S,
    stats: PipelineStats,
}

impl<'a, T, S, const N: usize> PipelineRunner<'a, T, S, N>
where
    S: Stage<Input = T>,
{
    /// Creates a new pipeline runner.
    #[must_use]
    pub fn new(consumer: Consumer<'a, T, N>, stage: S) -> Self {
        Self {
            consumer,
            stage,
            stats: PipelineStats::default(),
        }
    }

    /// Processes one item if available.
    ///
    /// Returns `Some(output)` if an item was processed and produced output,
    /// `None` if the buffer was empty or the item was filtered.
    pub fn poll(&mut self) -> Option<S::Output> {
        match self.consumer.pop() {
            Some(item) => {
                self.stats.processed_count += 1;
                match self.stage.process(item) {
                    Some(output) => {
                        self.stats.output_count += 1;
                        Some(output)
                    }
                    None => {
                        self.stats.filtered_count += 1;
                        None
                    }
                }
            }
            None => {
                self.stats.empty_polls += 1;
                None
            }
        }
    }

    /// Processes all available items, calling the callback for each output.
    ///
    /// Returns the number of items processed.
    pub fn drain<F>(&mut self, mut callback: F) -> usize
    where
        F: FnMut(S::Output),
    {
        let mut count = 0;
        while let Some(item) = self.consumer.pop() {
            self.stats.processed_count += 1;
            count += 1;

            if let Some(output) = self.stage.process(item) {
                self.stats.output_count += 1;
                callback(output);
            } else {
                self.stats.filtered_count += 1;
            }
        }
        count
    }

    /// Returns the current statistics.
    #[must_use]
    pub fn stats(&self) -> &PipelineStats {
        &self.stats
    }

    /// Resets the statistics.
    pub fn reset_stats(&mut self) {
        self.stats = PipelineStats::default();
    }

    /// Resets the stage state.
    pub fn reset_stage(&mut self) {
        self.stage.reset();
    }

    /// Returns a reference to the underlying stage.
    #[must_use]
    pub fn stage(&self) -> &S {
        &self.stage
    }

    /// Returns a mutable reference to the underlying stage.
    pub fn stage_mut(&mut self) -> &mut S {
        &mut self.stage
    }

    /// Calls tick on the stage and returns any output.
    pub fn tick(&mut self) -> Option<S::Output> {
        self.stage.tick()
    }
}

/// A handle to control a running pipeline thread.
#[cfg(feature = "std")]
pub struct PipelineHandle {
    /// Flag to signal the pipeline to stop.
    stop_flag: Arc<AtomicBool>,
    /// Thread join handle.
    thread: Option<JoinHandle<PipelineStats>>,
    /// Processed count (updated by worker thread).
    processed_count: Arc<AtomicU64>,
}

#[cfg(feature = "std")]
impl PipelineHandle {
    /// Signals the pipeline to stop.
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Release);
    }

    /// Waits for the pipeline thread to finish and returns statistics.
    ///
    /// # Panics
    ///
    /// Panics if the thread panicked.
    pub fn join(mut self) -> PipelineStats {
        if let Some(handle) = self.thread.take() {
            handle.join().expect("Pipeline thread panicked")
        } else {
            PipelineStats::default()
        }
    }

    /// Returns `true` if the stop flag has been set.
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.stop_flag.load(Ordering::Acquire)
    }

    /// Returns the current processed count (approximate).
    #[must_use]
    pub fn processed_count(&self) -> u64 {
        self.processed_count.load(Ordering::Relaxed)
    }
}

#[cfg(feature = "std")]
impl Drop for PipelineHandle {
    fn drop(&mut self) {
        self.stop();
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

/// Spawns a pipeline processing thread.
///
/// The thread will continuously poll the consumer and process items
/// until the stop flag is set.
///
/// # Arguments
///
/// * `consumer` - The ring buffer consumer
/// * `stage` - The processing stage
/// * `callback` - Called for each output produced
///
/// # Returns
///
/// A handle to control the pipeline thread.
#[cfg(feature = "std")]
pub fn spawn_pipeline<T, S, F, O, const N: usize>(
    consumer: Consumer<'static, T, N>,
    stage: S,
    mut callback: F,
) -> PipelineHandle
where
    T: Send + 'static,
    S: Stage<Input = T, Output = O> + Send + 'static,
    F: FnMut(O) + Send + 'static,
    O: Send,
{
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_clone = Arc::clone(&stop_flag);

    let processed_count = Arc::new(AtomicU64::new(0));
    let processed_count_clone = Arc::clone(&processed_count);

    let thread = thread::spawn(move || {
        let mut runner = PipelineRunner::new(consumer, stage);

        while !stop_flag_clone.load(Ordering::Acquire) {
            if let Some(output) = runner.poll() {
                callback(output);
            } else {
                // No data available, yield to avoid spinning
                thread::yield_now();
            }

            // Periodically update the shared counter
            if runner.stats().processed_count % 1000 == 0 {
                processed_count_clone.store(runner.stats().processed_count, Ordering::Relaxed);
            }
        }

        // Final update
        processed_count_clone.store(runner.stats().processed_count, Ordering::Relaxed);

        runner.stats().clone()
    });

    PipelineHandle {
        stop_flag,
        thread: Some(thread),
        processed_count,
    }
}

/// Options for batch processing.
#[derive(Debug, Clone)]
pub struct BatchOptions {
    /// Maximum items to process per batch.
    pub max_batch_size: usize,
    /// Whether to yield after each batch.
    pub yield_between_batches: bool,
}

impl Default for BatchOptions {
    fn default() -> Self {
        Self {
            max_batch_size: 100,
            yield_between_batches: true,
        }
    }
}

impl BatchOptions {
    /// Creates options for high-throughput processing.
    #[must_use]
    pub fn high_throughput() -> Self {
        Self {
            max_batch_size: 1000,
            yield_between_batches: false,
        }
    }

    /// Creates options for low-latency processing.
    #[must_use]
    pub fn low_latency() -> Self {
        Self {
            max_batch_size: 1,
            yield_between_batches: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::RingBuffer;
    use crate::stage::Map;

    #[test]
    fn test_pipeline_runner_basic() {
        let buffer: RingBuffer<i32, 16> = RingBuffer::new();
        let (producer, consumer) = buffer.split();

        producer.push(1).unwrap();
        producer.push(2).unwrap();
        producer.push(3).unwrap();

        let mut runner = PipelineRunner::new(consumer, Map::new(|x: i32| x * 2));

        assert_eq!(runner.poll(), Some(2));
        assert_eq!(runner.poll(), Some(4));
        assert_eq!(runner.poll(), Some(6));
        assert_eq!(runner.poll(), None); // Empty

        assert_eq!(runner.stats().processed_count, 3);
        assert_eq!(runner.stats().output_count, 3);
        assert_eq!(runner.stats().empty_polls, 1);
    }

    #[test]
    fn test_pipeline_runner_with_filter() {
        use crate::stage::{Chain, Filter};

        let buffer: RingBuffer<i32, 16> = RingBuffer::new();
        let (producer, consumer) = buffer.split();

        for i in 1..=5 {
            producer.push(i).unwrap();
        }

        let stage = Chain::new(
            Filter::new(|x: &i32| *x % 2 == 0),
            Map::new(|x: i32| x * 10),
        );

        let mut runner = PipelineRunner::new(consumer, stage);
        let mut outputs = Vec::new();

        runner.drain(|x| outputs.push(x));

        assert_eq!(outputs, vec![20, 40]); // Only even numbers
        assert_eq!(runner.stats().processed_count, 5);
        assert_eq!(runner.stats().output_count, 2);
        assert_eq!(runner.stats().filtered_count, 3);
    }

    #[test]
    fn test_pipeline_stats() {
        let stats = PipelineStats {
            processed_count: 100,
            output_count: 80,
            filtered_count: 20,
            empty_polls: 5,
        };

        assert!((stats.filter_rate() - 0.2).abs() < 0.01);
        assert!((stats.throughput(10.0) - 10.0).abs() < 0.01);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_spawn_pipeline() {
        use std::sync::atomic::AtomicI32;
        use std::time::Duration;

        // We need 'static lifetime, so use a leaked box
        let buffer = Box::leak(Box::new(RingBuffer::<i32, 256>::new()));
        let (producer, consumer) = buffer.split();

        let sum = Arc::new(AtomicI32::new(0));
        let sum_clone = Arc::clone(&sum);

        let handle = spawn_pipeline(consumer, Map::new(|x: i32| x * 2), move |x| {
            sum_clone.fetch_add(x, Ordering::SeqCst);
        });

        // Push some data
        for i in 1..=10 {
            producer.push(i).unwrap();
        }

        // Wait for processing
        thread::sleep(Duration::from_millis(50));

        handle.stop();
        let stats = handle.join();

        // sum of 2+4+6+...+20 = 2*(1+2+...+10) = 2*55 = 110
        assert_eq!(sum.load(Ordering::SeqCst), 110);
        assert_eq!(stats.processed_count, 10);
    }
}
