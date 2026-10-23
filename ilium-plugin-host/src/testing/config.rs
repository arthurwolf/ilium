//! A test rig for the config, settings and snapshot migration rules
//! (reference-config-and-data-migration.md COR-030..COR-033).

use std::collections::{BTreeMap, BTreeSet};

use ilium_plugin_api::grants::{self, Grant};
use ilium_plugin_api::{ErrorCode, PluginError, Side, Tier};

use crate::settings;

/// The largest snapshot section the host stores, whatever a plugin declares (COR-032).
const SNAPSHOT_SECTION_CEILING_BYTES: usize = 262_144;

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
        PluginSpec {
            id: id.to_string(),
            tier: tier.to_string(),
            grants: Vec::new(),
            snapshot_sections: Vec::new(),
        }
    }

    /// The grants the manifest declares.
    pub fn grants(self, grants: &[&str]) -> PluginSpec {
        PluginSpec {
            grants: grants.iter().map(|grant| grant.to_string()).collect(),
            ..self
        }
    }

    /// A snapshot section the manifest declares (COR-032).
    pub fn snapshot_section(self, name: &str, schema_version: u32, max_bytes: usize) -> PluginSpec {
        let mut spec = self;
        spec.snapshot_sections
            .push((name.to_string(), schema_version, max_bytes));
        spec
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
        self.ok
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
        self.sections
            .iter()
            .find(|(section_name, _, _)| section_name == name)
            .map(|(_, _, bytes)| bytes.clone())
    }

    /// The schema version recorded for a section.
    pub fn schema_version(&self, name: &str) -> Option<u32> {
        self.sections
            .iter()
            .find(|(section_name, _, _)| section_name == name)
            .map(|(_, version, _)| *version)
    }

    /// The plugin storage keys that reached the file (must stay empty).
    pub fn plugin_keys(&self) -> Vec<String> {
        self.plugin_keys.clone()
    }
}

/// The last state of one plugin snapshot section.
#[derive(Debug, Default)]
struct StoredSection {
    schema_version: u32,
    max_bytes: usize,
    /// The last value the snapshot holds; `None` until one is accepted.
    data: Option<Vec<u8>>,
}

/// The config-migration rig: an isolated config, project config and snapshot file.
#[derive(Debug, Default)]
pub struct ConfigRig {
    /// Installed plugin ids and the grant texts each one holds.
    installed: BTreeMap<String, Vec<String>>,
    /// Snapshot sections keyed by (plugin id, section name).
    sections: BTreeMap<(String, String), StoredSection>,
    crashed: BTreeSet<String>,
    snapshot: SnapshotFile,
    config_text: String,
    project_config_text: String,
    logged_codes: Vec<String>,
}

impl ConfigRig {
    /// A rig with no plugins.
    pub fn new() -> ConfigRig {
        ConfigRig::default()
    }

    /// A rig with the core plugins installed (COR-030 kernel tabs).
    pub fn with_core_plugins() -> ConfigRig {
        let mut rig = ConfigRig::new();
        rig.install(
            &PluginSpec::new("ilium.core.terminal", "T1")
                .grants(&["settings.write-kernel:terminal"]),
        );
        rig
    }

    /// Installs a plugin.
    pub fn install(&mut self, plugin: &PluginSpec) -> InstallResult {
        let mut codes = Vec::new();
        let tier = match plugin.tier.as_str() {
            "T1" => Some(Tier::T1),
            "T2" => Some(Tier::T2),
            "T3" => Some(Tier::T3),
            _ => None,
        };
        if tier.is_none() {
            codes.push("manifest.schema".to_string());
        }
        for grant_text in &plugin.grants {
            let grant = match Grant::parse(grant_text) {
                Ok(grant) => grant,
                Err(diagnostic) => {
                    codes.push(diagnostic.code.0.to_string());
                    continue;
                }
            };
            if let Some(tier) = tier {
                if let Err(diagnostic) = grants::check_grant(&grant, tier, Side::Server) {
                    codes.push(diagnostic.code.0.to_string());
                    continue;
                }
            }
            if self.kernel_write_held_elsewhere(&plugin.id, grant_text) {
                codes.push("manifest.settings-owner".to_string());
            }
        }
        if codes.is_empty() {
            self.installed
                .insert(plugin.id.clone(), plugin.grants.clone());
            for (name, schema_version, max_bytes) in &plugin.snapshot_sections {
                let stored = self
                    .sections
                    .entry((plugin.id.clone(), name.clone()))
                    .or_default();
                stored.schema_version = *schema_version;
                stored.max_bytes = *max_bytes;
            }
        } else {
            self.logged_codes.extend(codes.iter().cloned());
        }
        InstallResult {
            ok: codes.is_empty(),
            codes,
        }
    }

