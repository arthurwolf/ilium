//! Text, bar and table helpers of the Optimization tab. Pure functions over
//! strings and ratatui lines; display width is measured in terminal cells.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Greedy word wrap to `width` cells (at least 8). A word longer than the
/// width is split.
pub(super) fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut word = word.to_owned();
        while UnicodeWidthStr::width(word.as_str()) > width {
            let (head, tail) = split_at_width(&word, width);
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            lines.push(head);
            word = tail;
        }
        let candidate_width = if line.is_empty() {
            UnicodeWidthStr::width(word.as_str())
        } else {
            UnicodeWidthStr::width(line.as_str()) + 1 + UnicodeWidthStr::width(word.as_str())
        };
        if !line.is_empty() && candidate_width > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Splits `text` after at most `width` cells.
fn split_at_width(text: &str, width: usize) -> (String, String) {
    let mut used = 0;
    let mut head = String::new();
    let mut rest = text.chars();
    for character in rest.by_ref() {
        let cells = UnicodeWidthChar::width(character).unwrap_or(0);
        if used + cells > width && !head.is_empty() {
            let mut tail = String::new();
            tail.push(character);
            tail.extend(rest);
            return (head, tail);
        }
        used += cells;
        head.push(character);
    }
    (head, String::new())
}

/// `text` cut to `width` cells, with a trailing `…` when something was cut.
pub(super) fn truncate_to(text: &str, width: usize) -> String {
    if UnicodeWidthStr::width(text) <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let (mut head, _) = split_at_width(text, width.saturating_sub(1));
    head.push('…');
    head
}

pub(super) fn pad_right(text: &str, width: usize) -> String {
    let used = UnicodeWidthStr::width(text);
    format!("{text}{}", " ".repeat(width.saturating_sub(used)))
}

pub(super) fn pad_left(text: &str, width: usize) -> String {
    let used = UnicodeWidthStr::width(text);
    format!("{}{text}", " ".repeat(width.saturating_sub(used)))
}

/// The filled and the empty part of a `width`-cell bar for `fraction` (0..1).
pub(super) fn bar_parts(fraction: f64, width: usize) -> (String, String) {
    let fraction = if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let filled = ((fraction * width as f64).round() as usize).min(width);
    ("█".repeat(filled), "░".repeat(width - filled))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Align {
    Left,
    Right,
}

/// One table column. Columns are listed in display order; when the table is
/// too wide the columns with the highest `drop_rank` go first (rank 0 is
/// never dropped).
#[derive(Debug, Clone, Copy)]
pub(super) struct Column {
    pub title: &'static str,
    pub width: usize,
    pub align: Align,
    pub drop_rank: u8,
}

impl Column {
    pub(super) const fn left(title: &'static str, width: usize, drop_rank: u8) -> Self {
        Self {
            title,
            width,
            align: Align::Left,
            drop_rank,
        }
    }
    pub(super) const fn right(title: &'static str, width: usize, drop_rank: u8) -> Self {
        Self {
            title,
            width,
            align: Align::Right,
            drop_rank,
        }
    }
}

/// One table row: a text per column (every column, including those that end
/// up dropped) and the style of the whole row.
#[derive(Debug, Clone)]
pub(super) struct TableRow {
    pub cells: Vec<String>,
    pub style: Style,
}

const COLUMN_GAP: usize = 2;
const NARROW_COLUMN_GAP: usize = 1;

/// Indices of the columns that fit in `available` cells, and the gap used.
fn fit_columns(columns: &[Column], available: usize) -> (Vec<usize>, usize) {
    let total = |keep: &[usize], gap: usize| {
        keep.iter()
            .map(|&index| columns[index].width)
            .sum::<usize>()
            + gap * keep.len().saturating_sub(1)
    };
    let mut keep: Vec<usize> = (0..columns.len()).collect();
    loop {
        if total(&keep, COLUMN_GAP) <= available {
            return (keep, COLUMN_GAP);
        }
        if total(&keep, NARROW_COLUMN_GAP) <= available {
            return (keep, NARROW_COLUMN_GAP);
        }
        let victim = keep
            .iter()
            .enumerate()
            .filter(|(_, &index)| columns[index].drop_rank > 0)
            .max_by_key(|(position, &index)| (columns[index].drop_rank, *position))
            .map(|(position, _)| position);
        match victim {
            Some(position) => {
                keep.remove(position);
            }
            None => return (keep, NARROW_COLUMN_GAP),
        }
    }
}

fn cell_text(column: &Column, text: &str) -> String {
    let cut = truncate_to(text, column.width);
    match column.align {
        Align::Left => pad_right(&cut, column.width),
        Align::Right => pad_left(&cut, column.width),
    }
}

/// Header line (dim) plus one line per row, `indent` cells from the left.
pub(super) fn table_lines(
    columns: &[Column],
    rows: &[TableRow],
    available: usize,
    indent: usize,
    header_style: Style,
) -> Vec<Line<'static>> {
    let (keep, gap) = fit_columns(columns, available.saturating_sub(indent));
    let separator = " ".repeat(gap);
    let join = |texts: Vec<String>| texts.join(&separator);
    let mut lines = Vec::with_capacity(rows.len() + 1);
    let header: Vec<String> = keep
        .iter()
        .map(|&index| cell_text(&columns[index], columns[index].title))
        .collect();
    lines.push(Line::from(Span::styled(
        format!("{}{}", " ".repeat(indent), join(header)),
        header_style,
    )));
    for row in rows {
        let cells: Vec<String> = keep
            .iter()
            .map(|&index| {
                cell_text(
                    &columns[index],
                    row.cells.get(index).map_or("", String::as_str),
                )
            })
            .collect();
        lines.push(Line::from(Span::styled(
            format!("{}{}", " ".repeat(indent), join(cells)),
            row.style,
        )));
    }
    lines
}

/// Whether column `index` survives at `available` cells (tests).
#[cfg(test)]
pub(super) fn kept_columns(columns: &[Column], available: usize) -> Vec<&'static str> {
    fit_columns(columns, available)
        .0
        .into_iter()
        .map(|index| columns[index].title)
        .collect()
}
