//! Desktop notification on a pane's `Working -> Done`/`Idle` transition
//! (README M5). Two pieces, deliberately kept apart: [`is_finished_transition`]
//! is the pure decision ("does this status change deserve a notification at
//! all") that a plain `#[test]` can exercise for every relevant status-pair
//! combination with no D-Bus/notification-daemon dependency; [`send`] is the
//! thin I/O adapter around `notify-rust` that actually shows one, and is
//! deliberately *not* unit tested here -- see its own doc comment.

use ilium_core::{AgentActivity, PaneStatus};

/// True if going from `previous` to `new` is "an agent just finished a
/// turn and this is the first classification to say so" -- i.e. `previous`
/// was `Agent(_, Working)` or `Agent(_, WaitingBackground)` (both "the
/// agent is busy" states -- a live foreground turn, or waiting on
/// background subagents it dispatched) and `new` is `Agent(_, Idle)` or
/// `Agent(_, Done)`. Deliberately narrow: `None` (a pane's first-ever
/// classification, e.g. right after it was created) never notifies, since
/// there is no prior busy turn that just ended; `Idle -> Working` (a turn
/// starting) never notifies, since nothing finished; and
/// `Working -> WaitingApproval`/`Working -> WaitingBackground` never notify
/// either -- the agent is blocked on the user or on background work, not
/// done, and `ilium_detect::classify_activity` already distinguishes
/// those cases from "finished" precisely so callers like this one don't
/// have to guess.
pub fn is_finished_transition(previous: Option<&PaneStatus>, new: &PaneStatus) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    matches!(
        previous,
        PaneStatus::Agent(_, AgentActivity::Working | AgentActivity::WaitingBackground)
    ) && matches!(
        new,
        PaneStatus::Agent(_, AgentActivity::Idle | AgentActivity::Done)
    )
}

/// A notification-worthy transition, queued during a detection tick and
/// sent once the tree/pane locks that produced it have been released (see
/// `detection::run_due_panes`) -- a slow or unavailable notification daemon
/// must never hold up an attached client's tree access.
pub struct PendingNotification {
    pub session_name: String,
    pub pane_name: String,
}

/// Shows a desktop notification for `pending`. `notify-rust`'s `show()` is
/// synchronous, blocking I/O (a D-Bus round trip via `zbus` on Linux) even
/// though this crate's own call sites are async, so the actual call runs on
/// a `spawn_blocking` thread rather than inline on a tokio worker thread --
/// see `CLAUDE.md`'s async-task rule and `detection.rs`'s identical
/// treatment of the `sysinfo` refresh for the same reason.
///
/// Never propagates a failure: no notification daemon/D-Bus session (this
/// sandboxed environment, most containers, some window managers) is a
/// normal, expected condition, not a bug, and must never affect pane
/// detection -- logged and continued, exactly like every other per-pane
/// failure in the detection loop (see `detection::run_due_panes`'s
/// `set_pane_status` error handling for the established pattern this
/// mirrors). Deliberately not unit tested: exercising it for real would
/// mean asserting a notification daemon is present, which is exactly the
/// environment-dependent flakiness the pure `is_finished_transition` above
/// exists to keep out of the test suite.
pub async fn send(pending: PendingNotification) {
    let result = tokio::task::spawn_blocking(move || {
        notify_rust::Notification::new()
            .summary(&format!("{} finished", pending.pane_name))
            .body(&format!(
                "Session \"{}\": the agent in \"{}\" is done and waiting on you.",
                pending.session_name, pending.pane_name
            ))
            .show()
    })
    .await;

    match result {
        Ok(Ok(_handle)) => {}
        Ok(Err(error)) => {
            tracing::warn!(
                "desktop notification failed (no notification daemon? continuing): {error}"
            );
        }
        Err(join_error) => {
            tracing::warn!("desktop notification task panicked (continuing): {join_error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::AgentClass;

    fn working() -> PaneStatus {
        PaneStatus::Agent(AgentClass::Claude, AgentActivity::Working)
    }
    fn idle() -> PaneStatus {
        PaneStatus::Agent(AgentClass::Claude, AgentActivity::Idle)
    }
    fn done() -> PaneStatus {
        PaneStatus::Agent(AgentClass::Claude, AgentActivity::Done)
    }
    fn waiting_approval() -> PaneStatus {
        PaneStatus::Agent(AgentClass::Claude, AgentActivity::WaitingApproval)
    }
    fn waiting_background() -> PaneStatus {
        PaneStatus::Agent(AgentClass::Claude, AgentActivity::WaitingBackground)
    }
    fn plain_shell() -> PaneStatus {
        PaneStatus::PlainShell
    }

    #[test]
    fn working_to_idle_notifies() {
        assert!(is_finished_transition(Some(&working()), &idle()));
    }

    #[test]
    fn working_to_done_notifies() {
        assert!(is_finished_transition(Some(&working()), &done()));
    }

    #[test]
    fn idle_to_working_does_not_notify() {
        assert!(!is_finished_transition(Some(&idle()), &working()));
    }

    #[test]
    fn done_to_working_does_not_notify() {
        assert!(!is_finished_transition(Some(&done()), &working()));
    }

    #[test]
    fn working_to_waiting_approval_does_not_notify() {
        assert!(!is_finished_transition(
            Some(&working()),
            &waiting_approval()
        ));
    }

    #[test]
    fn waiting_approval_to_idle_does_not_notify() {
        assert!(!is_finished_transition(Some(&waiting_approval()), &idle()));
    }

    #[test]
    fn working_to_waiting_background_does_not_notify() {
        assert!(!is_finished_transition(
            Some(&working()),
            &waiting_background()
        ));
    }

    #[test]
    fn waiting_background_to_idle_notifies() {
        assert!(is_finished_transition(Some(&waiting_background()), &idle()));
    }

    #[test]
    fn waiting_background_to_done_notifies() {
        assert!(is_finished_transition(Some(&waiting_background()), &done()));
    }

    #[test]
    fn waiting_background_to_waiting_background_does_not_notify() {
        assert!(!is_finished_transition(
            Some(&waiting_background()),
            &waiting_background()
        ));
    }

    #[test]
    fn idle_to_done_does_not_notify() {
        assert!(!is_finished_transition(Some(&idle()), &done()));
    }

    #[test]
    fn done_to_idle_does_not_notify() {
        assert!(!is_finished_transition(Some(&done()), &idle()));
    }

    #[test]
    fn working_to_working_does_not_notify() {
        assert!(!is_finished_transition(Some(&working()), &working()));
    }

    #[test]
    fn idle_to_idle_does_not_notify() {
        assert!(!is_finished_transition(Some(&idle()), &idle()));
    }

    #[test]
    fn working_to_plain_shell_does_not_notify() {
        assert!(!is_finished_transition(Some(&working()), &plain_shell()));
    }

    #[test]
    fn plain_shell_to_working_does_not_notify() {
        assert!(!is_finished_transition(Some(&plain_shell()), &working()));
    }

    #[test]
    fn first_ever_classification_never_notifies_even_if_it_looks_finished() {
        assert!(!is_finished_transition(None, &idle()));
        assert!(!is_finished_transition(None, &done()));
        assert!(!is_finished_transition(None, &working()));
    }

    #[test]
    fn working_to_idle_notifies_regardless_of_which_agent_class() {
        let claude_working = PaneStatus::Agent(AgentClass::Claude, AgentActivity::Working);
        let codex_idle = PaneStatus::Agent(AgentClass::Codex, AgentActivity::Idle);
        assert!(is_finished_transition(Some(&claude_working), &codex_idle));
    }
}
