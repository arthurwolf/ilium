//! The disabled-blocked state (user-install-and-manage.md, user-troubleshooting.md and
//! guide-publishing.md): USR-174 to USR-177. The helpers below drive the plugin command line; they fail until the host blocks plugins.

use crate::support::*;

/// How the block came about.
enum BlockCause {
    KeyRevokedCompromised,
    RegistryTakedown,
}

/// Blocks an installed plugin the way the registry or the trust list would.
fn block_plugin(rig: &mut UserRig, plugin_id: &str, cause: BlockCause) {
    let output = match cause {
        BlockCause::KeyRevokedCompromised => rig.cli(&[
            "trust",
            "revoke",
            plugin_id,
            "--reason",
            "compromised",
            "--json",
        ]),
        BlockCause::RegistryTakedown => rig.cli(&["registry", "takedown", plugin_id, "--json"]),
    };
    assert_eq!(
        result_exit_code(&output),
        0,
        "blocking {plugin_id} failed: {:?}",
        error_codes(&output)
    );
}

/// The files that must survive a block: grants, settings and data of one plugin.
fn plugin_state_files(rig: &UserRig, plugin_id: &str) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    if let Ok(bytes) = std::fs::read(rig.grants_path()) {
        files.push(("grants".to_string(), bytes));
    }
    if let Some(value) = rig.plugin_setting(plugin_id, "mode") {
        files.push(("setting:mode".to_string(), value.into_bytes()));
    }
    for (name, bytes) in tree_snapshot(&rig.data_dir().join("plugins").join("data").join(plugin_id)) {
        files.push((format!("data:{name}"), bytes));
    }
    files
}

/// Runs `ilium plugin enable` and returns the error codes and the row state afterwards.
fn enable_blocked(
    rig: &mut UserRig,
    plugin_id: &str,
    accept_risk: bool,
) -> (Vec<String>, String) {
    let mut arguments = vec!["enable", plugin_id, "--json"];
    if accept_risk {
        arguments.push("--accept-risk");
    }
    let output = rig.cli(&arguments);
    (error_codes(&output), list_row(rig, plugin_id).state)
}

/// The failure count `ilium plugin list` reports for the plugin.
fn reported_failure_count(rig: &mut UserRig, plugin_id: &str) -> u32 {
    let output = rig.cli(&["list", "--json"]);
    let row = records(&output, "plugin")
        .into_iter()
        .find(|record| record["id"] == plugin_id)
        .unwrap_or_else(|| panic!("plugin {plugin_id} is not listed"));
    // A blocked plugin carries no failure count; a missing field reads as zero.
    row["failures"].as_u64().map_or(0, |count| count as u32)
}

/// REQ: USR-174
#[test]
fn usr_174_key_revocation_stops_the_plugin_and_shows_blocked() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.writer", "1.0.0", &[]));
    block_plugin(
        &mut rig,
        "com.example.writer",
        BlockCause::KeyRevokedCompromised,
    );
    let row = list_row(&mut rig, "com.example.writer");
    assert_eq!(row.state, "Blocked");
}

/// REQ: USR-174
#[test]
fn usr_174_takedown_blocks_and_keeps_grants_settings_and_data() {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.writer", "1.0.0", &["tree.read"]),
    );
    let before = plugin_state_files(&rig, "com.example.writer");
    block_plugin(&mut rig, "com.example.writer", BlockCause::RegistryTakedown);
    assert_eq!(list_row(&mut rig, "com.example.writer").state, "Blocked");
    assert_eq!(plugin_state_files(&rig, "com.example.writer"), before);
}

/// REQ: USR-175
#[test]
fn usr_175_plain_enable_is_refused_with_plugin_blocked() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.writer", "1.0.0", &[]));
    block_plugin(
        &mut rig,
        "com.example.writer",
        BlockCause::KeyRevokedCompromised,
    );
    let (codes, state) = enable_blocked(&mut rig, "com.example.writer", false);
    assert_eq!(codes, vec!["plugin.blocked".to_string()]);
    assert_eq!(state, "Blocked");
}

/// REQ: USR-175
#[test]
fn usr_175_accept_risk_turns_the_plugin_on() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.writer", "1.0.0", &[]));
    block_plugin(&mut rig, "com.example.writer", BlockCause::RegistryTakedown);
    let (codes, state) = enable_blocked(&mut rig, "com.example.writer", true);
    assert!(codes.is_empty());
    assert_eq!(state, "On");
}

/// REQ: USR-176
#[test]
fn usr_176_blocked_plugin_reports_no_failure_count() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.writer", "1.0.0", &[]));
    block_plugin(
        &mut rig,
        "com.example.writer",
        BlockCause::KeyRevokedCompromised,
    );
    assert_eq!(reported_failure_count(&mut rig, "com.example.writer"), 0);
}

/// REQ: USR-176
#[test]
fn usr_176_troubleshooting_row_names_both_reasons_and_the_way_out() {
    let page = manual_page("user-troubleshooting");
    assert!(page.contains("trust.key-revoked"));
    assert!(page.contains("registry.takedown"));
    assert!(page.contains("--accept-risk"));
}

/// REQ: USR-177
#[test]
fn usr_177_publisher_guide_explains_revocation_and_takedown_effects() {
    let page = manual_page("guide-publishing");
    assert!(page.contains("disabled-blocked"));
    assert!(page.contains("--accept-risk"));
    assert!(page.contains("another trusted key of your publisher id"));
}

/// REQ: USR-177
#[test]
fn usr_177_publisher_guide_says_blocked_version_is_not_installable_from_store() {
    let page = manual_page("guide-publishing");
    for phrase in [
        "a blocked version cannot be installed from the store",
        "A registry takedown (PKG-403) has the same effect on the named version.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}
