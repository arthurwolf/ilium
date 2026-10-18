//! Review, trust, replace and first-start dialogs: USR-014, USR-022, USR-043 (prompt), USR-082,
//! USR-120, USR-122, USR-214 (dialog row).

use crate::support::*;

const FIVE: [&str; 5] = [
    "tree.read",
    "panes.read",
    "notify.toast",
    "terminal.write",
    "clipboard.read",
];

// ---- USR-022 ----

/// REQ: USR-022
#[test]
fn usr_022_high_rows_start_unchecked_and_normal_rows_start_checked() {
    let mut rig = ScreensRig::new(80, 30);
    rig.open_review_dialog("com.example.mixed", &FIVE);
    let screen = rig.screen_text();
    for normal in ["tree.read", "panes.read", "notify.toast"] {
        assert!(
            row_with(&screen, normal).contains("[x]"),
            "{normal}: {screen}"
        );
    }
    for high in ["terminal.write", "clipboard.read"] {
        let row = row_with(&screen, high);
        assert!(row.contains("[ ]") && row.contains("HIGH"), "{high}: {row}");
    }
}

/// REQ: USR-022
#[test]
fn usr_022_a_package_with_only_normal_rows_has_no_unchecked_row() {
    let mut rig = ScreensRig::new(80, 30);
    rig.open_review_dialog("com.example.mild", &["tree.read", "panes.read"]);
    assert!(!rig.screen_text().contains("[ ]"));
}

/// REQ: USR-022
#[test]
fn usr_022_a_package_with_only_high_rows_has_no_checked_row() {
    let mut rig = ScreensRig::new(80, 30);
    rig.open_review_dialog("com.example.wild", &["terminal.write", "clipboard.read"]);
    assert!(!rig.screen_text().contains("[x]"));
}

// ---- USR-082 ----

/// REQ: USR-082
#[test]
fn usr_082_the_dialog_shows_the_grant_string_with_its_argument() {
    let mut rig = ScreensRig::new(80, 30);
    rig.open_review_dialog("com.example.web", &["http:api.example.com"]);
    let row = row_with(&rig.screen_text(), "http:api.example.com");
    assert!(row.contains("Talk to one named website or server"), "{row}");
    assert!(
        row.contains("http:api.example.com")
            && !row.contains("http:api.example.com*")
            && !row.contains("HIGH"),
        "{row}"
    );
}

/// REQ: USR-082
#[test]
fn usr_082_programs_and_places_are_shown_exactly_as_matched() {
    let mut rig = ScreensRig::new(100, 30);
    rig.open_review_dialog(
        "com.example.args",
        &["process.run:git", "fs.read:project", "http:*.example.com"],
    );
    let screen = rig.screen_text();
    for shown in ["process.run:git", "fs.read:project", "http:*.example.com"] {
        assert!(has_row_with(&screen, shown), "{shown}");
    }
}

/// REQ: USR-082
#[test]
fn usr_082_a_long_argument_is_never_cut_silently() {
    let long = "http:very-long-subdomain-name.example-organisation.example.com";
    let mut rig = ScreensRig::new(48, 30);
    rig.open_review_dialog("com.example.long", &[long]);
    let screen = rig.screen_text();
    let joined: String = screen
        .lines()
        .map(|line| line.trim_matches(|c: char| c == '|' || c == ' '))
        .collect::<Vec<_>>()
        .join("");
    assert!(
        joined.contains(long) || rig.scrolled_text().contains(long),
        "the full string is reachable"
    );
}

// ---- USR-014 ----

/// REQ: USR-014
#[test]
fn usr_014_not_now_three_times_then_the_fourth_start_shows_no_prompt() {
    let mut rig = ScreensRig::new(80, 30);
    rig.set_bundled_first_start("ilium.core.voice", &["audio.capture"]);
    for start in 1..=3 {
        rig.start_client();
        assert!(
            rig.screen_text().contains("First start"),
            "start {start} shows the prompt"
        );
        rig.press_escape();
        rig.stop_client();
    }
    rig.start_client();
    assert!(
        !rig.screen_text().contains("First start"),
        "start 4 shows no prompt"
    );
}

/// REQ: USR-014
#[test]
fn usr_014_at_most_one_prompt_per_start_and_other_plugins_are_not_blocked() {
    let mut rig = ScreensRig::new(80, 30);
    rig.set_bundled_first_start("ilium.core.voice", &["audio.capture"]);
    rig.set_bundled_first_start("ilium.core.location", &["location.observer"]);
    rig.start_client();
    assert_eq!(rig.screen_text().matches("First start").count(), 1);
    assert!(rig.plugin_is_running("ilium.core.tree"));
    assert!(
        rig.plugin_is_running("ilium.core.location"),
        "the plugin behind the prompt is not blocked either"
    );
}

