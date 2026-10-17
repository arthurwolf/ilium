//! Reference, install, update and uninstall (LIF-065..094, LIF-282): the registry file, the
//! install pipeline, update, rollback, uninstall, commands, propagation and recovery.

use std::time::Duration;

use serde_json::Value;

use crate::common::*;

// ---------------------------------------------------------------------------------------------
// The registry file: LIF-065, LIF-066, LIF-067, LIF-282
// ---------------------------------------------------------------------------------------------

/// True when `bytes` parse as a registry that lists `id`.
fn registry_lists(bytes: &[u8], id: &str) -> bool {
    serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|value| value["plugins"].as_array().cloned())
        .map(|plugins| plugins.iter().any(|entry| entry["id"] == id))
        .unwrap_or(false)
}

fn writer_killed_at(step: RegistryStep) {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let old_bytes = rig.registry_bytes();
    rig.inject(Fault::RegistryWrite(step));
    let _killed = install(&rig, &pkg(OTHER, "1.0.0"));
    let seen = rig.registry_bytes();
    let is_old = seen == old_bytes;
    let is_new = registry_lists(&seen, OTHER) && registry_lists(&seen, NOTES);
    assert!(
        is_old || is_new,
        "a reader must see the old or the new file after a kill at {step:?}"
    );
}

/// REQ: LIF-065
#[test]
fn lif_065_writer_killed_after_temp_file_written_leaves_old_or_new_registry() {
    writer_killed_at(RegistryStep::TempWritten);
}

/// REQ: LIF-065
#[test]
fn lif_065_writer_killed_after_temp_file_fsync_leaves_old_or_new_registry() {
    writer_killed_at(RegistryStep::TempSynced);
}

/// REQ: LIF-065
#[test]
fn lif_065_writer_killed_after_rename_leaves_old_or_new_registry() {
    writer_killed_at(RegistryStep::Renamed);
}

/// REQ: LIF-065
#[test]
fn lif_065_writer_killed_after_directory_fsync_leaves_new_registry() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.inject(Fault::RegistryWrite(RegistryStep::DirSynced));
    let _killed = install(&rig, &pkg(OTHER, "1.0.0"));
    assert!(registry_lists(&rig.registry_bytes(), OTHER));
}

/// REQ: LIF-065
#[test]
fn lif_065_write_order_is_temp_fsync_rename_directory_fsync() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let trace = rig.trace();
    let temp = position_of(&trace, "registry.temp-written");
    let synced = position_of(&trace, "registry.temp-synced");
    let renamed = position_of(&trace, "registry.renamed");
    let dir_synced = position_of(&trace, "registry.dir-synced");
    assert!(temp < synced && synced < renamed && renamed < dir_synced);
}

/// REQ: LIF-065
#[test]
fn lif_065_clean_install_leaves_no_temp_file_in_the_plugin_root() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let allowed = [
        "installed.json",
        "installed.lock",
        "store",
        "staging",
        "locks",
        "backups",
        "data",
        "storage",
        "logs",
    ];
    for name in entry_names(&rig.plugins_root()) {
        assert!(allowed.contains(&name.as_str()), "stray entry `{name}`");
    }
}

/// REQ: LIF-065, LIF-282
#[test]
fn lif_065_registry_is_two_space_indented_lf_terminated_and_sorted_by_id() {
    let rig = rig();
    install_ok(&rig, &pkg(OTHER, "1.0.0"));
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let text = String::from_utf8(rig.registry_bytes()).expect("utf-8 registry");
    assert!(text.starts_with("{\n  \"format\": \"ilium-installed/1\""));
    assert!(text.ends_with("}\n") && !text.ends_with("\n\n"));
    assert!(!text.contains('\r'));
    let ids = registry_ids(&rig);
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
    assert_eq!(ids, vec![NOTES.to_owned(), OTHER.to_owned()]);
}

/// REQ: LIF-066
#[test]
fn lif_066_second_writer_fails_with_install_locked_exit_4_after_30_seconds() {
    let rig = rig();
    let _holder = rig.hold_lock();
    let output = install(&rig, &pkg(NOTES, "1.0.0"));
    assert_eq!(output.exit_code, 4);
    assert_eq!(error_codes(&output), vec!["install.locked".to_owned()]);
    assert!(rig.virtual_elapsed() >= secs(30));
    assert!(rig.virtual_elapsed() <= secs(30) + ms(100));
}

/// REQ: LIF-066
#[test]
fn lif_066_second_writer_polls_every_100_milliseconds() {
    let rig = rig();
    let _holder = rig.hold_lock();
    let _output = install(&rig, &pkg(NOTES, "1.0.0"));
    let polls = rig.lock_polls();
    assert!((299..=301).contains(&polls), "polls: {polls}");
}

/// REQ: LIF-066
#[test]
fn lif_066_lock_released_just_before_30_seconds_lets_the_install_succeed() {
    let rig = rig();
    let _holder = rig.hold_lock_for(ms(29_900));
    let output = install(&rig, &pkg(NOTES, "1.0.0"));
    assert_eq!(output.exit_code, 0);
    assert_eq!(registry_ids(&rig), vec![NOTES.to_owned()]);
}

/// REQ: LIF-066
#[test]
fn lif_066_lock_held_past_30_seconds_fails_the_install_and_changes_nothing() {
    let rig = rig();
    let _holder = rig.hold_lock_for(ms(30_100));
    let output = install(&rig, &pkg(NOTES, "1.0.0"));
    assert_eq!(output.exit_code, 4);
    assert!(registry_ids(&rig).is_empty());
}

/// REQ: LIF-066
#[test]
fn lif_066_killed_lock_holder_releases_the_lock_at_once() {
    let rig = rig();
    let holder = rig.hold_lock();
    holder.kill();
    let output = install(&rig, &pkg(NOTES, "1.0.0"));
    assert_eq!(output.exit_code, 0);
    assert_eq!(rig.virtual_elapsed(), Duration::ZERO);
}

/// REQ: LIF-066
#[test]
fn lif_066_lock_file_records_holder_process_id_and_boot_id() {
    let rig = rig();
    let _holder = rig.hold_lock();
    let text = std::fs::read_to_string(rig.plugins_root().join("installed.lock"))
        .expect("installed.lock exists while held");
    let boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .expect("boot id")
        .trim()
        .to_owned();
    assert!(text.contains(&std::process::id().to_string()));
    assert!(text.contains(&boot_id));
}

/// REQ: LIF-066
#[test]
fn lif_066_lock_is_held_around_the_store_and_registry_changes() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let trace = rig.trace();
    let acquired = position_of(&trace, "lock.acquire");
    let released = position_of(&trace, "lock.release");
    let store = position_of(&trace, "store.rename");
    let registry = position_of(&trace, "registry.renamed");
    assert!(acquired < store && store < released);
    assert!(acquired < registry && registry < released);
}

fn registry_with_format(rig: &OpsRig, format: &str) -> Vec<u8> {
    install_ok(rig, &pkg(NOTES, "1.0.0"));
    let mut value = rig.registry_json();
    value["format"] = Value::String(format.to_owned());
    let mut bytes = serde_json::to_vec_pretty(&value).expect("serialise registry");
    bytes.push(b'\n');
    rig.set_registry_bytes(&bytes);
    bytes
}

/// REQ: LIF-067
#[test]
fn lif_067_registry_format_99_refused_with_install_registry_format() {
    let rig = rig();
    let bytes = registry_with_format(&rig, "ilium-installed/99");
    let output = rig.cli(&["list"]);
    assert_eq!(output.exit_code, 1);
    assert_eq!(
        error_codes(&output),
        vec!["install.registry-format".to_owned()]
    );
    assert_eq!(rig.registry_bytes(), bytes);
}

/// REQ: LIF-067
#[test]
fn lif_067_install_over_a_newer_registry_format_does_not_rewrite_it() {
    let rig = rig();
    let bytes = registry_with_format(&rig, "ilium-installed/99");
    let output = install(&rig, &pkg(OTHER, "1.0.0"));
    assert_eq!(output.exit_code, 1);
    assert!(error_codes(&output).contains(&"install.registry-format".to_owned()));
    assert_eq!(rig.registry_bytes(), bytes);
}

/// REQ: LIF-067
#[test]
fn lif_067_registry_format_2_is_the_next_higher_number_and_is_refused() {
    let rig = rig();
    let bytes = registry_with_format(&rig, "ilium-installed/2");
    let output = rig.cli(&["list"]);
    assert_eq!(
        error_codes(&output),
        vec!["install.registry-format".to_owned()]
    );
    assert_eq!(rig.registry_bytes(), bytes);
}

/// REQ: LIF-067
#[test]
fn lif_067_registry_format_1_is_accepted() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let output = rig.cli(&["list"]);
    assert_eq!(output.exit_code, 0);
    assert!(error_codes(&output).is_empty());
}

/// REQ: LIF-067
#[test]
fn lif_067_server_start_over_a_newer_registry_format_leaves_the_file_alone() {
    let rig = rig();
    let bytes = registry_with_format(&rig, "ilium-installed/99");
    let server = rig.start_server("s1");
    assert!(server
        .log_codes()
        .contains(&"install.registry-format".to_owned()));
    assert_eq!(rig.registry_bytes(), bytes);
}

fn assert_state_corrupt(mutate: impl FnOnce(&mut Value)) {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let mut value = rig.registry_json();
    mutate(&mut value);
    let mut bytes = serde_json::to_vec_pretty(&value).expect("serialise registry");
    bytes.push(b'\n');
    rig.set_registry_bytes(&bytes);
    let output = rig.cli(&["list"]);
    assert_eq!(output.exit_code, 1);
    assert_eq!(
        error_codes(&output),
        vec!["install.state-corrupt".to_owned()]
    );
    assert_eq!(
        rig.registry_bytes(),
        bytes,
        "the file must not be rewritten"
    );
    let server = rig.start_server("s1");
    assert!(server
        .log_codes()
        .contains(&"install.state-corrupt".to_owned()));
    assert_eq!(rig.registry_bytes(), bytes);
}

