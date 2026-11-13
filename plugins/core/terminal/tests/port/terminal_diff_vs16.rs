//! Twin of ilium-client/tests/terminal_diff_vs16.rs. The original drives a ratatui buffer diff
//! through a crossterm backend into a `vt100` parser; the plugin owns the repaint rule
//! (`screen_diff`), so the same frames go through `diff_rows` into a small cell-grid terminal
//! that, like vt100 for a VS16 icon, draws the icon in ONE cell while the planner counts two.

use ilium_plugin_core_terminal::screen_diff::{diff_rows, display_width, Repaint, VS16};

const WIDTH: usize = 96;
const HEIGHT: usize = 8;

/// A terminal that stores one string per cell and counts a VS16 icon as one cell.
struct Terminal {
    cells: Vec<Vec<String>>,
}

impl Terminal {
    fn new() -> Self {
        Self {
            cells: vec![vec![" ".to_string(); WIDTH]; HEIGHT],
        }
    }

    fn write(&mut self, row: usize, column: usize, text: &str) {
        let mut column = column;
        let mut chars = text.chars().peekable();
        while let Some(character) = chars.next() {
            let mut glyph = character.to_string();
            while chars.peek() == Some(&VS16) {
                glyph.push(chars.next().unwrap_or(VS16));
            }
            if column < WIDTH {
                self.cells[row][column] = glyph;
            }
            column += 1;
        }
    }

    fn apply(&mut self, repaint: &Repaint) {
        if repaint.clear_to_end {
            for cell in self.cells[repaint.row].iter_mut().skip(repaint.column) {
                *cell = " ".to_string();
            }
        }
        self.write(repaint.row, repaint.column, &repaint.text);
    }

    fn contents(&self) -> String {
        self.cells
            .iter()
            .map(|row| row.concat().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// `text` placed at `column` of a blank row, padded with spaces by the planner's own widths.
fn frame(placements: &[(usize, usize, &str)]) -> Vec<String> {
    let mut rows = vec![String::new(); HEIGHT];
    for (row, column, text) in placements {
        let current = display_width(&rows[*row]);
        assert!(current <= *column, "placements go left to right");
        rows[*row].push_str(&" ".repeat(column - current));
        rows[*row].push_str(text);
    }
    rows
}

/// REQ: PORT:ilium-client/tests/terminal_diff_vs16.rs::width_transition_preserves_text_after_vs16_icons
#[test]
fn width_transition_preserves_text_after_vs16_icons() {
    let blank = vec![String::new(); HEIGHT];
    let narrow = frame(&[
        (1, 1, "▼  🗂️      ledger-desk"),
        (1, 25, "~/dev/ai/ilium/data/tmp/ledger-desk"),
        (4, 3, "🖥️      Ledger"),
    ]);
    let wide = frame(&[
        (1, 1, "▼  🗂️       ledger-desk"),
        (1, 45, "~/dev/ai/ilium/data/tmp/ledger-desk"),
        (4, 3, "🖥️      Ledger"),
    ]);

    let mut terminal = Terminal::new();
    for repaint in diff_rows(&blank, &narrow) {
        terminal.apply(&repaint);
    }
    for repaint in diff_rows(&narrow, &wide) {
        terminal.apply(&repaint);
    }
    let rendered = terminal.contents();
    assert!(rendered.contains("ledger-desk"), "{rendered:?}");
    assert!(!rendered.contains("Ledgerr"), "{rendered:?}");
    assert!(
        rendered.contains("~/dev/ai/ilium/data/tmp/ledger-desk"),
        "{rendered:?}"
    );
}

#[test]
fn the_unchanged_icon_row_is_not_repainted() {
    let narrow = frame(&[(4, 3, "🖥️      Ledger")]);
    assert!(diff_rows(&narrow, &narrow).is_empty());
}
