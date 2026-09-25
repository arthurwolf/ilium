//! Decides when to (re)attempt background LLM session-title inference for
//! an agent pane -- a pure function of `App`'s current state plus what
//! `render_cache::apply` just did, kept separate from `crate::naming_workers`
//! (which only knows how to run one attempt) and from `crate::lib`'s event
//! loop (which is the only place with a `NamingWorkers` handle to actually
//! spawn one) so this decision stays unit-testable without a background
//! thread or a real provider call.
//!
//! Three triggers, all driven by [`AppliedEvent`] (what `render_cache::apply`
//! just observed):
//! - [`AppliedEvent::SessionIdResolved`] -- the first, most common trigger:
//!   the server just told this client an agent pane's session ID, right
//!   after which its transcript usually already has at least one prompt to
//!   summarize.
//! - [`AppliedEvent::PaneBecameAgent`] -- closes the event-ordering race where
//!   the ID reaches the client just before the first `PlainShell -> Agent`
//!   status event. Whichever event arrives second can start the worker.
//! - [`AppliedEvent::PaneBecameDone`] -- a turn just finished on a pane
//!   whose session ID is already known but a title hasn't been
//!   successfully inferred yet. This is the retry path that replaces the
//!   pre-client/server bin crate's permanent give-up-after-one-failed-attempt
//!   policy (`titles_inference_failed`, never retried for the rest of the
//!   run): that policy is what turned a merely-unlucky first attempt --
//!   e.g. session ID resolved (via a `/proc/<pid>/fd` scan) a moment before
//!   the CLI had written its first user-message event, so the transcript
//!   still looked empty -- into a pane that silently never got titled for
//!   the rest of the session. Retrying on every later `Done` transition,
//!   bounded by [`MAX_ATTEMPTS`], means a transient miss self-heals the
//!   next time the agent actually finishes a turn, instead of failing once
//!   and staying failed forever.

use ilium_core::{AgentClass, NodeId, NodeKind, PaneStatus};

use crate::app::{App, PaneRuntime};

/// Local navigation context for labels. Include the route to the parent so a
/// label can stand alone when its ancestors are collapsed in the tree.
pub fn nearby_title_context(app: &App, pane_id: NodeId) -> (String, Vec<String>) {
    let Some(parent_id) = app.tree.parent_of(pane_id) else {
        return (String::new(), Vec::new());
    };
    let mut ancestor_names = Vec::new();
    let mut ancestor_id = Some(parent_id);
    while let Some(id) = ancestor_id {
        if id == ilium_core::ROOT_ID {
            break;
        }
        let Some(node) = app.tree.get(id) else {
            break;
        };
        ancestor_names.push(node.name.clone());
        if node.is_project() {
            break;
        }
        ancestor_id = app.tree.parent_of(id);
    }
    ancestor_names.reverse();
    let ancestor_path = ancestor_names.join(" > ");

    let Ok(siblings) = app.tree.children_of(parent_id) else {
        return (ancestor_path, Vec::new());
    };
    // A bounded window around the pane keeps relevant neighbors even when a
    // large group has many older entries ahead of it.
    let target_index = siblings.iter().position(|id| *id == pane_id).unwrap_or(0);
    let window_start = target_index
        .saturating_sub(20)
        .min(siblings.len().saturating_sub(41));
    let nearby_titles = siblings
        .iter()
        .skip(window_start)
        .filter(|sibling_id| **sibling_id != pane_id)
        .filter_map(|sibling_id| app.tree.get(*sibling_id))
        .take(40)
        .map(|sibling| match sibling.short_name.as_deref() {
            Some(short) if short != sibling.name.as_str() => {
                format!("short: {short}; long: {}", sibling.name)
            }
            _ => sibling.name.clone(),
        })
        .collect();
    (ancestor_path, nearby_titles)
}

/// Bounds the retry path so a pane whose transcript genuinely never has
/// anything summarizable (e.g. a resumed session ilium can't read, or a
/// provider that's consistently down) doesn't retry on every single `Done`
/// transition for the rest of a long-running session.
pub const MAX_ATTEMPTS: u32 = 5;

