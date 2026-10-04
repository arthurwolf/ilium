//! Indexed lookup into the admitted immutable rows of one window.
use crate::{editor_pane::SourceVisualRow, source_stream::Viewport};
pub(crate) fn row_at(viewport: &Viewport, visual: usize) -> Option<SourceVisualRow> {
    let row = viewport.rows.get(visual.checked_sub(viewport.top)?)?;
    let last = row.glyphs.last();
    Some(SourceVisualRow {
        source_row: row.physical,
        start_byte: row.start_byte,
        end_byte: last.map_or(row.start_byte, |g| g.byte.saturating_add(g.source_bytes)),
        start_column: row.start_column,
        end_column: last.map_or(row.start_column, |g| g.column.saturating_add(g.columns)),
        first_in_source_row: row.start_column == 0,
        last_in_source_row: row.last,
    })
}
