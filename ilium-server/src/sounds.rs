//! Non-blocking server-owned sound playback and global config reload.
//!
//! Detection enqueues semantic events into one bounded actor per detached
//! server. The actor serializes external player processes away from the
//! detection and IPC loops, so a long sound or unavailable audio device can
//! never hold the tree/pane locks or delay another status broadcast.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ilium_sound::{SoundEvent, SoundSettings, SoundSourceKind};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::execution::{ExecutionClient, ExecutionError};
pub(crate) use crate::sound_settings_storage::SharedSoundSettings;
use crate::state::ServerState;
use ilium_execution::{JobCost, Lane, StorageAdmission};

const SOUND_QUEUE_CAPACITY: usize = 64;
const CONFIG_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Injectable blocking playback boundary. Production delegates to
/// `ilium-sound`; tests use a recorder or no-op and never touch audio.
pub trait SoundPlayer: Send + Sync {
    fn play(
        &self,
        settings: &SoundSettings,
        event: Option<SoundEvent>,
    ) -> Result<(), ilium_sound::SoundError>;

    /// Prepare generated PCM/WAV content before entering the ordered blocking
    /// playback lane. Existing players need not handle generated sounds until
    /// they opt into this split boundary.
    fn prepare_generated(
        &self,
        _settings: &SoundSettings,
    ) -> Result<Option<Vec<u8>>, ilium_sound::SoundError> {
        Ok(None)
    }

    /// Play a request whose generated content may already have been rendered
    /// by the bounded CPU lane. The default keeps injectable legacy players
    /// source-compatible while production can consume the prepared bytes.
    fn play_prepared(
        &self,
        settings: &SoundSettings,
        event: Option<SoundEvent>,
        _generated_wav: Option<&[u8]>,
    ) -> Result<(), ilium_sound::SoundError> {
        self.play(settings, event)
    }
}

/// Real operating-system player used by `ilium-server`'s binary entrypoint.
pub struct SystemSoundPlayer;

impl SoundPlayer for SystemSoundPlayer {
    fn play(
        &self,
        settings: &SoundSettings,
        _event: Option<SoundEvent>,
    ) -> Result<(), ilium_sound::SoundError> {
        ilium_sound::play(settings)
    }

    fn prepare_generated(
        &self,
        settings: &SoundSettings,
    ) -> Result<Option<Vec<u8>>, ilium_sound::SoundError> {
        Ok((settings.source == SoundSourceKind::Generated)
            .then(|| ilium_sound::render_wav(&settings.design)))
    }

    fn play_prepared(
        &self,
        settings: &SoundSettings,
        event: Option<SoundEvent>,
        generated_wav: Option<&[u8]>,
    ) -> Result<(), ilium_sound::SoundError> {
        if settings.source == SoundSourceKind::Generated {
            if let Some(wav) = generated_wav {
                return ilium_sound::play_prepared_wav(wav);
            }
        }
        self.play(settings, event)
    }
}

/// Silent player for integration tests whose subject is not audio.
pub struct NoopSoundPlayer;

