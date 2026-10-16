//! reference-project-templates.md (DEV-260..278). Channels: `ilium plugin new` JSONL, exit codes
//! and the written file tree.

use ilium_plugin_host::cli::{CliEnv, CliOutput};
use serde_json::Value;

use crate::common::*;

const TEMPLATES: [&str; 8] = ["server", "client", "both", "scene", "pane-kind", "core-style", "service-provider", "extension"];
const ID: &str = "example.agent-tools";

fn new_in(env: &CliEnv, dir: &str, extra: &[&str]) -> CliOutput {
    let mut args = vec!["new", dir];
    args.extend_from_slice(extra);
    args.push("--json");
    run(env, &args)
}

fn make(template: &str, lang: &str) -> (tempfile::TempDir, CliEnv, std::path::PathBuf) {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("out");
    let output = new_in(&env, dir.to_str().expect("utf8"), &["--id", ID, "--lang", lang, "--template", template]);
    assert_eq!(output.exit_code, 0, "{template}/{lang}: {:?}", output.stdout_jsonl);
    (root, env, dir)
}

fn all_text(dir: &std::path::Path) -> Vec<(String, String)> {
    tree_of(dir)
        .into_iter()
        .filter_map(|relative| std::fs::read_to_string(dir.join(&relative)).ok().map(|text| (relative, text)))
        .collect()
}

fn manifest_of(dir: &std::path::Path) -> Value {
    serde_json::from_str(&read_text(dir, "ilium-plugin.json")).expect("manifest json")
}

/// REQ: DEV-260
#[test]
fn dev_260_template_and_side_naming_the_same_template_are_accepted() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("a");
    let output = new_in(&env, dir.to_str().unwrap(), &["--id", ID, "--lang", "rust", "--template", "server", "--side", "server"]);
    assert_eq!(output.exit_code, 0);
}

/// REQ: DEV-260
#[test]
fn dev_260_template_and_side_naming_different_templates_are_refused() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("a");
    let output = new_in(&env, dir.to_str().unwrap(), &["--id", ID, "--lang", "rust", "--template", "server", "--side", "client"]);
    assert_eq!(output.exit_code, 2);
    assert!(!dir.exists() || tree_of(&dir).is_empty());
}

/// REQ: DEV-260
#[test]
fn dev_260_side_accepts_only_server_client_both_and_scene() {
    for side in ["server", "client", "both", "scene"] {
        let root = scratch();
        let env = env(root.path());
        let dir = root.path().join("a");
        let output = new_in(&env, dir.to_str().unwrap(), &["--id", ID, "--lang", "rust", "--side", side]);
        assert_eq!(output.exit_code, 0, "{side}");
    }
    for side in ["pane-kind", "core-style", "extension", "service-provider", "host"] {
        let root = scratch();
        let env = env(root.path());
        let dir = root.path().join("a");
        let output = new_in(&env, dir.to_str().unwrap(), &["--id", ID, "--lang", "rust", "--side", side]);
        assert_eq!(output.exit_code, 2, "{side}");
    }
}

/// REQ: DEV-260
#[test]
fn dev_260_all_eight_templates_are_reachable_through_template() {
    for template in TEMPLATES {
        let id = if template == "core-style" { "ilium.core.example" } else { ID };
        let root = scratch();
        let env = env(root.path());
        let dir = root.path().join("a");
        let mut extra = vec!["--id", id, "--lang", "rust", "--template", template];
        if template == "core-style" {
            extra.push("--first-party");
        }
        assert_eq!(new_in(&env, dir.to_str().unwrap(), &extra).exit_code, 0, "{template}");
    }
}

/// REQ: DEV-261
#[test]
fn dev_261_a_folder_that_holds_files_is_refused_and_left_untouched() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("full");
    write_text(&dir, "keep.txt", "mine");
    let output = new_in(&env, dir.to_str().unwrap(), &["--id", ID, "--lang", "rust", "--template", "server"]);
    assert_eq!(error_code(&output).as_deref(), Some("new.dir-not-empty"));
    assert_eq!(tree_of(&dir), vec!["keep.txt".to_owned()]);
}

