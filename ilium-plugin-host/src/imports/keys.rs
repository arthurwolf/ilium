//! Host implementation of `ilium:host/keys`.
//!
//! Grabs live in one [`KeyGrabs`] table per runtime (a session), capped at 32 grabs across all of
//! its plugins. Every grab remembers its owner, so the runtime frees a plugin's grabs when the
//! plugin stops. Nothing is process-wide: two runtimes in one process have two tables.

use ilium_plugin_api::generated::host::keys::{GrabSpec, HostKeys};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::GrabId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Session-wide cap on key grabs (api-host, API-070).
const SESSION_KEY_GRAB_CAP: usize = 32;

/// Chords the kernel keeps for itself; plugins cannot grab them.
const RESERVED_CHORDS: [&str; 2] = ["ctrl+shift+escape", "ctrl+alt+escape"];

#[derive(Default)]
struct GrabTable {
    next_id: u64,
    /// Grab id to (owning plugin, chords).
    grabs: HashMap<u64, (String, Vec<String>)>,
}

/// The key grabs of one runtime, shared by the imports of its plugins. Cloning shares the table.
#[derive(Clone, Default)]
pub struct KeyGrabs {
    table: Arc<Mutex<GrabTable>>,
}

impl std::fmt::Debug for KeyGrabs {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KeyGrabs")
            .field("held", &self.held())
            .finish()
    }
}

impl KeyGrabs {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, GrabTable> {
        // Every writer does one insert or remove, so the table is consistent after a panic.
        self.table.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// How many grabs are held in all.
    pub fn held(&self) -> usize {
        self.lock().grabs.len()
    }

    /// Frees every grab of a plugin that stopped.
    pub fn forget_plugin(&self, plugin: &str) {
        self.lock().grabs.retain(|_, (owner, _)| owner != plugin);
    }
}

fn refuse(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_string(),
        retry_after_ms: None,
    }
}

/// Host side of the `keys` interface of one plugin. Cloning shares the table and the owner.
#[derive(Debug, Clone)]
pub struct KeysImport {
    grabs: KeyGrabs,
    plugin: String,
}

impl Default for KeysImport {
    fn default() -> Self {
        Self::new(KeyGrabs::new(), "ilium.host.standalone")
    }
}

impl KeysImport {
    /// The import of `plugin` over the runtime's table.
    pub fn new(grabs: KeyGrabs, plugin: &str) -> Self {
        Self {
            grabs,
            plugin: plugin.to_owned(),
        }
    }
}

impl HostKeys for KeysImport {
    fn grab(&mut self, spec: GrabSpec) -> Result<GrabId, PluginError> {
        if spec.chords.is_empty() || spec.chords.iter().any(|chord| chord.is_empty()) {
            return Err(refuse(ErrorCode::InvalidArgument, "grab names no chord"));
        }
        if spec
            .chords
            .iter()
            .any(|chord| RESERVED_CHORDS.contains(&chord.as_str()))
        {
            return Err(refuse(ErrorCode::Conflict, "chord is reserved by the kernel"));
        }
        let mut table = self.grabs.lock();
        if table.grabs.len() >= SESSION_KEY_GRAB_CAP {
            return Err(refuse(
                ErrorCode::QuotaExceeded,
                "key grab limit reached",
            ));
        }
        let already_held = table
            .grabs
            .values()
            .flat_map(|(_, chords)| chords)
            .any(|held| spec.chords.contains(held));
        if already_held {
            return Err(refuse(ErrorCode::Conflict, "chord is already grabbed"));
        }
        table.next_id += 1;
        let id = table.next_id;
        table.grabs.insert(id, (self.plugin.clone(), spec.chords));
        Ok(GrabId(id))
    }

    fn release(&mut self, grab: GrabId) {
        let mut table = self.grabs.lock();
        // A plugin releases only its own grabs.
        if table
            .grabs
            .get(&grab.0)
            .is_some_and(|(owner, _)| *owner == self.plugin)
        {
            table.grabs.remove(&grab.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{KeyGrabs, KeysImport};
    use ilium_plugin_api::generated::host::keys::{GrabSpec, HostKeys};
    use ilium_plugin_api::generated::types::errors::ErrorCode;

    /// Session-wide cap on key grabs across all plugins of one session (api-host, API-070).
    const SESSION_KEY_GRAB_CAP: usize = 32;

    fn spec_for_chord(chord: &str) -> GrabSpec {
        GrabSpec {
            chords: vec![chord.to_string()],
            pane: None,
            until_release: false,
        }
    }

    /// REQ: API-070
    #[test]
    fn grab_refuses_with_quota_exceeded_once_the_key_grab_limit_is_reached() {
        let mut import = KeysImport::default();
        let chords: Vec<String> = ('a'..='z')
            .map(|letter| format!("alt+{letter}"))
            .chain(('a'..='z').map(|letter| format!("ctrl+{letter}")))
            .collect();

        let mut granted_count = 0usize;
        let mut refusal_code: Option<ErrorCode> = None;
        for chord in &chords {
            match import.grab(spec_for_chord(chord)) {
                Ok(_) => granted_count += 1,
                Err(error) => {
                    refusal_code = Some(error.code);
                    break;
                }
            }
        }

        assert_eq!(
            refusal_code,
            Some(ErrorCode::QuotaExceeded),
            "the grab past the key grab limit must return quota-exceeded"
        );
        assert!(
            granted_count <= SESSION_KEY_GRAB_CAP,
            "grants before the refusal ({granted_count}) must not exceed the cap of {SESSION_KEY_GRAB_CAP}"
        );
    }

    #[test]
    fn two_runtimes_have_two_grab_tables() {
        let mut first = KeysImport::new(KeyGrabs::new(), "a.one");
        let mut second = KeysImport::new(KeyGrabs::new(), "a.one");
        first.grab(spec_for_chord("alt+q")).expect("first grabs");
        second
            .grab(spec_for_chord("alt+q"))
            .expect("the other table does not know the chord");
    }

    #[test]
    fn plugins_of_one_table_conflict_and_release_only_their_own_grabs() {
        let grabs = KeyGrabs::new();
        let mut owner = KeysImport::new(grabs.clone(), "a.owner");
        let mut other = KeysImport::new(grabs.clone(), "a.other");
        let held = owner.grab(spec_for_chord("alt+q")).expect("grabbed");
        let error = other.grab(spec_for_chord("alt+q")).expect_err("conflict");
        assert_eq!(error.code, ErrorCode::Conflict);
        other.release(held);
        assert_eq!(grabs.held(), 1, "a foreign release changes nothing");
        owner.release(held);
        assert_eq!(grabs.held(), 0);
    }

    #[test]
    fn a_stopped_plugin_gives_its_grabs_back() {
        let grabs = KeyGrabs::new();
        let mut owner = KeysImport::new(grabs.clone(), "a.owner");
        owner.grab(spec_for_chord("alt+q")).expect("grabbed");
        owner.grab(spec_for_chord("alt+w")).expect("grabbed");
        grabs.forget_plugin("a.owner");
        assert_eq!(grabs.held(), 0);
    }
}
