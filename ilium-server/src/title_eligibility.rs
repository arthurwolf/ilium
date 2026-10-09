//! Server-owned title authorization. Capture under tree-then-panes locks,
//! collect transcript evidence after releasing them, then compare again under
//! the same lock order immediately before granting a presentation mutation.
//! Neither a client eligibility claim nor generic `last_prompt` is evidence.

use std::ops::Deref;
use std::path::PathBuf;

use ilium_agent_session::{GenuineRequestEvidence, TranscriptLocator};
use ilium_core::{
    AgentProcessKey, BuiltinAgentProvider, Node, NodeKind, NodePresentationRevision,
    PaneContentKind, PaneTitleSource,
};
use ilium_execution::{JobCost, Lane};
use ilium_ipc::PaneTitleObservation;

use crate::execution::ExecutionClient;

const MAX_TITLE_EVIDENCE_CANDIDATES: usize = 64;
const TITLE_EVIDENCE_JOB_CANDIDATES: usize = 4;
const MAX_TITLE_CANDIDATE_BYTES: usize = 64 * 1024;

use crate::pane::{TerminalOrigin, TerminalPaneRuntime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalTitleKind {
    Agent,
    ConfirmedPlainShell,
    Unresolved,
}

/// Stronger than the wire observation: a PID can be reused, so server evidence
/// also carries the process start time and the invocation generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TitleRuntimeSnapshot {
    pub observation: PaneTitleObservation,
    pub pty_identity: ilium_pty::PtySessionIdentity,
    pub agent_generation: u64,
    pub process_key: Option<AgentProcessKey>,
    pub kind: TerminalTitleKind,
    pub identity_invalidated: bool,
}

impl TitleRuntimeSnapshot {
    /// `is_plain_shell_confirmed` must come from authoritative foreground
    /// inspection, never from a missing detection sample or terminal text.
    pub(crate) fn capture(
        node: &Node,
        runtime: &TerminalPaneRuntime,
        is_plain_shell_confirmed: bool,
    ) -> Self {
        let status = match &node.kind {
            NodeKind::Pane { status, .. } => Some(status),
            _ => None,
        };
        let historical = status.and_then(|status| status.agent_recovery());
        let class = runtime
            .detected_agent_class
            .as_ref()
            .or(runtime.session_agent_class.as_ref())
            .or_else(|| {
                status
                    .and_then(|status| status.known_agent_state())
                    .map(|state| &state.class)
            })
            .cloned();
        let process_key = runtime
            .agent_process_key
            .clone()
            .or_else(|| historical.map(|recovery| recovery.process.clone()));
        let process_id = runtime
            .detected_agent_process_id
            .or_else(|| process_key.as_ref().map(|key| key.process_id));
        let session_id = if runtime.is_session_identity_invalidated {
            None
        } else if runtime.session_agent_class.as_ref() == class.as_ref() {
            runtime.session_id.clone()
        } else {
            None
        };
        // A historical session is usable only when no new process/class has
        // taken ownership. Recovery is evidence of identity, not live input.
        let session_id = session_id.or_else(|| {
            historical
                .filter(|recovery| {
                    !runtime.is_session_identity_invalidated
                        && process_key.as_ref() == Some(&recovery.process)
                        && class.as_ref() == Some(&recovery.process.class)
                        && process_id == Some(recovery.process.process_id)
                })
                .and_then(|recovery| recovery.session_id.clone())
        });
        let has_launch_intent = [&runtime.origin]
            .into_iter()
            .chain(runtime.deferred_workspace_origin.as_ref())
            .any(|origin| matches!(origin, TerminalOrigin::Command(command) if BuiltinAgentProvider::from_command_line(command).is_some()))
            || runtime.pending_generated_session_id.is_some();
        let kind = if class.is_some() || historical.is_some() {
            TerminalTitleKind::Agent
        } else if is_plain_shell_confirmed
            && !has_launch_intent
            && matches!(runtime.origin, TerminalOrigin::PlainShell)
            && runtime.deferred_workspace_origin.is_none()
            && process_id.is_none()
            && runtime.session_id.is_none()
            && !runtime.is_session_identity_invalidated
        {
            TerminalTitleKind::ConfirmedPlainShell
        } else {
            TerminalTitleKind::Unresolved
        };
        Self {
            pty_identity: runtime.session.identity(),
            observation: PaneTitleObservation {
                pane_id: node.id,
                presentation_revision: node.presentation_revision,
                agent_class: class,
                session_id,
                process_id,
                title_generation: runtime.title_generation,
            },
            agent_generation: runtime.agent_generation,
            process_key,
            kind,
            identity_invalidated: runtime.is_session_identity_invalidated
                || runtime.title_generation == u64::MAX,
        }
    }
}