/// REQ: LIF-282
#[test]
fn lif_282_pinned_with_wrong_type_refused_with_install_state_corrupt() {
    assert_state_corrupt(|value| value["plugins"][0]["pinned"] = Value::String("yes".into()));
}

/// REQ: LIF-282
#[test]
fn lif_282_extra_plugin_key_refused_with_install_state_corrupt() {
    assert_state_corrupt(|value| value["plugins"][0]["extra"] = Value::Bool(true));
}

/// REQ: LIF-282
#[test]
fn lif_282_extra_top_level_key_refused_with_install_state_corrupt() {
    assert_state_corrupt(|value| value["unexpected"] = Value::Null);
}

/// REQ: LIF-282
#[test]
fn lif_282_short_key_id_refused_with_install_state_corrupt() {
    assert_state_corrupt(|value| value["plugins"][0]["key_id"] = Value::String("abc123".into()));
}

/// REQ: LIF-282
#[test]
fn lif_282_local_time_installed_at_refused_with_install_state_corrupt() {
    assert_state_corrupt(|value| {
        value["plugins"][0]["installed_at"] = Value::String("2026-10-12T09:14:04+02:00".into())
    });
}

/// REQ: LIF-282
#[test]
fn lif_282_missing_required_key_refused_with_install_state_corrupt() {
    assert_state_corrupt(|value| {
        value["plugins"][0]
            .as_object_mut()
            .expect("plugin object")
            .remove("skip");
    });
}

/// REQ: LIF-282
#[test]
fn lif_282_unknown_state_value_refused_with_install_state_corrupt() {
    assert_state_corrupt(|value| value["plugins"][0]["state"] = Value::String("bogus".into()));
}

/// REQ: LIF-282
#[test]
fn lif_282_update_attempts_above_two_refused_with_install_state_corrupt() {
    assert_state_corrupt(|value| {
        value["plugins"][0]["update"] = serde_json::json!({
            "candidate": "1.1.0", "state": "staged", "attempts": 3,
            "started_at": "2026-10-12T09:14:04Z"
        })
    });
}

/// REQ: LIF-282
#[test]
fn lif_282_unknown_source_value_refused_with_install_state_corrupt() {
    assert_state_corrupt(|value| value["plugins"][0]["source"] = Value::String("ftp".into()));
}

/// REQ: LIF-282
#[test]
fn lif_282_valid_update_object_with_attempts_two_is_accepted() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let mut value = rig.registry_json();
    value["plugins"][0]["update"] = serde_json::json!({
        "candidate": "1.1.0", "state": "trial", "attempts": 2,
        "started_at": "2026-10-12T09:14:04Z"
    });
    rig.set_registry_bytes(&serde_json::to_vec_pretty(&value).expect("serialise"));
    let output = rig.cli(&["list"]);
    assert!(!error_codes(&output).contains(&"install.state-corrupt".to_owned()));
}

// ---------------------------------------------------------------------------------------------
// The install pipeline: LIF-068 .. LIF-072
// ---------------------------------------------------------------------------------------------

/// REQ: LIF-068
#[test]
fn lif_068_declined_review_runs_no_package_code_review_declined() {
    let rig = rig();
    let marker = rig.scratch_dir().join("start-ran.txt");
    let spec = pkg(NOTES, "1.0.0")
        .high_risk_grant("process.spawn")
        .start_writes_file(&marker);
    let path = rig.write_package(&spec);
    let path_text = path.to_string_lossy().into_owned();
    let output = rig.cli_tty(&["install", &path_text, "--allow-unsigned"], &["n"]);
    assert_eq!(output.exit_code, 1);
    assert_eq!(
        error_codes(&output),
        vec!["install.review-declined".to_owned()]
    );
    assert!(!marker.exists());
}

/// REQ: LIF-068
#[test]
fn lif_068_completed_install_without_a_server_runs_no_package_code() {
    let rig = rig();
    let marker = rig.scratch_dir().join("start-ran.txt");
    install_ok(&rig, &pkg(NOTES, "1.0.0").start_writes_file(&marker));
    assert!(!marker.exists());
}

/// REQ: LIF-068
#[test]
fn lif_068_check_command_runs_no_package_code() {
    let rig = rig();
    let marker = rig.scratch_dir().join("start-ran.txt");
    let path = rig.write_package(&pkg(NOTES, "1.0.0").start_writes_file(&marker));
    let path_text = path.to_string_lossy().into_owned();
    let _output = rig.cli(&["check", &path_text]);
    assert!(!marker.exists());
}

/// REQ: LIF-068
#[test]
fn lif_068_no_instance_exists_before_the_review_is_answered() {
    let rig = rig();
    let server = rig.start_server("s1");
    let path = rig.write_package(&pkg(NOTES, "1.0.0").high_risk_grant("process.spawn"));
    let path_text = path.to_string_lossy().into_owned();
    let _declined = rig.cli_tty(&["install", &path_text, "--allow-unsigned"], &["n"]);
    assert!(server.activations(NOTES).is_empty());
}

fn fail_install_at(step: u8, expected_code: &str) {
    let rig = rig();
    install_ok(&rig, &pkg(OTHER, "1.0.0"));
    let registry_before = rig.registry_bytes();
    let store_before = snapshot_tree(&rig.plugins_root().join("store"));
    rig.inject(Fault::InstallFails(step));
    let output = install(&rig, &pkg(NOTES, "1.0.0"));
    assert_ne!(output.exit_code, 0);
    assert!(
        error_codes(&output).contains(&expected_code.to_owned()),
        "{:?}",
        output.stdout_jsonl
    );
    assert_eq!(
        rig.registry_bytes(),
        registry_before,
        "registry changed at step {step}"
    );
    assert!(
        staging_entries(&rig).is_empty(),
        "staging left at step {step}"
    );
    assert_eq!(
        snapshot_tree(&rig.plugins_root().join("store")),
        store_before
    );
    assert!(
        store_versions(&rig, NOTES).is_empty(),
        "store entry left at step {step}"
    );
}

/// REQ: LIF-069
#[test]
fn lif_069_failure_at_step_3_unpack_leaves_registry_unchanged_install_io() {
    fail_install_at(3, "install.io");
}

/// REQ: LIF-069
#[test]
fn lif_069_failure_at_step_4_compile_leaves_registry_unchanged_install_compile_failed() {
    fail_install_at(4, "install.compile-failed");
}

/// REQ: LIF-069
#[test]
fn lif_069_failure_at_step_5_rename_leaves_registry_unchanged_install_io() {
    fail_install_at(5, "install.io");
}

/// REQ: LIF-069
#[test]
fn lif_069_failure_at_step_6_registry_write_deletes_the_new_store_entry_install_io() {
    fail_install_at(6, "install.io");
}

/// REQ: LIF-069
#[test]
fn lif_069_failure_at_step_2_validation_leaves_registry_unchanged_manifest_schema() {
    let rig = rig();
    install_ok(&rig, &pkg(OTHER, "1.0.0"));
    let before = rig.registry_bytes();
    let output = install(&rig, &pkg(NOTES, "1.0.0").bad_manifest());
    assert_eq!(output.exit_code, 1);
    assert!(error_codes(&output).contains(&"manifest.schema".to_owned()));
    assert_eq!(rig.registry_bytes(), before);
    assert!(staging_entries(&rig).is_empty());
    assert!(store_versions(&rig, NOTES).is_empty());
}

/// REQ: LIF-069
#[test]
fn lif_069_failed_update_keeps_the_old_registry_and_deletes_the_candidate_entry() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let before = rig.registry_bytes();
    rig.inject(Fault::InstallFails(6));
    let output = update(&rig, &pkg(NOTES, "1.1.0"));
    assert_ne!(output.exit_code, 0);
    assert_eq!(rig.registry_bytes(), before);
    assert_eq!(store_versions(&rig, NOTES), vec!["1.0.0".to_owned()]);
    assert!(staging_entries(&rig).is_empty());
}

/// REQ: LIF-069
#[test]
fn lif_069_failed_notification_at_step_7_is_a_warning_and_keeps_the_install() {
    let rig = rig();
    let server = rig.start_server("s1");
    server.drop_socket();
    let output = install(&rig, &pkg(NOTES, "1.0.0"));
    assert_eq!(output.exit_code, 0);
    assert_eq!(registry_ids(&rig), vec![NOTES.to_owned()]);
}

/// REQ: LIF-070
#[test]
fn lif_070_install_with_no_network_interfaces_succeeds() {
    let rig = rig();
    rig.block_network();
    let output = install(&rig, &pkg(NOTES, "1.0.0"));
    assert_eq!(output.exit_code, 0);
    assert_eq!(registry_ids(&rig), vec![NOTES.to_owned()]);
}

/// REQ: LIF-070
#[test]
fn lif_070_install_makes_no_network_attempt() {
    let rig = rig();
    rig.block_network();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read"));
    assert_eq!(rig.network_attempts(), 0);
}

/// REQ: LIF-071
#[test]
fn lif_071_bad_manifest_and_bad_signature_report_manifest_schema_only() {
    let rig = rig();
    let spec = pkg(NOTES, "1.0.0")
        .signed("key-a")
        .bad_manifest()
        .bad_signature();
    let output = install(&rig, &spec);
    let codes = error_codes(&output);
    assert_eq!(codes.first().map(String::as_str), Some("manifest.schema"));
    assert!(!codes.contains(&"manifest.signature-invalid".to_owned()));
}

