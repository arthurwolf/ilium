//! `ilium:host/extensions` of the running host (foundation contract section 20.5).
//!
//! A point exists while its owner is enabled: it is the owner's `extension_points` entry in
//! the contribution registry, so a disabled owner takes its points with it. Entries that
//! plugins register at run time live in one book the runtime shares between its plugins.
//! Each plugin gets its own import, which knows the plugin and its grants.
//!
//! `run-hook` validates the call and completes it with no handler reply, as the plain import
//! does: handler chains need an export the plugin world does not have yet.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};

use ilium_plugin_api::generated::host::extensions::{
    ExtensionEntry, ExtensionError, HostExtensions,
};
use ilium_plugin_api::generated::types::ids::{PluginId, RequestId};
use serde_json::Value;

use crate::contributions::ContributionRegistry;

/// Largest payload of a hook chain in bytes (EXT-023).
const PAYLOAD_CAP_BYTES: usize = 64 * 1024;
/// Largest contributor entry in bytes (EXT-023).
const ENTRY_CAP_BYTES: usize = 16 * 1024;
/// Highest priority an entry may carry (EXT-007).
const PRIORITY_MAX: u8 = 100;
/// The priority of a manifest entry that names none.
const PRIORITY_DEFAULT: u8 = 50;
/// Entries a point holds when its manifest names no `max_entries`.
const ENTRIES_DEFAULT_MAX: usize = 64;
/// Hook requests of one plugin whose reply was not taken yet.
const PENDING_REPLIES_MAX: usize = 256;

/// What plugins registered at run time, for every point.
#[derive(Debug, Default)]
pub struct BookState {
    /// Entries by global point name `<owner>/<point>`.
    entries: BTreeMap<String, Vec<ExtensionEntry>>,
    next_request: u64,
    /// Completed hook requests by (calling plugin, request id).
    replies: HashMap<(String, u64), Option<String>>,
}

/// The book of run-time extension entries, shared by the plugins of one runtime.
#[derive(Debug, Clone, Default)]
pub struct ExtensionBook(Arc<Mutex<BookState>>);

impl ExtensionBook {
    fn state(&self) -> MutexGuard<'_, BookState> {
        // Every writer does one insert or remove, so the data is consistent after a panic.
        self.0.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    /// Drops what a plugin registered, the entries of the points it owns and its pending
    /// hook replies. Called when the plugin is disabled: its points are gone with it.
    pub fn forget_plugin(&self, plugin: &str) {
        let owned = format!("{plugin}/");
        self.state()
            .entries
            .retain(|point, _| !point.starts_with(&owned));
        self.forget_instance(plugin);
    }

    /// Drops what one instance of a plugin registered and its pending hook replies. Called
    /// when the plugin fails or gets a new instance; the points it owns keep the entries
    /// other plugins registered.
    pub fn forget_instance(&self, plugin: &str) {
        let mut state = self.state();
        for entries in state.entries.values_mut() {
            entries.retain(|entry| entry.contributor.0 != plugin);
        }
        state.replies.retain(|(caller, _), _| caller != plugin);
    }

    /// The entries of a point in run order: priority, contributor, extension id.
    pub fn entries_of(&self, point: &str) -> Vec<ExtensionEntry> {
        let mut entries = self.state().entries.get(point).cloned().unwrap_or_default();
        entries.sort_by(|left, right| {
            (left.priority, &left.contributor.0, &left.extension).cmp(&(
                right.priority,
                &right.contributor.0,
                &right.extension,
            ))
        });
        entries
    }
}

/// What the manifest of the owner says about one point.
struct Declared {
    dynamic: bool,
    max_entries: usize,
}

/// `ilium:host/extensions` of one plugin.
pub struct PluginExtensions {
    plugin: String,
    granted: Vec<String>,
    registry: Arc<Mutex<ContributionRegistry>>,
    book: ExtensionBook,
}

impl PluginExtensions {
    /// The import of one plugin instance over the runtime's registry and book.
    pub fn new(
        plugin: &str,
        granted: &[String],
        registry: Arc<Mutex<ContributionRegistry>>,
        book: ExtensionBook,
    ) -> Self {
        Self {
            plugin: plugin.to_owned(),
            granted: granted.to_vec(),
            registry,
            book,
        }
    }

