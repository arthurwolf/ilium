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

#[cfg(test)]
mod tests {
    use super::{kernel_tab_ids, table_of_tab, ConfigFileSettings};
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
}