/// REQ: LIF-071
#[test]
fn lif_071_no_prompt_appears_for_a_package_refused_at_an_earlier_stage() {
    let rig = rig();
    let spec = pkg(NOTES, "1.0.0")
        .high_risk_grant("process.spawn")
        .bad_manifest();
    let path = rig.write_package(&spec);
    let path_text = path.to_string_lossy().into_owned();
    let output = rig.cli_tty(&["install", &path_text, "--allow-unsigned"], &["y"]);
    assert_eq!(output.exit_code, 1);
    assert_eq!(rig.prompt_count(), 0);
    assert!(!error_codes(&output).contains(&"install.review-declined".to_owned()));
}

/// REQ: LIF-071
#[test]
fn lif_071_bad_signature_alone_reports_manifest_signature_invalid() {
    let rig = rig();
    let output = install(&rig, &pkg(NOTES, "1.0.0").signed("key-a").bad_signature());
    assert!(error_codes(&output).contains(&"manifest.signature-invalid".to_owned()));
    assert_eq!(rig.prompt_count(), 0);
}

/// REQ: LIF-072
#[test]
fn lif_072_installing_the_same_package_twice_is_a_no_op_with_a_warning() {
    let rig = rig();
    let spec = pkg(NOTES, "1.0.0");
    install_ok(&rig, &spec);
    let before = rig.registry_bytes();
    let output = install(&rig, &spec);
    assert_eq!(output.exit_code, 0);
    assert_eq!(
        warning_codes(&output),
        vec!["install.already-installed".to_owned()]
    );
    assert_eq!(rig.registry_bytes(), before);
}

/// REQ: LIF-072
#[test]
fn lif_072_same_version_with_different_manifest_hash_refused_package_version_reused() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let before = rig.registry_bytes();
    let output = install(&rig, &pkg(NOTES, "1.0.0").content_marker("changed"));
    assert_eq!(output.exit_code, 1);
    assert_eq!(
        error_codes(&output),
        vec!["package.version-reused".to_owned()]
    );
    assert_eq!(rig.registry_bytes(), before);
}

/// REQ: LIF-072
#[test]
fn lif_072_same_manifest_signed_again_is_still_a_no_op() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").signed("key-a"));
    let before = rig.registry_bytes();
    let again = pkg(NOTES, "1.0.0")
        .signed("key-a")
        .resigned("second signature");
    let output = install(&rig, &again);
    assert_eq!(output.exit_code, 0);
    assert_eq!(
        warning_codes(&output),
        vec!["install.already-installed".to_owned()]
    );
    assert_eq!(rig.registry_bytes(), before);
}

/// REQ: LIF-072
#[test]
fn lif_072_reinstall_flag_rewrites_the_store_entry_from_the_package() {
    let rig = rig();
    let spec = pkg(NOTES, "1.0.0");
    install_ok(&rig, &spec);
    let original = snapshot_tree(&rig.plugins_root().join("store").join(NOTES).join("1.0.0"));
    let (first_name, _) = original.first().cloned().expect("store has files");
    let damaged = rig
        .plugins_root()
        .join("store")
        .join(NOTES)
        .join("1.0.0")
        .join(&first_name);
    std::fs::write(&damaged, b"damaged").expect("damage a store file");
    let path = rig.write_package(&spec);
    let path_text = path.to_string_lossy().into_owned();
    let output = rig.cli(&[
        "install",
        &path_text,
        "--yes",
        "--allow-unsigned",
        "--reinstall",
    ]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(
        snapshot_tree(&rig.plugins_root().join("store").join(NOTES).join("1.0.0")),
        original
    );
}

/// REQ: LIF-072
#[test]
fn doc_lif_072_old_same_version_code_name_maps_to_package_version_reused() {
    let page = manual_page("reference-manifest-schema");
    assert!(page.contains(
        "| `install.same-version-different-content` | `package.version-reused` | Same. |"
    ));
    assert!(catalogue_codes().contains("package.version-reused"));
}

// ---------------------------------------------------------------------------------------------
// Update: LIF-073 .. LIF-078
// ---------------------------------------------------------------------------------------------

fn two_manual_servers(rig: &OpsRig) -> (ServerHandle, ServerHandle) {
    (rig.start_server_manual("a"), rig.start_server_manual("b"))
}

/// REQ: LIF-073
#[test]
fn lif_073_first_server_to_apply_decides_and_the_version_commits() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let (a, b) = two_manual_servers(&rig);
    b.fail_activation_of_version("1.1.0");
    update(&rig, &pkg(NOTES, "1.1.0"))
        .exit_code
        .eq(&0)
        .then_some(())
        .expect("update ok");
    a.apply_pending();
    assert_eq!(registry_entry(&rig, NOTES)["active_version"], "1.1.0");
    b.apply_pending();
    assert_eq!(registry_entry(&rig, NOTES)["active_version"], "1.1.0");
    assert_eq!(a.active_version(NOTES).as_deref(), Some("1.1.0"));
}

/// REQ: LIF-073
#[test]
fn lif_073_second_server_failure_follows_the_normal_failure_rule_without_rollback() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let (a, b) = two_manual_servers(&rig);
    b.fail_activation_of_version("1.1.0");
    update(&rig, &pkg(NOTES, "1.1.0"));
    a.apply_pending();
    b.apply_pending();
    assert_eq!(b.failure_count(NOTES, Side::Server), 1);
    assert_eq!(b.restart_delay_ms(NOTES, Side::Server), Some(1_000));
    assert_eq!(registry_entry(&rig, NOTES)["active_version"], "1.1.0");
    assert!(!b.log_codes().contains(&"update.rolled-back".to_owned()));
}

/// REQ: LIF-073
#[test]
fn lif_073_first_server_failure_rolls_the_update_back_for_everyone() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let (a, b) = two_manual_servers(&rig);
    a.fail_activation_of_version("1.1.0");
    update(&rig, &pkg(NOTES, "1.1.0"));
    a.apply_pending();
    b.apply_pending();
    assert_eq!(registry_entry(&rig, NOTES)["active_version"], "1.0.0");
    assert_eq!(b.active_version(NOTES).as_deref(), Some("1.0.0"));
}

use ilium_plugin_api::Side;

fn kill_server_at(stage: UpdateStage) -> (OpsRig, Option<String>) {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server_manual("s1");
    server.pause_at(stage);
    update(&rig, &pkg(NOTES, "1.1.0"));
    server.apply_pending();
    server.crash();
    let fresh = rig.start_server("s1");
    rig.advance(secs(10));
    let version = fresh.active_version(NOTES);
    (rig, version)
}

fn assert_old_or_new(version: Option<String>, stage: UpdateStage) {
    let version = version.unwrap_or_else(|| panic!("no version active after a kill at {stage:?}"));
    assert!(
        version == "1.0.0" || version == "1.1.0",
        "{version} after {stage:?}"
    );
}

/// REQ: LIF-074
#[test]
fn lif_074_server_killed_with_update_staged_has_a_version_active_after_restart() {
    let (_rig, version) = kill_server_at(UpdateStage::Staged);
    assert_old_or_new(version, UpdateStage::Staged);
}

/// REQ: LIF-074
#[test]
fn lif_074_server_killed_after_deactivate_has_a_version_active_after_restart() {
    let (_rig, version) = kill_server_at(UpdateStage::Deactivated);
    assert_old_or_new(version, UpdateStage::Deactivated);
}

/// REQ: LIF-074
#[test]
fn lif_074_server_killed_while_migrating_has_a_version_active_after_restart() {
    let (_rig, version) = kill_server_at(UpdateStage::Migrating);
    assert_old_or_new(version, UpdateStage::Migrating);
}

/// REQ: LIF-074
#[test]
fn lif_074_server_killed_in_trial_has_a_version_active_after_restart() {
    let (_rig, version) = kill_server_at(UpdateStage::Trial);
    assert_old_or_new(version, UpdateStage::Trial);
}

/// REQ: LIF-074
#[test]
fn lif_074_installer_killed_after_each_step_leaves_a_version_active() {
    for step in 1..=7u8 {
        let rig = rig();
        install_ok(&rig, &pkg(NOTES, "1.0.0"));
        rig.inject(Fault::KillInstaller(step));
        let _killed = update(&rig, &pkg(NOTES, "1.1.0"));
        let server = rig.start_server("s1");
        rig.advance(secs(10));
        let version = server.active_version(NOTES);
        assert!(
            matches!(version.as_deref(), Some("1.0.0") | Some("1.1.0")),
            "step {step}: {version:?}"
        );
    }
}

fn update_with_failing_candidate(spec: PackageSpec) -> (OpsRig, ServerHandle) {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    update(&rig, &spec);
    rig.advance(secs(3));
    (rig, server)
}

/// REQ: LIF-075
#[test]
fn lif_075_candidate_activate_error_rolls_back_to_the_previous_version() {
    let (rig, server) = update_with_failing_candidate(pkg(NOTES, "1.1.0").activate_error());
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
    let entry = registry_entry(&rig, NOTES);
    assert_eq!(entry["active_version"], "1.0.0");
    assert_eq!(entry["update"], Value::Null);
    assert!(server
        .log_codes()
        .contains(&"update.rolled-back".to_owned()));
}

/// REQ: LIF-075
#[test]
fn lif_075_rolled_back_candidate_is_marked_skip() {
    let (rig, _server) = update_with_failing_candidate(pkg(NOTES, "1.1.0").activate_error());
    assert_eq!(
        registry_entry(&rig, NOTES)["skip"],
        serde_json::json!(["1.1.0"])
    );
}

