# Sensor-Bridge: Productionization Prompt

You are an expert Rust engineer helping transform **Sensor-Bridge** into a best-in-class open-source portfolio project. Work autonomously through all tasks below without asking for approval unless you encounter a **STOP** marker or a genuinely ambiguous destructive decision (e.g., deleting files, force-pushing, changing public API in a breaking way). For everything else: read, think, act, move on.

---

## Project Context

**Sensor-Bridge** is a Rust library for real-time sensor processing pipelines targeting robotics. Key capabilities:

- Lock-free SPSC ring buffer (0.3ns push, 9ns pop, cache-line padded)
- 4-stage pipeline: Ingestion → Filter → Aggregation → Output
- Zero-copy data sharing via object pooling and `Arc`
- Adaptive backpressure (block / drop / sample strategies)
- Per-stage metrics: latency histograms (HDR), throughput, jitter tracking, live dashboard
- `no_std`-compatible core with `std` feature gate for full capabilities

Performance numbers: >1M pipeline items/sec, 2.2B stage items/sec, ~20ns channel latency.

The codebase is at `/Users/hugh/Dev/projects/Sensor-Bridge`.

---

## Working Principles

- **Read before editing.** Always read a file fully before modifying it.
- **No unnecessary files.** Edit existing files rather than creating new ones unless structure demands it.
- **No over-engineering.** Only add what directly serves the tasks below.
- **Run `cargo check`, `cargo test`, and `cargo clippy -- -D warnings` after every significant change.** Fix all errors and warnings before moving to the next task.
- **Commit after each completed task group** with a clear message.
- **STOP and ask** only when: (1) a task says STOP, (2) a public API change would break existing examples/tests, or (3) you need credentials/tokens you don't have.

---

## Task Groups

### 1. Fix Metadata & Identity

**Goal:** The package metadata should be accurate and publishable to crates.io.

- [ ] In `Cargo.toml`: set `authors`, `repository`, `homepage`, `documentation` fields. Use the GitHub username `huecodes` and repo `huecodes/sensor-bridge`. Set `version = "0.1.0"`. Set `rust-version = "1.75"`.
- [ ] Confirm the crate name `sensor-pipeline` or rename it to `sensor-bridge` to match the repo. If renaming, update all internal `use sensor_pipeline::` paths across src, tests, examples, and benches. **STOP before renaming** — ask the user which name to keep.
- [ ] Add `keywords` and `categories` that maximize discoverability on crates.io.
- [ ] Add a `[badges]` section linking to CI.

---

### 2. README Overhaul

**Goal:** The README should be the best-in-class README for a Rust systems library.

