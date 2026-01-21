//! Integration tests for the sensor pipeline.

use sensor_pipeline::{
    buffer::RingBuffer,
    pipeline::{PipelineBuilder, PipelineRunner},
    sensor::{ImuReading, MockImu, NoiseConfig, Sensor, Vec3},
    stage::{Chain, ExponentialMovingAverage, Filter, Map, MovingAverage, Stage},
    timestamp::{MonotonicClock, Timestamped},
};

#[test]
fn test_full_pipeline_integration() {
    // Create buffer
    let buffer: RingBuffer<Timestamped<ImuReading>, 256> = RingBuffer::new();
    let (producer, consumer) = buffer.split();

    // Create mock sensor
    let mut imu = MockImu::new(100.0)
        .with_noise(NoiseConfig::none(), NoiseConfig::none())
        .with_base_accel(Vec3::new(0.0, 0.0, -9.81));

    // Create pipeline
    let pipeline = PipelineBuilder::new()
        .map(|ts: Timestamped<ImuReading>| ts.data.accel.z)
        .then(MovingAverage::<f32, 5>::new())
        .build();

    let mut runner = PipelineRunner::new(consumer, pipeline);

    // Generate and process samples
    for _ in 0..10 {
        let sample = imu.sample().expect("Sensor should produce sample");
        producer.push(sample).expect("Buffer should have space");
    }

    // Verify processing
    let mut outputs = Vec::new();
    runner.drain(|output| outputs.push(output));

    assert_eq!(outputs.len(), 10);

    // All outputs should be close to -9.81 (gravity)
    for output in outputs {
        assert!(
            (output - (-9.81)).abs() < 0.1,
            "Output {output} should be close to -9.81"
        );
    }
}

#[test]
fn test_pipeline_with_filtering() {
    let buffer: RingBuffer<i32, 64> = RingBuffer::new();
    let (producer, consumer) = buffer.split();

    // Pipeline that filters out negative numbers and doubles positives
    let pipeline = PipelineBuilder::new()
        .filter(|x: &i32| *x >= 0)
        .map(|x| x * 2)
        .build();

    let mut runner = PipelineRunner::new(consumer, pipeline);

    // Push mixed values
    for i in -5..=5 {
        producer.push(i).unwrap();
    }

    let mut outputs = Vec::new();
    runner.drain(|x| outputs.push(x));

    // Should only have doubled positive values (including 0)
    assert_eq!(outputs, vec![0, 2, 4, 6, 8, 10]);
    assert_eq!(runner.stats().processed_count, 11);
    assert_eq!(runner.stats().filtered_count, 5); // -5, -4, -3, -2, -1
}

#[test]
fn test_timestamp_preservation() {
    let clock = MonotonicClock::new();
    let buffer: RingBuffer<Timestamped<u32>, 64> = RingBuffer::new();
    let (producer, consumer) = buffer.split();

    // Pipeline that preserves timestamps
    let pipeline = PipelineBuilder::new()
        .map(|ts: Timestamped<u32>| (ts.timestamp_ns, ts.data * 2))
        .build();

    let mut runner = PipelineRunner::new(consumer, pipeline);

    // Push timestamped values
    let ts1 = clock.stamp(10u32);
    let ts2 = clock.stamp(20u32);

    producer.push(ts1).unwrap();
    producer.push(ts2).unwrap();

    // Verify timestamps are preserved
    let (out_ts1, val1) = runner.poll().unwrap();
    let (out_ts2, val2) = runner.poll().unwrap();

    assert_eq!(out_ts1, ts1.timestamp_ns);
    assert_eq!(val1, 20);
    assert_eq!(out_ts2, ts2.timestamp_ns);
    assert_eq!(val2, 40);

    // Timestamps should be monotonically increasing
    assert!(out_ts2 >= out_ts1);
}

#[test]
fn test_moving_average_convergence() {
    let mut filter: MovingAverage<f32, 10> = MovingAverage::new();

    // Feed constant value
    for _ in 0..100 {
        filter.process(5.0);
    }

    // Should converge to input value
    let result = filter.current().unwrap();
    assert!((result - 5.0).abs() < 0.001);
}

