//! What `ilium-server` and `ilium-client` share when they compose the real plugin host
//! (foundation contract sections 2.2, 2.4, 5 and 12): where the installed registry and the
//! plugin storage live, the import provider that backs `ilium:host/storage` and
//! `ilium:primitives/basic`, the list rows of the plugin list, and the state names that travel
//! over IPC.
//!
//! Nothing here calls a guest. The processes own the runtime thread and their kernel facade.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ilium_plugin_api::generated::host::log::Level;
use ilium_plugin_api::generated::primitives::basic::HostBasic;
use ilium_plugin_api::generated::types::{
    errors::PluginError, ids::NodeId, text::StyledText, tree::AgentActivity,
};
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::side::Side;
use serde_json::Value;

use crate::imports::backed::LogSink;
use crate::imports::basic::{BasicBackend, BasicChange};
use crate::imports::storage::{
    StorageAuthorization, StorageImport, StorageImportContext, StorageInstanceState,
};
use crate::registry;
use crate::runtime::{
    CataloguePlugin, ImportProvider, InstanceContext, PluginState, ESSENTIAL_PLUGINS,
};
use crate::storage::{StorageContext, BUCKET_MAX_BYTES};

/// The shared cache ceiling of one plugin (reference-errors-and-limits.md, storage).
const SHARED_CACHE_BYTES: usize = 64 * 1024 * 1024;

/// The environment variable that starts only core plugins.
pub const SAFE_PLUGINS_ENV: &str = "ILIUM_SAFE_PLUGINS";
/// The environment variable that starts no plugins. The `ilium` launcher sets it on the
/// server it spawns when the user passed `--no-plugins`.
pub const NO_PLUGINS_ENV: &str = "ILIUM_NO_PLUGINS";

/// The start mode the environment asks for. A variable counts as set when it is non-empty
/// and not `0`; no plugins beats safe plugins.
pub fn start_mode_from_env() -> crate::lifecycle::StartMode {
    let is_set = |name: &str| std::env::var(name).is_ok_and(|text| !text.is_empty() && text != "0");
    if is_set(NO_PLUGINS_ENV) {
        crate::lifecycle::StartMode::NoPlugins
    } else if is_set(SAFE_PLUGINS_ENV) {
        crate::lifecycle::StartMode::SafePlugins
    } else {
        crate::lifecycle::StartMode::Normal
    }
}

/// The catalogue rows of one side from `(id, manifest, component)` triples, which is how the
/// bundle crate hands its embedded plugins over (the host must not depend on the bundle).
pub fn catalogue<'a>(
    plugins: impl IntoIterator<Item = (&'a str, &'a str, Option<&'static [u8]>)>,
) -> Vec<CataloguePlugin> {
    plugins
        .into_iter()
        .map(|(id, manifest, component)| CataloguePlugin {
            id: id.to_owned(),
            manifest: manifest.to_owned(),
            component: component.map(std::borrow::Cow::Borrowed),
        })
        .collect()
}

/// `<data dir>/plugins/installed.json`, the one store of installed and enabled state.
pub fn installed_path(data_dir: &Path) -> PathBuf {
    data_dir.join("plugins").join("installed.json")
}

/// `<data dir>/plugins/storage`, the root of every plugin's private storage engine.
pub fn storage_root(data_dir: &Path) -> PathBuf {
    data_dir.join("plugins").join("storage")
}

/// The ids the installed registry marks as not startable. An unreadable registry disables
/// nothing and is reported through the returned message, so a damaged file never stops the
/// core plugins from starting.
pub fn disabled_at_start(data_dir: &Path) -> (BTreeSet<String>, Option<String>) {
    match registry::disabled_plugin_ids(&installed_path(data_dir)) {
        Ok(disabled) => (disabled, None),
        Err(error) => (BTreeSet::new(), Some(error.to_string())),
    }
}

/// The `version` of a manifest, or `0.0.0` when the text has none.
pub fn manifest_version(manifest: &str) -> String {
    serde_json::from_str::<Value>(manifest)
        .ok()
        .and_then(|value| {
            value
                .get("version")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "0.0.0".to_owned())
}

/// The name of a runtime state as it travels in `ControlBody::PluginState` and as the Settings
/// plugin list and `ilium plugin list` print it.
pub fn state_name(state: &PluginState) -> String {
    match state {
        PluginState::Loaded => "loaded".to_owned(),
        PluginState::Active => "running".to_owned(),
        PluginState::Disabled => "disabled".to_owned(),
        PluginState::ManifestOnly => "manifest-only".to_owned(),
        PluginState::Failed(code) => format!("failed:{code}"),
        PluginState::Stopped => "stopped".to_owned(),
    }
}

/// Whether a state name means the user turned the plugin off.
pub fn is_disabled_state(state: &str) -> bool {
    state == "disabled" || state == registry::STATE_DISABLED_BY_USER
}

/// One row of the plugin list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRow {
    /// The plugin id.
    pub id: String,
    /// The manifest version.
    pub version: String,
    /// The manifest name, or the id when the manifest has none.
    pub title: String,
    /// The state name (`state_name`), or the registry state when no runtime observed it.
    pub state: String,
    /// Whether the plugin is one of the four that cannot be disabled.
    pub essential: bool,
}