/// REQ: DEV-261
#[test]
fn dev_261_an_empty_existing_folder_is_accepted() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("empty");
    std::fs::create_dir_all(&dir).unwrap();
    let output = new_in(&env, dir.to_str().unwrap(), &["--id", ID, "--lang", "rust", "--template", "server"]);
    assert_eq!(output.exit_code, 0);
}

/// REQ: DEV-261
#[test]
fn dev_261_bad_ids_fail_with_id_invalid_and_write_nothing() {
    for bad in ["Example.Tools", "nodots", "example..x", "example.a_b", "", "example.x y", ".example.x"] {
        let root = scratch();
        let env = env(root.path());
        let dir = root.path().join("a");
        let output = new_in(&env, dir.to_str().unwrap(), &["--id", bad, "--lang", "rust", "--template", "server"]);
        assert_eq!(error_code(&output).as_deref(), Some("new.id-invalid"), "{bad:?}");
        assert!(!dir.exists() || tree_of(&dir).is_empty(), "{bad:?}");
    }
}

/// REQ: DEV-261
#[test]
fn dev_261_ilium_id_without_first_party_fails_with_id_reserved() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("a");
    let output = new_in(&env, dir.to_str().unwrap(), &["--id", "ilium.thing", "--lang", "rust", "--template", "server"]);
    assert_eq!(error_code(&output).as_deref(), Some("new.id-reserved"));
    assert!(!dir.exists() || tree_of(&dir).is_empty());
}

/// REQ: DEV-261
#[test]
fn dev_261_unknown_template_fails_with_template_unknown() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("a");
    let output = new_in(&env, dir.to_str().unwrap(), &["--id", ID, "--lang", "rust", "--template", "nope"]);
    assert_eq!(error_code(&output).as_deref(), Some("new.template-unknown"));
    assert!(!dir.exists() || tree_of(&dir).is_empty());
}

/// REQ: DEV-262
#[test]
fn dev_262_core_style_requires_first_party_and_an_ilium_id() {
    let root = scratch();
    let env = env(root.path());
    let a = root.path().join("a");
    assert_eq!(error_code(&new_in(&env, a.to_str().unwrap(), &["--id", "ilium.x", "--lang", "rust", "--template", "core-style"])).as_deref(), Some("new.id-reserved"));
    let b = root.path().join("b");
    assert_eq!(error_code(&new_in(&env, b.to_str().unwrap(), &["--id", ID, "--lang", "rust", "--template", "core-style", "--first-party"])).as_deref(), Some("new.id-reserved"));
    let c = root.path().join("c");
    assert_eq!(new_in(&env, c.to_str().unwrap(), &["--id", "ilium.x", "--lang", "rust", "--template", "core-style", "--first-party"]).exit_code, 0);
}

/// REQ: DEV-262
#[test]
fn dev_262_every_other_template_refuses_an_ilium_id_even_with_first_party() {
    for template in TEMPLATES.iter().filter(|name| **name != "core-style") {
        let root = scratch();
        let env = env(root.path());
        let dir = root.path().join("a");
        let output = new_in(&env, dir.to_str().unwrap(), &["--id", "ilium.x", "--lang", "rust", "--template", template, "--first-party"]);
        assert_eq!(error_code(&output).as_deref(), Some("new.id-reserved"), "{template}");
    }
}

