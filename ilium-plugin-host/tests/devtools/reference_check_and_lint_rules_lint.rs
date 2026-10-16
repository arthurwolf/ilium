//! reference-check-and-lint-rules.md "What lint does" (DEV-210..220). Channels: CLI JSONL
//! `lint` records and exit codes. Guest fixtures come from `plugins/fixtures/` (API_GAPS.md).

use serde_json::{json, Value};

use crate::common::*;

const MIB: u64 = 1024 * 1024;

fn grants_with(extra: &str) -> impl FnOnce(&mut Value) + '_ {
    move |manifest| {
        let list = manifest["capabilities"]["server"].as_array_mut().expect("server grants");
        list.push(json!(extra));
    }
}

fn skipped_messages(output: &ilium_plugin_host::cli::CliOutput) -> Vec<String> {
    records_of(output, "lint")
        .into_iter()
        .filter(|record| record["rule"] == "lint.skipped")
        .map(|record| record["message"].as_str().unwrap_or("").to_owned())
        .collect()
}

/// REQ: DEV-210
#[test]
fn dev_210_lint_reports_only_lint_rules_even_for_a_broken_manifest() {
    let (_root, env) = lint_env_with(|manifest| manifest["version"] = json!("1.x"));
    write_dist(&project_dir(&env), "server", b"not a component");
    let output = lint(&env, &[]);
    let rules = rules_of(&output, "lint");
    assert!(!rules.is_empty());
    assert!(rules.iter().all(|rule| rule.starts_with("lint.")), "{rules:?}");
    assert!(records_of(&output, "check").is_empty());
}

/// REQ: DEV-210
#[test]
fn dev_210_lint_reads_the_files_listed_by_the_manifest() {
    let (_root, env) = lint_env_with(|_| {});
    write_text(&project_dir(&env), "help/overview.md", "# Another title\n\nText.\n");
    assert!(has_rule(&lint(&env, &[]), "lint", "lint.help-title-mismatch"));
}

/// REQ: DEV-210
#[test]
fn dev_210_lint_reads_dist_when_present() {
    let (_root, env) = lint_env_with(grants_with("fs.read:project"));
    write_dist(&project_dir(&env), "server", &good_server_component());
    assert!(has_rule(&lint(&env, &[]), "lint", "lint.grant-unused"));
}

/// REQ: DEV-210
#[test]
fn dev_210_lint_reads_fixtures_when_present() {
    let (_root, env) = lint_env_with(|_| {});
    write_dist(&project_dir(&env), "server", &fixture_component("dedup-key-badchar", "server"));
    write_text(&project_dir(&env), "tests/fixtures/sample.json", r#"{"schema":1,"events":[{"tag":"agent-activity-changed","val":{"node":42,"previous":"idle","current":"working","revision":1}}]}"#);
    assert!(has_rule(&lint(&env, &[]), "lint", "lint.dedup-key-charset"));
}

/// REQ: DEV-211
#[test]
fn dev_211_rule_with_an_absent_input_is_skipped_without_a_finding_of_its_own() {
    let (_root, env) = lint_env_with(grants_with("fs.read:project"));
    let output = lint(&env, &[]);
    assert!(!has_rule(&output, "lint", "lint.grant-unused"));
    assert!(skipped_messages(&output).iter().any(|message| message.contains("dist")));
}

/// REQ: DEV-211
#[test]
fn dev_211_skipped_finding_is_info_and_there_is_one_per_skipped_rule() {
    let (_root, env) = lint_env_with(|_| {});
    let output = lint(&env, &[]);
    let skipped: Vec<&Value> =
        records_of(&output, "lint").into_iter().filter(|record| record["rule"] == "lint.skipped").collect();
    assert!(skipped.len() >= 2, "{skipped:?}");
    assert!(skipped.iter().all(|record| record["severity"] == "info"));
    let messages = skipped_messages(&output);
    let mut distinct = messages.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), messages.len(), "one finding per rule, not repeated: {messages:?}");
}

/// REQ: DEV-211
#[test]
fn dev_211_fixture_rule_is_skipped_with_the_missing_input_named() {
    let (_root, env) = lint_env_with(|_| {});
    write_dist(&project_dir(&env), "server", &good_server_component());
    let output = lint(&env, &[]);
    assert!(skipped_messages(&output).iter().any(|message| message.contains("tests/fixtures")));
    assert!(!has_rule(&output, "lint", "lint.dedup-key-charset"));
}

