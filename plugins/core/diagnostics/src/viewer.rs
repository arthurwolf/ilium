//! The agent debug log viewer of the client side: the popup that shows the retained history of one agent
//! pane. The kernel fetches the snapshot from the server and hands it over as the arguments of the
//! `show-debug-log` command (`{pane, name, dropped, entries}`); this module owns everything after that:
//! the cache, the panel-resize filter, scrolling, the text of every row and copying the report.
//!
//! Keys: Up/Down and PageUp/PageDown scroll, Home/End jump to the oldest/newest entry, `r` reveals or hides
//! the left-panel animation resizes, `c` copies the report to the clipboard, Esc or `q` closes.

use serde::Deserialize;

use crate::export::render_report;
use crate::report::{AgentDebugEntry, PaneDebugLog};
use crate::view::{
    context_summary, event_identity, format_timestamp, sanitize_single_line_text,
    AgentDebugLogCache, AgentDebugLogFilter,
};

/// The id of the command and of the popup it opens.
pub const SHOW_COMMAND: &str = "show-debug-log";

/// Rows kept for the frame and the key hint inside the popup.
const CHROME_ROWS: usize = 4;

/// The arguments of `show-debug-log`.
#[derive(Debug, Deserialize)]
pub struct ShowArguments {
    pub pane: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub dropped: u64,
    #[serde(default)]
    pub entries: Vec<AgentDebugEntry>,
}

/// What a key asks the plugin to do besides updating the viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyOutcome {
    /// The key is not the viewer's.
    Ignored,
    /// The viewer changed and must be drawn again.
    Redraw,
    /// Close the popup.
    Close,
    /// Put this report on the clipboard.
    Copy(String),
}

/// One open viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugViewer {
    pub pane: u64,
    pub name: String,
    pub cache: AgentDebugLogCache,
    pub filter: AgentDebugLogFilter,
    /// Entries scrolled up from the newest one; 0 shows the newest entries.
    pub scroll_from_newest: usize,
}

impl DebugViewer {
    /// Opens the viewer on the newest entries from the command arguments.
    pub fn from_arguments(arguments: ShowArguments) -> Self {
        let next_sequence = arguments
            .entries
            .last()
            .map_or(1, |entry| entry.sequence.saturating_add(1));
        let through_sequence = next_sequence.saturating_sub(1);
        let log = PaneDebugLog {
            entries: arguments.entries,
            next_sequence,
            dropped_entry_count: arguments.dropped,
            ..PaneDebugLog::default()
        };
        Self {
            pane: arguments.pane,
            name: arguments.name,
            cache: AgentDebugLogCache {
                log,
                through_sequence,
                is_loading: false,
                has_loaded_retained_history: true,
            },
            filter: AgentDebugLogFilter::default(),
            scroll_from_newest: 0,
        }
    }

    /// The entries the filter lets through, oldest first.
    pub fn visible_entries(&self) -> Vec<&AgentDebugEntry> {
        self.cache
            .log
            .entries
            .iter()
            .filter(|entry| self.filter.includes(entry))
            .collect()
    }

    /// The title of the popup frame.
    pub fn title(&self) -> String {
        format!("Agent debug log: {}", sanitize_single_line_text(&self.name))
    }

    /// The one-line summary under the entries: counts, filter state and discarded history.
    pub fn status_line(&self) -> String {
        let visible = self.filter.visible_entry_count(&self.cache);
        let hidden = self.filter.hidden_entry_count(&self.cache);
        let mut line = format!("{visible} events");
        if self.filter.is_tree_panel_animation_resize_hidden {
            line.push_str(&format!(" · panel resizes hidden ({hidden})"));
        } else {
            line.push_str(" · panel resizes shown");
        }
        if self.cache.log.dropped_entry_count > 0 {
            line.push_str(&format!(
                " · {} older discarded",
                self.cache.log.dropped_entry_count
            ));
        }
        line
    }

