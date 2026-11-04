//! Gap-review requirements of the lifecycle-ops lane (LIF-060..064, 147..149, 158..159,
//! 167..169, 177..179, 188..189, 194..198). Each requirement has one test named `a` for the
//! specified behaviour and one named `b` for the refusal or edge case. The tests drive the
//! `ilium_plugin_host::testing::ops` rig and assert the observable outcome that each row's
//! `testable_how` names. Where the rig has no direct call for a step, the test uses the nearest
//! existing entry point and the assertion still names the required result.

use std::path::{Path, PathBuf};

use crate::common::*;
use ilium_plugin_api::Side;
use serde_json::json;

/// The plugin that manages the other plugins' enabled state in the LIF-194 tests.
const CALLER: &str = "com.example.caller";
/// The core plugin whose repeated failures LIF-197 counts.
const CORE_TREE: &str = "ilium.core.tree";
/// The rows the `plugin stats` output must show (LIF-159).
const STATS_ROWS: [&str; 7] = [
    "descriptors",
    "processes",
    "timers",
    "watches",
    "pending-requests",
    "event-queue",
    "output-filters",
];

/// The purge tombstone list of `installed.json`.
fn tombstones(rig: &OpsRig) -> Vec<String> {
    rig.registry_json()["purge_tombstones"]
        .as_array()
        .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

/// Sets or clears the read-only flag of a file or folder.
fn set_read_only(path: &Path, read_only: bool) {
    let mut permissions = std::fs::metadata(path).expect("metadata of the read-only target").permissions();
    permissions.set_readonly(read_only);
    std::fs::set_permissions(path, permissions).expect("set the permissions of the target");
}

/// The cached package of an installed version in the store.
fn store_package(rig: &OpsRig, id: &str, version: &str) -> PathBuf {
    rig.plugins_root().join("store").join(id).join(version).join("package.iliumpkg")
}

/// Flips the first byte of a file, so its hash no longer matches.
fn flip_first_byte(path: &Path) {
    let mut bytes = std::fs::read(path).expect("read the cached package");
    if let Some(first) = bytes.first_mut() {
        *first ^= 0xff;
    }
    std::fs::write(path, bytes).expect("write the corrupted package");
}

/// The project `.ilium/` folder the rig uses for the second project.
fn project_ilium_dir(rig: &OpsRig) -> PathBuf {
    rig.scratch_dir().join("project").join(".ilium")
}

/// The folder where the crash marks of the server start live.
fn crash_mark_dir(rig: &OpsRig) -> PathBuf {
    rig.state_dir().join("crash-marks")
}

/// Writes one crash mark recording the boot id, monotonic uptime and build digest of its start.
fn plant_crash_mark(rig: &OpsRig, index: usize, boot_id: &str, monotonic_ms: u64, build_digest: &str) {
    let directory = crash_mark_dir(rig);
    std::fs::create_dir_all(&directory).expect("create the crash mark folder");
    let record = json!({
        "boot_id": boot_id,
        "monotonic_ms": monotonic_ms,
        "build_digest": build_digest,
        "reason": "crash",
    });
    std::fs::write(directory.join(format!("mark-{index}.json")), record.to_string()).expect("write a crash mark");
}

/// The boot id of the running machine, as Linux reports it.
fn current_boot_id() -> String {
    std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// Whether a server start shows the safe-mode footer.
fn in_safe_mode(server: &ServerHandle) -> bool {
    server.render_status_surface().contains("Safe mode:")
}

/// Every host log code recorded so far that equals `code`.
fn count_codes(rig: &OpsRig, code: &str) -> usize {
    rig.log_codes().iter().filter(|recorded| recorded.as_str() == code).count()
}

/// Overwrites every file in a folder with bytes that do not parse.
fn corrupt_every_file(directory: &Path) {
    for name in entry_names(directory) {
        std::fs::write(directory.join(name), b"\x00 not a snapshot {{").expect("corrupt a snapshot");
    }
}

/// REQ: LIF-060
#[test]
fn req_lif_060_purge_tombstone_written_before_delete_a() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.write_storage(NOTES, "project", "note", b"stale");
    rig.inject(Fault::KillUninstall(UninstallStep::StateWritten));
    let _killed = rig.cli(&["plugin", "uninstall", NOTES, "--purge"]);
    assert!(
        tombstones(&rig).contains(&NOTES.to_owned()),
        "the id must be in purge_tombstones once the uninstall-pending write is on disk"
    );
    let _restarted = rig.start_server("default");
    let _second_project = rig.start_server("second");
    assert!(
        rig.read_storage(NOTES, "project", "note").is_none(),
        "the second project's buckets for the purged id must be deleted after the restart"
    );
}

/// REQ: LIF-060
#[test]
fn req_lif_060_plain_uninstall_without_purge_writes_no_tombstone_b() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let output = rig.cli(&["plugin", "uninstall", NOTES]);
    assert_eq!(output.exit_code, 0, "a plain uninstall succeeds: {:?}", output.stdout_jsonl);
    assert!(
        !tombstones(&rig).contains(&NOTES.to_owned()),
        "only --purge may add a tombstone; a plain uninstall must not"
    );
}

