//! Server-side recorder for per-pane agent-debug history, as pure state. It is the only place that assigns
//! timestamps and sequences: the caller (the host, on every server event) passes the wall clock in as
//! `now_millis`, the recorder never reads one. The journal sink (the Agent debug menu) and the process-log sink
//! (file logging) are independent, so file logging stays change-only even while the journal is off.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::report::{
    AgentDebugContext, AgentDebugEntry, AgentDebugEventDraft, AgentDebugEventKind,
    AgentDebugEventMetadata, AgentDebugField, AgentDebugSeverity, AgentDebugSource, PaneDebugLog,
};

/// One restored/persisted pane journal. A vector is used by the session JSON because a pane id cannot be a JSON
/// object key without a custom serializer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneDebugLogSnapshot {
    pub pane_id: u64,
    pub log: PaneDebugLog,
}

/// Snapshot preflight rejected the requested retained-memory allowance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotMemoryLimit;

/// What a replay request returns: the last assigned sequence, the first retained sequence, how many entries were
/// dropped by retention, and the entries after the requested sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replay {
    pub through_sequence: u64,
    pub retained_from_sequence: u64,
    pub dropped_entry_count: u64,
    pub entries: Vec<AgentDebugEntry>,
}

#[derive(Default)]
struct ChangeOnlyObservations {
    detection_cycle: Option<AgentDebugObservation>,
    session_discovery: Option<AgentDebugObservation>,
}

#[derive(Clone, PartialEq, Eq)]
struct AgentDebugObservation {
    source: AgentDebugSource,
    context: AgentDebugContext,
    severity: AgentDebugSeverity,
    summary: String,
    fields: Vec<AgentDebugField>,
    correlation_id: Option<String>,
    metadata: AgentDebugEventMetadata,
}

impl AgentDebugObservation {
    fn new(
        source: AgentDebugSource,
        context: &AgentDebugContext,
        draft: &AgentDebugEventDraft,
    ) -> Self {
        Self {
            source,
            context: context.clone(),
            severity: draft.severity,
            summary: draft.summary.clone(),
            fields: draft.fields.clone(),
            correlation_id: draft.correlation_id.clone(),
            metadata: draft.metadata.clone(),
        }
    }
}

enum ChangeOnlySlot {
    DetectionCycle,
    SessionDiscovery,
}

/// Durable journal owner for one session.
#[derive(Default)]
pub struct AgentDebugRecorder {
    enabled: bool,
    logs: BTreeMap<u64, PaneDebugLog>,
    /// Last accepted semantic detector/discovery conclusions. This is separate from the optional persisted UI
    /// journal so file logging can remain change-only even when the Agent debug menu is disabled.
    change_only_observations: BTreeMap<u64, ChangeOnlyObservations>,
}

impl AgentDebugRecorder {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Returns whether the state changed.
    pub fn set_enabled(&mut self, enabled: bool) -> bool {
        let changed = self.enabled != enabled;
        self.enabled = enabled;
        changed
    }

    /// Appends with already-captured context. A disabled recorder creates no journal.
    pub fn append(
        &mut self,
        now_millis: i64,
        pane_id: u64,
        source: AgentDebugSource,
        context: AgentDebugContext,
        draft: AgentDebugEventDraft,
    ) -> Option<AgentDebugEntry> {
        if !self.enabled {
            return None;
        }
        self.logs
            .entry(pane_id)
            .or_default()
            .append(now_millis, source, context, draft)
    }

    /// Every journal, ordered by pane id.
    pub fn snapshot(&self) -> Vec<PaneDebugLogSnapshot> {
        self.logs
            .iter()
            .map(|(pane_id, log)| PaneDebugLogSnapshot {
                pane_id: *pane_id,
                log: log.clone(),
            })
            .collect()
    }

    /// Preflight before allocating a deep snapshot; a rejection preserves the logs.
    pub fn snapshot_with_limit(
        &self,
        limit: usize,
    ) -> Result<Vec<PaneDebugLogSnapshot>, SnapshotMemoryLimit> {
        let bytes = self.logs.values().fold(
            self.logs
                .len()
                .saturating_mul(std::mem::size_of::<PaneDebugLogSnapshot>()),
            |bytes, log| bytes.saturating_add(estimated_debug_log_bytes(log)),
        );
        if bytes > limit {
            return Err(SnapshotMemoryLimit);
        }
        Ok(self.snapshot())
    }

