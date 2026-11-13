//! Bounded reads of one verified Claude main-session transcript.
//!
//! The I/O job returns raw bytes; JSON parsing belongs on the CPU lane.

use ilium_agent_session::goal_evidence::{Freshness, GoalObservation, GoalPhase};
use ilium_agent_session::{
    goal_evidence::ClaudeGoalTranscript, TranscriptLocator, TranscriptReadLimits,
};
use ilium_core::AgentClass;
use ilium_core::NodeId;
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, StorageAdmission,
};
use ilium_platform::file_identity::{identity, FileIdentity};
use ilium_platform::secure_fs::open_regular_file_without_atime;
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use crate::session_stats_store::StatsRequest;

pub(crate) const CHUNK_BYTES: usize = 256 * 1024;
const BOUNDARY_BYTES: usize = 128;
pub(crate) const RECORD_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct Cursor {
    path: PathBuf,
    provider_homes: ilium_platform::process_info::AgentProviderHomes,
    identity: FileIdentity,
    offset: u64,
    observed_length: u64,
    modified: Option<SystemTime>,
    boundary: Vec<u8>,
}

pub(crate) struct ReadChunk {
    pub request: StatsRequest,
    pub process_id: u32,
    pub process_identity_inode: u64,
    pub transcript_path: PathBuf,
    pub cursor: Option<Cursor>,
    pub storage: Arc<StorageAdmission>,
}

pub(crate) struct Chunk {
    pub cursor: Cursor,
    pub bytes: Vec<u8>,
    pub reset: bool,
    pub caught_up: bool,
    pub storage: Arc<StorageAdmission>,
}

impl Job for ReadChunk {
    type Output = Chunk;
    type Error = &'static str;