/// Resolves the transcript-backed input for one configured automatic retitle.
/// Event selection is intentionally absent: the trigger router has already
/// decided *when* to act, while this function owns the stable eligibility
/// contract shared by every agent lifecycle event.
pub fn session_title_input(
    app: &App,
    pane_id: NodeId,
    allow_user_specified_title: bool,
) -> Option<crate::session_naming::SessionTitleInput> {
    if app.titles_loading.contains(&pane_id) {
        return None;
    }
    let session_id = app.agent_session_ids.get(&pane_id)?.clone();
    let node = app.tree.get(pane_id)?;
    let NodeKind::Pane {
        status,
        title_source,
        ..
    } = &node.kind
    else {
        return None;
    };
    if title_source.is_user_specified() && !allow_user_specified_title {
        return None;
    }
    let (class, activity, has_persistent_goal) = match status {
        PaneStatus::Agent(class, activity) => (class, activity, false),
        PaneStatus::AgentWithGoal(class, activity, _) => (class, activity, true),
        PaneStatus::PlainShell | PaneStatus::Editor { .. } | PaneStatus::Board => return None,
    };
    class.provider()?;
    let terminal_screen = match app.panes.get(&pane_id) {
        Some(PaneRuntime::Terminal(view)) => view.with_screen(|screen| screen.contents()),
        Some(PaneRuntime::Editor(_) | PaneRuntime::Board(_)) | None => String::new(),
    };
    let project_name = app
        .tree
        .project_ancestor(pane_id)
        .and_then(|project_id| app.tree.get(project_id))
        .map(|project| project.name.clone())
        .unwrap_or_default();
    let (parent_group, nearby_titles) = nearby_title_context(app, pane_id);
    Some(crate::session_naming::SessionTitleInput {
        pane_id,
        project_name,
        project_path: app
            .tree
            .project_path_for(pane_id)
            .unwrap_or(&app.session_cwd)
            .to_path_buf(),
        agent_class: class.clone(),
        session_id,
        process_id: app.agent_process_ids.get(&pane_id).copied(),
        current_title: node.name.clone(),
        current_short_title: node.short_name.clone(),
        current_icon: node.inferred_icon.clone(),
        title_source: *title_source,
        activity: *activity,
        has_persistent_goal,
        terminal_screen,
        parent_group,
        nearby_titles,
    })
}

/// What `render_cache::apply` just observed, as far as this module's two
/// triggers care -- everything else `apply` handles (tree snapshots,
/// screen updates, plain-shell status, errors) is [`AppliedEvent::Other`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppliedEvent {
    SessionIdResolved { pane_id: NodeId },
    PaneBecameAgent { pane_id: NodeId },
    PaneBecameDone { pane_id: NodeId },
    Other,
}

/// The pane/class/session-id title inference should be attempted for
/// right now, if any, given `applied` and `app`'s current state.
pub fn pane_ready_for_inference(
    app: &App,
    applied: &AppliedEvent,
) -> Option<(NodeId, AgentClass, String, u64)> {
    let pane_id = match applied {
        AppliedEvent::SessionIdResolved { pane_id }
        | AppliedEvent::PaneBecameAgent { pane_id }
        | AppliedEvent::PaneBecameDone { pane_id } => *pane_id,
        AppliedEvent::Other => return None,
    };

    if app.titles_loading.contains(&pane_id) {
        return None;
    }
    let session_id = app.agent_session_ids.get(&pane_id)?.clone();
    if app.inferred_title_session_ids.get(&pane_id) == Some(&session_id) {
        return None;
    }
    if app
        .title_inference_attempts
        .get(&(pane_id, session_id.clone()))
        .copied()
        .unwrap_or(0)
        >= MAX_ATTEMPTS
    {
        return None;
    }
    let NodeKind::Pane {
        status: PaneStatus::Agent(class, _) | PaneStatus::AgentWithGoal(class, _, _),
        title_source,
        ..
    } = &app.tree.get(pane_id)?.kind
    else {
        return None;
    };
    if title_source.is_user_specified() {
        return None;
    }
    class.provider()?;
    Some((
        pane_id,
        class.clone(),
        session_id,
        app.agent_title_generations
            .get(&pane_id)
            .copied()
            .unwrap_or(0),
    ))
}

#[cfg(test)]
mod tests {
    use ilium_core::{AgentActivity, AgentClass, PaneContentKind, PaneStatus, ROOT_ID};

    use super::*;

    const SESSION_ID: &str = "95fd0645-3331-408b-a7e5-36e6007bfb78";

    fn app_with_agent_pane(class: AgentClass, activity: AgentActivity) -> (App, NodeId) {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "claude", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_status(pane_id, PaneStatus::Agent(class, activity))
            .unwrap();
        (app, pane_id)
    }

    #[test]
    fn nearby_context_includes_ancestors_and_visible_short_titles() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        let parent = app.tree.add_group(ROOT_ID, "components").unwrap();
        let nested = app.tree.add_group(parent, "catalog").unwrap();
        let sibling = app
            .tree
            .add_pane(nested, "Cut Paper Component", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .rename_node(
                sibling,
                "Cut Paper Component",
                Some("CUT PAPER".to_string()),
                None,
            )
            .unwrap();
        let target = app
            .tree
            .add_pane(nested, "Coding Session", PaneContentKind::Terminal)
            .unwrap();

        let (path, nearby) = nearby_title_context(&app, target);
        assert_eq!(path, "components > catalog");
        assert_eq!(nearby, vec!["short: CUT PAPER; long: Cut Paper Component"]);
    }

    #[test]
    fn nearby_context_keeps_the_target_end_of_a_large_group() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        for index in 0..60 {
            app.tree
                .add_pane(
                    group,
                    format!("neighbor {index}"),
                    PaneContentKind::Terminal,
                )
                .unwrap();
        }
        let target = app
            .tree
            .add_pane(group, "Coding Session", PaneContentKind::Terminal)
            .unwrap();

