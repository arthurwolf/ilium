//! A test rig for the config, settings and snapshot migration rules
//! (reference-config-and-data-migration.md COR-030..COR-033).

use ilium_plugin_api::PluginError;

/// A plugin the rig installs: id, tier, grants and snapshot sections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSpec {
    id: String,
    tier: String,
    grants: Vec<String>,
    snapshot_sections: Vec<(String, u32, usize)>,
}

impl PluginSpec {
    /// A plugin of `id` at `tier` (`"T1"`, `"T2"`, ...).
    pub fn new(_id: &str, _tier: &str) -> PluginSpec {
        unimplemented!("host::testing::config::PluginSpec::new")
    }

    /// The grants the manifest declares.
    pub fn grants(self, _grants: &[&str]) -> PluginSpec {
        unimplemented!("host::testing::config::PluginSpec::grants")
    }

    /// A snapshot section the manifest declares (COR-032).
    pub fn snapshot_section(
        self,
        _name: &str,
        _schema_version: u32,
        _max_bytes: usize,
    ) -> PluginSpec {
        unimplemented!("host::testing::config::PluginSpec::snapshot_section")
    }
}

/// The result of an install through the rig.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InstallResult {
    /// Whether the install succeeded.
    pub ok: bool,
    /// The refusal codes when it did not.
    pub codes: Vec<String>,
}

impl InstallResult {
    /// Same as the `ok` field.
    pub fn is_ok(&self) -> bool {
        unimplemented!("host::testing::config::InstallResult::is_ok")
    }
}

/// A handle on one snapshot section of one plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionHandle {
    plugin: String,
    name: String,
}

/// The snapshot file as written.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SnapshotFile {
    sections: Vec<(String, u32, Vec<u8>)>,
    plugin_keys: Vec<String>,
}

impl SnapshotFile {
    /// The bytes of a section, if present.
    pub fn section(&self, _name: &str) -> Option<Vec<u8>> {
        unimplemented!("host::testing::config::SnapshotFile::section")
    }

    /// The schema version recorded for a section.
    pub fn schema_version(&self, _name: &str) -> Option<u32> {
        unimplemented!("host::testing::config::SnapshotFile::schema_version")
    }

    /// The plugin storage keys that reached the file (must stay empty).
    pub fn plugin_keys(&self) -> Vec<String> {
        unimplemented!("host::testing::config::SnapshotFile::plugin_keys")
    }
}

/// The config-migration rig: an isolated config, project config and snapshot file.
#[derive(Debug, Default)]
pub struct ConfigRig;

impl ConfigRig {
    /// A rig with no plugins.
    pub fn new() -> ConfigRig {
        unimplemented!("host::testing::config::ConfigRig::new")
    }

    /// A rig with the core plugins installed (COR-030 kernel tabs).
    pub fn with_core_plugins() -> ConfigRig {
        unimplemented!("host::testing::config::ConfigRig::with_core_plugins")
    }

    /// Installs a plugin.
    pub fn install(&mut self, _plugin: &PluginSpec) -> InstallResult {
        unimplemented!("host::testing::config::ConfigRig::install")
    }

    /// Uninstalls a plugin; true when it was installed.
    pub fn uninstall(&mut self, _id: &str) -> bool {
        unimplemented!("host::testing::config::ConfigRig::uninstall")
    }

    /// Whether a plugin is installed.
    pub fn is_installed(&self, _id: &str) -> bool {
        unimplemented!("host::testing::config::ConfigRig::is_installed")
    }

    /// The settings tabs the kernel owns.
    pub fn kernel_tabs(&self) -> Vec<String> {
        unimplemented!("host::testing::config::ConfigRig::kernel_tabs")
    }

    /// Who may write a settings tab.
    pub fn write_owners(&self, _tab: &str) -> Vec<String> {
        unimplemented!("host::testing::config::ConfigRig::write_owners")
    }

    /// The risk level of a grant as the settings screen shows it.
    pub fn risk_of(&self, _grant: &str) -> Option<String> {
        unimplemented!("host::testing::config::ConfigRig::risk_of")
    }

