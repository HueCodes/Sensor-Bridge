//! Full pipeline example demonstrating the complete sensor processing pipeline.
//!
//! This example creates a 4-stage pipeline:
//! 1. Ingestion: Receives raw sensor readings
//! 2. Filtering: Filters out invalid readings
//! 3. Aggregation: Computes running statistics
//! 4. Output: Formats results for display
//!
//! Run with: cargo run --example full_pipeline

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use sensor_pipeline::pipeline::{MultiStagePipelineBuilder, PipelineConfig};
use sensor_pipeline::stage::{Filter, Map};
use sensor_pipeline::metrics::PerformanceTargets;

/// A simulated sensor reading.
#[derive(Debug, Clone)]
struct SensorReading {
    timestamp_ns: u64,
    sensor_id: u32,
    value: f64,
}

/// Aggregated statistics.
#[derive(Debug, Clone)]
struct AggregatedStats {
    timestamp_ns: u64,
    sensor_id: u32,
    value: f64,
    count: u64,
    mean: f64,
}

/// Output record.
#[derive(Debug, Clone)]
struct OutputRecord {
    timestamp_ns: u64,
    sensor_id: u32,
    value: f64,
    mean: f64,
    status: String,
}

fn main() {
    println!("=== Sensor Pipeline Full Example ===\n");

    // Performance targets
    let targets = PerformanceTargets::default()
        .min_throughput(10_000.0)
        .max_p99_latency_ms(10.0)
        .max_jitter_ms(2.0);

    // Build the pipeline
    let config = PipelineConfig::default()
        .channel_capacity(1024)
        .shutdown_timeout(Duration::from_secs(5));

    // Aggregation state
    let count = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let sum = Arc::new(std::sync::atomic::AtomicU64::new(0));

    let count_clone = Arc::clone(&count);
    let sum_clone = Arc::clone(&sum);

    let mut pipeline = MultiStagePipelineBuilder::<SensorReading, OutputRecord, _, _, _, _>::new()
        .config(config)
        // Stage 1: Ingestion (pass through, could add normalization here)
        .ingestion(Map::new(|reading: SensorReading| {
            // Add ingestion timestamp if needed
            reading
        }))
        // Stage 2: Filtering (remove invalid readings)
        .filtering(Filter::new(|reading: &SensorReading| {
            // Filter out NaN, infinity, and out-of-range values
            reading.value.is_finite() && reading.value >= -1000.0 && reading.value <= 1000.0
        }))
        // Stage 3: Aggregation (compute statistics)
        .aggregation(Map::new(move |reading: SensorReading| {
            let c = count_clone.fetch_add(1, Ordering::Relaxed) + 1;
            // Store value as bits to use atomics
            let value_bits = (reading.value * 1000.0) as u64;
            let s = sum_clone.fetch_add(value_bits, Ordering::Relaxed) + value_bits;
            let mean = (s as f64 / 1000.0) / c as f64;

            AggregatedStats {
                timestamp_ns: reading.timestamp_ns,
                sensor_id: reading.sensor_id,
                value: reading.value,
                count: c,
                mean,
            }
        }))
        // Stage 4: Output (format for display)
        .output(Map::new(|stats: AggregatedStats| {
            let status = if stats.value > stats.mean * 1.5 {
                "HIGH"
            } else if stats.value < stats.mean * 0.5 {
                "LOW"
            } else {
                "NORMAL"
            };

            OutputRecord {
                timestamp_ns: stats.timestamp_ns,
                sensor_id: stats.sensor_id,
                value: stats.value,
                mean: stats.mean,
                status: status.to_string(),
            }
        }))
        .build();

    println!("Pipeline started with {} stages", 4);
    println!("Channel capacity: {}", 1024);
    println!("Performance targets:");
    println!("  - Throughput: >{:.0} items/sec", targets.min_throughput);
    println!("  - P99 Latency: <{:.0}us", targets.max_p99_latency_us);
    println!("  - Jitter: <{:.1}ms", targets.max_jitter_ms);
    println!();

    // Shutdown flag
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = Arc::clone(&shutdown);

    // Spawn producer thread
    let input_tx = pipeline.input().cloned();
    let producer = thread::spawn(move || {
        let mut sequence = 0u64;
        let start = Instant::now();

        while !shutdown_clone.load(Ordering::Relaxed) {
            if let Some(ref tx) = input_tx {
                let reading = SensorReading {
                    timestamp_ns: start.elapsed().as_nanos() as u64,
                    sensor_id: (sequence % 4) as u32,
                    value: 50.0 + (sequence as f64 * 0.1).sin() * 30.0,
                };

                if tx.try_send(reading).is_ok() {
                    sequence += 1;
                }
            }

            // Small delay to control rate
            if sequence % 100 == 0 {
                thread::sleep(Duration::from_micros(10));
            }
        }

        sequence
    });

    // Collect results for a while
    let start = Instant::now();
    let run_duration = Duration::from_secs(3);
    let mut output_count = 0u64;
    let mut last_output: Option<OutputRecord> = None;

    println!("Running for {} seconds...\n", run_duration.as_secs());

    while start.elapsed() < run_duration {
        if let Some(record) = pipeline.try_recv() {
            output_count += 1;
            last_output = Some(record);

            // Print every 10000th record
            if output_count % 10000 == 0 {
                if let Some(ref rec) = last_output {
                    println!(
                        "[{:>6}] Sensor {} | Value: {:>7.2} | Mean: {:>7.2} | Status: {}",
                        output_count, rec.sensor_id, rec.value, rec.mean, rec.status
                    );
                }
            }
        }
    }

    // Shutdown
    println!("\nShutting down...");
    shutdown.store(true, Ordering::Relaxed);
    let produced = producer.join().unwrap();

    pipeline.shutdown();
    pipeline.join().unwrap();

    // Final statistics
    println!("\n=== Results ===");
    println!("Items produced: {}", produced);
    println!("Items output:   {}", output_count);
    println!("Throughput:     {:.0} items/sec", pipeline.throughput());
    println!("Elapsed:        {:.2}s", pipeline.elapsed().as_secs_f64());

    println!("\nPer-stage metrics:");
    for (name, metrics) in pipeline.metrics() {
        println!(
            "  {}: in={} out={} filtered={} p99={:.1}us",
            name,
            metrics.input_count,
            metrics.output_count,
            metrics.filtered_count,
            metrics.latency.p99_us()
        );
    }

    if let Some(record) = last_output {
        println!("\nLast record:");
        println!("  Sensor:    {}", record.sensor_id);
        println!("  Value:     {:.2}", record.value);
        println!("  Mean:      {:.2}", record.mean);
        println!("  Status:    {}", record.status);
    }

    println!("\n=== Done ===");
}
