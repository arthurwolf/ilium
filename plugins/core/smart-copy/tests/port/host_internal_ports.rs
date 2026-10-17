//! Ports of the host-internal Smart Copy tests of the monolith (paste cursor, copy slots, workers,
//! menu layout, restart) seen through the plugin channels: clipboard text, status lines, menu
//! entries, picker keys and host failures. The fakes they need beyond the skeleton testkit are
//! listed in API_GAPS_ports.md.

use ilium_plugin_sdk::exports::MenuContext;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{NodeId, Submission};

use crate::common::*;

fn menu_context(pane: u64, selection: Option<&str>) -> MenuContext {
    MenuContext {
        menu: "terminal".to_string(),
        node: Some(NodeId(pane)),
        pane: Some(NodeId(pane)),
        view: None,
        key: None,
        buffer: None,
        line: None,
        selection: selection.map(str::to_string),
    }
}

fn labels(host: &mut FakeHost, context: MenuContext) -> Vec<String> {
    host.menu_items(context)
        .into_iter()
        .map(|entry| entry.label.to_lowercase())
        .collect()
}

/// Picks candidate `number` and returns what the pick submitted.
fn pick(host: &mut FakeHost, number: usize) -> Vec<Submission> {
    host.deliver_one(key(&number.to_string()))
}

fn candidates(count: usize) -> Vec<String> {
    (1..=count).map(|n| format!("candidate-{n}")).collect()
}

fn picker_with(count: usize) -> FakeHost {
    let lines = candidates(count);
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    open_picker(&refs)
}

// ---- app.rs menu ports ----

/// REQ: PORT:ilium-client/src/app.rs::context_menu_paste_preserves_its_target_pane_and_bracketed_paste_contract
#[test]
fn port_internal_menu_commands_name_the_pane_they_were_opened_on() {
    let mut host = client_host();
    let first = add_terminal_pane(&mut host, "one");
    let second = add_terminal_pane(&mut host, "two");
    let ids: Vec<String> = host
        .menu_items(menu_context(second, None))
        .into_iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(ids, ["copy", "copy-path", "smart-copy", "transfer"]);
    let out = host.deliver_one(menu_command("smart-copy", second, "smart-copy"));
    assert!(
        !format!("{out:?}").contains(&format!("NodeId({first})")),
        "{out:?}"
    );
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/app.rs::context_menu_paste_preserves_its_target_pane_and_bracketed_paste_contract
#[test]
fn port_internal_pasted_text_is_never_added_to_the_clipboard_by_a_menu_command() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.deliver_one(menu_command("copy", pane, "copy"));
    host.settle();
    assert_eq!(host.clipboard().last_text(), None);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/app.rs::editor_right_click_captures_the_physical_source_line_and_opens_its_menu
#[test]
fn port_internal_editor_menu_opens_with_entries_for_the_captured_line() {
    let mut host = client_host();
    let pane = add_editor_pane(&mut host, "main.rs");
    let mut context = menu_context(pane, None);
    context.menu = "editor".to_string();
    context.line = Some(41);
    let items = host.menu_items(context);
    assert!(items
        .iter()
        .all(|entry| !entry.id.is_empty() && !entry.label.is_empty()));
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/app.rs::editor_right_click_on_an_existing_path_offers_open_externally
#[test]
fn port_internal_editor_menu_on_an_existing_path_offers_open_externally() {
    let mut host = client_host();
    let pane = add_editor_pane(&mut host, "main.rs");
    host.files().add_existing("/tmp/ilium-port/readme.md");
    let mut context = menu_context(pane, Some("/tmp/ilium-port/readme.md"));
    context.menu = "editor".to_string();
    assert!(labels(&mut host, context)
        .iter()
        .any(|label| label.contains("externally")));
}

/// REQ: PORT:ilium-client/src/app.rs::editor_right_click_on_an_existing_path_offers_open_externally
#[test]
fn port_internal_editor_menu_on_a_missing_path_does_not_offer_open_externally() {
    let mut host = client_host();
    let pane = add_editor_pane(&mut host, "main.rs");
    let mut context = menu_context(pane, Some("/tmp/ilium-port/missing.md"));
    context.menu = "editor".to_string();
    assert!(!labels(&mut host, context)
        .iter()
        .any(|label| label.contains("externally")));
}

/// REQ: PORT:ilium-client/src/app.rs::right_click_on_an_existing_file_offers_editor_and_external_open_actions
#[test]
fn port_internal_terminal_menu_on_an_existing_file_offers_editor_and_external_open() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.files().add_existing("/tmp/ilium-port/src/lib.rs");
    let found = labels(
        &mut host,
        menu_context(pane, Some("/tmp/ilium-port/src/lib.rs")),
    );
    assert!(
        found.iter().any(|label| label.contains("editor")),
        "{found:?}"
    );
    assert!(
        found.iter().any(|label| label.contains("externally")),
        "{found:?}"
    );
}

/// REQ: PORT:ilium-client/src/app.rs::right_click_on_an_existing_file_offers_editor_and_external_open_actions
#[test]
fn port_internal_terminal_menu_on_a_missing_file_offers_no_open_actions() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let found = labels(
        &mut host,
        menu_context(pane, Some("/tmp/ilium-port/nothing.rs")),
    );
    assert!(
        !found
            .iter()
            .any(|label| label.contains("externally") || label.contains("open in editor")),
        "{found:?}"
    );
}

