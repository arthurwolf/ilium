//! Security model page (LIF-281).

use crate::common::*;

/// REQ: LIF-281
#[test]
fn lif_281_uninstall_removes_the_plugin_grants_from_grants_json() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read"));
    install_ok(&rig, &pkg(OTHER, "1.0.0").grant("fs.read"));
    rig.cli(&["uninstall", NOTES]);
    let grants = rig.grants_json();
    assert!(grants[NOTES].is_null());
    assert!(!grants[OTHER].is_null());
}

/// REQ: LIF-281
#[test]
fn lif_281_a_project_folder_listing_an_uninstalled_package_installs_nothing() {
    let rig = rig();
    rig.write_project_plugins_toml(&format!("[plugins.\"{NOTES}\"]\nenabled = true\n"));
    let server = rig.start_server("s1");
    rig.advance(secs(5));
    assert!(registry_ids(&rig).is_empty());
    assert_eq!(server.active_version(NOTES), None);
    assert!(rig.grants_json()[NOTES].is_null());
}

/// REQ: LIF-281
#[test]
fn lif_281_a_project_folder_cannot_enable_a_plugin_the_user_disabled() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.cli(&["disable", NOTES]);
    rig.write_project_plugins_toml(&format!("[plugins.\"{NOTES}\"]\nenabled = true\n"));
    let server = rig.start_server("s1");
    rig.advance(secs(5));
    assert_eq!(server.active_version(NOTES), None);
    assert_eq!(registry_entry(&rig, NOTES)["state"], "installed-disabled");
}
