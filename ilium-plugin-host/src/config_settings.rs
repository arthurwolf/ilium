//! Kernel settings tables over `config.toml` (foundation contract section 20.3).
//!
//! Both kernel facades hand this to `ilium:host/kernel-settings`. A kernel tab id of the
//! manual (`api-kernel-settings-and-keymap.md`, "The kernel tab ids") names one table of
//! the user's `config.toml`; a read returns that table as a JSON object and a write replaces
//! one key of it. The file is the single store, so the Settings screen and a plugin never
//! hold two copies of a value.

use ilium_platform::file_lock::ExclusiveFileLock;
use ilium_platform::secure_fs;
use ilium_plugin_api::generated::types::ErrorCode;
use ilium_plugin_api::kernel::{FacadeError, KernelSettings};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The largest `config.toml` a write produces, the same bound the Settings screen uses.
const CONFIG_BYTES_MAX: usize = 256 * 1024;

/// The lock every writer of `config.toml` holds (the client and the server use this name).
const CONFIG_LOCK_FILE: &str = ".agent-detection-settings.lock";

/// The frozen kernel tab ids and the `config.toml` table each one reads.
///
/// A tab without a table of its own (`about`) reads as an empty object. `icons` shares the
/// table of the User Interface tab, where its keys are stored.
const KERNEL_TABS: [(&str, &str); 25] = [
    ("user-interface", "ui"),
    ("icons", "ui"),
    ("agent-monitoring", "attention"),
    ("agent-cost", "cost"),
    ("optimization", "optimization"),
    ("remote-compaction", "remote_compaction"),
    ("keyboard", "keyboard"),
    ("terminal", "terminal"),
    ("editor", "editor"),
    ("session", "session"),
    ("git", "git"),
    ("kanban-board", "kanban_board"),
    ("animations", "animations"),
    ("sound", "sound"),
    ("voice-control", "voice"),
    ("reset-planning", "reset_planning"),
    ("inference", "inference"),
    ("llm-instructions", "agent_setup"),
    ("titles", "titles"),
    ("triggers", "triggers"),
    ("text-triggers", "text_triggers"),
    ("debug", "debug"),
    ("api", "api"),
    ("about", "about"),
    ("setup", "onboarding"),
];

/// The `config.toml` table of a kernel tab id, if the id is one of the frozen ids.
pub fn table_of_tab(tab: &str) -> Option<&'static str> {
    KERNEL_TABS
        .iter()
        .find(|(id, _)| *id == tab)
        .map(|(_, table)| *table)
}

/// Every frozen kernel tab id, in the order of the manual.
pub fn kernel_tab_ids() -> impl Iterator<Item = &'static str> {
    KERNEL_TABS.iter().map(|(id, _)| *id)
}

/// `KernelSettings` over one `config.toml`.
pub struct ConfigFileSettings {
    path: Option<PathBuf>,
    revision: AtomicU64,
}

impl ConfigFileSettings {
    /// The tables of the user's own `config.toml` (honours `ILIUM_CONFIG_DIR`).
    pub fn of_user() -> Self {
        Self {
            path: ilium_platform::paths::config_dir().map(|dir| dir.join("config.toml")),
            revision: AtomicU64::new(0),
        }
    }

    /// The tables of a given file, for tests and isolated runs.
    pub fn at(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            revision: AtomicU64::new(0),
        }
    }

    fn read_document(path: &Path) -> Result<toml::Table, FacadeError> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str::<toml::Table>(&text).map_err(|error| {
                FacadeError::new(
                    ErrorCode::Internal,
                    format!("config.toml does not parse: {error}"),
                )
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(toml::Table::new()),
            Err(error) => Err(FacadeError::new(
                ErrorCode::Internal,
                format!("config.toml is not readable: {error}"),
            )),
        }
    }

    fn write_document(path: &Path, document: &toml::Table) -> Result<(), FacadeError> {
        let io_error = |error: std::io::Error| {
            FacadeError::new(
                ErrorCode::Internal,
                format!("config.toml is not writable: {error}"),
            )
        };
        let serialized = toml::to_string_pretty(document).map_err(|error| {
            FacadeError::new(
                ErrorCode::Internal,
                format!("config.toml does not serialize: {error}"),
            )
        })?;
        if serialized.len() > CONFIG_BYTES_MAX {
            return Err(FacadeError::new(
                ErrorCode::InvalidArgument,
                "config.toml would exceed 256 KiB",
            ));
        }
        // config.toml holds credentials, so the temporary file is owner-only before any
        // byte is written, as in the Settings screen's own save path.
        let temporary = path.with_extension(format!(
            "toml.tmp-plugin-{}-{}",
            std::process::id(),
            TEMPORARY_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| -> std::io::Result<()> {
            let mut file = secure_fs::private_open_options()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            secure_fs::restrict_open_file_to_owner(&file)?;
            file.write_all(serialized.as_bytes())?;
            file.sync_all()?;
            drop(file);
            secure_fs::replace_file_durably(&temporary, path)
        })();
        if result.is_err() {
            // The unique temporary file of this write is removed; a missing one is fine.
            let _ = std::fs::remove_file(&temporary);
        }
        result.map_err(io_error)
    }
}

