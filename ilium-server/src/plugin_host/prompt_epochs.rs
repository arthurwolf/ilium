//! The prompt epoch of each terminal pane and the `if-current` precondition of `send-input`
//! (capability-agent-read-prompts.md, primitives-terminal-input.md).
//!
//! The kernel keeps one counter per pane, starting at 0, and increments it each time it records
//! a submitted prompt, at the moment it emits `input-submitted`. A plugin reads it with
//! `prompt-epoch` and passes it back on a submission; a stale value is `refused(conflict)`.
//! The record also remembers the submissions plugins made, so the next recorded prompt of that
//! pane reports the source `plugin` instead of `user` (EVT-040).

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use ilium_plugin_api::generated::types::events::{InputSource, InputSubmittedEvent, UserLineEvent};
use ilium_plugin_api::generated::types::intents::InputPrecondition;
use ilium_plugin_api::generated::types::tree::AgentActivity;
use ilium_plugin_api::generated::types::{EpochMs, Event, NodeId};

/// Panes whose counter is kept at once; the oldest pane ids go first (pane ids only grow).
const PANES_KEPT: usize = 4096;
/// How long a plugin's submission waits for the prompt the kernel records from it: the 10 s a
/// staged `send-input` may wait, plus time for the agent to take the prompt. A plugin
/// submission to a pane whose prompt is never recorded (a plain shell) expires instead of
/// labelling a later prompt of the user.
const PLUGIN_SUBMISSION_WINDOW: Duration = Duration::from_secs(20);
/// Plugin submissions remembered per pane.
const PLUGIN_SUBMISSIONS_KEPT: usize = 16;

#[derive(Debug, Default)]
struct PaneEpoch {
    epoch: u64,
    plugin_submissions: VecDeque<Instant>,
}

/// The prompt epochs of a session's panes.
#[derive(Debug, Default)]
pub(crate) struct PromptEpochs {
    panes: Mutex<BTreeMap<u64, PaneEpoch>>,
}

impl PromptEpochs {
    fn with<R>(&self, read: impl FnOnce(&mut BTreeMap<u64, PaneEpoch>) -> R) -> R {
        let mut panes = self
            .panes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        read(&mut panes)
    }

    /// The current epoch of a pane; 0 until its first recorded prompt.
    pub(crate) fn current(&self, pane: u64) -> u64 {
        self.with(|panes| panes.get(&pane).map_or(0, |entry| entry.epoch))
    }

    /// Notes that a plugin submitted a prompt to `pane` at `now`.
    pub(crate) fn note_plugin_submission(&self, pane: u64, now: Instant) {
        self.with(|panes| {
            let entry = panes.entry(pane).or_default();
            if entry.plugin_submissions.len() >= PLUGIN_SUBMISSIONS_KEPT {
                entry.plugin_submissions.pop_front();
            }
            entry.plugin_submissions.push_back(now);
            trim(panes);
        });
    }

    /// Records one submitted prompt of `pane` at `now`: increments the epoch and returns the
    /// new value with the source, `plugin` when a plugin submission is still waiting for it.
    pub(crate) fn record_prompt(&self, pane: u64, now: Instant) -> (u64, InputSource) {
        self.with(|panes| {
            let entry = panes.entry(pane).or_default();
            entry.epoch = entry.epoch.saturating_add(1);
            while entry
                .plugin_submissions
                .front()
                .is_some_and(|at| now.saturating_duration_since(*at) > PLUGIN_SUBMISSION_WINDOW)
            {
                entry.plugin_submissions.pop_front();
            }
            let source = if entry.plugin_submissions.pop_front().is_some() {
                InputSource::Plugin
            } else {
                InputSource::User
            };
            let epoch = entry.epoch;
            trim(panes);
            (epoch, source)
        })
    }
}

fn trim(panes: &mut BTreeMap<u64, PaneEpoch>) {
    while panes.len() > PANES_KEPT {
        panes.pop_first();
    }
}

