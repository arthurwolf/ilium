//! On-disk registries: installed packages, grants and enable flags (reference-packaging.md,
//! reference-security.md).

use std::path::{Path, PathBuf};

use ilium_plugin_api::grants::Grant;
use ilium_plugin_api::ids::PluginId;

use crate::error::HostError;

/// The package store directory (reference-packaging.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageStore {
    /// Root directory of the store.
    pub root: PathBuf,
}

impl PackageStore {
    /// The directory of one installed version.
    pub fn version_dir(&self, _plugin: &PluginId, _version: &str) -> PathBuf {
        unimplemented!("host::registry::PackageStore::version_dir")
    }
}

/// One line of `installed.json` (reference-packaging.md).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InstalledRecord {
    /// The plugin id.
    pub id: String,
    /// The installed version.
    pub version: String,
    /// Where the package came from.
    pub source: String,
    /// The release channel.
    pub channel: String,
    /// Whether the user pinned this version.
    pub pinned: bool,
}

/// The registry of installed plugins, `installed.json` (reference-packaging.md).
#[derive(Debug)]
pub struct InstalledRegistry;

impl InstalledRegistry {
    /// Opens or creates the registry file.
    pub fn open(_path: &Path) -> Result<InstalledRegistry, HostError> {
        unimplemented!("host::registry::InstalledRegistry::open")
    }

    /// Every installed record.
    pub fn list(&self) -> Result<Vec<InstalledRecord>, HostError> {
        unimplemented!("host::registry::InstalledRegistry::list")
    }

    /// Inserts or replaces one record, atomically.
    pub fn put(&mut self, _record: InstalledRecord) -> Result<(), HostError> {
        unimplemented!("host::registry::InstalledRegistry::put")
    }

    /// Removes a record.
    pub fn remove(&mut self, _plugin: &PluginId) -> Result<(), HostError> {
        unimplemented!("host::registry::InstalledRegistry::remove")
    }
}

/// The grant store, `$XDG_STATE_HOME/ilium/grants.json`: mode 0600, single writer, atomic replace
/// (reference-security.md).
#[derive(Debug)]
pub struct GrantStore;

impl GrantStore {
    /// Opens the store, taking the single-writer lock.
    pub fn open(_path: &Path) -> Result<GrantStore, HostError> {
        unimplemented!("host::registry::GrantStore::open")
    }

    /// The grants a user approved for a plugin.
    pub fn granted(&self, _plugin: &PluginId) -> Result<Vec<Grant>, HostError> {
        unimplemented!("host::registry::GrantStore::granted")
    }

    /// Replaces the approved grants of a plugin, atomically.
    pub fn set_granted(&mut self, _plugin: &PluginId, _grants: &[Grant]) -> Result<(), HostError> {
        unimplemented!("host::registry::GrantStore::set_granted")
    }
}

/// Enable flags, `config.toml [plugins.<id>]` (reference-packaging.md).
#[derive(Debug)]
pub struct EnableFlags;

impl EnableFlags {
    /// Opens the flags of a config file.
    pub fn open(_path: &Path) -> Result<EnableFlags, HostError> {
        unimplemented!("host::registry::EnableFlags::open")
    }

    /// Whether the user enabled the plugin; a plugin without a flag is enabled.
    pub fn is_enabled(&self, _plugin: &PluginId) -> bool {
        unimplemented!("host::registry::EnableFlags::is_enabled")
    }

    /// Sets the flag.
    pub fn set_enabled(&mut self, _plugin: &PluginId, _enabled: bool) -> Result<(), HostError> {
        unimplemented!("host::registry::EnableFlags::set_enabled")
    }
}

/// The package download folder under the cache directory (reference-install-and-update.md PKG-267).
pub fn downloads_dir(_cache: &Path) -> PathBuf {
    unimplemented!("host::registry::downloads_dir")
}

