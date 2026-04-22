# Writing a new Sensor driver

sensor-bridge does not ship hardware-specific drivers in the Rust crate
itself — the dependency cost of cross-platform HAL crates would be
disproportionate to the value. Instead, a driver is whatever implements
the `Sensor` trait and pushes data into the pipeline. This guide walks
through what that means in practice.

## The `Sensor` trait

```rust
pub trait Sensor: Send {
    type Reading: Send;
    fn sample(&mut self) -> Result<Timestamped<Self::Reading>>;
    fn name(&self) -> &str;
    fn sample_rate_hz(&self) -> f64;
    fn is_ready(&self) -> bool { true }
    fn initialize(&mut self) -> Result<()> { Ok(()) }
    fn reset(&mut self) -> Result<()> { Ok(()) }
    fn shutdown(&mut self) {}
}
```

- `sample` must not block. If nothing is available, return
  `Err(PipelineError::SensorError(SensorError::Timeout))` — the runner
  treats that as "try again later" rather than a fatal error.
- `name` and `sample_rate_hz` are informational; they show up in
  metrics dashboards and logs.

## Three common driver shapes

### 1. Blocking hardware sync

For a sensor that already has a blocking API (I2C over an embedded HAL,
a serial port, a memory-mapped device), call into it from `sample` and
translate the HAL error into a `PipelineError`. Keep the call fast —
the pipeline's terminal stage is back-pressured by `sample_rate_hz`, so
an unexpected long blocking call will back up every upstream stage.

### 2. Background thread producer

If the sensor blocks for long-ish chunks (USB serial, SPI transfer,
etc.), spawn a thread in your `new` constructor that reads from the
hardware and pushes into a `crossbeam::channel`. `sample` does a
`try_recv` on the consumer half. Stop the thread on drop.

```rust
pub struct MyDriver {
    rx: crossbeam::channel::Receiver<MyReading>,
    stop: Arc<AtomicBool>,
    clock: MonotonicClock,
}

impl Drop for MyDriver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}
```

### 3. Tokio async task

Look at `src/drivers/udp_receiver.rs` — the UDP driver binds a socket,
spawns a tokio task that reads, deserializes and pushes readings into an
`mpsc::channel`, and exposes a synchronous `Sensor::sample` that
`try_recv`s from the channel. Abort the task on drop.

## Timestamping

Always timestamp in the driver, not further downstream. Use
`MonotonicClock::stamp`; the pipeline assumes `timestamp_ns` never
goes backwards. If the hardware supplies its own clock, prefer it only
if you can guarantee monotonicity; otherwise, let `MonotonicClock` own
the clock.

## Error handling

The crate's error hierarchy lives in `src/error.rs`. Extend it if your
driver has a failure mode the existing variants don't cover. Do not
invent a parallel error enum — the pipeline's error reporting assumes a
single type.

## Testing

- Spin up a loopback version of whatever transport you use (see
  `tests/network.rs` for the UDP/TCP pattern).
- Assert on the `Sensor::sample` output, not on internal channels — the
  trait is the contract.
- Use `tokio::time::timeout` in async tests rather than sleeps; CI
  hosts vary wildly in real-time behavior.

## Packaging

Drivers that depend on platform-specific crates (rppal, linux-embedded
hal, esp-idf-sys, ...) belong in a separate crate that depends on
sensor-bridge. That way sensor-bridge itself stays portable to every
target.
