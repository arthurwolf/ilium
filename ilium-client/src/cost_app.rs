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
        // One extra ID makes overload explicit; never publish a misleading
        // partial peer set. This stack capture needs no unbounded UI Vec clone.
        let mut pane_ids = [NodeId(0); crate::cost_tracker::MAX_PANES + 1];
        let mut pane_count = 0;
        if self.cost_settings.is_any_enabled() {
            for pane_id in self.tree.all_ids() {
                if self.stats_agent_is_supported(pane_id) {
                    pane_ids[pane_count] = pane_id;
                    pane_count += 1;
                    if pane_count == pane_ids.len() {
                        break;
                    }
                }
            }
        }
        let agent_panes = &mut pane_ids[..pane_count];
        agent_panes.sort_unstable();

        let busy_workers = self.session_stats.in_flight_count();
        for pane_id in self
            .cost_tracker
            .plan_refreshes(agent_panes, busy_workers, now)
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
            agent_panes,
            now,
            now_ms: chrono::Utc::now().timestamp_millis(),
            home: home.as_deref(),
        };
        let changed = self.cost_tracker.tick(&input);
        let diagnostic = self
            .session_stats
            .diagnostic()
            .or_else(|| self.cost_tracker.history_error())
            .map(str::to_owned);
        if diagnostic != self.statistics_diagnostic {
            if let Some(error) = &diagnostic {
                self.status_message = Some(error.clone());
            }
            self.statistics_diagnostic = diagnostic;
            return true;
        }
        changed
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