/// REQ: LIF-061
#[test]
fn req_lif_061_reinstall_removes_tombstone_and_keeps_new_buckets_a() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let purged = rig.cli(&["plugin", "uninstall", NOTES, "--purge"]);
    assert_eq!(purged.exit_code, 0, "the purge succeeds: {:?}", purged.stdout_jsonl);
    install_ok(&rig, &pkg(NOTES, "1.0.1"));
    assert!(
        !tombstones(&rig).contains(&NOTES.to_owned()),
        "the reinstall must remove the id from purge_tombstones in the same registry write"
    );
    rig.write_storage(NOTES, "project", "note", b"new");
    let _first = rig.start_server("default");
    let _second = rig.start_server("second");
    assert!(
        rig.read_storage(NOTES, "project", "note").is_some(),
        "a bucket written by the new install must survive a later project open"
    );
}

/// REQ: LIF-061
#[test]
fn req_lif_061_install_of_another_id_keeps_the_tombstone_b() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let purged = rig.cli(&["plugin", "uninstall", NOTES, "--purge"]);
    assert_eq!(purged.exit_code, 0, "the purge succeeds: {:?}", purged.stdout_jsonl);
    install_ok(&rig, &pkg(OTHER, "1.0.0"));
    assert!(
        tombstones(&rig).contains(&NOTES.to_owned()),
        "installing a different id must not clear the tombstone of the purged id"
    );
}

/// REQ: LIF-062
#[test]
fn req_lif_062_read_only_project_defers_purge_and_keeps_tombstone_a() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.write_storage(NOTES, "project", "note", b"kept");
    let purged = rig.cli(&["plugin", "uninstall", NOTES, "--purge"]);
    assert_eq!(purged.exit_code, 0, "the purge succeeds: {:?}", purged.stdout_jsonl);
    let ilium_dir = project_ilium_dir(&rig);
    std::fs::create_dir_all(&ilium_dir).expect("create the project .ilium folder");
    set_read_only(&ilium_dir, true);
    let _read_only_open = rig.start_server("second");
    assert!(
        rig.read_storage(NOTES, "project", "note").is_some(),
        "the deferred buckets must still exist while .ilium is read-only"
    );
    assert!(tombstones(&rig).contains(&NOTES.to_owned()), "the tombstone must not be dropped");
    assert_eq!(count_codes(&rig, "host.data-readonly"), 1, "host.data-readonly is logged once");
    set_read_only(&ilium_dir, false);
    let _writable_open = rig.start_server("second");
    assert!(
        rig.read_storage(NOTES, "project", "note").is_none(),
        "the next writable open deletes the deferred buckets"
    );
}

/// REQ: LIF-062
#[test]
fn req_lif_062_deferred_buckets_are_not_passed_to_plugins_while_read_only_b() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.write_storage(NOTES, "project", "note", b"hidden");
    let purged = rig.cli(&["plugin", "uninstall", NOTES, "--purge"]);
    assert_eq!(purged.exit_code, 0, "the purge succeeds: {:?}", purged.stdout_jsonl);
    let ilium_dir = project_ilium_dir(&rig);
    std::fs::create_dir_all(&ilium_dir).expect("create the project .ilium folder");
    set_read_only(&ilium_dir, true);
    let _first = rig.start_server("second");
    let _second = rig.start_server("second");
    assert_eq!(
        count_codes(&rig, "host.data-readonly"),
        1,
        "the read-only notice is logged once, not on every open"
    );
    set_read_only(&ilium_dir, false);
}

