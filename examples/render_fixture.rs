//! Render a `ClockState(hh, mm, ss)` to a 256x64 PNG that matches the layout
//! `tools/decode_screenshot.py` expects from a Wokwi screenshot.
//!
//! Used by `check.sh` to round-trip every digit (`render → decode`) and
//! catch drift across the *entire* pipeline:
//!   - `font.rs`        glyph bitmaps
//!   - `display.rs`     `clock_to_frame` layout + `Framebuffer::to_devices`
//!     packing
//!   - `decode_screenshot.py`  PNG load, 8x8 cell sampling, threshold, FONT
//!     match
//!
//! This is intentionally an **example**, not a `[[bin]]` target: examples
//! aren't built for the firmware target by default and aren't shipped with
//! the binary, so adding `png` as a dev-dep doesn't bloat the .uf2.
//!
//! Usage (from project root):
//!   cargo run --example render_fixture --target=<host> -- HH MM SS OUT.png

use std::env;
use std::fs::File;
use std::io::BufWriter;

use wokwi_test::clock::ClockState;
use wokwi_test::display::{clock_to_frame, FB_COLS, FB_ROWS, MODULE_COLS};

/// Pixels per LED. Matches `decode_screenshot.py`'s assumption that the
/// screenshot is `FB_COLS * CELL` x `FB_ROWS * CELL`, with the centre pixel
/// of each 8x8 cell sampled at offset (4, 4).
const CELL: u32 = 8;

/// Lit-LED colour. Anything with luminance > 128 reads as "on" through the
/// decoder; the actual hue doesn't matter for round-trip correctness, but
/// we pick something close to the Wokwi `lightblue` matrix for readability
/// when an agent eyeballs the PNG.
const ON: [u8; 4] = [0xCC, 0xE5, 0xFF, 0xFF];

/// Dark-LED colour. Anything with luminance < 128 reads as "off".
const OFF: [u8; 4] = [0x10, 0x10, 0x10, 0xFF];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let (h, m, s, out) = match &args[1..] {
        [h, m, s, out] => (
            h.parse::<u8>()?,
            m.parse::<u8>()?,
            s.parse::<u8>()?,
            out.clone(),
        ),
        _ => {
            eprintln!("usage: render_fixture HH MM SS OUT.png");
            std::process::exit(2);
        }
    };

    let devices = clock_to_frame(&ClockState::new(h, m, s)).to_devices();

    let img_w = (FB_COLS as u32) * CELL;
    let img_h = (FB_ROWS as u32) * CELL;
    let mut pixels = vec![0u8; (img_w * img_h * 4) as usize];

    for r in 0..FB_ROWS {
        for c in 0..FB_COLS {
            let dev = c / MODULE_COLS;
            // Inside a device, MSB of the byte is the leftmost LED.
            let bit = 7 - (c % MODULE_COLS);
            let lit = ((devices[dev][r] >> bit) & 1) == 1;
            let colour = if lit { ON } else { OFF };
            // Fill the 8x8 block for this LED.
            let x0 = c as u32 * CELL;
            let y0 = r as u32 * CELL;
            for dy in 0..CELL {
                for dx in 0..CELL {
                    let i = (((y0 + dy) * img_w + (x0 + dx)) * 4) as usize;
                    pixels[i..i + 4].copy_from_slice(&colour);
                }
            }
        }
    }

    let file = BufWriter::new(File::create(&out)?);
    let mut enc = png::Encoder::new(file, img_w, img_h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&pixels)?;

    Ok(())
}
