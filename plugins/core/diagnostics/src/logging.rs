//! Ordered, bounded file-logging transitions as pure state. The host performs the file work and reports each
//! outcome back through [`DebugLogging::complete`]; this type only decides what is asked next, never echoes a
//! server broadcast back as a client request, and remembers the last failed desire.

use std::collections::VecDeque;

/// At most this many transitions wait; further client requests are refused, server broadcasts coalesce.
pub const MAX_TRANSITIONS: usize = 8;

/// Who asked for a transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Client,
    Server,
}

/// One requested state change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    pub enabled: bool,
    pub origin: Origin,
}

/// The admission queue is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmissionBusy;

/// The result of finishing the front transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub enabled: bool,
    pub origin: Origin,
    pub result: Result<(), String>,
    pub is_pending: bool,
}

/// Ordered transition queue with a replaceable server-state slot.
#[derive(Debug, Default)]
pub struct DebugLogging {
    requests: VecDeque<Transition>,
    /// Server broadcasts are replaceable state. Overflow retains the newest authoritative state until the
    /// admitted queue has capacity; it is never echoed.
    pending_server: Option<bool>,
    failed_desired: Option<bool>,
    confirmed_enabled: bool,
}

impl DebugLogging {
    pub fn new(confirmed_enabled: bool) -> Self {
        Self {
            confirmed_enabled,
            ..Self::default()
        }
    }

    pub fn is_pending(&self) -> bool {
        !self.requests.is_empty()
    }

    pub fn confirmed_enabled(&self) -> bool {
        self.confirmed_enabled
    }

    /// The state the user last asked for: a waiting server state, else the newest queued request, else the last
    /// failed desire, else the confirmed state.
    pub fn desired_enabled(&self) -> bool {
        self.pending_server
            .or_else(|| self.requests.back().map(|request| request.enabled))
            .or(self.failed_desired)
            .unwrap_or(self.confirmed_enabled)
    }

    /// The transition the host should perform now, if any.
    pub fn front(&self) -> Option<Transition> {
        self.requests.front().copied()
    }

    pub fn request(&mut self, enabled: bool) -> Result<(), AdmissionBusy> {
        if self.requests.len() >= MAX_TRANSITIONS {
            return Err(AdmissionBusy);
        }
        self.failed_desired = None;
        self.requests.push_back(Transition {
            enabled,
            origin: Origin::Client,
        });
        Ok(())
    }

    pub fn synchronize(&mut self, enabled: bool) {
        if self.requests.len() >= MAX_TRANSITIONS {
            self.pending_server = Some(enabled);
        } else {
            self.requests.push_back(Transition {
                enabled,
                origin: Origin::Server,
            });
        }
    }

    /// Finishes the front transition with the host's outcome and the state it now observes. `None` when nothing
    /// was in flight.
    pub fn complete(
        &mut self,
        result: Result<(), String>,
        observed_enabled: bool,
    ) -> Option<Completion> {
        let request = self.requests.pop_front()?;
        self.confirmed_enabled = observed_enabled;
        self.failed_desired = result.is_err().then_some(request.enabled);
        if let Some(enabled) = self.pending_server.take() {
            self.requests.push_back(Transition {
                enabled,
                origin: Origin::Server,
            });
        }
        Some(Completion {
            enabled: request.enabled,
            origin: request.origin,
            result,
            is_pending: !self.requests.is_empty(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transitions_are_bounded_and_server_sync_never_becomes_client_origin() {
        let mut logging = DebugLogging::new(false);
        for _ in 0..MAX_TRANSITIONS {
            assert!(logging.request(true).is_ok());
        }
        assert_eq!(logging.request(false), Err(AdmissionBusy));
        logging.synchronize(false);
        assert!(!logging.desired_enabled());
        let first = logging.complete(Ok(()), true).expect("front completes");
        assert_eq!(first.origin, Origin::Client);
        assert!(first.is_pending);
        // The coalesced server state is admitted last and keeps its origin.
        let mut last = first;
        while logging.is_pending() {
            last = logging.complete(Ok(()), true).expect("queued");
        }
        assert_eq!(last.origin, Origin::Server);
        assert!(!last.enabled);
    }

    #[test]
    fn a_failed_transition_is_remembered_as_the_desired_state() {
        let mut logging = DebugLogging::new(false);
        assert!(logging.request(true).is_ok());
        let completion = logging
            .complete(Err("disk full".into()), false)
            .expect("completes");
        assert!(completion.result.is_err());
        assert!(!logging.confirmed_enabled());
        assert!(logging.desired_enabled());
        assert!(logging.request(false).is_ok());
        assert!(!logging.desired_enabled());
        let finished = logging.complete(Ok(()), false).expect("completes");
        assert!(!finished.enabled);
        assert!(!finished.is_pending);
        assert!(!logging.desired_enabled());
    }
}
