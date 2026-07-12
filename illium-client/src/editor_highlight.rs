//! Source-mode rendering for editor panes whose language `syntax`
//! recognizes: draws the same line-number gutter, cursor block,
//! current-line underline, and selection tint `ratatui_textarea::TextArea`'s
//! own widget draws (see that crate's private `widget.rs`/`highlight.rs`,
//! which expose no per-token styling hook -- that's why this module
//! recomposes them here instead of extending the crate), plus per-token
//! syntax color from `syntax::highlight`. Only used for Source mode when
//! `EditorPane::highlighted_lines` returns `Some`; every other case still
//! renders the plain `TextArea` widget in `ui::draw_editor`, unchanged from
//! before syntax highlighting existed.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthChar;

use crate::editor_pane::EditorPane;
use crate::syntax::LineTokens;

/// Matches `ratatui_textarea`'s own default `cursor_style` (reversed
/// video) -- illium never calls `TextArea::set_cursor_style`, so this is
/// what Source mode has always shown.
const CURSOR_STYLE: Style = Style::new().add_modifier(Modifier::REVERSED);

/// Matches the crate's default `select_style`.
const SELECTION_STYLE: Style = Style::new().bg(Color::LightBlue);

/// Matches the crate's default `cursor_line_style` (underline the whole
/// current line).
const CURSOR_LINE_STYLE: Style = Style::new().add_modifier(Modifier::UNDERLINED);

/// Total gutter width (digits + 1 leading pad + 1 trailing space) for a
/// buffer of `total_lines` lines -- matches `ratatui_textarea`'s own
/// line-number column width exactly (see that crate's private
/// `util::num_digits` and `highlight::LineHighlighter::line_number`) so a
/// file's gutter never shifts between Source mode's plain and highlighted
/// renderers.
pub fn line_number_gutter_width(total_lines: usize) -> u16 {
    digit_count(total_lines) + 2
}

fn digit_count(n: usize) -> u16 {
    if n == 0 {
        1
    } else {
        n.ilog10() as u16 + 1
    }
}

/// Renders `editor`'s Source-mode buffer with per-token syntax colors from
/// `tokens` (one entry per buffer line, see `EditorPane::highlighted_lines`).
/// `editor.update_source_scroll_mirror`/`update_source_scroll_col_mirror`
/// must already have been called this frame with this same `area`, the
/// same way the plain-`TextArea` path calls the row mirror today.
pub fn render(frame: &mut Frame, area: Rect, editor: &EditorPane, tokens: &[LineTokens]) {
    let lines = editor.textarea.lines();
    let total_lines = lines.len();
    let gutter_width = if editor.show_line_numbers {
        line_number_gutter_width(total_lines)
    } else {
        0
    };

    let top_row = usize::from(editor.source_scroll_row());
    let bottom_row = (top_row + usize::from(area.height)).min(total_lines);
    let cursor = editor.textarea.cursor();
    let (cursor_row, cursor_col) = (cursor.0, cursor.1);
    let selection = editor.textarea.selection_range();
    let tab_len = editor.textarea.tab_length();

    let rendered: Vec<Line<'static>> = (top_row..bottom_row)
        .map(|row| {
            let line = &lines[row];
            let is_cursor_row = row == cursor_row;
            let select_bounds =
                selection.and_then(|(start, end)| row_selection_bytes(line, row, start, end));

            let mut spans = Vec::new();
            if editor.show_line_numbers {
                spans.push(line_number_span(row, gutter_width));
            }
            spans.extend(render_row_spans(
                line,
                &tokens[row],
                is_cursor_row,
                cursor_col,
                select_bounds,
                tab_len,
            ));
            Line::from(spans)
        })
        .collect();

    let paragraph = Paragraph::new(rendered).scroll((0, editor.source_scroll_col()));
    frame.render_widget(paragraph, area);
}

/// The gutter span for one row: right-aligned `row + 1` padded to
/// `gutter_width` total columns (digits + leading pad + trailing space) --
/// see `line_number_gutter_width`.
fn line_number_span(row: usize, gutter_width: u16) -> Span<'static> {
    let number_field = usize::from(gutter_width).saturating_sub(1);
    Span::styled(
        format!("{:>width$} ", row + 1, width = number_field),
        Style::new().fg(Color::DarkGray),
    )
}