/// REQ: LIF-063
#[test]
fn req_lif_063_shared_compile_cache_entry_survives_one_uninstall_a() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    install_ok(&rig, &pkg(OTHER, "1.0.0"));
    let cache_before = entry_names(&rig.compile_cache_dir());
    assert!(!cache_before.is_empty(), "the installs must leave a compile cache entry");
    let uninstalled = rig.cli(&["plugin", "uninstall", NOTES]);
    assert_eq!(uninstalled.exit_code, 0, "the uninstall succeeds: {:?}", uninstalled.stdout_jsonl);
    assert_eq!(
        entry_names(&rig.compile_cache_dir()),
        cache_before,
        "the entry shared with the other installed plugin must stay"
    );
    let _other = rig.start_server("default");
    assert!(
        !rig.log_codes().iter().any(|code| code.starts_with("compile")),
        "the other plugin starts without a compile log line"
    );
    let second = rig.cli(&["plugin", "uninstall", OTHER]);
    assert_eq!(second.exit_code, 0, "the second uninstall succeeds: {:?}", second.stdout_jsonl);
    assert!(
        !entry_names(&rig.compile_cache_dir()).iter().any(|name| cache_before.contains(name)),
        "the entry must be gone once no installed plugin references it"
    );
}

/// REQ: LIF-063
#[test]
fn req_lif_063_unshared_compile_cache_entry_is_removed_by_uninstall_b() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    assert!(!entry_names(&rig.compile_cache_dir()).is_empty(), "the install must leave a compile cache entry");
    let uninstalled = rig.cli(&["plugin", "uninstall", NOTES]);
    assert_eq!(uninstalled.exit_code, 0, "the uninstall succeeds: {:?}", uninstalled.stdout_jsonl);
    assert!(
        entry_names(&rig.compile_cache_dir()).is_empty(),
        "an entry no other plugin references is deleted on uninstall"
    );
}

/// REQ: LIF-064
#[test]
fn req_lif_064_reinstall_with_intact_cache_keeps_store_and_registry_a() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let store_before = snapshot_tree(&rig.plugins_root().join("store"));
    let registry_before = rig.registry_bytes();
    let output = rig.cli(&["plugin", "reinstall", NOTES]);
    assert_eq!(output.exit_code, 0, "reinstall of an intact cache succeeds: {:?}", output.stdout_jsonl);
    assert_eq!(snapshot_tree(&rig.plugins_root().join("store")), store_before, "the store is replaced only by a valid copy");
    assert_eq!(rig.registry_bytes(), registry_before, "a valid reinstall leaves the registry byte-identical");
}

/// REQ: LIF-064
#[test]
fn req_lif_064_flipped_cached_package_is_refused_with_package_corrupt_b() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    flip_first_byte(&store_package(&rig, NOTES, "1.0.0"));
    let store_before = snapshot_tree(&rig.plugins_root().join("store"));
    let registry_before = rig.registry_bytes();
    let output = rig.cli(&["plugin", "reinstall", NOTES]);
    assert_eq!(output.exit_code, 1, "a corrupt cached package exits 1: {:?}", output.stdout_jsonl);
    assert!(
        error_codes(&output).contains(&"package.corrupt".to_owned()),
        "the refusal must carry package.corrupt: {:?}",
        error_codes(&output)
    );
    assert_eq!(snapshot_tree(&rig.plugins_root().join("store")), store_before, "the store must stay unchanged");
    assert_eq!(rig.registry_bytes(), registry_before, "the registry must stay unchanged");
}

/// REQ: LIF-147
#[test]
fn req_lif_147_uninstall_waits_for_migrate_lock_then_completes_after_restart_a() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let hold = rig.hold_lock_for(secs(60));
    let output = rig.cli(&["plugin", "uninstall", NOTES]);
    assert_eq!(output.exit_code, 4, "a busy migrate lock exits 4: {:?}", output.stdout_jsonl);
    assert!(error_codes(&output).contains(&"install.busy".to_owned()), "the refusal is install.busy");
    assert_eq!(
        registry_entry(&rig, NOTES)["state"],
        "uninstall-pending",
        "the plugin is left in uninstall-pending"
    );
    drop(hold);
    let _restarted = rig.start_server("default");
    assert!(
        registry_entry(&rig, NOTES).is_null(),
        "the uninstall completes at the next start once the lock is free"
    );
}

