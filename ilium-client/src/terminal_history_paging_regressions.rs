use super::*;

#[test]
fn live_output_at_or_below_the_snapshot_watermark_is_applied_exactly_once() {
    let mut view = TerminalView::new(4, 30);
    view.apply_replay(b"screen at sequence ten\r\n", 10, true);
    let screen_after_snapshot = view.with_screen(|screen| screen.contents());

    // Output the snapshot already covers arrives again from a racing delta.
    view.apply_live_output(8, 9, b"already covered\r\n", true);
    assert_eq!(view.last_output_sequence(), 10);
    assert_eq!(
        view.with_screen(|screen| screen.contents()),
        screen_after_snapshot
    );

    view.apply_live_output(11, 11, b"fresh line\r\n", true);
    let screen_after_first_apply = view.with_screen(|screen| screen.contents());
    assert_eq!(view.last_output_sequence(), 11);
    assert!(screen_after_first_apply.contains("fresh line"));

    // The same sequence delivered twice must not draw its bytes twice.
    view.apply_live_output(11, 11, b"fresh line\r\n", true);
    assert_eq!(
        view.with_screen(|screen| screen.contents()),
        screen_after_first_apply
    );
}

#[test]
fn paging_requests_older_history_once_per_reply_and_stops_at_the_journal_start() {
    let budget = 8 * 1024 * 1024;
    let mut paging = HistoryPaging::default();
    paging.snapshot_applied(100, 1_000);
    assert_eq!(paging.next_request(budget), Some(101));

    paging.mark_request_sent();
    assert_eq!(
        paging.next_request(budget),
        None,
        "no second request while one is in flight"
    );

    paging.reply_arrived();
    paging.history_applied(60, 4_000, false);
    assert_eq!(
        paging.next_request(budget),
        Some(60),
        "the next page asks for output before the one just applied"
    );

    paging.mark_request_sent();
    paging.reply_arrived();
    paging.history_applied(1, 500, true);
    assert_eq!(
        paging.next_request(budget),
        None,
        "a reply that reaches the journal start ends paging"
    );
}

#[test]
fn paging_stops_once_the_retained_budget_is_full() {
    let mut paging = HistoryPaging::default();
    paging.snapshot_applied(50, 4 * 1024 * 1024);
    assert_eq!(paging.next_request(4 * 1024 * 1024), None);
    assert_eq!(paging.next_request(8 * 1024 * 1024), Some(51));
}

#[test]
fn a_delta_replay_stops_older_paging_until_the_next_snapshot() {
    let budget = 8 * 1024 * 1024;
    let mut paging = HistoryPaging::default();
    paging.snapshot_applied(100, 1_000);
    paging.replay_applied(2_000);
    assert_eq!(
        paging.next_request(budget),
        None,
        "a delta replay has no snapshot base to page back from"
    );
    paging.snapshot_applied(200, 1_000);
    assert_eq!(paging.next_request(budget), Some(201));
}
