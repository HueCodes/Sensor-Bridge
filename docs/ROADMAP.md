# Sensor-Bridge Roadmap

The 0.1.0 release shipped a lock-free pipeline core with mock sensors only. The
0.2 line turns the crate into a real-world sensor processing framework: real
network drivers, real DSP stages, real sinks, and a CLI that is demo-able in
under a minute without any hardware.

Consolidated from the earlier `PRODUCTIONIZE.md` and `PRODUCTIONIZE_V2.md`
planning docs.

## Status of 0.1.0

- Published on crates.io, CI green.
- Lock-free SPSC ring buffer; 4-stage pipeline (`src/pipeline/`).
- Zero-copy pools (`src/zero_copy/`), adaptive backpressure
  (`src/backpressure/`), HDR metrics (`src/metrics/`).
- `no_std`-capable core behind a `std` feature gate.
- Only mock sensors exist under `src/sensor/`. No network, no replay, no real
  hardware support.

## 0.2 — Real Sensors

### Phase 1 — Network ingestion (highest leverage)

- `network` feature gate; optional `tokio`, `serde`, `serde_json` deps.
- `src/drivers/udp_receiver.rs`: generic `UdpSensor<T: DeserializeOwned>`
  listening on a UDP port, implementing the existing `Sensor` trait.
- `src/drivers/tcp_receiver.rs`: length-prefixed framing with capped
  exponential-backoff reconnect.
- `scripts/mock_udp_sender.py`: realistic MPU-6050, DHT11 and HC-SR04 packets
  at configurable rate with gaussian noise.
- `examples/udp_demo.rs`: UDP receiver into a full 4-stage pipeline with the
  existing metrics dashboard.
- Network-level integration tests under `#[cfg(feature = "network")]`.

### Phase 2 — Signal processing stages

- `src/stage/filters.rs`: `MovingAverage<const N: usize>`,
  `Median<const N: usize>`, `ExponentialMovingAverage`.
- `src/stage/complementary.rs`: gyro high-pass + accel low-pass with
  configurable `alpha`.
- `src/stage/kalman.rs`: linear Kalman filter. Prefer a hand-rolled 6-state
  implementation; fall back to `nalgebra` under a `dsp` feature only if the
  math gets unwieldy.
- `examples/imu_fusion.rs`: complementary filter on simulated MPU-6050 data.

### Phase 3 — Sinks

- `src/sinks/csv_sink.rs`, `udp_sink.rs`, `tap_sink.rs` (non-blocking
  latest-N reader).
- `src/sinks/ws_sink.rs` behind `websocket = ["network", "tokio-tungstenite"]`.
- `static/dashboard.html`: single-file, dependency-free (CDN chart lib only)
  live view for IMU / distance / temperature streams.

### Phase 4 — CLI binary

- `src/bin/sensor-bridge.rs` gated behind a `cli` feature with `clap`.
- Subcommands: `demo`, `listen`, `record`, `replay`, `bench`.
- `demo` tries to spawn the Python mock sender as a child process, falling
  back to an in-process Rust mock so `cargo run -- demo` always works.

### Phase 5 — Hardening

- `docs/ARCHITECTURE.md` (thread model, memory layout, backpressure state
  machine).
- `docs/DRIVERS.md` (how to implement a new `Sensor`).
- `docs/PERFORMANCE.md` (benchmark methodology + current numbers, labeled by
  CPU class).
- `.github/ISSUE_TEMPLATE/` bug and feature templates.
- `CHANGELOG.md` `## [Unreleased]` section rolled up into a 0.2.0 entry at
  release time.
- README hero updated to surface real-sensor support with a link to
  `examples/udp_demo.rs`.

## Feature layout target

```toml
[features]
default = ["std"]
std = ["crossbeam/std", "parking_lot", "hdrhistogram"]
network = ["dep:tokio", "dep:serde", "dep:serde_json"]
websocket = ["network", "dep:tokio-tungstenite"]
dsp = ["dep:nalgebra"]
tracing = ["dep:tracing", "dep:tracing-subscriber"]
cli = ["dep:clap"]
full = ["std", "network", "websocket", "dsp", "tracing", "cli"]
```

The default feature set stays small. `cargo build` with no flags must stay
dependency-light and `no_std` must keep working.

## Non-goals for 0.2

- No crate version bump; no publish from this branch.
- No CI changes unless a new test genuinely needs a new job.
- No rewrite of ring buffer, pipeline core or metrics unless a real bug is
  found — the README performance claims depend on that code.
- No MSRV bump. Stay on 1.75.
- No new error hierarchy — extend `src/error.rs` if needed.
- No new `unsafe` without a `// SAFETY:` comment.

## Hardware notes (future phase)

The developer has on hand: MPU-6050 IMU (I2C), DHT11 temp/humidity, HC-SR04
ultrasonic, DS18B20 1-Wire temp, LM35 analog temp, assorted analog sensors,
SSD1306 OLEDs, ESP32 DevKitC boards, Heltec WiFi LoRa 32 V3, STM32 Nucleo,
NRF24L01+, Arduino Mega 2560, a 24MHz logic analyzer, and SD adapters.

ESP32 is the practical bridge: read sensors over I2C/GPIO, send JSON packets
over UDP/WiFi to the Rust host. That firmware is deferred — the Python mock
sender covers the entire demo path for 0.2.
