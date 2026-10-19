//! Config and data migration reference (reference-config-and-data-migration.md): COR-090.

use super::common::*;

/// REQ: COR-090
#[test]
fn cor_090_unparseable_config_toml_is_left_unchanged_and_the_host_boots() {
    let rig = rig();
    let broken = "[ui\ntheme = ";
    rig.config(broken);
    let server = rig.server("main", "/project", "default");
    assert_eq!(
        rig.config_text(),
        broken,
        "the unparseable file stays byte for byte"
    );
    assert!(
        rig.log_codes().contains(&"config.parse-error".to_owned()),
        "one config.parse-error warning names the file"
    );
    assert!(server.is_running(), "the host still boots with defaults");
}

/// REQ: COR-090
#[test]
fn cor_090_unparseable_config_is_not_rewritten_after_a_settings_write() {
    let rig = rig();
    let broken = "[ui\ntheme = ";
    rig.config(broken);
    let server = rig.server("main", "/project", "default");
    server.write_setting("ui", "theme", "dark");
    assert_eq!(rig.config_text(), broken);
}
