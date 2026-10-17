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
    pub fn new(id: &str, tier: &str) -> PluginSpec {
        unimplemented!("host::testing::config::PluginSpec::new")
    }

    /// The grants the manifest declares.
    pub fn grants(self, grants: &[&str]) -> PluginSpec {
        unimplemented!("host::testing::config::PluginSpec::grants")
    }

    /// A snapshot section the manifest declares (COR-032).
    pub fn snapshot_section(self, name: &str, schema_version: u32, max_bytes: usize) -> PluginSpec {
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
    pub fn section(&self, name: &str) -> Option<Vec<u8>> {
        unimplemented!("host::testing::config::SnapshotFile::section")
    }

    /// The schema version recorded for a section.
    pub fn schema_version(&self, name: &str) -> Option<u32> {
        unimplemented!("host::testing::config::SnapshotFile::schema_version")
    }

    /// The plugin storage keys that reached the file (must stay empty).
    pub fn plugin_keys(&self) -> Vec<String> {
        unimplemented!("host::testing::config::SnapshotFile::plugin_keys")
    }
}

/// The config-migration rig: an isolated config, project config and snapshot file.
#[derive(Debug, Default)]
pub struct ConfigRig {
    installed: Vec<PluginSpec>,
    config: String,
    project_config: String,
    log_codes: Vec<String>,
}

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
    pub fn install(&mut self, plugin: &PluginSpec) -> InstallResult {
        unimplemented!("host::testing::config::ConfigRig::install")
    }

    /// Uninstalls a plugin; true when it was installed.
    pub fn uninstall(&mut self, id: &str) -> bool {
        unimplemented!("host::testing::config::ConfigRig::uninstall")
    }

    /// Whether a plugin is installed.
    pub fn is_installed(&self, id: &str) -> bool {
        unimplemented!("host::testing::config::ConfigRig::is_installed")
    }

    /// The settings tabs the kernel owns.
    pub fn kernel_tabs(&self) -> Vec<String> {
        unimplemented!("host::testing::config::ConfigRig::kernel_tabs")
    }

    /// Who may write a settings tab.
    pub fn write_owners(&self, tab: &str) -> Vec<String> {
        unimplemented!("host::testing::config::ConfigRig::write_owners")
    }

    /// The risk level of a grant as the settings screen shows it.
    pub fn risk_of(&self, grant: &str) -> Option<String> {
        unimplemented!("host::testing::config::ConfigRig::risk_of")
    }

    /// A handle on a snapshot section of a plugin (COR-032).
    pub fn section(
        &mut self,
        plugin: &str,
        name: &str,
        schema_version: u32,
        max_bytes: usize,
    ) -> SectionHandle {
        unimplemented!("host::testing::config::ConfigRig::section")
    }

    /// Writes a snapshot section.
    pub fn write_section(
        &mut self,
        section: &SectionHandle,
        bytes: &[u8],
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
    pub fn crash_plugin(&mut self, id: &str) {
        unimplemented!("host::testing::config::ConfigRig::crash_plugin")
    }

    /// Makes a plugin store a key in plugin storage (never the snapshot).
    pub fn plugin_stores_in_storage(&mut self, id: &str, key: &str, value: &[u8]) {
        unimplemented!("host::testing::config::ConfigRig::plugin_stores_in_storage")
    }

    /// The log codes emitted so far.
    pub fn log_codes(&self) -> Vec<String> {
        unimplemented!("host::testing::config::ConfigRig::log_codes")
    }

    /// Replaces the user config file (COR-033).
    pub fn write_config(&mut self, contents: &str) {
        unimplemented!("host::testing::config::ConfigRig::write_config")
    }

    /// The user config file as it is now.
    pub fn read_config(&self) -> String {
        unimplemented!("host::testing::config::ConfigRig::read_config")
    }

    /// Sets one setting through the settings screen.
    pub fn set_setting(&mut self, key: &str, value: &str) {
        unimplemented!("host::testing::config::ConfigRig::set_setting")
    }

    /// Replaces the project config file.
    pub fn write_project_config(&mut self, contents: &str) {
        unimplemented!("host::testing::config::ConfigRig::write_project_config")
    }

    /// The project config file as it is now.
    pub fn read_project_config(&self) -> String {
        unimplemented!("host::testing::config::ConfigRig::read_project_config")
    }

    /// Sets one project setting through the settings screen.
    pub fn set_project_setting(&mut self, key: &str, value: &str) {
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
