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

/// Total content height (in terminal rows) of `document` wrapped to
/// `width` -- the caller uses this to clamp scroll offsets.
pub fn content_height(document: &RenderedDocument, width: u16) -> u16 {
    document
        .blocks
        .iter()
        .map(|block| block_height(block, width))
        .fold(0u16, u16::saturating_add)
}

/// Largest valid row offset for a document in the exact viewport geometry
/// used to draw it. Centralizing this prevents input paths from accidentally
/// counting editor chrome as visible Markdown rows.
pub fn max_scroll(document: &RenderedDocument, width: u16, viewport_height: u16) -> u16 {
    content_height(document, width).saturating_sub(viewport_height)
}

/// Builds the word-wrapped `Paragraph` for a `Text` block's lines. The one
/// place that owns this wrap configuration, so `block_height` (measuring
/// for a scroll-clamp pass) and `draw_block` (measuring immediately before
/// drawing) can't drift apart on how a text block wraps.
fn text_paragraph(lines: &[ratatui::text::Line<'static>]) -> Paragraph<'static> {
    Paragraph::new(lines.to_vec()).wrap(Wrap { trim: false })
}

fn block_height(block: &RenderedBlock, width: u16) -> u16 {
    match block {
        RenderedBlock::Text(lines) => {
            u16::try_from(text_paragraph(lines).line_count(width.max(1))).unwrap_or(u16::MAX)
        }
        RenderedBlock::BlankLines(lines) => u16::try_from(lines.len()).unwrap_or(u16::MAX),
        RenderedBlock::Header(protocol) | RenderedBlock::Image(protocol) => protocol.size().height,
        RenderedBlock::Placeholder(_) => 1,
    }
}

/// Draws `document` into `area`, scrolled down by `scroll` rows.
pub fn render(frame: &mut Frame, area: Rect, document: &RenderedDocument, scroll: u16) {
    let mut y = i64::from(area.y) - i64::from(scroll);
    for block in &document.blocks {
        y += draw_block(frame, area, block, y);
    }
}

/// Draws one block at running cursor `y` (clipped to `area`) and returns its
/// full, unclipped height so the caller can advance past it regardless of
/// how much -- if any -- was actually on screen.
///
/// A `Text` block's word-wrapped `Paragraph` is built exactly once here: the
/// same owned clone of `lines` backs both the `line_count` height query and
/// the widget actually handed to `render_widget`. Measuring and drawing used
/// to build that `Paragraph` from a fresh `lines.clone()` each, which meant
/// cloning every visible text block's content twice per frame -- and this
/// runs on every frame a Rendered-mode markdown pane is on screen.
fn draw_block(frame: &mut Frame, area: Rect, block: &RenderedBlock, y: i64) -> i64 {
    let area_top = i64::from(area.y);
    let area_bottom = i64::from(area.bottom());

    match block {
        RenderedBlock::Text(lines) => {
            let paragraph = text_paragraph(lines);
            let height = u16::try_from(paragraph.line_count(area.width.max(1))).unwrap_or(u16::MAX);
            let visible = visible_rect(area, area_top, area_bottom, y, height);
            if let Some((visible_top, _, rect)) = visible {
                let skip = (visible_top - y) as u16;
                frame.render_widget(paragraph.scroll((skip, 0)), rect);
            }
            i64::from(height)
        }
        RenderedBlock::BlankLines(lines) => {
            let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
            let visible = visible_rect(area, area_top, area_bottom, y, height);
            if let Some((visible_top, _, rect)) = visible {
                let skip = (visible_top - y) as u16;
                frame.render_widget(Paragraph::new((**lines).clone()).scroll((skip, 0)), rect);
            }
            i64::from(height)
        }
        RenderedBlock::Placeholder(line) => {
            let height: u16 = 1;
            if let Some((_, _, rect)) = visible_rect(area, area_top, area_bottom, y, height) {
                frame.render_widget(Paragraph::new(line.clone()), rect);
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
        assert_eq!(content_height(&spaced_document(), 20), 4);
    }

    #[test]
    fn render_leaves_blank_rows_before_following_content() {
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).unwrap();
        terminal
            .draw(|frame| render(frame, Rect::new(0, 0, 20, 4), &spaced_document(), 0))
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((0, 0)).unwrap().symbol(), "b");
        assert_eq!(buffer.cell((0, 1)).unwrap().symbol(), " ");
        assert_eq!(buffer.cell((0, 2)).unwrap().symbol(), " ");
        assert_eq!(buffer.cell((0, 3)).unwrap().symbol(), "a");
    }

    #[test]
    fn scrolling_across_blank_rows_keeps_following_content_aligned() {
        let scroll = max_scroll(&spaced_document(), 20, 2);
        assert_eq!(scroll, 2);

        let mut terminal = Terminal::new(TestBackend::new(20, 2)).unwrap();
        terminal
            .draw(|frame| render(frame, Rect::new(0, 0, 20, 2), &spaced_document(), scroll))
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((0, 0)).unwrap().symbol(), " ");
        assert_eq!(buffer.cell((0, 1)).unwrap().symbol(), "a");
    }

    #[test]
    fn fitted_document_has_no_scroll_range() {
        assert_eq!(max_scroll(&spaced_document(), 20, 4), 0);
    }
}
