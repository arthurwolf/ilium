//! Non-blocking server-owned sound playback and global config reload.
//!
//! Detection enqueues semantic events into one bounded actor per detached
//! server. The actor serializes external player processes away from the
//! detection and IPC loops, so a long sound or unavailable audio device can
//! never hold the tree/pane locks or delay another status broadcast.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use std::{hash::DefaultHasher, hash::Hash, hash::Hasher};

use ilium_sound::{SoundEvent, SoundSettings};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::state::ServerState;

const SOUND_QUEUE_CAPACITY: usize = 64;
const CONFIG_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Injectable blocking playback boundary. Production delegates to
/// `ilium-sound`; tests use a recorder or no-op and never touch audio.
pub trait SoundPlayer: Send + Sync {
    fn play(&self, settings: &SoundSettings) -> Result<(), ilium_sound::SoundError>;
}

/// Real operating-system player used by `ilium-server`'s binary entrypoint.
pub struct SystemSoundPlayer;

impl SoundPlayer for SystemSoundPlayer {
    fn play(&self, settings: &SoundSettings) -> Result<(), ilium_sound::SoundError> {
        ilium_sound::play(settings)
    }
}

/// Silent player for integration tests whose subject is not audio.
pub struct NoopSoundPlayer;

impl SoundPlayer for NoopSoundPlayer {
    fn play(&self, _settings: &SoundSettings) -> Result<(), ilium_sound::SoundError> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PlaybackRequest {
    pub settings: SoundSettings,
    pub event: Option<SoundEvent>,
    pub pane_name: Option<String>,
}

/// Creates the actor's bounded sender and owned task.
pub(crate) fn spawn(
    player: Arc<dyn SoundPlayer>,
) -> (mpsc::Sender<PlaybackRequest>, JoinHandle<()>) {
    let (sender, mut receiver) = mpsc::channel::<PlaybackRequest>(SOUND_QUEUE_CAPACITY);
    let task = tokio::spawn(async move {
        while let Some(request) = receiver.recv().await {
            let player = Arc::clone(&player);
            let settings = request.settings;
            let result = tokio::task::spawn_blocking(move || player.play(&settings)).await;
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::warn!(
                    "sound playback failed for {:?} in pane {:?}: {error}",
                    request.event,
                    request.pane_name
                ),
                Err(error) => tracing::warn!(
                    "sound playback task panicked for {:?} in pane {:?}: {error}",
                    request.event,
                    request.pane_name
                ),
            }
        }
    });
    (sender, task)
}

/// Enqueues without awaiting capacity. A full audio queue must never turn a
/// burst of agent transitions into backpressure on detection.
pub(crate) fn enqueue(state: &ServerState, request: PlaybackRequest) {
    if let Err(error) = state.sound_requests.try_send(request) {
        tracing::warn!("dropping sound request because the playback queue is unavailable: {error}");
    }
}

/// Polls the user-global config file for changes so all already-running
/// project servers converge on settings changed in any attached client.
/// Invalid/intermediate writes retain the last known-good value and retry on
/// the next tick.
pub(crate) fn spawn_config_watcher(
    state: Arc<ServerState>,
    config_path: PathBuf,
) -> JoinHandle<()> {
    spawn_config_watcher_with_interval(state, config_path, CONFIG_POLL_INTERVAL)
}

fn spawn_config_watcher_with_interval(
    state: Arc<ServerState>,
    config_path: PathBuf,
    poll_interval: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut observed_fingerprint = poll_config_blocking(&config_path).await;
        let mut interval = tokio::time::interval(poll_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;
            let fingerprint = poll_config_blocking(&config_path).await;
            if fingerprint == observed_fingerprint {
                continue;
            }
            let Some(config_dir) = config_path.parent().map(Path::to_path_buf) else {
                continue;
            };
            match load_config_blocking(config_dir).await {
                Ok(config) => {
                    *state.sound_settings.write().await = config.sound;
                    observed_fingerprint = fingerprint;
                }
                Err(error) => tracing::warn!(
                    "failed to reload sound settings from {:?}, keeping last valid settings: {error}",
                    config_path
                ),
            }
        }
    })
}

