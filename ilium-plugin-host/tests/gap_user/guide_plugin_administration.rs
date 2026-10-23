//! Per-OS-user state of the guide `guide-plugin-administration` (USR-252).

use crate::common::*;

/// REQ: USR-252
#[test]
fn usr_252_grant_by_one_os_user_is_not_visible_to_another() {
    let rig = rig();
    let user_a = rig.as_os_user("alice");
    let user_b = rig.as_os_user("bob");
    install_ok(&user_a, &pkg(NOTES, "1.0.0").grant("fs.read"));
    let granted = user_a.cli(&["grant", NOTES, "fs.read", "--yes"]);
    assert_eq!(
        granted.exit_code, 0,
        "grant as alice failed: {}",
        granted.stderr
    );
    assert!(user_a.granted_for(NOTES).contains(&"fs.read".to_string()));
    assert!(
        !user_b.granted_for(NOTES).contains(&"fs.read".to_string()),
        "bob must see the grant as not granted"
    );
}

/// REQ: USR-252
#[test]
fn usr_252_trusted_key_of_one_os_user_does_not_apply_to_another() {
    let rig = rig();
    let user_a = rig.as_os_user("alice");
    let user_b = rig.as_os_user("bob");
    user_a.trust_publisher("key-team-a");
    assert!(user_a.is_trusted("key-team-a"));
    assert!(
        !user_b.is_trusted("key-team-a"),
        "a trust decision leaked to another OS user"
    );
}