    fn run(self, context: JobContext) -> Result<Chunk, Self::Error> {
        if context.stop_requested() {
            return Err("Transcript read cancelled");
        }
        let process = ilium_platform::process_control::capture_pty_process(self.process_id)
            .map_err(|_| "Agent process identity unavailable")?;
        if process.pidfs_inode() != Some(self.process_identity_inode) {
            return Err("Agent process identity changed");
        }
        let homes = ilium_platform::process_info::agent_provider_homes(self.process_id)
            .ok_or("Agent provider home is unavailable")?;
        let current_record = || {
            ilium_platform::claude_session_record::session_for_process(
                self.process_id,
                &process,
                &homes.claude_config,
                &self.request.project_path,
            )
        };
        let record = current_record();
        if record
            .as_deref()
            .is_some_and(|session_id| session_id != self.request.session_id)
        {
            return Err("Agent session changed");
        }
        let descriptor_matches = crate::session_goal_custody::only_transcript_open(
            self.process_id,
            &self.transcript_path,
        );
        let use_record = record.is_some();
        if !use_record && !descriptor_matches {
            return Err("Agent transcript ownership is unavailable or ambiguous");
        }
        let locator = TranscriptLocator::new_bounded(
            &homes.home,
            &self.request.project_path,
            TranscriptReadLimits {
                line_bytes: 1024 * 1024,
                total_read_bytes: 16 * 1024 * 1024,
                scanned_entries: 1_000_000,
                retained_path_bytes: 1024 * 1024,
            },
        )
        .with_claude_config_dir(&homes.claude_config)
        .without_access_time_updates();
        let transcript = locator
            .transcript_from_path(&AgentClass::Claude, &self.transcript_path)
            .filter(|transcript| transcript.session_id == self.request.session_id);
        if locator.read_limit_reached() {
            return Err("Transcript ownership could not be established within its limits");
        }
        let transcript = transcript.ok_or("No verified Claude main transcript")?;
        let mut file = open_regular_file_without_atime(&transcript.path)
            .map_err(|_| "Transcript unavailable")?;
        if !use_record
            && !ilium_platform::process_info::process_has_open_file(self.process_id, &file, 4096)
                .map_err(|_| "Agent transcript identity unavailable")?
        {
            return Err("Agent no longer holds this transcript file");
        }
        let metadata = file
            .metadata()
            .map_err(|_| "Transcript metadata unavailable")?;
        let file_identity = identity(&file).map_err(|_| "Transcript identity unavailable")?;
        let length = metadata.len();
        let modified = metadata.modified().ok();
        let mut reset = true;
        let mut offset = 0;
        if let Some(previous) = self.cursor.filter(|cursor| {
            cursor.provider_homes == homes
                && cursor.path == transcript.path
                && cursor.identity == file_identity
                && cursor.offset <= length
                && cursor.observed_length <= length
        }) {
            if previous.observed_length != length || previous.modified == modified {
                let boundary_start = previous
                    .offset
                    .saturating_sub(previous.boundary.len() as u64);
                file.seek(SeekFrom::Start(boundary_start))
                    .map_err(|_| "Transcript seek failed")?;
                let mut boundary = vec![0; previous.boundary.len()];
                file.read_exact(&mut boundary)
                    .map_err(|_| "Transcript boundary unavailable")?;
                if boundary == previous.boundary {
                    reset = false;
                    offset = previous.offset;
                }
            }
        }
        let count = (length - offset).min(CHUNK_BYTES as u64) as usize;
        file.seek(SeekFrom::Start(offset))
            .map_err(|_| "Transcript seek failed")?;
        let mut bytes = vec![0; count];
        file.read_exact(&mut bytes)
            .map_err(|_| "Transcript changed while reading")?;
        let next_offset = offset + count as u64;
        let boundary_start = next_offset.saturating_sub(BOUNDARY_BYTES as u64);
        file.seek(SeekFrom::Start(boundary_start))
            .map_err(|_| "Transcript seek failed")?;
        let mut boundary = vec![0; (next_offset - boundary_start) as usize];
        file.read_exact(&mut boundary)
            .map_err(|_| "Transcript changed while reading")?;
        let after = file
            .metadata()
            .map_err(|_| "Transcript metadata unavailable")?;
        if after.len() != length || after.modified().ok() != modified {
            return Err("Transcript changed while reading");
        }
        let verified = locator
            .transcript_from_path(&AgentClass::Claude, &transcript.path)
            .is_some_and(|verified| verified.session_id == self.request.session_id);
        let reopened = open_regular_file_without_atime(&transcript.path)
            .map_err(|_| "Transcript disappeared")?;
        if !verified
            || locator.read_limit_reached()
            || identity(&reopened).ok() != Some(file_identity)
            || if use_record {
                current_record().as_deref() != Some(self.request.session_id.as_str())
            } else {
                current_record()
                    .as_deref()
                    .is_some_and(|session_id| session_id != self.request.session_id)
                    || !crate::session_goal_custody::only_transcript_open(
                        self.process_id,
                        &transcript.path,
                    )
                    || !ilium_platform::process_info::process_has_open_file(
                        self.process_id,
                        &file,
                        4096,
                    )
                    .unwrap_or(false)
            }
            || !ilium_platform::process_control::captured_process_is_current(&process)
                .unwrap_or(false)
            || ilium_platform::process_info::agent_provider_homes(self.process_id).as_ref()
                != Some(&homes)
        {
            return Err("Transcript identity changed while reading");
        }
        if context.stop_requested() {
            return Err("Transcript read cancelled");
        }
        Ok(Chunk {
            cursor: Cursor {
                path: transcript.path,
                provider_homes: homes,
                identity: file_identity,
                offset: next_offset,
                observed_length: length,
                modified,
                boundary,
            },
            bytes,
            reset,
            caught_up: next_offset == length,
            storage: self.storage,
        })
    }
}

pub(crate) struct ParseChunk {
    pub chunk: Chunk,
    pub transcript: ClaudeGoalTranscript,
    pub previous_storage: Arc<StorageAdmission>,
}

pub(crate) struct ParsedChunk {
    pub cursor: Cursor,
    pub transcript: ClaudeGoalTranscript,
    pub caught_up: bool,
    pub _storage: Arc<StorageAdmission>,
    pub _previous_storage: Arc<StorageAdmission>,
}

