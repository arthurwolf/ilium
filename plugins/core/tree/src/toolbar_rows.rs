//! Row rules of the tree toolbar editor (pure part of `ilium-client/src/tree_toolbar_settings.rs`).
//!
//! The editor lists one row per toolbar button, then an "Add button" row, then the "Inactive
//! after" row. This module holds the selection, scrolling, stepping and column rules; drawing and
//! the live `App` state stay in the kernel until settings rows can be edited by plugins.

/// Smallest and largest inactivity threshold, in minutes.
pub const MINIMUM_INACTIVITY_MINUTES: u32 = 1;
/// Largest inactivity threshold, in minutes.
pub const MAXIMUM_INACTIVITY_MINUTES: u32 = 1440;
/// Width from which the editor shows the live preview beside the list.
pub const WIDE_COLUMNS: u16 = 70;

/// A rectangle in terminal cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    /// Left column.
    pub x: u16,
    /// Top row.
    pub y: u16,
    /// Width in cells.
    pub width: u16,
    /// Height in cells.
    pub height: u16,
}

/// Rows of the editor for `entries` buttons: the buttons, "Add button" and "Inactive after".
pub const fn row_count(entries: usize) -> usize {
    entries + 2
}

/// Splits the editor area into the list and the preview: side by side from 70 columns, stacked
/// (preview in the last three rows) below that.
pub fn columns(area: Area) -> (Area, Area) {
    if area.width >= WIDE_COLUMNS {
        let width = area.width / 2;
        (
            Area { width, ..area },
            Area {
                x: area.x + width + 1,
                width: area.width - width - 1,
                ..area
            },
        )
    } else {
        (
            Area {
                height: area.height.saturating_sub(3),
                ..area
            },
            Area {
                y: area.y.saturating_add(area.height.saturating_sub(3)),
                height: area.height.min(3),
                ..area
            },
        )
    }
}

/// The first visible row so that the selected row stays on screen.
pub fn first_visible(selected_row: usize, height: usize) -> usize {
    selected_row.saturating_add(1).saturating_sub(height.max(1))
}

/// The row after an Up or Down key; the last row is the inactivity row at `entries + 1`.
pub fn next_selection(selected_row: usize, up: bool, entries: usize) -> usize {
    if up {
        selected_row.saturating_sub(1)
    } else {
        (selected_row + 1).min(entries + 1)
    }
}

/// Whether Shift+Up/Down moves a button: both rows must be buttons.
pub fn reorders(selected_row: usize, next: usize, entries: usize) -> bool {
    selected_row < entries && next < entries
}

/// The inactivity threshold after Left (`increase == false`) or Right, kept within 1 to 1440.
pub fn step_inactivity(minutes: u32, increase: bool) -> u32 {
    if increase {
        minutes.saturating_add(1).min(MAXIMUM_INACTIVITY_MINUTES)
    } else {
        minutes.saturating_sub(1).max(MINIMUM_INACTIVITY_MINUTES)
    }
}

/// The selection of the "Add button" picker after Up or Down, within `available` choices.
pub fn next_picker(selected: usize, up: bool, available: usize) -> usize {
    if up {
        selected.saturating_sub(1)
    } else {
        (selected + 1).min(available.saturating_sub(1))
    }
}

/// The selected row after the buttons changed: it never points past the "Add button" row.
pub fn clamp_selection(selected_row: usize, entries: usize) -> usize {
    selected_row.min(entries + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Area = Area {
        x: 2,
        y: 4,
        width: 100,
        height: 20,
    };

    #[test]
    fn wide_areas_put_the_preview_next_to_the_list() {
        let (list, preview) = columns(AREA);
        assert_eq!((list.x, list.width), (2, 50));
        assert_eq!((preview.x, preview.width), (53, 49));
        assert_eq!(list.height, 20);
    }

    #[test]
    fn narrow_areas_stack_the_preview_in_the_last_three_rows() {
        let (list, preview) = columns(Area { width: 40, ..AREA });
        assert_eq!(list.height, 17);
        assert_eq!((preview.y, preview.height), (21, 3));
        assert_eq!(preview.width, 40);
    }

    #[test]
    fn the_selection_scrolls_into_view() {
        assert_eq!(first_visible(0, 5), 0);
        assert_eq!(first_visible(4, 5), 0);
        assert_eq!(first_visible(5, 5), 1);
        assert_eq!(first_visible(3, 0), 3);
    }

    #[test]
    fn selection_moves_within_the_buttons_add_and_inactivity_rows() {
        assert_eq!(next_selection(0, true, 3), 0);
        assert_eq!(next_selection(3, false, 3), 4);
        assert_eq!(next_selection(4, false, 3), 4);
        assert!(reorders(1, 2, 3));
        assert!(!reorders(2, 3, 3));
        assert_eq!(row_count(3), 5);
        assert_eq!(clamp_selection(9, 3), 4);
    }

    #[test]
    fn inactivity_minutes_stay_between_one_and_1440() {
        assert_eq!(step_inactivity(1, false), 1);
        assert_eq!(step_inactivity(10, false), 9);
        assert_eq!(step_inactivity(1440, true), 1440);
        assert_eq!(step_inactivity(10, true), 11);
    }

    #[test]
    fn the_picker_stays_within_the_available_entries() {
        assert_eq!(next_picker(0, true, 4), 0);
        assert_eq!(next_picker(3, false, 4), 3);
        assert_eq!(next_picker(0, false, 0), 0);
    }
}
