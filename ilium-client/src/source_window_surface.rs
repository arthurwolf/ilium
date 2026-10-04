//! Immutable partial-source paint and hit mapping; owns actual admitted glyphs.
//! This DTO never pretends a viewport is a complete file/chapter body.
use crate::{document_preparation::PreparationKey, source_stream::Viewport};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

pub(crate) struct InstalledWindow {
    pub key: PreparationKey,
    pub physical_count: usize,
    pub viewport: ilium_execution::RetiringArc<Viewport>,
}
#[derive(Clone)]
pub(crate) struct PaintedWindow {
    pub installed: ilium_execution::RetiringArc<InstalledWindow>,
    pub content_area: Rect,
    pub minimap_area: Option<Rect>,
}
impl PaintedWindow {
    /// Hit testing reads only the exact immutable acknowledged row. Unicode
    /// clusters/tabs are mapped to their ORIGINAL character column, so clicking
    /// a second tab cell never becomes a character offset in expanded spaces.
    pub fn position(&self, x: u16, y: u16) -> Option<(usize, usize)> {
        if !self
            .content_area
            .contains(ratatui::layout::Position::new(x, y))
        {
            return None;
        }
        let row = self
            .installed
            .viewport
            .rows
            .get(usize::from(y - self.content_area.y))?;
        let gutter = if self.installed.key.gutter {
            crate::editor_highlight::line_number_gutter_width(self.installed.physical_count)
        } else {
            0
        };
        let mut target = usize::from(x - self.content_area.x).saturating_sub(usize::from(gutter));
        if self.installed.key.line_display == crate::config::LineDisplay::Clip {
            target = target.saturating_add(
                self.installed
                    .viewport
                    .left
                    .saturating_sub(row.glyphs.first().map_or(0, |glyph| glyph.display)),
            );
        }
        for glyph in &row.glyphs {
            if target < glyph.cells {
                return Some((glyph.physical, glyph.column));
            }
            target = target.saturating_sub(glyph.cells);
        }
        let last = row.glyphs.last();
        Some((
            row.physical,
            last.map_or(row.start_column, |glyph| {
                glyph.column.saturating_add(glyph.columns)
            }),
        ))
    }
}
pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    source: &InstalledWindow,
    cursor: (usize, usize),
    selection: Option<((usize, usize), (usize, usize))>,
    current: bool,
) {
    let gutter = if source.key.gutter {
        crate::editor_highlight::line_number_gutter_width(source.physical_count)
    } else {
        0
    };
    for (screen, row) in source
        .viewport
        .rows
        .iter()
        .take(usize::from(area.height))
        .enumerate()
    {
        let mut spans = Vec::with_capacity(row.glyphs.len() + 2);
        if gutter > 0 {
            let text = if row.start_column == 0 {
                format!(
                    "{:>width$} ",
                    row.physical + 1,
                    width = usize::from(gutter.saturating_sub(1))
                )
            } else {
                " ".repeat(usize::from(gutter))
            };
            frame.render_widget(
                Paragraph::new(text).style(Style::new().fg(ratatui::style::Color::DarkGray)),
                Rect::new(area.x, area.y + screen as u16, gutter.min(area.width), 1),
            );
        }
        for (index, glyph) in row.glyphs.iter().enumerate() {
            let mut style = source
                .viewport
                .styles
                .as_ref()
                .and_then(|styles| styles.rows.get(screen))
                .and_then(|styles| styles.get(index))
                .copied()
                .unwrap_or_default();
            if current && glyph.physical == cursor.0 {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            if current
                && glyph.physical == cursor.0
                && (glyph.column..glyph.column + glyph.columns).contains(&cursor.1)
            {
                style = style.add_modifier(Modifier::REVERSED);
            }
            if current
                && selection.is_some_and(|(start, end)| {
                    let at = (glyph.physical, glyph.column);
                    at >= start && at < end
                })
            {
                style = style.bg(ratatui::style::Color::LightBlue);
            }
            spans.push(Span::styled(glyph.text.as_str(), style));
        }
        if current
            && row.last
            && row.physical == cursor.0
            && row
                .glyphs
                .last()
                .map_or(row.start_column, |glyph| glyph.column + glyph.columns)
                == cursor.1
        {
            spans.push(Span::styled(
                " ",
                Style::new().add_modifier(Modifier::REVERSED),
            ));
        }
        let scroll = if source.key.line_display == crate::config::LineDisplay::Clip {
            source
                .viewport
                .left
                .saturating_sub(row.glyphs.first().map_or(0, |glyph| glyph.display))
                .min(usize::from(u16::MAX)) as u16
        } else {
            0
        };
        frame.render_widget(
            Paragraph::new(Line::from(spans)).scroll((0, scroll)),
            Rect::new(
                area.x.saturating_add(gutter),
                area.y + screen as u16,
                area.width.saturating_sub(gutter),
                1,
            ),
        );
    }
}
