//! F-069 Worktree status and branch line: the server side probes the worktrees every ten seconds
//! and publishes the `changed` topic; the client side renders one `leader-row` per worktree with a
//! branch icon, the dotted leader and the state (page sample "Status and branch line").

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{Event, PluginId, TopicPublishedEvent, ViewTarget};
use proptest::prelude::*;
use serde_json::{json, Value};

use crate::common::*;

const TREE_PANEL: &str = "worktrees";

fn branch_rows(host: &mut FakeHost, columns: u16) -> Vec<String> {
    let screen = host.render(ViewTarget::Panel(TREE_PANEL.to_string()), columns, 12);
    (0..screen.rows)
        .map(|row| screen.row(row))
        .filter(|row| row.contains('🌿'))
        .map(|row| row.trim_end().to_string())
        .collect()
}

fn publish(host: &mut FakeHost, path: &str, state: &str) {
    host.deliver_one(changed_topic(path, state));
    host.settle();
}

/// REQ: F-069
#[test]
fn feature_069_a_changed_topic_makes_one_branch_row_with_icon_label_and_state() {
    let mut host = client_host();
    publish(&mut host, "/work/ilium.worktrees/fix-auth", "clean");
    let rows = branch_rows(&mut host, 48);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].contains("🌿 fix-auth"), "{}", rows[0]);
    assert!(rows[0].ends_with("clean"), "{}", rows[0]);
}

/// REQ: F-069
#[test]
fn feature_069_the_state_is_joined_to_the_label_by_dotted_leaders_with_a_blank_on_each_side() {
    let mut host = client_host();
    publish(&mut host, "/work/ilium.worktrees/fix-auth", "clean");
    for columns in [40u16, 48, 80, 120] {
        let row = branch_rows(&mut host, columns).remove(0);
        let first = row
            .find('…')
            .unwrap_or_else(|| panic!("no leader at {columns}: {row}"));
        let last = row.rfind('…').expect("last leader");
        assert!(
            row[first..=last + '…'.len_utf8() - 1]
                .chars()
                .all(|character| character == '…'),
            "{row}"
        );
        assert_eq!(row[..first].chars().last(), Some(' '), "{row}");
        assert_eq!(
            row[last + '…'.len_utf8()..].chars().next(),
            Some(' '),
            "{row}"
        );
    }
}

/// REQ: F-069
#[test]
fn feature_069_the_leader_gap_is_at_least_five_cells_or_blank() {
    let mut host = client_host();
    publish(&mut host, "/work/ilium.worktrees/fix-auth", "clean");
    for columns in [24u16, 40, 80, 120] {
        let row = branch_rows(&mut host, columns).remove(0);
        let dots = row.chars().filter(|character| *character == '…').count();
        let gap = row
            .strip_prefix("🌿 fix-auth")
            .and_then(|rest| rest.strip_suffix("clean"))
            .unwrap_or_else(|| panic!("{columns}: {row}"));
        if dots == 0 {
            assert!(gap.chars().all(|c| c == ' '), "{columns}: {row}");
        } else {
            // one blank, the dots, one blank; a gap under five cells stays blank
            assert_eq!(gap, format!(" {} ", "…".repeat(dots)), "{columns}: {row}");
            assert!(gap.chars().count() >= 5, "{columns}: {row}");
        }
        if columns >= 80 {
            assert!(dots > 0, "{columns}: {row}");
        }
    }
}

/// REQ: F-069
#[test]
fn feature_069_the_state_stays_right_aligned_at_every_width() {
    let mut host = client_host();
    publish(&mut host, "/work/ilium.worktrees/docs-sweep", "2 changed");
    let mut widths = Vec::new();
    for columns in [40u16, 80, 120] {
        let row = branch_rows(&mut host, columns).remove(0);
        assert!(row.ends_with("2 changed"), "{row}");
        widths.push(row.chars().count());
    }
    assert!(widths[0] < widths[1] && widths[1] < widths[2], "{widths:?}");
}

/// REQ: F-069
#[test]
fn feature_069_two_worktrees_make_two_rows_in_arrival_order() {
    let mut host = client_host();
    publish(&mut host, "/work/ilium.worktrees/fix-auth", "clean");
    publish(&mut host, "/work/ilium.worktrees/docs-sweep", "2 changed");
    let rows = branch_rows(&mut host, 80);
    assert_eq!(rows.len(), 2);
    assert!(rows[0].contains("fix-auth") && rows[0].ends_with("clean"));
    assert!(rows[1].contains("docs-sweep") && rows[1].ends_with("2 changed"));
}

/// REQ: F-069
#[test]
fn feature_069_a_second_change_of_the_same_worktree_replaces_its_state() {
    let mut host = client_host();
    publish(&mut host, "/work/ilium.worktrees/fix-auth", "clean");
    publish(&mut host, "/work/ilium.worktrees/fix-auth", "2 changed");
    let rows = branch_rows(&mut host, 80);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].ends_with("2 changed"));
}

