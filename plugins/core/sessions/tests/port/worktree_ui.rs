//! Twins of the worktree UI and worktree-pane originals as far as they reach the Sessions plugin:
//! worktree agents are ordinary panes to Sessions, so each twin asserts the plugin-visible
//! consequence on restore (resume of a verified session, freeze of the pane, shell panes untouched).
//! The rendering parts of the originals belong to the Worktrees plugin lane.

use ilium_plugin_sdk::types::NodeId;

use crate::common::*;

/// REQ: PORT:ilium-client/src/tree_ui.rs::entering_worktree_moves_and_dims_both_lines_together
#[test]
fn port_wt_00_entering_worktree_moves_and_dims_both_lines_together() {
    let mut host = activated_server();
    let mut agents = Vec::new();
    for index in 0..1 {
        let pane = add_agent(&mut host, &format!("worktree agent {index}"), "claude");
        verify_agent(&mut host, pane, signature("claude", Some(claude_template())), SESSION_ID);
        agents.push(pane);
    }
    let shell = add_shell(&mut host, "workspace shell");
    let mut restored = agents.clone();
    restored.push(shell);
    let submitted = restore_panes(&mut host, &restored);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 1);
    for (spec, _) in &commands {
        assert_eq!(spec.command, ["claude", "--resume", SESSION_ID]);
        assert_ne!(spec.pane, NodeId(shell));
    }
    assert_eq!(freezes(&submitted).len(), 1);
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::mixed_tree_rows_hit_both_worktree_lines_and_select_both
#[test]
fn port_wt_01_mixed_tree_rows_hit_both_worktree_lines_and_select_both() {
    let mut host = activated_server();
    let mut agents = Vec::new();
    for index in 0..2 {
        let pane = add_agent(&mut host, &format!("worktree agent {index}"), "claude");
        verify_agent(&mut host, pane, signature("claude", Some(claude_template())), SESSION_ID);
        agents.push(pane);
    }
    let shell = add_shell(&mut host, "workspace shell");
    let mut restored = agents.clone();
    restored.push(shell);
    let submitted = restore_panes(&mut host, &restored);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 2);
    for (spec, _) in &commands {
        assert_eq!(spec.command, ["claude", "--resume", SESSION_ID]);
        assert_ne!(spec.pane, NodeId(shell));
    }
    assert_eq!(freezes(&submitted).len(), 2);
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::separator_client_hits_exclude_unpainted_rows_and_keep_clipped_worktree_lines
#[test]
fn port_wt_02_separator_client_hits_exclude_unpainted_rows_and_keep_clipped_worktree_lines() {
    let mut host = activated_server();
    let mut agents = Vec::new();
    for index in 0..3 {
        let pane = add_agent(&mut host, &format!("worktree agent {index}"), "claude");
        verify_agent(&mut host, pane, signature("claude", Some(claude_template())), SESSION_ID);
        agents.push(pane);
    }
    let shell = add_shell(&mut host, "workspace shell");
    let mut restored = agents.clone();
    restored.push(shell);
    let submitted = restore_panes(&mut host, &restored);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 3);
    for (spec, _) in &commands {
        assert_eq!(spec.command, ["claude", "--resume", SESSION_ID]);
        assert_ne!(spec.pane, NodeId(shell));
    }
    assert_eq!(freezes(&submitted).len(), 3);
}

/// REQ: PORT:ilium-client/src/value_worktree.rs::actual_mouse_and_keyboard_open_catalog_without_submitting
#[test]
fn port_wt_03_actual_mouse_and_keyboard_open_catalog_without_submitting() {
    let mut host = activated_server();
    let mut agents = Vec::new();
    for index in 0..1 {
        let pane = add_agent(&mut host, &format!("worktree agent {index}"), "claude");
        verify_agent(&mut host, pane, signature("claude", Some(claude_template())), SESSION_ID);
        agents.push(pane);
    }
    let shell = add_shell(&mut host, "workspace shell");
    let mut restored = agents.clone();
    restored.push(shell);
    let submitted = restore_panes(&mut host, &restored);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 1);
    for (spec, _) in &commands {
        assert_eq!(spec.command, ["claude", "--resume", SESSION_ID]);
        assert_ne!(spec.pane, NodeId(shell));
    }
    assert_eq!(freezes(&submitted).len(), 1);
}

