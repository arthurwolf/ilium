//! A 3x5 dot-matrix font for the digital readout: digits and a colon.

/// Glyph index of the colon.
pub const COLON: usize = 10;

/// Rows top to bottom; bit 2 is the left column, bit 0 the right one.
/// The colon uses only the middle column of a one-wide glyph (bit 0).
const GLYPHS: [[u8; 5]; 11] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
    [0b0, 0b1, 0b0, 0b1, 0b0],
];

/// Glyph width in font pixels.
pub fn glyph_width(glyph: usize) -> usize {
    if glyph == COLON {
        1
    } else {
        3
    }
}

/// Whether the font pixel at (`column`, `row`) is lit.
pub fn pixel(glyph: usize, column: usize, row: usize) -> bool {
    let Some(rows) = GLYPHS.get(glyph) else {
        return false;
    };
    let width = glyph_width(glyph);
    if column >= width || row >= 5 {
        return false;
    }
    rows[row] >> (width - 1 - column) & 1 == 1
}
