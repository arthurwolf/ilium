//! Pure schema and retention policy for one pane's durable agent-debug
//! history. The detached server owns mutation and persistence; IPC transports
//! these values; the client owns their presentation.

use ilium_core::{AgentActivity, AgentClass};
use serde::{Deserialize, Serialize};

/// Current serialized journal schema. The outer session snapshot remains
/// independently versioned because a journal-only evolution must not imply a
/// tree migration.
pub const AGENT_DEBUG_SCHEMA_VERSION: u32 = 3;

/// A generous semantic-event ceiling prevents an unattended, focused agent
/// from growing its project snapshot without bound.
pub const MAXIMUM_AGENT_DEBUG_ENTRIES: usize = 5_000;

/// A second approximate byte ceiling protects against a small number of very
/// large prompts, provider responses, or error bodies.
pub const MAXIMUM_AGENT_DEBUG_BYTES: usize = 4 * 1024 * 1024;

/// Severity is data rather than presentation so errors remain distinguishable
/// in persisted history and alternate clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentDebugSeverity {
    Trace,
    Information,
    Success,
    Warning,
    Error,
}

/// Which architectural boundary observed an event. A client cannot assign
/// this field: the server stamps it when accepting an observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentDebugSource {
    Server,
    Client,
    Detector,
    SessionDiscovery,
    Pty,
    Persistence,
    Inference,
    Voice,
}

/// Stable semantic event identity. `Custom` is the intentional extension
/// point for future agent/provider events without turning presentation data
/// into the persistence contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentDebugEventKind {
    RecordingEnabled,
    RecordingDisabled,
    PaneCreated,
    PaneRestored,
    PaneFocused,
    PaneResized,
    PaneClosed,
    PtyInputWritten,
    PtyOutputClosed,
    AgentDetected,
    AgentLost,
    AgentChanged,
    DetectionCycle,
    ActivityChanged,
    GoalDetected,
    GoalCleared,
    ExecutionStarted,
    ExecutionFinished,
    ApprovalRequested,
    BackgroundWait,
    SessionDiscovery,
    SessionResolved,
    SessionCleared,
    ConversationCleared,
    PromptSubmitted,
    ScheduledInputCreated,
    ScheduledInputReplaced,
    ScheduledInputDelivered,
    ScheduledInputCleared,
    PromptQueued,
    QueuedPromptDelivered,
    PromptQueueCleared,
    TriggerEvaluated,
    TriggerActionStarted,
    TriggerActionFinished,
    TitleInferenceRequested,
    TitleInferenceSucceeded,
    TitleInferenceFailed,
    TitleInferenceDiscarded,
    TitleApplied,
    VoiceAction,
    PersistenceSaved,
    IpcRequest,
    Error,
    RetentionNotice,
    Custom(String),
}

/// Presentation hint for one structured value. It remains deliberately
/// limited to content shape; icons, colours, and terminal styles stay client
/// concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentDebugFieldPresentation {
    Plain,
    Code,
    Multiline,
    Sensitive,
}

/// One named evidence value shown below an event summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDebugField {
    pub label: String,
    pub value: String,
    pub presentation: AgentDebugFieldPresentation,
}

impl AgentDebugField {
    pub fn plain(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            presentation: AgentDebugFieldPresentation::Plain,
        }
    }

    pub fn multiline(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            presentation: AgentDebugFieldPresentation::Multiline,
        }
    }

    pub fn sensitive(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            presentation: AgentDebugFieldPresentation::Sensitive,
        }
    }
}

/// Client-side geometry transition that caused a real PTY resize. Keeping
/// provenance in the durable event contract lets viewers suppress animation
/// noise without discarding evidence needed for deeper terminal debugging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneResizeCause {
    HostTerminal,
    TreePanelAnimation,
    RightPanelPresentation,
    UserInterfaceSettings,
}

impl PaneResizeCause {
    pub const fn label(self) -> &'static str {
        match self {
            Self::HostTerminal => "Host terminal geometry changed",
            Self::TreePanelAnimation => "Left panel focus/hover animation",
            Self::RightPanelPresentation => "Visible pane or split presentation changed",
            Self::UserInterfaceSettings => "User interface layout settings changed",
        }
    }
}

