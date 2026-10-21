//! Host implementation of `ilium:host/keys` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostKeys` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::keys::{GrabSpec, HostKeys};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::GrabId;

/// Host side of the `keys` interface.
#[derive(Debug)]
pub struct KeysImport;

impl HostKeys for KeysImport {
    fn grab(&mut self, _spec: GrabSpec) -> Result<GrabId, PluginError> {
        unimplemented!("host::imports::keys::grab")
    }

    fn release(&mut self, _grab: GrabId) {
        unimplemented!("host::imports::keys::release")
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