/// REQ: LIF-147
#[test]
fn req_lif_147_busy_lock_is_waited_for_up_to_thirty_seconds_b() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let _hold = rig.hold_lock_for(secs(60));
    let _output = rig.cli(&["plugin", "uninstall", NOTES]);
    assert!(
        rig.virtual_elapsed() >= secs(30),
        "the uninstall must wait the full 30 seconds before it gives up"
    );
    assert!(rig.lock_polls() > 0, "the uninstall must poll the migrate lock while it waits");
}

/// REQ: LIF-148
#[test]
fn req_lif_148_purge_warns_once_per_absent_path_and_exits_zero_a() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    std::fs::create_dir_all(rig.data_dir(NOTES)).expect("data directory");
    let plain = rig.cli(&["plugin", "uninstall", NOTES]);
    assert_eq!(plain.exit_code, 0, "the plain uninstall succeeds: {:?}", plain.stdout_jsonl);
    let _ = std::fs::remove_dir_all(rig.data_dir(NOTES));
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let _ = std::fs::remove_dir_all(rig.plugins_root().join("backups").join(NOTES));
    let output = rig.cli(&["plugin", "uninstall", NOTES, "--purge"]);
    assert_eq!(output.exit_code, 0, "purge over absent items exits 0: {:?}", output.stdout_jsonl);
    let absent_warnings = warning_codes(&output)
        .iter()
        .filter(|code| code.as_str() == "uninstall.already-absent")
        .count();
    assert!(absent_warnings >= 2, "one uninstall.already-absent warning per absent path, got {absent_warnings}");
    assert!(!rig.data_dir(NOTES).exists(), "the items that exist are still deleted");
}

/// REQ: LIF-148
#[test]
fn req_lif_148_purge_with_everything_present_warns_about_nothing_b() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    std::fs::create_dir_all(rig.data_dir(NOTES)).expect("data directory");
    let output = rig.cli(&["plugin", "uninstall", NOTES, "--purge"]);
    assert_eq!(output.exit_code, 0, "the purge succeeds: {:?}", output.stdout_jsonl);
    assert!(
        !warning_codes(&output).iter().any(|code| code.as_str() == "uninstall.already-absent"),
        "no path was absent, so no already-absent warning may be printed"
    );
}

/// REQ: LIF-149
#[test]
fn req_lif_149_reinstall_from_store_copy_keeps_identical_manifest_a() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let source = store_package(&rig, NOTES, "1.0.0");
    let entry_before = registry_entry(&rig, NOTES);
    let source_text = source.to_string_lossy().into_owned();
    let output = rig.cli(&["install", &source_text, "--reinstall", "--yes", "--allow-unsigned"]);
    assert_eq!(output.exit_code, 0, "reinstall from the store copy succeeds: {:?}", output.stdout_jsonl);
    assert_eq!(registry_entry(&rig, NOTES), entry_before, "the manifest hash must not change");
}

/// REQ: LIF-149
#[test]
fn req_lif_149_interrupted_reinstall_from_store_leaves_one_complete_entry_b() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let source = store_package(&rig, NOTES, "1.0.0");
    let source_text = source.to_string_lossy().into_owned();
    rig.inject(Fault::KillInstaller(2));
    let _killed = rig.cli(&["install", &source_text, "--reinstall", "--yes", "--allow-unsigned"]);
    let server = rig.start_server("default");
    assert_eq!(
        store_versions(&rig, NOTES),
        vec!["1.0.0".to_owned()],
        "exactly one complete store entry must remain"
    );
    assert!(staging_entries(&rig).is_empty(), "the staged copy must not be left behind");
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"), "the plugin must load");
}

/// REQ: LIF-158
#[test]
fn req_lif_158_update_cancel_removes_partial_download_a() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let downloads = rig.cache_dir().join("ilium").join("plugins").join("downloads");
    std::fs::create_dir_all(&downloads).expect("downloads folder");
    std::fs::write(downloads.join(format!("{NOTES}-2.0.0.part")), b"partial bytes").expect("partial file");
    let output = rig.cli(&["plugin", "update-cancel", NOTES]);
    assert_eq!(output.exit_code, 0, "update-cancel succeeds: {:?}", output.stdout_jsonl);
    assert!(entry_names(&downloads).is_empty(), "no partial file may remain after the cancel");
    assert!(registry_entry(&rig, NOTES)["update"].is_null(), "the candidate must not be staged");
}