/// Immutable agent/conversation identity captured when the event occurs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDebugContext {
    pub class: Option<AgentClass>,
    pub activity: Option<AgentActivity>,
    pub process_id: Option<u32>,
    pub session_id: Option<String>,
    pub title_generation: u64,
}

/// Typed event-specific attributes that do not belong to agent identity or
/// generic human-readable evidence fields. New optional attributes can be
/// added here without weakening the stable event-kind contract.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDebugEventMetadata {
    /// Present only for [`AgentDebugEventKind::PaneResized`]. Older snapshots
    /// deserialize as unknown provenance and remain visible to filters.
    #[serde(default)]
    pub pane_resize_cause: Option<PaneResizeCause>,
}

/// An unstamped observation. Callers describe facts; the server supplies the
/// authoritative time, source, sequence, and current agent context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDebugEventDraft {
    pub severity: AgentDebugSeverity,
    pub kind: AgentDebugEventKind,
    pub summary: String,
    pub fields: Vec<AgentDebugField>,
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub metadata: AgentDebugEventMetadata,
}

impl AgentDebugEventDraft {
    pub fn information(kind: AgentDebugEventKind, summary: impl Into<String>) -> Self {
        Self {
            severity: AgentDebugSeverity::Information,
            kind,
            summary: summary.into(),
            fields: Vec::new(),
            correlation_id: None,
            metadata: AgentDebugEventMetadata::default(),
        }
    }

    pub fn with_fields(mut self, fields: Vec<AgentDebugField>) -> Self {
        self.fields = fields;
        self
    }

    /// Connects related observations across asynchronous subsystem boundaries.
    pub fn with_correlation_id(mut self, correlation_id: Option<String>) -> Self {
        self.correlation_id = correlation_id;
        self
    }

    pub fn with_pane_resize_cause(mut self, pane_resize_cause: PaneResizeCause) -> Self {
        self.metadata.pane_resize_cause = Some(pane_resize_cause);
        self
    }
}

/// One server-stamped durable history entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDebugEntry {
    pub sequence: u64,
    pub occurred_at_unix_millis: i64,
    pub severity: AgentDebugSeverity,
    pub source: AgentDebugSource,
    pub kind: AgentDebugEventKind,
    pub summary: String,
    pub fields: Vec<AgentDebugField>,
    pub correlation_id: Option<String>,
    pub context: AgentDebugContext,
    #[serde(default)]
    pub metadata: AgentDebugEventMetadata,
}

impl AgentDebugEntry {
    pub fn is_tree_panel_animation_resize(&self) -> bool {
        self.kind == AgentDebugEventKind::PaneResized
            && self.metadata.pane_resize_cause == Some(PaneResizeCause::TreePanelAnimation)
    }
}

/// Persisted bounded journal for one stable pane `NodeId`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneDebugLog {
    pub schema_version: u32,
    pub next_sequence: u64,
    pub dropped_entry_count: u64,
    pub dropped_approximate_bytes: u64,
    /// Cached size of retained entries. Restores recompute this value before
    /// accepting new events, so snapshots from before the field existed stay
    /// safe without imposing an O(history) scan on every append.
    #[serde(default)]
    pub retained_approximate_bytes: u64,
    pub entries: Vec<AgentDebugEntry>,
}

impl Default for PaneDebugLog {
    fn default() -> Self {
        Self {
            schema_version: AGENT_DEBUG_SCHEMA_VERSION,
            next_sequence: 1,
            dropped_entry_count: 0,
            dropped_approximate_bytes: 0,
            retained_approximate_bytes: 0,
            entries: Vec::new(),
        }
    }
}

