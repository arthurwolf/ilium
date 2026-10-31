//! The slice of the session statistics the timeline works on.

/// What the agent was doing between one transcript event and the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WorkKind {
    Idle,
    Model,
    Shell,
    Edit,
    Read,
    Agent,
    Web,
    Other,
}

impl WorkKind {
    pub const ALL: [WorkKind; 8] = [
        Self::Idle,
        Self::Model,
        Self::Shell,
        Self::Edit,
        Self::Read,
        Self::Agent,
        Self::Web,
        Self::Other,
    ];

    /// The snake_case name used on the wire.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Model => "model",
            Self::Shell => "shell",
            Self::Edit => "edit",
            Self::Read => "read",
            Self::Agent => "agent",
            Self::Web => "web",
            Self::Other => "other",
        }
    }

    /// Parses a wire name.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }
}

/// How an Ilium progress monitor run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressOutcome {
    Running,
    Success,
    Failure,
    Unknown,
    Cleared,
}

impl ProgressOutcome {
    /// The snake_case name used on the wire.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Unknown => "unknown",
            Self::Cleared => "cleared",
        }
    }

    /// Parses a wire name.
    pub fn parse(name: &str) -> Option<Self> {
        [Self::Running, Self::Success, Self::Failure, Self::Unknown, Self::Cleared]
            .into_iter()
            .find(|outcome| outcome.name() == name)
    }
}

/// One progress-bar run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressSpan {
    pub start_ms: i64,
    pub end_ms: Option<i64>,
    pub outcome: ProgressOutcome,
}

/// One change of work kind at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkMark {
    pub at_ms: i64,
    pub kind: WorkKind,
}

/// Timeline facts of one session.
#[derive(Debug, Clone, Default)]
pub struct SessionStats {
    pub first_at_ms: Option<i64>,
    pub last_at_ms: Option<i64>,
    pub work_marks: Vec<WorkMark>,
    pub progress_spans: Vec<ProgressSpan>,
}
