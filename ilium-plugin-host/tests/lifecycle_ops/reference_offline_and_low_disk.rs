//! Reference, offline and low disk (LIF-170..176).

use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::Side;

use crate::common::*;

/// REQ: LIF-170
#[test]
fn lif_170_installed_plugins_load_activate_and_run_with_no_network() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    install_ok(&rig, &pkg(OTHER, "1.0.0").client_side());
    rig.block_network();
    let server = rig.start_server("s1");
    rig.advance(secs(5));
    assert_eq!(server.state_of(NOTES, Side::Server), InstanceState::Active);
    assert_eq!(server.state_of(OTHER, Side::Server), InstanceState::Active);
    assert_eq!(rig.network_attempts(), 0);
}

/// REQ: LIF-170
#[test]
fn lif_170_a_plugin_that_fails_a_network_call_is_not_counted_as_failed_by_the_host() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("http:example.invalid"));
    rig.block_network();
    let server = rig.start_server("s1");
    rig.advance(secs(5));
    let _ = server.call_plugin(NOTES, "fetch");
    assert_eq!(server.failure_count(NOTES, Side::Server), 0);
}

/// REQ: LIF-170
#[test]
fn lif_170_every_fixture_package_starts_offline() {
    let rig = rig();
    for index in 0..4 {
        install_ok(&rig, &pkg(&format!("com.example.p{index}"), "1.0.0"));
    }
    rig.block_network();
    let server = rig.start_server("s1");
    rig.advance(secs(5));
    for index in 0..4 {
        let id = format!("com.example.p{index}");
        assert_eq!(
            server.state_of(&id, Side::Server),
            InstanceState::Active,
            "{id}"
        );
    }
}

/// REQ: LIF-171
#[test]
fn lif_171_install_of_a_local_file_makes_no_network_attempt() {
    let rig = rig();
    rig.block_network();
    let output = install(&rig, &pkg(NOTES, "1.0.0").signed("key-a"));
    assert_eq!(output.exit_code, 0);
    assert_eq!(rig.network_attempts(), 0);
}

/// REQ: LIF-171
#[test]
fn lif_171_update_of_a_local_file_makes_no_network_attempt() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.block_network();
    let output = update(&rig, &pkg(NOTES, "1.1.0"));
    assert_eq!(output.exit_code, 0);
    assert_eq!(rig.network_attempts(), 0);
}

/// REQ: LIF-171
#[test]
fn lif_171_check_of_a_local_file_makes_no_network_attempt() {
    let rig = rig();
    rig.block_network();
    let path = rig.write_package(&pkg(NOTES, "1.0.0"));
    let text = path.to_string_lossy().into_owned();
    rig.cli(&["check", &text]);
    assert_eq!(rig.network_attempts(), 0);
}

/// REQ: LIF-172
#[test]
fn lif_172_install_on_a_volume_without_enough_space_is_refused_and_leaves_nothing() {
    let rig = rig();
    rig.set_free_bytes(1_000);
    let output = install(&rig, &pkg(NOTES, "1.0.0").padding(1_000_000));
    assert_ne!(output.exit_code, 0);
    assert!(
        error_codes(&output).contains(&"install.disk-full".to_owned()),
        "{:?}",
        output.stdout_jsonl
    );
    assert!(registry_ids(&rig).is_empty());
    assert!(staging_entries(&rig).is_empty());
    assert!(store_versions(&rig, NOTES).is_empty());
}

/// REQ: LIF-172
#[test]
fn lif_172_update_on_a_volume_without_enough_space_keeps_the_old_version_and_no_partials() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let before = rig.registry_bytes();
    rig.set_free_bytes(1_000);
    let output = update(&rig, &pkg(NOTES, "1.1.0").padding(1_000_000));
    assert_ne!(output.exit_code, 0);
    assert_eq!(rig.registry_bytes(), before);
    assert!(staging_entries(&rig).is_empty());
    assert_eq!(store_versions(&rig, NOTES), vec!["1.0.0".to_owned()]);
}

/// REQ: LIF-172
#[test]
fn lif_172_the_check_runs_before_any_file_is_written() {
    let rig = rig();
    rig.set_free_bytes(1_000);
    install(&rig, &pkg(NOTES, "1.0.0").padding(1_000_000));
    let trace = rig.trace();
    assert!(
        position_of(&trace, "disk.check")
            < trace
                .iter()
                .position(|t| t.starts_with("staging.create"))
                .unwrap_or(usize::MAX)
    );
}

/// REQ: LIF-172
#[test]
fn lif_172_install_with_just_enough_space_succeeds() {
    let rig = rig();
    let spec = pkg(NOTES, "1.0.0");
    let need = rig.required_free_bytes(&spec);
    rig.set_free_bytes(need);
    assert_eq!(install(&rig, &spec).exit_code, 0);
}

/// REQ: LIF-172
#[test]
fn lif_172_install_with_one_byte_too_little_is_refused() {
    let rig = rig();
    let spec = pkg(NOTES, "1.0.0");
    let need = rig.required_free_bytes(&spec);
    rig.set_free_bytes(need - 1);
    assert_ne!(install(&rig, &spec).exit_code, 0);
    assert!(staging_entries(&rig).is_empty());
}

/// REQ: LIF-173
#[test]
fn lif_173_disk_full_on_a_plugin_write_is_not_counted_as_a_failure() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    rig.fill_disk();
    let result = server.write_storage_checked(NOTES, "global", "k", b"v");
    assert_eq!(result.err().as_deref(), Some("storage.disk-full"));
    assert_eq!(server.failure_count(NOTES, Side::Server), 0);
    assert_eq!(server.state_of(NOTES, Side::Server), InstanceState::Active);
}

