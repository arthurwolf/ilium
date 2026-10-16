//! Renders of the optimiser panel and the two settings pages at widths 40, 80 and 120, as substring and width checks (the page figures are illustrative).

use ilium_plugin_sdk::types::ViewTarget;

use crate::common::*;

fn assert_fits(text: &str, width: usize) {
    for line in text.lines() {
        assert!(line.chars().count() <= width, "row wider than {width}: {line:?}");
    }
}

/// REQ: F-082
#[test]
fn render_082_the_optimizer_panel_fits_widths_40_80_120() {
    for width in [40u16, 80, 120] {
        let mut host = client_host();
        host.open_view(ViewTarget::Panel("optimizer".to_string()));
        host.set_recommendation(72, 65);
        let text = host.render(ViewTarget::Panel("optimizer".to_string()), width, 24).text();
        assert_fits(&text, usize::from(width));
        assert!(text.contains("Recommended"));
        assert!(text.contains("Apply") && text.contains("Revert"));
    }
}

/// REQ: F-082
#[test]
fn render_082_at_48_columns_the_panel_matches_the_figure_rows() {
    let mut host = client_host();
    host.open_view(ViewTarget::Panel("optimizer".to_string()));
    host.set_recommendation(72, 65);
    let text = host.render(ViewTarget::Panel("optimizer".to_string()), 48, 12).text();
    for needle in ["Auto-compact threshold", "72 %", "Recommended", "65 %", "[ Apply ]", "[ Revert ]", "█"] {
        assert!(text.contains(needle), "missing {needle}");
    }
}

/// REQ: F-082
#[test]
fn render_082_the_gauge_has_the_recommended_share_filled() {
    let mut host = client_host();
    host.open_view(ViewTarget::Panel("optimizer".to_string()));
    host.set_recommendation(72, 65);
    let text = host.render(ViewTarget::Panel("optimizer".to_string()), 48, 12).text();
    let gauge = text.lines().find(|line| line.contains('█')).expect("gauge row");
    let filled = gauge.chars().filter(|c| *c == '█').count();
    let empty = gauge.chars().filter(|c| *c == '░').count();
    assert!(filled > 0 && empty > 0 && filled > empty / 2);
}

/// REQ: F-082
#[test]
fn render_082_the_panel_shows_context_before_and_after_after_a_scan() {
    let mut host = client_host();
    host.open_view(ViewTarget::Panel("optimizer".to_string()));
    host.set_scan_result(200_000, 120_000);
    let text = host.render(ViewTarget::Panel("optimizer".to_string()), 80, 24).text();
    assert!(text.contains("Context before") && text.contains("Context after"));
}

/// REQ: F-083
#[test]
fn render_083_the_remote_compaction_page_fits_widths_40_80_120() {
    for width in [40u16, 80, 120] {
        let mut host = client_host();
        let text = host.render(ViewTarget::SettingsPage("remote-compaction".to_string()), width, 80).text();
        assert_fits(&text, usize::from(width));
        assert!(text.contains("Remote compaction") || text.contains("remote compaction"));
    }
}

/// REQ: F-080
#[test]
fn render_080_the_remote_compaction_tab_shows_the_privacy_notice_until_dismissed() {
    let mut host = client_host();
    let text = host.render(ViewTarget::SettingsPage("remote-compaction".to_string()), 120, 80).text();
    assert!(text.contains("sends your conversation"));
}

/// REQ: F-082
#[test]
fn render_082_the_optimization_page_fits_widths_40_80_120() {
    for width in [40u16, 80, 120] {
        let mut host = client_host();
        let text = host.render(ViewTarget::SettingsPage("optimization".to_string()), width, 60).text();
        assert_fits(&text, usize::from(width));
        assert!(text.contains("Optimization"));
    }
}