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
    assert!(scrollback_rows > 0 && scrollback_rows <= state.render_scrollback_rows);

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
    assert!(resize_peak <= MAX_STATE_BYTES);
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
fn shared_journal_append_has_a_bounded_peak_and_preserves_published_bytes() {
    let mut state = TerminalState::new(3, 20);
    state.feed(b"seed");
    let history = state.searchable_history_snapshot();
    let mut cursor = OrderedOutputCursor::default();
    state
        .apply_ordered_output(1, 1, b" tail", false, &mut cursor, |state, chunk| {
            state.prepare_input(chunk, MAX_STATE_BYTES);
            Ok(())
        })
        .unwrap();
    assert_eq!(history.to_vec(), b"seed");
    assert_eq!(state.searchable_history_snapshot().to_vec(), b"seed tail");
    assert_eq!(cursor.consumed, 5);
    assert!(state
        .history_segments
        .iter()
        .all(|segment| segment.bytes.capacity() <= 65536));
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

#[test]
fn pressure_reclamation_preserves_split_utf8_escape_state_and_output_sequence() {
    let mut state = TerminalState::with_scrollback_budget_mib(4, 80, 96);
    state.feed(b"\x1b[?2004h\x1b[?1000hprefix ");
    // Artificially retained raw history makes admission fail without trimming,
    // without spending this regression parsing tens of millions of cells.
    for _ in 0..20 {
        state.append_history_for_benchmark(&vec![b'x'; 65536]);
    }
    let mut cursor = OrderedOutputCursor::default();
    state
        .apply_ordered_output(1, 1, b"\xe2\x82", false, &mut cursor, |_, _| Ok(()))
        .unwrap();
    let limit = state.retained_allocation_bytes() - 512 * 1024;
    let mut cursor = OrderedOutputCursor::default();
    state
        .apply_ordered_output(2, 2, b"\xac\x1b[31", false, &mut cursor, |state, chunk| {
            state.prepare_input(chunk, limit);
            assert!(state.input_peak_bytes(chunk) <= limit);
            Ok(())
        })
        .unwrap();
    let mut cursor = OrderedOutputCursor::default();
    state
        .apply_ordered_output(3, 3, b"mred", false, &mut cursor, |state, chunk| {
            state.prepare_input(chunk, limit);
            assert!(state.input_peak_bytes(chunk) <= limit);
            Ok(())
        })
        .unwrap();
    assert_eq!(state.output_sequence(), 3);
    assert!(state.wants_mouse_protocol());
    assert!(state.wants_bracketed_paste());
    assert!(state.with_screen(|screen| screen.contents().contains("prefix €red")));
    assert!(state.history_retained_len < 20 * 65536);
}

#[test]
fn alternate_screen_pressure_reclaims_hidden_primary_history_and_preserves_frozen_view() {
    let mut state = TerminalState::new(4, 80);
    for _ in 0..500 {
        state.feed(b"primary retained line\r\n");
    }
    state.scroll_up(20);
    let frozen = state.historical_viewport.as_ref().unwrap().screen.clone();
    state.feed(b"\x1b[?1049hactive alternate");
    assert_eq!(state.scrollback_total, 0);
    let live_before = state.parser.allocation_bytes();
    state.fit_to_allocation_limit(0);
    assert!(state.parser.allocation_bytes() < live_before);
    assert!(state
        .parser
        .screen()
        .contents()
        .contains("active alternate"));
    assert_eq!(
        state
            .historical_viewport
            .as_ref()
            .unwrap()
            .screen
            .contents(),
        frozen.contents()
    );
    assert!(state.historical_viewport.as_ref().unwrap().scrollback_total <= 4);
}

#[test]
fn split_alternate_activation_reserves_lazy_grid_with_no_render_history() {
    let mut state = TerminalState::with_scrollback_budget_mib(4, 80, 96);
    state.render_scrollback_rows = 0;
    state.parser.screen_mut().set_scrollback_limit(0);
    let mut cursor = OrderedOutputCursor::default();
    state
        .apply_ordered_output(
            1,
            1,
            b"PR\x1b[2;4H\x1b[?1049",
            false,
            &mut cursor,
            |_, _| Ok(()),
        )
        .unwrap();
    for _ in 0..20 {
        state.append_history_for_benchmark(&[0; 65536]);
    }
    let pending = state.parser.screen().pending_screen_allocation_bytes();
    assert!(pending > 0);
    assert!(state.input_peak_bytes(b"h") >= state.retained_allocation_bytes() + pending);
    let limit = state.retained_allocation_bytes() / 2;
    let mut cursor = OrderedOutputCursor::default();
    state
        .apply_ordered_output(2, 2, b"hALT", false, &mut cursor, |state, bytes| {
            state.prepare_input(bytes, limit);
            assert!(state.input_peak_bytes(bytes) <= limit);
            Ok(())
        })
        .unwrap();
    assert_eq!(state.parser.screen().cursor_position(), (0, 3));
    assert!(state.parser.screen().contents().contains("ALT"));
    let mut cursor = OrderedOutputCursor::default();
    state
        .apply_ordered_output(3, 3, b"\x1b[?1049lX", false, &mut cursor, |state, bytes| {
            state.prepare_input(bytes, limit);
            assert!(state.input_peak_bytes(bytes) <= limit);
            Ok(())
        })
        .unwrap();
    assert_eq!(state.output_sequence(), 3);
    assert_eq!(state.parser.screen().cursor_position(), (1, 4));
    assert!(state.parser.screen().contents().contains("PR"));
}

#[test]
fn frozen_history_pressure_preserves_chosen_rows_and_keeps_output_progressing() {
    let mut state = TerminalState::new(4, 80);
    for row in 0..600 {
        state.feed(format!("history row {row:04}\r\n").as_bytes());
    }
    state.scroll_up(300);
    let chosen = state.with_screen(vt100::Screen::contents);
    let held = state.publish();
    for _ in 0..20 {
        state.append_history_for_benchmark(&[0; 65536]);
    }
    let mut cursor = OrderedOutputCursor::default();
    state
        .apply_ordered_output(
            1,
            1,
            b"live output survived",
            false,
            &mut cursor,
            |state, bytes| {
                state.prepare_input(bytes, 768 * 1024);
                assert!(state.input_peak_bytes(bytes) <= 768 * 1024);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(state.output_sequence(), 1);
    assert!(state
        .parser
        .screen()
        .contents()
        .contains("live output survived"));
    assert_eq!(state.with_screen(vt100::Screen::contents), chosen);
    assert!(state.historical_viewport.as_ref().unwrap().scrollback_total <= 4);
    assert_eq!(held.visible.contents(), chosen);

    state.resize(8, 120);
    assert_eq!(state.parser.screen().size(), (8, 120));
    assert_eq!(held.visible.size(), (4, 80));
    assert_eq!(held.visible.contents(), chosen);
}

#[test]
fn frozen_view_that_cannot_fit_returns_to_live_instead_of_fencing_output() {
    let mut state = TerminalState::new(40, 100);
    for _ in 0..100 {
        state.feed(b"retained history\r\n");
    }
    state.scroll_up(50);
    assert!(state.historical_viewport.is_some());
    state.prepare_input(b"alive", 700 * 1024);
    assert!(state.historical_viewport.is_none());
    assert!(state.input_peak_bytes(b"alive") <= 700 * 1024);
    let mut cursor = OrderedOutputCursor::default();
    state
        .apply_ordered_output(1, 1, b"alive", false, &mut cursor, |_, _| Ok(()))
        .unwrap();
    assert_eq!(state.output_sequence(), 1);
    assert!(state.parser.screen().contents().contains("alive"));
}

#[test]
fn tiny_appends_with_held_history_snapshots_allocate_proportionally() {
    let mut state = TerminalState::new(4, 80);
    let mut held = Vec::new();
    for _ in 0..1024 {
        state.append_history_for_benchmark(b"x");
        held.push(state.searchable_history_snapshot());
    }
    assert_eq!(state.history_retained_len, 1024);
    let allocated = state
        .history_segments
        .iter()
        .map(|segment| segment.bytes.capacity())
        .sum::<usize>();
    assert!(
        allocated <= 8 * 1024,
        "one-byte append reserved {allocated} bytes"
    );
    assert_eq!(held.first().unwrap().to_vec(), b"x");
    assert_eq!(held.last().unwrap().len(), 1024);
    assert!(
        state.input_peak_bytes(b"y")
            >= state.retained_allocation_bytes()
                + state.history_segments.capacity() * 2 * std::mem::size_of::<HistorySegment>()
    );
}

#[test]
fn allocation_caches_follow_segment_growth_rotation_replay_and_link_eviction() {
    fn assert_exact(state: &TerminalState) {
        assert_eq!(
            state.history_allocation_bytes,
            state
                .history_segments
                .iter()
                .map(|segment| segment.bytes.capacity())
                .sum::<usize>()
        );
        assert_eq!(
            state.osc8_link_allocation_bytes,
            state
                .osc8_links
                .iter()
                .map(|(label, target)| label.capacity() + target.capacity())
                .sum::<usize>()
        );
    }
    let mut state = TerminalState::new(4, 80);
    let mut held = Vec::new();
    for index in 0..300 {
        state.append_history_for_benchmark(&vec![b'x'; index + 1]);
        held.push(state.searchable_history_snapshot());
        state.observe_osc8_links(
            format!("\x1b]8;;https://example.test/{index}\x07label{index}\x1b]8;;\x07").as_bytes(),
        );
        assert_exact(&state);
    }
    state.history_budget_bytes = 1024;
    state.trim_history_to_budget();
    assert_exact(&state);
    state.fit_to_allocation_limit(0);
    assert_exact(&state);
    state.apply_replay(b"fresh", 1, true);
    assert_exact(&state);
    assert_eq!(state.osc8_link_allocation_bytes, 0);
    assert_eq!(held.first().unwrap().to_vec(), b"x");
}

#[test]
fn replay_large_history_tracks_allocation_capacity_and_trims_without_underflow() {
    let mut state = TerminalState::new(4, 80);
    let mut cursor = OrderedOutputCursor::default();
    // 256 KiB replay reserves capacity on its initial segment
    let replay_bytes = vec![b'a'; 256 * 1024];
    state
        .apply_ordered_replay(&replay_bytes, 1, &mut cursor, |_, _| Ok(()))
        .unwrap();

    assert_eq!(
        state.history_allocation_bytes,
        state
            .history_segments
            .iter()
            .map(|segment| segment.bytes.capacity())
            .sum::<usize>()
    );

    // Trimming the budget down to 1 KiB must not underflow history_allocation_bytes
    state.history_budget_bytes = 1024;
    state.trim_history_to_budget();

    assert_eq!(
        state.history_allocation_bytes,
        state
            .history_segments
            .iter()
            .map(|segment| segment.bytes.capacity())
            .sum::<usize>()
    );
}

#[test]
fn inert_escape_runs_use_bounded_quanta_without_executing_pending_commands() {
    let mut state = TerminalState::new(4, 80);
    let mut expected = TerminalState::new(4, 80);
    // Each prefix leaves a different incomplete command. ESC must cancel it,
    // rather than completing a scroll, OSC, UTF-8 codepoint or alternate entry.
    for prefix in [
        b"\x1b[999999".as_slice(),
        b"\x1b]8;;uri",
        b"\xe2\x82",
        b"\x1b[?1049",
    ] {
        state.parser.process(prefix);
        expected.parser.process(prefix);
        let bytes = vec![0x1b; 1024 * 1024];
        let mut admissions = 0;
        let mut cursor = OrderedOutputCursor::default();
        let sequence = state.output_sequence() + 1;
        state
            .apply_ordered_output(
                sequence,
                sequence,
                &bytes,
                false,
                &mut cursor,
                |state, chunk| {
                    admissions += 1;
                    assert!(chunk.len() <= 16 * 1024);
                    // No viewport-sized allocation allowance for inert escape bytes.
                    assert!(
                        state.input_peak_bytes(chunk)
                            < state.retained_allocation_bytes() + 512 * 1024
                    );
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(admissions, 64);
        expected.parser.process(&bytes);
        state.parser.process(b"[2;3Hdone");
        expected.parser.process(b"[2;3Hdone");
        assert_eq!(
            state.parser.screen().contents_formatted(),
            expected.parser.screen().contents_formatted()
        );
        assert_eq!(
            state.parser.screen().cursor_position(),
            expected.parser.screen().cursor_position()
        );
        assert_eq!(state.output_sequence(), sequence);
    }
}