/// The rows of a catalogue with the state each plugin has: the observed runtime state when
/// `observed` has one, else `disabled` for a disabled id, else `not-running`.
pub fn plugin_rows(
    catalogue: &[CataloguePlugin],
    disabled: &BTreeSet<String>,
    observed: &BTreeMap<String, String>,
) -> Vec<PluginRow> {
    let mut rows: Vec<PluginRow> = catalogue
        .iter()
        .map(|plugin| {
            let manifest: Value = serde_json::from_str(&plugin.manifest).unwrap_or(Value::Null);
            let text = |key: &str| manifest.get(key).and_then(Value::as_str).map(str::to_owned);
            let state = match observed.get(&plugin.id) {
                Some(state) => state.clone(),
                None if disabled.contains(&plugin.id) => "disabled".to_owned(),
                None => "not-running".to_owned(),
            };
            PluginRow {
                id: plugin.id.clone(),
                version: text("version").unwrap_or_else(|| "0.0.0".to_owned()),
                title: text("name").unwrap_or_else(|| plugin.id.clone()),
                state,
                essential: ESSENTIAL_PLUGINS.contains(&plugin.id.as_str()),
            }
        })
        .collect();
    rows.sort_by(|left, right| left.id.cmp(&right.id));
    rows
}

/// The wire bytes of one contribution entry (`ContributionOp::Put::entry` of `ilium-ipc`): a
/// JSON object with the entry's fields. The host crate does not know the IPC types, so each
/// process maps its registry operations with this pair.
pub fn encode_entry(entry: &crate::contributions::Entry) -> Vec<u8> {
    serde_json::json!({
        "plugin": entry.plugin,
        "kind": entry.kind,
        "name": entry.name,
        "side": entry.side.map(Side::as_str),
        "payload": entry.payload,
        "order": entry.order,
    })
    .to_string()
    .into_bytes()
}

/// The entry of `encode_entry` bytes; `None` when the bytes are not such an object.
pub fn decode_entry(bytes: &[u8]) -> Option<crate::contributions::Entry> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    Some(crate::contributions::Entry {
        plugin: value.get("plugin")?.as_str()?.to_owned(),
        kind: value.get("kind")?.as_str()?.to_owned(),
        name: value.get("name")?.as_str()?.to_owned(),
        side: value
            .get("side")
            .and_then(Value::as_str)
            .and_then(|side| side.parse().ok()),
        payload: value.get("payload").cloned().unwrap_or(Value::Null),
        order: value.get("order").and_then(Value::as_u64).unwrap_or(0),
    })
}

/// What a process hands the import provider.
#[derive(Clone)]
pub struct ProcessImports {
    /// The root of the plugin storage engines (`storage_root`).
    pub storage_root: PathBuf,
    /// The project and session the storage buckets are scoped to.
    pub scope: StorageContext,
    /// The basic-pane backend of this process, when it has one (the server).
    pub basic: Option<Arc<dyn BasicBackend>>,
    /// Where storage refusals are logged.
    pub log: Arc<dyn LogSink>,
}

impl std::fmt::Debug for ProcessImports {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProcessImports")
            .field("storage_root", &self.storage_root)
            .field("has_basic", &self.basic.is_some())
            .finish_non_exhaustive()
    }
}

/// `ilium:primitives/basic` over the process backend. The runtime removes the interface again
/// for an instance without the capability, so this adapter does not repeat the grant check.
struct BackendBasic {
    backend: Arc<dyn BasicBackend>,
    plugin: String,
}

impl HostBasic for BackendBasic {
    fn set_title(&mut self, pane: NodeId, title: String) -> Result<(), PluginError> {
        self.backend
            .apply_basic(&self.plugin, pane, BasicChange::Title(title))
    }

    fn set_status(&mut self, pane: NodeId, status: StyledText) -> Result<(), PluginError> {
        self.backend
            .apply_basic(&self.plugin, pane, BasicChange::Status(status))
    }

    fn request_focus(&mut self, pane: NodeId) -> Result<(), PluginError> {
        self.backend
            .apply_basic(&self.plugin, pane, BasicChange::Focus)
    }

    fn size(&mut self, pane: NodeId) -> Option<(u16, u16)> {
        self.backend.basic_size(&self.plugin, pane)
    }

    fn report_activity(
        &mut self,
        pane: NodeId,
        activity: AgentActivity,
    ) -> Result<(), PluginError> {
        self.backend
            .apply_basic(&self.plugin, pane, BasicChange::Activity(activity))
    }

