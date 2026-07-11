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

fn block_height(block: &RenderedBlock, width: u16) -> u16 {
    match block {
        RenderedBlock::Text(lines) => u16::try_from(
            Paragraph::new(lines.clone())
                .wrap(Wrap { trim: false })
                .line_count(width.max(1)),
        )
        .unwrap_or(u16::MAX),
        RenderedBlock::BlankLines(lines) => u16::try_from(lines.len()).unwrap_or(u16::MAX),
        RenderedBlock::Header(protocol) | RenderedBlock::Image(protocol) => protocol.size().height,
        RenderedBlock::Placeholder(_) => 1,
    }
}

/// Draws `document` into `area`, scrolled down by `scroll` rows.
pub fn render(frame: &mut Frame, area: Rect, document: &RenderedDocument, scroll: u16) {
    let mut y = i64::from(area.y) - i64::from(scroll);
    for block in &document.blocks {
        let height = block_height(block, area.width);
        draw_block(frame, area, block, y, height);
        y += i64::from(height);
    }
}

fn draw_block(frame: &mut Frame, area: Rect, block: &RenderedBlock, y: i64, height: u16) {
    let area_top = i64::from(area.y);
    let area_bottom = i64::from(area.bottom());
    let visible_top = y.max(area_top);
    let visible_bottom = (y + i64::from(height)).min(area_bottom);
    if visible_bottom <= visible_top || height == 0 {
        return;
    }
    let visible_height = (visible_bottom - visible_top) as u16;
    let rect = Rect::new(area.x, visible_top as u16, area.width, visible_height);

    match block {
        RenderedBlock::Text(lines) => {
            let skip = (visible_top - y) as u16;
            let paragraph = Paragraph::new(lines.clone())
                .wrap(Wrap { trim: false })
                .scroll((skip, 0));
            frame.render_widget(paragraph, rect);
        }
        RenderedBlock::BlankLines(lines) => {
            let skip = (visible_top - y) as u16;
            frame.render_widget(
                Paragraph::new(lines.clone()).scroll((skip, 0)),
                rect,
            );
        }
        RenderedBlock::Placeholder(line) => {
            frame.render_widget(Paragraph::new(line.clone()), rect);
        }
        RenderedBlock::Header(protocol) | RenderedBlock::Image(protocol) => {
            // Graphics protocols declare a fixed cell size and (mostly)
            // can't partially render past it -- only draw once fully
            // visible; it pops in as the user finishes scrolling to it.
            if visible_top == y && visible_height == height {
                frame.render_widget(Image::new(protocol), rect);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::text::Line;
    use ratatui::Terminal;

    /// Builds a text/space/text document used to prove height and drawing
    /// share one vertical-layout contract.
    fn spaced_document() -> RenderedDocument {
        RenderedDocument {
            blocks: vec![
                RenderedBlock::Text(vec![Line::from("before")]),
                RenderedBlock::BlankLines(vec![Line::default(), Line::default()]),
                RenderedBlock::Text(vec![Line::from("after")]),
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
