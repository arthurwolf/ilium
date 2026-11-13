//! Human-readable text of one cached per-agent debug journal. The server remains the authority for event
//! ordering and retention; this module only turns a retained snapshot into portable text. Writing the file
//! (owner-only, symlinks refused) is the host's `fs` job, not the plugin's.

use crate::report::AgentDebugEntry;
use crate::view::{
    context_summary, event_identity, format_timestamp, sanitize_single_line_text,
    sanitize_terminal_text, source_label, AgentDebugLogCache, AgentDebugLogFilter,
};

/// Builds the complete text written by the Save action.
pub fn render_report(
    pane_name: &str,
    pane_id: u64,
    cache: &AgentDebugLogCache,
    filter: AgentDebugLogFilter,
    saved_at_unix_millis: i64,
) -> String {
    let mut report = String::new();
    report.push_str("ILIUM AGENT DEBUG LOG\n");
    // Pane names can be LLM-inferred titles, so they are untrusted text for a line-oriented report.
    report.push_str(&format!("Pane: {}\n", sanitize_single_line_text(pane_name)));
    report.push_str(&format!("Pane ID: {pane_id}\n"));
    report.push_str(&format!(
        "Saved: {}\n",
        format_timestamp(saved_at_unix_millis)
    ));
    report.push_str(&format!("Retained events: {}\n", cache.log.entries.len()));
    report.push_str(&format!(
        "Exported events: {}\n",
        filter.visible_entry_count(cache)
    ));
    report.push_str(if filter.is_tree_panel_animation_resize_hidden {
        "Filter: left-panel focus/hover animation resize events excluded\n"
    } else {
        "Filter: left-panel focus/hover animation resize events included\n"
    });
    if cache.log.dropped_entry_count > 0 {
        report.push_str(&format!(
            "Older events discarded by retention: {}\n",
            cache.log.dropped_entry_count
        ));
    }
    report.push_str(
        "Detection policy: a detection decision is recorded only when identity, activity, goal, session, or supporting evidence changes.\n",
    );
    for entry in cache
        .log
        .entries
        .iter()
        .filter(|entry| filter.includes(entry))
    {
        append_entry(&mut report, entry);
    }
    report
}

/// How the host must write a saved report: it replaces a real file with one complete private report and never
/// follows a symlink, so an editable destination cannot redirect sensitive prompt/session evidence elsewhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportWrite {
    pub path: String,
    pub contents: String,
    /// Unix permission bits of a created or existing report (owner only).
    pub mode: u32,
    pub create: bool,
    pub truncate: bool,
    pub follow_symlinks: bool,
}

/// The write contract of the Save action, executed by the host's `fs` capability.
pub fn plan_report_write(path: &str, report: &str) -> ReportWrite {
    ReportWrite {
        path: path.to_string(),
        contents: report.to_string(),
        mode: 0o600,
        create: true,
        truncate: true,
        follow_symlinks: false,
    }
}

