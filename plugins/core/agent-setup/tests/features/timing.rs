//! Timing (page "What it covers" and the sample "Refresh on attach with a durable timer"): global blocks are
//! installed and refreshed when a client attaches (through one durable timer `refresh` that each attach moves to
//! two seconds from now) and when a project opens; project copies are never added automatically.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ClientId, DurableTimerEvent, Event, TimerId};

use crate::common::*;

fn attach(host: &mut FakeHost, client: u64) {
    host.deliver_one(Event::ClientAttached(ClientId(client)));
}

fn fire_late(host: &mut FakeHost, name: &str, late_by_millis: u64) {
    host.deliver_one(Event::DurableTimerFired(DurableTimerEvent {
        timer: TimerId::default(),
        name: name.to_string(),
        late_by_millis,
    }));
    host.settle();
}

fn has_block(host: &mut FakeHost, path: ilium_plugin_sdk::types::FsPath, feature: &str) -> bool {
    get(host, path)
        .is_some_and(|text| text.contains(&format!("<!-- ilium-agent-feature: {feature}")))
}

/// REQ: F-063
#[test]
fn timing_063_an_attach_installs_nothing_before_two_seconds_have_passed() {
    let mut host = server_host();
    attach(&mut host, 1);
    host.advance_clock(1_999);
    host.settle();
    assert!(!has_block(&mut host, claude_file(), "progress"));
}

/// REQ: F-063
#[test]
fn timing_063_two_seconds_after_an_attach_both_global_files_get_both_blocks() {
    let mut host = server_host();
    attach(&mut host, 1);
    host.advance_clock(2_000);
    host.settle();
    for file in [claude_file(), codex_file()] {
        assert!(has_block(&mut host, file.clone(), "chatroom"), "{file:?}");
        assert!(has_block(&mut host, file, "progress"));
    }
}

/// REQ: F-063
#[test]
fn timing_063_a_second_attach_moves_the_timer_to_two_seconds_after_it() {
    let mut host = server_host();
    attach(&mut host, 1);
    host.advance_clock(1_000);
    attach(&mut host, 2);
    host.advance_clock(1_500);
    host.settle();
    assert!(
        !has_block(&mut host, claude_file(), "progress"),
        "2.5 s after the first, 1.5 s after the second attach"
    );
    host.advance_clock(500);
    host.settle();
    assert!(
        has_block(&mut host, claude_file(), "progress"),
        "2 s after the second attach"
    );
}

/// REQ: F-063
#[test]
fn timing_063_many_attaches_in_a_burst_end_in_one_stable_result() {
    let mut host = server_host();
    for client in 1..=10 {
        attach(&mut host, client);
    }
    host.advance_clock(2_000);
    host.settle();
    let text = get(&mut host, claude_file()).expect("file");
    assert_eq!(text.matches(PROGRESS_OPEN).count(), 1);
    assert_eq!(text.matches(CHATROOM_OPEN).count(), 1);
}

/// REQ: F-063
#[test]
fn timing_063_a_late_timer_still_runs_one_refresh_pass() {
    let mut host = server_host();
    fire_late(&mut host, "refresh", 3_600_000);
    assert!(has_block(&mut host, claude_file(), "progress"));
}

/// REQ: F-063
#[test]
fn timing_063_a_timer_with_another_name_does_nothing() {
    let mut host = server_host();
    fire_late(&mut host, "something-else", 0);
    assert_eq!(get(&mut host, claude_file()), None);
}

/// REQ: F-063
#[test]
fn timing_063_the_refresh_upgrades_an_older_global_block_automatically() {
    let mut host = server_host();
    put(
        &mut host,
        claude_file(),
        &format!("# Mine\n\n{}", old_progress_block(12)),
    );
    fire_late(&mut host, "refresh", 0);
    let text = get(&mut host, claude_file()).expect("file");
    assert!(text.contains(PROGRESS_OPEN) && !text.contains("version=12"));
    assert!(text.starts_with("# Mine\n"));
}

/// REQ: F-063
#[test]
fn timing_063_the_refresh_leaves_a_newer_block_untouched() {
    let mut host = server_host();
    let text = old_progress_block(99);
    put(&mut host, claude_file(), &text);
    fire_late(&mut host, "refresh", 0);
    assert_eq!(
        get(&mut host, claude_file()).as_deref(),
        Some(text.as_str())
    );
}

/// REQ: F-063
#[test]
fn timing_063_the_refresh_leaves_look_alike_text_untouched() {
    let mut host = server_host();
    put(&mut host, claude_file(), PROGRESS_LOOK_ALIKE);
    fire_late(&mut host, "refresh", 0);
    assert_eq!(
        get(&mut host, claude_file()).as_deref(),
        Some(PROGRESS_LOOK_ALIKE)
    );
}

/// REQ: F-063
#[test]
fn timing_063_a_refused_file_does_not_stop_the_refresh_of_the_other_global_file() {
    let mut host = server_host();
    let broken = "<!-- ilium-agent-feature: progress version=abc -->\nx\n<!-- /ilium-agent-feature: progress -->\n";
    put(&mut host, claude_file(), broken);
    fire_late(&mut host, "refresh", 0);
    assert_eq!(get(&mut host, claude_file()).as_deref(), Some(broken));
    assert!(has_block(&mut host, codex_file(), "progress"));
}

/// REQ: F-063
#[test]
fn timing_063_a_set_up_file_is_not_rewritten_by_a_second_refresh() {
    let mut host = server_host();
    fire_late(&mut host, "refresh", 0);
    let first = get(&mut host, claude_file());
    fire_late(&mut host, "refresh", 0);
    assert_eq!(get(&mut host, claude_file()), first);
}

/// REQ: F-063, F-037
#[test]
fn timing_063_opening_a_project_refreshes_the_global_blocks() {
    let mut host = server_host();
    host.open_project("ilium");
    host.advance_clock(2_000);
    host.settle();
    assert!(has_block(&mut host, claude_file(), "progress"));
    assert!(has_block(&mut host, codex_file(), "chatroom"));
}

/// REQ: F-063, F-037
#[test]
fn timing_063_project_copies_are_never_added_automatically() {
    let mut host = server_host();
    host.open_project("ilium");
    attach(&mut host, 1);
    host.advance_clock(60_000);
    host.settle();
    assert_eq!(get(&mut host, project_claude_file()), None);
    assert_eq!(get(&mut host, project_agents_file()), None);
    assert_eq!(get(&mut host, chatroom_file()), None);
}

/// REQ: F-063, F-037
#[test]
fn timing_063_an_existing_project_file_is_not_changed_by_the_automatic_refresh() {
    let mut host = server_host();
    host.open_project("ilium");
    put(&mut host, project_claude_file(), "# Project rules\n");
    attach(&mut host, 1);
    host.advance_clock(2_000);
    host.settle();
    assert_eq!(
        get(&mut host, project_claude_file()).as_deref(),
        Some("# Project rules\n")
    );
}

/// REQ: F-063, F-037
#[test]
fn timing_063_the_refresh_follows_a_custom_global_claude_path() {
    let mut host = server_host();
    host.settings()
        .set(
            "agent_setup.progress_global_file",
            serde_json::json!("~/.claude/ALT.md"),
        )
        .expect("custom path");
    fire_late(&mut host, "refresh", 0);
    assert!(has_block(&mut host, claude_alt_file(), "progress"));
    assert!(
        !has_block(&mut host, claude_file(), "progress"),
        "the default file is not used for Progress"
    );
}
