//! Renders of the pane frame (client side): header model text, prompt banner and the toolbar by agent signature.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{AgentActivity, PluginSide, ViewTarget};
use serde_json::json;

use crate::common::*;

/// Wide enough that no toolbar button is dropped for lack of room.
const WIDE: u16 = 240;

fn pane_view(pane: u64) -> ViewTarget {
    ViewTarget::Pane(ilium_plugin_sdk::types::NodeId(pane))
}

fn host_with_pane(signature: &str) -> (FakeHost, u64) {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, signature, signature, AgentActivity::Idle);
    (host, pane)
}

fn labels_for(signature: &str) -> String {
    let (mut host, pane) = host_with_pane(signature);
    host.render(pane_view(pane), WIDE, 6).text()
}

/// REQ: F-035
#[test]
fn f_035_header_shows_the_model_and_context_text_of_the_page_figure() {
    let (mut host, pane) = host_with_pane("claude");
    let payload = format!("{pane}|opus · 41% context");
    host.deliver_one(message_from(
        PluginSide::Server,
        "model",
        payload.as_bytes(),
    ));
    let screen = host.render(pane_view(pane), 48, 7);
    assert!(
        screen.row(0).contains("opus · 41% context"),
        "{}",
        screen.row(0)
    );
}

/// REQ: F-033
#[test]
fn f_033_banner_is_the_first_row_inside_the_frame() {
    let (mut host, pane) = host_with_pane("claude");
    host.deliver_one(user_line(pane, "refactor the parser to use a lexer"));
    let screen = host.render(pane_view(pane), 48, 7);
    assert_eq!(
        screen.row(1),
        "│ > refactor the parser to use a lexer         │"
    );
}

/// REQ: F-033
#[test]
fn f_033_page_figure_first_two_rows_match_at_48_columns() {
    let (mut host, pane) = host_with_pane("claude");
    host.deliver_one(message_from(
        PluginSide::Server,
        "model",
        format!("{pane}|opus · 41% context").as_bytes(),
    ));
    host.deliver_one(user_line(pane, "refactor the parser to use a lexer"));
    let screen = host.render(pane_view(pane), 48, 7);
    let figure = term_blocks(&page_text())[0].clone();
    let lines: Vec<&str> = figure.lines().collect();
    assert_eq!(screen.row(1), lines[1]);
    assert!(screen.row(0).starts_with("╭ ") && screen.row(0).ends_with('╮'));
}

/// REQ: F-033
#[test]
fn f_033_banner_is_gone_after_the_agent_stops() {
    let (mut host, pane) = host_with_pane("claude");
    host.deliver_one(user_line(pane, "refactor the parser"));
    host.deliver_one(activity(pane, AgentActivity::Working, AgentActivity::Done));
    let text = host.render(pane_view(pane), 80, 8).text();
    assert!(!text.contains("> refactor the parser"));
    assert!(text.contains("Stopped. Last prompt: refactor the parser"));
}

/// REQ: F-033
#[test]
fn f_033_recover_hint_sits_in_the_footer_row() {
    let (mut host, pane) = host_with_pane("claude");
    host.deliver_one(user_line(pane, "refactor the parser"));
    host.deliver_one(activity(pane, AgentActivity::Working, AgentActivity::Done));
    let screen = host.render(pane_view(pane), 80, 8);
    let (row, _) = screen.find("Stopped. Last prompt:").expect("hint");
    assert!(row >= 6, "hint at row {row} is not in the lower rows");
}

/// REQ: F-033
#[test]
fn f_033_default_budget_keeps_the_first_two_and_last_two_rows_of_a_long_prompt() {
    let (mut host, pane) = host_with_pane("claude");
    host.deliver_one(user_line(pane, "row1\nrow2\nrow3\nrow4\nrow5\nrow6"));
    let text = host.render(pane_view(pane), 80, 10).text();
    assert!(
        text.contains("> row1")
            && text.contains("row2")
            && text.contains("row5")
            && text.contains("row6")
    );
    assert!(!text.contains("row3") && !text.contains("row4"), "{text}");
}

/// REQ: F-033
#[test]
fn f_033_a_prompt_within_the_budget_keeps_every_row() {
    let (mut host, pane) = host_with_pane("claude");
    host.deliver_one(user_line(pane, "row1\nrow2\nrow3\nrow4"));
    let text = host.render(pane_view(pane), 80, 10).text();
    for row in ["row1", "row2", "row3", "row4"] {
        assert!(text.contains(row), "{row} missing in\n{text}");
    }
}

