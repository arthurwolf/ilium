//! Compile-time embedded agent prompt catalog.

pub const CHATROOM_INSTRUCTIONS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/chatroom-instructions.hbs"
));
pub const PROGRESS_INSTRUCTIONS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/progress-instructions.hbs"
));
pub const COORDINATION_GUIDANCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/coordination-guidance.hbs"
));
pub const ASK_FOR_UPDATE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/ask-for-update.hbs"
));

pub const MANAGED_BLOCK: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/managed-block.hbs"
));
pub const GOAL_FROM_LINE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/goal-from-line.hbs"
));
pub const CHATROOM_RECORD_ROW: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/chatroom-record-row.hbs"
));
pub const CHATROOM_EMPTY_CONTEXT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/chatroom-empty-context.hbs"
));
pub const CHATROOM_CONTEXT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/chatroom-context.hbs"
));
pub const CHATROOM_CONTEXT_ROW: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/chatroom-context-row.hbs"
));
pub const CHATROOM_TITLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/chatroom-title.hbs"
));
pub const CHATROOM_HEADER_GUIDANCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/chatroom-header-guidance.hbs"
));
pub const CHATROOM_MESSAGES_HEADING: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/chatroom-messages-heading.hbs"
));
pub const PROGRESS_DONE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/progress-done.hbs"
));
pub const PROGRESS_ERROR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/progress-error.hbs"
));
pub const PROGRESS_UNSPECIFIED_ERROR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/progress-unspecified-error.hbs"
));
pub const PROGRESS_UNKNOWN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/progress-unknown.hbs"
));
pub const PROGRESS_MONITOR_FAILURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/progress-monitor-failure.hbs"
));
pub const PROGRESS_EMPTY_STATUS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/progress-empty-status.hbs"
));

pub const PROGRESS_OBSERVATION_STOPPED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/progress-observation-stopped.hbs"
));

pub const TEMPLATES: &[(&str, &str)] = &[
    (
        "agent/progress-observation-stopped",
        PROGRESS_OBSERVATION_STOPPED,
    ),
    ("agent/chatroom-instructions", CHATROOM_INSTRUCTIONS),
    ("agent/progress-instructions", PROGRESS_INSTRUCTIONS),
    ("agent/coordination-guidance", COORDINATION_GUIDANCE),
    ("agent/ask-for-update", ASK_FOR_UPDATE),
    ("agent/managed-block", MANAGED_BLOCK),
    ("agent/goal-from-line", GOAL_FROM_LINE),
    ("agent/chatroom-record-row", CHATROOM_RECORD_ROW),
    ("agent/chatroom-empty-context", CHATROOM_EMPTY_CONTEXT),
    ("agent/chatroom-context", CHATROOM_CONTEXT),
    ("agent/chatroom-context-row", CHATROOM_CONTEXT_ROW),
    ("agent/chatroom-title", CHATROOM_TITLE),
    ("agent/chatroom-header-guidance", CHATROOM_HEADER_GUIDANCE),
    ("agent/chatroom-messages-heading", CHATROOM_MESSAGES_HEADING),
    ("agent/progress-done", PROGRESS_DONE),
    ("agent/progress-error", PROGRESS_ERROR),
    (
        "agent/progress-unspecified-error",
        PROGRESS_UNSPECIFIED_ERROR,
    ),
    ("agent/progress-unknown", PROGRESS_UNKNOWN),
    ("agent/progress-monitor-failure", PROGRESS_MONITOR_FAILURE),
    ("agent/progress-empty-status", PROGRESS_EMPTY_STATUS),
];