impl Job for ParseChunk {
    type Output = ParsedChunk;
    type Error = &'static str;

    fn run(self, context: JobContext) -> Result<ParsedChunk, Self::Error> {
        if context.stop_requested() {
            return Err("Goal parsing cancelled");
        }
        let mut transcript = if self.chunk.reset {
            ClaudeGoalTranscript::default()
        } else {
            self.transcript
        };
        transcript
            .push(&self.chunk.bytes, RECORD_LIMIT)
            .map_err(|_| "Goal transcript record exceeds the 8 MiB bound")?;
        if context.stop_requested() {
            return Err("Goal parsing cancelled");
        }
        Ok(ParsedChunk {
            cursor: self.chunk.cursor,
            transcript,
            caught_up: self.chunk.caught_up,
            _storage: self.chunk.storage,
            _previous_storage: self.previous_storage,
        })
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ClaudeContext {
    pub request: StatsRequest,
    pub process_id: u32,
    pub process_identity_inode: u64,
    pub transcript_path: PathBuf,
    pub identity_generation: u64,
    pub screen: Option<GoalPhase>,
}

impl ClaudeContext {
    fn same_identity(&self, other: &Self) -> bool {
        self.request == other.request
            && self.process_id == other.process_id
            && self.process_identity_inode == other.process_identity_inode
            && self.transcript_path == other.transcript_path
            && self.identity_generation == other.identity_generation
    }
}

struct Entry {
    context: ClaudeContext,
    generation: u64,
    cursor: Option<Cursor>,
    transcript: Option<ClaudeGoalTranscript>,
    observation: GoalObservation,
    due: Instant,
    storage: Arc<StorageAdmission>,
}

struct Owner {
    pane: NodeId,
    generation: u64,
}
enum Pending {
    Read(Owner, Box<Receipt<ReadChunk>>),
    ParseReady(Owner, Box<ParseChunk>),
    Parse(Owner, Box<Receipt<ParseChunk>>),
}
impl Pending {
    fn owner(&self) -> &Owner {
        match self {
            Self::Read(owner, _) | Self::ParseReady(owner, _) | Self::Parse(owner, _) => owner,
        }
    }
    fn cancel(&self) {
        match self {
            Self::Read(_, receipt) => receipt.cancel(),
            Self::Parse(_, receipt) => receipt.cancel(),
            Self::ParseReady(_, _) => {}
        }
    }
}

#[derive(Default)]
pub(crate) struct ClaudeGoalStore {
    client: Option<Client>,
    entries: BTreeMap<NodeId, Entry>,
    pending: Option<Pending>,
    generation: u64,
    last_scheduled: Option<NodeId>,
}

impl ClaudeGoalStore {
    /// Drop evidence and cancel any read belonging to a superseded binding.
    pub(crate) fn invalidate(&mut self, pane: NodeId) {
        self.entries.remove(&pane);
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.owner().pane == pane)
        {
            if let Some(pending) = self.pending.take() {
                pending.cancel();
            }
        }
    }

    pub(crate) fn configure_execution(&mut self, client: Client) {
        self.client = Some(client);
    }
    pub(crate) fn observation(
        &self,
        pane: NodeId,
        context: &ClaudeContext,
    ) -> Option<&GoalObservation> {
        let entry = self.entries.get(&pane)?;
        entry
            .context
            .same_identity(context)
            .then_some(&entry.observation)
    }
    fn live(&self, owner: &Owner) -> bool {
        self.entries
            .get(&owner.pane)
            .is_some_and(|entry| entry.generation == owner.generation)
    }
    pub(crate) fn tick(&mut self, contexts: BTreeMap<NodeId, ClaudeContext>, now: Instant) -> bool {
        let before = self.entries.len();
        self.entries.retain(|pane, entry| {
            contexts
                .get(pane)
                .is_some_and(|context| entry.context.same_identity(context))
        });
        let mut changed = before != self.entries.len();
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| !self.live(pending.owner()))
        {
            if let Some(pending) = self.pending.take() {
                pending.cancel();
            }
        }
        for (pane, context) in contexts {
            if let Some(entry) = self.entries.get_mut(&pane) {
                if entry.observation.phase != context.screen {
                    entry.observation.phase = context.screen;
                    changed = true;
                }
                entry.context.screen = context.screen;
                continue;
            }
            let bytes = context.request.session_id.capacity()
                + context.request.project_path.capacity()
                + context.request.home.capacity()
                + context.transcript_path.capacity()
                + 1024;
            let Ok(storage) = crate::execution::process_quota().reserve_external_storage(bytes)
            else {
                continue;
            };
            let Some(generation) = self.generation.checked_add(1) else {
                continue;
            };
            self.generation = generation;
            let observation = GoalObservation {
                phase: context.screen,
                objective: None,
                freshness: Freshness::Stale,
                absent: false,
            };
            self.entries.insert(
                pane,
                Entry {
                    context,
                    generation,
                    cursor: None,
                    transcript: Some(ClaudeGoalTranscript::default()),
                    observation,
                    due: now,
                    storage: Arc::new(storage),
                },
            );
            changed = true;
        }
        changed |= self.collect(now);
        if self.pending.is_none() {
            changed |= self.schedule(now);
        }
        changed
    }