    fn declared(&self, point: &str) -> Result<Declared, ExtensionError> {
        let registry = self
            .registry
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let entry = registry
            .find_global("extension_points", point)
            .ok_or(ExtensionError::UnknownPoint)?;
        Ok(Declared {
            dynamic: entry
                .payload
                .get("dynamic")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            max_entries: entry
                .payload
                .get("max_entries")
                .and_then(Value::as_u64)
                .and_then(|max| usize::try_from(max).ok())
                .unwrap_or(ENTRIES_DEFAULT_MAX),
        })
    }

    /// The entries enabled plugins declare for a point in their manifests.
    fn manifest_entries(&self, point: &str) -> Vec<ExtensionEntry> {
        let registry = self
            .registry
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        registry
            .entries("extensions")
            .into_iter()
            .filter(|entry| entry.payload.get("point").and_then(Value::as_str) == Some(point))
            .filter_map(|entry| {
                let payload = entry.payload.get("entry")?;
                Some(ExtensionEntry {
                    extension: entry.name.clone(),
                    contributor: PluginId(entry.plugin.clone()),
                    priority: entry
                        .payload
                        .get("priority")
                        .and_then(Value::as_u64)
                        .and_then(|priority| u8::try_from(priority).ok())
                        .unwrap_or(PRIORITY_DEFAULT),
                    entry_json: payload.to_string(),
                })
            })
            .collect()
    }

    fn owns(&self, point: &str) -> bool {
        point
            .split_once('/')
            .is_some_and(|(owner, _)| owner == self.plugin)
    }

    fn holds(&self, family: &str, point: &str) -> bool {
        let wanted = format!("{family}:{point}");
        self.granted.iter().any(|grant| *grant == wanted)
    }

    /// The owner reads and runs its point; `extensions.provide` names the same right.
    fn provides(&self, point: &str) -> bool {
        self.owns(point) || self.holds("extensions.provide", point)
    }
}

impl HostExtensions for PluginExtensions {
    fn list(&mut self, point: String) -> Result<Vec<ExtensionEntry>, ExtensionError> {
        self.declared(&point)?;
        if !self.provides(&point) && !self.holds("extensions.contribute", &point) {
            return Err(ExtensionError::NotGranted);
        }
        // Entries come from two places: the `contributes.extensions` lists of the enabled
        // plugins' manifests, and what plugins registered at run time.
        let mut entries = self.manifest_entries(&point);
        entries.extend(self.book.entries_of(&point));
        entries.sort_by(|left, right| {
            (left.priority, &left.contributor.0, &left.extension).cmp(&(
                right.priority,
                &right.contributor.0,
                &right.extension,
            ))
        });
        Ok(entries)
    }

    fn run_hook(
        &mut self,
        point: String,
        payload_json: String,
    ) -> Result<RequestId, ExtensionError> {
        if payload_json.len() > PAYLOAD_CAP_BYTES {
            return Err(ExtensionError::CapExceeded);
        }
        self.declared(&point)?;
        if !self.provides(&point) {
            return Err(ExtensionError::NotGranted);
        }
        if serde_json::from_str::<Value>(&payload_json).is_err() {
            return Err(ExtensionError::SchemaMismatch);
        }
        let mut state = self.book.state();
        let pending = state
            .replies
            .keys()
            .filter(|(caller, _)| *caller == self.plugin)
            .count();
        if pending >= PENDING_REPLIES_MAX {
            return Err(ExtensionError::CapExceeded);
        }
        state.next_request += 1;
        let request = state.next_request;
        // No handler chain runs yet: the request completes at once without a reply.
        state.replies.insert((self.plugin.clone(), request), None);
        Ok(RequestId(request))
    }

    fn take_reply(&mut self, request: RequestId) -> Option<String> {
        self.book
            .state()
            .replies
            .remove(&(self.plugin.clone(), request.0))
            .flatten()
    }

