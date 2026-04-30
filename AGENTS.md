# AGENTS.md

## Project Overview

`wokwi-rust-demo` is a Rust **embedded firmware** project (`no_std`, `no_main`) for the **Raspberry Pi Pico** (RP2040, Cortex-M0+). It drives a chain of four **MAX7219** 8x8 LED matrices (FC16 layout) as an `HH:MM:SS` clock, with a push-button on `GP15` to set the minutes (acceleration on hold). The firmware runs on real hardware **and** in the [Wokwi](https://wokwi.com/) simulator using the same `.uf2` artifact.

Key technologies:

- **Rust 2021**, target `thumbv6m-none-eabi`
- **RTIC 1.x** (`cortex-m-rtic`) for interrupt-driven concurrency
- **rp-pico 0.9** HAL (with `boot2`)
- **max7219 0.4** display driver over SPI
- **embedded-hal 0.2**, `panic-halt` for the panic handler
- Wokwi simulator (`wokwi.toml`, `diagram.json`) + Wokwi CLI for headless / agent-driven runs (`scenario.yaml`, `run-sim.sh`)

> `defmt`, `defmt-rtt`, and `panic-probe` are declared in `Cargo.toml` but currently **unimported and unused** anywhere in `src/`. The firmware emits no log output today; see [Adding agent-visible serial](#adding-agent-visible-serial) if you need it.

There is no `cargo test` target and no GitHub Actions CI today. Functional behavior is verified by running the firmware in Wokwi — interactively in the web UI / VS Code extension, or headless via `./run-sim.sh`, which captures `target/wokwi/{before,after}.png` for a human or agent to inspect (see [Running in Wokwi (CLI / agent-driven)](#running-in-wokwi-cli--agent-driven)). For host-side checks, see [Verification](#verification).

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
├── run-sim.sh           # build + run scenario via wokwi-cli; outputs to target/wokwi/
├── tools/
│   └── decode_screenshot.py  # decode a matrix1 PNG back to "HH:MM:SS"
├── .env                 # local-only: WOKWI_CLI_TOKEN=... (gitignored, not committed)
├── src/
│   ├── main.rs          # RTIC `#[app]`: init + 3 hw tasks + 1 sw task
│   ├── clock.rs         # `ClockState` (hours/mins/secs, tick, add_minute)
│   ├── display.rs       # `prepare_buffer` → 4×[u8; 8] for the FC16 chain
│   └── font.rs          # 3x8 bitmap font for digits 0–9 and `:`
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
- For agent-driven verification (no human in the loop), run `./run-sim.sh`
  and assert on the screenshots in `target/wokwi/` — either by `read`-ing
  the PNGs as images or by piping them through `tools/decode_screenshot.py`.
  See [Running in Wokwi (CLI / agent-driven)](#running-in-wokwi-cli--agent-driven).

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
- **`.env` holds secrets, never commit it.** `WOKWI_CLI_TOKEN` lives there;
  `.env` and `.env.*` are gitignored (with `!.env.example` carved out for an
  optional sample). If you ever paste a token in a commit message or a
  committed file, rotate it on the [Wokwi CI dashboard][dash].
- **Wokwi sim time runs faster than wall-clock for this RP2040 build.** See
  the [timing caveat](#timing-caveat-rp2040-in-wokwi). Write CLI/scenario
  assertions in terms of *deltas between snapshots*, not absolute `HH:MM:SS`.

[dash]: https://wokwi.com/dashboard/ci