        let (_, nearby) = nearby_title_context(&app, target);
        assert_eq!(nearby.len(), 40);
        assert!(nearby.contains(&"neighbor 59".to_string()));
        assert!(!nearby.contains(&"neighbor 0".to_string()));
    }

    #[test]
    fn session_id_resolved_triggers_inference_for_an_eligible_pane() {
        let (mut app, pane_id) = app_with_agent_pane(AgentClass::Claude, AgentActivity::Working);
        app.agent_session_ids
            .insert(pane_id, SESSION_ID.to_string());

        let result = pane_ready_for_inference(&app, &AppliedEvent::SessionIdResolved { pane_id });

        assert_eq!(
            result,
            Some((pane_id, AgentClass::Claude, SESSION_ID.to_string(), 0))
        );
    }

    #[test]
    fn other_events_never_trigger_inference() {
        let (mut app, pane_id) = app_with_agent_pane(AgentClass::Claude, AgentActivity::Working);
        app.agent_session_ids
            .insert(pane_id, SESSION_ID.to_string());

        assert_eq!(pane_ready_for_inference(&app, &AppliedEvent::Other), None);
    }

    #[test]
    fn no_session_id_yet_is_not_eligible() {
        let (app, pane_id) = app_with_agent_pane(AgentClass::Claude, AgentActivity::Working);

        assert_eq!(
            pane_ready_for_inference(&app, &AppliedEvent::SessionIdResolved { pane_id }),
            None
        );
    }

    #[test]
    fn a_user_renamed_pane_is_never_eligible() {
        let (mut app, pane_id) = app_with_agent_pane(AgentClass::Claude, AgentActivity::Working);
        app.agent_session_ids
            .insert(pane_id, SESSION_ID.to_string());
        app.tree
            .rename_node(pane_id, "my custom name", None, None)
            .unwrap();

        assert_eq!(
            pane_ready_for_inference(&app, &AppliedEvent::SessionIdResolved { pane_id }),
            None
        );
    }

    #[test]
    fn an_other_agent_class_is_not_eligible() {
        let (mut app, pane_id) = app_with_agent_pane(
            AgentClass::Other("opencode".to_string()),
            AgentActivity::Idle,
        );
        app.agent_session_ids
            .insert(pane_id, SESSION_ID.to_string());

        assert_eq!(
            pane_ready_for_inference(&app, &AppliedEvent::SessionIdResolved { pane_id }),
            None
        );
    }

    #[test]
    fn already_loading_is_not_eligible_again() {
        let (mut app, pane_id) = app_with_agent_pane(AgentClass::Claude, AgentActivity::Working);
        app.agent_session_ids
            .insert(pane_id, SESSION_ID.to_string());
        app.titles_loading.insert(pane_id);

        assert_eq!(
            pane_ready_for_inference(&app, &AppliedEvent::PaneBecameDone { pane_id }),
            None
        );
    }

    #[test]
    fn a_pane_already_successfully_titled_is_never_retried_for_the_same_session() {
        let (mut app, pane_id) = app_with_agent_pane(AgentClass::Claude, AgentActivity::Done);
        app.agent_session_ids
            .insert(pane_id, SESSION_ID.to_string());
        app.inferred_title_session_ids
            .insert(pane_id, SESSION_ID.to_string());

        assert_eq!(
            pane_ready_for_inference(&app, &AppliedEvent::PaneBecameDone { pane_id }),
            None
        );
    }

    #[test]
    fn a_changed_session_id_is_eligible_after_the_previous_session_was_titled() {
        let (mut app, pane_id) = app_with_agent_pane(AgentClass::Claude, AgentActivity::Done);
        app.agent_session_ids
            .insert(pane_id, "old-session".to_string());
        app.inferred_title_session_ids
            .insert(pane_id, "old-session".to_string());
        app.agent_session_ids
            .insert(pane_id, SESSION_ID.to_string());

        assert_eq!(
            pane_ready_for_inference(&app, &AppliedEvent::SessionIdResolved { pane_id }),
            Some((pane_id, AgentClass::Claude, SESSION_ID.to_string(), 0))
        );
    }

    #[test]
    fn pane_became_done_retries_up_to_the_attempt_cap_then_stops() {
        let (mut app, pane_id) = app_with_agent_pane(AgentClass::Claude, AgentActivity::Done);
        app.agent_session_ids
            .insert(pane_id, SESSION_ID.to_string());

        for attempt in 0..MAX_ATTEMPTS {
            app.title_inference_attempts
                .insert((pane_id, SESSION_ID.to_string()), attempt);
            assert!(
                pane_ready_for_inference(&app, &AppliedEvent::PaneBecameDone { pane_id }).is_some(),
                "expected attempt {attempt} (below the cap) to still be eligible"
            );
        }

        app.title_inference_attempts
            .insert((pane_id, SESSION_ID.to_string()), MAX_ATTEMPTS);
        assert_eq!(
            pane_ready_for_inference(&app, &AppliedEvent::PaneBecameDone { pane_id }),
            None,
            "expected the cap to stop further retries"
        );
    }
}