/// REQ: LIF-173
#[test]
fn lif_173_repeated_disk_full_errors_never_disable_the_plugin() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    rig.fill_disk();
    for _ in 0..10 {
        let _ = server.write_storage_checked(NOTES, "global", "k", b"v");
    }
    assert!(!server.is_disabled_by_failure(NOTES));
}

/// REQ: LIF-173
#[test]
fn lif_173_writes_work_again_after_space_is_freed() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.fill_disk();
    let _ = server.write_storage_checked(NOTES, "global", "k", b"v");
    rig.free_disk();
    assert!(server
        .write_storage_checked(NOTES, "global", "k", b"v")
        .is_ok());
}

/// REQ: LIF-174
#[test]
fn lif_174_read_only_data_folder_keeps_the_server_and_plugins_running() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    rig.make_data_read_only(NOTES);
    let _ = server.write_data_file_checked(NOTES, "x.txt", b"x");
    assert!(server.is_running());
    assert_eq!(server.state_of(NOTES, Side::Server), InstanceState::Active);
    assert_eq!(server.failure_count(NOTES, Side::Server), 0);
}

/// REQ: LIF-174
#[test]
fn lif_174_plugin_can_still_read_while_the_data_folder_is_read_only() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_data(NOTES, 1);
    let server = rig.start_server("s1");
    rig.make_data_read_only(NOTES);
    assert!(server.read_data_file(NOTES, "note.txt").is_some());
}

/// REQ: LIF-174
#[test]
fn lif_174_other_plugins_are_unaffected() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    install_ok(&rig, &pkg(OTHER, "1.0.0"));
    let server = rig.start_server("s1");
    rig.make_data_read_only(NOTES);
    assert!(server.write_data_file_checked(OTHER, "x.txt", b"x").is_ok());
}

/// REQ: LIF-175
#[test]
fn lif_175_corrupt_storage_database_is_moved_aside_not_deleted() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_data(NOTES, 1);
    let db = rig.storage_db_path(NOTES);
    rig.corrupt_file(&db, b"garbage garbage garbage");
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let aside: Vec<String> = entry_names(db.parent().expect("storage dir"))
        .into_iter()
        .filter(|name| name.contains("corrupt"))
        .collect();
    assert_eq!(aside.len(), 1, "one moved-aside file");
    let kept =
        std::fs::read(db.parent().expect("storage dir").join(&aside[0])).expect("aside file");
    assert_eq!(kept, b"garbage garbage garbage");
    assert_eq!(server.read_storage(NOTES, "global", "anything"), None);
}

/// REQ: LIF-175
#[test]
fn lif_175_the_replacement_database_is_empty_and_usable() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_data(NOTES, 1);
    rig.corrupt_file(&rig.storage_db_path(NOTES), b"garbage");
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    assert!(server
        .write_storage_checked(NOTES, "global", "k", b"v")
        .is_ok());
    assert_eq!(
        server.read_storage(NOTES, "global", "k"),
        Some(b"v".to_vec())
    );
}

/// REQ: LIF-175
#[test]
fn lif_175_the_plugin_is_not_counted_as_failed_and_a_log_line_names_the_move() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_data(NOTES, 1);
    rig.corrupt_file(&rig.storage_db_path(NOTES), b"garbage");
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    assert_eq!(server.failure_count(NOTES, Side::Server), 0);
    assert!(server
        .log_codes()
        .contains(&"storage.corrupt-moved".to_owned()));
}

/// REQ: LIF-176
#[test]
fn lif_176_cache_key_includes_component_hash_wasmtime_version_and_cpu_features() {
    let base = ilium_plugin_host::testing::ops::compile_cache_key("hash-a", "30.0.0", "avx2");
    assert_ne!(
        base,
        ilium_plugin_host::testing::ops::compile_cache_key("hash-b", "30.0.0", "avx2")
    );
    assert_ne!(
        base,
        ilium_plugin_host::testing::ops::compile_cache_key("hash-a", "31.0.0", "avx2")
    );
    assert_ne!(
        base,
        ilium_plugin_host::testing::ops::compile_cache_key("hash-a", "30.0.0", "sse4")
    );
    assert_eq!(
        base,
        ilium_plugin_host::testing::ops::compile_cache_key("hash-a", "30.0.0", "avx2")
    );
}

/// REQ: LIF-176
#[test]
fn lif_176_missing_compile_cache_does_not_stop_the_plugin() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let first = rig.start_server("s1");
    rig.advance(secs(3));
    first.shutdown();
    rig.delete_compile_cache();
    let second = rig.start_server("s1");
    rig.advance(secs(5));
    assert_eq!(second.state_of(NOTES, Side::Server), InstanceState::Active);
    assert_eq!(second.failure_count(NOTES, Side::Server), 0);
}

/// REQ: LIF-176
#[test]
fn lif_176_cache_is_rebuilt_after_it_was_deleted() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let first = rig.start_server("s1");
    first.shutdown();
    rig.delete_compile_cache();
    let _second = rig.start_server("s1");
    rig.advance(secs(5));
    assert!(!snapshot_tree(&rig.compile_cache_dir()).is_empty());
}

/// REQ: LIF-176
#[test]
fn lif_176_unreadable_cache_directory_does_not_stop_the_plugin() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.make_compile_cache_unwritable();
    let server = rig.start_server("s1");
    rig.advance(secs(5));
    assert_eq!(server.state_of(NOTES, Side::Server), InstanceState::Active);
}