/// REQ: LIF-158
#[test]
fn req_lif_158_uninstall_also_aborts_download_and_removes_partial_b() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let downloads = rig.cache_dir().join("ilium").join("plugins").join("downloads");
    std::fs::create_dir_all(&downloads).expect("downloads folder");
    std::fs::write(downloads.join(format!("{NOTES}-2.0.0.part")), b"partial bytes").expect("partial file");
    let output = rig.cli(&["plugin", "uninstall", NOTES]);
    assert_eq!(output.exit_code, 0, "uninstall succeeds: {:?}", output.stdout_jsonl);
    assert!(entry_names(&downloads).is_empty(), "uninstall must delete the partial download of the id");
}

/// REQ: LIF-159
#[test]
fn req_lif_159_stats_json_lists_every_budget_row_a() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let output = rig.cli(&["plugin", "stats", NOTES, "--json"]);
    assert_eq!(output.exit_code, 0, "stats succeeds: {:?}", output.stdout_jsonl);
    for row in STATS_ROWS {
        assert!(
            output.stdout_raw.contains(&format!("\"{row}\"")),
            "the --json stats must contain a `{row}` row: {}",
            output.stdout_raw
        );
    }
}

/// REQ: LIF-159
#[test]
fn req_lif_159_stats_text_lists_every_budget_row_b() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let output = rig.cli(&["plugin", "stats", NOTES]);
    assert_eq!(output.exit_code, 0, "stats succeeds: {:?}", output.stdout_jsonl);
    for row in STATS_ROWS {
        assert!(output.stdout_text.contains(row), "the text stats must show a `{row}` row: {}", output.stdout_text);
    }
}

/// REQ: LIF-167
#[test]
fn req_lif_167_unwritable_crash_mark_lets_start_proceed_with_guard_off_a() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    set_read_only(&rig.state_dir(), true);
    for _ in 0..3 {
        let server = rig.start_server("default");
        server.kill_sigkill();
    }
    set_read_only(&rig.state_dir(), false);
    let server = rig.start_server("default");
    assert!(!in_safe_mode(&server), "the start must not enter safe mode");
    assert!(
        count_codes(&rig, "crash-guard.unavailable") >= 3,
        "crash-guard.unavailable must be logged on each start"
    );
    let footer = server.render_status_surface().to_lowercase();
    assert!(footer.contains("crash-loop guard") && footer.contains("off"), "the footer must show the guard is off");
}

/// REQ: LIF-167
#[test]
fn req_lif_167_writable_state_folder_logs_no_unavailable_notice_b() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let _server = rig.start_server("default");
    assert_eq!(
        count_codes(&rig, "crash-guard.unavailable"),
        0,
        "a writable state folder must not report the crash guard as unavailable"
    );
}

/// REQ: LIF-168
#[test]
fn req_lif_168_wall_clock_jump_does_not_count_marks_of_same_boot_a() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    for index in 0..3 {
        plant_crash_mark(&rig, index, "boot-a", 1_000 + index as u64, "build-x");
    }
    rig.advance(secs(3600));
    let server = rig.start_server("default");
    assert!(!in_safe_mode(&server), "marks of another boot, one hour earlier, must not trigger safe mode");
}

/// REQ: LIF-168
#[test]
fn req_lif_168_three_fresh_marks_under_current_boot_enter_safe_mode_b() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let boot = current_boot_id();
    for index in 0..3 {
        plant_crash_mark(&rig, index, &boot, 1_000 + index as u64, "build-x");
    }
    let server = rig.start_server("default");
    assert!(
        in_safe_mode(&server),
        "three fresh marks written under the current boot id must enter safe mode"
    );
}

/// REQ: LIF-169
#[test]
fn req_lif_169_marks_from_another_build_do_not_trigger_safe_mode_a() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let boot = current_boot_id();
    for index in 0..3 {
        plant_crash_mark(&rig, index, &boot, 1_000 + index as u64, "build-digest-x");
    }
    let server = rig.start_server("default");
    assert!(!in_safe_mode(&server), "marks written by another build must not count");
    rig.advance(secs(300));
    let _settled = rig.start_server("default");
    assert!(
        entry_names(&crash_mark_dir(&rig)).is_empty(),
        "marks of another build are still cleared by LIF-294 after five minutes"
    );
}

/// REQ: LIF-169
#[test]
fn req_lif_169_marks_of_the_running_build_are_counted_b() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let boot = current_boot_id();
    for index in 0..3 {
        plant_crash_mark(&rig, index, &boot, 1_000 + index as u64, env!("CARGO_PKG_VERSION"));
    }
    let server = rig.start_server("default");
    assert!(in_safe_mode(&server), "three marks of the running build must count toward safe mode");
}

