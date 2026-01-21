# Sensor Pipeline

A lock-free, real-time sensor data pipeline for robotics in Rust.

## Features

- Lock-free SPSC ring buffer (~50ns latency)
- Composable processing stages (filters, transforms, fusion)
- Zero allocations in hot paths
- Timestamp synchronization for multi-sensor fusion

## Quick Start

```rust
use sensor_pipeline::{buffer::RingBuffer, stage::MovingAverage, Stage};

let buffer: RingBuffer<f32, 1024> = RingBuffer::new();
let (producer, consumer) = buffer.split();

producer.push(1.0).unwrap();
assert_eq!(consumer.pop(), Some(1.0));
```

## Run Examples

```bash
cargo run --features std --example simple_imu
cargo run --features std --example benchmark_latency --release
```

## Run Tests

```bash
cargo test --features std
```

## License

MIT OR Apache-2.0
