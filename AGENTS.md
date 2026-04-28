# AGENTS.md

## Project Overview

`wokwi-rust-demo` is a Rust **embedded firmware** project (`no_std`, `no_main`) for the **Raspberry Pi Pico** (RP2040, Cortex-M0+). It drives a chain of four **MAX7219** 8x8 LED matrices (FC16 layout) as an `HH:MM:SS` clock, with a push-button on `GP15` to set the minutes (acceleration on hold). The firmware runs on real hardware **and** in the [Wokwi](https://wokwi.com/) simulator using the same `.uf2` artifact.

Key technologies:

- **Rust 2021**, target `thumbv6m-none-eabi`
- **RTIC 1.x** (`cortex-m-rtic`) for interrupt-driven concurrency
- **rp-pico 0.9** HAL (with `boot2`)
- **max7219 0.4** display driver over SPI
- **embedded-hal 0.2**, `defmt` + `defmt-rtt`, `panic-halt`
- Wokwi simulator (`wokwi.toml`, `diagram.json`)

There is no test suite, no CI, and no separate lint configuration in this repo — see [Code Style](#code-style) and [Verification](#verification) for what to run instead.

## Repository Layout

```
.
├── Cargo.toml           # crate name: `wokwi-test`
├── .cargo/config.toml   # default target + linker flags + `picotool` runner
├── memory.x             # RP2040 linker script (BOOT2/FLASH/RAM/SRAM4-5)
├── build.sh             # convenience: cargo build --release && elf2uf2-rs
├── wokwi.toml           # points Wokwi at the release ELF + UF2
├── diagram.json         # Wokwi circuit (Pico + MAX7219 chain + button)
├── number-design.png    # design reference for the 3x8 digit font
├── src/
│   ├── main.rs          # RTIC `#[app]`: init + 3 hw tasks + 1 sw task
│   ├── clock.rs         # `ClockState` (hours/mins/secs, tick, add_minute)
│   ├── display.rs       # `prepare_buffer` → 4×[u8; 8] for the FC16 chain
│   └── font.rs          # 3x8 bitmap font for digits 0–9 and `:`
└── target/              # build output (gitignored)
```

The crate's package name is `wokwi-test` (binary), so build artifacts live under
`target/thumbv6m-none-eabi/release/wokwi-test{,.uf2}`.

## Prerequisites

One-time toolchain setup (do this before any build):

```sh
rustup target add thumbv6m-none-eabi
cargo install elf2uf2-rs
```

`elf2uf2-rs` is required because Wokwi (`wokwi.toml`) and the BOOTSEL flashing
path both consume the `.uf2`, not the raw ELF.

`picotool` is **only** needed if you want to flash with `cargo run` or
`picotool load` — see [Deploying to Hardware](#deploying-to-hardware). It is not
required for `cargo build` or for Wokwi.

## Setup Commands

This is a single Cargo crate; there is nothing to install at the repo level
beyond fetching dependencies (`cargo` does this implicitly on first build):

```sh
cargo fetch
```

The default target is set to `thumbv6m-none-eabi` in `.cargo/config.toml`, so
`cargo` commands automatically cross-compile — **do not** pass
`--target=...` unless you intentionally want a different target.

## Build

Use either of:

```sh
# 1) Manual
cargo build --release
elf2uf2-rs target/thumbv6m-none-eabi/release/wokwi-test \
           target/thumbv6m-none-eabi/release/wokwi-test.uf2

# 2) Convenience script (does both of the above)
./build.sh
```

Notes for agents:

- Always build `--release`. The Wokwi config (`wokwi.toml`) and the README
  flashing instructions both reference the release path.
- The release profile in `Cargo.toml` is tuned for size (`opt-level = "z"`,
  `lto = true`, `codegen-units = 1`); changing it can break flash fit.
- `cargo build` (debug) works for syntax/type checking but is **not** what
  Wokwi or hardware load.

## Running in Wokwi (simulator)

Two equivalent options:

1. Open `diagram.json` directly in <https://wokwi.com/> and click **Run**.
2. If the Wokwi VS Code / IntelliJ plugin is installed locally, opening the
   project root and starting the simulator picks up `wokwi.toml`, which already
   points at:
   - `firmware = "target/thumbv6m-none-eabi/release/wokwi-test.uf2"`
   - `elf      = "target/thumbv6m-none-eabi/release/wokwi-test"`

You **must** rebuild (`./build.sh` or the manual two commands above) after any
source change before re-running the simulator — Wokwi reads the on-disk UF2.

## Deploying to Hardware

### A) BOOTSEL drag-and-drop (no extra tools)

Hold the **BOOTSEL** button while plugging the Pico into USB; it mounts as
`RPI-RP2`. Then copy the UF2 (path is OS-specific):

```sh
cp target/thumbv6m-none-eabi/release/wokwi-test.uf2 /run/media/$USER/RPI-RP2/   # Linux
cp target/thumbv6m-none-eabi/release/wokwi-test.uf2 /Volumes/RPI-RP2/           # macOS
```

### B) `picotool` (Pico already attached / debug probe)

`.cargo/config.toml` defines the runner as
`picotool load --update --verify --execute -t elf`, so:

```sh
cargo run --release
```

…will build and flash via `picotool` in one step. Equivalent manual command:

```sh
picotool load -f target/thumbv6m-none-eabi/release/wokwi-test.uf2
```

There is also a commented-out `probe-rs run --chip RP2040` runner in
`.cargo/config.toml` for SWD debug probes; uncomment that line (and comment the
`picotool` one) if you want to use `probe-rs` instead.

## Hardware Wiring (from `diagram.json`)

If you change pin assignments in `src/main.rs`, update `diagram.json` to match,
or Wokwi will silently mis-wire:

| Pico pin | MAX7219 / button | Purpose                 |
| -------- | ---------------- | ----------------------- |
| 3V3      | matrix1.VCC      | logic supply            |
| VBUS     | matrix1.V+       | LED supply              |
| GND      | matrix1.GND      | ground                  |
| GP18     | matrix1.CLK      | SPI0 SCK                |
| GP19     | matrix1.DIN      | SPI0 MOSI               |
| GP17     | matrix1.CS       | chip select (GPIO out)  |
| GP15     | btn1             | active-low, pull-up in  |
| GP25     | (onboard LED)    | 1 Hz heartbeat blink    |

The MAX7219 chain length (`4`) is hard-coded in `MAX7219::from_spi_cs(4, …)`
and in `display::prepare_buffer` (`[[u8; 8]; 4]`). Change both together if you
add/remove modules.

## Code Style

- **Rust 2021**, `#![no_std]` + `#![no_main]`. Do not introduce `std`,
  heap allocators, or panicking-on-format paths without explicit reason.
- The whole RTIC app lives in `mod app` inside `src/main.rs`. New
  interrupt-driven logic should be a new RTIC task with explicit
  `shared = [...]` / `local = [...]` resource lists, not ad-hoc `static mut`.
- Time math goes in `src/clock.rs`; pixel math goes in `src/display.rs`;
  glyphs go in `src/font.rs`. Keep `main.rs` focused on hardware setup and
  task wiring.
- Use the existing `fugit` rate/duration extensions (`u32::Hz()`,
  `u32::micros()`) for SPI/timer values rather than raw integers.
- Follow the surrounding formatting (`rustfmt` defaults — there is no
  `rustfmt.toml`). Run `cargo fmt` before committing.
- **Do not add or remove comments unless the task requires it.** Several
  modules currently have leading blank lines; preserve them on edits.

## Verification

Before declaring a change done, in addition to a clean release build:

```sh
cargo check --release          # fast type check on the embedded target
cargo build --release          # full compile (must succeed)
cargo clippy --release -- -D warnings   # optional but recommended
cargo fmt --check              # formatting
./build.sh                     # ensures the .uf2 still regenerates
```

If your change is observable at runtime (display output, button behavior,
timing), additionally:

- Re-run the Wokwi simulation from the freshly built `.uf2` and visually
  confirm the clock advances and the button still increments minutes
  (with acceleration when held).

There is no `cargo test` target — the crate is `no_std` firmware and has no
host-side tests today. Do **not** add `#[cfg(test)]` blocks that pull in
`std` without first refactoring the affected module to be host-buildable.

## Pull Request Guidelines

- Keep changes scoped: firmware logic, simulator config (`diagram.json`,
  `wokwi.toml`), and toolchain config (`.cargo/config.toml`, `memory.x`)
  often need to move together — call this out in the PR description.
- Required local checks before pushing:
  - `cargo build --release`
  - `cargo fmt --check`
  - (recommended) `cargo clippy --release -- -D warnings`
- Existing commit messages are short, lower-case, imperative summaries
  (e.g. `add rtic for clock timing`, `clean up code`). Match that style.

## Common Gotchas

- **Wokwi runs stale firmware.** The simulator reads
  `target/thumbv6m-none-eabi/release/wokwi-test.uf2` from disk. Rebuild
  (`./build.sh`) after every source edit; `cargo build` alone does not refresh
  the UF2.
- **Wrong target.** `.cargo/config.toml` pins the default to
  `thumbv6m-none-eabi`. If you see weird "can't find `core`" errors, you are
  probably building from a directory where that config isn't picked up — run
  cargo from the repo root.
- **Display chain length mismatch.** The `4` in `MAX7219::from_spi_cs(4, …)`
  must match `diagram.json`'s `"chain": "4"` and the `[[u8; 8]; 4]` buffer in
  `display.rs`.
- **Button bouncing.** The button task disables its own GPIO interrupt and
  re-enables it only after release in `button_repeat`. New button-related code
  must respect that handshake or you will get spurious presses.
- **Picotool is optional.** Don't gate builds or CI on it; the BOOTSEL +
  `cp` workflow has no extra dependencies.
