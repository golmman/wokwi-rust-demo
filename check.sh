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

# Both feature flag configurations have to keep building. The
# `dcf77-loopback` feature swaps in a TX path inside the dcf77_sample
# task; cfg-gated regressions there only surface when that variant is
# actually compiled.
step "cargo build --release --features dcf77-loopback"
cargo build --release --features dcf77-loopback

step "tools/check_decoder.py (Rust GOLDEN ↔ Python FONT anchor)"
python3 tools/check_decoder.py

step "render → decode round-trip (every digit)"
# Renders firmware-side `clock_to_frame(hh,mm,ss)` to a host PNG via
# `examples/render_fixture.rs`, then decodes it back through
# `tools/decode_screenshot.py`. If any of {font.rs, display.rs, the PNG
# layout, the decoder's sampling/threshold} drifts from the others, the
# decoded text won't match the input. The fixtures below collectively use
# every digit 0-9 and the colon glyph in every digit position.
mkdir -p target/check
for t in "00:00:00" "12:34:56" "18:27:39" "23:59:59"; do
    h="${t%%:*}"
    rest="${t#*:}"
    m="${rest%%:*}"
    s="${rest#*:}"
    out="target/check/render-${h}-${m}-${s}.png"
    cargo run --quiet --release --example render_fixture --target "$HOST_TRIPLE" \
        -- "$h" "$m" "$s" "$out"
    decoded=$(python3 tools/decode_screenshot.py "$out" | sed 's/^.*: //')
    if [ "$decoded" != "$t" ]; then
        echo "render→decode drift: rendered $t, decoded $decoded ($out)" >&2
        exit 1
    fi
    printf '  ok: %s\n' "$t"
done

printf '\nAll checks passed.\n'
