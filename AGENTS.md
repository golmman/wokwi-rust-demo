# AGENTS.md

## Project Overview

`wokwi-rust-demo` is a Rust **embedded firmware** project (`no_std`, `no_main`) for the **Raspberry Pi Pico** (RP2040, Cortex-M0+). It drives a chain of four **MAX7219** 8x8 LED matrices (FC16 layout) as an `HH:MM:SS` clock, with a push-button on `GP15` to set the minutes (acceleration on hold) and an optional **DCF77** longwave receiver on `GP14` for self-correcting time. The firmware runs on real hardware **and** in the [Wokwi](https://wokwi.com/) simulator using the same `.uf2` artifact.

Key technologies:

- **Rust 2021**, target `thumbv6m-none-eabi`
- **RTIC 1.x** (`cortex-m-rtic`) for interrupt-driven concurrency
- **rp-pico 0.9** HAL (with `boot2`)
- **max7219 0.4** display driver over SPI
- **embedded-hal 0.2**, `panic-halt` for the panic handler
- Wokwi simulator (`wokwi.toml`, `diagram.json`) + Wokwi CLI for headless / agent-driven runs (`scenario.yaml`, `run-sim.sh`)

> The firmware emits no log output today (no `defmt`, no UART). Re-add `defmt`/`defmt-rtt` to `Cargo.toml` (and `-Tdefmt.x` to `.cargo/config.toml`'s `rustflags`) if you wire up RTT, or follow [Adding agent-visible serial](#adding-agent-visible-serial) for UART output that `wokwi-cli` can capture.

The crate is split into `src/lib.rs` (pure logic — `clock`, `display`, `font`, `config`) and `src/main.rs` (the firmware binary — RTIC `#[app]` + `bsp`). The library has zero external deps and is host-`cargo test`-able; the binary's deps are gated to `[target.thumbv6m-none-eabi.dependencies]`. `./check.sh` runs the full host-side battery (fmt + clippy + tests + firmware build + decoder-anchor) in ~2s; `./run-sim.sh` is the slow path for hardware behaviour. There's no GitHub Actions CI today — the same checks should be run locally before pushing. See [Verification](#verification).

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
├── scenario.yaml        # Wokwi CLI automation scenario (button + screenshots)
├── run-sim.sh           # slow path: build + run scenario via wokwi-cli (cloud, ~10s, burns sim minutes)
├── check.sh             # fast path: cargo fmt + clippy + cargo test --lib (host) + build + byte-anchor + render→decode round-trip (~2-4s)
├── tools/
│   ├── decode_screenshot.py  # decode a matrix1 PNG back to "HH:MM:SS"
│   └── check_decoder.py # byte-level anchor: decode_screenshot.py's FONT matches src/font.rs via display::GOLDEN_12_34_56
├── examples/
│   └── render_fixture.rs # host-side renderer: clock_to_frame(hh,mm,ss) → 256x64 PNG, used by check.sh's render→decode round-trip
├── .env                 # local-only: WOKWI_CLI_TOKEN=... (gitignored, not committed)
├── plans/dcf77/plan.md  # design rationale for the DCF77 receiver integration (see "DCF77 sync" below)
├── diagram.dcf77.json   # alt diagram with `pico:GP13 -> pico:GP14` wire for the DCF77 loopback sim
├── scenario.dcf77.yaml  # alt scenario for the loopback sim (used by `run-sim-dcf77.sh`)
├── run-sim-dcf77.sh     # slow-path opt-in: build with `--features dcf77-loopback` + run the loopback sim
├── src/
│   ├── main.rs          # binary: RTIC `#[app]` task wiring only — pulls hardware from `bsp`, logic from the library
│   ├── lib.rs           # library root: re-exports `clock`, `config`, `dcf77`, `display`, `font` for host-side `cargo test`
│   ├── bsp.rs           # binary-only: pin assignments, SPI, MAX7219 bring-up, alarms (`Board::take`) — not host-testable
│   ├── config.rs        # Runtime tunables: tick interval, button repeat / debounce, SPI freq, intensity, INITIAL_TIME, DCF77 sample/pulse-width windows
│   ├── clock.rs         # `ClockState` with private fields, accessors, `tick`/`add_{second,minute,hour}`/`set_time` (+ unit tests)
│   ├── dcf77.rs         # `Decoder` (pulse stream → `Frame`) + `decode_bits` (telegram → `Frame`) + `TxState` (loopback encoder, used with `dcf77-loopback` feature) (+ unit tests)
│   ├── display.rs       # `Framebuffer` (32×8 bitmap) + `clock_to_frame` adapter + `GOLDEN_12_34_56` (+ unit tests)
│   └── font.rs          # `Glyph` enum + 3×8 bitmaps for digits 0–9 and `:` (+ unit tests)
└── target/              # build output (gitignored, includes target/wokwi/ artifacts)
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

## Running in Wokwi (CLI / agent-driven)

The Wokwi web UI and VS Code extension are convenient for humans, but they are
not observable to a coding agent. For a closed-loop "edit firmware → run sim →
inspect output" workflow, use the [Wokwi CLI][wokwi-cli] against `scenario.yaml`.
This is what `run-sim.sh` does; the resulting screenshots can be read by the
agent's image tooling and decoded back to text via `tools/decode_screenshot.py`.

[wokwi-cli]: https://docs.wokwi.com/wokwi-ci/cli-usage

### One-time setup

1. Install the CLI (binary release; `~/.local/bin` is on PATH):
   ```sh
   curl -fsSL -o ~/.local/bin/wokwi-cli \
     https://github.com/wokwi/wokwi-cli/releases/download/v0.26.1/wokwi-cli-macos-arm64
   chmod +x ~/.local/bin/wokwi-cli
   ```
   On Linux, swap the asset name for `wokwi-cli-linuxstatic-{x64,arm64}`.
2. Generate a CI token at <https://wokwi.com/dashboard/ci>. Free tier gives 50
   simulation-minutes/month, plenty for iteration.
3. Drop it in a local `.env` file (gitignored — never commit):
   ```sh
   echo 'WOKWI_CLI_TOKEN=<your-token>' > .env
   ```

### Run a scenario

```sh
./run-sim.sh
```

This (re)builds the UF2, sources `.env`, calls `wokwi-cli --scenario scenario.yaml`,
and writes:

- `target/wokwi/before.png` and `target/wokwi/after.png` — screenshots of the
  `matrix1` LED chain (256×64 PNG, 8×8 px per LED) before and after a button press.
- `target/wokwi/serial.log` — UART output. **Empty today** because the firmware
  emits nothing on UART; see [Adding agent-visible serial](#adding-agent-visible-serial).

To programmatically read the displayed time from a screenshot:

```sh
python3 tools/decode_screenshot.py target/wokwi/before.png target/wokwi/after.png
# target/wokwi/before.png: 12:35:57
# target/wokwi/after.png:  12:36:59
```

The decoder mirrors `src/font.rs` and `src/display.rs`; if you change either,
update `tools/decode_screenshot.py` to match or it will silently misread.

### Wokwi CLI capabilities exposed to an agent

Verified against `wokwi-cli 0.26.1` and the scenario steps documented at
<https://docs.wokwi.com/wokwi-ci/automation-scenarios>:

| Channel             | How                                                      |
| ------------------- | -------------------------------------------------------- |
| Serial / UART       | streams to stdout; `--serial-log-file <path>` to capture |
| Per-part screenshot | `--screenshot-part <id> --screenshot-time <ms>`, or scenario `take-screenshot:` |
| Pin assertions      | scenario `expect-pin: { part-id, pin, expected }`        |
| Drive button/sensor | scenario `set-control: { part-id, control, value }`      |
| Logic-analyzer VCD  | `--vcd-file <path>` (requires a `wokwi-logic-analyzer` part in `diagram.json`) |
| Text assertion      | `--expect-text <s>` / `--fail-text <s>` / `wait-serial:` |

What is **not** observable: `defmt-rtt` output (Wokwi captures UART, not RTT —
even though `defmt`/`defmt-rtt`/`panic-probe` are listed in `Cargo.toml`,
nothing in `src/` actually uses them), GDB state, raw framebuffer.

### Adding agent-visible serial

To unlock `wait-serial` / `--expect-text` / `target/wokwi/serial.log`, add a
UART writer to the firmware (e.g. `UART0` on `GP0`) and emit `HH:MM:SS` once
per `timer_tick`. Wire `pico:GP0` → a `wokwi-virtual-uart` (or any TX-able
sink) in `diagram.json` so the CLI sees the bytes. This was deliberately not
done in the current setup; do it if/when an agent task needs textual feedback.

### Timing caveat (RP2040 in Wokwi)

In `wokwi-cli 0.26.1` the RP2040 simulation advances the firmware's wall-time
view **much faster than nominal**: an explicit `--screenshot-time 1500`
(documented as simulation milliseconds) shows the clock at `12:35:57`, i.e.
~61 firmware seconds elapsed against an init of `12:34:56`. The single-press
+1-minute behavior of `btn1` still works — it just lands in a clock that has
already advanced ~40× during the scenario's first `delay`. Don't rely on
exact `delay: <Nms>` values to land at a specific `HH:MM:SS`; instead, snapshot
twice and assert on the *delta* (e.g. "minutes increased by exactly 1 from
before to after"), which is robust to Wokwi's sim speed.

### Linting the diagram

`wokwi-cli lint` validates `diagram.json` against the part registry. The
current diagram has one pre-existing warning — `matrix1` is wired to a `VCC`
pin that doesn't exist on `wokwi-max7219-matrix` (only `V+`, plus `V+.2`).
The simulation runs anyway; left as-is to match the existing real-hardware
wiring described below.

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

| Pico pin | MAX7219 / button / DCF77 | Purpose                 |
| -------- | ------------------------ | ----------------------- |
| 3V3      | matrix1.VCC              | logic supply            |
| VBUS     | matrix1.V+               | LED supply              |
| GND      | matrix1.GND              | ground                  |
| GP18     | matrix1.CLK              | SPI0 SCK                |
| GP19     | matrix1.DIN              | SPI0 MOSI               |
| GP17     | matrix1.CS               | chip select (GPIO out)  |
| GP15     | btn1                     | active-low, pull-up in  |
| GP14     | DCF77 receiver DATA      | active-LOW pulse, pull-up in (idle reads HIGH) |
| GP13     | (loopback TX, optional)  | only configured with the `dcf77-loopback` feature; wired to GP14 in `diagram.dcf77.json` so the firmware can drive its own receiver in the simulator |
| GP25     | (onboard LED)            | 1 Hz heartbeat blink    |

> The `3V3 → matrix1.VCC` row reflects the intended wiring on a physical
> MAX7219 module, but the Wokwi part `wokwi-max7219-matrix` doesn't expose a
> `VCC` pin — its only valid power pins are `V+` and `V+.2`. `wokwi-cli lint`
> will flag this as `[invalid-pin] Invalid pin "VCC"`; the simulation runs
> regardless. Don't "fix" it by deleting the line without verifying real
> hardware still works. See [Linting the diagram](#linting-the-diagram).

The MAX7219 chain length (`4`) is hard-coded in `MAX7219::from_spi_cs(4, …)`
and in `display::prepare_buffer` (`[[u8; 8]; 4]`). Change both together if you
add/remove modules.

## Code Style

- **Rust 2021**, `#![no_std]` + `#![no_main]`. Do not introduce `std`,
  heap allocators, or panicking-on-format paths without explicit reason.
- The whole RTIC app lives in `mod app` inside `src/main.rs`. New
  interrupt-driven logic should be a new RTIC task with explicit
  `shared = [...]` / `local = [...]` resource lists, not ad-hoc `static mut`.
- Module responsibilities — keep them honest:
  - `lib.rs` is the **library root**. It re-exports `clock`, `config`, `dcf77`, `display`, `font`. Adding a module here means it can be host-tested via `cargo test --lib`. Adding embedded-only deps to a library module breaks that, so don't.
  - `bsp.rs` is **binary-only** and owns hardware: pin numbers, peripheral type aliases, SPI/MAX7219 bring-up, alarms 0..=3. **Pin changes happen here only.** Cannot be host-tested.
  - `config.rs` owns tunables: tick interval, button repeat / debounce parameters, SPI frequency, brightness, initial time, DCF77 sample / pulse-width / gap windows. **No types, no logic.**
  - `clock.rs` owns `ClockState` with private fields and `tick`/`add_*`/`set_time` semantics. Don't widen the public surface to `pub` fields. `set_time` is the DCF77-sync entry point; treat it like `new()` (clamps inputs). Has `#[cfg(test)] mod tests` covering rollovers, `new()` clamping, and `set_time`.
  - `dcf77.rs` owns `Decoder` (10 ms `sample(level, dt_us)` → `Option<Frame>`) plus the pure-data `decode_bits` plus the loopback `TxState`. Pulse-width / gap thresholds come from `config`. Has three layers of `#[cfg(test)] mod tests`: bit-level decoder, pulse-stream decoder, and `encode → modulate → decode` round-trip (mirrors `check.sh`'s `render → decode` round-trip). The encoder + `TxState` are always compiled but only reachable via `Some(TxState)` when the `dcf77-loopback` feature is on; LTO drops them otherwise.
  - `display.rs` owns `Framebuffer` (pure pixels) plus the `clock_to_frame` adapter and the `GOLDEN_12_34_56` golden constant. Don't import `clock` from anywhere except via that adapter. Has `#[cfg(test)] mod tests` including the golden-byte test that pins `clock_to_frame(12,34,56)`'s exact output.
  - `font.rs` owns the `Glyph` enum and bitmaps. Adding a new glyph is one variant + one match arm + one bitmap const (with `#[rustfmt::skip]`) — no integer offsets to keep in sync. Has `#[cfg(test)] mod tests` ensuring `Glyph::digit` returns the right variant for `0..=9` and clipping invariants.
  - `main.rs` is the **binary**: RTIC task wiring + ISR bodies only. It pulls hardware from `bsp` and pure logic from the library via `use wokwi_test::{...}`. It should not call into `rp_pico::hal` directly for anything `bsp.rs` could provide.
- Use the existing `fugit` rate/duration extensions (`u32::Hz()`,
  `u32::micros()`) for SPI/timer values rather than raw integers.
- Follow the surrounding formatting (`rustfmt` defaults — there is no
  `rustfmt.toml`). Run `cargo fmt` before committing.
- **Do not add or remove comments unless the task requires it.** Several
  modules currently have leading blank lines; preserve them on edits.

## Verification

The fast path covers ~80% of changes. Run **one** command:

```sh
./check.sh
```

That runs, in order: `cargo fmt --check`, `cargo clippy -D warnings`
(firmware target + host lib), `cargo test --lib --target=<host>`,
`cargo build --release` (default features) and `cargo build --release
--features dcf77-loopback` (loopback variant), `tools/check_decoder.py`
(byte-level decoder anchor), and a render→decode round-trip for four
`HH:MM:SS` fixtures covering every digit. Total wall time: ~4s cold,
~2.4s warm. No cloud, no token, no sim-minute quota. Use this as the
inner loop while iterating on logic.

The slow path is required only when a change touches **hardware behaviour**
— `bsp.rs`, anything in `main.rs::init`, or runtime SPI/GPIO/timer
interaction. For those, additionally:

```sh
./run-sim.sh
python3 tools/decode_screenshot.py target/wokwi/before.png target/wokwi/after.png
```

…and assert on the decoded `HH:MM:SS` deltas (not absolute values — see
[Timing caveat](#timing-caveat-rp2040-in-wokwi)). This costs ~10s wall and
~1 sim-minute against your monthly quota.

### Iteration loop for agents

Recommended sequence for any code change:

1. **Edit.**
2. **`./check.sh`.** Iterate until green. Most logic regressions surface here
   with a `cargo test` failure that pinpoints `file:line` and shows the
   actual vs expected values — no PNG decoding, no cloud round-trip.
3. **Only if step 2 changed something hardware-touching:** `./run-sim.sh`,
   then read the screenshots (or pipe through `tools/decode_screenshot.py`)
   and verify behaviour. Frame assertions as **deltas** between `before.png`
   and `after.png`, not absolute clock values. If your change touches the
   DCF77 RX/TX wiring (`dcf77_sample`, `Dcf77InPin`/`Dcf77OutPin`,
   `diagram.dcf77.json`), also run `./run-sim-dcf77.sh` — that one builds
   with the loopback feature on and asserts the receiver actually decoded
   a TX-broadcast frame.

Concrete classes of change and which step is sufficient:

| Change type                                                         | `check.sh` is enough | Need `run-sim.sh` too | Need `run-sim-dcf77.sh` too |
| ------------------------------------------------------------------- | -------------------- | --------------------- | --------------------------- |
| `clock.rs` rollover / arithmetic                                    | ✅                    |                       |                             |
| `font.rs` glyph redesign                                            | ✅ (golden test catches drift) |             |                             |
| `display.rs` `Framebuffer` / layout / packing                       | ✅                    |                       |                             |
| `config.rs` constant retune (e.g. brightness, tick rate, DCF77 windows) | ✅                | ✅ (visual)            |                             |
| `dcf77.rs` decoder / encoder logic (pure data, no hw)               | ✅ (host round-trip catches drift) |          |                             |
| `bsp.rs` pin assignment / SPI freq / GPIO-IRQ wiring                | ✅ for compile        | ✅                     |                             |
| `main.rs` non-DCF77 task / RTIC resource changes                    | ✅ for compile        | ✅                     |                             |
| `main.rs::dcf77_sample` task body / DCF77 RX/TX wiring              | ✅ for compile        | ✅                     | ✅                           |
| `diagram.json` / `wokwi.toml`                                       | n/a                  | ✅                     |                             |
| `diagram.dcf77.json` / `scenario.dcf77.yaml`                        | n/a                  |                       | ✅                           |

### Decoder anchoring (why both `check_decoder.py` and `render_fixture` exist)

`tools/decode_screenshot.py` duplicates the 3×8 glyph patterns from
`src/font.rs` because Python can't import Rust `const`s. Without a check,
someone could redesign a glyph in `font.rs`, update
`display::GOLDEN_12_34_56` to match, and silently leave
`decode_screenshot.py` decoding the new bitmap as the wrong character —
or the *decoder's* PNG-sampling could regress (threshold, cell layout)
without the firmware build noticing.

Two anchors run in `check.sh`. The pipeline round-trip is the load-bearing
one; the byte-level smoke test is a faster auxiliary check whose coverage
is a strict subset of the round-trip's.

1. **Pipeline round-trip (load-bearing)** — `examples/render_fixture.rs` +
   `tools/decode_screenshot.py`. Calls the firmware's real `clock_to_frame`
   for several `HH:MM:SS` fixtures, writes a 256×64 PNG with the same 8×8
   cell layout Wokwi uses, then runs the full PNG decoder on each and
   asserts it reads back the original. Catches drift in *every* layer:
   `font.rs` glyphs, `display.rs` packing, the PNG cell layout, the
   decoder's sampling/threshold logic, the renderer's PNG writing. The
   fixtures collectively use every digit 0–9 and the colon glyph in every
   digit position.

2. **Byte-level smoke test (auxiliary)** — `tools/check_decoder.py`.
   Hardcodes the same `[[u8; 8]; 4]` device buffers the Rust golden test
   pins for `12:34:56`, unpacks them into a 32×8 grid, and feeds them to
   `decode_screenshot.py`'s `decode_grid()` helper. Asserts the Python
   decoder reads `12:34:56`. Faster than the round-trip and bypasses PNG
   IO, useful as a quick sanity check, but anything it would catch is also
   caught by the round-trip above. Keep its `GOLDEN_12_34_56` in sync with
   `display::GOLDEN_12_34_56` whenever you update the Rust constant — if
   you forget, this script will fail before the round-trip does.

If you intentionally change a glyph, the failures cascade and tell you
exactly which file to edit:

1. The Rust unit test `display::tests::clock_to_frame_golden_for_12_34_56`
   fails first — copy the new bytes into `display::GOLDEN_12_34_56`.
2. `tools/check_decoder.py` then fails — update `GOLDEN_12_34_56` in that
   file *and* `decode_screenshot.py::FONT` to match the new glyph.
3. The render→decode round-trip fails for any fixture using the changed
   glyph — there's no separate update needed; once step 2 is done, the
   round-trip should pass.

Run `./check.sh`; it's green only once all three are consistent. Bonus: you
can render any clock state locally for visual inspection without a Wokwi
sim run — `cargo run --example render_fixture --target=<host> -- 12 34 56 /tmp/out.png`.

### What's not covered by host tests

- `bsp.rs` peripheral bring-up (SPI init, MAX7219 init): only the firmware
  build exercises this, and only Wokwi shows it actually drives the chip.
- RTIC task scheduling, ISR timing, button interrupt arming: not testable
  on host.
- The 40× sim-time-over-wall-time quirk (see [Timing caveat](#timing-caveat-rp2040-in-wokwi)).

For all of those, `./run-sim.sh` is the only check.

## DCF77 sync

The clock can self-correct from a [DCF77](https://en.wikipedia.org/wiki/DCF77)
longwave receiver wired to `GP14`. See `plans/dcf77/plan.md` for the full
design rationale (decisions, alternatives, risks); the summary:

- **Where the logic lives** — `src/dcf77.rs` (pure, host-testable). It has
  three layers: `decode_bits` (a fully-received `[bool; 60]` → `Frame`), the
  streaming `Decoder` (`sample(level, dt_us) → Option<Frame>`), and the
  loopback `TxState` used by the sim to drive its own receiver. All three
  are `#[cfg(test)]`-covered, and the encode → modulate → decode round-trip
  is in `check.sh`'s host test set, so most regressions surface there.
- **How the firmware drives it** — `src/main.rs`'s `dcf77_sample` task,
  bound to `TIMER_IRQ_3` (= `alarm3`), polls `bsp::Dcf77InPin` every
  `DCF77_SAMPLE_US` (10 ms) and feeds samples to `Decoder`. When `Decoder`
  emits `Some(Frame)` (= falling edge that ends a minute-marker gap),
  `clock.set_time(h, m, 0)` is applied. With no receiver wired the pin
  idles HIGH (internal pull-up), the decoder stays in `SearchingForGap`,
  and nothing is written to the clock — production firmware behaves as
  before.
- **`dcf77-loopback` Cargo feature (off by default)** — turns the
  `dcf77_sample` task into a TX-and-RX combo: each tick it also drives
  `bsp::Dcf77OutPin` on `GP13` with the encoded telegram for the
  firmware's *current* `(h, m)`. Wire `GP13 → GP14` in `diagram.dcf77.json`
  and the firmware becomes its own DCF77 source inside Wokwi. The
  feature only swaps in `Some(...)` for the `Option<TxState>` /
  `Option<Dcf77OutPin>` resources; without it both are `None` and LTO
  drops the encoder + transmitter from the binary (~1 kB savings).
- **Slow-path verification** — `./run-sim-dcf77.sh` builds with the
  feature on, runs `scenario.dcf77.yaml` against `diagram.dcf77.json`,
  and asserts the displayed minute changed at least once between
  `dcf77-before.png` and `dcf77-after.png` (proof that RX decoded a
  TX-broadcast frame and applied it to the clock).
- **Initial sync time** — the receiver needs ~1 firmware minute to
  acquire the first minute-marker gap, plus another ~1 minute to
  collect a full frame, so the first `set_time` lands ~2 firmware
  minutes after boot. The display continues to show `INITIAL_TIME`
  advancing during that window.

## Pull Request Guidelines

- Keep changes scoped: firmware logic, simulator config (`diagram.json`,
  `wokwi.toml`), and toolchain config (`.cargo/config.toml`, `memory.x`)
  often need to move together — call this out in the PR description.
- Required local checks before pushing: **`./check.sh`** (covers fmt,
  clippy, host-side `cargo test`, firmware build for both feature
  configurations, and the Python decoder anchor). Run `./run-sim.sh`
  additionally if your change touches `bsp.rs`, `main.rs::init`, or
  hardware-driven behaviour. Run `./run-sim-dcf77.sh` additionally if
  your change touches the DCF77 RX/TX wiring or `diagram.dcf77.json`.
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
- **Button bouncing.** Three RTIC tasks cooperate on a single press:
  `button_press` (`IO_IRQ_BANK0`) registers the +1 minute and immediately
  disables the GPIO `EdgeLow` IRQ; `button_debounce` (`TIMER_IRQ_2`,
  `alarm2`) polls every `BUTTON_DEBOUNCE_US` and only re-enables the IRQ
  once the button is settled HIGH; `button_repeat` (`TIMER_IRQ_1`,
  `alarm1`) drives accelerating auto-repeat while still held and, on
  release, hands off to `alarm2` so post-release bouncing settles before
  the next press is accepted. New button-related code must respect this
  three-task handshake or you will get spurious presses or eaten clicks.
- **Picotool is optional.** Don't gate builds or CI on it; the BOOTSEL +
  `cp` workflow has no extra dependencies.
- **`.env` holds secrets, never commit it.** `WOKWI_CLI_TOKEN` lives there;
  `.env` and `.env.*` are gitignored (with `!.env.example` carved out for an
  optional sample). If you ever paste a token in a commit message or a
  committed file, rotate it on the [Wokwi CI dashboard][dash].
- **Wokwi sim time runs faster than wall-clock for this RP2040 build.** See
  the [timing caveat](#timing-caveat-rp2040-in-wokwi). Write CLI/scenario
  assertions in terms of *deltas between snapshots*, not absolute `HH:MM:SS`.
- **DCF77 RX polling and TX (loopback) share `alarm3`.** The RP2040 only has
  4 hardware alarms (0 = 1 Hz tick, 1 = button repeat, 2 = button debounce,
  3 = DCF77 10 ms poll). The loopback transmitter is folded into the same
  `dcf77_sample` task body — it doesn't get its own alarm. If you ever need
  to split RX and TX onto independent cadences, you have to either (a) free
  up an alarm by reworking the button state machine, or (b) drop the sample
  rate to 5 ms and time-slice. There is no fifth alarm.
- **DCF77 receiver polarity.** The decoder assumes idle-HIGH, LOW pulses
  (the convention used by HKW / Conrad / C-MAX modules). If you wire up a
  receiver whose output is inverted (rare but it happens), every pulse will
  read as a glitch and the decoder will sit in `SearchingForGap` forever.
  Workaround: invert in software (`!level` in `dcf77_sample`) or pass the
  signal through a transistor. We do **not** auto-detect polarity.
- **`dcf77-loopback` feature is in `check.sh`.** Both the default config
  and `--features dcf77-loopback` are built. Don't gate code on
  `cfg(feature = ...)` in a way that compiles only one of the two — we
  test both. RTIC's `#[app]` macro doesn't honour `#[cfg]` on tasks (the
  `mod app` token stream is parsed before `cfg` strips items), so to keep
  the loopback opt-in we use `Option<TxState>` / `Option<Dcf77OutPin>`
  with `cfg`-gated initialisers, not `#[cfg]` on the task body or the
  `local = [...]` list.

[dash]: https://wokwi.com/dashboard/ci
