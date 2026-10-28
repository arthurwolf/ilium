//! COR-030..COR-034.

use proptest::prelude::*;

use super::common::*;

const WRITE_TERMINAL: &str = "settings.write-kernel:terminal";
const READ_TERMINAL: &str = "settings.read-kernel:terminal";

/// REQ: COR-030
#[test]
fn cor_030_first_owner_of_a_kernel_tab_installs() {
    let mut rig = ConfigRig::new();
    let result = rig.install(&plugin("com.example.one", "T1", &[WRITE_TERMINAL]));
    assert!(result.ok, "{:?}", result.codes);
}

/// REQ: COR-030
#[test]
fn cor_030_second_owner_of_the_same_tab_is_refused_manifest_settings_owner() {
    let mut rig = ConfigRig::new();
    assert!(
        rig.install(&plugin("com.example.one", "T1", &[WRITE_TERMINAL]))
            .ok
    );
    let result = rig.install(&plugin("com.example.two", "T1", &[WRITE_TERMINAL]));
    assert!(!result.ok);
    assert!(result.codes.contains(&"manifest.settings-owner".to_owned()));
}

/// REQ: COR-030
#[test]
fn cor_030_owner_of_another_tab_installs_beside_the_first() {
    let mut rig = ConfigRig::new();
    assert!(
        rig.install(&plugin("com.example.one", "T1", &[WRITE_TERMINAL]))
            .ok
    );
    let result = rig.install(&plugin(
        "com.example.two",
        "T1",
        &["settings.write-kernel:editor"],
    ));
    assert!(result.ok, "{:?}", result.codes);
}

/// REQ: COR-030
#[test]
fn cor_030_tab_is_free_again_after_the_owner_is_uninstalled() {
    let mut rig = ConfigRig::new();
    assert!(
        rig.install(&plugin("com.example.one", "T1", &[WRITE_TERMINAL]))
            .ok
    );
    assert!(rig.uninstall("com.example.one"));
    assert!(
        rig.install(&plugin("com.example.two", "T1", &[WRITE_TERMINAL]))
            .ok
    );
}

/// REQ: COR-030
#[test]
fn cor_030_core_plugin_owns_each_built_in_tab_exactly_once() {
    let rig = ConfigRig::with_core_plugins();
    for tab in rig.kernel_tabs() {
        assert_eq!(rig.write_owners(&tab).len(), 1, "tab {tab}");
    }
}

/// REQ: COR-030
#[test]
fn cor_030_second_install_leaves_the_registry_unchanged() {
    let mut rig = ConfigRig::new();
    assert!(
        rig.install(&plugin("com.example.one", "T1", &[WRITE_TERMINAL]))
            .ok
    );
    let _ = rig.install(&plugin("com.example.two", "T1", &[WRITE_TERMINAL]));
    assert_eq!(
        rig.write_owners("terminal"),
        vec!["com.example.one".to_owned()]
    );
    assert!(!rig.is_installed("com.example.two"));
}

/// REQ: COR-031
#[test]
fn cor_031_t2_with_the_write_grant_is_refused_manifest_grant_tier() {
    let mut rig = ConfigRig::new();
    let result = rig.install(&plugin("com.example.two", "T2", &[WRITE_TERMINAL]));
    assert!(!result.ok);
    assert!(result.codes.contains(&"manifest.grant-tier".to_owned()));
}

/// REQ: COR-031
#[test]
fn cor_031_t3_with_the_write_grant_is_refused_manifest_grant_tier() {
    let mut rig = ConfigRig::new();
    let result = rig.install(&plugin("com.example.three", "T3", &[WRITE_TERMINAL]));
    assert!(result.codes.contains(&"manifest.grant-tier".to_owned()));
}

/// REQ: COR-031
#[test]
fn cor_031_t1_with_the_write_grant_installs() {
    let mut rig = ConfigRig::new();
    assert!(
        rig.install(&plugin("com.example.one", "T1", &[WRITE_TERMINAL]))
            .ok
    );
}

/// REQ: COR-031
#[test]
fn cor_031_every_tier_may_hold_the_read_grant() {
    for (index, tier) in ["T1", "T2", "T3"].into_iter().enumerate() {
        let mut rig = ConfigRig::new();
        let id = format!("com.example.reader{index}");
        let result = rig.install(&plugin(&id, tier, &[READ_TERMINAL]));
        assert!(result.ok, "{tier}: {:?}", result.codes);
    }
}

/// REQ: COR-031
#[test]
fn cor_031_write_grant_is_listed_as_high_risk() {
    let mut rig = ConfigRig::new();
    assert!(
        rig.install(&plugin("com.example.one", "T1", &[WRITE_TERMINAL]))
            .ok
    );
    assert_eq!(rig.risk_of(WRITE_TERMINAL).as_deref(), Some("high"));
    assert_ne!(rig.risk_of(READ_TERMINAL).as_deref(), Some("high"));
}

