//! Rules that can only be checked by reading the pages: the extension points, the service, the
//! topic, the risk marks (SEC-293), the kernel tab (COR-030, COR-033, COR-022) and the [git] keys.

use crate::common::{json_fences, page, page_text, table_after, user_doc};

fn points() -> Vec<Vec<String>> {
    table_after(&page_text(), "### Extension points")
}

/// REQ: EXT-050, F-067
#[test]
fn extension_050_the_page_lists_exactly_three_points_in_order() {
    let names: Vec<String> = points().iter().map(|row| row[0].clone()).collect();
    assert_eq!(
        names,
        ["`before-create`", "`post-create-steps`", "`before-remove`"]
    );
}

/// REQ: F-067
#[test]
fn feature_067_before_create_is_a_transform_hook_over_repo_branch_base_and_agent() {
    let rows = points();
    assert_eq!(rows[0][1], "hook (transform)");
    assert_eq!(rows[0][3], "repo, branch, base, agent; reply: branch");
}

/// REQ: F-071
#[test]
fn feature_071_post_create_steps_is_a_collect_point_with_command_and_working_directory() {
    let rows = points();
    assert_eq!(rows[1][1], "collect");
    assert_eq!(rows[1][3], "entry: id, title, command, working_directory");
}

/// REQ: F-070
#[test]
fn feature_070_before_remove_is_a_veto_hook_over_path_branch_dirty_and_merged() {
    let rows = points();
    assert_eq!(rows[2][1], "hook (veto)");
    assert_eq!(rows[2][3], "path, branch, dirty, merged");
}

/// REQ: F-067
#[test]
fn feature_067_the_service_create_returns_a_path_and_the_topic_changed_carries_path_and_state() {
    let text = page_text();
    assert!(text.contains(
        "- `ilium.core.worktrees/create`: request `{ repo, branch, base?, agent? }`, response `{ path, branch }`."
    ));
    assert!(text.contains("- `ilium.core.worktrees/changed`: { path, state }"));
}

/// REQ: EXT-050
#[test]
fn extension_050_the_page_states_the_plugin_is_replaceable() {
    assert!(page_text().contains("**Replaceable:** yes."));
}

/// REQ: EXT-050
#[test]
fn extension_050_summary_row_counts_three_points_one_service_one_topic_and_replaceable() {
    let summary = page("reference-core-extension-points");
    let row = summary
        .lines()
        .find(|line| line.contains("`ilium.core.worktrees` |"))
        .expect("summary row");
    let cells: Vec<&str> = row.trim_matches('|').split('|').map(str::trim).collect();
    assert_eq!(&cells[2..], ["3", "1", "1", "yes"]);
}

/// REQ: EXT-050
#[test]
fn extension_050_core_points_page_repeats_the_three_points_with_kind_and_risk() {
    let core = page("reference-core-extension-points");
    assert!(core.contains("| `ilium.core.worktrees/before-create` | hook: transform | standard | Choose the branch name or refuse a new worktree. |"));
    assert!(core.contains("| `ilium.core.worktrees/post-create-steps` | collect | standard | Add a step that runs after a worktree is created. |"));
    assert!(core.contains("| `ilium.core.worktrees/before-remove` | hook: veto | standard | Refuse removing a worktree. |"));
}

/// REQ: EXT-050
#[test]
fn extension_050_a_collect_example_entry_needs_a_process_run_grant_by_the_rule_of_the_reference() {
    let rules = page("reference-requirements-extension-points");
    let row = rules
        .lines()
        .find(|line| line.contains("[EXT-050]"))
        .expect("EXT-050 row");
    assert!(row.contains("worktrees/post-create-steps.command"));
    assert!(row.contains("manifest.grant-dependency"));
}

/// REQ: EXT-050
#[test]
fn extension_050_the_example_contribution_names_the_point_an_id_a_priority_and_a_command_entry() {
    let text = page_text();
    let start = text
        .find("#### Example: Runs `bun install`")
        .expect("example heading");
    let fences = json_fences(&text[start..]);
    let manifest: serde_json::Value = serde_json::from_str(&fences[0]).expect("example is JSON");
    let entry = &manifest["contributes"]["extensions"][0];
    assert_eq!(entry["point"], "ilium.core.worktrees/post-create-steps");
    assert_eq!(entry["id"], "install-deps");
    assert_eq!(entry["priority"], 50);
    assert_eq!(entry["entry"]["title"], "Install dependencies");
    assert_eq!(entry["entry"]["command"], "bun install");
    assert_eq!(
        manifest["capabilities"]["server"][0],
        "extensions.contribute:ilium.core.worktrees/post-create-steps"
    );
}

/// REQ: SEC-293
#[test]
fn security_293_post_create_steps_has_the_same_risk_on_the_page_as_in_the_security_table() {
    let security = page("reference-extension-security");
    let table_risk = security
        .lines()
        .find(|line| line.starts_with("| `ilium.core.worktrees/post-create-steps` |"))
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .nth(1)
                .expect("risk cell")
                .trim()
                .to_string()
        })
        .expect("security row");
    let page_risk = points()[1][4].clone();
    assert_eq!(page_risk, table_risk);
}

