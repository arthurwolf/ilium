//! reference-extension-security.md: kernel security UI that plugins cannot imitate or read, text
//! sanitising and display-name rules (SEC-377 .. 381).

use ilium_plugin_api::Tier;
use ilium_plugin_host::render::sanitize_plugin_text;
use proptest::prelude::*;
use serde_json::json;

use crate::support::*;

const KERNEL_DIALOGS: [&str; 5] = ["review", "trust", "consent", "breach", "recovery"];
const PLUGIN_CHROME: [&str; 5] = [
    "toast",
    "tree-chrome",
    "palette-entry",
    "help-page",
    "intent-chrome",
];
const TEXT_FIELDS: [&str; 6] = [
    "label",
    "title",
    "toast",
    "reason",
    "status-segment",
    "help-text",
];

fn forbidden(character: char) -> bool {
    let code = character as u32;
    code < 0x20
        || (0x7f..=0x9f).contains(&code)
        || (0x202a..=0x202e).contains(&code)
        || (0x2066..=0x2069).contains(&code)
        || matches!(code, 0x200b | 0x200c | 0x200d | 0x2060 | 0xfeff)
}

// ---- SEC-377 ----

/// REQ: SEC-377
#[test]
fn sec_377_every_kernel_security_dialog_is_drawn_on_top_of_plugin_ui() {
    for kind in KERNEL_DIALOGS {
        let rig = t3(&[]);
        rig.host
            .plugin_draw_rows(&["PLUGIN ROW ONE", "PLUGIN ROW TWO"])
            .ok();
        rig.host.open_kernel_dialog(kind);
        let screen = rig.host.composited_screen().join("\n");
        assert!(
            rig.host.visible_dialogs().contains(&kind.to_owned()),
            "{kind}"
        );
        assert!(
            !screen.contains("PLUGIN ROW ONE") || rig.host.dialog_is_topmost(kind),
            "{kind}"
        );
        assert!(rig.host.dialog_is_topmost(kind), "{kind}");
    }
}

/// REQ: SEC-377
#[test]
fn sec_377_the_safe_mode_badge_is_drawn_on_top_of_plugin_status_segments() {
    let rig = setup(&[]).safe_mode().start();
    rig.host.plugin_status_segment(
        "A very long plugin segment that tries to push the badge out of the bar",
    );
    assert!(rig.host.status_bar_text().contains("SAFE"));
}

/// REQ: SEC-377
#[test]
fn sec_377_no_kernel_dialog_opens_while_the_user_types_in_a_terminal_pane() {
    for kind in KERNEL_DIALOGS {
        let rig = t3(&[]);
        rig.host.set_user_typing_in_terminal(true);
        rig.host.open_kernel_dialog(kind);
        assert!(rig.host.visible_dialogs().is_empty(), "{kind}");
        assert!(
            rig.host.queued_dialogs().contains(&kind.to_owned()),
            "{kind}"
        );
    }
}

/// REQ: SEC-377
#[test]
fn sec_377_a_queued_dialog_opens_when_the_user_leaves_the_pane() {
    let rig = t3(&[]);
    rig.host.set_user_typing_in_terminal(true);
    rig.host.open_kernel_dialog("consent");
    rig.host.set_user_typing_in_terminal(false);
    assert!(rig.host.visible_dialogs().contains(&"consent".to_owned()));
    assert!(rig.host.queued_dialogs().is_empty());
}

/// REQ: SEC-377
#[test]
fn sec_377_a_queued_dialog_opens_when_the_user_presses_the_dialog_key() {
    let rig = t3(&[]);
    rig.host.set_user_typing_in_terminal(true);
    rig.host.open_kernel_dialog("review");
    rig.host.press_dialog_key();
    assert!(rig.host.visible_dialogs().contains(&"review".to_owned()));
}

/// REQ: SEC-377
#[test]
fn sec_377_keystrokes_typed_before_a_dialog_appears_never_reach_it() {
    let rig = t3(&[]);
    rig.host.set_user_typing_in_terminal(true);
    rig.host.type_keys("yes\r");
    rig.host.set_user_typing_in_terminal(false);
    rig.host.open_kernel_dialog("consent");
    assert_eq!(rig.host.dialog_received_keys(), "");
    assert!(
        rig.host.visible_dialogs().contains(&"consent".to_owned()),
        "a typed Enter must not accept the dialog"
    );
}

