//! guide-plugin-documentation.md, part 2 (DEV-797): the newest changelog version comes first.

use crate::common::*;
use crate::lint_support::*;

/// Version numbers of the `## [x.y.z]` headings, top to bottom (Unreleased skipped).
fn versions(text: &str) -> Vec<(u32, u32, u32)> {
    text.lines()
        .filter_map(|line| line.strip_prefix("## ["))
        .filter_map(|rest| rest.split(']').next())
        .filter_map(|v| {
            let parts: Vec<u32> = v.split('.').filter_map(|p| p.parse().ok()).collect();
            (parts.len() == 3).then(|| (parts[0], parts[1], parts[2]))
        })
        .collect()
}

fn manual_example() -> String {
    let guide = std::fs::read_to_string(
        ilium_plugin_testkit::manual::pages_dir().join("guide-plugin-documentation.md"),
    )
    .expect("read page");
    let start = guide.find("## A complete changelog").expect("section");
    let rest = &guide[start..];
    let begin = rest.find("```markdown").expect("fence") + "```markdown".len();
    let end = rest[begin..].find("```").expect("fence end");
    rest[begin..begin + end].to_owned()
}

/// REQ: DEV-797
#[test]
fn doc_797_the_manual_example_lists_the_newest_version_first() {
    let found = versions(&manual_example());
    assert_eq!(found, vec![(0, 2, 0), (0, 1, 0)]);
}

/// REQ: DEV-797
#[test]
fn dev_797_fresh_project_changelog_has_unreleased_or_the_newest_version_on_top() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("out");
    let made = run(
        &env,
        &[
            "new",
            dir.to_str().unwrap(),
            "--id",
            "example.agent-tools",
            "--lang",
            "rust",
            "--template",
            "server",
            "--json",
        ],
    );
    assert_eq!(made.exit_code, 0);
    let text = read_text(&dir, "CHANGELOG.md");
    let first = text
        .lines()
        .find(|l| l.starts_with("## "))
        .expect("a release heading");
    let manifest: serde_json::Value =
        serde_json::from_str(&read_text(&dir, "ilium-plugin.json")).unwrap();
    let version = manifest["version"].as_str().unwrap();
    assert!(
        first.contains("Unreleased") || first.contains(version),
        "{first}"
    );
    let found = versions(&text);
    assert!(found.windows(2).all(|w| w[0] > w[1]), "{found:?}");
}

/// REQ: DEV-797
#[test]
fn dev_797_lint_of_a_newest_first_changelog_reports_no_changelog_finding() {
    let (_root, env) = lint_env_with(|manifest| manifest["version"] = serde_json::json!("0.2.0"));
    write_text(&project_dir(&env), "CHANGELOG.md", "# Changelog\n\n## [0.2.0] - 2030-02-01\n\n### Added\n\n- B.\n\n## [0.1.0] - 2030-01-01\n\n### Added\n\n- A.\n");
    let output = lint(&env, &[]);
    assert!(!has_rule(&output, "lint", "lint.changelog-missing"));
    assert!(
        rules_of(&output, "lint")
            .iter()
            .all(|rule| !rule.starts_with("lint.changelog")),
        "{:?}",
        rules_of(&output, "lint")
    );
}

/// REQ: DEV-797
#[test]
fn dev_797_lint_of_an_oldest_first_changelog_is_not_accepted_silently() {
    let (_root, env) = lint_env_with(|manifest| manifest["version"] = serde_json::json!("0.2.0"));
    write_text(&project_dir(&env), "CHANGELOG.md", "# Changelog\n\n## [0.1.0] - 2030-01-01\n\n### Added\n\n- A.\n\n## [0.2.0] - 2030-02-01\n\n### Added\n\n- B.\n");
    let output = lint(&env, &[]);
    assert!(
        rules_of(&output, "lint")
            .iter()
            .any(|rule| rule.starts_with("lint.changelog")),
        "{:?}",
        rules_of(&output, "lint")
    );
}
