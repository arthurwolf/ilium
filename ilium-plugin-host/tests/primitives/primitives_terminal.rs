//! Terminal primitive (primitives-terminal*.md): reads, input, pointer claims, highlights, output
//! filters, history, snapshots, input lock. Requirement ids PRI-001..008 and the 21 WIT functions
//! of `ilium:primitives/terminal`.

use ilium_plugin_api::generated::primitives::terminal::{
    CellRegion, HostTerminal, OutputFilter, ScreenPart, Selection, TerminalMode,
};
use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::intents::{
    Delivery, InputPrecondition, InputStage, Intent, SendInputSpec,
};
use ilium_plugin_api::generated::types::ui::PointerModifiers;
use ilium_plugin_api::generated::types::tree::AgentActivity;
use ilium_plugin_api::ErrorCode;
use proptest::prelude::*;

use crate::common::*;

const READ_SCREEN: &[&str] = &["terminal.read-screen"];
const READ_CELLS: &[&str] = &["terminal.read-screen", "terminal.read-cells"];
const HISTORY: &[&str] = &["terminal.read-screen", "terminal.history"];
const WRITE: &[&str] = &["terminal.write", "terminal.raw-input", "terminal.input-lock"];
const FILTER: &[&str] = &["terminal.read-screen", "terminal.output-filter"];
const POINTER: &[&str] = &["terminal.read-screen", "terminal.pointer"];

fn send(pane: ilium_plugin_api::generated::types::ids::NodeId) -> SendInputSpec {
    SendInputSpec {
        pane,
        text: "ls".to_owned(),
        delivery: Delivery::Submitted,
        prompt_epoch: None,
        stages: Vec::new(),
        raw: None,
        mouse: None,
        if_current: None,
        lock_user_input: false,
        journal: false,
    }
}

fn stages(count: usize, delay_ms: u16) -> Vec<InputStage> {
    (0..count)
        .map(|index| InputStage { text: format!("s{index}"), delay_ms, delivery: Delivery::Typed })
        .collect()
}

fn filter(id: &str, regex: &str, rate: u16) -> OutputFilter {
    OutputFilter { id: id.to_owned(), regex: regex.to_owned(), rate_per_second: rate }
}

// ---------------------------------------------------------------- PRI-001

/// REQ: PRI-001, WIT:ilium:primitives/terminal.read-cells
#[test]
fn pri_001_read_cells_1000_rows_accepted() {
    let rig = rig(READ_CELLS);
    rig.kernel.set_screen(PANE, screen_of(1_200, 80));
    let rows = rig.host.terminal().read_cells(PANE, ScreenPart::Visible, 0, 1_000);
    assert_eq!(cell_rows_len(&rows), Some(1_000));
}

/// REQ: PRI-001
#[test]
fn pri_001_read_cells_999_rows_accepted() {
    let rig = rig(READ_CELLS);
    rig.kernel.set_screen(PANE, screen_of(1_200, 80));
    let rows = rig.host.terminal().read_cells(PANE, ScreenPart::Visible, 0, 999);
    assert_eq!(cell_rows_len(&rows), Some(999));
}

/// REQ: PRI-001
#[test]
fn pri_001_read_cells_1001_rows_quota_exceeded() {
    // The WIT signature returns option, so the code is observed as the refusal line in the log.
    let rig = rig(READ_CELLS);
    rig.kernel.set_screen(PANE, screen_of(1_200, 80));
    let rows = rig.host.terminal().read_cells(PANE, ScreenPart::Visible, 0, 1_001);
    assert!(rows.is_none());
    assert!(log_count(&rig.host, "quota-exceeded") >= 1, "{}", log_text(&rig.host));
}

/// REQ: PRI-001, WIT:ilium:primitives/terminal.read-screen
#[test]
fn pri_001_read_screen_without_grant_not_permitted() {
    let rig = rig(&[]);
    assert!(rig.host.terminal().read_screen(PANE, ScreenPart::Visible, 24).is_none());
    assert!(log_count(&rig.host, "not-permitted") >= 1, "{}", log_text(&rig.host));
}

/// REQ: PRI-001, WIT:ilium:primitives/terminal.read-cells
#[test]
fn pri_001_read_cells_without_grant_not_permitted() {
    let rig = rig(READ_SCREEN);
    assert!(rig.host.terminal().read_cells(PANE, ScreenPart::Visible, 0, 5).is_none());
    assert!(log_count(&rig.host, "not-permitted") >= 1, "{}", log_text(&rig.host));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    // REQ: PRI-001
    #[test]
    fn pri_001_read_cells_returns_rows_only_up_to_1000(count in 1u32..2_000) {
        let rig = rig(READ_CELLS);
        rig.kernel.set_screen(PANE, screen_of(2_000, 8));
        let rows = rig.host.terminal().read_cells(PANE, ScreenPart::Visible, 0, count);
        prop_assert_eq!(rows.is_some(), count <= 1_000);
    }
}

// ---------------------------------------------------------------- PRI-002

fn history_rig(lines: usize) -> Rig {
    let rig = rig(HISTORY);
    rig.kernel.set_screen(PANE, screen_with_history(lines, KIB));
    rig
}

fn read_history(rig: &mut Rig, max_bytes: u32) -> String {
    let request = rig.host.terminal().history_text(PANE, max_bytes).expect("request accepted");
    rig.host.pump(8);
    rig.host.terminal().take_history(request).expect("history completed")
}

