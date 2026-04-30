#!/usr/bin/env python3
"""Decode a Wokwi screenshot of the 4xMAX7219 FC16 chain back to HH:MM:SS.

Mirrors the 3x8 glyph font in src/font.rs and the column layout in
src/display.rs (3-px-wide chars, 1-px gap between every char, 8 chars = 31
columns inside a 32-LED-wide chain).

Usage:
    python3 tools/decode_screenshot.py target/wokwi/before.png ...

Lets an agent assert exact display contents instead of eyeballing PNGs.
"""
from __future__ import annotations

import sys
from PIL import Image

FONT: dict[str, list[list[int]]] = {
    "0": [[0,1,0],[1,0,1],[1,0,1],[1,0,1],[1,0,1],[1,0,1],[1,0,1],[0,1,0]],
    "1": [[0,0,1],[0,1,1],[1,0,1],[0,0,1],[0,0,1],[0,0,1],[0,0,1],[0,0,1]],
    "2": [[0,1,0],[1,0,1],[0,0,1],[0,1,0],[1,0,0],[1,0,0],[1,0,0],[1,1,1]],
    "3": [[0,1,0],[1,0,1],[0,0,1],[0,1,0],[0,0,1],[0,0,1],[1,0,1],[0,1,0]],
    "4": [[0,0,1],[0,1,1],[1,0,1],[1,0,1],[1,1,1],[0,0,1],[0,0,1],[0,0,1]],
    "5": [[1,1,1],[1,0,0],[1,0,0],[1,1,0],[0,0,1],[0,0,1],[1,0,1],[0,1,0]],
    "6": [[0,1,0],[1,0,0],[1,0,0],[1,1,0],[1,0,1],[1,0,1],[1,0,1],[0,1,0]],
    "7": [[1,1,1],[0,0,1],[0,0,1],[0,1,0],[0,1,0],[0,1,0],[0,1,0],[0,1,0]],
    "8": [[0,1,0],[1,0,1],[1,0,1],[0,1,0],[1,0,1],[1,0,1],[1,0,1],[0,1,0]],
    "9": [[0,1,0],[1,0,1],[1,0,1],[0,1,1],[0,0,1],[0,0,1],[0,0,1],[0,1,0]],
    ":": [[0,0,0],[0,0,0],[0,1,0],[0,0,0],[0,0,0],[0,1,0],[0,0,0],[0,0,0]],
}


def decode(path: str) -> str:
    img = Image.open(path).convert("L")
    w, h = img.size
    if w % 8 or h % 8:
        raise SystemExit(f"{path}: expected dims to be multiples of 8, got {w}x{h}")
    px = img.load()
    cols, rows = w // 8, h // 8
    if cols != 32 or rows != 8:
        raise SystemExit(f"{path}: expected 32x8 LED grid, got {cols}x{rows}")
    # Sample center of each 8x8 LED cell; threshold at half-bright.
    grid = [[1 if px[c * 8 + 4, r * 8 + 4] > 128 else 0
             for c in range(cols)] for r in range(rows)]
    out: list[str] = []
    for char_idx in range(8):
        c0 = char_idx * 4  # 3 px glyph + 1 px gap
        glyph = [row[c0:c0 + 3] for row in grid]
        best, best_diff = "?", 1 << 30
        for ch, ref in FONT.items():
            diff = sum(abs(glyph[r][c] - ref[r][c])
                       for r in range(8) for c in range(3))
            if diff < best_diff:
                best, best_diff = ch, diff
        out.append(best)
    return "".join(out)


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    for arg in sys.argv[1:]:
        print(f"{arg}: {decode(arg)}")
