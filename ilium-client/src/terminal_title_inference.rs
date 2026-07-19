//! Decides whether a plain terminal pane is currently eligible for
//! automatic LLM screen-text retitling -- the terminal-pane analogue of
//! `crate::title_inference`, which gates the same decision for agent panes
//! off a resolved session ID. A plain shell has neither a session ID nor a
//! transcript, so eligibility here is driven entirely by the pane's current
//! `PaneStatus`/`PaneTitleSource` plus whether a worker is already in
//! flight; the *cadence* (every second completed Enter press) is
//! `App::maybe_trigger_terminal_retitle`'s concern, not this module's.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use ilium_core::{NodeId, NodeKind, PaneContentKind, PaneStatus};

use crate::app::App;

/// Retitle after every Nth completed Enter press in an eligible terminal
/// pane -- "once every 2 commands," per the product requirement.
pub const RETITLE_ENTER_INTERVAL: u32 = 2;

/// Hashes a captured terminal screen for `App::terminal_retitle_content_hashes`
/// -- lets `App::maybe_trigger_terminal_retitle` skip the LLM call when the
/// pane's visible content hasn't materially changed since the last automatic
/// retitle (e.g. two `ls` in a row on an otherwise-idle pane), instead of
/// firing on cadence alone every `RETITLE_ENTER_INTERVAL` commands regardless
/// of whether there's anything new to summarize.
pub fn hash_screen_text(screen_text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    screen_text.hash(&mut hasher);
    hasher.finish()
}

/// Whether `pane_id` is eligible for automatic terminal-screen LLM
/// retitling right now: a plain-shell terminal pane (not an agent, not an
/// editor) whose title hasn't been genuinely user-specified, and that
/// doesn't already have a retitle worker in flight.
pub fn terminal_ready_for_retitle(app: &App, pane_id: NodeId) -> bool {
    if app.titles_loading.contains(&pane_id) {
        return false;
    }
    matches!(
        app.tree.get(pane_id).map(|node| &node.kind),
        Some(NodeKind::Pane {
            content: PaneContentKind::Terminal,
            status: PaneStatus::PlainShell,
            title_source,
            ..
        }) if !title_source.is_user_specified()
    )
}

#[cfg(test)]
mod tests {
    use ilium_core::{AgentActivity, AgentClass, PaneContentKind, PaneStatus, ROOT_ID};

    use super::*;

    fn app_with_terminal_pane(status: PaneStatus) -> (App, NodeId) {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        app.tree.set_pane_status(pane_id, status).unwrap();
        (app, pane_id)
    }

    #[test]
    fn a_plain_shell_pane_is_eligible() {
        let (app, pane_id) = app_with_terminal_pane(PaneStatus::PlainShell);
        assert!(terminal_ready_for_retitle(&app, pane_id));
    }

    #[test]
    fn an_agent_pane_is_never_eligible() {
        let (app, pane_id) = app_with_terminal_pane(PaneStatus::Agent(
            AgentClass::Claude,
            AgentActivity::Working,
        ));
        assert!(!terminal_ready_for_retitle(&app, pane_id));
    }

    #[test]
    fn a_user_renamed_pane_is_never_eligible() {
        let (mut app, pane_id) = app_with_terminal_pane(PaneStatus::PlainShell);
        app.tree
            .rename_node(pane_id, "my custom name", None, None)
            .unwrap();
        assert!(!terminal_ready_for_retitle(&app, pane_id));
    }

    #[test]
    fn a_pane_already_loading_is_not_eligible_again() {
        let (mut app, pane_id) = app_with_terminal_pane(PaneStatus::PlainShell);
        app.titles_loading.insert(pane_id);
        assert!(!terminal_ready_for_retitle(&app, pane_id));
    }

    #[test]
    fn an_editor_pane_is_never_eligible() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "notes.md", PaneContentKind::Editor)
            .unwrap();
        assert!(!terminal_ready_for_retitle(&app, pane_id));
    }

    #[test]
    fn an_unknown_node_id_is_not_eligible() {
        let app = App::new("test".to_string(), std::env::temp_dir());
        assert!(!terminal_ready_for_retitle(&app, NodeId(9999)));
    }
}
