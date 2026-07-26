//! Geometry and presentation contracts for the completed-agent close action.
//!
//! `App` decides whether a pane is a completed agent, while this module owns
//! the one-row reservation shared by rendering and pointer hit-testing.

use ratatui::layout::Rect;

use crate::split_layout::PaneViewport;

/// Visible label for the destructive action shown after an agent completes.
pub const CLOSE_COMPLETED_AGENT_LABEL: &str = " ✕ Close finished agent ";

/// The bottom-row action and the terminal region left above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompletedAgentCloseAction {
    pub terminal_area: Rect,
    pub button_area: Rect,
}

/// Reserves the final inner row of one pane viewport for its close action.
///
/// A viewport without an inner cell cannot display a useful action, so it
/// returns `None` instead of manufacturing an invalid zero-height rectangle.
pub fn layout(viewport: PaneViewport) -> Option<CompletedAgentCloseAction> {
    if viewport.content_area.is_empty() {
        return None;
    }

    let button_area = Rect::new(
        viewport.content_area.x,
        viewport.content_area.bottom().saturating_sub(1),
        viewport.content_area.width,
        1,
    );
    let terminal_area = Rect::new(
        viewport.content_area.x,
        viewport.content_area.y,
        viewport.content_area.width,
        viewport.content_area.height.saturating_sub(1),
    );

    Some(CompletedAgentCloseAction {
        terminal_area,
        button_area,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::NodeId;

    #[test]
    fn reserves_the_full_bottom_inner_row_for_the_close_action() {
        let viewport = PaneViewport {
            pane_id: NodeId(7),
            outer_area: Rect::new(10, 4, 40, 12),
            content_area: Rect::new(11, 5, 38, 10),
            slot_index: 0,
        };

        let action = layout(viewport).expect("non-empty pane content has an action row");

        assert_eq!(action.terminal_area, Rect::new(11, 5, 38, 9));
        assert_eq!(action.button_area, Rect::new(11, 14, 38, 1));
    }

    #[test]
    fn omits_the_action_when_the_pane_has_no_inner_cells() {
        let viewport = PaneViewport {
            pane_id: NodeId(7),
            outer_area: Rect::new(10, 4, 2, 2),
            content_area: Rect::new(11, 5, 0, 0),
            slot_index: 0,
        };

        assert_eq!(layout(viewport), None);
    }
}