/// REQ: PRI-002, WIT:ilium:primitives/terminal.history-text, WIT:ilium:primitives/terminal.take-history
#[test]
fn pri_002_history_of_exactly_4_mib_kept_whole() {
    let mut rig = history_rig(4 * KIB);
    let text = read_history(&mut rig, (8 * MIB) as u32);
    assert_eq!(text.len(), 4 * MIB);
    assert!(text.contains("hist-0."));
}

/// REQ: PRI-002
#[test]
fn pri_002_history_of_5_mib_keeps_newest_4_mib() {
    let mut rig = history_rig(5 * KIB);
    let text = read_history(&mut rig, (8 * MIB) as u32);
    assert!(text.len() <= 4 * MIB, "{} bytes", text.len());
    assert!(text.contains(&format!("hist-{}.", 5 * KIB - 1)));
    assert!(!text.contains("hist-0."));
}

/// REQ: PRI-002
#[test]
fn pri_002_history_one_line_over_4_mib_drops_oldest_line() {
    let mut rig = history_rig(4 * KIB + 1);
    let text = read_history(&mut rig, (8 * MIB) as u32);
    assert!(text.len() <= 4 * MIB);
    assert!(!text.contains("hist-0."));
    assert!(text.contains("hist-1."));
}

/// REQ: PRI-002
#[test]
fn pri_002_history_respects_a_smaller_max_bytes_newest_first() {
    let mut rig = history_rig(64);
    let text = read_history(&mut rig, (10 * KIB) as u32);
    assert!(text.len() <= 10 * KIB);
    assert!(text.contains("hist-63."));
}

/// REQ: PRI-002, WIT:ilium:primitives/terminal.history-text
#[test]
fn pri_002_history_text_without_grant_not_permitted() {
    let rig = rig(READ_SCREEN);
    let result = rig.host.terminal().history_text(PANE, 1_024);
    assert_eq!(error_of(result), Some(ErrorCode::NotPermitted));
}

/// REQ: WIT:ilium:primitives/terminal.take-history
#[test]
fn wit_terminal_take_history_unknown_request_returns_none() {
    let rig = rig(HISTORY);
    assert!(rig.host.terminal().take_history(ilium_plugin_api::generated::types::ids::RequestId(424_242)).is_none());
}

// ---------------------------------------------------------------- PRI-003

fn input_outcome(spec: SendInputSpec) -> ilium_plugin_api::generated::types::requests::Outcome {
    let mut rig = rig(WRITE);
    rig.submit(Intent::SendInput(spec))
}

/// REQ: PRI-003
#[test]
fn pri_003_16_stages_accepted() {
    let mut spec = send(PANE);
    spec.stages = stages(16, 0);
    assert!(accepted(&input_outcome(spec)));
}

/// REQ: PRI-003
#[test]
fn pri_003_17_stages_refused_invalid_argument() {
    let mut spec = send(PANE);
    spec.stages = stages(17, 0);
    assert_eq!(refusal_of(&input_outcome(spec)), Some(ErrorCode::InvalidArgument));
}

/// REQ: PRI-003
#[test]
fn pri_003_total_stage_delay_10_s_accepted() {
    let mut spec = send(PANE);
    spec.stages = stages(10, 1_000);
    assert!(accepted(&input_outcome(spec)));
}

/// REQ: PRI-003
#[test]
fn pri_003_total_stage_delay_10_001_ms_refused_invalid_argument() {
    let mut spec = send(PANE);
    spec.stages = stages(2, 5_001);
    spec.stages[0].delay_ms = 5_000;
    assert_eq!(refusal_of(&input_outcome(spec)), Some(ErrorCode::InvalidArgument));
}

/// REQ: PRI-003
#[test]
fn pri_003_raw_64_kib_accepted() {
    let mut spec = send(PANE);
    spec.raw = Some(vec![0x41; 64 * KIB]);
    assert!(accepted(&input_outcome(spec)));
}

/// REQ: PRI-003
#[test]
fn pri_003_raw_64_kib_plus_one_refused_invalid_argument() {
    let mut spec = send(PANE);
    spec.raw = Some(vec![0x41; 64 * KIB + 1]);
    assert_eq!(refusal_of(&input_outcome(spec)), Some(ErrorCode::InvalidArgument));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    // REQ: PRI-003
    #[test]
    fn pri_003_stage_count_accepted_up_to_16(count in 0usize..40) {
        let mut spec = send(PANE);
        spec.stages = stages(count, 0);
        prop_assert_eq!(accepted(&input_outcome(spec)), count <= 16);
    }
}

// ---------------------------------------------------------------- PRI-004

/// REQ: PRI-004
#[test]
fn pri_004_missing_grant_beats_bad_stages_not_permitted() {
    let mut rig = rig(&[]);
    let mut spec = send(PANE);
    spec.stages = stages(17, 0);
    let outcome = rig.submit(Intent::SendInput(spec));
    assert_eq!(refusal_of(&outcome), Some(ErrorCode::NotPermitted));
}

/// REQ: PRI-004
#[test]
fn pri_004_bad_stages_beat_stale_precondition_invalid_argument() {
    let mut rig = rig(WRITE);
    let mut spec = send(PANE);
    spec.stages = stages(17, 0);
    spec.if_current = Some(InputPrecondition {
        activity: Some(AgentActivity::Working),
        screen_revision: Some(0),
        foreground: None,
    });
    let outcome = rig.submit(Intent::SendInput(spec));
    assert_eq!(refusal_of(&outcome), Some(ErrorCode::InvalidArgument));
}

