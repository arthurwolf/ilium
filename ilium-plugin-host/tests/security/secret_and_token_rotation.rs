//! reference-plugin-files.md: secret rotation, token rotation and secret expiry (SEC-451 .. SEC-453).
//! Doc checks: each test asserts the rule text the manual page must state.

use crate::support::*;

/// REQ: SEC-451
#[test]
fn sec_451_the_plugin_files_page_states_the_secret_rotation_rule() {
    assert_page_has(
        "reference-plugin-files.md",
        &[
            "`ilium plugin secret rotate <id> <name>` MUST replace a stored secret value without reinstalling or disabling the plugin",
            "the plugin MUST receive a `secret-changed` event for that name",
            "the audit log MUST record the rotation without the value",
        ],
    );
}

/// REQ: SEC-452
#[test]
fn sec_452_the_plugin_files_page_states_the_token_rotation_rule() {
    assert_page_has(
        "reference-plugin-files.md",
        &[
            "`ilium plugin token rotate [--http|--lock <id>]`",
            "The old token MUST stop working at the moment the command returns",
            "without a restart",
        ],
    );
}

/// REQ: SEC-453
#[test]
fn sec_453_the_plugin_files_page_states_the_secret_expiry_warning_and_secret_expired_rule() {
    assert_page_has(
        "reference-plugin-files.md",
        &[
            "The Plugins screen MUST warn 14 days before it passes",
            "the plugin gets `secret.expired` (not an empty value)",
        ],
    );
}
