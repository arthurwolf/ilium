//! Retained presentation of blocking failures and correlated creation receipts.
use std::collections::HashMap;
use std::time::{Duration, Instant};

use ilium_core::{NodeId, ROOT_ID};
use ilium_ipc::{BlockingError, ClientRequest, NewPaneKind};

use crate::app::App;
use crate::status_icons::{safe_tooltip_text, TooltipContent};
use crate::tree_ui::TreeToolbarAction;

pub(crate) struct PendingPaneCreation {
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
        let ClientRequest::NewPane { request_id, parent_group, kind, .. } = request else {
            return None;
        };
        *request_id = self.next_workspace_request_id();
        let action = match kind {
            NewPaneKind::PlainShell => TreeToolbarAction::Shell,
            NewPaneKind::Command(command) | NewPaneKind::CommandClosingOnExit(command)
            | NewPaneKind::CommandWithInitialInput { command_line: command, .. } => Self::toolbar_action_for_command(command),
            NewPaneKind::Editor(_) => TreeToolbarAction::Editor,
        };
        Some((*request_id, PendingPaneCreation { parent_group: *parent_group, action, started: Instant::now(), uncertainty_reported: false }))
    }

    pub(crate) fn receive_pane_create_finished(&mut self, request_id: u64, result: Result<NodeId, String>) {
        let Some(pending) = self.pending_pane_creations.remove(&request_id) else { return; };
        match result {
            Ok(_) => {
                if let Some(errors) = self.local_blocking_errors.get_mut(&pending.parent_group) {
                    errors.retain(|error| error.operation != format!("Create pane request {request_id}"));
                }
            }
            Err(message) => {
                self.record_local_blocking_error(pending.parent_group, &format!("Create pane request {request_id}"), &message);
                self.status_message = Some(format!("Could not create pane: {message}"));
            }
        }
    }

    pub(crate) fn expire_pending_pane_creations(&mut self, now: Instant) -> bool {
        let expired: Vec<_> = self.pending_pane_creations.iter()
            .filter(|(_, pending)| !pending.uncertainty_reported && now.saturating_duration_since(pending.started) >= Duration::from_secs(120))
            .map(|(id, pending)| (*id, pending.parent_group)).collect();
        for (request_id, target) in &expired {
            if let Some(pending) = self.pending_pane_creations.get_mut(request_id) { pending.uncertainty_reported = true; }
            self.record_local_blocking_error(*target, &format!("Create pane request {request_id}"), "No creation confirmation was received. The outcome is unknown; check the project before retrying.");
        }
        !expired.is_empty()
    }

    pub(crate) fn record_local_blocking_error(&mut self, target: NodeId, operation: &str, message: &str) {
        // Error storage is finite even when retries generate distinct diagnostics.
        if self.local_blocking_errors.len() >= 128 && !self.local_blocking_errors.contains_key(&target) {
            let overflow = self.local_blocking_errors.entry(ROOT_ID).or_default();
            if !overflow.iter().any(|error| error.operation == "Error reporting") { overflow.push(BlockingError { node_id: None, operation: "Error reporting".into(), message: "Additional blocking errors occurred; inspect file logging for details.".into() }); }
            return;
        }
        let errors = self.local_blocking_errors.entry(target).or_default();
        if errors.iter().any(|error| error.operation == operation && error.message == message) { return; }
        if errors.len() >= 8 {
            if !errors.iter().any(|error| error.operation == "Error reporting") {
                errors[7] = BlockingError { node_id: Some(target), operation: "Error reporting".into(), message: "Additional blocking errors occurred; inspect file logging for details.".into() };
            }
            return;
        }
        errors.push(BlockingError { node_id: Some(target), operation: operation.chars().take(128).collect(), message: if message.chars().count() > 4096 { format!("{} … (message truncated)", message.chars().take(4096).collect::<String>()) } else { message.into() } });
    }

    pub(crate) fn replace_blocking_errors(&mut self, errors: Vec<BlockingError>) {
        let mut by_node: HashMap<NodeId, Vec<BlockingError>> = HashMap::new();
        for error in errors {
            let target = error.node_id.unwrap_or(ROOT_ID);
            by_node.entry(target).or_default().push(error);
        }
        self.blocking_errors = by_node;
    }

    pub(crate) fn blocking_error_tooltip(&self, node_id: NodeId) -> Option<TooltipContent> {
        let mut messages = Vec::new();
        for (target, errors) in self.blocking_errors.iter().chain(&self.local_blocking_errors) {
            let mut ancestor = Some(*target);
            let mut is_descendant = *target == ROOT_ID;
            while let Some(id) = ancestor {
                if id == node_id { is_descendant = true; break; }
                ancestor = self.tree.get(id).and_then(|node| node.parent);
            }
            if !is_descendant { continue; }
            let label = if *target == ROOT_ID { "Session" } else {
                self.tree.get(*target).map_or("Removed entry", |node| node.name.as_str())
            };
            for error in errors {
                let message = format!("{} — {}: {}", safe_tooltip_text(label), safe_tooltip_text(&error.operation), safe_tooltip_text(&error.message));
                if !messages.contains(&message) { messages.push(message); }
            }
        }
        if messages.is_empty() { return None; }
        messages.sort();
        Some(TooltipContent { title: "Blocking errors".into(), body: messages.join("\n\n"), reason: messages.first().cloned() })
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
            app.pending_pane_creations.insert(request_id, PendingPaneCreation { parent_group: ROOT_ID, action, started: Instant::now(), uncertainty_reported: false });
        }
        app.receive_pane_create_finished(42, Err("launch refused".into()));
        assert!(app.pending_pane_creations.contains_key(&41));
        assert!(!app.pending_pane_creations.contains_key(&42));
        assert!(app.blocking_error_tooltip(ROOT_ID).unwrap().body.contains("launch refused"));
        app.receive_pane_create_finished(41, Ok(NodeId(9)));
        assert!(app.pending_pane_creations.is_empty());
        assert!(app.blocking_error_tooltip(ROOT_ID).unwrap().body.contains("launch refused"));
    }

    #[test]
    fn authoritative_recovery_clears_warning_without_discarding_local_refusal() {
        let mut app = App::new("errors".into(), std::env::temp_dir());
        app.replace_blocking_errors(vec![BlockingError { node_id: None, operation: "Restore".into(), message: "permission denied".into() }]);
        assert!(app.blocking_error_tooltip(ROOT_ID).unwrap().body.contains("permission denied"));
        app.record_local_blocking_error(ROOT_ID, "Create pane", "queue full");
        app.replace_blocking_errors(Vec::new());
        let tooltip = app.blocking_error_tooltip(ROOT_ID).unwrap();
        assert!(!tooltip.body.contains("permission denied"));
        assert!(tooltip.body.contains("queue full"));
    }

    #[test]
    fn unconfirmed_creation_timeout_is_visible_and_explicitly_uncertain() {
        let mut app = App::new("errors".into(), std::env::temp_dir());
        let started = Instant::now();
        app.pending_pane_creations.insert(7, PendingPaneCreation { parent_group: ROOT_ID, action: TreeToolbarAction::Shell, started, uncertainty_reported: false });
        assert!(app.expire_pending_pane_creations(started + Duration::from_secs(121)));
        assert!(app.pending_pane_creations[&7].uncertainty_reported);
        assert!(app.blocking_error_tooltip(ROOT_ID).unwrap().body.contains("outcome is unknown"));
        app.receive_pane_create_finished(7, Ok(NodeId(9)));
        assert!(app.blocking_error_tooltip(ROOT_ID).is_none());
    }
}