fn append_entry(report: &mut String, entry: &AgentDebugEntry) {
    let (_, category) = event_identity(&entry.kind);
    report.push('\n');
    report.push_str(&format!(
        "{}  [{}]  {}\n",
        format_timestamp(entry.occurred_at_unix_millis),
        category,
        sanitize_single_line_text(&entry.summary),
    ));
    report.push_str(&format!("Observed by: {}\n", source_label(entry.source)));
    if let Some(context) = context_summary(entry) {
        report.push_str(&format!("Context: {context}\n"));
    }
    if let Some(correlation_id) = entry.correlation_id.as_deref() {
        report.push_str(&format!(
            "Correlation: {}\n",
            sanitize_single_line_text(correlation_id)
        ));
    }
    for field in &entry.fields {
        // Values may legitimately span lines; continuation lines are indented so the report's `- label:`
        // structure stays parseable by eye.
        let value = sanitize_terminal_text(&field.value).replace('\n', "\n    ");
        report.push_str(&format!(
            "- {}: {value}\n",
            sanitize_single_line_text(&field.label)
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{
        AgentActivity, AgentClass, AgentDebugContext, AgentDebugEventKind, AgentDebugEventMetadata,
        AgentDebugField, AgentDebugSeverity, AgentDebugSource, PaneDebugLog, PaneResizeCause,
    };

    /// REQ: PORT:ilium-client/src/agent_debug_export.rs::report_is_complete_human_readable_and_omits_internal_heartbeat_counters
    #[test]
    fn report_is_complete_human_readable_and_omits_internal_heartbeat_counters() {
        let cache = AgentDebugLogCache {
            log: PaneDebugLog {
                entries: vec![AgentDebugEntry {
                    sequence: 7,
                    occurred_at_unix_millis: 1_700_000_000_000,
                    severity: AgentDebugSeverity::Information,
                    source: AgentDebugSource::Detector,
                    kind: AgentDebugEventKind::DetectionCycle,
                    summary: "Detected Codex and classified it as working".to_string(),
                    fields: vec![AgentDebugField::plain(
                        "Activity decision",
                        "Working because the visible terminal contains a live status line.",
                    )],
                    correlation_id: Some("submission-7".to_string()),
                    context: AgentDebugContext {
                        class: Some(AgentClass::Codex),
                        activity: Some(AgentActivity::Working),
                        process_id: Some(42),
                        session_id: Some("session-a".to_string()),
                        title_generation: 2,
                    },
                    metadata: Default::default(),
                }],
                ..Default::default()
            },
            through_sequence: 7,
            is_loading: false,
            has_loaded_retained_history: true,
        };
        let report = render_report(
            "Codex pane",
            12,
            &cache,
            AgentDebugLogFilter::default(),
            1_700_000_001_000,
        );
        assert!(report.contains("Detection policy: a detection decision is recorded only when"));
        assert!(report.contains("[DETECTION]  Detected Codex"));
        assert!(report.contains("Observed by: Agent detection engine"));
        assert!(report.contains("Context: Codex · working · process 42 · session session-a"));
        assert!(report.contains("Correlation: submission-7"));
        assert!(report.contains("Activity decision: Working because"));
        for forbidden in ["phase 1", "request generation", "repetition_count"] {
            assert!(!report.contains(forbidden));
        }
    }

    /// REQ: PORT:ilium-client/src/agent_debug_export.rs::report_applies_the_same_panel_resize_filter_as_the_viewer
    #[test]
    fn report_applies_the_same_panel_resize_filter_as_the_viewer() {
        let resize_entry = |sequence, summary: &str, cause| AgentDebugEntry {
            sequence,
            occurred_at_unix_millis: 1_700_000_000_000,
            severity: AgentDebugSeverity::Information,
            source: AgentDebugSource::Pty,
            kind: AgentDebugEventKind::PaneResized,
            summary: summary.to_string(),
            fields: Vec::new(),
            correlation_id: None,
            context: AgentDebugContext::default(),
            metadata: AgentDebugEventMetadata {
                pane_resize_cause: Some(cause),
            },
        };
        let cache = AgentDebugLogCache {
            log: PaneDebugLog {
                entries: vec![
                    resize_entry(
                        1,
                        "tree animation resize",
                        PaneResizeCause::TreePanelAnimation,
                    ),
                    resize_entry(2, "host terminal resize", PaneResizeCause::HostTerminal),
                ],
                ..Default::default()
            },
            through_sequence: 2,
            has_loaded_retained_history: true,
            ..AgentDebugLogCache::default()
        };
        let filtered = render_report(
            "Codex pane",
            12,
            &cache,
            AgentDebugLogFilter::default(),
            1_700_000_001_000,
        );
        assert!(filtered.contains("Exported events: 1"));
        assert!(filtered.contains("host terminal resize"));
        assert!(!filtered.contains("tree animation resize"));
        let unfiltered = render_report(
            "Codex pane",
            12,
            &cache,
            AgentDebugLogFilter {
                is_tree_panel_animation_resize_hidden: false,
            },
            1_700_000_001_000,
        );
        assert!(unfiltered.contains("Exported events: 2"));
        assert!(unfiltered.contains("tree animation resize"));
    }

    /// REQ: PORT:ilium-client/src/agent_debug_export.rs::report_keeps_untrusted_text_on_one_line_and_strips_escapes
    #[test]
    fn report_keeps_untrusted_text_on_one_line_and_strips_escapes() {
        let cache = AgentDebugLogCache {
            log: PaneDebugLog {
                entries: vec![AgentDebugEntry {
                    sequence: 1,
                    occurred_at_unix_millis: 1_700_000_000_000,
                    severity: AgentDebugSeverity::Information,
                    source: AgentDebugSource::Detector,
                    kind: AgentDebugEventKind::DetectionCycle,
                    summary: "line one\nline two".to_string(),
                    fields: Vec::new(),
                    correlation_id: None,
                    context: AgentDebugContext {
                        class: Some(AgentClass::Other("evil\u{1b}[31m\nagent".to_string())),
                        activity: None,
                        process_id: None,
                        session_id: Some("id\nwith newline".to_string()),
                        title_generation: 0,
                    },
                    metadata: Default::default(),
                }],
                ..Default::default()
            },
            through_sequence: 1,
            is_loading: false,
            has_loaded_retained_history: true,
        };
        let report = render_report(
            "pane\nname\u{1b}[2J",
            3,
            &cache,
            AgentDebugLogFilter::default(),
            1_700_000_001_000,
        );
        assert!(report.contains("Pane: pane name\u{fffd}[2J\n"));
        assert!(report.contains("[DETECTION]  line one line two\n"));
        assert!(report.contains("Context: evil\u{fffd}[31m agent · session id with newline\n"));
        assert!(!report.contains('\u{1b}'));
    }

    /// REQ: PORT:ilium-client/src/agent_debug_export.rs::saved_report_replaces_existing_content
    #[test]
    fn saved_report_replaces_existing_content() {
        let plan = plan_report_write("/home/me/agent-debug.log", "new report\n");
        assert!(plan.create && plan.truncate);
        assert_eq!(plan.contents, "new report\n");
    }

    /// REQ: PORT:ilium-client/src/agent_debug_export.rs::saved_report_is_private
    #[test]
    fn saved_report_is_private() {
        assert_eq!(plan_report_write("/p", "r").mode & 0o777, 0o600);
    }

    /// REQ: PORT:ilium-client/src/agent_debug_export.rs::saved_report_refuses_a_symlink_destination
    #[test]
    fn saved_report_refuses_a_symlink_destination() {
        assert!(!plan_report_write("/p", "r").follow_symlinks);
    }
}
