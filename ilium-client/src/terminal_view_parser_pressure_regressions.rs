use super::*;
use crate::terminal_parsing::MAX_STATE_BYTES;

#[test]
fn history_rebuild_uses_its_admitted_row_cap_and_preserves_the_requested_endpoint() {
    let mut state = TerminalState::new(4, 4096);
    state.resize(4, 128);
    let journal = (0..300)
        .map(|row| format!("journal row {row:04}\r\n"))
        .collect::<String>()
        .into_bytes();
    state.feed(&journal);
    let (peak_bytes, scrollback_rows) = state.history_rebuild_budget(journal.len());
    assert!(peak_bytes <= MAX_STATE_BYTES);
    assert!(scrollback_rows > 0 && scrollback_rows < state.render_scrollback_rows);

    state.jump_to_history_byte(journal.len());
    assert_eq!(
        state.historical_viewport.as_ref().unwrap().scrollback_total,
        scrollback_rows
    );
    assert!(state.with_screen(|screen| screen.contents().contains("journal row 0299")));
    assert_eq!(state.searchable_history_snapshot().to_vec(), journal);
    assert!(state.retained_allocation_bytes() <= MAX_STATE_BYTES);
}

#[test]
fn external_owner_release_cannot_change_fenced_resize_or_history_peaks() {
    let mut state = TerminalState::new(4, 128);
    state.feed(b"first\r\nneedle\r\nlast");
    assert!(crate::terminal_parsing::validate_geometry(255, 4096).is_ok());
    let resize_peak = state.resize_peak_bytes(255, 4096);
    let history_peak = state.history_rebuild_peak_bytes(14);
    assert!(resize_peak > MAX_STATE_BYTES);
    assert!(history_peak <= MAX_STATE_BYTES);

    let snapshot = state.publish();
    let history = state.searchable_history_snapshot();
    assert_eq!(state.resize_peak_bytes(255, 4096), resize_peak);
    assert_eq!(state.history_rebuild_peak_bytes(14), history_peak);
    drop(snapshot);
    drop(history);
    assert_eq!(state.resize_peak_bytes(255, 4096), resize_peak);
    assert_eq!(state.history_rebuild_peak_bytes(14), history_peak);
}

#[test]
fn releasing_the_shared_active_history_segment_resumes_the_same_output_cursor() {
    let mut state = TerminalState::new(3, 20);
    state.feed(b"seed");

    Arc::get_mut(&mut state.history_segments.back_mut().unwrap().bytes)
        .unwrap()
        .reserve(128);
    let bytes = b" copy-on-write";
    let unshared_peak = state.input_peak_bytes(bytes);
    let history = state.searchable_history_snapshot();
    assert!(state.input_peak_bytes(bytes) > unshared_peak);
    let mut cursor = OrderedOutputCursor::default();
    let mut before = |current: &TerminalState, chunk: &[u8]| {
        if current.input_peak_bytes(chunk) > unshared_peak {
            Err("forced copy-on-write peak".to_string())
        } else {
            Ok(())
        }
    };
    assert!(state
        .apply_ordered_output(1, 1, bytes, false, &mut cursor, &mut before)
        .is_err());
    assert_eq!(cursor.consumed, 0);
    assert_eq!(state.output_sequence(), 0);
    assert_eq!(state.searchable_history_snapshot().to_vec(), b"seed");

    drop(history);
    assert_eq!(state.input_peak_bytes(bytes), unshared_peak);
    state
        .apply_ordered_output(1, 1, bytes, false, &mut cursor, &mut before)
        .unwrap();
    assert_eq!(cursor.consumed, bytes.len());
    assert_eq!(state.output_sequence(), 1);
    assert_eq!(
        state.searchable_history_snapshot().to_vec(),
        b"seed copy-on-write"
    );
}

#[test]
fn oversized_pane_sheds_history_to_fit_instead_of_deferring_forever() {
    // A 64 MiB history budget plus Vec doubling and render scrollback once
    // pushed one pane past the 128 MiB cap, wedging the whole client.
    let mut state = TerminalState::with_scrollback_budget_mib(24, 200, 96);
    let chunk = vec![b'x'; 1024 * 1024];
    for _ in 0..90 {
        state.append_history_for_benchmark(&chunk);
    }
    let limit = state.retained_allocation_bytes() / 2;
    assert!(state.retained_allocation_bytes() > limit);
    assert!(state.fit_to_allocation_limit(limit));
    assert!(state.retained_allocation_bytes() <= limit);
    assert!(state
        .searchable_history_snapshot()
        .to_vec()
        .iter()
        .all(|byte| *byte == b'x'));

    // The configured budget must survive: more history can be appended and is
    // retained up to that budget instead of the shrunken post-trim size.
    let trimmed_len = state.searchable_history_snapshot().len();
    for _ in 0..4 {
        state.append_history_for_benchmark(&chunk);
    }
    assert!(state.searchable_history_snapshot().len() >= trimmed_len + 4 * chunk.len());
}
