//! The kernel half of the crash-recovery question.
//!
//! With the recovery policy "Ask before restoring" the server finds a stored snapshot, sends
//! `SessionRecoveryAvailable` and holds the attach until the client answers
//! `ResolveSessionRecovery`. The question itself ("Restore previous session?", its wording and
//! its Discard and Restore buttons) belongs to the Sessions plugin: the kernel runs the plugin's
//! command `session-recovery` with the stored pane count and carries out the answer, the kernel
//! command `kernel.resolve-session-recovery`. The kernel keeps only what a plugin cannot own:
//! the obligation to answer the server.
//!
//! - The question opens only over the normal view, so it never lands on top of another dialog
//!   and the first-run setup offers wait for it.
//! - A popup the user closes without a button, or one the kernel could not show, answers
//!   nothing; the kernel asks again.
//! - Without the plugin (disabled, or no plugin host) there is nobody to ask, so the kernel
//!   restores the snapshot, which is what the default policy does anyway.

use std::time::{Duration, Instant};

use ilium_ipc::ClientRequest;
use ilium_plugin_api::generated::types::CommandOrigin;

use crate::app::{App, Mode};

/// The plugin that owns the question.
pub(crate) const SESSIONS_PLUGIN: &str = "ilium.core.sessions";
/// The id of its command inside the plugin.
const QUESTION_COMMAND_ID: &str = "session-recovery";
/// The full id of the command the kernel runs.
pub(crate) const QUESTION_COMMAND: &str = "ilium.core.sessions/session-recovery";
/// The popup the command opens.
const QUESTION_POPUP: &str = "session-recovery";
/// The kernel command with which the plugin answers.
pub(crate) const RESOLVE_COMMAND: &str = "kernel.resolve-session-recovery";

/// How long the kernel waits for the popup to show before it asks again. The plugin host loads
/// and activates plugins on its own thread, so the first ask can wait behind that.
const ASK_TIMEOUT: Duration = Duration::from_secs(20);
/// After this many asks that never produced a popup the kernel stops waiting and restores.
const MAXIMUM_UNANSWERED_ASKS: u8 = 3;

/// The restore-or-discard decision the server waits for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingSessionRecovery {
    pane_count: usize,
    /// When the plugin was last asked; `None` while the question still has to be asked.
    asked_at: Option<Instant>,
    /// How many asks timed out without a popup ever showing.
    unanswered_asks: u8,
}

impl PendingSessionRecovery {
    /// A decision for a snapshot of `pane_count` panes that was not asked yet.
    pub(crate) fn new(pane_count: usize) -> Self {
        Self {
            pane_count,
            asked_at: None,
            unanswered_asks: 0,
        }
    }
}

/// The restore flag of a `kernel.resolve-session-recovery` invocation: `{"restore": true}`.
pub(crate) fn restore_argument(arguments_json: &str) -> Result<bool, String> {
    const USAGE: &str = "kernel.resolve-session-recovery takes {\"restore\": true|false}";
    let value: serde_json::Value =
        serde_json::from_str(arguments_json).map_err(|error| format!("{USAGE}: {error}"))?;
    value
        .get("restore")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| USAGE.to_owned())
}

impl App {
    /// The server found a stored snapshot and waits for the decision.
    pub(crate) fn begin_session_recovery(&mut self, pane_count: usize) {
        self.session_recovery_pending = Some(PendingSessionRecovery::new(pane_count));
        self.drive_session_recovery(Instant::now());
    }

    /// Tells the server what to do with the stored snapshot.
    pub fn resolve_session_recovery(&mut self, restore: bool) {
        self.queue_request(ClientRequest::ResolveSessionRecovery { restore });
    }

    /// Whether an enabled plugin offers the question.
    fn session_recovery_question_available(&self) -> bool {
        self.wasm_plugins.as_ref().is_some_and(|plugins| {
            plugins
                .contributed("commands")
                .iter()
                .any(|item| item.plugin == SESSIONS_PLUGIN && item.id == QUESTION_COMMAND_ID)
        })
    }