/// The events of one recorded prompt, in delivery order: `input-submitted` for a pane with an
/// identified agent, then `user-line-submitted` when the user typed it. A prompt a plugin sent
/// fires `input-submitted` only (EVT-040).
pub(crate) fn prompt_events(
    pane: u64,
    text: String,
    (epoch, source): (u64, InputSource),
    has_agent: bool,
    at: EpochMs,
) -> Vec<Event> {
    let mut batch = Vec::with_capacity(2);
    if has_agent {
        batch.push(Event::InputSubmitted(InputSubmittedEvent {
            node: NodeId(pane),
            text: text.clone(),
            source,
            epoch,
        }));
    }
    if source == InputSource::User {
        batch.push(Event::UserLineSubmitted(UserLineEvent {
            pane: NodeId(pane),
            text,
            at,
        }));
    }
    batch
}

/// What the kernel knows of a pane when a `send-input` with `if-current` arrives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PaneNow {
    /// The activity of the pane's agent, `none` without one.
    pub activity: AgentActivity,
    /// The revision `read-screen` and `read-cells` report, when the pane has a live screen.
    pub screen_revision: Option<u64>,
    /// The name of the foreground process, when the kernel can see it.
    pub foreground: Option<String>,
}

/// The first condition of `wanted` the pane does not meet, named for the `conflict` message,
/// or `None` when every stated condition holds. An unknown revision or foreground never meets
/// a stated one.
pub(crate) fn precondition_failure(wanted: &InputPrecondition, now: &PaneNow) -> Option<String> {
    if let Some(activity) = wanted.activity {
        if activity != now.activity {
            return Some(format!(
                "if-current activity: the agent is {:?}, not {activity:?}",
                now.activity
            ));
        }
    }
    if let Some(revision) = wanted.screen_revision {
        if now.screen_revision != Some(revision) {
            return Some(match now.screen_revision {
                Some(current) => {
                    format!(
                        "if-current screen-revision: the screen is at {current}, not {revision}"
                    )
                }
                None => "if-current screen-revision: the pane has no screen".to_owned(),
            });
        }
    }
    if let Some(foreground) = &wanted.foreground {
        if now.foreground.as_deref() != Some(foreground.as_str()) {
            return Some(match &now.foreground {
                Some(current) => {
                    format!("if-current foreground: the foreground process is {current}, not {foreground}")
                }
                None => "if-current foreground: the foreground process is unknown".to_owned(),
            });
        }
    }
    None
}

