//! The client side Agents panel (page "Agents panel"): a table with Agent, State and Goal columns, a badge for
//! agents awaiting approval and a spinner label. Rendered at widths 40, 80 and 120 and at the page's 48 columns.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{AgentActivity, ViewTarget};

use crate::common::{add_agent_pane, client_host, page_text, workspace_root};

const PANEL: &str = "agent-detect.panel";

fn host_with_three_agents() -> FakeHost {
    let mut host = client_host();
    add_agent_pane(
        &mut host,
        "claude",
        "claude",
        AgentActivity::Working,
        Some("ship the parser port"),
    );
    add_agent_pane(&mut host, "codex", "codex", AgentActivity::Idle, None);
    add_agent_pane(
        &mut host,
        "gemini",
        "gemini",
        AgentActivity::AwaitingApproval,
        None,
    );
    host.settle();
    host
}

fn render_text(host: &mut FakeHost, columns: u16) -> String {
    host.render(ViewTarget::Panel(PANEL.to_string()), columns, 10)
        .text()
}

fn row_containing<'a>(text: &'a str, needle: &str) -> &'a str {
    text.lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no row with {needle:?} in\n{text}"))
}

/// The `<pre class="term">` figure that follows `caption_start`, as plain screen text.
fn page_art(caption: &str) -> String {
    let page = page_text();
    let caption_at = page.find(caption).expect("figcaption");
    let before = &page[..caption_at];
    let open = before.rfind("<pre class=\"term\">").expect("pre") + "<pre class=\"term\">".len();
    let close = before[open..].find("</pre>").expect("pre end");
    let mut out = String::new();
    let mut in_tag = false;
    for character in before[open..open + close].chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            other if !in_tag => out.push(other),
            _ => {}
        }
    }
    out.lines()
        .map(|line| line.trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// REQ: F-027, F-034
#[test]
fn feature_034_panel_header_shows_agent_and_state_columns_at_forty_columns() {
    let mut host = host_with_three_agents();
    let text = render_text(&mut host, 40);
    assert!(text.contains("Agent"), "{text}");
    assert!(text.contains("State"), "{text}");
}

/// REQ: F-027, F-029, F-034
#[test]
fn feature_034_panel_header_shows_all_three_columns_at_eighty_columns() {
    let mut host = host_with_three_agents();
    let text = render_text(&mut host, 80);
    let header = row_containing(&text, "Agent");
    assert!(
        header.contains("State") && header.contains("Goal"),
        "{header}"
    );
}

/// REQ: F-027, F-029, F-034
#[test]
fn feature_034_panel_header_shows_all_three_columns_at_one_hundred_twenty_columns() {
    let mut host = host_with_three_agents();
    let text = render_text(&mut host, 120);
    let header = row_containing(&text, "Agent");
    assert!(
        header.contains("State") && header.contains("Goal"),
        "{header}"
    );
}

/// REQ: F-029, F-034
#[test]
fn feature_034_the_working_agent_row_shows_state_and_goal_text() {
    let mut host = host_with_three_agents();
    let text = render_text(&mut host, 80);
    let row = row_containing(&text, "claude");
    assert!(row.contains("working"), "{row}");
    assert!(row.contains("ship the parser port"), "{row}");
}

/// REQ: F-027, F-034
#[test]
fn feature_034_the_working_state_is_prefixed_with_a_spinner_glyph() {
    let mut host = host_with_three_agents();
    let text = render_text(&mut host, 80);
    let row = row_containing(&text, "claude");
    assert!(row.contains("⠹ working"), "{row}");
}

/// REQ: F-027, F-034
#[test]
fn feature_034_an_idle_agent_row_shows_the_plain_state_word() {
    let mut host = host_with_three_agents();
    let text = render_text(&mut host, 80);
    let row = row_containing(&text, "codex");
    assert!(row.contains("idle"), "{row}");
    assert!(!row.contains("⠹"), "{row}");
}

/// REQ: F-027, F-034
#[test]
fn feature_034_an_agent_awaiting_approval_shows_the_word_approval() {
    let mut host = host_with_three_agents();
    let text = render_text(&mut host, 80);
    let row = row_containing(&text, "gemini");
    assert!(row.contains("approval"), "{row}");
}

/// REQ: F-034
#[test]
fn feature_034_panel_has_one_row_per_agent_in_tree_order() {
    let mut host = host_with_three_agents();
    let text = render_text(&mut host, 80);
    let claude = text.find("claude").expect("claude");
    let codex = text.find("codex").expect("codex");
    let gemini = text.find("gemini").expect("gemini");
    assert!(claude < codex && codex < gemini, "{text}");
}

/// REQ: F-034
#[test]
fn feature_034_no_rendered_row_is_wider_than_the_panel() {
    for columns in [40u16, 80, 120] {
        let mut host = host_with_three_agents();
        let screen = host.render(ViewTarget::Panel(PANEL.to_string()), columns, 10);
        for row in 0..screen.rows {
            assert!(
                screen.row(row).chars().count() <= usize::from(columns),
                "row {row} at {columns} columns"
            );
        }
    }
}

/// REQ: F-034
#[test]
fn feature_034_the_spinner_label_names_what_the_panel_does() {
    let mut host = host_with_three_agents();
    let text = render_text(&mut host, 80);
    assert!(text.contains("Watching agents"), "{text}");
}

/// REQ: F-027, F-034
#[test]
fn feature_034_the_waiting_badge_counts_agents_awaiting_approval() {
    let mut host = host_with_three_agents();
    let text = render_text(&mut host, 80);
    let first_row = text.lines().next().expect("a row");
    assert!(first_row.contains('1'), "badge row: {first_row}");
}

/// REQ: F-034
#[test]
fn feature_034_a_panel_without_agents_renders_the_header_and_no_agent_rows() {
    let mut host = client_host();
    let text = render_text(&mut host, 80);
    assert!(text.contains("Agent"), "{text}");
    assert!(!text.contains("claude"), "{text}");
}

/// REQ: F-034
#[test]
fn feature_034_a_state_change_updates_the_row_after_the_event() {
    let mut host = host_with_three_agents();
    let node = 2u64;
    host.tree().set_activity(node, AgentActivity::Working);
    host.settle();
    let text = render_text(&mut host, 80);
    let row = row_containing(&text, "codex");
    assert!(row.contains("working"), "{row}");
}

/// REQ: F-034
#[test]
fn feature_034_the_golden_agents_panel_equals_the_figure_of_the_page() {
    let golden = std::fs::read_to_string(
        workspace_root()
            .join("plugins/core/agent-detect/tests/features/golden/agents_panel_48.txt"),
    )
    .expect("golden file");
    let art = page_art("Rendered at 48 columns. Illustrative. One row per agent");
    ilium_plugin_sdk::testing::assert_golden("tests/features/golden/agents_panel_48.txt", &art);
    assert_eq!(golden.replace("\r\n", "\n").trim_end(), art.trim_end());
}
