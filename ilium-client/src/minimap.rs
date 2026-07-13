//! A dense, color-coded overview of a text buffer, rendered as one
//! character column summarizing every line's shape (length + kind:
//! heading/code/quote/list/blank/plain) -- an IDE-style minimap adapted to
//! a single terminal column, since plain buffer text doesn't have
//! per-pixel glyph rendering available (unlike the rasterized markdown
//! headers in `markdown/raster.rs`).
//!
//! Unlike a naive "one full-width block per row" bar chart, each row's
//! glyph run is width-scaled to how far that row's text actually reaches
//! (so short lines read as short marks, and a paragraph's last line
//! trails off before the right edge instead of slamming into it) and
//! indent-scaled to the row's leading whitespace (so nested code/list
//! text starts a little further right, echoing the real indentation).
//! The glyph itself is chosen from a density ramp (`·`, `-`, `=`, `≡`,
//! `≣`, `█`) rather than a single repeated block, so denser buckets read
//! as visually heavier strokes -- the closer analogue a monospace cell
//! grid has to VS Code's downscaled-text minimap. Heading rows are
//! additionally capped to a short mark, since real headings read as
//! short titles, not full-width bars.
//!
//! Works identically whether the pane is in Source or Rendered view mode
//! -- it always summarizes the underlying source lines, never the
//! rendered document, so toggling view mode never changes what the
//! minimap shows.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Density ramp from emptiest to inkiest. `≡` (U+2261) and `≣` (U+2263)
/// are genuine three- and four-stroke horizontal glyphs, closer to how a
/// dense paragraph or code line actually looks than a solid block.
const DENSITY_GLYPHS: [char; 6] = ['·', '-', '=', '≡', '≣', '█'];

/// A heading's bar is capped to this fraction of the minimap's width so
/// long header text still reads as a short "title mark" rather than a
/// full-width bar.
const HEADING_WIDTH_FRACTION: f32 = 0.55;

/// What kind of line a buffer row is, for the minimap's per-row accent
/// color. Declaration order is priority order when a bucket spans several
/// source lines (derived `Ord` picks the max): `Heading` wins over
/// `Code`, which wins over `Quote`, then `ListItem`, then plain `Text`,
/// then `Blank`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LineKind {
    Blank,
    Text,
    ListItem,
    Quote,
    Code,
    Heading,
}

/// Classifies one source line for the minimap via cheap text heuristics
/// (not a full markdown parse -- this runs over every line on every
/// render, so it must stay proportional to line count).
pub fn classify_line(line: &str) -> LineKind {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        LineKind::Blank
    } else if trimmed.starts_with('#') {
        LineKind::Heading
    } else if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
        LineKind::Code
    } else if trimmed.starts_with('>') {
        LineKind::Quote
    } else if is_bullet_list_marker(trimmed) || is_ordered_list_marker(trimmed) {
        LineKind::ListItem
    } else {
        LineKind::Text
    }
}

/// True if `trimmed` starts with a markdown bullet-list marker: `-`, `*`,
/// or `+` followed by a space (or the marker alone on the line). Requiring
/// the separator -- not just checking the first byte, which the previous
/// heuristic here did -- keeps prose that merely starts with the same
/// character from being misread as a list item: `**bold**`/`*emphasis*`,
/// a `---`/`***` thematic break, or `-3 degrees` would otherwise all match.
fn is_bullet_list_marker(trimmed: &str) -> bool {
    let mut chars = trimmed.chars();
    matches!(chars.next(), Some('-' | '*' | '+')) && matches!(chars.next(), Some(' ') | None)
}

/// True if `trimmed` starts with a markdown ordered-list marker: one or
/// more digits, then `.` or `)`, then a space (or end of line) -- the
/// same marker shape `markdown::checkbox::find_checkbox` recognizes. A
/// bare "starts with a digit" check (the previous heuristic here)
/// misclassified ordinary prose like "2024 was a great year" or "42
/// apples" as a list item, so this requires the actual punctuation+space
/// that makes it a marker.
fn is_ordered_list_marker(trimmed: &str) -> bool {
    let mut chars = trimmed.chars();
    let mut saw_digit = false;
    for c in chars.by_ref() {
        if c.is_ascii_digit() {
            saw_digit = true;
            continue;
        }
        if !saw_digit || !matches!(c, '.' | ')') {
            return false;
        }
        return matches!(chars.next(), Some(' ') | None);
    }
    false
}

