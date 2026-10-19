//! The Setup tab of the kernel Settings screen (page "Setup tab: statuses, rows and files"): a check-list of
//! `block|target` rows with status badges, the Global file path field, the confirm dialog and the disabled notice.

use ilium_plugin_sdk::testing::FakeHost;

use crate::common::*;

const SURFACE: &str = "setup";

fn render(host: &mut FakeHost, width: u16) -> String {
    host.render_kernel_tab(SURFACE, width, 30).text()
}

fn lines_fit(text: &str, width: u16) {
    for line in text.lines() {
        assert!(
            line.chars().count() <= usize::from(width),
            "{line:?} wider than {width}"
        );
    }
}

/// REQ: F-063, F-037
#[test]
fn tab_037_renders_inside_forty_eighty_and_one_hundred_twenty_columns() {
    for width in [40u16, 80, 120] {
        let mut host = project_host();
        let text = render(&mut host, width);
        assert!(!text.trim().is_empty(), "{width}");
        lines_fit(&text, width);
        assert!(
            text.contains("Chatroom") && text.contains("Progress"),
            "{width}\n{text}"
        );
    }
}

/// REQ: F-063, F-037
#[test]
fn tab_037_golden_at_forty_columns() {
    let mut host = project_host();
    assert_golden("setup_tab_40", &render(&mut host, 40));
}

/// REQ: F-063, F-037
#[test]
fn tab_037_golden_at_eighty_columns() {
    let mut host = project_host();
    assert_golden("setup_tab_80", &render(&mut host, 80));
}

/// REQ: F-063, F-037
#[test]
fn tab_037_golden_at_one_hundred_twenty_columns() {
    let mut host = project_host();
    assert_golden("setup_tab_120", &render(&mut host, 120));
}

/// REQ: F-063, F-037
#[test]
fn tab_037_golden_matches_the_forty_eight_column_art_of_the_page() {
    let mut host = project_host();
    assert_golden("setup_tab_48", &render(&mut host, 48));
}

/// REQ: F-037
#[test]
fn tab_037_shows_the_global_file_field_with_the_default_path() {
    let mut host = project_host();
    let text = render(&mut host, 80);
    assert!(text.contains("Global file"), "{text}");
    assert!(text.contains("~/.claude/CLAUDE.md"), "{text}");
}

/// REQ: F-037
#[test]
fn tab_037_has_a_section_per_block_with_a_global_row_and_a_project_row() {
    let mut host = project_host();
    let text = render(&mut host, 80);
    assert_eq!(rows_with(&text, "Global").len(), 2, "{text}");
    assert_eq!(rows_with(&text, "Project ilium").len(), 2, "{text}");
}

/// REQ: F-037
#[test]
fn tab_037_rows_show_the_status_words_as_badges() {
    let mut host = project_host();
    put(&mut host, claude_file(), &old_progress_block(12));
    let text = render(&mut host, 80);
    assert!(text.contains("Ilium update required"), "{text}");
    assert!(text.contains("Not set up"), "{text}");
}

/// REQ: F-037
#[test]
fn tab_037_a_set_up_row_reads_set_up_and_a_newer_row_reads_newer() {
    let mut host = project_host();
    install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("install");
    put(&mut host, codex_file(), &old_progress_block(99));
    let text = render(&mut host, 80);
    assert!(text.contains("Set up by Ilium"), "{text}");
    assert!(text.contains("Newer"), "{text}");
}

/// REQ: F-037
#[test]
fn tab_037_look_alike_text_reads_instruction_detected() {
    let mut host = project_host();
    put(&mut host, claude_file(), PROGRESS_LOOK_ALIKE);
    assert!(render(&mut host, 80).contains("Instruction detected"));
}

/// REQ: F-037
#[test]
fn tab_037_without_an_open_project_there_are_no_project_rows() {
    let mut host = server_host();
    assert!(!render(&mut host, 80).contains("Project ilium"));
}

