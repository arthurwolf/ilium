//! Deterministic configuration model for COR-030..033. This fixture models
//! documents and section replies; it does not start guests, migrate a real server,
//! or claim filesystem durability. Runtime and disk acceptance need separate tests.
use ilium_plugin_api::grants::{check_grant, risk_of, Grant};
use ilium_plugin_api::{ErrorCode, PluginError, Side, Tier};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSpec {
    id: String,
    tier: String,
    grants: Vec<String>,
    snapshot_sections: Vec<(String, u32, usize)>,
}
impl PluginSpec {
    pub fn new(id: &str, tier: &str) -> Self {
        Self {
            id: id.into(),
            tier: tier.into(),
            grants: vec![],
            snapshot_sections: vec![],
        }
    }
    pub fn grants(mut self, grants: &[&str]) -> Self {
        self.grants = grants.iter().map(|value| (*value).into()).collect();
        self
    }
    pub fn snapshot_section(mut self, name: &str, version: u32, max: usize) -> Self {
        self.snapshot_sections.push((name.into(), version, max));
        self
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InstallResult {
    pub ok: bool,
    pub codes: Vec<String>,
}
impl InstallResult {
    pub fn is_ok(&self) -> bool {
        self.ok
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionHandle {
    plugin: String,
    name: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct SnapshotFile {
    sections: Vec<(String, u32, Vec<u8>)>,
    plugin_keys: Vec<String>,
}
impl SnapshotFile {
    pub fn section(&self, name: &str) -> Option<Vec<u8>> {
        self.sections
            .iter()
            .find(|entry| entry.0 == name)
            .map(|entry| entry.2.clone())
    }
    pub fn schema_version(&self, name: &str) -> Option<u32> {
        self.sections
            .iter()
            .find(|entry| entry.0 == name)
            .map(|entry| entry.1)
    }
    pub fn plugin_keys(&self) -> Vec<String> {
        self.plugin_keys.clone()
    }
}
#[derive(Debug, Default)]
pub struct ConfigRig {
    plugins: BTreeMap<String, PluginSpec>,
    sections: BTreeMap<String, (String, u32, usize, Vec<u8>)>,
    crashed: BTreeSet<String>,
    storage: BTreeMap<(String, String), Vec<u8>>,
    snapshot: SnapshotFile,
    config: String,
    project: String,
    codes: Vec<String>,
}
fn error(message: &str) -> PluginError {
    PluginError {
        code: ErrorCode::InvalidArgument,
        message: message.into(),
        retry_after_ms: None,
    }
}
impl ConfigRig {
    pub fn new() -> Self {
        Self::default()
    }
    /// Native Editor and Kanban tabs deliberately have no plugin owner in v1.
    pub fn with_core_plugins() -> Self {
        let mut rig = Self::new();
        let owners = [
            "settings",
            "settings",
            "agent-detect",
            "cost",
            "compaction",
            "compaction",
            "keys-help",
            "terminal",
            "",
            "sessions",
            "worktrees",
            "",
            "ambient-look",
            "notifications",
            "voice",
            "cost",
            "inference",
            "inference",
            "titles",
            "titles",
            "text-triggers",
            "settings",
            "fleet",
            "settings",
            "agent-setup",
        ];
        for (tab, owner) in crate::settings::kernel_tab_ids().iter().zip(owners) {
            if owner.is_empty() {
                continue;
            }
            let id = format!("ilium.core.{owner}");
            let spec = rig
                .plugins
                .entry(id.clone())
                .or_insert_with(|| PluginSpec::new(&id, "T1"));
            spec.grants.push(format!("settings.write-kernel:{tab}"));
        }
        rig
    }
    pub fn install(&mut self, plugin: &PluginSpec) -> InstallResult {
        let mut codes = BTreeSet::new();
        let tier = match plugin.tier.as_str() {
            "T1" => Tier::T1,
            "T2" => Tier::T2,
            "T3" => Tier::T3,
            _ => {
                codes.insert("manifest.schema".into());
                Tier::T3
            }
        };
        if plugin.id.is_empty()
            || !plugin.id.contains('.')
            || plugin
                .id
                .bytes()
                .any(|b| !(b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-'))
        {
            codes.insert("manifest.id-invalid".into());
        }
        if self.plugins.contains_key(&plugin.id) {
            codes.insert("install.already-installed".into());
        }
        for text in &plugin.grants {
            match Grant::parse(text) {
                Err(_) => {
                    codes.insert("manifest.grant-unknown".into());
                }
                Ok(grant) => {
                    if let Err(diagnostic) = check_grant(&grant, tier, Side::Server) {
                        codes.insert(diagnostic.code.as_str().to_owned());
                    }
                    if let Some(tab) = text.strip_prefix("settings.write-kernel:") {
                        if !self.write_owners(tab).is_empty() {
                            codes.insert("manifest.settings-owner".into());
                        }
                    }
                }
            }
        }
        if plugin.snapshot_sections.len() > 1
            || plugin.snapshot_sections.iter().any(|(name, _, max)| {
                name.is_empty() || *max == 0 || *max > 262144 || self.sections.contains_key(name)
            })
        {
            codes.insert("manifest.schema".into());
        }
        if !codes.is_empty() {
            return InstallResult {
                ok: false,
                codes: codes.into_iter().collect(),
            };
        }
        self.plugins.insert(plugin.id.clone(), plugin.clone());
        for (name, version, max) in &plugin.snapshot_sections {
            self.sections
                .insert(name.clone(), (plugin.id.clone(), *version, *max, vec![]));
        }
        InstallResult {
            ok: true,
            codes: vec![],
        }
    }
    pub fn uninstall(&mut self, id: &str) -> bool {
        self.plugins.remove(id).is_some()
    }
    pub fn is_installed(&self, id: &str) -> bool {
        self.plugins.contains_key(id)
    }
    pub fn kernel_tabs(&self) -> Vec<String> {
        crate::settings::kernel_tab_ids()
            .iter()
            .map(|tab| (*tab).into())
            .collect()
    }
    pub fn write_owners(&self, tab: &str) -> Vec<String> {
        let grant = format!("settings.write-kernel:{tab}");
        self.plugins
            .values()
            .filter(|plugin| plugin.grants.contains(&grant))
            .map(|plugin| plugin.id.clone())
            .collect()
    }
    pub fn risk_of(&self, text: &str) -> Option<String> {
        Grant::parse(text)
            .ok()
            .map(|grant| risk_of(&[grant]).as_str().into())
    }
    /// Declares a fixture handler's section. Invalid declarations yield unusable
    /// handles; they never replace another owner's section or increase its quota.
    pub fn section(&mut self, plugin: &str, name: &str, version: u32, max: usize) -> SectionHandle {
        if self.is_installed(plugin)
            && !name.is_empty()
            && max > 0
            && max <= 262144
            && !self.sections.contains_key(name)
            && !self.sections.values().any(|entry| entry.0 == plugin)
        {
            self.sections
                .insert(name.into(), (plugin.into(), version, max, vec![]));
        }
        SectionHandle {
            plugin: plugin.into(),
            name: name.into(),
        }
    }
    pub fn write_section(
        &mut self,
        section: &SectionHandle,
        bytes: &[u8],
    ) -> Result<(), PluginError> {
        if !self.is_installed(&section.plugin) || self.crashed.contains(&section.plugin) {
            return Err(error("section owner is unavailable"));
        }
        let entry = self
            .sections
            .get_mut(&section.name)
            .ok_or_else(|| error("undeclared section"))?;
        if entry.0 != section.plugin {
            return Err(error("section belongs to another plugin"));
        }
        if bytes.len() > entry.2 {
            self.codes.push("snapshot.section-too-large".into());
            return Err(error("snapshot.section-too-large"));
        }
        entry.3 = bytes.to_vec();
        self.write_snapshot()
    }
    pub fn write_snapshot(&mut self) -> Result<(), PluginError> {
        // Encode and decode the actual candidate, rather than return a canned result.
        // Private plugin storage is deliberately not part of this fixture document.
        let candidate = SnapshotFile {
            sections: self
                .sections
                .iter()
                .map(|(name, (_, version, _, bytes))| (name.clone(), *version, bytes.clone()))
                .collect(),
            plugin_keys: vec![],
        };
        let bytes =
            serde_json::to_vec(&candidate).map_err(|_| error("snapshot encoding failed"))?;
        self.snapshot =
            serde_json::from_slice(&bytes).map_err(|_| error("snapshot decoding failed"))?;
        Ok(())
    }
    pub fn snapshot(&self) -> SnapshotFile {
        self.snapshot.clone()
    }
    pub fn crash_plugin(&mut self, id: &str) {
        self.crashed.insert(id.into());
    }
    pub fn plugin_stores_in_storage(&mut self, id: &str, key: &str, value: &[u8]) {
        if self.is_installed(id) {
            self.storage.insert((id.into(), key.into()), value.to_vec());
        }
    }
    pub fn log_codes(&self) -> Vec<String> {
        self.codes.clone()
    }
    pub fn write_config(&mut self, contents: &str) {
        self.config = contents.into();
    }
    pub fn read_config(&self) -> String {
        self.config.clone()
    }
    pub fn set_setting(&mut self, key: &str, value: &str) {
        match edit_toml(&self.config, key, value) {
            Ok(text) => self.config = text,
            Err(code) => self.codes.push(code.into()),
        }
    }
    pub fn write_project_config(&mut self, contents: &str) {
        self.project = contents.into();
    }
    pub fn read_project_config(&self) -> String {
        self.project.clone()
    }
    pub fn set_project_setting(&mut self, _key: &str, _value: &str) {
        // No YAML parser/round-trip writer is available in this crate. Fail closed,
        // preserving the original instead of pretending a line substitution is YAML.
        self.codes.push("config.yaml-context-required".into());
    }
    pub fn config_key_paths(&self) -> Vec<String> {
        fn walk(value: &toml::Value, prefix: &str, result: &mut Vec<String>) {
            if let Some(table) = value.as_table() {
                for (key, value) in table {
                    let path = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    walk(value, &path, result);
                }
            } else {
                result.push(prefix.into());
            }
        }
        let mut result = vec![];
        if let Ok(value) = self.config.parse::<toml::Value>() {
            walk(&value, "", &mut result);
        }
        result
    }
    /// There is no guest build runner in this fixture: do not claim one ran.
    pub fn run_plugin_build(&mut self) {
        self.codes.push("config.build-context-required".into());
    }
}

/// A conservative, formatting-preserving edit of an existing simple TOML key.
/// The complete document and replacement are parsed before and after the edit.
/// Complex/quoted/dotted source syntax is refused rather than rewritten lossily.
fn edit_toml(source: &str, path: &str, replacement: &str) -> Result<String, &'static str> {
    let original: toml::Value = source.parse().map_err(|_| "config.parse-error")?;
    let candidate: toml::Value = format!("value = {replacement}")
        .parse()
        .map_err(|_| "settings.invalid-value")?;
    let value = candidate.get("value").ok_or("settings.invalid-value")?;
    let keys: Vec<_> = path.split('.').collect();
    let mut old = &original;
    for key in &keys {
        old = old.get(*key).ok_or("settings.unknown-key")?;
    }
    if old == value {
        return Ok(source.into());
    }
    let mut table = String::new();
    let mut output = String::new();
    let mut found = false;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            table = trimmed[1..trimmed.len() - 1].to_owned();
        }
        let expected = if table.is_empty() {
            keys.last().copied().unwrap_or("").to_owned()
        } else {
            format!("{table}.{}", keys.last().copied().unwrap_or(""))
        };
        match line.split_once('=') {
            Some((left, _))
                if expected == path && left.trim() == keys.last().copied().unwrap_or("") =>
            {
                if found || line.contains('#') {
                    return Err("config.edit-context-required");
                }
                let suffix = if line.ends_with('\n') { "\n" } else { "" };
                output.push_str(&format!("{left}= {replacement}{suffix}"));
                found = true;
            }
            _ => output.push_str(line),
        }
    }
    if !found {
        return Err("config.edit-context-required");
    }
    let updated: toml::Value = output.parse().map_err(|_| "config.edit-context-required")?;
    let mut expected = original;
    let mut target = &mut expected;
    for key in &keys[..keys.len() - 1] {
        target = target.get_mut(*key).ok_or("settings.unknown-key")?;
    }
    *target
        .get_mut(keys[keys.len() - 1])
        .ok_or("settings.unknown-key")? = value.clone();
    if updated != expected {
        return Err("config.edit-context-required");
    }
    Ok(output)
}
