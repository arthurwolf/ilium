//! The consent gate and dialog guards (reference-grant-review-and-consent.md): one consent dialog
//! at a time, rate limits, timeouts, and the input grace after a dialog opens.

use std::collections::{BTreeMap, VecDeque};
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
    now: Duration,
    pending: Option<PendingConsent>,
    shown: BTreeMap<String, VecDeque<Duration>>,
    declined: BTreeMap<(String, String), u8>,
}

#[derive(Debug)]
struct PendingConsent {
    plugin: String,
    target: String,
    opened_at: Duration,
}

/// A refusal with the retry delay required by SEC-105. Declined targets have no
/// automatic retry: SEC-110 requires an explicit user reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsentRejection {
    pub outcome: ConsentOutcome,
    pub retry_after: Option<Duration>,
}

const PROMPT_TIMEOUT: Duration = Duration::from_secs(120);
const PROMPT_WINDOW: Duration = Duration::from_secs(3600);
const MAX_PROMPTS: usize = 6;

impl ConsentGate {
    /// A gate with no dialog open.
    pub fn new() -> ConsentGate {
        Self::default()
    }

    /// Whether plugin-drawn consent dialogs are accepted (never: the kernel draws them).
    pub fn accepts_plugin_drawn_dialogs(&self) -> bool {
        false
    }

    /// Opens a consent dialog of `plugin` for `grant`.
    pub fn open(&mut self, plugin: &str, grant: &str) -> Result<(), ConsentOutcome> {
        self.open_detailed(plugin, grant)
            .map_err(|rejection| rejection.outcome)
    }

    /// Opens a kernel-owned prompt. `target` must be the host's canonical target
    /// identity (for example an origin), not untrusted display text. The host must
    /// call `tick` with monotonic elapsed time even when no prompt is open.
    pub fn open_detailed(&mut self, plugin: &str, target: &str) -> Result<(), ConsentRejection> {
        if self.pending.is_some() {
            return Err(ConsentRejection {
                outcome: ConsentOutcome::Busy,
                retry_after: Some(Duration::from_secs(1)),
            });
        }
        if self
            .declined
            .get(&(plugin.into(), target.into()))
            .copied()
            .unwrap_or(0)
            >= 3
        {
            return Err(ConsentRejection {
                outcome: ConsentOutcome::Declined,
                retry_after: None,
            });
        }
        self.prune_history();
        let history = self.shown.entry(plugin.into()).or_default();
        if let Some(oldest) = history.front().filter(|_| history.len() >= MAX_PROMPTS) {
            return Err(ConsentRejection {
                outcome: ConsentOutcome::RateLimited,
                retry_after: Some(PROMPT_WINDOW.saturating_sub(self.now.saturating_sub(*oldest))),
            });
        }
        history.push_back(self.now);
        self.pending = Some(PendingConsent {
            plugin: plugin.into(),
            target: target.into(),
            opened_at: self.now,
        });
        Ok(())
    }

    /// Clears the denial history of this exact target after an authenticated user
    /// reset. Does not clear the rolling prompt rate limit or approve any request.
    pub fn reset_declines(&mut self, plugin: &str, target: &str) {
        self.declined.remove(&(plugin.into(), target.into()));
    }

    /// Finishes a prompt after the kernel has authenticated an explicit user
    /// approval. This only releases the gate; permission persistence is the host's
    /// responsibility. Plugin input must never call this path.
    pub fn close_approved(&mut self) -> bool {
        self.pending.take().is_some()
    }

    fn prune_history(&mut self) {
        self.shown.retain(|_, history| {
            while history
                .front()
                .is_some_and(|shown| self.now.saturating_sub(*shown) >= PROMPT_WINDOW)
            {
                history.pop_front();
            }
            !history.is_empty()
        });
    }

    /// Closes the open dialog with its outcome.
    pub fn close(&mut self, outcome: ConsentOutcome) {
        // Admission failures do not answer or cancel the currently pending call.
        if matches!(outcome, ConsentOutcome::Busy | ConsentOutcome::RateLimited) {
            return;
        }
        if let Some(pending) = self.pending.take() {
            if outcome == ConsentOutcome::Declined {
                let count = self
                    .declined
                    .entry((pending.plugin, pending.target))
                    .or_default();
                *count = count.saturating_add(1).min(3);
            }
        }
    }

    /// Time passed; returns the outcome when the open dialog timed out.
    pub fn tick(&mut self, elapsed: Duration) -> Option<ConsentOutcome> {
        self.now = self.now.saturating_add(elapsed);
        self.prune_history();
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| self.now.saturating_sub(pending.opened_at) >= PROMPT_TIMEOUT)
        {
            self.pending = None;
            return Some(ConsentOutcome::TimedOut);
        }
        None
    }
}

/// Ignores input for a grace period after a dialog opens (no click-through).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogGuard {
    opened_at: Duration,
    last_accepted: Option<Duration>,
    last_key_press: Option<(String, Duration)>,
}

impl DialogGuard {
    /// A guard for a dialog opened at `opened_at`.
    pub fn opened_at(opened_at: Duration) -> DialogGuard {
        Self {
            opened_at,
            last_accepted: None,
            last_key_press: None,
        }
    }

    /// Whether input at `now` is accepted.
    pub fn accepts_input_at(&mut self, now: Duration) -> bool {
        if now.saturating_sub(self.opened_at) < Duration::from_millis(500)
            || now < self.opened_at
            || self
                .last_accepted
                .is_some_and(|previous| now.saturating_sub(previous) < Duration::from_millis(80))
        {
            return false;
        }
        self.last_accepted = Some(now);
        true
    }

    /// Event-aware SEC-103 path. The host supplies real key identity/release state
    /// and event arrival time; `opened_at` must be the completed draw time. Only
    /// Enter/Space confirmations receive the grace/repeat suppression. Navigation
    /// and Cancel remain usable immediately after the dialog is drawn.
    pub fn accepts_key_at(
        &mut self,
        key: &str,
        now: Duration,
        released_since_previous: bool,
    ) -> bool {
        if now < self.opened_at {
            return false;
        }
        let key = if key == " " { "Space" } else { key };
        let confirmation = matches!(key, "Enter" | "Space");
        let repeating = !released_since_previous
            && self
                .last_key_press
                .as_ref()
                .is_some_and(|(previous_key, previous_at)| {
                    previous_key == key
                        && now.saturating_sub(*previous_at) < Duration::from_millis(80)
                });
        self.last_key_press = Some((key.into(), now));
        !confirmation
            || now.saturating_sub(self.opened_at) >= Duration::from_millis(500) && !repeating
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
        true
    }

    /// The label naming the source of the dialog.
    pub fn source_label(self) -> &'static str {
        "Ilium"
    }
}