/// REQ: F-037
#[test]
fn tab_037_the_hint_line_names_enter_and_r() {
    let mut host = project_host();
    assert!(render(&mut host, 120).contains("Enter refreshes a row. R restores path."));
}

/// REQ: F-037
#[test]
fn tab_037_the_tab_is_a_check_list_with_row_ids_of_the_form_block_bar_target() {
    let host = project_host();
    let ids = host.kernel_tab_row_ids(SURFACE);
    assert!(
        ids.contains(&format!("progress|{CLAUDE_GLOBAL}")),
        "{ids:?}"
    );
    assert!(
        ids.contains(&format!("chatroom|{CODEX_PROJECT}")),
        "{ids:?}"
    );
}

/// REQ: F-063
#[test]
fn tab_063_enter_on_a_row_opens_the_confirm_dialog_with_the_documented_texts() {
    let mut host = project_host();
    put(&mut host, codex_file(), &old_progress_block(12));
    host.press_row(SURFACE, &format!("progress|{CODEX_GLOBAL}"));
    let dialog = host.render_popup(80);
    for needle in [
        "Install Progress block",
        "File: ~/.codex/AGENTS.md",
        "Block version 12 will become version 13.",
        "Text outside the markers stays as it is.",
        "The limit is 512 KiB.",
        "[ Update ]",
        "[ Cancel ]",
    ] {
        assert!(dialog.contains(needle), "{needle:?} missing in\n{dialog}");
    }
}

/// REQ: F-063
#[test]
fn tab_063_the_dialog_shows_the_file_size_in_kib() {
    let mut host = project_host();
    put(&mut host, codex_file(), &"a".repeat(4096));
    host.press_row(SURFACE, &format!("progress|{CODEX_GLOBAL}"));
    assert!(host.render_popup(80).contains("The file is 4 KiB."));
}

/// REQ: F-063
#[test]
fn tab_063_confirming_the_dialog_installs_the_block() {
    let mut host = project_host();
    host.press_row(SURFACE, &format!("progress|{CLAUDE_GLOBAL}"));
    host.press_popup_button("Update");
    host.settle();
    assert!(get(&mut host, claude_file())
        .expect("file")
        .contains(PROGRESS_OPEN));
}

/// REQ: F-063
#[test]
fn tab_063_cancel_closes_the_dialog_and_writes_nothing() {
    let mut host = project_host();
    host.press_row(SURFACE, &format!("progress|{CLAUDE_GLOBAL}"));
    host.press_popup_button("Cancel");
    host.settle();
    assert_eq!(get(&mut host, claude_file()), None);
    assert!(host.render_popup(80).trim().is_empty());
}

/// REQ: F-063
#[test]
fn tab_063_there_is_no_on_off_toggle_on_a_row() {
    let mut host = project_host();
    assert!(!render(&mut host, 80).contains("[x]") && !render(&mut host, 80).contains("[ ] "));
}

/// REQ: F-063, F-037
#[test]
fn tab_063_enter_on_a_project_row_installs_into_the_project_file_after_confirming() {
    let mut host = project_host();
    host.press_row(SURFACE, &format!("chatroom|{CLAUDE_PROJECT}"));
    host.press_popup_button("Install");
    host.settle();
    assert!(get(&mut host, project_claude_file())
        .expect("file")
        .contains(CHATROOM_OPEN));
}

/// REQ: F-037
#[test]
fn tab_037_a_disabled_plugin_shows_the_notice_and_keeps_the_values() {
    let mut host = project_host();
    host.disable_plugin();
    let text = render(&mut host, 80);
    assert!(
        text.contains("Agent setup is disabled. Values are kept."),
        "{text}"
    );
    assert!(text.contains("~/.claude/CLAUDE.md"), "{text}");
}

/// REQ: F-037
#[test]
fn tab_037_a_disabled_plugin_does_not_write_on_enter() {
    let mut host = project_host();
    host.disable_plugin();
    host.press_row(SURFACE, &format!("progress|{CLAUDE_GLOBAL}"));
    host.settle();
    assert_eq!(get(&mut host, claude_file()), None);
}
