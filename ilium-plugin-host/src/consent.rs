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
pub struct ConsentGate {
    open: Option<(String, String)>,
    elapsed: Duration,
}

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
    pub fn open(&mut self, plugin: &str, grant: &str) -> Result<(), ConsentOutcome> {
        unimplemented!("host::consent::ConsentGate::open")
    }

    /// Closes the open dialog with its outcome.
    pub fn close(&mut self, outcome: ConsentOutcome) {
        unimplemented!("host::consent::ConsentGate::close")
    }

    /// Time passed; returns the outcome when the open dialog timed out.
    pub fn tick(&mut self, elapsed: Duration) -> Option<ConsentOutcome> {
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
    pub fn opened_at(opened_at: Duration) -> DialogGuard {
        unimplemented!("host::consent::DialogGuard::opened_at")
    }

    /// Whether input at `now` is accepted.
    pub fn accepts_input_at(&mut self, now: Duration) -> bool {
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
