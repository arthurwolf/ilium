//! Server-owned recorder for per-pane agent-debug history. This module is the
//! only place that assigns timestamps/sequences and broadcasts durable entry
//! updates, keeping producers small and preventing client-authored ordering.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use ilium_agent_debug::{
    AgentDebugContext, AgentDebugEntry, AgentDebugEventDraft, AgentDebugEventKind, AgentDebugField,
    AgentDebugSeverity, AgentDebugSource, PaneDebugLog,
};
use ilium_core::{NodeId, NodeKind, PaneStatus};
use ilium_ipc::ServerEvent;
use tokio::sync::RwLock;

use crate::pane::PaneResource;
use crate::state::ServerState;

/// One restored/persisted pane journal. A vector is used by the session JSON
/// because `NodeId` cannot be a JSON object key without a custom serializer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PaneDebugLogSnapshot {
    pub pane_id: NodeId,
    pub log: PaneDebugLog,
}

/// Durable journal owner for one detached session server.
pub struct AgentDebugRecorder {
    enabled: AtomicBool,
    logs: RwLock<HashMap<NodeId, PaneDebugLog>>,
    /// Last accepted semantic detector/discovery conclusions. This is separate
    /// from the optional persisted UI journal so `/tmp` file logging can remain
    /// change-only even when the Agent debug menu is disabled.
    change_only_observations: RwLock<HashMap<NodeId, ChangeOnlyObservations>>,
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
    fields: Vec<ilium_agent_debug::AgentDebugField>,
    correlation_id: Option<String>,
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
        }
    }
}

impl AgentDebugRecorder {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            logs: RwLock::new(HashMap::new()),
            change_only_observations: RwLock::new(HashMap::new()),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    pub fn set_enabled(&self, enabled: bool) -> bool {
        self.enabled.swap(enabled, Ordering::AcqRel) != enabled
    }

    /// Appends with already-captured context. Detection uses this after
    /// releasing its tree/pane locks; other producers normally use
    /// [`record`] so context cannot drift from the server state.
    pub async fn append(
        &self,
        pane_id: NodeId,
        source: AgentDebugSource,
        context: AgentDebugContext,
        draft: AgentDebugEventDraft,
    ) -> Option<AgentDebugEntry> {
        if !self.is_enabled() {
            return None;
        }

        let mut logs = self.logs.write().await;
        logs.entry(pane_id)
            .or_default()
            .append(current_unix_millis(), source, context, draft)
    }

    pub async fn snapshot(&self) -> Vec<PaneDebugLogSnapshot> {
        let logs = self.logs.read().await;
        let mut snapshots: Vec<_> = logs
            .iter()
            .map(|(pane_id, log)| PaneDebugLogSnapshot {
                pane_id: *pane_id,
                log: log.clone(),
            })
            .collect();
        snapshots.sort_by_key(|snapshot| snapshot.pane_id);
        snapshots
    }

    pub async fn restore(&self, snapshots: Vec<PaneDebugLogSnapshot>) {
        let mut logs = self.logs.write().await;
        *logs = snapshots
            .into_iter()
            .map(|snapshot| {
                let mut log = snapshot.log;
                log.normalize_after_restore();
                (snapshot.pane_id, log)
            })
            .collect();
    }

    pub async fn remove(&self, pane_ids: &[NodeId]) {
        let mut logs = self.logs.write().await;
        for pane_id in pane_ids {
            logs.remove(pane_id);
        }
        drop(logs);

        let mut observations = self.change_only_observations.write().await;
        for pane_id in pane_ids {
            observations.remove(pane_id);
        }
    }

    pub async fn clear(&self) {
        self.logs.write().await.clear();
        self.change_only_observations.write().await.clear();
    }

    pub async fn replay(
        &self,
        pane_id: NodeId,
        after_sequence: Option<u64>,
    ) -> Option<(u64, u64, u64, Vec<AgentDebugEntry>)> {
        let logs = self.logs.read().await;
        let log = logs.get(&pane_id)?;
        Some((
            log.next_sequence.saturating_sub(1),
            log.retained_from_sequence(),
            log.dropped_entry_count,
            log.entries_after(after_sequence),
        ))
    }