/// Distinguishes the temporary files of concurrent writes of one process.
static TEMPORARY_SERIAL: AtomicU64 = AtomicU64::new(0);

/// A JSON value as the TOML value `config.toml` stores. TOML has no null.
fn json_to_toml(value: &serde_json::Value) -> Option<toml::Value> {
    Some(match value {
        serde_json::Value::Null => return None,
        serde_json::Value::Bool(flag) => toml::Value::Boolean(*flag),
        serde_json::Value::Number(number) => match number.as_i64() {
            Some(integer) => toml::Value::Integer(integer),
            None => toml::Value::Float(number.as_f64()?),
        },
        serde_json::Value::String(text) => toml::Value::String(text.clone()),
        serde_json::Value::Array(items) => {
            toml::Value::Array(items.iter().map(json_to_toml).collect::<Option<Vec<_>>>()?)
        }
        serde_json::Value::Object(fields) => {
            let mut table = toml::Table::new();
            for (key, field) in fields {
                table.insert(key.clone(), json_to_toml(field)?);
            }
            toml::Value::Table(table)
        }
    })
}

impl KernelSettings for ConfigFileSettings {
    /// `key` is a kernel tab id; the value is the JSON object of that tab's table.
    fn get(&self, key: &str) -> Option<String> {
        let table = table_of_tab(key)?;
        let document = self
            .path
            .as_deref()
            .and_then(|path| Self::read_document(path).ok())
            .unwrap_or_default();
        let values = match document.get(table) {
            Some(toml::Value::Table(values)) => serde_json::to_value(values).ok()?,
            _ => serde_json::Value::Object(serde_json::Map::new()),
        };
        Some(values.to_string())
    }

    /// `key` is `<tab-id>.<key>`; the value replaces that one key of the tab's table.
    fn set(&self, key: &str, value_json: &str) -> Result<(), FacadeError> {
        let (tab, name) = key.split_once('.').ok_or_else(|| {
            FacadeError::new(ErrorCode::InvalidArgument, "the key is not <tab>.<key>")
        })?;
        let table = table_of_tab(tab)
            .ok_or_else(|| FacadeError::new(ErrorCode::NotFound, format!("unknown tab {tab:?}")))?;
        if name.is_empty() || name.contains('.') {
            return Err(FacadeError::new(
                ErrorCode::InvalidArgument,
                "the key names one value of the tab",
            ));
        }
        let value = serde_json::from_str::<serde_json::Value>(value_json)
            .ok()
            .as_ref()
            .and_then(json_to_toml)
            .ok_or_else(|| {
                FacadeError::new(
                    ErrorCode::InvalidArgument,
                    "the value has no config.toml form",
                )
            })?;
        let path = self.path.as_deref().ok_or_else(|| {
            FacadeError::new(ErrorCode::Unsupported, "no configuration directory")
        })?;
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent).map_err(|error| {
            FacadeError::new(
                ErrorCode::Internal,
                format!("the configuration directory is not writable: {error}"),
            )
        })?;
        // Held across read, change and write: the Settings screen and the server's
        // detection saver take the same lock.
        let _lock =
            ExclusiveFileLock::acquire(&parent.join(CONFIG_LOCK_FILE)).map_err(|error| {
                FacadeError::new(
                    ErrorCode::Internal,
                    format!("config.toml is not lockable: {error}"),
                )
            })?;
        let mut document = Self::read_document(path)?;
        let entry = document
            .entry(table.to_owned())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        let toml::Value::Table(values) = entry else {
            return Err(FacadeError::new(
                ErrorCode::Internal,
                format!("[{table}] of config.toml is not a table"),
            ));
        };
        values.insert(name.to_owned(), value);
        Self::write_document(path, &document)?;
        self.revision.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }
}

/// Why a write of plugin settings did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginSettingsError {
    /// The patch breaks the schema; the text names the value and the rule, for the row.
    Invalid(String),
    /// The file could not be read, locked or written.
    Store(FacadeError),
}