    /// A handle on a snapshot section of a plugin (COR-032).
    pub fn section(
        &mut self,
        _plugin: &str,
        _name: &str,
        _schema_version: u32,
        _max_bytes: usize,
    ) -> SectionHandle {
        unimplemented!("host::testing::config::ConfigRig::section")
    }

    /// Writes a snapshot section.
    pub fn write_section(
        &mut self,
        _section: &SectionHandle,
        _bytes: &[u8],
    ) -> Result<(), PluginError> {
        unimplemented!("host::testing::config::ConfigRig::write_section")
    }

    /// Writes the snapshot file.
    pub fn write_snapshot(&mut self) -> Result<(), PluginError> {
        unimplemented!("host::testing::config::ConfigRig::write_snapshot")
    }

    /// Reads the snapshot file back.
    pub fn snapshot(&self) -> SnapshotFile {
        unimplemented!("host::testing::config::ConfigRig::snapshot")
    }

    /// Crashes a plugin instance.
    pub fn crash_plugin(&mut self, _id: &str) {
        unimplemented!("host::testing::config::ConfigRig::crash_plugin")
    }

    /// Makes a plugin store a key in plugin storage (never the snapshot).
    pub fn plugin_stores_in_storage(&mut self, _id: &str, _key: &str, _value: &[u8]) {
        unimplemented!("host::testing::config::ConfigRig::plugin_stores_in_storage")
    }

    /// The log codes emitted so far.
    pub fn log_codes(&self) -> Vec<String> {
        unimplemented!("host::testing::config::ConfigRig::log_codes")
    }

    /// Replaces the user config file (COR-033).
    pub fn write_config(&mut self, _contents: &str) {
        unimplemented!("host::testing::config::ConfigRig::write_config")
    }

    /// The user config file as it is now.
    pub fn read_config(&self) -> String {
        unimplemented!("host::testing::config::ConfigRig::read_config")
    }

    /// Sets one setting through the settings screen.
    pub fn set_setting(&mut self, _key: &str, _value: &str) {
        unimplemented!("host::testing::config::ConfigRig::set_setting")
    }

    /// Replaces the project config file.
    pub fn write_project_config(&mut self, _contents: &str) {
        unimplemented!("host::testing::config::ConfigRig::write_project_config")
    }

    /// The project config file as it is now.
    pub fn read_project_config(&self) -> String {
        unimplemented!("host::testing::config::ConfigRig::read_project_config")
    }

    /// Sets one project setting through the settings screen.
    pub fn set_project_setting(&mut self, _key: &str, _value: &str) {
        unimplemented!("host::testing::config::ConfigRig::set_project_setting")
    }

    /// Every config key path the kernel and the plugins know.
    pub fn config_key_paths(&self) -> Vec<String> {
        unimplemented!("host::testing::config::ConfigRig::config_key_paths")
    }

    /// Runs the plugin build step that regenerates config keys.
    pub fn run_plugin_build(&mut self) {
        unimplemented!("host::testing::config::ConfigRig::run_plugin_build")
    }
}

#[cfg(test)]
mod tests {
    use super::{ConfigRig, InstallResult, PluginSpec};

    const KERNEL_TAB_WRITE_GRANT: &str = "settings.write-kernel:terminal";
    const KERNEL_TAB_READ_GRANT: &str = "settings.read-kernel:terminal";
    const SNAPSHOT_SECTION_NAME: &str = "state";
    const SNAPSHOT_SECTION_MAX_BYTES: usize = 262_144;
    const KIB: usize = 1024;

    /// REQ: COR-030
    #[test]
    fn single_t1_plugin_with_kernel_tab_write_grant_installs() {
        let mut rig = ConfigRig::new();
        let result = rig.install(&PluginSpec::new("fixture.writer", "T1").grants(&[KERNEL_TAB_WRITE_GRANT]));
        assert!(result.is_ok());
        assert!(result.codes.is_empty());
        assert!(rig.is_installed("fixture.writer"));
    }

