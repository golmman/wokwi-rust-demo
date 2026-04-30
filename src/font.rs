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
// of these patterns. `#[rustfmt::skip]` keeps the rows laid out in the
// visually-grouped form below; without it `cargo fmt` puts each `[u8;3]`
// on its own line and the glyph shapes become unreadable.
#[rustfmt::skip]
const D0: [[u8; 3]; 8] = [
    [0, 1, 0], [1, 0, 1], [1, 0, 1], [1, 0, 1],
    [1, 0, 1], [1, 0, 1], [1, 0, 1], [0, 1, 0],
];
#[rustfmt::skip]
const D1: [[u8; 3]; 8] = [
    [0, 0, 1], [0, 1, 1], [1, 0, 1], [0, 0, 1],
    [0, 0, 1], [0, 0, 1], [0, 0, 1], [0, 0, 1],
];
#[rustfmt::skip]
const D2: [[u8; 3]; 8] = [
    [0, 1, 0], [1, 0, 1], [0, 0, 1], [0, 1, 0],
    [1, 0, 0], [1, 0, 0], [1, 0, 0], [1, 1, 1],
];
#[rustfmt::skip]
const D3: [[u8; 3]; 8] = [
    [0, 1, 0], [1, 0, 1], [0, 0, 1], [0, 1, 0],
    [0, 0, 1], [0, 0, 1], [1, 0, 1], [0, 1, 0],
];
#[rustfmt::skip]
const D4: [[u8; 3]; 8] = [
    [0, 0, 1], [0, 1, 1], [1, 0, 1], [1, 0, 1],
    [1, 1, 1], [0, 0, 1], [0, 0, 1], [0, 0, 1],
];
#[rustfmt::skip]
const D5: [[u8; 3]; 8] = [
    [1, 1, 1], [1, 0, 0], [1, 0, 0], [1, 1, 0],
    [0, 0, 1], [0, 0, 1], [1, 0, 1], [0, 1, 0],
];
#[rustfmt::skip]
const D6: [[u8; 3]; 8] = [
    [0, 1, 0], [1, 0, 0], [1, 0, 0], [1, 1, 0],
    [1, 0, 1], [1, 0, 1], [1, 0, 1], [0, 1, 0],
];
#[rustfmt::skip]
const D7: [[u8; 3]; 8] = [
    [1, 1, 1], [0, 0, 1], [0, 0, 1], [0, 1, 0],
    [0, 1, 0], [0, 1, 0], [0, 1, 0], [0, 1, 0],
];
#[rustfmt::skip]
const D8: [[u8; 3]; 8] = [
    [0, 1, 0], [1, 0, 1], [1, 0, 1], [0, 1, 0],
    [1, 0, 1], [1, 0, 1], [1, 0, 1], [0, 1, 0],
];
#[rustfmt::skip]
const D9: [[u8; 3]; 8] = [
    [0, 1, 0], [1, 0, 1], [1, 0, 1], [0, 1, 1],
    [0, 0, 1], [0, 0, 1], [0, 0, 1], [0, 1, 0],
];
#[rustfmt::skip]
const COLON: [[u8; 3]; 8] = [
    [0, 0, 0], [0, 0, 0], [0, 1, 0], [0, 0, 0],
    [0, 0, 0], [0, 1, 0], [0, 0, 0], [0, 0, 0],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digit_picks_the_right_variant_for_0_9() {
        // We can't compare `Glyph` variants directly without `PartialEq`,
        // and pointer-equality on `&'static` references to `const` items
        // is unreliable (the compiler may or may not dedupe). Compare
        // bitmap contents instead — that's the externally observable bit.
        for d in 0u8..=9 {
            assert_eq!(
                Glyph::digit(d).bitmap(),
                expected(d),
                "Glyph::digit({d}) returned the wrong variant"
            );
        }
    }

    #[test]
    fn digit_wraps_modulo_10() {
        // `Glyph::digit(13)` should be the same glyph as `Glyph::digit(3)`.
        let a = Glyph::digit(13).bitmap();
        let b = Glyph::digit(3).bitmap();
        assert_eq!(a, b);
    }

    #[test]
    fn colon_lights_only_two_pixels() {
        let bm = Glyph::Colon.bitmap();
        let mut lit = 0;
        for r in 0..GLYPH_HEIGHT {
            for c in 0..GLYPH_WIDTH {
                if bm[r][c] != 0 {
                    lit += 1;
                }
            }
        }
        assert_eq!(lit, 2, "colon glyph should light exactly 2 pixels");
        assert_eq!(bm[2][1], 1);
        assert_eq!(bm[5][1], 1);
    }

    #[test]
    fn every_glyph_uses_only_0_or_1_pixels() {
        let all = [
            Glyph::D0,
            Glyph::D1,
            Glyph::D2,
            Glyph::D3,
            Glyph::D4,
            Glyph::D5,
            Glyph::D6,
            Glyph::D7,
            Glyph::D8,
            Glyph::D9,
            Glyph::Colon,
        ];
        for g in all {
            for row in g.bitmap() {
                for &p in row {
                    assert!(p == 0 || p == 1, "glyph has non-binary pixel: {p}");
                }
            }
        }
    }

    fn expected(d: u8) -> &'static [[u8; 3]; 8] {
        match d {
            0 => &D0,
            1 => &D1,
            2 => &D2,
            3 => &D3,
            4 => &D4,
            5 => &D5,
            6 => &D6,
            7 => &D7,
            8 => &D8,
            _ => &D9,
        }
    }
}
