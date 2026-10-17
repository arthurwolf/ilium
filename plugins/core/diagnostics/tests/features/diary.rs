//! The plugin's own diary written with the host log: five levels, at most 200 lines for each second.

use crate::common::{build_report, session, PLUGIN_ID};

/// REQ: COR-050
#[test]
fn cor_050_building_a_report_writes_an_info_line_to_the_diary() {
    let mut session = session();
    build_report(&mut session, false).expect("report builds");
    let lines = session.server(PLUGIN_ID).logs();
    assert!(
        lines
            .iter()
            .any(|line| line.level == "info" && line.message.starts_with("report built")),
        "{lines:?}"
    );
}

/// REQ: COR-050
#[test]
fn cor_050_the_report_line_names_the_number_of_sections() {
    let mut session = session();
    build_report(&mut session, false).expect("report builds");
    let lines = session.server(PLUGIN_ID).logs();
    assert!(
        lines.iter().any(|line| line.message.contains("sections")),
        "{lines:?}"
    );
}

/// REQ: COR-050
#[test]
fn cor_050_a_section_without_a_reply_is_logged_at_warn_with_its_id() {
    let mut session = session();
    crate::common::stall_section(&mut session, "example.gpu", "gpu", "GPU");
    build_report(&mut session, false).expect("report builds");
    let lines = session.server(PLUGIN_ID).logs();
    assert!(
        lines
            .iter()
            .any(|line| line.level == "warn" && line.message == "section gpu: no reply in 2000 ms"),
        "{lines:?}"
    );
}

/// REQ: COR-050
#[test]
fn cor_050_the_diary_levels_are_only_the_five_documented_ones() {
    let mut session = session();
    build_report(&mut session, false).expect("report builds");
    for line in session.server(PLUGIN_ID).logs() {
        assert!(
            ["trace", "debug", "info", "warn", "error"].contains(&line.level.as_str()),
            "{line:?}"
        );
    }
}

/// REQ: COR-050
#[test]
fn cor_050_at_most_200_diary_lines_are_kept_for_one_second() {
    let mut session = session();
    for _ in 0..300 {
        build_report(&mut session, false).expect("report builds");
    }
    assert!(session.server(PLUGIN_ID).logs().len() <= 200);
}

/// REQ: COR-050
#[test]
fn cor_050_after_a_second_the_diary_accepts_lines_again() {
    let mut session = session();
    for _ in 0..300 {
        build_report(&mut session, false).expect("report builds");
    }
    let before = session.server(PLUGIN_ID).logs().len();
    session.server(PLUGIN_ID).advance_clock(1000);
    build_report(&mut session, false).expect("report builds");
    assert!(session.server(PLUGIN_ID).logs().len() > before);
}

/// REQ: COR-050
#[test]
fn cor_050_diary_lines_do_not_appear_in_the_ilium_log_file_tail() {
    let mut session = session();
    build_report(&mut session, false).expect("report builds");
    let panel = crate::common::warnings_panel(&mut session, 100, 20);
    assert!(!panel.contains("report built"), "{panel}");
}