    /// REQ: COR-030
    #[test]
    fn second_plugin_declaring_same_kernel_tab_write_grant_is_refused() {
        let mut rig = ConfigRig::new();
        let first = rig.install(&PluginSpec::new("fixture.first", "T1").grants(&[KERNEL_TAB_WRITE_GRANT]));
        assert!(first.is_ok());
        let second = rig.install(&PluginSpec::new("fixture.second", "T1").grants(&[KERNEL_TAB_WRITE_GRANT]));
        assert!(!second.is_ok());
        assert!(second.codes.iter().any(|code| code == "manifest.settings-owner"));
        assert!(rig.is_installed("fixture.first"));
        assert!(!rig.is_installed("fixture.second"));
    }

    /// REQ: COR-030
    #[test]
    fn kernel_tab_write_owner_is_the_only_installed_holder() {
        let mut rig = ConfigRig::new();
        rig.install(&PluginSpec::new("fixture.writer", "T1").grants(&[KERNEL_TAB_WRITE_GRANT]));
        assert_eq!(rig.write_owners("terminal"), vec!["fixture.writer".to_string()]);
    }

    /// REQ: COR-030
    #[test]
    fn tab_without_installed_owner_has_no_write_owner_and_stays_listed() {
        let rig = ConfigRig::new();
        assert!(rig.write_owners("terminal").is_empty());
        assert!(rig.kernel_tabs().iter().any(|tab| tab == "terminal"));
    }

    /// REQ: COR-030
    #[test]
    fn uninstalling_the_owner_frees_the_tab_for_a_new_holder() {
        let mut rig = ConfigRig::new();
        rig.install(&PluginSpec::new("fixture.first", "T1").grants(&[KERNEL_TAB_WRITE_GRANT]));
        assert!(rig.uninstall("fixture.first"));
        assert!(rig.write_owners("terminal").is_empty());
        let replacement = rig.install(&PluginSpec::new("fixture.second", "T1").grants(&[KERNEL_TAB_WRITE_GRANT]));
        assert!(replacement.is_ok());
    }

    /// REQ: COR-030
    #[test]
    fn uninstalling_a_plugin_that_is_not_installed_returns_false() {
        let mut rig = ConfigRig::new();
        assert!(!rig.uninstall("fixture.never-installed"));
    }

    /// REQ: COR-031
    #[test]
    fn t2_package_with_kernel_tab_write_grant_fails_with_grant_tier() {
        let mut rig = ConfigRig::new();
        let result = rig.install(&PluginSpec::new("fixture.t2-writer", "T2").grants(&[KERNEL_TAB_WRITE_GRANT]));
        assert!(!result.is_ok());
        assert!(result.codes.iter().any(|code| code == "manifest.grant-tier"));
        assert!(!rig.is_installed("fixture.t2-writer"));
    }

    /// REQ: COR-031
    #[test]
    fn t3_package_with_kernel_tab_write_grant_fails_with_grant_tier() {
        let mut rig = ConfigRig::new();
        let result = rig.install(&PluginSpec::new("fixture.t3-writer", "T3").grants(&[KERNEL_TAB_WRITE_GRANT]));
        assert!(result.codes.iter().any(|code| code == "manifest.grant-tier"));
        assert!(!rig.is_installed("fixture.t3-writer"));
    }

    /// REQ: COR-031
    #[test]
    fn t3_package_with_kernel_tab_read_grant_installs() {
        let mut rig = ConfigRig::new();
        let result = rig.install(&PluginSpec::new("fixture.t3-reader", "T3").grants(&[KERNEL_TAB_READ_GRANT]));
        assert!(result.is_ok());
        assert!(rig.is_installed("fixture.t3-reader"));
    }

    /// REQ: COR-031
    #[test]
    fn t2_package_with_kernel_tab_read_grant_installs() {
        let mut rig = ConfigRig::new();
        let result = rig.install(&PluginSpec::new("fixture.t2-reader", "T2").grants(&[KERNEL_TAB_READ_GRANT]));
        assert!(result.is_ok());
    }

