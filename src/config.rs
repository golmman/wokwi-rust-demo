//! Runtime tunables for the clock firmware.
//!
//! All magic numbers that affect behavior live here. Hardware pin choices
//! live in `src/bsp.rs`; pixel layout / chain-length lives next to the
//! code that consumes it (see `display::CHAIN_LEN`).
//!
//! Edit a constant here, rebuild, re-run `./run-sim.sh` to see the effect.

/// Period of the seconds-tick interrupt, in microseconds. The RP2040 TIMER
/// counts at 1 MHz, so `1_000_000` ticks == 1 wall-clock second.
pub const TICK_INTERVAL_US: u32 = 1_000_000;

/// How long the button must be held before auto-repeat kicks in
/// (microseconds).
pub const BUTTON_REPEAT_INITIAL_US: u32 = 500_000;

/// Length of the post-press / post-release debounce window
/// (microseconds). The GPIO IRQ stays masked for this long after a press
/// (and again after a release detected during auto-repeat) so contact
/// bounce can't fire spurious events. Should be a few × the switch's
/// physical bounce time (typically 1–10 ms) but short enough that
/// realistic re-clicks aren't blocked.
pub const BUTTON_DEBOUNCE_US: u32 = 30_000;

/// Lower bound on the auto-repeat period (microseconds). Each repeat
/// shrinks the period by `BUTTON_REPEAT_DECAY_NUM / BUTTON_REPEAT_DECAY_DEN`
/// until it hits this floor.
pub const BUTTON_REPEAT_MIN_US: u32 = 20_000;

/// Auto-repeat acceleration factor: `next = current * NUM / DEN`.
pub const BUTTON_REPEAT_DECAY_NUM: u32 = 8;
pub const BUTTON_REPEAT_DECAY_DEN: u32 = 10;

/// SPI bus frequency for the MAX7219 chain (Hz). The MAX7219 datasheet
/// allows up to 10 MHz; 2 MHz is a comfortable margin for breadboard wiring.
pub const SPI_FREQ_HZ: u32 = 2_000_000;

/// Number of MAX7219 modules daisy-chained as one logical 32x8 display.
/// Must match `diagram.json`'s `"chain": "..."` attribute and
/// `display::prepare_buffer`'s fixed-size buffer dimensions.
pub const CHAIN_LEN: usize = 4;

/// Display brightness, range `0x0..=0xF` (4 bits). `0x0` is the dimmest
/// usable setting; raise for more pop on real hardware.
pub const DISPLAY_INTENSITY: u8 = 0x0;

/// Boot-up wall-clock value (hours, minutes, seconds). Chosen so Wokwi
/// smoke-tests have a recognizable starting point.
pub const INITIAL_TIME: (u8, u8, u8) = (12, 34, 56);