/// REQ: SEC-293
#[test]
fn security_293_before_remove_is_standard_on_the_page_and_in_the_security_table() {
    let security = page("reference-extension-security");
    assert!(security.contains(
        "| `ilium.core.tree/before-close`, `ilium.core.worktrees/before-remove` | standard |"
    ));
    assert_eq!(points()[2][4], "standard");
}

/// REQ: SEC-293
#[test]
fn security_293_before_create_is_standard_on_the_page_and_absent_from_the_raised_marks() {
    let security = page("reference-extension-security");
    assert!(!security.contains("`ilium.core.worktrees/before-create` |"));
    assert_eq!(points()[0][4], "standard");
}

/// REQ: SEC-293
#[test]
fn security_293_the_security_section_of_the_page_points_to_the_corrected_table() {
    let text = page_text();
    assert!(text.contains("[Extension security](/pages/reference-extension-security/) ([SEC-293])"));
}

/// REQ: COR-030
#[test]
fn cor_030_the_plugin_is_the_single_holder_of_the_git_kernel_tab() {
    let rows = table_after(&page_text(), "| Tab id");
    let _ = rows;
    let text = page_text();
    assert!(text.contains("| `git` | Git | `[git]` | Fleet |"));
    let migration = page("reference-config-and-data-migration");
    assert!(migration.contains("| `git` | Git | `[git]` | Worktrees | Fleet | `[git]` |"));
}

/// REQ: COR-033
#[test]
fn cor_033_the_git_table_keys_are_the_seven_documented_ones_and_none_is_renamed() {
    let migration = page("reference-config-and-data-migration");
    assert!(migration.contains(
        "| `[git]` | `default_where`, `branch_prefix`, `worktree_location_template`, `default_base`, `branch_line`, `setup_command`, `default_close_policy` | Worktrees | yes |"
    ));
}

/// REQ: COR-022
#[test]
fn cor_022_disabling_keeps_the_tab_read_only_with_the_stated_notice_and_fails_the_cli_flag() {
    let text = page_text();
    assert!(text.contains("\"Worktrees is disabled. Values are kept.\""));
    let boot = page("reference-kernel-and-boot");
    assert!(boot.contains("| Worktrees | `ilium new-pane --worktree` fails with `cli.owner-disabled` | Plain `ilium new-pane` works |"));
}

/// REQ: COR-011
#[test]
fn cor_011_the_user_guide_lists_the_seven_git_settings_with_their_keys_and_defaults() {
    let guide = user_doc("worktrees.md");
    let rows = table_after(&guide, "## Settings -> Git");
    let keys: Vec<(String, String)> = rows
        .iter()
        .map(|row| (row[1].clone(), row[2].clone()))
        .collect();
    assert_eq!(
        keys,
        [
            ("`default_where`".to_string(), "`here`".to_string()),
            ("`branch_prefix`".to_string(), "`agent/`".to_string()),
            (
                "`worktree_location_template`".to_string(),
                "`{repo_parent}/{repo_name}.worktrees/{branch_slug}`".to_string()
            ),
            ("`default_base`".to_string(), "`current`".to_string()),
            ("`branch_line`".to_string(), "`worktree_only`".to_string()),
            ("`setup_command`".to_string(), "empty".to_string()),
            ("`default_close_policy`".to_string(), "`keep`".to_string()),
        ]
    );
}

/// REQ: COR-011
#[test]
fn cor_011_the_default_worktree_path_template_is_documented_on_the_migration_page() {
    let migration = page("reference-config-and-data-migration");
    assert!(migration.contains("`<repo parent>/<main checkout name>.worktrees/<branch slug>`"));
    assert!(migration.contains("`{repo_parent}/{repo_name}.worktrees/{branch_slug}`"));
}

/// REQ: F-072
#[test]
fn feature_072_the_settings_sample_rows_are_a_path_a_text_and_a_notice_row() {
    let text = page_text();
    let start = text.find("const GIT_ROWS").expect("sample");
    let body = &text[start..];
    let open = body.find("r#\"").expect("raw string") + 3;
    let close = body.find("\"#;").expect("raw string end");
    let rows: serde_json::Value = serde_json::from_str(&body[open..close]).expect("rows are JSON");
    let types: Vec<&str> = rows
        .as_array()
        .expect("rows")
        .iter()
        .map(|row| row["type"].as_str().expect("type"))
        .collect();
    assert_eq!(types, ["path", "text", "notice"]);
    assert_eq!(rows[0]["key"], "worktree_root");
    assert_eq!(rows[0]["picker"], "directory");
    assert_eq!(rows[1]["key"], "post_create");
    assert_eq!(rows[1]["default"], "");
    assert_eq!(rows[2]["tone"], "warning");
    assert_eq!(rows[2]["dismissible"], false);
    assert_eq!(
        rows[2]["text"],
        "A worktree with uncommitted changes or a running process cannot be removed."
    );
}

/// REQ: F-072
#[test]
fn feature_072_the_page_says_a_settings_notice_is_static_and_the_live_blocker_is_a_panel_notice() {
    assert!(page_text().contains("A settings notice cannot change at run time, so the live blocker for one worktree is shown by a `notice` widget in the client panel."));
}