/// REQ: DEV-211
#[test]
fn dev_211_a_present_input_prints_no_skipped_finding_for_its_rule() {
    let (_root, env) = lint_env_with(grants_with("fs.read:project"));
    write_dist(&project_dir(&env), "server", &good_server_component());
    let output = lint(&env, &[]);
    assert!(!skipped_messages(&output).iter().any(|message| message.contains("dist/server.wasm") && message.contains("grant-unused")));
    assert!(has_rule(&output, "lint", "lint.grant-unused"));
}

/// REQ: DEV-212
#[test]
fn dev_212_lint_renders_views_in_the_embedded_host_without_a_session() {
    let (_root, env) = lint_env_with(|manifest| {
        manifest["client"] = json!({ "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": ["views"] });
        manifest["capabilities"]["client"] = json!(["ui.panels"]);
        manifest["contributes"]["panels"] = json!([{ "id": "main", "title": "Agent tools", "icon": "layout-sidebar", "modal": false }]);
    });
    write_dist(&project_dir(&env), "client", &fixture_component("client-view-keyless", "client"));
    let output = lint(&env, &[]);
    let code = error_code(&output);
    assert!(code.as_deref().map_or(true, |code| !code.starts_with("session.")), "{code:?}");
    assert_ne!(output.exit_code, 6);
    assert!(has_rule(&output, "lint", "lint.widget-missing-key"));
}