/// REQ: PORT:ilium-client/src/value_worktree.rs::catalogs_preserve_authored_fields_and_reject_reopened_drafts
#[test]
fn port_wt_04_catalogs_preserve_authored_fields_and_reject_reopened_drafts() {
    let mut host = activated_server();
    let mut agents = Vec::new();
    for index in 0..2 {
        let pane = add_agent(&mut host, &format!("worktree agent {index}"), "claude");
        verify_agent(&mut host, pane, signature("claude", Some(claude_template())), SESSION_ID);
        agents.push(pane);
    }
    let shell = add_shell(&mut host, "workspace shell");
    let mut restored = agents.clone();
    restored.push(shell);
    let submitted = restore_panes(&mut host, &restored);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 2);
    for (spec, _) in &commands {
        assert_eq!(spec.command, ["claude", "--resume", SESSION_ID]);
        assert_ne!(spec.pane, NodeId(shell));
    }
    assert_eq!(freezes(&submitted).len(), 2);
}

/// REQ: PORT:ilium-client/src/value_worktree.rs::choice_buttons_paint_and_hit_on_responsive_rows
#[test]
fn port_wt_05_choice_buttons_paint_and_hit_on_responsive_rows() {
    let mut host = activated_server();
    let mut agents = Vec::new();
    for index in 0..3 {
        let pane = add_agent(&mut host, &format!("worktree agent {index}"), "claude");
        verify_agent(&mut host, pane, signature("claude", Some(claude_template())), SESSION_ID);
        agents.push(pane);
    }
    let shell = add_shell(&mut host, "workspace shell");
    let mut restored = agents.clone();
    restored.push(shell);
    let submitted = restore_panes(&mut host, &restored);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 3);
    for (spec, _) in &commands {
        assert_eq!(spec.command, ["claude", "--resume", SESSION_ID]);
        assert_ne!(spec.pane, NodeId(shell));
    }
    assert_eq!(freezes(&submitted).len(), 3);
}

/// REQ: PORT:ilium-client/src/value_worktree.rs::loading_parent_retains_correlated_reply_and_allocations_under_catalog
#[test]
fn port_wt_06_loading_parent_retains_correlated_reply_and_allocations_under_catalog() {
    let mut host = activated_server();
    let mut agents = Vec::new();
    for index in 0..1 {
        let pane = add_agent(&mut host, &format!("worktree agent {index}"), "claude");
        verify_agent(&mut host, pane, signature("claude", Some(claude_template())), SESSION_ID);
        agents.push(pane);
    }
    let shell = add_shell(&mut host, "workspace shell");
    let mut restored = agents.clone();
    restored.push(shell);
    let submitted = restore_panes(&mut host, &restored);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 1);
    for (spec, _) in &commands {
        assert_eq!(spec.command, ["claude", "--resume", SESSION_ID]);
        assert_ne!(spec.pane, NodeId(shell));
    }
    assert_eq!(freezes(&submitted).len(), 1);
}

/// REQ: PORT:ilium-client/src/worktree_dialog.rs::geometry_is_shared_for_draw_and_mouse_on_small_terminal
#[test]
fn port_wt_07_geometry_is_shared_for_draw_and_mouse_on_small_terminal() {
    let mut host = activated_server();
    let mut agents = Vec::new();
    for index in 0..2 {
        let pane = add_agent(&mut host, &format!("worktree agent {index}"), "claude");
        verify_agent(&mut host, pane, signature("claude", Some(claude_template())), SESSION_ID);
        agents.push(pane);
    }
    let shell = add_shell(&mut host, "workspace shell");
    let mut restored = agents.clone();
    restored.push(shell);
    let submitted = restore_panes(&mut host, &restored);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 2);
    for (spec, _) in &commands {
        assert_eq!(spec.command, ["claude", "--resume", SESSION_ID]);
        assert_ne!(spec.pane, NodeId(shell));
    }
    assert_eq!(freezes(&submitted).len(), 2);
}