/// REQ: F-069, F-072
#[test]
fn feature_069_branch_line_off_hides_every_branch_row() {
    let mut host = client_host();
    set_git(&mut host, "branch_line", json!("off")).expect("valid");
    publish(&mut host, "/work/ilium.worktrees/fix-auth", "clean");
    assert!(branch_rows(&mut host, 80).is_empty());
}

/// REQ: F-069, F-072
#[test]
fn feature_069_branch_line_worktree_only_shows_the_rows_again_after_off() {
    let mut host = client_host();
    set_git(&mut host, "branch_line", json!("off")).expect("valid");
    set_git(&mut host, "branch_line", json!("worktree_only")).expect("valid");
    publish(&mut host, "/work/ilium.worktrees/fix-auth", "clean");
    assert_eq!(branch_rows(&mut host, 80).len(), 1);
}

/// REQ: F-069
#[test]
fn feature_069_a_topic_of_another_plugin_makes_no_row() {
    let mut host = client_host();
    host.deliver_one(Event::TopicPublished(TopicPublishedEvent {
        topic: "ilium.core.fleet/changed".to_string(),
        from: PluginId("ilium.core.fleet".to_string()),
        payload_json: json!({"path": "/work/x/other", "state": "clean"}).to_string(),
    }));
    host.settle();
    assert!(branch_rows(&mut host, 80).is_empty());
}

/// REQ: F-069
#[test]
fn feature_069_a_malformed_topic_payload_makes_no_row_and_no_failure() {
    let mut host = client_host();
    host.deliver_one(Event::TopicPublished(TopicPublishedEvent {
        topic: TOPIC_CHANGED.to_string(),
        from: PluginId(PLUGIN_ID.to_string()),
        payload_json: "not json".to_string(),
    }));
    host.settle();
    assert!(branch_rows(&mut host, 80).is_empty());
    assert!(host.failures().is_empty());
}

/// REQ: F-069, PORT:ilium-client/src/tree_ui.rs::worktree_branch_line_is_dim_aligned_and_elides_the_middle
#[test]
fn port_069_a_long_branch_is_elided_in_the_middle_keeping_prefix_and_suffix_within_the_width() {
    let mut host = client_host();
    publish(
        &mut host,
        "/work/ilium.worktrees/agent-fix-very-long-recognizable-suffix",
        "clean",
    );
    host.deliver_one(Event::TopicPublished(TopicPublishedEvent {
        topic: TOPIC_CHANGED.to_string(),
        from: PluginId(PLUGIN_ID.to_string()),
        payload_json: json!({"path": "/work/ilium.worktrees/agent-fix-very-long-recognizable-suffix", "state": "clean", "branch": "agent/fix-very-long-recognizable-suffix"}).to_string(),
    }));
    host.settle();
    let row = branch_rows(&mut host, 40).remove(0);
    let prefix = row.find("🌿 agent/").unwrap_or_else(|| panic!("{row}"));
    let ellipsis = row.find('…').unwrap_or_else(|| panic!("{row}"));
    let suffix = row.find("suffix").unwrap_or_else(|| panic!("{row}"));
    assert!(prefix < ellipsis && ellipsis < suffix, "{row}");
    assert!(row.ends_with("clean"), "{row}");
    assert!(row.chars().count() <= 40, "{row}");
}

/// REQ: F-069, PORT:ilium-client/src/tree_ui.rs::worktree_branch_line_uses_the_live_checkout_branch
#[test]
fn port_069_the_live_checkout_branch_wins_over_the_recorded_one_and_detached_head_is_named() {
    let mut host = client_host();
    let send = |host: &mut FakeHost, extra: Value| {
        let mut payload = json!({"path": "/work/ilium.worktrees/agent-original", "state": "clean", "recorded_branch": "agent/original"});
        for (key, value) in extra.as_object().expect("object") {
            payload[key] = value.clone();
        }
        host.deliver_one(Event::TopicPublished(TopicPublishedEvent {
            topic: TOPIC_CHANGED.to_string(),
            from: PluginId(PLUGIN_ID.to_string()),
            payload_json: payload.to_string(),
        }));
        host.settle();
        branch_rows(host, 80).remove(0)
    };
    let switched = send(&mut host, json!({"branch": "agent/switched"}));
    assert!(switched.contains("🌿 agent/switched"), "{switched}");
    assert!(!switched.contains("agent/original"), "{switched}");
    let detached = send(&mut host, json!({"branch": null, "detached": true}));
    assert!(detached.contains("🌿 detached HEAD"), "{detached}");
    assert!(!detached.contains("agent/original"), "{detached}");
    assert!(send(
        &mut host,
        json!({"branch": null, "detached": true, "missing": true})
    )
    .contains("agent/original"));
}