/// REQ: F-033
#[test]
fn f_033_a_budget_of_one_keeps_only_the_first_row() {
    let (mut host, pane) = host_with_pane("claude");
    host.settings()
        .set("ui.last_prompt_max_lines", json!(1))
        .expect("accepted");
    host.deliver_one(user_line(pane, "row1\nrow2\nrow3"));
    let text = host.render(pane_view(pane), 80, 10).text();
    assert!(text.contains("> row1"));
    assert!(!text.contains("row2") && !text.contains("row3"), "{text}");
}

/// REQ: F-033
#[test]
fn f_033_an_odd_budget_gives_the_extra_row_to_the_head() {
    let (mut host, pane) = host_with_pane("claude");
    host.settings()
        .set("ui.last_prompt_max_lines", json!(3))
        .expect("accepted");
    host.deliver_one(user_line(pane, "row1\nrow2\nrow3\nrow4\nrow5\nrow6"));
    let text = host.render(pane_view(pane), 80, 10).text();
    assert!(text.contains("> row1") && text.contains("row2") && text.contains("row6"));
    assert!(
        !text.contains("row3") && !text.contains("row4") && !text.contains("row5"),
        "{text}"
    );
}

/// REQ: F-033
#[test]
fn f_033_a_long_single_line_wraps_instead_of_being_clipped() {
    let (mut host, pane) = host_with_pane("claude");
    host.deliver_one(user_line(
        pane,
        "alpha beta gamma delta epsilon zeta eta theta iota kappa",
    ));
    let screen = host.render(pane_view(pane), 30, 10);
    assert!(screen.text().contains("kappa"), "{}", screen.text());
}

/// REQ: F-033
#[test]
fn f_033_the_banner_never_widens_the_frame() {
    for width in [40u16, 80, 120] {
        let (mut host, pane) = host_with_pane("claude");
        host.deliver_one(user_line(pane, &"w".repeat(300)));
        let screen = host.render(pane_view(pane), width, 10);
        for line in screen.text().lines() {
            assert!(
                line.chars().count() <= usize::from(width),
                "row wider than {width}: {line}"
            );
        }
    }
}

/// REQ: F-033
#[test]
fn f_033_disabling_the_banner_removes_it_from_the_frame() {
    let (mut host, pane) = host_with_pane("claude");
    host.deliver_one(user_line(pane, "visible"));
    host.settings()
        .set("ui.last_prompt_enabled", json!(false))
        .expect("accepted");
    assert!(!host
        .render(pane_view(pane), 80, 8)
        .text()
        .contains("> visible"));
}

/// REQ: F-035
#[test]
fn f_035_claude_toolbar_shows_the_claude_and_universal_buttons_with_labels() {
    let text = labels_for("claude");
    for label in [
        "Compact",
        "Clear",
        "Configure",
        "Stop",
        "Copy screen",
        "Smart Copy",
        "Copy last message",
        "Effort",
        "Model",
        "Exit",
        "Text selection",
    ] {
        assert!(text.contains(label), "{label} missing in\n{text}");
    }
}

/// REQ: F-035
#[test]
fn f_035_codex_toolbar_shows_the_same_buttons_as_claude() {
    let text = labels_for("codex");
    for label in [
        "Compact",
        "Clear",
        "Configure",
        "Stop",
        "Copy screen",
        "Smart Copy",
        "Copy last message",
        "Effort",
        "Model",
        "Exit",
        "Text selection",
    ] {
        assert!(text.contains(label), "{label} missing in\n{text}");
    }
}

/// REQ: F-035
#[test]
fn f_035_an_unknown_agent_gets_only_the_buttons_for_all_agents() {
    let text = labels_for("someagent");
    for label in [
        "Stop",
        "Copy screen",
        "Smart Copy",
        "Exit",
        "Text selection",
    ] {
        assert!(text.contains(label), "{label} missing in\n{text}");
    }
    for label in [
        "Compact",
        "Clear",
        "Configure",
        "Copy last message",
        "Effort",
        "Model",
    ] {
        assert!(!text.contains(label), "{label} must be hidden in\n{text}");
    }
}

/// REQ: F-035
#[test]
fn f_035_antigravity_hides_compact_and_fast_mode() {
    let text = labels_for("antigravity");
    assert!(!text.contains("Compact"), "{text}");
    assert!(!text.contains("Fast mode"), "{text}");
    assert!(text.contains("Stop"));
}

