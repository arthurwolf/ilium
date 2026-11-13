//! The page's figures, samples and links stay consistent with the plugin.

use crate::common::*;

/// REQ: F-082
#[test]
fn doc_page_the_optimizer_figure_shows_the_threshold_recommendation_and_both_buttons() {
    let blocks = term_blocks(&page_text());
    let figure = blocks
        .iter()
        .find(|block| block.contains("Auto-compact threshold"))
        .expect("optimizer figure");
    for needle in ["Recommended", "[ Apply ]", "[ Revert ]", "72 %", "65 %"] {
        assert!(figure.contains(needle), "{needle}");
    }
}

/// REQ: F-080
#[test]
fn doc_page_the_compacting_figure_is_present() {
    assert!(term_blocks(&page_text())
        .iter()
        .any(|block| block.contains("Compacting claude: refactor")));
}

/// REQ: F-115
#[test]
fn doc_page_the_convert_session_demo_and_open_external_text_are_present() {
    let page = page_text();
    assert!(page.contains("26-convert-session.gif"));
    assert!(page.contains("the client submits `open-external` with the converted session file"));
}

/// REQ: F-080
#[test]
fn doc_page_see_also_links_the_open_external_grant_and_chrome_intents() {
    let page = page_text();
    assert!(page.contains("/pages/capability-open-external/"));
    assert!(page.contains("/pages/api-intent-chrome/"));
}

/// REQ: F-082
#[test]
fn doc_page_the_widget_and_row_references_are_linked() {
    let page = page_text();
    for link in [
        "/pages/widgets-index/",
        "/pages/widgets-data-display/",
        "/pages/reference-settings-rows/",
        "/pages/widgets-input/",
    ] {
        assert!(page.contains(link), "{link}");
    }
}
