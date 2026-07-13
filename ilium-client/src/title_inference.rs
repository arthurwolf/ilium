//! Decides when to (re)attempt background LLM session-title inference for
//! an agent pane -- a pure function of `App`'s current state plus what
//! `render_cache::apply` just did, kept separate from `crate::naming_workers`
//! (which only knows how to run one attempt) and from `crate::lib`'s event
//! loop (which is the only place with a `NamingWorkers` handle to actually
//! spawn one) so this decision stays unit-testable without a background
//! thread or a real gateway call.
//!
//! Two triggers, both driven by [`AppliedEvent`] (what `render_cache::apply`
//! just observed):
//! - [`AppliedEvent::SessionIdResolved`] -- the first, most common trigger:
//!   the server just told this client an agent pane's session ID, right
//!   after which its transcript usually already has at least one prompt to
//!   summarize.
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

use crate::app::App;

/// Bounds the retry path so a pane whose transcript genuinely never has
/// anything summarizable (e.g. a resumed session ilium can't read, or a
/// gateway that's consistently down) doesn't retry on every single `Done`
/// transition for the rest of a long-running session.
pub const MAX_ATTEMPTS: u32 = 5;

/// What `render_cache::apply` just observed, as far as this module's two
/// triggers care -- everything else `apply` handles (tree snapshots,
/// screen updates, plain-shell status, errors) is [`AppliedEvent::Other`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppliedEvent {
    SessionIdResolved { pane_id: NodeId },
    PaneBecameDone { pane_id: NodeId },
    Other,
}

/// The pane/class/session-id title inference should be attempted for
/// right now, if any, given `applied` and `app`'s current state.
pub fn pane_ready_for_inference(
    app: &App,
    applied: &AppliedEvent,
) -> Option<(NodeId, AgentClass, String)> {
    let pane_id = match applied {
        AppliedEvent::SessionIdResolved { pane_id } | AppliedEvent::PaneBecameDone { pane_id } => {
            *pane_id
        }
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
        status: PaneStatus::Agent(class, _),
        title_source,
        ..
    } = &app.tree.get(pane_id)?.kind
    else {
        return None;
    };
    if title_source.is_user_specified() {
        return None;
    }
    if !matches!(class, AgentClass::Claude | AgentClass::Codex) {
        return None;
    }
    Some((pane_id, class.clone(), session_id))
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
    fn session_id_resolved_triggers_inference_for_an_eligible_pane() {
        let (mut app, pane_id) = app_with_agent_pane(AgentClass::Claude, AgentActivity::Working);
        app.agent_session_ids
            .insert(pane_id, SESSION_ID.to_string());

        let result = pane_ready_for_inference(&app, &AppliedEvent::SessionIdResolved { pane_id });

        assert_eq!(
            result,
            Some((pane_id, AgentClass::Claude, SESSION_ID.to_string()))
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
            .rename_node(pane_id, "my custom name", None)
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
            Some((pane_id, AgentClass::Claude, SESSION_ID.to_string()))
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
