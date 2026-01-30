//! Benchmark comparing channel implementations.
//!
//! Compares throughput and latency between:
//! - std::sync::mpsc
//! - crossbeam bounded channels
//! - Our wrapped channels with metrics

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use std::sync::mpsc;
use std::thread;

fn bench_std_mpsc(c: &mut Criterion) {
    let mut group = c.benchmark_group("channels/std_mpsc");
    group.throughput(Throughput::Elements(10000));

    group.bench_function("send_recv_10k", |b| {
        b.iter(|| {
            let (tx, rx) = mpsc::channel::<u64>();

            let producer = thread::spawn(move || {
                for i in 0..10000u64 {
                    tx.send(i).unwrap();
                }
            });

            let consumer = thread::spawn(move || {
                let mut sum = 0u64;
                for _ in 0..10000 {
                    sum += rx.recv().unwrap();
                }
                sum
            });

            producer.join().unwrap();
            black_box(consumer.join().unwrap());
        });
    });

    group.finish();
}

fn bench_crossbeam_bounded(c: &mut Criterion) {
    let mut group = c.benchmark_group("channels/crossbeam_bounded");
    group.throughput(Throughput::Elements(10000));

    group.bench_function("send_recv_10k", |b| {
        b.iter(|| {
            let (tx, rx) = crossbeam::channel::bounded::<u64>(1024);

            let producer = thread::spawn(move || {
                for i in 0..10000u64 {
                    tx.send(i).unwrap();
                }
            });

            let consumer = thread::spawn(move || {
                let mut sum = 0u64;
                for _ in 0..10000 {
                    sum += rx.recv().unwrap();
                }
                sum
            });

            producer.join().unwrap();
            black_box(consumer.join().unwrap());
        });
    });

    group.finish();
}

fn bench_sensor_pipeline_channel(c: &mut Criterion) {
    let mut group = c.benchmark_group("channels/sensor_pipeline");
    group.throughput(Throughput::Elements(10000));

    group.bench_function("send_recv_10k", |b| {
        b.iter(|| {
            let (tx, rx) = sensor_pipeline::channel::bounded::<u64>(1024);

            let producer = thread::spawn(move || {
                for i in 0..10000u64 {
                    tx.send(i).unwrap();
                }
            });

            let consumer = thread::spawn(move || {
                let mut sum = 0u64;
                for _ in 0..10000 {
                    if let Some(v) = rx.recv() {
                        sum += v;
                    }
                }
                sum
            });

            producer.join().unwrap();
            black_box(consumer.join().unwrap());
        });
    });

    group.bench_function("try_send_recv_10k", |b| {
        b.iter(|| {
            let (tx, rx) = sensor_pipeline::channel::bounded::<u64>(1024);

            let producer = thread::spawn(move || {
                for i in 0..10000u64 {
                    while tx.try_send(i).is_err() {
                        thread::yield_now();
                    }
                }
            });

            let consumer = thread::spawn(move || {
                let mut sum = 0u64;
                let mut count = 0;
                while count < 10000 {
                    if let Ok(v) = rx.try_recv() {
                        sum += v;
                        count += 1;
                    } else {
                        thread::yield_now();
                    }
                }
                sum
            });

            producer.join().unwrap();
            black_box(consumer.join().unwrap());
        });
    });

    group.finish();
}

fn bench_channel_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("channels/latency");

    group.bench_function("crossbeam_single_item", |b| {
        let (tx, rx) = crossbeam::channel::bounded::<u64>(1);

        b.iter(|| {
            tx.send(42).unwrap();
            black_box(rx.recv().unwrap());
        });
    });

    group.bench_function("sensor_pipeline_single_item", |b| {
        let (tx, rx) = sensor_pipeline::channel::bounded::<u64>(1);

        b.iter(|| {
            tx.send(42).unwrap();
            black_box(rx.recv().unwrap());
        });
    });

    group.finish();
}

fn bench_channel_capacity(c: &mut Criterion) {
    let mut group = c.benchmark_group("channels/capacity");

    for capacity in [64, 256, 1024, 4096] {
        group.throughput(Throughput::Elements(10000));

        group.bench_function(format!("capacity_{}", capacity), |b| {
            b.iter(|| {
                let (tx, rx) = sensor_pipeline::channel::bounded::<u64>(capacity);

                let producer = thread::spawn(move || {
                    for i in 0..10000u64 {
                        tx.send(i).unwrap();
                    }
                });

                let consumer = thread::spawn(move || {
                    let mut sum = 0u64;
                    for _ in 0..10000 {
                        if let Some(v) = rx.recv() {
                            sum += v;
                        }
                    }
                    sum
                });

                producer.join().unwrap();
                black_box(consumer.join().unwrap());
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_std_mpsc,
    bench_crossbeam_bounded,
    bench_sensor_pipeline_channel,
    bench_channel_latency,
    bench_channel_capacity,
);

criterion_main!(benches);