    fn fail(&mut self, owner: &Owner, now: Instant) -> bool {
        let Some(entry) = self
            .entries
            .get_mut(&owner.pane)
            .filter(|entry| entry.generation == owner.generation)
        else {
            return false;
        };
        // Any unreadable or ambiguous transcript invalidates cached text. A
        // retry starts at the beginning instead of pretending it saw clear.
        entry.cursor = None;
        entry.transcript = Some(ClaudeGoalTranscript::default());
        entry.observation.objective = None;
        entry.observation.freshness = Freshness::Stale;
        entry.due = now + Duration::from_secs(3);
        true
    }

    fn schedule(&mut self, now: Instant) -> bool {
        let Some(client) = self.client.as_ref() else {
            return false;
        };
        let pane = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.due <= now && entry.transcript.is_some())
            .find(|(pane, _)| self.last_scheduled.is_none_or(|last| **pane > last))
            .or_else(|| {
                self.entries
                    .iter()
                    .find(|(_, entry)| entry.due <= now && entry.transcript.is_some())
            })
            .map(|(pane, _)| *pane);
        let Some(pane) = pane else {
            return false;
        };
        self.last_scheduled = Some(pane);
        let Some(entry) = self.entries.get_mut(&pane) else {
            return false;
        };
        entry.due = now + Duration::from_secs(3);
        let Ok(storage) =
            crate::execution::process_quota().reserve_external_storage(RECORD_LIMIT * 6)
        else {
            entry.observation.objective = None;
            entry.observation.freshness = Freshness::Stale;
            return true;
        };
        let owner = Owner {
            pane,
            generation: entry.generation,
        };
        let job = ReadChunk {
            request: entry.context.request.clone(),
            process_id: entry.context.process_id,
            process_identity_inode: entry.context.process_identity_inode,
            transcript_path: entry.context.transcript_path.clone(),
            cursor: entry.cursor.clone(),
            storage: Arc::new(storage),
        };
        if let Ok(receipt) = client.try_submit(
            Lane::Io,
            JobCost {
                input_bytes: CHUNK_BYTES * 2,
                result_bytes: CHUNK_BYTES * 2,
            },
            job,
        ) {
            self.pending = Some(Pending::Read(owner, Box::new(receipt)));
            false
        } else {
            entry.observation.objective = None;
            entry.observation.freshness = Freshness::Stale;
            true
        }
    }

    fn collect(&mut self, now: Instant) -> bool {
        let Some(pending) = self.pending.take() else {
            return false;
        };
        match pending {
            Pending::Read(owner, mut receipt) => match receipt.try_take() {
                JobPoll::Pending => self.pending = Some(Pending::Read(owner, receipt)),
                JobPoll::Ready(outcome) => {
                    let (outcome, hold) = outcome.into_parts();
                    if !self.live(&owner) {
                        return false;
                    }
                    match outcome {
                        JobOutcome::Finished(Ok(chunk)) => {
                            let Some(entry) = self.entries.get_mut(&owner.pane) else {
                                return false;
                            };
                            if chunk.reset {
                                entry.observation.objective = None;
                                entry.observation.freshness = Freshness::Stale;
                            }
                            let Some(transcript) = entry.transcript.take() else {
                                return self.fail(&owner, now);
                            };
                            self.pending = Some(Pending::ParseReady(
                                owner,
                                Box::new(ParseChunk {
                                    chunk,
                                    transcript,
                                    previous_storage: Arc::clone(&entry.storage),
                                }),
                            ));
                        }
                        _ => {
                            drop(hold);
                            return self.fail(&owner, now);
                        }
                    }
                    drop(hold);
                }
                _ => return self.fail(&owner, now),
            },
            Pending::ParseReady(owner, job) => {
                let Some(client) = self.client.as_ref() else {
                    return self.fail(&owner, now);
                };
                match client.try_submit(
                    Lane::Cpu,
                    JobCost {
                        input_bytes: RECORD_LIMIT * 3,
                        result_bytes: RECORD_LIMIT * 3,
                    },
                    *job,
                ) {
                    Ok(receipt) => self.pending = Some(Pending::Parse(owner, Box::new(receipt))),
                    Err(rejected) if rejected.reason == ilium_execution::RejectReason::Busy => {
                        self.pending = Some(Pending::ParseReady(owner, Box::new(rejected.value)))
                    }
                    Err(_) => return self.fail(&owner, now),
                }
            }
            Pending::Parse(owner, mut receipt) => match receipt.try_take() {
                JobPoll::Pending => self.pending = Some(Pending::Parse(owner, receipt)),
                JobPoll::Ready(outcome) => {
                    let (outcome, hold) = outcome.into_parts();
                    if !self.live(&owner) {
                        return false;
                    }
                    match outcome {
                        JobOutcome::Finished(Ok(parsed)) => {
                            let Some(entry) = self.entries.get_mut(&owner.pane) else {
                                return false;
                            };
                            let cursor_bytes = parsed.cursor.path.capacity()
                                + parsed.cursor.provider_homes.home.capacity()
                                + parsed.cursor.provider_homes.claude_config.capacity()
                                + parsed.cursor.provider_homes.codex_home.capacity()
                                + parsed.cursor.boundary.capacity();
                            let bytes = parsed
                                .transcript
                                .retained_bytes()
                                .saturating_mul(3)
                                .saturating_add(
                                    entry.context.request.session_id.capacity()
                                        + entry.context.request.project_path.capacity()
                                        + entry.context.request.home.capacity()
                                        + entry.context.transcript_path.capacity()
                                        + cursor_bytes
                                        + 1024,
                                );
                            let Ok(storage) =
                                crate::execution::process_quota().reserve_external_storage(bytes)
                            else {
                                drop(hold);
                                return self.fail(&owner, now);
                            };
                            entry.observation.objective =
                                if parsed.caught_up && !parsed.transcript.has_partial_record() {
                                    parsed.transcript.evidence().objective().map(str::to_owned)
                                } else {
                                    None
                                };
                            entry.observation.freshness =
                                if parsed.caught_up && !parsed.transcript.has_partial_record() {
                                    Freshness::Current
                                } else {
                                    Freshness::Stale
                                };
                            entry.cursor = Some(parsed.cursor);
                            entry.transcript = Some(parsed.transcript);
                            entry.storage = Arc::new(storage);
                            entry.due = if parsed.caught_up {
                                now + Duration::from_secs(3)
                            } else {
                                now
                            };
                            drop(hold);
                            return true;
                        }
                        _ => {
                            drop(hold);
                            return self.fail(&owner, now);
                        }
                    }
                }
                _ => return self.fail(&owner, now),
            },
        }
        false
    }
}