fn kind_color(kind: LineKind) -> Color {
    match kind {
        LineKind::Blank => Color::Reset,
        LineKind::Text => Color::Gray,
        LineKind::ListItem => Color::Rgb(0x7a, 0xa2, 0xf7),
        LineKind::Quote => Color::Rgb(0x7a, 0x8a, 0xa0),
        LineKind::Code => Color::Rgb(0x2c, 0xb6, 0x7d),
        LineKind::Heading => Color::Rgb(0xe0, 0xaf, 0x68),
    }
}

/// Draws the minimap into `area`: one row per bucket of source lines
/// (buckets exist because a long buffer has more lines than the minimap
/// has rows). Each row's bar is indent-shifted and width-scaled against
/// `content_width` (the real editor content area's width, so the scale
/// matches what "a full line" actually looks like), glyph-weighted by
/// how dense the bucket's text is, and accent-colored by the bucket's
/// dominant line kind. The bucket containing `highlight_line` (the
/// cursor row in Source mode, or the scroll-proportional line in
/// Rendered mode) gets a full-width background wash as a "you are here"
/// viewport marker, independent of how far its own bar reaches.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    lines: &[&str],
    highlight_line: usize,
    content_width: u16,
) {
    if area.height == 0 || area.width == 0 || lines.is_empty() {
        return;
    }
    let rows = area.height as usize;
    let bucket_size = bucket_size(lines.len(), rows);
    let highlight_bucket = highlight_line / bucket_size;
    let width = usize::from(area.width).max(1);

    let mut rendered_lines = Vec::with_capacity(rows);
    for row in 0..rows {
        let start = row * bucket_size;
        if start >= lines.len() {
            rendered_lines.push(Line::from(""));
            continue;
        }
        let end = (start + bucket_size).min(lines.len());
        let bucket = &lines[start..end];
        let is_highlighted = row == highlight_bucket;

        rendered_lines.push(render_bucket_row(
            bucket,
            content_width,
            width,
            is_highlighted,
        ));
    }

    frame.render_widget(Paragraph::new(rendered_lines), area);
}

/// Builds one minimap row from the source lines in `bucket`: an
/// indent-then-glyph bar reflecting the bucket's longest line, tinted by
/// its dominant `LineKind`, optionally washed with a full-width "you are
/// here" background.
fn render_bucket_row<'a>(
    bucket: &[&str],
    content_width: u16,
    area_width: usize,
    is_highlighted: bool,
) -> Line<'a> {
    let dominant = bucket
        .iter()
        .map(|line| classify_line(line))
        .max()
        .unwrap_or(LineKind::Blank);

    let highlight_bg = Color::Rgb(0x3b, 0x41, 0x52);
    let base_style = Style::new().fg(kind_color(dominant));
    let bar_style = if is_highlighted {
        base_style.bg(highlight_bg).add_modifier(Modifier::BOLD)
    } else {
        base_style
    };

    if dominant == LineKind::Blank {
        return if is_highlighted {
            let pad: String = std::iter::repeat_n(' ', area_width).collect();
            Line::from(Span::styled(pad, Style::new().bg(highlight_bg)))
        } else {
            Line::from("")
        };
    }

    // A blank/whitespace-only line carries no visual "ink" (it never draws
    // its own bar -- see the early return above), so it must not stand in
    // for the bucket's content width either: a short line of real text next
    // to a longer whitespace-only line would otherwise have its bar sized
    // and indented from the blank line's raw character count instead of
    // from any actual content, making the bar collapse to a near-invisible
    // sliver despite real text being present. `dominant != Blank` here
    // guarantees at least one non-blank line exists.
    let content_lines: Vec<&str> = bucket
        .iter()
        .copied()
        .filter(|line| classify_line(line) != LineKind::Blank)
        .collect();

    let widest_line = content_lines
        .iter()
        .max_by_key(|line| line.chars().count())
        .copied()
        .unwrap_or("");
    let max_len = widest_line.chars().count();
    let total_len: usize = content_lines.iter().map(|line| line.chars().count()).sum();
    let mean_len = total_len / content_lines.len().max(1);

    let indent_cols = scale_to_width(leading_whitespace(widest_line), content_width, area_width);
    let mut bar_cols = scale_to_width(max_len, content_width, area_width)
        .saturating_sub(indent_cols)
        .max(1);
    if dominant == LineKind::Heading {
        bar_cols = bar_cols.min(((area_width as f32 * HEADING_WIDTH_FRACTION) as usize).max(1));
    }
    bar_cols = bar_cols.min(area_width.saturating_sub(indent_cols));

    let glyph = DENSITY_GLYPHS[density_index(mean_len, content_width)];
    let bar: String = std::iter::repeat_n(' ', indent_cols)
        .chain(std::iter::repeat_n(glyph, bar_cols))
        .collect();

    if is_highlighted {
        let drawn = indent_cols + bar_cols;
        let pad: String = std::iter::repeat_n(' ', area_width.saturating_sub(drawn)).collect();
        Line::from(vec![
            Span::styled(bar, bar_style),
            Span::styled(pad, Style::new().bg(highlight_bg)),
        ])
    } else {
        Line::from(Span::styled(bar, bar_style))
    }
}

