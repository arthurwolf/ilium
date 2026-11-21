//! Private feed sequence validation, separate from native peer authentication.
use crate::polling::PollingFeedEvent;
use crate::ServerEvent;

#[derive(Debug, PartialEq, Eq)]
pub enum SubscriptionObservation {
    Pending,
    Subscribed,
    Activity(PollingFeedEvent),
    Rejected,
}

/// A wire gate cannot establish provenance. The transport owner must first
/// authenticate its peer, bound decoding, and retain its native lease proof.
pub struct SubscriptionGate {
    request_id: u64,
    expected_incarnation: String,
    active: bool,
    closed: bool,
    skipped: usize,
}

impl SubscriptionGate {
    pub fn new(request_id: u64, expected_incarnation: String) -> Option<Self> {
        if expected_incarnation.is_empty() || expected_incarnation.len() > 256 {
            return None;
        }
        Some(Self {
            request_id,
            expected_incarnation,
            active: false,
            closed: false,
            skipped: 0,
        })
    }

    /// Correlation and incarnation only: the consumer still owns generation
    /// and source-range continuity. Validate on an admitted codec worker.
    pub fn observe(&mut self, event: ServerEvent) -> SubscriptionObservation {
        if self.closed {
            return SubscriptionObservation::Rejected;
        }
        if !self.active {
            return match event {
                ServerEvent::Polling { request_id, reply } if request_id == self.request_id => {
                    match reply {
                        crate::polling::PollingReply::EvidenceSubscribed { server_incarnation }
                            if server_incarnation == self.expected_incarnation =>
                        {
                            self.active = true;
                            SubscriptionObservation::Subscribed
                        }
                        _ => self.reject(),
                    }
                }
                ServerEvent::PollingEvidence { .. } => self.reject(),
                _ if self.skipped < 64 => {
                    self.skipped += 1;
                    SubscriptionObservation::Pending
                }
                _ => self.reject(),
            };
        }
        let ServerEvent::PollingEvidence {
            subscription_id,
            event,
        } = event
        else {
            return self.reject();
        };
        if subscription_id != self.request_id {
            return self.reject();
        }
        let valid = match &event {
            PollingFeedEvent::Batch(batch) => {
                batch.validate().is_ok() && batch.binding.incarnation == self.expected_incarnation
            }
            PollingFeedEvent::Closed(binding) => {
                binding.incarnation == self.expected_incarnation
                    && !binding.session.is_empty()
                    && binding.session.len() <= 256
                    && binding.pane != ilium_core::ROOT_ID
            }
            PollingFeedEvent::Stopped(_) => true,
        };
        if !valid {
            return self.reject();
        }
        if matches!(&event, PollingFeedEvent::Stopped(_)) {
            self.closed = true;
            self.active = false;
        }
        SubscriptionObservation::Activity(event)
    }

    fn reject(&mut self) -> SubscriptionObservation {
        self.closed = true;
        self.active = false;
        SubscriptionObservation::Rejected
    }
}
