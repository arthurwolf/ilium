//! `App` glue for the agent-spend indicators: picks which panes to track,
//! asks the statistics store to keep their transcripts current, and lets the
//! [`crate::cost_tracker::CostTracker`] re-derive the overlay the tree draws.

use std::time::Instant;

use ilium_core::NodeId;

use crate::app::App;
use crate::cost_tracker::TickInput;

impl App {
    /// Periodic maintenance, called from every tick after the statistics
    /// store applied its finished results. Returns whether the overlay was
    /// rebuilt, i.e. whether a redraw is needed.
    pub(crate) fn tick_cost(&mut self, now: Instant) -> bool {
        let agent_panes: Vec<NodeId> = if self.cost_settings.is_any_enabled() {
            let mut panes: Vec<NodeId> = self
                .tree
                .all_ids()
                .filter(|pane_id| self.stats_agent_is_supported(*pane_id))
                .collect();
            panes.sort_unstable();
            panes
        } else {
            Vec::new()
        };

        let busy_workers = self.session_stats.in_flight_count();
        for pane_id in self
            .cost_tracker
            .plan_refreshes(&agent_panes, busy_workers, now)
        {
            if let Some(request) = self.session_stats_request(pane_id) {
                self.session_stats.request_refresh(pane_id, request, now);
            }
        }

        let home = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
        let input = TickInput {
            settings: &self.cost_settings,
            tree: &self.tree,
            tree_version: self.tree_version,
            store: &self.session_stats,
            agent_panes: &agent_panes,
            now,
            now_ms: chrono::Utc::now().timestamp_millis(),
            home: home.as_deref(),
        };
        self.cost_tracker.tick(&input)
    }

    /// The agent whose cost card should be showing and the screen row of its
    /// tree entry. Hover-only visibility follows the pointer; always-visible
    /// keeps the card on the selected agent while nothing else is hovered.
    pub(crate) fn cost_card_target(&self) -> Option<(NodeId, u16)> {
        let overlay = self.cost_tracker.overlay();
        let option = overlay.settings.detail_card;
        if !option.enabled {
            return None;
        }
        let hovered = self
            .hovered_tree_node
            .filter(|hit| overlay.details.contains_key(&hit.id))
            .map(|hit| (hit.id, hit.row));
        if hovered.is_some() || option.visibility == crate::cost_settings::CostVisibility::Hover {
            return hovered;
        }
        let selected = *self.tree_state.selected().last()?;
        if !overlay.details.contains_key(&selected) {
            return None;
        }
        let row = self
            .tree_state
            .rendered_rows()
            .find(|(identifier, _, _)| identifier.last() == Some(&selected))
            .map(|(_, first_row, _)| first_row)?;
        Some((selected, row))
    }

    /// The tree order actually applied: cost ordering when the Agent Cost
    /// tab's sort switch is on, otherwise the User Interface tab's choice.
    pub(crate) fn effective_tree_order(&self) -> crate::config::TreeOrder {
        if self.cost_settings.sort_by_cost {
            crate::config::TreeOrder::CostDescending
        } else {
            self.ui_settings.tree_order
        }
    }
}
