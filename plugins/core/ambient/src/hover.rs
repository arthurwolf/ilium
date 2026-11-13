//! The hover popover that explains why a choice option (for example the GPU renderer) is disabled.
//!
//! Ported from `ilium-client/src/animation_hover.rs` and the word wrapping of
//! `ilium-client/src/last_prompt_banner.rs`. The pointer path only records which row it rests on;
//! the tick reveals the popover once the pointer has been still for `HOVER_DELAY_MS`. Any key or
//! click, leaving the row, scrolling and leaving the Animations tab dismiss it. `popover_geometry`
//! is a pure function of the screen, the controls panel and the row, so it can be tested for
//! staying on screen. Time is a millisecond count supplied by the caller (no clock here).

use crate::rows::DisabledOption;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// How long the pointer must rest on the row before the popover appears.
pub const HOVER_DELAY_MS: u64 = 150;
/// Preferred and minimum popover widths, border included.
pub const MAXIMUM_WIDTH: u16 = 46;
const MINIMUM_SIDE_WIDTH: u16 = 24;
/// A popover needs a border, one text row and a border.
pub const MINIMUM_HEIGHT: u16 = 3;
/// Width of the animations panel (the kernel `PANEL_WIDTH`).
pub const PANEL_WIDTH: u16 = 112;

/// A screen rectangle in cells.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Whether the cell at column `x`, row `y` lies inside.
    pub fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub fn right(&self) -> u16 {
        self.x.saturating_add(self.width)
    }

    pub fn bottom(&self) -> u16 {
        self.y.saturating_add(self.height)
    }

    pub fn intersects(&self, other: Rect) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }
}

/// The controls panel inside the content `area` (the kernel `layout(area).panel`).
pub fn panel_rect(area: Rect) -> Rect {
    Rect::new(area.x, area.y, PANEL_WIDTH.min(area.width), area.height)
}

/// The row the pointer rests on and whether the delay has elapsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationHover {
    pub row: usize,
    pub since_ms: u64,
    pub is_shown: bool,
}

/// Where the popover goes and the wrapped text lines it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopoverGeometry {
    pub rectangle: Rect,
    pub lines: Vec<String>,
}

/// Pointer-rest state machine of one hover (the kernel `App::{set,clear,tick}_animation_hover`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HoverState {
    hover: Option<AnimationHover>,
}

impl HoverState {
    pub fn current(&self) -> Option<AnimationHover> {
        self.hover
    }

    /// Records where the pointer rests: `Some(row)` for a row with disabled options, `None`
    /// anywhere else. Resting on the same row keeps the original timestamp so the delay is
    /// measured from first arrival.
    pub fn set(&mut self, row: Option<usize>, now_ms: u64) {
        self.hover = match (row, self.hover) {
            (Some(row), Some(hover)) if hover.row == row => Some(hover),
            (Some(row), _) => Some(AnimationHover {
                row,
                since_ms: now_ms,
                is_shown: false,
            }),
            (None, _) => None,
        };
    }

    /// Dismisses the popover (any key or click, leaving the row).
    pub fn clear(&mut self) {
        self.hover = None;
    }

    /// Reveals the popover after the pointer has rested long enough and drops it when the
    /// Animations tab is no longer on screen. Returns whether the picture changed.
    pub fn tick(&mut self, now_ms: u64, is_on_screen: bool) -> bool {
        let Some(mut hover) = self.hover else {
            return false;
        };
        if !is_on_screen {
            self.hover = None;
            return hover.is_shown;
        }
        if hover.is_shown || now_ms.saturating_sub(hover.since_ms) < HOVER_DELAY_MS {
            return false;
        }
        hover.is_shown = true;
        self.hover = Some(hover);
        true
    }
}

/// The popover title and body for the disabled options of one row.
pub fn popover_text(options: &[DisabledOption]) -> (String, String) {
    match options {
        [] => (String::new(), String::new()),
        [only] => (format!(" {} unavailable ", only.label), only.reason.clone()),
        many => (
            " Unavailable options ".to_owned(),
            many.iter()
                .map(|option| format!("{}: {}", option.label, option.reason))
                .collect::<Vec<_>>()
                .join("\n\n"),
        ),
    }
}