    /// REQ: COR-031
    #[test]
    fn kernel_tab_write_grant_is_high_risk() {
        let rig = ConfigRig::new();
        assert_eq!(rig.risk_of(KERNEL_TAB_WRITE_GRANT).as_deref(), Some("high"));
    }

    /// REQ: COR-031
    #[test]
    fn risk_of_an_unknown_grant_is_none() {
        let rig = ConfigRig::new();
        assert!(rig.risk_of("settings.no-such-grant:terminal").is_none());
    }

    /// REQ: COR-032
    #[test]
    fn section_written_and_snapshotted_keeps_its_schema_version() {
        let mut rig = ConfigRig::with_core_plugins();
        let handle = rig.section("fixture.state", SNAPSHOT_SECTION_NAME, 3, SNAPSHOT_SECTION_MAX_BYTES);
        assert!(rig.write_section(&handle, b"{\"monitors\":1}").is_ok());
        assert!(rig.write_snapshot().is_ok());
        let snapshot = rig.snapshot();
        assert_eq!(snapshot.section(SNAPSHOT_SECTION_NAME), Some(b"{\"monitors\":1}".to_vec()));
        assert_eq!(snapshot.schema_version(SNAPSHOT_SECTION_NAME), Some(3));
    }

    /// REQ: COR-032
    #[test]
    fn section_exactly_at_max_bytes_is_stored() {
        let mut rig = ConfigRig::new();
        let handle = rig.section("fixture.limit", SNAPSHOT_SECTION_NAME, 1, KIB);
        let exact = vec![b'a'; KIB];
        assert!(rig.write_section(&handle, &exact).is_ok());
        assert!(rig.write_snapshot().is_ok());
        assert_eq!(rig.snapshot().section(SNAPSHOT_SECTION_NAME), Some(exact));
    }

    /// REQ: COR-032
    #[test]
    fn oversized_section_is_dropped_and_previous_value_is_kept() {
        let mut rig = ConfigRig::new();
        let handle = rig.section("fixture.big", SNAPSHOT_SECTION_NAME, 1, SNAPSHOT_SECTION_MAX_BYTES);
        let previous = b"previous value".to_vec();
        assert!(rig.write_section(&handle, &previous).is_ok());
        assert!(rig.write_snapshot().is_ok());
        let oversized = vec![b'x'; 300 * KIB];
        let _oversize_result = rig.write_section(&handle, &oversized);
        assert!(rig.write_snapshot().is_ok());
        assert_eq!(rig.snapshot().section(SNAPSHOT_SECTION_NAME), Some(previous));
    }

    /// REQ: COR-032
    #[test]
    fn crashed_plugin_does_not_block_snapshot_and_keeps_last_value() {
        let mut rig = ConfigRig::new();
        let handle = rig.section("fixture.crashy", SNAPSHOT_SECTION_NAME, 1, SNAPSHOT_SECTION_MAX_BYTES);
        assert!(rig.write_section(&handle, b"last good").is_ok());
        assert!(rig.write_snapshot().is_ok());
        rig.crash_plugin("fixture.crashy");
        assert!(rig.write_snapshot().is_ok());
        assert_eq!(rig.snapshot().section(SNAPSHOT_SECTION_NAME), Some(b"last good".to_vec()));
    }

    /// REQ: COR-032
    #[test]
    fn empty_section_is_stored_as_empty_bytes() {
        let mut rig = ConfigRig::new();
        let handle = rig.section("fixture.empty", SNAPSHOT_SECTION_NAME, 1, KIB);
        assert!(rig.write_section(&handle, &[]).is_ok());
        assert!(rig.write_snapshot().is_ok());
        assert_eq!(rig.snapshot().section(SNAPSHOT_SECTION_NAME), Some(Vec::new()));
    }

    /// REQ: COR-032
    #[test]
    fn snapshot_without_sections_reports_no_section_and_no_schema_version() {
        let mut rig = ConfigRig::new();
        assert!(rig.write_snapshot().is_ok());
        let snapshot = rig.snapshot();
        assert!(snapshot.section(SNAPSHOT_SECTION_NAME).is_none());
        assert!(snapshot.schema_version(SNAPSHOT_SECTION_NAME).is_none());
    }