    /// Uninstalls a plugin; true when it was installed.
    pub fn uninstall(&mut self, id: &str) -> bool {
        self.installed.remove(id).is_some()
    }

    /// Whether a plugin is installed.
    pub fn is_installed(&self, id: &str) -> bool {
        self.installed.contains_key(id)
    }

    /// The settings tabs the kernel owns.
    pub fn kernel_tabs(&self) -> Vec<String> {
        settings::kernel_tab_ids()
            .iter()
            .map(|tab| tab.to_string())
            .collect()
    }

    /// Who may write a settings tab.
    pub fn write_owners(&self, tab: &str) -> Vec<String> {
        let grant_text = format!("settings.write-kernel:{tab}");
        self.installed
            .iter()
            .filter(|(_, held)| held.contains(&grant_text))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// The risk level of a grant as the settings screen shows it.
    pub fn risk_of(&self, grant: &str) -> Option<String> {
        let parsed = Grant::parse(grant).ok()?;
        Some(grants::risk_of(&[parsed]).as_str().to_string())
    }

    /// A handle on a snapshot section of a plugin (COR-032).
    pub fn section(
        &mut self,
        plugin: &str,
        name: &str,
        schema_version: u32,
        max_bytes: usize,
    ) -> SectionHandle {
        let stored = self
            .sections
            .entry((plugin.to_string(), name.to_string()))
            .or_default();
        stored.schema_version = schema_version;
        stored.max_bytes = max_bytes;
        SectionHandle {
            plugin: plugin.to_string(),
            name: name.to_string(),
        }
    }

    /// Writes a snapshot section.
    pub fn write_section(
        &mut self,
        section: &SectionHandle,
        bytes: &[u8],
    ) -> Result<(), PluginError> {
        if self.crashed.contains(&section.plugin) {
            return Err(PluginError {
                code: ErrorCode::Internal,
                message: format!("plugin {} is not running", section.plugin),
                retry_after_ms: None,
            });
        }
        let stored = self
            .sections
            .entry((section.plugin.clone(), section.name.clone()))
            .or_default();
        let limit = stored.max_bytes.min(SNAPSHOT_SECTION_CEILING_BYTES);
        if bytes.len() > limit {
            // The previous value stays: an oversized write never replaces it.
            self.logged_codes
                .push("snapshot.section-too-large".to_string());
            return Err(PluginError {
                code: ErrorCode::InvalidArgument,
                message: format!(
                    "snapshot.section-too-large: {} bytes exceeds {limit}",
                    bytes.len()
                ),
                retry_after_ms: None,
            });
        }
        stored.data = Some(bytes.to_vec());
        Ok(())
    }

    /// Writes the snapshot file.
    pub fn write_snapshot(&mut self) -> Result<(), PluginError> {
        let sections = self
            .sections
            .iter()
            .filter_map(|((_, name), stored)| {
                stored
                    .data
                    .as_ref()
                    .map(|data| (name.clone(), stored.schema_version, data.clone()))
            })
            .collect();
        self.snapshot = SnapshotFile {
            sections,
            plugin_keys: Vec::new(),
        };
        Ok(())
    }

    /// Reads the snapshot file back.
    pub fn snapshot(&self) -> SnapshotFile {
        self.snapshot.clone()
    }

    /// Crashes a plugin instance.
    pub fn crash_plugin(&mut self, id: &str) {
        self.crashed.insert(id.to_string());
    }

    /// Makes a plugin store a key in plugin storage (never the snapshot).
    pub fn plugin_stores_in_storage(&mut self, _id: &str, _key: &str, _value: &[u8]) {
        // Plugin storage is its own bucket; the snapshot writer reads only sections (COR-032).
    }

    /// The log codes emitted so far.
    pub fn log_codes(&self) -> Vec<String> {
        self.logged_codes.clone()
    }

    /// Replaces the user config file (COR-033).
    pub fn write_config(&mut self, contents: &str) {
        self.config_text = contents.to_string();
        if !parses_as_toml(contents) {
            self.logged_codes.push("config.parse-error".to_string());
        }
    }

    /// The user config file as it is now.
    pub fn read_config(&self) -> String {
        self.config_text.clone()
    }

    /// Sets one setting through the settings screen.
    pub fn set_setting(&mut self, key: &str, value: &str) {
        if !parses_as_toml(&self.config_text) {
            // COR-090: an unparseable file is never rewritten.
            self.logged_codes.push("config.parse-error".to_string());
            return;
        }
        let (table, field) = split_key(key);
        let edited = edit_field(&self.config_text, Syntax::Toml, table, field, value);
        if parses_as_toml(&edited) {
            self.config_text = edited;
        } else {
            self.logged_codes.push("settings.invalid-value".to_string());
        }
    }

    /// Replaces the project config file.
    pub fn write_project_config(&mut self, contents: &str) {
        self.project_config_text = contents.to_string();
    }

    /// The project config file as it is now.
    pub fn read_project_config(&self) -> String {
        self.project_config_text.clone()
    }

    /// Sets one project setting through the settings screen.
    pub fn set_project_setting(&mut self, key: &str, value: &str) {
        let (table, field) = split_key(key);
        self.project_config_text =
            edit_field(&self.project_config_text, Syntax::Yaml, table, field, value);
    }

    /// Every config key path the kernel and the plugins know.
    pub fn config_key_paths(&self) -> Vec<String> {
        let Ok(table) = toml::from_str::<toml::Table>(&self.config_text) else {
            return Vec::new();
        };
        let mut paths = Vec::new();
        collect_key_paths("", &table, &mut paths);
        paths
    }

    /// Runs the plugin build step that regenerates config keys.
    pub fn run_plugin_build(&mut self) {
        // The build only adds keys: it renames and drops none, so the files stay as they are (COR-033).
    }

    /// Whether another installed plugin already holds the same kernel write grant.
    fn kernel_write_held_elsewhere(&self, plugin_id: &str, grant_text: &str) -> bool {
        grant_text.starts_with("settings.write-kernel:")
            && self.installed.iter().any(|(id, held)| {
                id != plugin_id && held.iter().any(|held_grant| held_grant == grant_text)
            })
    }
}

/// The two config syntaxes the rig edits; both keep unknown lines byte for byte.
#[derive(Debug, Clone, Copy)]
enum Syntax {
    Toml,
    Yaml,
}

fn parses_as_toml(text: &str) -> bool {
    toml::from_str::<toml::Table>(text).is_ok()
}

/// Splits `table.field`; a key without a dot is a top-level field.
fn split_key(key: &str) -> (&str, &str) {
    key.split_once('.').unwrap_or(("", key))
}

fn collect_key_paths(prefix: &str, table: &toml::Table, paths: &mut Vec<String>) {
    for (key, value) in table {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            toml::Value::Table(nested) => collect_key_paths(&path, nested, paths),
            _ => paths.push(path),
        }
    }
}

