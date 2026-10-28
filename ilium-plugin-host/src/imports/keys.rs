//! Host implementation of `ilium:host/keys`.
//!
//! Tracks grabs in one process-wide table capped at the session limit of 32 grabs.

use ilium_plugin_api::generated::host::keys::{GrabSpec, HostKeys};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::GrabId;
use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

/// Session-wide cap on key grabs (api-host, API-070).
const SESSION_KEY_GRAB_CAP: usize = 32;

/// Chords the kernel keeps for itself; plugins cannot grab them.
const RESERVED_CHORDS: [&str; 2] = ["ctrl+shift+escape", "ctrl+alt+escape"];

#[derive(Default)]
struct GrabTable {
    next_id: u64,
    grabs: HashMap<u64, Vec<String>>,
}

static GRABS: Mutex<Option<GrabTable>> = Mutex::new(None);

fn refuse(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_string(),
        retry_after_ms: None,
    }
}

/// Host side of the `keys` interface.
#[derive(Debug)]
pub struct KeysImport;

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
        let mut guard = GRABS.lock().unwrap_or_else(PoisonError::into_inner);
        let table = guard.get_or_insert_with(GrabTable::default);
        if table.grabs.len() >= SESSION_KEY_GRAB_CAP {
            return Err(refuse(
                ErrorCode::QuotaExceeded,
                "key grab limit reached",
            ));
        }
        let already_held = table
            .grabs
            .values()
            .flatten()
            .any(|held| spec.chords.contains(held));
        if already_held {
            return Err(refuse(ErrorCode::Conflict, "chord is already grabbed"));
        }
        table.next_id += 1;
        let id = table.next_id;
        table.grabs.insert(id, spec.chords);
        Ok(GrabId(id))
    }

    fn release(&mut self, grab: GrabId) {
        let mut guard = GRABS.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(table) = guard.as_mut() {
            table.grabs.remove(&grab.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::KeysImport;
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
        let mut import = KeysImport;
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
}