/// Scales a character count (a line's length, or its leading whitespace)
/// from the real content area's width down to the minimap's own width.
fn scale_to_width(char_count: usize, content_width: u16, area_width: usize) -> usize {
    if content_width == 0 {
        return 0;
    }
    let ratio = (char_count as f32 / f32::from(content_width)).clamp(0.0, 1.0);
    (ratio * area_width as f32).round() as usize
}

/// Counts a line's leading whitespace characters, used to shift its
/// minimap bar right so nested/indented text reads as indented here too.
fn leading_whitespace(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).count()
}

/// How many source lines each minimap row summarizes, for an area of
/// `rows` rows over a buffer of `total_lines` lines. Shared by `render`
/// (bucket -> row) and `line_for_click` (row -> bucket) so the two
/// mappings can never drift apart.
fn bucket_size(total_lines: usize, rows: usize) -> usize {
    let rows = rows.max(1);
    (total_lines.saturating_add(rows - 1) / rows).max(1)
}

/// Inverts `render`'s bucketing: given a click at `row` (0-based, relative
/// to the minimap's own area -- i.e. already `mouse.row - area.y`) over an
/// area of `area_height` rows and a buffer of `total_lines` lines, returns
/// the source line to jump to (the first line of the clicked bucket).
pub fn line_for_click(row: u16, area_height: u16, total_lines: usize) -> usize {
    if total_lines == 0 || area_height == 0 {
        return 0;
    }
    let bucket_size = bucket_size(total_lines, area_height as usize);
    (row as usize * bucket_size).min(total_lines - 1)
}