/// Construct only after acknowledged delivery of substantive authored text
/// plus Enter to this exact invocation. Undelivered queues and injected input
/// cannot create it. The input adapter owns storage and transition clearing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthoredRequestReceipt {
    pub agent_generation: u64,
    pub process_key: AgentProcessKey,
    pub title_generation: u64,
    pub session_id: Option<String>,
}

impl AuthoredRequestReceipt {
    pub(crate) fn for_invocation(current: &TitleRuntimeSnapshot) -> Option<Self> {
        let process_key = current.process_key.as_ref()?;
        if current.kind != TerminalTitleKind::Agent
            || current.identity_invalidated
            || current.observation.agent_class.as_ref() != Some(&process_key.class)
            || current.observation.process_id != Some(process_key.process_id)
        {
            return None;
        }
        Some(Self {
            agent_generation: current.agent_generation,
            process_key: process_key.clone(),
            title_generation: current.observation.title_generation,
            session_id: current.observation.session_id.clone(),
        })
    }

    pub(crate) fn matches(&self, current: &TitleRuntimeSnapshot) -> bool {
        !current.identity_invalidated
            && current.kind == TerminalTitleKind::Agent
            && self.agent_generation == current.agent_generation
            && current.process_key.as_ref() == Some(&self.process_key)
            && current.observation.process_id == Some(self.process_key.process_id)
            && current.observation.agent_class.as_ref() == Some(&self.process_key.class)
            && self.title_generation == current.observation.title_generation
            // Initial authored input can arrive before discovery publishes a
            // session. A later ID is admissible only for the same invocation.
            && self.session_id.as_ref().is_none_or(|session| current.observation.session_id.as_ref() == Some(session))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TitleEvidenceCandidate {
    pub observation: PaneTitleObservation,
    /// Server-resolved pane/project cwd, not an IPC path supplied by a client.
    pub project_cwd: PathBuf,
    pub runtime: Option<TitleRuntimeSnapshot>,
}

#[derive(Debug, Clone)]
pub(crate) struct CollectedTitleEvidence {
    candidate: TitleEvidenceCandidate,
    transcript: GenuineRequestEvidence,
}

pub(crate) struct CollectedTitleEvidenceBatch {
    entries: Vec<CollectedTitleEvidence>,
    // The flattened vector is admitted separately and remains charged through
    // caller reconciliation after each worker result is consumed.
    _storage: Option<std::sync::Arc<ilium_execution::StorageAdmission>>,
}

impl Deref for CollectedTitleEvidenceBatch {
    type Target = [CollectedTitleEvidence];

    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

impl CollectedTitleEvidence {
    pub(crate) fn observation(&self) -> &PaneTitleObservation {
        &self.candidate.observation
    }

    /// Only an unresolved shell candidate can gain a plain-shell grant from
    /// a fresh off-lock foreground observation. Agent evidence is independent.
    pub(crate) fn needs_shell_confirmation(&self) -> bool {
        self.candidate
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.kind == TerminalTitleKind::Unresolved)
    }

    pub(crate) fn proves_current_empty_history(
        &self,
        node: &Node,
        current: &TitleRuntimeSnapshot,
    ) -> bool {
        self.transcript == GenuineRequestEvidence::VerifiedEmpty
            && !node.is_name_fixed
            && node.presentation_revision == self.candidate.observation.presentation_revision
            && current.kind == TerminalTitleKind::Agent
            && !current.identity_invalidated
            && current.observation == self.candidate.observation
            && self.candidate.runtime.as_ref() == Some(current)
    }
}

/// `home_dir` is ServerState's platform-resolved home. Transcript reads run on
/// the shared bounded I/O bank. Call with no tree/panes locks held. Missing cwd,
/// unreadable history and worker failure all decline history-based authorization.
pub(crate) async fn collect_title_evidence(
    execution: Option<&ExecutionClient>,
    home_dir: PathBuf,
    candidates: Vec<TitleEvidenceCandidate>,
) -> CollectedTitleEvidenceBatch {
    let Some(execution) = execution else {
        return CollectedTitleEvidenceBatch {
            entries: Vec::new(),
            _storage: None,
        };
    };
    if candidates.len() > MAX_TITLE_EVIDENCE_CANDIDATES {
        return unavailable_batch(execution, candidates).await;
    }
    if candidates
        .iter()
        .any(|candidate| candidate_cost(candidate).is_none())
    {
        return CollectedTitleEvidenceBatch {
            entries: Vec::new(),
            _storage: None,
        };
    }
    let output_bytes = candidates
        .iter()
        .map(|candidate| {
            candidate_cost(candidate)
                .unwrap_or_default()
                .saturating_add(std::mem::size_of::<CollectedTitleEvidence>())
        })
        .sum::<usize>()
        .max(1);
    let output_storage = match execution.reserve_storage(output_bytes).await {
        Ok(storage) => storage,
        Err(error) => {
            tracing::warn!(?error, "title evidence result storage admission failed");
            return CollectedTitleEvidenceBatch {
                entries: Vec::new(),
                _storage: None,
            };
        }
    };

    let mut entries = Vec::with_capacity(candidates.len());
    let mut candidates = candidates.into_iter();
    loop {
        let batch = candidates
            .by_ref()
            .take(TITLE_EVIDENCE_JOB_CANDIDATES)
            .collect::<Vec<_>>();
        if batch.is_empty() {
            break;
        }
        let metadata_bytes = batch
            .iter()
            .map(|candidate| candidate_cost(candidate).expect("validated candidate"))
            .sum::<usize>();
        let transcript_bytes = batch.len().saturating_mul(
            crate::session_id::TRANSCRIPT_READ_LIMITS
                .total_read_bytes
                .saturating_add(crate::session_id::TRANSCRIPT_READ_LIMITS.retained_path_bytes),
        );
        let reservation = match execution
            .reserve(
                Lane::Io,
                JobCost {
                    input_bytes: metadata_bytes.saturating_add(transcript_bytes).max(1),
                    result_bytes: metadata_bytes
                        .saturating_add(
                            batch
                                .len()
                                .saturating_mul(std::mem::size_of::<CollectedTitleEvidence>()),
                        )
                        .max(1),
                },
            )
            .await
        {
            Ok(reservation) => reservation,
            Err(error) => {
                tracing::warn!(?error, "title transcript evidence admission failed");
                append_unavailable(&mut entries, batch);
                append_unavailable(&mut entries, candidates.collect());
                return CollectedTitleEvidenceBatch {
                    entries,
                    _storage: Some(output_storage),
                };
            }
        };
        let job_home = home_dir.clone();
        let result = execution
            .run_reserved(reservation, move |context: ilium_execution::JobContext| {
                let mut collected = Vec::with_capacity(batch.len());
                for candidate in batch {
                    if context.stop_requested() {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Interrupted,
                            "title transcript collection cancelled",
                        ));
                    }
                    let transcript = collect_one(&job_home, &candidate);
                    collected.push(CollectedTitleEvidence {
                        candidate,
                        transcript,
                    });
                }
                Ok::<_, std::io::Error>(collected)
            })
            .await;
        match result {
            Ok(retained) => {
                let (mut batch_entries, retention) = retained.into_parts();
                entries.append(&mut batch_entries);
                drop(retention);
            }
            Err(error) => {
                tracing::warn!(%error, "title transcript evidence worker failed");
                return CollectedTitleEvidenceBatch {
                    entries: Vec::new(),
                    _storage: None,
                };
            }
        }
    }
    CollectedTitleEvidenceBatch {
        entries,
        _storage: Some(output_storage),
    }
}

