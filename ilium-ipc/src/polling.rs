//! Secret-free messages for the advisory polling detector.
//!
//! These values are a transport contract, not authority to read a transcript
//! or make a billable request. The serving coordinator must authenticate and
//! validate every operation before acting.

use ilium_core::polling_detection::{DetectorStatus, Finding, PollingSettings};
use serde::{Deserialize, Serialize};

pub use crate::polling_subscription::{SubscriptionGate, SubscriptionObservation};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindingFeedback {
    Correct,
    Incorrect,
    Unsure,
}

/// A cursor is valid only for the accounting snapshot that issued it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryCursor {
    pub revision: u64,
    pub after_sequence: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollingSession {
    pub session_id: String,
    /// Changes when the serving process is replaced or restarted.
    pub incarnation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PollingOperation {
    Snapshot,
    History {
        cursor: Option<HistoryCursor>,
        limit: u16,
    },
    UpdateSettings {
        settings: PollingSettings,
    },
    Feedback {
        session: PollingSession,
        pane_id: ilium_core::NodeId,
        generation: u64,
        finding_id: String,
        feedback: FindingFeedback,
    },
    Dismiss {
        session: PollingSession,
        pane_id: ilium_core::NodeId,
        generation: u64,
        finding_id: String,
    },
    EstimateRecent {
        session: PollingSession,
        pane_id: ilium_core::NodeId,
        generation: u64,
    },
    AnalyzeRecent {
        session: PollingSession,
        pane_id: ilium_core::NodeId,
        generation: u64,
        estimate_id: String,
    },
}

impl PollingOperation {
    /// Validate untrusted requests before carrying out an operation.
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Snapshot => Ok(()),
            Self::History { cursor, limit } => {
                if !(1..=100).contains(limit)
                    || cursor
                        .as_ref()
                        .is_some_and(|value| value.after_sequence <= 0)
                {
                    return Err("invalid polling history page");
                }
                Ok(())
            }
            Self::UpdateSettings { settings } => settings.validate(),
            Self::Feedback {
                session,
                finding_id,
                ..
            }
            | Self::Dismiss {
                session,
                finding_id,
                ..
            } => {
                if !session.valid() || finding_id.is_empty() || finding_id.len() > 256 {
                    return Err("invalid polling finding identity");
                }
                Ok(())
            }
            Self::EstimateRecent { session, .. } => {
                if !session.valid() {
                    return Err("invalid polling session");
                }
                Ok(())
            }
            Self::AnalyzeRecent {
                session,
                estimate_id,
                ..
            } => {
                if !session.valid() || estimate_id.is_empty() || estimate_id.len() > 256 {
                    return Err("invalid polling estimate identity");
                }
                Ok(())
            }
        }
    }
}

impl PollingSession {
    fn valid(&self) -> bool {
        [&self.session_id, &self.incarnation]
            .iter()
            .all(|value| !value.is_empty() && value.len() <= 256)
    }
}

/// Unknown provider charges remain separate from priced usage and reserves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollingAccounting {
    pub attempts: u64,
    pub known_cost_nanodollars: u64,
    pub reserved_nanodollars: u64,
    pub unknown_attempts: u64,
    pub input_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollingSnapshot {
    pub revision: u64,
    pub status: DetectorStatus,
    pub settings: PollingSettings,
    pub key_configured: bool,
    pub findings: Vec<Finding>,
    pub day: Option<PollingAccounting>,
    pub month: Option<PollingAccounting>,
    pub all_time: Option<PollingAccounting>,
    pub coalesced: u64,
    pub dropped_capacity: u64,
}

/// History records contain accounting metadata only, never transcript text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PollingAttemptState {
    Prepared,
    Dispatched,
    Completed,
    Cancelled,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollingHistoryRecord {
    pub sequence: i64,
    pub created_utc: String,
    pub project_id: String,
    pub model: String,
    pub state: PollingAttemptState,
    pub cost_nanodollars: Option<u64>,
    pub reserved_nanodollars: u64,
    pub input_tokens: Option<u64>,
    pub billing_unknown: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollingHistoryPage {
    pub revision: u64,
    pub records: Vec<PollingHistoryRecord>,
    pub next: Option<HistoryCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollingCostEstimate {
    pub estimate_id: String,
    /// Maximum total request input, including policy and examples.
    pub maximum_input_tokens: u32,
    /// None means the estimate cannot be priced safely.
    pub maximum_nanodollars: Option<u64>,
    pub evidence_truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PollingRejection {
    InvalidRequest,
    Unauthorized,
    Unavailable,
    StaleGeneration,
    StaleFinding,
    StaleCursor,
    StaleEstimate,
    BudgetReached,
    KeyMissing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PollingReply {
    Snapshot(Box<PollingSnapshot>),
    History(PollingHistoryPage),
    Estimate(PollingCostEstimate),
    Accepted,
    Rejected(PollingRejection),
    EvidenceSubscribed { server_incarnation: String },
}

/// Only the authenticated lease owner may request this feed. The server reads
/// opt-in settings from its configuration authority, never from this request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollingEvidenceSubscription {
    pub expected_server_incarnation: String,
    pub leader_epoch: String,
}

pub use ilium_core::polling_detection::PollingActivityBatch;

/// Private, connection-local observations for the authenticated detector only.
/// Ordinary attached clients never subscribe to or broadcast these events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PollingFeedEvent {
    Batch(PollingActivityBatch),
    Closed(ilium_core::polling_detection::AgentBinding),
    Stopped(PollingFeedStopReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PollingFeedStopReason {
    Disabled,
    AuthorityLost,
    Unavailable,
}
