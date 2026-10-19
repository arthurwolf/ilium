//! Plugin-level twins of the kernel UI tests of the worktree feature: what the manual defines for
//! them is the rendered branch line (leader-row), the `remove` guard and the server-side facts.
//! Kernel layout and hit testing have no manual surface; the twins assert the plugin's share.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{Event, NodeId, ViewTarget};
use serde_json::json;

use crate::common::*;
use crate::support::*;

const PANEL: &str = "worktrees";

fn publish(host: &mut FakeHost, path: &str, state: &str) {
    host.deliver_one(changed_topic(path, state));
    host.settle();
}

fn lines(host: &mut FakeHost, columns: u16, rows: u16) -> Vec<String> {
    let screen = host.render(ViewTarget::Panel(PANEL.to_string()), columns, rows);
    (0..screen.rows)
        .map(|row| screen.row(row).trim_end().to_string())
        .collect()
}

fn branch_lines(host: &mut FakeHost, columns: u16, rows: u16) -> Vec<String> {
    lines(host, columns, rows)
        .into_iter()
        .filter(|row| row.contains('🌿'))
        .collect()
}

fn two(host: &mut FakeHost) {
    publish(host, "/work/ilium.worktrees/fix-auth", "clean");
    publish(host, "/work/ilium.worktrees/docs-sweep", "2 changed");
}

/// REQ: F-069, PORT:ilium-client/src/tree_ui.rs::entering_worktree_moves_and_dims_both_lines_together
#[test]
fn port_069_a_state_change_updates_label_and_state_of_the_same_branch_line() {
    let mut host = client_host();
    publish(&mut host, "/work/ilium.worktrees/fix-auth", "clean");
    publish(&mut host, "/work/ilium.worktrees/fix-auth", "2 changed");
    let rows = branch_lines(&mut host, 48, 12);
    assert_eq!(rows.len(), 1, "one line per worktree: {rows:?}");
    assert!(
        rows[0].contains("🌿 fix-auth") && rows[0].ends_with("2 changed"),
        "{}",
        rows[0]
    );
}

/// REQ: F-069, PORT:ilium-client/src/tree_ui.rs::mixed_tree_rows_hit_both_worktree_lines_and_select_both
#[test]
fn port_069_two_worktrees_render_two_branch_lines_in_publication_order() {
    let mut host = client_host();
    two(&mut host);
    let rows = branch_lines(&mut host, 48, 12);
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(
        rows[0].contains("fix-auth") && rows[0].ends_with("clean"),
        "{}",
        rows[0]
    );
    assert!(
        rows[1].contains("docs-sweep") && rows[1].ends_with("2 changed"),
        "{}",
        rows[1]
    );
}

/// REQ: F-069, PORT:ilium-client/src/tree_ui.rs::separator_client_hits_exclude_unpainted_rows_and_keep_clipped_worktree_lines
#[test]
fn port_069_a_panel_too_short_for_all_lines_paints_no_more_rows_than_it_has() {
    let mut host = client_host();
    two(&mut host);
    for rows in [1u16, 2, 3] {
        let painted = lines(&mut host, 48, rows);
        assert_eq!(painted.len(), rows as usize);
        assert!(branch_lines(&mut host, 48, rows).len() <= rows as usize);
    }
    // manual does not pin the exact clipping beyond "first lines survive, later ones are cut"
    let clipped = branch_lines(&mut host, 48, 2);
    assert!(
        clipped.iter().any(|row| row.contains("fix-auth")),
        "the first branch line survives: {clipped:?}"
    );
    assert!(
        clipped.iter().all(|row| !row.contains("docs-sweep")),
        "the second branch line is clipped at two rows: {clipped:?}"
    );
}

/// REQ: F-072, PORT:ilium-client/src/value_worktree.rs::actual_mouse_and_keyboard_open_catalog_without_submitting
#[test]
fn port_072_reading_the_git_tab_submits_nothing_and_changes_no_setting() {
    let mut host = client_host();
    let before = host.kernel_settings().get("git", "default_where");
    let submitted = host.submitted().len();
    let _ = host.render(ViewTarget::SettingsPage("git".to_string()), 48, 8);
    assert_eq!(host.kernel_settings().get("git", "default_where"), before);
    assert_eq!(host.submitted().len(), submitted);
    assert!(host.process().calls().is_empty());
}