fn candidate_cost(candidate: &TitleEvidenceCandidate) -> Option<usize> {
    let mut bytes = std::mem::size_of::<TitleEvidenceCandidate>()
        .checked_add(candidate.project_cwd.as_os_str().as_encoded_bytes().len())?;
    bytes = bytes.checked_add(
        candidate
            .observation
            .session_id
            .as_ref()
            .map_or(0, String::len),
    )?;
    bytes = bytes.checked_add(
        candidate
            .observation
            .agent_class
            .as_ref()
            .and_then(agent_class_bytes)
            .unwrap_or(0),
    )?;
    if let Some(runtime) = &candidate.runtime {
        bytes = bytes.checked_add(
            runtime
                .observation
                .session_id
                .as_ref()
                .map_or(0, String::len),
        )?;
        if let Some(class) = &runtime.observation.agent_class {
            bytes = bytes.checked_add(agent_class_bytes(class)?)?;
        }
        if let Some(process) = &runtime.process_key {
            bytes = bytes.checked_add(agent_class_bytes(&process.class)?)?;
        }
    }
    (bytes <= MAX_TITLE_CANDIDATE_BYTES).then_some(bytes.max(1))
}

fn agent_class_bytes(class: &ilium_core::AgentClass) -> Option<usize> {
    match class {
        ilium_core::AgentClass::Other(name) => Some(name.len()),
        _ => Some(0),
    }
}

