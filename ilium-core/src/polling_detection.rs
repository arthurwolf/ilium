//! User-wide polling detector configuration and validation.
use serde::{Deserialize, Serialize};

pub mod scheduler;

/// Identity accepted only after the serving process verifies its provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentBinding {
    pub session: String,
    pub incarnation: String,
    pub pane: crate::NodeId,
    pub generation: u64,
}

/// Normalized observations retain an explicit source gap and exact generation.
/// The cursor is an opaque transcript byte position, not a filesystem path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollingActivityBatch {
    pub binding: AgentBinding,
    pub source_start: u64,
    pub source_end: u64,
    pub contains_gap: bool,
    pub events: Vec<ActivityEvent>,
}

impl PollingActivityBatch {
    pub fn validate(&self) -> Result<(), &'static str> {
        if [&self.binding.session, &self.binding.incarnation]
            .iter()
            .any(|value| value.is_empty() || value.len() > 256)
            || self.binding.pane == crate::ROOT_ID
            || self.source_end < self.source_start
            || (!self.events.is_empty() && self.source_end == self.source_start)
            || self.events.len() > 512
        {
            return Err("invalid polling activity identity, cursor or batch size");
        }
        let mut total_bytes = 0usize;
        for event in &self.events {
            let fields = [
                (event.text.as_str(), 1200),
                (event.timestamp.as_deref().unwrap_or_default(), 256),
                (event.name.as_deref().unwrap_or_default(), 320),
                (event.call_id.as_deref().unwrap_or_default(), 1024),
            ];
            for (value, maximum) in fields {
                if value.len() > maximum {
                    return Err("polling activity field exceeds its bound");
                }
                total_bytes = total_bytes
                    .checked_add(value.len())
                    .ok_or("polling activity size overflow")?;
            }
        }
        if total_bytes > 1024 * 1024 {
            return Err("polling activity batch exceeds its byte bound");
        }
        Ok(())
    }
}

/// Provider-neutral, bounded transcript observations. Provenance is supplied
/// separately by the server; deserialization alone never establishes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivityKind {
    Message,
    ToolCall,
    ToolResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub kind: ActivityKind,
    pub timestamp: Option<String>,
    pub name: Option<String>,
    pub call_id: Option<String>,
    pub text: String,
    pub truncated: bool,
    pub is_error: Option<bool>,
}

impl ActivityEvent {
    pub fn counts_for_interval(&self) -> bool {
        self.kind != ActivityKind::ToolResult
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PollingScope {
    #[default]
    AllSessions,
    CurrentSession,
}

/// Observable detector state. Unknown history is distinct from a clean result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectorStatus {
    Off,
    Watching,
    WaitingForActivity,
    BudgetReached,
    NoApiKey,
    Offline,
    HistoryUnavailable,
    UnsupportedAgent,
}

/// Advisory labels returned by Decisions; they do not authorize agent input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    PossiblePolling,
    RepeatedUpdates,
    WaitChurn,
    NoFinding,
    InsufficientEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// Each result is bound to the specific session and pane generation.
    pub finding_id: String,
    pub session_id: String,
    /// Server incarnation changes on restart, fencing reused pane generations.
    pub session_incarnation: String,
    pub pane_id: crate::NodeId,
    pub generation: u64,
    pub kind: FindingKind,
    /// Basis points from the provider, when supplied and valid.
    pub confidence_basis_points: Option<u16>,
    pub observed_utc: Option<String>,
    /// Locally counted facts; none purport to be a model explanation.
    pub repeated_calls: u32,
    pub repeated_updates: u32,
    pub ownership_known: bool,
    pub evidence_incomplete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PollingSettings {
    pub enabled: bool,
    pub event_interval: u8,
    pub window_events: u8,
    pub minimum_spacing_seconds: u16,
    pub candidate_filter: bool,
    pub confidence_percent: u8,
    pub maximum_evidence_tokens: u16,
    /// Zero disables automatic spending unless unlimited is explicitly set.
    pub monthly_limit_microdollars: u64,
    pub unlimited_budget: bool,
    pub alert_cooldown_minutes: u16,
    pub scope: PollingScope,
    pub excluded_projects: Vec<String>,
    /// Canonical project and session recorded when the scope was set to the
    /// current session. They travel with the saved settings, so a different
    /// leader never substitutes its own session.
    pub current_project: Option<String>,
    pub current_session: Option<String>,
}

impl Default for PollingSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            event_interval: 5,
            window_events: 30,
            minimum_spacing_seconds: 60,
            candidate_filter: true,
            confidence_percent: 80,
            maximum_evidence_tokens: 3000,
            monthly_limit_microdollars: 5_000_000,
            unlimited_budget: false,
            alert_cooldown_minutes: 15,
            scope: PollingScope::AllSessions,
            excluded_projects: Vec::new(),
            current_project: None,
            current_session: None,
        }
    }
}

impl PollingSettings {
    /// Select the originating canonical project/session, independent of leader.
    /// A relative project path is refused and leaves the settings unchanged.
    pub fn select_current_session(&mut self, project: std::path::PathBuf, session: String) -> bool {
        let Some(project) = project.to_str().filter(|_| project.is_absolute()) else {
            return false;
        };
        if session.is_empty() || project.len() > 4096 || session.len() > 256 {
            return false;
        }
        self.current_project = Some(project.to_owned());
        self.current_session = Some(session);
        self.scope = PollingScope::CurrentSession;
        true
    }