/// REQ: SEC-377
#[test]
fn sec_377_a_plugin_cannot_imitate_a_kernel_dialog_while_the_user_types() {
    let rig = t3(&[]);
    rig.host.set_user_typing_in_terminal(true);
    let drawn = rig
        .host
        .plugin_draw_rows(&["\u{256d}\u{2500}\u{2500} Install plugin \u{256e}"]);
    let screen = rig.host.composited_screen().join("\n");
    assert!(
        drawn.is_err() || !screen.contains("Install plugin"),
        "{screen}"
    );
}

// ---- SEC-378 ----

/// REQ: SEC-378
#[test]
fn sec_378_plugin_chrome_carries_the_display_name_as_a_prefix() {
    let rig = t3(&[]);
    let name = rig.host.plugin_display_name();
    for kind in PLUGIN_CHROME {
        let text = rig.host.render_plugin_chrome(kind, "hello");
        assert!(
            text.starts_with(&name) || text.contains(&format!("{name}:")),
            "{kind}: {text}"
        );
    }
}

/// REQ: SEC-378
#[test]
fn sec_378_the_plugin_cannot_remove_the_prefix() {
    let rig = t3(&[]);
    let name = rig.host.plugin_display_name();
    for kind in PLUGIN_CHROME {
        for text in [
            "",
            " ",
            "\u{8}\u{8}\u{8}\u{8}\u{8}\u{8}\u{8}\u{8}",
            "\r",
            "\x1b[2K\x1b[1G",
        ] {
            assert!(
                rig.host.render_plugin_chrome(kind, text).contains(&name),
                "{kind} {text:?}"
            );
        }
    }
}

/// REQ: SEC-378
#[test]
fn sec_378_plugin_chrome_never_uses_the_rounded_border_of_security_dialogs() {
    let rig = t3(&[]);
    for kind in PLUGIN_CHROME {
        let text = rig
            .host
            .render_plugin_chrome(kind, "\u{256d}\u{2570} \u{256e}\u{256f}");
        assert!(
            !text.contains(['\u{256d}', '\u{256e}', '\u{2570}', '\u{256f}']),
            "{kind}: {text}"
        );
    }
}

/// REQ: SEC-378
#[test]
fn sec_378_kernel_dialogs_use_the_rounded_border() {
    let rig = t3(&[]);
    for kind in KERNEL_DIALOGS {
        let lines = rig.host.render_dialog(kind, 48, true);
        assert!(
            lines
                .first()
                .is_some_and(|line| line.starts_with('\u{256d}')),
            "{kind}"
        );
        assert!(
            lines
                .last()
                .is_some_and(|line| line.starts_with('\u{2570}')),
            "{kind}"
        );
    }
}

/// REQ: SEC-378
#[test]
fn sec_378_a_plugin_name_that_imitates_the_kernel_cannot_be_installed() {
    let codes = manifest_error_codes(&server_manifest(json!({ "name": "Ilium Security" })));
    assert!(
        codes.iter().any(|code| code == "manifest.display-name"),
        "{codes:?}"
    );
}

// ---- SEC-379 ----

/// REQ: SEC-379
#[test]
fn sec_379_a_plugin_draw_call_does_not_change_the_cells_of_a_kernel_dialog() {
    let rig = t3(&[]);
    rig.host.open_kernel_dialog("consent");
    let before = rig.host.dialog_cells("consent");
    let _ = rig
        .host
        .plugin_draw_rows(&["XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX"; 30]);
    assert_eq!(rig.host.dialog_cells("consent"), before);
    assert!(!rig
        .host
        .composited_screen()
        .join("\n")
        .contains("XXXXXXXXXX\u{0}"));
}