impl From<FacadeError> for PluginSettingsError {
    fn from(error: FacadeError) -> Self {
        Self::Store(error)
    }
}

/// The values plugins store, in `[plugins."<plugin-id>"]` of `config.toml` (manual:
/// `api-settings.md`, "Where values live").
///
/// The Settings screen, the client's plugin rows and a server-side plugin all write through
/// this one store, under the lock every writer of `config.toml` holds, so two sides never
/// interleave a read and a write.
pub struct PluginSettingsStore {
    file: ConfigFileSettings,
}

impl PluginSettingsStore {
    /// The store of the user's own `config.toml` (honours `ILIUM_CONFIG_DIR`).
    pub fn of_user() -> Self {
        Self {
            file: ConfigFileSettings::of_user(),
        }
    }

    /// The store of a given file, for tests and isolated runs.
    pub fn at(path: PathBuf) -> Self {
        Self {
            file: ConfigFileSettings::at(path),
        }
    }

    /// Counts the writes this store made; a reader compares it to know its copy is stale.
    pub fn revision(&self) -> u64 {
        self.file.revision.load(Ordering::Acquire)
    }

    /// The stored values of a plugin; empty when it stored nothing.
    pub fn read(
        &self,
        plugin: &str,
    ) -> Result<serde_json::Map<String, serde_json::Value>, FacadeError> {
        let Some(path) = self.file.path.as_deref() else {
            return Ok(serde_json::Map::new());
        };
        let document = ConfigFileSettings::read_document(path)?;
        Ok(stored_values(&document, plugin))
    }

    /// Stores one value of a plugin without checking it; `null` removes the stored value.
    pub fn write(
        &self,
        plugin: &str,
        key: &str,
        value: &serde_json::Value,
    ) -> Result<(), FacadeError> {
        let mut patch = serde_json::Map::new();
        patch.insert(key.to_owned(), value.clone());
        self.update(plugin, |_stored| Ok(patch))
            .map(|_| ())
            .map_err(|error| match error {
                PluginSettingsError::Store(error) => error,
                PluginSettingsError::Invalid(message) => {
                    FacadeError::new(ErrorCode::InvalidArgument, message)
                }
            })
    }