    fn register(
        &mut self,
        point: String,
        extension: String,
        priority: u8,
        entry_json: String,
    ) -> Result<(), ExtensionError> {
        if entry_json.len() > ENTRY_CAP_BYTES {
            return Err(ExtensionError::CapExceeded);
        }
        if priority > PRIORITY_MAX {
            return Err(ExtensionError::SchemaMismatch);
        }
        let declared = self.declared(&point)?;
        if !self.owns(&point) && !self.holds("extensions.contribute", &point) {
            return Err(ExtensionError::NotGranted);
        }
        if !declared.dynamic {
            return Err(ExtensionError::NotDynamic);
        }
        if extension.is_empty() || serde_json::from_str::<Value>(&entry_json).is_err() {
            return Err(ExtensionError::SchemaMismatch);
        }
        let contributor = PluginId(self.plugin.clone());
        let entry = ExtensionEntry {
            extension: extension.clone(),
            contributor: contributor.clone(),
            priority,
            entry_json,
        };
        let mut state = self.book.state();
        let entries = state.entries.entry(point).or_default();
        match entries
            .iter_mut()
            .find(|held| held.extension == extension && held.contributor == contributor)
        {
            Some(held) => *held = entry,
            None => {
                if entries.len() >= declared.max_entries {
                    return Err(ExtensionError::CapExceeded);
                }
                entries.push(entry);
            }
        }
        Ok(())
    }

    fn unregister(&mut self, point: String, extension: String) -> Result<(), ExtensionError> {
        let declared = self.declared(&point)?;
        if !declared.dynamic {
            return Err(ExtensionError::NotDynamic);
        }
        let mut state = self.book.state();
        let entries = state
            .entries
            .get_mut(&point)
            .ok_or(ExtensionError::NotFound)?;
        let position = entries
            .iter()
            .position(|held| held.extension == extension && held.contributor.0 == self.plugin)
            .ok_or(ExtensionError::NotFound)?;
        entries.remove(position);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ExtensionBook, PluginExtensions};
    use crate::contributions::ContributionRegistry;
    use ilium_plugin_api::generated::host::extensions::{ExtensionError, HostExtensions};
    use std::sync::{Arc, Mutex};

    const OWNER: &str = "ilium.core.search";
    const POINT: &str = "ilium.core.search/sources";

    fn registry() -> Arc<Mutex<ContributionRegistry>> {
        let mut registry = ContributionRegistry::new();
        let manifest = serde_json::json!({
            "id": OWNER,
            "contributes": {"extension_points": [
                {"id": "sources", "kind": "collect", "dynamic": true, "max_entries": 1},
                {"id": "fixed", "kind": "collect", "dynamic": false}
            ]}
        });
        registry
            .load_manifest(OWNER, &manifest)
            .expect("the manifest loads");
        Arc::new(Mutex::new(registry))
    }

    fn import(plugin: &str, grants: &[&str], book: &ExtensionBook) -> PluginExtensions {
        let granted: Vec<String> = grants.iter().map(|grant| (*grant).to_owned()).collect();
        PluginExtensions::new(plugin, &granted, registry(), book.clone())
    }

    #[test]
    fn a_contributor_registers_and_the_owner_lists_the_entry() {
        let book = ExtensionBook::default();
        let mut contributor = import(
            "com.example.notes",
            &["extensions.contribute:ilium.core.search/sources"],
            &book,
        );
        contributor
            .register(POINT.to_owned(), "notes".to_owned(), 50, "{}".to_owned())
            .expect("registered");
        let mut owner = import(OWNER, &[], &book);
        let entries = owner.list(POINT.to_owned()).expect("listed");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].contributor.0, "com.example.notes");

        book.forget_plugin("com.example.notes");
        assert!(owner.list(POINT.to_owned()).expect("listed").is_empty());
    }

    #[test]
    fn grants_caps_and_static_points_are_enforced() {
        let book = ExtensionBook::default();
        let mut stranger = import("com.example.other", &[], &book);
        assert_eq!(
            stranger.register(POINT.to_owned(), "x".to_owned(), 1, "{}".to_owned()),
            Err(ExtensionError::NotGranted)
        );
        assert_eq!(
            stranger.list("ilium.core.search/none".to_owned()),
            Err(ExtensionError::UnknownPoint)
        );
        let mut owner = import(OWNER, &[], &book);
        assert_eq!(
            owner.register(
                "ilium.core.search/fixed".to_owned(),
                "x".to_owned(),
                1,
                "{}".to_owned()
            ),
            Err(ExtensionError::NotDynamic)
        );
        owner
            .register(POINT.to_owned(), "one".to_owned(), 1, "{}".to_owned())
            .expect("the first entry fits");
        assert_eq!(
            owner.register(POINT.to_owned(), "two".to_owned(), 1, "{}".to_owned()),
            Err(ExtensionError::CapExceeded)
        );
    }
}
