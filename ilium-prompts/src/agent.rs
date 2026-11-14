//! Compile-time embedded agent prompt catalog.

pub const CHATROOM_INSTRUCTIONS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/chatroom-instructions.hbs"
));
pub const PROGRESS_INSTRUCTIONS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/progress-instructions.hbs"
));
pub const COMPACT_CHATROOM_INSTRUCTIONS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/compact-chatroom-instructions.hbs"
));
pub const COMPACT_PROGRESS_INSTRUCTIONS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/compact-progress-instructions.hbs"
));
/// Appended to the initial prompt of every worktree agent, so the agent knows
/// how to wrap up its own worktree through `ilium close-pane`.
pub const WORKTREE_WRAP_UP_INSTRUCTIONS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/agent/worktree-wrap-up.hbs"
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
    (
        "agent/compact-chatroom-instructions",
        COMPACT_CHATROOM_INSTRUCTIONS,
    ),
    (
        "agent/compact-progress-instructions",
        COMPACT_PROGRESS_INSTRUCTIONS,
    ),
    ("agent/worktree-wrap-up", WORKTREE_WRAP_UP_INSTRUCTIONS),
    ("agent/ask-for-update", ASK_FOR_UPDATE),
    ("agent/managed-block", MANAGED_BLOCK),
    ("agent/goal-from-line", GOAL_FROM_LINE),
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