impl SoundPlayer for NoopSoundPlayer {
    fn play(
        &self,
        _settings: &SoundSettings,
        _event: Option<SoundEvent>,
    ) -> Result<(), ilium_sound::SoundError> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PlaybackRequest {
    pub settings: Arc<SharedSoundSettings>,
    pub event: Option<SoundEvent>,
    pub pane_name: Option<String>,
}

// The request is destroyed before its admission guard. Shared Arc owners
// keep the original request charged through queueing, playback and cancellation.
pub(crate) struct AdmittedPlaybackRequest {
    request: PlaybackRequest,
    preview_reply: Option<crate::ipc::DirectEventSender>,
    _admission: Arc<StorageAdmission>,
}
impl std::ops::Deref for AdmittedPlaybackRequest {
    type Target = PlaybackRequest;
    fn deref(&self) -> &Self::Target {
        &self.request
    }
}
#[derive(Clone)]
pub(crate) struct PlaybackSender {
    sender: mpsc::Sender<Arc<AdmittedPlaybackRequest>>,
    execution: ExecutionClient,
}
enum PlaybackSendError {
    Admission {
        reason: ilium_execution::RejectReason,
        _request: PlaybackRequest,
    },
    Queue(mpsc::error::TrySendError<Arc<AdmittedPlaybackRequest>>),
    #[cfg(test)]
    Closed(mpsc::error::SendError<Arc<AdmittedPlaybackRequest>>),
}
impl std::fmt::Debug for PlaybackSendError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)
    }
}
impl std::fmt::Display for PlaybackSendError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Admission { reason, .. } => {
                write!(formatter, "sound request admission: {reason:?}")
            }
            Self::Queue(error) => std::fmt::Display::fmt(error, formatter),
            #[cfg(test)]
            Self::Closed(error) => std::fmt::Display::fmt(error, formatter),
        }
    }
}
fn request_bytes(request: &PlaybackRequest) -> Option<usize> {
    std::mem::size_of::<AdmittedPlaybackRequest>()
        .checked_add(std::mem::size_of::<StorageAdmission>())?
        .checked_add(8 * std::mem::size_of::<usize>())?
        .checked_add(request.pane_name.as_ref().map_or(0, String::capacity))
}
impl PlaybackSender {
    // Reserve before allocating the new pane label and request Arc. Shared
    // settings keep their original storage; no path copy is needed.
    pub(crate) fn prepare(
        &self,
        settings: &Arc<SharedSoundSettings>,
        event: Option<SoundEvent>,
        pane_name: Option<&str>,
    ) -> Result<Arc<AdmittedPlaybackRequest>, ilium_execution::RejectReason> {
        let bytes = std::mem::size_of::<AdmittedPlaybackRequest>()
            .checked_add(std::mem::size_of::<StorageAdmission>())
            .and_then(|n| n.checked_add(8 * std::mem::size_of::<usize>()))
            .and_then(|n| n.checked_add(pane_name.map_or(0, str::len)))
            .ok_or(ilium_execution::RejectReason::InvalidCost)?;
        let admission = self.execution.try_reserve_storage(bytes)?;
        let pane_name = pane_name.map(|text| {
            let mut owned = String::with_capacity(text.len());
            owned.push_str(text);
            owned
        });
        Ok(Arc::new(AdmittedPlaybackRequest {
            request: PlaybackRequest {
                settings: Arc::clone(settings),
                event,
                pane_name,
            },
            preview_reply: None,
            _admission: admission,
        }))
    }
    pub(crate) fn admit_settings(
        &self,
        settings: SoundSettings,
    ) -> Result<Arc<SharedSoundSettings>, (ilium_execution::RejectReason, SoundSettings)> {
        SharedSoundSettings::try_new(&self.execution, settings)
    }
    fn try_send_prepared(
        &self,
        request: Arc<AdmittedPlaybackRequest>,
    ) -> Result<(), PlaybackSendError> {
        self.sender
            .try_send(request)
            .map_err(PlaybackSendError::Queue)
    }

    fn try_send(&self, request: PlaybackRequest) -> Result<(), PlaybackSendError> {
        self.try_send_with_preview_reply(request, None)
    }

    fn try_send_with_preview_reply(
        &self,
        request: PlaybackRequest,
        preview_reply: Option<crate::ipc::DirectEventSender>,
    ) -> Result<(), PlaybackSendError> {
        let Some(bytes) = request_bytes(&request) else {
            return Err(PlaybackSendError::Admission {
                reason: ilium_execution::RejectReason::InvalidCost,
                _request: request,
            });
        };
        let admission = match self.execution.try_reserve_storage(bytes) {
            Ok(admission) => admission,
            Err(reason) => {
                return Err(PlaybackSendError::Admission {
                    reason,
                    _request: request,
                });
            }
        };
        // Capacity and byte refusal retain the sole original request in the
        // typed error until the producer explicitly logs/disposes the overflow.
        self.sender
            .try_send(Arc::new(AdmittedPlaybackRequest {
                request,
                preview_reply,
                _admission: admission,
            }))
            .map_err(PlaybackSendError::Queue)
    }
    #[cfg(test)]
    async fn send(&self, request: PlaybackRequest) -> Result<(), PlaybackSendError> {
        let Some(bytes) = request_bytes(&request) else {
            return Err(PlaybackSendError::Admission {
                reason: ilium_execution::RejectReason::InvalidCost,
                _request: request,
            });
        };
        let admission = match self.execution.reserve_storage(bytes).await {
            Ok(admission) => admission,
            Err(reason) => {
                return Err(PlaybackSendError::Admission {
                    reason,
                    _request: request,
                });
            }
        };
        self.sender
            .send(Arc::new(AdmittedPlaybackRequest {
                request,
                preview_reply: None,
                _admission: admission,
            }))
            .await
            .map_err(PlaybackSendError::Closed)
    }
}
#[cfg(test)]
pub(crate) fn test_channel(
    capacity: usize,
) -> (PlaybackSender, mpsc::Receiver<Arc<AdmittedPlaybackRequest>>) {
    let (sender, receiver) = mpsc::channel(capacity);
    (
        PlaybackSender {
            sender,
            execution: crate::execution::test_general_client(),
        },
        receiver,
    )
}

