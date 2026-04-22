set shell := ["bash", "-cu"]

# Default target: show the recipe list.
default:
    @just --list

# Formatting + typecheck + lint.
check:
    cargo fmt --all --check
    cargo check --all-features --all-targets
    cargo build --no-default-features

# Strict clippy over every target under every feature combo we care about.
lint:
    cargo clippy --all-features --all-targets -- -D warnings

# Run the full test suite.
test:
    cargo test --all-features

# Run criterion benches. Slow; only run after touching hot-path code.
bench:
    cargo bench

# Hero demo. Runs the CLI's demo subcommand, which spins up an in-process
# mock sensor feed when no Python is available.
demo:
    cargo run --features "network cli" --bin sensor-bridge -- demo

# Run the UDP example plus Python sender in two panes. Requires `python3`
# on PATH and the script's requirements installed.
udp-demo:
    cargo run --features network --example udp_demo

# CI-style: everything, in order, stop on first failure.
ci: check lint test
