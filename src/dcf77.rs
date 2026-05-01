//! DCF77 telegram decoder.
//!
//! [DCF77](https://en.wikipedia.org/wiki/DCF77) is a 77.5 kHz longwave time
//! signal broadcast from Mainflingen, Germany. A typical receiver module
//! demodulates the AM envelope and outputs one second-wide pulse per second
//! on a single digital pin.
//!
//! This module is split into two layers:
//!
//! - [`decode_bits`] — pure-data decoder. Validates a fully-received
//!   `[bool; 60]` telegram (start bit, time marker, even-parity over the
//!   minute and hour fields, BCD nibble ranges) and returns a [`Frame`]
//!   carrying `(hours, minutes)`.
//! - [`Decoder`] — streaming pulse decoder fed by `(level, dt_us)`
//!   samples from a polled GPIO; reconstructs the 60-bit telegram, runs
//!   it through `decode_bits`, and emits a `Frame` exactly when the
//!   firmware should apply the new time (the falling edge that ends the
//!   minute-marker gap).
//!
//! Pure logic, `no_std`, no allocations, no embedded deps — host-testable.

use crate::config::{
    DCF77_BIT0_MAX_MS, DCF77_BIT0_MIN_MS, DCF77_BIT1_MAX_MS, DCF77_BIT1_MIN_MS, DCF77_GAP_MIN_MS,
};

// The encoder is always compiled (it's tiny and host tests need it).
// Production firmware that doesn't enable the `dcf77-loopback` feature
// holds an `Option<TxState>::None`, so the encoder code is reachable
// only via `Some(...)` — LTO removes it from the final image.
mod encoder {
    /// Encode `(hours, minutes)` into a 60-bit DCF77 telegram.
    ///
    /// Produces the exact bit pattern a real DCF77 transmitter would
    /// broadcast for this `(h, m)` at minute boundaries: start bit at 0,
    /// start-of-time marker at 1, BCD-packed minutes + hours with
    /// even-parity guards, everything else (date/weekday/year and their
    /// parity) left at zero.
    ///
    /// Inputs are clamped (`hours % 24`, `minutes % 60`) so a caller
    /// can't produce a telegram that the decoder would reject on range.
    pub fn encode_bits(hours: u8, minutes: u8) -> [bool; 60] {
        let hours = hours % 24;
        let minutes = minutes % 60;

        let mut bits = [false; 60];
        bits[20] = true;

        write_bcd_digit(&mut bits, 21, 4, minutes % 10);
        write_bcd_digit(&mut bits, 25, 3, minutes / 10);
        bits[28] = even_parity_bit(&bits, 21, 7);

        write_bcd_digit(&mut bits, 29, 4, hours % 10);
        write_bcd_digit(&mut bits, 33, 2, hours / 10);
        bits[35] = even_parity_bit(&bits, 29, 6);

        bits
    }

    /// Write a BCD digit `value` into `bits[start..start+len]` LSB-first.
    /// `value` must already fit in `len` bits; callers pass at most 9.
    fn write_bcd_digit(bits: &mut [bool; 60], start: usize, len: usize, value: u8) {
        for i in 0..len {
            bits[start + i] = ((value >> i) & 1) != 0;
        }
    }

    /// Compute the even-parity bit that, appended to the data slice
    /// `bits[start..start+len]`, makes the total count of `1`s even.
    fn even_parity_bit(bits: &[bool; 60], start: usize, len: usize) -> bool {
        bits[start..start + len].iter().filter(|&&b| b).count() & 1 == 1
    }
}

pub use encoder::encode_bits;

/// DCF77 loopback transmitter state machine.
///
/// Drives an output GPIO with the encoded telegram for the firmware's
/// current `(hours, minutes)`, allowing the same firmware to be its
/// own DCF77 source inside the Wokwi simulator (where there's no
/// built-in DCF77 part).
///
/// Production firmware (without the `dcf77-loopback` Cargo feature)
/// stores this in an `Option<TxState>::None` so the type is reachable
/// only through `Some(...)` and LTO drops it from the binary.
///
/// Designed to be called from the same 10 ms `dcf77_sample` task that
/// drives the receiver; both share `alarm3`. Pulse widths are
/// quantized to 10 ms steps, which fits comfortably inside the
/// decoder's ±35 ms tolerance windows.
pub struct TxState {
    /// Current minute's encoded telegram (refreshed at every
    /// second-59 → second-0 boundary).
    bits: [bool; 60],
    /// Current second within the minute (0..=59).
    second: u8,
    /// Microseconds elapsed inside the current second (0..1_000_000).
    second_progress_us: u32,
}

