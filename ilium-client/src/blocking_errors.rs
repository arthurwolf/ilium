//! Retained presentation of blocking failures and correlated creation receipts.
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use ilium_core::{NodeId, ROOT_ID};
use ilium_ipc::{BlockingError, ClientRequest, NewPaneKind};

use crate::app::App;
use crate::status_icons::{safe_tooltip_text, TooltipContent};
use crate::tree_ui::TreeToolbarAction;

/// The retained-error facts the custom status rules read, precomputed from
/// `App` so the tree renderer's context stays a set of plain borrows.
// `pub` (not `pub(crate)`) because the public `tree_ui::TreeRenderOptions`
// borrows it; the module itself stays private.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BlockingErrorFacts {
    /// Nodes that are the exact target of at least one retained error.
    pub panes: HashSet<NodeId>,
    /// Whether a session-level error (no node) is retained.
    pub session: bool,
    /// How Custom mode presents these errors (`[ui] blocking_error_display`).
    /// Carried with the facts because the tree renderer decides row widths
    /// (the Second icon slot) from the same per-frame value it reads the
    /// facts from, so geometry and painting can never disagree.
    pub custom_display: crate::config::BlockingErrorDisplay,
}

impl BlockingErrorFacts {
    /// Whether this pane is the exact target of a retained error.
    pub fn pane_has_error(&self, node_id: NodeId) -> bool {
        self.panes.contains(&node_id)
    }
}

pub(crate) struct PendingPaneCreation {
    pub custom_profile_id: Option<String>,
    pub parent_group: NodeId,
    pub action: TreeToolbarAction,
    pub started: Instant,
    pub uncertainty_reported: bool,
}

impl App {
    pub(crate) fn prepare_pane_creation(
        &mut self,
        request: &mut ClientRequest,
    ) -> Option<(u64, PendingPaneCreation)> {
        let ClientRequest::NewPane {
            request_id,
            parent_group,
            kind,
            ..
        } = request
        else {
            return None;
        };
        *request_id = self.next_workspace_request_id();
        ilium_platform::startup_timing::record("creation_intent", *request_id, 0, 0, 0);
        let action = match kind {
            NewPaneKind::PlainShell => TreeToolbarAction::Shell,
            NewPaneKind::Command(command)
            | NewPaneKind::CommandClosingOnExit(command)
            | NewPaneKind::CommandWithInitialInput {
                command_line: command,
                ..
            } => Self::toolbar_action_for_command(command),
            NewPaneKind::Editor(_) => TreeToolbarAction::Editor,
            NewPaneKind::Plugin { .. } => TreeToolbarAction::Editor,
            NewPaneKind::CustomAgent { identity, .. } => {
                TreeToolbarAction::Agent(identity.base_provider)
            }
        };
        Some((
            *request_id,
            PendingPaneCreation {
                custom_profile_id: match kind {
                    NewPaneKind::CustomAgent { identity, .. } => Some(identity.profile_id.clone()),
                    _ => None,
                },
                parent_group: *parent_group,
                action,
                started: Instant::now(),
                uncertainty_reported: false,
            },
        ))
    }

    pub(crate) fn receive_pane_create_finished(
        &mut self,
        request_id: u64,
        result: Result<NodeId, String>,
    ) {
        let Some(pending) = self.pending_pane_creations.remove(&request_id) else {
            return;
        };
        ilium_platform::startup_timing::record(
            if result.is_ok() {
                "creation_receipt"
            } else {
                "creation_failed"
            },
            request_id,
            result.as_ref().map_or(0, |pane| pane.0),
            0,
            0,
        );
        match result {
            Ok(_) => {
                if let Some(errors) = self.local_blocking_errors.get_mut(&pending.parent_group) {
                    errors.retain(|error| {
                        error
                            .operation
                            .strip_prefix("Create pane request ")
                            .and_then(|id| id.parse::<u64>().ok())
                            .is_none_or(|id| id > request_id)
                    });
                }
            }
            Err(message) => {
                self.record_local_blocking_error(
                    pending.parent_group,
                    &format!("Create pane request {request_id}"),
                    &message,
                );
                self.status_message = Some(format!("Could not create pane: {message}"));
            }
        }
    }

