//! Presentation helpers of the agent-debug viewer and the report as pure functions: the default filter, the
//! cached journal mirror, timestamps, event categories, labels and text sanitizers. The host renders the text
//! these produce; no terminal style lives here.

use chrono::DateTime;

use crate::report::{
    AgentActivity, AgentClass, AgentDebugEntry, AgentDebugEventKind, AgentDebugSource, PaneDebugLog,
};

/// Ephemeral viewer/export policy. The retained server journal stays complete; operators can reveal animation
/// evidence without restarting or recapturing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentDebugLogFilter {
    pub is_tree_panel_animation_resize_hidden: bool,
}

impl Default for AgentDebugLogFilter {
    fn default() -> Self {
        Self {
            is_tree_panel_animation_resize_hidden: true,
        }
    }
}

impl AgentDebugLogFilter {
    pub fn includes(self, entry: &AgentDebugEntry) -> bool {
        !self.is_tree_panel_animation_resize_hidden || !entry.is_tree_panel_animation_resize()
    }

    pub fn visible_entry_count(self, cache: &AgentDebugLogCache) -> usize {
        cache
            .log
            .entries
            .iter()
            .filter(|entry| self.includes(entry))
            .count()
    }

    pub fn hidden_entry_count(self, cache: &AgentDebugLogCache) -> usize {
        cache
            .log
            .entries
            .len()
            .saturating_sub(self.visible_entry_count(cache))
    }
}

/// Bounded, sequence-ordered mirror of one server-owned pane journal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentDebugLogCache {
    pub log: PaneDebugLog,
    pub through_sequence: u64,
    pub is_loading: bool,
    pub has_loaded_retained_history: bool,
}

/// Local-looking timestamp in European date order, `YYYY-MM-DD HH:MM:SS.mmm`. The plugin has no time zone
/// source, so the time is UTC.
pub fn format_timestamp(unix_millis: i64) -> String {
    DateTime::from_timestamp_millis(unix_millis).map_or_else(
        || "0000-00-00 00:00:00.000".to_string(),
        |timestamp| timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
    )
}

/// Icon and category label of an event kind.
pub fn event_identity(kind: &AgentDebugEventKind) -> (&'static str, &'static str) {
    match kind {
        AgentDebugEventKind::DetectionCycle => ("⌁", "DETECTION"),
        AgentDebugEventKind::AgentDetected
        | AgentDebugEventKind::AgentLost
        | AgentDebugEventKind::AgentChanged
        | AgentDebugEventKind::ActivityChanged => ("◉", "AGENT"),
        AgentDebugEventKind::GoalDetected | AgentDebugEventKind::GoalCleared => ("⚑", "GOAL"),
        AgentDebugEventKind::ExecutionStarted
        | AgentDebugEventKind::ExecutionFinished
        | AgentDebugEventKind::ApprovalRequested
        | AgentDebugEventKind::BackgroundWait => ("⚙", "EXECUTION"),
        AgentDebugEventKind::SessionDiscovery
        | AgentDebugEventKind::SessionResolved
        | AgentDebugEventKind::SessionCleared
        | AgentDebugEventKind::ConversationCleared => ("⌘", "SESSION"),
        AgentDebugEventKind::PromptSubmitted
        | AgentDebugEventKind::ScheduledInputCreated
        | AgentDebugEventKind::ScheduledInputReplaced
        | AgentDebugEventKind::ScheduledInputDelivered
        | AgentDebugEventKind::ScheduledInputCleared
        | AgentDebugEventKind::PromptQueued
        | AgentDebugEventKind::QueuedPromptDelivered
        | AgentDebugEventKind::PromptQueueCleared
        | AgentDebugEventKind::PtyInputWritten => ("➜", "INPUT"),
        AgentDebugEventKind::TitleInferenceRequested
        | AgentDebugEventKind::TitleInferenceSucceeded
        | AgentDebugEventKind::TitleInferenceFailed
        | AgentDebugEventKind::TitleInferenceDiscarded
        | AgentDebugEventKind::TitleApplied => ("✦", "LLM TITLE"),
        AgentDebugEventKind::TriggerEvaluated
        | AgentDebugEventKind::TriggerActionStarted
        | AgentDebugEventKind::TriggerActionFinished => ("⌁", "TRIGGER"),
        AgentDebugEventKind::PaneCreated
        | AgentDebugEventKind::PaneRestored
        | AgentDebugEventKind::PaneFocused
        | AgentDebugEventKind::PaneResized
        | AgentDebugEventKind::PaneClosed
        | AgentDebugEventKind::PtyOutputClosed => ("▣", "PANE"),
        AgentDebugEventKind::RecordingEnabled
        | AgentDebugEventKind::RecordingDisabled
        | AgentDebugEventKind::PersistenceSaved
        | AgentDebugEventKind::RetentionNotice => ("◆", "JOURNAL"),
        AgentDebugEventKind::VoiceAction => ("◍", "VOICE"),
        AgentDebugEventKind::IpcRequest => ("⇄", "IPC"),
        AgentDebugEventKind::Error => ("⚠", "ERROR"),
        AgentDebugEventKind::Custom(_) => ("•", "EVENT"),
    }
}

pub fn source_label(source: AgentDebugSource) -> &'static str {
    match source {
        AgentDebugSource::Server => "Detached session server",
        AgentDebugSource::Client => "Attached TUI client",
        AgentDebugSource::Detector => "Agent detection engine",
        AgentDebugSource::SessionDiscovery => "Session identity verifier",
        AgentDebugSource::Pty => "Terminal input/output boundary",
        AgentDebugSource::Persistence => "Session snapshot persistence",
        AgentDebugSource::Inference => "LLM inference worker",
        AgentDebugSource::Voice => "Voice control",
    }
}

