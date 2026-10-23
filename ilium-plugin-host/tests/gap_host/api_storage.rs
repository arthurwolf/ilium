//! Manual page `api-storage`: per-plugin data export and import as JSON lines, with the data version
//! guard and per-bucket all-or-nothing import (HST-656).

use super::common::{install_ok, pkg, rig, NOTES};

/// REQ: HST-656
#[test]
fn hst_656_data_export_import_round_trips_every_bucket_and_refuses_newer_data_versions() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plugin_storage_put(NOTES, "note/a", b"alpha")
        .expect("write accepted");
    let dir = rig.path_in_scratch("export");
    let exported = rig.cli(&[
        "plugin",
        "data",
        "export",
        NOTES,
        dir.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(exported.exit_code, 0, "export succeeds");
    let lines =
        std::fs::read_to_string(dir.join("project.jsonl")).expect("bucket written as JSON lines");
    let first: serde_json::Value =
        serde_json::from_str(lines.lines().next().expect("a line")).expect("valid JSON");
    assert!(
        first.get("key").is_some()
            && first.get("value").is_some()
            && first.get("data_version").is_some(),
        "base64 key, value and data version"
    );
    rig.set_exported_data_version(&dir, 99);
    let newer = rig.cli(&[
        "plugin",
        "data",
        "import",
        NOTES,
        dir.to_str().expect("utf-8 path"),
    ]);
    assert!(
        newer.stderr.contains("migration.data-newer"),
        "data version above the installed plugin's is refused"
    );
    assert_eq!(
        rig.plugin_storage_get(NOTES, "note/a"),
        Some(b"alpha".to_vec()),
        "a refused import changes nothing"
    );
}
