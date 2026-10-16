//! Port twins of the tests of `shell_command_titles.rs`.
#![allow(unused_imports)]

use ilium_plugin_sdk::types::*;
use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-server/tests/shell_command_titles.rs::completed_shell_commands_update_the_title_for_other_clients_until_user_rename
#[test]
fn port_041_inferred_title_is_applied_once() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "  Login fix  ");
    let applied = renames(&host.submitted());
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].title, "Login fix");
    assert_eq!(applied[0].source, TitleSource::Inferred);
}