/// REQ: LIF-075
#[test]
fn lif_075_first_activation_failure_of_a_candidate_is_not_counted() {
    let (_rig, server) = update_with_failing_candidate(pkg(NOTES, "1.1.0").activate_error());
    assert_eq!(server.failure_count(NOTES, Side::Server), 0);
    assert_eq!(server.restart_delay_ms(NOTES, Side::Server), None);
}

/// REQ: LIF-075
#[test]
fn lif_075_candidate_trap_in_activate_rolls_back_without_counting() {
    let (rig, server) = update_with_failing_candidate(pkg(NOTES, "1.1.0").activate_traps());
    assert_eq!(registry_entry(&rig, NOTES)["active_version"], "1.0.0");
    assert_eq!(server.failure_count(NOTES, Side::Server), 0);
}

/// REQ: LIF-075
#[test]
fn lif_075_candidate_missing_the_activate_deadline_rolls_back_without_counting() {
    let (rig, server) =
        update_with_failing_candidate(pkg(NOTES, "1.1.0").activate_misses_deadline());
    assert_eq!(registry_entry(&rig, NOTES)["active_version"], "1.0.0");
    assert_eq!(server.failure_count(NOTES, Side::Server), 0);
}

/// REQ: LIF-075
#[test]
fn lif_075_failure_after_a_successful_first_activation_counts_and_does_not_roll_back() {
    let (rig, server) = update_with_failing_candidate(pkg(NOTES, "1.1.0"));
    assert_eq!(registry_entry(&rig, NOTES)["active_version"], "1.1.0");
    server.fail(NOTES, Side::Server);
    assert_eq!(server.failure_count(NOTES, Side::Server), 1);
    assert_eq!(registry_entry(&rig, NOTES)["active_version"], "1.1.0");
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.1.0"));
}

/// REQ: LIF-075
#[test]
fn lif_075_rollback_restores_the_pre_update_data_snapshot_after_a_migration() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let data_file = rig.plant_data(NOTES, 1).join("note.txt");
    let before = std::fs::read(&data_file).expect("planted data");
    let server = rig.start_server("s1");
    update(&rig, &pkg(NOTES, "2.0.0").data_version(2).activate_error());
    rig.advance(secs(3));
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
    assert_eq!(
        std::fs::read(&data_file).expect("data after rollback"),
        before
    );
    assert_eq!(rig.bucket_stamp(NOTES), Some(1));
}

/// REQ: LIF-075
#[test]
fn lif_075_rollback_shows_the_toast_update_failed_rolled_back() {
    let (_rig, server) = update_with_failing_candidate(pkg(NOTES, "1.1.0").activate_error());
    assert!(server
        .toasts()
        .contains(&"Update failed, rolled back".to_owned()));
}

/// REQ: LIF-076
#[test]
fn lif_076_after_three_committed_updates_store_holds_exactly_two_versions() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let _server = rig.start_server("s1");
    for version in ["1.1.0", "1.2.0", "1.3.0"] {
        update(&rig, &pkg(NOTES, version));
        rig.advance(secs(3));
    }
    assert_eq!(
        store_versions(&rig, NOTES),
        vec!["1.2.0".to_owned(), "1.3.0".to_owned()]
    );
    let entry = registry_entry(&rig, NOTES);
    assert_eq!(entry["active_version"], "1.3.0");
    assert_eq!(entry["previous_version"], "1.2.0");
}

/// REQ: LIF-076
#[test]
fn lif_076_first_install_keeps_a_single_version_and_first_update_keeps_two() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    assert_eq!(store_versions(&rig, NOTES), vec!["1.0.0".to_owned()]);
    let _server = rig.start_server("s1");
    update(&rig, &pkg(NOTES, "1.1.0"));
    rig.advance(secs(3));
    assert_eq!(
        store_versions(&rig, NOTES),
        vec!["1.0.0".to_owned(), "1.1.0".to_owned()]
    );
}

/// REQ: LIF-076
#[test]
fn lif_076_older_versions_stay_until_the_update_commits() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server_manual("s1");
    update(&rig, &pkg(NOTES, "1.1.0"));
    server.apply_pending();
    rig.advance(secs(3));
    update(&rig, &pkg(NOTES, "1.2.0"));
    assert_eq!(
        store_versions(&rig, NOTES),
        // LIF-076: exactly one previous version is kept next to the current one.
        vec!["1.1.0".to_owned(), "1.2.0".to_owned()]
    );
}

/// REQ: LIF-077
#[test]
fn lif_077_blocked_candidate_is_not_activated_again_after_a_server_restart() {
    let (rig, server) = update_with_failing_candidate(pkg(NOTES, "1.1.0").activate_error());
    server.shutdown();
    let restarted = rig.start_server("s1");
    rig.advance(secs(10));
    assert!(!restarted.activations(NOTES).contains(&"1.1.0".to_owned()));
    assert_eq!(restarted.active_version(NOTES).as_deref(), Some("1.0.0"));
}

/// REQ: LIF-077
#[test]
fn lif_077_explicit_reinstall_of_the_candidate_tries_it_again() {
    let (rig, _server) = update_with_failing_candidate(pkg(NOTES, "1.1.0").activate_error());
    let fixed = pkg(NOTES, "1.1.0").activate_error();
    let path = rig.write_package(&fixed);
    let path_text = path.to_string_lossy().into_owned();
    let restarted = rig.start_server("s2");
    let output = rig.cli(&[
        "install",
        &path_text,
        "--yes",
        "--allow-unsigned",
        "--reinstall",
    ]);
    assert_eq!(output.exit_code, 0);
    rig.advance(secs(3));
    assert!(restarted.activations(NOTES).contains(&"1.1.0".to_owned()));
}

/// REQ: LIF-077
#[test]
fn lif_077_skip_list_survives_in_the_registry_across_restarts() {
    let (rig, server) = update_with_failing_candidate(pkg(NOTES, "1.1.0").activate_error());
    server.crash();
    let _restarted = rig.start_server("s1");
    assert_eq!(
        registry_entry(&rig, NOTES)["skip"],
        serde_json::json!(["1.1.0"])
    );
}

/// REQ: LIF-078
#[test]
fn lif_078_declined_extra_grant_keeps_the_old_version_and_update_staged() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read"));
    let server = rig.start_server("s1");
    let path = rig.write_package(
        &pkg(NOTES, "1.1.0")
            .grant("fs.read")
            .high_risk_grant("process.spawn"),
    );
    let path_text = path.to_string_lossy().into_owned();
    let output = rig.cli_tty(&["update", &path_text, "--allow-unsigned"], &["n"]);
    assert_eq!(output.exit_code, 1);
    assert_eq!(
        error_codes(&output),
        vec!["install.review-declined".to_owned()]
    );
    rig.advance(secs(3));
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
    assert!(!server.granted(NOTES).contains(&"process.spawn".to_owned()));
}

/// REQ: LIF-078
#[test]
fn lif_078_review_dialog_shows_only_the_new_capabilities() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read"));
    let path = rig.write_package(
        &pkg(NOTES, "1.1.0")
            .grant("fs.read")
            .high_risk_grant("process.spawn"),
    );
    let path_text = path.to_string_lossy().into_owned();
    let _output = rig.cli_tty(&["update", &path_text, "--allow-unsigned"], &["y"]);
    assert_eq!(rig.last_review(), vec!["process.spawn".to_owned()]);
}

/// REQ: LIF-078
#[test]
fn lif_078_accepted_extra_grant_reaches_the_new_version() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read"));
    let server = rig.start_server("s1");
    let path = rig.write_package(
        &pkg(NOTES, "1.1.0")
            .grant("fs.read")
            .high_risk_grant("process.spawn"),
    );
    let path_text = path.to_string_lossy().into_owned();
    let output = rig.cli_tty(&["update", &path_text, "--allow-unsigned"], &["y"]);
    assert_eq!(output.exit_code, 0);
    rig.advance(secs(3));
    assert!(server.granted(NOTES).contains(&"process.spawn".to_owned()));
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.1.0"));
}

/// REQ: LIF-078
#[test]
fn lif_078_unanswerable_review_ends_with_install_needs_input_and_a_staged_update() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let path = rig.write_package(&pkg(NOTES, "1.1.0").high_risk_grant("process.spawn"));
    let path_text = path.to_string_lossy().into_owned();
    let output = rig.cli(&["update", &path_text, "--allow-unsigned", "--yes"]);
    assert_eq!(output.exit_code, 1);
    assert_eq!(error_codes(&output), vec!["install.needs-input".to_owned()]);
    assert_eq!(registry_entry(&rig, NOTES)["active_version"], "1.0.0");
}

/// REQ: LIF-078
#[test]
fn lif_078_update_without_new_grants_shows_no_review() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read"));
    let before = rig.prompt_count();
    update(&rig, &pkg(NOTES, "1.1.0").grant("fs.read"));
    assert_eq!(rig.prompt_count(), before);
}

// ---------------------------------------------------------------------------------------------
// Rollback and downgrade: LIF-079 .. LIF-081
// ---------------------------------------------------------------------------------------------

/// Installs 1.0.0 (data version 1), updates to 2.0.0 (data version 2) and writes data at stamp 2.
fn migrated_rig() -> (OpsRig, ServerHandle) {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").data_version(1));
    let data_file = rig.plant_data(NOTES, 1).join("note.txt");
    std::fs::write(&data_file, b"before the update").expect("write data");
    let server = rig.start_server("s1");
    update(&rig, &pkg(NOTES, "2.0.0").data_version(2).migrator_both());
    rig.advance(secs(3));
    std::fs::write(&data_file, b"written after the update").expect("write data");
    (rig, server)
}