    /// Applies a merge patch against a schema, all or nothing, and returns the keys whose
    /// value changed (empty when the patch changes nothing, in which case nothing is written).
    ///
    /// `null` resets a key to its default. The candidate that is checked is the defaults of the
    /// schema, then the stored values, then the patch, so a key outside the schema, a value of
    /// the wrong type and a value out of its range are all refused before anything is stored.
    pub fn apply_patch(
        &self,
        plugin: &str,
        schema_json: &str,
        patch: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Vec<String>, PluginSettingsError> {
        let defaults = schema_defaults(schema_json);
        self.update(plugin, |stored| {
            let mut candidate = defaults.clone();
            candidate.extend(stored.clone());
            for (key, value) in patch {
                if value.is_null() {
                    match defaults.get(key) {
                        Some(default) => candidate.insert(key.clone(), default.clone()),
                        None => candidate.remove(key),
                    };
                } else {
                    candidate.insert(key.clone(), value.clone());
                }
            }
            let text = serde_json::Value::Object(candidate).to_string();
            crate::settings::validate_settings(schema_json, &text).map_err(|findings| {
                PluginSettingsError::Invalid(
                    findings
                        .iter()
                        .map(|finding| {
                            if finding.pointer.is_empty() {
                                finding.message.clone()
                            } else {
                                format!("{}: {}", finding.pointer, finding.message)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("; "),
                )
            })?;
            Ok(patch.clone())
        })
    }

    /// Reads, changes and writes the plugin's table under the shared lock. `plan` sees the
    /// stored values and returns the patch to store (`null` removes); the keys it changes are
    /// returned.
    fn update(
        &self,
        plugin: &str,
        plan: impl FnOnce(
            &serde_json::Map<String, serde_json::Value>,
        )
            -> Result<serde_json::Map<String, serde_json::Value>, PluginSettingsError>,
    ) -> Result<Vec<String>, PluginSettingsError> {
        let path = self.file.path.as_deref().ok_or_else(|| {
            FacadeError::new(ErrorCode::Unsupported, "no configuration directory")
        })?;
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent).map_err(|error| {
            FacadeError::new(
                ErrorCode::Internal,
                format!("the configuration directory is not writable: {error}"),
            )
        })?;
        let _lock =
            ExclusiveFileLock::acquire(&parent.join(CONFIG_LOCK_FILE)).map_err(|error| {
                FacadeError::new(
                    ErrorCode::Internal,
                    format!("config.toml is not lockable: {error}"),
                )
            })?;
        let mut document = ConfigFileSettings::read_document(path)?;
        let stored = stored_values(&document, plugin);
        let patch = plan(&stored)?;
        let mut changed = Vec::new();
        let mut table = stored.clone();
        for (key, value) in patch {
            if value.is_null() {
                if table.remove(&key).is_some() {
                    changed.push(key);
                }
            } else if table.get(&key) != Some(&value) {
                table.insert(key.clone(), value);
                changed.push(key);
            }
        }
        if changed.is_empty() {
            return Ok(changed);
        }
        let mut values = toml::Table::new();
        for (key, value) in &table {
            let value = json_to_toml(value).ok_or_else(|| {
                PluginSettingsError::Invalid(format!("{key}: the value has no config.toml form"))
            })?;
            values.insert(key.clone(), value);
        }
        let plugins = document
            .entry("plugins".to_owned())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        let toml::Value::Table(plugins) = plugins else {
            return Err(FacadeError::new(
                ErrorCode::Internal,
                "[plugins] of config.toml is not a table",
            )
            .into());
        };
        plugins.insert(plugin.to_owned(), toml::Value::Table(values));
        ConfigFileSettings::write_document(path, &document)?;
        self.file.revision.fetch_add(1, Ordering::AcqRel);
        Ok(changed)
    }
}

/// The stored values of a plugin in a parsed `config.toml`.
fn stored_values(
    document: &toml::Table,
    plugin: &str,
) -> serde_json::Map<String, serde_json::Value> {
    document
        .get("plugins")
        .and_then(toml::Value::as_table)
        .and_then(|plugins| plugins.get(plugin))
        .and_then(toml::Value::as_table)
        .and_then(|values| serde_json::to_value(values).ok())
        .and_then(|value| match value {
            serde_json::Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default()
}

/// The `default` of every property of an object schema, as one JSON object.
pub fn schema_defaults(schema_json: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut defaults = serde_json::Map::new();
    let Ok(schema) = serde_json::from_str::<serde_json::Value>(schema_json) else {
        return defaults;
    };
    if let Some(properties) = schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
    {
        for (key, property) in properties {
            if let Some(default) = property.get("default") {
                defaults.insert(key.clone(), default.clone());
            }
        }
    }
    defaults
}

#[cfg(test)]
mod tests {
    use super::{
        kernel_tab_ids, table_of_tab, ConfigFileSettings, PluginSettingsError, PluginSettingsStore,
    };
    use ilium_plugin_api::generated::types::ErrorCode;
    use ilium_plugin_api::kernel::KernelSettings;

    #[test]
    fn the_tab_ids_are_the_twenty_five_of_the_manual() {
        assert_eq!(kernel_tab_ids().count(), 25);
        assert_eq!(table_of_tab("voice-control"), Some("voice"));
        assert_eq!(table_of_tab("nope"), None);
    }

    #[test]
    fn a_tab_reads_as_an_object_and_a_write_changes_one_key() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("config.toml");
        std::fs::write(&path, "[terminal]\nscrollback = 1000\nshell = \"zsh\"\n")
            .expect("config written");
        let settings = ConfigFileSettings::at(path);
        assert_eq!(settings.get("about").as_deref(), Some("{}"));
        assert_eq!(settings.get("unknown"), None);

        settings
            .set("terminal.scrollback", "2000")
            .expect("the write is accepted");
        let terminal: serde_json::Value =
            serde_json::from_str(&settings.get("terminal").expect("terminal tab"))
                .expect("JSON object");
        assert_eq!(terminal["scrollback"], 2000);
        assert_eq!(terminal["shell"], "zsh");
        assert_eq!(settings.revision(), 1);
    }

    #[test]
    fn a_write_to_an_unknown_tab_or_with_null_is_refused() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let settings = ConfigFileSettings::at(directory.path().join("config.toml"));
        let unknown = settings.set("nope.key", "1").expect_err("unknown tab");
        assert_eq!(unknown.code, ErrorCode::NotFound);
        let null = settings.set("terminal.key", "null").expect_err("null");
        assert_eq!(null.code, ErrorCode::InvalidArgument);
        assert_eq!(settings.revision(), 0);
    }

    const SCHEMA: &str = r#"{"type":"object","properties":{
        "volume":{"type":"integer","minimum":0,"maximum":10,"default":5},
        "name":{"type":"string","maxLength":8,"default":"ilium"},
        "loud":{"type":"boolean","default":false}}}"#;

    fn patch(json: &str) -> serde_json::Map<String, serde_json::Value> {
        match serde_json::from_str(json).expect("patch JSON") {
            serde_json::Value::Object(map) => map,
            other => panic!("not an object: {other}"),
        }
    }

    #[test]
    fn a_plugin_reads_nothing_until_it_stores_a_value() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let store = PluginSettingsStore::at(directory.path().join("config.toml"));
        assert!(store.read("acme.demo").expect("read").is_empty());
        assert_eq!(store.revision(), 0);
    }