/// REQ: F-069
#[test]
fn feature_069_the_server_probes_each_worktree_every_ten_seconds() {
    let mut host = server_host();
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    host.deliver_one(pane_created(14, "fix-auth"));
    host.settle();
    let before = host.process().calls().len();
    host.advance_clock(9_999);
    host.settle();
    assert_eq!(
        host.process().calls().len(),
        before,
        "no probe before ten seconds"
    );
    host.advance_clock(1);
    host.settle();
    assert!(
        host.process().calls().len() > before,
        "a probe at ten seconds"
    );
}

/// REQ: F-069
#[test]
fn feature_069_the_probe_runs_git_and_not_a_shell() {
    let mut host = server_host();
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    host.deliver_one(pane_created(14, "fix-auth"));
    host.settle();
    let before = host.process().calls().len();
    host.advance_clock(10_000);
    host.settle();
    let calls = host.process().calls();
    let probes = &calls[before..];
    assert!(!probes.is_empty(), "a probe ran");
    assert!(probes.iter().all(|call| call.program == "git"));
}

/// REQ: F-069
#[test]
fn feature_069_at_most_two_probes_run_at_the_same_time() {
    let mut host = server_host();
    for index in 0..5 {
        create_worktree(&mut host, &json!({"branch": format!("b{index}")}));
        host.deliver_one(pane_created(100 + index, &format!("b{index}")));
    }
    host.settle();
    let before = host.process().calls().len();
    host.advance_clock(10_000);
    host.settle();
    assert!(
        host.process().calls().len() - before >= 5,
        "a probe per worktree"
    );
    assert_eq!(host.process().max_concurrent(), 2);
}

/// REQ: F-069
#[test]
fn feature_069_a_probe_publishes_the_changed_topic_with_the_state_of_the_worktree() {
    let mut host = server_host();
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    host.deliver_one(pane_created(14, "fix-auth"));
    host.settle();
    let before = host.published(TOPIC_CHANGED).len();
    host.advance_clock(10_000);
    host.settle();
    let published = host.published(TOPIC_CHANGED);
    assert_eq!(
        published.len(),
        before + 1,
        "one probe round publishes the topic exactly once for the one worktree"
    );
    let last: Value = serde_json::from_str(published.last().expect("a publication")).expect("JSON");
    assert!(last["path"].as_str().expect("path").ends_with("fix-auth"));
    assert_eq!(last["state"], "clean");
}

proptest! {
    // REQ: F-069
    #[test]
    fn feature_069_every_state_text_ends_the_row_at_every_width(
        state in "[a-z0-9]{1,12}( [a-z0-9]{1,8}){0,2}",
        columns in 30u16..=140,
    ) {
        let mut host = client_host();
        publish(&mut host, "/work/ilium.worktrees/fix-auth", &state);
        let row = branch_rows(&mut host, columns).remove(0);
        prop_assert!(row.ends_with(&state), "{}", row);
        prop_assert!(row.chars().count() <= columns as usize);
    }
}

fn screen_text(host: &mut FakeHost, columns: u16) -> String {
    let screen = host.render(ViewTarget::Panel(TREE_PANEL.to_string()), columns, 8);
    (0..screen.rows)
        .map(|row| screen.row(row).trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn two_worktrees(host: &mut FakeHost) {
    publish(host, "/work/ilium.worktrees/fix-auth", "clean");
    publish(host, "/work/ilium.worktrees/docs-sweep", "2 changed");
}

/// REQ: F-069
#[test]
fn feature_069_golden_tree_at_48_columns_matches_the_page_figure() {
    let mut host = client_host();
    two_worktrees(&mut host);
    ilium_plugin_sdk::testing::assert_golden(
        &golden_path("page_figure_0_48.txt"),
        &screen_text(&mut host, 48),
    );
}

/// REQ: F-069
#[test]
fn feature_069_golden_tree_at_40_80_and_120_columns() {
    for columns in [40u16, 80, 120] {
        let mut host = client_host();
        two_worktrees(&mut host);
        ilium_plugin_sdk::testing::assert_golden(
            &golden_path(&format!("tree_{columns}.txt")),
            &screen_text(&mut host, columns),
        );
    }
}

/// REQ: F-072
#[test]
fn feature_072_golden_git_tab_at_48_columns_matches_the_page_figure() {
    let mut host = client_host();
    let screen = host.render(ViewTarget::SettingsPage("git".to_string()), 48, 8);
    let text = (0..screen.rows)
        .map(|row| screen.row(row).trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    ilium_plugin_sdk::testing::assert_golden(&golden_path("page_figure_1_48.txt"), &text);
}

/// REQ: F-072
#[test]
fn feature_072_golden_git_tab_at_40_80_and_120_columns() {
    for columns in [40u16, 80, 120] {
        let mut host = client_host();
        let screen = host.render(ViewTarget::SettingsPage("git".to_string()), columns, 8);
        let text = (0..screen.rows)
            .map(|row| screen.row(row).trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        ilium_plugin_sdk::testing::assert_golden(
            &golden_path(&format!("git_{columns}.txt")),
            &text,
        );
    }
}
