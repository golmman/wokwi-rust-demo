

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
        let bm = glyph.bitmap();
        for r in 0..GLYPH_HEIGHT {
            for c in 0..GLYPH_WIDTH {
                if bm[r][c] != 0 {
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
        for dev in 0..CHAIN_LEN {
            let shift = (CHAIN_LEN - 1 - dev) * MODULE_COLS;
            for r in 0..FB_ROWS {
                out[dev][r] = ((self.rows[r] >> shift) & 0xFF) as u8;
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
