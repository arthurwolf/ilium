//! Compile-time embedded agent prompt catalog.

pub const CHATROOM_INSTRUCTIONS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/agent/chatroom-instructions.hbs"));
pub const PROGRESS_INSTRUCTIONS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/agent/progress-instructions.hbs"));
pub const COORDINATION_GUIDANCE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/agent/coordination-guidance.hbs"));
pub const ASK_FOR_UPDATE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/agent/ask-for-update.hbs"));

pub const TEMPLATES: &[(&str, &str)] = &[
    ("agent/chatroom-instructions", CHATROOM_INSTRUCTIONS),
    ("agent/progress-instructions", PROGRESS_INSTRUCTIONS),
    ("agent/coordination-guidance", COORDINATION_GUIDANCE),
    ("agent/ask-for-update", ASK_FOR_UPDATE),
];
