//! Pure right-panel viewport allocation for ordinary panes and split views.
//!
//! Rendering, PTY sizing, editor geometry, and pointer hit-testing all consume
//! these exact rectangles. No caller should independently recreate split
//! geometry.

use ilium_core::{NodeId, SplitOrientation, MAXIMUM_SPLIT_VIEW_PANES};
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};

/// A visual cardinal relationship between two adjacent pane viewports.
///
/// This is deliberately presentation-only: it describes the current split
/// geometry without knowing whether either pane is a terminal or an agent.
/// `screen_transfer` applies the terminal eligibility rule above this layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneDirection {
    Left,
    Right,
    Up,
    Down,
}

impl PaneDirection {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Left => "on the left",
            Self::Right => "on the right",
            Self::Up => "above",
            Self::Down => "below",
        }
    }
}

/// One directed relationship across a physical split separator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdjacentPaneDirection {
    pub source_pane_id: NodeId,
    pub destination_pane_id: NodeId,
    pub direction: PaneDirection,
    /// One terminal cell on the source side of the shared separator.
    pub control_area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneViewport {
    pub pane_id: NodeId,
    pub outer_area: Rect,
    pub content_area: Rect,
    pub slot_index: usize,
    /// The reserved agent-toolbar row, one cell above `content_area`'s
    /// current top edge -- `None` unless `App::pane_viewports` decided this
    /// pane shows its toolbar. This crate stays tree/settings-agnostic (see
    /// the module doc), so that decision and the resulting shrink of
    /// `content_area` both happen in `App`, not here; this field only
    /// carries the result so rendering, PTY sizing, and hit-testing agree.
    pub toolbar_area: Option<Rect>,
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
            toolbar_area: None,
        }
    }

    /// Reserves one row at the top of `content_area` for the agent toolbar,
    /// returning the adjusted viewport. A no-op on an already-empty content
    /// area so a sliver-sized split can't underflow into a huge `Rect`.
    pub fn with_agent_toolbar_reserved(self) -> Self {
        if self.content_area.height == 0 {
            return self;
        }
        let toolbar_area = Rect::new(
            self.content_area.x,
            self.content_area.y,
            self.content_area.width,
            1,
        );
        let content_area = Rect::new(
            self.content_area.x,
            self.content_area.y.saturating_add(1),
            self.content_area.width,
            self.content_area.height.saturating_sub(1),
        );
        Self {
            content_area,
            toolbar_area: Some(toolbar_area),
            ..self
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

/// Finds every ordered, edge-sharing relationship in the visible split.
///
/// A four-pane grid produces eight directed actions: left/right for each row
/// and up/down for each column. Diagonal panes are intentionally excluded;
/// users see controls only on the separator they can actually point at.
pub fn adjacent_pane_directions(viewports: &[PaneViewport]) -> Vec<AdjacentPaneDirection> {
    let mut directions = Vec::new();
    for (left_index, first) in viewports.iter().enumerate() {
        for second in viewports.iter().skip(left_index + 1) {
            if first.outer_area.right() == second.outer_area.x {
                add_horizontal_directions(&mut directions, *first, *second);
            } else if second.outer_area.right() == first.outer_area.x {
                add_horizontal_directions(&mut directions, *second, *first);
            } else if first.outer_area.bottom() == second.outer_area.y {
                add_vertical_directions(&mut directions, *first, *second);
            } else if second.outer_area.bottom() == first.outer_area.y {
                add_vertical_directions(&mut directions, *second, *first);
            }
        }
    }
    directions
}

fn add_horizontal_directions(
    directions: &mut Vec<AdjacentPaneDirection>,
    left: PaneViewport,
    right: PaneViewport,
) {
    let overlap_top = left.outer_area.y.max(right.outer_area.y);
    let overlap_bottom = left.outer_area.bottom().min(right.outer_area.bottom());
    if overlap_top >= overlap_bottom {
        return;
    }
    let center_y = overlap_top + (overlap_bottom - overlap_top) / 2;
    directions.extend([
        AdjacentPaneDirection {
            source_pane_id: left.pane_id,
            destination_pane_id: right.pane_id,
            direction: PaneDirection::Right,
            control_area: Rect::new(left.outer_area.right().saturating_sub(1), center_y, 1, 1),
        },
        AdjacentPaneDirection {
            source_pane_id: right.pane_id,
            destination_pane_id: left.pane_id,
            direction: PaneDirection::Left,
            control_area: Rect::new(right.outer_area.x, center_y, 1, 1),
        },
    ]);
}

fn add_vertical_directions(
    directions: &mut Vec<AdjacentPaneDirection>,
    top: PaneViewport,
    bottom: PaneViewport,
) {
    let overlap_left = top.outer_area.x.max(bottom.outer_area.x);
    let overlap_right = top.outer_area.right().min(bottom.outer_area.right());
    if overlap_left >= overlap_right {
        return;
    }
    let center_x = overlap_left + (overlap_right - overlap_left) / 2;
    directions.extend([
        AdjacentPaneDirection {
            source_pane_id: top.pane_id,
            destination_pane_id: bottom.pane_id,
            direction: PaneDirection::Down,
            control_area: Rect::new(center_x, top.outer_area.bottom().saturating_sub(1), 1, 1),
        },
        AdjacentPaneDirection {
            source_pane_id: bottom.pane_id,
            destination_pane_id: top.pane_id,
            direction: PaneDirection::Up,
            control_area: Rect::new(center_x, bottom.outer_area.y, 1, 1),
        },
    ]);
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

    #[test]
    fn agent_toolbar_reservation_shrinks_content_from_the_top() {
        let viewports =
            allocate_viewports(Rect::new(0, 0, 40, 20), SplitOrientation::Vertical, &ids(1));
        let viewport = viewports[0];
        assert_eq!(viewport.toolbar_area, None);

        let reserved = viewport.with_agent_toolbar_reserved();
        assert_eq!(reserved.outer_area, viewport.outer_area);
        assert_eq!(
            reserved.toolbar_area,
            Some(Rect::new(
                viewport.content_area.x,
                viewport.content_area.y,
                viewport.content_area.width,
                1
            ))
        );
        assert_eq!(
            reserved.content_area,
            Rect::new(
                viewport.content_area.x,
                viewport.content_area.y + 1,
                viewport.content_area.width,
                viewport.content_area.height - 1
            )
        );
    }

    #[test]
    fn agent_toolbar_reservation_is_a_no_op_on_an_empty_content_area() {
        let viewports = allocate_viewports(
            Rect::new(u16::MAX, u16::MAX, 1, 1),
            SplitOrientation::Horizontal,
            &ids(1),
        );
        let reserved = viewports[0].with_agent_toolbar_reserved();
        assert_eq!(reserved.toolbar_area, None);
        assert_eq!(reserved.content_area.height, 0);
    }

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

    #[test]
    fn tiny_areas_saturate_content_geometry_without_panicking() {
        let viewports = allocate_viewports(
            Rect::new(u16::MAX, u16::MAX, 1, 1),
            SplitOrientation::Horizontal,
            &ids(4),
        );

        assert_eq!(viewports.len(), 4);
        assert!(viewports
            .iter()
            .all(|viewport| viewport.content_area.width == 0 && viewport.content_area.height == 0));
    }

    #[test]
    fn adjacent_panes_expose_bidirectional_separator_controls_without_diagonals() {
        let viewports = allocate_viewports(
            Rect::new(0, 0, 100, 40),
            SplitOrientation::Vertical,
            &ids(4),
        );
        let directions = adjacent_pane_directions(&viewports);

        assert_eq!(directions.len(), 8);
        assert!(directions.contains(&AdjacentPaneDirection {
            source_pane_id: NodeId(1),
            destination_pane_id: NodeId(2),
            direction: PaneDirection::Right,
            control_area: Rect::new(49, 10, 1, 1),
        }));
        assert!(directions.contains(&AdjacentPaneDirection {
            source_pane_id: NodeId(4),
            destination_pane_id: NodeId(2),
            direction: PaneDirection::Up,
            control_area: Rect::new(75, 20, 1, 1),
        }));
        assert!(!directions.iter().any(|direction| {
            direction.source_pane_id == NodeId(1) && direction.destination_pane_id == NodeId(4)
        }));
    }
}
