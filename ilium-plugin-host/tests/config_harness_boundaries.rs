use ilium_plugin_host::testing::config::{ConfigRig, PluginSpec};

#[test]
fn owner_refusal_is_atomic_and_uninstall_releases_ownership() {
    let mut rig = ConfigRig::new();
    let first = PluginSpec::new("example.first", "T1").grants(&["settings.write-kernel:terminal"]);
    let second = PluginSpec::new("example.second", "T1").grants(&["settings.write-kernel:terminal"]);
    assert!(rig.install(&first).is_ok());
    assert!(!rig.install(&second).is_ok());
    assert!(!rig.is_installed("example.second"));
    assert_eq!(rig.write_owners("terminal"), vec!["example.first"]);
    assert!(rig.uninstall("example.first"));
    assert!(rig.install(&second).is_ok());
}

#[test]
fn section_custody_limits_and_crash_preserve_last_serialized_value() {
    let mut rig = ConfigRig::new();
    assert!(rig.install(&PluginSpec::new("example.first", "T1")).is_ok());
    assert!(rig.install(&PluginSpec::new("example.second", "T1")).is_ok());
    let section = rig.section("example.first", "state", 3, 4);
    rig.write_section(&section, b"old").unwrap();
    let intruder = rig.section("example.second", "state", 3, 4);
    assert!(rig.write_section(&intruder, b"bad").is_err());
    assert!(rig.write_section(&section, b"large").is_err());
    rig.plugin_stores_in_storage("example.first", "private", b"secret");
    rig.crash_plugin("example.first");
    assert!(rig.write_section(&section, b"new").is_err());
    rig.write_snapshot().unwrap();
    assert_eq!(rig.snapshot().section("state"), Some(b"old".to_vec()));
    assert_eq!(rig.snapshot().schema_version("state"), Some(3));
    assert!(rig.snapshot().plugin_keys().is_empty());
}

#[test]
fn malformed_and_complex_documents_are_preserved_without_lossy_edits() {
    let mut rig = ConfigRig::new();
    rig.write_config("[ui");
    rig.set_setting("ui.theme", "1");
    assert_eq!(rig.read_config(), "[ui");
    assert!(rig.log_codes().contains(&"config.parse-error".into()));
    let text = "# notes\n[terminal]\nscrollback = 1\nunknown = 'keep'\n";
    rig.write_config(text);
    rig.set_setting("terminal.scrollback", "2");
    assert_eq!(rig.read_config(), text.replace("scrollback = 1", "scrollback = 2"));
    let commented = "[terminal]\nscrollback = 1 # important\n";
    rig.write_config(commented);
    rig.set_setting("terminal.scrollback", "2");
    assert_eq!(rig.read_config(), commented);
    rig.write_project_config("future: { preserve: true }\n");
    rig.set_project_setting("future.preserve", "false");
    assert_eq!(rig.read_project_config(), "future: { preserve: true }\n");
    assert!(rig.log_codes().contains(&"config.yaml-context-required".into()));
}

#[test]
fn native_tabs_do_not_acquire_fabricated_plugin_owners() {
    let rig = ConfigRig::with_core_plugins();
    assert_eq!(rig.kernel_tabs().len(), 25);
    assert!(rig.write_owners("editor").is_empty());
    assert!(rig.write_owners("kanban-board").is_empty());
    assert_eq!(rig.write_owners("terminal"), vec!["ilium.core.terminal"]);
}
