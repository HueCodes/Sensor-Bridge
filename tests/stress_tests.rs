//! Stress tests for the sensor pipeline.
//!
//! These tests verify behavior under high load and extended operation.
//! Run with: cargo test --test stress_tests -- --ignored

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use sensor_bridge::backpressure::{AdaptiveController, RateLimiter, RateLimiterConfig};
use sensor_bridge::channel::bounded;
use sensor_bridge::pipeline::{MultiStagePipelineBuilder, PipelineConfig};
use sensor_bridge::stage::{Identity, Map};
use sensor_bridge::zero_copy::ObjectPool;

/// Test sustained high throughput over extended period.
#[test]
#[ignore]
fn stress_test_sustained_throughput() {
    let mut pipeline = MultiStagePipelineBuilder::<u64, u64, _, _, _, _>::new()
        .config(PipelineConfig::default().channel_capacity(8192))
        .ingestion(Identity::new())
        .filtering(Identity::new())
        .aggregation(Identity::new())
        .output(Identity::new())
        .build();

    let shutdown = Arc::new(AtomicBool::new(false));
    let produced = Arc::new(AtomicU64::new(0));
    let received = Arc::new(AtomicU64::new(0));

    let shutdown_clone = Arc::clone(&shutdown);
    let produced_clone = Arc::clone(&produced);
    let input = pipeline.input().cloned().unwrap();

    // Producer thread
    let producer = thread::spawn(move || {
        let mut seq = 0u64;
        while !shutdown_clone.load(Ordering::Relaxed) {
            if input.try_send(seq).is_ok() {
                seq += 1;
                produced_clone.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    // Consumer thread
    let shutdown_clone = Arc::clone(&shutdown);
    let received_clone = Arc::clone(&received);
    let output = pipeline.output().cloned().unwrap();

    let consumer = thread::spawn(move || {
        while !shutdown_clone.load(Ordering::Relaxed) {
            if output.try_recv().is_ok() {
                received_clone.fetch_add(1, Ordering::Relaxed);
            }
        }
        // Drain remaining
        while output.try_recv().is_ok() {
            received_clone.fetch_add(1, Ordering::Relaxed);
        }
    });

    // Run for 30 seconds
    let duration = Duration::from_secs(30);
    let start = Instant::now();
    let mut last_report = Instant::now();
    let mut last_received = 0u64;

    while start.elapsed() < duration {
        thread::sleep(Duration::from_secs(1));

        if last_report.elapsed() >= Duration::from_secs(5) {
            let current_received = received.load(Ordering::Relaxed);
            let throughput = (current_received - last_received) as f64 / 5.0;
            println!(
                "[{:>3}s] Throughput: {:.0} items/sec, Total: {}",
                start.elapsed().as_secs(),
                throughput,
                current_received
            );
            last_received = current_received;
            last_report = Instant::now();
        }
    }

    shutdown.store(true, Ordering::Relaxed);
    producer.join().unwrap();

    pipeline.shutdown();
    pipeline.join().unwrap();
    consumer.join().unwrap();

    let total_produced = produced.load(Ordering::Relaxed);
    let total_received = received.load(Ordering::Relaxed);
    let throughput = total_received as f64 / duration.as_secs_f64();

    println!("\n=== Stress Test Results ===");
    println!("Duration: {}s", duration.as_secs());
    println!("Produced: {}", total_produced);
    println!("Received: {}", total_received);
    println!("Throughput: {:.0} items/sec", throughput);
    println!(
        "Drop rate: {:.2}%",
        (1.0 - total_received as f64 / total_produced as f64) * 100.0
    );

    assert!(throughput > 10_000.0, "Throughput too low: {}", throughput);
}

/// Test behavior under 10x overload.
#[test]
#[ignore]
fn stress_test_overload_graceful_degradation() {
    let controller = Arc::new(
        AdaptiveController::builder()
            .high_water_mark(0.8)
            .low_water_mark(0.5)
            .hysteresis_threshold(5)
            .build(),
    );

    let mut pipeline = MultiStagePipelineBuilder::<u64, u64, _, _, _, _>::new()
        .config(PipelineConfig::default().channel_capacity(1024))
        .ingestion(Map::new(|x: u64| {
            // Simulate processing time
            std::thread::sleep(Duration::from_nanos(100));
            x
        }))
        .filtering(Identity::new())
        .aggregation(Identity::new())
        .output(Identity::new())
        .build();

    let shutdown = Arc::new(AtomicBool::new(false));
    let accepted = Arc::new(AtomicU64::new(0));
    let rejected = Arc::new(AtomicU64::new(0));

    let shutdown_clone = Arc::clone(&shutdown);
    let accepted_clone = Arc::clone(&accepted);
    let rejected_clone = Arc::clone(&rejected);
    let controller_clone = Arc::clone(&controller);
    let input = pipeline.input().cloned().unwrap();

    // Overload producer - tries to send at 10x normal rate
    let producer = thread::spawn(move || {
        let mut seq = 0u64;
        while !shutdown_clone.load(Ordering::Relaxed) {
            // Estimate queue utilization
            let utilization = input.len() as f32 / 1024.0;

            if controller_clone.should_accept(utilization) {
                if input.try_send(seq).is_ok() {
                    seq += 1;
                    accepted_clone.fetch_add(1, Ordering::Relaxed);
                } else {
                    rejected_clone.fetch_add(1, Ordering::Relaxed);
                }
            } else {
                rejected_clone.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    // Consumer
    let received = Arc::new(AtomicU64::new(0));
    let shutdown_clone = Arc::clone(&shutdown);
    let received_clone = Arc::clone(&received);
    let output = pipeline.output().cloned().unwrap();

    let consumer = thread::spawn(move || {
        while !shutdown_clone.load(Ordering::Relaxed) {
            if output.try_recv().is_ok() {
                received_clone.fetch_add(1, Ordering::Relaxed);
            }
        }
        while output.try_recv().is_ok() {
            received_clone.fetch_add(1, Ordering::Relaxed);
        }
    });

    // Run for 10 seconds
    thread::sleep(Duration::from_secs(10));

    shutdown.store(true, Ordering::Relaxed);
    producer.join().unwrap();
    pipeline.shutdown();
    pipeline.join().unwrap();
    consumer.join().unwrap();

    let total_accepted = accepted.load(Ordering::Relaxed);
    let total_rejected = rejected.load(Ordering::Relaxed);
    let total_received = received.load(Ordering::Relaxed);

    println!("\n=== Overload Test Results ===");
    println!("Accepted: {}", total_accepted);
    println!("Rejected by controller: {}", total_rejected);
    println!("Received: {}", total_received);
    println!(
        "Acceptance rate: {:.1}%",
        total_accepted as f64 / (total_accepted + total_rejected) as f64 * 100.0
    );
    println!("Controller quality: {:?}", controller.quality());

    // Under overload, we should have gracefully degraded
    assert!(total_received > 0, "Should have processed some items");
}

/// Test memory stability over extended operation.
#[test]
#[ignore]
fn stress_test_memory_stability() {
    let pool = ObjectPool::new(|| vec![0u8; 1024], 100);
    pool.prefill(100);

    let shutdown = Arc::new(AtomicBool::new(false));
    let iterations = Arc::new(AtomicU64::new(0));

    let mut handles = vec![];

    for _ in 0..4 {
        let pool = Arc::clone(&pool);
        let shutdown = Arc::clone(&shutdown);
        let iterations = Arc::clone(&iterations);

        handles.push(thread::spawn(move || {
            while !shutdown.load(Ordering::Relaxed) {
                let mut buf = pool.acquire();
                buf[0] = 42;
                iterations.fetch_add(1, Ordering::Relaxed);
                // Brief hold
                std::thread::sleep(Duration::from_nanos(100));
            }
        }));
    }

    // Run for 60 seconds
    let start = Instant::now();
    let duration = Duration::from_secs(60);

    while start.elapsed() < duration {
        thread::sleep(Duration::from_secs(10));
        println!(
            "[{:>3}s] Iterations: {}, Pool available: {}, Created: {}",
            start.elapsed().as_secs(),
            iterations.load(Ordering::Relaxed),
            pool.available(),
            pool.total_created()
        );
    }

    shutdown.store(true, Ordering::Relaxed);
    for h in handles {
        h.join().unwrap();
    }

    let total_iterations = iterations.load(Ordering::Relaxed);
    let total_created = pool.total_created();

    println!("\n=== Memory Stability Results ===");
    println!("Total iterations: {}", total_iterations);
    println!("Objects created: {}", total_created);
    println!(
        "Reuse rate: {:.1}%",
        (1.0 - total_created as f64 / total_iterations as f64) * 100.0
    );

    // With 100 pooled objects and 4 threads, should have very few allocations
    // Allow some slack for initial warmup
    assert!(
        total_created <= 200,
        "Too many allocations: {}, expected <= 200",
        total_created
    );
}

/// Test channel throughput under contention.
#[test]
#[ignore]
fn stress_test_channel_contention() {
    let (tx, rx) = bounded::<u64>(4096);
    let shutdown = Arc::new(AtomicBool::new(false));
    let sent = Arc::new(AtomicU64::new(0));
    let received = Arc::new(AtomicU64::new(0));

    let mut handles = vec![];

    // Multiple producers
    for _ in 0..4 {
        let tx = tx.clone();
        let shutdown = Arc::clone(&shutdown);
        let sent = Arc::clone(&sent);

        handles.push(thread::spawn(move || {
            let mut seq = 0u64;
            while !shutdown.load(Ordering::Relaxed) {
                if tx.try_send(seq).is_ok() {
                    seq += 1;
                    sent.fetch_add(1, Ordering::Relaxed);
                }
            }
        }));
    }

    // Multiple consumers
    for _ in 0..4 {
        let rx = rx.clone();
        let shutdown = Arc::clone(&shutdown);
        let received = Arc::clone(&received);

        handles.push(thread::spawn(move || {
            while !shutdown.load(Ordering::Relaxed) {
                if rx.try_recv().is_ok() {
                    received.fetch_add(1, Ordering::Relaxed);
                }
            }
            // Drain
            while rx.try_recv().is_ok() {
                received.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    // Run for 10 seconds
    thread::sleep(Duration::from_secs(10));
    shutdown.store(true, Ordering::Relaxed);

    drop(tx); // Close producer side

    for h in handles {
        h.join().unwrap();
    }

    let total_sent = sent.load(Ordering::Relaxed);
    let total_received = received.load(Ordering::Relaxed);
    let throughput = total_received as f64 / 10.0;

    println!("\n=== Channel Contention Results ===");
    println!("Sent: {}", total_sent);
    println!("Received: {}", total_received);
    println!("Throughput: {:.0} items/sec", throughput);

    // Should handle high throughput even with contention
    assert!(throughput > 100_000.0, "Throughput too low: {}", throughput);
}

/// Test rate limiter under burst traffic.
#[test]
#[ignore]
fn stress_test_rate_limiter_burst() {
    let limiter = RateLimiter::new(
        RateLimiterConfig::default()
            .capacity(1000)
            .initial_tokens(1000)
            .refill_rate(10_000.0), // 10k/sec
    );

    let mut acquired = 0u64;
    let mut rejected = 0u64;
    let start = Instant::now();
    let duration = Duration::from_secs(10);

    while start.elapsed() < duration {
        // Try to acquire at 100k/sec (10x rate)
        for _ in 0..1000 {
            if limiter.try_acquire() {
                acquired += 1;
            } else {
                rejected += 1;
            }
        }
        thread::sleep(Duration::from_millis(10));
    }

    let throughput = acquired as f64 / duration.as_secs_f64();
    let expected_rate = 10_000.0;

    println!("\n=== Rate Limiter Burst Results ===");
    println!("Acquired: {}", acquired);
    println!("Rejected: {}", rejected);
    println!(
        "Throughput: {:.0}/sec (target: {:.0}/sec)",
        throughput, expected_rate
    );
    println!("Rate accuracy: {:.1}%", throughput / expected_rate * 100.0);

    // Should be within 20% of target rate
    assert!(
        (throughput - expected_rate).abs() / expected_rate < 0.2,
        "Rate limiter not accurate: {}/sec vs {}/sec",
        throughput,
        expected_rate
    );
}