pub fn agent_class_label(class: &AgentClass) -> &str {
    match class {
        AgentClass::Claude => "Claude Code",
        AgentClass::Codex => "Codex",
        AgentClass::Antigravity => "Antigravity",
        AgentClass::Other(name) => name,
    }
}

pub const fn activity_label(activity: AgentActivity) -> &'static str {
    match activity {
        AgentActivity::Working => "working",
        AgentActivity::WaitingApproval => "waiting for approval",
        AgentActivity::WaitingBackground => "waiting for background work",
        AgentActivity::BackgroundTaskStillRunning => "a background task is still finishing up",
        AgentActivity::Idle => "idle",
        AgentActivity::Done => "done",
    }
}

/// The one-line identity of an entry: agent class, activity, process, session and conversation revision.
pub fn context_summary(entry: &AgentDebugEntry) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(class) = entry.context.class.as_ref() {
        // `AgentClass::Other` carries a detector-provided name, which is untrusted text.
        parts.push(sanitize_single_line_text(agent_class_label(class)));
    }
    if let Some(activity) = entry.context.activity {
        parts.push(activity_label(activity).to_string());
    }
    if let Some(process_id) = entry.context.process_id {
        parts.push(format!("process {process_id}"));
    }
    if let Some(session_id) = entry.context.session_id.as_deref() {
        parts.push(format!("session {}", sanitize_single_line_text(session_id)));
    }
    if entry.context.title_generation > 0 {
        parts.push(format!(
            "conversation revision {}",
            entry.context.title_generation
        ));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// Single-row variant for text that must stay on one line: a newline or tab collapses to a space, every other
/// control character is rendered inert.
pub fn sanitize_single_line_text(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\n' | '\t' => ' ',
            character if character.is_control() => '\u{fffd}',
            character => character,
        })
        .collect()
}

/// Multi-line text: keeps newlines and tabs, replaces every other control character so a terminal escape
/// sequence from captured screen content cannot re-execute.
pub fn sanitize_terminal_text(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character == '\n' || character == '\t' || !character.is_control() {
                character
            } else {
                '\u{fffd}'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{
        AgentDebugContext, AgentDebugEventMetadata, AgentDebugSeverity, PaneResizeCause,
    };

    fn resize_entry(
        sequence: u64,
        summary: &str,
        pane_resize_cause: Option<PaneResizeCause>,
    ) -> AgentDebugEntry {
        AgentDebugEntry {
            sequence,
            occurred_at_unix_millis: 1_700_000_000_000,
            severity: AgentDebugSeverity::Information,
            source: AgentDebugSource::Pty,
            kind: AgentDebugEventKind::PaneResized,
            summary: summary.to_string(),
            fields: Vec::new(),
            correlation_id: None,
            context: AgentDebugContext::default(),
            metadata: AgentDebugEventMetadata { pane_resize_cause },
        }
    }

    /// REQ: PORT:ilium-client/src/agent_debug_ui.rs::terminal_control_sequences_are_rendered_inert
    #[test]
    fn terminal_control_sequences_are_rendered_inert() {
        assert_eq!(
            sanitize_terminal_text("safe\u{1b}[31m red"),
            "safe\u{fffd}[31m red"
        );
    }

    /// REQ: PORT:ilium-client/src/agent_debug_ui.rs::single_line_sanitizer_collapses_line_breaks_and_tabs
    #[test]
    fn single_line_sanitizer_collapses_line_breaks_and_tabs() {
        assert_eq!(
            sanitize_single_line_text("multi\nline\tsummary\u{1b}[31m"),
            "multi line summary\u{fffd}[31m"
        );
    }

    /// REQ: PORT:ilium-client/src/agent_debug_ui.rs::timestamps_use_european_date_order
    #[test]
    fn timestamps_use_european_date_order() {
        assert!(format_timestamp(1_700_000_000_000).starts_with("2023-11-"));
        assert_eq!(
            format_timestamp(1_700_000_000_123),
            "2023-11-14 22:13:20.123"
        );
    }

    /// REQ: PORT:ilium-client/src/agent_debug_ui.rs::default_filter_hides_only_typed_tree_panel_animation_resizes
    #[test]
    fn default_filter_hides_only_typed_tree_panel_animation_resizes() {
        let cache = AgentDebugLogCache {
            log: PaneDebugLog {
                entries: vec![
                    resize_entry(
                        1,
                        "tree animation resize",
                        Some(PaneResizeCause::TreePanelAnimation),
                    ),
                    resize_entry(
                        2,
                        "host terminal resize",
                        Some(PaneResizeCause::HostTerminal),
                    ),
                    resize_entry(3, "legacy resize", None),
                ],
                ..Default::default()
            },
            ..AgentDebugLogCache::default()
        };
        let default_filter = AgentDebugLogFilter::default();
        let shown: Vec<&str> = cache
            .log
            .entries
            .iter()
            .filter(|entry| default_filter.includes(entry))
            .map(|entry| entry.summary.as_str())
            .collect();
        assert_eq!(shown, ["host terminal resize", "legacy resize"]);
        assert_eq!(default_filter.visible_entry_count(&cache), 2);
        assert_eq!(default_filter.hidden_entry_count(&cache), 1);
        let unfiltered = AgentDebugLogFilter {
            is_tree_panel_animation_resize_hidden: false,
        };
        assert_eq!(unfiltered.visible_entry_count(&cache), 3);
        assert_eq!(unfiltered.hidden_entry_count(&cache), 0);
    }

    #[test]
    fn every_event_kind_has_a_category() {
        assert_eq!(event_identity(&AgentDebugEventKind::Error), ("⚠", "ERROR"));
        assert_eq!(
            event_identity(&AgentDebugEventKind::Custom("x".into())),
            ("•", "EVENT")
        );
    }
}