/// The offline cache folder of one registry URL under the cache directory (PKG-267).
pub fn registry_cache_dir(_cache: &Path, _url: &str) -> PathBuf {
    unimplemented!("host::registry::registry_cache_dir")
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use ilium_plugin_api::grants::Grant;
    use ilium_plugin_api::ids::{parse_plugin_id, PluginId};

    use super::{
        downloads_dir, registry_cache_dir, EnableFlags, GrantStore, InstalledRecord,
        InstalledRegistry, PackageStore,
    };

    /// Parses a plugin id that the manual says is valid; a refusal fails the test.
    fn plugin(text: &str) -> PluginId {
        match parse_plugin_id(text) {
            Ok(plugin_id) => plugin_id,
            Err(diagnostic) => {
                panic!("expected plugin id `{text}` to be accepted, got {diagnostic}")
            }
        }
    }

    /// Parses a grant string that the manual says is valid; a refusal fails the test.
    fn grant(text: &str) -> Grant {
        match Grant::parse(text) {
            Ok(parsed) => parsed,
            Err(diagnostic) => panic!("expected grant `{text}` to parse, got {diagnostic}"),
        }
    }

    /// One installed record with the fixed fields the tests do not vary.
    fn record(id: &str, version: &str) -> InstalledRecord {
        InstalledRecord {
            id: id.to_owned(),
            version: version.to_owned(),
            source: "registry".to_owned(),
            channel: "stable".to_owned(),
            pinned: false,
        }
    }

    /// The text of every grant of a list, in order, for comparison with expected strings.
    fn grant_texts(grants: &[Grant]) -> Vec<String> {
        grants
            .iter()
            .map(|grant| grant.as_str().to_owned())
            .collect()
    }

    /// A fresh temporary directory; the returned guard keeps it alive.
    fn scratch() -> tempfile::TempDir {
        tempfile::tempdir().expect("temporary directory")
    }

    // ---- PackageStore::version_dir -------------------------------------------------------

    /// REQ: PKG-266
    #[test]
    fn version_dir_ends_with_id_and_version_under_root() {
        let dir = scratch();
        let store = PackageStore {
            root: dir.path().to_path_buf(),
        };
        let path = store.version_dir(&plugin("com.example.notes"), "1.2.0");
        assert!(
            path.starts_with(&store.root),
            "{} is not under the store root",
            path.display()
        );
        assert!(
            path.ends_with(Path::new("com.example.notes").join("1.2.0")),
            "{} does not end with id/version",
            path.display()
        );
    }

    /// REQ: LIF-076
    #[test]
    fn version_dir_differs_for_each_version_of_one_plugin() {
        let dir = scratch();
        let store = PackageStore {
            root: dir.path().to_path_buf(),
        };
        let notes = plugin("com.example.notes");
        let old = store.version_dir(&notes, "1.2.0");
        let new = store.version_dir(&notes, "1.3.0");
        assert_ne!(
            old, new,
            "an update must write a new folder, not reuse the old one"
        );
        assert_eq!(
            old.parent(),
            new.parent(),
            "both versions sit under the same plugin folder"
        );
    }

    /// REQ: PKG-266
    #[test]
    fn version_dir_differs_between_plugins_with_the_same_version() {
        let dir = scratch();
        let store = PackageStore {
            root: dir.path().to_path_buf(),
        };
        let first = store.version_dir(&plugin("com.example.notes"), "1.0.0");
        let second = store.version_dir(&plugin("com.example.tasks"), "1.0.0");
        assert_ne!(first, second);
    }

    // ---- InstalledRegistry ---------------------------------------------------------------

    /// REQ: PKG-265
    #[test]
    fn installed_open_missing_file_creates_registry_with_no_records() {
        let dir = scratch();
        let path = dir.path().join("installed.json");
        let registry = InstalledRegistry::open(&path).expect("opens or creates the registry");
        assert!(registry.list().expect("lists").is_empty());
    }

    /// REQ: LIF-065
    #[test]
    fn installed_put_then_reopen_returns_the_same_record() {
        let dir = scratch();
        let path = dir.path().join("installed.json");
        let notes = record("com.example.notes", "1.2.0");
        {
            let mut registry = InstalledRegistry::open(&path).expect("opens");
            registry.put(notes.clone()).expect("puts");
        }
        let reopened = InstalledRegistry::open(&path).expect("reopens");
        assert_eq!(reopened.list().expect("lists"), vec![notes]);
    }

    /// REQ: PKG-265
    #[test]
    fn installed_put_with_same_id_replaces_the_record() {
        let dir = scratch();
        let path = dir.path().join("installed.json");
        let mut registry = InstalledRegistry::open(&path).expect("opens");
        registry
            .put(record("com.example.notes", "1.2.0"))
            .expect("first put");
        registry
            .put(record("com.example.notes", "1.3.0"))
            .expect("second put");
        let records = registry.list().expect("lists");
        assert_eq!(records, vec![record("com.example.notes", "1.3.0")]);
    }

    /// REQ: PKG-265
    #[test]
    fn installed_list_is_sorted_by_id_in_byte_order() {
        let dir = scratch();
        let path = dir.path().join("installed.json");
        let mut registry = InstalledRegistry::open(&path).expect("opens");
        registry
            .put(record("com.example.zeta", "1.0.0"))
            .expect("puts zeta");
        registry
            .put(record("com.example.alpha", "1.0.0"))
            .expect("puts alpha");
        let ids: Vec<String> = registry
            .list()
            .expect("lists")
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(
            ids,
            vec![
                "com.example.alpha".to_owned(),
                "com.example.zeta".to_owned()
            ]
        );
    }

    /// REQ: PKG-293
    #[test]
    fn installed_remove_deletes_only_the_named_record() {
        let dir = scratch();
        let path = dir.path().join("installed.json");
        let mut registry = InstalledRegistry::open(&path).expect("opens");
        registry
            .put(record("com.example.notes", "1.2.0"))
            .expect("puts notes");
        registry
            .put(record("com.example.tasks", "2.0.0"))
            .expect("puts tasks");
        registry
            .remove(&plugin("com.example.notes"))
            .expect("removes notes");
        assert_eq!(
            registry.list().expect("lists"),
            vec![record("com.example.tasks", "2.0.0")]
        );
    }

    /// REQ: LIF-065
    #[test]
    fn installed_put_leaves_no_temporary_file_behind() {
        let dir = scratch();
        let path = dir.path().join("installed.json");
        let mut registry = InstalledRegistry::open(&path).expect("opens");
        registry
            .put(record("com.example.notes", "1.2.0"))
            .expect("puts");
        let leftovers: Vec<String> = std::fs::read_dir(dir.path())
            .expect("reads folder")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .filter(|name| name.starts_with("installed.json.") || name.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temporary files left behind: {leftovers:?}"
        );
    }

    /// REQ: LIF-282
    #[test]
    fn installed_open_refuses_unparseable_file_and_keeps_its_bytes() {
        let dir = scratch();
        let path = dir.path().join("installed.json");
        std::fs::write(&path, "{ not json").expect("writes corrupt file");
        let error = InstalledRegistry::open(&path).expect_err("corrupt registry is refused");
        assert_eq!(error.code().0, "install.state-corrupt");
        assert_eq!(
            std::fs::read(&path).expect("reads back"),
            b"{ not json".to_vec()
        );
    }

    /// REQ: LIF-282
    #[test]
    fn installed_open_refuses_wrong_format_string() {
        let dir = scratch();
        let path = dir.path().join("installed.json");
        let text = r#"{"format":"ilium-installed/0","plugins":[],"purge_tombstones":[]}"#;
        std::fs::write(&path, text).expect("writes file");
        let error = InstalledRegistry::open(&path).expect_err("unknown format is refused");
        assert_eq!(error.code().0, "install.state-corrupt");
        assert_eq!(std::fs::read_to_string(&path).expect("reads back"), text);
    }

    /// REQ: LIF-282
    #[test]
    fn installed_open_refuses_plugins_that_are_not_a_list() {
        let dir = scratch();
        let path = dir.path().join("installed.json");
        let text =
            r#"{"format":"ilium-installed/1","plugins":"com.example.notes","purge_tombstones":[]}"#;
        std::fs::write(&path, text).expect("writes file");
        let error = InstalledRegistry::open(&path).expect_err("wrong type is refused");
        assert_eq!(error.code().0, "install.state-corrupt");
    }

    // ---- GrantStore ----------------------------------------------------------------------

    /// REQ: SEC-112
    #[test]
    fn grants_open_new_store_has_no_grants_for_any_plugin() {
        let dir = scratch();
        let path = dir.path().join("grants.json");
        let store = GrantStore::open(&path).expect("opens or creates the store");
        assert!(store
            .granted(&plugin("com.example.notes"))
            .expect("reads")
            .is_empty());
    }

    /// REQ: HST-023
    #[test]
    fn grants_set_then_read_returns_the_same_grants_in_order() {
        let dir = scratch();
        let path = dir.path().join("grants.json");
        let notes = plugin("com.example.notes");
        let mut store = GrantStore::open(&path).expect("opens");
        store
            .set_granted(&notes, &[grant("tree.read"), grant("fs.read:project")])
            .expect("sets grants");
        let granted = store.granted(&notes).expect("reads");
        assert_eq!(grant_texts(&granted), vec!["tree.read", "fs.read:project"]);
    }

    /// REQ: HST-023
    #[test]
    fn grants_set_replaces_the_previous_set() {
        let dir = scratch();
        let path = dir.path().join("grants.json");
        let notes = plugin("com.example.notes");
        let mut store = GrantStore::open(&path).expect("opens");
        store
            .set_granted(&notes, &[grant("tree.read")])
            .expect("first set");
        store
            .set_granted(&notes, &[grant("sound.play")])
            .expect("second set");
        let granted = store.granted(&notes).expect("reads");
        assert_eq!(grant_texts(&granted), vec!["sound.play"]);
    }

    /// REQ: HST-023
    #[test]
    fn grants_set_to_empty_list_clears_the_plugin() {
        let dir = scratch();
        let path = dir.path().join("grants.json");
        let notes = plugin("com.example.notes");
        let mut store = GrantStore::open(&path).expect("opens");
        store
            .set_granted(&notes, &[grant("tree.read")])
            .expect("sets grants");
        store.set_granted(&notes, &[]).expect("clears grants");
        assert!(store.granted(&notes).expect("reads").is_empty());
    }

    /// REQ: SEC-112
    #[test]
    fn grants_of_one_plugin_do_not_leak_into_another() {
        let dir = scratch();
        let path = dir.path().join("grants.json");
        let mut store = GrantStore::open(&path).expect("opens");
        store
            .set_granted(&plugin("com.example.notes"), &[grant("tree.read")])
            .expect("sets notes grants");
        let other = store
            .granted(&plugin("com.example.tasks"))
            .expect("reads other");
        assert!(other.is_empty());
    }

    /// REQ: HST-023
    #[test]
    fn grants_survive_reopen() {
        let dir = scratch();
        let path = dir.path().join("grants.json");
        let notes = plugin("com.example.notes");
        {
            let mut store = GrantStore::open(&path).expect("opens");
            store
                .set_granted(&notes, &[grant("sound.play")])
                .expect("sets grants");
        }
        let reopened = GrantStore::open(&path).expect("reopens");
        let granted = reopened.granted(&notes).expect("reads");
        assert_eq!(grant_texts(&granted), vec!["sound.play"]);
    }

    /// REQ: HST-023
    #[test]
    fn grants_second_writer_is_refused_while_the_first_holds_the_lock() {
        let dir = scratch();
        let path = dir.path().join("grants.json");
        let _first = GrantStore::open(&path).expect("first writer opens");
        assert!(
            GrantStore::open(&path).is_err(),
            "a second writer must not open the store"
        );
    }

    /// REQ: HST-044
    #[test]
    fn grants_unparseable_file_is_reported_as_unreadable() {
        let dir = scratch();
        let path = dir.path().join("grants.json");
        std::fs::write(&path, "not a grant store").expect("writes corrupt file");
        let error = GrantStore::open(&path).expect_err("unreadable store is reported");
        assert_eq!(error.code().0, "host.grants-unreadable");
    }

    /// REQ: SEC-114
    #[test]
    fn grants_newer_store_version_is_refused_and_file_is_untouched() {
        let dir = scratch();
        let path = dir.path().join("grants.json");
        let text = r#"{"version":999,"plugins":{}}"#;
        std::fs::write(&path, text).expect("writes newer file");
        let error = GrantStore::open(&path).expect_err("newer version is refused");
        assert_eq!(error.code().0, "store.version");
        assert_eq!(std::fs::read_to_string(&path).expect("reads back"), text);
    }

    // ---- EnableFlags ---------------------------------------------------------------------

    /// REQ: LIF-018
    #[test]
    fn enable_flags_plugin_without_flag_is_enabled() {
        let dir = scratch();
        let flags = EnableFlags::open(&dir.path().join("config.toml")).expect("opens");
        assert!(flags.is_enabled(&plugin("com.example.notes")));
    }

    /// REQ: LIF-018
    #[test]
    fn enable_flags_disable_then_read_back() {
        let dir = scratch();
        let notes = plugin("com.example.notes");
        let mut flags = EnableFlags::open(&dir.path().join("config.toml")).expect("opens");
        flags.set_enabled(&notes, false).expect("disables");
        assert!(!flags.is_enabled(&notes));
    }

    /// REQ: LIF-018
    #[test]
    fn enable_flags_writes_plugin_table_with_enabled_key() {
        let dir = scratch();
        let path = dir.path().join("config.toml");
        let mut flags = EnableFlags::open(&path).expect("opens");
        flags
            .set_enabled(&plugin("com.example.notes"), false)
            .expect("disables");
        let text = std::fs::read_to_string(&path).expect("reads config");
        let table: toml::Table = toml::from_str(&text).expect("config is valid TOML");
        let enabled = table
            .get("plugins")
            .and_then(|plugins| plugins.get("com.example.notes"))
            .and_then(|plugin_table| plugin_table.get("enabled"))
            .and_then(|value| value.as_bool());
        assert_eq!(enabled, Some(false));
    }

    /// REQ: LIF-019
    #[test]
    fn enable_flags_enable_again_after_disable() {
        let dir = scratch();
        let notes = plugin("com.example.notes");
        let mut flags = EnableFlags::open(&dir.path().join("config.toml")).expect("opens");
        flags.set_enabled(&notes, false).expect("disables");
        flags.set_enabled(&notes, true).expect("enables");
        assert!(flags.is_enabled(&notes));
    }

    /// REQ: LIF-019
    #[test]
    fn enable_flags_setting_the_current_value_is_ok_and_changes_nothing() {
        let dir = scratch();
        let notes = plugin("com.example.notes");
        let mut flags = EnableFlags::open(&dir.path().join("config.toml")).expect("opens");
        flags
            .set_enabled(&notes, true)
            .expect("enabling an enabled plugin is ok");
        assert!(flags.is_enabled(&notes));
        flags.set_enabled(&notes, false).expect("disables");
        flags
            .set_enabled(&notes, false)
            .expect("disabling a disabled plugin is ok");
        assert!(!flags.is_enabled(&notes));
    }

    /// REQ: LIF-018
    #[test]
    fn enable_flags_persist_across_reopen() {
        let dir = scratch();
        let path = dir.path().join("config.toml");
        let notes = plugin("com.example.notes");
        {
            let mut flags = EnableFlags::open(&path).expect("opens");
            flags.set_enabled(&notes, false).expect("disables");
        }
        let reopened = EnableFlags::open(&path).expect("reopens");
        assert!(!reopened.is_enabled(&notes));
    }

    /// REQ: LIF-018
    #[test]
    fn enable_flags_change_for_one_plugin_leaves_others_enabled() {
        let dir = scratch();
        let mut flags = EnableFlags::open(&dir.path().join("config.toml")).expect("opens");
        flags
            .set_enabled(&plugin("com.example.notes"), false)
            .expect("disables notes");
        assert!(flags.is_enabled(&plugin("com.example.tasks")));
    }

    // ---- downloads_dir and registry_cache_dir --------------------------------------------

    /// REQ: PKG-267
    #[test]
    fn downloads_dir_is_plugins_downloads_under_cache() {
        let cache = Path::new("/home/user/.cache");
        assert_eq!(
            downloads_dir(cache),
            PathBuf::from("/home/user/.cache/ilium/plugins/downloads")
        );
    }

    /// REQ: PKG-267
    #[test]
    fn registry_cache_dir_is_one_folder_under_plugins_registry() {
        let cache = Path::new("/home/user/.cache");
        let folder = registry_cache_dir(cache, "https://plugins.example.org/ilium/v1/");
        assert_eq!(
            folder.parent(),
            Some(Path::new("/home/user/.cache/ilium/plugins/registry"))
        );
    }

    /// REQ: PKG-267
    #[test]
    fn registry_cache_dir_is_stable_for_the_same_url() {
        let cache = Path::new("/home/user/.cache");
        let url = "https://plugins.example.org/ilium/v1/";
        assert_eq!(
            registry_cache_dir(cache, url),
            registry_cache_dir(cache, url)
        );
    }

    /// REQ: PKG-267
    #[test]
    fn registry_cache_dir_differs_between_urls() {
        let cache = Path::new("/home/user/.cache");
        let first = registry_cache_dir(cache, "https://plugins.example.org/ilium/v1/");
        let second = registry_cache_dir(cache, "https://mirror.example.net/ilium/v1/");
        assert_ne!(first, second);
    }

    /// REQ: PKG-267
    #[test]
    fn registry_cache_dir_accepts_empty_url_without_panicking() {
        let cache = Path::new("/home/user/.cache");
        let folder = registry_cache_dir(cache, "");
        assert_eq!(
            folder.parent(),
            Some(Path::new("/home/user/.cache/ilium/plugins/registry"))
        );
    }
}
