//! Per-pane, generation-fenced goal observations, refreshed without a popover.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ilium_agent_session::goal_evidence::{
    GoalGeneration, GoalObservation, GoalPhase, GoalReconciler,
};
use ilium_core::NodeId;
use tokio::sync::oneshot;

use crate::session_goal_codex::{CodexGoalService, ProviderGoalResult, ProviderReply};

const REFRESH: Duration = Duration::from_secs(3);
// A queued or stalled reader must not leave its last result current forever.
const MAX_PROVIDER_AGE: Duration = Duration::from_secs(15);

fn provider_home_matches(
    previous: Option<&ilium_platform::process_info::AgentProviderHomes>,
    current: Option<&ilium_platform::process_info::AgentProviderHomes>,
) -> bool {
    current.is_some_and(|current| previous.is_none_or(|previous| previous == current))
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CodexContext {
    pub session_id: String,
    pub project: PathBuf,
    pub home: PathBuf,
    /// Exact-process transcript hint, if the server could verify one.
    pub transcript_path: Option<PathBuf>,
    pub process_id: u32,
    pub process_identity_inode: u64,
    pub identity_generation: u64,
    pub screen: Option<GoalPhase>,
}

impl CodexContext {
    fn same_identity(&self, other: &Self) -> bool {
        self.session_id == other.session_id
            && self.project == other.project
            && self.home == other.home
            && self.transcript_path == other.transcript_path
            && self.process_id == other.process_id
            && self.process_identity_inode == other.process_identity_inode
            && self.identity_generation == other.identity_generation
    }
}

struct Entry {
    context: CodexContext,
    generation: GoalGeneration,
    evidence: GoalReconciler,
    retained: Option<ilium_execution::StorageAdmission>,
    provider_homes: Option<ilium_platform::process_info::AgentProviderHomes>,
    valid_until: Option<Instant>,
    due: Instant,
    _storage: ilium_execution::StorageAdmission,
}

struct Pending {
    pane: NodeId,
    generation: GoalGeneration,
    started: Instant,
    receiver: oneshot::Receiver<ProviderReply>,
}

#[derive(Default)]
pub(crate) struct SessionGoalStore {
    service: Option<CodexGoalService>,
    entries: BTreeMap<NodeId, Entry>,
    pending: Option<Pending>,
    next_generation: u64,
    last_scheduled: Option<NodeId>,
    retry_service_at: Option<Instant>,
}

impl SessionGoalStore {
    /// A renewed server binding can represent a new process even when its
    /// numeric ID and session fields were reused.
    pub(crate) fn invalidate(&mut self, pane: NodeId) {
        self.entries.remove(&pane);
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.pane == pane)
        {
            self.pending = None;
        }
    }

    pub(crate) fn observation(
        &self,
        pane: NodeId,
        current: &CodexContext,
    ) -> Option<&GoalObservation> {
        let entry = self.entries.get(&pane)?;
        entry
            .context
            .same_identity(current)
            .then(|| entry.evidence.observation())
    }

    /// Reconcile identities before collecting work so even an ABA reuse is
    /// unable to accept a result from a previous observed generation.
    pub(crate) fn tick(&mut self, contexts: BTreeMap<NodeId, CodexContext>, now: Instant) -> bool {
        let previous_count = self.entries.len();
        self.entries.retain(|pane, entry| {
            contexts
                .get(pane)
                .is_some_and(|context| entry.context.same_identity(context))
        });
        let mut changed = previous_count != self.entries.len();
        if self.pending.as_ref().is_some_and(|pending| {
            !self
                .entries
                .get(&pending.pane)
                .is_some_and(|entry| entry.generation == pending.generation)
        }) {
            self.pending = None;
        }
        for (pane, context) in contexts {
            if let Some(entry) = self.entries.get_mut(&pane) {
                entry.context.screen = context.screen;
                if entry.valid_until.is_some_and(|deadline| now >= deadline) {
                    entry.valid_until = None;
                    changed |= entry
                        .evidence
                        .apply_codex(entry.generation, None, context.screen);
                }
                changed |= entry.evidence.update_screen(context.screen);
                continue;
            }
            let size = context.session_id.capacity()
                + context.project.capacity()
                + context.home.capacity()
                + context
                    .transcript_path
                    .as_ref()
                    .map_or(0, PathBuf::capacity)
                + 16 * 1024;
            let Ok(storage) = crate::execution::process_quota().reserve_external_storage(size)
            else {
                continue;
            };
            let Some(generation) = self.next_generation.checked_add(1) else {
                continue;
            };
            self.next_generation = generation;
            let generation = GoalGeneration(generation);
            let mut evidence = GoalReconciler::new(generation);
            evidence.apply_codex(generation, None, context.screen);
            self.entries.insert(
                pane,
                Entry {
                    context,
                    generation,
                    evidence,
                    retained: None,
                    provider_homes: None,
                    valid_until: None,
                    due: now,
                    _storage: storage,
                },
            );
            changed = true;
        }
        if let Some(mut pending) = self.pending.take() {
            match pending.receiver.try_recv() {
                Err(oneshot::error::TryRecvError::Empty) => self.pending = Some(pending),
                result => {
                    if let Some(entry) = self
                        .entries
                        .get_mut(&pending.pane)
                        .filter(|entry| entry.generation == pending.generation)
                    {
                        let reply = result.ok();
                        let homes = reply.as_ref().and_then(|value| value.homes.as_ref());
                        let home_matches =
                            provider_home_matches(entry.provider_homes.as_ref(), homes);
                        let binding_valid = reply.as_ref().is_some_and(|value| value.binding_valid);
                        if !home_matches || !binding_valid {
                            entry
                                .evidence
                                .invalidate_provider_identity(entry.context.screen);
                            entry.retained = None;
                        }
                        // A new home is not proof that the existing thread binding
                        // belongs to it. Reject its result until the pane receives
                        // a new verified identity and therefore a new entry.
                        if home_matches {
                            entry.provider_homes = homes.cloned();
                        }
                        let deadline = pending.started + MAX_PROVIDER_AGE;
                        let provider = if home_matches && binding_valid && now < deadline {
                            reply.and_then(|value| value.result)
                        } else {
                            None
                        };
                        // Keep the previous reservation until reconciliation has
                        // dropped the old objective. A failed lookup retains both.
                        let (lookup, incoming_storage) = match provider {
                            Some(ProviderGoalResult { lookup, storage }) => {
                                (Some(lookup), Some(storage))
                            }
                            None => (None, None),
                        };
                        entry.valid_until = lookup.as_ref().map(|_| deadline);
                        entry
                            .evidence
                            .apply_codex(entry.generation, lookup, entry.context.screen);
                        if let Some(storage) = incoming_storage {
                            entry.retained = Some(storage);
                        }
                        entry.due = now + REFRESH;
                        changed = true;
                    }
                }
            }
        }
        if self.entries.is_empty() {
            self.pending = None;
            self.service = None;
            return changed;
        }
        if self
            .service
            .as_ref()
            .is_some_and(CodexGoalService::is_closed)
        {
            self.service = None;
        }
        if self.service.is_none() && self.retry_service_at.is_none_or(|due| now >= due) {
            self.service = CodexGoalService::start();
            self.retry_service_at = Some(now + REFRESH);
        }
        if self.pending.is_none() {
            let eligible = |entry: &&Entry| entry.due <= now;
            let pane = self
                .entries
                .iter()
                .filter(|(_, entry)| eligible(entry))
                .find(|(pane, _)| self.last_scheduled.is_none_or(|last| **pane > last))
                .or_else(|| self.entries.iter().find(|(_, entry)| entry.due <= now))
                .map(|(pane, _)| *pane);
            if let Some(pane) = pane {
                self.last_scheduled = Some(pane);
                if let Some(entry) = self.entries.get_mut(&pane) {
                    entry.due = now + REFRESH;
                    if let Some(receiver) = self.service.as_ref().and_then(|service| {
                        service.try_get(
                            entry.context.session_id.clone(),
                            entry.context.process_id,
                            entry.context.process_identity_inode,
                            entry.context.project.clone(),
                            entry.context.transcript_path.clone()?,
                        )
                    }) {
                        self.pending = Some(Pending {
                            pane,
                            generation: entry.generation,
                            started: now,
                            receiver,
                        });
                    } else {
                        // Queue admission failure is not evidence that the
                        // provider home changed. Preserve the previously
                        // verified text as stale until identity is disproved.
                        entry.valid_until = None;
                        // Retain the last binding as a fence: losing queue
                        // admission must not make a later different home look
                        // like the first verified home for this generation.
                        changed |= entry.evidence.apply_codex(
                            entry.generation,
                            None,
                            entry.context.screen,
                        );
                    }
                }
            }
        }
        changed
    }
}

