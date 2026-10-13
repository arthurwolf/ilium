//! Author tooling behind `ilium plugin new/build/check/pack/sign` (reference-dev-tools.md,
//! DEV-200..233). Builds are described as plans; the host does not run cargo here.

use std::path::Path;

use ilium_plugin_api::manifest::Diagnostic;

/// One lint finding of `ilium plugin check` (DEV-200..233).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintFinding {
    /// The rule id, for example `DEV-210`.
    pub rule: String,
    /// The diagnostic it raises.
    pub diagnostic: Diagnostic,
}

/// The cargo invocation a build would run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlan {
    /// Program and arguments.
    pub command: Vec<String>,
    /// Environment variables to set.
    pub env: Vec<(String, String)>,
}

/// The files a template creates.
pub fn template_files(template: &str, plugin_id: &str) -> Result<Vec<(String, String)>, Diagnostic> {
    unimplemented!("host::devtools::template_files")
}

/// The plan of `ilium plugin build` for a project directory.
pub fn build_plan(project_dir: &Path) -> Result<BuildPlan, Diagnostic> {
    unimplemented!("host::devtools::build_plan")
}

/// Lints a project directory.
pub fn lint(project_dir: &Path) -> Vec<LintFinding> {
    unimplemented!("host::devtools::lint")
}