/// Byte range within `line` (row `row`) that the selection `start..end`
/// (from `TextArea::selection_range`, both `(row, char_col)` pairs with
/// `start` always <= `end`) covers on this row, plus whether the selection
/// extends past this row's last character (fully inside a multi-row
/// selection, or ends on a later row) -- mirrors
/// `ratatui_textarea::highlight::LineHighlighter::selection`, private to
/// that crate.
fn row_selection_bytes(
    line: &str,
    row: usize,
    start: (usize, usize),
    end: (usize, usize),
) -> Option<(usize, usize, bool)> {
    if row < start.0 || row > end.0 {
        return None;
    }
    let start_byte = if row == start.0 {
        char_byte_offset(line, start.1)
    } else {
        0
    };
    let (end_byte, trailing) = if row == end.0 {
        (char_byte_offset(line, end.1), false)
    } else {
        (line.len(), true)
    };
    if start_byte == end_byte && !trailing {
        return None;
    }
    Some((start_byte, end_byte, trailing))
}

/// Byte offset of the `char_col`-th character in `line`, or `line.len()`
/// when `char_col` is at or past the line's character count (cursor/
/// selection sitting at end-of-line).
fn char_byte_offset(line: &str, char_col: usize) -> usize {
    line.char_indices()
        .nth(char_col)
        .map_or(line.len(), |(byte, _)| byte)
}

