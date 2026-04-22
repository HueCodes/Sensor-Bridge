# Performance

sensor-bridge is designed to keep the hot path lock-free and
allocation-free. This document explains how the numbers in the README
are produced and how to reproduce them locally.

## Benchmark methodology

All benches use [Criterion](https://bheisler.github.io/criterion.rs/).
Builds use `--release` with LTO and `codegen-units = 1` (see the
`[profile.bench]` section in `Cargo.toml`). Samples are warmed for
three seconds before the measurement phase.

Four bench binaries cover the main subsystems:

| Bench | Target | What it measures |
|-------|--------|------------------|
| `ring_buffer` | lock-free SPSC queue | push, pop, wraparound, cold vs warm |
| `pipeline` | multi-stage runner | end-to-end throughput across 4 stages |
| `channels` | crossbeam vs ring buffer | bounded-channel comparison |
| `allocations` | object / buffer pools | pool hit vs miss cost |

Run the full suite with:

```bash
just bench
# or
cargo bench
```

HTML reports land in `target/criterion/`.

## Reference numbers

Taken on an Apple M2 MacBook Air, macOS 15, Rust stable 1.89 at the
time of writing. These are the numbers the README quotes. Re-run locally
before trusting them — they are sensitive to CPU class, governor
settings, and background load.

| Metric | Result |
|--------|--------|
| Ring buffer push | ~0.3 ns |
| Ring buffer pop | ~9 ns |
| Channel latency | ~20 ns |
| Stage processing | ~2.2 B items/s |
| 4-stage pipeline throughput | >1 M items/s |

The large gap between single-stage throughput (2.2 B/s) and pipeline
throughput (1 M/s) is expected: the multi-stage pipeline crosses three
bounded channels plus three thread-to-thread synchronizations. Each
hop is cheap but not free.

## Tuning levers

- **`PipelineConfig::channel_capacity`** — bigger channels smooth out
  bursty producers at the cost of tail latency. Start at 1024 and bump
  only if `packets_dropped` is non-zero.
- **Stage granularity** — collapsing two cheap stages into one `Map`
  removes a channel and halves the lock-free handoff cost.
- **Backpressure strategy** — `Sample` is cheaper than `Block` in hot
  loops because it never parks a thread.
- **Zero-copy pools** — reuse `ObjectPool<T>` or `BufferPool` across
  iterations; the first allocation is on the slow path but every
  subsequent one is free-list pop.

## What not to optimize

- Don't chase single-stage throughput below ~1 ns — you're measuring
  CPU pipeline noise, not code. The `Stage` trait's virtual dispatch
  is already inlined under release builds.
- Don't replace the ring buffer with something fancier. The current
  implementation is in the paper-citation zone (see Vyukov,
  Morrison–Afek, etc.) and any swap needs a `loom` proof.

## When numbers drop unexpectedly

1. Run `cargo bench --bench ring_buffer` in isolation first. If this
   is slow, the rest will be.
2. Check CPU governor: `pmset -g ac` on macOS, `cpupower frequency-info`
   on Linux. Performance governors matter for microbenchmarks.
3. Make sure nothing else is hot-looping on your machine. The bench
   process sharing cores with a busy Chrome tab will swing p99 by 5-10x.

## Benchmarking a new stage

Add a new group to `benches/pipeline.rs`:

```rust
fn bench_my_stage(c: &mut Criterion) {
    let mut group = c.benchmark_group("stage_processing");
    group.bench_function("my_stage", |b| {
        let mut stage = MyStage::new();
        b.iter(|| {
            black_box(stage.process(black_box(42)));
        });
    });
    group.finish();
}
criterion_group!(benches, bench_my_stage);
criterion_main!(benches);
```

Keep the input construction outside `iter` so you measure the stage,
not setup.