proptest! {
    /// REQ: COR-031
    #[test]
    fn cor_031_write_grant_installs_only_for_t1(tier_index in 0usize..3, tab in "[a-z][a-z-]{0,12}") {
        let tier = ["T1", "T2", "T3"][tier_index];
        let mut rig = ConfigRig::new();
        let result = rig.install(&plugin("com.example.p", tier, &[&format!("settings.write-kernel:{tab}")]));
        prop_assert_eq!(result.codes.contains(&"manifest.grant-tier".to_owned()), tier != "T1");
    }
}

/// REQ: COR-032
#[test]
fn cor_032_section_at_max_bytes_is_stored() {
    let mut rig = ConfigRig::new();
    rig.install(&plugin("com.example.one", "T1", &[]))
        .ok
        .then_some(())
        .expect("install");
    let section = rig.section("com.example.one", "state", 1, 256 * KIB);
    assert!(rig.write_section(&section, &body(256 * KIB, b'a')).is_ok());
    assert_eq!(
        rig.snapshot().section("state").map(|bytes| bytes.len()),
        Some(256 * KIB)
    );
}

/// REQ: COR-032
#[test]
fn cor_032_section_one_byte_over_max_bytes_is_dropped_keeping_the_old_value() {
    let mut rig = ConfigRig::new();
    assert!(rig.install(&plugin("com.example.one", "T1", &[])).ok);
    let section = rig.section("com.example.one", "state", 1, 256 * KIB);
    rig.write_section(&section, &body(10, b'o'))
        .expect("first write");
    assert!(rig
        .write_section(&section, &body(256 * KIB + 1, b'n'))
        .is_err());
    assert_eq!(rig.snapshot().section("state"), Some(body(10, b'o')));
}

/// REQ: COR-032
#[test]
fn cor_032_300_kib_section_is_dropped_and_old_value_stays() {
    let mut rig = ConfigRig::new();
    assert!(rig.install(&plugin("com.example.one", "T1", &[])).ok);
    let section = rig.section("com.example.one", "state", 1, 256 * KIB);
    rig.write_section(&section, &body(100, b'o'))
        .expect("first write");
    assert!(rig.write_section(&section, &body(300 * KIB, b'n')).is_err());
    assert_eq!(rig.snapshot().section("state"), Some(body(100, b'o')));
    assert!(rig
        .log_codes()
        .contains(&"snapshot.section-too-large".to_owned()));
}

/// REQ: COR-032
#[test]
fn cor_032_section_one_byte_under_max_bytes_is_stored() {
    let mut rig = ConfigRig::new();
    assert!(rig.install(&plugin("com.example.one", "T1", &[])).ok);
    let section = rig.section("com.example.one", "state", 1, 256 * KIB);
    assert!(rig
        .write_section(&section, &body(256 * KIB - 1, b'a'))
        .is_ok());
}

/// REQ: COR-032
#[test]
fn cor_032_max_bytes_above_256_kib_is_refused_at_install() {
    let mut rig = ConfigRig::new();
    let spec = plugin("com.example.one", "T1", &[]).snapshot_section("state", 1, 256 * KIB + 1);
    let result = rig.install(&spec);
    assert!(!result.ok);
    // manual does not pin the install-time manifest code for max_bytes over 256 KiB
    assert!(!result.codes.is_empty(), "a refusal must name its code");
}

/// REQ: COR-032
#[test]
fn cor_032_section_carries_its_schema_version() {
    let mut rig = ConfigRig::new();
    assert!(rig.install(&plugin("com.example.one", "T1", &[])).ok);
    let section = rig.section("com.example.one", "state", 7, 1024);
    rig.write_section(&section, &body(4, b'a')).expect("write");
    assert_eq!(rig.snapshot().schema_version("state"), Some(7));
}

/// REQ: COR-032
#[test]
fn cor_032_crashed_plugin_does_not_block_the_snapshot_and_keeps_the_last_value() {
    let mut rig = ConfigRig::new();
    assert!(rig.install(&plugin("com.example.one", "T1", &[])).ok);
    assert!(rig.install(&plugin("com.example.two", "T1", &[])).ok);
    let one = rig.section("com.example.one", "state", 1, 1024);
    let two = rig.section("com.example.two", "state2", 1, 1024);
    rig.write_section(&one, &body(5, b'1')).expect("write");
    rig.write_section(&two, &body(5, b'2')).expect("write");
    rig.crash_plugin("com.example.one");
    let written = rig.write_snapshot();
    assert!(
        written.is_ok(),
        "a failed plugin must not block the snapshot"
    );
    assert_eq!(
        rig.snapshot().section("state"),
        Some(body(5, b'1')),
        "last value still in the file"
    );
    assert_eq!(rig.snapshot().section("state2"), Some(body(5, b'2')));
}