/// The `conflict` message of a submission whose `prompt-epoch` is not the pane's current one.
pub(crate) fn stale_epoch(wanted: Option<u64>, current: u64) -> Option<String> {
    wanted
        .filter(|epoch| *epoch != current)
        .map(|epoch| format!("prompt-epoch {epoch} is stale; the pane is at {current}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane_now() -> PaneNow {
        PaneNow {
            activity: AgentActivity::Idle,
            screen_revision: Some(41),
            foreground: Some("codex".to_owned()),
        }
    }

    #[test]
    fn the_epoch_starts_at_zero_and_grows_once_per_recorded_prompt() {
        let epochs = PromptEpochs::default();
        let now = Instant::now();
        assert_eq!(epochs.current(7), 0);
        assert_eq!(epochs.record_prompt(7, now).0, 1);
        assert_eq!(epochs.record_prompt(7, now).0, 2);
        assert_eq!(epochs.current(7), 2);
        assert_eq!(epochs.current(8), 0, "every pane has its own counter");
    }

    #[test]
    fn a_prompt_after_a_plugin_submission_reports_the_plugin_once() {
        let epochs = PromptEpochs::default();
        let now = Instant::now();
        epochs.note_plugin_submission(3, now);
        assert_eq!(epochs.record_prompt(3, now), (1, InputSource::Plugin));
        assert_eq!(epochs.record_prompt(3, now), (2, InputSource::User));
    }

    #[test]
    fn a_plugin_submission_that_was_never_recorded_expires() {
        let epochs = PromptEpochs::default();
        let then = Instant::now();
        epochs.note_plugin_submission(3, then);
        let later = then + PLUGIN_SUBMISSION_WINDOW + Duration::from_secs(1);
        assert_eq!(epochs.record_prompt(3, later), (1, InputSource::User));
    }

    #[test]
    fn a_plugin_submission_does_not_move_the_epoch() {
        let epochs = PromptEpochs::default();
        epochs.note_plugin_submission(5, Instant::now());
        assert_eq!(epochs.current(5), 0);
    }

    /// REQ: EVT-040
    #[test]
    fn a_plugin_prompt_fires_input_submitted_only() {
        let batch = prompt_events(
            9,
            "fix the build".to_owned(),
            (3, InputSource::Plugin),
            true,
            EpochMs(5),
        );
        assert_eq!(batch.len(), 1);
        let Event::InputSubmitted(event) = &batch[0] else {
            panic!("input-submitted, not {batch:?}");
        };
        assert_eq!(
            (event.node, event.source, event.epoch),
            (NodeId(9), InputSource::Plugin, 3)
        );
        assert_eq!(event.text, "fix the build");
    }

    /// REQ: EVT-040
    #[test]
    fn a_user_prompt_fires_input_submitted_then_the_user_line() {
        let batch = prompt_events(
            9,
            "hello".to_owned(),
            (1, InputSource::User),
            true,
            EpochMs(5),
        );
        assert!(matches!(batch[0], Event::InputSubmitted(_)), "{batch:?}");
        assert!(matches!(batch[1], Event::UserLineSubmitted(_)), "{batch:?}");
        assert_eq!(batch.len(), 2);
        let plain = prompt_events(
            4,
            "ls".to_owned(),
            (1, InputSource::User),
            false,
            EpochMs(5),
        );
        assert_eq!(
            plain.len(),
            1,
            "a pane without an agent has no input-submitted"
        );
        assert!(matches!(plain[0], Event::UserLineSubmitted(_)), "{plain:?}");
    }

    #[test]
    fn only_a_stale_stated_epoch_conflicts() {
        assert_eq!(stale_epoch(None, 4), None);
        assert_eq!(stale_epoch(Some(4), 4), None);
        let message = stale_epoch(Some(3), 4).expect("stale");
        assert!(message.contains("prompt-epoch 3"), "{message}");
    }

    #[test]
    fn an_empty_precondition_always_holds() {
        let wanted = InputPrecondition {
            activity: None,
            screen_revision: None,
            foreground: None,
        };
        assert_eq!(precondition_failure(&wanted, &pane_now()), None);
    }

    #[test]
    fn each_failed_condition_is_named_in_check_order() {
        let all = InputPrecondition {
            activity: Some(AgentActivity::Idle),
            screen_revision: Some(41),
            foreground: Some("codex".to_owned()),
        };
        assert_eq!(precondition_failure(&all, &pane_now()), None);
        let working = PaneNow {
            activity: AgentActivity::Working,
            screen_revision: Some(40),
            ..pane_now()
        };
        let message = precondition_failure(&all, &working).expect("activity differs");
        assert!(message.starts_with("if-current activity"), "{message}");
        let moved = PaneNow {
            screen_revision: Some(42),
            ..pane_now()
        };
        let message = precondition_failure(&all, &moved).expect("revision differs");
        assert!(
            message.starts_with("if-current screen-revision"),
            "{message}"
        );
        let shell = PaneNow {
            foreground: Some("zsh".to_owned()),
            ..pane_now()
        };
        let message = precondition_failure(&all, &shell).expect("foreground differs");
        assert!(message.contains("zsh"), "{message}");
    }

    #[test]
    fn an_unknown_screen_or_process_never_meets_a_stated_condition() {
        let unknown = PaneNow {
            activity: AgentActivity::None,
            screen_revision: None,
            foreground: None,
        };
        let revision = InputPrecondition {
            activity: None,
            screen_revision: Some(0),
            foreground: None,
        };
        assert!(precondition_failure(&revision, &unknown).is_some());
        let foreground = InputPrecondition {
            activity: None,
            screen_revision: None,
            foreground: Some("codex".to_owned()),
        };
        assert!(precondition_failure(&foreground, &unknown).is_some());
    }
}
