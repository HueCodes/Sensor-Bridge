//! Benchmarks for the SPSC ring buffer.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use sensor_pipeline::buffer::RingBuffer;
use std::sync::Arc;
use std::thread;

fn bench_push_pop_single_thread(c: &mut Criterion) {
    let mut group = c.benchmark_group("ring_buffer_single_thread");
    group.throughput(Throughput::Elements(1));

    group.bench_function("push", |b| {
        let buffer: RingBuffer<u64, 1024> = RingBuffer::new();
        let (producer, consumer) = buffer.split();

        b.iter(|| {
            producer.push(black_box(42u64)).ok();
            consumer.pop();
        });
    });

    group.bench_function("push_pop_batch_100", |b| {
        let buffer: RingBuffer<u64, 1024> = RingBuffer::new();
        let (producer, consumer) = buffer.split();

        b.iter(|| {
            for i in 0..100 {
                producer.push(black_box(i)).ok();
            }
            for _ in 0..100 {
                black_box(consumer.pop());
            }
        });
    });

    group.finish();
}

fn bench_push_pop_concurrent(c: &mut Criterion) {
    let mut group = c.benchmark_group("ring_buffer_concurrent");

    group.bench_function("producer_consumer_10k", |b| {
        b.iter_custom(|iters| {
            let mut total = std::time::Duration::ZERO;

            for _ in 0..iters {
                let buffer = Box::leak(Box::new(RingBuffer::<u64, 8192>::new()));
                let (producer, consumer) = buffer.split();
                let count = 10_000u64;

                let start = std::time::Instant::now();

                let producer_handle = thread::spawn(move || {
                    for i in 0..count {
                        while producer.push(i).is_err() {
                            thread::yield_now();
                        }
                    }
                });

                let consumer_handle = thread::spawn(move || {
                    let mut received = 0u64;
                    while received < count {
                        if consumer.pop().is_some() {
                            received += 1;
                        } else {
                            thread::yield_now();
                        }
                    }
                });

                producer_handle.join().unwrap();
                consumer_handle.join().unwrap();

                total += start.elapsed();
            }

            total
        });
    });

    group.finish();
}

fn bench_buffer_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("ring_buffer_sizes");
    group.throughput(Throughput::Elements(1000));

    macro_rules! bench_size {
        ($size:expr) => {
            group.bench_function(format!("size_{}", $size), |b| {
                let buffer: RingBuffer<u64, $size> = RingBuffer::new();
                let (producer, consumer) = buffer.split();

                b.iter(|| {
                    for i in 0..($size / 2) {
                        producer.push(i as u64).ok();
                    }
                    for _ in 0..($size / 2) {
                        black_box(consumer.pop());
                    }
                });
            });
        };
    }

    bench_size!(64);
    bench_size!(256);
    bench_size!(1024);
    bench_size!(4096);

    group.finish();
}

fn bench_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("ring_buffer_latency");

    group.bench_function("push_latency", |b| {
        let buffer: RingBuffer<u64, 1024> = RingBuffer::new();
        let (producer, _consumer) = buffer.split();

        b.iter(|| {
            producer.push(black_box(42u64)).ok();
        });
    });

    group.bench_function("pop_latency_empty", |b| {
        let buffer: RingBuffer<u64, 1024> = RingBuffer::new();
        let (_producer, consumer) = buffer.split();

        b.iter(|| {
            black_box(consumer.pop());
        });
    });

    group.bench_function("pop_latency_with_data", |b| {
        let buffer: RingBuffer<u64, 1024> = RingBuffer::new();
        let (producer, consumer) = buffer.split();

        // Keep buffer partially filled
        for i in 0..512 {
            producer.push(i).ok();
        }

        b.iter(|| {
            if let Some(v) = consumer.pop() {
                producer.push(v).ok();
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_push_pop_single_thread,
    bench_push_pop_concurrent,
    bench_buffer_sizes,
    bench_latency,
);

criterion_main!(benches);