impl TxState {
    /// Fresh transmitter parked in bit 59 (the minute marker / gap), so
    /// the decoder sees ~1 second of clean idle-HIGH at boot before the
    /// first pulse arrives — a built-in initial sync window.
    pub const fn new() -> Self {
        Self {
            bits: [false; 60],
            second: 59,
            second_progress_us: 0,
        }
    }

    /// Advance the TX clock by `dt_us` microseconds and return the
    /// desired output pin level (`true` = HIGH = idle, `false` = LOW =
    /// pulse). At every minute boundary the bits buffer is re-encoded
    /// from the supplied `(current_h, current_m)`. Per the DCF77
    /// protocol the encoded value names the time at the **start of
    /// the next minute** (the moment the receiver applies the frame),
    /// so we forecast `(current_h, current_m + 1)` with rollover.
    pub fn step(&mut self, dt_us: u32, current_h: u8, current_m: u8) -> bool {
        self.second_progress_us = self.second_progress_us.saturating_add(dt_us);
        while self.second_progress_us >= 1_000_000 {
            self.second_progress_us -= 1_000_000;
            self.second = (self.second + 1) % 60;
            if self.second == 0 {
                let mut h = current_h;
                let mut m = current_m.saturating_add(1);
                if m >= 60 {
                    m = 0;
                    h = h.saturating_add(1) % 24;
                }
                self.bits = encode_bits(h, m);
            }
        }

        // Bit 59 is the minute marker — no pulse, idle HIGH.
        if self.second >= 59 {
            return true;
        }
        // Bits 0..=58: LOW pulse for 100 ms (bit = 0) or 200 ms
        // (bit = 1) at the start of the second, then HIGH for the
        // rest of the slot.
        let pulse_us = if self.bits[self.second as usize] {
            200_000
        } else {
            100_000
        };
        self.second_progress_us >= pulse_us
    }
}

impl Default for TxState {
    fn default() -> Self {
        Self::new()
    }
}

/// Decoded `(hours, minutes)` from a DCF77 telegram. Per the protocol the
/// encoded fields name the time at the **start of the next minute** —
/// i.e. the moment the post-bit-58 minute-marker gap ends. The streaming
/// `Decoder` is responsible for surfacing the frame at exactly that
/// instant; this struct itself just carries the values.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Frame {
    pub hours: u8,
    pub minutes: u8,
}

/// Why a 60-bit DCF77 telegram failed to decode.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum DecodeError {
    /// Bit 0 was `1` (must be `0` per protocol).
    BadStartBit,
    /// Bit 20 was `0` (must be `1` per protocol — start-of-time marker).
    BadTimeMarker,
    /// Even-parity check over bits 21..=28 failed.
    MinuteParity,
    /// Even-parity check over bits 29..=35 failed.
    HourParity,
    /// A BCD low nibble decoded to a value > 9.
    BadBcd,
    /// Decoded minutes value was >= 60.
    MinuteRange,
    /// Decoded hours value was >= 24.
    HourRange,
}

/// Decode a fully-received 60-bit DCF77 telegram into a [`Frame`].
///
/// Only the time fields are validated and decoded; the date / weekday /
/// year fields and their parity bit (58) are intentionally ignored — the
/// firmware's display has no date today (see plan §3.1).
///
/// Bit layout (LSB-first within each BCD nibble):
///
/// | bits     | field                    |
/// | -------- | ------------------------ |
/// | 0        | start (must be 0)        |
/// | 1..=19   | reserved / DST / leap    |
/// | 20       | start-of-time (must be 1)|
/// | 21..=24  | minute ones (BCD, weights 1,2,4,8)    |
/// | 25..=27  | minute tens (BCD, weights 10,20,40)   |
/// | 28       | minute parity (even)     |
/// | 29..=32  | hour ones (BCD, weights 1,2,4,8)      |
/// | 33..=34  | hour tens (BCD, weights 10,20)        |
/// | 35       | hour parity (even)       |
/// | 36..=58  | date + weekday + year + parity (ignored)  |
/// | 59       | minute marker (gap)      |
pub fn decode_bits(bits: &[bool; 60]) -> Result<Frame, DecodeError> {
    if bits[0] {
        return Err(DecodeError::BadStartBit);
    }
    if !bits[20] {
        return Err(DecodeError::BadTimeMarker);
    }

    // Even-parity: count of `1`s over data + parity bit must be even.
    if count_ones(bits, 21, 8) & 1 != 0 {
        return Err(DecodeError::MinuteParity);
    }
    if count_ones(bits, 29, 7) & 1 != 0 {
        return Err(DecodeError::HourParity);
    }

    let min_low = bcd_nibble(bits, 21, 4);
    let min_high = bcd_nibble(bits, 25, 3);
    if min_low > 9 {
        return Err(DecodeError::BadBcd);
    }
    let minutes = min_high * 10 + min_low;
    if minutes >= 60 {
        return Err(DecodeError::MinuteRange);
    }

    let hour_low = bcd_nibble(bits, 29, 4);
    let hour_high = bcd_nibble(bits, 33, 2);
    if hour_low > 9 {
        return Err(DecodeError::BadBcd);
    }
    let hours = hour_high * 10 + hour_low;
    if hours >= 24 {
        return Err(DecodeError::HourRange);
    }

    Ok(Frame { hours, minutes })
}