    /// Accepts every independently meaningful process-log event, while
    /// suppressing an unchanged detector/discovery conclusion even if prompts
    /// or other events were logged between two polls. The persisted UI journal
    /// applies the same comparison in `PaneDebugLog` but keeps independent
    /// lifetime state, so enabling that sink later still records a baseline.
    async fn process_log_accepts_observation(
        &self,
        pane_id: NodeId,
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
        let mut observations = self.change_only_observations.write().await;
        let pane_observations = observations.entry(pane_id).or_default();
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

enum ChangeOnlySlot {
    DetectionCycle,
    SessionDiscovery,
}

/// Captures current context, records one entry, publishes it, and schedules
/// snapshot persistence. A missing/non-terminal pane rejects the observation.
pub async fn record(
    state: &ServerState,
    pane_id: NodeId,
    source: AgentDebugSource,
    draft: AgentDebugEventDraft,
) -> Option<AgentDebugEntry> {
    if !is_any_debug_sink_enabled(state) {
        return None;
    }

    let context = current_context(state, pane_id).await?;
    publish_recorded(state, pane_id, source, context, draft)
        .await
        .entry
}

/// Records with immutable context already captured by a producer while its
/// authoritative locks were held.
pub async fn record_with_context(
    state: &ServerState,
    pane_id: NodeId,
    source: AgentDebugSource,
    context: AgentDebugContext,
    draft: AgentDebugEventDraft,
) -> Option<AgentDebugEntry> {
    publish_recorded(state, pane_id, source, context, draft)
        .await
        .entry
}

struct PublishResult {
    accepted: bool,
    entry: Option<AgentDebugEntry>,
}

impl PublishResult {
    const REJECTED: Self = Self {
        accepted: false,
        entry: None,
    };
}

async fn publish_recorded(
    state: &ServerState,
    pane_id: NodeId,
    source: AgentDebugSource,
    context: AgentDebugContext,
    draft: AgentDebugEventDraft,
) -> PublishResult {
    if !is_any_debug_sink_enabled(state) {
        return PublishResult::REJECTED;
    }

    let process_log_accepted = ilium_logging::is_enabled()
        && state
            .agent_debug
            .process_log_accepts_observation(pane_id, source, &context, &draft)
            .await;
    if process_log_accepted {
        emit_process_log_event(pane_id, source, &context, &draft);
    }
    let entry = state
        .agent_debug
        .append(pane_id, source, context, draft)
        .await;
    if let Some(entry) = &entry {
        state.broadcast(ServerEvent::PaneDebugEntryAppended {
            pane_id,
            entry: entry.clone(),
        });
        state.request_snapshot_save();
    }
    PublishResult {
        accepted: process_log_accepted || entry.is_some(),
        entry,
    }
}

fn is_any_debug_sink_enabled(state: &ServerState) -> bool {
    state.agent_debug.is_enabled() || ilium_logging::is_enabled()
}

/// Mirrors one accepted semantic event into the timestamped process log. The
/// JSON value is intentionally structured and single-line. This opt-in private
/// diagnostic sink retains the exact lifecycle evidence and session context
/// needed to diagnose ownership mistakes, including submitted commands and
/// old/new session IDs. Credential-shaped JSON keys still pass through the
/// shared logging redactor before the event is written to the private `0600`
/// session log.
fn emit_process_log_event(
    pane_id: NodeId,
    source: AgentDebugSource,
    context: &AgentDebugContext,
    draft: &AgentDebugEventDraft,
) {
    if !ilium_logging::is_enabled() {
        return;
    }

    let payload = process_log_payload(pane_id, source, context, draft);
    match draft.severity {
        AgentDebugSeverity::Warning => tracing::warn!(
            target: "ilium_agent_event",
            pane_id = pane_id.0,
            event_kind = ?draft.kind,
            agent_event = %payload,
            "agent lifecycle event"
        ),
        AgentDebugSeverity::Error => tracing::error!(
            target: "ilium_agent_event",
            pane_id = pane_id.0,
            event_kind = ?draft.kind,
            agent_event = %payload,
            "agent lifecycle event"
        ),
        AgentDebugSeverity::Trace
        | AgentDebugSeverity::Information
        | AgentDebugSeverity::Success => tracing::info!(
            target: "ilium_agent_event",
            pane_id = pane_id.0,
            event_kind = ?draft.kind,
            agent_event = %payload,
            "agent lifecycle event"
        ),
    }
}

fn process_log_payload(
    pane_id: NodeId,
    source: AgentDebugSource,
    context: &AgentDebugContext,
    draft: &AgentDebugEventDraft,
) -> String {
    let process_log_fields: Vec<AgentDebugField> = draft
        .fields
        .iter()
        .map(|field| AgentDebugField {
            label: field.label.clone(),
            value: ilium_logging::redacted_diagnostic_field_value(&field.label, &field.value),
            presentation: field.presentation,
        })
        .collect();
    let payload = serde_json::json!({
        "pane_id": pane_id.0,
        "severity": &draft.severity,
        "source": source,
        "kind": &draft.kind,
        "summary": &draft.summary,
        "fields": process_log_fields,
        "correlation_id": &draft.correlation_id,
        "context": context,
    });
    ilium_logging::redacted_json_credentials(&payload).to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientEventRecordOutcome {
    Recorded,
    Suppressed,
    Disabled,
    Stale,
    MissingPane,
}

/// Validates a client-owned observation against the current conversation
/// identity before assigning durable ordering. Expected best-effort outcomes
/// remain distinct so stale diagnostics never masquerade as request failures.
pub async fn record_client_event(
    state: &ServerState,
    pane_id: NodeId,
    expected_session_id: Option<&str>,
    expected_title_generation: u64,
    draft: AgentDebugEventDraft,
) -> ClientEventRecordOutcome {
    if !is_any_debug_sink_enabled(state) {
        return ClientEventRecordOutcome::Disabled;
    }
    let Some(context) = current_context(state, pane_id).await else {
        return ClientEventRecordOutcome::MissingPane;
    };
    if context.session_id.as_deref() != expected_session_id
        || context.title_generation != expected_title_generation
    {
        return ClientEventRecordOutcome::Stale;
    }
    if publish_recorded(state, pane_id, AgentDebugSource::Client, context, draft)
        .await
        .accepted
    {
        ClientEventRecordOutcome::Recorded
    } else {
        ClientEventRecordOutcome::Suppressed
    }
}

async fn current_context(state: &ServerState, pane_id: NodeId) -> Option<AgentDebugContext> {
    let tree = state.tree.read().await;
    let node = tree.get(pane_id)?;
    let NodeKind::Pane { status, .. } = &node.kind else {
        return None;
    };
    let (class, activity) = match status {
        PaneStatus::Agent(class, activity) | PaneStatus::AgentWithGoal(class, activity) => {
            (Some(class.clone()), Some(*activity))
        }
        _ => (None, None),
    };
    let panes = state.panes.read().await;
    let (process_id, session_id, title_generation) = match panes.get(&pane_id) {
        Some(PaneResource::Terminal(runtime)) => (
            runtime
                .detected_agent_process_id
                .or(runtime.session_process_id),
            runtime.session_id.clone(),
            runtime.title_generation,
        ),
        _ => (None, None, 0),
    };
    Some(AgentDebugContext {
        class,
        activity,
        process_id,
        session_id,
        title_generation,
    })
}

fn current_unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_agent_debug::AgentDebugEventKind;

    fn draft(summary: &str) -> AgentDebugEventDraft {
        AgentDebugEventDraft::information(AgentDebugEventKind::PromptSubmitted, summary)
    }

    #[tokio::test]
    async fn disabled_recorder_does_not_create_a_journal() {
        let recorder = AgentDebugRecorder::new(false);

        assert_eq!(
            recorder
                .append(
                    NodeId(7),
                    AgentDebugSource::Pty,
                    AgentDebugContext::default(),
                    draft("hidden prompt"),
                )
                .await,
            None
        );
        assert!(recorder.snapshot().await.is_empty());
    }

    #[tokio::test]
    async fn replay_supports_initial_and_incremental_reads() {
        let recorder = AgentDebugRecorder::new(true);
        let pane_id = NodeId(7);
        for summary in ["first", "second", "third"] {
            recorder
                .append(
                    pane_id,
                    AgentDebugSource::Pty,
                    AgentDebugContext::default(),
                    draft(summary),
                )
                .await
                .expect("enabled recorder appends");
        }

        let (through_sequence, retained_from, dropped, initial) = recorder
            .replay(pane_id, None)
            .await
            .expect("journal exists");
        assert_eq!((through_sequence, retained_from, dropped), (3, 1, 0));
        assert_eq!(initial.len(), 3);

        let (_, _, _, incremental) = recorder
            .replay(pane_id, Some(2))
            .await
            .expect("journal exists");
        assert_eq!(incremental.len(), 1);
        assert_eq!(incremental[0].summary, "third");
    }

    #[tokio::test]
    async fn snapshot_restore_and_remove_follow_the_pane_lifecycle() {
        let pane_id = NodeId(11);
        let original = AgentDebugRecorder::new(true);
        original
            .append(
                pane_id,
                AgentDebugSource::Server,
                AgentDebugContext::default(),
                draft("persist me"),
            )
            .await
            .expect("enabled recorder appends");

        let restored = AgentDebugRecorder::new(true);
        restored.restore(original.snapshot().await).await;
        assert_eq!(
            restored.replay(pane_id, None).await.unwrap().3[0].summary,
            "persist me"
        );

        restored.remove(&[pane_id]).await;
        assert!(restored.replay(pane_id, None).await.is_none());
    }

    #[tokio::test]
    async fn change_only_filter_ignores_repeated_detection_across_other_events() {
        let recorder = AgentDebugRecorder::new(false);
        let pane_id = NodeId(19);
        let context = AgentDebugContext {
            class: Some(ilium_core::AgentClass::Codex),
            activity: Some(ilium_core::AgentActivity::Idle),
            process_id: Some(42),
            session_id: Some("session-a".to_string()),
            title_generation: 3,
        };
        let detection =
            AgentDebugEventDraft::information(AgentDebugEventKind::DetectionCycle, "Codex is idle");

        assert!(
            recorder
                .process_log_accepts_observation(
                    pane_id,
                    AgentDebugSource::Detector,
                    &context,
                    &detection,
                )
                .await
        );
        assert!(
            recorder
                .process_log_accepts_observation(
                    pane_id,
                    AgentDebugSource::Pty,
                    &context,
                    &draft("independent prompt"),
                )
                .await
        );
        assert!(
            !recorder
                .process_log_accepts_observation(
                    pane_id,
                    AgentDebugSource::Detector,
                    &context,
                    &detection,
                )
                .await
        );

        let changed = AgentDebugEventDraft::information(
            AgentDebugEventKind::DetectionCycle,
            "Codex is working",
        );
        assert!(
            recorder
                .process_log_accepts_observation(
                    pane_id,
                    AgentDebugSource::Detector,
                    &context,
                    &changed,
                )
                .await
        );
    }

    #[tokio::test]
    async fn enabling_the_ui_journal_after_file_logging_still_records_a_baseline() {
        let recorder = AgentDebugRecorder::new(false);
        let pane_id = NodeId(23);
        let context = AgentDebugContext::default();
        let detection =
            AgentDebugEventDraft::information(AgentDebugEventKind::DetectionCycle, "Codex is idle");

        assert!(
            recorder
                .process_log_accepts_observation(
                    pane_id,
                    AgentDebugSource::Detector,
                    &context,
                    &detection,
                )
                .await
        );
        assert!(recorder.set_enabled(true));
        assert!(recorder
            .append(pane_id, AgentDebugSource::Detector, context, detection,)
            .await
            .is_some());
    }

    #[test]
    fn process_log_payload_keeps_context_and_exact_lifecycle_evidence() {
        let context = AgentDebugContext {
            class: Some(ilium_core::AgentClass::Codex),
            activity: Some(ilium_core::AgentActivity::Working),
            process_id: Some(77),
            session_id: Some("session-new".to_string()),
            title_generation: 4,
        };
        let event = draft("Prompt accepted by the agent PTY").with_fields(vec![
            ilium_agent_debug::AgentDebugField::sensitive("submitted input", "/clear"),
            ilium_agent_debug::AgentDebugField::sensitive("API key", "field-secret"),
        ]);

        let payload = process_log_payload(NodeId(9), AgentDebugSource::Pty, &context, &event);
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