- [ ] Merge the best elements of `README.md` and `README.draft.md` into a single polished `README.md`.
- [ ] Structure: Hero section with badges → one-line description → architecture diagram reference → feature bullets → performance table → quick start → installation → examples index → benchmarks → contributing → license.
- [ ] Badges to include: crates.io version, docs.rs, GitHub Actions CI, license, Rust MSRV.
- [ ] The `assets/images/pipeline.png` reference: check if the file exists. If not, note it as a TODO comment in the README (don't delete the reference). Do not generate images yourself.
- [ ] Remove `README.draft.md` after merging.
- [ ] Ensure all code snippets in the README compile (add `no_run` where needed).

---

### 3. Documentation

**Goal:** Every public item has rustdoc. `cargo doc --no-deps --open` should produce a useful, navigable site.

- [ ] Audit all `pub` items across `src/`. Add or improve doc comments where missing.
- [ ] The crate-level `lib.rs` doc comment should have a "Getting Started" example that actually compiles.
- [ ] Add `//! # Overview` sections to each module's `mod.rs`.
- [ ] Add `#[doc = include_str!("../README.md")]` to `lib.rs` so the README renders on docs.rs.
- [ ] Run `cargo doc --no-deps 2>&1` and fix all warnings.

---

### 4. Code Quality Pass

**Goal:** Zero clippy warnings, idiomatic Rust, no unsafe footguns.

- [ ] Run `cargo clippy --all-targets --all-features -- -D warnings`. Fix every warning.
- [ ] Run `cargo fmt --check`. Apply formatting.
- [ ] Check all `unsafe` blocks have a `// SAFETY:` comment explaining the invariant.
- [ ] Review `error.rs`: ensure `PipelineError` implements `std::error::Error`, has `Display`, and all variants have context.
- [ ] Add `#[must_use]` to builder methods and any function returning `Result`/`Option`.

---

### 5. Test Coverage

**Goal:** Comprehensive, fast, reliable tests.

- [ ] Read `tests/integration.rs` and `tests/stress_tests.rs`. Identify gaps.
- [ ] Ensure every public error path is tested.
- [ ] Add property-based tests using `proptest` for the ring buffer (fuzz push/pop ordering).
- [ ] Add a test that verifies backpressure strategies (block, drop, sample) behave correctly under load.
- [ ] Add a `loom`-based concurrency test for the SPSC ring buffer under `#[cfg(loom)]`.
- [ ] Run `cargo test --all-features` and ensure 100% pass rate.

---

### 6. Benchmark Hygiene

**Goal:** Benchmarks are reproducible, well-labeled, and tell a story.

- [ ] Read all files in `benches/`. Ensure every benchmark group has a clear name and uses `criterion::BenchmarkId` for parameterized runs.
- [ ] Add benchmark for end-to-end pipeline latency (single item, cold and warm).
- [ ] Add benchmark comparing backpressure strategies.
- [ ] Ensure `cargo bench` runs clean with no warnings.
- [ ] Update the performance table in README if benchmark results differ materially.

---

### 7. CI/CD Pipeline

**Goal:** Every push is validated automatically. The project should feel maintained.

Create `.github/workflows/ci.yml` with the following jobs (all run on `ubuntu-latest` and `macos-latest`):

- **test**: `cargo test --all-features`
- **clippy**: `cargo clippy --all-targets --all-features -- -D warnings`
- **fmt**: `cargo fmt --check`
- **doc**: `cargo doc --no-deps --all-features`
- **bench** (optional, only on push to main): `cargo bench -- --test` (runs benchmarks in test mode for smoke-check)
- **msrv**: test on Rust 1.75 using `actions-rs/toolchain`

Also create `.github/workflows/release.yml` that runs `cargo publish --dry-run` on tag pushes (don't actually publish — user controls that).

---

### 8. Examples Polish

**Goal:** Examples are runnable, realistic, and demonstrate the library's power.

- [ ] Read all files in `examples/`. Ensure each compiles and runs successfully (`cargo run --example <name>`).
- [ ] `simple_imu.rs`: Should be a clean 30-50 line "Hello World" that a robotics engineer would recognize. No noise.
- [ ] `multi_sensor.rs`: Should demonstrate sensor fusion — two input streams (IMU + GPS or IMU + lidar) being merged.
- [ ] `full_pipeline.rs`: Should demonstrate all 4 stages with realistic-looking sensor data, print throughput at the end.
- [ ] `metrics_demo.rs`: Should show the live dashboard and print a metrics summary.
- [ ] Add example comments explaining *why* choices are made (e.g., why SPSC, why object pooling).

---

### 9. CHANGELOG & Contributing

**Goal:** Project signals it is open to contributors.

- [ ] Create `CHANGELOG.md` following Keep a Changelog format. Add a `[0.1.0] - Unreleased` section summarizing what exists.
- [ ] Create `CONTRIBUTING.md` covering: how to run tests, how to run benchmarks, clippy/fmt requirements, PR process.
- [ ] Verify `LICENSE` file exists with MIT license text.

---

### 10. Final Polish

- [ ] Run the full check suite one last time: `cargo check && cargo test --all-features && cargo clippy --all-targets --all-features -- -D warnings && cargo doc --no-deps`.
- [ ] Ensure `cargo package --list` shows no unexpected files and no missing files.
- [ ] Commit everything with message: `chore: productionize for portfolio release`.

---

## Definition of Done

This project is portfolio-ready when:
1. `cargo test --all-features` passes with 0 failures
2. `cargo clippy -- -D warnings` is clean
3. `cargo doc --no-deps` produces docs with no warnings
4. README reads like a project you'd star on GitHub
5. CI badge would show green
6. A Rust robotics engineer could add this to their `Cargo.toml` and be productive in 5 minutes
