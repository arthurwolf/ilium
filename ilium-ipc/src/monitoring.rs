//! Shared, deterministic descriptions of session monitoring changes.

use ilium_core::NodeId;
use serde::{Deserialize, Serialize};

/// The observable change that caused a monitoring event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitoringEventKind {
    Started,
    Idle,
    Done,
    NeedsInput,
    Error,
    Stuck,
    Closed,
    Progress,
    ResourceChanged,
}

impl MonitoringEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Idle => "idle",
            Self::Done => "done",
            Self::NeedsInput => "needs_input",
            Self::Error => "error",
            Self::Stuck => "stuck",
            Self::Closed => "closed",
            Self::Progress => "progress",
            Self::ResourceChanged => "resource_changed",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [
            Self::Started,
            Self::Idle,
            Self::Done,
            Self::NeedsInput,
            Self::Error,
            Self::Stuck,
            Self::Closed,
            Self::Progress,
            Self::ResourceChanged,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == name)
    }
}

/// One event in a single session's bounded, server-lifetime event stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitoringEvent {
    pub cursor: u64,
    /// Stable work-phase identity; repeated observations keep this value.
    pub generation: u64,
    pub pane_id: Option<NodeId>,
    /// Pane context captured when the event was published, when available.
    pub project: Option<String>,
    pub title: Option<String>,
    pub agent: Option<String>,
    pub kind: MonitoringEventKind,
    pub observed_at_unix_millis: u64,
    pub summary: String,
}

/// Server-side selection and output limits for an event read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitoringQuery {
    pub view: MonitoringView,
    pub since: Option<String>,
    pub consumer: Option<String>,
    pub follow: bool,
    pub min_severity: Option<String>,
    pub kinds: Vec<MonitoringEventKind>,
    pub project: Option<String>,
    pub agent: Option<String>,
    pub pane_id: Option<NodeId>,
    pub max_items: usize,
    pub max_chars: usize,
    pub format: MonitoringFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MonitoringFormat {
    Compact,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MonitoringView {
    Events,
    Digest,
    Brief,
}
