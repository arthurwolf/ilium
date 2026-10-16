//! guide-plugin-documentation.md (DEV-780..783, 785..788, 790, 791, 795, 796). Channels: lint
//! JSONL for docs rules, the files `new` writes, and the manual's own examples.

use serde_json::json;

use crate::common::*;

const MIB_KIB: usize = 1024;

fn fresh(template: &str) -> (tempfile::TempDir, ilium_plugin_host::cli::CliEnv, std::path::PathBuf) {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("out");
    let made = run(&env, &["new", dir.to_str().unwrap(), "--id", "example.agent-tools", "--lang", "rust", "--template", template, "--json"]);
    assert_eq!(made.exit_code, 0);
    (root, env, dir)
}

fn lint_dir(env: &ilium_plugin_host::cli::CliEnv, dir: &std::path::Path) -> ilium_plugin_host::cli::CliOutput {
    run(env, &["lint", dir.to_str().unwrap(), "--json"])
}

/// REQ: DEV-780
#[test]
fn dev_780_new_ships_readme_changelog_and_one_topic_per_help_entry() {
    let (_root, _env, dir) = fresh("server");
    let manifest: serde_json::Value = serde_json::from_str(&read_text(&dir, "ilium-plugin.json")).unwrap();
    let files = tree_of(&dir);
    assert!(files.contains(&"README.md".to_owned()) && files.contains(&"CHANGELOG.md".to_owned()));
    for entry in manifest["contributes"]["help"].as_array().expect("help entries") {
        let topic = format!("help/{}.md", entry["id"].as_str().unwrap());
        assert!(files.contains(&topic), "{topic}");
    }
}

/// REQ: DEV-780
#[test]
fn dev_780_removing_a_listed_topic_gives_help_topic_missing() {
    let (_root, env, dir) = fresh("server");
    std::fs::remove_file(dir.join("help/overview.md")).unwrap();
    assert!(has_rule(&lint_dir(&env, &dir), "lint", "lint.help-topic-missing"));
}

/// REQ: DEV-781
#[test]
fn dev_781_a_grant_missing_from_the_readme_is_reported() {
    let (_root, env, dir) = fresh("server");
    write_text(&dir, "README.md", "# Agent tools\n\nNothing about grants.\n");
    let output = lint_dir(&env, &dir);
    let messages: Vec<String> = records_of(&output, "lint").into_iter().filter(|r| r["rule"] == "lint.grant-unexplained").map(|r| r["message"].as_str().unwrap().to_owned()).collect();
    assert!(messages.iter().any(|m| m.contains("tree.read")) && messages.iter().any(|m| m.contains("tree.decorate")), "{messages:?}");
}

/// REQ: DEV-781
#[test]
fn dev_781_a_readme_that_names_every_grant_passes() {
    let (_root, env, dir) = fresh("server");
    write_text(&dir, "README.md", "# Agent tools\n\n- `tree.read`: reads activity.\n- `tree.decorate`: sets the badge.\n");
    assert!(!has_rule(&lint_dir(&env, &dir), "lint", "lint.grant-unexplained"));
}

/// REQ: DEV-781
#[test]
fn dev_781_the_fresh_readme_already_explains_its_grants() {
    for template in ["server", "client", "pane-kind", "extension", "service-provider"] {
        let (_root, env, dir) = fresh(template);
        assert!(!has_rule(&lint_dir(&env, &dir), "lint", "lint.grant-unexplained"), "{template}");
    }
}

/// REQ: DEV-782
#[test]
fn dev_782_the_fresh_readme_gives_the_build_and_test_commands() {
    let (_root, _env, dir) = fresh("server");
    let readme = read_text(&dir, "README.md");
    assert!(readme.contains("ilium plugin build") && readme.contains("ilium plugin test"), "{readme}");
}

/// REQ: DEV-782
#[test]
fn dev_782_the_manual_example_readme_shows_check_lint_test_and_build() {
    let guide = std::fs::read_to_string(ilium_plugin_testkit::manual::pages_dir().join("guide-plugin-documentation.md")).unwrap();
    for command in ["ilium plugin check", "ilium plugin lint --deny warnings", "ilium plugin test", "ilium plugin build"] {
        assert!(guide.contains(command), "{command}");
    }
}

fn headings(text: &str) -> Vec<String> {
    text.lines().filter(|l| l.starts_with("## ") || l.starts_with("### ")).map(str::to_owned).collect()
}

/// REQ: DEV-783
#[test]
fn dev_783_fresh_changelog_uses_keep_a_changelog_headings_and_a_matching_top_section() {
    let (_root, _env, dir) = fresh("server");
    let text = read_text(&dir, "CHANGELOG.md");
    let allowed = ["### Added", "### Changed", "### Fixed", "### Removed"];
    for heading in headings(&text).iter().filter(|h| h.starts_with("### ")) {
        assert!(allowed.contains(&heading.as_str()), "{heading}");
    }
    let manifest: serde_json::Value = serde_json::from_str(&read_text(&dir, "ilium-plugin.json")).unwrap();
    let top = headings(&text).into_iter().find(|h| h.starts_with("## ")).expect("top section");
    let version = manifest["version"].as_str().unwrap();
    assert!(top.contains(version) || top.contains("Unreleased"), "{top}");
}