/// REQ: DEV-262
#[test]
fn dev_262_core_style_package_outside_the_source_tree_is_refused_at_install() {
    let (_root, env, dir) = make_core();
    write_dist(&dir, "server", &good_server_component());
    let packed = run(&env, &["pack", dir.to_str().unwrap(), "--allow-debug", "--json"]);
    let file = records_of(&packed, "artifact").first().map(|r| r["path"].as_str().unwrap_or("").to_owned()).unwrap_or_default();
    let installed = run(&env, &["install", &file, "--json"]);
    assert_eq!(error_code(&installed).as_deref(), Some("manifest.id-reserved"));
}

fn make_core() -> (tempfile::TempDir, CliEnv, std::path::PathBuf) {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("out");
    let output = new_in(&env, dir.to_str().unwrap(), &["--id", "ilium.example", "--lang", "rust", "--template", "core-style", "--first-party"]);
    assert_eq!(output.exit_code, 0);
    (root, env, dir)
}

/// REQ: DEV-263
#[test]
fn dev_263_same_flags_write_byte_identical_files_for_every_template_and_language() {
    for template in TEMPLATES {
        for lang in ["rust", "typescript"] {
            let id = if template == "core-style" { "ilium.core.example" } else { ID };
            let mut trees = Vec::new();
            for _ in 0..2 {
                let root = scratch();
                let env = env(root.path());
                let dir = root.path().join("out");
                let mut extra = vec!["--id", id, "--lang", lang, "--template", template, "--name", "Agent tools", "--publisher", "Example"];
                if template == "core-style" {
                    extra.push("--first-party");
                }
                assert_eq!(new_in(&env, dir.to_str().unwrap(), &extra).exit_code, 0);
                let files: Vec<(String, Vec<u8>)> = tree_of(&dir).into_iter().map(|rel| (rel.clone(), std::fs::read(dir.join(&rel)).unwrap())).collect();
                trees.push(files);
            }
            assert_eq!(trees[0], trees[1], "{template}/{lang}");
        }
    }
}

/// REQ: DEV-264
#[test]
fn dev_264_no_placeholder_token_remains_in_any_written_file() {
    for template in TEMPLATES {
        for lang in ["rust", "typescript"] {
            let id = if template == "core-style" { "ilium.core.example" } else { ID };
            let root = scratch();
            let env = env(root.path());
            let dir = root.path().join("out");
            let mut extra = vec!["--id", id, "--lang", lang, "--template", template];
            if template == "core-style" {
                extra.push("--first-party");
            }
            assert_eq!(new_in(&env, dir.to_str().unwrap(), &extra).exit_code, 0);
            for (path, text) in all_text(&dir) {
                assert!(!text.contains("@@"), "{template}/{lang}: {path}");
            }
        }
    }
}

/// REQ: DEV-264
#[test]
fn dev_264_id_name_publisher_and_package_are_substituted() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("out");
    new_in(&env, dir.to_str().unwrap(), &["--id", "example.agent-tools", "--lang", "rust", "--template", "server", "--name", "Agent tools", "--publisher", "Example Ltd"]);
    let manifest = manifest_of(&dir);
    assert_eq!(manifest["id"], "example.agent-tools");
    assert_eq!(manifest["name"], "Agent tools");
    assert_eq!(manifest["publisher"], "Example Ltd");
    assert!(read_text(&dir, "Cargo.toml").contains("name = \"agent-tools\""));
}

/// REQ: DEV-264
#[test]
fn dev_264_default_name_is_the_last_label_with_a_capital() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("out");
    new_in(&env, dir.to_str().unwrap(), &["--id", "example.agent-tools", "--lang", "typescript", "--template", "server"]);
    assert_eq!(manifest_of(&dir)["name"], "Agent-tools");
}

/// REQ: DEV-264
#[test]
fn dev_264_typescript_package_json_uses_the_package_label() {
    let (_root, _env, dir) = make("server", "typescript");
    let package: Value = serde_json::from_str(&read_text(&dir, "package.json")).unwrap();
    assert_eq!(package["name"], "agent-tools");
}

