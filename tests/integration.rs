//! Integration tests for the sensor pipeline.
//!
//! These tests verify end-to-end functionality of the pipeline.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use sensor_pipeline::channel::bounded;
use sensor_pipeline::metrics::{PerformanceTargets, StageMetricsCollector};
use sensor_pipeline::pipeline::{MultiStagePipelineBuilder, PipelineConfig, PipelineState};
use sensor_pipeline::stage::{Filter, Identity, Map};

#[test]
fn test_pipeline_lifecycle() {
    let mut pipeline = MultiStagePipelineBuilder::<i32, i32, _, _, _, _>::new()
        .config(PipelineConfig::default().channel_capacity(16))
        .ingestion(Identity::new())
        .filtering(Identity::new())
        .aggregation(Identity::new())
        .output(Identity::new())
        .build();

    assert_eq!(pipeline.state(), PipelineState::Running);

    // Send some data
    for i in 0..10 {
        pipeline.send(i).unwrap();
    }

    // Wait for processing
    thread::sleep(Duration::from_millis(50));

    // Receive results
    let mut received = 0;
    while pipeline.try_recv().is_some() {
        received += 1;
    }
    assert_eq!(received, 10);

    // Shutdown
    pipeline.shutdown();
    pipeline.join().unwrap();

    assert_eq!(pipeline.state(), PipelineState::Stopped);
}

#[test]
fn test_pipeline_filtering() {
    let mut pipeline = MultiStagePipelineBuilder::<i32, i32, _, _, _, _>::new()
        .config(PipelineConfig::default().channel_capacity(16))
        .ingestion(Identity::new())
        .filtering(Filter::new(|x: &i32| *x > 0))
        .aggregation(Identity::new())
        .output(Identity::new())
        .build();

    // Send mixed positive and negative numbers
    for i in -5..=5 {
        pipeline.send(i).unwrap();
    }

    thread::sleep(Duration::from_millis(50));

    // Only positive numbers should pass
    let mut results = Vec::new();
    while let Some(v) = pipeline.try_recv() {
        results.push(v);
    }

    assert_eq!(results.len(), 5); // 1, 2, 3, 4, 5
    assert!(results.iter().all(|&x| x > 0));

    pipeline.shutdown();
    pipeline.join().unwrap();
}

#[test]
fn test_pipeline_transformation() {
    let mut pipeline = MultiStagePipelineBuilder::<i32, i32, _, _, _, _>::new()
        .config(PipelineConfig::default().channel_capacity(16))
        .ingestion(Map::new(|x: i32| x + 1))
        .filtering(Map::new(|x: i32| x * 2))
        .aggregation(Map::new(|x: i32| x - 1))
        .output(Identity::new())
        .build();

    // f(x) = ((x + 1) * 2) - 1 = 2x + 1
    for i in 0..5 {
        pipeline.send(i).unwrap();
    }

    thread::sleep(Duration::from_millis(50));

    let mut results = Vec::new();
    while let Some(v) = pipeline.try_recv() {
        results.push(v);
    }

    assert_eq!(results.len(), 5);
    for (i, &v) in results.iter().enumerate() {
        assert_eq!(v, 2 * (i as i32) + 1);
    }

    pipeline.shutdown();
    pipeline.join().unwrap();
}

#[test]
fn test_channel_basic() {
    let (tx, rx) = bounded::<u32>(100);

    for i in 0..50 {
        tx.send(i).unwrap();
    }

    for i in 0..50 {
        assert_eq!(rx.recv(), Some(i));
    }

    assert_eq!(tx.metrics().sent(), 50);
    assert_eq!(rx.metrics().received(), 50);
}

#[test]
fn test_channel_threaded() {
    let (tx, rx) = bounded::<u64>(1024);
    let count = 50_000u64;

    let producer = thread::spawn(move || {
        for i in 0..count {
            tx.send(i).unwrap();
        }
    });

    let consumer = thread::spawn(move || {
        let mut received = 0u64;
        let mut expected = 0u64;
        while expected < count {
            if let Some(v) = rx.recv() {
                assert_eq!(v, expected);
                expected += 1;
                received += 1;
            }
        }
        received
    });

    producer.join().unwrap();
    let received = consumer.join().unwrap();
    assert_eq!(received, count);
}

