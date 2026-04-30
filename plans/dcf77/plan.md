# Plan: integrate a DCF77 receiver

Status: **draft, awaiting review** — no code written yet. This document is the
proposal; implementation should not start until each "Decision" below is
either explicitly accepted or overridden.

## 1. Goal

Make the clock self-correcting: instead of starting at the hard-coded
`INITIAL_TIME` (12:34:56) and drifting forever (or being nudged by the
button), pick up the real wall-clock time from a DCF77 longwave receiver
on a GPIO input and apply it to `ClockState` whenever a valid telegram
has been received. The button keeps working as a manual override / fallback.

The integration must:

- Compile and run on real Pico hardware against any common DCF77 receiver
  module (HKW, Conrad/DCF1, C-MAX CMMR-6P-60, ...).
- Run inside Wokwi for agent-driven tests, since there is no built-in
  Wokwi DCF77 part. The simulation strategy is one of the open decisions
  below (§3.2).
- Keep the host-testable / firmware-testable split that already separates
  `src/lib.rs` (pure logic) from `src/main.rs` + `src/bsp.rs` (HAL-bound),
  per `AGENTS.md` §"Code Style".
- Keep `./check.sh` green in <5s, and only push to `./run-sim.sh` for
  hardware-touching changes (RTIC wiring, pin assignment, the simulation
  source).

Non-goals (explicitly out of scope, see §10 for follow-up ideas):

- Showing a date or weekday on the matrix (the display today is
  `HH:MM:SS` only).
- DST / timezone arithmetic. DCF77 broadcasts CET/CEST already, the clock
  just shows whatever it receives.
- Leap-second handling beyond "ignore the announcement bit".
- Dealing with the receiver's antenna / RF front-end. We treat the
  module as a black box that gives us a clean(ish) digital pulse on a
  GPIO.

## 2. Background

### 2.1 DCF77 in 30 lines

DCF77 is a 77.5 kHz longwave transmitter in Mainflingen, Germany. A
typical receiver module demodulates the AM envelope and outputs one
1-second-wide pulse per second on a single digital pin. Signal shape
(active-HIGH idle, LOW pulses — the most common polarity, but inverted
modules exist; see §3.4):

```
bit 0  : LOW for 100 ms, then HIGH for 900 ms     (1-second slot)
bit 1  : LOW for 200 ms, then HIGH for 800 ms     (1-second slot)
minute : LOW kept low, no pulse during second 59  (~2 s gap; sync marker)
```

A full telegram is 60 bits transmitted over 60 seconds. Bits 20..58
encode minute / hour / day / weekday / month / year as BCD with three
even-parity bits (28, 36, 58). Bit 0 is always 0, bit 19 is always 1,
bit 59 is the gap. The encoded time is **the time at the start of the
next minute**, i.e. valid the instant the gap ends and the next bit-0
pulse begins.

| Bit range | Field             | Notes                              |
| --------- | ----------------- | ---------------------------------- |
| 0         | start             | always 0                           |
| 1..14     | reserved          | civil-defence / weather (ignore)   |
| 15        | call bit          | ignore                             |
| 16        | DST announce      | 1 in the hour before a DST change  |
| 17        | CEST flag         | 1 = summer (UTC+2)                 |
| 18        | CET flag          | 1 = winter (UTC+1) (XOR with 17)   |
| 19        | leap announce / start of time | always 1                  |
| 20..27    | minutes BCD + parity (28) | 4 ones + 3 tens, P even    |
| 29..35    | hours BCD + parity (36)   | 4 ones + 3 tens, P even    |
| 37..41    | day (BCD)         | 4 ones + 1 tens                    |
| 42..44    | weekday (1..7)    | 1 = Mon                            |
| 45..49    | month (BCD)       | 4 ones + 1 tens                    |
| 50..57    | year (BCD)        | year = 2000 + value                |
| 58        | parity for 37..57 | even                               |
| 59        | minute marker     | gap, no pulse                      |

