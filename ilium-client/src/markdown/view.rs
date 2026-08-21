//! Draws a `RenderedDocument` into a `Rect`, scrolled by a row offset.
//!
//! Unlike `mdfried`'s hand-rolled buffer painter, this walks blocks with a
//! running Y cursor and calls `Frame::render_widget` per block -- text and
//! styled blank rows via `Paragraph` (which owns word-wrap), headers/images
//! via `ratatui_image::Image` -- since all are ordinary `Widget`s and don't
//! need a shared abstraction beyond "here's your `Rect`".

use ratatui::layout::Rect;
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;
use ratatui_image::Image;

use super::render::{RenderedBlock, RenderedDocument};
use crate::config::LineDisplay;

/// Total content height (in terminal rows) of `document` at `width` under
/// `line_display` -- the caller uses this to clamp scroll offsets.
pub fn content_height(document: &RenderedDocument, width: u16, line_display: LineDisplay) -> u16 {
    document
        .blocks
        .iter()
        .map(|block| block_height(block, width, line_display))
        .fold(0u16, u16::saturating_add)
}

/// Largest valid row offset for a document in the exact viewport geometry
/// used to draw it. Centralizing this prevents input paths from accidentally
/// counting editor chrome as visible Markdown rows.
pub fn max_scroll(
    document: &RenderedDocument,
    width: u16,
    viewport_height: u16,
    line_display: LineDisplay,
) -> u16 {
    content_height(document, width, line_display).saturating_sub(viewport_height)
}

/// Builds a text block with the exact overflow policy selected in the
/// editor toolbar. Keeping this decision here makes rendered Markdown's
/// height measurement and drawing agree on whether a long logical line
/// consumes one clipped row or several soft-wrapped rows.
fn text_paragraph(
    lines: &[ratatui::text::Line<'static>],
    line_display: LineDisplay,
) -> Paragraph<'static> {
    let paragraph = Paragraph::new(lines.to_vec());
    if matches!(line_display, LineDisplay::Wrap) {
        paragraph.wrap(Wrap { trim: false })
    } else {
        paragraph
    }
}

fn block_height(block: &RenderedBlock, width: u16, line_display: LineDisplay) -> u16 {
    match block {
        RenderedBlock::Text(lines) => paragraph_height(lines, width, line_display),
        // A placeholder line can carry an interpolated URL or filesystem path
        // (see `render_image`/`render_block`'s fallback text) that's routinely
        // wider than the pane -- it must share Text's overflow policy instead
        // of a hardcoded one-row assumption, or Wrap mode would silently clip
        // it while every neighboring text block wraps.
        RenderedBlock::Placeholder(line) => {
            paragraph_height(std::slice::from_ref(line), width, line_display)
        }
        RenderedBlock::BlankLines(lines) => u16::try_from(lines.len()).unwrap_or(u16::MAX),
        RenderedBlock::Header(protocol) | RenderedBlock::Image(protocol) => protocol.size().height,
    }
}

fn paragraph_height(
    lines: &[ratatui::text::Line<'static>],
    width: u16,
    line_display: LineDisplay,
) -> u16 {
    u16::try_from(text_paragraph(lines, line_display).line_count(width.max(1))).unwrap_or(u16::MAX)
}

/// Draws `document` into `area`, scrolled down by `scroll` rows under the
/// selected line-overflow policy.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    document: &RenderedDocument,
    scroll: u16,
    line_display: LineDisplay,
) {
    let mut y = i64::from(area.y) - i64::from(scroll);
    for block in &document.blocks {
        y += draw_block(frame, area, block, y, line_display);
    }
}

/// Draws one block at running cursor `y` (clipped to `area`) and returns its
/// full, unclipped height so the caller can advance past it regardless of
/// how much -- if any -- was actually on screen.
fn draw_block(
    frame: &mut Frame,
    area: Rect,
    block: &RenderedBlock,
    y: i64,
    line_display: LineDisplay,
) -> i64 {
    let area_top = i64::from(area.y);
    let area_bottom = i64::from(area.bottom());

    match block {
        RenderedBlock::Text(lines) => {
            draw_text_block(frame, area, area_top, area_bottom, lines, y, line_display)
        }
        // Shares `Text`'s overflow policy -- see the matching comment on
        // `block_height` for why a placeholder can't stay a hardcoded 1 row.
        RenderedBlock::Placeholder(line) => draw_text_block(
            frame,
            area,
            area_top,
            area_bottom,
            std::slice::from_ref(line),
            y,
            line_display,
        ),
        RenderedBlock::BlankLines(lines) => {
            let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
            let visible = visible_rect(area, area_top, area_bottom, y, height);
            if let Some((visible_top, _, rect)) = visible {
                let skip = (visible_top - y) as u16;
                frame.render_widget(Paragraph::new((**lines).clone()).scroll((skip, 0)), rect);
            }
            i64::from(height)
        }
        RenderedBlock::Header(protocol) | RenderedBlock::Image(protocol) => {
            let height = protocol.size().height;
            if let Some((visible_top, visible_height, rect)) =
                visible_rect(area, area_top, area_bottom, y, height)
            {
                // Graphics protocols declare a fixed cell size and (mostly)
                // can't partially render past it -- only draw once fully
                // visible; it pops in as the user finishes scrolling to it.
                if visible_top == y && visible_height == height {
                    frame.render_widget(Image::new(protocol), rect);
                }
            }
            i64::from(height)
        }
    }
}