    pub(crate) fn expire_pending_pane_creations(&mut self, now: Instant) -> bool {
        let expired: Vec<_> = self
            .pending_pane_creations
            .iter()
            .filter(|(_, pending)| {
                !pending.uncertainty_reported
                    && now.saturating_duration_since(pending.started) >= Duration::from_secs(120)
            })
            .map(|(id, pending)| (*id, pending.parent_group))
            .collect();
        for (request_id, target) in &expired {
            if let Some(pending) = self.pending_pane_creations.get_mut(request_id) {
                pending.uncertainty_reported = true;
            }
            self.record_local_blocking_error(*target, &format!("Create pane request {request_id}"), "No creation confirmation was received. The outcome is unknown; check the project before retrying.");
        }
        !expired.is_empty()
    }

    pub(crate) fn record_local_blocking_error(
        &mut self,
        target: NodeId,
        operation: &str,
        message: &str,
    ) {
        // Error storage is finite even when retries generate distinct diagnostics.
        if self.local_blocking_errors.len() >= 128
            && !self.local_blocking_errors.contains_key(&target)
        {
            let overflow = self.local_blocking_errors.entry(ROOT_ID).or_default();
            if !overflow
                .iter()
                .any(|error| error.operation == "Error reporting")
            {
                overflow.push(BlockingError {
                    node_id: None,
                    operation: "Error reporting".into(),
                    message:
                        "Additional blocking errors occurred; inspect file logging for details."
                            .into(),
                });
            }
            return;
        }
        let errors = self.local_blocking_errors.entry(target).or_default();
        if errors
            .iter()
            .any(|error| error.operation == operation && error.message == message)
        {
            return;
        }
        if errors.len() >= 8 {
            if !errors
                .iter()
                .any(|error| error.operation == "Error reporting")
            {
                errors[7] = BlockingError {
                    node_id: Some(target),
                    operation: "Error reporting".into(),
                    message:
                        "Additional blocking errors occurred; inspect file logging for details."
                            .into(),
                };
            }
            return;
        }
        errors.push(BlockingError {
            node_id: Some(target),
            operation: operation.chars().take(128).collect(),
            message: if message.chars().count() > 4096 {
                format!(
                    "{} … (message truncated)",
                    message.chars().take(4096).collect::<String>()
                )
            } else {
                message.into()
            },
        });
    }

    pub(crate) fn replace_blocking_errors(&mut self, errors: Vec<BlockingError>) {
        let mut by_node: HashMap<NodeId, Vec<BlockingError>> = HashMap::new();
        for error in errors {
            let target = error.node_id.unwrap_or(ROOT_ID);
            by_node.entry(target).or_default().push(error);
        }
        self.blocking_errors = by_node;
    }

    /// Whether a retained blocking error targets exactly this node, from the
    /// server's records or this client's own refusals. Errors of descendants
    /// do not count: the `blocking-error` rule condition describes the pane
    /// itself. A session-level error (filed under ROOT_ID) is never a pane
    /// error; see [`App::session_has_blocking_error`].
    pub(crate) fn pane_has_blocking_error(&self, node_id: NodeId) -> bool {
        node_id != ROOT_ID
            && [&self.blocking_errors, &self.local_blocking_errors]
                .into_iter()
                .any(|errors| errors.get(&node_id).is_some_and(|list| !list.is_empty()))
    }