    pub fn restore(&mut self, snapshots: Vec<PaneDebugLogSnapshot>) {
        self.logs = snapshots
            .into_iter()
            .map(|snapshot| {
                let mut log = snapshot.log;
                log.normalize_after_restore();
                (snapshot.pane_id, log)
            })
            .collect();
    }

    pub fn remove(&mut self, pane_ids: &[u64]) {
        for pane_id in pane_ids {
            self.logs.remove(pane_id);
            self.change_only_observations.remove(pane_id);
        }
    }

    pub fn clear(&mut self) {
        self.logs.clear();
        self.change_only_observations.clear();
    }

    pub fn replay(&self, pane_id: u64, after_sequence: Option<u64>) -> Option<Replay> {
        let log = self.logs.get(&pane_id)?;
        Some(Replay {
            through_sequence: log.next_sequence.saturating_sub(1),
            retained_from_sequence: log.retained_from_sequence(),
            dropped_entry_count: log.dropped_entry_count,
            entries: log.entries_after(after_sequence),
        })
    }

    /// Accepts every independently meaningful process-log event, while suppressing an unchanged detector or
    /// discovery conclusion even if prompts or other events were logged between two polls. The persisted UI
    /// journal applies the same comparison in `PaneDebugLog` but keeps independent lifetime state, so enabling
    /// that sink later still records a baseline.
    pub fn process_log_accepts_observation(
        &mut self,
        pane_id: u64,
        source: AgentDebugSource,
        context: &AgentDebugContext,
        draft: &AgentDebugEventDraft,
    ) -> bool {
        let slot = match draft.kind {
            AgentDebugEventKind::DetectionCycle => ChangeOnlySlot::DetectionCycle,
            AgentDebugEventKind::SessionDiscovery => ChangeOnlySlot::SessionDiscovery,
            _ => return true,
        };
        let observation = AgentDebugObservation::new(source, context, draft);
        let pane_observations = self.change_only_observations.entry(pane_id).or_default();
        let previous = match slot {
            ChangeOnlySlot::DetectionCycle => &mut pane_observations.detection_cycle,
            ChangeOnlySlot::SessionDiscovery => &mut pane_observations.session_discovery,
        };
        if previous.as_ref() == Some(&observation) {
            return false;
        }
        *previous = Some(observation);
        true
    }
}

/// Approximate retained heap of one journal, charging the reserved capacity so spare room counts.
pub fn estimated_debug_log_bytes(log: &PaneDebugLog) -> usize {
    let mut bytes = log
        .entries
        .capacity()
        .saturating_mul(std::mem::size_of::<AgentDebugEntry>());
    for entry in &log.entries {
        for amount in [
            entry.summary.capacity(),
            entry.correlation_id.as_ref().map_or(0, String::capacity),
            entry
                .context
                .session_id
                .as_ref()
                .map_or(0, String::capacity),
            entry
                .fields
                .capacity()
                .saturating_mul(std::mem::size_of::<AgentDebugField>()),
        ] {
            bytes = bytes.saturating_add(amount);
        }
        if let Some(crate::report::AgentClass::Other(class)) = &entry.context.class {
            bytes = bytes.saturating_add(class.capacity());
        }
        for field in &entry.fields {
            bytes = bytes
                .saturating_add(field.label.capacity())
                .saturating_add(field.value.capacity());
        }
    }
    bytes
}

/// Every name whose value is a credential, in the normalized form `is_credential_name` produces: lowercase with
/// every separator removed (copy of the kernel `ilium-logging` registry).
const CREDENTIAL_NAMES: &[&str] = &[
    "apikey",
    "authorization",
    "proxyauthorization",
    "password",
    "secret",
    "clientsecret",
    "accesstoken",
    "refreshtoken",
    "idtoken",
    "authtoken",
    "token",
    "cookie",
    "setcookie",
    "xapikey",
    "xgoogapikey",
    "xauthtoken",
    "xaccesstoken",
];

