//! Stored plugin settings in the runtime: the import a plugin gets, the values it is activated
//! with and the `settings-changed` notices its writes leave behind (manual: `api-settings.md`).
//!
//! The runtime owns one [`SettingsDelivery`]. It builds a [`StoredSettingsImport`] per plugin
//! instance, hands the plugin its stored values at activation (`settings-json`) and collects
//! one [`SettingsNotice`] per accepted write that changed a value. The pump turns each notice
//! into one `settings-changed` event at the plugin that owns the keys.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::Value;

use crate::config_settings::{schema_defaults, PluginSettingsStore};
use crate::imports::settings::{
    PeerSchemas, SettingsChangeSink, SettingsNotice, SharedRowOverlays, StoredSettingsImport,
};
use crate::settings::{manifest_row_names, schema_for_side};

/// The settings store, the notices of its writes and the live row overlays of every plugin.
pub(crate) struct SettingsDelivery {
    store: Arc<PluginSettingsStore>,
    notices: Arc<Mutex<Vec<SettingsNotice>>>,
    overlays: Mutex<BTreeMap<String, SharedRowOverlays>>,
}

impl SettingsDelivery {
    /// A delivery over the user's `config.toml`, or over a given store in tests.
    pub(crate) fn new(store: Arc<PluginSettingsStore>) -> Self {
        Self {
            store,
            notices: Arc::default(),
            overlays: Mutex::default(),
        }
    }

    /// The store the imports write to.
    pub(crate) fn store(&self) -> Arc<PluginSettingsStore> {
        Arc::clone(&self.store)
    }

    /// The import of one plugin instance for the side `side` ("client" or "server").
    ///
    /// `peers` are the schemas of the other installed plugins, `read_all` and `write_all` the
    /// `settings.read-all` and `settings.write-all` grants of this plugin.
    pub(crate) fn import(
        &self,
        plugin: &str,
        manifest: &Value,
        side: &str,
        peers: PeerSchemas,
        read_all: bool,
        write_all: bool,
    ) -> StoredSettingsImport {
        let notices = Arc::clone(&self.notices);
        let sink: SettingsChangeSink = Arc::new(move |notice| lock(&notices).push(notice));
        let overlays = Arc::clone(lock(&self.overlays).entry(plugin.to_owned()).or_default());
        StoredSettingsImport::new(
            plugin,
            schema_for_side(manifest, side).to_string(),
            manifest_row_names(manifest),
            self.store(),
            sink,
        )
        .with_overlays(overlays)
        .with_peers(peers, read_all, write_all)
    }

    /// The `settings-json` of an activation: the defaults of the side's schema with the stored
    /// values over them.
    pub(crate) fn activation_json(&self, plugin: &str, manifest: &Value, side: &str) -> String {
        let schema = schema_for_side(manifest, side).to_string();
        let mut values = schema_defaults(&schema);
        // An unreadable file leaves the defaults: the plugin still starts.
        if let Ok(stored) = self.store.read(plugin) {
            values.extend(stored);
        }
        Value::Object(values).to_string()
    }

    /// The notices queued since the last call, oldest first.
    pub(crate) fn take_notices(&self) -> Vec<SettingsNotice> {
        std::mem::take(&mut *lock(&self.notices))
    }

    /// The live row overlays of a plugin, shared with the client that draws its settings.
    pub(crate) fn overlays_of(&self, plugin: &str) -> SharedRowOverlays {
        Arc::clone(lock(&self.overlays).entry(plugin.to_owned()).or_default())
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::SettingsDelivery;
    use crate::config_settings::PluginSettingsStore;
    use crate::imports::settings::PeerSchemas;
    use ilium_plugin_api::generated::host::settings::HostSettings;
    use ilium_plugin_api::generated::types::events::SettingsOrigin;
    use serde_json::{json, Value};
    use std::sync::Arc;

    fn manifest() -> Value {
        json!({"contributes": {"settings": [{
            "kind": "tab", "id": "main", "title": "Demo",
            "sections": [{"title": "Sound", "rows": [
                {"type": "number", "key": "volume", "title": "Volume",
                 "minimum": 0, "maximum": 10, "default": 5},
                {"type": "info", "id": "status", "title": "Status"},
                {"type": "group", "rows": [
                    {"type": "toggle", "key": "muted", "title": "Muted", "default": false}]}
            ]}, {"title": "Daemon", "scope": "server", "rows": [
                {"type": "toggle", "key": "daemon", "title": "Daemon", "default": true}]}]
        }]}})
    }

    fn delivery() -> (SettingsDelivery, tempfile::TempDir) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let store = PluginSettingsStore::at(directory.path().join("config.toml"));
        (SettingsDelivery::new(Arc::new(store)), directory)
    }

    #[test]
    fn activation_carries_the_defaults_and_the_stored_values_of_the_side() {
        let (delivery, _directory) = delivery();
        let first: Value =
            serde_json::from_str(&delivery.activation_json("acme.demo", &manifest(), "client"))
                .expect("JSON");
        assert_eq!(first, json!({"volume": 5, "muted": false}));
        delivery
            .import(
                "acme.demo",
                &manifest(),
                "client",
                PeerSchemas::new(),
                false,
                false,
            )
            .set_json(r#"{"volume":9}"#.to_owned())
            .expect("stored");
        let second: Value =
            serde_json::from_str(&delivery.activation_json("acme.demo", &manifest(), "client"))
                .expect("JSON");
        assert_eq!(second, json!({"volume": 9, "muted": false}));
        let server: Value =
            serde_json::from_str(&delivery.activation_json("acme.demo", &manifest(), "server"))
                .expect("JSON");
        assert_eq!(server, json!({"daemon": true}));
    }

    #[test]
    fn one_write_leaves_exactly_one_notice() {
        let (delivery, _directory) = delivery();
        let mut import = delivery.import(
            "acme.demo",
            &manifest(),
            "client",
            PeerSchemas::new(),
            false,
            false,
        );
        import
            .set_json(r#"{"volume":9,"muted":true}"#.to_owned())
            .expect("stored");
        import
            .set_json(r#"{"volume":9}"#.to_owned())
            .expect("unchanged");
        let notices = delivery.take_notices();
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].origin, SettingsOrigin::Plugin);
        assert!(delivery.take_notices().is_empty());
    }

    #[test]
    fn a_key_of_the_other_side_is_refused() {
        let (delivery, _directory) = delivery();
        let mut import = delivery.import(
            "acme.demo",
            &manifest(),
            "client",
            PeerSchemas::new(),
            false,
            false,
        );
        let failure = import
            .set_json(r#"{"daemon":false}"#.to_owned())
            .expect_err("owned by the server side");
        assert_eq!(
            failure.code,
            ilium_plugin_api::generated::types::errors::ErrorCode::InvalidArgument
        );
        assert!(delivery.take_notices().is_empty());
    }

    #[test]
    fn declared_row_names_include_info_ids_and_group_children() {
        let (delivery, _directory) = delivery();
        let mut import = delivery.import(
            "acme.demo",
            &manifest(),
            "client",
            PeerSchemas::new(),
            false,
            false,
        );
        import
            .set_info("status".to_owned(), "ok".to_owned())
            .expect("info id is declared");
        import
            .set_row_note("muted".to_owned(), Some("n".to_owned()))
            .expect("group child is declared");
        assert!(delivery
            .overlays_of("acme.demo")
            .lock()
            .expect("lock")
            .contains_key("status"));
    }
}
