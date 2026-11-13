//! Twins of the query-state tests of ilium-client/src/search_ui.rs and the geometry rules
//! that moved with them, run against the plugin's `results` module.

use ilium_plugin_core_search::matching::{Location, ObjectKind, SearchResult};
use ilium_plugin_core_search::results::{
    group_by_kind, result_at_row, result_content_width, result_metadata, visible_result_rows,
    ContinuationAction, SearchState, MAX_PENDING_CONTINUATIONS, RESULT_HEIGHT,
    SEARCH_DEBOUNCE_MILLIS,
};

fn hit(kind: ObjectKind) -> SearchResult {
    SearchResult {
        pane_id: 1,
        kind,
        object_name: "n".to_string(),
        automatic_title: None,
        path: None,
        last_command: None,
        before: String::new(),
        matched: "m".to_string(),
        after: String::new(),
        location: Location::Board,
    }
}

/// REQ: PORT:ilium-client/src/search_ui.rs::search_state_waits_for_a_full_second_of_quiet_before_claiming_work
#[test]
fn port_search_state_waits_for_a_full_second_of_quiet_before_claiming_work() {
    let start = 5_000;
    let mut state = SearchState::new();
    state.query = "needle".to_string();
    state.note_query_changed(start);
    assert!(state
        .take_due_search(start + SEARCH_DEBOUNCE_MILLIS - 1)
        .is_none());
    assert_eq!(
        state.take_due_search(start + SEARCH_DEBOUNCE_MILLIS),
        Some((1, "needle".to_string()))
    );
    assert!(state.is_searching());
}

/// REQ: PORT:ilium-client/src/search_ui.rs::stale_worker_completion_cannot_replace_a_newer_query
#[test]
fn port_stale_worker_completion_cannot_replace_a_newer_query() {
    let start = 0;
    let mut state = SearchState::new();
    state.query = "first".to_string();
    state.note_query_changed(start);
    let (first_revision, _) = state
        .take_due_search(start + SEARCH_DEBOUNCE_MILLIS)
        .expect("first request");
    state.query = "second".to_string();
    state.note_query_changed(start + SEARCH_DEBOUNCE_MILLIS);
    assert!(!state.complete_search(first_revision, Vec::new()));
    assert!(state.results.is_empty());
    assert_eq!(
        state.take_due_search(start + 2 * SEARCH_DEBOUNCE_MILLIS),
        Some((2, "second".to_string()))
    );
}

/// REQ: PORT:ilium-client/src/app.rs::workspace_search_debounce_wakes_at_its_exact_deadline
#[test]
fn port_the_state_reports_the_exact_instant_a_quiet_query_becomes_due() {
    let mut state = SearchState::new();
    assert_eq!(state.next_due_search_at(), None);
    state.query = "q".to_string();
    state.note_query_changed(40);
    assert_eq!(
        state.next_due_search_at(),
        Some(40 + SEARCH_DEBOUNCE_MILLIS)
    );
    state.take_due_search(40 + SEARCH_DEBOUNCE_MILLIS);
    assert_eq!(state.next_due_search_at(), None);
}

#[test]
fn a_blank_query_is_never_claimed() {
    let mut state = SearchState::new();
    state.query = "   ".to_string();
    state.note_query_changed(0);
    assert!(state.take_due_search(10_000).is_none());
    assert!(!state.is_preparing());
}

#[test]
fn a_failed_launch_is_retryable_after_it_is_cancelled() {
    let mut state = SearchState::new();
    state.query = "x".to_string();
    state.note_query_changed(0);
    let (revision, _) = state.take_due_search(1000).expect("due");
    assert!(state.take_due_search(5000).is_none());
    state.cancel_in_flight_search(revision);
    assert!(state.take_due_search(5000).is_some());
}

#[test]
fn a_new_query_cancels_queued_continuations_and_says_so() {
    let mut state = SearchState::new();
    state.query = "x".to_string();
    state.note_query_changed(0);
    state
        .queue_control(ContinuationAction::Open(None))
        .expect("queued");
    assert_eq!(state.pending().len(), 1);
    state.note_query_changed(10);
    assert!(state.pending().is_empty());
    assert_eq!(
        state.control_error.as_deref(),
        Some("Previous search continuations cancelled by a newer query")
    );
}

#[test]
fn the_continuation_queue_holds_thirty_two_and_refuses_the_next() {
    let mut state = SearchState::new();
    for _ in 0..MAX_PENDING_CONTINUATIONS {
        state
            .queue_control(ContinuationAction::Move(1))
            .expect("room");
    }
    assert!(state.queue_control(ContinuationAction::Move(1)).is_err());
}

#[test]
fn selection_moves_clamp_and_scroll_keeps_the_selection_visible() {
    let mut state = SearchState::new();
    state.replace_results((0..10).map(|_| hit(ObjectKind::File)).collect());
    state.move_selection(4, 3);
    assert_eq!((state.selected_index, state.scroll), (4, 2));
    state.move_selection(100, 3);
    assert_eq!(state.selected_index, 9);
    state.move_selection(-100, 3);
    assert_eq!((state.selected_index, state.scroll), (0, 0));
    let mut empty = SearchState::new();
    empty.move_selection(1, 3);
    assert_eq!(empty.selected_index, 0);
}

#[test]
fn synchronous_results_reject_a_still_running_worker() {
    let mut state = SearchState::new();
    state.query = "x".to_string();
    state.note_query_changed(0);
    let (revision, _) = state.take_due_search(1000).expect("due");
    state.adopt_synchronous_results(vec![hit(ObjectKind::Agent)]);
    assert!(!state.complete_search(revision, Vec::new()));
    assert_eq!(state.results.len(), 1);
}

/// REQ: PORT:ilium-client/src/search_ui.rs::overflowing_results_reserve_a_separate_scrollbar_column
#[test]
fn port_overflowing_results_reserve_a_separate_scrollbar_column() {
    // Height 15 shows three results; three fit without a gutter, four need one column.
    assert_eq!(result_content_width(40, 15, 3), 40);
    assert_eq!(result_content_width(40, 15, 4), 39);
    for width in [0u16, 1, 2] {
        assert!(result_content_width(width, 1, 10) <= width);
    }
}

#[test]
fn clicks_below_the_last_whole_result_select_nothing() {
    // Height 12 paints two whole results (10 rows); rows 10 and 11 are leftover space.
    assert_eq!(visible_result_rows(12), 2);
    assert_eq!(result_at_row(0, 12, 0, 10), Some(0));
    assert_eq!(result_at_row(RESULT_HEIGHT, 12, 3, 10), Some(4));
    assert_eq!(result_at_row(10, 12, 0, 10), None);
    assert_eq!(result_at_row(0, 12, 9, 9), None);
    assert_eq!(visible_result_rows(0), 1);
}

#[test]
fn metadata_lists_title_path_and_last_command_or_a_generic_line() {
    let mut found = hit(ObjectKind::Agent);
    assert_eq!(result_metadata(&found), "Open workspace object");
    found.automatic_title = Some("Fix".to_string());
    found.path = Some("/a/b".to_string());
    found.last_command = Some("git status".to_string());
    assert_eq!(
        result_metadata(&found),
        "AI title: Fix  ·  /a/b  ·  last command: git status"
    );
}

#[test]
fn grouping_keeps_first_seen_kind_order() {
    let hits = [ObjectKind::File, ObjectKind::Agent, ObjectKind::File]
        .map(hit)
        .to_vec();
    let groups = group_by_kind(&hits);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0], (ObjectKind::File, vec![0, 2]));
    assert_eq!(groups[1], (ObjectKind::Agent, vec![1]));
}