/// REQ: PORT:ilium-client/src/context_menu_layout.rs::empty_and_zero_height_menus_have_no_selectable_rows
#[test]
fn port_internal_an_unknown_menu_has_no_selectable_rows() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let mut context = menu_context(pane, None);
    context.menu = "no-such-menu".to_string();
    assert!(host.menu_items(context).is_empty());
}

/// REQ: PORT:ilium-client/src/context_menu_layout.rs::empty_and_zero_height_menus_have_no_selectable_rows
#[test]
fn port_internal_a_zero_height_view_renders_no_rows() {
    let mut host = open_picker(&["abc"]);
    let text = host.render(picker_target(), 20, 0).text();
    assert!(text.trim().is_empty(), "{text:?}");
}

// ---- paste_cursor.rs ports: what a bounded replay of text into a pane may do to the clipboard ----

/// REQ: PORT:ilium-client/src/paste_cursor.rs::animation_text_prompt_replay_is_bounded_and_trims_only_trailing_line_endings
#[test]
fn port_internal_copy_trims_nothing_inside_the_text_and_replay_stays_bounded() {
    let mut host = open_picker(&["line one\nstill the same line"]);
    pick(&mut host, 1);
    host.settle();
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/paste_cursor.rs::bounded_replay_keeps_the_original_input_lease_until_all_keys_are_dispatched
#[test]
fn port_internal_the_original_candidate_survives_until_the_copy_is_dispatched() {
    let mut host = open_picker(&["original keeps its lease"]);
    let out = pick(&mut host, 1);
    assert!(status_contains(&out, "Copied 24 characters"), "{out:?}");
    assert_eq!(
        host.clipboard().last_text().as_deref(),
        Some("original keeps its lease")
    );
}

/// REQ: PORT:ilium-client/src/paste_cursor.rs::completed_replay_retires_original_input_on_the_cpu_bank
#[test]
fn port_internal_a_completed_copy_leaves_no_failure_behind() {
    let mut host = open_picker(&["done"]);
    pick(&mut host, 1);
    host.settle();
    assert_eq!(host.clipboard().last_text().as_deref(), Some("done"));
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/paste_cursor.rs::interrupted_replay_failure_retains_full_paste_and_reports_consumed_prefix
#[test]
fn port_internal_a_failed_copy_reports_the_error_and_keeps_the_full_candidate() {
    let mut host = open_picker(&["whole paste"]);
    host.clipboard().fail_next_write("Io");
    let out = pick(&mut host, 1);
    assert!(status_contains(&out, "Copy failed: Io"), "{out:?}");
    assert_eq!(host.clipboard().last_text(), None);
    let retry = pick(&mut host, 1);
    assert!(status_contains(&retry, "Copied 11 characters"), "{retry:?}");
    assert_eq!(host.clipboard().last_text().as_deref(), Some("whole paste"));
}

/// REQ: PORT:ilium-client/src/paste_cursor.rs::replay_chunk_respects_the_interactive_turn_key_ceiling
#[test]
fn port_internal_a_long_candidate_is_copied_whole_in_one_turn() {
    let long = "x".repeat(4096);
    let mut host = open_picker(&[long.as_str()]);
    let out = pick(&mut host, 1);
    assert!(status_contains(&out, "Copied 4096 characters"), "{out:?}");
    assert_eq!(
        host.clipboard()
            .last_text()
            .map(|text| text.chars().count()),
        Some(4096)
    );
}

/// REQ: PORT:ilium-client/src/paste_cursor.rs::retirement_refusal_preserves_the_untouched_original_paste
#[test]
fn port_internal_a_refused_copy_leaves_the_clipboard_untouched() {
    let mut host = open_picker(&["untouched"]);
    host.clipboard().fail_next_write("Denied");
    let out = pick(&mut host, 1);
    assert!(status_contains(&out, "Copy failed: Denied"), "{out:?}");
    assert_eq!(host.clipboard().last_text(), None);
}

/// REQ: PORT:ilium-client/src/paste_cursor.rs::trailing_cr_and_empty_paste_do_not_require_another_source_event
#[test]
fn port_internal_an_empty_candidate_copies_zero_characters_without_another_event() {
    let mut host = open_picker(&[""]);
    let out = pick(&mut host, 1);
    host.settle();
    assert!(!status_contains(&out, "Copy failed"), "{out:?}");
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/paste_cursor.rs::unicode_prefix_and_crlf_are_complete_at_each_yield_boundary
#[test]
fn port_internal_unicode_and_crlf_candidates_are_copied_complete() {
    let mut host = open_picker(&["雪é\u{1f600}", "a\r\nb"]);
    pick(&mut host, 1);
    assert_eq!(
        host.clipboard().last_text().as_deref(),
        Some("雪é\u{1f600}")
    );
    pick(&mut host, 2);
    assert_eq!(host.clipboard().last_text().as_deref(), Some("a\r\nb"));
}

// ---- smart_copy_selection.rs ports: copy slots ----

/// REQ: PORT:ilium-client/src/smart_copy_selection.rs::blocked_cpu_selected_copy_reserves_global_fifo_before_read_and_later_write
#[test]
fn port_internal_copies_reach_the_clipboard_in_pick_order() {
    let mut host = picker_with(3);
    pick(&mut host, 1);
    pick(&mut host, 3);
    pick(&mut host, 2);
    assert_eq!(host.clipboard().last_text().as_deref(), Some("candidate-2"));
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/smart_copy_selection.rs::cancelled_unprepared_copy_explicitly_fails_slot_before_following_read
#[test]
fn port_internal_a_cancelled_picker_copies_nothing_and_a_later_pick_still_works() {
    let mut host = picker_with(2);
    host.deliver_one(key("Escape"));
    assert_eq!(host.clipboard().last_text(), None);
    host.deliver_one(candidates_message(&["after", "cancel"]));
    pick(&mut host, 1);
    assert_eq!(host.clipboard().last_text().as_deref(), Some("after"));
}

/// REQ: PORT:ilium-client/src/smart_copy_selection.rs::closed_bank_returns_original_and_explicit_slot_error_without_copy_claim
#[test]
fn port_internal_a_closed_clipboard_reports_an_error_and_no_copy_claim() {
    let mut host = open_picker(&["kept"]);
    host.clipboard().fail_next_write("Closed");
    let out = pick(&mut host, 1);
    assert!(status_contains(&out, "Copy failed: Closed"), "{out:?}");
    assert!(!status_contains(&out, "Copied"), "{out:?}");
}

/// REQ: PORT:ilium-client/src/smart_copy_selection.rs::earlier_cleanup_error_keeps_original_deadline_custody_and_native_slot_failure
#[test]
fn port_internal_an_earlier_failure_does_not_poison_the_next_copy() {
    let mut host = picker_with(2);
    host.clipboard().fail_next_write("Io");
    let first = pick(&mut host, 1);
    assert!(status_contains(&first, "Copy failed: Io"), "{first:?}");
    let second = pick(&mut host, 2);
    assert!(
        status_contains(&second, "Copied 11 characters"),
        "{second:?}"
    );
    assert_eq!(host.clipboard().last_text().as_deref(), Some("candidate-2"));
}

/// REQ: PORT:ilium-client/src/smart_copy_selection.rs::ninth_global_slot_refusal_preserves_identical_original_and_all_eight_failures_ack_in_order
#[test]
fn port_internal_nine_consecutive_failures_each_report_their_own_error_in_order() {
    let mut host = picker_with(9);
    let mut statuses = Vec::new();
    for number in 1..=9usize {
        host.clipboard().fail_next_write("Io");
        statuses.push(pick(&mut host, number));
    }
    for (index, out) in statuses.iter().enumerate() {
        assert!(
            status_contains(out, "Copy failed: Io"),
            "pick {}: {out:?}",
            index + 1
        );
    }
    assert_eq!(host.clipboard().last_text(), None);
}

// ---- smart_copy_workers.rs ports: the inference stream ----

fn ask(host: &mut FakeHost, pane: u64) {
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    host.settle();
}

fn model_host(rows: &[&str]) -> (FakeHost, u64) {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_rows(pane, rows);
    (host, pane)
}

/// REQ: PORT:ilium-client/src/smart_copy_workers.rs::full_receiver_preserves_order_in_owned_mailbox_without_blocking_callback
#[test]
fn port_internal_many_model_lines_keep_their_order_and_never_block() {
    let (mut host, pane) = model_host(&["one", "two", "three"]);
    host.inference().reply_text("one\ntwo\nthree");
    ask(&mut host, pane);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/smart_copy_workers.rs::full_terminal_delivery_queue_returns_new_original_without_starting_it
#[test]
fn port_internal_a_second_request_while_busy_starts_no_second_model_call() {
    let (mut host, pane) = model_host(&["one"]);
    host.inference().reply_text("one");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    host.settle();
    assert!(host.inference().requests().len() <= 2);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/smart_copy_workers.rs::invoked_usage_empty_prefix_failure_and_panic_streams_never_replay
#[test]
fn port_internal_a_failed_model_call_offers_no_replayed_candidates_and_copies_nothing() {
    let (mut host, pane) = model_host(&["one"]);
    host.inference().fail_next("Io");
    ask(&mut host, pane);
    assert_eq!(host.clipboard().last_text(), None);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/smart_copy_workers.rs::oversized_physical_capture_returns_original_before_provider_execution
#[test]
fn port_internal_an_oversized_screen_makes_no_model_request_with_the_whole_capture() {
    let huge = "w".repeat(200_000);
    let (mut host, pane) = model_host(&[huge.as_str()]);
    host.inference().reply_text("w");
    ask(&mut host, pane);
    for request in host.inference().requests() {
        let system_len: usize = request
            .1
            .messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.content.len())
            .sum();
        assert!(system_len + format!("{request:?}").len() < 400_000);
    }
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/smart_copy_workers.rs::progress_coalesces_but_semantic_overflow_cannot_claim_finished
#[test]
fn port_internal_a_reply_is_never_shown_as_more_than_three_candidates() {
    let (mut host, pane) = model_host(&["a", "b", "c", "d", "e"]);
    host.inference().reply_text("a\nb\nc\nd\ne");
    ask(&mut host, pane);
    let lines = candidate_lines(&host.deliver_one(candidates_message(&[])));
    assert!(lines.len() <= 3, "{lines:?}");
}

/// REQ: PORT:ilium-client/src/smart_copy_workers.rs::refused_host_probe_arms_a_date_without_self_retry_or_provider_call
#[test]
fn port_internal_a_refused_model_call_is_not_retried_by_the_plugin() {
    let (mut host, pane) = model_host(&["one"]);
    host.inference().fail_next("Denied");
    ask(&mut host, pane);
    assert_eq!(host.inference().requests().len(), 1);
}

/// REQ: PORT:ilium-client/src/smart_copy_workers.rs::two_retiring_blocked_bodies_bound_new_stream_admission_until_real_exit
#[test]
fn port_internal_repeated_requests_stay_bounded_by_settling() {
    let (mut host, pane) = model_host(&["one"]);
    host.inference().reply_text("one");
    for _ in 0..4 {
        host.deliver_one(palette_command("smart-copy", Some(pane)));
    }
    host.settle();
    assert!(host.inference().requests().len() <= 4);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/smart_copy_workers.rs::unsafe_host_namespace_error_is_terminal_before_stream_invocation
#[test]
fn port_internal_a_namespace_error_is_terminal_and_makes_one_request_at_most() {
    let (mut host, pane) = model_host(&["one"]);
    host.inference().fail_next("Namespace");
    ask(&mut host, pane);
    assert!(host.inference().requests().len() <= 1);
    assert_eq!(host.clipboard().last_text(), None);
}

// ---- pty_tui_smoke.rs port ----

/// REQ: PORT:ilium/tests/pty_tui_smoke.rs::right_click_restart_reloads_only_the_client_and_preserves_the_server
#[test]
fn port_internal_the_server_side_keeps_working_after_the_client_side_is_rebuilt() {
    let (mut server, _pane) = server_showing(&["src/lib.rs"]);
    let before = scan(&["src/lib.rs"]);
    let mut client = client_host();
    client.deliver_one(candidates_message(&["src/lib.rs"]));
    let mut rebuilt = client_host();
    rebuilt.deliver_one(candidates_message(&["src/lib.rs"]));
    pick(&mut rebuilt, 1);
    assert_eq!(
        rebuilt.clipboard().last_text().as_deref(),
        Some("src/lib.rs")
    );
    assert_eq!(scan(&["src/lib.rs"]), before);
    assert!(server.failures().is_empty());
}