impl PaneDebugLog {
    /// Stamps, appends, and retains one event. Detection/discovery observations
    /// are change-only: a poll that reaches the same semantic conclusion as the
    /// latest observation of that kind is ignored completely. Other events
    /// remain individually represented because repeated prompts, errors, and
    /// actions are independently useful facts.
    pub fn append(
        &mut self,
        occurred_at_unix_millis: i64,
        source: AgentDebugSource,
        context: AgentDebugContext,
        draft: AgentDebugEventDraft,
    ) -> Option<AgentDebugEntry> {
        if matches!(
            draft.kind,
            AgentDebugEventKind::DetectionCycle | AgentDebugEventKind::SessionDiscovery
        ) {
            let latest_same_kind = self
                .entries
                .iter()
                .rev()
                .find(|entry| entry.kind == draft.kind);
            let is_unchanged = latest_same_kind.is_some_and(|entry| {
                entry.source == source
                    && entry.severity == draft.severity
                    && entry.summary == draft.summary
                    && entry.fields == draft.fields
                    && entry.correlation_id == draft.correlation_id
                    && entry.metadata == draft.metadata
                    && entry.context == context
            });
            if is_unchanged {
                return None;
            }
        }

        let entry = AgentDebugEntry {
            sequence: self.next_sequence,
            occurred_at_unix_millis,
            severity: draft.severity,
            source,
            kind: draft.kind,
            summary: draft.summary,
            fields: draft.fields,
            correlation_id: draft.correlation_id,
            context,
            metadata: draft.metadata,
        };
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.retained_approximate_bytes = self
            .retained_approximate_bytes
            .saturating_add(approximate_entry_bytes(&entry) as u64);
        self.entries.push(entry.clone());
        self.enforce_retention();
        Some(entry)
    }

    /// Merges one already-sequenced entry into the log at its sorted
    /// position, replacing any existing entry with the same sequence, then
    /// enforces the same retention policy `append` uses. Unlike `append`,
    /// this never stamps a sequence of its own: it exists for a mirror that
    /// receives entries whose sequence was assigned by the journal's real
    /// owner and that can arrive out of order relative to each other (a
    /// connected client reconciling a retained-history snapshot with live
    /// broadcasts -- see `ilium-client`'s `render_cache` module). Keeping
    /// this retention logic here, instead of duplicating it client-side,
    /// means that mirror can never grow past the same bound this journal
    /// itself enforces.
    pub fn merge_synced_entry(&mut self, entry: AgentDebugEntry) {
        let entry_bytes = approximate_entry_bytes(&entry) as u64;
        match self
            .entries
            .binary_search_by_key(&entry.sequence, |candidate| candidate.sequence)
        {
            Ok(index) => {
                let previous_bytes = approximate_entry_bytes(&self.entries[index]) as u64;
                self.retained_approximate_bytes = self
                    .retained_approximate_bytes
                    .saturating_sub(previous_bytes)
                    .saturating_add(entry_bytes);
                self.entries[index] = entry;
            }
            Err(index) => {
                self.retained_approximate_bytes =
                    self.retained_approximate_bytes.saturating_add(entry_bytes);
                self.entries.insert(index, entry);
            }
        }
        self.next_sequence = self.next_sequence.max(
            self.entries
                .last()
                .map_or(1, |entry| entry.sequence.saturating_add(1)),
        );
        self.enforce_retention();
    }

    /// Re-establishes all derived retention metadata after deserialization.
    /// The server invokes this for every restored pane before the journal can
    /// receive a new entry.
    pub fn normalize_after_restore(&mut self) {
        self.schema_version = AGENT_DEBUG_SCHEMA_VERSION;
        self.next_sequence = self
            .entries
            .last()
            .map_or(1, |entry| entry.sequence.saturating_add(1))
            .max(self.next_sequence);
        self.retained_approximate_bytes = approximate_entries_bytes(&self.entries) as u64;
        self.enforce_retention();
    }

    /// Drops every entry older than `floor_sequence`, keeping
    /// `retained_approximate_bytes` in lockstep. A reconciled snapshot's
    /// authoritative `retained_from_sequence` can be stricter than this
    /// mirror's own approximate byte/count retention (see
    /// `merge_synced_entry`), so a caller reconciling a mirror against it
    /// must trim through this method rather than filtering `entries`
    /// directly -- doing that would desynchronize the byte accounting
    /// `enforce_retention` relies on, and a later append could then try to
    /// evict from an already-empty vector.
    pub fn retain_from_sequence(&mut self, floor_sequence: u64) {
        let mut removed_bytes: u64 = 0;
        self.entries.retain(|entry| {
            if entry.sequence >= floor_sequence {
                true
            } else {
                removed_bytes = removed_bytes.saturating_add(approximate_entry_bytes(entry) as u64);
                false
            }
        });
        self.retained_approximate_bytes = self
            .retained_approximate_bytes
            .saturating_sub(removed_bytes);
    }

    pub fn retained_from_sequence(&self) -> u64 {
        self.entries
            .first()
            .map_or(self.next_sequence, |entry| entry.sequence)
    }