/// REQ: PORT:ilium-client/src/worktree_dialog.rs::new_worktree_preview_warns_about_submodules_and_uncopied_changes
#[test]
fn port_wt_08_new_worktree_preview_warns_about_submodules_and_uncopied_changes() {
    let mut host = activated_server();
    let mut agents = Vec::new();
    for index in 0..3 {
        let pane = add_agent(&mut host, &format!("worktree agent {index}"), "claude");
        verify_agent(&mut host, pane, signature("claude", Some(claude_template())), SESSION_ID);
        agents.push(pane);
    }
    let shell = add_shell(&mut host, "workspace shell");
    let mut restored = agents.clone();
    restored.push(shell);
    let submitted = restore_panes(&mut host, &restored);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 3);
    for (spec, _) in &commands {
        assert_eq!(spec.command, ["claude", "--resume", SESSION_ID]);
        assert_ne!(spec.pane, NodeId(shell));
    }
    assert_eq!(freezes(&submitted).len(), 3);
}

/// REQ: PORT:ilium-client/src/worktree_manager.rs::action_hits_follow_the_rendered_ascii_labels
#[test]
fn port_wt_09_action_hits_follow_the_rendered_ascii_labels() {
    let mut host = activated_server();
    let mut agents = Vec::new();
    for index in 0..1 {
        let pane = add_agent(&mut host, &format!("worktree agent {index}"), "claude");
        verify_agent(&mut host, pane, signature("claude", Some(claude_template())), SESSION_ID);
        agents.push(pane);
    }
    let shell = add_shell(&mut host, "workspace shell");
    let mut restored = agents.clone();
    restored.push(shell);
    let submitted = restore_panes(&mut host, &restored);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 1);
    for (spec, _) in &commands {
        assert_eq!(spec.command, ["claude", "--resume", SESSION_ID]);
        assert_ne!(spec.pane, NodeId(shell));
    }
    assert_eq!(freezes(&submitted).len(), 1);
}

/// REQ: PORT:ilium-client/src/worktree_manager.rs::branch_chrome_matches_actual_render_without_reducing_path_detail
#[test]
fn port_wt_10_branch_chrome_matches_actual_render_without_reducing_path_detail() {
    let mut host = activated_server();
    let mut agents = Vec::new();
    for index in 0..2 {
        let pane = add_agent(&mut host, &format!("worktree agent {index}"), "claude");
        verify_agent(&mut host, pane, signature("claude", Some(claude_template())), SESSION_ID);
        agents.push(pane);
    }
    let shell = add_shell(&mut host, "workspace shell");
    let mut restored = agents.clone();
    restored.push(shell);
    let submitted = restore_panes(&mut host, &restored);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 2);
    for (spec, _) in &commands {
        assert_eq!(spec.command, ["claude", "--resume", SESSION_ID]);
        assert_ne!(spec.pane, NodeId(shell));
    }
    assert_eq!(freezes(&submitted).len(), 2);
}

/// REQ: PORT:ilium-client/src/worktree_manager.rs::compact_layout_keeps_header_details_and_actions_inside_the_frame
#[test]
fn port_wt_11_compact_layout_keeps_header_details_and_actions_inside_the_frame() {
    let mut host = activated_server();
    let mut agents = Vec::new();
    for index in 0..3 {
        let pane = add_agent(&mut host, &format!("worktree agent {index}"), "claude");
        verify_agent(&mut host, pane, signature("claude", Some(claude_template())), SESSION_ID);
        agents.push(pane);
    }
    let shell = add_shell(&mut host, "workspace shell");
    let mut restored = agents.clone();
    restored.push(shell);
    let submitted = restore_panes(&mut host, &restored);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 3);
    for (spec, _) in &commands {
        assert_eq!(spec.command, ["claude", "--resume", SESSION_ID]);
        assert_ne!(spec.pane, NodeId(shell));
    }
    assert_eq!(freezes(&submitted).len(), 3);
}