    fn set_dirty(&mut self, pane: NodeId, dirty: bool) -> Result<(), PluginError> {
        self.backend
            .apply_basic(&self.plugin, pane, BasicChange::Dirty(dirty))
    }
}

/// The provider both processes give `RuntimeConfig::provider`: it opens the private storage
/// engine of the new instance and, when the process has a basic-pane backend, links
/// `ilium:primitives/basic`. A storage engine that cannot be opened is logged and left out,
/// so the instance still starts and a storage call reports `host.engine-error`.
pub fn import_provider(imports: ProcessImports) -> ImportProvider {
    Box::new(move |context: &InstanceContext, set| {
        let plugin = PluginId(context.plugin.clone());
        let side: Side = context.side;
        let authorization = Arc::new(Mutex::new(StorageAuthorization::new(
            plugin.clone(),
            side,
            StorageInstanceState::Active,
        )));
        let diagnostic_log = Arc::clone(&imports.log);
        let diagnostic_plugin = context.plugin.clone();
        let opened = StorageImport::open(
            &imports.storage_root,
            StorageImportContext {
                plugin,
                side,
                scope: imports.scope.clone(),
                tier: context.tier,
                bucket_limit_bytes: BUCKET_MAX_BYTES,
                shared_limit_bytes: SHARED_CACHE_BYTES,
            },
            authorization,
            move |operation, error| {
                diagnostic_log.line(
                    &diagnostic_plugin,
                    side,
                    Level::Warn,
                    &format!("storage {operation:?} refused: {}", error.message),
                );
            },
        );
        match opened {
            Ok(storage) => set.storage = Some(Box::new(storage)),
            Err(error) => imports.log.line(
                &context.plugin,
                side,
                Level::Error,
                &format!("storage could not be opened: {}", error.message),
            ),
        }
        if let Some(backend) = &imports.basic {
            set.basic = Some(Box::new(BackendBasic {
                backend: Arc::clone(backend),
                plugin: context.plugin.clone(),
            }));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, manifest: &str) -> CataloguePlugin {
        CataloguePlugin {
            id: id.to_owned(),
            manifest: manifest.to_owned(),
            component: None,
        }
    }

    #[test]
    fn rows_prefer_the_observed_state_then_the_disabled_flag() {
        let catalogue = vec![
            entry("ilium.core.tree", r#"{"name":"Tree","version":"0.1.0"}"#),
            entry("ilium.core.voice", r#"{"version":"0.2.0"}"#),
            entry("ilium.core.cost", "{}"),
        ];
        let disabled = BTreeSet::from(["ilium.core.voice".to_owned()]);
        let observed = BTreeMap::from([("ilium.core.tree".to_owned(), "running".to_owned())]);
        let rows = plugin_rows(&catalogue, &disabled, &observed);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].id, "ilium.core.cost");
        assert_eq!(rows[0].state, "not-running");
        assert_eq!(rows[0].version, "0.0.0");
        assert_eq!(rows[1].title, "Tree");
        assert_eq!(rows[1].state, "running");
        assert!(rows[1].essential);
        assert_eq!(rows[2].state, "disabled");
        assert!(!rows[2].essential);
    }

    #[test]
    fn a_failed_state_carries_its_trap_code() {
        assert_eq!(
            state_name(&PluginState::Failed("trap.abi".to_owned())),
            "failed:trap.abi"
        );
        assert!(is_disabled_state(&state_name(&PluginState::Disabled)));
        assert!(!is_disabled_state("running"));
    }

    #[test]
    fn enable_state_round_trips_through_installed_json() {
        let directory = tempfile::tempdir().expect("tempdir");
        let (disabled, problem) = disabled_at_start(directory.path());
        assert!(disabled.is_empty());
        assert!(problem.is_none());
        let installed = installed_path(directory.path());
        let changed = registry::set_plugin_enabled(&installed, "ilium.core.voice", "0.1.0", false)
            .expect("disable");
        assert!(changed);
        let (disabled, _) = disabled_at_start(directory.path());
        assert!(disabled.contains("ilium.core.voice"));
        let again = registry::set_plugin_enabled(&installed, "ilium.core.voice", "0.1.0", false)
            .expect("disable twice");
        assert!(!again);
        registry::set_plugin_enabled(&installed, "ilium.core.voice", "0.1.0", true)
            .expect("enable");
        let (disabled, _) = disabled_at_start(directory.path());
        assert!(disabled.is_empty());
    }

    #[test]
    fn a_contribution_entry_round_trips_through_its_wire_bytes() {
        let entry = crate::contributions::Entry {
            plugin: "ilium.core.layout".to_owned(),
            kind: "commands".to_owned(),
            name: "split".to_owned(),
            side: Some(Side::Server),
            payload: serde_json::json!({"title": "Split"}),
            order: 7,
        };
        assert_eq!(decode_entry(&encode_entry(&entry)), Some(entry));
        assert_eq!(decode_entry(b"not json"), None);
    }
}
