# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - Unreleased

### Added

- **Lock-free SPSC ring buffer** with cache-line padding to prevent false sharing
  - Wait-free `push` and `pop` operations
  - Generic over element type and capacity (const generic)
  - 0.3ns push, 9ns pop in benchmarks

- **4-stage multi-threaded pipeline** (Ingestion → Filter → Aggregation → Output)
  - `MultiStagePipelineBuilder` for ergonomic pipeline construction
  - Per-stage dedicated threads connected via bounded channels
  - Adaptive shutdown with configurable timeout

- **Composable single-threaded pipeline** (`PipelineBuilder` / `PipelineRunner`)
  - `map`, `filter`, `then` combinators
  - Zero-allocation hot path with `Stage` trait

- **Zero-copy data sharing**
  - `ObjectPool<T>` — typed object pool with automatic return on drop
  - `BufferPool` — tiered byte-buffer pool (small / medium / large)
  - `SharedData<T>` — `Arc`-backed read-only shared reference

- **Adaptive backpressure**
  - `BackpressureStrategy` enum: `Block`, `Drop`, `Sample`
  - `AdaptiveController` with high/low watermarks and hysteresis
  - `RateLimiter` — token-bucket rate limiter

- **Metrics and observability**
  - `LatencyHistogram` backed by HDR histogram for accurate percentiles (p50/p95/p99/p999)
  - `JitterTracker` for inter-arrival time variance
  - `StageMetricsCollector` and `PipelineMetricsAggregator`
  - `Dashboard` and `StatusLine` for live terminal output

- **Sensor abstractions**
  - `ImuReading` with `Vec3` for accelerometer/gyroscope data
  - `LidarReading` point cloud type
  - `MockImu` sensor with configurable noise and sinusoidal motion
  - `TimestampSync` for multi-sensor temporal alignment

- **`no_std`-compatible core** — buffer, error, sensor, stage, and timestamp modules
  compile without the standard library; `std` feature gates channels, metrics, and pipeline

- **Examples**: `simple_imu`, `multi_sensor`, `full_pipeline`, `metrics_demo`, `benchmark_latency`

- **Benchmarks** using Criterion: ring buffer, pipeline stages, channel comparison, allocations

[Unreleased]: https://github.com/huecodes/sensor-bridge/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/huecodes/sensor-bridge/releases/tag/v0.1.0
