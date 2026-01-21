//! Simple IMU example demonstrating basic pipeline usage.
//!
//! This example shows:
//! - Creating a mock IMU sensor
//! - Setting up a processing pipeline with filters
//! - Running the pipeline in separate threads

use sensor_pipeline::{
    buffer::RingBuffer,
    pipeline::{PipelineBuilder, PipelineRunner},
    sensor::{MockImu, NoiseConfig, Sensor, Vec3},
    stage::MovingAverage,
    timestamp::Timestamped,
    ImuReading,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn main() {
    println!("=== Simple IMU Pipeline Example ===\n");

    // Create a ring buffer for IMU data
    // Using a leaked box to get 'static lifetime for thread spawning
    let buffer = Box::leak(Box::new(
        RingBuffer::<Timestamped<ImuReading>, 1024>::new(),
    ));
    let (producer, consumer) = buffer.split();

    // Shutdown flag
    let running = Arc::new(AtomicBool::new(true));
    let running_producer = Arc::clone(&running);
    let running_consumer = Arc::clone(&running);

    // Producer thread - simulates sensor driver
    let producer_handle = thread::spawn(move || {
        // Create a mock IMU at 100Hz with some noise
        let mut imu = MockImu::new(100.0)
            .with_name("mock_imu_0")
            .with_base_accel(Vec3::new(0.0, 0.0, -9.81)) // Gravity
            .with_noise(NoiseConfig::low(), NoiseConfig::low())
            .with_sinusoidal_motion(1.0, Vec3::new(0.5, 0.0, 0.0)); // 1Hz oscillation

        println!("Producer: Starting IMU at {}Hz", imu.sample_rate_hz());

        let mut sample_count = 0u64;
        while running_producer.load(Ordering::Relaxed) {
            match imu.sample() {
                Ok(reading) => {
                    if producer.push(reading).is_err() {
                        println!("Producer: Buffer full, dropping sample");
                    }
                    sample_count += 1;
                }
                Err(e) => {
                    println!("Producer: Sensor error: {e}");
                }
            }

            // Sleep to simulate sensor timing
            thread::sleep(Duration::from_millis(10)); // 100Hz
        }

        println!("Producer: Stopped after {sample_count} samples");
    });

    // Consumer thread - runs the processing pipeline
    let consumer_handle = thread::spawn(move || {
        // Build a processing pipeline:
        // 1. Extract acceleration magnitude
        // 2. Apply moving average filter
        // 3. Convert to readable format
        let pipeline = PipelineBuilder::new()
            .map(|ts: Timestamped<ImuReading>| {
                // Calculate total acceleration magnitude
                let mag = ts.data.accel.magnitude();
                (ts.timestamp_ns, ts.seq, mag)
            })
            .then(
                // Apply moving average to smooth the signal
                MovingAverageWrapper::new(),
            )
            .build();

        let mut runner = PipelineRunner::new(consumer, pipeline);

        println!("Consumer: Pipeline ready\n");
        println!("Time(ms)     Seq    Raw Accel(m/s²)  Filtered");
        println!("--------     ---    --------------  --------");

        let mut last_print = std::time::Instant::now();

        while running_consumer.load(Ordering::Relaxed) {
            if let Some((ts_ns, seq, raw, filtered)) = runner.poll() {
                // Print every 100ms
                if last_print.elapsed() >= Duration::from_millis(100) {
                    let ts_ms = ts_ns / 1_000_000;
                    println!("{ts_ms:8}     {seq:3}    {raw:14.3}  {filtered:8.3}");
                    last_print = std::time::Instant::now();
                }
            } else {
                thread::yield_now();
            }
        }

        let stats = runner.stats();
        println!("\nConsumer: Pipeline statistics:");
        println!("  Processed: {}", stats.processed_count);
        println!("  Output: {}", stats.output_count);
        println!("  Filtered: {}", stats.filtered_count);
    });

    // Run for 2 seconds
    println!("Running for 2 seconds...\n");
    thread::sleep(Duration::from_secs(2));

    // Signal shutdown
    running.store(false, Ordering::Relaxed);

    // Wait for threads
    producer_handle.join().expect("Producer thread panicked");
    consumer_handle.join().expect("Consumer thread panicked");

    println!("\n=== Done ===");
}

/// Wrapper to make MovingAverage work with our tuple type.
struct MovingAverageWrapper {
    filter: MovingAverage<f32, 10>,
}

impl MovingAverageWrapper {
    fn new() -> Self {
        Self {
            filter: MovingAverage::new(),
        }
    }
}

impl sensor_pipeline::stage::Stage for MovingAverageWrapper {
    type Input = (u64, u64, f32);
    type Output = (u64, u64, f32, f32);

    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        let (ts, seq, raw) = input;
        let filtered = self.filter.process(raw)?;
        Some((ts, seq, raw, filtered))
    }

    fn reset(&mut self) {
        self.filter.reset();
    }
}
