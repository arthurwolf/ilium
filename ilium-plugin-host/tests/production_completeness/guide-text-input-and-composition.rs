/// REQ: PDT-006
#[test]
fn pdt_006_printable_key_commands_suppressed_during_composition() {
    // Observable: Bind a command to `j`, start a composition in the harness and send `j` as preedit: the command's invocation count stays 0 until the composition is committed or cancelled.
    panic!(
        "PDT-006 not implemented: While a composition is active the host MUST NOT run plugin commands bound to printable keys."
    );
}

/// REQ: PDT-007
#[test]
fn pdt_007_change_event_carries_only_committed_string() {
    // Observable: Compose `に` through preedit `n`,`i` and commit: the plugin receives exactly one change event whose `value` ends with `に` and no event containing `n` or `ni`.
    panic!(
        "PDT-007 not implemented: A text-input change event MUST carry only the committed string, never preedit text."
    );
}

/// REQ: PDT-008
#[test]
fn pdt_008_bracketed_paste_one_event_truncated_at_1mib() {
    // Observable: Paste 2 MiB with bracketed paste in the harness: the plugin receives one change event with a 1 MiB value and the harness records one notice `input.paste-truncated`.
    panic!(
        "PDT-008 not implemented: A bracketed paste MUST arrive as one change event of up to 1 MiB, and a larger paste MUST be truncated to 1 MiB with the notice code `input.paste-truncated`."
    );
}

/// REQ: PDT-009
#[test]
fn pdt_009_wide_character_advances_cursor_two_cells() {
    // Observable: Type `漢` into an empty input: the cursor cell index reported by the harness is 2, and the fixture `fixtures/input-wide-cursor` passes.
    panic!(
        "PDT-009 not implemented: A wide character MUST advance the cursor by two cells, and the harness MUST ship a fixture that checks this."
    );
}

/// REQ: PDT-010
#[test]
fn pdt_010_maxlength_counted_in_grapheme_clusters() {
    // Observable: Set `maxlength: 3` and type `é`, `漢`, `a`, `b`: the value after the fourth keystroke contains exactly three clusters and `b` is refused.
    panic!(
        "PDT-010 not implemented: An input `maxlength` MUST be counted in grapheme clusters, not bytes or code points."
    );
}

/// REQ: PDT-011
#[test]
fn pdt_011_cursor_moves_one_grapheme_per_arrow_press() {
    // Observable: Put the cursor after `éx` and press Left twice: the cursor is at index 0 and the first press moved it before `x`.
    panic!(
        "PDT-011 not implemented: Cursor movement in a plugin text input MUST move exactly one grapheme cluster per Left or Right key press."
    );
}