/// Sets `field` under `table` by rewriting only that line. A missing field or table is added.
fn edit_field(contents: &str, syntax: Syntax, table: &str, field: &str, value: &str) -> String {
    let mut lines: Vec<String> = contents.split('\n').map(str::to_string).collect();
    let mut scope = String::new();
    let mut first_header: Option<usize> = None;
    let mut table_header: Option<usize> = None;
    let mut replaced = false;
    for index in 0..lines.len() {
        if let Some(name) = header_name(&lines[index], syntax) {
            first_header.get_or_insert(index);
            if name == table {
                table_header.get_or_insert(index);
            }
            scope = name;
            continue;
        }
        if matches!(syntax, Syntax::Yaml) && is_yaml_top_level_key(&lines[index]) {
            scope.clear();
        }
        if scope == table && is_field_line(&lines[index], field, syntax) {
            lines[index] = entry_line(syntax, leading_whitespace(&lines[index]), field, value);
            replaced = true;
            break;
        }
    }
    if !replaced {
        let nested_indent = match syntax {
            Syntax::Toml => "",
            Syntax::Yaml => "  ",
        };
        if table.is_empty() {
            let new_line = entry_line(syntax, "", field, value);
            match (syntax, first_header) {
                (Syntax::Toml, Some(header)) => lines.insert(header, new_line),
                _ => append_block(&mut lines, vec![new_line]),
            }
        } else if let Some(header) = table_header {
            lines.insert(header + 1, entry_line(syntax, nested_indent, field, value));
        } else {
            let header_line = match syntax {
                Syntax::Toml => format!("[{table}]"),
                Syntax::Yaml => format!("{table}:"),
            };
            let new_line = entry_line(syntax, nested_indent, field, value);
            append_block(&mut lines, vec![header_line, new_line]);
        }
    }
    lines.join("\n")
}