// Concrete typed callback follows the verified881 lifetime repair. The same
// original request Arc retains queued bytes until the native call returns.
struct PlaybackJob {
    player: Arc<dyn SoundPlayer>,
    request: Arc<AdmittedPlaybackRequest>,
    generated_wav: Option<Vec<u8>>,
    _generated_retention: Option<ilium_execution::Retention>,
}
impl ilium_execution::Job for PlaybackJob {
    type Output = ();
    type Error = ilium_sound::SoundError;
    fn run(self, _context: ilium_execution::JobContext) -> Result<(), Self::Error> {
        self.player.play_prepared(
            &self.request.request.settings,
            self.request.request.event,
            self.generated_wav.as_deref(),
        )
    }
}

struct GeneratedSoundPreparation {
    player: Arc<dyn SoundPlayer>,
    settings: Arc<SharedSoundSettings>,
}
impl ilium_execution::Job for GeneratedSoundPreparation {
    type Output = Option<Vec<u8>>;
    type Error = ilium_sound::SoundError;
    fn run(self, _context: ilium_execution::JobContext) -> Result<Self::Output, Self::Error> {
        self.player.prepare_generated(&self.settings)
    }
}

/// Creates the actor's bounded sender and owned task.
pub(crate) fn spawn(
    player: Arc<dyn SoundPlayer>,
    execution: ExecutionClient,
) -> (PlaybackSender, JoinHandle<()>) {
    let (sender, mut receiver) =
        mpsc::channel::<Arc<AdmittedPlaybackRequest>>(SOUND_QUEUE_CAPACITY);
    let sender = PlaybackSender {
        sender,
        execution: execution.clone(),
    };
    let task = tokio::spawn(async move {
        while let Some(request) = receiver.recv().await {
            let (generated_wav, generated_retention) =
                if request.request.settings.source == SoundSourceKind::Generated {
                    let preparation = execution
                        .reserve(
                            Lane::Cpu,
                            JobCost {
                                // Three seconds of mono 44.1kHz PCM plus its
                                // WAV output fit under this fixed admitted cap.
                                input_bytes: 1024 * 1024,
                                result_bytes: 512 * 1024,
                            },
                        )
                        .await;
                    let preparation = match preparation {
                        Ok(reservation) => reservation,
                        Err(reason) => {
                            tracing::warn!(?reason, event = ?request.request.event,
                                pane = ?request.request.pane_name,
                                "generated sound CPU admission refused");
                            complete_preview(&request, false).await;
                            continue;
                        }
                    };
                    let prepared = execution
                        .run_reserved(
                            preparation,
                            GeneratedSoundPreparation {
                                player: Arc::clone(&player),
                                settings: Arc::clone(&request.request.settings),
                            },
                        )
                        .await;
                    match prepared {
                        Ok(retained) => {
                            let (wav, retention) = retained.into_parts();
                            (wav, Some(retention))
                        }
                        Err(error) => {
                            tracing::warn!(%error, event = ?request.request.event,
                                pane = ?request.request.pane_name,
                                "generated sound preparation failed");
                            complete_preview(&request, false).await;
                            continue;
                        }
                    }
                } else {
                    (None, None)
                };
            let Some(cost) = playback_cost(&request.request) else {
                tracing::warn!(event = ?request.request.event, pane = ?request.request.pane_name,
                    "sound playback allocation declaration overflow");
                complete_preview(&request, false).await;
                continue;
            };
            // Wait with the original request still owned by this sole actor.
            // No later notification can overtake a temporarily refused job.
            let reservation = match execution.reserve(Lane::Io, cost).await {
                Ok(reservation) => reservation,
                Err(reason) => {
                    tracing::warn!(?reason, event = ?request.request.event, pane = ?request.request.pane_name,
                        "sound playback admission closed or permanently refused");
                    complete_preview(&request, false).await;
                    continue;
                }
            };
            let player = Arc::clone(&player);
            let native_request = Arc::clone(&request);
            let result = execution
                .run_reserved(
                    reservation,
                    PlaybackJob {
                        player,
                        request: native_request,
                        generated_wav,
                        _generated_retention: generated_retention,
                    },
                )
                .await;
            match result {
                Ok(completion) => {
                    drop(completion);
                    complete_preview(&request, true).await;
                }
                Err(ExecutionError::Failed(error)) => {
                    tracing::warn!(
                        "sound playback failed for {:?} in pane {:?}: {}",
                        request.request.event,
                        request.request.pane_name,
                        error.view()
                    );
                    complete_preview(&request, false).await;
                }
                Err(error) => {
                    tracing::warn!(
                        "sound playback worker outcome for {:?} in pane {:?}: {error}",
                        request.request.event,
                        request.request.pane_name
                    );
                    complete_preview(&request, false).await;
                }
            }
        }
    });
    (sender, task)
}