/// REQ: PRI-004, WIT:ilium:primitives/terminal.lock-input
#[test]
fn pri_004_stale_precondition_on_locked_pane_refused_conflict_and_lock_unchanged() {
    let mut rig = rig(WRITE);
    rig.host.terminal().lock_input(PANE, 1_500).expect("first lock");
    let mut spec = send(PANE);
    spec.lock_user_input = true;
    spec.if_current = Some(InputPrecondition {
        activity: None,
        screen_revision: Some(u64::MAX),
        foreground: None,
    });
    let outcome = rig.submit(Intent::SendInput(spec));
    assert_eq!(refusal_of(&outcome), Some(ErrorCode::Conflict));
    // The original lock is still the only one: a peer cannot take it, our own release works.
    let peer = rig.host.peer("peer.plugin", WRITE);
    assert_eq!(error_of(peer.terminal().lock_input(PANE, 500)), Some(ErrorCode::Conflict));
    rig.host.terminal().unlock_input(PANE);
    assert!(peer.terminal().lock_input(PANE, 500).is_ok());
}

/// REQ: PRI-004
#[test]
fn pri_004_matching_precondition_is_accepted() {
    let mut rig = rig(WRITE);
    let revision = rig.host.terminal().read_screen(PANE, ScreenPart::Visible, 24).map(|s| s.revision);
    let mut spec = send(PANE);
    spec.if_current = Some(InputPrecondition {
        activity: None,
        screen_revision: Some(revision.unwrap_or(0)),
        foreground: None,
    });
    assert!(accepted(&rig.submit(Intent::SendInput(spec))));
}

// ---------------------------------------------------------------- PRI-005

/// REQ: PRI-005, WIT:ilium:primitives/terminal.lock-input
#[test]
fn pri_005_lock_of_2000_ms_accepted() {
    let rig = rig(WRITE);
    assert!(rig.host.terminal().lock_input(PANE, 2_000).is_ok());
}

/// REQ: PRI-005
#[test]
fn pri_005_lock_of_2001_ms_invalid_argument() {
    let rig = rig(WRITE);
    assert_eq!(error_of(rig.host.terminal().lock_input(PANE, 2_001)), Some(ErrorCode::InvalidArgument));
}

/// REQ: PRI-005
#[test]
fn pri_005_lock_of_1999_ms_accepted() {
    let rig = rig(WRITE);
    assert!(rig.host.terminal().lock_input(PANE, 1_999).is_ok());
}

/// REQ: PRI-005
#[test]
fn pri_005_second_lock_on_a_locked_pane_conflict() {
    let rig = rig(WRITE);
    rig.host.terminal().lock_input(PANE, 1_000).expect("first lock");
    assert_eq!(error_of(rig.host.terminal().lock_input(PANE, 1_000)), Some(ErrorCode::Conflict));
}

/// REQ: PRI-005, WIT:ilium:primitives/terminal.unlock-input
#[test]
fn pri_005_unlock_frees_the_pane_for_a_new_lock() {
    let rig = rig(WRITE);
    rig.host.terminal().lock_input(PANE, 1_000).expect("first lock");
    rig.host.terminal().unlock_input(PANE);
    assert!(rig.host.terminal().lock_input(PANE, 1_000).is_ok());
}

/// REQ: PRI-005
#[test]
fn pri_005_lock_expires_after_its_time() {
    let mut rig = rig(WRITE);
    rig.host.terminal().lock_input(PANE, 2_000).expect("first lock");
    advance_ms(&mut rig.host, 2_001);
    assert!(rig.host.terminal().lock_input(PANE, 100).is_ok());
}

/// REQ: PRI-005
#[test]
fn pri_005_lock_still_held_just_before_expiry_conflict() {
    let mut rig = rig(WRITE);
    rig.host.terminal().lock_input(PANE, 2_000).expect("first lock");
    advance_ms(&mut rig.host, 1_999);
    assert_eq!(error_of(rig.host.terminal().lock_input(PANE, 100)), Some(ErrorCode::Conflict));
}

/// REQ: PRI-005, WIT:ilium:primitives/terminal.lock-input
#[test]
fn pri_005_lock_without_grant_not_permitted() {
    let rig = rig(READ_SCREEN);
    assert_eq!(error_of(rig.host.terminal().lock_input(PANE, 100)), Some(ErrorCode::NotPermitted));
}

/// REQ: PRI-005, WIT:ilium:primitives/terminal.lock-input
#[test]
fn pri_005_lock_on_a_missing_pane_not_found() {
    let rig = rig(WRITE);
    assert_eq!(error_of(rig.host.terminal().lock_input(MISSING_PANE, 100)), Some(ErrorCode::NotFound));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    // REQ: PRI-005
    #[test]
    fn pri_005_lock_duration_valid_up_to_2000_ms(millis in 1u16..4_000) {
        let rig = rig(WRITE);
        prop_assert_eq!(rig.host.terminal().lock_input(PANE, millis).is_ok(), millis <= 2_000);
    }
}

// ---------------------------------------------------------------- PRI-006

fn flood(rate: u16, matches: usize) -> (Rig, usize) {
    let mut rig = rig(FILTER);
    rig.host.terminal().add_output_filter(PANE, filter("errors", "^ERR", rate)).expect("filter added");
    rig.kernel.emit_lines(PANE, (0..matches).map(|index| format!("ERR {index}")).collect());
    rig.host.pump(16);
    let delivered = rig
        .seen
        .lock()
        .expect("seen lock")
        .iter()
        .filter(|event| matches!(event, Event::OutputMatched(_)))
        .count();
    (rig, delivered)
}