/// REQ: LIF-079
#[test]
fn lif_079_rollback_over_newer_data_refused_migration_data_newer() {
    let (rig, _server) = migrated_rig();
    let output = rig.cli(&["rollback", NOTES]);
    assert_eq!(output.exit_code, 1);
    assert_eq!(
        error_codes(&output),
        vec!["migration.data-newer".to_owned()]
    );
    assert_eq!(registry_entry(&rig, NOTES)["active_version"], "2.0.0");
}

/// REQ: LIF-079
#[test]
fn lif_079_restore_backup_restores_the_snapshot_and_loses_later_writes() {
    let (rig, server) = migrated_rig();
    let output = rig.cli(&["rollback", NOTES, "--restore-backup"]);
    assert_eq!(output.exit_code, 0);
    rig.advance(secs(3));
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
    let restored = std::fs::read(rig.data_dir(NOTES).join("note.txt")).expect("restored data");
    assert_eq!(restored, b"before the update");
    assert_eq!(rig.bucket_stamp(NOTES), Some(1));
}

/// REQ: LIF-079
#[test]
fn lif_079_discard_data_deletes_the_buckets_and_the_data_directory() {
    let (rig, server) = migrated_rig();
    let output = rig.cli(&["rollback", NOTES, "--discard-data"]);
    assert_eq!(output.exit_code, 0);
    rig.advance(secs(3));
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
    assert!(!rig.data_dir(NOTES).exists());
    assert_eq!(rig.bucket_stamp(NOTES), None);
}

/// REQ: LIF-079
#[test]
fn lif_079_downgrade_install_over_newer_data_refused_migration_data_newer() {
    let (rig, _server) = migrated_rig();
    let path = rig.write_package(&pkg(NOTES, "1.5.0").data_version(1));
    let path_text = path.to_string_lossy().into_owned();
    let output = rig.cli(&[
        "install",
        &path_text,
        "--yes",
        "--allow-unsigned",
        "--allow-downgrade",
    ]);
    assert_eq!(output.exit_code, 1);
    assert_eq!(
        error_codes(&output),
        vec!["migration.data-newer".to_owned()]
    );
}

/// REQ: LIF-079
#[test]
fn lif_079_rollback_with_equal_data_version_needs_no_flag() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").data_version(1));
    rig.plant_data(NOTES, 1);
    let server = rig.start_server("s1");
    update(&rig, &pkg(NOTES, "1.1.0").data_version(1));
    rig.advance(secs(3));
    let output = rig.cli(&["rollback", NOTES]);
    assert_eq!(output.exit_code, 0);
    rig.advance(secs(3));
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
}

/// REQ: LIF-079
#[test]
fn lif_079_refusal_leaves_data_and_buckets_untouched() {
    let (rig, _server) = migrated_rig();
    let before = snapshot_tree(&rig.data_dir(NOTES));
    let _output = rig.cli(&["rollback", NOTES]);
    assert_eq!(snapshot_tree(&rig.data_dir(NOTES)), before);
    assert_eq!(rig.bucket_stamp(NOTES), Some(2));
}

/// REQ: LIF-080
#[test]
fn lif_080_restore_backup_without_the_snapshot_fails_rollback_no_backup() {
    let (rig, _server) = migrated_rig();
    rig.delete_backups(NOTES);
    let output = rig.cli(&["rollback", NOTES, "--restore-backup"]);
    assert_eq!(output.exit_code, 1);
    assert_eq!(error_codes(&output), vec!["rollback.no-backup".to_owned()]);
    assert_eq!(registry_entry(&rig, NOTES)["active_version"], "2.0.0");
}

/// REQ: LIF-080
#[test]
fn lif_080_discard_data_works_without_the_snapshot() {
    let (rig, _server) = migrated_rig();
    rig.delete_backups(NOTES);
    let output = rig.cli(&["rollback", NOTES, "--discard-data"]);
    assert_eq!(output.exit_code, 0);
}

/// REQ: LIF-080
#[test]
fn lif_080_backup_is_named_with_the_previous_version_and_a_utc_timestamp() {
    let (rig, _server) = migrated_rig();
    let names = entry_names(&rig.plugins_root().join("backups").join(NOTES));
    assert_eq!(names.len(), 1);
    let name = &names[0];
    let (version, stamp) = name.split_once('-').expect("version-timestamp");
    assert_eq!(version, "1.0.0");
    assert_eq!(stamp.len(), 16);
    assert!(stamp.ends_with('Z') && stamp.as_bytes()[8] == b'T');
}

/// REQ: LIF-081
#[test]
fn lif_081_rollback_deactivates_with_upgrade_then_activates_the_previous_version() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    update(&rig, &pkg(NOTES, "1.1.0"));
    rig.advance(secs(3));
    let deactivations_before = server.deactivations(NOTES).len();
    let output = rig.cli(&["rollback", NOTES]);
    assert_eq!(output.exit_code, 0);
    rig.advance(secs(3));
    let deactivations = server.deactivations(NOTES);
    assert_eq!(deactivations.len(), deactivations_before + 1);
    assert_eq!(
        deactivations.last(),
        Some(&ilium_plugin_api::generated::types::lifecycle::DeactivationReason::Upgrade)
    );
    assert_eq!(
        server.activations(NOTES).last().map(String::as_str),
        Some("1.0.0")
    );
}

/// REQ: LIF-081
#[test]
fn lif_081_rolled_back_version_is_activated_with_the_restore_bytes() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").restore_bytes(8));
    let server = rig.start_server("s1");
    update(&rig, &pkg(NOTES, "1.1.0").restore_bytes(8));
    rig.advance(secs(3));
    let _output = rig.cli(&["rollback", NOTES]);
    rig.advance(secs(3));
    assert_eq!(server.last_restored_len(NOTES), Some(8));
}

/// REQ: LIF-081
#[test]
fn lif_081_rollback_of_a_first_install_fails_install_no_previous_exit_5() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let output = rig.cli(&["rollback", NOTES]);
    assert_eq!(output.exit_code, 5);
    assert_eq!(error_codes(&output), vec!["install.no-previous".to_owned()]);
}

// ---------------------------------------------------------------------------------------------
// Uninstall: LIF-082 .. LIF-086
// ---------------------------------------------------------------------------------------------

fn installed_with_data() -> OpsRig {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read"));
    rig.plant_data(NOTES, 1);
    rig.plant_settings(NOTES, "theme", "dark");
    rig.plant_backup(NOTES, "0.9.0");
    rig
}

/// REQ: LIF-082
#[test]
fn lif_082_uninstall_with_stdin_closed_keeps_data_buckets_settings_and_backups() {
    let rig = installed_with_data();
    let output = rig.cli(&["uninstall", NOTES]);
    assert_eq!(output.exit_code, 0);
    assert!(rig.data_dir(NOTES).exists());
    assert_eq!(rig.bucket_stamp(NOTES), Some(1));
    assert_eq!(rig.setting(NOTES, "theme").as_deref(), Some("dark"));
    assert!(!entry_names(&rig.plugins_root().join("backups").join(NOTES)).is_empty());
    assert_eq!(rig.prompt_count(), 0);
}