/// REQ: DEV-783
#[test]
fn dev_783_the_manual_example_changelog_follows_the_same_headings() {
    let guide = std::fs::read_to_string(ilium_plugin_testkit::manual::pages_dir().join("guide-plugin-documentation.md")).unwrap();
    for heading in ["### Added", "### Changed", "### Fixed"] {
        assert!(guide.contains(heading));
    }
}

/// REQ: DEV-785
#[test]
fn dev_785_title_equal_to_the_manifest_entry_passes_and_a_different_one_warns() {
    let (_root, env, dir) = fresh("server");
    let manifest: serde_json::Value = serde_json::from_str(&read_text(&dir, "ilium-plugin.json")).unwrap();
    let title = manifest["contributes"]["help"][0]["title"].as_str().unwrap().to_owned();
    write_text(&dir, "help/overview.md", &format!("# {title}\n\nText.\n"));
    assert!(!has_rule(&lint_dir(&env, &dir), "lint", "lint.help-title-mismatch"));
    write_text(&dir, "help/overview.md", &format!("# {title} extra\n\nText.\n"));
    let output = lint_dir(&env, &dir);
    assert_eq!(severity_of(&output, "lint", "lint.help-title-mismatch").as_deref(), Some("warn"));
}

/// REQ: DEV-785
#[test]
fn dev_785_a_topic_that_does_not_start_with_a_level_one_heading_warns() {
    let (_root, env, dir) = fresh("server");
    write_text(&dir, "help/overview.md", "Some text first.\n\n# Title\n");
    assert!(has_rule(&lint_dir(&env, &dir), "lint", "lint.help-title-mismatch"));
    write_text(&dir, "help/overview.md", "## Level two\n");
    assert!(has_rule(&lint_dir(&env, &dir), "lint", "lint.help-title-mismatch"));
}

fn topic_of_size(env: &ilium_plugin_host::cli::CliEnv, dir: &std::path::Path, bytes: usize) -> ilium_plugin_host::cli::CliOutput {
    let manifest: serde_json::Value = serde_json::from_str(&read_text(dir, "ilium-plugin.json")).unwrap();
    let title = manifest["contributes"]["help"][0]["title"].as_str().unwrap().to_owned();
    let head = format!("# {title}\n\n");
    let body = "a".repeat(bytes - head.len());
    write_text(dir, "help/overview.md", &format!("{head}{body}"));
    assert_eq!(std::fs::metadata(dir.join("help/overview.md")).unwrap().len() as usize, bytes);
    lint_dir(env, dir)
}

/// REQ: DEV-786
#[test]
fn dev_786_topic_of_exactly_20_kib_does_not_warn() {
    let (_root, env, dir) = fresh("server");
    assert!(!has_rule(&topic_of_size(&env, &dir, 20 * MIB_KIB), "lint", "lint.help-topic-too-large"));
}

/// REQ: DEV-786
#[test]
fn dev_786_topic_one_byte_over_20_kib_warns() {
    let (_root, env, dir) = fresh("server");
    let output = topic_of_size(&env, &dir, 20 * MIB_KIB + 1);
    assert_eq!(severity_of(&output, "lint", "lint.help-topic-too-large").as_deref(), Some("warn"));
}

/// REQ: DEV-787
#[test]
fn dev_787_link_to_an_unshipped_topic_warns() {
    let (_root, env, dir) = fresh("server");
    let manifest: serde_json::Value = serde_json::from_str(&read_text(&dir, "ilium-plugin.json")).unwrap();
    let title = manifest["contributes"]["help"][0]["title"].as_str().unwrap().to_owned();
    write_text(&dir, "help/overview.md", &format!("# {title}\n\nSee [more](help:no-such-topic).\n"));
    assert!(has_rule(&lint_dir(&env, &dir), "lint", "lint.help-link-broken"));
}

/// REQ: DEV-787
#[test]
fn dev_787_link_to_a_shipped_topic_does_not_warn() {
    let (_root, env, dir) = fresh("server");
    let manifest: serde_json::Value = serde_json::from_str(&read_text(&dir, "ilium-plugin.json")).unwrap();
    let title = manifest["contributes"]["help"][0]["title"].as_str().unwrap().to_owned();
    write_text(&dir, "help/overview.md", &format!("# {title}\n\nSee [self](help:overview).\n"));
    assert!(!has_rule(&lint_dir(&env, &dir), "lint", "lint.help-link-broken"));
}

/// REQ: DEV-788 WARN
#[test]
fn dev_788_fresh_topic_names_each_command_the_plugin_adds() {
    let (_root, _env, dir) = fresh("server");
    let manifest: serde_json::Value = serde_json::from_str(&read_text(&dir, "ilium-plugin.json")).unwrap();
    let topic = read_text(&dir, "help/overview.md");
    for command in manifest["contributes"]["commands"].as_array().cloned().unwrap_or_default() {
        assert!(topic.contains(command["title"].as_str().unwrap()), "{command}");
    }
}