    /// Whether a session belongs to the configured scope; project exclusions
    /// always win. Without a recorded origin, `CurrentSession` includes nothing.
    pub fn includes_session(&self, project: &std::path::Path, session: &str) -> bool {
        if self
            .excluded_projects
            .iter()
            .any(|excluded| std::path::Path::new(excluded) == project)
        {
            return false;
        }
        match self.scope {
            PollingScope::AllSessions => true,
            PollingScope::CurrentSession => {
                self.current_project.as_deref().map(std::path::Path::new) == Some(project)
                    && self.current_session.as_deref() == Some(session)
            }
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=100).contains(&self.event_interval) {
            return Err("event interval must be between 1 and 100");
        }
        if !(5..=100).contains(&self.window_events) {
            return Err("window must contain between 5 and 100 events");
        }
        if self.minimum_spacing_seconds > 3600 {
            return Err("minimum spacing must not exceed 3600 seconds");
        }
        if !(50..=99).contains(&self.confidence_percent) {
            return Err("confidence must be between 50 and 99 percent");
        }
        if !(500..=12000).contains(&self.maximum_evidence_tokens) {
            return Err("evidence limit must be between 500 and 12000 tokens");
        }
        if self.monthly_limit_microdollars > (i64::MAX as u64) / 1000 {
            return Err("monthly limit exceeds supported accounting range");
        }
        if !(1..=1440).contains(&self.alert_cooldown_minutes) {
            return Err("alert cooldown must be between 1 and 1440 minutes");
        }
        if self.scope == PollingScope::CurrentSession
            && (self.current_project.is_none() || self.current_session.is_none())
        {
            return Err("a leader cannot invent the originating session");
        }
        if self.excluded_projects.len() > 256
            || self.excluded_projects.iter().any(|path| path.len() > 4096)
        {
            return Err("project exclusions exceed capacity");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenUsage {
    /// Provider total includes cache read and write tokens.
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenRates {
    pub uncached_nanos_per_million: u64,
    pub cache_read_nanos_per_million: u64,
    pub cache_write_nanos_per_million: u64,
}

/// Estimated nanodollars, rounded upward once so a small request is not free.
/// `None` denotes invalid usage or overflow, never a zero-cost request.
pub fn estimate_cost_nanodollars(usage: TokenUsage, rates: TokenRates) -> Option<u64> {
    let uncached = usage
        .input
        .checked_sub(usage.cache_read)?
        .checked_sub(usage.cache_write)?;
    let weighted = u128::from(uncached)
        .checked_mul(u128::from(rates.uncached_nanos_per_million))?
        .checked_add(
            u128::from(usage.cache_read)
                .checked_mul(u128::from(rates.cache_read_nanos_per_million))?,
        )?
        .checked_add(
            u128::from(usage.cache_write)
                .checked_mul(u128::from(rates.cache_write_nanos_per_million))?,
        )?;
    let rounded = weighted.checked_add(999_999)? / 1_000_000;
    u64::try_from(rounded).ok()
}

/// In-memory admission only; the durable repository must still reserve budget
/// and fence each dispatch against its persisted identity before sending.
pub struct PollingScheduler {
    maximum_in_flight: usize,
    maximum_pending: usize,
    pending: std::collections::VecDeque<String>,
    in_flight: std::collections::HashSet<String>,
    coalesced: u64,
    dropped_capacity: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Queued,
    Coalesced,
    DroppedCapacity,
}

impl Default for PollingScheduler {
    fn default() -> Self {
        Self::new(2, 128)
    }
}

impl PollingScheduler {
    pub fn new(maximum_in_flight: usize, maximum_pending: usize) -> Self {
        Self {
            maximum_in_flight,
            maximum_pending,
            pending: std::collections::VecDeque::new(),
            in_flight: std::collections::HashSet::new(),
            coalesced: 0,
            dropped_capacity: 0,
        }
    }

    pub fn offer(&mut self, agent: &str) -> Admission {
        if self.pending.iter().any(|queued| queued == agent) {
            self.coalesced = self.coalesced.saturating_add(1);
            return Admission::Coalesced;
        }
        if self.pending.len() >= self.maximum_pending {
            self.dropped_capacity = self.dropped_capacity.saturating_add(1);
            return Admission::DroppedCapacity;
        }
        self.pending.push_back(agent.to_owned());
        Admission::Queued
    }

    pub fn start_next(&mut self) -> Option<String> {
        if self.in_flight.len() >= self.maximum_in_flight {
            return None;
        }
        let position = self
            .pending
            .iter()
            .position(|agent| !self.in_flight.contains(agent))?;
        let agent = self.pending.remove(position)?;
        self.in_flight.insert(agent.clone());
        Some(agent)
    }

    pub fn finish(&mut self, agent: &str) {
        self.in_flight.remove(agent);
    }

    pub fn clear_pending(&mut self) {
        self.pending.clear();
    }

    pub fn coalesced(&self) -> u64 {
        self.coalesced
    }

    pub fn dropped_capacity(&self) -> u64 {
        self.dropped_capacity
    }
}
