//! Human-readable file export for one cached per-agent debug journal. The
//! server remains the authority for event ordering and retention; this module
//! only turns the client's complete retained snapshot into portable text and
//! writes the user-selected local path.

use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use ilium_core::{AgentActivity, AgentClass, NodeId};
use ilium_ipc::{AgentDebugEntry, AgentDebugSource};

use crate::app::{AgentDebugLogCache, AgentDebugLogFilter};

/// Builds the complete text written by the Save action.
pub fn render_report(
    pane_name: &str,
    pane_id: NodeId,
    cache: &AgentDebugLogCache,
    filter: AgentDebugLogFilter,
    saved_at_unix_millis: i64,
) -> String {
    let mut report = String::new();
    report.push_str("ILIUM AGENT DEBUG LOG\n");
    report.push_str(&format!("Pane: {pane_name}\n"));
    report.push_str(&format!("Pane ID: {}\n", pane_id.0));
    report.push_str(&format!(
        "Saved: {}\n",
        crate::agent_debug_ui::format_timestamp(saved_at_unix_millis)
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

/// Replaces a real file with one complete private report. Refusing symlinks
/// prevents an editable destination from accidentally redirecting sensitive
/// prompt/session evidence into an unrelated target.
pub fn write_report(path: &Path, report: &str) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(report.as_bytes())?;
    file.flush()
}

fn append_entry(report: &mut String, entry: &AgentDebugEntry) {
    let (_, category) = crate::agent_debug_ui::event_identity(&entry.kind);
    report.push('\n');
    report.push_str(&format!(
        "{}  [{}]  {}\n",
        crate::agent_debug_ui::format_timestamp(entry.occurred_at_unix_millis),
        category,
        clean_text(&entry.summary),
    ));
    report.push_str(&format!("Observed by: {}\n", source_label(entry.source)));
    if let Some(context) = context_summary(entry) {
        report.push_str(&format!("Context: {context}\n"));
    }
    if let Some(correlation_id) = entry.correlation_id.as_deref() {
        report.push_str(&format!("Correlation: {}\n", clean_text(correlation_id)));
    }
    for field in &entry.fields {
        let value = clean_text(&field.value).replace('\n', "\n    ");
        report.push_str(&format!("- {}: {value}\n", clean_text(&field.label)));
    }
}

fn context_summary(entry: &AgentDebugEntry) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(class) = entry.context.class.as_ref() {
        parts.push(agent_class_label(class).to_string());
    }
    if let Some(activity) = entry.context.activity {
        parts.push(activity_label(activity).to_string());
    }
    if let Some(process_id) = entry.context.process_id {
        parts.push(format!("process {process_id}"));
    }
    if let Some(session_id) = entry.context.session_id.as_deref() {
        parts.push(format!("session {session_id}"));
    }
    if entry.context.title_generation > 0 {
        parts.push(format!(
            "conversation revision {}",
            entry.context.title_generation
        ));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn source_label(source: AgentDebugSource) -> &'static str {
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

fn agent_class_label(class: &AgentClass) -> &str {
    match class {
        AgentClass::Claude => "Claude Code",
        AgentClass::Codex => "Codex",
        AgentClass::Antigravity => "Antigravity",
        AgentClass::Other(name) => name,
    }
}

const fn activity_label(activity: AgentActivity) -> &'static str {
    match activity {
        AgentActivity::Working => "working",
        AgentActivity::WaitingApproval => "waiting for approval",
        AgentActivity::WaitingBackground => "waiting for background work",
        AgentActivity::Idle => "idle",
        AgentActivity::Done => "done",
    }
}

fn clean_text(value: &str) -> String {
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
    use ilium_ipc::{
        AgentDebugContext, AgentDebugEventKind, AgentDebugEventMetadata, AgentDebugSeverity,
        AgentDebugSource, PaneResizeCause,
    };

    use super::*;

    #[test]
    fn report_is_complete_human_readable_and_omits_internal_heartbeat_counters() {
        let cache = AgentDebugLogCache {
            log: ilium_ipc::PaneDebugLog {
                entries: vec![AgentDebugEntry {
                    sequence: 7,
                    occurred_at_unix_millis: 1_700_000_000_000,
                    severity: AgentDebugSeverity::Information,
                    source: AgentDebugSource::Detector,
                    kind: AgentDebugEventKind::DetectionCycle,
                    summary: "Detected Codex and classified it as working".to_string(),
                    fields: vec![ilium_ipc::AgentDebugField::plain(
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
            NodeId(12),
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
            log: ilium_ipc::PaneDebugLog {
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
            NodeId(12),
            &cache,
            AgentDebugLogFilter::default(),
            1_700_000_001_000,
        );
        assert!(filtered.contains("Exported events: 1"));
        assert!(filtered.contains("host terminal resize"));
        assert!(!filtered.contains("tree animation resize"));

        let unfiltered = render_report(
            "Codex pane",
            NodeId(12),
            &cache,
            AgentDebugLogFilter {
                is_tree_panel_animation_resize_hidden: false,
            },
            1_700_000_001_000,
        );
        assert!(unfiltered.contains("Exported events: 2"));
        assert!(unfiltered.contains("tree animation resize"));
    }

    #[test]
    fn saved_report_is_private_and_replaces_existing_content() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("agent-debug.log");
        std::fs::write(&path, "older and longer content").expect("seed destination");

        write_report(&path, "new report\n").expect("save report");

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new report\n");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn saved_report_refuses_a_symlink_destination() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("temporary directory");
        let target = directory.path().join("target.log");
        let path = directory.path().join("agent-debug.log");
        std::fs::write(&target, "must remain unchanged").expect("seed target");
        symlink(&target, &path).expect("create symlink");

        assert!(write_report(&path, "private evidence\n").is_err());
        assert_eq!(
            std::fs::read_to_string(target).unwrap(),
            "must remain unchanged"
        );
    }
}