(Authoritative reference: [Wikipedia: DCF77 time code](https://en.wikipedia.org/wiki/DCF77).)

### 2.2 Current architecture, abridged

(See `AGENTS.md` for the full version.) Relevant bits:

- `src/lib.rs` — pure-logic library, host-testable.
  - `clock::ClockState` — `HH:MM:SS` with `tick()` / `add_*()`.
  - `display::Framebuffer` + `clock_to_frame` adapter.
  - `font::Glyph` — 3×8 bitmaps.
  - `config` — tunables (`TICK_INTERVAL_US`, button timing, brightness, ...).
- `src/main.rs` — RTIC `#[app]`. Tasks today:
  - `timer_tick` (TIMER_IRQ_0, alarm0): 1 Hz, ticks the clock, repaints display.
  - `button_press` (IO_IRQ_BANK0): +1 minute on button down.
  - `button_repeat` (TIMER_IRQ_1, alarm1): accelerating auto-repeat.
  - `button_debounce` (TIMER_IRQ_2, alarm2): debounce window.
  - `update_display` (software task).
- `src/bsp.rs` — pin numbers, SPI, MAX7219 bring-up, owns alarms 0/1/2.
  RP2040 has 4 alarms total, so `alarm3` is **available** for new work.
- Wokwi side: `diagram.json` (parts/wires), `wokwi.toml` (firmware path),
  `scenario.yaml` (CLI test scenario), `tools/decode_screenshot.py`
  (firmware → display → PNG → decoded text round-trip).

## 3. Decisions to confirm before coding

Each row has a **recommended default**. If the user is happy with the
default, the implementation just proceeds. The plan should be re-read
once any of these changes — most of them have ripple effects.

### 3.1 Sync granularity: time only, or time + date

| Option                        | What changes                                                                 | Recommendation |
| ----------------------------- | ---------------------------------------------------------------------------- | -------------- |
| **A. Time only (`HH:MM`)**    | `clock::ClockState` keeps `(h, m, s)`. DCF77 only writes `h, m`, sets `s=0`. | ✅ **default**  |
| B. Time + date                | Extend `ClockState` with `(year, month, day, weekday)`. Display unchanged. Larger blast radius (golden bytes, decoder, fixtures). |   |

Rationale for A: the display has no date, so the date fields would only
be visible to host tests. Adding them is real work (golden-buffer
updates, decoder anchor, render fixture) for zero user-visible payoff
right now. Easy to extend later if a "show the date" feature lands.

> **Decision needed:** stick with A, or do B?

### 3.2 Wokwi simulation source for the DCF77 signal

There is **no built-in Wokwi DCF77 part**. `scenario.yaml` cannot drive
a GPIO pin directly — only `delay`, `set-control` (on parts that expose
controls like buttons / sliders), `expect-pin` (read), `wait-serial`,
`write-serial`, `take-screenshot`, `touch`. (Verified against
<https://docs.wokwi.com/wokwi-ci/automation-scenarios>.) So the signal
has to be generated **inside the Wokwi project**, by one of:

| Option                         | Sketch                                                                                                                                                                                 | Pros                                                                                            | Cons                                                                                                                                              |
| ------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| **A. Loopback transmitter on a Pico GPIO** | Add a `dcf77_tx` feature to the firmware itself. Behind a Cargo feature flag (`dcf77-loopback`), a separate RTIC task drives an output GPIO with the encoded telegram. In `diagram.json` we wire that output pin to the receiver input pin. | No new toolchain, no WASM, pure Rust. The encoder is host-testable like everything else. Same firmware artifact runs in Wokwi and on hardware (the `dcf77-loopback` feature is off in production builds). | The "transmitter" is the same MCU as the "receiver", which feels like cheating. But it still exercises the real pulse-timing path end-to-end inside the simulator. The encoder may share bugs with the decoder if we're not careful — mitigated by having an independent host-side encoder reference. | 
| B. Wokwi custom chip (C → WASM) | Write a `chips/dcf77/chip.c` + `chip.json` that emits the telegram on its `OUT` pin. Reference it from `diagram.json` as a `wokwi-custom-chip` part.                                    | "Cleanest" — the receiver firmware is unchanged, the chip is a drop-in DCF77 simulator. Reusable. | Requires Emscripten + an extra build step. New language in the repo. ~1-2 days of extra dev. The chip + the firmware decoder still need a shared spec to agree on.       |
| C. Wokwi custom chip (Rust → WASM) | Same as B but the chip is in Rust. Could in theory share an `Encoder` with the firmware via `wokwi_test::dcf77`.                                                                  | Pure Rust monorepo. Code reuse with the firmware encoder.                                       | Custom-chip Rust support is less documented than C. Still adds wasm-pack to the toolchain. Most of the cost of B without quite the polish.        |
| D. No Wokwi DCF77 sim          | Only host-test the decoder. Wokwi smoke-tests the existing button/display path; DCF77 stays untested in the integration loop.                                                          | Simplest.                                                                                       | We never see the decoder run end-to-end against a real GPIO until hardware bring-up. Loses the closed-loop "agent edits decoder, sim verifies" property of the project. |

**Recommendation: A (loopback transmitter behind a Cargo feature)**.
Reasons:

1. Keeps the toolchain to "rust + elf2uf2-rs" — no WASM build chain.
2. The encoder + decoder are both pure-Rust modules, so we get a
   host-side `encode → modulate → decoder.process(level, dt) → decoded
   frame → matches encoded` round-trip test for free, just like the
   existing `render → decode` round-trip in `check.sh`.
3. Wokwi just sees an extra wire in `diagram.json`; the firmware still
   builds to one .uf2.
4. Production firmware (no `dcf77-loopback` feature) doesn't ship the
   transmitter task at all — so the Pico's GPIO-output won't fight a
   real DCF77 receiver wired to that pin.

If the user wants the "real" simulator (option B/C) for documentation /
demo value, we can add it as a follow-up once the decoder + integration
land. The plan below assumes A.

> **Decision needed:** A (loopback) — confirm? Or pick B/C/D?

### 3.3 Polling vs. edge-triggered IRQ for the receiver pin

| Option                     | Sketch                                                                                                                                                                            | Recommendation |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------- |
| **A. Periodic poll on alarm3** | A new RTIC task `dcf77_sample` bound to `TIMER_IRQ_3`, scheduled every `DCF77_SAMPLE_US` (default `10_000` µs). Reads the pin level each time, feeds (level, dt) into the decoder. | ✅ **default**  |
| B. EdgeBoth IRQ on the pin | Add the pin to `IO_IRQ_BANK0` with EdgeRising + EdgeFalling enabled. The existing `button_press` ISR has to demux on which pin fired. Use the timer counter to measure dt.        |                |

Rationale for A:

- `IO_IRQ_BANK0` is already busy with the button's three-task debounce
  handshake (see `AGENTS.md` §"Common Gotchas"). Adding another
  pin-source to that ISR makes the demux logic significantly more
  fragile.
- DCF77's smallest meaningful pulse is 100 ms. Sampling at 10 ms gives
  10× oversampling — comfortable margin against noise / jitter, and the
  decoder's pulse-width measurement only needs ~±35 ms tolerance.
- 10 ms × 100 cycles/s × ~50 cycles per sample = ~5000 cycles/s on a
  125 MHz core = essentially free.
- alarm3 is the last unused hardware alarm on the RP2040 — a clean fit.

> **Decision needed:** A (10 ms polling on alarm3) — confirm?

### 3.4 Receiver polarity convention

| Option                          | Sketch                                                                                  | Recommendation |
| ------------------------------- | --------------------------------------------------------------------------------------- | -------------- |
| **A. Active-HIGH idle, LOW pulses** | Decoder treats LOW as "carrier reduced" (= pulse). Module idles HIGH between pulses. | ✅ **default**  |
| B. Hard-coded inverted          | Decoder treats HIGH as pulse.                                                           |                |
| C. Run-time configurable        | A `config::DCF77_ACTIVE_LOW_PULSE: bool` flag.                                           |                |

Rationale for A: this matches the most common modules (HKW, Conrad
DCF1, C-MAX CMMR-6P-60). Modules with inverted output are unusual; if
someone hits one, they can either pass the signal through a transistor
(real hw) or flip the constant in `config.rs`. Keeping it
non-runtime-configurable saves one branch per sample and keeps the
decoder math simple.

> **Decision needed:** A — confirm? Or want C for flexibility?

### 3.5 Pin assignment for the DCF77 input

The Pico has these free pins right now: GP0–GP14, GP20–GP24, GP26–GP28.
Used: GP15 (button), GP16/17/18/19 (SPI/CS), GP25 (LED).

| Option           | Why                                                                                              |
| ---------------- | ------------------------------------------------------------------------------------------------ |
| **GP14 (input)** | Adjacent to GP15 (button) on the breadboard / pinout, tidy wiring. ✅ **default**                |
| GP20             | Free, but on the opposite side of the chip from the button.                                      |
| GP26             | Has ADC, "wasted" on a digital input.                                                            |

For loopback (decision §3.2 A), we additionally need an **output** pin
to drive the simulated DCF77 line. **GP13** (also adjacent to GP14/GP15,
fits the same wiring corner) works. With the loopback feature on,
`diagram.json` adds `pico:GP13 → pico:GP14` as a single short wire.

> **Decision needed:** GP14 (in) + GP13 (loopback out) — confirm?

### 3.6 Sync application policy

When the decoder produces a valid frame, **when** do we apply it?

| Option                       | Sketch                                                                                                                                                              | Recommendation |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------- |
| **A. Apply on the falling edge that starts the next bit-0** | Buffer the decoded `(h, m)` when the frame finishes (end of bit 58 → start of 2 s gap). On the falling edge that ends the gap, set `ClockState` to `(h, m, 0)`.   | ✅ **default**  |
| B. Apply at frame complete   | On bit 58, immediately set `ClockState` to `(h, m, 59)` and let `tick()` roll into `(h, m+1, 0)` ~1 s later.                                                      |                |
| C. Apply only after N consecutive matching frames ("majority vote") | Decode three frames in a row, only sync if all three predict consistent times. Robust to single-bit corruption that survived parity by accident. | (later)         |

A is the textbook DCF77 timing. B is a 1-second-fuzzy approximation. C
is the production-grade "real DCF77 clock" approach but adds 2-3
minutes to first-sync; not worth the complexity for v1. Easy to add
later as a `config::DCF77_REQUIRED_CONSECUTIVE_FRAMES: u8 = 1` knob.

> **Decision needed:** A for v1, with C tracked as future work? Or jump straight to C?

### 3.7 Sync cadence

DCF77 broadcasts a fresh telegram every minute. Once we've achieved
sync, do we keep applying every valid frame, or only periodically?

| Option                                | Recommendation |
| ------------------------------------- | -------------- |
| **A. Apply every valid frame**        | ✅ **default** — cheap (a few writes through `clock.lock`), nothing to gain by skipping. |
| B. Apply once per N minutes / hours   |                |

> **Decision needed:** A — confirm?

## 4. Proposed design

(All under the assumption that the §3 defaults are accepted. If a
decision flips, I'll re-edit this section.)

### 4.1 New / changed modules

```
src/
├── dcf77.rs     # NEW: pure-logic decoder + (under feature) encoder
├── clock.rs     # ADD a `set_time(h, m, s)` mutator
├── config.rs    # ADD DCF77_SAMPLE_US, DCF77_PULSE_*_MIN/MAX_MS, DCF77_GAP_MIN_MS
├── bsp.rs       # ADD Dcf77InPin, (feature) Dcf77OutPin, Alarm3
├── main.rs      # ADD dcf77_sample task (+ feature: dcf77_tx task)
└── lib.rs       # ADD `pub mod dcf77;`
```

`Cargo.toml`: add a `[features]` block with `dcf77-loopback = []` (off
by default — production builds skip the transmitter task).

`diagram.json`: add nothing new in production, **or** (feature on) add
the wire `pico:GP13 → pico:GP14`.

### 4.2 `dcf77` library module — public API sketch

```rust
//! DCF77 telegram decoder (and, with `dcf77-loopback`, encoder).
//!
//! Pure-logic, no_std, no allocations, no embedded deps. The host can
//! `cargo test --lib` it; the firmware just polls `Decoder::sample`
//! once every ~10 ms.

use crate::clock::ClockState;

/// What `sample()` returns when a complete and valid frame has just
/// landed. Wraps the time the firmware should program into the clock
/// **at the next bit-0 falling edge** (which `Decoder` reports as the
/// `Some(_)` return value of the *next* `sample()` after the gap ends).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Frame {
    pub hours: u8,
    pub minutes: u8,
    // Optional: pub day / month / year / weekday — feature-gated, see §3.1.
}

/// Streaming pulse decoder. Fed level samples on a fixed cadence
/// (`config::DCF77_SAMPLE_US`) and emits `Some(Frame)` exactly once,
/// at the moment the firmware should apply the new time (= start of
/// the new minute, second 0).
pub struct Decoder { /* private */ }

impl Decoder {
    pub const fn new() -> Self { /* ... */ }

    /// Feed one sample.
    /// - `level`: the instantaneous pin level (`true` = HIGH = idle).
    /// - `dt_us`: time elapsed since the previous `sample()` call.
    /// Returns `Some(Frame)` only on a freshly-valid frame whose
    /// minute boundary has just elapsed. Otherwise `None`.
    pub fn sample(&mut self, level: bool, dt_us: u32) -> Option<Frame>;

    /// Reset to the "looking for sync" state. Useful in tests and on
    /// receiver hot-plug.
    pub fn reset(&mut self);

    /// For tests / introspection.
    pub fn state(&self) -> DecoderState;
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum DecoderState {
    SearchingForGap,        // haven't seen a 2-s LOW yet
    AwaitingFirstBit,       // gap ended, next falling edge starts bit 0
    CollectingBits { idx: u8 },  // 0..60
    FrameReady,             // a frame just emitted, idle until next sample
}
```

Internal layout:

- A small bit buffer `[bool; 60]`.
- A "current pulse" tracker: which level we're in, and how long we've
  been in it (in ms, computed from accumulated `dt_us`).
- On a HIGH→LOW transition: reset the LOW timer.
- On a LOW→HIGH transition (= end of a pulse): classify the pulse.
  - 65–135 ms → bit 0
  - 165–235 ms → bit 1
  - <65 or 135–165 or >235 → glitch, drop the bit and resync.
  - The 1500 ms+ "still LOW" timer is what detects the minute marker;
    it's checked every sample, not on transition.
- On accumulated 60 bits + valid parity + valid BCD ranges → emit.

Three `#[cfg(test)] mod tests` blocks:

1. Pure-data decoders: feed canned `[bool; 60]` arrays through the
   bit-level decoder (parity / BCD / range checks) and assert the
   `Frame` they produce. Includes "all zeros", "12:34", "23:59",
   parity errors, BCD digits >9, empty year, etc.

2. Pulse-stream decoders: feed canned `[(level, dt_us)]` sequences
   through `sample()`, assert the right `DecoderState` transitions
   and the right `Some(Frame)` at the right call.

3. **Encode → modulate → decode round-trip** (host only, behind
   `#[cfg(test)]`, mirrors the existing `render → decode` round-trip
   in `check.sh`):
   - A test-only `Encoder::frame_to_bits(h, m) -> [bool; 60]` (with
     correct parity + BCD packing).
   - A test-only `Encoder::bits_to_pulses(&[bool; 60]) -> [(level, dt_us); …]`
     producing the level/dt stream that real DCF77 would.
   - Run that stream through `Decoder::sample` and assert the emitted
     `Frame` round-trips. Cover (00:00), (12:34), (23:59), and a
     handful of times that exercise BCD carries (e.g. 09:09 → 10:00).

If §3.2 A wins, the encoder module gets `pub`-promoted out of `cfg(test)`
behind the `dcf77-loopback` feature, so the TX task can use it. The
test code uses it unconditionally.

### 4.3 `clock` additions

Add one method, no struct field changes:

```rust
impl ClockState {
    /// Force the clock to a specific time. Used by DCF77 sync.
    /// Inputs are clamped just like `new()`.
    pub fn set_time(&mut self, hours: u8, mins: u8, secs: u8) {
        self.hours = hours % 24;
        self.mins  = mins  % 60;
        self.secs  = secs  % 60;
    }
}
```

Tests:

- `set_time(13, 37, 0)` → fields read back `(13, 37, 0)`.
- `set_time(99, 99, 99)` → fields are clamped consistently with `new()`.
- After `set_time(10, 30, 0)` + `tick()`, fields are `(10, 30, 1)` — i.e.
  no leftover state from before the sync.

### 4.4 `config` additions

```rust
/// DCF77 polling cadence (microseconds). 10 ms gives ~10× oversampling
/// of the shortest meaningful pulse (100 ms).
pub const DCF77_SAMPLE_US: u32 = 10_000;

/// Pulse-width window for "bit = 0" (milliseconds, inclusive).
pub const DCF77_BIT0_MIN_MS: u32 = 65;
pub const DCF77_BIT0_MAX_MS: u32 = 135;
/// Pulse-width window for "bit = 1" (milliseconds, inclusive).
pub const DCF77_BIT1_MIN_MS: u32 = 165;
pub const DCF77_BIT1_MAX_MS: u32 = 235;
/// "LOW for at least this long" => minute marker (gap detected).
pub const DCF77_GAP_MIN_MS: u32 = 1_500;
```

(Loopback feature only:) `DCF77_TX_PULSE_BIT0_US`, `DCF77_TX_PULSE_BIT1_US`,
nominal 100 ms / 200 ms.

### 4.5 `bsp` additions

```rust
pub type Dcf77InPin = Pin<Gpio14, FunctionSio<SioInput>, PullUp>;
#[cfg(feature = "dcf77-loopback")]
pub type Dcf77OutPin = Pin<Gpio13, FunctionSio<SioOutput>, PullDown>;
pub use rp_pico::hal::timer::Alarm3;

pub struct Board {
    // ... existing fields ...
    pub dcf77_in: Dcf77InPin,
    #[cfg(feature = "dcf77-loopback")]
    pub dcf77_out: Dcf77OutPin,
    pub alarm3: Alarm3,
}
```

`Board::take` adds:

```rust
let mut alarm3 = timer.alarm_3().unwrap();
alarm3.schedule(DCF77_SAMPLE_US.micros()).unwrap();
alarm3.enable_interrupt();

let dcf77_in: Dcf77InPin = pins.gpio14.into_pull_up_input();
#[cfg(feature = "dcf77-loopback")]
let dcf77_out: Dcf77OutPin = pins.gpio13.into_push_pull_output();
```

No GPIO IRQ wiring — we're polling, not edge-triggering.

### 4.6 `main.rs` (RTIC) additions

New shared resource: nothing new — `clock` is already shared. Add a
`Local` slot for the `Decoder`:

```rust
#[local]
struct Local {
    // ... existing ...
    dcf77_decoder: dcf77::Decoder,
    dcf77_in: bsp::Dcf77InPin,
    alarm3: bsp::Alarm3,
    #[cfg(feature = "dcf77-loopback")]
    dcf77_tx: dcf77_tx::TxState, // small TX state machine, see §4.7
    #[cfg(feature = "dcf77-loopback")]
    dcf77_out: bsp::Dcf77OutPin,
}
```

New task:

```rust
#[task(binds = TIMER_IRQ_3, priority = 1,
       shared = [clock],
       local  = [dcf77_decoder, dcf77_in, alarm3])]
fn dcf77_sample(mut ctx: dcf77_sample::Context) {
    ctx.local.alarm3.clear_interrupt();
    let _ = ctx.local.alarm3.schedule(DCF77_SAMPLE_US.micros());

    let level = ctx.local.dcf77_in.is_high().unwrap_or(true);
    if let Some(frame) = ctx.local.dcf77_decoder.sample(level, DCF77_SAMPLE_US) {
        ctx.shared.clock.lock(|c| c.set_time(frame.hours, frame.minutes, 0));
        update_display::spawn().ok();
    }
}
```

That's the entire receive path. The button tasks are untouched.

### 4.7 (Loopback feature) TX task

Behind `#[cfg(feature = "dcf77-loopback")]`. Uses `alarm3` … wait, the
RP2040 only has 4 alarms and we just spent the last one on RX polling.
Two cleanest options:

| Option                                           | Note |
| ------------------------------------------------ | ---- |
| **A. Drive TX from the existing 1 Hz `timer_tick`** | The TX state machine just toggles `dcf77_out` according to "where in the 1-second slot are we?" — but `timer_tick` only fires once per second, so we can't do sub-second pulse-width modulation directly there. ❌ |
| **B. Reuse `dcf77_sample` (alarm3) as a 10 ms tick that drives both RX poll AND TX state machine.** | Single 10 ms task, one `if cfg!(feature)` block, no extra alarms. ✅ default. |
| C. Move TX to a software task spawned by the 1 Hz tick that re-arms itself. | Possible but more wiring. |

Going with B: the `dcf77_sample` task, when the feature is on,
additionally walks a small state machine that drives `dcf77_out`
according to the encoded telegram for the *current* `ClockState` (so
the TX always broadcasts the firmware's idea of "current time"). With
10 ms granularity, pulse widths are quantized to 10 ms steps, which
fits comfortably inside the decoder's ±35 ms tolerance windows.

The TX state machine (`dcf77_tx` submodule, host-testable):

```rust
#[cfg(feature = "dcf77-loopback")]
pub struct TxState { /* current second 0..60, ms-into-second 0..1000, encoded bits */ }

#[cfg(feature = "dcf77-loopback")]
impl TxState {
    pub const fn new() -> Self;

    /// Advance the TX clock by `dt_ms` and return the desired output
    /// pin level (true = HIGH = idle, false = LOW = pulse). Re-encodes
    /// the next minute's bits when crossing the second-59 → second-0
    /// boundary, given the supplied `(h, m)`.
    pub fn step(&mut self, dt_ms: u32, current_h: u8, current_m: u8) -> bool;
}
```

Tests for this live next to the encoder, in `dcf77.rs::tests`.

### 4.8 `diagram.json`

Production: **unchanged**. The DCF77 input pin (GP14) is wired by
whoever attaches a real receiver module on real hw. In the simulator,
GP14 reads idle-HIGH (because of the internal pull-up), the decoder
stays in `SearchingForGap`, and the clock keeps ticking from
`INITIAL_TIME` exactly as today. So the existing scenario.yaml
(button + display) is unaffected.

With `dcf77-loopback`: add one wire `pico:GP13 → pico:GP14`. Could
either:

- Edit `diagram.json` in place and gate it via a build-time choice
  (Wokwi reads the file as-is, so this needs a separate diagram).
- Maintain a second diagram `diagram.dcf77.json` + a second
  `wokwi.dcf77.toml`, and have a new `run-sim-dcf77.sh` point at them.

The second option is cleaner — keeps the production sim untouched and
makes the loopback test a separate, opt-in pipeline. Recommended.

## 5. Testing strategy

Three layers, mirroring the project's existing layered approach.

### 5.1 Host-side unit tests (in `check.sh`, fast)

All new tests run via `cargo test --lib --target=<host>`:

- `clock::tests::set_time_*` — clamps, no-leftover-state.
- `dcf77::tests::bits_*` — parity / BCD / range checks on raw `[bool; 60]`.
- `dcf77::tests::stream_*` — pulse-level state-machine tests on canned
  `[(level, dt_us)]` sequences (sync acquisition, mid-frame glitch
  resync, bad parity rejection, bad BCD rejection, exactly-on-boundary
  pulse widths).
- `dcf77::tests::encode_decode_round_trip_*` — mirror of the
  `render → decode` round-trip in `check.sh`. Uses the encoder +
  modulator to produce a stream, runs it through the decoder, asserts
  the decoded `Frame` matches the input. Covers (00:00), (09:09),
  (10:00), (12:34), (23:59).

No new external Python tooling needed — encoder and decoder are both
Rust, so the round-trip is just another `#[test]` in the same module.

### 5.2 Decoder anchor (analogue to `tools/check_decoder.py`)

If we ever add a non-Rust component (we don't, in plan A), we'd need a
cross-language anchor. For now, **skip** — the entire pipeline is in
one language.

### 5.3 Wokwi integration test (in `./run-sim.sh`, slow path, hardware-touching)

Two scenarios, the second only if `dcf77-loopback` is on:

1. **Existing `scenario.yaml`** — runs unchanged. With DCF77 feature
   off and GP14 floating-pulled-high, the decoder is silent and the
   button + display behaviour is identical to today. Anchor against
   regression: same `before.png` / `after.png` deltas as before.

2. **New `scenario.dcf77.yaml`** (only with `dcf77-loopback`):

   ```yaml
   name: 'DCF77 sync via GP13->GP14 loopback'
   version: 1
   steps:
     - delay: 100ms                     # init
     - take-screenshot:
         part-id: matrix1
         save-to: target/wokwi/dcf77-before.png
     # Wait for at least one full DCF77 minute (60 sim-seconds), plus
     # ~2 s of pre-sync gap-search. The Wokwi RP2040 sim runs ~40x faster
     # than wall-clock (per AGENTS.md timing caveat), so this is ~1.5 s
     # wall.
     - delay: 65000ms
     - take-screenshot:
         part-id: matrix1
         save-to: target/wokwi/dcf77-after.png
   ```

   Then in `run-sim-dcf77.sh`:

   ```sh
   ./build.sh --features dcf77-loopback
   wokwi-cli --diagram-file diagram.dcf77.json \
             --scenario scenario.dcf77.yaml ...
   python3 tools/decode_screenshot.py target/wokwi/dcf77-{before,after}.png
   ```

   Assertion: the after-screenshot decodes to a time consistent with
   the firmware's own TX (which is just `INITIAL_TIME` advancing).
   Specifically, `after` minutes should match TX's encoded
   minute = `INITIAL_TIME.minute + sim_seconds_elapsed / 60`. Phrased
   as a delta to be robust against the 40× sim-time-over-wall quirk:
   the *decoded minute* in `after` should equal the *raw display
   minute* in `after` (i.e. the receiver and the rest of the firmware
   agree).

   This is a low-confidence test by itself (the firmware is feeding
   itself), but combined with the host-side encode→decode round-trip
   it shows the actual GPIO + RTIC + alarm3 wiring works in the
   simulator, not just on paper.

### 5.4 `check.sh` and `run-sim.sh` updates

`check.sh`:

- Add nothing new explicitly — `cargo test --lib` already picks up the
  new `#[cfg(test)]` modules.
- Optionally: add one extra fixture to the existing
  `render → decode` block, e.g. `01:23:45`, just to make sure
  `set_time` integration works in `clock_to_frame`. This is a host-only
  change; ~50 ms extra wall.

`run-sim.sh`: untouched. New `run-sim-dcf77.sh` is the opt-in slow path
for the loopback scenario. Both still gated on `WOKWI_CLI_TOKEN`.

`AGENTS.md`: append a new section "DCF77 sync" with:

- a table row in "Iteration loop" classifying changes,
- a pointer to `plans/dcf77/plan.md` for design rationale,
- a Common Gotchas entry: "RX polling and TX (loopback) share `alarm3`;
  splitting them needs a second alarm or a 2× sample rate."

## 6. Implementation phases

I propose splitting the work into commits that each individually pass
`./check.sh`, so reviews can be incremental. Suggested order:

1. **`clock::set_time` + tests.** (~30 lines.) Lands the API DCF77
   needs without touching anything else. `check.sh` green.

2. **`dcf77` module — bit decoder.** (~150 lines incl. tests.) Pure
   data decoder: `[bool; 60] → Result<Frame, _>`. All parity / BCD /
   range tests. `check.sh` green.

3. **`dcf77` module — pulse decoder.** (~200 lines incl. tests.) The
   `Decoder::sample` state machine. Stream tests. `check.sh` green.

4. **`dcf77` module — encoder + round-trip test.** (~150 lines.) The
   encoder, modulator, and `encode_decode_round_trip_*` tests.
   `check.sh` green.

5. **`bsp` + `main` + `config` wiring (RX only).** (~80 lines across
   files.) Adds `Dcf77InPin`, `alarm3`, the `dcf77_sample` task. With
   `dcf77-loopback` off, the decoder runs but only ever sees idle-HIGH
   in the sim — no behaviour change observable, but `cargo build
   --release` and `./check.sh` both green.

6. **Loopback TX behind `dcf77-loopback` feature.** (~150 lines.) TX
   state machine, second `diagram.dcf77.json`, `wokwi.dcf77.toml`,
   `run-sim-dcf77.sh`, `scenario.dcf77.yaml`. `./run-sim.sh` (existing)
   still green. `./run-sim-dcf77.sh` shows decoded minute increments
   correctly.

7. **`AGENTS.md` updates.** (~50 lines.) The "DCF77 sync" section,
   gotchas, iteration-loop table row.

Each phase ends with `./check.sh`. Phases 5 and 6 also end with
`./run-sim.sh` (and 6 with `./run-sim-dcf77.sh`).

## 7. Risks and mitigations

| Risk                                                                                   | Mitigation                                                                                                                                                                                                                                                                            |
| -------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Wokwi's 40× sim-time-over-wall quirk distorts pulse widths.                            | Pulse widths are measured *in the sim's time domain* (via the timer hardware = `DCF77_SAMPLE_US` accumulation), so they're internally consistent. A 100 ms "real" pulse looks like 100 ms to the firmware regardless of how fast wall-clock runs.                                     |
| TX encoder and RX decoder share an unwritten spec → bugs that round-trip cleanly but mismatch real DCF77 receivers. | The host-side encode/decode round-trip alone won't catch this. Defence: keep the encoder on a strict diet — only what the protocol says, with row-by-row Wikipedia citations in comments — and validate against one known-good reference frame from a real DCF77 trace if available. |
| `alarm3` re-entry between RX poll and TX state machine drops samples.                  | Both run inside the same RTIC task at `priority = 1`, so by definition no re-entry. The whole task takes <50 µs of work, well under the 10 ms reschedule window.                                                                                                                       |
| Adding the GP14 input pin + always-on polling task on hardware where nothing's wired drains battery / generates spurious decodes. | The pin is pull-up — idle HIGH — the decoder stays in `SearchingForGap` indefinitely with zero state churn. Polling is always-on but cheap (~50 µs / 10 ms = 0.5% CPU). On a tight power-budget build, can be gated behind a `dcf77-rx` feature too.                                |
| Real DCF77 modules with inverted output silently produce 100% bit errors.              | Documented in `AGENTS.md` "Common Gotchas". Easy fix: invert at the pin (`!level`) — call out as a one-line config knob if it ever bites.                                                                                                                                              |
| First-sync latency: from cold boot, the user sees `INITIAL_TIME` for up to ~2 minutes before sync. | Acceptable for v1 (matches every DCF77 wall clock ever made — they all flash a "no signal" indicator for the first minute). Can add a "syncing…" indicator on the display in a follow-up.                                                                                              |
| Cargo feature flags + `#[cfg(feature = ...)]` cross-cutting concerns are easy to get wrong (the firmware compiles for one config but bit-rots for another). | `check.sh` already builds with the default features. Add one extra line: `cargo build --release --features dcf77-loopback`. Fast (~3 s). Catches feature-gated regressions.                                                                                                          |

## 8. Out of scope / future work

- **Date support on the display** — would unblock proper DCF77 use (year /
  month / day are wasted today). Plan: extend `ClockState` with date
  fields, add a "date page" that the display swaps to via the button or
  on a timer.
- **DST awareness** — once dates land, decoding bits 16/17/18 is trivial
  and lets us flip CET ↔ CEST automatically.
- **Majority voting** (§3.6 C) — a `DCF77_REQUIRED_CONSECUTIVE_FRAMES`
  knob that requires N matching frames before applying. Trivially
  retrofittable.
- **"Real" Wokwi DCF77 part** (§3.2 B/C) — a custom WASM chip would
  make the sim more honest. Worth doing once the Rust path is mature
  and we want a community-shareable artifact.
- **Sync status on the display** — a 1-pixel "lock" indicator in a
  spare corner of the matrix while the decoder is in `SearchingForGap`.
- **UART telemetry** — once `wokwi-virtual-uart` is wired (per
  AGENTS.md "Adding agent-visible serial"), emit `SYNC HH:MM:SS` and
  `LOST SYNC` lines so `wokwi-cli --expect-text` can drive sharper
  assertions than screenshot delta.

## 9. Summary of decisions for the user

The plan above will be **executed as-is** if all of these defaults are
acceptable. Any "no" turns into a re-edit before we start coding.

| #   | Decision                                                                          | Default                                                            | Override?          |
| --- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------ | ------------------ |
| 3.1 | Sync time only, or time + date?                                                   | Time only (`HH:MM`)                                                |                    |
| 3.2 | Wokwi simulation source for the DCF77 signal?                                     | A — firmware-internal loopback behind `dcf77-loopback` feature     |                    |
| 3.3 | Polling vs. edge-triggered?                                                       | A — 10 ms polling on `alarm3`                                      |                    |
| 3.4 | Receiver polarity convention?                                                     | A — active-HIGH idle, LOW pulses                                   |                    |
| 3.5 | Pin assignment?                                                                   | GP14 (RX), GP13 (TX, loopback only)                                |                    |
| 3.6 | When to apply a decoded frame?                                                    | A — on the falling edge that starts the next bit-0; v1 single-frame, no majority vote |  |
| 3.7 | Sync cadence?                                                                     | A — every valid frame                                              |                    |

Awaiting confirmation (or overrides) on these before starting phase 1.