/// REQ: SEC-379
#[test]
fn sec_379_screen_changed_payloads_never_contain_dialog_text() {
    let rig = t3(&["terminal.read-screen"]);
    rig.host.open_kernel_dialog("review");
    rig.host.pane_output("shell output\n");
    rig.host.pump(8);
    let dialog_text = rig.host.render_dialog("review", 48, true).join("\n");
    let marker = dialog_text
        .lines()
        .find(|line| line.chars().filter(|c| c.is_alphabetic()).count() > 8)
        .unwrap_or("Install")
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_owned();
    for payload in rig.host.screen_changed_payloads() {
        assert!(!payload.contains(&marker), "{payload}");
    }
}

/// REQ: SEC-379
#[test]
fn sec_379_no_capture_call_returns_dialog_text() {
    let rig = t3(&["terminal.read-screen", "terminal.read-cells"]);
    rig.host.open_kernel_dialog("trust");
    let capture = rig.host.plugin_screen_capture();
    for line in rig.host.render_dialog("trust", 48, true) {
        let trimmed = line.trim_matches(|c: char| !c.is_alphanumeric());
        if trimmed.chars().count() > 8 {
            assert!(
                !capture.iter().any(|line| line.contains(trimmed)),
                "{trimmed}"
            );
        }
    }
}

// ---- SEC-380 ----

/// REQ: SEC-380
#[test]
fn sec_380_c0_controls_are_removed_or_replaced() {
    for code in 0u32..0x20 {
        let character = char::from_u32(code).expect("control");
        let cleaned = sanitize_plugin_text(&format!("a{character}b"), 120);
        assert!(
            !cleaned.chars().any(forbidden),
            "U+{code:04X} left in {cleaned:?}"
        );
        assert!(
            cleaned.starts_with('a') && cleaned.ends_with('b'),
            "{cleaned:?}"
        );
    }
}

/// REQ: SEC-380
#[test]
fn sec_380_c1_controls_are_removed_or_replaced() {
    for code in 0x7fu32..=0x9f {
        let character = char::from_u32(code).expect("control");
        let cleaned = sanitize_plugin_text(&format!("a{character}b"), 120);
        assert!(
            !cleaned.chars().any(forbidden),
            "U+{code:04X} left in {cleaned:?}"
        );
    }
}

/// REQ: SEC-380
#[test]
fn sec_380_escape_sequences_are_removed() {
    let cleaned = sanitize_plugin_text("\x1b[31mred\x1b[0m \x1b]0;title\x07done", 120);
    assert!(
        !cleaned.contains('\x1b') && !cleaned.contains("[31m") && !cleaned.contains("[0m"),
        "{cleaned:?}"
    );
    assert!(cleaned.contains("red") && cleaned.contains("done"));
}

/// REQ: SEC-380
#[test]
fn sec_380_bidirectional_overrides_are_removed() {
    let marks: Vec<char> = (0x202au32..=0x202e)
        .chain(0x2066..=0x2069)
        .filter_map(char::from_u32)
        .collect();
    assert_eq!(marks.len(), 9);
    for mark in marks {
        let cleaned = sanitize_plugin_text(&format!("safe{mark}txt.exe"), 120);
        assert!(!cleaned.contains(mark), "U+{:04X}", mark as u32);
    }
}

/// REQ: SEC-380
#[test]
fn sec_380_zero_width_characters_are_removed() {
    for code in [0x200bu32, 0x200c, 0x200d, 0x2060, 0xfeff] {
        let character = char::from_u32(code).expect("zero width");
        let cleaned = sanitize_plugin_text(&format!("Il{character}ium"), 120);
        assert!(!cleaned.contains(character), "U+{code:04X}");
    }
}

/// REQ: SEC-380
#[test]
fn sec_380_box_drawing_runs_of_three_or_more_are_removed_or_replaced() {
    for run in [3usize, 4, 10, 40] {
        let cleaned = sanitize_plugin_text(&format!("a{}b", "\u{2500}".repeat(run)), 120);
        assert!(!cleaned.contains('\u{2500}'), "run of {run}: {cleaned:?}");
    }
    let mixed = sanitize_plugin_text("x\u{2500}\u{2502}\u{256d}y", 120);
    assert!(
        !mixed.contains(['\u{2500}', '\u{2502}', '\u{256d}']),
        "{mixed:?}"
    );
}

