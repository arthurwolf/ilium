//! Terminal cell width, wrapping and fitting of text (guide-internationalisation, USR-316).

const ELLIPSIS: char = '\u{2026}';

/// Combining marks and zero-width characters occupy no cell.
fn is_zero_width(glyph: char) -> bool {
    matches!(glyph as u32,
        0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF
        | 0xFE00..=0xFE0F | 0xFE20..=0xFE2F | 0x200B..=0x200F | 0x2060..=0x2064)
}

/// East Asian wide and fullwidth glyphs occupy two cells.
fn is_wide(glyph: char) -> bool {
    matches!(glyph as u32,
        0x1100..=0x115F | 0x2E80..=0x303E | 0x3041..=0x33FF | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF | 0xA000..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF
        | 0xFE30..=0xFE6F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x1F300..=0x1F64F
        | 0x1F900..=0x1F9FF | 0x20000..=0x3FFFD)
}

fn glyph_width(glyph: char) -> usize {
    if is_zero_width(glyph) {
        0
    } else if is_wide(glyph) {
        2
    } else {
        1
    }
}

/// The width of `text` in terminal cells.
pub fn terminal_width(text: &str) -> usize {
    text.chars().map(glyph_width).sum()
}

/// Wraps `text` into lines of at most `cells` cells. A glyph is never split; a glyph wider than
/// the box gets a line of its own.
pub fn wrap_to_width(text: &str, cells: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;
    for glyph in text.chars() {
        let width = glyph_width(glyph);
        if current_width + width > cells && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push(glyph);
        current_width += width;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Fits `text` into a box of `cells` cells, ending with an ellipsis when it was cut.
pub fn fit_to_box(text: &str, cells: usize) -> String {
    if terminal_width(text) <= cells {
        return text.to_string();
    }
    if cells == 0 {
        return String::new();
    }
    let budget = cells - 1;
    let mut fitted = String::new();
    let mut used = 0;
    for glyph in text.chars() {
        let width = glyph_width(glyph);
        if used + width > budget {
            break;
        }
        fitted.push(glyph);
        used += width;
    }
    fitted.push(ELLIPSIS);
    fitted
}
