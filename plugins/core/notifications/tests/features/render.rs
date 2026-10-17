//! What the user sees: the two figures of the page (a toast and the Sounds tab), pinned as golden
//! files, and their structure. Live renders of the client side are in the features_gaps target.

use crate::common::*;

fn golden(name: &str) -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/features/golden")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"))
}

/// The `pre.term` blocks of the page, HTML-unescaped.
fn page_figures() -> Vec<String> {
    let page = page_text();
    let mut figures = Vec::new();
    let mut rest = page.as_str();
    while let Some(start) = rest.find("<pre class=\"term\">") {
        let body = &rest[start + "<pre class=\"term\">".len()..];
        let end = body.find("</pre>").expect("figure end");
        figures.push(
            body[..end]
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&"),
        );
        rest = &body[end..];
    }
    figures
}

/// REQ: F-047
#[test]
fn render_047_the_page_has_two_figures() {
    assert_eq!(page_figures().len(), 2);
}

/// REQ: F-047
#[test]
fn render_047_the_toast_golden_equals_the_first_page_figure() {
    assert_eq!(
        golden("toast_48.txt").trim_end(),
        page_figures()[0].trim_end()
    );
}

/// REQ: F-047
#[test]
fn render_047_the_sounds_golden_equals_the_second_page_figure() {
    assert_eq!(
        golden("sounds_48.txt").trim_end(),
        page_figures()[1].trim_end()
    );
}

/// REQ: F-047
#[test]
fn render_047_every_golden_row_is_48_cells_wide() {
    for name in ["toast_48.txt", "sounds_48.txt"] {
        for line in golden(name).lines() {
            assert_eq!(line.chars().count(), 48, "{name}: {line}");
        }
    }
}

/// REQ: F-047
#[test]
fn render_047_the_sounds_figure_lists_the_three_sound_rows_and_two_desktop_rows() {
    let figure = golden("sounds_48.txt");
    for label in [
        "Approval needed",
        "Agent finished",
        "Task done",
        "Desktop notifications",
        "Merge task alerts within",
    ] {
        assert!(figure.contains(label), "{label}");
    }
}

/// REQ: F-047
#[test]
fn render_047_the_sounds_figure_connects_label_and_value_with_dotted_leaders() {
    for line in golden("sounds_48.txt").lines().skip(1) {
        if line.contains('…') {
            assert!(line.contains(" … ") || line.contains("… "), "{line}");
            assert!(!line.contains("..."), "{line}");
        }
    }
}

/// REQ: F-047
#[test]
fn render_047_the_sounds_figure_shows_the_sound_choices_in_angle_quotes() {
    let figure = golden("sounds_48.txt");
    assert!(figure.contains("‹ ask ›"));
    assert!(figure.contains("‹ finished ›"));
    assert!(figure.contains("‹ none ›"));
    assert!(figure.contains("‹ On ›"));
}

/// REQ: F-047
#[test]
fn render_047_the_sounds_figure_shows_the_default_merge_window_of_30_seconds() {
    assert!(golden("sounds_48.txt").contains("Merge task alerts within …………………………………… 30 s"));
}

/// REQ: F-048
#[test]
fn render_048_the_toast_figure_has_a_title_notch_a_body_two_actions_and_a_gauge() {
    let figure = golden("toast_48.txt");
    assert!(figure.starts_with("╭─ Build finished ─"));
    assert!(figure.contains("Pane claude-api reached done."));
    assert!(figure.contains("Open pane · Dismiss"));
    assert!(figure.contains('━') && figure.contains('─'));
    assert!(figure.trim_end().ends_with('╯'));
}

/// REQ: F-048
#[test]
fn render_048_the_toast_gauge_is_about_70_percent_through() {
    let figure = golden("toast_48.txt");
    let gauge = figure
        .lines()
        .find(|line| line.contains('━'))
        .expect("gauge row");
    let spent = gauge.chars().filter(|c| *c == '━').count() as f64;
    let total = gauge.chars().filter(|c| *c == '━' || *c == '─').count() as f64;
    let share = spent / total;
    assert!((0.6..=0.8).contains(&share), "{share}");
}