/// REQ: SEC-380
#[test]
fn sec_380_box_drawing_runs_shorter_than_three_are_kept() {
    assert_eq!(sanitize_plugin_text("a\u{2500}b", 120), "a\u{2500}b");
    assert_eq!(
        sanitize_plugin_text("a\u{2500}\u{2500}b", 120),
        "a\u{2500}\u{2500}b"
    );
}

/// REQ: SEC-380
#[test]
fn sec_380_plain_text_passes_unchanged() {
    assert_eq!(
        sanitize_plugin_text("Build finished: 3 tests passed", 120),
        "Build finished: 3 tests passed"
    );
    assert_eq!(
        sanitize_plugin_text("caf\u{e9} \u{65e5}\u{672c}", 120),
        "caf\u{e9} \u{65e5}\u{672c}"
    );
}

/// REQ: SEC-380
#[test]
fn sec_380_text_is_clipped_to_the_limit_with_an_ellipsis() {
    let cleaned = sanitize_plugin_text(&"a".repeat(200), 120);
    assert_eq!(cleaned.chars().count(), 120);
    assert!(cleaned.ends_with('\u{2026}'));
}

/// REQ: SEC-380
#[test]
fn sec_380_text_at_the_limit_is_not_clipped_and_one_over_is() {
    let exact = "b".repeat(120);
    assert_eq!(sanitize_plugin_text(&exact, 120), exact);
    let over = sanitize_plugin_text(&"b".repeat(121), 120);
    assert_eq!(over.chars().count(), 120);
    assert!(over.ends_with('\u{2026}'));
}

/// REQ: SEC-380
#[test]
fn sec_380_every_ui_text_field_is_sanitised_before_it_is_shown() {
    let rig = t3(&[]);
    let corpus = [
        "\x1b[2Jclear",
        "evil\u{202e}gnp.exe",
        "Il\u{200b}ium",
        "\u{250c}\u{2500}\u{2500}\u{2500}\u{2510}",
        "bell\x07\x08\x7f",
        "\u{85}next",
    ];
    for field in TEXT_FIELDS {
        for text in corpus {
            let shown = rig.host.render_plugin_text(field, text);
            assert!(!shown.chars().any(forbidden), "{field}: {shown:?}");
            assert!(
                !shown.contains("\u{2500}\u{2500}\u{2500}"),
                "{field}: {shown:?}"
            );
        }
    }
}

/// REQ: SEC-380
#[test]
fn sec_380_a_reason_longer_than_120_characters_is_clipped_when_shown() {
    let rig = t3(&[]);
    let shown = rig.host.render_plugin_text("reason", &"r".repeat(300));
    assert!(shown.contains('\u{2026}'));
    assert!(!shown.contains(&"r".repeat(121)));
}

proptest! {
    // REQ: SEC-380
    #[test]
    fn sec_380_sanitised_text_has_no_forbidden_characters_and_fits_the_limit(text in ".{0,300}", limit in 4usize..160) {
        let cleaned = sanitize_plugin_text(&text, limit);
        prop_assert!(!cleaned.chars().any(forbidden));
        prop_assert!(cleaned.chars().count() <= limit);
    }

    // REQ: SEC-380
    #[test]
    fn sec_380_sanitising_is_idempotent(text in ".{0,200}") {
        let once = sanitize_plugin_text(&text, 120);
        prop_assert_eq!(sanitize_plugin_text(&once, 120), once);
    }

    // REQ: SEC-380
    #[test]
    fn sec_380_no_run_of_three_box_drawing_characters_survives(prefix in "[a-z ]{0,10}", run in 3usize..30, suffix in "[a-z ]{0,10}") {
        let cleaned = sanitize_plugin_text(&format!("{prefix}{}{suffix}", "\u{2550}".repeat(run)), 120);
        let run_of_three = "\u{2550}".repeat(3);
        prop_assert!(!cleaned.contains(run_of_three.as_str()));
    }
}

// ---- SEC-381 ----

fn name_codes(name: &str, id: &str) -> Vec<String> {
    manifest_error_codes(&server_manifest(json!({ "name": name, "id": id })))
}

