//! Manual page `guide-enterprise-deployment`: per-user policy caps, read-only system-wide installs,
//! two users on one project folder, and the administrator purge (HST-686, 687, 688, 689).

use super::common::{install_ok, pkg, rig, NOTES};

/// REQ: HST-686
#[test]
fn hst_686_user_disk_cap_refuses_install_with_policy_user_quota_showing_used_and_allowed() {
    let rig = rig();
    rig.set_policy("max_user_plugin_disk_mib", "1");
    rig.set_policy("max_user_active_plugins", "1");
    rig.fill_plugin_data_mib(NOTES, 1);
    let package = rig.write_package(&pkg("com.example.big", "1.0.0").padding(2 * 1024 * 1024));
    let output = rig.cli(&[
        "install",
        package.to_str().expect("utf-8 path"),
        "--yes",
        "--allow-unsigned",
    ]);
    assert_ne!(output.exit_code, 0, "install over the disk cap refused");
    assert!(
        output.stderr.contains("policy.user-quota"),
        "code policy.user-quota"
    );
    assert!(output.stderr.contains("MiB"), "shows used and allowed MiB");
}

/// REQ: HST-687
#[test]
fn hst_687_system_install_is_read_only_for_users_verified_on_start_and_has_a_per_user_cache() {
    let rig = rig();
    rig.install_system_wide(&pkg(NOTES, "1.0.0"));
    assert!(
        rig.system_install_is_read_only_for_other_users(NOTES),
        "owner root, no write for others"
    );
    rig.corrupt_system_install_file(NOTES, "payload.bin");
    let server = rig.start_server_as_user("alice");
    assert!(
        server.log_lines_with_code("store.manifest-mismatch").len() == 1,
        "verified by manifest.sha256 at each start"
    );
    assert!(
        rig.compiled_cache_dir_for_user("alice") != rig.compiled_cache_dir_for_user("bob"),
        "compiled cache per user"
    );
    assert_eq!(
        rig.copies_of_system_package_made(),
        0,
        "no per-user copy of the package"
    );
}

/// REQ: HST-688
#[test]
fn hst_688_two_users_on_one_project_keep_separate_grants_and_buckets() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let alice = rig.start_server_as_user("alice");
    rig.grant_as_user("alice", NOTES, "fs:project");
    let bob = rig.start_server_as_user("bob");
    assert!(
        bob.log_lines_with_code("project.owned-by-other-user")
            .is_empty()
            || bob.is_running(),
        "second user handled"
    );
    assert!(
        !rig.user_has_grant("bob", NOTES, "fs:project"),
        "grants are per session, not shared"
    );
    assert!(
        !rig.plugin_can_read_bucket_of_other_user(NOTES, "bob", "alice"),
        "plugin cannot read the other user's bucket"
    );
    assert_eq!(
        rig.ilium_folder_mode(),
        0o700,
        "the .ilium folder keeps 0700"
    );
    alice.shutdown();
}

/// REQ: HST-689
#[test]
fn hst_689_purge_user_requires_yes_audits_and_refuses_an_active_session_server() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server_as_user("alice");
    let refused = rig.cli(&["plugin", "admin", "purge-user", "alice", "--yes"]);
    assert!(
        refused.stderr.contains("purge.user-active"),
        "running session server refuses the purge"
    );
    server.shutdown();
    let without_yes = rig.cli(&["plugin", "admin", "purge-user", "alice"]);
    assert_ne!(without_yes.exit_code, 0, "--yes is required");
    let purged = rig.cli(&["plugin", "admin", "purge-user", "alice", "--yes"]);
    assert_eq!(purged.exit_code, 0, "purge succeeds for an inactive user");
    assert!(
        rig.user_plugin_data_absent("alice"),
        "data, storage, grants, logs and cache deleted"
    );
    assert_eq!(
        rig.system_audit_entries("purge-user").len(),
        1,
        "system audit entry written"
    );
}