/// True when `name` itself names a credential in any spelling (`api_key`, `api-key`, `apiKey`, `API key`).
pub fn is_credential_name(name: &str) -> bool {
    let normalized: String = name
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect();
    CREDENTIAL_NAMES.contains(&normalized.as_str())
}

/// Redacts one structured diagnostic field only when its label is itself a credential name.
pub fn redacted_diagnostic_field_value(label: &str, value: &str) -> String {
    if is_credential_name(label) {
        "<redacted>".to_string()
    } else {
        value.to_string()
    }
}

/// The single-line structured JSON mirrored into the timestamped process log for one accepted semantic event.
pub fn process_log_payload(
    pane_id: u64,
    source: AgentDebugSource,
    context: &AgentDebugContext,
    draft: &AgentDebugEventDraft,
) -> String {
    let fields: Vec<AgentDebugField> = draft
        .fields
        .iter()
        .map(|field| AgentDebugField {
            label: field.label.clone(),
            value: redacted_diagnostic_field_value(&field.label, &field.value),
            presentation: field.presentation,
        })
        .collect();
    serde_json::json!({
        "pane_id": pane_id,
        "severity": &draft.severity,
        "source": source,
        "kind": &draft.kind,
        "summary": &draft.summary,
        "fields": fields,
        "correlation_id": &draft.correlation_id,
        "context": context,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{AgentActivity, AgentClass};

    fn draft(summary: &str) -> AgentDebugEventDraft {
        AgentDebugEventDraft::information(AgentDebugEventKind::PromptSubmitted, summary)
    }

    const NOW: i64 = 1_700_000_000_000;

    /// REQ: PORT:ilium-server/src/agent_debug.rs::snapshot_preflight_rejects_spare_capacity_without_losing_journal
    #[test]
    fn snapshot_preflight_rejects_spare_capacity_without_losing_journal() {
        let mut recorder = AgentDebugRecorder::new(true);
        recorder
            .append(
                NOW,
                7,
                AgentDebugSource::Pty,
                AgentDebugContext::default(),
                draft("kept"),
            )
            .expect("entry");
        if let Some(log) = recorder.logs.get_mut(&7) {
            log.entries[0].summary.reserve(8192);
        }
        assert!(recorder.snapshot_with_limit(1024).is_err());
        let snapshot = recorder
            .snapshot_with_limit(16384)
            .expect("larger allowance");
        assert_eq!(snapshot[0].log.entries[0].summary, "kept");
        assert_eq!(snapshot[0].log.next_sequence, 2);
        assert_eq!(recorder.replay(7, None).map(|r| r.entries.len()), Some(1));
    }

    /// REQ: PORT:ilium-server/src/agent_debug.rs::disabled_recorder_does_not_create_a_journal
    #[test]
    fn disabled_recorder_does_not_create_a_journal() {
        let mut recorder = AgentDebugRecorder::new(false);
        assert_eq!(
            recorder.append(
                NOW,
                7,
                AgentDebugSource::Pty,
                AgentDebugContext::default(),
                draft("hidden prompt"),
            ),
            None
        );
        assert!(recorder.snapshot().is_empty());
    }

    /// REQ: PORT:ilium-server/src/agent_debug.rs::replay_supports_initial_and_incremental_reads
    #[test]
    fn replay_supports_initial_and_incremental_reads() {
        let mut recorder = AgentDebugRecorder::new(true);
        for summary in ["first", "second", "third"] {
            recorder
                .append(
                    NOW,
                    7,
                    AgentDebugSource::Pty,
                    AgentDebugContext::default(),
                    draft(summary),
                )
                .expect("enabled recorder appends");
        }
        let initial = recorder.replay(7, None).expect("journal exists");
        assert_eq!(
            (
                initial.through_sequence,
                initial.retained_from_sequence,
                initial.dropped_entry_count
            ),
            (3, 1, 0)
        );
        assert_eq!(initial.entries.len(), 3);
        let incremental = recorder.replay(7, Some(2)).expect("journal exists");
        assert_eq!(incremental.entries.len(), 1);
        assert_eq!(incremental.entries[0].summary, "third");
    }

    /// REQ: PORT:ilium-server/src/agent_debug.rs::snapshot_restore_and_remove_follow_the_pane_lifecycle
    #[test]
    fn snapshot_restore_and_remove_follow_the_pane_lifecycle() {
        let mut original = AgentDebugRecorder::new(true);
        original
            .append(
                NOW,
                11,
                AgentDebugSource::Server,
                AgentDebugContext::default(),
                draft("persist me"),
            )
            .expect("enabled recorder appends");
        let mut restored = AgentDebugRecorder::new(true);
        restored.restore(original.snapshot());
        assert_eq!(
            restored.replay(11, None).expect("restored").entries[0].summary,
            "persist me"
        );
        restored.remove(&[11]);
        assert!(restored.replay(11, None).is_none());
    }

    /// REQ: PORT:ilium-server/src/agent_debug.rs::change_only_filter_ignores_repeated_detection_across_other_events
    #[test]
    fn change_only_filter_ignores_repeated_detection_across_other_events() {
        let mut recorder = AgentDebugRecorder::new(false);
        let context = AgentDebugContext {
            class: Some(AgentClass::Codex),
            activity: Some(AgentActivity::Idle),
            process_id: Some(42),
            session_id: Some("session-a".to_string()),
            title_generation: 3,
        };
        let detection =
            AgentDebugEventDraft::information(AgentDebugEventKind::DetectionCycle, "Codex is idle");
        assert!(recorder.process_log_accepts_observation(
            19,
            AgentDebugSource::Detector,
            &context,
            &detection
        ));
        assert!(recorder.process_log_accepts_observation(
            19,
            AgentDebugSource::Pty,
            &context,
            &draft("independent prompt")
        ));
        assert!(!recorder.process_log_accepts_observation(
            19,
            AgentDebugSource::Detector,
            &context,
            &detection
        ));
        let changed = AgentDebugEventDraft::information(
            AgentDebugEventKind::DetectionCycle,
            "Codex is working",
        );
        assert!(recorder.process_log_accepts_observation(
            19,
            AgentDebugSource::Detector,
            &context,
            &changed
        ));
    }

    /// REQ: PORT:ilium-server/src/agent_debug.rs::enabling_the_ui_journal_after_file_logging_still_records_a_baseline
    #[test]
    fn enabling_the_ui_journal_after_file_logging_still_records_a_baseline() {
        let mut recorder = AgentDebugRecorder::new(false);
        let context = AgentDebugContext::default();
        let detection =
            AgentDebugEventDraft::information(AgentDebugEventKind::DetectionCycle, "Codex is idle");
        assert!(recorder.process_log_accepts_observation(
            23,
            AgentDebugSource::Detector,
            &context,
            &detection
        ));
        assert!(recorder.set_enabled(true));
        assert!(recorder
            .append(NOW, 23, AgentDebugSource::Detector, context, detection)
            .is_some());
    }

    /// REQ: PORT:ilium-server/src/agent_debug.rs::process_log_payload_keeps_context_and_exact_lifecycle_evidence
    #[test]
    fn process_log_payload_keeps_context_and_exact_lifecycle_evidence() {
        let context = AgentDebugContext {
            class: Some(AgentClass::Codex),
            activity: Some(AgentActivity::Working),
            process_id: Some(77),
            session_id: Some("session-new".to_string()),
            title_generation: 4,
        };
        let event = draft("Prompt accepted by the agent PTY").with_fields(vec![
            AgentDebugField::sensitive("submitted input", "/clear"),
            AgentDebugField::sensitive("API key", "field-secret"),
        ]);
        let payload = process_log_payload(9, AgentDebugSource::Pty, &context, &event);
        let parsed: serde_json::Value = serde_json::from_str(&payload).expect("valid JSON payload");
        assert_eq!(parsed["pane_id"], 9);
        assert_eq!(parsed["kind"], "prompt_submitted");
        assert_eq!(parsed["context"]["process_id"], 77);
        assert_eq!(parsed["context"]["session_id"], "session-new");
        assert_eq!(parsed["fields"][0]["value"], "/clear");
        assert_eq!(parsed["fields"][1]["value"], "<redacted>");
        assert!(payload.contains("/clear"));
        assert!(!payload.contains("field-secret"));
    }
}