/// REQ: DEV-212
#[test]
fn dev_212_lint_replays_fixtures_without_a_session() {
    let (_root, env) = lint_env_with(|_| {});
    write_dist(&project_dir(&env), "server", &fixture_component("dedup-key-ok", "server"));
    write_text(&project_dir(&env), "tests/fixtures/sample.json", r#"{"schema":1,"events":[]}"#);
    let output = lint(&env, &[]);
    assert_ne!(output.exit_code, 6);
    assert!(error_code(&output).map_or(true, |code| !code.starts_with("session.")));
}

fn grant_unused_output(edit: impl FnOnce(&mut Value), imports: &[(&str, &str)]) -> ilium_plugin_host::cli::CliOutput {
    let (_root, env) = lint_env_with(edit);
    write_dist(&project_dir(&env), "server", &server_component(imports, "ilium:server/base@0.1.0", [0, 1, 0]));
    lint(&env, &[])
}

/// REQ: DEV-213
#[test]
fn dev_213_grant_without_import_or_contribution_is_reported_with_its_key_path() {
    let output = grant_unused_output(grants_with("fs.read:project"), &[]);
    let record = finding(&output, "lint", "lint.grant-unused").expect("finding");
    assert_eq!(record["severity"], "warn");
    assert_eq!(record["key_path"], "/capabilities/server/2");
    assert!(record["message"].as_str().expect("message").contains("fs.read:project"));
}

/// REQ: DEV-213
#[test]
fn dev_213_grant_needed_by_a_component_import_is_used() {
    let output = grant_unused_output(grants_with("fs.read:project"), &[("ilium:host/fs@0.1.0", "read-all")]);
    assert!(!has_rule(&output, "lint", "lint.grant-unused"));
}

/// REQ: DEV-213
#[test]
fn dev_213_grant_needed_by_a_contribution_is_used() {
    let output = grant_unused_output(|_| {}, &[]);
    let unused_keys: Vec<&str> = records_of(&output, "lint")
        .into_iter()
        .filter(|record| record["rule"] == "lint.grant-unused")
        .filter_map(|record| record["key_path"].as_str())
        .collect();
    assert!(!unused_keys.contains(&"/capabilities/server/1"), "tree.decorate is needed by decoration_slots: {unused_keys:?}");
}

/// REQ: DEV-213
#[test]
fn dev_213_grant_needed_by_an_extensions_entry_is_used() {
    let output = grant_unused_output(
        |manifest| {
            manifest["capabilities"]["server"].as_array_mut().expect("grants").push(json!("extensions.contribute:ilium.core.smart-copy/after-copy"));
            manifest["server"]["exports"] = json!(["hook-handler"]);
            manifest["contributes"]["extensions"] = json!([{ "point": "ilium.core.smart-copy/after-copy@0.1.0", "id": "strip-keys", "priority": 70, "handler": "strip_keys" }]);
        },
        &[],
    );
    let unused: Vec<&str> = records_of(&output, "lint")
        .into_iter()
        .filter(|record| record["rule"] == "lint.grant-unused")
        .filter_map(|record| record["message"].as_str())
        .collect();
    assert!(!unused.iter().any(|message| message.contains("extensions.contribute")), "{unused:?}");
}

/// REQ: DEV-213
#[test]
fn dev_213_grant_named_only_through_requires_is_skipped() {
    let output = grant_unused_output(
        |manifest| {
            manifest["capabilities"]["server"].as_array_mut().expect("grants").push(json!("services.use:example.exporter/export"));
            manifest["requires"] = json!({ "services": ["example.exporter/export"] });
        },
        &[],
    );
    let unused: Vec<&str> = records_of(&output, "lint")
        .into_iter()
        .filter(|record| record["rule"] == "lint.grant-unused")
        .filter_map(|record| record["message"].as_str())
        .collect();
    assert!(!unused.iter().any(|message| message.contains("services.use:example.exporter")), "{unused:?}");
}

/// REQ: DEV-213
#[test]
fn dev_213_rule_is_skipped_when_dist_is_absent() {
    let (_root, env) = lint_env_with(grants_with("fs.read:project"));
    let output = lint(&env, &[]);
    assert!(!has_rule(&output, "lint", "lint.grant-unused"));
    assert!(has_rule(&output, "lint", "lint.skipped"));
}

fn keyed_view_output(fixture: &str) -> ilium_plugin_host::cli::CliOutput {
    let (_root, env) = lint_env_with(|manifest| {
        manifest["client"] = json!({ "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": ["views"] });
        manifest["capabilities"]["client"] = json!(["ui.panels"]);
        manifest["contributes"]["panels"] = json!([{ "id": "main", "title": "Agent tools", "icon": "layout-sidebar", "modal": false }]);
    });
    write_dist(&project_dir(&env), "client", &fixture_component(fixture, "client"));
    lint(&env, &[])
}

/// REQ: DEV-214
#[test]
fn dev_214_interactive_node_without_key_is_reported() {
    let output = keyed_view_output("client-view-keyless");
    let record = finding(&output, "lint", "lint.widget-missing-key").expect("finding");
    assert_eq!(record["severity"], "warn");
    assert_eq!(record["file"], "dist/client.wasm");
}

/// REQ: DEV-214
#[test]
fn dev_214_view_whose_interactive_nodes_all_have_keys_passes() {
    assert!(!has_rule(&keyed_view_output("client-view-keyed"), "lint", "lint.widget-missing-key"));
}

/// REQ: DEV-214
#[test]
fn dev_214_view_is_rendered_with_an_empty_fixture_at_40_by_12() {
    // The fixture view adds its keyless button only when the render size is exactly 40 by 12.
    assert!(has_rule(&keyed_view_output("client-view-size-gated"), "lint", "lint.widget-missing-key"));
}

/// REQ: DEV-214
#[test]
fn dev_214_a_trapping_view_is_a_render_trap_error() {
    let output = keyed_view_output("client-view-trap");
    assert_eq!(severity_of(&output, "lint", "lint.render-trap").as_deref(), Some("error"));
    assert_eq!(output.exit_code, 3);
}

/// REQ: DEV-215
#[test]
fn dev_215_value_at_the_third_party_ceiling_does_not_fire_and_one_above_does() {
    let ceilings: [(&str, u64); 6] = [
        ("memory_mib", 128),
        ("table_elements", 20_000),
        ("event_deadline_ms", 20),
        ("event_queue", 1_024),
        ("ui_tree_nodes", 4_096),
        ("job_wall_ms", 100_000),
    ];
    for (key, ceiling) in ceilings {
        let at = lint_env_with(|manifest| manifest["limits"] = json!({ key: ceiling }));
        assert!(!has_rule(&lint(&at.1, &[]), "lint", "lint.limit-above-ceiling"), "{key} at {ceiling}");
        let above = lint_env_with(|manifest| manifest["limits"] = json!({ key: ceiling + 1 }));
        let output = lint(&above.1, &[]);
        assert!(has_rule(&output, "lint", "lint.limit-above-ceiling"), "{key} at {}", ceiling + 1);
        assert_eq!(severity_of(&output, "lint", "lint.limit-above-ceiling").as_deref(), Some("warn"));
    }
}

/// REQ: DEV-215
#[test]
fn dev_215_value_between_the_t2_and_t1_ceiling_fires_for_a_non_bundled_package() {
    let (_root, env) = lint_env_with(|manifest| manifest["limits"] = json!({ "memory_mib": 256 }));
    assert!(has_rule(&lint(&env, &[]), "lint", "lint.limit-above-ceiling"));
}

/// REQ: DEV-215
#[test]
fn dev_215_bundled_t1_plugin_may_use_the_t1_ceiling() {
    let (_root, env) = lint_env_with(|manifest| {
        manifest["id"] = json!("ilium.core.example");
        manifest["publisher"] = json!("Ilium");
        manifest["limits_tier"] = json!("first-party");
        manifest["limits"] = json!({ "memory_mib": 256 });
    });
    assert!(!has_rule(&lint(&env, &[]), "lint", "lint.limit-above-ceiling"));
}

fn size_output(total: u64) -> ilium_plugin_host::cli::CliOutput {
    let (_root, env) = lint_env_with(|_| {});
    write_sparse(&project_dir(&env), "dist/server.wasm", total);
    lint(&env, &[])
}

/// REQ: DEV-216
#[test]
fn dev_216_package_just_below_100_mib_does_not_fire() {
    let output = size_output(100 * MIB - 1);
    assert!(!has_rule(&output, "lint", "lint.package-size-near-cap"));
}

/// REQ: DEV-216
#[test]
fn dev_216_package_just_above_100_mib_fires_as_a_warning() {
    let output = size_output(100 * MIB + 1);
    assert_eq!(severity_of(&output, "lint", "lint.package-size-near-cap").as_deref(), Some("warn"));
}

/// REQ: DEV-216
#[test]
fn dev_216_lint_and_pack_agree_at_the_cap() {
    let (_root, env) = lint_env_with(|_| {});
    write_sparse(&project_dir(&env), "dist/server.wasm", 100 * MIB + 1);
    let linted = lint(&env, &[]);
    let packed = run_in_project(&env, "pack", &["--allow-debug"]);
    let lint_fires = has_rule(&linted, "lint", "lint.package-size-near-cap");
    let pack_warns = records_of(&packed, "warning").iter().any(|record| record["code"] == "pack.size-cap");
    assert_eq!(lint_fires, pack_warns);
    assert!(lint_fires);
}

fn icon_output(icon: &str) -> ilium_plugin_host::cli::CliOutput {
    let (_root, env) = lint_env_with(|manifest| {
        manifest["contributes"]["commands"] = json!([{ "id": "open", "title": "Open notes", "icon": icon }]);
    });
    lint(&env, &[])
}

/// REQ: DEV-217
#[test]
fn dev_217_known_tabler_icon_does_not_fire() {
    assert!(!has_rule(&icon_output("layout-sidebar"), "lint", "lint.icon-name-unknown"));
}

/// REQ: DEV-217
#[test]
fn dev_217_unknown_icon_one_edit_away_names_the_closest_known_name() {
    let output = icon_output("layout-sidebr");
    let record = finding(&output, "lint", "lint.icon-name-unknown").expect("finding");
    assert!(record["message"].as_str().expect("message").contains("layout-sidebar"));
}

/// REQ: DEV-217
#[test]
fn dev_217_unknown_icon_two_edits_away_still_names_the_closest_known_name() {
    let output = icon_output("layout-sideb");
    let record = finding(&output, "lint", "lint.icon-name-unknown").expect("finding");
    assert!(record["message"].as_str().expect("message").contains("layout-sidebar"));
}

/// REQ: DEV-217
#[test]
fn dev_217_unknown_icon_three_edits_away_names_no_suggestion() {
    let output = icon_output("layout-side");
    let record = finding(&output, "lint", "lint.icon-name-unknown").expect("finding");
    assert!(!record["message"].as_str().expect("message").contains("layout-sidebar"));
}

/// REQ: DEV-217
#[test]
fn dev_217_unknown_icon_far_from_every_name_has_no_suggestion_but_still_fires() {
    let output = icon_output("zzzzzzzzzzzzqqqq");
    assert_eq!(severity_of(&output, "lint", "lint.icon-name-unknown").as_deref(), Some("warn"));
}

fn dedup_output(fixture: &str) -> ilium_plugin_host::cli::CliOutput {
    let (_root, env) = lint_env_with(|_| {});
    write_dist(&project_dir(&env), "server", &fixture_component(fixture, "server"));
    write_text(&project_dir(&env), "tests/fixtures/sample.json", r#"{"schema":1,"events":[{"tag":"agent-activity-changed","val":{"node":42,"previous":"idle","current":"working","revision":1}}]}"#);
    lint(&env, &[])
}

/// REQ: DEV-218
#[test]
fn dev_218_dedup_key_with_a_character_outside_the_set_fires() {
    assert_eq!(severity_of(&dedup_output("dedup-key-badchar"), "lint", "lint.dedup-key-charset").as_deref(), Some("warn"));
}

/// REQ: DEV-218
#[test]
fn dev_218_dedup_key_of_129_characters_fires() {
    assert!(has_rule(&dedup_output("dedup-key-129"), "lint", "lint.dedup-key-charset"));
}

/// REQ: DEV-218
#[test]
fn dev_218_dedup_key_of_128_characters_in_the_set_does_not_fire() {
    assert!(!has_rule(&dedup_output("dedup-key-128"), "lint", "lint.dedup-key-charset"));
}

/// REQ: DEV-218
#[test]
fn dev_218_without_fixtures_the_rule_is_skipped() {
    let (_root, env) = lint_env_with(|_| {});
    write_dist(&project_dir(&env), "server", &fixture_component("dedup-key-badchar", "server"));
    let output = lint(&env, &[]);
    assert!(!has_rule(&output, "lint", "lint.dedup-key-charset"));
    assert!(skipped_messages(&output).iter().any(|message| message.contains("fixtures")));
}

/// REQ: DEV-219
#[test]
fn dev_219_high_risk_grants_are_listed_by_name_in_the_message() {
    let (_root, env) = lint_env_with(|manifest| {
        let list = manifest["capabilities"]["server"].as_array_mut().expect("grants");
        list.push(json!("terminal.history"));
        list.push(json!("terminal.raw-input"));
    });
    let record = finding(&lint(&env, &[]), "lint", "lint.high-risk-grants").map(|record| record.clone()).expect("finding");
    let message = record["message"].as_str().expect("message");
    assert!(message.contains("terminal.history") && message.contains("terminal.raw-input"), "{message}");
    assert_eq!(record["severity"], "info");
}

/// REQ: DEV-219
#[test]
fn dev_219_no_high_risk_grant_means_no_finding() {
    let (_root, env) = lint_env_with(|_| {});
    assert!(!has_rule(&lint(&env, &[]), "lint", "lint.high-risk-grants"));
}

/// REQ: DEV-219
#[test]
fn dev_219_normal_grants_are_not_listed_in_the_message() {
    let (_root, env) = lint_env_with(grants_with("terminal.history"));
    let output = lint(&env, &[]);
    let message = finding(&output, "lint", "lint.high-risk-grants").expect("finding")["message"].as_str().expect("message").to_owned();
    assert!(!message.contains("tree.read"));
}

/// REQ: DEV-220
#[test]
fn dev_220_missing_readme_is_a_finding_not_a_crash() {
    let (_root, env) = lint_env_with(|_| {});
    std::fs::remove_file(project_dir(&env).join("README.md")).expect("remove readme");
    let output = lint(&env, &[]);
    assert!(has_rule(&output, "lint", "lint.readme-missing"));
    assert_eq!(output.exit_code, 0);
    assert_eq!(output.stdout_jsonl.last().expect("result")["type"], "result");
}

/// REQ: DEV-220
#[test]
fn dev_220_missing_changelog_is_a_finding_not_a_crash() {
    let (_root, env) = lint_env_with(|_| {});
    std::fs::remove_file(project_dir(&env).join("CHANGELOG.md")).expect("remove changelog");
    let output = lint(&env, &[]);
    assert!(has_rule(&output, "lint", "lint.changelog-missing"));
    assert_eq!(output.exit_code, 0);
}

/// REQ: DEV-220
#[test]
fn dev_220_missing_license_file_is_a_finding_for_that_file() {
    let (_root, env) = lint_env_with(|_| {});
    std::fs::remove_file(project_dir(&env).join("LICENSE")).expect("remove license");
    let output = lint(&env, &[]);
    assert_eq!(severity_of(&output, "lint", "lint.license-file-missing").as_deref(), Some("info"));
}

/// REQ: DEV-220
#[test]
fn dev_220_missing_help_file_named_by_the_manifest_is_not_a_crash() {
    let (_root, env) = lint_env_with(|_| {});
    std::fs::remove_file(project_dir(&env).join("help/overview.md")).expect("remove help topic");
    let output = lint(&env, &[]);
    assert!(matches!(output.exit_code, 0 | 3), "exit {}", output.exit_code);
    assert!(records_of(&output, "lint").iter().any(|record| record["file"] == "help/overview.md"));
}

/// REQ: DEV-220
#[test]
fn dev_220_docs_folder_is_read_for_the_extension_point_rule() {
    let (_root, env) = lint_env_with(|manifest| {
        manifest["contributes"]["extension_points"] = json!([{ "id": "before-open", "kind": "hook", "mode": "veto", "title": "Before open", "deadline_ms": 20, "max_handlers": 8, "version": "0.1.0" }]);
    });
    assert!(has_rule(&lint(&env, &[]), "lint", "lint.extension-point-undocumented"));
    write_text(&project_dir(&env), "docs/EXTENDING.md", "# Extending\n\n| before-open | hook |\n");
    assert!(!has_rule(&lint(&env, &[]), "lint", "lint.extension-point-undocumented"));
}
