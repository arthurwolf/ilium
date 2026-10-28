//! Host implementation of `ilium:host/sounds`.
//!
//! The catalog lists the sound ids a plugin may pass to `sound.play`: the one sound Ilium ships
//! inside the client (capability-sound-play.md, "Sound ids and files"). Plugin-contributed ids
//! (`contributes.sounds`) are merged in by the registry that owns manifests, not by this import.

use ilium_plugin_api::generated::host::sounds::HostSounds;

/// The sound bundled inside the client binary.
const BUNDLED_SOUND_ID: &str = "chirping";

/// Host side of the `sounds` interface.
#[derive(Debug)]
pub struct SoundsImport;

impl HostSounds for SoundsImport {
    fn catalog(&mut self) -> Vec<String> {
        vec![BUNDLED_SOUND_ID.to_owned()]
    }
}

#[cfg(test)]
mod tests {
    use super::SoundsImport;
    use ilium_plugin_api::generated::host::sounds::HostSounds;
    use std::collections::HashSet;

    /// The one sound that Ilium ships inside the client (capability-sound-play.md, "Sound ids and files").
    const BUNDLED_SOUND_ID: &str = "chirping";

    fn catalog_listing() -> Vec<String> {
        let mut sounds_import = SoundsImport;
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
}
