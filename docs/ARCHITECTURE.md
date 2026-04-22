# Architecture

This document describes how sensor-bridge is put together: the thread
model, the memory layout of the core data structures, and the state
machine behind adaptive backpressure. It targets someone who wants to
extend the library or debug behavior that isn't explained by rustdoc
alone.

## Layering

```
                 user code
                     |
  +------------------+-------------------+
  |                                      |
  v                                      v
drivers (feature=network)             sensor::MockImu etc.
  UdpSensor / TcpSensor                (synchronous sensors)
  |                                      |
  +--------------- Sensor trait ---------+
                     |
                     v
              pipeline (std)
     MultiStagePipelineBuilder / runner
                     |
                     v
                 stage (no_std-safe)
   Stage trait, filters, fusion, transforms
                     |
                     v
                 sinks (std)
     CsvSink, UdpSink, TapSink, WsSink
```

All modules below `pipeline` are safe to use in `no_std`. The network
drivers, sinks and CLI live above it, gated by the `std` (implicit via
feature bundles) and `network` / `websocket` / `cli` features.

## Thread model

`MultiStagePipeline` spawns one OS thread per stage plus one thread for
the producer (outside the library) and one for the consumer. Stages are
connected via bounded `crossbeam::channel` endpoints with a fixed
capacity (`PipelineConfig::channel_capacity`).

```
    input channel              s1->s2 channel              s2->s3 channel
  ┌────────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐
  │ Producer   │─▶│ Ingest   │─▶│ Filter   │─▶│Aggregate │─▶│  Output  │─▶ consumer
  │ (caller)   │  │ (thread) │  │ (thread) │  │ (thread) │  │ (thread) │
  └────────────┘  └──────────┘  └──────────┘  └──────────┘  └──────────┘
                       ↑              ↑              ↑             ↑
                       └──────────────┴──────────────┴─────────────┘
                                 shutdown_flag (AtomicBool)
```

Each stage polls `try_recv` with a small park on backpressure; when the
caller flips `shutdown_flag`, every thread drains its in-flight work and
exits. `MultiStagePipeline::join` waits for all four threads with a
`shutdown_timeout` (default 5s).

UDP and TCP drivers add a tokio runtime. Their receiver task lives on
the runtime; decoded readings land in an `mpsc::channel` and the
synchronous `Sensor::sample` method drains it with `try_recv`. This
keeps the async boundary narrow — the async stack ends at the channel,
and the rest of the pipeline stays in synchronous OS-thread land.

## Memory layout

### Ring buffer

`RingBuffer<T, const N>` is a cache-line padded SPSC queue. Head and tail
counters sit in separate `CachePadded<AtomicU64>` cells so a producer
writing the tail does not invalidate the line holding the head on the
consumer's CPU.

```
  +------------------------+  <- 64B cache line
  | padding                |
  | head: AtomicU64        |
  | padding                |
  +------------------------+  <- 64B cache line
  | padding                |
  | tail: AtomicU64        |
  | padding                |
  +------------------------+
  | slots: [MaybeUninit<T>; N]
  +------------------------+
```

`push` does `store(Release)` on `tail` after writing the slot; `pop`
does `load(Acquire)` on `tail` before reading. That pairing is what
makes the queue wait-free without locks.

### Zero-copy pools

`ObjectPool<T>` and `BufferPool` keep a small free list of pre-allocated
instances. When a caller takes an entry, they get an opaque RAII handle
that returns the entry to the pool on drop. No allocation happens in the
hot path after warmup. `SharedData<T>` is a lightweight read-only
`Arc<T>` that skips interior mutability to avoid the `RwLock` / atomic
dance some shared-sensor-data types carry.

## Backpressure state machine

`AdaptiveController` tracks queue depth with a moving average and
transitions between three states:

```
                  depth >= high_water
               +--------------------------+
               |                          v
         +-----+----+                 +------+------+
         |  Normal  |                 | Backpressed |
         +----------+                 +------+------+
               ^                          |
               +--------------------------+
                  depth <= low_water
                  (hysteresis band)
```

The controller produces one of three strategies per decision tick:

- `Block` — caller stalls until depth drops below `low_water`. Safe but
  pushes latency upstream.
- `Drop` — reject the current item; preserves latency for everyone else.
  Drops are counted via `NetworkMetrics::packets_dropped` for network
  drivers or the pipeline's own `dropped` counter.
- `Sample` — drop every Nth item based on how far over `high_water` the
  queue is. Meant for sensors where you want to degrade rate instead of
  latency (e.g., 1 kHz IMU feeding a 100 Hz consumer).

Hysteresis between `high_water` and `low_water` prevents rapid
oscillation. Watermarks are set on construction and are not
runtime-tunable.

## Lifetimes and invariants

- `RingBuffer<T, N>`: exactly one `Producer` and one `Consumer` at a
  time, enforced by `split` taking `&mut`. Cloning is not supported;
  use `split_arc` if you need both sides on separate threads.
- `MultiStagePipeline`: must be `shutdown()` then `join()` before drop.
  A Drop guard prints a warning to stderr if you skip this — stage
  threads would otherwise linger past the pipeline's lifetime.
- `UdpSensor` / `TcpSensor`: the background tokio task is aborted on
  drop. In-flight packets in the mpsc channel are lost.
- `WsSink`: dropping the sink aborts the accept task; already-connected
  clients see the websocket close.

## Adding a new driver

The `Sensor` trait (`src/sensor/traits.rs`) is the only integration
point. A new driver just needs to:

1. Expose a type that implements `Sensor<Reading = T>`.
2. Make `sample()` non-blocking (return `Err(SensorError::Timeout)`
   when there's nothing ready — the pipeline treats this as a no-op).
3. Surface any long-running background work via a drop impl that stops
   it.

See `docs/DRIVERS.md` for a step-by-step example.