#[test]
fn test_performance_targets() {
    let targets = PerformanceTargets::default()
        .min_throughput(10_000.0)
        .max_p99_latency_ms(10.0)
        .max_jitter_ms(2.0);

    assert_eq!(targets.min_throughput, 10_000.0);
    assert_eq!(targets.max_p99_latency_us, 10_000.0);
    assert_eq!(targets.max_jitter_ms, 2.0);
}

#[test]
fn test_metrics_aggregator() {
    let mut aggregator = sensor_pipeline::metrics::PipelineMetricsAggregator::new();

    let stage = Arc::new(StageMetricsCollector::new("test_stage"));
    aggregator.add_stage(Arc::clone(&stage));

    // Simulate processing
    for _ in 0..100 {
        aggregator.record_input();
        stage.record_input();
        stage.record_output(1000);
        aggregator.record_output(1000);
    }

    let snapshot = aggregator.snapshot();
    assert_eq!(snapshot.total_input, 100);
    assert_eq!(snapshot.total_output, 100);
    assert_eq!(snapshot.stages.len(), 1);
    assert_eq!(snapshot.stages[0].output_count, 100);
}

#[test]
fn test_object_pool() {
    use sensor_pipeline::zero_copy::ObjectPool;

    let pool = ObjectPool::new(|| Vec::<u8>::with_capacity(256), 10);
    pool.prefill(10);

    assert_eq!(pool.available(), 10);

    {
        let mut buf = pool.acquire();
        buf.extend_from_slice(b"test");
        assert_eq!(&*buf, b"test");
    }

    // Buffer returned to pool
    assert_eq!(pool.available(), 10);
    assert_eq!(pool.acquired(), 0);
}

#[test]
fn test_buffer_pool() {
    use sensor_pipeline::zero_copy::BufferPool;

    let pool = BufferPool::new(10);
    pool.prefill(5, 3, 1);

    assert_eq!(pool.available_small(), 5);
    assert_eq!(pool.available_medium(), 3);
    assert_eq!(pool.available_large(), 1);

    {
        let mut buf = pool.acquire(100);
        buf.extend_from_slice(b"sensor data");
    }

    // Buffer returned
    assert_eq!(pool.available_small(), 5);
}

#[test]
fn test_shared_data() {
    use sensor_pipeline::zero_copy::SharedData;

    let data = SharedData::new(vec![1, 2, 3, 4, 5]);
    let data2 = data.clone();

    assert_eq!(&*data, &*data2);
    assert_eq!(data.ref_count(), 2);

    drop(data2);
    assert_eq!(data.ref_count(), 1);
    assert!(data.is_unique());
}

#[test]
fn test_rate_limiter() {
    use sensor_pipeline::backpressure::{RateLimiter, RateLimiterConfig};

    let limiter = RateLimiter::new(
        RateLimiterConfig::default()
            .capacity(10)
            .initial_tokens(10)
            .refill_rate(0.0), // No refill for deterministic test
    );

    // Should allow 10 acquisitions
    for _ in 0..10 {
        assert!(limiter.try_acquire());
    }

    // Should reject
    assert!(!limiter.try_acquire());

    assert_eq!(limiter.acquired_count(), 10);
    assert_eq!(limiter.rejected_count(), 1);
}

#[test]
fn test_adaptive_controller() {
    use sensor_pipeline::backpressure::{AdaptiveController, QualityLevel};

    let controller = AdaptiveController::builder()
        .high_water_mark(0.8)
        .low_water_mark(0.5)
        .hysteresis_threshold(5)
        .build();

    assert_eq!(controller.quality(), QualityLevel::Full);

    // Simulate high load
    for _ in 0..10 {
        controller.update(0.9);
    }

    assert!(controller.quality() > QualityLevel::Full);

    // Reset
    controller.reset();
    assert_eq!(controller.quality(), QualityLevel::Full);
}

#[test]
fn test_jitter_tracker() {
    use sensor_pipeline::metrics::JitterTracker;

    let tracker = JitterTracker::new();

    // Record some values
    for v in [100, 110, 90, 105, 95] {
        tracker.record(v);
    }

    assert_eq!(tracker.count(), 5);
    assert!((tracker.mean() - 100.0).abs() < 1.0);
    assert!(tracker.std_dev() > 0.0);
    assert_eq!(tracker.min(), Some(90));
    assert_eq!(tracker.max(), Some(110));
}