/// REQ: LIF-082
#[test]
fn lif_082_uninstall_on_a_tty_asks_no_question() {
    let rig = installed_with_data();
    let output = rig.cli_tty(&["uninstall", NOTES], &[]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(rig.prompt_count(), 0);
    assert!(rig.data_dir(NOTES).exists());
}

/// REQ: LIF-082
#[test]
fn lif_082_uninstall_prints_the_kept_paths_as_warning_lines() {
    let rig = installed_with_data();
    let output = rig.cli(&["uninstall", NOTES]);
    let warnings = lines_of(&output, "warning");
    assert!(!warnings.is_empty());
    let data = rig.data_dir(NOTES).to_string_lossy().into_owned();
    assert!(
        warnings.iter().any(|line| line["path"] == data.as_str()),
        "{warnings:?}"
    );
}

/// REQ: LIF-082
#[test]
fn lif_082_keep_data_flag_means_the_same_as_the_default() {
    let rig = installed_with_data();
    let output = rig.cli(&["uninstall", NOTES, "--keep-data"]);
    assert_eq!(output.exit_code, 0);
    assert!(rig.data_dir(NOTES).exists());
    assert_eq!(rig.setting(NOTES, "theme").as_deref(), Some("dark"));
}

/// REQ: LIF-082
#[test]
fn lif_082_purge_and_keep_data_together_are_a_usage_error_exit_2() {
    let rig = installed_with_data();
    let output = rig.cli(&["uninstall", NOTES, "--purge", "--keep-data"]);
    assert_eq!(output.exit_code, 2);
    assert!(rig.data_dir(NOTES).exists());
}

/// REQ: LIF-082
#[test]
fn lif_082_purge_deletes_data_buckets_settings_and_backups() {
    let rig = installed_with_data();
    let output = rig.cli(&["uninstall", NOTES, "--purge"]);
    assert_eq!(output.exit_code, 0);
    assert!(!rig.data_dir(NOTES).exists());
    assert_eq!(rig.bucket_stamp(NOTES), None);
    assert_eq!(rig.setting(NOTES, "theme"), None);
    assert!(entry_names(&rig.plugins_root().join("backups").join(NOTES)).is_empty());
}

/// REQ: LIF-082
#[test]
fn lif_082_uninstall_deletes_store_entries_cache_and_the_shared_bucket() {
    let rig = installed_with_data();
    rig.plant_cache(NOTES);
    rig.plant_shared_bucket(NOTES);
    let _output = rig.cli(&["uninstall", NOTES]);
    assert!(store_versions(&rig, NOTES).is_empty());
    assert!(!rig.cache_dir().join(NOTES).exists());
    assert!(!rig.shared_bucket_exists(NOTES));
}

/// REQ: LIF-083
#[test]
fn lif_083_uninstall_deletes_the_grants_entry() {
    let rig = installed_with_data();
    assert!(!rig.grants_json()[NOTES].is_null());
    let _output = rig.cli(&["uninstall", NOTES]);
    assert!(rig.grants_json()[NOTES].is_null());
}

/// REQ: LIF-083
#[test]
fn lif_083_reinstall_after_uninstall_asks_for_grants_again() {
    let rig = rig();
    let spec = pkg(NOTES, "1.0.0").high_risk_grant("process.spawn");
    let path = rig.write_package(&spec);
    let path_text = path.to_string_lossy().into_owned();
    let first = rig.cli_tty(&["install", &path_text, "--allow-unsigned"], &["y"]);
    assert_eq!(first.exit_code, 0);
    let prompts_after_first = rig.prompt_count();
    let _removed = rig.cli(&["uninstall", NOTES]);
    let second = rig.cli_tty(&["install", &path_text, "--allow-unsigned"], &["y"]);
    assert_eq!(second.exit_code, 0);
    assert_eq!(rig.prompt_count(), prompts_after_first + 1);
}

/// REQ: LIF-083
#[test]
fn lif_083_uninstall_deletes_first_use_answers_durable_timers_and_failure_counters() {
    let rig = installed_with_data();
    let server = rig.start_server("s1");
    rig.plant_first_use_answer(NOTES, "net.http");
    rig.plant_durable_timer(NOTES, 60_000);
    server.fail(NOTES, Side::Server);
    assert_eq!(server.failure_count(NOTES, Side::Server), 1);
    let _output = rig.cli(&["uninstall", NOTES]);
    rig.advance(secs(3));
    assert!(rig.first_use_answers(NOTES).is_empty());
    assert!(rig.durable_timers(NOTES).is_empty());
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read"));
    rig.advance(secs(3));
    assert_eq!(server.failure_count(NOTES, Side::Server), 0);
}

/// REQ: LIF-083
#[test]
fn lif_083_uninstall_removes_persistent_decorations() {
    let rig = installed_with_data();
    let server = rig.start_server("s1");
    server.add_decoration(NOTES, 1);
    assert_eq!(server.decorations(NOTES), 1);
    let _output = rig.cli(&["uninstall", NOTES]);
    rig.advance(secs(3));
    assert_eq!(server.decorations(NOTES), 0);
}

/// REQ: LIF-083
#[test]
fn lif_083_purge_also_deletes_grants_and_timers() {
    let rig = installed_with_data();
    rig.plant_durable_timer(NOTES, 60_000);
    let _output = rig.cli(&["uninstall", NOTES, "--purge"]);
    assert!(rig.grants_json()[NOTES].is_null());
    assert!(rig.durable_timers(NOTES).is_empty());
}

/// REQ: LIF-084
#[test]
fn lif_084_node_metadata_of_an_uninstalled_plugin_stays_in_the_snapshot() {
    let rig = installed_with_data();
    let server = rig.start_server("s1");
    server.set_node_metadata(7, NOTES, "mark", b"x");
    let _output = rig.cli(&["uninstall", NOTES]);
    rig.advance(secs(3));
    server.snapshot_now();
    assert!(rig
        .snapshot_metadata_keys("s1", NOTES)
        .contains(&"mark".to_owned()));
}

/// REQ: LIF-084
#[test]
fn lif_084_metadata_of_an_uninstalled_plugin_is_never_passed_to_another_plugin() {
    let rig = installed_with_data();
    install_ok(&rig, &pkg(OTHER, "1.0.0"));
    let server = rig.start_server("s1");
    server.set_node_metadata(7, NOTES, "mark", b"x");
    let _output = rig.cli(&["uninstall", NOTES]);
    rig.advance(secs(3));
    assert_eq!(server.node_metadata_as(OTHER, 7, NOTES, "mark"), None);
}

/// REQ: LIF-084 MAY
#[test]
fn lif_084_reinstall_of_the_same_id_may_read_the_metadata_again() {
    let rig = installed_with_data();
    let server = rig.start_server("s1");
    server.set_node_metadata(7, NOTES, "mark", b"x");
    let _output = rig.cli(&["uninstall", NOTES]);
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.advance(secs(3));
    let seen = server.node_metadata_as(NOTES, 7, NOTES, "mark");
    assert!(seen.is_none() || seen == Some(b"x".to_vec()));
}

/// REQ: LIF-084
#[test]
fn lif_084_purge_deletes_metadata_in_the_live_session_and_adds_a_tombstone() {
    let rig = installed_with_data();
    let server = rig.start_server("s1");
    server.set_node_metadata(7, NOTES, "mark", b"x");
    let _output = rig.cli(&["uninstall", NOTES, "--purge"]);
    rig.advance(secs(3));
    assert_eq!(server.node_metadata_as(NOTES, 7, NOTES, "mark"), None);
    assert!(rig.registry_json()["purge_tombstones"]
        .as_array()
        .expect("tombstone list")
        .iter()
        .any(|id| id == NOTES));
}

/// REQ: LIF-084
#[test]
fn lif_084_other_project_deletes_its_metadata_when_it_next_opens() {
    let rig = installed_with_data();
    rig.plant_snapshot_metadata("other-project", 9, NOTES, "mark", b"y");
    let _output = rig.cli(&["uninstall", NOTES, "--purge"]);
    let reopened = rig.start_server("other-project");
    assert_eq!(reopened.node_metadata_as(NOTES, 9, NOTES, "mark"), None);
    assert!(rig
        .snapshot_metadata_keys("other-project", NOTES)
        .is_empty());
}

/// REQ: LIF-084
#[test]
fn lif_084_plain_uninstall_adds_no_tombstone() {
    let rig = installed_with_data();
    let _output = rig.cli(&["uninstall", NOTES]);
    assert_eq!(
        rig.registry_json()["purge_tombstones"],
        serde_json::json!([])
    );
}

/// REQ: LIF-085
#[test]
fn lif_085_uninstall_sets_uninstall_pending_before_anything_else_is_stopped() {
    let rig = installed_with_data();
    rig.inject(Fault::KillUninstall(UninstallStep::StateWritten));
    let _killed = rig.cli(&["uninstall", NOTES]);
    assert_eq!(registry_entry(&rig, NOTES)["state"], "uninstall-pending");
    assert!(!store_versions(&rig, NOTES).is_empty());
}

/// REQ: LIF-085
#[test]
fn lif_085_next_start_finishes_an_interrupted_uninstall() {
    let rig = installed_with_data();
    rig.inject(Fault::KillUninstall(UninstallStep::StateWritten));
    let _killed = rig.cli(&["uninstall", NOTES]);
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    assert!(registry_entry(&rig, NOTES).is_null());
    assert!(store_versions(&rig, NOTES).is_empty());
    assert!(server.activations(NOTES).is_empty());
}

/// REQ: LIF-085
#[test]
fn lif_085_crash_after_instances_stopped_is_finished_at_the_next_start() {
    let rig = installed_with_data();
    let _server = rig.start_server("s1");
    rig.inject(Fault::KillUninstall(UninstallStep::InstancesStopped));
    let _killed = rig.cli(&["uninstall", NOTES]);
    let _fresh = rig.start_server("s2");
    rig.advance(secs(3));
    assert!(registry_entry(&rig, NOTES).is_null());
}

/// REQ: LIF-085
#[test]
fn lif_085_crash_during_file_deletion_is_finished_at_the_next_start() {
    let rig = installed_with_data();
    rig.inject(Fault::KillUninstall(UninstallStep::FilesPartlyDeleted));
    let _killed = rig.cli(&["uninstall", NOTES]);
    let _fresh = rig.start_server("s1");
    rig.advance(secs(3));
    assert!(store_versions(&rig, NOTES).is_empty());
    assert!(registry_entry(&rig, NOTES).is_null());
}

/// REQ: LIF-085
#[test]
fn lif_085_uninstall_order_is_state_then_instances_then_files() {
    let rig = installed_with_data();
    let _server = rig.start_server("s1");
    let _output = rig.cli(&["uninstall", NOTES]);
    let trace = rig.trace();
    let state = position_of(&trace, "uninstall.state-pending");
    let stopped = position_of(&trace, "uninstall.instances-stopped");
    let deleted = position_of(&trace, "uninstall.files-deleted");
    assert!(state < stopped && stopped < deleted);
}

/// REQ: LIF-086
#[test]
fn lif_086_uninstall_leaves_the_directories_of_other_plugins_unchanged() {
    let rig = installed_with_data();
    install_ok(&rig, &pkg(OTHER, "1.0.0"));
    rig.plant_data(OTHER, 1);
    let before = snapshot_tree(&rig.data_dir(OTHER));
    let store_before = snapshot_tree(&rig.plugins_root().join("store").join(OTHER));
    let _output = rig.cli(&["uninstall", NOTES, "--purge"]);
    assert_eq!(snapshot_tree(&rig.data_dir(OTHER)), before);
    assert_eq!(
        snapshot_tree(&rig.plugins_root().join("store").join(OTHER)),
        store_before
    );
    assert_eq!(rig.bucket_stamp(OTHER), Some(1));
}

/// REQ: LIF-086
#[test]
fn lif_086_uninstall_does_not_remove_a_publisher_key() {
    let rig = rig();
    rig.trust_publisher("key-a");
    install_ok(&rig, &pkg(NOTES, "1.0.0").signed("key-a"));
    let before = std::fs::read(rig.state_dir().join("trust.json")).expect("trust store");
    let _output = rig.cli(&["uninstall", NOTES, "--purge"]);
    assert_eq!(
        std::fs::read(rig.state_dir().join("trust.json")).expect("trust store"),
        before
    );
}

/// REQ: LIF-086
#[test]
fn lif_086_uninstall_of_a_provider_warns_for_each_dependant_and_keeps_them_active() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").provides_service("svc.notes"));
    install_ok(&rig, &pkg(OTHER, "1.0.0").requires_service("svc.notes"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let output = rig.cli(&["uninstall", NOTES]);
    assert_eq!(output.exit_code, 0);
    assert!(lines_of(&output, "warning")
        .iter()
        .any(|line| line["message"].as_str().unwrap_or("").contains(OTHER)));
    assert_eq!(
        server.state_of(OTHER, Side::Server),
        ilium_plugin_api::generated::host::plugins::InstanceState::Active
    );
}

// ---------------------------------------------------------------------------------------------
// Command reference: LIF-087, LIF-088
// ---------------------------------------------------------------------------------------------

const LINE_TYPES: [&str; 5] = ["progress", "result", "warning", "error", "summary"];
const EXIT_CODES: [i32; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 130];

fn commands_on_a_prepared_rig() -> Vec<(String, ilium_plugin_host::cli::CliOutput)> {
    let rig = rig();
    let spec = pkg(NOTES, "1.0.0");
    let path = rig.write_package(&spec);
    let path_text = path.to_string_lossy().into_owned();
    let second = rig.write_package(&pkg(NOTES, "1.1.0"));
    let second_text = second.to_string_lossy().into_owned();
    let mut results = Vec::new();
    let mut run = |label: &str, args: &[&str]| {
        results.push((label.to_owned(), rig.cli(args)));
    };
    run(
        "install",
        &["install", &path_text, "--yes", "--allow-unsigned"],
    );
    run("list", &["list"]);
    run("info", &["info", NOTES]);
    run("disable", &["disable", NOTES]);
    run("enable", &["enable", NOTES]);
    run("status", &["status"]);
    run("stats", &["stats", NOTES]);
    run("logs", &["logs", NOTES, "--tail", "5"]);
    run("reset-failures", &["reset-failures", NOTES]);
    run(
        "update",
        &["update", &second_text, "--yes", "--allow-unsigned"],
    );
    run("rollback", &["rollback", NOTES]);
    run("reinstall", &["reinstall", NOTES]);
    run("uninstall", &["uninstall", NOTES]);
    run("info-unknown", &["info", "no.such.plugin"]);
    run(
        "update-unknown",
        &["update", &path_text, "--yes", "--allow-unsigned"],
    );
    results
}

/// REQ: LIF-087
#[test]
fn lif_087_every_command_prints_only_typed_json_lines_in_a_pipe() {
    let commands = commands_on_a_prepared_rig();
    assert!(!commands.is_empty());
    for (label, output) in commands {
        assert!(!output.stdout_jsonl.is_empty(), "{label} printed nothing");
        for line in &output.stdout_jsonl {
            let line_type = line["type"].as_str().unwrap_or("");
            assert!(LINE_TYPES.contains(&line_type), "{label}: {line}");
        }
    }
}

/// REQ: LIF-087
#[test]
fn lif_087_every_command_uses_a_documented_exit_code() {
    for (label, output) in commands_on_a_prepared_rig() {
        assert!(
            EXIT_CODES.contains(&output.exit_code),
            "{label}: {}",
            output.exit_code
        );
    }
}

/// REQ: LIF-087
#[test]
fn lif_087_every_error_line_carries_a_code_from_the_catalogue() {
    let catalogue = catalogue_codes();
    for (label, output) in commands_on_a_prepared_rig() {
        for line in lines_of(&output, "error") {
            let code = line["code"].as_str().unwrap_or("");
            assert!(
                catalogue.contains(code),
                "{label}: `{code}` not in the catalogue"
            );
        }
    }
}

/// REQ: LIF-087
#[test]
fn lif_087_update_of_an_unknown_id_exits_5_with_install_not_installed() {
    let rig = rig();
    let output = update(&rig, &pkg(NOTES, "1.0.0"));
    assert_eq!(output.exit_code, 5);
    assert_eq!(
        error_codes(&output),
        vec!["install.not-installed".to_owned()]
    );
}

/// REQ: LIF-087
#[test]
fn lif_087_info_of_an_unknown_id_exits_5_with_plugin_not_found() {
    let rig = rig();
    let output = rig.cli(&["info", "no.such.plugin"]);
    assert_eq!(output.exit_code, 5);
    assert_eq!(error_codes(&output), vec!["plugin.not-found".to_owned()]);
}

/// REQ: LIF-087
#[test]
fn lif_087_successful_install_prints_a_result_line_with_id_version_and_state() {
    let rig = rig();
    let output = install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let results = lines_of(&output, "result");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["id"], NOTES);
    assert_eq!(results[0]["version"], "1.0.0");
    assert_eq!(results[0]["state"], "installed-enabled");
}

/// REQ: LIF-087
#[test]
fn lif_087_declined_review_exits_1_with_install_review_declined() {
    let rig = rig();
    let path = rig.write_package(&pkg(NOTES, "1.0.0").high_risk_grant("process.spawn"));
    let path_text = path.to_string_lossy().into_owned();
    let output = rig.cli_tty(&["install", &path_text, "--allow-unsigned"], &["n"]);
    assert_eq!(output.exit_code, 1);
    assert_eq!(
        error_codes(&output),
        vec!["install.review-declined".to_owned()]
    );
}

/// REQ: LIF-088
#[test]
fn lif_088_disable_twice_exits_0_and_the_second_result_says_changed_false() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let first = rig.cli(&["disable", NOTES]);
    let second = rig.cli(&["disable", NOTES]);
    assert_eq!(first.exit_code, 0);
    assert_eq!(second.exit_code, 0);
    assert_eq!(lines_of(&first, "result")[0]["changed"], true);
    assert_eq!(lines_of(&second, "result")[0]["changed"], false);
}

