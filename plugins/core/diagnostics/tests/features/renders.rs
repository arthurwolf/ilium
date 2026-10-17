//! Golden renders: the 48-column report preview of the page and the warnings panel at 40, 80 and 120.

use ilium_plugin_sdk::testing::assert_golden;
use ilium_plugin_sdk::types::ViewTarget;

use crate::common::{log_line, provide_section, session, warnings_panel, write_log, PLUGIN_ID};

const LOG: &str = "log-2026-10-12 09-14-03.txt";

/// REQ: COR-050
#[test]
fn cor_050_report_preview_at_48_columns_matches_the_page_render() {
    let mut session = session();
    provide_section(
        &mut session,
        "example.gpu",
        "gpu",
        "gpu",
        "Renderer ........ software",
    );
    let screen = session
        .client(PLUGIN_ID)
        .render(ViewTarget::Popup("report-preview".to_string()), 48, 10)
        .text();
    assert_golden("tests/features/golden/report_preview_48.txt", &screen);
}

fn warnings_session() -> ilium_plugin_sdk::testing::FakeSession {
    let mut session = session();
    let text = [
        log_line("WARN", "section gpu: no reply in 2000 ms"),
        log_line(
            "ERROR",
            "model call failed after three retries with a long explanation",
        ),
    ]
    .concat();
    write_log(&mut session, LOG, text.as_bytes());
    session.pump().expect("settles");
    session
}

fn assert_rows_fit(text: &str, width: usize) {
    for row in text.lines() {
        assert!(
            row.chars().count() <= width,
            "row wider than {width}: {row:?}"
        );
    }
}

/// REQ: COR-050
#[test]
fn cor_050_warnings_panel_rows_fit_40_columns() {
    let mut session = warnings_session();
    let text = warnings_panel(&mut session, 40, 12);
    assert_rows_fit(&text, 40);
    assert!(text.contains("section gpu"), "{text}");
}

/// REQ: COR-050
#[test]
fn cor_050_warnings_panel_rows_fit_80_columns() {
    let mut session = warnings_session();
    let text = warnings_panel(&mut session, 80, 12);
    assert_rows_fit(&text, 80);
    assert!(text.contains("section gpu: no reply in 2000 ms"), "{text}");
}

/// REQ: COR-050
#[test]
fn cor_050_warnings_panel_rows_fit_120_columns() {
    let mut session = warnings_session();
    let text = warnings_panel(&mut session, 120, 12);
    assert_rows_fit(&text, 120);
    assert!(
        text.contains("model call failed after three retries with a long explanation"),
        "{text}"
    );
}

/// REQ: COR-050
#[test]
fn cor_050_warnings_panel_at_80_columns_matches_its_golden() {
    let mut session = warnings_session();
    let text = warnings_panel(&mut session, 80, 12);
    assert_golden("tests/features/golden/warnings_panel_80.txt", &text);
}