    #[test]
    fn a_stored_value_is_read_back_from_the_plugin_table_of_the_file() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("config.toml");
        std::fs::write(&path, "[terminal]\nscrollback = 1000\n").expect("config written");
        let store = PluginSettingsStore::at(path.clone());
        let changed = store
            .apply_patch("acme.demo", SCHEMA, &patch(r#"{"volume":7}"#))
            .expect("the patch is accepted");
        assert_eq!(changed, vec!["volume".to_owned()]);
        assert_eq!(store.read("acme.demo").expect("read")["volume"], 7);
        assert_eq!(store.revision(), 1);
        let text = std::fs::read_to_string(&path).expect("file");
        assert!(text.contains("[plugins.\"acme.demo\"]"), "{text}");
        assert!(text.contains("scrollback = 1000"), "{text}");
        // Another plugin has its own table.
        assert!(store.read("acme.other").expect("read").is_empty());
    }

    #[test]
    fn an_unknown_key_a_wrong_type_and_an_out_of_range_value_are_refused_and_store_nothing() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("config.toml");
        let store = PluginSettingsStore::at(path.clone());
        for bad in [
            r#"{"nope":1}"#,
            r#"{"volume":"loud"}"#,
            r#"{"volume":11}"#,
            r#"{"name":"far too long"}"#,
            r#"{"volume":3,"loud":"yes"}"#,
        ] {
            let refused = store
                .apply_patch("acme.demo", SCHEMA, &patch(bad))
                .expect_err(bad);
            assert!(matches!(refused, PluginSettingsError::Invalid(_)), "{bad}");
        }
        assert!(!path.exists(), "a refused patch must not create the file");
        assert_eq!(store.revision(), 0);
    }

    #[test]
    fn the_refusal_names_the_value_and_the_rule() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let store = PluginSettingsStore::at(directory.path().join("config.toml"));
        let Err(PluginSettingsError::Invalid(text)) =
            store.apply_patch("acme.demo", SCHEMA, &patch(r#"{"volume":11}"#))
        else {
            panic!("a value out of range must be refused");
        };
        assert!(text.contains("/volume"), "{text}");
    }

    #[test]
    fn null_resets_a_key_and_a_patch_that_changes_nothing_writes_nothing() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let store = PluginSettingsStore::at(directory.path().join("config.toml"));
        store
            .apply_patch("acme.demo", SCHEMA, &patch(r#"{"volume":9,"loud":true}"#))
            .expect("stored");
        assert_eq!(store.revision(), 1);
        let same = store
            .apply_patch("acme.demo", SCHEMA, &patch(r#"{"volume":9}"#))
            .expect("accepted");
        assert!(same.is_empty());
        assert_eq!(store.revision(), 1);
        let reset = store
            .apply_patch("acme.demo", SCHEMA, &patch(r#"{"volume":null}"#))
            .expect("reset");
        assert_eq!(reset, vec!["volume".to_owned()]);
        let stored = store.read("acme.demo").expect("read");
        assert!(!stored.contains_key("volume"));
        assert_eq!(stored["loud"], true);
    }

    #[test]
    fn a_single_unchecked_write_stores_and_removes_a_value() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let store = PluginSettingsStore::at(directory.path().join("config.toml"));
        store
            .write("acme.demo", "note", &serde_json::json!("hi"))
            .expect("write");
        assert_eq!(store.read("acme.demo").expect("read")["note"], "hi");
        store
            .write("acme.demo", "note", &serde_json::Value::Null)
            .expect("remove");
        assert!(store.read("acme.demo").expect("read").is_empty());
    }

    #[test]
    fn schema_defaults_lists_the_default_of_each_property() {
        let defaults = super::schema_defaults(SCHEMA);
        assert_eq!(defaults["volume"], 5);
        assert_eq!(defaults["name"], "ilium");
        assert_eq!(defaults["loud"], false);
        assert!(super::schema_defaults("not json").is_empty());
    }
}