/// Hashes the config file's bytes on a blocking thread. This tick fires
/// every `poll_interval` for the lifetime of the server, so -- exactly like
/// `detection.rs`'s `sysinfo` refresh and `notifications.rs`'s `notify-rust`
/// call -- the filesystem read must not run inline on a tokio worker thread.
/// A panicked hashing task is treated as "no observable change this tick"
/// rather than propagated, since a transient failure here must never bring
/// down the watcher loop.
async fn poll_config_blocking(config_path: &Path) -> Option<u64> {
    let config_path = config_path.to_path_buf();
    tokio::task::spawn_blocking(move || file_fingerprint(&config_path))
        .await
        .unwrap_or(None)
}

/// Parses the config file on a blocking thread for the same reason as
/// `poll_config_blocking`. A panicked parse task is surfaced as a config
/// load failure (stringified rather than a new `ServerError` variant, since
/// the only consumer logs it) so the caller reports it and keeps the last
/// known-good settings, rather than silently proceeding as if nothing
/// changed.
async fn load_config_blocking(config_dir: PathBuf) -> Result<crate::config::ServerConfig, String> {
    match tokio::task::spawn_blocking(move || crate::config::load(&config_dir)).await {
        Ok(result) => result.map_err(|error| error.to_string()),
        Err(join_error) => Err(format!("config reload task panicked: {join_error}")),
    }
}

fn file_fingerprint(path: &Path) -> Option<u64> {
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some(hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct RecordingPlayer {
        calls: Arc<Mutex<Vec<SoundSettings>>>,
    }

    impl SoundPlayer for RecordingPlayer {
        fn play(&self, settings: &SoundSettings) -> Result<(), ilium_sound::SoundError> {
            self.calls.lock().unwrap().push(settings.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn actor_forwards_requests_in_order_without_real_audio() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let player = Arc::new(RecordingPlayer {
            calls: Arc::clone(&calls),
        });
        let (sender, task) = spawn(player);
        let first = SoundSettings {
            file: Some(PathBuf::from("/first.oga")),
            ..SoundSettings::default()
        };
        let mut second = first.clone();
        second.file = Some(PathBuf::from("/second.oga"));

        sender
            .send(PlaybackRequest {
                settings: first.clone(),
                event: Some(SoundEvent::AgentFinished),
                pane_name: Some("one".to_string()),
            })
            .await
            .unwrap();
        sender
            .send(PlaybackRequest {
                settings: second.clone(),
                event: Some(SoundEvent::ApprovalRequired),
                pane_name: Some("two".to_string()),
            })
            .await
            .unwrap();
        drop(sender);
        task.await.unwrap();

        assert_eq!(*calls.lock().unwrap(), vec![first, second]);
    }

    #[tokio::test]
    async fn config_watcher_updates_an_already_running_server() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("config.toml");
        std::fs::write(&config_path, "[sound]\nsource = \"system_beep\"\n").unwrap();
        let (sound_requests, playback_task) = spawn(Arc::new(NoopSoundPlayer));
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "watcher-test".to_string(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("snapshot.json"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
        }));
        let watcher = spawn_config_watcher_with_interval(
            Arc::clone(&state),
            config_path.clone(),
            Duration::from_millis(20),
        );

        // Let the watcher record the initial config before testing a subsequent edit.
        tokio::time::sleep(Duration::from_millis(30)).await;

        std::fs::write(&config_path, "[sound.events]\napproval_required = true\n").unwrap();
        let updated = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if state.sound_settings.read().await.events.approval_required {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;

        watcher.abort();
        playback_task.abort();
        assert!(
            updated.is_ok(),
            "watcher did not apply the changed sound table"
        );
    }
}
