#!/usr/bin/env python3
"""Anchor the Python decoder to the firmware-side golden.

Reproduces the `[[u8; 8]; 4]` device buffers that
`src/display.rs::GOLDEN_12_34_56` pins for `ClockState::new(12, 34, 56)`,
unpacks them back into the 32x8 on/off grid that `decode_screenshot.py`
expects, and asserts the decoder reads `12:34:56`.

Why this exists: the Python decoder duplicates the 3x8 glyph patterns from
`src/font.rs`. Without this check, someone could redesign a glyph in
`font.rs`, update the Rust golden, and silently leave `decode_screenshot.py`
producing wrong text. With this check, both the Rust test and this script
must be updated together — drift is caught immediately.

If this fails after a deliberate font/layout change:
1. Update `src/display.rs::GOLDEN_12_34_56` (a Rust unit test will tell you
   the new bytes if you forget).
2. Update `decode_screenshot.py::FONT` to match the new glyph(s).
3. Update `GOLDEN_12_34_56` in this file with the new bytes.
4. Re-run `./check.sh`.
"""
from __future__ import annotations

import sys
from pathlib import Path

# Make `decode_screenshot` importable when this script is run from the
# project root or directly.
sys.path.insert(0, str(Path(__file__).parent))
from decode_screenshot import decode_grid  # noqa: E402

# Must match `src/display.rs::GOLDEN_12_34_56` exactly. Same byte-for-byte
# representation: 4 devices, 8 rows each, MSB of each byte = leftmost LED.
GOLDEN_12_34_56: list[list[int]] = [
    [0x24, 0x6A, 0xA2, 0x24, 0x28, 0x28, 0x28, 0x2E],  # device 0 (cols 0..7)
    [0x04, 0x0A, 0x42, 0x04, 0x02, 0x42, 0x0A, 0x04],  # device 1 (cols 8..15)
    [0x20, 0x60, 0xA4, 0xA0, 0xE0, 0x24, 0x20, 0x20],  # device 2 (cols 16..23)
    [0xE4, 0x88, 0x88, 0xCC, 0x2A, 0x2A, 0xAA, 0x44],  # device 3 (cols 24..31)
]
EXPECTED_TEXT = "12:34:56"


def devices_to_grid(devices: list[list[int]]) -> list[list[int]]:
    """Inverse of `Framebuffer::to_devices`: 4×[u8; 8] back to a 32x8 grid."""
    if len(devices) != 4 or any(len(d) != 8 for d in devices):
        raise ValueError(f"expected 4 devices x 8 rows, got {len(devices)}")
    grid = [[0] * 32 for _ in range(8)]
    for dev, rows in enumerate(devices):
        for r, byte in enumerate(rows):
            for bit in range(8):  # bit 7 = MSB = leftmost col in this device
                col = dev * 8 + (7 - bit)
                grid[r][col] = (byte >> bit) & 1
    return grid


def main() -> int:
    grid = devices_to_grid(GOLDEN_12_34_56)
    text = decode_grid(grid)
    if text != EXPECTED_TEXT:
        print(
            f"FAIL: GOLDEN_12_34_56 decoded as {text!r}, expected "
            f"{EXPECTED_TEXT!r}.\n"
            f"     Either the Rust golden bytes drifted (update them here \n"
            f"     and in src/display.rs) or decode_screenshot.py's FONT \n"
            f"     dict no longer matches src/font.rs.",
            file=sys.stderr,
        )
        return 1
    print(f"ok: GOLDEN_12_34_56 decodes to {EXPECTED_TEXT!r}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