fn count_ones(bits: &[bool; 60], start: usize, len: usize) -> usize {
    bits[start..start + len].iter().filter(|&&b| b).count()
}

fn bcd_nibble(bits: &[bool; 60], start: usize, len: usize) -> u8 {
    let mut acc: u8 = 0;
    for i in 0..len {
        if bits[start + i] {
            acc |= 1 << i;
        }
    }
    acc
}

/// Externally-visible state of a streaming [`Decoder`], for
/// tests / introspection.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum DecoderState {
    /// No sync yet — waiting for a sustained HIGH of at least
    /// `DCF77_GAP_MIN_MS` ms to identify the minute-marker gap.
    SearchingForGap,
    /// Gap observed; the next falling edge starts bit 0 of a new frame.
    AwaitingFirstBit,
    /// Bits 0..=58 are being collected; `idx` is the count filled so far
    /// (0..=59). A fresh sync after the initial gap starts at `idx = 0`;
    /// `idx = 59` means all 59 pulse-bits have been captured and we are
    /// waiting for the end-of-frame gap.
    CollectingBits { idx: u8 },
}

/// Internal phase of the pulse-level state machine. Finer-grained than
/// [`DecoderState`]: distinguishes "currently in a LOW pulse" from
/// "between pulses on HIGH", which matters for edge handling but not for
/// external observers.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Phase {
    /// Looking for initial sync (no gap observed yet, or a glitch
    /// forced a resync).
    WaitingForGap,
    /// Gap observed; waiting for the falling edge that starts bit 0.
    AwaitingFirstBit,
    /// In the middle of a LOW pulse; measuring duration.
    InPulse,
    /// Between pulses (HIGH portion of a 1-s slot, or the end-of-frame
    /// gap if `bit_idx == 59`).
    BetweenPulses,
}

/// Streaming DCF77 pulse decoder.
///
/// Fed `sample(level, dt_us)` at a fixed cadence (`config::DCF77_SAMPLE_US`),
/// it reconstructs the 60-bit telegram from pulse widths, validates it
/// via [`decode_bits`], and returns `Some(Frame)` exactly once per valid
/// minute — at the moment the firmware should program the new
/// `(hours, minutes, 0)` into the clock (the falling edge that ends the
/// minute-marker gap).
pub struct Decoder {
    /// Pin level observed on the previous sample. Initialised `true`
    /// (idle HIGH); a first sample of `false` will register a falling
    /// edge, which is a no-op while `phase == WaitingForGap`.
    level: bool,
    /// Microseconds accumulated at `level` (reset on every level change).
    dur_us: u32,
    /// Bit buffer. Only indices 0..=58 are meaningful — bit 59 is the
    /// gap and has no pulse. [`decode_bits`] ignores bit 59.
    bits: [bool; 60],
    /// Number of bits collected so far in the current frame
    /// (0..=59). Reaching 59 means "wait for the gap, then emit".
    bit_idx: u8,
    phase: Phase,
}

impl Decoder {
    /// Fresh decoder in the initial "need sync" state.
    pub const fn new() -> Self {
        Self {
            level: true,
            dur_us: 0,
            bits: [false; 60],
            bit_idx: 0,
            phase: Phase::WaitingForGap,
        }
    }

    /// Drop any collected bits and go back to searching for the minute
    /// gap. The last-observed pin level is preserved so the next sample
    /// doesn't spuriously register an edge.
    pub fn reset(&mut self) {
        self.dur_us = 0;
        self.bits = [false; 60];
        self.bit_idx = 0;
        self.phase = Phase::WaitingForGap;
    }

    /// Current external-facing state.
    pub fn state(&self) -> DecoderState {
        match self.phase {
            Phase::WaitingForGap => DecoderState::SearchingForGap,
            Phase::AwaitingFirstBit => DecoderState::AwaitingFirstBit,
            Phase::InPulse | Phase::BetweenPulses => {
                DecoderState::CollectingBits { idx: self.bit_idx }
            }
        }
    }