/// REQ: PRI-006, WIT:ilium:primitives/terminal.add-output-filter
#[test]
fn pri_006_500_matches_at_rate_10_deliver_10() {
    let (_rig, delivered) = flood(10, 500);
    assert_eq!(delivered, 10);
}

/// REQ: PRI-006
#[test]
fn pri_006_matches_at_exactly_the_rate_all_delivered() {
    let (_rig, delivered) = flood(10, 10);
    assert_eq!(delivered, 10);
}

/// REQ: PRI-006
#[test]
fn pri_006_one_match_over_the_rate_is_dropped() {
    let (_rig, delivered) = flood(10, 11);
    assert_eq!(delivered, 10);
}

/// REQ: PRI-006
#[test]
fn pri_006_next_batch_starts_with_events_dropped() {
    let (mut rig, _) = flood(10, 500);
    rig.host.deliver(one_event());
    rig.host.pump(8);
    let seen = rig.seen.lock().expect("seen lock");
    let dropped = seen.iter().find_map(|event| match event {
        Event::EventsDropped(counts) => Some(counts.clone()),
        _ => None,
    });
    let counts = dropped.expect("an events-dropped record is delivered");
    assert!(counts.iter().any(|entry| entry.event == "output-matched" && entry.count == 490));
}

/// REQ: PRI-006
#[test]
fn pri_006_rate_budget_refills_after_a_second() {
    let (mut rig, _) = flood(10, 50);
    advance_ms(&mut rig.host, 1_000);
    rig.kernel.emit_lines(PANE, (0..5).map(|index| format!("ERR again {index}")).collect());
    rig.host.pump(16);
    let delivered = rig
        .seen
        .lock()
        .expect("seen lock")
        .iter()
        .filter(|event| matches!(event, Event::OutputMatched(m) if m.line.contains("again")))
        .count();
    assert_eq!(delivered, 5);
}

/// REQ: PRI-006
#[test]
fn pri_006_rate_zero_invalid_argument() {
    let rig = rig(FILTER);
    let result = rig.host.terminal().add_output_filter(PANE, filter("f", "x", 0));
    assert_eq!(error_of(result), Some(ErrorCode::InvalidArgument));
}

/// REQ: PRI-006
#[test]
fn pri_006_rate_100_accepted() {
    let rig = rig(FILTER);
    assert!(rig.host.terminal().add_output_filter(PANE, filter("f", "x", 100)).is_ok());
}

/// REQ: PRI-006
#[test]
fn pri_006_rate_101_invalid_argument() {
    let rig = rig(FILTER);
    let result = rig.host.terminal().add_output_filter(PANE, filter("f", "x", 101));
    assert_eq!(error_of(result), Some(ErrorCode::InvalidArgument));
}

/// REQ: WIT:ilium:primitives/terminal.add-output-filter
#[test]
fn wit_terminal_add_output_filter_empty_id_invalid_argument() {
    let rig = rig(FILTER);
    let result = rig.host.terminal().add_output_filter(PANE, filter("", "x", 5));
    assert_eq!(error_of(result), Some(ErrorCode::InvalidArgument));
}

/// REQ: WIT:ilium:primitives/terminal.add-output-filter
#[test]
fn wit_terminal_add_output_filter_bad_regex_invalid_argument() {
    let rig = rig(FILTER);
    let result = rig.host.terminal().add_output_filter(PANE, filter("f", "(unclosed", 5));
    assert_eq!(error_of(result), Some(ErrorCode::InvalidArgument));
}

/// REQ: WIT:ilium:primitives/terminal.add-output-filter
#[test]
fn wit_terminal_add_output_filter_pattern_1_kib_accepted() {
    let rig = rig(FILTER);
    let result = rig.host.terminal().add_output_filter(PANE, filter("f", &"a".repeat(KIB), 5));
    assert!(result.is_ok());
}

/// REQ: WIT:ilium:primitives/terminal.add-output-filter
#[test]
fn wit_terminal_add_output_filter_pattern_over_1_kib_invalid_argument() {
    let rig = rig(FILTER);
    let result = rig.host.terminal().add_output_filter(PANE, filter("f", &"a".repeat(KIB + 1), 5));
    assert_eq!(error_of(result), Some(ErrorCode::InvalidArgument));
}

/// REQ: WIT:ilium:primitives/terminal.add-output-filter
#[test]
fn wit_terminal_add_output_filter_without_grant_not_permitted() {
    let rig = rig(READ_SCREEN);
    let result = rig.host.terminal().add_output_filter(PANE, filter("f", "x", 5));
    assert_eq!(error_of(result), Some(ErrorCode::NotPermitted));
}

/// REQ: WIT:ilium:primitives/terminal.add-output-filter
#[test]
fn wit_terminal_add_output_filter_session_limit_512_quota_exceeded() {
    let rig = rig(FILTER);
    let mut last = Ok(());
    for index in 0..513 {
        last = rig.host.terminal().add_output_filter(PANE, filter(&format!("f{index}"), "x", 5));
        if last.is_err() {
            break;
        }
    }
    assert_eq!(error_of(last), Some(ErrorCode::QuotaExceeded));
}

/// REQ: WIT:ilium:primitives/terminal.remove-output-filter
#[test]
fn wit_terminal_remove_output_filter_stops_delivery() {
    let mut rig = rig(FILTER);
    rig.host.terminal().add_output_filter(PANE, filter("errors", "^ERR", 50)).expect("added");
    rig.host.terminal().remove_output_filter(PANE, "errors".to_owned());
    rig.kernel.emit_lines(PANE, vec!["ERR one".to_owned()]);
    rig.host.pump(8);
    let delivered = rig.seen_names().iter().filter(|name| *name == "output-matched").count();
    assert_eq!(delivered, 0);
}