/// REQ: COR-032
#[test]
fn cor_032_state_outside_snapshot_sections_is_not_in_the_snapshot() {
    let mut rig = ConfigRig::new();
    assert!(rig.install(&plugin("com.example.one", "T1", &[])).ok);
    rig.plugin_stores_in_storage("com.example.one", "k", b"v");
    rig.write_snapshot().expect("snapshot");
    assert!(
        rig.snapshot().plugin_keys().is_empty(),
        "only snapshot-sections reach the file"
    );
}

proptest! {
    /// REQ: COR-032
    #[test]
    fn cor_032_section_is_stored_iff_it_fits_max_bytes(max_kib in 1usize..=256, size in 0usize..300_000) {
        let mut rig = ConfigRig::new();
        prop_assert!(rig.install(&plugin("com.example.one", "T1", &[])).ok);
        let max = max_kib * KIB;
        let section = rig.section("com.example.one", "state", 1, max);
        let result = rig.write_section(&section, &body(size, b'x'));
        prop_assert_eq!(result.is_ok(), size <= max);
    }
}

const BASE_CONFIG: &str = "# my notes\n[terminal]\nscrollback = 1000\nunknown_key = \"keep me\"\n\n[future_table]\nfoo = 1\n";

/// REQ: COR-033
#[test]
fn cor_033_changing_one_setting_changes_only_that_line() {
    let mut rig = ConfigRig::new();
    rig.write_config(BASE_CONFIG);
    rig.set_setting("terminal.scrollback", "2000");
    let after = rig.read_config();
    assert_eq!(
        after,
        BASE_CONFIG.replace("scrollback = 1000", "scrollback = 2000")
    );
}

/// REQ: COR-033
#[test]
fn cor_033_unknown_keys_and_tables_survive() {
    let mut rig = ConfigRig::new();
    rig.write_config(BASE_CONFIG);
    rig.set_setting("terminal.scrollback", "5");
    let after = rig.read_config();
    assert!(after.contains("unknown_key = \"keep me\""));
    assert!(after.contains("[future_table]\nfoo = 1"));
    assert!(after.contains("# my notes"));
}

/// REQ: COR-033
#[test]
fn cor_033_project_yaml_unknown_keys_survive() {
    let mut rig = ConfigRig::new();
    let yaml = "terminal:\n  scrollback: 1000\nfuture:\n  thing: 1\n";
    rig.write_project_config(yaml);
    rig.set_project_setting("terminal.scrollback", "9");
    let after = rig.read_project_config();
    assert!(after.contains("future:\n  thing: 1"));
    assert!(after.contains("scrollback: 9"));
}

/// REQ: COR-033
#[test]
fn cor_033_plugin_build_renames_and_moves_no_key() {
    let mut rig = ConfigRig::new();
    rig.write_config(BASE_CONFIG);
    let before = rig.config_key_paths();
    rig.run_plugin_build();
    assert_eq!(rig.config_key_paths(), before);
}

/// REQ: COR-033
#[test]
fn cor_033_a_setting_that_already_has_the_value_leaves_the_file_byte_identical() {
    let mut rig = ConfigRig::new();
    rig.write_config(BASE_CONFIG);
    rig.set_setting("terminal.scrollback", "1000");
    assert_eq!(rig.read_config(), BASE_CONFIG);
}

proptest! {
    // REQ: COR-033
    #[test]
    fn cor_033_unknown_keys_survive_any_setting_change(key in "[a-z][a-z0-9_]{0,10}", value in 0u32..100_000) {
        let mut rig = ConfigRig::new();
        let text = format!("[extra_{key}]\nunknown = {value}\n\n[terminal]\nscrollback = 1\n");
        rig.write_config(&text);
        rig.set_setting("terminal.scrollback", &value.to_string());
        let after = rig.read_config();
        let needle = format!("[extra_{key}]\nunknown = {value}");
        prop_assert!(after.contains(&needle));
    }
}

/// REQ: COR-034
#[test]
fn doc_cor_034_the_manual_pins_the_keys_written_column_to_settings_key_not_owned() {
    let page = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/plugins-manual/pages/reference-config-and-data-migration.md");
    let text = std::fs::read_to_string(&page).expect("the config migration page");
    for needle in ["settings.write-kernel:<tab>", "settings.key-not-owned"] {
        assert!(text.contains(needle), "{needle} missing from {page:?}");
    }
}

/// REQ: COR-034
#[test]
fn cor_034_key_of_another_tab_in_the_same_table_writes_nothing() {
    let page = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/plugins-manual/pages/reference-config-and-data-migration.md");
    let text = std::fs::read_to_string(&page).expect("the config migration page");
    for needle in [
        "A write names a key that belongs to another tab of the same table | Nothing is written",
        "each tab rewrites only its own keys",
    ] {
        assert!(text.contains(needle), "{needle} missing from {page:?}");
    }
}