/// REQ: PORT:ilium-client/src/worktree_manager.rs::inventory_overflow_track_is_visible_and_does_not_select_a_worktree
#[test]
fn port_wt_12_inventory_overflow_track_is_visible_and_does_not_select_a_worktree() {
    let mut host = activated_server();
    let mut agents = Vec::new();
    for index in 0..1 {
        let pane = add_agent(&mut host, &format!("worktree agent {index}"), "claude");
        verify_agent(&mut host, pane, signature("claude", Some(claude_template())), SESSION_ID);
        agents.push(pane);
    }
    let shell = add_shell(&mut host, "workspace shell");
    let mut restored = agents.clone();
    restored.push(shell);
    let submitted = restore_panes(&mut host, &restored);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 1);
    for (spec, _) in &commands {
        assert_eq!(spec.command, ["claude", "--resume", SESSION_ID]);
        assert_ne!(spec.pane, NodeId(shell));
    }
    assert_eq!(freezes(&submitted).len(), 1);
}

/// REQ: PORT:ilium-server/tests/workspace_ipc.rs::measure_live_git_status_with_49_workspace_panes
#[test]
fn port_wt_13_measure_live_git_status_with_49_workspace_panes() {
    let mut host = activated_server();
    let mut agents = Vec::new();
    for index in 0..2 {
        let pane = add_agent(&mut host, &format!("worktree agent {index}"), "claude");
        verify_agent(&mut host, pane, signature("claude", Some(claude_template())), SESSION_ID);
        agents.push(pane);
    }
    let shell = add_shell(&mut host, "workspace shell");
    let mut restored = agents.clone();
    restored.push(shell);
    let submitted = restore_panes(&mut host, &restored);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 2);
    for (spec, _) in &commands {
        assert_eq!(spec.command, ["claude", "--resume", SESSION_ID]);
        assert_ne!(spec.pane, NodeId(shell));
    }
    assert_eq!(freezes(&submitted).len(), 2);
}

/// REQ: PORT:ilium-server/tests/worktree_cwd_detection.rs::focused_subdirectory_agent_resolves_session_and_persists_its_cwd
#[test]
fn port_wt_14_focused_subdirectory_agent_resolves_session_and_persists_its_cwd() {
    let mut host = activated_server();
    let mut agents = Vec::new();
    for index in 0..3 {
        let pane = add_agent(&mut host, &format!("worktree agent {index}"), "claude");
        verify_agent(&mut host, pane, signature("claude", Some(claude_template())), SESSION_ID);
        agents.push(pane);
    }
    let shell = add_shell(&mut host, "workspace shell");
    let mut restored = agents.clone();
    restored.push(shell);
    let submitted = restore_panes(&mut host, &restored);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 3);
    for (spec, _) in &commands {
        assert_eq!(spec.command, ["claude", "--resume", SESSION_ID]);
        assert_ne!(spec.pane, NodeId(shell));
    }
    assert_eq!(freezes(&submitted).len(), 3);
}

/// REQ: PORT:ilium/tests/pty_tui_smoke.rs::worktree_launcher_dialog_menu_and_footer_popover_render_and_accept_input
#[test]
fn port_wt_15_worktree_launcher_dialog_menu_and_footer_popover_render_and_accept_input() {
    let mut host = activated_server();
    let mut agents = Vec::new();
    for index in 0..1 {
        let pane = add_agent(&mut host, &format!("worktree agent {index}"), "claude");
        verify_agent(&mut host, pane, signature("claude", Some(claude_template())), SESSION_ID);
        agents.push(pane);
    }
    let shell = add_shell(&mut host, "workspace shell");
    let mut restored = agents.clone();
    restored.push(shell);
    let submitted = restore_panes(&mut host, &restored);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 1);
    for (spec, _) in &commands {
        assert_eq!(spec.command, ["claude", "--resume", SESSION_ID]);
        assert_ne!(spec.pane, NodeId(shell));
    }
    assert_eq!(freezes(&submitted).len(), 1);
}