async fn complete_preview(request: &AdmittedPlaybackRequest, succeeded: bool) {
    if let Some(reply) = &request.preview_reply {
        let _ = reply
            .send(ilium_ipc::ServerEvent::SoundPreviewCompleted { succeeded })
            .await;
    }
}

/// Declare simultaneous bounded PCM/WAV and native command/error preparation.
/// Path capacity accounts for the original allocation, including spare bytes.
/// Native device/child/library memory is not an RSS guarantee of this ledger.
fn playback_cost(request: &PlaybackRequest) -> Option<JobCost> {
    let path_bytes = request.settings.file.as_ref().map_or(0, PathBuf::capacity);
    let dynamic_bytes =
        path_bytes.checked_add(request.pane_name.as_ref().map_or(0, String::capacity))?;
    Some(JobCost {
        // 3 s * 44100 Hz * 2 bytes each for simultaneous PCM and WAV is
        // 529244 bytes including the WAV header; one MiB leaves preparation
        // headroom without introducing a second synthesis/playback pipeline.
        input_bytes: dynamic_bytes.checked_mul(16)?.checked_add(1024 * 1024)?,
        result_bytes: path_bytes.checked_mul(8)?.checked_add(64 * 1024)?,
    })
}

/// Enqueues without awaiting capacity. A full audio queue must never turn a
/// burst of agent transitions into backpressure on detection.
pub(crate) fn enqueue(state: &ServerState, request: PlaybackRequest) {
    if let Err(error) = state.sound_requests.try_send(request) {
        tracing::warn!("dropping sound request because the playback queue is unavailable: {error}");
    }
}

/// Preview is a semantic request: unlike replaceable detection alerts, queue
/// refusal must produce a truthful completion for the requesting client.
pub(crate) async fn enqueue_preview(
    state: &ServerState,
    request: PlaybackRequest,
    reply: crate::ipc::DirectEventSender,
) {
    if let Err(error) = state
        .sound_requests
        .try_send_with_preview_reply(request, Some(reply.clone()))
    {
        tracing::warn!("sound preview queue refused request: {error}");
        let _ = reply
            .send(ilium_ipc::ServerEvent::SoundPreviewCompleted { succeeded: false })
            .await;
    }
}

