//! Twin of the Git text setting key test of ilium-client/src/keys.rs: Enter on the Branch prefix row of the Git
//! tab opens an editor, Esc leaves it and the stored prefix is untouched. The tab is the kernel settings page `git`.

use ilium_plugin_sdk::types::ViewTarget;
use serde_json::json;

use crate::common::*;

/// REQ: F-007, PORT:ilium-client/src/keys.rs::git_text_setting_opens_prompt_and_returns_to_git_tab
#[test]
fn port_072_enter_on_the_branch_prefix_opens_an_editor_and_escape_returns_to_the_git_tab() {
    let mut host = client_host();
    let page = ViewTarget::SettingsPage("git".to_string());
    host.render(page.clone(), 80, 12);
    host.key(page.clone(), "down");
    host.key(page.clone(), "enter");
    host.settle();
    let editing = host.render(page.clone(), 80, 12).text();
    assert!(
        editing.contains("agent/"),
        "the editor shows the current prefix: {editing}"
    );
    host.key(page.clone(), "esc");
    host.settle();
    let back = host.render(page, 80, 12).text();
    assert!(
        back.contains("Worktree folder"),
        "the Git tab is shown again: {back}"
    );
    assert_eq!(
        host.kernel_settings().get("git", "branch_prefix"),
        Some(json!("agent/"))
    );
}
