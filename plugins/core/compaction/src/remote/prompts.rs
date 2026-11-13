//! Compile-time embedded prompt catalog of the compaction and conversion templates (copied from the kernel
//! `ilium-prompts` crate). Rendering is pure: the registry is built once from the embedded sources and no file is read.

use std::sync::OnceLock;

use handlebars::Handlebars;
use serde_json::Value;

pub mod compaction {
    pub const BEST_OF_ALL_WORLDS_SYSTEM: &str =
        include_str!("../../templates/compaction/best-of-all-worlds-system.hbs");
    pub const CLAUDE_CODE_SYSTEM: &str =
        include_str!("../../templates/compaction/claude-code-system.hbs");
    pub const CLAUDE_SUMMARY_WRAPPER: &str =
        include_str!("../../templates/compaction/claude-summary-wrapper.hbs");
    pub const CODEX_SUMMARY_PREFIX: &str =
        include_str!("../../templates/compaction/codex-summary-prefix.hbs");
    pub const CODEX_SYSTEM: &str = include_str!("../../templates/compaction/codex-system.hbs");
    pub const CUSTOM_SYSTEM: &str = include_str!("../../templates/compaction/custom-system.hbs");
    pub const FALLBACK_SUMMARY: &str =
        include_str!("../../templates/compaction/fallback-summary.hbs");
    pub const GEMINI_CLI_SYSTEM: &str =
        include_str!("../../templates/compaction/gemini-cli-system.hbs");
    pub const GUARD: &str = include_str!("../../templates/compaction/guard.hbs");
    pub const IMAGE_PLACEHOLDER: &str =
        include_str!("../../templates/compaction/image-placeholder.hbs");
    pub const LEDGER_APPENDIX: &str =
        include_str!("../../templates/compaction/ledger-appendix.hbs");
    pub const LEDGER: &str = include_str!("../../templates/compaction/ledger.hbs");
    pub const MERGE_USER: &str = include_str!("../../templates/compaction/merge-user.hbs");
    pub const OPAQUE_COMPACTION_NOTE: &str =
        include_str!("../../templates/compaction/opaque-compaction-note.hbs");
    pub const OPENCODE_SYSTEM: &str =
        include_str!("../../templates/compaction/opencode-system.hbs");
    pub const RETRY_NOTE: &str = include_str!("../../templates/compaction/retry-note.hbs");
    pub const TAIL_ACTIVITY: &str = include_str!("../../templates/compaction/tail-activity.hbs");
    pub const TOOL_RESULT_OMITTED: &str =
        include_str!("../../templates/compaction/tool-result-omitted.hbs");
    pub const TURN_TRUNCATED: &str = include_str!("../../templates/compaction/turn-truncated.hbs");
    pub const USER: &str = include_str!("../../templates/compaction/user.hbs");
}

pub mod conversion {
    pub const BASH_DESCRIPTION_WORKDIR: &str =
        include_str!("../../templates/conversion/bash-description-workdir.hbs");
    pub const BASH_DESCRIPTION: &str =
        include_str!("../../templates/conversion/bash-description.hbs");
    pub const EMPTY_RESULT: &str = include_str!("../../templates/conversion/empty-result.hbs");
    pub const MISSING_RESULT: &str = include_str!("../../templates/conversion/missing-result.hbs");
    pub const SYNTHETIC_FIRST_PROMPT: &str =
        include_str!("../../templates/conversion/synthetic-first-prompt.hbs");
    pub const TRUNCATED_RESULT: &str =
        include_str!("../../templates/conversion/truncated-result.hbs");
}

/// Every embedded template, keyed by its catalog name.
const TEMPLATES: &[(&str, &str)] = &[
    (
        "compaction/best-of-all-worlds-system",
        compaction::BEST_OF_ALL_WORLDS_SYSTEM,
    ),
    (
        "compaction/claude-code-system",
        compaction::CLAUDE_CODE_SYSTEM,
    ),
    (
        "compaction/claude-summary-wrapper",
        compaction::CLAUDE_SUMMARY_WRAPPER,
    ),
    (
        "compaction/codex-summary-prefix",
        compaction::CODEX_SUMMARY_PREFIX,
    ),
    ("compaction/codex-system", compaction::CODEX_SYSTEM),
    ("compaction/custom-system", compaction::CUSTOM_SYSTEM),
    ("compaction/fallback-summary", compaction::FALLBACK_SUMMARY),
    (
        "compaction/gemini-cli-system",
        compaction::GEMINI_CLI_SYSTEM,
    ),
    ("compaction/guard", compaction::GUARD),
    (
        "compaction/image-placeholder",
        compaction::IMAGE_PLACEHOLDER,
    ),
    ("compaction/ledger-appendix", compaction::LEDGER_APPENDIX),
    ("compaction/ledger", compaction::LEDGER),
    ("compaction/merge-user", compaction::MERGE_USER),
    (
        "compaction/opaque-compaction-note",
        compaction::OPAQUE_COMPACTION_NOTE,
    ),
    ("compaction/opencode-system", compaction::OPENCODE_SYSTEM),
    ("compaction/retry-note", compaction::RETRY_NOTE),
    ("compaction/tail-activity", compaction::TAIL_ACTIVITY),
    (
        "compaction/tool-result-omitted",
        compaction::TOOL_RESULT_OMITTED,
    ),
    ("compaction/turn-truncated", compaction::TURN_TRUNCATED),
    ("compaction/user", compaction::USER),
    (
        "conversion/bash-description-workdir",
        conversion::BASH_DESCRIPTION_WORKDIR,
    ),
    ("conversion/bash-description", conversion::BASH_DESCRIPTION),
    ("conversion/empty-result", conversion::EMPTY_RESULT),
    ("conversion/missing-result", conversion::MISSING_RESULT),
    (
        "conversion/synthetic-first-prompt",
        conversion::SYNTHETIC_FIRST_PROMPT,
    ),
    ("conversion/truncated-result", conversion::TRUNCATED_RESULT),
];

fn registry() -> Result<&'static Handlebars<'static>, String> {
    static REGISTRY: OnceLock<Result<Handlebars<'static>, String>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| {
            let mut registry = Handlebars::new();
            // Prompts are plain text; user values keep their punctuation and markup.
            registry.register_escape_fn(handlebars::no_escape);
            for &(name, source) in TEMPLATES {
                registry
                    .register_template_string(name, source)
                    .map_err(|error| format!("invalid embedded prompt {name}: {error}"))?;
            }
            Ok(registry)
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Renders the embedded template `name` with `context`; an unknown name or a render failure is an error text.
pub fn render(name: &str, context: &Value) -> Result<String, String> {
    registry()?
        .render(name, context)
        .map_err(|error| format!("could not render prompt {name}: {error}"))
}

/// Renders a catalog-owned template. The names are fixed call-site identifiers covered by the catalog test, so a
/// failure cannot happen with a shipped build; it degrades to an empty prompt instead of panicking.
pub fn render_value(name: &str, context: &Value) -> String {
    render(name, context).unwrap_or_default()
}

/// The embedded catalog names, for tests.
pub fn catalog_names() -> Vec<&'static str> {
    TEMPLATES.iter().map(|&(name, _)| name).collect()
}