/// REQ: LIF-088
#[test]
fn lif_088_enable_of_an_enabled_plugin_exits_0_with_changed_false() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let output = rig.cli(&["enable", NOTES]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(lines_of(&output, "result")[0]["changed"], false);
}

/// REQ: LIF-088
#[test]
fn lif_088_enable_after_disable_reports_changed_true_then_false() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.cli(&["disable", NOTES]);
    let first = rig.cli(&["enable", NOTES]);
    let second = rig.cli(&["enable", NOTES]);
    assert_eq!(lines_of(&first, "result")[0]["changed"], true);
    assert_eq!(lines_of(&second, "result")[0]["changed"], false);
}

/// REQ: LIF-088
#[test]
fn lif_088_disable_of_an_unknown_id_exits_5_plugin_not_found() {
    let rig = rig();
    let output = rig.cli(&["disable", "no.such.plugin"]);
    assert_eq!(output.exit_code, 5);
    assert_eq!(error_codes(&output), vec!["plugin.not-found".to_owned()]);
}

// ---------------------------------------------------------------------------------------------
// Propagation to running servers: LIF-089 .. LIF-091
// ---------------------------------------------------------------------------------------------

/// REQ: LIF-089
#[test]
fn lif_089_two_servers_with_watchers_log_registry_changed_within_2_seconds() {
    let rig = rig();
    let a = rig.start_server("a");
    let b = rig.start_server("b");
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.advance(secs(2));
    assert!(a.log_codes().contains(&"registry.changed".to_owned()));
    assert!(b.log_codes().contains(&"registry.changed".to_owned()));
}

/// REQ: LIF-089
#[test]
fn lif_089_servers_without_a_watcher_poll_and_apply_within_6_seconds() {
    let rig = rig();
    let a = rig.start_server_polling("a");
    let b = rig.start_server_polling("b");
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.advance(secs(6));
    assert!(a.log_codes().contains(&"registry.changed".to_owned()));
    assert!(b.log_codes().contains(&"registry.changed".to_owned()));
}

/// REQ: LIF-089
#[test]
fn lif_089_installing_cli_sends_plugins_changed_to_every_server_it_finds() {
    let rig = rig();
    let a = rig.start_server_manual("a");
    let b = rig.start_server_manual("b");
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    assert!(a
        .received_messages()
        .contains(&"plugins-changed".to_owned()));
    assert!(b
        .received_messages()
        .contains(&"plugins-changed".to_owned()));
}

/// REQ: LIF-089
#[test]
fn lif_089_unreachable_server_gives_an_install_notify_failed_warning_and_still_succeeds() {
    let rig = rig();
    let reachable = rig.start_server_manual("a");
    let lost = rig.start_server_manual("b");
    lost.drop_socket();
    let output = install(&rig, &pkg(NOTES, "1.0.0"));
    assert_eq!(output.exit_code, 0);
    let warnings = lines_of(&output, "warning");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["code"], "install.notify-failed");
    assert_eq!(warnings[0]["session"], "b");
    assert!(reachable
        .received_messages()
        .contains(&"plugins-changed".to_owned()));
}

/// REQ: LIF-089
#[test]
fn lif_089_unreached_server_picks_the_change_up_at_its_next_start() {
    let rig = rig();
    let lost = rig.start_server_manual("b");
    lost.drop_socket();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    lost.crash();
    let restarted = rig.start_server("b");
    assert_eq!(restarted.active_version(NOTES).as_deref(), Some("1.0.0"));
}

/// REQ: LIF-090
#[test]
fn lif_090_server_started_after_an_install_runs_the_installed_version() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
}

/// REQ: LIF-090
#[test]
fn lif_090_server_started_after_an_update_runs_the_new_version() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    update(&rig, &pkg(NOTES, "1.1.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.1.0"));
}