fn rejects(name: &str, id: &str) -> bool {
    name_codes(name, id)
        .iter()
        .any(|code| code == "manifest.display-name")
}

/// REQ: SEC-381
#[test]
fn sec_381_a_display_name_of_3_to_40_characters_is_accepted() {
    for name in ["Abc", "Example demo", &"x".repeat(40)] {
        assert!(!rejects(name, "example.demo"), "{name}");
    }
}

/// REQ: SEC-381
#[test]
fn sec_381_a_display_name_of_2_characters_is_refused() {
    assert!(rejects("Ab", "example.demo"));
    assert!(rejects("A", "example.demo"));
    assert!(rejects("", "example.demo"));
}

/// REQ: SEC-381
#[test]
fn sec_381_a_display_name_of_41_characters_is_refused() {
    assert!(rejects(&"x".repeat(41), "example.demo"));
}

/// REQ: SEC-381
#[test]
fn sec_381_the_length_counts_characters_not_bytes() {
    assert!(!rejects("\u{e9}\u{e9}\u{e9}", "example.demo"));
    assert!(!rejects(&"\u{e9}".repeat(40), "example.demo"));
    assert!(rejects(&"\u{e9}".repeat(41), "example.demo"));
}

/// REQ: SEC-381
#[test]
fn sec_381_a_name_equal_to_ilium_or_kernel_is_refused_for_a_third_party() {
    assert!(rejects("Ilium", "example.demo"));
    assert!(rejects("Kernel", "example.demo"));
}

/// REQ: SEC-381
#[test]
fn sec_381_a_name_starting_with_ilium_or_kernel_is_refused_for_a_third_party() {
    for name in [
        "Ilium Tools",
        "Iliumx",
        "Kernel Panic",
        "Kernel Security Center",
    ] {
        assert!(rejects(name, "example.demo"), "{name}");
    }
}

/// REQ: SEC-381
#[test]
fn sec_381_a_name_that_only_contains_ilium_or_kernel_is_accepted() {
    for name in ["My Ilium Tools", "Pop Kernel Helper"] {
        assert!(!rejects(name, "example.demo"), "{name}");
    }
}

/// REQ: SEC-381
#[test]
fn sec_381_a_plugin_id_starting_with_ilium_dot_may_use_the_names() {
    for name in ["Ilium", "Ilium Notes", "Kernel Tools"] {
        assert!(!rejects(name, "ilium.notes"), "{name}");
    }
}

/// REQ: SEC-381
#[test]
fn sec_381_an_id_that_merely_resembles_ilium_does_not_unlock_the_names() {
    assert!(rejects("Ilium Tools", "iliumx.notes"));
    assert!(rejects("Ilium Tools", "example.ilium"));
}

/// REQ: SEC-381
#[test]
fn sec_381_the_install_check_refuses_with_the_display_name_code() {
    let rig = t3(&[]);
    assert_eq!(
        rig.host
            .install_check_with_name("example.demo", Tier::T3, "Ilium Tools"),
        Err("manifest.display-name".to_owned())
    );
    assert_eq!(
        rig.host
            .install_check_with_name("example.demo", Tier::T3, "Demo Tools"),
        Ok(())
    );
}

proptest! {
    // REQ: SEC-381
    #[test]
    fn sec_381_names_are_accepted_exactly_when_the_length_is_3_to_40(length in 0usize..60) {
        let name = format!("M{}", "a".repeat(length.saturating_sub(1)));
        let name: String = if length == 0 { String::new() } else { name };
        prop_assert_eq!(rejects(&name, "example.demo"), !(3..=40).contains(&length));
    }

    // REQ: SEC-381
    #[test]
    fn sec_381_a_reserved_prefix_is_always_refused_for_third_parties(suffix in "[A-Za-z ]{0,20}", reserved in prop::sample::select(vec!["Ilium", "Kernel"])) {
        let name = format!("{reserved}{suffix}");
        if (3..=40).contains(&name.chars().count()) {
            prop_assert!(rejects(&name, "example.demo"));
        }
    }
}