    /// The key hint row.
    pub fn hint_line() -> &'static str {
        "Up/Down scroll   PgUp/PgDn page   Home/End ends   r panel resizes   c copy report   Esc close"
    }

    /// The text rows of the entries that fit `rows` rows, newest at the bottom.
    pub fn window(&self, rows: usize) -> Vec<String> {
        let visible = self.visible_entries();
        let rows = rows.max(1);
        let end = visible.len().saturating_sub(self.scroll_from_newest);
        let start = end.saturating_sub(rows);
        visible[start..end]
            .iter()
            .map(|entry| entry_row(entry))
            .collect()
    }

    /// How many entry rows a popup of `popup_rows` rows has room for.
    pub fn entry_rows(popup_rows: usize) -> usize {
        popup_rows.saturating_sub(CHROME_ROWS).max(1)
    }

    /// The full report text of the current filter, as the clipboard receives it.
    pub fn report(&self, now_unix_millis: i64) -> String {
        render_report(
            &self.name,
            self.pane,
            &self.cache,
            self.filter,
            now_unix_millis,
        )
    }

    /// Applies one key. `page` is the number of entry rows of a page.
    pub fn press(&mut self, chord: &str, page: usize, now_unix_millis: i64) -> KeyOutcome {
        let last = self.visible_entries().len().saturating_sub(1);
        match chord {
            "esc" | "escape" | "q" => KeyOutcome::Close,
            "up" | "k" => self.scroll_up(1, last),
            "down" | "j" => self.scroll_down(1),
            "pageup" => self.scroll_up(page.max(1), last),
            "pagedown" => self.scroll_down(page.max(1)),
            "home" => self.scroll_up(usize::MAX, last),
            "end" => self.scroll_down(usize::MAX),
            "r" => {
                self.filter.is_tree_panel_animation_resize_hidden =
                    !self.filter.is_tree_panel_animation_resize_hidden;
                self.scroll_from_newest = 0;
                KeyOutcome::Redraw
            }
            "c" => KeyOutcome::Copy(self.report(now_unix_millis)),
            _ => KeyOutcome::Ignored,
        }
    }

    fn scroll_up(&mut self, amount: usize, last: usize) -> KeyOutcome {
        self.scroll_from_newest = self.scroll_from_newest.saturating_add(amount).min(last);
        KeyOutcome::Redraw
    }

    fn scroll_down(&mut self, amount: usize) -> KeyOutcome {
        self.scroll_from_newest = self.scroll_from_newest.saturating_sub(amount);
        KeyOutcome::Redraw
    }
}