    /// Whether the session has a retained session-level error: one with no
    /// node (`node_id` None, filed under ROOT_ID by
    /// [`App::replace_blocking_errors`]) or a local refusal recorded against
    /// the root group.
    pub(crate) fn session_has_blocking_error(&self) -> bool {
        [&self.blocking_errors, &self.local_blocking_errors]
            .into_iter()
            .any(|errors| errors.get(&ROOT_ID).is_some_and(|list| !list.is_empty()))
    }

    /// Both error facts at once, for the tree renderer, which sees a render
    /// context rather than `App`. Computed once per frame.
    pub(crate) fn blocking_error_facts(&self) -> BlockingErrorFacts {
        BlockingErrorFacts {
            panes: self
                .blocking_errors
                .iter()
                .chain(&self.local_blocking_errors)
                .filter(|(target, errors)| **target != ROOT_ID && !errors.is_empty())
                .map(|(target, _)| *target)
                .collect(),
            session: self.session_has_blocking_error(),
            custom_display: self.ui_settings.blocking_error_display,
        }
    }

    pub(crate) fn blocking_error_tooltip(&self, node_id: NodeId) -> Option<TooltipContent> {
        let mut messages = Vec::new();
        for (target, errors) in self
            .blocking_errors
            .iter()
            .chain(&self.local_blocking_errors)
        {
            // Walk from the failing node up to the root. A session-level
            // error is filed under ROOT_ID, so the walk starts at the root and
            // matches only the root (session) row itself: it must not make
            // every pane look failed. A pane error still reaches its real
            // ancestor groups.
            let mut ancestor = Some(*target);
            let mut is_descendant = false;
            while let Some(id) = ancestor {
                if id == node_id {
                    is_descendant = true;
                    break;
                }
                ancestor = self.tree.get(id).and_then(|node| node.parent);
            }
            if !is_descendant {
                continue;
            }
            let label = if *target == ROOT_ID {
                "Session"
            } else {
                self.tree
                    .get(*target)
                    .map_or("Removed entry", |node| node.name.as_str())
            };
            for error in errors {
                let message = format!(
                    "{} — {}: {}",
                    safe_tooltip_text(label),
                    safe_tooltip_text(&error.operation),
                    error
                        .message
                        .chars()
                        .map(|character| {
                            if character.is_control() && character != '\n' && character != '\t' {
                                '�'
                            } else {
                                character
                            }
                        })
                        .collect::<String>()
                );
                if !messages.contains(&message) {
                    messages.push(message);
                }
            }
        }
        if messages.is_empty() {
            return None;
        }
        messages.sort();
        Some(TooltipContent {
            title: "Recorded operation failures".into(),
            body: messages.join("\n\n"),
            reason: Some("Retained error records may describe earlier failed attempts. They do not establish a current hang; compare the operation with its latest result and current monitor registration.".into()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::BuiltinAgentProvider;

    #[test]
    fn concurrent_create_failure_clears_only_its_correlated_spinner() {
        let mut app = App::new("errors".into(), std::env::temp_dir());
        let action = TreeToolbarAction::Agent(BuiltinAgentProvider::Codex);
        for request_id in [41, 42] {
            app.pending_pane_creations.insert(
                request_id,
                PendingPaneCreation {
                    custom_profile_id: None,
                    parent_group: ROOT_ID,
                    action,
                    started: Instant::now(),
                    uncertainty_reported: false,
                },
            );
        }
        app.receive_pane_create_finished(42, Err("launch refused".into()));
        assert!(app.pending_pane_creations.contains_key(&41));
        assert!(!app.pending_pane_creations.contains_key(&42));
        assert!(app
            .blocking_error_tooltip(ROOT_ID)
            .unwrap()
            .body
            .contains("launch refused"));
        app.receive_pane_create_finished(41, Ok(NodeId(9)));
        assert!(app.pending_pane_creations.is_empty());
        assert!(app
            .blocking_error_tooltip(ROOT_ID)
            .unwrap()
            .body
            .contains("launch refused"));
        app.pending_pane_creations.insert(
            43,
            PendingPaneCreation {
                custom_profile_id: None,
                parent_group: ROOT_ID,
                action,
                started: Instant::now(),
                uncertainty_reported: false,
            },
        );
        app.receive_pane_create_finished(43, Ok(NodeId(10)));
        assert!(app.blocking_error_tooltip(ROOT_ID).is_none());
    }

    #[test]
    fn authoritative_recovery_clears_warning_without_discarding_local_refusal() {
        let mut app = App::new("errors".into(), std::env::temp_dir());
        app.replace_blocking_errors(vec![BlockingError {
            node_id: None,
            operation: "Restore".into(),
            message: "permission denied".into(),
        }]);
        assert!(app
            .blocking_error_tooltip(ROOT_ID)
            .unwrap()
            .body
            .contains("permission denied"));
        app.record_local_blocking_error(ROOT_ID, "Create pane", "queue full");
        app.replace_blocking_errors(Vec::new());
        let tooltip = app.blocking_error_tooltip(ROOT_ID).unwrap();
        assert!(!tooltip.body.contains("permission denied"));
        assert!(tooltip.body.contains("queue full"));
    }

    #[test]
    fn session_errors_stay_on_the_session_and_pane_errors_on_their_pane() {
        use ilium_core::PaneContentKind;

        let mut app = App::new("errors".into(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "agents").unwrap();
        let failing = app
            .tree
            .add_pane(group, "failing", PaneContentKind::Terminal)
            .unwrap();
        let healthy = app
            .tree
            .add_pane(group, "healthy", PaneContentKind::Terminal)
            .unwrap();
        assert!(!app.session_has_blocking_error());
        assert_eq!(app.blocking_error_facts(), BlockingErrorFacts::default());

        // One session-level error must not mark every pane.
        app.replace_blocking_errors(vec![BlockingError {
            node_id: None,
            operation: "session".into(),
            message: "snapshot write failed".into(),
        }]);
        assert!(app.session_has_blocking_error());
        assert!(!app.pane_has_blocking_error(failing));
        assert!(!app.pane_has_blocking_error(ROOT_ID));
        assert!(app.blocking_error_tooltip(ROOT_ID).is_some());
        assert!(app.blocking_error_tooltip(group).is_none());
        assert!(app.blocking_error_tooltip(healthy).is_none());

        // A pane error marks that pane and its ancestor group only.
        app.record_local_blocking_error(failing, "Send input", "pipe closed");
        assert!(app.pane_has_blocking_error(failing));
        assert!(!app.pane_has_blocking_error(healthy));
        assert!(app.blocking_error_tooltip(failing).is_some());
        assert!(app.blocking_error_tooltip(group).is_some());
        assert!(app.blocking_error_tooltip(healthy).is_none());
        let facts = app.blocking_error_facts();
        assert!(facts.session);
        assert!(facts.pane_has_error(failing));
        assert!(!facts.pane_has_error(healthy));
        assert!(!facts.pane_has_error(group));
    }

    #[test]
    fn unconfirmed_creation_timeout_is_visible_and_explicitly_uncertain() {
        let mut app = App::new("errors".into(), std::env::temp_dir());
        let started = Instant::now();
        app.pending_pane_creations.insert(
            7,
            PendingPaneCreation {
                custom_profile_id: None,
                parent_group: ROOT_ID,
                action: TreeToolbarAction::Shell,
                started,
                uncertainty_reported: false,
            },
        );
        assert!(app.expire_pending_pane_creations(started + Duration::from_secs(121)));
        assert!(app.pending_pane_creations[&7].uncertainty_reported);
        assert!(app
            .blocking_error_tooltip(ROOT_ID)
            .unwrap()
            .body
            .contains("outcome is unknown"));
        app.receive_pane_create_finished(7, Ok(NodeId(9)));
        assert!(app.blocking_error_tooltip(ROOT_ID).is_none());
    }
}
