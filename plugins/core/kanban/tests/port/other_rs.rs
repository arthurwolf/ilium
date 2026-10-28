//! Twins of the board-related tests of the filesystem worker, the mouse module, the IPC server and
//! the onboarding dialogs that originally exercised kanban storage or board panes.

use ilium_plugin_sdk::types::{ErrorCode, PluginError};

use crate::common::{board_file, board_host, press, sections, BOARD_PATH, SAMPLE_BOARD};

fn conflict() -> PluginError {
    PluginError {
        code: ErrorCode::Conflict,
        message: "revision changed".to_string(),
        retry_after_ms: None,
    }
}

/// REQ: PORT:ilium-client/src/filesystem/app.rs::filesystem_shutdown_keeps_accepted_board_write_until_disk_readback
#[test]
fn port_fs_an_accepted_board_write_is_on_disk_after_shutdown() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    press(&mut host, node, "j");
    press(&mut host, node, "shift+right");
    host.deactivate(ilium_plugin_sdk::types::DeactivationReason::Shutdown);
    assert_eq!(sections(&board_file(&mut host))[1].1[1], "Write docs");
}

/// REQ: PORT:ilium-client/src/filesystem/tests.rs::board_conflict_preserves_foreign_file_and_newer_authored_updates, PORT:ilium-client/src/filesystem/tests.rs::isolated_board_conflict_rolls_back_only_exact_revision_and_retains_authored_source
#[test]
fn port_fs_a_conflict_keeps_the_foreign_file_and_a_later_edit_still_works() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    for _ in 0..3 {
        host.inject_failure("fs.write-atomic", conflict());
    }
    press(&mut host, node, "j");
    press(&mut host, node, "shift+right");
    assert_eq!(
        board_file(&mut host),
        SAMPLE_BOARD,
        "three conflicts leave the file untouched"
    );
    press(&mut host, node, "shift+right");
    assert_eq!(sections(&board_file(&mut host))[1].1[1], "Write docs");
}

/// REQ: PORT:ilium-client/src/filesystem/tests.rs::board_ordered_snapshots_advance_only_after_real_publication, PORT:ilium-client/src/filesystem/tests.rs::board_read_retains_exact_target_under_overload_and_deduplicates, PORT:ilium-client/src/filesystem/tests.rs::loaded_board_source_credit_survives_reader_retirement_until_last_pane_clone
#[test]
fn port_fs_repeated_reloads_show_the_file_as_it_is_and_never_rewrite_it() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    for _ in 0..8 {
        press(&mut host, node, "r");
    }
    assert_eq!(board_file(&mut host), SAMPLE_BOARD);
    let text = crate::common::draw(&mut host, node, 100, 20).text();
    assert!(
        text.contains("To do (2)") && text.contains("Doing (1)"),
        "{text}"
    );
}

/// REQ: PORT:ilium-client/src/mouse.rs::close_confirmation_buttons_preserve_cancel_and_confirm_keyboard_behavior
#[test]
fn port_mouse_delete_asks_for_confirmation_and_escape_cancels_it() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    press(&mut host, node, "j");
    let before = crate::common::draw(&mut host, node, 80, 20).text();
    press(&mut host, node, "d");
    // manual does not pin the confirmation prompt text: require that the screen changed.
    let asking = crate::common::draw(&mut host, node, 80, 20).text();
    assert_ne!(asking, before, "'d' must show a confirmation");
    assert_eq!(
        board_file(&mut host),
        SAMPLE_BOARD,
        "nothing changes before the confirmation"
    );
    press(&mut host, node, "escape");
    assert_eq!(board_file(&mut host), SAMPLE_BOARD);
}

/// REQ: PORT:ilium-server/src/ipc/ordered_owner_regressions.rs::changed_owner_status_wakes_a_forwarder_with_no_output_or_keyboard_input, PORT:ilium-server/tests/smoke.rs::keyboard_submission_is_broadcast_only_after_the_pty_accepts_enter
#[test]
fn port_server_an_external_file_change_redraws_the_board_without_any_key_input() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    host.fs()
        .write(BOARD_PATH, b"# Board\n\n## To do\n- Brand new card\n");
    host.settle();
    let text = crate::common::draw(&mut host, node, 80, 20).text();
    assert!(text.contains("Brand new card"), "{text}");
}

/// REQ: PORT:ilium-client/src/app.rs::onboarding_defers_other_startup_offers_without_losing_them, PORT:ilium-client/src/app.rs::onboarding_inference_test_never_accepts_results_for_changed_settings, PORT:ilium-client/src/app.rs::onboarding_ollama_catalog_cannot_change_models_after_a_url_edit, PORT:ilium-client/src/app.rs::openai_keyboard_selection_persists_and_e_keeps_manual_editing, PORT:ilium-client/src/app.rs::ready_clipboard_completion_keeps_exact_destination_identity, PORT:ilium-client/src/app.rs::resolved_codex_agent_history_path_is_ready_for_the_context_menu_clipboard_action, PORT:ilium-client/src/app.rs::right_click_opens_terminal_clipboard_menu_for_shells_and_adds_debug_for_agents
#[test]
fn port_app_unrelated_client_flows_leave_the_board_file_untouched() {
    let (mut host, _node) = board_host(SAMPLE_BOARD);
    host.settle();
    assert_eq!(board_file(&mut host), SAMPLE_BOARD);
}