impl Drop for ClaudeGoalStore {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.take() {
            pending.cancel();
        }
    }
}

impl crate::app::App {
    fn claude_goal_context(&self, pane: NodeId) -> Option<ClaudeContext> {
        use ilium_core::{GoalState, NodeKind, PaneStatus};
        let node = self.tree.get(pane)?;
        let NodeKind::Pane {
            status: PaneStatus::Agent(agent),
            ..
        } = &node.kind
        else {
            return None;
        };
        if agent.class != AgentClass::Claude {
            return None;
        }
        let (class, session_id, project_path) = self.known_agent_history_context(pane)?;
        if class != AgentClass::Claude {
            return None;
        }
        let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
        let process_id = *self.agent_process_ids.get(&pane)?;
        let process_identity_inode = *self.agent_process_identity_inode.get(&pane)?;
        let transcript_path = self.agent_transcript_paths.get(&pane)?.clone();
        let identity_generation = self
            .agent_title_generations
            .get(&pane)
            .copied()
            .filter(|value| *value != u64::MAX)?;
        let screen = agent.goal.map(|phase| match phase {
            GoalState::Active => GoalPhase::Active,
            GoalState::Paused => GoalPhase::Paused,
            GoalState::Blocked => GoalPhase::Blocked,
            GoalState::UsageLimited => GoalPhase::UsageLimited,
            GoalState::Reached => GoalPhase::Reached,
        });
        Some(ClaudeContext {
            request: StatsRequest {
                class,
                session_id,
                project_path,
                home,
            },
            process_id,
            process_identity_inode,
            transcript_path,
            identity_generation,
            screen,
        })
    }

