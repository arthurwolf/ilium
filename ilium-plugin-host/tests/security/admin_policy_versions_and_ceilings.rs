//! Requirements SEC-950 .. SEC-952 (reference-admin-policy.md): the policy has no ceiling key for
//! session resources, and `packages.blocked_versions` and `packages.pinned_versions` act on exact
//! versions. The version rules are proposed by the manual and await owner confirmation.

use ilium_plugin_api::Tier;
use serde_json::json;

use crate::support::*;

const BLOCKED: &str = "version = 1\n[packages]\nblocked_versions = [\"example.pane-tools@2.0.0\"]\n";
const PINNED: &str = "version = 1\n\n[packages.pinned_versions]\n\"example.pane-tools\" = \"1.4.2\"\n";

/// REQ: SEC-950
#[test]
fn sec_950_the_admin_policy_page_names_no_administrator_ceiling_for_session_resources() {
    let page = manual_page("reference-admin-policy.md");
    assert!(
        page.contains("The policy has no key that sets a ceiling for a session resource"),
        "the page must state that the policy has no session ceiling key"
    );
    assert!(
        page.contains(
            "`session_memory_mib`, `session_helpers`, `session_processes`, `disk_mib`, `storage_bucket_mib` and the `ilium-execution` CPU budget"
        ),
        "the page must list the six session resources without a ceiling"
    );
    assert!(
        page.contains("this page does not promise any administrator cap"),
        "the page must not promise an administrator cap"
    );
}

/// REQ: SEC-950
#[test]
fn sec_950_a_future_administrator_ceiling_key_is_only_proposed_and_absent_from_the_key_table() {
    let page = manual_page("reference-admin-policy.md");
    assert!(
        page.contains("`limits.session_memory_mib_max` (proposed, owner to confirm)"),
        "the only ceiling key named by the page is marked as proposed"
    );
    assert!(
        !page.contains("| `limits.session_memory_mib_max`"),
        "the proposed ceiling key must not appear in the policy key table"
    );
}

/// REQ: SEC-950
#[test]
fn sec_950_a_user_may_lower_a_session_limit_below_the_default() {
    let rig = setup(&[]).start();
    rig.host
        .user_set_setting("plugins.limits.session_memory_mib", json!(64))
        .expect("a user may lower a session limit");
    assert_eq!(
        rig.host.effective_setting("plugins.limits.session_memory_mib"),
        Some(json!(64))
    );
}

/// REQ: SEC-950
#[test]
fn sec_950_a_user_value_above_the_default_is_clamped_and_never_taken_as_is() {
    let rig = setup(&[]).start();
    let _ = rig.host.user_set_setting("plugins.limits.session_memory_mib", json!(999_999));
    assert_ne!(
        rig.host.effective_setting("plugins.limits.session_memory_mib"),
        Some(json!(999_999)),
        "a higher user value must be clamped to the default"
    );
}

/// REQ: SEC-951
#[test]
fn sec_951_a_blocked_exact_version_is_refused_at_install_with_policy_version_denied() {
    let rig = setup(&[]).user_policy(BLOCKED).start();
    let refusal = rig
        .host
        .install_check("example.pane-tools@2.0.0", Tier::T3)
        .expect_err("2.0.0 is listed in blocked_versions");
    assert!(
        refusal.contains("policy.version-denied"),
        "the refusal must carry policy.version-denied, got {refusal}"
    );
}

/// REQ: SEC-951
#[test]
fn sec_951_other_versions_of_a_blocked_id_install_and_an_installed_blocked_copy_is_disabled() {
    let rig = setup(&[])
        .user_policy(BLOCKED)
        .option("installed-plugins", json!(["example.pane-tools"]))
        .start();
    rig.host
        .install_check("example.pane-tools@2.0.1", Tier::T3)
        .expect("2.0.1 is not blocked and must install");
    let rig = rig.restart();
    assert!(
        rig.host.plugin_disabled("example.pane-tools"),
        "an installed copy of a blocked version is disabled at the next start"
    );
    assert!(
        rig.host
            .notices()
            .iter()
            .any(|notice| notice.contains("Disabled by your administrator")),
        "the disabled copy shows the Disabled by your administrator notice"
    );
}

/// REQ: SEC-952
#[test]
fn sec_952_the_pinned_version_of_an_id_installs() {
    let rig = setup(&[]).user_policy(PINNED).start();
    rig.host
        .install_check("example.pane-tools@1.4.2", Tier::T3)
        .expect("the pinned version must install");
}

/// REQ: SEC-952
#[test]
fn sec_952_an_update_to_another_version_is_refused_with_policy_version_pinned() {
    let rig = setup(&[]).user_policy(PINNED).start();
    let refusal = rig
        .host
        .install_check("example.pane-tools@2.0.1", Tier::T3)
        .expect_err("only 1.4.2 may be installed while the pin holds");
    assert!(
        refusal.contains("policy.version-pinned"),
        "the refusal must carry policy.version-pinned, got {refusal}"
    );
}

/// REQ: SEC-952
#[test]
fn sec_952_a_pin_never_removes_plugin_data() {
    let rig = setup(&[]).user_policy(PINNED).start();
    rig.host.write_plugin_data("example.pane-tools", b"kept-by-pin");
    let rig = rig.restart();
    assert_eq!(
        rig.host.read_plugin_data("example.pane-tools"),
        Some(b"kept-by-pin".to_vec()),
        "a pin must leave the plugin data in place"
    );
}
