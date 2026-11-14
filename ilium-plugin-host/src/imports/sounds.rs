//! Host implementation of `ilium:host/sounds`.
//!
//! The catalog lists the sound ids a plugin may pass to `sound.play`: the one sound Ilium ships
//! inside the client, then the plugin's own `contributes.sounds` ids in manifest order
//! (capability-sound-play.md, "Sound ids and files"). A plugin never sees another plugin's ids.

use ilium_plugin_api::generated::host::sounds::HostSounds;
use serde_json::Value;

/// The sound bundled inside the client binary.
pub const BUNDLED_SOUND_ID: &str = "chirping";

/// Host side of the `sounds` interface, for one plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundsImport {
    /// The bundled id, then the plugin's contributed ids.
    catalog: Vec<String>,
}

impl Default for SoundsImport {
    /// The catalog of a plugin that contributes no sounds: the bundled id alone.
    fn default() -> Self {
        Self {
            catalog: vec![BUNDLED_SOUND_ID.to_owned()],
        }
    }
}

impl SoundsImport {
    /// The catalog of the plugin whose parsed manifest is `manifest`.
    pub fn for_manifest(manifest: &Value) -> Self {
        let mut import = Self::default();
        for id in contributed_sound_ids(manifest) {
            if !import.catalog.contains(&id) {
                import.catalog.push(id);
            }
        }
        import
    }

    /// Whether `sound` is in this plugin's catalog.
    pub fn contains(&self, sound: &str) -> bool {
        self.catalog.iter().any(|id| id == sound)
    }
}

/// The ids of a manifest's `contributes.sounds` entries, in order. Entries without a
/// non-empty string `id` are skipped; the install check already refused them, and the
/// bundled id cannot be taken (`manifest.sound-id-taken`).
pub fn contributed_sound_ids(manifest: &Value) -> Vec<String> {
    manifest
        .pointer("/contributes/sounds")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.get("id").and_then(Value::as_str))
                .filter(|id| !id.is_empty() && *id != BUNDLED_SOUND_ID)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

impl HostSounds for SoundsImport {
    fn catalog(&mut self) -> Vec<String> {
        self.catalog.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::SoundsImport;
    use ilium_plugin_api::generated::host::sounds::HostSounds;
    use serde_json::json;
    use std::collections::HashSet;

    /// The one sound that Ilium ships inside the client (capability-sound-play.md, "Sound ids and files").
    const BUNDLED_SOUND_ID: &str = "chirping";

    fn catalog_listing() -> Vec<String> {
        let mut sounds_import = SoundsImport::default();
        sounds_import.catalog()
    }

    /// REQ: PKG-065
    #[test]
    fn catalog_lists_the_bundled_chirping_sound() {
        let listing = catalog_listing();
        assert!(
            listing
                .iter()
                .any(|sound_id| sound_id.as_str() == BUNDLED_SOUND_ID),
            "catalog must hold the bundled id `{BUNDLED_SOUND_ID}`, got {listing:?}"
        );
    }

    /// REQ: PKG-065
    #[test]
    fn catalog_is_never_empty_when_no_plugin_contributes_sounds() {
        let listing = catalog_listing();
        assert!(
            !listing.is_empty(),
            "a plugin with no contributes.sounds still sees the bundled sound"
        );
    }

    /// REQ: PKG-065
    #[test]
    fn catalog_lists_each_sound_id_exactly_once() {
        let listing = catalog_listing();
        let distinct_ids: HashSet<&str> = listing.iter().map(String::as_str).collect();
        assert_eq!(
            distinct_ids.len(),
            listing.len(),
            "a sound id appears twice in {listing:?}"
        );
    }

    /// REQ: PKG-065
    #[test]
    fn catalog_holds_no_empty_sound_id() {
        let listing = catalog_listing();
        assert!(
            listing.iter().all(|sound_id| !sound_id.is_empty()),
            "an empty sound id is in {listing:?}"
        );
    }

    /// REQ: PKG-065
    #[test]
    fn catalog_returns_the_same_listing_on_repeated_calls() {
        let first_listing = catalog_listing();
        let second_listing = catalog_listing();
        assert_eq!(
            first_listing, second_listing,
            "catalog is synchronous and stateless"
        );
    }

    /// REQ: PKG-065
    #[test]
    fn a_plugin_sees_the_bundled_id_then_its_own_contributed_ids() {
        let manifest = json!({
            "id": "com.example.notes",
            "contributes": { "sounds": [
                { "id": "done", "file": "sounds/done.ogg" },
                { "id": "chirping", "file": "sounds/taken.ogg" },
                { "id": "done", "file": "sounds/again.ogg" },
                { "file": "sounds/no-id.ogg" },
                { "id": "ping", "file": "sounds/ping.wav" }
            ] }
        });
        let mut import = SoundsImport::for_manifest(&manifest);
        assert_eq!(import.catalog(), ["chirping", "done", "ping"]);
        assert!(import.contains("done"));
        // Another plugin's catalog does not hold these ids.
        let other = SoundsImport::for_manifest(&json!({ "id": "com.example.other" }));
        assert!(!other.contains("done"));
        assert!(other.contains("chirping"));
    }
}