/// Polls the user-global config file for changes so all already-running
/// project servers converge on settings changed in any attached client.
/// Invalid/intermediate writes -- including a transient window where the
/// file is briefly unreadable during a write-then-rename -- retain the last
/// known-good value and retry on the next tick.
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
    let execution = state.sound_requests.execution.clone();
    tokio::spawn(async move {
        if config_path.parent().is_none() {
            tracing::warn!(?config_path, "config watcher path has no parent directory");
            return;
        }
        let mut observed_fingerprint = None;
        let mut interval = tokio::time::interval(poll_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if state.text_trigger_config_path.get().is_some() {
                if let Err(error) = crate::text_trigger_config::refresh(&state).await {
                    tracing::warn!(%error, "Text Triggers reload failed; retaining accepted rules");
                }
            }
            let Some(input_bytes) = config_path
                .capacity()
                .checked_mul(4)
                .and_then(|n| n.checked_add(64 * 1024 * 1024))
            else {
                tracing::warn!("config read allocation declaration overflow");
                continue;
            };
            let reservation = match execution
                .reserve(
                    Lane::Io,
                    JobCost {
                        input_bytes,
                        result_bytes: 4 * 1024 * 1024,
                    },
                )
                .await
            {
                Ok(reservation) => reservation,
                Err(reason) => {
                    tracing::warn!(
                        ?reason,
                        "config refresh admission closed; retaining accepted settings"
                    );
                    continue;
                }
            };
            // Clone the path only after admission; the native owner reads once.
            let prepared = match execution
                .run_reserved(
                    reservation,
                    crate::config_refresh::RefreshJob {
                        path: config_path.clone(),
                        observed_fingerprint,
                    },
                )
                .await
            {
                Ok(prepared) => prepared,
                Err(error) => {
                    tracing::warn!(%error, "config refresh failed; retaining accepted settings");
                    continue;
                }
            };
            let (output, retention) = prepared.into_parts();
            if let Some(refresh) = output {
                match SharedSoundSettings::try_new(&execution, refresh.settings) {
                    Ok(settings) => {
                        *state.sound_settings.write().await = settings;
                        *state.notifications_config.write().await = refresh.notifications;
                        state.set_session_backups_enabled(refresh.backups_enabled);
                        // Refusals never consume the observed revision.
                        observed_fingerprint = Some(refresh.fingerprint);
                    }
                    Err((reason, original)) => {
                        tracing::warn!(
                            ?reason,
                            "config settings storage refused; retaining accepted settings"
                        );
                        drop(original);
                    }
                }
            }
            // Original parsed settings retired or have their own storage lease.
            drop(retention);
        }
    })
}

