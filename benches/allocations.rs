//! Benchmark measuring allocations in the pipeline.
//!
//! Measures allocation overhead with and without object pooling.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use std::sync::Arc;

use sensor_bridge::zero_copy::{BufferPool, ObjectPool, SharedData};

/// A typical sensor reading structure.
#[derive(Clone, Default)]
struct SensorReading {
    timestamp: u64,
    sensor_id: u32,
    values: [f64; 6], // e.g., IMU with gyro + accel
    quality: u8,
}

fn bench_allocations_without_pool(c: &mut Criterion) {
    let mut group = c.benchmark_group("allocations/without_pool");
    group.throughput(Throughput::Elements(1000));

    group.bench_function("create_1000_readings", |b| {
        b.iter(|| {
            let mut readings = Vec::with_capacity(1000);
            for i in 0..1000u64 {
                readings.push(SensorReading {
                    timestamp: i,
                    sensor_id: (i % 4) as u32,
                    values: [0.0; 6],
                    quality: 100,
                });
            }
            black_box(readings);
        });
    });

    group.bench_function("box_1000_readings", |b| {
        b.iter(|| {
            let mut readings: Vec<Box<SensorReading>> = Vec::with_capacity(1000);
            for i in 0..1000u64 {
                readings.push(Box::new(SensorReading {
                    timestamp: i,
                    sensor_id: (i % 4) as u32,
                    values: [0.0; 6],
                    quality: 100,
                }));
            }
            black_box(readings);
        });
    });

    group.finish();
}

fn bench_allocations_with_object_pool(c: &mut Criterion) {
    let mut group = c.benchmark_group("allocations/with_object_pool");
    group.throughput(Throughput::Elements(1000));

    group.bench_function("acquire_release_1000", |b| {
        let pool = ObjectPool::with_reset(
            SensorReading::default,
            |r| {
                r.timestamp = 0;
                r.sensor_id = 0;
                r.values = [0.0; 6];
                r.quality = 0;
            },
            100,
        );
        pool.prefill(100);

        b.iter(|| {
            for i in 0..1000u64 {
                let mut reading = pool.acquire();
                reading.timestamp = i;
                reading.sensor_id = (i % 4) as u32;
                reading.quality = 100;
                black_box(&*reading);
            }
        });
    });

    group.bench_function("acquire_take_1000", |b| {
        let pool = ObjectPool::new(SensorReading::default, 1000);
        pool.prefill(1000);

        b.iter(|| {
            let mut readings = Vec::with_capacity(1000);
            for i in 0..1000 {
                let mut reading = pool.acquire();
                reading.timestamp = i;
                readings.push(reading.take());
            }
            black_box(readings);
        });
    });

    group.finish();
}

fn bench_allocations_with_buffer_pool(c: &mut Criterion) {
    let mut group = c.benchmark_group("allocations/with_buffer_pool");
    group.throughput(Throughput::Elements(1000));

    group.bench_function("acquire_release_1000_small", |b| {
        let pool = BufferPool::new(100);
        pool.prefill(100, 50, 10);

        b.iter(|| {
            for _ in 0..1000 {
                let mut buf = pool.acquire(100);
                buf.extend_from_slice(b"sensor data payload here");
                black_box(&*buf);
            }
        });
    });

    group.bench_function("acquire_release_1000_medium", |b| {
        let pool = BufferPool::new(100);
        pool.prefill(50, 100, 10);

        b.iter(|| {
            for _ in 0..1000 {
                let mut buf = pool.acquire(2000);
                buf.resize(2000, 0);
                black_box(&*buf);
            }
        });
    });

    group.finish();
}

fn bench_shared_data(c: &mut Criterion) {
    let mut group = c.benchmark_group("allocations/shared_data");
    group.throughput(Throughput::Elements(1000));

    group.bench_function("clone_1000", |b| {
        let data = SharedData::new(SensorReading::default());

        b.iter(|| {
            let mut clones = Vec::with_capacity(1000);
            for _ in 0..1000 {
                clones.push(data.clone());
            }
            black_box(clones);
        });
    });

    group.bench_function("arc_clone_1000", |b| {
        let data = Arc::new(SensorReading::default());

        b.iter(|| {
            let mut clones = Vec::with_capacity(1000);
            for _ in 0..1000 {
                clones.push(Arc::clone(&data));
            }
            black_box(clones);
        });
    });

    group.finish();
}

fn bench_allocation_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("allocations/comparison");
    group.throughput(Throughput::Elements(1000));

    // Baseline: direct allocation
    group.bench_function("vec_push_1000_boxes", |b| {
        b.iter(|| {
            let mut v: Vec<Box<[u8; 256]>> = Vec::with_capacity(1000);
            for _ in 0..1000 {
                v.push(Box::new([0u8; 256]));
            }
            black_box(v);
        });
    });

    // With object pool
    group.bench_function("pool_acquire_1000_buffers", |b| {
        let pool = ObjectPool::new(|| [0u8; 256], 100);
        pool.prefill(100);

        b.iter(|| {
            for _ in 0..1000 {
                let buf = pool.acquire();
                black_box(&*buf);
            }
        });
    });

    // Count allocations (approximate via reuse rate)
    group.bench_function("count_allocations_with_pool", |b| {
        let pool = ObjectPool::new(|| [0u8; 256], 50);
        pool.prefill(50);

        b.iter(|| {
            let before = pool.total_created();
            for _ in 0..1000 {
                let buf = pool.acquire();
                black_box(&*buf);
            }
            let after = pool.total_created();
            black_box(after - before);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_allocations_without_pool,
    bench_allocations_with_object_pool,
    bench_allocations_with_buffer_pool,
    bench_shared_data,
    bench_allocation_comparison,
);

criterion_main!(benches);