    /// Feed one level sample taken `dt_us` microseconds after the
    /// previous sample (or after construction, for the first call).
    ///
    /// Returns `Some(Frame)` only on the falling edge that ends a
    /// successfully-decoded minute gap. Every other call returns `None`.
    pub fn sample(&mut self, level: bool, dt_us: u32) -> Option<Frame> {
        if level != self.level {
            // Edge transition. `self.dur_us` is the duration we spent at
            // the *previous* level; reset the accumulator to this sample's
            // dt so the next transition measures from here.
            let prev_dur_us = self.dur_us;
            self.level = level;
            self.dur_us = dt_us;
            return if level {
                self.on_rising_edge(prev_dur_us)
            } else {
                self.on_falling_edge()
            };
        }

        // No edge — just accumulate time at the current level.
        self.dur_us = self.dur_us.saturating_add(dt_us);

        // Gap detection: sustained HIGH crossing the threshold.
        if level && self.dur_us >= DCF77_GAP_MIN_MS.saturating_mul(1000) {
            self.on_gap_detected();
        }

        None
    }

    fn on_falling_edge(&mut self) -> Option<Frame> {
        match self.phase {
            Phase::WaitingForGap => None,
            Phase::AwaitingFirstBit => {
                // Gap just ended. If we collected a full frame beforehand
                // (bit_idx == 59), decode it now and emit. Then start
                // collecting bit 0 of the new frame.
                let frame = if self.bit_idx == 59 {
                    decode_bits(&self.bits).ok()
                } else {
                    None
                };
                self.bits = [false; 60];
                self.bit_idx = 0;
                self.phase = Phase::InPulse;
                frame
            }
            Phase::InPulse => {
                // We were already LOW and saw another falling edge —
                // impossible for a well-formed stream. Resync.
                self.reset_sync();
                None
            }
            Phase::BetweenPulses => {
                // Normal inter-bit falling edge — start the next pulse.
                self.phase = Phase::InPulse;
                None
            }
        }
    }

    fn on_rising_edge(&mut self, low_dur_us: u32) -> Option<Frame> {
        match self.phase {
            Phase::WaitingForGap => None,
            Phase::AwaitingFirstBit | Phase::BetweenPulses => {
                // We were HIGH already — impossible for a well-formed
                // stream. Resync.
                self.reset_sync();
                None
            }
            Phase::InPulse => {
                let low_ms = low_dur_us / 1000;
                let bit = if (DCF77_BIT0_MIN_MS..=DCF77_BIT0_MAX_MS).contains(&low_ms) {
                    Some(false)
                } else if (DCF77_BIT1_MIN_MS..=DCF77_BIT1_MAX_MS).contains(&low_ms) {
                    Some(true)
                } else {
                    None
                };
                match bit {
                    Some(b) if self.bit_idx < 59 => {
                        self.bits[self.bit_idx as usize] = b;
                        self.bit_idx += 1;
                        self.phase = Phase::BetweenPulses;
                    }
                    _ => {
                        // Pulse width outside tolerance, or we've
                        // somehow collected 59 bits without a gap —
                        // either way, resync.
                        self.reset_sync();
                    }
                }
                None
            }
        }
    }

    fn on_gap_detected(&mut self) {
        match self.phase {
            Phase::WaitingForGap => {
                // Initial sync acquired.
                self.phase = Phase::AwaitingFirstBit;
                self.bits = [false; 60];
                self.bit_idx = 0;
            }
            Phase::BetweenPulses if self.bit_idx == 59 => {
                // Normal end-of-frame gap.
                self.phase = Phase::AwaitingFirstBit;
            }
            Phase::BetweenPulses => {
                // Gap mid-frame — we missed bits. Resync.
                self.reset_sync();
            }
            Phase::AwaitingFirstBit | Phase::InPulse => {
                // AwaitingFirstBit: already synced, still waiting for
                //   the falling edge — nothing to do.
                // InPulse: we're LOW, so HIGH-gap detection shouldn't
                //   be reachable. No-op.
            }
        }
    }

