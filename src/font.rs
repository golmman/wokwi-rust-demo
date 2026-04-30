

/// Width in pixels of every glyph in this font.
pub const GLYPH_WIDTH: usize = 3;
/// Height in pixels of every glyph in this font.
pub const GLYPH_HEIGHT: usize = 8;

/// One renderable character. Adding a new glyph means: add a variant here,
/// add a `match` arm in `bitmap()`, and append the bitmap below.
#[derive(Copy, Clone, Debug)]
pub enum Glyph {
    D0,
    D1,
    D2,
    D3,
    D4,
    D5,
    D6,
    D7,
    D8,
    D9,
    Colon,
}

impl Glyph {
    /// Pick the glyph for a `0..=9` digit.
    #[inline]
    pub const fn digit(d: u8) -> Self {
        match d % 10 {
            0 => Glyph::D0,
            1 => Glyph::D1,
            2 => Glyph::D2,
            3 => Glyph::D3,
            4 => Glyph::D4,
            5 => Glyph::D5,
            6 => Glyph::D6,
            7 => Glyph::D7,
            8 => Glyph::D8,
            _ => Glyph::D9,
        }
    }

    /// 3x8 row-major on/off bitmap for this glyph (rows top-to-bottom,
    /// columns left-to-right). `1` is lit, `0` is dark.
    #[inline]
    pub const fn bitmap(self) -> &'static [[u8; GLYPH_WIDTH]; GLYPH_HEIGHT] {
        match self {
            Glyph::D0 => &D0,
            Glyph::D1 => &D1,
            Glyph::D2 => &D2,
            Glyph::D3 => &D3,
            Glyph::D4 => &D4,
            Glyph::D5 => &D5,
            Glyph::D6 => &D6,
            Glyph::D7 => &D7,
            Glyph::D8 => &D8,
            Glyph::D9 => &D9,
            Glyph::Colon => &COLON,
        }
    }
}

// Visual 3x8 bitmaps (8 rows, 3 cols). See number-design.png for the source
// of these patterns.
const D0: [[u8; 3]; 8] = [
    [0, 1, 0], [1, 0, 1], [1, 0, 1], [1, 0, 1],
    [1, 0, 1], [1, 0, 1], [1, 0, 1], [0, 1, 0],
];
const D1: [[u8; 3]; 8] = [
    [0, 0, 1], [0, 1, 1], [1, 0, 1], [0, 0, 1],
    [0, 0, 1], [0, 0, 1], [0, 0, 1], [0, 0, 1],
];
const D2: [[u8; 3]; 8] = [
    [0, 1, 0], [1, 0, 1], [0, 0, 1], [0, 1, 0],
    [1, 0, 0], [1, 0, 0], [1, 0, 0], [1, 1, 1],
];
const D3: [[u8; 3]; 8] = [
    [0, 1, 0], [1, 0, 1], [0, 0, 1], [0, 1, 0],
    [0, 0, 1], [0, 0, 1], [1, 0, 1], [0, 1, 0],
];
const D4: [[u8; 3]; 8] = [
    [0, 0, 1], [0, 1, 1], [1, 0, 1], [1, 0, 1],
    [1, 1, 1], [0, 0, 1], [0, 0, 1], [0, 0, 1],
];
const D5: [[u8; 3]; 8] = [
    [1, 1, 1], [1, 0, 0], [1, 0, 0], [1, 1, 0],
    [0, 0, 1], [0, 0, 1], [1, 0, 1], [0, 1, 0],
];
const D6: [[u8; 3]; 8] = [
    [0, 1, 0], [1, 0, 0], [1, 0, 0], [1, 1, 0],
    [1, 0, 1], [1, 0, 1], [1, 0, 1], [0, 1, 0],
];
const D7: [[u8; 3]; 8] = [
    [1, 1, 1], [0, 0, 1], [0, 0, 1], [0, 1, 0],
    [0, 1, 0], [0, 1, 0], [0, 1, 0], [0, 1, 0],
];
const D8: [[u8; 3]; 8] = [
    [0, 1, 0], [1, 0, 1], [1, 0, 1], [0, 1, 0],
    [1, 0, 1], [1, 0, 1], [1, 0, 1], [0, 1, 0],
];
const D9: [[u8; 3]; 8] = [
    [0, 1, 0], [1, 0, 1], [1, 0, 1], [0, 1, 1],
    [0, 0, 1], [0, 0, 1], [0, 0, 1], [0, 1, 0],
];
const COLON: [[u8; 3]; 8] = [
    [0, 0, 0], [0, 0, 0], [0, 1, 0], [0, 0, 0],
    [0, 0, 0], [0, 1, 0], [0, 0, 0], [0, 0, 0],
];