/// REQ: WIT:ilium:primitives/terminal.remove-output-filter
#[test]
fn wit_terminal_remove_output_filter_unknown_id_is_silent() {
    let rig = rig(FILTER);
    rig.host.terminal().remove_output_filter(PANE, "never-added".to_owned());
    assert!(log_count(&rig.host, "internal") == 0);
}

// ---------------------------------------------------------------- PRI-007

/// REQ: PRI-007, WIT:ilium:primitives/terminal.claim-pointer
#[test]
fn pri_007_second_plugin_claim_on_same_pane_conflict() {
    let rig = rig(POINTER);
    let peer = rig.host.peer("peer.plugin", POINTER);
    rig.host.terminal().claim_pointer(PANE).expect("first claim");
    assert_eq!(error_of(peer.terminal().claim_pointer(PANE)), Some(ErrorCode::Conflict));
}

/// REQ: PRI-007
#[test]
fn pri_007_same_plugin_claiming_twice_is_not_a_conflict() {
    let rig = rig(POINTER);
    rig.host.terminal().claim_pointer(PANE).expect("first claim");
    assert!(rig.host.terminal().claim_pointer(PANE).is_ok());
}

/// REQ: PRI-007, WIT:ilium:primitives/terminal.release-pointer
#[test]
fn pri_007_release_lets_the_other_plugin_claim() {
    let rig = rig(POINTER);
    let peer = rig.host.peer("peer.plugin", POINTER);
    rig.host.terminal().claim_pointer(PANE).expect("first claim");
    rig.host.terminal().release_pointer(PANE);
    assert!(peer.terminal().claim_pointer(PANE).is_ok());
}

/// REQ: PRI-007, WIT:ilium:primitives/terminal.claim-pointer-gesture
#[test]
fn pri_007_overlapping_gesture_claim_conflict() {
    let rig = rig(POINTER);
    let peer = rig.host.peer("peer.plugin", POINTER);
    rig.host.terminal().claim_pointer_gesture(PANE, PointerModifiers::CONTROL).expect("first");
    let second = peer.terminal().claim_pointer_gesture(PANE, PointerModifiers::CONTROL);
    assert_eq!(error_of(second), Some(ErrorCode::Conflict));
}

/// REQ: PRI-007
#[test]
fn pri_007_different_modifier_gestures_do_not_conflict() {
    let rig = rig(POINTER);
    let peer = rig.host.peer("peer.plugin", POINTER);
    rig.host.terminal().claim_pointer_gesture(PANE, PointerModifiers::CONTROL).expect("first");
    assert!(peer.terminal().claim_pointer_gesture(PANE, PointerModifiers::ALT).is_ok());
}

/// REQ: PRI-007, WIT:ilium:primitives/terminal.claim-pointer-gesture
#[test]
fn pri_007_gesture_with_empty_modifiers_invalid_argument() {
    let rig = rig(POINTER);
    let result = rig.host.terminal().claim_pointer_gesture(PANE, PointerModifiers(0));
    assert_eq!(error_of(result), Some(ErrorCode::InvalidArgument));
}

/// REQ: PRI-007, WIT:ilium:primitives/terminal.claim-pointer
#[test]
fn pri_007_claim_on_a_missing_pane_not_found() {
    let rig = rig(POINTER);
    assert_eq!(error_of(rig.host.terminal().claim_pointer(MISSING_PANE)), Some(ErrorCode::NotFound));
}

/// REQ: PRI-007, WIT:ilium:primitives/terminal.claim-pointer
#[test]
fn pri_007_claim_on_an_editor_pane_unsupported() {
    let rig = rig(POINTER);
    assert_eq!(error_of(rig.host.terminal().claim_pointer(EDITOR_PANE)), Some(ErrorCode::Unsupported));
}

/// REQ: PRI-007, WIT:ilium:primitives/terminal.claim-pointer
#[test]
fn pri_007_claim_without_grant_not_permitted() {
    let rig = rig(READ_SCREEN);
    assert_eq!(error_of(rig.host.terminal().claim_pointer(PANE)), Some(ErrorCode::NotPermitted));
}

// ---------------------------------------------------------------- PRI-008

fn screen_changes(per_second: usize, seconds: usize) -> usize {
    let mut rig = rig(READ_SCREEN);
    for _second in 0..seconds {
        for _change in 0..per_second {
            rig.kernel.change_screen(PANE);
            rig.host.pump(2);
            advance_ms(&mut rig.host, (1_000 / per_second) as u64);
        }
    }
    rig.seen_names().iter().filter(|name| *name == "screen-changed").count()
}

/// REQ: PRI-008
#[test]
fn pri_008_100_changes_in_a_second_deliver_at_most_10() {
    assert!(screen_changes(100, 1) <= 10);
}

/// REQ: PRI-008
#[test]
fn pri_008_100_changes_in_a_second_deliver_at_least_one() {
    assert!(screen_changes(100, 1) >= 1);
}

/// REQ: PRI-008
#[test]
fn pri_008_10_changes_in_a_second_all_delivered() {
    assert_eq!(screen_changes(10, 1), 10);
}

/// REQ: PRI-008
#[test]
fn pri_008_three_seconds_of_flood_deliver_at_most_30() {
    assert!(screen_changes(100, 3) <= 30);
}

// ---------------------------------------------------------------- other WIT functions

