//! Latency benchmarking example.
//!
//! Measures end-to-end latency through the pipeline and reports percentiles.

use sensor_bridge::{
    buffer::RingBuffer,
    metrics::LatencyHistogram,
    pipeline::{PipelineBuilder, PipelineRunner},
    sensor::{ImuReading, Vec3},
    stage::MovingAverage,
    timestamp::{MonotonicClock, Timestamped},
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

fn main() {
    println!("=== Pipeline Latency Benchmark ===\n");

    // Configuration
    let num_samples = 100_000;
    let buffer_size = 4096;

    println!("Configuration:");
    println!("  Samples: {num_samples}");
    println!("  Buffer size: {buffer_size}");
    println!();

    // Run different benchmark scenarios
    benchmark_raw_buffer();
    benchmark_simple_pipeline();
    benchmark_complex_pipeline();
    benchmark_concurrent();
}

fn benchmark_raw_buffer() {
    println!("--- Raw Ring Buffer (no processing) ---");

    let buffer: RingBuffer<u64, 4096> = RingBuffer::new();
    let (producer, consumer) = buffer.split();

    let histogram = LatencyHistogram::new();
    let clock = MonotonicClock::new();

    let iterations = 100_000;

    for _ in 0..iterations {
        let start = clock.now_ns();
        producer.push(start).ok();
        if let Some(ts) = consumer.pop() {
            let latency = clock.now_ns() - ts;
            histogram.record(latency);
        }
    }

    print_histogram_stats(&histogram);
    println!();
}

fn benchmark_simple_pipeline() {
    println!("--- Simple Pipeline (map only) ---");

    let buffer: RingBuffer<Timestamped<i32>, 4096> = RingBuffer::new();
    let (producer, consumer) = buffer.split();

    let pipeline = PipelineBuilder::new()
        .map(|ts: Timestamped<i32>| (ts.timestamp_ns, ts.data * 2))
        .build();

    let mut runner = PipelineRunner::new(consumer, pipeline);
    let histogram = LatencyHistogram::new();
    let clock = MonotonicClock::new();

    let iterations = 100_000;

    for i in 0..iterations {
        let ts = clock.stamp(i);
        producer.push(ts).ok();

        if let Some((start_ns, _)) = runner.poll() {
            let latency = clock.now_ns() - start_ns;
            histogram.record(latency);
        }
    }

    print_histogram_stats(&histogram);
    println!();
}

fn benchmark_complex_pipeline() {
    println!("--- Complex Pipeline (moving average + transforms) ---");

    let buffer: RingBuffer<Timestamped<ImuReading>, 4096> = RingBuffer::new();
    let (producer, consumer) = buffer.split();

    // Create a more realistic pipeline
    let pipeline = PipelineBuilder::new()
        .map(|ts: Timestamped<ImuReading>| {
            // Extract and transform
            let mag = ts.data.accel.magnitude();
            (ts.timestamp_ns, mag)
        })
        .then(LatencyTracker::new())
        .build();

    let mut runner = PipelineRunner::new(consumer, pipeline);
    let histogram = LatencyHistogram::new();
    let clock = MonotonicClock::new();

    let reading = ImuReading::new(Vec3::new(0.1, 0.2, 0.97), Vec3::new(0.01, 0.02, 0.03));

    let iterations = 100_000;

    for _ in 0..iterations {
        let ts = clock.stamp(reading);
        producer.push(ts).ok();

        if let Some((start_ns, _)) = runner.poll() {
            let latency = clock.now_ns() - start_ns;
            histogram.record(latency);
        }
    }

    print_histogram_stats(&histogram);
    println!();
}

fn benchmark_concurrent() {
    println!("--- Concurrent Producer/Consumer ---");

    let buffer = Box::leak(Box::new(RingBuffer::<Timestamped<u64>, 8192>::new()));
    let (producer, consumer) = buffer.split();

    let histogram = Arc::new(LatencyHistogram::new());
    let hist_clone = Arc::clone(&histogram);

    let running = Arc::new(AtomicBool::new(true));
    let running_producer = Arc::clone(&running);
    let running_consumer = Arc::clone(&running);

    let iterations = 100_000u64;

    // Producer
    let producer_handle = thread::spawn(move || {
        let clock = MonotonicClock::new();
        let mut count = 0u64;

        while count < iterations && running_producer.load(Ordering::Relaxed) {
            let ts = clock.stamp(count);
            while producer.push(ts).is_err() {
                thread::yield_now();
            }
            count += 1;
        }
    });

    // Consumer
    let consumer_handle = thread::spawn(move || {
        let clock = MonotonicClock::new();
        let mut received = 0u64;

        while received < iterations && running_consumer.load(Ordering::Relaxed) {
            if let Some(ts) = consumer.pop() {
                let latency = clock.now_ns().saturating_sub(ts.timestamp_ns);
                hist_clone.record(latency);
                received += 1;
            } else {
                thread::yield_now();
            }
        }
    });

    let start = Instant::now();
    producer_handle.join().unwrap();
    consumer_handle.join().unwrap();
    let elapsed = start.elapsed();

    running.store(false, Ordering::Relaxed);

    let throughput = iterations as f64 / elapsed.as_secs_f64();
    println!("Throughput: {throughput:.0} items/sec");

    print_histogram_stats(&histogram);
    println!();
}

fn print_histogram_stats(histogram: &LatencyHistogram) {
    let snap = histogram.snapshot();

    println!("  Count: {}", snap.count);
    println!("  Mean:  {:.1} ns ({:.3} µs)", snap.mean_ns, snap.mean_us());
    println!(
        "  Min:   {} ns",
        snap.min_ns.map_or("N/A".to_string(), |v| v.to_string())
    );
    println!(
        "  Max:   {} ns",
        snap.max_ns.map_or("N/A".to_string(), |v| v.to_string())
    );
    println!("  p50:   {} ns", snap.p50_ns);
    println!("  p95:   {} ns", snap.p95_ns);
    println!("  p99:   {} ns ({:.3} µs)", snap.p99_ns, snap.p99_us());
    println!("  p999:  {} ns", snap.p999_ns);
}

/// A stage that tracks latency but passes through data.
struct LatencyTracker {
    filter: MovingAverage<f32, 10>,
}

impl LatencyTracker {
    fn new() -> Self {
        Self {
            filter: MovingAverage::new(),
        }
    }
}

impl sensor_bridge::stage::Stage for LatencyTracker {
    type Input = (u64, f32);
    type Output = (u64, f32);

    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        let (ts, val) = input;
        let filtered = self.filter.process(val)?;
        Some((ts, filtered))
    }

    fn reset(&mut self) {
        self.filter.reset();
    }
}