/// Composes one row's spans: syntax colors as the base layer, then the
/// current-line underline, selection tint, and cursor block patched on top
/// (in that priority order) via `Style::patch`, which keeps a token's
/// syntax color visible underneath the cursor/selection overlay instead of
/// replacing it outright the way `LineHighlighter` does.
fn render_row_spans(
    line: &str,
    tokens: &LineTokens,
    is_cursor_row: bool,
    cursor_col: usize,
    select_bounds: Option<(usize, usize, bool)>,
    tab_len: u8,
) -> Vec<Span<'static>> {
    let mut per_byte = vec![Style::default(); line.len()];
    for (range, style) in tokens {
        let start = range.start.min(line.len());
        let end = range.end.min(line.len());
        for byte_style in &mut per_byte[start..end] {
            *byte_style = *style;
        }
    }
    if is_cursor_row {
        for byte_style in &mut per_byte {
            *byte_style = byte_style.patch(CURSOR_LINE_STYLE);
        }
    }
    if let Some((start, end, _)) = select_bounds {
        let end = end.min(line.len());
        for byte_style in &mut per_byte[start.min(end)..end] {
            *byte_style = byte_style.patch(SELECTION_STYLE);
        }
    }
    let mut cursor_char_len = 0usize;
    if is_cursor_row {
        if let Some((start, ch)) = line.char_indices().nth(cursor_col) {
            let end = start + ch.len_utf8();
            cursor_char_len = ch.len_utf8();
            for byte_style in &mut per_byte[start..end] {
                *byte_style = byte_style.patch(CURSOR_STYLE);
            }
        }
    }

    let mut spans = Vec::new();
    let mut run_style: Option<Style> = None;
    let mut run_text = String::new();
    let mut visual_col = 0usize;
    let tab_width = usize::from(tab_len).max(1);
    for (byte_index, ch) in line.char_indices() {
        let style = per_byte[byte_index];
        if run_style != Some(style) {
            if !run_text.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut run_text),
                    run_style.expect("run_text non-empty implies a style was set"),
                ));
            }
            run_style = Some(style);
        }
        if ch == '\t' {
            let pad = tab_width - visual_col % tab_width;
            run_text.extend(std::iter::repeat_n(' ', pad));
            visual_col += pad;
        } else {
            run_text.push(ch);
            visual_col += ch.width().unwrap_or(0);
        }
    }
    if !run_text.is_empty() {
        spans.push(Span::styled(
            run_text,
            run_style.expect("run_text non-empty implies a style was set"),
        ));
    }

    let cursor_at_row_end =
        is_cursor_row && cursor_char_len == 0 && cursor_col >= line.chars().count();
    let select_trailing = select_bounds.is_some_and(|(_, _, trailing)| trailing);
    if cursor_at_row_end {
        let mut style = Style::default();
        if is_cursor_row {
            style = style.patch(CURSOR_LINE_STYLE);
        }
        spans.push(Span::styled(" ", style.patch(CURSOR_STYLE)));
    } else if select_trailing {
        let mut style = Style::default();
        if is_cursor_row {
            style = style.patch(CURSOR_LINE_STYLE);
        }
        spans.push(Span::styled(" ", style.patch(SELECTION_STYLE)));
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_pane::EditorPane;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn scratch_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("illium-editor-highlight-tests")
            .join(format!("{:?}", std::thread::current().id()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    fn load_pane(file_name: &str, source: &str) -> EditorPane {
        let dir = scratch_dir();
        let path = dir.join(file_name);
        std::fs::write(&path, source).expect("write fixture");
        EditorPane::load(path).expect("load fixture")
    }

    /// Renders one editor pane's Source-mode highlighting into a
    /// `TestBackend` buffer, the same "render to a real ratatui `Buffer`
    /// and inspect cell colors" idiom `markdown::render`'s own tests use
    /// (see `local_image_paints_its_color_into_the_buffer`) -- proves the
    /// actual `Style`s landing on screen, not just that `syntax::highlight`
    /// returned something.
    fn render_to_buffer(editor: &EditorPane, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let tokens = editor
            .highlighted_lines()
            .expect("fixture uses a recognized language");
        editor.update_source_scroll_mirror(height);
        editor.update_source_scroll_col_mirror(width);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                render(frame, Rect::new(0, 0, width, height), editor, &tokens);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn keyword_and_string_literal_get_different_colors() {
        let editor = load_pane("sample.rs", "fn main() { let s = \"hi\"; }");
        let buffer = render_to_buffer(&editor, 80, 3);

        let gutter = usize::from(line_number_gutter_width(1));
        let rendered: String = (gutter..usize::from(buffer.area.width))
            .map(|x| buffer.cell((x as u16, 0)).unwrap().symbol())
            .collect();
        assert!(rendered.starts_with("fn main"), "rendered: {rendered:?}");

        let fn_color = buffer.cell(((gutter) as u16, 0)).unwrap().fg;
        let quote_offset = rendered.find('"').expect("string literal present");
        let string_color = buffer.cell(((gutter + quote_offset) as u16, 0)).unwrap().fg;

        assert_ne!(
            fn_color, string_color,
            "keyword and string literal should carry different syntax colors"
        );
        assert_ne!(fn_color, Color::Reset);
        assert_ne!(string_color, Color::Reset);
    }

    #[test]
    fn line_number_gutter_shows_one_based_row_numbers() {
        let editor = load_pane("sample.rs", "fn a() {}\nfn b() {}\nfn c() {}");
        let buffer = render_to_buffer(&editor, 40, 4);

        for (row, expected_number) in [(0u16, '1'), (1, '2'), (2, '3')] {
            let symbol = buffer.cell((1, row)).unwrap().symbol();
            assert_eq!(
                symbol,
                expected_number.to_string(),
                "row {row} gutter digit"
            );
        }
    }

    #[test]
    fn cursor_cell_is_reversed_and_current_line_is_underlined() {
        let editor = load_pane("sample.rs", "fn main() {}\nlet x = 1;");
        let buffer = render_to_buffer(&editor, 40, 3);

        let gutter = usize::from(line_number_gutter_width(2));
        // Cursor starts at (row 0, col 0) on load, i.e. the leading `f` of
        // `fn` right after the gutter on the first rendered row.
        let cursor_cell = buffer.cell((gutter as u16, 0)).unwrap();
        assert!(cursor_cell.modifier.contains(Modifier::REVERSED));
        assert!(cursor_cell.modifier.contains(Modifier::UNDERLINED));

        // The second row (not the cursor's row) must not be underlined.
        let other_row_cell = buffer.cell((gutter as u16, 1)).unwrap();
        assert!(!other_row_cell.modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn unrecognized_language_leaves_highlighted_lines_empty() {
        let editor = load_pane("notes.unknownext", "just plain text");
        assert!(editor.highlighted_lines().is_none());
    }
}