#[test]
fn test_ema_smoothing() {
    let mut ema: ExponentialMovingAverage<f32> = ExponentialMovingAverage::new(0.5);

    // First value is passed through
    assert_eq!(ema.process(10.0), Some(10.0));

    // Second value is averaged
    // EMA = 0.5 * 0 + 0.5 * 10 = 5
    let result = ema.process(0.0).unwrap();
    assert!((result - 5.0).abs() < 0.001);
}

#[test]
fn test_chain_composition() {
    let double = Map::new(|x: i32| x * 2);
    let add_one = Map::new(|x: i32| x + 1);
    let filter_big = Filter::new(|x: &i32| *x > 10);

    let mut chain = Chain::new(Chain::new(double, add_one), filter_big);

    // 5 -> 10 -> 11 -> passes filter
    assert_eq!(chain.process(5), Some(11));

    // 3 -> 6 -> 7 -> filtered out
    assert_eq!(chain.process(3), None);
}

#[test]
fn test_buffer_full_behavior() {
    // Small buffer to test full behavior
    let buffer: RingBuffer<u32, 4> = RingBuffer::new();
    let (producer, consumer) = buffer.split();

    // Fill buffer (capacity is N-1 = 3)
    assert!(producer.push(1).is_ok());
    assert!(producer.push(2).is_ok());
    assert!(producer.push(3).is_ok());

    // Buffer is now full
    assert!(producer.is_full());
    assert!(producer.push(4).is_err());

    // Pop one, should allow one more push
    assert_eq!(consumer.pop(), Some(1));
    assert!(producer.push(4).is_ok());

    // Verify order
    assert_eq!(consumer.pop(), Some(2));
    assert_eq!(consumer.pop(), Some(3));
    assert_eq!(consumer.pop(), Some(4));
    assert_eq!(consumer.pop(), None);
}

#[test]
fn test_vec3_operations() {
    let v1 = Vec3::new(1.0, 2.0, 3.0);
    let v2 = Vec3::new(4.0, 5.0, 6.0);

    // Addition
    let sum = v1 + v2;
    assert_eq!(sum, Vec3::new(5.0, 7.0, 9.0));

    // Magnitude of (3, 4, 0) should be 5
    let v3 = Vec3::new(3.0, 4.0, 0.0);
    assert!((v3.magnitude() - 5.0).abs() < 0.001);

    // Dot product
    let dot = v1.dot(&v2);
    // 1*4 + 2*5 + 3*6 = 4 + 10 + 18 = 32
    assert!((dot - 32.0).abs() < 0.001);
}

#[test]
fn test_imu_reading_structure() {
    let reading = ImuReading::from_arrays([1.0, 2.0, 3.0], [0.1, 0.2, 0.3]);

    assert_eq!(reading.accel_array(), [1.0, 2.0, 3.0]);
    assert_eq!(reading.gyro_array(), [0.1, 0.2, 0.3]);

    // Test magnitude
    let accel_mag = reading.accel_magnitude();
    // sqrt(1 + 4 + 9) = sqrt(14) ≈ 3.74
    assert!((accel_mag - 14.0f32.sqrt()).abs() < 0.001);
}

#[test]
fn test_sequence_numbers() {
    let clock = MonotonicClock::new();

    let ts1 = clock.stamp(1u32);
    let ts2 = clock.stamp(2u32);
    let ts3 = clock.stamp(3u32);

    assert_eq!(ts1.seq, 0);
    assert_eq!(ts2.seq, 1);
    assert_eq!(ts3.seq, 2);

    // Timestamps should be monotonic
    assert!(ts2.timestamp_ns >= ts1.timestamp_ns);
    assert!(ts3.timestamp_ns >= ts2.timestamp_ns);
}

#[test]
fn test_pipeline_stats() {
    let buffer: RingBuffer<i32, 64> = RingBuffer::new();
    let (producer, consumer) = buffer.split();

    let pipeline = PipelineBuilder::new()
        .filter(|x: &i32| *x % 2 == 0)
        .map(|x| x * 2)
        .build();

    let mut runner = PipelineRunner::new(consumer, pipeline);

    // Push 10 values (5 even, 5 odd)
    for i in 0..10 {
        producer.push(i).unwrap();
    }

    runner.drain(|_| {});

    let stats = runner.stats();
    assert_eq!(stats.processed_count, 10);
    assert_eq!(stats.output_count, 5); // Only even numbers
    assert_eq!(stats.filtered_count, 5); // Odd numbers filtered
}