/// REQ: LIF-178
#[test]
fn req_lif_178_unreadable_crash_mark_counts_as_crash_and_is_cleared_later_a() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let boot = current_boot_id();
    plant_crash_mark(&rig, 0, &boot, 1_000, "build-x");
    plant_crash_mark(&rig, 1, &boot, 1_001, "build-x");
    let unreadable = crash_mark_dir(&rig).join("mark-truncated.json");
    std::fs::write(&unreadable, b"{\"boot_id\":").expect("truncated mark");
    let server = rig.start_server("default");
    assert!(in_safe_mode(&server), "two valid marks plus one unreadable mark must enter safe mode");
    assert!(
        count_codes(&rig, "crash-guard.mark-unreadable") >= 1,
        "the unreadable mark must be logged as crash-guard.mark-unreadable"
    );
    rig.advance(secs(300));
    let _settled = rig.start_server("default");
    assert!(entry_names(&crash_mark_dir(&rig)).is_empty(), "all three marks must be gone after five minutes");
}

/// REQ: LIF-178
#[test]
fn req_lif_178_unreadable_crash_mark_is_neither_deleted_nor_rewritten_before_cleanup_b() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let unreadable = crash_mark_dir(&rig).join("mark-truncated.json");
    std::fs::create_dir_all(crash_mark_dir(&rig)).expect("crash mark folder");
    std::fs::write(&unreadable, b"{\"boot_id\":").expect("truncated mark");
    let _server = rig.start_server("default");
    assert_eq!(
        std::fs::read(&unreadable).expect("the unreadable mark must still exist"),
        b"{\"boot_id\":",
        "the host must not rewrite the mark before the cleanup under LIF-294"
    );
}

/// REQ: LIF-179
#[test]
fn req_lif_179_helper_unstable_stop_is_not_written_to_failure_stopped_a() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    for _ in 0..6 {
        server.fail(NOTES, Side::Server);
    }
    assert!(
        registry_entry(&rig, NOTES)["failure_stopped"].is_null(),
        "failure_stopped must be absent when the stop reason is helper.unstable"
    );
    server.shutdown();
    let restarted = rig.start_server("default");
    assert!(
        format!("{:?}", restarted.state_of(NOTES, Side::Server)).to_lowercase().contains("active"),
        "the sides must be active again at the next start"
    );
}

/// REQ: LIF-179
#[test]
fn req_lif_179_plugin_failures_are_still_written_to_failure_stopped_b() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").activate_traps());
    let server = rig.start_server("default");
    for _ in 0..3 {
        server.fail(NOTES, Side::Server);
    }
    assert!(
        !registry_entry(&rig, NOTES)["failure_stopped"].is_null(),
        "a plugin's own failures must still be recorded in failure_stopped"
    );
}

/// REQ: LIF-188
#[test]
fn req_lif_188_connection_drop_deactivates_with_shutdown_and_cancels_pending_a() {
    let rig = rig();
    let server = rig.start_server("s1");
    install_ok(&rig, &pkg(NOTES, "1.0.0").restore_bytes(16));
    let _client = server.attach_client("client-a");
    let requests: Vec<_> = (0..3).map(|_| server.send_pending_request(NOTES)).collect();
    server.kill_sigkill();
    assert!(
        format!("{:?}", server.deactivations(NOTES)).contains("Shutdown"),
        "the client must deactivate with the reason shutdown"
    );
    for request in &requests {
        assert_eq!(request.outcome().as_deref(), Some("cancelled"), "pending requests complete as cancelled");
    }
    let next = rig.start_server("s1");
    let _reattached = next.attach_client("client-b");
    assert_eq!(
        next.last_restored_len(NOTES),
        Some(16),
        "the restore bytes must be sent to the next server and restored"
    );
}

/// REQ: LIF-188
#[test]
fn req_lif_188_attached_client_is_not_deactivated_while_connection_holds_b() {
    let rig = rig();
    let server = rig.start_server("s1");
    install_ok(&rig, &pkg(NOTES, "1.0.0").restore_bytes(16));
    let _client = server.attach_client("client-a");
    assert!(
        server.deactivations(NOTES).is_empty(),
        "a live connection must not produce a shutdown deactivation"
    );
}