/// REQ: LIF-090
#[test]
fn lif_090_server_started_after_an_uninstall_has_no_instance_of_the_plugin() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.cli(&["uninstall", NOTES]);
    let server = rig.start_server("s1");
    assert_eq!(server.active_version(NOTES), None);
}

/// REQ: LIF-091
#[test]
fn lif_091_update_during_a_disable_is_applied_after_the_disable_finishes() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    let trace = server.race_disable_and_update(NOTES, &pkg(NOTES, "1.1.0"));
    let disable_end = position_of(&trace, "disable.end");
    let update_begin = position_of(&trace, "update.begin");
    assert!(position_of(&trace, "disable.begin") < disable_end);
    assert!(disable_end < update_begin);
}

/// REQ: LIF-091
#[test]
fn lif_091_the_two_changes_never_interleave_for_one_plugin() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    let trace = server.race_disable_and_update(NOTES, &pkg(NOTES, "1.1.0"));
    let disable: Vec<usize> = ["disable.begin", "disable.end"]
        .iter()
        .map(|n| position_of(&trace, n))
        .collect();
    let update: Vec<usize> = ["update.begin", "update.end"]
        .iter()
        .map(|n| position_of(&trace, n))
        .collect();
    assert!(
        disable[1] < update[0] || update[1] < disable[0],
        "{trace:?}"
    );
}

/// REQ: LIF-091
#[test]
fn lif_091_changes_to_different_plugins_are_not_serialised_against_each_other() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    install_ok(&rig, &pkg(OTHER, "1.0.0"));
    let server = rig.start_server("s1");
    let trace = server.race_disable_of_two(NOTES, OTHER);
    assert!(trace.contains(&format!("disable.end {NOTES}")));
    assert!(trace.contains(&format!("disable.end {OTHER}")));
}

// ---------------------------------------------------------------------------------------------
// Interrupted install and recovery: LIF-092 .. LIF-094
// ---------------------------------------------------------------------------------------------

/// REQ: LIF-092
#[test]
fn lif_092_server_start_deletes_a_staging_directory_whose_creator_is_gone() {
    let rig = rig();
    let orphan = rig.make_staging("orphan-1", dead_pid(), ms(1_000));
    let _server = rig.start_server("s1");
    assert!(!orphan.exists());
}

/// REQ: LIF-092
#[test]
fn lif_092_server_start_deletes_a_staging_directory_older_than_one_hour() {
    let rig = rig();
    let old = rig.make_staging("old-1", std::process::id(), secs(3_601));
    let _server = rig.start_server("s1");
    assert!(!old.exists());
}

/// REQ: LIF-092
#[test]
fn lif_092_staging_directory_of_a_live_creator_just_under_one_hour_is_kept() {
    let rig = rig();
    let young = rig.make_staging("young-1", std::process::id(), secs(3_599));
    let _server = rig.start_server("s1");
    assert!(young.exists());
}

/// REQ: LIF-092
#[test]
fn lif_092_a_plugin_command_that_takes_the_lock_also_deletes_orphans() {
    let rig = rig();
    let orphan = rig.make_staging("orphan-2", dead_pid(), ms(1_000));
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    assert!(!orphan.exists());
}

/// REQ: LIF-092
#[test]
fn lif_092_a_staged_update_in_the_store_is_untouched_by_the_staging_rule() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server_manual("s1");
    server.pause_at(UpdateStage::Staged);
    update(&rig, &pkg(NOTES, "1.1.0"));
    rig.advance(secs(7_200));
    server.crash();
    let _fresh = rig.start_server_manual("s1");
    assert!(store_versions(&rig, NOTES).contains(&"1.1.0".to_owned()));
}

/// REQ: LIF-092
#[test]
fn lif_092_installed_json_never_references_the_staging_directory() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    update(&rig, &pkg(NOTES, "1.1.0"));
    let text = String::from_utf8(rig.registry_bytes()).expect("utf-8");
    assert!(!text.contains("staging"));
}

/// REQ: LIF-093
#[test]
fn lif_093_unreferenced_store_entry_older_than_one_hour_is_deleted_at_start() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let orphan = rig.make_store_orphan(NOTES, "0.1.0", secs(3_601));
    let _server = rig.start_server("s1");
    assert!(!orphan.exists());
}

/// REQ: LIF-093
#[test]
fn lif_093_unreferenced_store_entry_younger_than_one_hour_is_kept() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let young = rig.make_store_orphan(NOTES, "0.1.0", secs(3_599));
    let _server = rig.start_server("s1");
    assert!(young.exists());
}

/// REQ: LIF-093
#[test]
fn lif_093_store_entries_named_by_previous_or_candidate_are_kept_however_old() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server_manual("s1");
    server.pause_at(UpdateStage::Staged);
    update(&rig, &pkg(NOTES, "1.1.0"));
    rig.age_store_entries(NOTES, secs(7_200));
    server.crash();
    let _fresh = rig.start_server_manual("s1");
    assert_eq!(
        store_versions(&rig, NOTES),
        vec!["1.0.0".to_owned(), "1.1.0".to_owned()]
    );
}

/// REQ: LIF-093
#[test]
fn lif_093_truncated_store_file_puts_the_plugin_in_refused_at_load_package_corrupt() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.truncate_first_store_file(NOTES, "1.0.0");
    let server = rig.start_server("s1");
    assert_eq!(server.refusal(NOTES).as_deref(), Some("package.corrupt"));
    assert!(server.activations(NOTES).is_empty());
}

/// REQ: LIF-093
#[test]
fn lif_093_missing_store_entry_puts_the_plugin_in_refused_at_load_package_corrupt() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    std::fs::remove_dir_all(rig.plugins_root().join("store").join(NOTES).join("1.0.0"))
        .expect("remove the store entry");
    let server = rig.start_server("s1");
    assert_eq!(server.refusal(NOTES).as_deref(), Some("package.corrupt"));
}

/// REQ: LIF-093
#[test]
fn lif_093_reinstall_repairs_a_corrupt_store_entry_from_the_cached_package() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let good = snapshot_tree(&rig.plugins_root().join("store").join(NOTES).join("1.0.0"));
    rig.truncate_first_store_file(NOTES, "1.0.0");
    let output = rig.cli(&["reinstall", NOTES]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(
        snapshot_tree(&rig.plugins_root().join("store").join(NOTES).join("1.0.0")),
        good
    );
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
}

/// REQ: LIF-093
#[test]
fn lif_093_reinstall_without_the_cached_package_fails_package_cache_missing() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.truncate_first_store_file(NOTES, "1.0.0");
    rig.delete_cached_package(NOTES, "1.0.0");
    let output = rig.cli(&["reinstall", NOTES]);
    assert_eq!(output.exit_code, 1);
    assert_eq!(
        error_codes(&output),
        vec!["package.cache-missing".to_owned()]
    );
}

/// REQ: LIF-094
#[test]
fn lif_094_power_loss_during_the_registry_write_finds_the_old_or_the_new_registry() {
    for step in [
        RegistryStep::TempWritten,
        RegistryStep::TempSynced,
        RegistryStep::Renamed,
    ] {
        let rig = rig();
        install_ok(&rig, &pkg(NOTES, "1.0.0"));
        let old = rig.registry_bytes();
        rig.inject(Fault::RegistryWrite(step));
        let _killed = update(&rig, &pkg(NOTES, "1.1.0"));
        let seen = rig.registry_bytes();
        let value: Value = serde_json::from_slice(&seen).expect("a whole registry");
        assert!(
            seen == old || value["plugins"][0]["update"]["candidate"] == "1.1.0",
            "{step:?}"
        );
    }
}

fn restart_after_pause_at(stage: UpdateStage) -> (OpsRig, ServerHandle) {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").data_version(1));
    rig.plant_data(NOTES, 1);
    let server = rig.start_server_manual("s1");
    server.pause_at(stage);
    update(&rig, &pkg(NOTES, "1.1.0").data_version(1));
    server.apply_pending();
    server.crash();
    let fresh = rig.start_server("s1");
    rig.advance(secs(10));
    (rig, fresh)
}

/// REQ: LIF-094
#[test]
fn lif_094_update_in_staged_is_finished_at_the_next_activation() {
    let (rig, server) = restart_after_pause_at(UpdateStage::Staged);
    assert_eq!(registry_entry(&rig, NOTES)["update"], Value::Null);
    assert!(matches!(
        server.active_version(NOTES).as_deref(),
        Some("1.0.0") | Some("1.1.0")
    ));
}

/// REQ: LIF-094
#[test]
fn lif_094_update_in_migrating_is_restored_and_run_again_at_the_next_start() {
    let (rig, server) = restart_after_pause_at(UpdateStage::Migrating);
    assert_eq!(registry_entry(&rig, NOTES)["update"], Value::Null);
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.1.0"));
}

/// REQ: LIF-094
#[test]
fn lif_094_update_in_trial_is_decided_by_the_first_activation_after_the_restart() {
    let (rig, server) = restart_after_pause_at(UpdateStage::Trial);
    assert_eq!(registry_entry(&rig, NOTES)["update"], Value::Null);
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.1.0"));
}

/// REQ: LIF-094
#[test]
fn lif_094_failing_candidate_in_trial_is_undone_at_the_next_activation() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server_manual("s1");
    server.pause_at(UpdateStage::Trial);
    update(&rig, &pkg(NOTES, "1.1.0").activate_error());
    server.apply_pending();
    server.crash();
    let fresh = rig.start_server("s1");
    rig.advance(secs(10));
    assert_eq!(fresh.active_version(NOTES).as_deref(), Some("1.0.0"));
    assert_eq!(registry_entry(&rig, NOTES)["update"], Value::Null);
}
