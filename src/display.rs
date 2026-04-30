//! Pixel rendering for the MAX7219 chain.
//!
//! Layered as:
//! - `Framebuffer` — a plain `FB_COLS x FB_ROWS` bitmap. Knows nothing about
//!   clocks or hardware. Useful for splash screens, status overlays, etc.
//! - `clock_to_frame` — adapter that rasterises a `ClockState` into a
//!   `Framebuffer`. The only place the display layer touches the clock layer.
//! - `Framebuffer::to_devices` — packs the bitmap into the per-MAX7219 byte
//!   buffers that `MAX7219::write_raw(dev_idx, &[u8; 8])` expects.

use crate::clock::ClockState;
use crate::config::CHAIN_LEN;
use crate::font::{Glyph, GLYPH_HEIGHT, GLYPH_WIDTH};

/// LED columns inside one MAX7219 module.
pub const MODULE_COLS: usize = 8;

/// LED rows on every module (single-row chain).
pub const FB_ROWS: usize = GLYPH_HEIGHT;

/// Total LED columns across the whole daisy-chain. With CHAIN_LEN = 4 this
/// is 32, which is why each row fits in a `u32`.
pub const FB_COLS: usize = CHAIN_LEN * MODULE_COLS;

// One row must fit in our packing word. Tighten / widen if CHAIN_LEN > 4.
const _ASSERT_FB_FITS_U32: () = assert!(FB_COLS <= 32);

/// `FB_COLS x FB_ROWS` framebuffer. Each row is bit-packed left-to-right:
/// bit `FB_COLS - 1` is column 0 (the leftmost LED), bit 0 is column
/// `FB_COLS - 1`.
pub struct Framebuffer {
    rows: [u32; FB_ROWS],
}

impl Framebuffer {
    /// All-dark framebuffer.
    pub const fn new() -> Self {
        Self { rows: [0; FB_ROWS] }
    }

    /// Stamp a glyph at column `x` (top-aligned). Pixels that fall past
    /// `FB_COLS` are silently clipped.
    pub fn draw_glyph(&mut self, x: usize, glyph: Glyph) {
        for (r, row) in glyph.bitmap().iter().enumerate() {
            for (c, &pixel) in row.iter().enumerate() {
                if pixel != 0 {
                    let col = x + c;
                    if col < FB_COLS {
                        self.rows[r] |= 1 << ((FB_COLS - 1) - col);
                    }
                }
            }
        }
    }

    /// Slice the framebuffer into per-MAX7219-device byte buffers for
    /// `MAX7219::write_raw`. Device 0 holds the leftmost 8 columns.
    pub fn to_devices(&self) -> [[u8; FB_ROWS]; CHAIN_LEN] {
        let mut out = [[0u8; FB_ROWS]; CHAIN_LEN];
        for (dev, dev_buf) in out.iter_mut().enumerate() {
            let shift = (CHAIN_LEN - 1 - dev) * MODULE_COLS;
            for (r, byte) in dev_buf.iter_mut().enumerate() {
                *byte = ((self.rows[r] >> shift) & 0xFF) as u8;
            }
        }
        out
    }
}

impl Default for Framebuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// Render `clock` as `HH:MM:SS` into a fresh `Framebuffer`.
///
/// Layout is `D D : D D : D D` with one blank pixel between every char.
/// With `GLYPH_WIDTH = 3` and 7 inter-char gaps that uses 31 of 32 columns;
/// the last column stays dark.
pub fn clock_to_frame(clock: &ClockState) -> Framebuffer {
    let glyphs = [
        Glyph::digit(clock.hours() / 10),
        Glyph::digit(clock.hours() % 10),
        Glyph::Colon,
        Glyph::digit(clock.mins() / 10),
        Glyph::digit(clock.mins() % 10),
        Glyph::Colon,
        Glyph::digit(clock.secs() / 10),
        Glyph::digit(clock.secs() % 10),
    ];

    let mut fb = Framebuffer::new();
    let mut cursor = 0;
    for (i, glyph) in glyphs.iter().enumerate() {
        fb.draw_glyph(cursor, *glyph);
        cursor += GLYPH_WIDTH;
        if i < glyphs.len() - 1 {
            cursor += 1; // 1-pixel gap between every char
        }
    }
    fb
}

