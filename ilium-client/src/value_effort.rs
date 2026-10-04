//! Requested agent effort uses the existing admitted toolbar commands.
use crate::agent_toolbar::EffortLevel;
use crate::app::{App, Mode, PaneRuntime};
use crate::value_dialog::{ChoiceDialogState, ChoiceOption, DialogOutcome};
use crate::value_dialog_host::{ValueDialogHost, ValueTarget};
use ilium_core::{BuiltinAgentProvider, NodeId};

pub(crate) fn effort_options() -> Vec<ChoiceOption> {
    EffortLevel::ALL
        .into_iter()
        .map(|effort| ChoiceOption {
            id: format!("{effort:?}"),
            label: match effort {
                EffortLevel::Auto => "Automatic",
                EffortLevel::Low => "Low",
                EffortLevel::Medium => "Medium",
                EffortLevel::High => "High",
                EffortLevel::XHigh => "Extra high",
                EffortLevel::Max => "Maximum",
                EffortLevel::Ultracode => "Ultracode (stage in next prompt)",
            }
            .into(),
            disabled_reason: None,
        })
        .collect()
}
impl ValueDialogHost {
    pub(crate) fn agent_effort(pane_id: NodeId, original: EffortLevel) -> Result<Self, String> {
        Ok(Self::choice_host(
            ValueTarget::AgentEffort { pane_id, original },
            ChoiceDialogState::new(
                "Requested reasoning effort",
                effort_options(),
                Some(format!("{original:?}")),
            )?,
        ))
    }
}
impl App {
    pub(crate) fn begin_agent_effort_dialog(&mut self, pane_id: NodeId) {
        if !matches!(self.mode, Mode::Normal)
            || self.agent_toolbar_provider(pane_id) != Some(BuiltinAgentProvider::Claude)
            || !matches!(self.panes.get(&pane_id), Some(PaneRuntime::Terminal(_)))
        {
            return;
        }
        let original = self
            .agent_toolbar_effort
            .get(&pane_id)
            .copied()
            .unwrap_or_default();
        match ValueDialogHost::agent_effort(pane_id, original) {
            Ok(host) => self.push_modal(Mode::ValueDialog(Box::new(host))),
            Err(error) => self.status_message = Some(error),
        }
    }
    pub(crate) fn set_agent_effort(
        &mut self,
        pane_id: NodeId,
        next: EffortLevel,
    ) -> Result<(), String> {
        if self.agent_toolbar_provider(pane_id) != Some(BuiltinAgentProvider::Claude)
            || !matches!(self.panes.get(&pane_id), Some(PaneRuntime::Terminal(_)))
        {
            return Err("This pane no longer supports requested effort".into());
        }
        let previous = self
            .agent_toolbar_effort
            .get(&pane_id)
            .copied()
            .unwrap_or_default();
        if next == previous {
            return Ok(());
        }
        let result = if next == EffortLevel::Ultracode {
            self.send_terminal_bytes(pane_id, b"ultracode ".to_vec(), None)
        } else {
            self.send_terminal_submission(
                pane_id,
                format!("/effort {}", next.command_word()),
                ilium_ipc::PromptSubmissionSource::ToolbarAction,
            )
        };
        result.map_err(|_| "Effort command rejected before admission".to_string())?;
        self.agent_toolbar_effort.insert(pane_id, next);
        Ok(())
    }
    pub(crate) fn step_agent_effort(&mut self, pane_id: NodeId, previous: bool) {
        let current = self
            .agent_toolbar_effort
            .get(&pane_id)
            .copied()
            .unwrap_or_default();
        if let Err(error) = self.set_agent_effort(
            pane_id,
            if previous {
                current.previous()
            } else {
                current.next()
            },
        ) {
            self.status_message = Some(error);
        }
    }
    pub(crate) fn commit_agent_effort_dialog(
        &mut self,
        host: &ValueDialogHost,
        outcome: &DialogOutcome,
    ) -> Result<(), String> {
        let ValueTarget::AgentEffort { pane_id, original } = &host.target else {
            return Err("This is not an effort choice".into());
        };
        let DialogOutcome::Choose(id) = outcome else {
            return Err("Choose a requested effort".into());
        };
        if !matches!(self.modal_stack.last(), Some(Mode::Normal))
            || self
                .agent_toolbar_effort
                .get(pane_id)
                .copied()
                .unwrap_or_default()
                != *original
        {
            return Err("The pane effort changed; reopen its choices".into());
        }
        let next = EffortLevel::ALL
            .into_iter()
            .find(|level| format!("{level:?}") == *id)
            .ok_or("This effort level is unavailable")?;
        self.set_agent_effort(*pane_id, next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_view::TerminalView;
    use crate::value_dialog::ValueDialogState;
    use ilium_core::{AgentActivity, AgentClass, PaneContentKind, PaneStatus, ROOT_ID};
    use ilium_ipc::{ClientRequest, PromptSubmissionSource};
    fn synthetic_app() -> (App, NodeId) {
        let mut app = App::new("synthetic-effort".into(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "synthetic").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "claude", PaneContentKind::Terminal)
            .unwrap();
        app.panes.insert(
            pane_id,
            PaneRuntime::Terminal(Box::new(TerminalView::new(24, 80))),
        );
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Working, None),
            )
            .unwrap();
        app.take_outbound_requests();
        (app, pane_id)
    }
    #[test]
    fn all_native_efforts_and_reverse_order_are_preserved() {
        let options = effort_options();
        assert_eq!(options.len(), 7);
        for (index, level) in EffortLevel::ALL.into_iter().enumerate() {
            assert_eq!(options[index].id, format!("{level:?}"));
            assert_eq!(level.next().previous(), level);
            assert_eq!(level.previous().next(), level);
        }
    }
    #[test]
    fn choice_routes_exact_command_and_ultracode_without_submitting_user_prompt() {
        let (mut app, pane_id) = synthetic_app();
        app.begin_agent_effort_dialog(pane_id);
        let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("catalog")
        };
        let ValueDialogState::Choice(dialog) = &host.dialog else {
            panic!("choice")
        };
        assert_eq!(dialog.options().len(), 7);
        app.finish_value_dialog(host, DialogOutcome::Choose("Max".into()));
        assert_eq!(app.agent_toolbar_effort[&pane_id], EffortLevel::Max);
        assert_eq!(
            app.take_outbound_requests(),
            vec![ClientRequest::SubmitTerminalText {
                pane_id,
                text: "/effort max".into(),
                source: PromptSubmissionSource::ToolbarAction
            }]
        );
        app.step_agent_effort(pane_id, false);
        assert_eq!(app.agent_toolbar_effort[&pane_id], EffortLevel::Ultracode);
        assert_eq!(
            app.take_outbound_requests(),
            vec![ClientRequest::KeyInput {
                pane_id,
                bytes: b"ultracode ".to_vec(),
                submission: None
            }]
        );
        app.step_agent_effort(pane_id, true);
        assert_eq!(app.agent_toolbar_effort[&pane_id], EffortLevel::Max);
    }
    #[test]
    fn stale_effort_or_changed_provider_is_refused_without_command() {
        let (mut app, pane_id) = synthetic_app();
        app.begin_agent_effort_dialog(pane_id);
        let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("catalog")
        };
        app.agent_toolbar_effort.insert(pane_id, EffortLevel::High);
        assert!(app
            .commit_agent_effort_dialog(&host, &DialogOutcome::Choose("Max".into()))
            .is_err());
        assert!(app.take_outbound_requests().is_empty());
        app.agent_toolbar_effort.remove(&pane_id);
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::from_activity(AgentClass::Codex, AgentActivity::Working, None),
            )
            .unwrap();
        assert!(app
            .commit_agent_effort_dialog(&host, &DialogOutcome::Choose("Max".into()))
            .is_err());
        assert!(app.take_outbound_requests().is_empty());
    }
    #[test]
    fn refused_command_keeps_original_effort_and_catalog_for_retry() {
        let (mut app, pane_id) = synthetic_app();
        app.begin_agent_effort_dialog(pane_id);
        let client = app.outbound_admission.take();
        let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("catalog")
        };
        app.finish_value_dialog(host, DialogOutcome::Choose("Max".into()));
        assert_eq!(
            app.agent_toolbar_effort
                .get(&pane_id)
                .copied()
                .unwrap_or_default(),
            EffortLevel::Auto
        );
        assert!(app.take_outbound_requests().is_empty());
        let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("refused catalog retained")
        };
        app.outbound_admission = client;
        app.finish_value_dialog(host, DialogOutcome::Choose("Max".into()));
        assert_eq!(app.agent_toolbar_effort[&pane_id], EffortLevel::Max);
        assert_eq!(app.take_outbound_requests().len(), 1);
        assert!(matches!(app.mode, Mode::Normal));
    }
}