impl crate::app::App {
    pub(crate) fn prune_goal_tab_selection(&mut self) -> bool {
        let removed: Vec<_> = self
            .goal_tab_selected
            .iter()
            .copied()
            .filter(|pane| {
                !self.ui_settings.goal_tab_enabled || self.current_goal_phase(*pane).is_none()
            })
            .collect();
        for pane in &removed {
            self.goal_tab_selected.remove(pane);
        }
        !removed.is_empty()
    }

    pub(crate) fn goal_observation(&self, pane: NodeId) -> Option<&GoalObservation> {
        use ilium_core::{AgentClass, NodeKind, PaneStatus};
        let NodeKind::Pane {
            status: PaneStatus::Agent(agent),
            ..
        } = &self.tree.get(pane)?.kind
        else {
            return None;
        };
        match &agent.class {
            AgentClass::Claude => self.claude_goal_observation(pane),
            AgentClass::Codex => self.codex_goal_observation(pane),
            _ => None,
        }
    }

    pub(crate) fn current_goal_phase(&self, pane: NodeId) -> Option<GoalPhase> {
        use ilium_core::{AgentClass, GoalState, NodeKind, PaneStatus};
        let NodeKind::Pane {
            status: PaneStatus::Agent(agent),
            ..
        } = &self.tree.get(pane)?.kind
        else {
            return None;
        };
        if !matches!(&agent.class, AgentClass::Claude | AgentClass::Codex) {
            return None;
        }
        if let Some(observation) = self.goal_observation(pane) {
            if observation.absent {
                return None;
            }
            return observation
                .phase
                .filter(|phase| *phase != GoalPhase::Reached);
        }
        agent.goal.and_then(|goal| match goal {
            GoalState::Active => Some(GoalPhase::Active),
            GoalState::Paused => Some(GoalPhase::Paused),
            GoalState::Blocked => Some(GoalPhase::Blocked),
            GoalState::UsageLimited => Some(GoalPhase::UsageLimited),
            GoalState::Reached => None,
        })
    }