/// REQ: DEV-265
#[test]
fn dev_265_quote_and_backslash_in_name_and_publisher_keep_every_file_valid() {
    for lang in ["rust", "typescript"] {
        let root = scratch();
        let env = env(root.path());
        let dir = root.path().join("out");
        let output = new_in(&env, dir.to_str().unwrap(), &["--id", ID, "--lang", lang, "--template", "server", "--name", "A \"quoted\" \\ name", "--publisher", "Pub\\\"lisher"]);
        assert_eq!(output.exit_code, 0);
        let manifest = manifest_of(&dir);
        assert_eq!(manifest["name"], "A \"quoted\" \\ name");
        assert_eq!(manifest["publisher"], "Pub\\\"lisher");
        if lang == "rust" {
            let toml: toml::Value = read_text(&dir, "Cargo.toml").parse().expect("valid toml");
            assert!(toml.get("package").is_some());
        } else {
            serde_json::from_str::<Value>(&read_text(&dir, "package.json")).expect("valid package.json");
        }
    }
}

/// REQ: DEV-265
#[test]
fn dev_265_newline_in_a_value_cannot_inject_a_key() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("out");
    new_in(&env, dir.to_str().unwrap(), &["--id", ID, "--lang", "rust", "--template", "server", "--name", "x\ninjected = 1"]);
    let toml: toml::Value = read_text(&dir, "Cargo.toml").parse().expect("valid toml");
    assert!(toml.get("injected").is_none());
}

/// REQ: DEV-266
#[test]
fn dev_266_typescript_entry_sources_are_named_after_the_side() {
    for (template, entries) in [("server", vec!["src/server.ts"]), ("client", vec!["src/client.ts"]), ("scene", vec!["src/scene.ts"]), ("both", vec!["src/server.ts", "src/client.ts"])] {
        let (_root, _env, dir) = make(template, "typescript");
        let files = tree_of(&dir);
        for entry in entries {
            assert!(files.contains(&entry.to_owned()), "{template}: {entry} in {files:?}");
        }
        for old in ["src/main.ts", "src/plugin.ts", "src/index.ts"] {
            assert!(!files.contains(&old.to_owned()), "{template}: {old}");
        }
    }
}

/// REQ: DEV-266
#[test]
fn dev_266_single_side_rust_entry_is_src_lib_rs() {
    for template in ["server", "client", "scene", "pane-kind", "service-provider", "extension"] {
        let (_root, _env, dir) = make(template, "rust");
        assert!(tree_of(&dir).contains(&"src/lib.rs".to_owned()), "{template}");
    }
}

/// REQ: DEV-267
#[test]
fn dev_267_two_side_rust_project_is_a_workspace_with_server_client_and_shared() {
    let (_root, _env, dir) = make("both", "rust");
    let files = tree_of(&dir);
    for crate_dir in ["server", "client", "shared"] {
        assert!(files.contains(&format!("{crate_dir}/Cargo.toml")), "{files:?}");
    }
    let root_toml: toml::Value = read_text(&dir, "Cargo.toml").parse().unwrap();
    assert!(root_toml.get("workspace").is_some());
}

/// REQ: DEV-267
#[test]
fn dev_267_each_side_crate_declares_its_side_in_package_metadata() {
    let (_root, _env, dir) = make("both", "rust");
    for side in ["server", "client"] {
        let toml: toml::Value = read_text(&dir, &format!("{side}/Cargo.toml")).parse().unwrap();
        assert_eq!(toml["package"]["metadata"]["ilium"]["side"].as_str(), Some(side));
    }
    let shared: toml::Value = read_text(&dir, "shared/Cargo.toml").parse().unwrap();
    assert!(shared["package"].get("metadata").and_then(|m| m.get("ilium")).is_none());
}

/// REQ: DEV-267
#[test]
fn dev_267_core_style_two_side_project_is_a_workspace_too() {
    let (_root, _env, dir) = make_core();
    assert!(tree_of(&dir).contains(&"shared/Cargo.toml".to_owned()));
}

