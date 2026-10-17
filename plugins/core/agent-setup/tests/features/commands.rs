//! The command that refreshes the global blocks, and the absence of an `agent-setup` CLI command.

use ilium_plugin_sdk::types::Event;

use crate::common::*;

/// REQ: F-063
#[test]
fn command_063_refresh_installs_the_global_blocks_at_once() {
    let mut host = server_host();
    host.invoke_command("agent-setup.refresh");
    host.settle();
    assert!(get(&mut host, claude_file())
        .expect("file")
        .contains(PROGRESS_OPEN));
    assert!(get(&mut host, codex_file())
        .expect("file")
        .contains(CHATROOM_OPEN));
}

/// REQ: F-063, F-037
#[test]
fn command_063_refresh_never_creates_project_copies() {
    let mut host = project_host();
    host.invoke_command("agent-setup.refresh");
    host.settle();
    assert_eq!(get(&mut host, project_claude_file()), None);
    assert_eq!(get(&mut host, project_agents_file()), None);
}

/// REQ: F-063
#[test]
fn command_063_refresh_upgrades_an_older_block_and_leaves_a_newer_one() {
    let mut host = server_host();
    put(&mut host, claude_file(), &old_progress_block(12));
    let newer = old_progress_block(99);
    put(&mut host, codex_file(), &newer);
    host.invoke_command("agent-setup.refresh");
    host.settle();
    assert!(get(&mut host, claude_file())
        .expect("file")
        .contains(PROGRESS_OPEN));
    assert_eq!(
        get(&mut host, codex_file()).as_deref(),
        Some(newer.as_str())
    );
}

/// REQ: F-063
#[test]
fn command_063_refresh_works_when_invoked_from_the_client_event_path() {
    let mut host = server_host();
    host.deliver_one(Event::CommandInvoked(
        ilium_plugin_sdk::types::CommandInvokedEvent {
            command: "agent-setup.refresh".to_string(),
            origin: ilium_plugin_sdk::types::CommandOrigin::Palette,
            arguments_json: "{}".to_string(),
            context: ilium_plugin_sdk::types::CommandContext::default(),
        },
    ));
    host.settle();
    assert!(get(&mut host, claude_file()).is_some());
}

/// REQ: F-063
#[test]
fn command_063_refresh_is_idempotent() {
    let mut host = server_host();
    host.invoke_command("agent-setup.refresh");
    let first = get(&mut host, claude_file());
    host.invoke_command("agent-setup.refresh");
    assert_eq!(get(&mut host, claude_file()), first);
}

/// REQ: F-063
#[test]
fn command_063_the_current_cli_has_no_agent_setup_command() {
    let help = manual_page("core-plugin-agent-setup.md");
    assert!(
        !help.contains("ilium agent-setup"),
        "the page must not promise a CLI command"
    );
}