/// REQ: F-072, PORT:ilium-client/src/value_worktree.rs::catalogs_preserve_authored_fields_and_reject_reopened_drafts
#[test]
fn port_072_choosing_a_default_where_keeps_the_authored_branch_prefix() {
    let mut host = client_host();
    set_git(&mut host, "branch_prefix", json!("team/")).expect("valid");
    set_git(&mut host, "default_where", json!("new_worktree")).expect("valid");
    assert_eq!(
        host.kernel_settings().get("git", "branch_prefix"),
        Some(json!("team/"))
    );
    assert_eq!(
        host.kernel_settings().get("git", "default_where"),
        Some(json!("new_worktree"))
    );
}

/// REQ: F-072, PORT:ilium-client/src/value_worktree.rs::catalogs_preserve_authored_fields_and_reject_reopened_drafts
#[test]
fn port_072_an_unknown_default_where_is_refused_and_the_old_value_stays() {
    let mut host = client_host();
    set_git(&mut host, "default_where", json!("existing_worktree")).expect("valid");
    let error = set_git(&mut host, "default_where", json!("elsewhere"))
        .expect_err("an unknown default_where must be refused");
    assert_eq!(
        error.code,
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
    assert_eq!(
        host.kernel_settings().get("git", "default_where"),
        Some(json!("existing_worktree"))
    );
}

/// REQ: F-072, PORT:ilium-client/src/value_worktree.rs::choice_buttons_paint_and_hit_on_responsive_rows
#[test]
fn port_072_the_git_tab_paints_the_same_rows_at_every_width_inside_its_frame() {
    for columns in [40u16, 80, 120] {
        let mut host = client_host();
        let screen = host.render(ViewTarget::SettingsPage("git".to_string()), columns, 8);
        let rows: Vec<String> = (0..screen.rows)
            .map(|row| screen.row(row).trim_end().to_string())
            .collect();
        assert!(
            rows.iter().any(|row| row.contains("Worktree folder")),
            "{columns}: {rows:?}"
        );
        assert!(
            rows.iter().any(|row| row.contains("Post-create command")),
            "{columns}: {rows:?}"
        );
        assert!(
            rows.iter()
                .all(|row| row.chars().count() <= columns as usize),
            "{columns}"
        );
    }
}

/// REQ: F-067, F-072, PORT:ilium-client/src/value_worktree.rs::loading_parent_retains_correlated_reply_and_allocations_under_catalog
#[test]
fn port_067_a_request_sent_while_settings_change_uses_the_value_valid_at_creation() {
    let mut host = server_host_at("/projects/acme");
    set_git(&mut host, "branch_prefix", json!("one/")).expect("valid");
    host.deliver_one(new_agent_message(&json!({})));
    set_git(&mut host, "branch_prefix", json!("two/")).expect("valid");
    host.settle();
    let arguments = add_arguments(&mut host);
    assert!(
        arguments[3].starts_with("one/") && !arguments[3].starts_with("two/"),
        "the request keeps the prefix valid at its creation: {}",
        arguments[3]
    );
    assert_eq!(worktree_add_calls(&host.process().calls()).len(), 1);
}

/// REQ: F-067, PORT:ilium-client/src/worktree_dialog.rs::geometry_is_shared_for_draw_and_mouse_on_small_terminal
#[test]
fn port_067_the_branch_line_of_a_narrow_panel_stays_inside_the_width() {
    let mut host = client_host();
    two(&mut host);
    for columns in [20u16, 24, 30] {
        let rows = branch_lines(&mut host, columns, 12);
        assert_eq!(rows.len(), 2, "{columns}: {rows:?}");
        for row in rows {
            assert!(row.chars().count() <= columns as usize, "{columns}: {row}");
        }
    }
}

/// REQ: F-067, F-071, PORT:ilium-client/src/worktree_dialog.rs::new_worktree_preview_warns_about_submodules_and_uncopied_changes
#[test]
fn port_067_a_new_worktree_is_created_from_the_checked_out_commit_without_copying_changes() {
    let mut host = server_host_at("/projects/acme");
    with_repository_facts(&mut host);
    create_worktree(&mut host, &request("fix-auth"));
    let arguments = add_arguments(&mut host);
    assert_eq!(
        arguments[5], "topic",
        "the base is the checked-out branch; uncommitted files are not part of it"
    );
    assert!(host
        .process()
        .calls()
        .iter()
        .all(|call| call.program != "cp"));
}

/// REQ: F-070, PORT:ilium-client/src/worktree_manager.rs::action_hits_follow_the_rendered_ascii_labels
#[test]
fn port_070_removal_acts_on_exactly_the_path_the_request_names() {
    let mut host = server_host_at("/projects/acme");
    create_worktree(&mut host, &request("fix-auth"));
    remove_request(
        &mut host,
        json!({"path": "/projects/acme.worktrees/fix-auth", "branch": "fix-auth"}),
    );
    let removed = removal_calls(&mut host);
    assert_eq!(removed.len(), 1);
    assert!(removed[0]
        .iter()
        .any(|argument| argument == "/projects/acme.worktrees/fix-auth"));
}

/// REQ: F-069, PORT:ilium-client/src/worktree_manager.rs::branch_chrome_matches_actual_render_without_reducing_path_detail
#[test]
fn port_069_the_branch_line_keeps_the_full_branch_label_at_every_sufficient_width() {
    let mut host = client_host();
    publish(&mut host, "/work/ilium.worktrees/fix-auth", "clean");
    for columns in [40u16, 48, 80, 120] {
        let rows = branch_lines(&mut host, columns, 12);
        assert!(rows[0].contains("🌿 fix-auth"), "{columns}: {}", rows[0]);
    }
}

/// REQ: F-069, PORT:ilium-client/src/worktree_manager.rs::compact_layout_keeps_header_details_and_actions_inside_the_frame
#[test]
fn port_069_the_compact_tree_keeps_its_frame_rows_within_the_requested_size() {
    let mut host = client_host();
    two(&mut host);
    let painted = lines(&mut host, 40, 8);
    assert_eq!(painted.len(), 8);
    assert_eq!(branch_lines(&mut host, 40, 8).len(), 2);
    assert!(painted.iter().all(|row| row.chars().count() <= 40));
}

/// REQ: F-070, PORT:ilium-client/src/worktree_manager.rs::inventory_overflow_track_is_visible_and_does_not_select_a_worktree
#[test]
fn port_070_a_long_inventory_selects_and_removes_nothing_by_itself() {
    let mut host = client_host();
    for index in 0..30 {
        publish(
            &mut host,
            &format!("/work/ilium.worktrees/branch-{index}"),
            "clean",
        );
    }
    let visible = branch_lines(&mut host, 48, 8);
    assert!(!visible.is_empty(), "the inventory paints branch lines");
    assert!(visible.len() <= 8);
    assert!(host.process().calls().is_empty());
    assert!(host.submitted().is_empty());
}

/// REQ: F-069, PORT:ilium-server/tests/workspace_ipc.rs::measure_live_git_status_with_49_workspace_panes
#[test]
fn port_069_forty_nine_worktree_panes_never_run_more_than_two_probes_at_once() {
    let mut host = server_host_at("/projects/acme");
    for index in 0..49u64 {
        create_worktree(&mut host, &request(&format!("b{index}")));
        host.deliver_one(pane_created(100 + index, &format!("b{index}")));
    }
    host.settle();
    host.advance_clock(10_000);
    host.settle();
    assert!(host.process().max_concurrent() <= 2);
    assert_eq!(registered(&mut host).len(), 49);
}

/// REQ: F-067, PORT:ilium-server/tests/worktree_cwd_detection.rs::focused_subdirectory_agent_resolves_session_and_persists_its_cwd
#[test]
fn port_067_the_agent_pane_cwd_is_the_worktree_folder_and_the_registration_names_it() {
    let mut host = server_host_at("/projects/acme");
    let submitted = create_worktree(&mut host, &request("fix-auth"));
    let panes = create_panes(&submitted);
    assert_eq!(panes.len(), 1);
    assert!(
        cwd_text(&panes[0]).contains("/projects/acme.worktrees/fix-auth"),
        "{}",
        cwd_text(&panes[0])
    );
    let workspaces = registered(&mut host);
    assert_eq!(workspaces.len(), 1);
    assert_eq!(workspaces[0].branch, "fix-auth");
    host.deliver_one(pane_created(14, "fix-auth"));
    host.deliver_one(Event::NodeRemoved(NodeId(14)));
    host.settle();
    assert_eq!(
        registered(&mut host).len(),
        1,
        "closing the pane keeps the registration by default"
    );
}

/// REQ: F-067, PORT:ilium/tests/pty_tui_smoke.rs::worktree_launcher_dialog_menu_and_footer_popover_render_and_accept_input
#[test]
fn port_067_the_command_from_the_client_creates_one_worktree_pane_on_the_server() {
    let mut client = client_host();
    let submitted = client.deliver_one(command_invoked(
        COMMAND,
        r#"{"branch":"fix-auth"}"#,
        ilium_plugin_sdk::types::CommandOrigin::Keybinding,
    ));
    let messages = server_messages(&submitted);
    assert_eq!(messages.len(), 1);
    let mut server = server_host_at("/projects/acme");
    let payload: serde_json::Value = serde_json::from_str(&messages[0].1).expect("JSON");
    let panes = create_panes(&create_worktree(&mut server, &payload));
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].title, "fix-auth");
}