async fn unavailable_batch(
    execution: &ExecutionClient,
    candidates: Vec<TitleEvidenceCandidate>,
) -> CollectedTitleEvidenceBatch {
    if candidates.len() > MAX_TITLE_EVIDENCE_CANDIDATES + 1 {
        return CollectedTitleEvidenceBatch {
            entries: Vec::new(),
            _storage: None,
        };
    }
    let Some(output_bytes) = candidates.iter().try_fold(0_usize, |total, candidate| {
        total.checked_add(
            candidate_cost(candidate)?.checked_add(std::mem::size_of::<CollectedTitleEvidence>())?,
        )
    }) else {
        return CollectedTitleEvidenceBatch {
            entries: Vec::new(),
            _storage: None,
        };
    };
    let storage = match execution.reserve_storage(output_bytes.max(1)).await {
        Ok(storage) => storage,
        Err(error) => {
            tracing::warn!(?error, "title fallback result storage admission failed");
            return CollectedTitleEvidenceBatch {
                entries: Vec::new(),
                _storage: None,
            };
        }
    };
    let mut entries = Vec::with_capacity(candidates.len());
    append_unavailable(&mut entries, candidates);
    CollectedTitleEvidenceBatch {
        entries,
        _storage: Some(storage),
    }
}

fn append_unavailable(
    entries: &mut Vec<CollectedTitleEvidence>,
    candidates: Vec<TitleEvidenceCandidate>,
) {
    entries.extend(
        candidates
            .into_iter()
            .map(|candidate| CollectedTitleEvidence {
                candidate,
                transcript: GenuineRequestEvidence::Unavailable,
            }),
    );
}

fn collect_one(
    home_dir: &std::path::Path,
    candidate: &TitleEvidenceCandidate,
) -> GenuineRequestEvidence {
    let Some(runtime) = candidate.runtime.as_ref() else {
        return GenuineRequestEvidence::Unavailable;
    };
    if runtime.observation != candidate.observation
        || runtime.kind != TerminalTitleKind::Agent
        || runtime.identity_invalidated
    {
        return GenuineRequestEvidence::Unavailable;
    }
    let (Some(class), Some(session_id)) = (
        runtime.observation.agent_class.as_ref(),
        runtime.observation.session_id.as_deref(),
    ) else {
        return GenuineRequestEvidence::Unavailable;
    };
    let Ok(project_cwd) = candidate.project_cwd.canonicalize() else {
        return GenuineRequestEvidence::Unavailable;
    };
    TranscriptLocator::new_bounded(
        home_dir,
        &project_cwd,
        crate::session_id::TRANSCRIPT_READ_LIMITS,
    )
        .genuine_request_evidence(class, session_id)
        .unwrap_or_else(|error| {
            tracing::debug!(pane_id = candidate.observation.pane_id.0, %error, "title eligibility history unavailable");
            GenuineRequestEvidence::Unavailable
        })
}