    /// Asks the question when it is due, and restores when nobody can ask it. Runs when the
    /// server reports the snapshot and on every tick. Returns whether anything changed.
    pub(crate) fn drive_session_recovery(&mut self, now: Instant) -> bool {
        let Some(mut pending) = self.session_recovery_pending.take() else {
            return false;
        };
        if !self.session_recovery_question_available() {
            self.resolve_session_recovery(true);
            return true;
        }
        let normal_view = matches!(self.mode, Mode::Normal) && self.onboarding.is_none();
        if let Some(asked_at) = pending.asked_at {
            if !normal_view || now.saturating_duration_since(asked_at) < ASK_TIMEOUT {
                self.session_recovery_pending = Some(pending);
                return false;
            }
            pending.unanswered_asks += 1;
            if pending.unanswered_asks >= MAXIMUM_UNANSWERED_ASKS {
                self.status_message = Some(
                    "The session recovery question did not open; restored the snapshot".into(),
                );
                self.resolve_session_recovery(true);
                return true;
            }
            pending.asked_at = None;
        }
        if !normal_view {
            self.session_recovery_pending = Some(pending);
            return false;
        }
        pending.asked_at = Some(now);
        let arguments = serde_json::json!({ "pane_count": pending.pane_count }).to_string();
        self.session_recovery_pending = Some(pending);
        self.invoke_plugin_command(QUESTION_COMMAND, arguments, CommandOrigin::Cli);
        true
    }

    /// The plugin answered (`kernel.resolve-session-recovery`). Only the Sessions plugin can
    /// answer, and only while the server waits; any other call changes nothing.
    pub(crate) fn answer_session_recovery(&mut self, plugin: &str, restore: bool) -> bool {
        if plugin != SESSIONS_PLUGIN || self.session_recovery_pending.take().is_none() {
            return false;
        }
        self.resolve_session_recovery(restore);
        true
    }

    /// A popup went away without the plugin's answer (the user pressed Escape or clicked
    /// outside it, or another dialog kept it from showing): the question is asked again.
    pub(crate) fn session_recovery_popup_gone(&mut self, plugin: &str, popup: &str) {
        if plugin == SESSIONS_PLUGIN && popup == QUESTION_POPUP {
            if let Some(pending) = self.session_recovery_pending.as_mut() {
                pending.asked_at = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        App::new("session-recovery".to_owned(), std::env::temp_dir())
    }

    fn requests(app: &mut App) -> Vec<ClientRequest> {
        app.take_outbound_requests()
    }

    #[test]
    fn the_answer_argument_is_a_restore_flag() {
        assert_eq!(restore_argument(r#"{"restore": true}"#), Ok(true));
        assert_eq!(restore_argument(r#"{"restore": false}"#), Ok(false));
        for bad in ["", "[]", "{}", r#"{"restore": "yes"}"#, "true"] {
            assert!(restore_argument(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn without_the_plugin_the_kernel_restores_at_once() {
        let mut app = app();
        app.begin_session_recovery(4);
        assert!(app.session_recovery_pending.is_none());
        assert_eq!(
            requests(&mut app),
            vec![ClientRequest::ResolveSessionRecovery { restore: true }]
        );
        assert!(matches!(app.mode, Mode::Normal));
    }

    #[test]
    fn only_the_sessions_plugin_answers_and_only_while_the_server_waits() {
        let mut app = app();
        assert!(!app.answer_session_recovery(SESSIONS_PLUGIN, true));
        assert!(requests(&mut app).is_empty());

        app.session_recovery_pending = Some(PendingSessionRecovery::new(2));
        assert!(!app.answer_session_recovery("other.plugin", true));
        assert!(app.session_recovery_pending.is_some());
        assert!(requests(&mut app).is_empty());

        assert!(app.answer_session_recovery(SESSIONS_PLUGIN, false));
        assert!(app.session_recovery_pending.is_none());
        assert_eq!(
            requests(&mut app),
            vec![ClientRequest::ResolveSessionRecovery { restore: false }]
        );
        // The decision is final: a second answer changes nothing.
        assert!(!app.answer_session_recovery(SESSIONS_PLUGIN, true));
        assert!(requests(&mut app).is_empty());
    }

    #[test]
    fn a_popup_that_went_away_without_an_answer_is_asked_again() {
        let mut app = app();
        let mut pending = PendingSessionRecovery::new(2);
        pending.asked_at = Some(Instant::now());
        app.session_recovery_pending = Some(pending);

        app.session_recovery_popup_gone("other.plugin", QUESTION_POPUP);
        app.session_recovery_popup_gone(SESSIONS_PLUGIN, "other-popup");
        assert!(app
            .session_recovery_pending
            .as_ref()
            .is_some_and(|pending| pending.asked_at.is_some()));

        app.session_recovery_popup_gone(SESSIONS_PLUGIN, QUESTION_POPUP);
        assert!(app
            .session_recovery_pending
            .as_ref()
            .is_some_and(|pending| pending.asked_at.is_none()));
        assert!(requests(&mut app).is_empty());
    }
}