/// REQ: DEV-268
#[test]
fn dev_268_every_template_writes_the_four_gitignore_lines() {
    for template in TEMPLATES {
        let id = if template == "core-style" { "ilium.core.example" } else { ID };
        let root = scratch();
        let env = env(root.path());
        let dir = root.path().join("out");
        let mut extra = vec!["--id", id, "--lang", "rust", "--template", template];
        if template == "core-style" {
            extra.push("--first-party");
        }
        new_in(&env, dir.to_str().unwrap(), &extra);
        let text = read_text(&dir, ".gitignore");
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        for needed in ["dist/", ".ilium/", "target/", "node_modules/"] {
            assert!(lines.contains(&needed), "{template}: {needed}");
        }
    }
}

/// REQ: DEV-269
#[test]
fn dev_269_readme_changelog_and_a_listed_help_topic_exist_in_every_template() {
    for template in TEMPLATES {
        let id = if template == "core-style" { "ilium.core.example" } else { ID };
        let root = scratch();
        let env = env(root.path());
        let dir = root.path().join("out");
        let mut extra = vec!["--id", id, "--lang", "typescript", "--template", template];
        if template == "core-style" {
            extra.push("--first-party");
        }
        new_in(&env, dir.to_str().unwrap(), &extra);
        let files = tree_of(&dir);
        for needed in ["README.md", "CHANGELOG.md", "help/overview.md"] {
            assert!(files.contains(&needed.to_owned()), "{template}: {needed}");
        }
        let help = manifest_of(&dir)["contributes"]["help"].to_string();
        assert!(help.contains("help/overview.md"), "{template}: {help}");
    }
}

/// REQ: DEV-269
#[test]
fn dev_269_fresh_project_has_no_missing_docs_lint_findings() {
    let (_root, env, dir) = make("server", "rust");
    let output = run(&env, &["lint", dir.to_str().unwrap(), "--json"]);
    for rule in ["lint.readme-missing", "lint.changelog-missing", "lint.help-topic-missing"] {
        assert!(!has_rule(&output, "lint", rule), "{rule}");
    }
}

/// REQ: DEV-270
#[test]
fn dev_270_fresh_project_passes_lint_with_deny_warnings_for_every_template() {
    for template in TEMPLATES {
        let id = if template == "core-style" { "ilium.core.example" } else { ID };
        let root = scratch();
        let env = env(root.path());
        let dir = root.path().join("out");
        let mut extra = vec!["--id", id, "--lang", "rust", "--template", template];
        if template == "core-style" {
            extra.push("--first-party");
        }
        new_in(&env, dir.to_str().unwrap(), &extra);
        let output = run(&env, &["lint", dir.to_str().unwrap(), "--deny", "warnings", "--json"]);
        assert_eq!(output.exit_code, 0, "{template}: {:?}", records_of(&output, "lint"));
    }
}

/// REQ: DEV-270
#[test]
fn dev_270_fresh_project_check_reports_nothing_beyond_the_unbuilt_component() {
    let (_root, env, dir) = make("server", "rust");
    let output = run(&env, &["check", dir.to_str().unwrap(), "--json"]);
    let rules = rules_of(&output, "check");
    assert!(rules.iter().all(|rule| rule == "component.missing"), "{rules:?}");
}

/// REQ: DEV-270
#[test]
fn dev_270_fresh_project_builds_and_then_tests_with_zero_warnings() {
    let (_root, env, dir) = make("server", "rust");
    let built = run(&env, &["build", dir.to_str().unwrap(), "--json"]);
    assert_eq!(built.exit_code, 0, "{:?}", built.stdout_jsonl);
    assert!(records_of(&built, "warning").is_empty());
    let tested = run(&env, &["test", dir.to_str().unwrap(), "--json"]);
    assert_eq!(tested.exit_code, 0);
    assert!(records_of(&tested, "warning").is_empty());
    assert_eq!(run(&env, &["check", dir.to_str().unwrap(), "--json"]).exit_code, 0);
}