/// One entry as one row: time, icon, category, summary and the agent identity when it is known.
fn entry_row(entry: &AgentDebugEntry) -> String {
    let (icon, category) = event_identity(&entry.kind);
    let mut row = format!(
        "{} {icon} {category:<10} {}",
        format_timestamp(entry.occurred_at_unix_millis),
        sanitize_single_line_text(&entry.summary)
    );
    if let Some(context) = context_summary(entry) {
        row.push_str(&format!("  ({context})"));
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{
        AgentDebugContext, AgentDebugEventKind, AgentDebugEventMetadata, AgentDebugSeverity,
        AgentDebugSource, PaneResizeCause,
    };

    fn entry(
        sequence: u64,
        kind: AgentDebugEventKind,
        cause: Option<PaneResizeCause>,
    ) -> AgentDebugEntry {
        AgentDebugEntry {
            sequence,
            occurred_at_unix_millis: 1_700_000_000_000 + sequence as i64,
            severity: AgentDebugSeverity::Information,
            source: AgentDebugSource::Detector,
            kind,
            summary: format!("event {sequence}"),
            fields: Vec::new(),
            correlation_id: None,
            context: AgentDebugContext::default(),
            metadata: AgentDebugEventMetadata {
                pane_resize_cause: cause,
            },
        }
    }

    fn viewer(count: u64) -> DebugViewer {
        let mut entries: Vec<_> = (1..=count)
            .map(|sequence| entry(sequence, AgentDebugEventKind::DetectionCycle, None))
            .collect();
        entries.push(entry(
            count + 1,
            AgentDebugEventKind::PaneResized,
            Some(PaneResizeCause::TreePanelAnimation),
        ));
        DebugViewer::from_arguments(ShowArguments {
            pane: 9,
            name: "Codex".to_string(),
            dropped: 3,
            entries,
        })
    }

    /// REQ: F-032
    #[test]
    fn the_viewer_opens_on_the_newest_entries_with_animation_resizes_hidden() {
        let viewer = viewer(10);

        let rows = viewer.window(3);

        assert_eq!(rows.len(), 3);
        assert!(rows[2].contains("event 10"), "{rows:?}");
        assert!(viewer.status_line().contains("panel resizes hidden (1)"));
        assert!(viewer.status_line().contains("3 older discarded"));
    }

    /// REQ: F-032
    #[test]
    fn scrolling_stops_at_the_oldest_and_newest_entries() {
        let mut viewer = viewer(10);

        assert_eq!(viewer.press("home", 3, 0), KeyOutcome::Redraw);
        assert!(viewer.window(3)[0].contains("event 1"));
        viewer.press("up", 3, 0);
        assert_eq!(viewer.scroll_from_newest, 9);
        viewer.press("end", 3, 0);
        assert_eq!(viewer.scroll_from_newest, 0);
        viewer.press("pageup", 3, 0);
        assert_eq!(viewer.scroll_from_newest, 3);
    }

    /// REQ: F-032
    #[test]
    fn the_r_key_reveals_and_hides_the_panel_animation_resizes() {
        let mut viewer = viewer(2);
        assert_eq!(viewer.visible_entries().len(), 2);

        viewer.press("r", 3, 0);

        assert_eq!(viewer.visible_entries().len(), 3);
        assert!(viewer.status_line().contains("panel resizes shown"));
        viewer.press("r", 3, 0);
        assert_eq!(viewer.visible_entries().len(), 2);
    }

    /// REQ: F-032
    #[test]
    fn the_c_key_copies_the_report_of_the_current_filter() {
        let mut viewer = viewer(2);

        let KeyOutcome::Copy(report) = viewer.press("c", 3, 1_700_000_001_000) else {
            panic!("expected a copy outcome");
        };

        assert!(report.starts_with("ILIUM AGENT DEBUG LOG\n"));
        assert!(report.contains("Pane: Codex"));
        assert!(report.contains("Exported events: 2"));
    }

    /// REQ: F-032
    #[test]
    fn escape_and_q_close_and_other_keys_are_ignored() {
        let mut viewer = viewer(1);

        assert_eq!(viewer.press("esc", 3, 0), KeyOutcome::Close);
        assert_eq!(viewer.press("q", 3, 0), KeyOutcome::Close);
        assert_eq!(viewer.press("x", 3, 0), KeyOutcome::Ignored);
    }

    /// REQ: F-032
    #[test]
    fn an_empty_history_still_renders_without_rows_and_never_panics_on_scroll() {
        let mut viewer = DebugViewer::from_arguments(ShowArguments {
            pane: 1,
            name: String::new(),
            dropped: 0,
            entries: Vec::new(),
        });

        assert!(viewer.window(5).is_empty());
        viewer.press("up", 3, 0);
        viewer.press("pageup", 3, 0);
        assert_eq!(viewer.scroll_from_newest, 0);
    }

    /// REQ: F-032
    #[test]
    fn arguments_from_the_kernel_deserialize_into_entries() {
        let json = serde_json::json!({
            "pane": 4, "name": "agent", "dropped": 2,
            "entries": [serde_json::to_value(entry(1, AgentDebugEventKind::DetectionCycle, None)).unwrap()]
        });

        let arguments: ShowArguments = serde_json::from_value(json).unwrap();

        assert_eq!(arguments.pane, 4);
        assert_eq!(arguments.entries.len(), 1);
    }
}
