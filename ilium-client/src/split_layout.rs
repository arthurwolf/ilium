//! Pure right-panel viewport allocation for ordinary panes and split views.
//!
//! Rendering, PTY sizing, editor geometry, and pointer hit-testing all consume
//! these exact rectangles. No caller should independently recreate split
//! geometry.

use ilium_core::{NodeId, SplitOrientation, MAXIMUM_SPLIT_VIEW_PANES};
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneViewport {
    pub pane_id: NodeId,
    pub outer_area: Rect,
    pub content_area: Rect,
    pub slot_index: usize,
}

impl PaneViewport {
    fn new(pane_id: NodeId, outer_area: Rect, slot_index: usize) -> Self {
        let content_area = Rect::new(
            outer_area.x.saturating_add(1),
            outer_area.y.saturating_add(1),
            outer_area.width.saturating_sub(2),
            outer_area.height.saturating_sub(2),
        );
        Self {
            pane_id,
            outer_area,
            content_area,
            slot_index,
        }
    }
}

pub fn allocate_viewports(
    area: Rect,
    orientation: SplitOrientation,
    pane_ids: &[NodeId],
) -> Vec<PaneViewport> {
    let pane_ids = &pane_ids[..pane_ids.len().min(MAXIMUM_SPLIT_VIEW_PANES)];
    let areas = match pane_ids.len() {
        0 => Vec::new(),
        1 => vec![area],
        2 | 3 => equal_areas(area, orientation, pane_ids.len()),
        _ => grid_areas(area),
    };
    pane_ids
        .iter()
        .zip(areas)
        .enumerate()
        .map(|(slot_index, (pane_id, outer_area))| {
            PaneViewport::new(*pane_id, outer_area, slot_index)
        })
        .collect()
}

pub fn viewport_at(viewports: &[PaneViewport], position: Position) -> Option<PaneViewport> {
    viewports
        .iter()
        .copied()
        .find(|viewport| viewport.outer_area.contains(position))
}

fn equal_areas(area: Rect, orientation: SplitOrientation, count: usize) -> Vec<Rect> {
    let direction = match orientation {
        SplitOrientation::Vertical => Direction::Horizontal,
        SplitOrientation::Horizontal => Direction::Vertical,
    };
    Layout::default()
        .direction(direction)
        .constraints(vec![Constraint::Ratio(1, count as u32); count])
        .split(area)
        .to_vec()
}

fn grid_areas(area: Rect) -> Vec<Rect> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .split(area)
        .iter()
        .flat_map(|row| {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
                .split(*row)
                .to_vec()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(count: u64) -> Vec<NodeId> {
        (1..=count).map(NodeId).collect()
    }

    #[test]
    fn empty_and_single_pane_allocations_are_not_split() {
        let area = Rect::new(10, 2, 90, 30);
        assert!(allocate_viewports(area, SplitOrientation::Vertical, &[]).is_empty());
        let viewports = allocate_viewports(area, SplitOrientation::Horizontal, &ids(1));
        assert_eq!(viewports[0].outer_area, area);
        assert_eq!(viewports[0].content_area, Rect::new(11, 3, 88, 28));
    }

    #[test]
    fn two_and_three_panes_follow_the_configured_orientation() {
        let area = Rect::new(0, 0, 90, 30);
        let vertical = allocate_viewports(area, SplitOrientation::Vertical, &ids(3));
        assert_eq!(vertical[0].outer_area, Rect::new(0, 0, 30, 30));
        assert_eq!(vertical[2].outer_area, Rect::new(60, 0, 30, 30));

        let horizontal = allocate_viewports(area, SplitOrientation::Horizontal, &ids(2));
        assert_eq!(horizontal[0].outer_area, Rect::new(0, 0, 90, 15));
        assert_eq!(horizontal[1].outer_area, Rect::new(0, 15, 90, 15));
    }

    #[test]
    fn four_panes_use_a_two_by_two_grid_for_both_orientations() {
        let area = Rect::new(0, 0, 100, 40);
        for orientation in [SplitOrientation::Vertical, SplitOrientation::Horizontal] {
            let viewports = allocate_viewports(area, orientation, &ids(4));
            assert_eq!(viewports[0].outer_area, Rect::new(0, 0, 50, 20));
            assert_eq!(viewports[3].outer_area, Rect::new(50, 20, 50, 20));
        }
    }

    #[test]
    fn viewport_hit_testing_uses_the_allocated_rectangles() {
        let viewports = allocate_viewports(
            Rect::new(10, 5, 80, 20),
            SplitOrientation::Vertical,
            &ids(2),
        );
        assert_eq!(
            viewport_at(&viewports, Position::new(70, 10)).map(|viewport| viewport.pane_id),
            Some(NodeId(2))
        );
    }
}
