//! The conversion prompt templates (copied from `ilium-prompts/templates/conversion`, see `templates/`).

pub const BASH_DESCRIPTION: &str = include_str!("../templates/bash-description.hbs");
pub const EMPTY_RESULT: &str = include_str!("../templates/empty-result.hbs");
pub const MISSING_RESULT: &str = include_str!("../templates/missing-result.hbs");
pub const SYNTHETIC_FIRST_PROMPT: &str = include_str!("../templates/synthetic-first-prompt.hbs");
const BASH_DESCRIPTION_WORKDIR: &str = include_str!("../templates/bash-description-workdir.hbs");
const TRUNCATED_RESULT: &str = include_str!("../templates/truncated-result.hbs");

/// The shell description naming the working directory.
pub fn bash_description_workdir(workdir: &str) -> String {
    BASH_DESCRIPTION_WORKDIR.replace("{{v0}}", workdir)
}

/// The marker appended to a cut tool result, naming how many characters are missing.
pub fn truncated_result(missing_characters: usize) -> String {
    TRUNCATED_RESULT.replace("{{v0}}", &missing_characters.to_string())
}