/// REQ: DEV-790
#[test]
fn dev_790_a_settings_row_without_help_warns_and_one_with_help_does_not() {
    let (_root, env, dir) = fresh("server");
    let mut manifest: serde_json::Value = serde_json::from_str(&read_text(&dir, "ilium-plugin.json")).unwrap();
    manifest["contributes"]["settings"] = json!([{ "id": "main", "title": "Agent tools", "schema": { "type": "object", "properties": { "refresh_seconds": { "type": "integer", "default": 5 } } } }]);
    write_text(&dir, "ilium-plugin.json", &manifest.to_string());
    assert!(has_rule(&lint_dir(&env, &dir), "lint", "lint.settings-help-missing"));
    manifest["contributes"]["settings"][0]["schema"]["properties"]["refresh_seconds"]["help"] = json!("overview");
    write_text(&dir, "ilium-plugin.json", &manifest.to_string());
    assert!(!has_rule(&lint_dir(&env, &dir), "lint", "lint.settings-help-missing"));
}

fn sentence_case(title: &str) -> bool {
    let mut words = title.split_whitespace();
    let first_ok = words.next().and_then(|w| w.chars().next()).map_or(false, char::is_uppercase);
    let rest_ok = words.all(|w| w.chars().next().map_or(true, |c| !c.is_uppercase()) || w.chars().all(|c| c.is_uppercase()));
    first_ok && rest_ok && !title.ends_with('.')
}

/// REQ: DEV-791
#[test]
fn dev_791_every_template_command_title_is_sentence_case_without_a_final_period() {
    for template in ["server", "client", "both", "scene", "pane-kind", "service-provider", "extension"] {
        let (_root, _env, dir) = fresh(template);
        let manifest: serde_json::Value = serde_json::from_str(&read_text(&dir, "ilium-plugin.json")).unwrap();
        for command in manifest["contributes"]["commands"].as_array().cloned().unwrap_or_default() {
            let title = command["title"].as_str().unwrap();
            assert!(sentence_case(title), "{template}: {title:?}");
        }
    }
}

/// REQ: DEV-791
#[test]
fn dev_791_the_sentence_case_predicate_rejects_title_case_and_final_periods() {
    assert!(sentence_case("Open notes") && sentence_case("Clear all badges"));
    assert!(!sentence_case("Open Notes") && !sentence_case("Open notes.") && !sentence_case("open notes"));
}

/// REQ: DEV-795
#[test]
fn dev_795_extension_point_in_the_readme_or_extending_md_is_documented() {
    let (_root, env, dir) = fresh("server");
    let mut manifest: serde_json::Value = serde_json::from_str(&read_text(&dir, "ilium-plugin.json")).unwrap();
    manifest["contributes"]["extension_points"] = json!([{ "id": "before-open", "kind": "hook", "mode": "veto", "title": "Before open", "deadline_ms": 20, "max_handlers": 8, "version": "0.1.0" }]);
    write_text(&dir, "ilium-plugin.json", &manifest.to_string());
    assert!(has_rule(&lint_dir(&env, &dir), "lint", "lint.extension-point-undocumented"));
    write_text(&dir, "README.md", &format!("{}\nExtension point `before-open`.\n", read_text(&dir, "README.md")));
    assert!(!has_rule(&lint_dir(&env, &dir), "lint", "lint.extension-point-undocumented"));
}

/// REQ: DEV-795
#[test]
fn dev_795_a_fresh_core_style_project_documents_its_point_in_extending_md() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("out");
    run(&env, &["new", dir.to_str().unwrap(), "--id", "ilium.core.example", "--lang", "rust", "--template", "core-style", "--first-party", "--json"]);
    assert!(!has_rule(&lint_dir(&env, &dir), "lint", "lint.extension-point-undocumented"));
}

/// REQ: DEV-796
#[test]
fn dev_796_fresh_changelog_dates_use_the_iso_form() {
    let (_root, _env, dir) = fresh("server");
    for line in read_text(&dir, "CHANGELOG.md").lines().filter(|l| l.starts_with("## [") && l.contains(" - ")) {
        let date = line.rsplit(" - ").next().unwrap().trim();
        let parts: Vec<&str> = date.split('-').collect();
        assert!(parts.len() == 3 && parts[0].len() == 4 && parts[1].len() == 2 && parts[2].len() == 2 && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())), "{line}");
    }
}

/// REQ: DEV-796
#[test]
fn dev_796_the_manual_example_changelog_dates_use_the_iso_form() {
    let guide = std::fs::read_to_string(ilium_plugin_testkit::manual::pages_dir().join("guide-plugin-documentation.md")).unwrap();
    let dated: Vec<&str> = guide.lines().filter(|l| l.starts_with("## [") && l.contains(" - ")).collect();
    assert!(!dated.is_empty());
    for line in dated {
        let date = line.rsplit(" - ").next().unwrap();
        assert_eq!(date.len(), 10, "{line}");
        assert_eq!(date.matches('-').count(), 2, "{line}");
    }
}