/// REQ: WIT:ilium:primitives/terminal.read-screen
#[test]
fn wit_terminal_read_screen_visible_returns_the_rows() {
    let rig = rig(READ_SCREEN);
    let screen = rig.host.terminal().read_screen(PANE, ScreenPart::Visible, 24).expect("screen");
    assert_eq!(screen.rows.len(), 24);
    assert!(screen.rows[0].starts_with("row-0"));
}

/// REQ: WIT:ilium:primitives/terminal.read-screen
#[test]
fn wit_terminal_read_screen_max_rows_limits_the_result() {
    let rig = rig(READ_SCREEN);
    let screen = rig.host.terminal().read_screen(PANE, ScreenPart::Visible, 5).expect("screen");
    assert_eq!(screen.rows.len(), 5);
}

/// REQ: WIT:ilium:primitives/terminal.read-screen
#[test]
fn wit_terminal_read_screen_scrollback_returns_history() {
    let rig = rig(HISTORY);
    rig.kernel.set_screen(PANE, screen_with_history(100, 64));
    let screen = rig.host.terminal().read_screen(PANE, ScreenPart::Scrollback, 100).expect("screen");
    assert_eq!(screen.rows.len(), 100);
}

/// REQ: WIT:ilium:primitives/terminal.read-screen
#[test]
fn wit_terminal_read_screen_missing_pane_returns_none() {
    let rig = rig(READ_SCREEN);
    assert!(rig.host.terminal().read_screen(MISSING_PANE, ScreenPart::Visible, 24).is_none());
}

/// REQ: WIT:ilium:primitives/terminal.selection
#[test]
fn wit_terminal_selection_returns_the_selected_text() {
    let rig = rig(READ_SCREEN);
    rig.kernel.set_selection(
        PANE,
        Some(Selection { first_row: 1, first_column: 0, last_row: 1, last_column: 4, text: "row-1".into() }),
    );
    assert_eq!(rig.host.terminal().selection(PANE).map(|s| s.text), Some("row-1".to_owned()));
}

/// REQ: WIT:ilium:primitives/terminal.selection
#[test]
fn wit_terminal_selection_without_selection_returns_none() {
    let rig = rig(READ_SCREEN);
    assert!(rig.host.terminal().selection(PANE).is_none());
}

/// REQ: WIT:ilium:primitives/terminal.selection
#[test]
fn wit_terminal_selection_without_grant_returns_none() {
    let rig = rig(&[]);
    rig.kernel.set_selection(PANE, Some(Selection::default()));
    assert!(rig.host.terminal().selection(PANE).is_none());
}

fn region(first: u32, last: u32) -> CellRegion {
    CellRegion { first_row: first, first_column: 0, last_row: last, last_column: 5, ..CellRegion::default() }
}

const HIGHLIGHT: &[&str] = &["terminal.read-screen", "terminal.pointer", "ui.chrome"];

/// REQ: WIT:ilium:primitives/terminal.set-highlights
#[test]
fn wit_terminal_set_highlights_accepts_a_layer() {
    let rig = rig(HIGHLIGHT);
    assert!(rig.host.terminal().set_highlights(PANE, "search".into(), vec![region(0, 1)]).is_ok());
}

/// REQ: WIT:ilium:primitives/terminal.set-highlights
#[test]
fn wit_terminal_set_highlights_4096_regions_accepted() {
    let rig = rig(HIGHLIGHT);
    let regions = (0..4_096).map(|_| region(0, 1)).collect();
    assert!(rig.host.terminal().set_highlights(PANE, "search".into(), regions).is_ok());
}

/// REQ: WIT:ilium:primitives/terminal.set-highlights
#[test]
fn wit_terminal_set_highlights_4097_regions_invalid_argument() {
    let rig = rig(HIGHLIGHT);
    let regions = (0..4_097).map(|_| region(0, 1)).collect();
    let result = rig.host.terminal().set_highlights(PANE, "search".into(), regions);
    assert_eq!(error_of(result), Some(ErrorCode::InvalidArgument));
}

/// REQ: WIT:ilium:primitives/terminal.set-highlights
#[test]
fn wit_terminal_set_highlights_bad_layer_name_invalid_argument() {
    let rig = rig(HIGHLIGHT);
    for layer in ["", "Upper", "has space", &"a".repeat(33)] {
        let result = rig.host.terminal().set_highlights(PANE, layer.to_owned(), vec![region(0, 1)]);
        assert_eq!(error_of(result), Some(ErrorCode::InvalidArgument), "layer {layer:?}");
    }
}

/// REQ: WIT:ilium:primitives/terminal.set-highlights
#[test]
fn wit_terminal_set_highlights_layer_name_32_chars_accepted() {
    let rig = rig(HIGHLIGHT);
    let result = rig.host.terminal().set_highlights(PANE, "a".repeat(32), vec![region(0, 1)]);
    assert!(result.is_ok());
}

/// REQ: WIT:ilium:primitives/terminal.set-highlights
#[test]
fn wit_terminal_set_highlights_first_after_last_invalid_argument() {
    let rig = rig(HIGHLIGHT);
    let result = rig.host.terminal().set_highlights(PANE, "x".into(), vec![region(5, 2)]);
    assert_eq!(error_of(result), Some(ErrorCode::InvalidArgument));
}

/// REQ: WIT:ilium:primitives/terminal.set-highlights
#[test]
fn wit_terminal_set_highlights_label_over_128_chars_invalid_argument() {
    let rig = rig(HIGHLIGHT);
    let mut bad = region(0, 1);
    bad.label = Some("l".repeat(129));
    let result = rig.host.terminal().set_highlights(PANE, "x".into(), vec![bad]);
    assert_eq!(error_of(result), Some(ErrorCode::InvalidArgument));
}