/// REQ: F-035
#[test]
fn f_035_labels_follow_the_row_order_of_the_action_table() {
    let text = labels_for("claude");
    let order = [
        "Compact",
        "Clear",
        "Configure",
        "Stop",
        "Copy screen",
        "Smart Copy",
        "Copy last message",
        "Effort",
        "Model",
        "Exit",
        "Text selection",
    ];
    let positions: Vec<usize> = order
        .iter()
        .map(|label| text.find(label).unwrap_or(usize::MAX))
        .collect();
    assert!(
        positions.iter().all(|position| *position != usize::MAX),
        "{text}"
    );
    let mut sorted = positions.clone();
    sorted.sort_unstable();
    assert_eq!(positions, sorted, "{text}");
}

/// REQ: F-035
#[test]
fn f_035_without_labels_the_toolbar_shows_icons_only() {
    let (mut host, pane) = host_with_pane("claude");
    host.settings()
        .set("ui.show_toolbar_labels", json!(false))
        .expect("accepted");
    let text = host.render(pane_view(pane), WIDE, 6).text();
    for label in ["Compact", "Clear", "Copy screen", "Smart Copy"] {
        assert!(!text.contains(label), "{label} must be hidden in\n{text}");
    }
}

/// REQ: F-035
#[test]
fn f_035_disabling_the_toolbar_hides_every_button() {
    let (mut host, pane) = host_with_pane("claude");
    host.settings()
        .set("ui.agent_toolbar_enabled", json!(false))
        .expect("accepted");
    let text = host.render(pane_view(pane), WIDE, 6).text();
    for label in ["Compact", "Stop", "Exit", "Text selection"] {
        assert!(!text.contains(label), "{label} must be hidden in\n{text}");
    }
}

/// REQ: F-035
#[test]
fn f_035_reenabling_the_toolbar_brings_the_buttons_back() {
    let (mut host, pane) = host_with_pane("claude");
    host.settings()
        .set("ui.agent_toolbar_enabled", json!(false))
        .expect("accepted");
    host.settings()
        .set("ui.agent_toolbar_enabled", json!(true))
        .expect("accepted");
    assert!(host
        .render(pane_view(pane), WIDE, 6)
        .text()
        .contains("Stop"));
}

/// REQ: F-035
#[test]
fn f_035_a_pane_without_an_agent_has_no_toolbar() {
    let mut host = client_host();
    let shell = host.tree().add(ilium_plugin_sdk::testing::TreeNode {
        id: 0,
        parent: None,
        name: "shell".into(),
        class: ilium_plugin_sdk::types::NodeClass::Pane,
        content_kind: Some("ilium.terminal".into()),
        agent: None,
    });
    let text = host.render(pane_view(shell), WIDE, 6).text();
    assert!(
        !text.contains("Stop") && !text.contains("Compact"),
        "{text}"
    );
}

/// REQ: F-035
#[test]
fn f_035_the_toolbar_fits_the_width_at_40_80_and_120_columns() {
    for width in [40u16, 80, 120] {
        let (mut host, pane) = host_with_pane("claude");
        let screen = host.render(pane_view(pane), width, 8);
        for line in screen.text().lines() {
            assert!(
                line.chars().count() <= usize::from(width),
                "row wider than {width}: {line}"
            );
        }
    }
}

/// REQ: F-035
#[test]
fn f_035_a_narrow_toolbar_keeps_the_exit_button() {
    let (mut host, pane) = host_with_pane("claude");
    let text = host.render(pane_view(pane), 40, 8).text();
    assert!(
        text.contains("Exit") || text.contains("×") || text.contains("✕"),
        "{text}"
    );
}

/// REQ: F-035
#[test]
fn f_035_clicking_the_compact_label_invokes_the_compact_command() {
    let (mut host, pane) = host_with_pane("claude");
    let screen = host.render(pane_view(pane), WIDE, 6);
    let (row, column) = screen.find("Compact").expect("Compact button");
    assert!(host.click(pane_view(pane), column, row));
    let sent = messages(&host.submitted());
    assert!(
        sent.iter()
            .any(|message| message.2 == "compact" && message.3 == pane.to_string().into_bytes()),
        "{sent:?}"
    );
}

/// REQ: F-035
#[test]
fn f_035_a_click_outside_the_toolbar_row_hits_no_button() {
    let (mut host, pane) = host_with_pane("claude");
    assert!(!host.click(pane_view(pane), 0, 5));
    assert!(messages(&host.submitted()).is_empty());
}