    pub(crate) fn claude_goal_observation(&self, pane: NodeId) -> Option<&GoalObservation> {
        self.claude_goals
            .observation(pane, &self.claude_goal_context(pane)?)
    }

    pub(crate) fn tick_claude_goals(&mut self, now: Instant) -> bool {
        let mut contexts = BTreeMap::new();
        if self.ui_settings.goal_tab_enabled || self.ui_settings.goal_toolbar_icon_enabled {
            for node in self.tree.panes() {
                if let Some(context) = self.claude_goal_context(node.id) {
                    contexts.insert(node.id, context);
                }
            }
        }
        self.claude_goals.tick(contexts, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_changes_fence_cached_claude_text() {
        let original = ClaudeContext {
            request: StatsRequest {
                class: AgentClass::Claude,
                session_id: "session-a".into(),
                project_path: "/project-a".into(),
                home: "/home-a".into(),
            },
            process_id: 42,
            process_identity_inode: 123,
            transcript_path: "/home-a/transcript.jsonl".into(),
            identity_generation: 7,
            screen: Some(GoalPhase::Active),
        };
        let mut changed = original.clone();
        changed.screen = Some(GoalPhase::Paused);
        assert!(original.same_identity(&changed));
        changed = original.clone();
        changed.process_id += 1;
        assert!(!original.same_identity(&changed));
        changed = original.clone();
        changed.process_identity_inode += 1;
        assert!(!original.same_identity(&changed));
        changed = original.clone();
        changed.transcript_path = "/other/transcript.jsonl".into();
        assert!(!original.same_identity(&changed));
        changed = original.clone();
        changed.identity_generation += 1;
        assert!(!original.same_identity(&changed));
        changed = original.clone();
        changed.request.session_id = "session-b".into();
        assert!(!original.same_identity(&changed));
        changed = original.clone();
        changed.request.project_path = "/project-b".into();
        assert!(!original.same_identity(&changed));
        changed = original.clone();
        changed.request.home = "/home-b".into();
        assert!(!original.same_identity(&changed));
        changed = original.clone();
        changed.request.class = AgentClass::Codex;
        assert!(!original.same_identity(&changed));
    }
}
