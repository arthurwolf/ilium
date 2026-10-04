//! Works out where newly drawn text came from.
//!
//! The host gives the scene one occupancy mask per update. Comparing the new
//! mask with the previous one finds the cells that just became occupied. For
//! each row that gained cells the analysis asks: is this row the previous
//! content of a neighbouring row (the screen scrolled vertically), or the same
//! row moved sideways (text inserted or shifted), or neither (text appeared)?
//! The answer is a push per new cell: the direction the content travelled, or
//! "away from the cell" for text with no visible source.

use crate::scene::OccupancyMask;

/// Why a cell became occupied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PushKind {
    /// Content moved here; the push follows its motion. `distance` is the
    /// number of cells it travelled.
    Scroll { dx: i32, dy: i32, distance: i32 },
    /// Content appeared with no matching source.
    Appear,
}

/// One newly occupied cell and the reason.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellPush {
    pub column: i32,
    pub row: i32,
    pub kind: PushKind,
}

/// A row counts as moved when this share of its occupied cells matches.
const MATCH_SHARE: f32 = 0.75;
/// Fewest occupied cells in a row for it to be judged as moved content.
const MIN_ROW_CELLS: usize = 3;

fn row_cells(mask: &OccupancyMask, row: i32) -> impl Iterator<Item = i32> + '_ {
    (0..i32::from(mask.width())).filter(move |column| mask.is_occupied(*column, row))
}

/// Cells of `current` row `row` that were occupied in `previous` at the offset.
fn matches_at(
    previous: &OccupancyMask,
    current: &OccupancyMask,
    row: i32,
    dx: i32,
    dy: i32,
) -> usize {
    row_cells(current, row)
        .filter(|column| {
            let (source_column, source_row) = (column - dx, row - dy);
            source_row >= 0
                && source_row < i32::from(previous.height())
                && source_column >= 0
                && source_column < i32::from(previous.width())
                && previous.is_occupied(source_column, source_row)
        })
        .count()
}

/// The source row of a vertical move must have changed, otherwise the new row
/// is a copy of a row that is still there (a new line under an old one).
fn source_row_vacated(
    previous: &OccupancyMask,
    current: &OccupancyMask,
    row: i32,
    dy: i32,
) -> bool {
    let source_row = row - dy;
    if source_row < 0 || source_row >= i32::from(previous.height()) {
        return true;
    }
    let before = row_cells(previous, source_row).count();
    if before == 0 {
        return true;
    }
    let kept = row_cells(previous, source_row)
        .filter(|column| current.is_occupied(*column, source_row))
        .count();
    (kept as f32) < before as f32 * 0.95
}

/// Best shift of `row` along one axis: `(offset, matches)`, preferring small
/// offsets. `vertical` selects the axis.
fn best_shift(
    previous: &OccupancyMask,
    current: &OccupancyMask,
    row: i32,
    range: i32,
    vertical: bool,
    row_cell_count: usize,
) -> Option<(i32, usize)> {
    let still = matches_at(previous, current, row, 0, 0);
    let mut best: Option<(i32, usize)> = None;
    for distance in 1..=range {
        for sign in [-1, 1] {
            let offset = distance * sign;
            let (dx, dy) = if vertical { (0, offset) } else { (offset, 0) };
            let matched = matches_at(previous, current, row, dx, dy);
            if matched as f32 >= row_cell_count as f32 * MATCH_SHARE
                && matched > still
                && best.is_none_or(|(_, kept)| matched > kept)
                && (!vertical || source_row_vacated(previous, current, row, dy))
            {
                best = Some((offset, matched));
            }
        }
    }
    best
}

/// Compares two masks of equal size and classifies every newly occupied cell.
/// `scroll_range` is the largest recognised jump in cells.
pub fn analyze(
    previous: &OccupancyMask,
    current: &OccupancyMask,
    scroll_range: i32,
) -> Vec<CellPush> {
    let mut pushes = Vec::new();
    if previous.width() != current.width() || previous.height() != current.height() {
        return pushes;
    }
    for row in 0..i32::from(current.height()) {
        let new_cells: Vec<i32> = row_cells(current, row)
            .filter(|column| !previous.is_occupied(*column, row))
            .collect();
        if new_cells.is_empty() {
            continue;
        }
        let count = row_cells(current, row).count();
        let shift = if count >= MIN_ROW_CELLS {
            best_shift(previous, current, row, scroll_range, true, count)
                .map(|(offset, _)| (0, offset))
                .or_else(|| {
                    best_shift(previous, current, row, scroll_range, false, count)
                        .map(|(offset, _)| (offset, 0))
                })
        } else {
            None
        };
        for column in new_cells {
            let kind = match shift {
                Some((dx, dy)) => PushKind::Scroll {
                    dx: dx.signum(),
                    dy: dy.signum(),
                    distance: dx.abs().max(dy.abs()),
                },
                None => PushKind::Appear,
            };
            pushes.push(CellPush { column, row, kind });
        }
    }
    pushes
}