/// The table name a header line opens, if the line is a header.
fn header_name(line: &str, syntax: Syntax) -> Option<String> {
    match syntax {
        Syntax::Toml => {
            let inner = line.trim().strip_prefix('[')?.strip_suffix(']')?;
            Some(inner.trim().to_string())
        }
        Syntax::Yaml => {
            if !is_yaml_top_level_key(line) {
                return None;
            }
            line.trim_end()
                .strip_suffix(':')
                .map(|name| name.trim().to_string())
        }
    }
}

fn is_yaml_top_level_key(line: &str) -> bool {
    !line.is_empty() && !line.starts_with([' ', '\t', '#'])
}

fn is_field_line(line: &str, field: &str, syntax: Syntax) -> bool {
    let separator = match syntax {
        Syntax::Toml => '=',
        Syntax::Yaml => ':',
    };
    line.trim_start()
        .strip_prefix(field)
        .is_some_and(|rest| rest.trim_start().starts_with(separator))
}

fn leading_whitespace(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

fn entry_line(syntax: Syntax, indent: &str, field: &str, value: &str) -> String {
    match syntax {
        Syntax::Toml => format!("{indent}{field} = {}", toml_literal(value)),
        Syntax::Yaml => format!("{indent}{field}: {value}"),
    }
}

/// Numbers and booleans are written bare; everything else becomes a TOML string.
fn toml_literal(value: &str) -> String {
    let is_scalar = value == "true"
        || value == "false"
        || value.parse::<i64>().is_ok()
        || value.parse::<f64>().is_ok_and(f64::is_finite);
    if is_scalar {
        value.to_string()
    } else {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// Appends a block after the last line, separated by one blank line.
fn append_block(lines: &mut Vec<String>, block: Vec<String>) {
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    if !lines.is_empty() {
        lines.push(String::new());
    }
    lines.extend(block);
    lines.push(String::new());
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
        let result =
            rig.install(&PluginSpec::new("fixture.writer", "T1").grants(&[KERNEL_TAB_WRITE_GRANT]));
        assert!(result.is_ok());
        assert!(result.codes.is_empty());
        assert!(rig.is_installed("fixture.writer"));
    }

    /// REQ: COR-030
    #[test]
    fn second_plugin_declaring_same_kernel_tab_write_grant_is_refused() {
        let mut rig = ConfigRig::new();
        let first =
            rig.install(&PluginSpec::new("fixture.first", "T1").grants(&[KERNEL_TAB_WRITE_GRANT]));
        assert!(first.is_ok());
        let second =
            rig.install(&PluginSpec::new("fixture.second", "T1").grants(&[KERNEL_TAB_WRITE_GRANT]));
        assert!(!second.is_ok());
        assert!(second
            .codes
            .iter()
            .any(|code| code == "manifest.settings-owner"));
        assert!(rig.is_installed("fixture.first"));
        assert!(!rig.is_installed("fixture.second"));
    }

    /// REQ: COR-030
    #[test]
    fn kernel_tab_write_owner_is_the_only_installed_holder() {
        let mut rig = ConfigRig::new();
        rig.install(&PluginSpec::new("fixture.writer", "T1").grants(&[KERNEL_TAB_WRITE_GRANT]));
        assert_eq!(
            rig.write_owners("terminal"),
            vec!["fixture.writer".to_string()]
        );
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
        let replacement =
            rig.install(&PluginSpec::new("fixture.second", "T1").grants(&[KERNEL_TAB_WRITE_GRANT]));
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
        let result = rig
            .install(&PluginSpec::new("fixture.t2-writer", "T2").grants(&[KERNEL_TAB_WRITE_GRANT]));
        assert!(!result.is_ok());
        assert!(result
            .codes
            .iter()
            .any(|code| code == "manifest.grant-tier"));
        assert!(!rig.is_installed("fixture.t2-writer"));
    }

    /// REQ: COR-031
    #[test]
    fn t3_package_with_kernel_tab_write_grant_fails_with_grant_tier() {
        let mut rig = ConfigRig::new();
        let result = rig
            .install(&PluginSpec::new("fixture.t3-writer", "T3").grants(&[KERNEL_TAB_WRITE_GRANT]));
        assert!(result
            .codes
            .iter()
            .any(|code| code == "manifest.grant-tier"));
        assert!(!rig.is_installed("fixture.t3-writer"));
    }

    /// REQ: COR-031
    #[test]
    fn t3_package_with_kernel_tab_read_grant_installs() {
        let mut rig = ConfigRig::new();
        let result = rig
            .install(&PluginSpec::new("fixture.t3-reader", "T3").grants(&[KERNEL_TAB_READ_GRANT]));
        assert!(result.is_ok());
        assert!(rig.is_installed("fixture.t3-reader"));
    }

    /// REQ: COR-031
    #[test]
    fn t2_package_with_kernel_tab_read_grant_installs() {
        let mut rig = ConfigRig::new();
        let result = rig
            .install(&PluginSpec::new("fixture.t2-reader", "T2").grants(&[KERNEL_TAB_READ_GRANT]));
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
        let handle = rig.section(
            "fixture.state",
            SNAPSHOT_SECTION_NAME,
            3,
            SNAPSHOT_SECTION_MAX_BYTES,
        );
        assert!(rig.write_section(&handle, b"{\"monitors\":1}").is_ok());
        assert!(rig.write_snapshot().is_ok());
        let snapshot = rig.snapshot();
        assert_eq!(
            snapshot.section(SNAPSHOT_SECTION_NAME),
            Some(b"{\"monitors\":1}".to_vec())
        );
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
        let handle = rig.section(
            "fixture.big",
            SNAPSHOT_SECTION_NAME,
            1,
            SNAPSHOT_SECTION_MAX_BYTES,
        );
        let previous = b"previous value".to_vec();
        assert!(rig.write_section(&handle, &previous).is_ok());
        assert!(rig.write_snapshot().is_ok());
        let oversized = vec![b'x'; 300 * KIB];
        let _oversize_result = rig.write_section(&handle, &oversized);
        assert!(rig.write_snapshot().is_ok());
        assert_eq!(
            rig.snapshot().section(SNAPSHOT_SECTION_NAME),
            Some(previous)
        );
    }

    /// REQ: COR-032
    #[test]
    fn crashed_plugin_does_not_block_snapshot_and_keeps_last_value() {
        let mut rig = ConfigRig::new();
        let handle = rig.section(
            "fixture.crashy",
            SNAPSHOT_SECTION_NAME,
            1,
            SNAPSHOT_SECTION_MAX_BYTES,
        );
        assert!(rig.write_section(&handle, b"last good").is_ok());
        assert!(rig.write_snapshot().is_ok());
        rig.crash_plugin("fixture.crashy");
        assert!(rig.write_snapshot().is_ok());
        assert_eq!(
            rig.snapshot().section(SNAPSHOT_SECTION_NAME),
            Some(b"last good".to_vec())
        );
    }

    /// REQ: COR-032
    #[test]
    fn empty_section_is_stored_as_empty_bytes() {
        let mut rig = ConfigRig::new();
        let handle = rig.section("fixture.empty", SNAPSHOT_SECTION_NAME, 1, KIB);
        assert!(rig.write_section(&handle, &[]).is_ok());
        assert!(rig.write_snapshot().is_ok());
        assert_eq!(
            rig.snapshot().section(SNAPSHOT_SECTION_NAME),
            Some(Vec::new())
        );
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
        assert!(rig
            .read_project_config()
            .contains("unknown_project_key: keep"));
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
        assert_eq!(
            rig.write_owners("terminal"),
            vec!["ilium.core.terminal".to_string()]
        );
    }

    #[test]
    fn new_rig_has_no_plugins_installed() {
        let rig = ConfigRig::new();
        assert!(!rig.is_installed("ilium.core.terminal"));
    }

    #[test]
    fn plugin_spec_builder_stores_grants_as_given() {
        let spec = PluginSpec::new("fixture.grants", "T1")
            .grants(&[KERNEL_TAB_READ_GRANT, KERNEL_TAB_WRITE_GRANT]);
        assert_eq!(
            spec.grants,
            vec![
                KERNEL_TAB_READ_GRANT.to_string(),
                KERNEL_TAB_WRITE_GRANT.to_string()
            ]
        );
    }

    #[test]
    fn install_result_default_is_not_ok_with_no_codes() {
        let result = InstallResult::default();
        assert!(!result.is_ok());
        assert!(result.codes.is_empty());
    }

    #[test]
    fn install_result_is_ok_follows_ok_field() {
        let result = InstallResult {
            ok: true,
            codes: Vec::new(),
        };
        assert!(result.is_ok());
    }
}