/// REQ: WIT:ilium:primitives/terminal.set-highlights
#[test]
fn wit_terminal_set_highlights_17th_layer_quota_exceeded() {
    let rig = rig(HIGHLIGHT);
    for layer in 0..16 {
        rig.host.terminal().set_highlights(PANE, format!("layer-{layer}"), vec![region(0, 0)]).expect("layer");
    }
    let result = rig.host.terminal().set_highlights(PANE, "layer-16".into(), vec![region(0, 0)]);
    assert_eq!(error_of(result), Some(ErrorCode::QuotaExceeded));
}

/// REQ: WIT:ilium:primitives/terminal.set-highlights
#[test]
fn wit_terminal_set_highlights_on_an_editor_pane_unsupported() {
    let rig = rig(HIGHLIGHT);
    let result = rig.host.terminal().set_highlights(EDITOR_PANE, "x".into(), vec![region(0, 0)]);
    assert_eq!(error_of(result), Some(ErrorCode::Unsupported));
}

/// REQ: WIT:ilium:primitives/terminal.clear-highlights
#[test]
fn wit_terminal_clear_highlights_frees_the_layer_slot() {
    let rig = rig(HIGHLIGHT);
    for layer in 0..16 {
        rig.host.terminal().set_highlights(PANE, format!("layer-{layer}"), vec![region(0, 0)]).expect("layer");
    }
    rig.host.terminal().clear_highlights(PANE, "layer-0".into());
    assert!(rig.host.terminal().set_highlights(PANE, "layer-16".into(), vec![region(0, 0)]).is_ok());
}

/// REQ: WIT:ilium:primitives/terminal.clear-highlights
#[test]
fn wit_terminal_clear_highlights_unknown_layer_is_silent() {
    let rig = rig(HIGHLIGHT);
    rig.host.terminal().clear_highlights(PANE, "never-set".into());
    assert_eq!(log_count(&rig.host, "internal"), 0);
}

/// REQ: WIT:ilium:primitives/terminal.read-cells
#[test]
fn wit_terminal_read_cells_returns_first_row_and_width() {
    let rig = rig(READ_CELLS);
    let rows = rig.host.terminal().read_cells(PANE, ScreenPart::Visible, 3, 2).expect("cells");
    assert_eq!(rows.first_row, 3);
    assert_eq!(rows.columns, 80);
    assert_eq!(rows.rows.len(), 2);
}

/// REQ: WIT:ilium:primitives/terminal.read-cells
#[test]
fn wit_terminal_read_cells_restored_pane_pane_not_ready() {
    let rig = rig(READ_CELLS);
    assert!(rig.host.terminal().read_cells(RESTORED_PANE, ScreenPart::Visible, 0, 1).is_none());
    assert!(log_count(&rig.host, "pane-not-ready") >= 1, "{}", log_text(&rig.host));
}

/// REQ: WIT:ilium:primitives/terminal.read-cells
#[test]
fn wit_terminal_read_cells_missing_pane_not_found() {
    let rig = rig(READ_CELLS);
    assert!(rig.host.terminal().read_cells(MISSING_PANE, ScreenPart::Visible, 0, 1).is_none());
}

/// REQ: WIT:ilium:primitives/terminal.modes
#[test]
fn wit_terminal_modes_reports_the_pane_modes() {
    let rig = rig(READ_SCREEN);
    rig.kernel.set_modes(PANE, TerminalMode::ALTERNATE_SCREEN | TerminalMode::BRACKETED_PASTE);
    let modes = rig.host.terminal().modes(PANE).expect("modes");
    assert!(modes.contains(TerminalMode::ALTERNATE_SCREEN));
    assert!(modes.contains(TerminalMode::BRACKETED_PASTE));
    assert!(!modes.contains(TerminalMode::MOUSE_REPORTING));
}

/// REQ: WIT:ilium:primitives/terminal.modes
#[test]
fn wit_terminal_modes_missing_pane_returns_none() {
    let rig = rig(READ_SCREEN);
    assert!(rig.host.terminal().modes(MISSING_PANE).is_none());
}

/// REQ: WIT:ilium:primitives/terminal.foreground-process
#[test]
fn wit_terminal_foreground_process_reports_name_and_command_line() {
    let rig = rig(READ_SCREEN);
    rig.kernel.set_foreground(PANE, "vim", &["vim", "notes.md"]);
    let process = rig.host.terminal().foreground_process(PANE).expect("process");
    assert_eq!(process.name, "vim");
    assert_eq!(process.command_line, vec!["vim".to_owned(), "notes.md".to_owned()]);
}

/// REQ: WIT:ilium:primitives/terminal.foreground-process
#[test]
fn wit_terminal_foreground_process_restored_pane_returns_none() {
    let rig = rig(READ_SCREEN);
    assert!(rig.host.terminal().foreground_process(RESTORED_PANE).is_none());
}

/// REQ: WIT:ilium:primitives/terminal.freeze-snapshot, WIT:ilium:primitives/terminal.read-snapshot-cells
#[test]
fn wit_terminal_snapshot_keeps_the_screen_it_froze() {
    let rig = rig(READ_CELLS);
    let snapshot = rig.host.terminal().freeze_snapshot(PANE).expect("snapshot");
    rig.kernel.set_screen(PANE, screen_of(24, 80));
    rig.kernel.change_screen(PANE);
    let rows = rig.host.terminal().read_snapshot_cells(snapshot, 0, 3).expect("snapshot cells");
    assert_eq!(rows.rows.len(), 3);
    assert_eq!(rows.rows[0][0].text, "r");
}