/// REQ: LIF-189
#[test]
fn req_lif_189_corrupt_bisect_state_is_refused_and_renamed_a() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let _started = rig.cli(&["plugin", "bisect", "start"]);
    let bisect = rig.plugins_root().join("bisect.json");
    std::fs::write(&bisect, b"not json {{").expect("garbage bisect state");
    let registry_before = rig.registry_bytes();
    let output = rig.cli(&["plugin", "bisect", "good"]);
    assert!(
        error_codes(&output).contains(&"bisect.state-corrupt".to_owned()),
        "bisect must refuse with bisect.state-corrupt: {:?}",
        error_codes(&output)
    );
    assert!(
        entry_names(&rig.plugins_root()).iter().any(|name| name.starts_with("bisect.json.corrupt-")),
        "the corrupt file must be renamed to bisect.json.corrupt-<timestamp>"
    );
    assert_eq!(rig.registry_bytes(), registry_before, "installed.json must not change");
}

/// REQ: LIF-189
#[test]
fn req_lif_189_start_with_unreadable_bisect_state_runs_saved_set_b() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    std::fs::write(rig.plugins_root().join("bisect.json"), b"garbage").expect("garbage bisect state");
    let registry_before = rig.registry_bytes();
    let server = rig.start_server("default");
    assert!(server.active_version(NOTES).is_some(), "every plugin of the saved set must be enabled");
    assert_eq!(rig.registry_bytes(), registry_before, "a start must not rewrite installed.json");
}

/// REQ: LIF-194
#[test]
fn req_lif_194_set_enabled_clears_failure_window_once_per_ten_minutes_a() {
    let rig = rig();
    install_ok(&rig, &pkg(CALLER, "1.0.0").grant("plugins.manage"));
    install_ok(&rig, &pkg(NOTES, "1.0.0").activate_traps());
    let server = rig.start_server("default");
    for _ in 0..3 {
        server.fail(NOTES, Side::Server);
    }
    let request = json!({"id": NOTES, "enabled": true});
    let first = server.plugin_host_call(CALLER, "plugins.set-enabled", &request);
    assert!(first.is_ok(), "the first set-enabled clears the window: {first:?}");
    server.advance_clock(secs(300));
    let second = server.plugin_host_call(CALLER, "plugins.set-enabled", &request);
    let second_text = format!("{second:?}");
    assert!(second_text.contains("busy"), "a second call inside ten minutes must return busy: {second_text}");
    assert!(second_text.contains("retry-after-ms"), "busy must carry retry-after-ms: {second_text}");
    server.advance_clock(secs(600));
    let third = server.plugin_host_call(CALLER, "plugins.set-enabled", &request);
    assert!(!format!("{third:?}").contains("busy"), "a call after the limit window may clear the window again");
}

/// REQ: LIF-194
#[test]
fn req_lif_194_user_enable_is_not_rate_limited_b() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    for _ in 0..4 {
        server.enable(NOTES);
        assert!(server.is_enabled(NOTES), "a user action through Settings or the CLI is never rate limited");
    }
}

/// REQ: LIF-195
#[test]
fn req_lif_195_failed_failure_stopped_write_still_stops_side_and_retries_later_a() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").activate_traps());
    install_ok(&rig, &pkg(OTHER, "1.0.0"));
    let server = rig.start_server("default");
    let registry = rig.plugins_root().join("installed.json");
    set_read_only(&registry, true);
    for _ in 0..3 {
        server.fail(NOTES, Side::Server);
    }
    assert!(
        format!("{:?}", server.state_of(NOTES, Side::Server)).to_lowercase().contains("failure"),
        "the side must be stopped-by-failure for the running session"
    );
    assert_eq!(count_codes(&rig, "registry.write-failed"), 1, "registry.write-failed is logged once");
    assert!(server.restart_delay_ms(NOTES, Side::Server).is_none(), "the side must not restart");
    set_read_only(&registry, false);
    server.disable(OTHER);
    assert!(
        !registry_entry(&rig, NOTES)["failure_stopped"].is_null(),
        "the next successful registry write must record failure_stopped"
    );
}

/// REQ: LIF-195
#[test]
fn req_lif_195_repeated_failed_writes_log_the_warning_only_once_b() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").activate_traps());
    let server = rig.start_server("default");
    let registry = rig.plugins_root().join("installed.json");
    set_read_only(&registry, true);
    for _ in 0..4 {
        server.fail(NOTES, Side::Server);
    }
    assert_eq!(count_codes(&rig, "registry.write-failed"), 1, "the write warning is logged once, not per failure");
    set_read_only(&registry, false);
}

