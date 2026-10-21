//! The consent gate and dialog guards (reference-grant-review-and-consent.md): one consent dialog
//! at a time, rate limits, timeouts, and the input grace after a dialog opens.

use std::time::Duration;

/// Why a consent request did not get an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConsentOutcome {
    /// Another consent dialog is open.
    Busy,
    /// The plugin asked too often.
    RateLimited,
    /// The dialog timed out.
    TimedOut,
    /// The user declined.
    Declined,
}

/// Serialises consent dialogs and enforces their limits.
#[derive(Debug, Default)]
pub struct ConsentGate;

impl ConsentGate {
    /// A gate with no dialog open.
    pub fn new() -> ConsentGate {
        unimplemented!("host::consent::ConsentGate::new")
    }

    /// Whether plugin-drawn consent dialogs are accepted (never: the kernel draws them).
    pub fn accepts_plugin_drawn_dialogs(&self) -> bool {
        unimplemented!("host::consent::ConsentGate::accepts_plugin_drawn_dialogs")
    }

    /// Opens a consent dialog of `plugin` for `grant`.
    pub fn open(&mut self, _plugin: &str, _grant: &str) -> Result<(), ConsentOutcome> {
        unimplemented!("host::consent::ConsentGate::open")
    }

    /// Closes the open dialog with its outcome.
    pub fn close(&mut self, _outcome: ConsentOutcome) {
        unimplemented!("host::consent::ConsentGate::close")
    }

    /// Time passed; returns the outcome when the open dialog timed out.
    pub fn tick(&mut self, _elapsed: Duration) -> Option<ConsentOutcome> {
        unimplemented!("host::consent::ConsentGate::tick")
    }
}

/// Ignores input for a grace period after a dialog opens (no click-through).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogGuard {
    opened_at: Duration,
}

impl DialogGuard {
    /// A guard for a dialog opened at `opened_at`.
    pub fn opened_at(_opened_at: Duration) -> DialogGuard {
        unimplemented!("host::consent::DialogGuard::opened_at")
    }

    /// Whether input at `now` is accepted.
    pub fn accepts_input_at(&mut self, _now: Duration) -> bool {
        unimplemented!("host::consent::DialogGuard::accepts_input_at")
    }
}

/// The kernel-drawn dialogs of the grant workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DialogKind {
    /// The install-time grant review.
    Review,
    /// A runtime consent.
    Consent,
    /// An update review.
    Update,
    /// A publisher trust prompt.
    Trust,
}

impl DialogKind {
    /// Every kind is drawn by the kernel, never by a plugin.
    pub fn is_kernel_drawn(self) -> bool {
        unimplemented!("host::consent::DialogKind::is_kernel_drawn")
    }

    /// The label naming the source of the dialog.
    pub fn source_label(self) -> &'static str {
        unimplemented!("host::consent::DialogKind::source_label")
    }
}

#[cfg(test)]
mod tests {
    use super::{ConsentGate, ConsentOutcome, DialogGuard, DialogKind};
    use std::time::Duration;

    /// REQ: SEC-102
    #[test]
    fn sec_102_new_gate_refuses_plugin_drawn_dialogs() {
        let gate = ConsentGate::new();
        assert!(!gate.accepts_plugin_drawn_dialogs());
    }

    /// REQ: SEC-102
    #[test]
    fn sec_102_default_gate_refuses_plugin_drawn_dialogs() {
        let gate = ConsentGate::default();
        assert!(!gate.accepts_plugin_drawn_dialogs());
    }

    /// REQ: SEC-102
    #[test]
    fn sec_102_every_dialog_kind_is_kernel_drawn() {
        for kind in [
            DialogKind::Review,
            DialogKind::Consent,
            DialogKind::Update,
            DialogKind::Trust,
        ] {
            assert!(kind.is_kernel_drawn(), "{kind:?} must be drawn by the kernel");
        }
    }

    /// REQ: SEC-102
    #[test]
    fn sec_102_each_dialog_kind_has_its_own_nonempty_source_label() {
        let labels = [
            DialogKind::Review.source_label(),
            DialogKind::Consent.source_label(),
            DialogKind::Update.source_label(),
            DialogKind::Trust.source_label(),
        ];
        for label in labels {
            assert!(!label.trim().is_empty(), "source label must not be empty");
        }
        for (index, label) in labels.iter().enumerate() {
            for other in labels.iter().skip(index + 1) {
                assert_ne!(label, other, "two dialog kinds share a source label");
            }
        }
    }

    /// REQ: SEC-103
    #[test]
    fn sec_103_input_at_the_moment_a_dialog_opens_is_ignored() {
        let mut guard = DialogGuard::opened_at(Duration::from_secs(1));
        assert!(!guard.accepts_input_at(Duration::from_secs(1)));
    }

    /// REQ: SEC-103
    #[test]
    fn sec_103_input_that_arrived_before_the_dialog_was_drawn_is_ignored() {
        let mut guard = DialogGuard::opened_at(Duration::from_secs(1));
        assert!(!guard.accepts_input_at(Duration::from_millis(999)));
    }

