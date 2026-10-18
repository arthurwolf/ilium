//! WIT export `ilium:exports/migrator.migrate` (guide-data-migration.md, reference-config-and-data-migration.md).

use ilium_plugin_api::ErrorCode;

use super::common::*;

/// REQ: WIT:ilium:exports/migrator.migrate
#[test]
fn wit_migrator_migrate_receives_from_and_to_and_returns_ok() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    update_ok(&rig, &notes_v3(Vec::new()), &[]);
    server.upgrade(NOTES);
    let calls = rig.migrate_calls(NOTES);
    assert_eq!(calls.len(), 1);
    assert_eq!((calls[0].from, calls[0].to), (1, 3));
    assert!(calls[0].returned_ok);
}

/// REQ: WIT:ilium:exports/migrator.migrate
#[test]
fn wit_migrator_migrate_error_is_a_plugin_error_with_code_and_message() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    update_ok(
        &rig,
        &notes_v2(vec![Step::Fail(plugin_error(
            ErrorCode::Unsupported,
            "no path from 1",
        ))]),
        &[],
    );
    let report = server.upgrade(NOTES);
    assert!(report.rolled_back);
    let calls = rig.migrate_calls(NOTES);
    let error = calls[0].returned_error.as_ref().expect("an error result");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.message, "no path from 1");
}

/// REQ: WIT:ilium:exports/migrator.migrate
#[test]
fn wit_migrator_export_is_optional_and_absent_means_skipped() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    update_ok(&rig, &PluginSpec::new(NOTES, "2.0.0").data_version(2), &[]);
    server.upgrade(NOTES);
    assert!(rig.migrate_calls(NOTES).is_empty());
    assert!(rig.log_codes().contains(&"migration.skipped".to_owned()));
}

/// REQ: WIT:ilium:exports/migrator.migrate
#[test]
fn wit_migrator_called_once_per_scope_not_per_version_step() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    update_ok(&rig, &notes_v3(Vec::new()), &[]);
    server.upgrade(NOTES);
    assert_eq!(
        global_calls(&rig, NOTES).len(),
        1,
        "one call from 1 to 3, not 1 to 2 then 2 to 3"
    );
}
