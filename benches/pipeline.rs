//! Benchmarks for the sensor pipeline.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use sensor_pipeline::{
    buffer::RingBuffer,
    pipeline::{PipelineBuilder, PipelineRunner},
    sensor::{ImuReading, Vec3},
    stage::{ExponentialMovingAverage, KalmanFilter1D, Map, MovingAverage, Stage},
    timestamp::Timestamped,
};

fn bench_stage_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("stage_processing");
    group.throughput(Throughput::Elements(1));

    group.bench_function("map_stage", |b| {
        let mut stage = Map::new(|x: i32| x * 2);
        b.iter(|| {
            black_box(stage.process(black_box(42)));
        });
    });

    group.bench_function("moving_average_10", |b| {
        let mut stage: MovingAverage<f32, 10> = MovingAverage::new();
        b.iter(|| {
            black_box(stage.process(black_box(1.0f32)));
        });
    });

    group.bench_function("ema", |b| {
        let mut stage: ExponentialMovingAverage<f32> = ExponentialMovingAverage::new(0.1);
        b.iter(|| {
            black_box(stage.process(black_box(1.0f32)));
        });
    });

    group.bench_function("kalman_1d", |b| {
        let mut stage = KalmanFilter1D::new(0.0, 1.0, 0.01, 0.1);
        b.iter(|| {
            black_box(stage.process(black_box(1.0f64)));
        });
    });

    group.finish();
}

fn bench_pipeline_chain(c: &mut Criterion) {
    let mut group = c.benchmark_group("pipeline_chain");
    group.throughput(Throughput::Elements(1));

    group.bench_function("chain_3_maps", |b| {
        let mut pipeline = PipelineBuilder::new()
            .map(|x: i32| x * 2)
            .map(|x| x + 1)
            .map(|x| x * 3)
            .build();

        b.iter(|| {
            black_box(pipeline.process(black_box(10)));
        });
    });

    group.bench_function("map_filter_map", |b| {
        let mut pipeline = PipelineBuilder::new()
            .map(|x: i32| x * 2)
            .filter(|x| *x > 10)
            .map(|x| x + 1)
            .build();

        b.iter(|| {
            black_box(pipeline.process(black_box(10)));
        });
    });

    group.bench_function("imu_processing_pipeline", |b| {
        let mut pipeline = PipelineBuilder::new()
            .map(|reading: ImuReading| reading.accel.magnitude())
            .then(MovingAverage::<f32, 10>::new())
            .map(|x| x * 9.81)
            .build();

        let reading = ImuReading::new(Vec3::new(0.1, 0.2, 0.97), Vec3::new(0.01, 0.02, 0.03));

        b.iter(|| {
            black_box(pipeline.process(black_box(reading)));
        });
    });

    group.finish();
}

fn bench_end_to_end_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("end_to_end");
    group.throughput(Throughput::Elements(1));

    group.bench_function("buffer_to_output", |b| {
        let buffer: RingBuffer<Timestamped<ImuReading>, 1024> = RingBuffer::new();
        let (producer, consumer) = buffer.split();

        let stage = PipelineBuilder::new()
            .map(|ts: Timestamped<ImuReading>| ts.data.accel.magnitude())
            .then(MovingAverage::<f32, 5>::new())
            .build();

        let mut runner = PipelineRunner::new(consumer, stage);

        let reading = ImuReading::new(Vec3::new(0.1, 0.2, 0.97), Vec3::new(0.01, 0.02, 0.03));

        b.iter(|| {
            let ts = Timestamped::new(reading, 0, 0);
            producer.push(ts).ok();
            black_box(runner.poll());
        });
    });

    group.bench_function("high_throughput_1000_items", |b| {
        let buffer: RingBuffer<i32, 2048> = RingBuffer::new();
        let (producer, consumer) = buffer.split();

        let stage = PipelineBuilder::new()
            .map(|x: i32| x * 2)
            .filter(|x| *x > 0)
            .map(|x| x + 1)
            .build();

        let mut runner = PipelineRunner::new(consumer, stage);

        b.iter(|| {
            // Push 1000 items
            for i in 0..1000 {
                producer.push(i).ok();
            }

            // Process all
            runner.drain(|x| {
                black_box(x);
            });
        });
    });

    group.finish();
}

fn bench_vec3_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("vec3_operations");
    group.throughput(Throughput::Elements(1));

    let v1 = Vec3::new(1.0, 2.0, 3.0);
    let v2 = Vec3::new(4.0, 5.0, 6.0);

    group.bench_function("add", |b| {
        b.iter(|| black_box(black_box(v1) + black_box(v2)));
    });

    group.bench_function("magnitude", |b| {
        b.iter(|| black_box(black_box(v1).magnitude()));
    });

    group.bench_function("dot", |b| {
        b.iter(|| black_box(black_box(v1).dot(&v2)));
    });

    group.bench_function("cross", |b| {
        b.iter(|| black_box(black_box(v1).cross(&v2)));
    });

    group.bench_function("normalize", |b| {
        b.iter(|| black_box(black_box(v1).normalized()));
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_stage_processing,
    bench_pipeline_chain,
    bench_end_to_end_latency,
    bench_vec3_operations,
);

criterion_main!(benches);