    /// REQ: SEC-103
    #[test]
    fn sec_103_grace_window_ends_exactly_500_ms_after_opening() {
        let mut guard = DialogGuard::opened_at(Duration::from_secs(2));
        assert!(!guard.accepts_input_at(Duration::from_millis(2_499)));
        assert!(guard.accepts_input_at(Duration::from_millis(2_500)));
    }

    /// REQ: SEC-103
    #[test]
    fn sec_103_input_stays_accepted_long_after_the_grace_window() {
        let mut guard = DialogGuard::opened_at(Duration::ZERO);
        assert!(guard.accepts_input_at(Duration::from_secs(60)));
        assert!(guard.accepts_input_at(Duration::from_secs(3_600)));
    }

    /// REQ: SEC-105
    #[test]
    fn sec_105_a_second_prompt_for_the_same_plugin_is_busy() {
        let mut gate = ConsentGate::new();
        gate.open("test.plugin", "http:example.com")
            .expect("first prompt opens");
        let outcome = gate
            .open("test.plugin", "http:example.org")
            .expect_err("second prompt is refused while one is pending");
        assert_eq!(outcome, ConsentOutcome::Busy);
    }

    /// REQ: SEC-105
    #[test]
    fn sec_105_a_prompt_from_another_plugin_is_busy_while_a_dialog_is_open() {
        let mut gate = ConsentGate::new();
        gate.open("test.plugin", "http:example.com")
            .expect("first prompt opens");
        let outcome = gate
            .open("other.plugin", "http:example.org")
            .expect_err("only one consent dialog is open at a time");
        assert_eq!(outcome, ConsentOutcome::Busy);
    }

    /// REQ: SEC-105
    #[test]
    fn sec_105_a_closed_prompt_frees_the_gate_for_the_next_prompt() {
        let mut gate = ConsentGate::new();
        gate.open("test.plugin", "http:example.com")
            .expect("first prompt opens");
        gate.close(ConsentOutcome::Declined);
        gate.open("test.plugin", "http:example.org")
            .expect("next prompt opens after the first is closed");
    }

    /// REQ: SEC-105
    #[test]
    fn sec_105_six_prompts_in_an_hour_are_allowed_for_one_plugin() {
        let mut gate = ConsentGate::new();
        for prompt_number in 1..=6 {
            if let Err(outcome) = gate.open("test.plugin", "http:example.com") {
                panic!("prompt {prompt_number} of 6 was refused: {outcome:?}");
            }
            gate.close(ConsentOutcome::Declined);
        }
    }

    /// REQ: SEC-105
    #[test]
    fn sec_105_the_seventh_prompt_in_an_hour_is_rate_limited() {
        let mut gate = ConsentGate::new();
        for _ in 0..6 {
            gate.open("test.plugin", "http:example.com")
                .expect("the first six prompts open");
            gate.close(ConsentOutcome::Declined);
        }
        let outcome = gate
            .open("test.plugin", "http:example.com")
            .expect_err("the seventh prompt is refused");
        assert_eq!(outcome, ConsentOutcome::RateLimited);
    }

    /// REQ: SEC-105
    #[test]
    fn sec_105_rate_limit_is_per_plugin_and_a_refused_prompt_opens_no_dialog() {
        let mut gate = ConsentGate::new();
        for _ in 0..6 {
            gate.open("test.plugin", "http:example.com")
                .expect("the first six prompts open");
            gate.close(ConsentOutcome::Declined);
        }
        let outcome = gate
            .open("test.plugin", "http:example.com")
            .expect_err("the seventh prompt is refused");
        assert_eq!(outcome, ConsentOutcome::RateLimited);
        gate.open("other.plugin", "http:example.com")
            .expect("another plugin still gets its prompt, so no dialog was left open");
    }

    /// REQ: SEC-105
    #[test]
    fn sec_105_a_prompt_is_still_pending_just_before_120_seconds() {
        let mut gate = ConsentGate::new();
        gate.open("test.plugin", "http:example.com")
            .expect("prompt opens");
        assert_eq!(gate.tick(Duration::from_millis(119_999)), None);
    }

    /// REQ: SEC-105
    #[test]
    fn sec_105_a_prompt_times_out_at_120_seconds() {
        let mut gate = ConsentGate::new();
        gate.open("test.plugin", "http:example.com")
            .expect("prompt opens");
        assert_eq!(
            gate.tick(Duration::from_secs(120)),
            Some(ConsentOutcome::TimedOut)
        );
    }

    /// REQ: SEC-105
    #[test]
    fn sec_105_zero_elapsed_time_does_not_time_out_a_prompt() {
        let mut gate = ConsentGate::new();
        gate.open("test.plugin", "http:example.com")
            .expect("prompt opens");
        assert_eq!(gate.tick(Duration::ZERO), None);
    }

    /// REQ: SEC-105
    #[test]
    fn sec_105_tick_with_no_open_prompt_reports_nothing() {
        let mut gate = ConsentGate::new();
        assert_eq!(gate.tick(Duration::from_secs(600)), None);
    }

    /// REQ: SEC-105
    #[test]
    fn sec_105_an_answered_prompt_does_not_time_out_later() {
        let mut gate = ConsentGate::new();
        gate.open("test.plugin", "http:example.com")
            .expect("prompt opens");
        gate.close(ConsentOutcome::Declined);
        assert_eq!(gate.tick(Duration::from_secs(120)), None);
    }
}