/// Places the popover for the row drawn at `anchor_y`. It goes to the right of the controls
/// `panel` (over the field) when that is wide enough, so it never covers the row; otherwise above
/// or below the row inside the panel columns. The result always lies inside `area` and never
/// includes the last row, which belongs to the voice affordance. `None` when nothing fits.
pub fn popover_geometry(
    area: Rect,
    panel: Rect,
    anchor_y: u16,
    body: &str,
) -> Option<PopoverGeometry> {
    let usable_bottom = if area.height >= 3 {
        area.bottom() - 1
    } else {
        area.bottom()
    };
    let usable_height = usable_bottom.saturating_sub(area.y);
    if usable_height < MINIMUM_HEIGHT || area.width < 4 {
        return None;
    }
    let wrapped = |width: u16| wrap_lines(body, width.saturating_sub(2).max(1));

    // Beside the panel: the row stays fully visible.
    let side_x = panel.right().saturating_add(1);
    let side_room = area.right().saturating_sub(side_x);
    if side_room >= MINIMUM_SIDE_WIDTH {
        let width = side_room.min(MAXIMUM_WIDTH);
        let mut lines = wrapped(width);
        let height = (lines.len() as u16 + 2).min(usable_height);
        lines.truncate(usize::from(height - 2));
        let y = anchor_y
            .min(usable_bottom.saturating_sub(height))
            .max(area.y);
        return Some(PopoverGeometry {
            rectangle: Rect::new(side_x, y, width, height),
            lines,
        });
    }

    // Narrow screens: over the panel, on whichever side of the row has room.
    let width = panel.width.min(area.width).max(4);
    let x = panel.x.max(area.x);
    let space_below = usable_bottom.saturating_sub(anchor_y.saturating_add(1));
    let space_above = anchor_y.saturating_sub(area.y);
    let mut lines = wrapped(width);
    let wanted = lines.len() as u16 + 2;
    let is_below = if wanted <= space_below {
        true
    } else if wanted <= space_above {
        false
    } else {
        space_below >= space_above
    };
    let space = if is_below { space_below } else { space_above };
    if space < MINIMUM_HEIGHT {
        return None;
    }
    let height = wanted.min(space);
    lines.truncate(usize::from(height - 2));
    if lines.len() < wrapped(width).len() {
        if let Some(last) = lines.last_mut() {
            last.push('\u{2026}');
        }
    }
    let y = if is_below {
        anchor_y + 1
    } else {
        anchor_y - height
    };
    Some(PopoverGeometry {
        rectangle: Rect::new(x, y, width.min(area.right() - x), height),
        lines,
    })
}

/// Splits `text` into logical lines, then word-wraps each to `width` columns. Whitespace runs
/// collapse; a word wider than `width` is hard-broken so no row exceeds `width`; an empty logical
/// line still yields one empty row.
pub fn wrap_lines(text: &str, width: u16) -> Vec<String> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    normalized
        .lines()
        .flat_map(|line| wrap_one_line(line, width))
        .collect()
}

fn wrap_one_line(line: &str, width: u16) -> Vec<String> {
    let width = usize::from(width.max(1));
    if line.is_empty() {
        return vec![String::new()];
    }
    let mut rows = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for word in line.split_whitespace() {
        let word_width = UnicodeWidthStr::width(word);
        if word_width > width {
            if !current.is_empty() {
                rows.push(std::mem::take(&mut current));
            }
            let (piece, piece_width) = hard_break_word(word, width, &mut rows);
            current = piece;
            current_width = piece_width;
            continue;
        }
        let gap = usize::from(!current.is_empty());
        if !current.is_empty() && current_width + gap + word_width > width {
            rows.push(std::mem::take(&mut current));
            current_width = 0;
        }
        if !current.is_empty() {
            current.push(' ');
            current_width += 1;
        }
        current.push_str(word);
        current_width += word_width;
    }
    if !current.is_empty() || rows.is_empty() {
        rows.push(current);
    }
    rows
}

/// Breaks one word too wide for any row at character boundaries, pushing full rows into `rows`
/// and returning the final partial row and its width.
fn hard_break_word(word: &str, width: usize, rows: &mut Vec<String>) -> (String, usize) {
    let mut piece = String::new();
    let mut piece_width = 0usize;
    for character in word.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0).max(1);
        if piece_width + character_width > width && !piece.is_empty() {
            rows.push(std::mem::take(&mut piece));
            piece_width = 0;
        }
        piece.push(character);
        piece_width += character_width;
    }
    (piece, piece_width)
}