/// REQ: DEV-271
#[test]
fn dev_271_fresh_project_ships_native_unit_tests() {
    let (_root, _env, dir) = make("server", "rust");
    assert!(all_text(&dir).iter().any(|(path, text)| path.ends_with(".rs") && text.contains("#[test]")));
    let (_root2, _env2, dir2) = make("server", "typescript");
    assert!(tree_of(&dir2).iter().any(|path| path.ends_with(".test.ts")));
}

/// REQ: DEV-271
#[test]
fn dev_271_typescript_project_declares_a_bun_test_script() {
    let (_root, _env, dir) = make("server", "typescript");
    let package: Value = serde_json::from_str(&read_text(&dir, "package.json")).unwrap();
    assert!(package["scripts"]["test"].as_str().unwrap_or("").contains("bun test"));
}

fn grants_of(manifest: &Value, side: &str) -> Vec<String> {
    manifest["capabilities"][side].as_array().map(|list| list.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

/// REQ: DEV-272
#[test]
fn dev_272_fresh_project_has_no_unused_grant() {
    for template in ["server", "client", "both", "scene", "pane-kind", "service-provider", "extension"] {
        let (_root, env, dir) = make(template, "rust");
        let output = run(&env, &["lint", dir.to_str().unwrap(), "--json"]);
        assert!(!has_rule(&output, "lint", "lint.grant-unused"), "{template}");
    }
}

/// REQ: DEV-272
#[test]
fn dev_272_scene_template_requests_no_capability_at_all() {
    let (_root, _env, dir) = make("scene", "rust");
    let manifest = manifest_of(&dir);
    assert!(grants_of(&manifest, "scene").is_empty() && grants_of(&manifest, "server").is_empty() && grants_of(&manifest, "client").is_empty());
}

/// REQ: DEV-273
#[test]
fn dev_273_fresh_project_logs_nothing_at_warn_or_error_during_activation() {
    let (_root, env, dir) = make("server", "rust");
    run(&env, &["build", dir.to_str().unwrap(), "--json"]);
    let tested = run(&env, &["test", dir.to_str().unwrap(), "--json"]);
    let loud: Vec<&Value> = records_of(&tested, "log").into_iter().filter(|r| matches!(r["level"].as_str(), Some("warn" | "error"))).collect();
    assert!(loud.is_empty(), "{loud:?}");
}

fn expected_grants(template: &str) -> Vec<(&'static str, Vec<&'static str>)> {
    match template {
        "server" => vec![("server", vec!["tree.read", "tree.decorate"])],
        "client" => vec![("client", vec!["ui.panels"])],
        "both" => vec![("client", vec!["ui.panels"])],
        "scene" => vec![],
        "pane-kind" => vec![("client", vec!["ui.panels", "panes.provide-kind"])],
        "core-style" => vec![("server", vec!["tree.read"]), ("client", vec!["ui.panels"])],
        "service-provider" => vec![("server", vec!["services.provide:example.agent-tools/greeter"])],
        "extension" => vec![("server", vec!["extensions.contribute:ilium.core.smart-copy/after-copy"])],
        _ => unreachable!(),
    }
}

/// REQ: DEV-274
#[test]
fn dev_274_each_template_requests_exactly_the_grants_of_the_table_per_side() {
    for template in TEMPLATES {
        let id = if template == "core-style" { "ilium.core.example" } else { ID };
        let root = scratch();
        let env = env(root.path());
        let dir = root.path().join("out");
        let mut extra = vec!["--id", id, "--lang", "rust", "--template", template];
        if template == "core-style" {
            extra.push("--first-party");
        }
        new_in(&env, dir.to_str().unwrap(), &extra);
        let manifest = manifest_of(&dir);
        for (side, wanted) in expected_grants(template) {
            let mut got = grants_of(&manifest, side);
            got.sort();
            let mut wanted: Vec<String> = wanted.iter().map(|g| g.replace("example.agent-tools", id)).collect();
            wanted.sort();
            assert_eq!(got, wanted, "{template}/{side}");
        }
    }
}

/// REQ: DEV-274
#[test]
fn dev_274_a_side_the_template_does_not_have_lists_no_grants() {
    let (_root, _env, dir) = make("server", "rust");
    assert!(grants_of(&manifest_of(&dir), "client").is_empty());
}

/// REQ: DEV-275
#[test]
fn dev_275_package_ident_replaces_hyphens_with_underscores_in_rust_tests() {
    let (_root, _env, dir) = make("server", "rust");
    let rust_files: Vec<(String, String)> = all_text(&dir).into_iter().filter(|(p, _)| p.ends_with(".rs")).collect();
    assert!(rust_files.iter().any(|(_, text)| text.contains("agent_tools")));
    assert!(rust_files.iter().all(|(_, text)| !text.contains("@@package_ident@@")));
}

/// REQ: DEV-275
#[test]
fn dev_275_a_package_without_hyphens_is_unchanged() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("out");
    new_in(&env, dir.to_str().unwrap(), &["--id", "example.tools", "--lang", "rust", "--template", "server"]);
    assert!(all_text(&dir).iter().any(|(p, t)| p.ends_with(".rs") && t.contains("tools")));
}

/// REQ: DEV-276
#[test]
fn dev_276_client_template_ships_a_golden_file_with_unicode_box_characters() {
    let (_root, _env, dir) = make("client", "rust");
    let golden = tree_of(&dir).into_iter().find(|p| p.contains("golden")).expect("a golden file");
    let text = read_text(&dir, &golden);
    assert!(text.chars().any(|c| ('\u{2500}'..='\u{257f}').contains(&c)), "{golden}");
}

/// REQ: DEV-276
#[test]
fn dev_276_sample_view_test_compares_against_the_golden_with_assert_golden() {
    let (_root, _env, dir) = make("client", "rust");
    assert!(all_text(&dir).iter().any(|(p, t)| p.ends_with(".rs") && t.contains("assert_golden")));
    let (_root2, _env2, dir2) = make("client", "typescript");
    assert!(all_text(&dir2).iter().any(|(p, t)| p.ends_with(".ts") && t.contains("assertGolden")));
}

/// REQ: DEV-277
#[test]
fn dev_277_both_template_test_uses_fake_session_and_client_attached() {
    let (_root, _env, dir) = make("both", "rust");
    let tests: Vec<String> = all_text(&dir).into_iter().filter(|(p, _)| p.ends_with(".rs")).map(|(_, t)| t).collect();
    assert!(tests.iter().any(|t| t.contains("FakeSession") && t.contains("client-attached")));
}

/// REQ: DEV-277
#[test]
fn dev_277_other_templates_do_not_need_the_two_side_test() {
    let (_root, _env, dir) = make("server", "rust");
    assert!(!all_text(&dir).iter().any(|(_, t)| t.contains("client-attached")));
}

/// REQ: DEV-278
#[test]
fn dev_278_core_style_writes_extending_md_with_a_table_of_extension_points() {
    let (_root, _env, dir) = make_core();
    let text = read_text(&dir, "docs/EXTENDING.md");
    assert!(text.contains('|'));
    for point in manifest_of(&dir)["contributes"]["extension_points"].as_array().expect("points") {
        assert!(text.contains(point["id"].as_str().unwrap()));
    }
}

/// REQ: DEV-278
#[test]
fn dev_278_other_templates_do_not_write_extending_md() {
    let (_root, _env, dir) = make("server", "rust");
    assert!(!tree_of(&dir).contains(&"docs/EXTENDING.md".to_owned()));
}