    /// REQ: COR-032
    #[test]
    fn plugin_storage_key_never_reaches_the_snapshot_file() {
        let mut rig = ConfigRig::new();
        rig.plugin_stores_in_storage("fixture.storage", "private-key", b"value");
        assert!(rig.write_snapshot().is_ok());
        assert!(rig.snapshot().plugin_keys().is_empty());
    }

    /// REQ: COR-090
    #[test]
    fn unparseable_config_is_left_byte_for_byte_unchanged() {
        let mut rig = ConfigRig::new();
        rig.write_config("[ui");
        assert_eq!(rig.read_config(), "[ui");
    }

    /// REQ: COR-033
    #[test]
    fn setting_change_preserves_unknown_keys_and_tables() {
        let mut rig = ConfigRig::new();
        rig.write_config("[ui]\nunknown_future_key = \"keep me\"\n\n[future_table]\nvalue = 2\n");
        rig.set_setting("ui.color_scheme", "dark");
        let after = rig.read_config();
        assert!(after.contains("unknown_future_key = \"keep me\""));
        assert!(after.contains("[future_table]"));
        assert!(after.contains("value = 2"));
    }

    /// REQ: COR-033
    #[test]
    fn plugin_build_does_not_rename_or_drop_existing_keys() {
        let mut rig = ConfigRig::new();
        let original = "config_version = 0\n\n[ui]\nunknown_future_key = \"keep me\"\n";
        rig.write_config(original);
        rig.run_plugin_build();
        let after = rig.read_config();
        assert!(after.contains("unknown_future_key = \"keep me\""));
        assert!(after.contains("[ui]"));
    }

    /// REQ: COR-033
    #[test]
    fn empty_config_file_reads_back_as_empty() {
        let mut rig = ConfigRig::new();
        rig.write_config("");
        assert_eq!(rig.read_config(), "");
    }

    /// REQ: COR-033
    #[test]
    fn project_setting_change_preserves_unknown_keys() {
        let mut rig = ConfigRig::new();
        rig.write_project_config("unknown_project_key: keep\n");
        rig.set_project_setting("animation.speed", "2");
        assert!(rig.read_project_config().contains("unknown_project_key: keep"));
    }

    /// REQ: COR-030
    #[test]
    fn kernel_tabs_are_the_25_frozen_tab_ids() {
        let rig = ConfigRig::new();
        let tabs = rig.kernel_tabs();
        assert_eq!(tabs.len(), 25);
        assert!(tabs.iter().any(|tab| tab == "user-interface"));
        assert!(tabs.iter().any(|tab| tab == "terminal"));
    }

    #[test]
    fn with_core_plugins_installs_the_core_terminal_owner() {
        let rig = ConfigRig::with_core_plugins();
        assert!(rig.is_installed("ilium.core.terminal"));
        assert_eq!(rig.write_owners("terminal"), vec!["ilium.core.terminal".to_string()]);
    }

    #[test]
    fn new_rig_has_no_plugins_installed() {
        let rig = ConfigRig::new();
        assert!(!rig.is_installed("ilium.core.terminal"));
    }

    #[test]
    fn plugin_spec_builder_stores_grants_as_given() {
        let spec = PluginSpec::new("fixture.grants", "T1").grants(&[KERNEL_TAB_READ_GRANT, KERNEL_TAB_WRITE_GRANT]);
        assert_eq!(spec.grants, vec![KERNEL_TAB_READ_GRANT.to_string(), KERNEL_TAB_WRITE_GRANT.to_string()]);
    }

    #[test]
    fn install_result_default_is_not_ok_with_no_codes() {
        let result = InstallResult::default();
        assert!(!result.is_ok());
        assert!(result.codes.is_empty());
    }

    #[test]
    fn install_result_is_ok_follows_ok_field() {
        let result = InstallResult { ok: true, codes: Vec::new() };
        assert!(result.is_ok());
    }
}
