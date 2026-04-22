# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Network sensor drivers** behind a new `network` feature
  - `UdpSensor<T>` binds a tokio UDP socket, deserializes JSON datagrams
    and exposes them as a `Sensor`
  - `TcpSensor<T>` consumes length-prefixed JSON frames with capped
    exponential-backoff reconnect
  - Shared `NetworkMetrics` counters for packets / drops / parse errors /
    socket errors / reconnects
- **Signal-processing stages**
  - `ComplementaryFilter` — gyro high-pass + accel low-pass per axis
  - `KalmanFilter2D` — 2-state constant-velocity Kalman, closed-form
    covariance, no linear-algebra dep
- **Output sinks** (`src/sinks/`)
  - `CsvSink<T: CsvRow>`, `TapSink<T>`, `UdpSink`, `WsSink`
    (WebSocket broadcast, feature = `websocket`)
  - Single-file `static/dashboard.html` for a live browser view
- **CLI binary** `sensor-bridge` behind the new `cli` feature, with
  subcommands `demo`, `listen`, `record`, `replay`, `bench`
- **Examples**: `imu_fusion` (complementary filter on simulated
  MPU-6050), `udp_demo` (live network → pipeline → stats)
- **Python mock sender** (`scripts/mock_udp_sender.py`) — stdlib-only,
  simulates MPU-6050 + DHT11 + HC-SR04
- **Architecture / driver / performance docs** under `docs/`
- **GitHub issue templates** (bug, feature)
- **`justfile`** wrapping `check`, `lint`, `test`, `bench`, `demo`
- **Consolidated roadmap** at `docs/ROADMAP.md`

### Fixed

- `cargo build --no-default-features` now succeeds. Pulls in `libm` for
  `sqrt` / `sin_cos` / `ceil` and routes `String` through `alloc`
  for the sensor metadata types.

### Feature flags

`default = ["std"]`, plus opt-in `network`, `websocket`, `cli` and
`full` (everything except `dsp`, which is reserved for a future
`nalgebra`-backed Kalman).

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
