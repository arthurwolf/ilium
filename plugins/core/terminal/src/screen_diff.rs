//! Repaint planning for rows that contain emoji-presentation icons (VS16, U+FE0F).
//!
//! Terminals disagree on the width of a symbol followed by U+FE0F: some draw it in one cell,
//! some in two. A diff that skips cells it believes unchanged leaves the text after such an
//! icon at the wrong column when the width assumption flips, which showed up as `Ledgerr` and
//! half-overwritten paths. The rule here is the one the width-transition regression pins: when
//! a row changes and either version of it holds a VS16 icon, repaint the row from the icon (or
//! from the first changed cell, if that is earlier) to the end of the row and clear the rest.
//! Everything is pure text in, repaint list out, so it needs no terminal.

use unicode_width::UnicodeWidthChar;

/// The emoji variation selector that asks for emoji presentation.
pub const VS16: char = '\u{FE0F}';

/// One displayed cell group: a base character, its combining marks and the VS16 if any.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Glyph {
    column: usize,
    width: usize,
    text: String,
    vs16: bool,
}

/// The cell width this planner assumes: a character followed by VS16 takes two cells.
fn glyphs(row: &str) -> Vec<Glyph> {
    let mut list: Vec<Glyph> = Vec::new();
    let mut column = 0;
    for character in row.chars() {
        if character == VS16 {
            if let Some(last) = list.last_mut() {
                if !last.vs16 {
                    last.vs16 = true;
                    last.text.push(character);
                    if last.width < 2 {
                        column += 2 - last.width;
                        last.width = 2;
                    }
                    continue;
                }
            }
        }
        let width = UnicodeWidthChar::width(character).unwrap_or(0);
        if width == 0 {
            if let Some(last) = list.last_mut() {
                last.text.push(character);
                continue;
            }
        }
        list.push(Glyph {
            column,
            width,
            text: character.to_string(),
            vs16: false,
        });
        column += width;
    }
    list
}

/// Display width of `row` in cells under this planner's rule (VS16 icons are two cells wide).
pub fn display_width(row: &str) -> usize {
    glyphs(row)
        .last()
        .map_or(0, |glyph| glyph.column + glyph.width)
}

/// One row segment to rewrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repaint {
    /// Zero-based row.
    pub row: usize,
    /// First cell to rewrite.
    pub column: usize,
    /// The text to write from `column`.
    pub text: String,
    /// The cells after the written text must be erased.
    pub clear_to_end: bool,
}

/// Plans the rewrites that turn `previous` into `next`, row by row. Rows past the end of one
/// list count as empty.
pub fn diff_rows(previous: &[String], next: &[String]) -> Vec<Repaint> {
    let empty = String::new();
    (0..previous.len().max(next.len()))
        .filter_map(|row| {
            repaint_row(
                row,
                previous.get(row).unwrap_or(&empty),
                next.get(row).unwrap_or(&empty),
            )
        })
        .collect()
}

fn repaint_row(row: usize, previous: &str, next: &str) -> Option<Repaint> {
    if previous == next {
        return None;
    }
    let before = glyphs(previous);
    let after = glyphs(next);
    let shared = before.len().min(after.len());
    let first_difference = (0..shared).find(|index| before[*index] != after[*index]);
    let diff_column = match first_difference {
        Some(index) => before[index].column.min(after[index].column),
        None if before.len() > after.len() => before[shared].column,
        None => after.get(shared)?.column,
    };
    let icon_column = before
        .iter()
        .chain(after.iter())
        .filter(|glyph| glyph.vs16)
        .map(|glyph| glyph.column)
        .min();
    let mut column = icon_column.map_or(diff_column, |icon| icon.min(diff_column));
    // A column inside a wide glyph of the new row starts at that glyph.
    if let Some(glyph) = after
        .iter()
        .find(|glyph| glyph.column < column && glyph.column + glyph.width > column)
    {
        column = glyph.column;
    }
    let text: String = after
        .iter()
        .filter(|glyph| glyph.column >= column)
        .map(|glyph| glyph.text.as_str())
        .collect();
    Some(Repaint {
        row,
        column,
        text,
        clear_to_end: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vs16_icon_is_two_cells_wide() {
        assert_eq!(display_width("a\u{1F5C2}\u{FE0F}b"), 4);
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width(""), 0);
    }

    #[test]
    fn an_unchanged_row_plans_nothing() {
        let rows = vec!["same \u{1F5C2}\u{FE0F}".to_string()];
        assert!(diff_rows(&rows, &rows).is_empty());
    }

    #[test]
    fn a_plain_row_is_repainted_from_the_first_changed_cell() {
        let plan = diff_rows(&["hello world".to_string()], &["hello there".to_string()]);
        assert_eq!(
            plan,
            vec![Repaint {
                row: 0,
                column: 6,
                text: "there".to_string(),
                clear_to_end: true
            }]
        );
    }

    #[test]
    fn a_change_after_an_icon_repaints_from_the_icon() {
        let plan = diff_rows(
            &["x \u{1F5C2}\u{FE0F} aa".to_string()],
            &["x \u{1F5C2}\u{FE0F} bb".to_string()],
        );
        assert_eq!(plan[0].column, 2);
        assert_eq!(plan[0].text, "\u{1F5C2}\u{FE0F} bb");
    }

    #[test]
    fn a_shorter_new_row_clears_the_tail() {
        let plan = diff_rows(&["abcdef".to_string()], &["abc".to_string()]);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].column, 3);
        assert_eq!(plan[0].text, "");
        assert!(plan[0].clear_to_end);
    }
}
