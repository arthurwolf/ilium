/// REQ: PDT-012
#[test]
fn pdt_012_no_color_set_emits_no_colour_escapes_and_keeps_bold_underline_reverse() {
    // Observable: Run a plugin view in the harness with `NO_COLOR=1` and capture the output: it contains no SGR colour parameters (30-37, 38, 40-47, 48, 90-97, 100-107) and still contains the bold sequence for a bold span.
    panic!(
        "PDT-012 not implemented: With `NO_COLOR` set to any non-empty value the host MUST emit no colour escape sequences for plugin text and MUST keep bold, underline and reverse attributes."
    );
}

/// REQ: PDT-013
#[test]
fn pdt_013_rgb_colour_on_256_terminal_maps_to_nearest_xterm_index_by_cie76() {
    // Observable: Render `rgb(250, 128, 114)` with `--colour-depth 256`: the emitted index is 209, the nearest xterm-256 entry by CIE76.
    panic!(
        "PDT-013 not implemented: An rgb colour on a 256-colour terminal MUST map to the nearest xterm-256 index by CIE76 distance in CIELAB."
    );
}

/// REQ: PDT-014
#[test]
fn pdt_014_monochrome_danger_role_shows_bold_plus_literal_prefix_not_colour_alone() {
    // Observable: Render a `danger` span with prefix `!` at `--colour-depth 8` mono: the output contains the bold sequence and the literal `!` before the text, and no colour sequence.
    panic!(
        "PDT-014 not implemented: At monochrome depth the host MUST show the `danger` role as bold plus the literal prefix text the plugin supplied, never by colour alone."
    );
}

/// REQ: PDT-015
#[test]
fn pdt_015_harness_accepts_colour_depth_8_16_256_truecolor_and_refuses_others_with_exit_2() {
    // Observable: Run `ilium plugin test --colour-depth 64`: the exit code is 2 and the output contains `usage.invalid-value`; `--colour-depth 16` runs and reports depth 16 in the report file.
    panic!(
        "PDT-015 not implemented: The harness MUST accept `--colour-depth 8|16|256|truecolor` for render checks and MUST refuse any other value with exit 2 and `usage.invalid-value`."
    );
}

/// REQ: PDT-016
#[test]
fn pdt_016_lint_reports_colour_only_state_when_states_differ_only_by_role_colour() {
    // Observable: Declare two badge states with identical text and glyph and different role colours: `ilium plugin lint` prints `ui.colour-only-state` naming the widget.
    panic!(
        "PDT-016 not implemented: `ilium plugin lint` MUST report `ui.colour-only-state` when two declared states of one widget differ only by a role colour."
    );
}