/// Maps a line length (in characters -- typically a bucket's mean line
/// length, so the glyph reflects how full the bucket is on average, not
/// just its single longest line) to a density glyph index, scaled
/// against `content_width` (a line filling the whole editor width maps
/// to the inkiest glyph).
fn density_index(len: usize, content_width: u16) -> usize {
    if content_width == 0 {
        return 0;
    }
    let ratio = (len as f32 / f32::from(content_width)).clamp(0.0, 1.0);
    ((ratio * (DENSITY_GLYPHS.len() - 1) as f32).round() as usize).min(DENSITY_GLYPHS.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_line_shapes() {
        assert_eq!(classify_line(""), LineKind::Blank);
        assert_eq!(classify_line("# Title"), LineKind::Heading);
        assert_eq!(classify_line("```rust"), LineKind::Code);
        assert_eq!(classify_line("> quoted"), LineKind::Quote);
        assert_eq!(classify_line("- item"), LineKind::ListItem);
        assert_eq!(classify_line("1. item"), LineKind::ListItem);
        assert_eq!(classify_line("12) item"), LineKind::ListItem);
        assert_eq!(classify_line("plain text"), LineKind::Text);
        assert_eq!(
            classify_line("2024 was a great year"),
            LineKind::Text,
            "a digit-led prose line is not a list item without marker punctuation"
        );
        assert_eq!(classify_line("42 apples"), LineKind::Text);
        assert_eq!(classify_line("3D printing is neat"), LineKind::Text);
        assert_eq!(classify_line("*"), LineKind::ListItem);
        assert_eq!(classify_line("-"), LineKind::ListItem);
        assert_eq!(
            classify_line("**bold**"),
            LineKind::Text,
            "emphasis/bold markup is not a bullet list item without a following space"
        );
        assert_eq!(classify_line("*emphasis*"), LineKind::Text);
        assert_eq!(
            classify_line("---"),
            LineKind::Text,
            "a thematic break is not a bullet list item"
        );
        assert_eq!(classify_line("-3 degrees"), LineKind::Text);
    }

    #[test]
    fn density_index_scales_with_content_width() {
        assert_eq!(density_index(0, 80), 0);
        assert_eq!(density_index(80, 80), DENSITY_GLYPHS.len() - 1);
        assert_eq!(density_index(40, 80), DENSITY_GLYPHS.len() / 2);
    }

    /// Renders a small fixture buffer into a `TestBackend` and inspects the
    /// symbol grid, proving the width-scaled-bar behaviour end to end
    /// (not just the pure helpers in isolation): a heading's bar is capped
    /// shorter than a full-width paragraph line, a blank line draws no
    /// glyph at all, a short trailing line doesn't reach the right edge,
    /// and an indented line's glyph run starts with leading blank cells.
    #[test]
    fn render_shapes_bars_by_length_indent_and_kind() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let long_paragraph =
            "This is a fairly long paragraph line that stretches quite far across the editor.";
        let lines = [
            "# A Rather Long Heading Title For This Fixture Test Case",
            "",
            long_paragraph,
            "Short line.",
            "    indented code line",
        ];
        let content_width = long_paragraph.chars().count() as u16;

        let area_width = 24u16;
        let mut terminal = Terminal::new(TestBackend::new(area_width, 5)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    Rect::new(0, 0, area_width, 5),
                    &lines,
                    0,
                    content_width,
                );
            })
            .unwrap();

        let row_symbols = |row: u16| -> String {
            (0..area_width)
                .map(|col| {
                    terminal
                        .backend()
                        .buffer()
                        .cell((col, row))
                        .unwrap()
                        .symbol()
                })
                .collect()
        };
        let bar_len = |row: u16| row_symbols(row).trim_end().chars().count();

        let heading_len = bar_len(0);
        let blank_len = bar_len(1);
        let paragraph_len = bar_len(2);
        let short_line = row_symbols(3);
        let indented_line = row_symbols(4);

        assert_eq!(blank_len, 0, "blank line must draw no glyph");
        assert!(
            paragraph_len >= area_width as usize - 2,
            "the fixture's longest line should reach ~the full width, got {paragraph_len}"
        );
        assert!(
            heading_len < paragraph_len,
            "heading bar ({heading_len}) must be capped shorter than a full-width line ({paragraph_len})"
        );
        assert!(
            short_line.trim_end().chars().count() < paragraph_len,
            "a short trailing line must not stretch to the full width"
        );
        assert!(
            indented_line.starts_with(' ') && indented_line.trim_start() != indented_line,
            "an indented source line must start its bar with leading blank cells"
        );
    }

    /// A line of only whitespace (e.g. trailing spaces on an otherwise
    /// empty line, common in real files) is still classified `Blank` and
    /// must draw no glyph at all -- same as a truly empty `""` line.
    #[test]
    fn whitespace_only_line_draws_no_glyph() {
        let bucket = ["   "];
        let line = render_bucket_row(&bucket, 80, 24, false);
        let rendered: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(
            rendered.trim().is_empty(),
            "a whitespace-only line must draw no visible glyph, got {rendered:?}"
        );
    }

    #[test]
    fn heading_dominates_bucket_classification() {
        let bucket = ["plain", "# Heading", "> quote"];
        let dominant = bucket.iter().map(|line| classify_line(line)).max().unwrap();
        assert_eq!(dominant, LineKind::Heading);
    }

    #[test]
    fn line_for_click_inverts_render_bucketing() {
        // 100 lines over a 10-row minimap: 10 lines per bucket.
        assert_eq!(line_for_click(0, 10, 100), 0);
        assert_eq!(line_for_click(1, 10, 100), 10);
        assert_eq!(line_for_click(9, 10, 100), 90);
    }

    #[test]
    fn line_for_click_clamps_to_last_line() {
        // A click past the last populated row must not overshoot the buffer.
        assert_eq!(line_for_click(9, 10, 3), 2);
    }

    #[test]
    fn line_for_click_on_empty_buffer_is_zero() {
        assert_eq!(line_for_click(4, 10, 0), 0);
    }
}
