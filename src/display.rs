

use crate::font::FONT;
use crate::clock::ClockState;

/// Prepares the 8x8 buffers for the 4 chained MAX7219 devices using FC16 layout.
pub fn prepare_buffer(clock: &ClockState) -> [[u8; 8]; 4] {
    let digits = [
        (clock.hours / 10), (clock.hours % 10),
        10, // :
        (clock.mins / 10), (clock.mins % 10),
        10, // :
        (clock.secs / 10), (clock.secs % 10),
    ];

    let mut fb_rows = [0u32; 8];
    let mut cursor = 0; // Start at col 0

    for (i, &d) in digits.iter().enumerate() {
        for r in 0..8 {
            for c in 0..3 {
                if FONT[d as usize][r][c] != 0 {
                    let bit_pos = 31 - (cursor + c);
                    if bit_pos < 32 {
                        fb_rows[r] |= 1 << bit_pos;
                    }
                }
            }
        }
        cursor += 3;
        if i < 7 {
            cursor += 1;
        }
    }

    let mut device_buffers = [[0u8; 8]; 4];
    for dev_idx in 0..4 {
        for r in 0..8 {
            let shift = 24 - (dev_idx * 8);
            device_buffers[dev_idx][r] = ((fb_rows[r] >> shift) & 0xFF) as u8;
        }
    }

    device_buffers
}
