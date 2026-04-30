#!/bin/bash
# Single agent / pre-commit entrypoint: run every check that does NOT need
# the Wokwi cloud. Fast-feedback loop for ~80% of changes.
#
# - cargo fmt --check          formatting
# - cargo clippy ...            lints (firmware target)
# - cargo test --lib (host)     unit tests for clock/font/display
# - cargo build --release       firmware compiles for thumbv6m-none-eabi
# - tools/check_decoder.py      Python decoder still matches Rust GOLDEN
#
# Use `./run-sim.sh` afterwards only when a change actually touches hardware
# (bsp.rs, RTIC tasks, SPI/GPIO/timer-dependent behaviour).
set -euo pipefail

cd "$(dirname "$0")"

# Resolve host triple dynamically so this works on any machine running an
# `aarch64-apple-darwin` / `x86_64-unknown-linux-gnu` / etc. toolchain.
HOST_TRIPLE="$(rustc -vV | sed -n 's/host: //p')"

step() { printf '\n=== %s ===\n' "$1"; }

step "cargo fmt --check"
cargo fmt --check

step "cargo clippy (firmware target, deny warnings)"
cargo clippy --release --bin wokwi-test -- -D warnings
# The library has no embedded deps, so lint it on the host explicitly.
cargo clippy --release --lib --target "$HOST_TRIPLE" -- -D warnings

step "cargo test --lib (host: $HOST_TRIPLE)"
cargo test --lib --target "$HOST_TRIPLE"

step "cargo build --release (firmware: thumbv6m-none-eabi)"
cargo build --release

step "tools/check_decoder.py (Rust GOLDEN ↔ Python FONT anchor)"
python3 tools/check_decoder.py

printf '\nAll checks passed.\n'