/// REQ: USR-014
#[test]
fn usr_014_allow_selected_ends_the_prompts_and_not_now_three_times_stops_them() {
    let mut allowed = ScreensRig::new(80, 30);
    allowed.set_bundled_first_start("ilium.core.voice", &["audio.capture"]);
    allowed.start_client();
    allowed.press_enter();
    allowed.stop_client();
    allowed.start_client();
    assert!(!allowed.screen_text().contains("First start"));
    let mut declined = ScreensRig::new(80, 30);
    declined.set_bundled_first_start("ilium.core.voice", &["audio.capture"]);
    for _ in 0..3 {
        declined.start_client();
        declined.press_escape();
        declined.stop_client();
    }
    declined.start_client();
    assert!(
        !declined.screen_text().contains("First start"),
        "the third Not now answer is the last ask"
    );
}

// ---- USR-120 / USR-122 ----

/// REQ: USR-120
#[test]
fn usr_120_trust_prompt_shows_the_full_fingerprint_with_the_short_form_beside_it() {
    let mut rig = ScreensRig::new(80, 30);
    let full = "3f2a".to_string() + &"0".repeat(56) + "9c01";
    rig.open_trust_prompt("Example Publisher", &full);
    let screen = rig.screen_text();
    assert!(screen.replace(' ', "").contains(&full), "{screen}");
    assert!(
        screen.contains("3f2a0000"),
        "short form beside the full form: {screen}"
    );
    assert!(
        !screen.contains("3f2a\u{2026}9c01"),
        "no 4+4 form: {screen}"
    );
}

/// REQ: USR-120
#[test]
fn usr_120_the_prompt_has_no_default_trust_answer() {
    let mut rig = ScreensRig::new(80, 30);
    rig.open_trust_prompt(
        "Example Publisher",
        &("ab12".to_string() + &"0".repeat(56) + "ef34"),
    );
    rig.press_enter();
    assert!(
        !rig.trust_answers().contains(&true),
        "Enter alone does not trust the key"
    );
}

/// REQ: USR-122
#[test]
fn usr_122_update_prompt_names_the_old_and_the_new_fingerprint() {
    let mut rig = ScreensRig::new(80, 30);
    let old = "1111".to_string() + &"0".repeat(56) + "aaaa";
    let new = "2222".to_string() + &"0".repeat(56) + "bbbb";
    rig.open_key_change_prompt("com.example.signed", &old, &new);
    let screen = rig.screen_text();
    assert!(screen.contains("11110000"), "{screen}");
    assert!(screen.contains("2222\u{2026}bbbb"), "{screen}");
}

// ---- USR-043 ----

/// REQ: USR-043
#[test]
fn usr_043_replace_prompt_names_the_core_plugin_and_offers_install_without() {
    let mut rig = ScreensRig::new(80, 30);
    rig.open_replace_prompt("com.example.better", "ilium.core.editor");
    let screen = rig.screen_text();
    assert!(screen.contains("ilium.core.editor"));
    assert!(screen.contains("Install without"));
}

/// REQ: USR-043
#[test]
fn usr_043_the_default_choice_is_not_to_replace() {
    let mut rig = ScreensRig::new(80, 30);
    rig.open_replace_prompt("com.example.better", "ilium.core.editor");
    rig.press_enter();
    assert_eq!(rig.replace_answers(), vec![false]);
}

// ---- USR-214 ----

/// REQ: USR-214
#[test]
fn usr_214_a_denied_row_is_locked_with_the_administrator_text_in_the_dialog() {
    let mut rig = ScreensRig::new(80, 30);
    rig.set_policy_denied_grants(&["clipboard.read"]);
    rig.open_review_dialog("com.example.clip", &["clipboard.read", "tree.read"]);
    let row = row_with(&rig.screen_text(), "clipboard.read");
    assert!(row.contains("Blocked by your administrator"), "{row}");
    assert!(!row.contains("[x]"));
    rig.move_selection_to("clipboard.read");
    rig.press_space();
    assert!(
        !row_with(&rig.screen_text(), "clipboard.read").contains("[x]"),
        "a locked row cannot be checked"
    );
}

/// REQ: USR-214
#[test]
fn usr_214_the_capabilities_tab_shows_the_same_text() {
    let mut rig = ScreensRig::new(80, 30);
    rig.set_policy_denied_grants(&["clipboard.read"]);
    rig.install_plugin_with_grants("com.example.clip", &["clipboard.read", "tree.read"]);
    rig.open_capabilities_tab("com.example.clip");
    let screen = rig.screen_text();
    assert!(row_with(&screen, "clipboard.read").contains("Blocked by your administrator"));
    assert!(!row_with(&screen, "tree.read").contains("Blocked by your administrator"));
}
