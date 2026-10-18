//! The disabled-blocked state (user-install-and-manage.md, user-troubleshooting.md and
//! guide-publishing.md): USR-174 to USR-177. Red phase: the rig operations below are skeletons.

use crate::support::*;

/// How the block came about.
enum BlockCause {
    KeyRevokedCompromised,
    RegistryTakedown,
}

/// Blocks an installed plugin the way the registry or the trust list would.
fn block_plugin(_rig: &mut UserRig, _plugin_id: &str, _cause: BlockCause) {
    unimplemented!("revoke the signing key as compromised or apply a registry takedown")
}

/// The files that must survive a block: grants, settings and data of one plugin.
fn plugin_state_files(_rig: &UserRig, _plugin_id: &str) -> Vec<(String, Vec<u8>)> {
    unimplemented!("read the grant file, the settings file and the data folder of the plugin")
}

/// Runs `ilium plugin enable` and returns the error codes and the row state afterwards.
fn enable_blocked(
    _rig: &mut UserRig,
    _plugin_id: &str,
    _accept_risk: bool,
) -> (Vec<String>, String) {
    unimplemented!("run the enable command and read the plugin row")
}

/// The failure count `ilium plugin list` reports for the plugin.
fn reported_failure_count(_rig: &mut UserRig, _plugin_id: &str) -> u32 {
    unimplemented!("read the failure count of the plugin row")
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