    fn reset_sync(&mut self) {
        self.bits = [false; 60];
        self.bit_idx = 0;
        self.phase = Phase::WaitingForGap;
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_zero_bits_fails_at_time_marker() {
        // bit 0 = 0 ✓, but bit 20 = 0 ✗
        let bits = [false; 60];
        assert_eq!(decode_bits(&bits), Err(DecodeError::BadTimeMarker));
    }

    #[test]
    fn decode_valid_00_00() {
        let bits = encode_bits(0, 0);
        assert_eq!(
            decode_bits(&bits),
            Ok(Frame {
                hours: 0,
                minutes: 0
            })
        );
    }

    #[test]
    fn decode_valid_12_34() {
        let bits = encode_bits(12, 34);
        assert_eq!(
            decode_bits(&bits),
            Ok(Frame {
                hours: 12,
                minutes: 34
            })
        );
    }

    #[test]
    fn decode_valid_23_59() {
        let bits = encode_bits(23, 59);
        assert_eq!(
            decode_bits(&bits),
            Ok(Frame {
                hours: 23,
                minutes: 59
            })
        );
    }

    #[test]
    fn decode_valid_09_09() {
        // Picks up the all-ones-in-low-nibble case for both fields,
        // which is a useful corner case for parity computation.
        let bits = encode_bits(9, 9);
        assert_eq!(
            decode_bits(&bits),
            Ok(Frame {
                hours: 9,
                minutes: 9
            })
        );
    }

    #[test]
    fn decode_rejects_bit0_set() {
        let mut bits = encode_bits(12, 34);
        bits[0] = true;
        assert_eq!(decode_bits(&bits), Err(DecodeError::BadStartBit));
    }

    #[test]
    fn decode_rejects_bit20_clear() {
        let mut bits = encode_bits(12, 34);
        bits[20] = false;
        assert_eq!(decode_bits(&bits), Err(DecodeError::BadTimeMarker));
    }

    #[test]
    fn decode_rejects_bad_minute_parity() {
        let mut bits = encode_bits(12, 34);
        bits[28] = !bits[28];
        assert_eq!(decode_bits(&bits), Err(DecodeError::MinuteParity));
    }

    #[test]
    fn decode_rejects_bad_hour_parity() {
        let mut bits = encode_bits(12, 34);
        bits[35] = !bits[35];
        assert_eq!(decode_bits(&bits), Err(DecodeError::HourParity));
    }

    #[test]
    fn decode_rejects_bcd_nibble_above_9() {
        // Set minute-low nibble to BCD `1010` (= 10): bits 22 + 24.
        // Parity over data bits has 2 ones, so leave bit 28 = 0 → even.
        let mut bits = [false; 60];
        bits[20] = true;
        bits[22] = true;
        bits[24] = true;
        assert_eq!(decode_bits(&bits), Err(DecodeError::BadBcd));
    }

    #[test]
    fn decode_rejects_minute_range() {
        // minute-high = 6, minute-low = 0 → minutes = 60.
        // Bits 25..=27 weight (1, 2, 4) → high=6 means bits 26 + 27.
        let mut bits = [false; 60];
        bits[20] = true;
        bits[26] = true;
        bits[27] = true;
        // 2 ones in data → parity bit 28 stays 0 → even total.
        assert_eq!(decode_bits(&bits), Err(DecodeError::MinuteRange));
    }

    #[test]
    fn decode_rejects_hour_range() {
        // Build a valid 00:00 frame, then set hour-tens nibble to 3
        // (bits 33, 34) → hours = 30. Parity over 29..=34 stays even
        // (2 ones), so bit 35 stays 0 and we don't recompute it.
        let mut bits = encode_bits(0, 0);
        bits[33] = true;
        bits[34] = true;
        assert_eq!(decode_bits(&bits), Err(DecodeError::HourRange));
    }

    #[test]
    fn decode_rejects_minute_high_above_5_via_range() {
        // minute-high = 7 (max for 3 bits), minute-low = 9 → minutes = 79
        // ≥ 60, so the decoder fails with `MinuteRange` before the
        // out-of-spec high-nibble value itself ever becomes an error.
        // Built by hand: `encode_bits` clamps and would round-trip to 19.
        let mut bits = [false; 60];
        bits[20] = true;
        // minute-low = 9 (1001 LSB-first): bits 21 + 24.
        bits[21] = true;
        bits[24] = true;
        // minute-high = 7 (111 LSB-first): bits 25 + 26 + 27.
        bits[25] = true;
        bits[26] = true;
        bits[27] = true;
        // Data has 5 ones → even-parity bit 28 = 1.
        bits[28] = true;
        assert_eq!(decode_bits(&bits), Err(DecodeError::MinuteRange));
    }

    #[test]
    fn decode_independent_of_unused_date_bits() {
        // Time fields valid; flip every date bit (36..=58). Decoder must
        // still produce the same frame because we don't validate them.
        let mut bits = encode_bits(12, 34);
        for i in 36..=58 {
            bits[i] = true;
        }
        assert_eq!(
            decode_bits(&bits),
            Ok(Frame {
                hours: 12,
                minutes: 34
            })
        );
    }

    // === Encode → decode round-trip (bit level) ===

    #[test]
    fn encode_decode_round_trip_via_bits() {
        // Covers every BCD carry we care about: 00:00 all zero, 09:09
        // tests low-nibble maxed on both fields, 10:00 tests a minute
        // tens carry, 12:34 / 23:59 cover generic cases.
        for (h, m) in [(0, 0), (9, 9), (10, 0), (12, 34), (23, 59)] {
            let bits = encode_bits(h, m);
            assert_eq!(
                decode_bits(&bits),
                Ok(Frame {
                    hours: h,
                    minutes: m
                }),
                "bit-level round trip failed for {h:02}:{m:02}"
            );
        }
    }

    #[test]
    fn encode_bits_clamps_out_of_range_input() {
        // Out-of-range inputs are clamped, not rejected. Round-tripping
        // `encode → decode` must therefore land on the clamped value.
        let bits = encode_bits(99, 99);
        assert_eq!(
            decode_bits(&bits),
            Ok(Frame {
                hours: 99 % 24,
                minutes: 99 % 60
            })
        );
    }

    // === Streaming decoder (pulse-level) tests ===

    /// `dt_us` used by the test pulse stream. Matches `DCF77_SAMPLE_US`.
    const DT: u32 = 10_000;

    /// Length in samples of a LOW pulse of `ms` milliseconds at `DT`
    /// sampling cadence (rounded down).
    const fn ms_samples(ms: u32) -> u32 {
        ms * 1000 / DT
    }

    /// Push `n` repeated `(level, DT)` samples into `out`.
    fn push_n(out: &mut Vec<(bool, u32)>, n: u32, level: bool) {
        for _ in 0..n {
            out.push((level, DT));
        }
    }

    /// Construct a full DCF77 pulse stream for `(hours, minutes)`:
    ///
    /// - a 2-s leading HIGH for initial sync,
    /// - 59 pulses (LOW for 100/200 ms, HIGH for 900/800 ms) for bits 0..=58,
    /// - a ~1.8-s HIGH gap for bit 59,
    /// - one final LOW sample simulating the falling edge that starts
    ///   bit 0 of the *next* frame (the apply point).
    fn pulse_stream(hours: u8, minutes: u8) -> Vec<(bool, u32)> {
        let bits = encode_bits(hours, minutes);
        let mut out = Vec::new();

        // Leading gap.
        push_n(&mut out, ms_samples(2000), true);

        // 59 bit slots.
        for &b in bits.iter().take(59) {
            let low_ms = if b { 200 } else { 100 };
            let high_ms = 1000 - low_ms;
            push_n(&mut out, ms_samples(low_ms), false);
            push_n(&mut out, ms_samples(high_ms), true);
        }

        // End-of-frame gap (bit 59 marker): ~1.8 s of HIGH beyond the
        // final inter-pulse HIGH that's already there from bit 58.
        push_n(&mut out, ms_samples(1000), true);

        // Falling edge into bit 0 of next frame — the apply point.
        out.push((false, DT));

        out
    }

    fn feed(d: &mut Decoder, stream: &[(bool, u32)]) -> Option<Frame> {
        let mut last = None;
        for &(level, dt) in stream {
            if let Some(f) = d.sample(level, dt) {
                last = Some(f);
            }
        }
        last
    }

    #[test]
    fn new_decoder_is_searching_for_gap() {
        let d = Decoder::new();
        assert_eq!(d.state(), DecoderState::SearchingForGap);
    }

    #[test]
    fn initial_sync_transitions_to_awaiting_first_bit() {
        // 150 samples × 10 ms = 1500 ms at HIGH triggers gap detection.
        let mut d = Decoder::new();
        for _ in 0..150 {
            assert_eq!(d.sample(true, DT), None);
        }
        assert_eq!(d.state(), DecoderState::AwaitingFirstBit);
    }

    #[test]
    fn short_high_does_not_sync() {
        // 100 samples × 10 ms = 1000 ms is below the 1500 ms gap threshold.
        let mut d = Decoder::new();
        for _ in 0..100 {
            d.sample(true, DT);
        }
        assert_eq!(d.state(), DecoderState::SearchingForGap);
    }

    #[test]
    fn full_stream_emits_expected_frame() {
        let mut d = Decoder::new();
        let stream = pulse_stream(12, 34);
        let frame = feed(&mut d, &stream);
        assert_eq!(
            frame,
            Some(Frame {
                hours: 12,
                minutes: 34
            })
        );
        // After the apply-point falling edge we're now collecting bit 0
        // of the *next* frame.
        assert_eq!(d.state(), DecoderState::CollectingBits { idx: 0 });
    }

    #[test]
    fn stream_emits_frame_at_falling_edge_not_earlier() {
        let mut d = Decoder::new();
        let stream = pulse_stream(23, 59);

        // Feed everything *except* the final falling-edge sample.
        let head = &stream[..stream.len() - 1];
        let tail = stream[stream.len() - 1];
        let frame_before_edge = feed(&mut d, head);
        assert_eq!(
            frame_before_edge, None,
            "frame must not be emitted until the apply-point falling edge"
        );
        assert_eq!(d.state(), DecoderState::AwaitingFirstBit);

        let frame = d.sample(tail.0, tail.1);
        assert_eq!(
            frame,
            Some(Frame {
                hours: 23,
                minutes: 59
            })
        );
    }

    #[test]
    fn two_consecutive_frames_both_emit() {
        let mut d = Decoder::new();
        let first = feed(&mut d, &pulse_stream(0, 0));
        assert_eq!(
            first,
            Some(Frame {
                hours: 0,
                minutes: 0
            })
        );

        // For the second frame we're already sync'd — no leading gap
        // needed. Build the remainder directly.
        let bits = encode_bits(1, 2);
        let mut stream = Vec::new();
        // We're mid-pulse (the apply edge). Finish pulse-0 as 100 ms low
        // minus the 1 sample we already consumed.
        push_n(&mut stream, ms_samples(100) - 1, false);
        push_n(&mut stream, ms_samples(900), true);
        for &b in bits.iter().take(59).skip(1) {
            let low_ms = if b { 200 } else { 100 };
            let high_ms = 1000 - low_ms;
            push_n(&mut stream, ms_samples(low_ms), false);
            push_n(&mut stream, ms_samples(high_ms), true);
        }
        push_n(&mut stream, ms_samples(1000), true);
        stream.push((false, DT));

        let second = feed(&mut d, &stream);
        assert_eq!(
            second,
            Some(Frame {
                hours: 1,
                minutes: 2
            })
        );
    }

    #[test]
    fn glitchy_pulse_width_triggers_resync() {
        let mut d = Decoder::new();

        // Leading sync.
        push_leading_sync(&mut d);

        // One valid bit 0 (100 ms LOW).
        feed_pulse(&mut d, 100, 900);
        assert_eq!(d.state(), DecoderState::CollectingBits { idx: 1 });

        // A too-short pulse (50 ms LOW): outside 65..=135 ms so it's a
        // glitch — decoder must resync.
        feed_pulse(&mut d, 50, 900);
        assert_eq!(d.state(), DecoderState::SearchingForGap);
    }

    #[test]
    fn dead_zone_pulse_width_triggers_resync() {
        let mut d = Decoder::new();
        push_leading_sync(&mut d);

        // A 150 ms pulse lands in the dead zone (> 135, < 165) — glitch.
        feed_pulse(&mut d, 150, 850);
        assert_eq!(d.state(), DecoderState::SearchingForGap);
    }

    #[test]
    fn reset_goes_back_to_searching() {
        let mut d = Decoder::new();
        push_leading_sync(&mut d);
        assert_eq!(d.state(), DecoderState::AwaitingFirstBit);
        d.reset();
        assert_eq!(d.state(), DecoderState::SearchingForGap);
    }

    #[test]
    fn encode_decode_round_trip_via_pulses() {
        // Full encode → modulate → decode loop. Covers every BCD carry
        // (09:09 → 10:00 tens-digit carry) plus both hour-tens digits
        // (0, 1, 2) and the maximum valid fields (23:59). Mirrors the
        // `render → decode` round-trip in `check.sh`.
        for (h, m) in [(0, 0), (9, 9), (10, 0), (12, 34), (19, 27), (23, 59)] {
            let mut d = Decoder::new();
            let stream = pulse_stream(h, m);
            assert_eq!(
                feed(&mut d, &stream),
                Some(Frame {
                    hours: h,
                    minutes: m
                }),
                "pulse-level round trip failed for {h:02}:{m:02}"
            );
        }
    }

    // === Loopback `TxState` (encoder + modulator) tests ===

    #[test]
    fn tx_state_starts_in_gap() {
        let mut tx = TxState::new();
        // The first dt advances us 10 ms into bit 59 → still HIGH.
        assert!(tx.step(DT, 12, 34));
    }

    #[test]
    fn tx_emits_short_pulse_for_bit_zero_at_second_zero() {
        // Bit 0 of telegram `(0, 0)` is `false` (start bit) → 100 ms
        // LOW pulse + 900 ms HIGH idle. Walk through 199 samples
        // covering [tail of gap | bit-0 second].
        let mut tx = TxState::new();
        let mut levels = Vec::with_capacity(199);
        for _ in 0..199 {
            levels.push(tx.step(DT, 0, 0));
        }

        // Steps 1..=99 (indices 0..99): inside bit 59 (gap) → HIGH.
        assert!(
            levels[0..99].iter().all(|&b| b),
            "gap must remain HIGH; got {:?}",
            &levels[0..99]
        );
        // Steps 100..=109 (indices 99..109): bit-0 100 ms LOW pulse.
        assert!(
            levels[99..109].iter().all(|&b| !b),
            "bit-0 pulse must be LOW; got {:?}",
            &levels[99..109]
        );
        // Steps 110..=199 (indices 109..199): post-pulse HIGH idle.
        assert!(
            levels[109..199].iter().all(|&b| b),
            "post-pulse must be HIGH; got {:?}",
            &levels[109..199]
        );
    }

    #[test]
    fn tx_emits_long_pulse_for_bit_one() {
        // Bit 20 of any telegram is the start-of-time marker (`true`)
        // → 200 ms LOW pulse + 800 ms HIGH idle. Walk to just before
        // the boundary into second 20, then collect that second's
        // worth of samples.
        let mut tx = TxState::new();
        // 1 s gap + 20 s of leading bits, minus one sample so the
        // *next* step is the boundary into second 20.
        for _ in 0..(ms_samples(1000) + 20 * ms_samples(1000) - 1) {
            tx.step(DT, 0, 0);
        }

        let mut levels = Vec::with_capacity(100);
        for _ in 0..100 {
            levels.push(tx.step(DT, 0, 0));
        }

        // Bit 20 = true → 200 ms LOW (20 samples) + 800 ms HIGH.
        assert!(
            levels[0..20].iter().all(|&b| !b),
            "bit-20 pulse must be LOW for 200 ms; got {:?}",
            &levels[0..20]
        );
        assert!(
            levels[20..100].iter().all(|&b| b),
            "post-pulse must be HIGH for 800 ms; got {:?}",
            &levels[20..100]
        );
    }

    #[test]
    fn tx_to_decoder_round_trip() {
        // Wire the TX directly into the RX and run for 3 simulated
        // minutes. The TX encodes `(h, m + 1)` per protocol (the
        // encoded value names the time at the gap-end / apply moment),
        // so we expect the decoded minute to be one greater than what
        // we fed in.
        let mut tx = TxState::new();
        let mut rx = Decoder::new();
        let (h, m) = (12, 34);
        let mut last_frame = None;
        for _ in 0..18_000 {
            let level = tx.step(DT, h, m);
            if let Some(frame) = rx.sample(level, DT) {
                last_frame = Some(frame);
            }
        }
        assert_eq!(
            last_frame,
            Some(Frame {
                hours: h,
                minutes: m + 1
            })
        );
    }

    #[test]
    fn tx_picks_up_minute_change_on_next_boundary() {
        // The TX always encodes "one minute in the future" relative to
        // the supplied `(h, m)` at each boundary. After we switch the
        // input from (0, 0) to (12, 34), the decoder should eventually
        // see (12, 35) (= 12, 34 + 1) — proving the new input made it
        // through the encoder.
        let mut tx = TxState::new();
        let mut rx = Decoder::new();
        let mut frames = Vec::new();
        for i in 0..(4 * 60 * 100) {
            let (h, m) = if i < 60 * 100 { (0, 0) } else { (12, 34) };
            let level = tx.step(DT, h, m);
            if let Some(f) = rx.sample(level, DT) {
                frames.push(f);
            }
        }
        assert!(
            frames.contains(&Frame {
                hours: 12,
                minutes: 35
            }),
            "expected (12, 35) (= input 12:34 + 1) after the switch, got {frames:?}"
        );
    }

    #[test]
    fn tx_handles_minute_rollover() {
        // Feed 12:59 — the encoder should forecast 13:00, with the
        // hour rolling over.
        let mut tx = TxState::new();
        let mut rx = Decoder::new();
        let mut last = None;
        for _ in 0..18_000 {
            let level = tx.step(DT, 12, 59);
            if let Some(f) = rx.sample(level, DT) {
                last = Some(f);
            }
        }
        assert_eq!(
            last,
            Some(Frame {
                hours: 13,
                minutes: 0
            })
        );
    }

    #[test]
    fn tx_handles_24h_rollover() {
        // 23:59 → 00:00 (next day).
        let mut tx = TxState::new();
        let mut rx = Decoder::new();
        let mut last = None;
        for _ in 0..18_000 {
            let level = tx.step(DT, 23, 59);
            if let Some(f) = rx.sample(level, DT) {
                last = Some(f);
            }
        }
        assert_eq!(
            last,
            Some(Frame {
                hours: 0,
                minutes: 0
            })
        );
    }

    fn push_leading_sync(d: &mut Decoder) {
        // 2 s HIGH — triggers gap detection at 1500 ms.
        for _ in 0..ms_samples(2000) {
            d.sample(true, DT);
        }
        assert_eq!(d.state(), DecoderState::AwaitingFirstBit);
    }

    fn feed_pulse(d: &mut Decoder, low_ms: u32, high_ms: u32) {
        for _ in 0..ms_samples(low_ms) {
            d.sample(false, DT);
        }
        for _ in 0..ms_samples(high_ms) {
            d.sample(true, DT);
        }
    }
}