    pub fn entries_after(&self, after_sequence: Option<u64>) -> Vec<AgentDebugEntry> {
        self.entries
            .iter()
            .filter(|entry| after_sequence.is_none_or(|sequence| entry.sequence > sequence))
            .cloned()
            .collect()
    }

    fn enforce_retention(&mut self) {
        // `retained_approximate_bytes` is normally kept in lockstep with the
        // real sum of entry sizes by every mutator in this type, but every
        // field here is `pub` for (de)serialization, so a caller can load a
        // stale or hand-edited snapshot whose byte count no longer matches
        // its (possibly empty) `entries`. Guarding on emptiness keeps
        // `remove(0)` panic-free instead of trusting that invariant blindly.
        while !self.entries.is_empty()
            && (self.entries.len() > MAXIMUM_AGENT_DEBUG_ENTRIES
                || self.retained_approximate_bytes > MAXIMUM_AGENT_DEBUG_BYTES as u64)
        {
            let removed_entry = self.entries.remove(0);
            let removed_bytes = approximate_entry_bytes(&removed_entry) as u64;
            self.dropped_entry_count = self.dropped_entry_count.saturating_add(1);
            self.dropped_approximate_bytes =
                self.dropped_approximate_bytes.saturating_add(removed_bytes);
            self.retained_approximate_bytes = self
                .retained_approximate_bytes
                .saturating_sub(removed_bytes);
        }
        // An empty journal always has zero retained bytes; self-heal any
        // residual drift from a corrupted or externally mutated count rather
        // than leaving it permanently wrong until the next full restore.
        if self.entries.is_empty() {
            self.retained_approximate_bytes = 0;
        }
    }
}

fn approximate_entries_bytes(entries: &[AgentDebugEntry]) -> usize {
    entries.iter().map(approximate_entry_bytes).sum()
}