/// Builds and draws one word-wrap/clip text block (`Text` or `Placeholder`)
/// at running cursor `y`, following the shared overflow policy in
/// `text_paragraph`. The `Paragraph` is built exactly once here: the same
/// owned clone of `lines` backs both the `line_count` height query and the
/// widget actually handed to `render_widget`.
fn draw_text_block(
    frame: &mut Frame,
    area: Rect,
    area_top: i64,
    area_bottom: i64,
    lines: &[ratatui::text::Line<'static>],
    y: i64,
    line_display: LineDisplay,
) -> i64 {
    let paragraph = text_paragraph(lines, line_display);
    let height = u16::try_from(paragraph.line_count(area.width.max(1))).unwrap_or(u16::MAX);
    let visible = visible_rect(area, area_top, area_bottom, y, height);
    if let Some((visible_top, _, rect)) = visible {
        let skip = (visible_top - y) as u16;
        frame.render_widget(paragraph.scroll((skip, 0)), rect);
    }
    i64::from(height)
}

/// Clips a block spanning rows `[y, y + height)` to `area`'s visible rows,
/// returning the visible top row, visible row count, and destination
/// `Rect` -- or `None` when the block is entirely scrolled out of view.
fn visible_rect(
    area: Rect,
    area_top: i64,
    area_bottom: i64,
    y: i64,
    height: u16,
) -> Option<(i64, u16, Rect)> {
    if height == 0 {
        return None;
    }
    let visible_top = y.max(area_top);
    let visible_bottom = (y + i64::from(height)).min(area_bottom);
    if visible_bottom <= visible_top {
        return None;
    }
    let visible_height = (visible_bottom - visible_top) as u16;
    let rect = Rect::new(area.x, visible_top as u16, area.width, visible_height);
    Some((visible_top, visible_height, rect))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::text::Line;
    use ratatui::Terminal;

    /// Builds a text/space/text document used to prove height and drawing
    /// share one vertical-layout contract.
    fn spaced_document() -> RenderedDocument {
        RenderedDocument {
            blocks: vec![
                RenderedBlock::Text(Arc::new(vec![Line::from("before")])),
                RenderedBlock::BlankLines(Arc::new(vec![Line::default(), Line::default()])),
                RenderedBlock::Text(Arc::new(vec![Line::from("after")])),
            ],
        }
    }

    #[test]
    fn content_height_counts_blank_line_blocks() {
        assert_eq!(content_height(&spaced_document(), 20, LineDisplay::Wrap), 4);
    }

    #[test]
    fn render_leaves_blank_rows_before_following_content() {
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    Rect::new(0, 0, 20, 4),
                    &spaced_document(),
                    0,
                    LineDisplay::Wrap,
                )
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((0, 0)).unwrap().symbol(), "b");
        assert_eq!(buffer.cell((0, 1)).unwrap().symbol(), " ");
        assert_eq!(buffer.cell((0, 2)).unwrap().symbol(), " ");
        assert_eq!(buffer.cell((0, 3)).unwrap().symbol(), "a");
    }

    #[test]
    fn scrolling_across_blank_rows_keeps_following_content_aligned() {
        let scroll = max_scroll(&spaced_document(), 20, 2, LineDisplay::Wrap);
        assert_eq!(scroll, 2);

        let mut terminal = Terminal::new(TestBackend::new(20, 2)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    Rect::new(0, 0, 20, 2),
                    &spaced_document(),
                    scroll,
                    LineDisplay::Wrap,
                )
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((0, 0)).unwrap().symbol(), " ");
        assert_eq!(buffer.cell((0, 1)).unwrap().symbol(), "a");
    }

    #[test]
    fn fitted_document_has_no_scroll_range() {
        assert_eq!(max_scroll(&spaced_document(), 20, 4, LineDisplay::Wrap), 0);
    }

    #[test]
    fn line_display_changes_both_rendered_height_and_visible_text() {
        let document = RenderedDocument {
            blocks: vec![RenderedBlock::Text(Arc::new(vec![Line::from("abcdef")]))],
        };
        assert_eq!(content_height(&document, 3, LineDisplay::Clip), 1);
        assert_eq!(content_height(&document, 3, LineDisplay::Wrap), 2);

        let mut terminal = Terminal::new(TestBackend::new(3, 2)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    Rect::new(0, 0, 3, 2),
                    &document,
                    0,
                    LineDisplay::Clip,
                )
            })
            .unwrap();
        let clipped = terminal.backend().buffer();
        assert_eq!(clipped.cell((0, 0)).unwrap().symbol(), "a");
        assert_eq!(clipped.cell((0, 1)).unwrap().symbol(), " ");

        terminal
            .draw(|frame| {
                render(
                    frame,
                    Rect::new(0, 0, 3, 2),
                    &document,
                    0,
                    LineDisplay::Wrap,
                )
            })
            .unwrap();
        let wrapped = terminal.backend().buffer();
        assert_eq!(wrapped.cell((0, 1)).unwrap().symbol(), "d");
    }
}
