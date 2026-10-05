//! Compile-time embedded remote-compaction prompt catalog.

pub const GUARD: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/compaction/guard.hbs"
));
pub const CLAUDE_CODE_SYSTEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/compaction/claude-code-system.hbs"
));
pub const CODEX_SYSTEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/compaction/codex-system.hbs"
));
pub const CODEX_SUMMARY_PREFIX: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/compaction/codex-summary-prefix.hbs"
));
pub const OPENCODE_SYSTEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/compaction/opencode-system.hbs"
));
pub const GEMINI_CLI_SYSTEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/compaction/gemini-cli-system.hbs"
));
pub const BEST_OF_ALL_WORLDS_SYSTEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/compaction/best-of-all-worlds-system.hbs"
));
pub const CUSTOM_SYSTEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/compaction/custom-system.hbs"
));
pub const USER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/compaction/user.hbs"
));
pub const MERGE_USER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/compaction/merge-user.hbs"
));
pub const RETRY_NOTE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/compaction/retry-note.hbs"
));
pub const CLAUDE_SUMMARY_WRAPPER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/compaction/claude-summary-wrapper.hbs"
));
pub const LEDGER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/compaction/ledger.hbs"
));
pub const FALLBACK_SUMMARY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/compaction/fallback-summary.hbs"
));
pub const TAIL_ACTIVITY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/compaction/tail-activity.hbs"
));
pub const TOOL_RESULT_OMITTED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/compaction/tool-result-omitted.hbs"
));
pub const TURN_TRUNCATED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/compaction/turn-truncated.hbs"
));
pub const IMAGE_PLACEHOLDER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/compaction/image-placeholder.hbs"
));
pub const OPAQUE_COMPACTION_NOTE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/compaction/opaque-compaction-note.hbs"
));

pub const LEDGER_APPENDIX: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/compaction/ledger-appendix.hbs"
));

pub const TEMPLATES: &[(&str, &str)] = &[
    ("compaction/guard", GUARD),
    ("compaction/claude-code-system", CLAUDE_CODE_SYSTEM),
    ("compaction/codex-system", CODEX_SYSTEM),
    ("compaction/codex-summary-prefix", CODEX_SUMMARY_PREFIX),
    ("compaction/opencode-system", OPENCODE_SYSTEM),
    ("compaction/gemini-cli-system", GEMINI_CLI_SYSTEM),
    (
        "compaction/best-of-all-worlds-system",
        BEST_OF_ALL_WORLDS_SYSTEM,
    ),
    ("compaction/custom-system", CUSTOM_SYSTEM),
    ("compaction/user", USER),
    ("compaction/merge-user", MERGE_USER),
    ("compaction/retry-note", RETRY_NOTE),
    ("compaction/claude-summary-wrapper", CLAUDE_SUMMARY_WRAPPER),
    ("compaction/ledger", LEDGER),
    ("compaction/ledger-appendix", LEDGER_APPENDIX),
    ("compaction/fallback-summary", FALLBACK_SUMMARY),
    ("compaction/tail-activity", TAIL_ACTIVITY),
    ("compaction/tool-result-omitted", TOOL_RESULT_OMITTED),
    ("compaction/turn-truncated", TURN_TRUNCATED),
    ("compaction/image-placeholder", IMAGE_PLACEHOLDER),
    ("compaction/opaque-compaction-note", OPAQUE_COMPACTION_NOTE),
];
