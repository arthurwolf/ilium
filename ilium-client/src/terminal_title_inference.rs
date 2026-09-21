//! Decides whether a plain terminal pane is currently eligible for
//! automatic LLM screen-text retitling -- the terminal-pane analogue of
//! `crate::title_inference`, which gates the same decision for agent panes
//! off a resolved session ID. A plain shell has neither a session ID nor a
//! transcript, so eligibility here is driven entirely by the pane's current
//! `PaneStatus`/`PaneTitleSource` plus whether a worker is already in
//! flight; the *cadence* (every second completed Enter press) is
//! the automatic trigger router's concern, not this module's.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use ilium_core::{NodeId, NodeKind, PaneContentKind, PaneStatus};

use crate::app::{App, PaneRuntime};
use crate::terminal_naming::TerminalTitleInput;

/// Retitle after every Nth completed Enter press in an eligible terminal
/// pane -- "once every 2 commands," per the product requirement.
pub const RETITLE_ENTER_INTERVAL: u32 = 2;

/// Rows read from each end of a terminal pane's accumulated scrollback
/// before the shared LLM-context character clip runs. Generous enough that
/// the character clip (`crate::naming::clip_llm_context_value`, ~4,000
/// characters per side) is almost always the binding limit in practice --
/// this row cap exists only so `full_history_contents_capped` never has to
/// read every row of a huge scrollback (up to `terminal.scrollback_budget_mib`,
/// configurable up to 512 MiB) while holding the pane's shared parser lock,
/// which a concurrent PTY reader thread is waiting on.
const TERMINAL_HISTORY_HEAD_ROWS: usize = 200;
const TERMINAL_HISTORY_TAIL_ROWS: usize = 200;

/// Hashes a captured terminal screen for `App::terminal_retitle_content_hashes`
/// -- lets the automatic trigger router skip the LLM call when the
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

/// Gathers `pane_id`'s current screen text plus its identity/project
/// metadata into one immutable input for `terminal_naming::infer_terminal_title`
/// -- the terminal-pane analogue of `crate::title_inference::session_title_input`.
/// Returns `None` only when the pane has no live terminal view to read a
/// screen from (e.g. it was closed between the eligibility check and this
/// call).
pub fn terminal_title_input(app: &App, pane_id: NodeId) -> Option<TerminalTitleInput> {
    let Some(PaneRuntime::Terminal(view)) = app.panes.get(&pane_id) else {
        return None;
    };
    // `full_history_contents_capped` (unlike `contents`) covers the pane's
    // entire scrollback, not just its current viewport -- naming a terminal
    // after only what's presently visible describes whatever it happened to
    // be doing last, not what it's generally for. The row cap keeps this
    // from reading every row of an untouched long-lived pane's scrollback
    // (which can run to hundreds of MiB, `terminal.scrollback_budget_mib`)
    // while holding the parser lock a concurrent PTY reader thread is
    // waiting on. The character clip that follows
    // (`crate::naming::clip_llm_context_value`, keeping both ends) applies
    // immediately rather than at the prompt-build boundary like
    // `session_naming` does: this value is hashed on every retitle-eligible
    // checkpoint (`App::queue_automatic_terminal_retitle`), and clipping here
    // keeps that hash, and every later clone of this input, bounded.
    let screen_text =
        view.with_screen(|screen| {
            crate::naming::clip_llm_context_value(&screen.full_history_contents_capped(
                TERMINAL_HISTORY_HEAD_ROWS,
                TERMINAL_HISTORY_TAIL_ROWS,
            ))
        });
    let node = app.tree.get(pane_id)?;
    let project_name = app
        .tree
        .project_ancestor(pane_id)
        .and_then(|project_id| app.tree.get(project_id))
        .map(|project| project.name.clone())
        .unwrap_or_default();
    let project_path = app
        .tree
        .project_path_for(pane_id)
        .unwrap_or(&app.session_cwd)
        .to_path_buf();
    Some(TerminalTitleInput {
        pane_id,
        project_name,
        project_path,
        current_title: node.name.clone(),
        screen_text,
    })
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
