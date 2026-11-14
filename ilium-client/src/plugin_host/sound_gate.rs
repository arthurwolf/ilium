//! Decides what one `play-sound` intent of a client plugin does (capability-sound-play.md):
//! the catalogue check, the rate of 4 per second per plugin, the user's mute, and which bytes
//! reach the audio output. Pure: the caller passes the time and plays what comes back, so the
//! rules are tested without a sound device.

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_host::imports::sounds::{SoundsImport, BUNDLED_SOUND_ID};

/// `play-sound` submissions one plugin may make per [`RATE_WINDOW`].
pub(crate) const SOUNDS_PER_WINDOW: usize = 4;
/// The window of the play rate.
pub(crate) const RATE_WINDOW: Duration = Duration::from_secs(1);

/// What a `play-sound` does.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SoundDecision {
    /// Accepted: play these WAV bytes.
    Play(&'static [u8]),
    /// Accepted, but the user muted sounds: nothing plays and the outcome is `ok`.
    Muted,
    /// Refused before anything played (`not-found`, `rate-limited`).
    Refused(PluginError),
    /// Accepted, but there is nothing this client can play (`failed(io)`).
    Failed(PluginError),
}

/// The per-plugin sound catalogues and play history of one client.
#[derive(Debug, Default)]
pub(crate) struct SoundGate {
    catalogs: BTreeMap<String, SoundsImport>,
    accepted: BTreeMap<String, VecDeque<Instant>>,
    muted: bool,
}

impl SoundGate {
    /// The catalogues of `plugins`, `(plugin id, manifest text)`. A manifest that does not
    /// parse gets the bundled id alone.
    pub(crate) fn new<'a>(plugins: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let catalogs = plugins
            .into_iter()
            .map(|(plugin, manifest)| {
                let catalog = serde_json::from_str::<serde_json::Value>(manifest)
                    .map(|manifest| SoundsImport::for_manifest(&manifest))
                    .unwrap_or_default();
                (plugin.to_owned(), catalog)
            })
            .collect();
        Self {
            catalogs,
            accepted: BTreeMap::new(),
            muted: false,
        }
    }

    /// Follows the user's sound setting (Settings > Sounds, source "Muted").
    pub(crate) fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    /// Forgets a plugin's play history (it was disabled or reloaded).
    pub(crate) fn forget(&mut self, plugin: &str) {
        self.accepted.remove(plugin);
    }

    /// Decides one `play-sound` of `plugin` at `now`.
    pub(crate) fn decide(&mut self, plugin: &str, sound: &str, now: Instant) -> SoundDecision {
        let in_catalog = self
            .catalogs
            .get(plugin)
            .map_or(sound == BUNDLED_SOUND_ID, |catalog| catalog.contains(sound));
        if !in_catalog {
            return SoundDecision::Refused(PluginError {
                code: ErrorCode::NotFound,
                message: format!("{sound:?} is not in this plugin's sound catalogue"),
                retry_after_ms: None,
            });
        }
        let accepted = self.accepted.entry(plugin.to_owned()).or_default();
        while accepted
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) >= RATE_WINDOW)
        {
            accepted.pop_front();
        }
        if accepted.len() >= SOUNDS_PER_WINDOW {
            // The oldest accepted play leaves the window first; a refusal reserves nothing.
            let wait = accepted.front().map_or(RATE_WINDOW, |oldest| {
                RATE_WINDOW.saturating_sub(now.saturating_duration_since(*oldest))
            });
            let wait_ms = u32::try_from(wait.as_millis()).unwrap_or(u32::MAX).max(1);
            return SoundDecision::Refused(PluginError {
                code: ErrorCode::RateLimited,
                message: "at most 4 sounds per second".to_owned(),
                retry_after_ms: Some(wait_ms),
            });
        }
        accepted.push_back(now);
        if self.muted {
            return SoundDecision::Muted;
        }
        if sound == BUNDLED_SOUND_ID {
            return SoundDecision::Play(ilium_sound::BUNDLED_CHIRPING_WAV);
        }
        // The client holds a plugin's manifest and component, not the files of its package.
        SoundDecision::Failed(PluginError {
            code: ErrorCode::Io,
            message: format!("the file of {sound:?} is not available to this client"),
            retry_after_ms: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTES: &str = r#"{"id": "com.example.notes",
        "contributes": {"sounds": [{"id": "done", "file": "sounds/done.ogg"}]}}"#;

    fn gate() -> SoundGate {
        SoundGate::new([("com.example.notes", NOTES), ("com.example.other", "{}")])
    }

    fn code(decision: &SoundDecision) -> Option<ErrorCode> {
        match decision {
            SoundDecision::Refused(error) | SoundDecision::Failed(error) => Some(error.code),
            SoundDecision::Play(_) | SoundDecision::Muted => None,
        }
    }

    #[test]
    fn the_bundled_sound_plays_its_embedded_bytes() {
        let decision = gate().decide("com.example.notes", "chirping", Instant::now());
        assert_eq!(
            decision,
            SoundDecision::Play(ilium_sound::BUNDLED_CHIRPING_WAV)
        );
    }

    #[test]
    fn another_plugins_sound_is_not_found() {
        let mut gate = gate();
        let now = Instant::now();
        assert_eq!(
            code(&gate.decide("com.example.other", "done", now)),
            Some(ErrorCode::NotFound)
        );
        assert_eq!(
            code(&gate.decide("com.example.notes", "bell", now)),
            Some(ErrorCode::NotFound)
        );
        // Its own contributed id passes the catalogue; its file is what is missing.
        assert_eq!(
            code(&gate.decide("com.example.notes", "done", now)),
            Some(ErrorCode::Io)
        );
    }

    #[test]
    fn the_fifth_sound_in_one_second_is_rate_limited_and_reserves_nothing() {
        let mut gate = gate();
        let start = Instant::now();
        for offset in 0..4 {
            let at = start + Duration::from_millis(offset * 100);
            assert!(code(&gate.decide("com.example.notes", "chirping", at)).is_none());
        }
        let fifth = gate.decide(
            "com.example.notes",
            "chirping",
            start + Duration::from_millis(400),
        );
        let SoundDecision::Refused(error) = fifth else {
            panic!("expected a refusal, got {fifth:?}");
        };
        assert_eq!(error.code, ErrorCode::RateLimited);
        assert_eq!(error.retry_after_ms, Some(600));
        // Another plugin has its own budget.
        assert!(code(&gate.decide("com.example.other", "chirping", start)).is_none());
        // Once the first play leaves the window, one more fits.
        assert!(code(&gate.decide(
            "com.example.notes",
            "chirping",
            start + Duration::from_millis(1000)
        ))
        .is_none());
    }

    #[test]
    fn a_muted_client_accepts_and_plays_nothing() {
        let mut gate = gate();
        gate.set_muted(true);
        assert_eq!(
            gate.decide("com.example.notes", "chirping", Instant::now()),
            SoundDecision::Muted
        );
        // The catalogue still refuses unknown ids while muted.
        assert_eq!(
            code(&gate.decide("com.example.notes", "bell", Instant::now())),
            Some(ErrorCode::NotFound)
        );
    }
}