/// REQ: LIF-196
#[test]
fn req_lif_196_corrupt_snapshot_restores_previous_and_keeps_broken_copy_a() {
    let rig = rig();
    let server = rig.start_server("default");
    server.open_pane("kept-pane");
    server.snapshot_now();
    server.open_pane("lost-pane");
    server.snapshot_now();
    let sessions = project_ilium_dir(&rig).join("sessions");
    std::fs::write(sessions.join("default.json"), b"\x00 corrupt").expect("corrupt the current snapshot");
    let restarted = rig.start_server("default");
    assert!(
        restarted.tree_nodes().iter().any(|node| node.1 == "kept-pane"),
        "the session must restore the tree of the previous snapshot"
    );
    assert_eq!(
        count_codes(&rig, "snapshot.restored-previous"),
        1,
        "snapshot.restored-previous must be logged with both file names"
    );
    assert!(
        entry_names(&sessions).iter().any(|name| name.starts_with("default.json.broken-")),
        "the corrupt current file must be kept as a .broken-<timestamp> copy"
    );
}

/// REQ: LIF-196
#[test]
fn req_lif_196_no_readable_snapshot_starts_with_one_empty_shell_pane_b() {
    let rig = rig();
    let server = rig.start_server("default");
    server.open_pane("kept-pane");
    server.snapshot_now();
    server.open_pane("lost-pane");
    server.snapshot_now();
    let sessions = project_ilium_dir(&rig).join("sessions");
    corrupt_every_file(&sessions);
    let restarted = rig.start_server("default");
    assert_eq!(restarted.pane_count(), 1, "with no readable snapshot the session starts with one empty shell pane");
}

/// REQ: LIF-197
#[test]
fn req_lif_197_essential_plugin_failure_storm_logs_once_and_keeps_restarting_a() {
    let rig = rig();
    let server = rig.start_server("default");
    for _ in 0..10 {
        server.fail(CORE_TREE, Side::Server);
    }
    assert_eq!(
        count_codes(&rig, "host.essential-unstable"),
        1,
        "host.essential-unstable is logged once per failure window"
    );
    assert!(server.recovery_page_open(), "the recovery page must open");
    assert_eq!(
        server.restart_delay_ms(CORE_TREE, Side::Server),
        Some(5_000),
        "the essential plugin keeps restarting every 5 seconds"
    );
    let banner = server.banner_text();
    server.advance_clock(secs(600));
    assert_ne!(server.banner_text(), banner, "the essential-failure banner must clear once the window empties");
}

/// REQ: LIF-197
#[test]
fn req_lif_197_essential_plugin_is_never_stopped_by_the_failure_storm_b() {
    let rig = rig();
    let server = rig.start_server("default");
    for _ in 0..10 {
        server.fail(CORE_TREE, Side::Server);
    }
    let state = format!("{:?}", server.state_of(CORE_TREE, Side::Server)).to_lowercase();
    assert!(!state.contains("stopped"), "the essential plugin must not be stopped, state: {state}");
}

/// REQ: LIF-198
#[test]
fn req_lif_198_first_start_after_host_upgrade_writes_one_upgraded_record_a() {
    let rig = rig();
    let _old = rig.start_server_with_env("default", &[("ILIUM_TEST_HOST_VERSION", "0.1.0")]);
    install_ok(&rig, &pkg(NOTES, "0.1.0"));
    let _new = rig.start_server_with_env("default", &[("ILIUM_TEST_HOST_VERSION", "0.3.0")]);
    let records = rig.audit_records("host.upgraded");
    assert_eq!(records.len(), 1, "exactly one host.upgraded audit record must be written: {records:?}");
    let text = records[0].to_string();
    assert!(
        text.contains(NOTES) && text.contains("0.1.0") && text.contains("0.3.0"),
        "the record must list the plugin id and both host versions: {text}"
    );
}

/// REQ: LIF-198
#[test]
fn req_lif_198_later_starts_on_the_same_host_version_write_no_upgraded_record_b() {
    let rig = rig();
    let _first = rig.start_server_with_env("default", &[("ILIUM_TEST_HOST_VERSION", "0.3.0")]);
    let _second = rig.start_server_with_env("default", &[("ILIUM_TEST_HOST_VERSION", "0.3.0")]);
    assert!(
        rig.audit_records("host.upgraded").is_empty(),
        "a start on an unchanged host version must not write host.upgraded"
    );
}
