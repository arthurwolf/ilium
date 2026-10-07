//! Shared visual-row mapping for tree and terminal context menus.
//! Action indices remain stable; decoration rows never dispatch commands.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuRow {
    Padding,
    Separator,
    Action(usize),
}

pub(crate) struct MenuLayout {
    pub(crate) rows: Vec<MenuRow>,
    pub(crate) has_rows_above: bool,
    pub(crate) has_rows_below: bool,
    pub(crate) row_offset: usize,
}

impl MenuLayout {
    pub(crate) fn desired_height(groups: &[u8]) -> usize {
        groups.len() + groups.windows(2).filter(|pair| pair[0] != pair[1]).count() + 4
    }

    #[cfg(test)]
    pub(crate) fn new(groups: &[u8], selected_index: usize, height: u16) -> Self {
        Self::with_offset(groups, selected_index, height, 0)
    }

    pub(crate) fn with_offset(
        groups: &[u8],
        selected_index: usize,
        height: u16,
        offset: usize,
    ) -> Self {
        if groups.is_empty() || height == 0 {
            return Self {
                rows: Vec::new(),
                has_rows_above: false,
                has_rows_below: false,
                row_offset: 0,
            };
        }
        let mut rows = Vec::with_capacity(Self::desired_height(groups).saturating_sub(2));
        rows.push(MenuRow::Padding);
        for (index, group) in groups.iter().enumerate() {
            if index > 0 && groups[index - 1] != *group {
                rows.push(MenuRow::Separator);
            }
            rows.push(MenuRow::Action(index));
        }
        rows.push(MenuRow::Padding);
        let selected = selected_index.min(groups.len() - 1);
        let selected_row = rows
            .iter()
            .position(|row| *row == MenuRow::Action(selected))
            .unwrap_or(1);
        let capacity = usize::from(height);
        // Keep visible rows still during hover. Scroll only when selection
        // leaves the current viewport; painting and hit testing share it.
        let mut start = offset.min(rows.len().saturating_sub(capacity));
        if selected_row < start {
            start = selected_row;
        } else if selected_row >= start.saturating_add(capacity) {
            start = selected_row.saturating_add(1).saturating_sub(capacity);
        }
        let end = start.saturating_add(capacity).min(rows.len());
        let has_rows_above = rows[..start]
            .iter()
            .any(|row| matches!(row, MenuRow::Action(_)));
        let has_rows_below = rows[end..]
            .iter()
            .any(|row| matches!(row, MenuRow::Action(_)));
        rows.truncate(end);
        rows.drain(..start);
        Self {
            rows,
            has_rows_above,
            has_rows_below,
            row_offset: start,
        }
    }

    pub(crate) fn action_at(&self, row: u16) -> Option<usize> {
        match self.rows.get(usize::from(row))? {
            MenuRow::Action(index) => Some(*index),
            MenuRow::Padding | MenuRow::Separator => None,
        }
    }

    pub(crate) fn row_for_action(&self, index: usize) -> Option<u16> {
        self.rows
            .iter()
            .position(|row| *row == MenuRow::Action(index))
            .and_then(|row| u16::try_from(row).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_and_group_boundaries_never_activate_an_action() {
        let layout = MenuLayout::new(&[0, 0, 1, 2], 0, 9);
        assert_eq!(
            layout.rows,
            vec![
                MenuRow::Padding,
                MenuRow::Action(0),
                MenuRow::Action(1),
                MenuRow::Separator,
                MenuRow::Action(2),
                MenuRow::Separator,
                MenuRow::Action(3),
                MenuRow::Padding
            ]
        );
        for row in [0, 3, 5, 7, 8] {
            assert_eq!(layout.action_at(row), None);
        }
        assert_eq!(MenuLayout::desired_height(&[0, 0, 1, 2]), 10);
    }

    #[test]
    fn every_action_remains_visible_and_clickable_on_a_short_menu() {
        let groups = [0, 0, 1, 1, 2, 3, 3, 4];
        for height in 1..6 {
            for selected in 0..groups.len() {
                let layout = MenuLayout::new(&groups, selected, height);
                let row = layout
                    .row_for_action(selected)
                    .expect("selected command must remain visible");
                assert_eq!(layout.action_at(row), Some(selected));
                assert!(layout.rows.len() <= usize::from(height));
            }
        }
        let last = MenuLayout::new(&groups, 7, 4);
        assert!(last.has_rows_above);
        assert!(!last.has_rows_below);
    }

    #[test]
    fn empty_and_zero_height_menus_have_no_selectable_rows() {
        assert!(MenuLayout::new(&[], 0, 10).rows.is_empty());
        assert!(MenuLayout::new(&[0], 0, 0).rows.is_empty());
    }

    #[test]
    fn hovering_a_visible_action_does_not_move_the_viewport() {
        let groups = [0, 0, 1, 1, 2, 2, 3, 3];
        let scrolled = MenuLayout::new(&groups, 5, 5);
        for row in &scrolled.rows {
            let MenuRow::Action(index) = row else {
                continue;
            };
            let hovered = MenuLayout::with_offset(&groups, *index, 5, scrolled.row_offset);
            assert_eq!(hovered.rows, scrolled.rows);
            assert_eq!(hovered.row_offset, scrolled.row_offset);
        }
    }
}
