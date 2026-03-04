//! Metrics dashboard demonstration.
//!
//! This example demonstrates the live metrics dashboard with:
//! - Real-time throughput tracking
//! - Latency percentiles
//! - Jitter calculation
//! - Performance target checking
//!
//! Run with: cargo run --example metrics_demo

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use sensor_bridge::metrics::{
    Dashboard, DashboardConfig, PerformanceTargets, PipelineMetricsAggregator,
    StageMetricsCollector, StatusLine,
};

fn main() {
    println!("=== Metrics Dashboard Demo ===\n");
    println!("This demo simulates a sensor pipeline and displays live metrics.\n");

    // Create metrics aggregator with stages
    let mut aggregator = PipelineMetricsAggregator::new();

    let stage1 = Arc::new(StageMetricsCollector::new("ingestion"));
    let stage2 = Arc::new(StageMetricsCollector::new("filtering"));
    let stage3 = Arc::new(StageMetricsCollector::new("aggregation"));
    let stage4 = Arc::new(StageMetricsCollector::new("output"));

    aggregator.add_stage(Arc::clone(&stage1));
    aggregator.add_stage(Arc::clone(&stage2));
    aggregator.add_stage(Arc::clone(&stage3));
    aggregator.add_stage(Arc::clone(&stage4));

    let metrics = Arc::new(aggregator);

    // Define performance targets
    let targets = PerformanceTargets::default()
        .min_throughput(10_000.0)
        .max_p99_latency_ms(10.0)
        .max_jitter_ms(2.0);

    // Create dashboard (but don't start the background thread for this demo)
    let dashboard_config = DashboardConfig::new()
        .refresh_interval(Duration::from_millis(500))
        .show_stages(true)
        .show_targets(true)
        .targets(targets.clone())
        .clear_screen(true);

    let mut dashboard = Dashboard::new(Arc::clone(&metrics), dashboard_config);

    // Create status line for simple output
    let _status_line = StatusLine::new(Arc::clone(&metrics)).with_targets(targets);

    // Shutdown flag
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = Arc::clone(&shutdown);

    // Spawn a simulated producer
    let stage1_clone = Arc::clone(&stage1);
    let stage2_clone = Arc::clone(&stage2);
    let stage3_clone = Arc::clone(&stage3);
    let stage4_clone = Arc::clone(&stage4);
    let metrics_clone = Arc::clone(&metrics);

    let producer = thread::spawn(move || {
        let mut count = 0u64;
        let _start = Instant::now();

        while !shutdown_clone.load(Ordering::Relaxed) {
            // Simulate processing through stages
            let process_start = Instant::now();

            // Stage 1: Ingestion
            stage1_clone.record_input();
            thread::sleep(Duration::from_nanos(100));
            stage1_clone.record_output(process_start.elapsed().as_nanos() as u64);

            // Stage 2: Filtering (90% pass rate)
            stage2_clone.record_input();
            if count % 10 == 0 {
                stage2_clone.record_filtered();
            } else {
                thread::sleep(Duration::from_nanos(50));
                stage2_clone.record_output(process_start.elapsed().as_nanos() as u64);

                // Stage 3: Aggregation
                stage3_clone.record_input();
                thread::sleep(Duration::from_nanos(200));
                stage3_clone.record_output(process_start.elapsed().as_nanos() as u64);

                // Stage 4: Output
                stage4_clone.record_input();
                thread::sleep(Duration::from_nanos(50));
                stage4_clone.record_output(process_start.elapsed().as_nanos() as u64);

                // Record end-to-end latency
                let total_latency = process_start.elapsed().as_nanos() as u64;
                metrics_clone.record_input();
                metrics_clone.record_output(total_latency);
            }

            count += 1;

            // Vary the rate to create interesting patterns
            let rate_factor = 1.0 + (count as f64 / 1000.0).sin() * 0.5;
            let delay = Duration::from_micros((50.0 * rate_factor) as u64);
            thread::sleep(delay);
        }

        count
    });

    // Run for a few seconds, updating dashboard
    let run_duration = Duration::from_secs(5);
    let start = Instant::now();
    let mut update_count = 0;

    println!(
        "Running metrics demo for {} seconds...\n",
        run_duration.as_secs()
    );
    println!("Watch the metrics update in real-time:\n");

    while start.elapsed() < run_duration {
        // Render dashboard
        dashboard.render_once();
        update_count += 1;

        thread::sleep(Duration::from_millis(200));
    }

    // Shutdown producer
    shutdown.store(true, Ordering::Relaxed);
    let produced = producer.join().unwrap();

    // Clear screen and show final results
    print!("\x1B[2J\x1B[H");

    println!("=== Final Results ===\n");

    // Get final snapshot
    let snapshot = metrics.snapshot();

    println!("Pipeline Summary:");
    println!("  Total input:     {}", snapshot.total_input);
    println!("  Total output:    {}", snapshot.total_output);
    println!("  Throughput:      {:.0} items/sec", snapshot.throughput());
    println!("  Drop rate:       {:.2}%", snapshot.drop_rate() * 100.0);
    println!("  Elapsed:         {:.2}s", snapshot.elapsed_secs);
    println!();

    println!("Latency:");
    println!("  Mean:  {:.2} us", snapshot.end_to_end_latency.mean_us());
    println!("  P50:   {} ns", snapshot.end_to_end_latency.p50_ns);
    println!("  P95:   {} ns", snapshot.end_to_end_latency.p95_ns);
    println!("  P99:   {} ns", snapshot.end_to_end_latency.p99_ns);
    println!("  P999:  {} ns", snapshot.end_to_end_latency.p999_ns);
    println!();

    println!("Jitter:");
    println!(
        "  Std Dev: {:.2} us ({:.4} ms)",
        snapshot.end_to_end_jitter.std_dev_us(),
        snapshot.end_to_end_jitter.std_dev_ms()
    );
    println!();

    println!("Per-Stage Metrics:");
    for stage in &snapshot.stages {
        println!(
            "  {}: in={} out={} filtered={} p99={:.1}us jitter={:.1}us",
            stage.name,
            stage.input_count,
            stage.output_count,
            stage.filtered_count,
            stage.latency.p99_us(),
            stage.jitter.std_dev_us()
        );
    }
    println!();

    // Check against targets
    let check = snapshot.check_targets(&PerformanceTargets::default());
    println!("Performance Check:");
    println!(
        "  Throughput: {} ({:.0} items/sec >= 10000)",
        if check.throughput_ok { "PASS" } else { "FAIL" },
        check.actual_throughput
    );
    println!(
        "  Latency:    {} ({:.1}us <= 10000us)",
        if check.latency_ok { "PASS" } else { "FAIL" },
        check.actual_p99_latency_us
    );
    println!(
        "  Jitter:     {} ({:.2}ms <= 2ms)",
        if check.jitter_ok { "PASS" } else { "FAIL" },
        check.actual_jitter_ms
    );
    println!();

    println!(
        "Status: {}",
        if check.all_ok() {
            "ALL TARGETS MET"
        } else {
            "SOME TARGETS MISSED"
        }
    );
    println!();

    println!("=== Demo Complete ===");
    println!(
        "Produced {} items, updated dashboard {} times",
        produced, update_count
    );
}