/// Golden device-buffer rendering of `ClockState::new(12, 34, 56)`.
///
/// This is the **single source of truth** that ties together `font.rs`,
/// `display.rs`, and `tools/check_decoder.py`. If you change any glyph or the
/// layout, the `clock_to_frame_golden_for_12_34_56` test below will fail with
/// the new bytes; copy them in here, then update `tools/check_decoder.py`'s
/// `GOLDEN_12_34_56` constant to match. That second update is what guarantees
/// the Python decoder is still in sync with the firmware's font.
pub const GOLDEN_12_34_56: [[u8; 8]; 4] = [
    // device 0 (cols 0..7) — covers "1" and the left two columns of "2"
    [0x24, 0x6A, 0xA2, 0x24, 0x28, 0x28, 0x28, 0x2E],
    // device 1 (cols 8..15) — "2" right column, ":", "3" left column
    [0x04, 0x0A, 0x42, 0x04, 0x02, 0x42, 0x0A, 0x04],
    // device 2 (cols 16..23) — "3" right two cols + "4" + ":"
    [0x20, 0x60, 0xA4, 0xA0, 0xE0, 0x24, 0x20, 0x20],
    // device 3 (cols 24..31) — "5" + "6"
    [0xE4, 0x88, 0x88, 0xCC, 0x2A, 0x2A, 0xAA, 0x44],
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ClockState;
    use crate::font::Glyph;

    #[test]
    fn framebuffer_starts_dark() {
        let fb = Framebuffer::new();
        let devices = fb.to_devices();
        for dev in devices {
            for byte in dev {
                assert_eq!(byte, 0, "fresh framebuffer should be all-dark");
            }
        }
    }

    #[test]
    fn draw_glyph_lights_expected_pixels() {
        // D8 at x=0: row 0 = [0,1,0] → only column 1 is lit, which is
        // bit 6 of device 0's byte = 0x40.
        let mut fb = Framebuffer::new();
        fb.draw_glyph(0, Glyph::D8);
        let devices = fb.to_devices();
        assert_eq!(devices[0][0], 0x40, "row 0 of D8 should light only col 1");
        for dev in 1..CHAIN_LEN {
            assert_eq!(
                devices[dev][0], 0,
                "drawing at x=0 must not touch device {dev} on row 0"
            );
        }
    }

    #[test]
    fn draw_glyph_clips_at_right_edge() {
        // D8 is 3 cols wide. At x=30, only cols 30 and 31 are inside the
        // 32-col grid; col 32 must clip silently rather than panic.
        let mut fb = Framebuffer::new();
        fb.draw_glyph(30, Glyph::D8);
        let devices = fb.to_devices();
        // Row 0 = [0,1,0]: col 30 dark, col 31 lit (bit 0 of device 3).
        assert_eq!(devices[3][0], 0x01);
        // Row 1 = [1,0,1]: col 30 lit (bit 1), col 32 clipped.
        assert_eq!(devices[3][1], 0x02);
    }

    #[test]
    fn clock_to_frame_golden_for_12_34_56() {
        let clock = ClockState::new(12, 34, 56);
        let actual = clock_to_frame(&clock).to_devices();
        assert_eq!(
            actual, GOLDEN_12_34_56,
            "clock_to_frame(12:34:56) drifted. If this is intentional (font \
             redesign / layout change), copy the actual bytes into \
             display::GOLDEN_12_34_56 AND update tools/check_decoder.py's \
             GOLDEN_12_34_56 to keep the Python decoder in sync."
        );
    }
}