pub(crate) fn enqueue_prepared(state: &ServerState, request: Arc<AdmittedPlaybackRequest>) {
    if let Err(error) = state.sound_requests.try_send_prepared(request) {
        tracing::warn!("dropping sound request because the playback queue is unavailable: {error}");
    }
}
#[cfg(test)]
pub(crate) fn test_settings(settings: SoundSettings) -> Arc<SharedSoundSettings> {
    // `try_new` is non-blocking: parallel tests share one server bank, so a
    // reservation can transiently report `Busy`. Production callers retry the
    // same way (`ExecutionClient::reserve_storage`); only a hard rejection fails.
    let client = crate::execution::test_general_client();
    let mut settings = settings;
    for _ in 0..10_000 {
        match SharedSoundSettings::try_new(&client, settings) {
            Ok(shared) => return shared,
            Err((ilium_execution::RejectReason::Busy, returned)) => {
                settings = returned;
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err((reason, _)) => panic!("admit fixture settings on shared server bank: {reason:?}"),
        }
    }
    panic!("fixture settings stayed busy on the shared server bank for 10 s");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    type PlaybackCall = (SoundSettings, Option<SoundEvent>);

    struct RecordingPlayer {
        calls: Arc<Mutex<Vec<PlaybackCall>>>,
    }

    impl SoundPlayer for RecordingPlayer {
        fn play(
            &self,
            settings: &SoundSettings,
            event: Option<SoundEvent>,
        ) -> Result<(), ilium_sound::SoundError> {
            self.calls.lock().unwrap().push((settings.clone(), event));
            Ok(())
        }
    }

    #[tokio::test]
    async fn actor_forwards_requests_in_order_without_real_audio() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let player = Arc::new(RecordingPlayer {
            calls: Arc::clone(&calls),
        });
        let (sender, task) = spawn(player, crate::execution::test_general_client());
        let first = SoundSettings {
            file: Some(PathBuf::from("/first.oga")),
            ..SoundSettings::default()
        };
        let mut second = first.clone();
        second.file = Some(PathBuf::from("/second.oga"));

        sender
            .send(PlaybackRequest {
                settings: test_settings(first.clone()),
                event: Some(SoundEvent::AgentFinished),
                pane_name: Some("one".to_string()),
            })
            .await
            .unwrap();
        sender
            .send(PlaybackRequest {
                settings: test_settings(second.clone()),
                event: Some(SoundEvent::ApprovalRequired),
                pane_name: Some("two".to_string()),
            })
            .await
            .unwrap();
        drop(sender);
        task.await.unwrap();

        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                (first, Some(SoundEvent::AgentFinished)),
                (second, Some(SoundEvent::ApprovalRequired)),
            ]
        );
    }

    #[tokio::test]
    async fn preview_completion_follows_the_playback_receipt() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let player = Arc::new(RecordingPlayer {
            calls: Arc::clone(&calls),
        });
        let (sender, task) = spawn(player, crate::execution::test_general_client());
        let (reply, mut events) = crate::ipc::DirectEventSender::channel(2);
        sender
            .try_send_with_preview_reply(
                PlaybackRequest {
                    settings: test_settings(SoundSettings::default()),
                    event: None,
                    pane_name: None,
                },
                Some(reply),
            )
            .unwrap();

        assert_eq!(
            events.recv().await,
            Some(ilium_ipc::ServerEvent::SoundPreviewCompleted { succeeded: true })
        );
        drop(sender);
        task.await.unwrap();
        assert_eq!(calls.lock().unwrap().len(), 1);
    }

    struct FailingPreviewPlayer;

    impl SoundPlayer for FailingPreviewPlayer {
        fn play(
            &self,
            _settings: &SoundSettings,
            _event: Option<SoundEvent>,
        ) -> Result<(), ilium_sound::SoundError> {
            Err(ilium_sound::SoundError::MissingFile(PathBuf::from(
                "missing-preview.ogg",
            )))
        }
    }

    #[tokio::test]
    async fn preview_failure_is_sent_after_the_playback_receipt_fails() {
        let (sender, task) = spawn(
            Arc::new(FailingPreviewPlayer),
            crate::execution::test_general_client(),
        );
        let (reply, mut events) = crate::ipc::DirectEventSender::channel(2);
        sender
            .try_send_with_preview_reply(
                PlaybackRequest {
                    settings: test_settings(SoundSettings::default()),
                    event: None,
                    pane_name: None,
                },
                Some(reply),
            )
            .unwrap();

        assert_eq!(
            events.recv().await,
            Some(ilium_ipc::ServerEvent::SoundPreviewCompleted { succeeded: false })
        );
        drop(sender);
        task.await.unwrap();
    }

    struct PreparedRecordingPlayer {
        calls: Arc<Mutex<Vec<(Option<SoundEvent>, Option<Vec<u8>>)>>>,
    }

    impl SoundPlayer for PreparedRecordingPlayer {
        fn play(
            &self,
            settings: &SoundSettings,
            event: Option<SoundEvent>,
        ) -> Result<(), ilium_sound::SoundError> {
            self.play_prepared(settings, event, None)
        }

        fn prepare_generated(
            &self,
            settings: &SoundSettings,
        ) -> Result<Option<Vec<u8>>, ilium_sound::SoundError> {
            assert!(
                std::thread::current()
                    .name()
                    .is_some_and(|name| name.starts_with("ilium-exec-cpu-")),
                "generated sound preparation must run on the bounded CPU bank"
            );
            Ok(Some(ilium_sound::render_wav(&settings.design)))
        }

        fn play_prepared(
            &self,
            _settings: &SoundSettings,
            event: Option<SoundEvent>,
            generated_wav: Option<&[u8]>,
        ) -> Result<(), ilium_sound::SoundError> {
            assert!(
                std::thread::current()
                    .name()
                    .is_some_and(|name| name.starts_with("ilium-exec-io-")),
                "generated sound playback must remain on the bounded I/O bank"
            );
            self.calls
                .lock()
                .unwrap()
                .push((event, generated_wav.map(<[u8]>::to_vec)));
            Ok(())
        }
    }

    #[tokio::test]
    async fn generated_sound_is_prepared_before_ordered_playback() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let player = Arc::new(PreparedRecordingPlayer {
            calls: Arc::clone(&calls),
        });
        let (sender, task) = spawn(player, crate::execution::test_general_client());
        let settings = SoundSettings {
            source: ilium_sound::SoundSourceKind::Generated,
            ..SoundSettings::default()
        };

        sender
            .send(PlaybackRequest {
                settings: test_settings(settings),
                event: Some(SoundEvent::ApprovalRequired),
                pane_name: Some("prepared-generated".to_string()),
            })
            .await
            .unwrap();
        drop(sender);
        task.await.unwrap();

        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, Some(SoundEvent::ApprovalRequired));
        let wav = calls[0].1.as_deref().expect("generated sound is prepared");
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert!(wav.len() <= 44 + ilium_sound::synthesis::MAX_PCM_SAMPLES * 2);
    }

    #[tokio::test]
    async fn config_watcher_updates_an_already_running_server() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("config.toml");
        std::fs::write(&config_path, "[sound]\nsource = \"system_beep\"\n").unwrap();
        let (sound_requests, playback_task) = spawn(
            Arc::new(NoopSoundPlayer),
            crate::execution::test_general_client(),
        );
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "watcher-test".to_string(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(SoundSettings::default()),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
        }));
        let watcher = spawn_config_watcher_with_interval(
            Arc::clone(&state),
            config_path.clone(),
            Duration::from_millis(20),
        );

        // Let the watcher record the initial config before testing a subsequent edit.
        tokio::time::sleep(Duration::from_millis(30)).await;

        std::fs::write(
            &config_path,
            "[sound.events]\napproval_required = true\n[session]\nbackups_enabled = false\n",
        )
        .unwrap();
        let updated = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if state.sound_settings.read().await.events.approval_required
                    && !state.session_backups_enabled()
                {
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

    struct BlockedPlayback {
        started: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl SoundPlayer for BlockedPlayback {
        fn play(
            &self,
            _settings: &SoundSettings,
            _event: Option<SoundEvent>,
        ) -> Result<(), ilium_sound::SoundError> {
            self.started
                .lock()
                .unwrap()
                .take()
                .unwrap()
                .send(())
                .unwrap();
            let _ = self
                .release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(60));
            Ok(())
        }
    }
    struct ReleaseBlockedPlayback(Option<std::sync::mpsc::SyncSender<()>>);
    impl Drop for ReleaseBlockedPlayback {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }
    #[tokio::test]
    async fn playback_callback_remains_admitted_after_actor_cancellation() {
        let execution = crate::execution::ServerExecution::start().unwrap();
        let quota = execution.quota_group();
        let (started_sender, started) = tokio::sync::oneshot::channel();
        let (release_sender, release) = std::sync::mpsc::sync_channel(1);
        let release_guard = ReleaseBlockedPlayback(Some(release_sender));
        let player = Arc::new(BlockedPlayback {
            started: Mutex::new(Some(started_sender)),
            release: Mutex::new(release),
        });
        let (sender, actor) = spawn(player, execution.client.clone());
        sender
            .send(PlaybackRequest {
                settings: SharedSoundSettings::try_new(&execution.client, SoundSettings::default())
                    .unwrap(),
                event: Some(SoundEvent::AgentFinished),
                pane_name: Some("blocked-custody".to_owned()),
            })
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(30), started)
            .await
            .unwrap()
            .unwrap();
        let while_blocked = quota.snapshot();
        actor.abort();
        assert!(actor.await.unwrap_err().is_cancelled());
        let after_abort = quota.snapshot();
        drop(sender);
        drop(release_guard);
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let notification = execution.client.completion_notification();
                let notified = notification.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if quota.snapshot().jobs == 0 {
                    break;
                }
                notified.await;
            }
        })
        .await
        .unwrap();
        assert!(
            while_blocked.jobs == 1 && after_abort.jobs == 1,
            "audio callback must remain admitted while native player is blocked: before={:?}, after={:?}",
            while_blocked,
            after_abort
        );
    }

    #[tokio::test]
    async fn queue_refusal_retains_original_bytes_until_the_error_is_disposed() {
        let execution = crate::execution::ServerExecution::start().unwrap();
        let quota = execution.quota_group();
        let baseline = quota.snapshot().worker_bytes;
        let (sender, mut receiver) = mpsc::channel(1);
        let sender = PlaybackSender {
            sender,
            execution: execution.client.clone(),
        };
        let request = || PlaybackRequest {
            settings: SharedSoundSettings::try_new(&execution.client, SoundSettings::default())
                .unwrap(),
            event: Some(SoundEvent::TaskSucceeded),
            pane_name: Some("first".to_owned()),
        };
        sender.try_send(request()).unwrap();
        let first_charge = quota.snapshot().worker_bytes;
        assert!(first_charge > baseline);
        let mut name = String::with_capacity(8192);
        name.push_str("retained-original");
        let original_capacity = name.capacity();
        let mut second = request();
        second.pane_name = Some(name);
        let refused = sender.try_send(second).unwrap_err();
        match &refused {
            PlaybackSendError::Queue(mpsc::error::TrySendError::Full(original)) => {
                assert_eq!(
                    original.request.pane_name.as_deref(),
                    Some("retained-original")
                );
                assert_eq!(
                    original.request.pane_name.as_ref().unwrap().capacity(),
                    original_capacity
                );
            }
            error => panic!("unexpected refusal: {error:?}"),
        }
        assert!(quota.snapshot().worker_bytes > first_charge);
        drop(refused);
        assert_eq!(quota.snapshot().worker_bytes, first_charge);
        let queued = receiver.try_recv().unwrap();
        assert_eq!(queued.request.pane_name.as_deref(), Some("first"));
        drop(queued);
        assert_eq!(quota.snapshot().worker_bytes, baseline);
    }
    #[tokio::test]
    async fn closed_admission_returns_the_exact_original_request() {
        let execution = crate::execution::ServerExecution::start().unwrap();
        let (sender, _receiver) = mpsc::channel(1);
        let sender = PlaybackSender {
            sender,
            execution: execution.client.clone(),
        };
        let settings =
            SharedSoundSettings::try_new(&execution.client, SoundSettings::default()).unwrap();
        execution.request_shutdown();
        let mut name = String::with_capacity(4096);
        name.push_str("original-notification");
        let capacity = name.capacity();
        let request = PlaybackRequest {
            settings,
            event: Some(SoundEvent::AgentFinished),
            pane_name: Some(name),
        };
        match sender.try_send(request) {
            Err(PlaybackSendError::Admission {
                reason: ilium_execution::RejectReason::Closed,
                _request: original,
            }) => {
                assert_eq!(original.event, Some(SoundEvent::AgentFinished));
                assert_eq!(original.pane_name.as_deref(), Some("original-notification"));
                assert_eq!(original.pane_name.unwrap().capacity(), capacity);
            }
            error => panic!("closed bank did not return the original request: {error:?}"),
        }
    }

    #[test]
    fn preparation_refusal_preserves_the_borrowed_settings_and_event() {
        let execution = crate::execution::ServerExecution::start().unwrap();
        let quota = execution.quota_group();
        let settings =
            SharedSoundSettings::try_new(&execution.client, SoundSettings::default()).unwrap();
        let (sender, _receiver) = mpsc::channel(1);
        let sender = PlaybackSender {
            sender,
            execution: execution.client.clone(),
        };
        let snapshot = quota.snapshot();
        let remaining = snapshot.limits.worker_bytes - snapshot.worker_bytes;
        let full = execution.client.try_reserve_storage(remaining).unwrap();
        let event = Some(SoundEvent::TaskSucceeded);
        assert!(matches!(
            sender.prepare(&settings, event, Some("borrowed-pane-name")),
            Err(ilium_execution::RejectReason::WorkerBytes)
        ));
        assert_eq!(Arc::strong_count(&settings), 1);
        assert_eq!(event, Some(SoundEvent::TaskSucceeded));
        drop(full);
        let prepared = sender
            .prepare(&settings, event, Some("borrowed-pane-name"))
            .unwrap();
        assert!(Arc::ptr_eq(&prepared.settings, &settings));
        assert_eq!(prepared.event, event);
        assert_eq!(prepared.pane_name.as_deref(), Some("borrowed-pane-name"));
    }
}
