# Contributing to sensor-bridge

Thank you for your interest in contributing! This document covers how to get started.

## Prerequisites

- Rust 1.75 or later (`rustup update stable`)
- `cargo` in your `PATH`

## Running Tests

```bash
# Unit and integration tests
cargo test --all-features

# Stress tests (long-running, ignored by default)
cargo test --test stress_tests -- --ignored
```

## Running Benchmarks

```bash
# Full benchmark suite (outputs HTML reports in target/criterion/)
cargo bench

# Quick smoke-check (no timing, just compile/run)
cargo bench -- --test
```

## Code Style

This project enforces zero warnings on every push. Before submitting a PR:

```bash
# Format
cargo fmt

# Lint (must be warning-free)
cargo clippy --all-targets --all-features -- -D warnings

# Build docs
cargo doc --no-deps --all-features
```

## Unsafe Code

Any `unsafe` block must have a `// SAFETY:` comment explaining the invariant being
upheld. New `unsafe` blocks require a reviewer to sign off.

## Pull Request Process

1. Fork the repo and create a feature branch from `main`.
2. Make your changes, ensuring all tests pass and there are no Clippy warnings.
3. Add or update tests for your change.
4. Update `CHANGELOG.md` under `[Unreleased]` with a brief description.
5. Open a PR against `main`. The CI must be green before merging.

## Commit Messages

Use conventional commit style:

```
feat: add kalman filter stage
fix: correct cache-line padding on 32-bit targets
docs: add backpressure strategy examples
bench: add end-to-end latency benchmark
```

## Reporting Issues

Please open a GitHub issue with:
- Your Rust version (`rustc --version`)
- Your OS and architecture
- A minimal reproducible example if applicable