/// REQ: WIT:ilium:primitives/terminal.freeze-snapshot
#[test]
fn wit_terminal_freeze_snapshot_4_live_snapshots_accepted() {
    let rig = rig(READ_CELLS);
    for _ in 0..4 {
        rig.host.terminal().freeze_snapshot(PANE).expect("snapshot");
    }
}

/// REQ: WIT:ilium:primitives/terminal.freeze-snapshot
#[test]
fn wit_terminal_freeze_snapshot_5th_snapshot_quota_exceeded() {
    let rig = rig(READ_CELLS);
    for _ in 0..4 {
        rig.host.terminal().freeze_snapshot(PANE).expect("snapshot");
    }
    assert_eq!(error_of(rig.host.terminal().freeze_snapshot(PANE)), Some(ErrorCode::QuotaExceeded));
}

/// REQ: WIT:ilium:primitives/terminal.freeze-snapshot
#[test]
fn wit_terminal_freeze_snapshot_restored_pane_pane_not_ready() {
    let rig = rig(READ_CELLS);
    assert_eq!(error_of(rig.host.terminal().freeze_snapshot(RESTORED_PANE)), Some(ErrorCode::PaneNotReady));
}

/// REQ: WIT:ilium:primitives/terminal.freeze-snapshot
#[test]
fn wit_terminal_freeze_snapshot_missing_pane_not_found() {
    let rig = rig(READ_CELLS);
    assert_eq!(error_of(rig.host.terminal().freeze_snapshot(MISSING_PANE)), Some(ErrorCode::NotFound));
}

/// REQ: WIT:ilium:primitives/terminal.drop-snapshot
#[test]
fn wit_terminal_drop_snapshot_frees_a_slot_and_invalidates_the_id() {
    let rig = rig(READ_CELLS);
    let snapshots: Vec<u64> =
        (0..4).map(|_| rig.host.terminal().freeze_snapshot(PANE).expect("snapshot")).collect();
    rig.host.terminal().drop_snapshot(snapshots[0]);
    assert!(rig.host.terminal().read_snapshot_cells(snapshots[0], 0, 1).is_none());
    assert!(rig.host.terminal().freeze_snapshot(PANE).is_ok());
}

/// REQ: WIT:ilium:primitives/terminal.read-snapshot-cells
#[test]
fn wit_terminal_read_snapshot_cells_unknown_snapshot_not_found() {
    let rig = rig(READ_CELLS);
    assert!(rig.host.terminal().read_snapshot_cells(777, 0, 1).is_none());
}

/// REQ: WIT:ilium:primitives/terminal.read-snapshot-cells
#[test]
fn wit_terminal_read_snapshot_cells_over_1000_rows_quota_exceeded() {
    let rig = rig(READ_CELLS);
    rig.kernel.set_screen(PANE, screen_of(1_200, 8));
    let snapshot = rig.host.terminal().freeze_snapshot(PANE).expect("snapshot");
    assert!(rig.host.terminal().read_snapshot_cells(snapshot, 0, 1_001).is_none());
}

/// REQ: WIT:ilium:primitives/terminal.search-history, WIT:ilium:primitives/terminal.take-matches
#[test]
fn wit_terminal_search_history_returns_the_matches() {
    let mut rig = rig(HISTORY);
    rig.kernel.set_screen(PANE, screen_with_history(20, 64));
    let request = rig.host.terminal().search_history(PANE, "hist-1[0-9]".into(), 100).expect("request");
    rig.host.pump(8);
    let matches = rig.host.terminal().take_matches(request).expect("matches");
    assert_eq!(matches.len(), 10);
}

/// REQ: WIT:ilium:primitives/terminal.search-history
#[test]
fn wit_terminal_search_history_max_matches_limits_the_result() {
    let mut rig = rig(HISTORY);
    rig.kernel.set_screen(PANE, screen_with_history(20, 64));
    let request = rig.host.terminal().search_history(PANE, "hist".into(), 3).expect("request");
    rig.host.pump(8);
    assert_eq!(rig.host.terminal().take_matches(request).expect("matches").len(), 3);
}

/// REQ: WIT:ilium:primitives/terminal.search-history
#[test]
fn wit_terminal_search_history_bad_regex_invalid_argument() {
    let rig = rig(HISTORY);
    let result = rig.host.terminal().search_history(PANE, "(oops".into(), 10);
    assert_eq!(error_of(result), Some(ErrorCode::InvalidArgument));
}

/// REQ: WIT:ilium:primitives/terminal.search-history
#[test]
fn wit_terminal_search_history_regex_over_1_kib_invalid_argument() {
    let rig = rig(HISTORY);
    let result = rig.host.terminal().search_history(PANE, "a".repeat(KIB + 1), 10);
    assert_eq!(error_of(result), Some(ErrorCode::InvalidArgument));
}

/// REQ: WIT:ilium:primitives/terminal.search-history
#[test]
fn wit_terminal_search_history_regex_exactly_1_kib_accepted() {
    let rig = rig(HISTORY);
    assert!(rig.host.terminal().search_history(PANE, "a".repeat(KIB), 10).is_ok());
}

/// REQ: WIT:ilium:primitives/terminal.take-matches
#[test]
fn wit_terminal_take_matches_second_take_returns_none() {
    let mut rig = rig(HISTORY);
    let request = rig.host.terminal().search_history(PANE, "x".into(), 10).expect("request");
    rig.host.pump(8);
    assert!(rig.host.terminal().take_matches(request).is_some());
    assert!(rig.host.terminal().take_matches(request).is_none());
}
