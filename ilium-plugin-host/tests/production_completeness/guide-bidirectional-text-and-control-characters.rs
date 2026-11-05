/// REQ: PDT-001
#[test]
fn pdt_001_rtl_runs_drawn_in_logical_order() {
    // Observable: Render the fixture string `abc שלום` in a plugin view with the harness: the cells left to right contain `a`,`b`,`c`,` `,`ש`,`ל`,`ו`,`ם`.
    panic!(
        "PDT-001 not implemented: The host MUST draw RTL runs in logical order, left to right in cells, and MUST NOT reorder or mirror characters."
    );
}

/// REQ: PDT-002
#[test]
fn pdt_002_bidi_controls_replaced_on_approval_prompts() {
    // Observable: Pass a path containing U+202E to an approval prompt: the rendered prompt shows U+FFFD in that position and the prompt text contains no code point in U+202A..U+202E or U+2066..U+2069.
    panic!(
        "PDT-002 not implemented: The host MUST replace each of U+202A to U+202E and U+2066 to U+2069 in every plugin-supplied string drawn on an approval, permission or install prompt with one U+FFFD cell, consistent with [SEC-104] for names."
    );
}

/// REQ: PDT-003
#[test]
fn pdt_003_truncation_keeps_grapheme_clusters_whole() {
    // Observable: Render a 10-cell label whose 10th cell would fall inside the cluster `é`: the output ends with `…` at cell 10 and the preceding cells contain only whole clusters.
    panic!(
        "PDT-003 not implemented: Truncation MUST NOT split a grapheme cluster or a combining sequence and MUST place the ellipsis `…` in the last cell."
    );
}

/// REQ: PDT-004
#[test]
fn pdt_004_lint_reports_bidi_control_in_manifest() {
    // Observable: Put U+202E in a manifest `title` and run `ilium plugin lint`: the output contains `ui.bidi-control` naming the `title` key.
    panic!(
        "PDT-004 not implemented: `ilium plugin lint` MUST report `ui.bidi-control` for any literal bidi control character in a manifest string or catalogue message."
    );
}

/// REQ: PDT-005
#[test]
fn pdt_005_permission_dialog_strips_bidi_controls() {
    // Observable: Install a plugin whose `title` contains U+202E: the dialog shows the stripped title and `ilium plugin inspect <id> --json` returns the `title` with the U+202E code point present.
    panic!(
        "PDT-005 not implemented: Names shown in the permission dialog MUST be rendered with bidi controls stripped, and `ilium plugin inspect <id> --json` MUST expose the raw unstripped value."
    );
}
