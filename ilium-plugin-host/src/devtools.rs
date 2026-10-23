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
pub fn template_files(
    _template: &str,
    _plugin_id: &str,
) -> Result<Vec<(String, String)>, Diagnostic> {
    unimplemented!("host::devtools::template_files")
}

/// The plan of `ilium plugin build` for a project directory.
pub fn build_plan(_project_dir: &Path) -> Result<BuildPlan, Diagnostic> {
    unimplemented!("host::devtools::build_plan")
}

/// Lints a project directory.
pub fn lint(_project_dir: &Path) -> Vec<LintFinding> {
    unimplemented!("host::devtools::lint")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use ilium_plugin_api::error::Severity;
    use serde_json::{json, Value};

    use super::{build_plan, lint, template_files, BuildPlan, LintFinding};

    const PLUGIN_ID: &str = "example.agent-tools";

    /// The seven templates that `template_files` accepts with a plain `example.` id.
    const TEMPLATES_WITHOUT_FIRST_PARTY: [&str; 7] = [
        "server",
        "client",
        "both",
        "scene",
        "pane-kind",
        "service-provider",
        "extension",
    ];

    const GOOD_README: &str = "# Agent tools\n\nMarks agent panes that are working with a badge in the tree.\n\n## Grants\n\n- `tree.read`: reads agent activity.\n- `tree.decorate`: sets the badge.\n";
    const GOOD_CHANGELOG: &str =
        "# Changelog\n\n## [Unreleased]\n\n### Added\n\n- First version.\n";
    const GOOD_HELP: &str = "# Agent tools\n\nMarks agent panes that are working.\n";

    fn files_of(template: &str, plugin_id: &str) -> Vec<(String, String)> {
        match template_files(template, plugin_id) {
            Ok(files) => files,
            Err(diagnostic) => panic!(
                "template {template} refused plugin id {plugin_id}: {}",
                diagnostic.code.as_str()
            ),
        }
    }

    fn refusal_code(template: &str, plugin_id: &str) -> String {
        match template_files(template, plugin_id) {
            Ok(_) => panic!("template {template} accepted plugin id {plugin_id}"),
            Err(diagnostic) => diagnostic.code.as_str().to_owned(),
        }
    }

    fn file_text<'a>(files: &'a [(String, String)], path: &str) -> &'a str {
        files
            .iter()
            .find(|(name, _)| name.as_str() == path)
            .map(|(_, text)| text.as_str())
            .unwrap_or_else(|| panic!("template wrote no {path}"))
    }

    fn manifest_of(files: &[(String, String)]) -> Value {
        serde_json::from_str(file_text(files, "manifest.json"))
            .expect("manifest.json of a template is valid JSON")
    }

    fn grants_of_side(manifest: &Value, side: &str) -> Vec<String> {
        manifest
            .get("capabilities")
            .and_then(|capabilities| capabilities.get(side))
            .and_then(Value::as_array)
            .map(|grants| {
                grants
                    .iter()
                    .filter_map(|grant| grant.as_str().map(str::to_owned))
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default()
    }

    fn build_refusal_code(project: &Path) -> String {
        match build_plan(project) {
            Ok(_) => panic!("build_plan accepted {}", project.display()),
            Err(diagnostic) => diagnostic.code.as_str().to_owned(),
        }
    }

    fn plan_of(project: &Path) -> BuildPlan {
        match build_plan(project) {
            Ok(plan) => plan,
            Err(diagnostic) => panic!(
                "build_plan refused {}: {}",
                project.display(),
                diagnostic.code.as_str()
            ),
        }
    }

    fn base_manifest() -> Value {
        json!({
            "api": "ilium:plugin@0.1.0",
            "id": PLUGIN_ID,
            "name": "Agent tools",
            "version": "0.1.0",
            "publisher": "example",
            "description": "Marks agents that start working.",
            "license": "MIT",
            "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": [] },
            "capabilities": { "server": ["tree.read", "tree.decorate"] },
            "contributes": {
                "decoration_slots": [{ "id": "work", "placement": "badge", "priority": 50 }],
                "help": [{ "id": "overview", "file": "help/overview.md", "title": "Agent tools" }]
            }
        })
    }

    fn write_file(dir: &Path, relative: &str, text: &str) {
        let path = dir.join(relative);
        std::fs::create_dir_all(path.parent().unwrap_or(dir)).expect("create fixture folders");
        std::fs::write(path, text).expect("write fixture file");
    }

    fn write_manifest(dir: &Path, manifest: &Value) {
        let text = serde_json::to_string_pretty(manifest).expect("serialize manifest");
        write_file(dir, "manifest.json", &text);
    }

    fn write_all_docs(dir: &Path) {
        write_file(dir, "README.md", GOOD_README);
        write_file(dir, "CHANGELOG.md", GOOD_CHANGELOG);
        write_file(dir, "LICENSE", "MIT License\n");
        write_file(dir, "help/overview.md", GOOD_HELP);
    }

    fn project_with(manifest: &Value) -> tempfile::TempDir {
        let project = tempfile::tempdir().expect("create scratch project");
        write_manifest(project.path(), manifest);
        project
    }

    fn project_with_description(description: &str) -> tempfile::TempDir {
        let mut manifest = base_manifest();
        manifest["description"] = json!(description);
        let project = project_with(&manifest);
        write_all_docs(project.path());
        project
    }

    fn findings_named<'a>(findings: &'a [LintFinding], rule: &str) -> Vec<&'a LintFinding> {
        findings
            .iter()
            .filter(|finding| finding.rule == rule)
            .collect()
    }

    fn warnings_and_errors(findings: &[LintFinding]) -> Vec<String> {
        findings
            .iter()
            .filter(|finding| {
                matches!(
                    finding.diagnostic.severity,
                    Severity::Warning | Severity::Error
                )
            })
            .map(|finding| format!("{}: {}", finding.rule, finding.diagnostic.message))
            .collect()
    }

    /// REQ: DEV-043
    #[test]
    fn template_files_refuses_unknown_template_names() {
        assert_eq!(refusal_code("", PLUGIN_ID), "new.template-unknown");
        assert_eq!(
            refusal_code("unknown-template", PLUGIN_ID),
            "new.template-unknown"
        );
    }

    /// REQ: DEV-043
    #[test]
    fn template_files_writes_files_for_each_template_without_first_party() {
        for template in TEMPLATES_WITHOUT_FIRST_PARTY {
            let files = files_of(template, PLUGIN_ID);
            assert!(!files.is_empty(), "template {template} wrote no files");
            assert!(
                files.iter().any(|(path, _)| path == "manifest.json"),
                "template {template} wrote no manifest.json"
            );
        }
    }

    /// REQ: DEV-042
    #[test]
    fn template_files_accepts_minimal_and_hyphenated_ids() {
        assert!(template_files("server", "a.b").is_ok());
        assert!(template_files("server", "a-1.b-2").is_ok());
        assert!(template_files("server", PLUGIN_ID).is_ok());
    }

    /// REQ: DEV-042
    #[test]
    fn template_files_refuses_malformed_plugin_ids() {
        for plugin_id in [
            "",
            "example",
            "Example.tools",
            "example..tools",
            ".tools",
            "example.",
            "example.tools!",
            "example tools",
        ] {
            assert_eq!(
                refusal_code("server", plugin_id),
                "new.id-invalid",
                "plugin id {plugin_id:?}"
            );
        }
    }

    /// REQ: DEV-262
    #[test]
    fn every_template_without_first_party_refuses_ilium_ids() {
        for template in TEMPLATES_WITHOUT_FIRST_PARTY {
            assert_eq!(
                refusal_code(template, "ilium.demo"),
                "new.id-reserved",
                "template {template}"
            );
        }
    }

    /// REQ: DEV-262
    #[test]
    fn core_style_template_refuses_ids_outside_the_ilium_namespace() {
        assert!(template_files("core-style", PLUGIN_ID).is_err());
    }

    /// REQ: DEV-263
    #[test]
    fn template_files_are_identical_across_runs() {
        for template in TEMPLATES_WITHOUT_FIRST_PARTY {
            assert_eq!(
                files_of(template, PLUGIN_ID),
                files_of(template, PLUGIN_ID),
                "template {template}"
            );
        }
    }

    /// REQ: DEV-264
    #[test]
    fn template_files_leave_no_placeholder_token() {
        for template in TEMPLATES_WITHOUT_FIRST_PARTY {
            for (path, text) in &files_of(template, PLUGIN_ID) {
                assert!(
                    !text.contains("@@"),
                    "template {template} left a placeholder in {path}"
                );
            }
        }
    }

    /// REQ: DEV-264
    #[test]
    fn template_manifest_carries_the_plugin_id() {
        for template in TEMPLATES_WITHOUT_FIRST_PARTY {
            let manifest = manifest_of(&files_of(template, PLUGIN_ID));
            assert_eq!(
                manifest.get("id").and_then(Value::as_str),
                Some(PLUGIN_ID),
                "template {template}"
            );
        }
    }

    /// REQ: DEV-268
    #[test]
    fn every_template_writes_the_required_gitignore_lines() {
        for template in TEMPLATES_WITHOUT_FIRST_PARTY {
            let files = files_of(template, PLUGIN_ID);
            let ignore = file_text(&files, ".gitignore");
            for required in ["dist/", ".ilium/", "target/", "node_modules/"] {
                assert!(
                    ignore.lines().any(|line| line.trim() == required),
                    "template {template} .gitignore lacks {required}"
                );
            }
        }
    }

    /// REQ: DEV-269
    #[test]
    fn every_template_writes_readme_changelog_and_help_topic() {
        for template in TEMPLATES_WITHOUT_FIRST_PARTY {
            let files = files_of(template, PLUGIN_ID);
            assert!(
                !file_text(&files, "README.md").trim().is_empty(),
                "template {template} wrote an empty README.md"
            );
            assert!(
                file_text(&files, "CHANGELOG.md").contains("## [Unreleased]"),
                "template {template} CHANGELOG.md lacks an Unreleased section"
            );
            let manifest = manifest_of(&files);
            let help_topic_listed = manifest
                .get("contributes")
                .and_then(|contributes| contributes.get("help"))
                .and_then(Value::as_array)
                .is_some_and(|topics| {
                    topics.iter().any(|topic| {
                        topic.get("file").and_then(Value::as_str) == Some("help/overview.md")
                    })
                });
            assert!(
                help_topic_listed,
                "template {template} manifest does not list help/overview.md"
            );
            file_text(&files, "help/overview.md");
        }
    }

    /// REQ: DEV-274
    #[test]
    fn server_template_requests_its_table_grants() {
        let manifest = manifest_of(&files_of("server", PLUGIN_ID));
        let mut granted = grants_of_side(&manifest, "server");
        granted.sort();
        assert_eq!(
            granted,
            vec!["tree.decorate".to_owned(), "tree.read".to_owned()]
        );
    }

    /// REQ: DEV-274
    #[test]
    fn client_template_requests_only_ui_panels() {
        let manifest = manifest_of(&files_of("client", PLUGIN_ID));
        assert_eq!(
            grants_of_side(&manifest, "client"),
            vec!["ui.panels".to_owned()]
        );
    }

    /// REQ: DEV-274
    #[test]
    fn scene_template_requests_no_grants() {
        let manifest = manifest_of(&files_of("scene", PLUGIN_ID));
        let granted: usize = manifest
            .get("capabilities")
            .and_then(Value::as_object)
            .map_or(0, |capabilities| {
                capabilities
                    .values()
                    .filter_map(Value::as_array)
                    .map(Vec::len)
                    .sum::<usize>()
            });
        assert_eq!(granted, 0);
    }

    /// REQ: DEV-002
    #[test]
    fn build_plan_refuses_a_project_with_both_language_files() {
        let project = tempfile::tempdir().expect("create scratch project");
        write_file(
            project.path(),
            "Cargo.toml",
            "[package]\nname = \"agent-tools\"\n",
        );
        write_file(project.path(), "package.json", "{}\n");
        assert_eq!(
            build_refusal_code(project.path()),
            "toolchain.language-ambiguous"
        );
    }

    /// REQ: DEV-002
    #[test]
    fn build_plan_refuses_a_project_without_language_files() {
        let project = tempfile::tempdir().expect("create scratch project");
        assert_eq!(
            build_refusal_code(project.path()),
            "toolchain.language-unknown"
        );
    }

    /// REQ: DEV-002
    #[test]
    fn build_plan_runs_cargo_for_a_rust_project() {
        let project = tempfile::tempdir().expect("create scratch project");
        write_file(
            project.path(),
            "Cargo.toml",
            "[package]\nname = \"agent-tools\"\n",
        );
        let plan = plan_of(project.path());
        assert_eq!(plan.command.first().map(String::as_str), Some("cargo"));
        assert!(
            plan.command
                .iter()
                .any(|argument| argument.as_str() == "build"),
            "cargo plan has no build subcommand: {:?}",
            plan.command
        );
        assert!(
            plan.command
                .iter()
                .any(|argument| argument.contains("wasm32-wasip2")),
            "cargo plan does not target wasm32-wasip2: {:?}",
            plan.command
        );
    }

    /// REQ: DEV-002
    #[test]
    fn build_plan_runs_bun_for_a_typescript_project() {
        let project = tempfile::tempdir().expect("create scratch project");
        write_file(project.path(), "package.json", "{}\n");
        let plan = plan_of(project.path());
        let program = plan.command.first().map(String::as_str).unwrap_or_default();
        assert!(
            program.contains("bun"),
            "TypeScript plan does not start with bun: {:?}",
            plan.command
        );
    }

    /// REQ: DEV-211
    #[test]
    fn lint_reports_no_warning_for_a_complete_project() {
        let project = project_with(&base_manifest());
        write_all_docs(project.path());
        let findings = lint(project.path());
        assert_eq!(warnings_and_errors(&findings), Vec::<String>::new());
    }

    /// REQ: DEV-075
    #[test]
    fn lint_reports_only_lint_rules_for_a_broken_manifest() {
        let mut manifest = base_manifest();
        manifest["api"] = json!("ilium:plugin@9.9.9");
        manifest["description"] = json!("");
        let project = project_with(&manifest);
        let findings = lint(project.path());
        assert!(!findings.is_empty(), "broken manifest produced no findings");
        for finding in &findings {
            assert!(
                finding.rule.starts_with("lint."),
                "lint reported a non-lint rule {}",
                finding.rule
            );
        }
    }

    /// REQ: DEV-211
    #[test]
    fn lint_announces_each_rule_skipped_for_a_missing_input_as_info() {
        let project = project_with(&base_manifest());
        write_all_docs(project.path());
        let findings = lint(project.path());
        let skipped = findings_named(&findings, "lint.skipped");
        assert!(
            !skipped.is_empty(),
            "no dist folder but no lint.skipped finding"
        );
        for finding in &skipped {
            assert_eq!(finding.diagnostic.severity, Severity::Info);
        }
        assert!(
            skipped
                .iter()
                .any(|finding| finding.diagnostic.message.contains("dist")),
            "no lint.skipped message names the missing dist folder"
        );
    }

    /// REQ: DEV-213
    #[test]
    fn lint_does_not_report_grant_unused_without_built_components() {
        let project = project_with(&base_manifest());
        write_all_docs(project.path());
        let findings = lint(project.path());
        assert!(findings_named(&findings, "lint.grant-unused").is_empty());
    }

    /// REQ: DEV-210
    #[test]
    fn lint_flags_an_empty_description_as_warning() {
        let project = project_with_description("");
        let findings = lint(project.path());
        let missing = findings_named(&findings, "lint.description-missing");
        assert!(!missing.is_empty(), "empty description not flagged");
        for finding in &missing {
            assert_eq!(finding.diagnostic.severity, Severity::Warning);
        }
    }

    /// REQ: DEV-210
    #[test]
    fn lint_accepts_a_description_of_exactly_280_characters() {
        let project = project_with_description(&"a".repeat(280));
        let findings = lint(project.path());
        assert!(findings_named(&findings, "lint.description-missing").is_empty());
    }

    /// REQ: DEV-210
    #[test]
    fn lint_flags_a_description_of_281_characters() {
        let project = project_with_description(&"a".repeat(281));
        let findings = lint(project.path());
        assert!(!findings_named(&findings, "lint.description-missing").is_empty());
    }

    /// REQ: DEV-210
    #[test]
    fn lint_reports_an_empty_license_as_info() {
        let mut manifest = base_manifest();
        manifest["license"] = json!("");
        let project = project_with(&manifest);
        write_all_docs(project.path());
        let findings = lint(project.path());
        let missing = findings_named(&findings, "lint.license-missing");
        assert!(!missing.is_empty(), "empty license not flagged");
        for finding in &missing {
            assert_eq!(finding.diagnostic.severity, Severity::Info);
        }
    }

    /// REQ: DEV-220
    #[test]
    fn lint_reports_missing_docs_as_findings_without_failing() {
        let project = project_with(&base_manifest());
        let findings = lint(project.path());
        assert!(!findings_named(&findings, "lint.readme-missing").is_empty());
        assert!(!findings_named(&findings, "lint.changelog-missing").is_empty());
    }

    /// REQ: DEV-220
    #[test]
    fn lint_flags_a_help_topic_whose_heading_differs_from_its_title() {
        let project = project_with(&base_manifest());
        write_all_docs(project.path());
        write_file(
            project.path(),
            "help/overview.md",
            "# Something else\n\nText.\n",
        );
        let findings = lint(project.path());
        let mismatch = findings_named(&findings, "lint.help-title-mismatch");
        assert!(!mismatch.is_empty(), "heading mismatch not flagged");
        for finding in &mismatch {
            assert_eq!(finding.diagnostic.severity, Severity::Warning);
        }
    }

    /// REQ: DEV-210
    #[test]
    fn lint_passes_a_project_without_contributes_or_grants() {
        let mut manifest = base_manifest();
        manifest
            .as_object_mut()
            .expect("manifest is a JSON object")
            .remove("contributes");
        manifest["capabilities"] = json!({});
        let project = project_with(&manifest);
        write_all_docs(project.path());
        let findings = lint(project.path());
        assert_eq!(warnings_and_errors(&findings), Vec::<String>::new());
    }
}