fn approximate_entry_bytes(entry: &AgentDebugEntry) -> usize {
    serde_json::to_vec(entry).map_or(0, |bytes| bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_assigns_monotonic_sequences_and_round_trips() {
        let mut log = PaneDebugLog::default();
        let first = log
            .append(
                1_700_000_000_000,
                AgentDebugSource::Server,
                AgentDebugContext::default(),
                AgentDebugEventDraft::information(
                    AgentDebugEventKind::AgentDetected,
                    "Codex detected",
                ),
            )
            .expect("first event must append");
        let second = log
            .append(
                1_700_000_000_001,
                AgentDebugSource::Pty,
                AgentDebugContext::default(),
                AgentDebugEventDraft::information(
                    AgentDebugEventKind::PromptSubmitted,
                    "Prompt submitted",
                ),
            )
            .expect("second event must append");

        assert_eq!((first.sequence, second.sequence), (1, 2));
        let json = serde_json::to_string(&log).unwrap();
        let restored: PaneDebugLog = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, log);
        assert_eq!(restored.next_sequence, 3);
    }

    #[test]
    fn resize_provenance_round_trips_and_identifies_only_tree_panel_animation_noise() {
        let mut log = PaneDebugLog::default();
        for pane_resize_cause in [
            PaneResizeCause::TreePanelAnimation,
            PaneResizeCause::HostTerminal,
        ] {
            let _ = log.append(
                1_700_000_000_000,
                AgentDebugSource::Pty,
                AgentDebugContext::default(),
                AgentDebugEventDraft::information(
                    AgentDebugEventKind::PaneResized,
                    "Agent PTY resized",
                )
                .with_pane_resize_cause(pane_resize_cause),
            );
        }

        let restored: PaneDebugLog =
            serde_json::from_str(&serde_json::to_string(&log).unwrap()).unwrap();

        assert!(restored.entries[0].is_tree_panel_animation_resize());
        assert!(!restored.entries[1].is_tree_panel_animation_resize());
    }

    #[test]
    fn entries_from_before_resize_provenance_remain_visible() {
        let mut serialized_entry = serde_json::to_value(AgentDebugEntry {
            sequence: 1,
            occurred_at_unix_millis: 1_700_000_000_000,
            severity: AgentDebugSeverity::Information,
            source: AgentDebugSource::Pty,
            kind: AgentDebugEventKind::PaneResized,
            summary: "Legacy PTY resize".to_string(),
            fields: Vec::new(),
            correlation_id: None,
            context: AgentDebugContext::default(),
            metadata: AgentDebugEventMetadata::default(),
        })
        .unwrap();
        serialized_entry
            .as_object_mut()
            .expect("entry serializes as an object")
            .remove("metadata");

        let restored: AgentDebugEntry = serde_json::from_value(serialized_entry).unwrap();

        assert_eq!(restored.metadata, AgentDebugEventMetadata::default());
        assert!(!restored.is_tree_panel_animation_resize());
    }

    #[test]
    fn unchanged_detection_polls_are_ignored_without_mutating_history() {
        let mut log = PaneDebugLog::default();
        let draft = AgentDebugEventDraft::information(
            AgentDebugEventKind::DetectionCycle,
            "Agent remains idle",
        );
        let first = log.append(
            1_700_000_000_000,
            AgentDebugSource::Detector,
            AgentDebugContext::default(),
            draft.clone(),
        );
        let repeated = log.append(
            1_700_000_001_000,
            AgentDebugSource::Detector,
            AgentDebugContext::default(),
            draft,
        );

        assert!(first.is_some());
        assert_eq!(repeated, None);
        assert_eq!(log.entries.len(), 1);
        assert_eq!(log.entries[0].occurred_at_unix_millis, 1_700_000_000_000);
        assert_eq!(log.next_sequence, 2);
    }

    #[test]
    fn unchanged_detection_is_ignored_across_intervening_non_detection_events() {
        let mut log = PaneDebugLog::default();
        let detection = AgentDebugEventDraft::information(
            AgentDebugEventKind::DetectionCycle,
            "Codex is working",
        );
        assert!(log
            .append(
                1,
                AgentDebugSource::Detector,
                AgentDebugContext::default(),
                detection.clone(),
            )
            .is_some());
        assert!(log
            .append(
                2,
                AgentDebugSource::Pty,
                AgentDebugContext::default(),
                AgentDebugEventDraft::information(
                    AgentDebugEventKind::PromptSubmitted,
                    "Prompt submitted",
                ),
            )
            .is_some());

        assert_eq!(
            log.append(
                3,
                AgentDebugSource::Detector,
                AgentDebugContext::default(),
                detection,
            ),
            None
        );
        assert_eq!(log.entries.len(), 2);
        assert_eq!(log.next_sequence, 3);
    }

    #[test]
    fn changed_detection_decision_appends_a_new_entry() {
        let mut log = PaneDebugLog::default();
        for summary in ["Codex is working", "Codex is waiting for approval"] {
            assert!(log
                .append(
                    1,
                    AgentDebugSource::Detector,
                    AgentDebugContext::default(),
                    AgentDebugEventDraft::information(
                        AgentDebugEventKind::DetectionCycle,
                        summary,
                    ),
                )
                .is_some());
        }

        assert_eq!(log.entries.len(), 2);
        assert_eq!(log.entries[1].sequence, 2);
    }

    #[test]
    fn retention_drops_oldest_entries_without_rescanning_the_journal() {
        let mut log = PaneDebugLog::default();
        for sequence in 0..=MAXIMUM_AGENT_DEBUG_ENTRIES {
            let _ = log.append(
                sequence as i64,
                AgentDebugSource::Detector,
                AgentDebugContext::default(),
                AgentDebugEventDraft::information(
                    AgentDebugEventKind::ActivityChanged,
                    format!("event {sequence}"),
                ),
            );
        }

        assert_eq!(log.entries.len(), MAXIMUM_AGENT_DEBUG_ENTRIES);
        assert_eq!(log.dropped_entry_count, 1);
        assert_eq!(log.entries.first().unwrap().sequence, 2);
        assert_eq!(
            log.retained_approximate_bytes,
            approximate_entries_bytes(&log.entries) as u64
        );
    }

    #[test]
    fn merge_synced_entry_inserts_out_of_order_and_replaces_duplicates() {
        let mut log = PaneDebugLog::default();
        let entry = |sequence: u64, summary: &str| AgentDebugEntry {
            sequence,
            occurred_at_unix_millis: 1_700_000_000_000 + sequence as i64,
            severity: AgentDebugSeverity::Information,
            source: AgentDebugSource::Detector,
            kind: AgentDebugEventKind::DetectionCycle,
            summary: summary.to_string(),
            fields: Vec::new(),
            correlation_id: None,
            context: AgentDebugContext::default(),
            metadata: AgentDebugEventMetadata::default(),
        };

        log.merge_synced_entry(entry(1, "first"));
        log.merge_synced_entry(entry(3, "third"));
        log.merge_synced_entry(entry(2, "second"));
        log.merge_synced_entry(entry(3, "third replay copy"));

        assert_eq!(
            log.entries
                .iter()
                .map(|entry| entry.sequence)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(log.entries[2].summary, "third replay copy");
    }

    #[test]
    fn merge_synced_entry_enforces_the_same_retention_cap_as_append_alone() {
        let mut log = PaneDebugLog::default();
        for sequence in 1..=(MAXIMUM_AGENT_DEBUG_ENTRIES as u64 + 50) {
            log.merge_synced_entry(AgentDebugEntry {
                sequence,
                occurred_at_unix_millis: sequence as i64,
                severity: AgentDebugSeverity::Information,
                source: AgentDebugSource::Detector,
                kind: AgentDebugEventKind::ActivityChanged,
                summary: format!("event {sequence}"),
                fields: Vec::new(),
                correlation_id: None,
                context: AgentDebugContext::default(),
                metadata: AgentDebugEventMetadata::default(),
            });
        }

        assert_eq!(log.entries.len(), MAXIMUM_AGENT_DEBUG_ENTRIES);
        assert_eq!(log.dropped_entry_count, 50);
        assert_eq!(log.retained_from_sequence(), 51);
        assert!(log.retained_approximate_bytes <= MAXIMUM_AGENT_DEBUG_BYTES as u64);
    }

    #[test]
    fn retain_from_sequence_keeps_byte_accounting_consistent_with_a_trimmed_empty_log() {
        let mut log = PaneDebugLog::default();
        for sequence in 1..=3u64 {
            log.merge_synced_entry(AgentDebugEntry {
                sequence,
                occurred_at_unix_millis: sequence as i64,
                severity: AgentDebugSeverity::Information,
                source: AgentDebugSource::Detector,
                kind: AgentDebugEventKind::ActivityChanged,
                summary: format!("event {sequence}"),
                fields: Vec::new(),
                correlation_id: None,
                context: AgentDebugContext::default(),
                metadata: AgentDebugEventMetadata::default(),
            });
        }

        // A reconciled snapshot can report a floor sequence past every entry
        // this mirror currently holds (e.g. the mirror was stale and the
        // server has since retention-dropped all of it). This is exactly
        // what `ilium-client`'s `render_cache` applies after replaying a
        // `PaneDebugLogSnapshot`.
        log.retain_from_sequence(100);

        assert!(log.entries.is_empty());
        assert_eq!(
            log.retained_approximate_bytes, 0,
            "trimming to empty must not leave stale bytes behind"
        );

        // Regression guard: before `retain_from_sequence` existed, trimming
        // `entries` directly left `retained_approximate_bytes` stale and
        // positive, and the next mutation's retention pass would then call
        // `Vec::remove(0)` on an already-empty vector and panic.
        let appended = log
            .append(
                1_700_000_000_000,
                AgentDebugSource::Detector,
                AgentDebugContext::default(),
                AgentDebugEventDraft::information(
                    AgentDebugEventKind::ActivityChanged,
                    "event after trim",
                ),
            )
            .expect("append after an empty trim must not panic");
        assert_eq!(appended.sequence, 4);
        assert_eq!(log.entries.len(), 1);
    }

    #[test]
    fn restore_recomputes_missing_byte_metadata_and_enforces_the_size_cap() {
        let oversized_value = "x".repeat(MAXIMUM_AGENT_DEBUG_BYTES / 2);
        let mut log = PaneDebugLog::default();
        for sequence in 0..3 {
            let _ = log.append(
                sequence,
                AgentDebugSource::Client,
                AgentDebugContext::default(),
                AgentDebugEventDraft::information(
                    AgentDebugEventKind::TitleInferenceRequested,
                    "large request",
                )
                .with_fields(vec![AgentDebugField::sensitive(
                    "request",
                    oversized_value.clone(),
                )]),
            );
        }
        log.retained_approximate_bytes = 0;

        log.normalize_after_restore();

        assert!(log.retained_approximate_bytes <= MAXIMUM_AGENT_DEBUG_BYTES as u64);
        assert!(log.dropped_entry_count > 0);
        assert_eq!(
            log.retained_approximate_bytes,
            approximate_entries_bytes(&log.entries) as u64
        );
    }
}