    /// Check the live binding again at render time: IPC identity changes can
    /// arrive between refresh ticks and must immediately hide the old text.
    pub(crate) fn codex_goal_observation(&self, pane: NodeId) -> Option<&GoalObservation> {
        use ilium_core::{AgentClass, NodeKind, PaneStatus};
        let node = self.tree.get(pane)?;
        let NodeKind::Pane {
            status: PaneStatus::Agent(agent),
            ..
        } = &node.kind
        else {
            return None;
        };
        if agent.class != AgentClass::Codex {
            return None;
        }
        let (class, session_id, project) = self.known_agent_history_context(pane)?;
        if class != AgentClass::Codex {
            return None;
        }
        let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
        let process_id = *self.agent_process_ids.get(&pane)?;
        let process_identity_inode = *self.agent_process_identity_inode.get(&pane)?;
        let identity_generation = self
            .agent_title_generations
            .get(&pane)
            .copied()
            .filter(|value| *value != u64::MAX)?;
        let transcript_path = self.agent_transcript_paths.get(&pane).cloned();
        self.session_goals.observation(
            pane,
            &CodexContext {
                session_id,
                project,
                home,
                transcript_path,
                process_id,
                process_identity_inode,
                identity_generation,
                screen: None,
            },
        )
    }

    pub(crate) fn tick_session_goals(&mut self, now: Instant) -> bool {
        use ilium_core::{AgentClass, GoalState, NodeKind, PaneStatus};

        let home = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
        let enabled =
            self.ui_settings.goal_tab_enabled || self.ui_settings.goal_toolbar_icon_enabled;
        let mut contexts = BTreeMap::new();
        if let (true, Some(home)) = (enabled, home) {
            for node in self.tree.panes() {
                let NodeKind::Pane {
                    status: PaneStatus::Agent(agent),
                    ..
                } = &node.kind
                else {
                    continue;
                };
                if agent.class != AgentClass::Codex {
                    continue;
                }
                let Some((class, session_id, project)) = self.known_agent_history_context(node.id)
                else {
                    continue;
                };
                if class != AgentClass::Codex {
                    continue;
                }
                let Some(process_id) = self.agent_process_ids.get(&node.id).copied() else {
                    continue;
                };
                let Some(process_identity_inode) =
                    self.agent_process_identity_inode.get(&node.id).copied()
                else {
                    continue;
                };
                // The server increments this on process replacement and
                // session invalidation, including reuse of the same PID.
                let Some(identity_generation) = self
                    .agent_title_generations
                    .get(&node.id)
                    .copied()
                    .filter(|value| *value != u64::MAX)
                else {
                    continue;
                };
                let screen = agent.goal.map(|phase| match phase {
                    GoalState::Active => GoalPhase::Active,
                    GoalState::Paused => GoalPhase::Paused,
                    GoalState::Blocked => GoalPhase::Blocked,
                    GoalState::UsageLimited => GoalPhase::UsageLimited,
                    GoalState::Reached => GoalPhase::Reached,
                });
                let transcript_path = self.agent_transcript_paths.get(&node.id).cloned();
                contexts.insert(
                    node.id,
                    CodexContext {
                        session_id,
                        project,
                        home: home.clone(),
                        transcript_path,
                        process_id,
                        process_identity_inode,
                        identity_generation,
                        screen,
                    },
                );
            }
        }
        self.session_goals.tick(contexts, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_reader_retains_previously_verified_text_as_stale() {
        use ilium_agent_session::goal_evidence::{Freshness, GoalLookup};
        use ilium_platform::process_info::AgentProviderHomes;
        let now = Instant::now();
        let pane = NodeId(3);
        let generation = GoalGeneration(1);
        let context = CodexContext {
            session_id: "synthetic-thread".into(),
            project: "/project".into(),
            home: "/home".into(),
            transcript_path: None,
            process_id: 42,
            process_identity_inode: 123,
            identity_generation: 1,
            screen: Some(GoalPhase::Paused),
        };
        let mut evidence = GoalReconciler::new(generation);
        evidence.apply_codex(
            generation,
            Some(GoalLookup::Present {
                phase: GoalPhase::Active,
                objective: "synthetic objective".into(),
            }),
            context.screen,
        );
        let storage = crate::execution::process_quota()
            .reserve_external_storage(4096)
            .unwrap();
        let retained = crate::execution::process_quota()
            .reserve_external_storage(4096)
            .unwrap();
        let mut store = SessionGoalStore::default();
        store.entries.insert(
            pane,
            Entry {
                context: context.clone(),
                generation,
                evidence,
                retained: Some(retained),
                provider_homes: Some(AgentProviderHomes {
                    home: "/home".into(),
                    claude_config: "/home/claude".into(),
                    codex_home: "/home/codex".into(),
                }),
                valid_until: Some(now + MAX_PROVIDER_AGE),
                due: now,
                _storage: storage,
            },
        );
        assert!(store.tick(BTreeMap::from([(pane, context.clone())]), now));
        let result = store.observation(pane, &context).unwrap();
        assert_eq!(result.freshness, Freshness::Stale);
        assert_eq!(result.phase, Some(GoalPhase::Paused));
        assert_eq!(result.objective.as_deref(), Some("synthetic objective"));
        assert!(store.entries.get(&pane).unwrap().retained.is_some());
    }

    #[test]
    fn changed_provider_home_discards_text_and_rejects_reply() {
        use ilium_agent_session::goal_evidence::{Freshness, GoalLookup};
        use ilium_platform::process_info::AgentProviderHomes;
        let now = Instant::now();
        let pane = NodeId(4);
        let generation = GoalGeneration(1);
        let context = CodexContext {
            session_id: "synthetic-thread".into(),
            project: "/project".into(),
            home: "/home".into(),
            transcript_path: None,
            process_id: 42,
            process_identity_inode: 123,
            identity_generation: 1,
            screen: Some(GoalPhase::Paused),
        };
        let homes = AgentProviderHomes {
            home: "/home".into(),
            claude_config: "/home/claude".into(),
            codex_home: "/home/codex".into(),
        };
        let changed_homes = AgentProviderHomes {
            codex_home: "/other/codex".into(),
            ..homes.clone()
        };
        let mut evidence = GoalReconciler::new(generation);
        evidence.apply_codex(
            generation,
            Some(GoalLookup::Present {
                phase: GoalPhase::Active,
                objective: "old objective".into(),
            }),
            context.screen,
        );
        let storage = crate::execution::process_quota()
            .reserve_external_storage(4096)
            .unwrap();
        let retained = crate::execution::process_quota()
            .reserve_external_storage(4096)
            .unwrap();
        let incoming = crate::execution::process_quota()
            .reserve_external_storage(4096)
            .unwrap();
        let (sender, receiver) = oneshot::channel();
        assert!(sender
            .send(ProviderReply {
                homes: Some(changed_homes),
                binding_valid: true,
                result: Some(ProviderGoalResult {
                    lookup: GoalLookup::Present {
                        phase: GoalPhase::Active,
                        objective: "other objective".into()
                    },
                    storage: incoming,
                })
            })
            .is_ok());
        let mut store = SessionGoalStore::default();
        store.entries.insert(
            pane,
            Entry {
                context: context.clone(),
                generation,
                evidence,
                retained: Some(retained),
                provider_homes: Some(homes.clone()),
                valid_until: Some(now + MAX_PROVIDER_AGE),
                due: now + REFRESH,
                _storage: storage,
            },
        );
        store.pending = Some(Pending {
            pane,
            generation,
            started: now,
            receiver,
        });
        assert!(store.tick(BTreeMap::from([(pane, context.clone())]), now));
        let result = store.observation(pane, &context).unwrap();
        assert_eq!(result.freshness, Freshness::Stale);
        assert_eq!(result.phase, Some(GoalPhase::Paused));
        assert!(result.objective.is_none());
        let entry = store.entries.get(&pane).unwrap();
        assert!(entry.retained.is_none());
        assert_eq!(entry.provider_homes.as_ref(), Some(&homes));
    }

    #[test]
    fn invalid_transcript_binding_discards_retained_goal() {
        use ilium_agent_session::goal_evidence::GoalLookup;
        use ilium_platform::process_info::AgentProviderHomes;
        let now = Instant::now();
        let pane = NodeId(40);
        let generation = GoalGeneration(1);
        let context = CodexContext {
            session_id: "thread".into(),
            project: "/project".into(),
            home: "/home".into(),
            transcript_path: Some("/home/thread.jsonl".into()),
            process_id: 42,
            process_identity_inode: 123,
            identity_generation: 1,
            screen: Some(GoalPhase::Active),
        };
        let homes = AgentProviderHomes {
            home: "/home".into(),
            claude_config: "/home/claude".into(),
            codex_home: "/home/codex".into(),
        };
        let mut evidence = GoalReconciler::new(generation);
        evidence.apply_codex(
            generation,
            Some(GoalLookup::Present {
                phase: GoalPhase::Active,
                objective: "old objective".into(),
            }),
            context.screen,
        );
        let storage = crate::execution::process_quota()
            .reserve_external_storage(4096)
            .unwrap();
        let retained = crate::execution::process_quota()
            .reserve_external_storage(4096)
            .unwrap();
        let (sender, receiver) = oneshot::channel();
        assert!(sender
            .send(ProviderReply {
                homes: Some(homes.clone()),
                result: None,
                binding_valid: false
            })
            .is_ok());
        let mut store = SessionGoalStore::default();
        store.entries.insert(
            pane,
            Entry {
                context: context.clone(),
                generation,
                evidence,
                retained: Some(retained),
                provider_homes: Some(homes),
                valid_until: Some(now + MAX_PROVIDER_AGE),
                due: now + REFRESH,
                _storage: storage,
            },
        );
        store.pending = Some(Pending {
            pane,
            generation,
            started: now,
            receiver,
        });
        assert!(store.tick(BTreeMap::from([(pane, context.clone())]), now));
        assert!(store
            .observation(pane, &context)
            .unwrap()
            .objective
            .is_none());
        assert!(store.entries.get(&pane).unwrap().retained.is_none());
    }

    #[test]
    fn overdue_provider_evidence_becomes_stale_while_another_read_is_pending() {
        use ilium_agent_session::goal_evidence::{Freshness, GoalLookup};
        let now = Instant::now();
        let pane = NodeId(1);
        let generation = GoalGeneration(1);
        let context = CodexContext {
            session_id: "synthetic-thread".into(),
            project: "/project".into(),
            home: "/home".into(),
            transcript_path: None,
            process_id: 42,
            process_identity_inode: 123,
            identity_generation: 1,
            screen: Some(GoalPhase::Paused),
        };
        let mut evidence = GoalReconciler::new(generation);
        evidence.apply_codex(
            generation,
            Some(GoalLookup::Present {
                phase: GoalPhase::Active,
                objective: "synthetic objective".into(),
            }),
            context.screen,
        );
        let storage = crate::execution::process_quota()
            .reserve_external_storage(4096)
            .unwrap();
        let (sender, receiver) = oneshot::channel();
        let mut store = SessionGoalStore::default();
        store.entries.insert(
            pane,
            Entry {
                context: context.clone(),
                generation,
                evidence,
                retained: None,
                provider_homes: None,
                valid_until: Some(now),
                due: now + REFRESH,
                _storage: storage,
            },
        );
        store.pending = Some(Pending {
            pane,
            generation,
            started: now,
            receiver,
        });
        assert!(store.tick(BTreeMap::from([(pane, context.clone())]), now));
        let result = store.observation(pane, &context).unwrap();
        assert_eq!(result.freshness, Freshness::Stale);
        assert_eq!(result.phase, Some(GoalPhase::Paused));
        assert_eq!(result.objective.as_deref(), Some("synthetic objective"));
        assert!(!result.absent);
        drop(sender);
    }

    #[test]
    fn late_absence_does_not_clear_current_screen_evidence() {
        use ilium_agent_session::goal_evidence::{Freshness, GoalLookup};
        use ilium_platform::process_info::AgentProviderHomes;
        let now = Instant::now();
        let pane = NodeId(2);
        let generation = GoalGeneration(1);
        let context = CodexContext {
            session_id: "synthetic-thread".into(),
            project: "/project".into(),
            home: "/home".into(),
            transcript_path: None,
            process_id: 42,
            process_identity_inode: 123,
            identity_generation: 1,
            screen: Some(GoalPhase::Active),
        };
        let homes = AgentProviderHomes {
            home: "/home".into(),
            claude_config: "/home/claude".into(),
            codex_home: "/home/codex".into(),
        };
        let mut evidence = GoalReconciler::new(generation);
        evidence.apply_codex(generation, None, context.screen);
        let storage = crate::execution::process_quota()
            .reserve_external_storage(4096)
            .unwrap();
        let result_storage = crate::execution::process_quota()
            .reserve_external_storage(4096)
            .unwrap();
        let (sender, receiver) = oneshot::channel();
        assert!(sender
            .send(ProviderReply {
                homes: Some(homes.clone()),
                binding_valid: true,
                result: Some(ProviderGoalResult {
                    lookup: GoalLookup::Absent,
                    storage: result_storage
                })
            })
            .is_ok());
        let mut store = SessionGoalStore::default();
        store.entries.insert(
            pane,
            Entry {
                context: context.clone(),
                generation,
                evidence,
                retained: None,
                provider_homes: Some(homes),
                valid_until: None,
                due: now + REFRESH,
                _storage: storage,
            },
        );
        store.pending = Some(Pending {
            pane,
            generation,
            started: now - MAX_PROVIDER_AGE,
            receiver,
        });
        assert!(store.tick(BTreeMap::from([(pane, context.clone())]), now));
        let result = store.observation(pane, &context).unwrap();
        assert_eq!(result.freshness, Freshness::Stale);
        assert_eq!(result.phase, Some(GoalPhase::Active));
        assert!(!result.absent);
    }

    #[test]
    fn provider_home_loss_or_change_rejects_a_reply() {
        use ilium_platform::process_info::AgentProviderHomes;
        let first = AgentProviderHomes {
            home: "/one".into(),
            claude_config: "/one/claude".into(),
            codex_home: "/one/codex".into(),
        };
        let second = AgentProviderHomes {
            codex_home: "/two/codex".into(),
            ..first.clone()
        };
        assert!(!provider_home_matches(None, None));
        assert!(provider_home_matches(None, Some(&first)));
        assert!(provider_home_matches(Some(&first), Some(&first)));
        assert!(!provider_home_matches(Some(&first), None));
        assert!(!provider_home_matches(Some(&first), Some(&second)));
    }

    #[test]
    fn identity_fences_process_reuse_and_ignores_screen_phase_changes() {
        let context = CodexContext {
            session_id: "thread".into(),
            project: "/project".into(),
            home: "/home".into(),
            transcript_path: Some("/home/.codex/sessions/thread.jsonl".into()),
            process_id: 42,
            process_identity_inode: 123,
            identity_generation: 5,
            screen: Some(GoalPhase::Active),
        };
        let mut next = context.clone();
        next.screen = Some(GoalPhase::Paused);
        assert!(context.same_identity(&next));
        next.transcript_path = Some("/home/.codex/sessions/other.jsonl".into());
        assert!(!context.same_identity(&next));
        next = context.clone();
        next.identity_generation += 1;
        assert!(!context.same_identity(&next));
        next.identity_generation = context.identity_generation;
        next.session_id = "other".into();
        assert!(!context.same_identity(&next));
        next = context.clone();
        next.process_id += 1;
        assert!(!context.same_identity(&next));
        next = context.clone();
        next.process_identity_inode += 1;
        assert!(!context.same_identity(&next));
        next = context.clone();
        next.project = "/other-project".into();
        assert!(!context.same_identity(&next));
        next = context.clone();
        next.home = "/other-home".into();
        assert!(!context.same_identity(&next));
    }
}