/// Pure final check while holding tree then panes. Rebuild `current` from the
/// locked runtime; never reuse the collection snapshot as current state.
/// Positive history is a historical fact bound to this invocation, not an
/// empty-history reset instruction. No filesystem operation runs here.
pub(crate) fn accepted_title_grant(
    node: &Node,
    current: Option<&TitleRuntimeSnapshot>,
    evidence: &CollectedTitleEvidence,
    receipt: Option<&AuthoredRequestReceipt>,
) -> Option<NodePresentationRevision> {
    let observed = &evidence.candidate.observation;
    if node.id != observed.pane_id
        || node.presentation_revision != observed.presentation_revision
        || node.is_name_fixed
        || matches!(
            &node.kind,
            NodeKind::Pane {
                title_source: PaneTitleSource::UserSpecified,
                ..
            }
        )
    {
        return None;
    }
    match &node.kind {
        NodeKind::Folder { .. }
        | NodeKind::Pane {
            content: PaneContentKind::Editor | PaneContentKind::Board,
            ..
        } => {}
        NodeKind::Pane {
            content: PaneContentKind::Terminal,
            ..
        } => {
            let current = current?;
            let captured = evidence.candidate.runtime.as_ref()?;
            let same_invocation = captured.observation == current.observation
                && captured.pty_identity == current.pty_identity
                && captured.agent_generation == current.agent_generation
                && captured.process_key == current.process_key
                && captured.identity_invalidated == current.identity_invalidated;
            // Collection intentionally did not run a native shell probe. A
            // recent separate observation may refine Unresolved to confirmed
            // shell, but may not change any other invocation evidence.
            let same_kind = captured.kind == current.kind
                || (captured.kind == TerminalTitleKind::Unresolved
                    && current.kind == TerminalTitleKind::ConfirmedPlainShell);
            if current.observation != *observed
                || !same_invocation
                || !same_kind
                || current.identity_invalidated
            {
                return None;
            }
            match current.kind {
                TerminalTitleKind::ConfirmedPlainShell => {}
                TerminalTitleKind::Agent => {
                    let process_key = current.process_key.as_ref()?;
                    if current.observation.agent_class.as_ref() != Some(&process_key.class)
                        || current.observation.process_id != Some(process_key.process_id)
                        || (!matches!(evidence.transcript, GenuineRequestEvidence::Present { .. })
                            && !receipt.is_some_and(|receipt| receipt.matches(current)))
                    {
                        return None;
                    }
                }
                TerminalTitleKind::Unresolved => return None,
            }
        }
        NodeKind::Container(_) => return None,
    }
    Some(NodePresentationRevision {
        node_id: node.id,
        revision: node.presentation_revision,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::{AgentClass, Tree, ROOT_ID};

    fn fixture() -> (Node, TitleRuntimeSnapshot, CollectedTitleEvidence) {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "test").unwrap();
        let pane = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        let node = tree.get(pane).unwrap().clone();
        let runtime = TitleRuntimeSnapshot {
            pty_identity: ilium_pty::PtySessionIdentity::default(),
            observation: PaneTitleObservation {
                pane_id: pane,
                presentation_revision: node.presentation_revision,
                agent_class: Some(AgentClass::Codex),
                session_id: Some("session".into()),
                process_id: Some(42),
                title_generation: 3,
            },
            agent_generation: 5,
            process_key: Some(AgentProcessKey {
                class: AgentClass::Codex,
                process_id: 42,
                started_at_unix_seconds: 10,
            }),
            kind: TerminalTitleKind::Agent,
            identity_invalidated: false,
        };
        let evidence = CollectedTitleEvidence {
            candidate: TitleEvidenceCandidate {
                observation: runtime.observation.clone(),
                project_cwd: PathBuf::from("/unused"),
                runtime: Some(runtime.clone()),
            },
            transcript: GenuineRequestEvidence::Present {
                last_record_end: 100,
            },
        };
        (node, runtime, evidence)
    }

    #[tokio::test]
    async fn over_limit_transcript_batch_fails_closed_without_partial_authorization() {
        const SESSION_ID: &str = "22222222-2222-4222-8222-222222222222";
        const MAX_TITLE_EVIDENCE_CANDIDATES: usize = 64;

        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let transcript_dir = home.path().join(".codex/sessions/2026/10/08");
        std::fs::create_dir_all(&transcript_dir).unwrap();
        let metadata = serde_json::json!({
            "type": "session_meta",
            "payload": {"id": SESSION_ID, "cwd": project},
        });
        let authored = serde_json::json!({
            "type": "event_msg",
            "payload": {"type": "user_message", "message": "rename this terminal"},
        });
        std::fs::write(
            transcript_dir.join(format!("rollout-2026-10-08T00-00-00-{SESSION_ID}.jsonl")),
            format!("{metadata}\n{authored}\n"),
        )
        .unwrap();

        let (_, runtime, mut evidence) = fixture();
        let mut runtime = runtime;
        runtime.observation.session_id = Some(SESSION_ID.to_owned());
        evidence.candidate.observation = runtime.observation.clone();
        evidence.candidate.project_cwd = project;
        evidence.candidate.runtime = Some(runtime);
        let candidates = vec![evidence.candidate; MAX_TITLE_EVIDENCE_CANDIDATES + 1];

        let execution = crate::execution::test_general_client();
        let collected =
            collect_title_evidence(Some(&execution), home.path().to_path_buf(), candidates).await;

        assert_eq!(collected.len(), MAX_TITLE_EVIDENCE_CANDIDATES + 1);
        assert!(collected
            .iter()
            .all(|evidence| evidence.transcript == GenuineRequestEvidence::Unavailable));
    }

    #[tokio::test]
    async fn oversized_transcript_line_fails_closed_without_partial_authorization() {
        const SESSION_ID: &str = "33333333-3333-4333-8333-333333333333";
        const MAX_LINE_BYTES: usize = 1024 * 1024;

        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let transcript_dir = home.path().join(".codex/sessions/2026/10/08");
        std::fs::create_dir_all(&transcript_dir).unwrap();
        let metadata = serde_json::json!({
            "type": "session_meta",
            "payload": {"id": SESSION_ID, "cwd": project},
        });
        let authored = serde_json::json!({
            "type": "event_msg",
            "payload": {"type": "user_message", "message": "rename this terminal"},
        });
        let oversized = serde_json::json!({
            "type": "event_msg",
            "payload": {
                "type": "user_message",
                "message": "x".repeat(MAX_LINE_BYTES),
            },
        });
        let path = transcript_dir.join(format!("rollout-2026-10-08T00-00-00-{SESSION_ID}.jsonl"));
        std::fs::write(
            path,
            format!(
                "{}\n{}\n{}\n",
                serde_json::to_string(&metadata).unwrap(),
                serde_json::to_string(&authored).unwrap(),
                serde_json::to_string(&oversized).unwrap(),
            ),
        )
        .unwrap();

        let (_, runtime, mut evidence) = fixture();
        let mut runtime = runtime;
        runtime.observation.session_id = Some(SESSION_ID.to_owned());
        evidence.candidate.observation = runtime.observation.clone();
        evidence.candidate.project_cwd = project;
        evidence.candidate.runtime = Some(runtime);

        let execution = crate::execution::test_general_client();
        let collected = collect_title_evidence(
            Some(&execution),
            home.path().to_path_buf(),
            vec![evidence.candidate],
        )
        .await;

        assert_eq!(collected.len(), 1);
        assert_eq!(
            collected[0].transcript,
            GenuineRequestEvidence::Unavailable,
            "an oversized real transcript must not authorize from its readable prefix",
        );
    }

    #[test]
    fn positive_own_history_grants_but_all_identity_changes_decline() {
        let (node, runtime, evidence) = fixture();
        assert!(accepted_title_grant(&node, Some(&runtime), &evidence, None).is_some());
        let mut variants = vec![runtime.clone(); 7];
        variants[0].observation.session_id = Some("other".into());
        variants[1].observation.process_id = Some(43);
        variants[2].observation.agent_class = Some(AgentClass::Claude);
        variants[3].observation.title_generation += 1;
        variants[4].agent_generation += 1;
        variants[5]
            .process_key
            .as_mut()
            .unwrap()
            .started_at_unix_seconds += 1;
        variants[6].identity_invalidated = true;
        variants.push(runtime.clone());
        variants[7].pty_identity = ilium_pty::PtySessionIdentity::default();
        for changed in variants {
            assert!(accepted_title_grant(&node, Some(&changed), &evidence, None).is_none());
        }
    }

    #[test]
    fn fixed_name_and_newer_presentation_win_over_positive_history() {
        let (mut node, runtime, evidence) = fixture();
        node.is_name_fixed = true;
        assert!(accepted_title_grant(&node, Some(&runtime), &evidence, None).is_none());
        node.is_name_fixed = false;
        node.presentation_revision += 1;
        assert!(accepted_title_grant(&node, Some(&runtime), &evidence, None).is_none());
    }

    #[test]
    fn missing_and_empty_history_need_exact_authored_delivery_receipt() {
        let (node, runtime, mut evidence) = fixture();
        let receipt = AuthoredRequestReceipt::for_invocation(&runtime).unwrap();
        for absent in [
            GenuineRequestEvidence::Unavailable,
            GenuineRequestEvidence::VerifiedEmpty,
        ] {
            evidence.transcript = absent;
            assert!(accepted_title_grant(&node, Some(&runtime), &evidence, None).is_none());
            assert!(
                accepted_title_grant(&node, Some(&runtime), &evidence, Some(&receipt)).is_some()
            );
            let mut stale = receipt.clone();
            stale.agent_generation += 1;
            assert!(accepted_title_grant(&node, Some(&runtime), &evidence, Some(&stale)).is_none());
        }
    }

    #[test]
    fn unresolved_terminal_declines_even_with_a_receipt() {
        let (node, mut runtime, mut evidence) = fixture();
        runtime.kind = TerminalTitleKind::Unresolved;
        evidence.candidate.runtime = Some(runtime.clone());
        let receipt = AuthoredRequestReceipt {
            agent_generation: runtime.agent_generation,
            process_key: runtime.process_key.clone().unwrap(),
            title_generation: runtime.observation.title_generation,
            session_id: runtime.observation.session_id.clone(),
        };
        assert!(accepted_title_grant(&node, Some(&runtime), &evidence, Some(&receipt)).is_none());
    }

    #[test]
    fn receipt_before_session_discovery_binds_the_exact_invocation() {
        let (_, mut runtime, _) = fixture();
        runtime.observation.session_id = None;
        let receipt = AuthoredRequestReceipt::for_invocation(&runtime).unwrap();
        runtime.observation.session_id = Some("discovered".into());
        runtime.observation.presentation_revision += 1;
        assert!(receipt.matches(&runtime));
        runtime.observation.title_generation += 1;
        assert!(!receipt.matches(&runtime));
    }

    #[test]
    fn shell_grants_need_current_and_captured_confirmation() {
        let (node, mut runtime, mut evidence) = fixture();
        runtime.kind = TerminalTitleKind::ConfirmedPlainShell;
        runtime.observation.agent_class = None;
        runtime.observation.session_id = None;
        runtime.observation.process_id = None;
        runtime.process_key = None;
        evidence.candidate.observation = runtime.observation.clone();
        evidence.candidate.runtime = Some(runtime.clone());
        evidence.transcript = GenuineRequestEvidence::Unavailable;
        assert!(accepted_title_grant(&node, Some(&runtime), &evidence, None).is_some());
        runtime.kind = TerminalTitleKind::Unresolved;
        assert!(accepted_title_grant(&node, Some(&runtime), &evidence, None).is_none());
    }

    #[test]
    fn fresh_shell_confirmation_only_refines_the_same_invocation() {
        let (node, mut current, mut evidence) = fixture();
        current.kind = TerminalTitleKind::ConfirmedPlainShell;
        current.observation.agent_class = None;
        current.observation.session_id = None;
        current.observation.process_id = None;
        current.process_key = None;
        let mut captured = current.clone();
        captured.kind = TerminalTitleKind::Unresolved;
        evidence.candidate.observation = captured.observation.clone();
        evidence.candidate.runtime = Some(captured);
        evidence.transcript = GenuineRequestEvidence::Unavailable;
        assert!(accepted_title_grant(&node, Some(&current), &evidence, None).is_some());
        current.pty_identity = ilium_pty::PtySessionIdentity::default();
        assert!(accepted_title_grant(&node, Some(&current), &evidence, None).is_none());
        current.pty_identity = evidence
            .candidate
            .runtime
            .as_ref()
            .unwrap()
            .pty_identity
            .clone();
        current.agent_generation += 1;
        assert!(accepted_title_grant(&node, Some(&current), &evidence, None).is_none());
    }

    #[test]
    fn non_terminal_presentation_compares_revision_and_fixed_ownership() {
        let (mut node, _, mut evidence) = fixture();
        node.kind = NodeKind::Folder {
            path: PathBuf::from("/unused"),
            expanded: false,
            locked_closed: false,
        };
        evidence.candidate.runtime = None;
        evidence.transcript = GenuineRequestEvidence::Unavailable;
        assert!(accepted_title_grant(&node, None, &evidence, None).is_some());
        node.is_name_fixed = true;
        assert!(accepted_title_grant(&node, None, &evidence, None).is_none());
        node.is_name_fixed = false;
        node.presentation_revision += 1;
        assert!(accepted_title_grant(&node, None, &evidence, None).is_none());
    }

    #[test]
    fn ambiguous_non_fixed_user_title_is_not_implicitly_repaired() {
        let (mut node, runtime, evidence) = fixture();
        if let NodeKind::Pane { title_source, .. } = &mut node.kind {
            *title_source = PaneTitleSource::UserSpecified;
        }
        assert!(accepted_title_grant(&node, Some(&runtime), &evidence, None).is_none());
    }
}
