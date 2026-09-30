//! Compile-time embedded conversion prompt catalog.

pub const MISSING_RESULT: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/conversion/missing-result.hbs"));
pub const EMPTY_RESULT: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/conversion/empty-result.hbs"));
pub const SYNTHETIC_FIRST_PROMPT: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/conversion/synthetic-first-prompt.hbs"));

pub const TEMPLATES: &[(&str, &str)] = &[
    ("conversion/missing-result", MISSING_RESULT),
    ("conversion/empty-result", EMPTY_RESULT),
    ("conversion/synthetic-first-prompt", SYNTHETIC_FIRST_PROMPT),
];
