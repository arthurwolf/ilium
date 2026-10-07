//! Worker-local procedural plugin adapter. Package evaluation runs only in the
//! protected helper; catalogue descriptors never become frame identities.
mod audio_capture_owner;
use super::{
    worker::{RenderRequest, SnapshotCell},
    AnimationSettings,
};
use crate::{
    animation_plugins::{
        review_bridge::{ReviewBridge, ReviewPhase},
        review_controller::{self, IntentOutcome, PickAudioSelection, PickSelection},
        PluginCatalogue,
    },
    filesystem::{
        plugin_permission_controller::{
            ActivationUpdate, PermissionCancellation, PluginPermissionController,
        },
        plugin_permissions::PluginPermissionFiles,
    },
};
use audio_capture_owner::AudioOwner;
use ilium_ambient::resources::{AmbientResources, WorkerCost};
#[cfg(test)]
use ilium_animation_js::surface::NoNativeRenderer;
use ilium_animation_js::{
    clip_chunk_store::ClipChunkStore,
    clock::AnimationClock,
    engine::HostRequest,
    engine::{ArraySpec, CreateState, EngineLimits, TypedArrayKind},
    helper::HelperLimits,
    http::SystemDns,
    manifest::AnimationMode,
    native_asset_host::NativeAssetHost,
    native_audio::{AudioSourceSelection, RetainedAudioSnapshot},
    native_audio_capture::QualifiedCaptureBinding,
    native_compute_host::NativeComputeHost,
    native_draw::DrawLimits,
    native_draw_host::NativeDrawHost,
    native_frame_inputs::{
        CachedInputProvider, ClockObservation, LocationObservation, NativeFrameInputs,
        OcclusionObservation, PointerObservation,
    },
    native_http_host::{HttpEvent, HttpObservation, NativeHttpHost},
    native_image_host::NativeImageHost,
    native_media::MediaLimits,
    native_presentation_host::NativePresentationHost,
    native_source_host::{
        NativeSourceFeedHost, NativeSourceHost, PreparedFeedRegistration, ProjectedFeedDescriptor,
        SourceActorEnvironment, SourceActorLimits, SourceCadence, SourceCall, SourceCompletion,
        SourceEvent, SourceFailure, SourceFeedCompletion, SourceFeedEvent,
    },
    native_storage::SelectedStorage,
    native_task_host::NativeTaskHost,
    native_video_host::NativeVideoHost,
    native_world_host::NativeWorldHost,
    permissions::{Capability, Ceiling, Invalidation, Selection},
    replay::{
        ClipSpec, ClipSpecification, FrozenEvidence, FrozenInputs, Playback, PlaybackMode,
        PlaybackSettings, Preparation, ReplayAuthority, ReplayAuthorization, ReplayCache,
        ReplayClip, ReplayLimits, ReplayPlayer, ReplayPreparationOwner,
    },
    runtime::VerifiedPreparation,
    runtime::{InstancePreparation, PackageInstance, RetainedFrameAuthority},
    sources::SourceClock,
    surface::{Data, Format, FrameMeta, Mode, Planes, Shape, Snapshot, Surface},
};
use ilium_execution::{
    Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, QuotaGroup, Receipt, Retention,
    StorageAdmission,
};
use ilium_platform::owned_worker::StopToken;
use ilium_platform::{animation_files::PinnedDirectory, secure_fs::NoFollowDirectory};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    sync::{Arc, Mutex, OnceLock, TryLockError},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const ARCHIVE_BYTES: usize = 32 * 1024 * 1024;
static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);
static NEXT_SOURCE_FEED: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginFrameIdentity {
    pub package_id: String,
    pub package_digest: String,
    pub verified_ilium: bool,
    pub instance_id: u64,
    pub revision: u64,
    pub plan_generation: u64,
    pub authorization_epoch: u64,
}
pub(super) struct PluginFrame {
    pub identity: PluginFrameIdentity,
    pub authority: RetainedFrameAuthority,
    pub cells: Vec<SnapshotCell>,
    pub frames_per_second: u32,
    pub resident_bytes: usize,
    pub replay: Option<ilium_animation_js::replay::PlaybackLease>,
}
struct Presentation {
    surface: Surface,
    clock: AnimationClock,
    compute: NativeComputeHost,
    http: NativeHttpHost,
    sources: NativeSourceOwner,
    drawing: NativeDrawHost,
    images: NativeImageHost,
    assets: NativeAssetHost,
    video: NativeVideoHost,
    inputs: NativeFrameInputs,
    audio: AudioOwner,
    tasks: NativeTaskHost,
    live_mode: bool,
    /// Live world and terminal-receipt services are created lazily only when
    /// the package actually requests their API.  Keeping them out of the
    /// normal presentation path avoids opening world/storage state for the
    /// many packages that only draw pixels.
    world: Option<NativeWorldHost>,
    world_resources: ilium_ambient::resources::AmbientResources,
    world_epoch: u64,
    world_limits: EngineLimits,
    saved_context: Option<ilium_animation_js::native_world_host::NativeSavedContext>,
    presentation: Option<NativePresentationHost>,
    creation: CreateState,
    unhandled: Option<HostRequest>,
    undispatched: Vec<HostRequest>,
    _surface_storage: StorageAdmission,
    _request_storage: StorageAdmission,
}
enum SourceTerminal {
    Complete {
        owner: SourceCompletion,
        state: SourcePublishState,
        feed_id: Option<String>,
        feed_descriptor: Option<ProjectedFeedDescriptor>,
        feed_registration: Option<PreparedFeedRegistration>,
    },
    Failed {
        owner: SourceFailure,
        state: SourcePublishState,
    },
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum SourcePublishState {
    Pending,
    Acknowledged,
    Uncertain,
}
struct SourceFeedTerminal {
    completion: SourceFeedCompletion,
    state: SourcePublishState,
}
struct SourceFeedRecord {
    host: NativeSourceFeedHost,
    descriptor: ProjectedFeedDescriptor,
    terminal: Option<SourceFeedTerminal>,
    closing: Option<HostRequest>,
    closing_state: SourcePublishState,
}
struct NativeSourceOwner {
    active: BTreeMap<u64, NativeSourceHost>,
    terminal: BTreeMap<u64, SourceTerminal>,
    feeds: BTreeMap<String, SourceFeedRecord>,
    client: ilium_execution::Client,
    cadence: Arc<SourceCadence>,
    closed: bool,
    _metadata: StorageAdmission,
}
fn source_clock() -> Result<SourceClock, String> {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    let monotonic_ms = ORIGIN.get_or_init(Instant::now).elapsed().as_millis();
    let monotonic_ms = u64::try_from(monotonic_ms)
        .map_err(|_| "Native source monotonic clock range".to_owned())?;
    let epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Native source civil clock precedes Unix epoch".to_owned())?
        .as_millis();
    let epoch_ms =
        i64::try_from(epoch_ms).map_err(|_| "Native source civil clock range".to_owned())?;
    Ok(SourceClock {
        monotonic_ms,
        epoch_ms,
    })
}
impl NativeSourceOwner {
    fn new(client: ilium_execution::Client, quota: QuotaGroup) -> Result<Self, String> {
        if !client.quota_group().shares_root(&quota) {
            return Err("Foreign native source client".into());
        }
        let metadata = quota
            .reserve_external_storage(128 * 1024)
            .map_err(|error| format!("Native source owner admission: {error:?}"))?;
        let cadence = crate::execution::native_source_cadence()?;
        if !cadence.shares_root(&quota) {
            return Err("Foreign native source cadence".into());
        }
        Ok(Self {
            active: BTreeMap::new(),
            terminal: BTreeMap::new(),
            feeds: BTreeMap::new(),
            client,
            cadence,
            closed: false,
            _metadata: metadata,
        })
    }
    fn dispatch(
        &mut self,
        instance: &mut PackageInstance,
        request: HostRequest,
        drawing: &mut NativeDrawHost,
    ) -> ilium_animation_js::error::Result<Option<HostRequest>> {
        if matches!(
            request.method.as_str(),
            "sources.series.close"
                | "sources.earthquakes.close"
                | "sources.aircraft.close"
                | "sources.boats.close"
                | "sources.chess.close"
                | "sources.weather.close"
        ) {
            instance.authorize_native_source_feed_close(&request)?;
            let fields = request.payload.metadata().as_object().ok_or_else(|| {
                ilium_animation_js::error::AnimationError::Runtime(
                    "native feed close payload".into(),
                )
            })?;
            let kind = request.method.strip_suffix(".close").ok_or_else(|| {
                ilium_animation_js::error::AnimationError::Runtime(
                    "native feed close method".into(),
                )
            })?;
            let id = fields
                .get("id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    ilium_animation_js::error::AnimationError::Runtime(
                        "native feed close id".into(),
                    )
                })?;
            if fields.len() != 2
                || fields.get("kind").and_then(serde_json::Value::as_str) != Some(kind)
                || !id.starts_with("source-feed-")
                || id.len() > 128
            {
                return Err(ilium_animation_js::error::AnimationError::PermissionDenied(
                    "native feed close identity".into(),
                ));
            }
            let record = self.feeds.get_mut(id).ok_or_else(|| {
                ilium_animation_js::error::AnimationError::PermissionDenied(
                    "native feed close has no original owner".into(),
                )
            })?;
            if record.descriptor.metadata()["kind"] != kind || record.closing.is_some() {
                return Err(ilium_animation_js::error::AnimationError::PermissionDenied(
                    "native feed close kind or pending control".into(),
                ));
            }
            record.host.cancel();
            record.closing = Some(request);
            record.closing_state = SourcePublishState::Pending;
            self.publish_feed_terminals(instance, drawing)
                .map_err(ilium_animation_js::error::AnimationError::Runtime)?;
            self.publish_feed_closes(instance)?;
            return Ok(None);
        }
        if request.method == "media.images.close" {
            let id = request
                .payload
                .metadata()
                .get("id")
                .and_then(serde_json::Value::as_str);
            if let Some(id) = id.filter(|id| id.starts_with("source-image-")) {
                if !drawing.owns_source_image(id) {
                    return Err(ilium_animation_js::error::AnimationError::PermissionDenied(
                        "source image close has no original native allocation".into(),
                    ));
                }
                let value = ilium_animation_js::engine::ServiceValue::copy_from_host(
                    &json!({"ok":true,"value":null}),
                    &[],
                    &BTreeMap::new(),
                    instance.engine_limits(),
                    self.client.quota_group(),
                )?;
                if instance.complete_native_source_image_close(&request, value)?
                    == ilium_animation_js::engine::CompletionState::Delivered
                {
                    let release = drawing.release_source_image(id);
                    for record in self.feeds.values_mut() {
                        if let Err(error) = record.descriptor.image_closed(id) {
                            drawing.revoke();
                            return Err(error);
                        }
                    }
                    if let Err(error) = release {
                        drawing.revoke();
                        return Err(error);
                    }
                }
                return Ok(None);
            }
            return Ok(Some(request));
        }
        if !SourceCall::recognized(&request.method) {
            return Ok(Some(request));
        }
        if self.closed
            || self.active.len() + self.terminal.len() + self.feeds.len() >= 32
            || self.active.contains_key(&request.id)
            || self.terminal.contains_key(&request.id)
        {
            return Err(ilium_animation_js::error::AnimationError::Budget(
                "native source owner registry closed or full".into(),
            ));
        }
        let clock = source_clock().map_err(ilium_animation_js::error::AnimationError::Runtime)?;
        let id = request.id;
        let host = NativeSourceHost::start(
            instance,
            request,
            clock,
            SourceActorEnvironment {
                client: self.client.clone(),
                dns: Arc::new(SystemDns),
                cadence: Arc::clone(&self.cadence),
                limits: SourceActorLimits {
                    pages: 256,
                    encoded_bytes: 256 * 1024 * 1024,
                    media: MediaLimits::default(),
                },
                credentials: None,
            },
        )?;
        self.active.insert(id, host);
        Ok(None)
    }
    fn on_completion_wake(
        &mut self,
        instance: &mut PackageInstance,
        drawing: &mut NativeDrawHost,
    ) -> Result<(), String> {
        let mut first_error = None;
        let ids: Vec<u64> = self.active.keys().copied().collect();
        for id in ids {
            let event = match self.active.get_mut(&id) {
                Some(host) => match host.on_completion_wake(instance) {
                    Ok(event) => event,
                    Err(error) => {
                        first_error.get_or_insert_with(|| error.to_string());
                        continue;
                    }
                },
                None => {
                    first_error
                        .get_or_insert_with(|| "Native source actor identity changed".to_owned());
                    continue;
                }
            };
            match event {
                Some(SourceEvent::Complete(completion)) => {
                    self.active.remove(&id);
                    let feed_id = if completion.is_feed() {
                        let number = NEXT_SOURCE_FEED
                            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                                value.checked_add(1)
                            })
                            .map_err(|_| "Native source feed identity exhausted".to_owned())?;
                        Some(format!("source-feed-{number}"))
                    } else {
                        None
                    };
                    self.terminal.insert(
                        id,
                        SourceTerminal::Complete {
                            owner: completion,
                            state: SourcePublishState::Pending,
                            feed_id,
                            feed_descriptor: None,
                            feed_registration: None,
                        },
                    );
                }
                Some(SourceEvent::Failed(failure)) => {
                    self.active.remove(&id);
                    self.terminal.insert(
                        id,
                        SourceTerminal::Failed {
                            owner: failure,
                            state: SourcePublishState::Pending,
                        },
                    );
                }
                Some(SourceEvent::Lost) => {
                    first_error.get_or_insert_with(|| {
                        "Native source receipt lost; physical retirement unproven".to_owned()
                    });
                }
                None => {}
            }
        }
        let ids: Vec<u64> = self.terminal.keys().copied().collect();
        for id in ids {
            let Some(mut terminal) = self.terminal.remove(&id) else {
                first_error
                    .get_or_insert_with(|| "Native source terminal identity changed".to_owned());
                continue;
            };
            match Self::publish_terminal(&mut terminal, instance, drawing, &self.client) {
                Ok(()) => {
                    if let SourceTerminal::Complete {
                        owner,
                        feed_id: Some(feed_id),
                        feed_descriptor: Some(descriptor),
                        feed_registration: Some(prepared),
                        ..
                    } = terminal
                    {
                        let host = NativeSourceFeedHost::from_delivered(
                            owner.into_feed_run(),
                            self.client.clone(),
                            Arc::new(SystemDns),
                            None,
                            prepared,
                        );
                        self.feeds.insert(
                            feed_id,
                            SourceFeedRecord {
                                host,
                                descriptor,
                                terminal: None,
                                closing: None,
                                closing_state: SourcePublishState::Pending,
                            },
                        );
                    }
                }
                Err(error) => {
                    self.terminal.insert(id, terminal);
                    first_error.get_or_insert(error);
                }
            }
        }
        let feed_ids: Vec<String> = self.feeds.keys().cloned().collect();
        for id in feed_ids {
            let Some(record) = self.feeds.get_mut(&id) else {
                continue;
            };
            match record.host.on_completion_wake(instance) {
                Ok(Some(SourceFeedEvent::Complete)) => {
                    let completion = match record.host.take_completion() {
                        Ok(completion) => completion,
                        Err(error) => {
                            first_error.get_or_insert_with(|| error.to_string());
                            continue;
                        }
                    };
                    if record.terminal.is_some() {
                        first_error
                            .get_or_insert_with(|| "Native feed terminal already held".to_owned());
                    } else {
                        record.terminal = Some(SourceFeedTerminal {
                            completion,
                            state: SourcePublishState::Pending,
                        });
                    }
                }
                Ok(Some(SourceFeedEvent::Failed(code))) => {
                    if code != "source_feed_cancelled" {
                        if record.closing.is_none() {
                            if let Err(error) = record.descriptor.mark_error() {
                                first_error.get_or_insert_with(|| error.to_string());
                            }
                        }
                        first_error.get_or_insert_with(|| format!("Native feed worker: {code}"));
                    }
                }
                Ok(Some(SourceFeedEvent::Lost)) => {
                    first_error
                        .get_or_insert_with(|| "Native feed original receipt lost".to_owned());
                }
                Ok(None) => {}
                Err(error) => {
                    first_error.get_or_insert_with(|| error.to_string());
                }
            }
        }
        if let Err(error) = self.publish_feed_terminals(instance, drawing) {
            first_error.get_or_insert(error);
        }
        if let Err(error) = self.publish_feed_closes(instance) {
            first_error.get_or_insert_with(|| error.to_string());
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    fn publish_feed_terminals(
        &mut self,
        instance: &mut PackageInstance,
        drawing: &mut NativeDrawHost,
    ) -> Result<(), String> {
        let ids: Vec<String> = self.feeds.keys().cloned().collect();
        for id in ids {
            let Some(record) = self.feeds.get_mut(&id) else {
                continue;
            };
            let Some(mut terminal) = record.terminal.take() else {
                continue;
            };
            if record.closing.is_some() {
                if let Err(error) = terminal.completion.settle(instance) {
                    record.terminal = Some(terminal);
                    return Err(error.to_string());
                }
                record
                    .host
                    .resume(terminal.completion)
                    .map_err(|error| error.to_string())?;
                record.host.cancel();
                continue;
            }
            if terminal.state == SourcePublishState::Uncertain {
                record.terminal = Some(terminal);
                return Err("Native feed snapshot publication outcome uncertain".into());
            }
            if terminal.state == SourcePublishState::Pending {
                let revision = record.descriptor.metadata()["revision"]
                    .as_u64()
                    .and_then(|value| value.checked_add(1))
                    .ok_or_else(|| "Native feed service revision exhausted".to_owned())?;
                let proposed = terminal
                    .completion
                    .project(instance, drawing, &id, revision)
                    .map_err(|error| error.to_string());
                let mut proposed = match proposed {
                    Ok(value) => Some(value),
                    Err(error) => {
                        record.terminal = Some(terminal);
                        return Err(error);
                    }
                };
                terminal.state = SourcePublishState::Uncertain;
                let delivered = terminal.completion.deliver(instance, || {
                    // The synchronous broker closure executes exactly once after
                    // its current-grant check, with no helper JS or worker wait.
                    let value = proposed.take().expect("one feed descriptor publication");
                    record.descriptor = value;
                });
                if let Err(error) = delivered {
                    if let Some(value) = proposed.take() {
                        for image in value.imported().iter().rev() {
                            drawing
                                .release_source_image(image)
                                .map_err(|error| error.to_string())?;
                        }
                    }
                    terminal.state = SourcePublishState::Pending;
                    record.terminal = Some(terminal);
                    return Err(error.to_string());
                }
                terminal.state = SourcePublishState::Acknowledged;
            }
            if let Err(error) = terminal.completion.settle(instance) {
                record.terminal = Some(terminal);
                return Err(error.to_string());
            }
            record
                .host
                .resume(terminal.completion)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
    fn publish_feed_closes(
        &mut self,
        instance: &mut PackageInstance,
    ) -> ilium_animation_js::error::Result<()> {
        let ids: Vec<String> = self.feeds.keys().cloned().collect();
        for id in ids {
            let Some(record) = self.feeds.get_mut(&id) else {
                continue;
            };
            if !record.host.is_drained() {
                continue;
            }
            let Some(request) = record.closing.as_ref() else {
                continue;
            };
            if record.closing_state == SourcePublishState::Uncertain {
                return Err(ilium_animation_js::error::AnimationError::Runtime(
                    "native feed close publication outcome uncertain".into(),
                ));
            }
            let revision = record.descriptor.metadata()["revision"]
                .as_u64()
                .and_then(|value| value.checked_add(1))
                .ok_or_else(|| {
                    ilium_animation_js::error::AnimationError::Runtime(
                        "source close revision exhausted".into(),
                    )
                })?;
            let descriptor = json!({"id":id,"kind":record.descriptor.metadata()["kind"],"revision":revision,"status":{"state":"closed"}});
            let value = ilium_animation_js::engine::ServiceValue::copy_from_host(
                &json!({"ok":true,"value":descriptor}),
                &[],
                &BTreeMap::new(),
                instance.engine_limits(),
                self.client.quota_group(),
            )?;
            record.closing_state = SourcePublishState::Uncertain;
            let _state = instance.complete_native_source_feed_close(request, value)?;
            // Every returned completion state is terminal for this request.
            // Only a transport error leaves publication outcome uncertain.
            self.feeds.remove(&id);
        }
        Ok(())
    }
    fn begin_due(&mut self, instance: &PackageInstance) -> Result<(), String> {
        let clock = source_clock()?;
        for record in self.feeds.values_mut() {
            if record.closing.is_none() && record.terminal.is_none() {
                if let Err(error) = record.host.begin_due(instance, clock) {
                    record
                        .descriptor
                        .mark_error()
                        .map_err(|error| error.to_string())?;
                    tracing::debug!(%error, "Native source feed refresh admission refused");
                }
            }
        }
        Ok(())
    }
    fn snapshots(
        &self,
        instance: &mut PackageInstance,
    ) -> ilium_animation_js::error::Result<ilium_animation_js::engine::ServiceValue> {
        let bound = self
            .feeds
            .values()
            .filter(|record| record.closing.is_none())
            .try_fold(64 * 1024usize, |total, record| {
                total.checked_add(record.descriptor.metadata_bound())
            })
            .ok_or_else(|| {
                ilium_animation_js::error::AnimationError::Budget("source seed copy bound".into())
            })?;
        let _scratch = self
            .client
            .quota_group()
            .reserve_external_storage(bound)
            .map_err(|_| {
                ilium_animation_js::error::AnimationError::Budget(
                    "source seed copy admission".into(),
                )
            })?;
        let values: Vec<_> = self
            .feeds
            .values()
            .filter(|record| record.closing.is_none())
            .map(|record| record.descriptor.metadata().clone())
            .collect();
        instance.copy_native_source_feed_snapshots(&values)
    }
    fn publish_terminal(
        terminal: &mut SourceTerminal,
        instance: &mut PackageInstance,
        drawing: &mut NativeDrawHost,
        client: &ilium_execution::Client,
    ) -> Result<(), String> {
        match terminal {
            SourceTerminal::Complete {
                owner,
                state,
                feed_id,
                feed_descriptor,
                feed_registration,
            } => {
                if *state == SourcePublishState::Uncertain {
                    return Err("Native source helper copy outcome is uncertain".into());
                }
                if *state == SourcePublishState::Pending {
                    let (value, imported) = if let Some(id) = feed_id {
                        let (value, descriptor) = owner
                            .copy_feed_open_result(instance, drawing, id)
                            .map_err(|error| error.to_string())?;
                        let imported = descriptor.imported().to_vec();
                        let stop = StopToken::default();
                        let prepared =
                            match owner.prepare_feed_transfer(instance, client, stop.clone()) {
                                Ok(prepared) => prepared,
                                Err(error) => {
                                    for image in imported.iter().rev() {
                                        drawing
                                            .release_source_image(image)
                                            .map_err(|error| error.to_string())?;
                                    }
                                    return Err(error.to_string());
                                }
                            };
                        *feed_descriptor = Some(descriptor);
                        *feed_registration = Some(prepared);
                        (value, imported)
                    } else {
                        owner
                            .copy_operation_result(instance, drawing)
                            .map_err(|error| error.to_string())?
                    };
                    *state = SourcePublishState::Uncertain;
                    let outcome = owner.publish(instance, value);
                    let delivered = outcome.as_ref().is_ok_and(|state| {
                        *state == ilium_animation_js::engine::CompletionState::Delivered
                    });
                    if !delivered {
                        for key in imported.iter().rev() {
                            if let Err(error) = drawing.release_source_image(key) {
                                drawing.revoke();
                                return Err(error.to_string());
                            }
                        }
                        *feed_descriptor = None;
                        if let Some(prepared) = feed_registration {
                            prepared.stop();
                        }
                    }
                    outcome.map_err(|error| error.to_string())?;
                    *state = SourcePublishState::Acknowledged;
                }
                owner.settle(instance).map_err(|error| error.to_string())
            }
            SourceTerminal::Failed { owner, state } => {
                if *state == SourcePublishState::Uncertain {
                    return Err("Native source error copy outcome is uncertain".into());
                }
                if *state == SourcePublishState::Pending {
                    let value = owner
                        .copy_error(instance)
                        .map_err(|error| error.to_string())?;
                    *state = SourcePublishState::Uncertain;
                    owner
                        .publish_error(instance, value)
                        .map_err(|error| error.to_string())?;
                    *state = SourcePublishState::Acknowledged;
                }
                owner.settle(instance).map_err(|error| error.to_string())
            }
        }
    }
    fn cancel_all(&mut self) {
        self.closed = true;
        for host in self.active.values_mut() {
            host.cancel();
        }
        for terminal in self.terminal.values() {
            match terminal {
                SourceTerminal::Complete { owner, .. } => owner.request().stop_token().stop(),
                SourceTerminal::Failed { owner, .. } => owner.request().stop_token().stop(),
            }
        }
        for record in self.feeds.values_mut() {
            record.host.cancel();
            if let Some(request) = &record.closing {
                request.stop_token().stop();
            }
        }
    }
    fn collect_retirement_on_wake(&mut self, instance: &mut PackageInstance) -> Result<(), String> {
        self.cancel_all();
        let mut first_error = None;
        let ids: Vec<u64> = self.active.keys().copied().collect();
        for id in ids {
            let event = match self.active.get_mut(&id) {
                Some(host) => match host.on_completion_wake(instance) {
                    Ok(event) => event,
                    Err(error) => {
                        first_error.get_or_insert_with(|| error.to_string());
                        continue;
                    }
                },
                None => {
                    first_error.get_or_insert_with(|| {
                        "Native source retirement identity changed".to_owned()
                    });
                    continue;
                }
            };
            match event {
                Some(SourceEvent::Complete(value)) => {
                    self.active.remove(&id);
                    self.terminal.insert(
                        id,
                        SourceTerminal::Complete {
                            owner: value,
                            state: SourcePublishState::Uncertain,
                            feed_id: None,
                            feed_descriptor: None,
                            feed_registration: None,
                        },
                    );
                }
                Some(SourceEvent::Failed(value)) => {
                    self.active.remove(&id);
                    self.terminal.insert(
                        id,
                        SourceTerminal::Failed {
                            owner: value,
                            state: SourcePublishState::Uncertain,
                        },
                    );
                }
                Some(SourceEvent::Lost) => {
                    first_error.get_or_insert_with(|| {
                        "Native source retirement lost physical receipt".to_owned()
                    });
                }
                None => {}
            }
        }
        for terminal in self.terminal.values() {
            if let Err(error) = match terminal {
                SourceTerminal::Complete { owner, .. } => owner.settle(instance),
                SourceTerminal::Failed { owner, .. } => owner.settle(instance),
            } {
                first_error.get_or_insert_with(|| error.to_string());
            }
        }
        for record in self.feeds.values_mut() {
            if let Some(terminal) = record.terminal.take() {
                if let Err(error) = terminal.completion.settle(instance) {
                    first_error.get_or_insert_with(|| error.to_string());
                    record.terminal = Some(terminal);
                } else if let Err(error) = record.host.resume(terminal.completion) {
                    first_error.get_or_insert_with(|| error.to_string());
                } else {
                    record.host.cancel();
                }
            }
            match record.host.on_completion_wake(instance) {
                Ok(Some(SourceFeedEvent::Complete)) => {
                    let value = match record.host.take_completion() {
                        Ok(value) => value,
                        Err(error) => {
                            first_error.get_or_insert_with(|| error.to_string());
                            continue;
                        }
                    };
                    if let Err(error) = value.settle(instance) {
                        first_error.get_or_insert_with(|| error.to_string());
                        record.terminal = Some(SourceFeedTerminal {
                            completion: value,
                            state: SourcePublishState::Uncertain,
                        });
                    } else if let Err(error) = record.host.resume(value) {
                        first_error.get_or_insert_with(|| error.to_string());
                    } else {
                        record.host.cancel();
                    }
                }
                Ok(Some(SourceFeedEvent::Lost)) => {
                    first_error
                        .get_or_insert_with(|| "Native feed retirement receipt lost".to_owned());
                }
                Ok(Some(SourceFeedEvent::Failed(_)) | None) => {}
                Err(error) => {
                    first_error.get_or_insert_with(|| error.to_string());
                }
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    fn is_drained(&self) -> bool {
        self.active.is_empty()
            && self.terminal.is_empty()
            && self
                .feeds
                .values()
                .all(|record| record.host.is_drained() && record.terminal.is_none())
    }
    fn release_terminal_after_helper_retirement(&mut self) {
        self.terminal.clear();
        self.feeds.retain(|_, record| !record.host.is_drained());
    }
}
struct PreparedSelection {
    verified: VerifiedPreparation,
    root: Arc<PinnedDirectory>,
    clip_root: Option<Arc<PinnedDirectory>>,
    state_root: Arc<PinnedDirectory>,
    request: RenderRequest,
}
struct SetupJob {
    request: RenderRequest,
    quota: QuotaGroup,
}
struct PickerJob {
    path: PathBuf,
    slot: String,
    selection: Selection,
    writable: bool,
    quota: QuotaGroup,
}
struct AudioPickerJob {
    endpoint: String,
    scope_device: String,
    capability: Capability,
    resources: AmbientResources,
}
impl Job for AudioPickerJob {
    type Output = QualifiedCaptureBinding;
    type Error = String;
    fn run(self, context: JobContext) -> Result<Self::Output, String> {
        stopped(&context.stop_token())?;
        let _enumeration_charge = self
            .resources
            .reserve_worker(WorkerCost {
                threads: 1,
                resident_bytes: 32 * 1024 * 1024,
            })
            .map_err(|error| format!("Audio enumeration admission: {error:?}"))?;
        let selected = ilium_platform::audio_backend::qualify_selected_pulse_source(
            &self.endpoint,
            self.capability == Capability::AudioLoopback,
        )
        .map_err(|error| format!("Selected audio endpoint unavailable: {error}"))?;
        stopped(&context.stop_token())?;
        let selector = match self.scope_device.as_str() {
            "loopback" => AudioSourceSelection::Loopback,
            "microphone" => AudioSourceSelection::Microphone,
            other => AudioSourceSelection::Device(other.to_owned()),
        };
        QualifiedCaptureBinding::from_selected_pulse(
            selector,
            selected,
            self.scope_device,
            self.capability,
            WorkerCost {
                threads: 1,
                resident_bytes: 32 * 1024 * 1024,
            },
        )
        .map_err(|error| error.to_string())
    }
}
impl Job for PickerJob {
    type Output = Arc<SelectedStorage>;
    type Error = String;
    fn run(self, context: JobContext) -> Result<Self::Output, String> {
        stopped(&context.stop_token())?;
        let selected = SelectedStorage::pin_user_path(
            &self.path,
            self.selection,
            self.slot,
            self.writable,
            self.quota,
        )
        .map_err(|error| error.to_string())?;
        stopped(&context.stop_token())?;
        Ok(Arc::new(selected))
    }
}
impl Job for SetupJob {
    type Output = PreparedSelection;
    type Error = String;
    fn run(self, context: JobContext) -> Result<Self::Output, String> {
        let stop = context.stop_token();
        let request = &self.request;
        let selection = request
            .settings
            .plugin
            .selected
            .as_ref()
            .ok_or_else(|| "No animation plugin selected".to_owned())?;
        stopped(&stop)?;
        let catalogue = PluginCatalogue::discover_with_stop(
            &crate::animation_plugins::package_directories()?,
            || stop.is_stopped(),
        );
        let descriptor = catalogue
            .entries
            .iter()
            .find(|descriptor| descriptor.manifest.id == selection.package_id)
            .ok_or_else(|| {
                "Selected animation package is not installed or has a conflicting ID".to_owned()
            })?;
        let selection = selection.validate(descriptor)?;
        let mut archive = File::open(&descriptor.archive_path)
            .map_err(|error| format!("Open animation package: {error}"))?;
        let size = usize::try_from(archive.metadata().map_err(|error| error.to_string())?.len())
            .map_err(|_| "Animation package size".to_owned())?;
        if size > ARCHIVE_BYTES {
            return Err("Animation archive exceeds 32 MiB".into());
        }
        let mut bytes = Vec::with_capacity(size + 1);
        archive
            .by_ref()
            .take((size + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() != size {
            return Err("Animation archive changed size during activation".into());
        }
        stopped(&stop)?;
        let verifier =
            ilium_animation_js::release::verifier().map_err(|error| error.to_string())?;
        let helper = helper_executable()?;
        let instance_id = NEXT_INSTANCE
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |id| id.checked_add(1))
            .map_err(|_| "Plugin instance identity exhausted".to_owned())?;
        let audio_selectable = selection.mode == AnimationMode::Live
            && ilium_platform::audio_backend::selected_pulse_capture_supported();
        let environment = json!({"viewport":{"cell_width":request.width,"cell_height":request.height,"dot_width":u32::from(request.width)*2,"dot_height":u32::from(request.height)*4,"revision":request.revision},"available":{"pointer":true,"audio":audio_selectable,"gpu":false,"location":true}});
        let verified = PackageInstance::verify(InstancePreparation {
            archive: &bytes,
            verifier: &verifier,
            helper_executable: &helper,
            trusted_bootstrap: ilium_animation_js::TRUSTED_BOOTSTRAP,
            settings: &selection.settings,
            mode: selection.mode.clone(),
            environment: &environment,
            host_policy: native_policy(&descriptor.manifest.capabilities, audio_selectable)?,
            instance_id,
            limits: HelperLimits::default(),
            quota: self.quota.clone(),
        })
        .map_err(|error| error.to_string())?;

        if verified.package().manifest().id != selection.package_id {
            return Err("Installed package identity changed during activation".into());
        }
        stopped(&stop)?;
        let directories = directories::ProjectDirs::from("", "", "ilium")
            .ok_or_else(|| "Ilium config directory unavailable".to_owned())?;
        let path = directories.config_dir().join("animation-permissions");
        ilium_platform::secure_fs::create_private_directory(&path)
            .map_err(|error| format!("Permission root creation: {error}"))?;
        let pinned = NoFollowDirectory::open_root(&path)
            .map_err(|error| format!("Permission root pin: {error}"))?;
        let root = Arc::new(
            PinnedDirectory::from_host(Arc::new(pinned)).map_err(|error| error.to_string())?,
        );
        let state_path = directories.data_dir().join("animation-state");
        ilium_platform::secure_fs::create_private_directory(&state_path)
            .map_err(|error| format!("Animation state root creation: {error}"))?;
        let state_root = Arc::new(
            PinnedDirectory::from_host(Arc::new(
                NoFollowDirectory::open_root(&state_path)
                    .map_err(|error| format!("Animation state root pin: {error}"))?,
            ))
            .map_err(|error| error.to_string())?,
        );
        let clip_root = if selection.mode == AnimationMode::PreRendered {
            let path = directories.cache_dir().join("animation-clips");
            ilium_platform::secure_fs::create_private_directory(&path)
                .map_err(|error| format!("Clip cache root creation: {error}"))?;
            let pinned = NoFollowDirectory::open_root(&path)
                .map_err(|error| format!("Clip cache root pin: {error}"))?;
            Some(Arc::new(
                PinnedDirectory::from_host(Arc::new(pinned)).map_err(|error| error.to_string())?,
            ))
        } else {
            None
        };
        stopped(&stop)?;
        Ok(PreparedSelection {
            verified,
            root,
            clip_root,
            state_root,
            request: self.request,
        })
    }
}
/// The controller also retains this exact instance Arc, so a lost/panicked
/// finite result cannot be mistaken for physical helper retirement.
struct PreRenderOwner {
    instance: Arc<Mutex<PackageInstance>>,
    presentation: Arc<Mutex<Presentation>>,
    quota: QuotaGroup,
    stop: StopToken,
    custody: Mutex<Option<Arc<StorageAdmission>>>,
}
impl ReplayPreparationOwner for PreRenderOwner {
    fn quota_group(&self) -> QuotaGroup {
        self.quota.clone()
    }
    fn bind_retirement_custody(&self, storage: Arc<StorageAdmission>) {
        *self
            .custody
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = Some(storage);
    }
    fn cancel(&self) {
        self.stop.stop();
    }
    fn retire(&self) -> ilium_animation_js::error::Result<()> {
        let mut instance = self.instance.lock().map_err(|_| {
            ilium_animation_js::error::AnimationError::Runtime(
                "original replay instance poisoned; physical retirement unknown".into(),
            )
        })?;
        let mut presentation = self.presentation.lock().map_err(|_| {
            ilium_animation_js::error::AnimationError::Runtime(
                "original replay presentation poisoned; native retirement unknown".into(),
            )
        })?;
        revoke_presentation(&mut presentation);
        if !presentation
            .video
            .retire_recorded_until(Instant::now() + Duration::from_secs(30))?
        {
            return Err(ilium_animation_js::error::AnimationError::Runtime(
                "recorded Video decoder or acquisition receipt has not physically retired".into(),
            ));
        }
        if !presentation_finite_work_drained(&presentation) {
            return Err(ilium_animation_js::error::AnimationError::Runtime(
                "pre-rendered native receipts remain pending; actual finite wake required".into(),
            ));
        }
        instance.retire_helper()?;
        if !instance.is_physically_retired() {
            return Err(ilium_animation_js::error::AnimationError::Runtime(
                "replay helper exit unproved".into(),
            ));
        }
        release_presentation_after_helper_retirement(&mut presentation)?;
        if !presentation_is_drained(&presentation) {
            return Err(ilium_animation_js::error::AnimationError::Runtime(
                "pre-rendered native owner survived helper retirement".into(),
            ));
        }
        Ok(())
    }
    fn is_retired(&self) -> bool {
        self.instance
            .try_lock()
            .is_ok_and(|instance| instance.is_physically_retired())
            && self
                .presentation
                .try_lock()
                .is_ok_and(|presentation| presentation_is_drained(&presentation))
    }
}
fn presentation_finite_work_drained(presentation: &Presentation) -> bool {
    // Valid decoded image allocations can be released only after the helper
    // has physically exited. Their presence is not an outstanding finite job.
    presentation.compute.is_drained()
        && presentation.http.is_drained()
        && presentation.sources.is_drained()
        && presentation.assets.is_drained()
        && presentation.video.finite_work_drained()
        && presentation.audio.is_drained()
        && presentation.tasks.is_drained()
        && presentation
            .world
            .as_ref()
            .is_none_or(NativeWorldHost::is_drained)
        && presentation
            .presentation
            .as_ref()
            .is_none_or(NativePresentationHost::is_drained)
}
fn presentation_is_drained(presentation: &Presentation) -> bool {
    presentation_finite_work_drained(presentation) && presentation.images.is_drained()
}
fn revoke_presentation(presentation: &mut Presentation) {
    presentation.audio.stop();
    presentation.tasks.revoke();
    presentation.compute.revoke();
    presentation.http.cancel_all();
    presentation.sources.cancel_all();
    presentation.images.revoke();
    presentation.assets.revoke();
    presentation.video.revoke();
    presentation.drawing.revoke();
    if let Some(world) = presentation.world.as_mut() {
        world.revoke();
    }
    if let Some(receipts) = presentation.presentation.as_mut() {
        receipts.revoke();
    }
    presentation.surface.abort();
}
fn release_presentation_after_helper_retirement(
    presentation: &mut Presentation,
) -> ilium_animation_js::error::Result<()> {
    presentation
        .sources
        .release_terminal_after_helper_retirement();
    presentation
        .images
        .release_terminal_after_helper_retirement(&mut presentation.drawing);
    presentation
        .assets
        .release_terminal_after_helper_retirement();
    presentation
        .video
        .release_terminal_after_helper_retirement(&mut presentation.drawing)?;
    Ok(())
}
struct PreRenderJob {
    request: RenderRequest,
    instance: Arc<Mutex<PackageInstance>>,
    presentation: Arc<Mutex<Presentation>>,
    cache: Arc<ReplayCache>,
    store: Arc<ClipChunkStore>,
    spec: Arc<ClipSpec>,
    authority: ReplayAuthority,
    authorization: Arc<dyn ReplayAuthorization>,
    quota: QuotaGroup,
    stop: StopToken,
}
impl Job for PreRenderJob {
    type Output = Arc<ReplayClip>;
    type Error = String;
    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        let owner = Arc::new(PreRenderOwner {
            instance: Arc::clone(&self.instance),
            presentation: Arc::clone(&self.presentation),
            quota: self.quota.clone(),
            stop: self.stop.clone(),
            custody: Mutex::new(None),
        });
        let result = (|| -> Result<Arc<ReplayClip>, String> {
            stopped(&self.stop)?;
            stopped(&context.stop_token())?;
            if self.spec.is_recorded_video() {
                let cached_before_retirement = self
                    .cache
                    .contains(self.spec.key())
                    .map_err(|error| error.to_string())?;
                if cached_before_retirement {
                    // CachedDelivery checks physical helper exit. A presence
                    // hint alone cannot authorize another activation's clip.
                    owner.retire().map_err(|error| error.to_string())?;
                }
                let owner_factory: Arc<dyn ReplayPreparationOwner> = owner.clone();
                let mut preparation = match self
                    .cache
                    .begin(
                        Arc::clone(&self.spec),
                        self.authority.clone(),
                        Arc::clone(&self.authorization),
                        self.stop.clone(),
                        || Ok(owner_factory),
                    )
                    .map_err(|error| error.to_string())?
                {
                    Preparation::Started(mut preparation) if cached_before_retirement => {
                        // Eviction between presence and lookup cannot make a
                        // retired helper into a new frame producer.
                        preparation.cancel();
                        drop(preparation);
                        self.cache
                            .collect_retired()
                            .map_err(|error| error.to_string())?;
                        return Err(
                            "Original recorded cache changed after helper retirement".into()
                        );
                    }
                    Preparation::Started(preparation) => preparation,
                    Preparation::Cached(clip) => {
                        if !cached_before_retirement {
                            return Err(
                                "Recorded cache became available before producer retirement".into(),
                            );
                        }
                        return Ok(clip);
                    }
                    Preparation::InProgress => {
                        return Err("Original recorded replay preparation already running".into());
                    }
                };
                for _ in 0..self.spec.frame_count() {
                    stopped(&self.stop)?;
                    stopped(&context.stop_token())?;
                    let sample = preparation
                        .next_sample()
                        .map_err(|error| error.to_string())?;
                    let mut instance = self
                        .instance
                        .lock()
                        .map_err(|_| "Original recorded replay instance poisoned".to_owned())?;
                    let mut presentation = self
                        .presentation
                        .lock()
                        .map_err(|_| "Original recorded replay presentation poisoned".to_owned())?;
                    presentation.render_prepared(
                        &mut instance,
                        &self.request,
                        sample,
                        &mut preparation,
                        &self.quota,
                        &self.stop,
                    )?;
                }
                stopped(&self.stop)?;
                stopped(&context.stop_token())?;
                return preparation.finish().map_err(|error| error.to_string());
            }
            // A complete cold index still requires THIS activation's helper to
            // physically retire before original-broker playback is possible.
            if self.store.open_procedural(&self.spec.key().hex()).is_ok() {
                owner.retire().map_err(|error| error.to_string())?;
                stopped(&self.stop)?;
                stopped(&context.stop_token())?;
                let clip = self
                    .cache
                    .open_procedural_from_disk(
                        Arc::clone(&self.spec),
                        self.authority.clone(),
                        Arc::clone(&self.authorization),
                        &self.store,
                    )
                    .map_err(|error| error.to_string())?;
                stopped(&self.stop)?;
                stopped(&context.stop_token())?;
                return Ok(clip);
            }
            // CachedDelivery is authorized only AFTER this activation's real
            // helper exit. A presence check carries no authority: re-enter the
            // cache under the original broker after retirement and refuse an
            // eviction/single-flight race rather than rendering on a dead V8.
            if self
                .cache
                .contains(self.spec.key())
                .map_err(|error| error.to_string())?
            {
                owner.retire().map_err(|error| error.to_string())?;
                stopped(&self.stop)?;
                stopped(&context.stop_token())?;
                let owner_factory: Arc<dyn ReplayPreparationOwner> = owner.clone();
                return match self
                    .cache
                    .begin_streaming(
                        Arc::clone(&self.spec),
                        self.authority.clone(),
                        Arc::clone(&self.authorization),
                        self.stop.clone(),
                        Arc::clone(&self.store),
                        || Ok(owner_factory),
                    )
                    .map_err(|error| error.to_string())?
                {
                    Preparation::Cached(clip) => {
                        stopped(&self.stop)?;
                        stopped(&context.stop_token())?;
                        Ok(clip)
                    }
                    Preparation::Started(mut preparation) => {
                        preparation.cancel();
                        drop(preparation);
                        self.cache
                            .collect_retired()
                            .map_err(|error| error.to_string())?;
                        Err("Original cached replay changed after helper retirement".into())
                    }
                    Preparation::InProgress => {
                        Err("Original cached replay became an active preparation".into())
                    }
                };
            }
            let owner_factory: Arc<dyn ReplayPreparationOwner> = owner.clone();
            let mut preparation = match self
                .cache
                .begin_streaming(
                    Arc::clone(&self.spec),
                    self.authority.clone(),
                    Arc::clone(&self.authorization),
                    self.stop.clone(),
                    Arc::clone(&self.store),
                    || Ok(owner_factory),
                )
                .map_err(|error| error.to_string())?
            {
                Preparation::Started(preparation) => preparation,
                Preparation::Cached(clip) => {
                    // A cache hit still leaves this newly accepted helper alive.
                    owner.retire().map_err(|error| error.to_string())?;
                    stopped(&self.stop)?;
                    stopped(&context.stop_token())?;
                    return Ok(clip);
                }
                Preparation::InProgress => {
                    return Err("Original replay preparation already running".into())
                }
            };
            for _ in 0..self.spec.frame_count() {
                stopped(&self.stop)?;
                stopped(&context.stop_token())?;
                let sample = preparation
                    .next_sample()
                    .map_err(|error| error.to_string())?;
                let mut instance = self
                    .instance
                    .lock()
                    .map_err(|_| "Original replay instance poisoned".to_owned())?;
                let mut presentation = self
                    .presentation
                    .lock()
                    .map_err(|_| "Original replay presentation poisoned".to_owned())?;
                presentation.render_prepared(
                    &mut instance,
                    &self.request,
                    sample,
                    &mut preparation,
                    &self.quota,
                    &self.stop,
                )?;
            }
            stopped(&self.stop)?;
            stopped(&context.stop_token())?;
            preparation.finish().map_err(|error| error.to_string())
        })();
        result
    }
}

// Pointer/occlusion have actual native input owners. Disk and qualified Linux
// audio selection remain reviewable only through their admitted native picker.
// Normalized NetworkHttp and StatePersist are reviewable because their actual
// original native owners are constructed before dispatch. NetworkLocal is
// deliberately excluded until its genuine dependency projection is implemented.
// Observer and GPU owners must extend policy only when actually composed.
fn native_policy(
    capabilities: &[ilium_animation_js::manifest::Capability],
    audio_selectable: bool,
) -> Result<Ceiling, String> {
    let mut permissions = Vec::new();
    for capability in capabilities {
        let right = ilium_animation_js::permission_projection::right(capability)
            .map_err(|error| error.to_string())?;
        if matches!(
            right.id,
            Capability::AudioLoopback | Capability::AudioMicrophone
        ) && !audio_selectable
        {
            continue;
        }
        if matches!(
            right.id,
            Capability::InputPointer
                | Capability::ScreenOcclusion
                | Capability::DiskRead
                | Capability::DiskWrite
                | Capability::AudioLoopback
                | Capability::AudioMicrophone
                | Capability::NetworkHttp
                | Capability::StatePersist
        ) {
            permissions.push(right);
        }
    }
    Ok(Ceiling { permissions })
}
struct Workflow {
    revision: u64,
    request: RenderRequest,
    controller: PluginPermissionController,
    picker: Option<(PickSelection, Receipt<PickerJob>)>,
    audio_picker: Option<(PickAudioSelection, Receipt<AudioPickerJob>)>,
    audio_picker_uncertain: bool,
    qualified_audio: Option<QualifiedCaptureBinding>,
    presentation: Option<Presentation>,
    // The controller retains this same owner even if the finite producer
    // panics, loses its result, or exits through a cold/cache shortcut.
    preparation_presentation: Option<Arc<Mutex<Presentation>>>,
    clip_root: Option<Arc<PinnedDirectory>>,
    state_root: Arc<PinnedDirectory>,
    preparation: Option<Receipt<PreRenderJob>>,
    preparation_retention: Option<Retention>,
    preparation_stop: Option<ilium_animation_js::runtime::StoppedInstance>,
    preparation_authority: Option<(ReplayAuthority, Arc<dyn ReplayAuthorization>)>,
    player: Option<ReplayPlayer>,
    playback_origin: Option<Instant>,
    last_playback_tick: Option<Duration>,
    last_playback_elapsed: Option<Duration>,
    playback_frozen: bool,
    update: Option<ActivationUpdate>,
    update_applied: bool,
    cancellation: Option<PermissionCancellation>,
    halted: bool,
    _setup_retention: Retention,
}
pub(super) struct PluginBackend {
    setup: Option<(u64, Receipt<SetupJob>)>,
    setup_cancelled: bool,
    workflow: Option<Workflow>,
    failure: Option<(u64, String)>,
    quota: QuotaGroup,
    resources: ilium_ambient::resources::AmbientResources,
    review: Arc<ReviewBridge>,
    actor_wake: Arc<dyn Fn() + Send + Sync>,
    ui_ready: Arc<tokio::sync::Notify>,
    selection_revision: Option<u64>,
    saved_runtime: Option<Arc<ilium_ambient::minecraft::saved_runtime::SavedRuntime>>,
}
impl PluginBackend {
    /// Bind the worker's existing saved-world authority before any package
    /// activation.  A package can request a saved world later, but it can
    /// never supply a replacement runtime or history namespace.
    pub(super) fn set_saved_runtime(
        &mut self,
        runtime: Arc<ilium_ambient::minecraft::saved_runtime::SavedRuntime>,
    ) {
        if self.saved_runtime.is_none() && self.workflow.is_none() {
            self.saved_runtime = Some(runtime);
        }
    }
    pub(super) fn new(
        quota: QuotaGroup,
        resources: ilium_ambient::resources::AmbientResources,
        review: Arc<ReviewBridge>,
        actor_wake: Arc<dyn Fn() + Send + Sync>,
        ui_ready: Arc<tokio::sync::Notify>,
    ) -> Self {
        Self {
            setup: None,
            setup_cancelled: false,
            workflow: None,
            failure: None,
            quota,
            resources,
            review,
            actor_wake,
            ui_ready,
            selection_revision: None,
            saved_runtime: None,
        }
    }
    /// Authority is blocked synchronously. Original owners stay retained until
    /// physical settlement is established; no Drop == drained inference.
    pub(super) fn stop(&mut self) {
        if let Some((_, receipt)) = &self.setup {
            receipt.cancel();
            self.setup_cancelled = true;
        }
        let Some(workflow) = self.workflow.as_mut() else {
            return;
        };
        if workflow.halted {
            return;
        }
        if let Some(receipt) = &workflow.preparation {
            receipt.cancel();
        }
        if let Some((_, receipt)) = &workflow.picker {
            receipt.cancel();
        }
        if let Some((_, receipt)) = &workflow.audio_picker {
            receipt.cancel();
        }
        if let Some(presentation) = &mut workflow.presentation {
            revoke_presentation(presentation);
        }
        if workflow.preparation.is_none() {
            if let Some(shared) = &workflow.preparation_presentation {
                match shared.try_lock() {
                    Ok(mut presentation) => revoke_presentation(&mut presentation),
                    Err(TryLockError::Poisoned(poison)) => {
                        let mut presentation = poison.into_inner();
                        revoke_presentation(&mut presentation);
                    }
                    Err(TryLockError::WouldBlock) => {}
                }
            }
        }
        workflow.cancellation = Some(workflow.controller.cancel());
        workflow.halted = true;
    }
    pub(super) fn fail_current(&mut self, message: &str) {
        let revision = self
            .workflow
            .as_ref()
            .map(|workflow| workflow.revision)
            .or_else(|| self.setup.as_ref().map(|(revision, _)| *revision))
            .or(self.selection_revision);
        self.stop();
        if let Some(revision) = revision {
            self.failure = Some((revision, message.to_owned()));
            if let Err(error) = self.review.deny_pending(revision, message) {
                tracing::warn!(%error, "Native review failure publication refused");
            }
        }
    }
    fn start_setup(&mut self, request: &RenderRequest) -> Result<(), String> {
        self.selection_revision = Some(request.revision);
        if !self.review.shares_root(&self.quota)
            || !self
                .resources
                .finite()
                .quota_group()
                .shares_root(&self.quota)
        {
            return Err("Foreign review original quota root".into());
        }
        if self.setup.is_some() || self.workflow.is_some() {
            return Err("Original plugin retirement has not physically settled".into());
        }
        let reservation = self
            .resources
            .finite()
            .try_reserve(
                Lane::Io,
                JobCost {
                    input_bytes: ARCHIVE_BYTES * 2 + 4 * 1024 * 1024,
                    result_bytes: 1024 * 1024,
                },
            )
            .map_err(|error| format!("Native setup admission: {error:?}"))?;
        // Reservation precedes the immutable settings/request copy.
        let receipt = reservation
            .submit(SetupJob {
                request: request.clone(),
                quota: self.quota.clone(),
            })
            .map_err(|error| format!("Native setup submission: {:?}", error.reason))?;
        self.setup = Some((request.revision, receipt));
        self.setup_cancelled = false;
        self.failure = None;
        Ok(())
    }
    fn collect_setup(&mut self) -> Result<(), String> {
        let Some((revision, receipt)) = self.setup.as_mut() else {
            return Ok(());
        };
        let revision = *revision;
        let outcome = match receipt.try_take() {
            JobPoll::Pending => return Ok(()),
            JobPoll::Ready(outcome) => outcome,
            JobPoll::Lost | JobPoll::Taken => {
                // Keep the original ambiguous receipt and refuse replacement.
                return Err("Native setup receipt lost: physical settlement unproven".into());
            }
        };
        self.setup = None;
        let (outcome, retention) = outcome.into_parts();
        if self.setup_cancelled {
            return Ok(());
        }
        let prepared = match outcome {
            JobOutcome::Finished(result) => result?,
            JobOutcome::NotStarted { .. } => return Err("Native setup did not run".into()),
            JobOutcome::Panicked => return Err("Native setup panicked; effects unknown".into()),
        };
        let files = PluginPermissionFiles::new(
            self.resources.finite(),
            prepared.root,
            Arc::clone(&self.ui_ready),
            Arc::clone(&self.actor_wake),
        )
        .map_err(|error| error.to_string())?;
        let mut controller = PluginPermissionController::new(files, self.resources.finite())?;
        controller.start(prepared.verified, revision)?;
        self.workflow = Some(Workflow {
            revision,
            request: prepared.request,
            controller,
            picker: None,
            audio_picker: None,
            audio_picker_uncertain: false,
            qualified_audio: None,
            presentation: None,
            preparation_presentation: None,
            clip_root: prepared.clip_root,
            state_root: prepared.state_root,
            preparation: None,
            preparation_retention: None,
            preparation_stop: None,
            preparation_authority: None,
            player: None,
            playback_origin: None,
            last_playback_tick: None,
            last_playback_elapsed: None,
            playback_frozen: false,
            update: None,
            update_applied: false,
            cancellation: None,
            halted: false,
            _setup_retention: retention,
        });
        Ok(())
    }
    /// Called ONLY after the worker mailbox lock has been released. Review
    /// intent never invokes native IO/JS from the UI or bank completion callback.
    pub(super) fn on_review_intent(&mut self) -> Result<(), String> {
        let Some(workflow) = self.workflow.as_mut() else {
            return Err("Native review intent has no original workflow".into());
        };
        if workflow.halted {
            return Err("Native workflow is retiring".into());
        }
        let outcome = review_controller::consume_intent(
            &mut workflow.controller,
            &self.review,
            workflow.revision,
            &mut apply_before_creation,
        )?;
        match outcome {
            IntentOutcome::Cancelled(cancellation) => {
                workflow.cancellation = Some(cancellation);
                workflow.halted = true;
            }
            IntentOutcome::Picking(pick) => {
                if workflow.picker.is_some()
                    || workflow.audio_picker.is_some()
                    || pick.selection_revision != workflow.revision
                {
                    return Err("Native picker already active or stale".into());
                }
                let reservation = self
                    .resources
                    .finite()
                    .try_reserve(
                        Lane::Io,
                        JobCost {
                            input_bytes: 16 * 1024,
                            result_bytes: 8192,
                        },
                    )
                    .map_err(|error| format!("Native picker admission: {error:?}"))?;
                let job = PickerJob {
                    path: PathBuf::from(&pick.path),
                    slot: pick.slot.clone(),
                    selection: pick.disk_selection,
                    writable: pick.writable,
                    quota: self.quota.clone(),
                };
                let receipt = reservation
                    .submit(job)
                    .map_err(|error| format!("Native picker submission: {:?}", error.reason))?;
                workflow.picker = Some((pick, receipt));
                return Ok(());
            }
            IntentOutcome::PickingAudio(pick) => {
                if workflow.picker.is_some()
                    || workflow.audio_picker.is_some()
                    || pick.selection_revision != workflow.revision
                {
                    return Err("Native audio picker already active or stale".into());
                }
                let reservation = self
                    .resources
                    .finite()
                    .try_reserve(
                        Lane::Io,
                        JobCost {
                            input_bytes: 16 * 1024,
                            result_bytes: 8192,
                        },
                    )
                    .map_err(|error| format!("Native audio picker admission: {error:?}"))?;
                let job = AudioPickerJob {
                    endpoint: pick.endpoint.clone(),
                    scope_device: pick.scope_device.clone(),
                    capability: pick.capability,
                    resources: self.resources.clone(),
                };
                let receipt = reservation.submit(job).map_err(|error| {
                    format!("Native audio picker submission: {:?}", error.reason)
                })?;
                workflow.audio_picker = Some((pick, receipt));
                return Ok(());
            }
            IntentOutcome::None | IntentOutcome::Resolved => {}
        }
        self.advance_controller()
    }
    fn collect_picker(&mut self) -> Result<(), String> {
        let Some(workflow) = self.workflow.as_mut() else {
            return Ok(());
        };
        let Some((pick, receipt)) = workflow.picker.as_mut() else {
            return Ok(());
        };
        let outcome = match receipt.try_take() {
            JobPoll::Pending => return Ok(()),
            JobPoll::Ready(outcome) => outcome,
            JobPoll::Lost | JobPoll::Taken => {
                return Err(
                    "Native picker receipt lost; original pinning job exit unproved".into(),
                );
            }
        };
        let pick = std::mem::replace(
            pick,
            PickSelection {
                selection_revision: 0,
                request_id: String::new(),
                path: String::new(),
                slot: String::new(),
                disk_selection: Selection::File,
                writable: false,
                review_revision: 0,
                authorization_epoch: 0,
            },
        );
        workflow.picker = None;
        let (outcome, _retention) = outcome.into_parts();
        if workflow.halted {
            return Ok(());
        }
        workflow.controller.with_review_state(
            workflow.revision,
            |_, _, _, revision, epoch| {
                if revision != pick.review_revision || epoch != pick.authorization_epoch {
                    Err("Native picker review authority changed".to_owned())
                } else {
                    Ok(())
                }
            },
        )??;
        let mut picker_error = None;
        let resource = match outcome {
            JobOutcome::Finished(result) => match result {
                Ok(resource) => Some((pick.request_id, resource)),
                Err(error) => {
                    tracing::warn!(%error, "Native picker refused host selection");
                    picker_error = Some(error);
                    None
                }
            },
            JobOutcome::NotStarted { .. } => {
                picker_error = Some("Native picker was cancelled before selection".to_owned());
                None
            }
            JobOutcome::Panicked => {
                return Err("Native picker panicked; job effects unknown".into())
            }
        };
        let revision = pick
            .review_revision
            .checked_add(1)
            .ok_or("Native picker revision exhausted")?;
        let review =
            workflow
                .controller
                .review_selected(workflow.revision, revision, resource, None)?;
        review_controller::publish_native_review(
            &workflow.controller,
            &self.review,
            workflow.revision,
            review,
        )?;
        if let Some(error) = picker_error {
            self.review.picker_error(workflow.revision, &error)?;
        }
        Ok(())
    }
    fn collect_audio_picker(&mut self) -> Result<(), String> {
        let Some(workflow) = self.workflow.as_mut() else {
            return Ok(());
        };
        let Some((_, receipt)) = workflow.audio_picker.as_mut() else {
            return Ok(());
        };
        let outcome = match receipt.try_take() {
            JobPoll::Pending => return Ok(()),
            JobPoll::Ready(outcome) => outcome,
            JobPoll::Lost | JobPoll::Taken => {
                workflow.audio_picker_uncertain = true;
                return Err(
                    "Native audio picker receipt lost; original endpoint query exit unproved"
                        .into(),
                );
            }
        };
        let (pick, _) = workflow
            .audio_picker
            .take()
            .ok_or("Native audio picker disappeared")?;
        let (outcome, _retention) = outcome.into_parts();
        if workflow.halted {
            return Ok(());
        }
        workflow.controller.with_review_state(
            workflow.revision,
            |_, _, _, revision, epoch| {
                if revision == pick.review_revision && epoch == pick.authorization_epoch {
                    Ok(())
                } else {
                    Err("Native audio picker review authority changed".to_owned())
                }
            },
        )??;
        let mut picker_error = None;
        let binding = match outcome {
            JobOutcome::Finished(Ok(binding)) => Some(binding),
            JobOutcome::Finished(Err(error)) => {
                tracing::warn!(%error, "Native audio endpoint selection refused");
                picker_error = Some(error);
                None
            }
            JobOutcome::NotStarted { .. } => {
                picker_error = Some("Native audio picker was cancelled before selection".into());
                None
            }
            JobOutcome::Panicked => {
                workflow.audio_picker_uncertain = true;
                return Err("Native audio picker panicked; query effects unknown".into());
            }
        };
        let revision = pick
            .review_revision
            .checked_add(1)
            .ok_or("Native audio picker revision exhausted")?;
        let review = workflow.controller.review_selected(
            workflow.revision,
            revision,
            None,
            Some((pick.request_id, binding.clone())),
        )?;
        workflow.qualified_audio = binding;
        review_controller::publish_native_review(
            &workflow.controller,
            &self.review,
            workflow.revision,
            review,
        )?;
        if let Some(error) = picker_error {
            self.review.picker_error(workflow.revision, &error)?;
        }
        Ok(())
    }
    fn advance_controller(&mut self) -> Result<(), String> {
        let Some(workflow) = self.workflow.as_mut() else {
            return Ok(());
        };
        if workflow.halted || workflow.update_applied {
            return Ok(());
        }
        if workflow.update.is_none() {
            workflow.update = review_controller::collect_controller(
                &mut workflow.controller,
                workflow.revision,
                &mut apply_before_creation,
            )?;
        }
        if matches!(workflow.update, Some(ActivationUpdate::Review(_))) {
            let Some(ActivationUpdate::Review(review)) = workflow.update.take() else {
                return Err("Original review ownership changed".into());
            };
            review_controller::publish_native_review(
                &workflow.controller,
                &self.review,
                workflow.revision,
                review,
            )?;
            return Ok(());
        }
        if let Some(update) = &workflow.update {
            // Keep genuine effect inventory through publication refusal/failure.
            review_controller::publish_controller_outcome(
                &self.review,
                workflow.revision,
                update,
                &mut apply_before_creation,
            )?;
            let creation = match update {
                ActivationUpdate::Finished(resolution) => resolution.accepted_creation(),
                ActivationUpdate::Failed { .. } | ActivationUpdate::Review(_) => None,
            };
            let Some(creation) = creation else {
                workflow.halted = true;
                return Err("Native permission activation was refused or failed".into());
            };
            if workflow.presentation.is_none() {
                let instance = workflow
                    .controller
                    .package_instance_mut()
                    .ok_or_else(|| "Accepted native instance missing".to_owned())?;
                workflow.presentation = Some(Presentation::new(
                    instance,
                    &workflow.request,
                    &self.quota,
                    self.resources.clone(),
                    Arc::clone(&workflow.state_root),
                    workflow.qualified_audio.take(),
                    workflow
                        .request
                        .settings
                        .plugin
                        .selected
                        .as_ref()
                        .is_some_and(|selected| selected.mode == AnimationMode::Live),
                    creation,
                    Arc::clone(&self.actor_wake),
                )?);
                if let (Some(presentation), Some(runtime)) =
                    (workflow.presentation.as_mut(), self.saved_runtime.as_ref())
                {
                    let normalized = workflow.request.settings.ambient.normalized();
                    presentation.saved_context =
                        Some(ilium_animation_js::native_world_host::NativeSavedContext {
                            settings: normalized.voxel_landscape,
                            environment: ilium_ambient::SceneEnv {
                                resources: self.resources.clone(),
                                location: normalized.location,
                                cache_dir: ilium_ambient::source::default_cache_dir(),
                                gpu: ilium_gpu::runner(),
                                saved_runtime: Arc::clone(runtime),
                                palette: workflow.request.settings.appearance.scene_palette(),
                            },
                        });
                }
            }
            let presentation = workflow
                .presentation
                .as_mut()
                .ok_or("Native presentation missing")?;
            let instance = workflow
                .controller
                .package_instance_mut()
                .ok_or("Accepted instance missing")?;
            presentation.dispatch_requests(instance)?;
            workflow.update_applied = true;
            self.review.set_phase(
                workflow.revision,
                if presentation.creation == CreateState::Ready
                    && workflow
                        .request
                        .settings
                        .plugin
                        .selected
                        .as_ref()
                        .is_some_and(|selected| selected.mode == AnimationMode::Live)
                {
                    ReviewPhase::Ready
                } else {
                    ReviewPhase::Creating
                },
                None,
            )?;
            // Resolution remains retained with the controller's ledger receipts.
            if presentation.creation == CreateState::Ready
                && workflow
                    .request
                    .settings
                    .plugin
                    .selected
                    .as_ref()
                    .is_some_and(|selected| selected.mode == AnimationMode::PreRendered)
            {
                Self::start_preparation(workflow, &self.quota, &self.resources)?;
            }
        }
        Ok(())
    }
    fn start_preparation(
        workflow: &mut Workflow,
        quota: &QuotaGroup,
        resources: &ilium_ambient::resources::AmbientResources,
    ) -> Result<(), String> {
        if workflow.preparation.is_some() || workflow.player.is_some() || workflow.halted {
            return Ok(());
        }
        let root = workflow
            .clip_root
            .as_ref()
            .ok_or("Pinned native clip root missing")?;
        let store = Arc::new(
            ClipChunkStore::new(Arc::clone(root), quota.clone(), 2 * 1024 * 1024 * 1024)
                .map_err(|error| error.to_string())?,
        );
        let cache = Arc::new(
            ReplayCache::new(quota.clone(), ReplayLimits::default())
                .map_err(|error| error.to_string())?,
        );
        let (spec, authority, authorization) = {
            let instance = workflow
                .controller
                .package_instance_mut()
                .ok_or("Accepted replay instance missing")?;
            let presentation = workflow
                .presentation
                .as_mut()
                .ok_or("Accepted replay presentation missing")?;
            let recorded_count = presentation.video.recorded_count();
            if presentation.video.pre_render_video_attempted() && recorded_count == 0 {
                return Err("Video acquisition has not produced a finite recorded source".into());
            }
            let certification = if recorded_count == 0 {
                instance.certify_procedural_replay()
            } else {
                instance.certify_recorded_video_replay(recorded_count)
            }
            .map_err(|error| error.to_string())?;
            let verifier =
                ilium_animation_js::release::verifier().map_err(|error| error.to_string())?;
            let package = verifier.verify(instance.package());
            let frozen = FrozenInputs::from_host(
                quota.clone(),
                None,
                &[],
                Sha256::digest([]).into(),
                None,
                if recorded_count == 0 {
                    "procedural"
                } else {
                    "recorded-video"
                },
                &[],
            )
            .map_err(|error| error.to_string())?;
            let native_evidence = if recorded_count == 0 {
                Vec::new()
            } else {
                presentation
                    .video
                    .recording_evidence()
                    .map_err(|error| error.to_string())?
            };
            let evidence = FrozenEvidence::from_native(quota.clone(), &package, &native_evidence)
                .map_err(|error| error.to_string())?;
            if recorded_count != 0 {
                presentation
                    .video
                    .bind_recording(&evidence)
                    .map_err(|error| error.to_string())?;
            }
            let appearance_digest: [u8; 32] = Sha256::digest(
                serde_json::to_vec(&workflow.request.settings)
                    .map_err(|error| error.to_string())?,
            )
            .into();
            let spec = ClipSpec::from_accepted(
                quota,
                ClipSpecification {
                    package: instance.package(),
                    verifier: &verifier,
                    plan: instance.plan(),
                    settings: instance.settings(),
                    shape: shape(
                        instance.plan(),
                        workflow.request.width,
                        workflow.request.height,
                    )?,
                    backend: "native-v8",
                    api_version: instance.package().manifest().api_version,
                    appearance_digest,
                    certification,
                    frozen,
                    evidence,
                },
            )
            .map_err(|error| error.to_string())?;
            let (authority, authorization) = instance
                .retain_procedural_replay_authorization()
                .map_err(|error| error.to_string())?;
            (spec, authority, authorization)
        };
        let reservation = resources
            .finite()
            .try_reserve(
                Lane::Cpu,
                JobCost {
                    input_bytes: 1024 * 1024,
                    result_bytes: 1024 * 1024,
                },
            )
            .map_err(|error| format!("Native clip producer admission: {error:?}"))?;
        let stop = StopToken::default();
        let instance = workflow.controller.delegate_accepted_replay(stop.clone())?;
        let presentation = workflow
            .presentation
            .take()
            .ok_or("Native clip surface missing")?;
        let presentation = Arc::new(Mutex::new(presentation));
        workflow.preparation_presentation = Some(Arc::clone(&presentation));
        let job = PreRenderJob {
            request: workflow.request.clone(),
            instance: Arc::clone(&instance),
            presentation,
            cache,
            store,
            spec,
            authority: authority.clone(),
            authorization: Arc::clone(&authorization),
            quota: quota.clone(),
            stop,
        };
        match reservation.submit(job) {
            Ok(receipt) => {
                workflow.preparation_authority = Some((authority, authorization));
                workflow.preparation = Some(receipt);
                Ok(())
            }
            Err(rejected) => {
                workflow.cancellation = Some(workflow.controller.cancel());
                workflow.halted = true;
                // Rejected publication returned the actual unstarted job; the
                // controller still retains this exact instance and broker.
                drop(rejected.value);
                let mut instance = instance
                    .lock()
                    .map_err(|_| "Rejected clip owner poisoned".to_owned())?;
                workflow.preparation_stop = Some(instance.stop());
                Err(format!(
                    "Native clip producer submission: {:?}",
                    rejected.reason
                ))
            }
        }
    }
    fn collect_preparation(workflow: &mut Workflow) -> Result<(), String> {
        let Some(receipt) = workflow.preparation.as_mut() else {
            return Ok(());
        };
        let outcome = match receipt.try_take() {
            JobPoll::Pending => return Ok(()),
            JobPoll::Ready(outcome) => outcome,
            JobPoll::Lost | JobPoll::Taken => {
                workflow.cancellation = Some(workflow.controller.cancel());
                workflow.halted = true;
                return Err("Native clip result lost; original helper exit unproved".into());
            }
        };
        workflow.preparation = None;
        let (outcome, retention) = outcome.into_parts();
        workflow.preparation_retention = Some(retention);
        let result = match outcome {
            JobOutcome::Finished(result) => result,
            JobOutcome::NotStarted { .. } => Err("Native clip job never started".into()),
            JobOutcome::Panicked => {
                Err("Native clip job panicked; original effects unknown".into())
            }
        };
        if workflow.halted {
            if let Some(instance) = workflow.controller.delegated_instance() {
                let mut instance = instance
                    .try_lock()
                    .map_err(|_| "Cancelled clip owner still running or poisoned".to_owned())?;
                workflow.preparation_stop = Some(instance.stop());
            }
            return Ok(());
        }
        let clip = match result {
            Ok(clip) => clip,
            Err(error) => {
                workflow.cancellation = Some(workflow.controller.cancel());
                workflow.halted = true;
                if let Some(instance) = workflow.controller.delegated_instance() {
                    if let Ok(mut instance) = instance.try_lock() {
                        workflow.preparation_stop = Some(instance.stop());
                    }
                }
                return Err(error);
            }
        };
        let (authority, authorization) = workflow
            .preparation_authority
            .take()
            .ok_or("Original replay authorization missing")?;
        if !workflow
            .controller
            .delegated_instance()
            .is_some_and(|instance| {
                instance
                    .try_lock()
                    .is_ok_and(|instance| instance.is_physically_retired())
            })
        {
            return Err("Clip job result lacks actual helper retirement".into());
        }
        let player = ReplayPlayer::new(
            clip,
            authority,
            authorization,
            PlaybackSettings {
                now: Duration::ZERO,
                speed: f64::from(workflow.request.settings.speed_percent) / 100.,
                mode: PlaybackMode::Repeat,
                max_leases: ReplayLimits::default().max_leases,
                stop: StopToken::default(),
            },
        )
        .map_err(|error| error.to_string())?;
        workflow.player = Some(player);
        // Success passed the replay owner's helper + every original native
        // inventory gate; the retained presentation can now be released.
        workflow.preparation_presentation = None;
        workflow.playback_origin = None;
        workflow.last_playback_tick = None;
        workflow.last_playback_elapsed = None;
        workflow.playback_frozen = false;
        Ok(())
    }
    /// Genuine finite completion wake only: collects native setup/ledger jobs,
    /// then native compute. UI intents and time frames never poll those receipts.
    pub(super) fn next_task_deadline(&self) -> Option<Instant> {
        self.workflow
            .as_ref()
            .filter(|workflow| !workflow.halted)
            .and_then(|workflow| workflow.presentation.as_ref())
            .and_then(|presentation| presentation.tasks.next_due())
    }
    #[cfg(test)]
    pub(super) fn test_revoke_current_activation(&mut self) -> Result<(), String> {
        let workflow = self.workflow.as_mut().ok_or("No accepted task workflow")?;
        let instance = workflow
            .controller
            .package_instance_mut()
            .ok_or("Accepted task instance unavailable")?;
        if instance
            .revoke_activation()
            .map_err(|error| error.to_string())?
            .is_none()
        {
            return Err("Original task activation already revoked".into());
        }
        Ok(())
    }
    /// Timer expiry is a separate scene-actor turn, never a finite receipt
    /// wake. The helper's Promise copy/ACK precedes any callback checkpoint.
    pub(super) fn on_task_deadline(&mut self) -> Result<(), String> {
        let Some(workflow) = self.workflow.as_mut() else {
            return Ok(());
        };
        if workflow.halted {
            return Ok(());
        }
        let Some(presentation) = workflow.presentation.as_mut() else {
            return Ok(());
        };
        let instance = workflow
            .controller
            .package_instance_mut()
            .ok_or_else(|| "Accepted task instance unavailable".to_owned())?;
        let completed = presentation
            .tasks
            .on_due(instance, Instant::now())
            .map_err(|error| error.to_string())?;
        if !completed {
            return Ok(());
        }
        presentation.creation = instance.pump().map_err(|error| error.to_string())?;
        presentation.dispatch_requests(instance)?;
        if presentation.creation == CreateState::Ready {
            if workflow
                .request
                .settings
                .plugin
                .selected
                .as_ref()
                .is_some_and(|selected| selected.mode == AnimationMode::PreRendered)
            {
                Self::start_preparation(workflow, &self.quota, &self.resources)?;
            } else {
                self.review
                    .set_phase(workflow.revision, ReviewPhase::Ready, None)?;
            }
        }
        Ok(())
    }
    pub(super) fn on_native_completion(&mut self) -> Result<(), String> {
        let early = (|| {
            self.collect_setup()?;
            self.collect_picker()?;
            self.collect_audio_picker()?;
            self.advance_controller()
        })();
        if let Err(error) = early {
            // A picker or controller failure can share this wake with HTTP,
            // source, asset, or audio retirement. Preserve the original hint.
            self.fail_current(&error);
            return match self.settle_retirement(true) {
                Ok(()) => Err(error),
                Err(retirement) => Err(format!("{error}; retirement: {retirement}")),
            };
        }
        let Some(workflow) = self.workflow.as_mut() else {
            return Ok(());
        };
        let preparation = Self::collect_preparation(workflow);
        if preparation.is_err() && !workflow.halted {
            workflow.cancellation = Some(workflow.controller.cancel());
            workflow.halted = true;
        }
        if workflow.halted {
            // The producer result and another native receipt can share this
            // single wake. A failed producer must not consume the hint and
            // leave the original selected/cache/HTTP/source receipt stranded.
            let retirement = self.settle_retirement(true);
            return match (preparation, retirement) {
                (Err(error), Err(retirement)) => Err(format!("{error}; retirement: {retirement}")),
                (Err(error), _) | (_, Err(error)) => Err(error),
                (Ok(()), Ok(())) => Ok(()),
            };
        }
        preparation?;
        let Some(presentation) = &mut workflow.presentation else {
            if workflow.player.is_some() {
                self.review
                    .set_phase(workflow.revision, ReviewPhase::Ready, None)?;
            }
            return Ok(());
        };
        let instance = workflow
            .controller
            .package_instance_mut()
            .ok_or_else(|| "Accepted native instance unavailable".to_owned())?;
        // Observe every independent original owner on this genuine finite wake
        // even if one fails. Do not consume a terminal hint then strand the
        // other owner's original ready receipt awaiting a nonexistent new hint.
        let compute = presentation
            .compute
            .on_completion_wake(instance)
            .map_err(|error| error.to_string());
        let http = presentation
            .http
            .on_completion_wake(instance)
            .map_err(|error| error.to_string())
            .and_then(observe_http_events);
        let assets = presentation
            .assets
            .on_completion_wake(instance)
            .map_err(|error| error.to_string());
        let worlds = presentation
            .world
            .as_mut()
            .map(|host| host.on_completion_wake(instance))
            .transpose()
            .map_err(|error| error.to_string());
        let video = presentation
            .video
            .on_completion_wake(instance)
            .map_err(|error| error.to_string());
        let sources = presentation
            .sources
            .on_completion_wake(instance, &mut presentation.drawing);
        let audio = presentation.audio.collect_on_wake();
        compute?;
        http?;
        assets?;
        worlds?;
        video?;
        sources?;
        audio?;
        presentation.creation = instance.pump().map_err(|error| error.to_string())?;
        presentation.dispatch_requests(instance)?;
        if presentation.creation == CreateState::Ready {
            if workflow
                .request
                .settings
                .plugin
                .selected
                .as_ref()
                .is_some_and(|selected| selected.mode == AnimationMode::PreRendered)
            {
                Self::start_preparation(workflow, &self.quota, &self.resources)?;
            } else {
                self.review
                    .set_phase(workflow.revision, ReviewPhase::Ready, None)?;
            }
        }
        Ok(())
    }
    pub(super) fn is_physically_settled(&self) -> bool {
        self.setup.is_none() && self.workflow.is_none()
    }
    /// `on_wake` means an ORIGINAL native finite completion hint was observed.
    /// Logical frame cadence/intent/shutdown checks may read predicates only.
    pub(super) fn settle_retirement(&mut self, on_wake: bool) -> Result<(), String> {
        let Some(workflow) = self.workflow.as_mut() else {
            return Ok(());
        };
        if !workflow.halted {
            return Ok(());
        }
        // A running producer locks instance then presentation. Touch neither
        // delegated owner from UI while its original finite receipt is live.
        let mut delegated_presentation = if workflow.preparation.is_none() {
            workflow
                .preparation_presentation
                .as_ref()
                .and_then(|presentation| match presentation.try_lock() {
                    Ok(guard) => Some(guard),
                    Err(TryLockError::Poisoned(poison)) => Some(poison.into_inner()),
                    Err(TryLockError::WouldBlock) => None,
                })
        } else {
            None
        };
        if on_wake {
            let ledger = workflow.controller.collect_retirement_on_wake();
            let (compute, http, sources, assets, video, audio) = if let Some(presentation) =
                workflow
                    .presentation
                    .as_mut()
                    .or_else(|| delegated_presentation.as_deref_mut())
            {
                presentation.http.cancel_all();
                presentation.sources.cancel_all();
                presentation.audio.stop();
                let compute = presentation
                    .compute
                    .collect_retirement_on_wake()
                    .map_err(|error| error.to_string());
                let http = workflow
                    .controller
                    .collect_http_retirement_on_wake(&mut presentation.http)
                    .and_then(observe_http_events);
                let sources = workflow
                    .controller
                    .collect_source_retirement_on_wake(|instance| {
                        presentation.sources.collect_retirement_on_wake(instance)
                    });
                let assets = workflow
                    .controller
                    .collect_source_retirement_on_wake(|instance| {
                        presentation
                            .assets
                            .on_completion_wake(instance)
                            .map_err(|error| error.to_string())
                    });
                let video = workflow
                    .controller
                    .collect_source_retirement_on_wake(|instance| {
                        presentation
                            .video
                            .on_completion_wake(instance)
                            .map_err(|error| error.to_string())
                    });
                let audio = presentation.audio.collect_on_wake();
                (compute, http, sources, assets, video, audio)
            } else {
                (Ok(()), Ok(()), Ok(()), Ok(()), Ok(()), Ok(()))
            };
            // Independent original inventories all receive the SAME real wake.
            // Every failure stays retained; no helper ACK or replacement grant.
            ledger?;
            compute?;
            http?;
            sources?;
            assets?;
            video?;
            audio?;
        }
        if let Some(presentation) = workflow
            .presentation
            .as_mut()
            .or_else(|| delegated_presentation.as_deref_mut())
        {
            revoke_presentation(presentation);
        }
        // Native source receipts join the same original retirement inventory.
        let mut apply = |_invalidation: &Invalidation| Ok(());
        if let Some(cancellation) = &workflow.cancellation {
            // Actual stop's independent errors remain original/caller retained.
            if let Some(stop) = &cancellation.stop {
                if let Some(error) = &stop.authority_error {
                    return Err(error.to_string());
                }
                if let Err(error) = &stop.cancellation {
                    return Err(error.to_string());
                }
            }
            if let Some(error) = &cancellation.persistence_error {
                return Err(error.clone());
            }
        }
        if let Some(ActivationUpdate::Failed {
            stop: Some(stop), ..
        }) = &workflow.update
        {
            if let Some(error) = &stop.authority_error {
                return Err(error.to_string());
            }
            if let Err(error) = &stop.cancellation {
                return Err(error.to_string());
            }
        }
        if let Some(ActivationUpdate::Finished(resolution)) = &workflow.update {
            if let Some(error) = &resolution.authority_error {
                return Err(error.to_string());
            }
            if let Some(error) = &resolution.teardown_error {
                return Err(error.to_string());
            }
        }
        if workflow.preparation.is_some()
            || workflow.picker.is_some()
            || workflow.audio_picker.is_some()
        {
            return Ok(()); // Original finite job still owns execution/callback custody.
        }
        if workflow.audio_picker_uncertain {
            return Err("Audio endpoint query physical exit remains unproved".into());
        }
        if let Some(stop) = &workflow.preparation_stop {
            if let Some(error) = &stop.authority_error {
                return Err(error.to_string());
            }
            if let Err(error) = &stop.cancellation {
                return Err(error.to_string());
            }
            if let Some(invalidation) = &stop.invalidation {
                apply_before_creation(invalidation)?;
            }
        }
        if workflow.controller.is_physically_settled() {
            if let Some(presentation) = workflow
                .presentation
                .as_mut()
                .or_else(|| delegated_presentation.as_deref_mut())
            {
                release_presentation_after_helper_retirement(presentation)
                    .map_err(|error| error.to_string())?;
            }
        }
        if !workflow.controller.is_physically_settled()
            || workflow
                .presentation
                .as_ref()
                .is_some_and(|p| !presentation_is_drained(p))
            || delegated_presentation
                .as_deref()
                .is_some_and(|p| !presentation_is_drained(p))
            || (workflow.preparation_presentation.is_some() && delegated_presentation.is_none())
        {
            return Ok(());
        }
        if let Some(cancellation) = &workflow.cancellation {
            // UI can already be on a different native selection. Stale phase
            // publication is refused independently from physical settlement.
            if let Err(error) = review_controller::publish_cancellation(
                &self.review,
                workflow.revision,
                cancellation,
                &mut apply,
            ) {
                tracing::debug!(%error, "Retired native review publication refused");
            }
        }
        // Genuine original helper+IO+compute proof permits releasing this ONE
        // workflow, including all ledger receipts and original effect inventory.
        drop(delegated_presentation);
        self.workflow = None;
        Ok(())
    }
    pub(super) fn render(
        &mut self,
        request: &RenderRequest,
        sequence: u64,
        stop: &StopToken,
    ) -> Result<Option<PluginFrame>, String> {
        stopped(stop)?;
        if self
            .failure
            .as_ref()
            .is_some_and(|(revision, _)| *revision == request.revision)
        {
            return Err(self
                .failure
                .as_ref()
                .map(|(_, message)| message.clone())
                .unwrap_or_default());
        }
        let same = self
            .workflow
            .as_ref()
            .is_some_and(|workflow| workflow.revision == request.revision)
            || self
                .setup
                .as_ref()
                .is_some_and(|(revision, _)| *revision == request.revision);
        if !same {
            self.stop();
            self.settle_retirement(false)?;
            self.start_setup(request)?;
            return Ok(None);
        }
        let Some(workflow) = self.workflow.as_mut() else {
            return Ok(None);
        };
        if workflow.halted {
            return Ok(None);
        }
        if request
            .settings
            .plugin
            .selected
            .as_ref()
            .is_some_and(|selected| selected.mode == AnimationMode::PreRendered)
        {
            let Some(player) = workflow.player.as_mut() else {
                return Ok(None);
            };
            // The request Instant is the monotonic playback clock. UI elapsed
            // may be forced to zero by Motion Off or re-quantized by a settings
            // change; it is only a control signal and never a clock timestamp.
            let origin = *workflow.playback_origin.get_or_insert(request.requested_at);
            let previous_tick = workflow.last_playback_tick.unwrap_or(Duration::ZERO);
            let tick = playback_tick(origin, previous_tick, request.requested_at)?;
            let playback_now = if request.elapsed == Duration::ZERO {
                if !workflow.playback_frozen {
                    player
                        .pause(previous_tick)
                        .map_err(|error| error.to_string())?;
                    workflow.playback_frozen = true;
                }
                previous_tick
            } else {
                if workflow.playback_frozen {
                    player.resume(tick).map_err(|error| error.to_string())?;
                    workflow.playback_frozen = false;
                } else if workflow
                    .last_playback_elapsed
                    .is_some_and(|elapsed| request.elapsed < elapsed)
                {
                    // An explicit UI timeline rewind invalidates old leases;
                    // the host tick itself remains monotonic for clock.reset.
                    player.seek(tick, 0.0).map_err(|error| error.to_string())?;
                }
                tick
            };
            let (playback, clock) = player
                .sample(playback_now)
                .map_err(|error| error.to_string())?;
            workflow.last_playback_tick = Some(tick);
            workflow.last_playback_elapsed = Some(request.elapsed);
            let Playback::Frame(lease) = playback else {
                return Ok(None);
            };
            let instance = workflow
                .controller
                .delegated_instance()
                .ok_or("Original replay instance missing")?;
            let instance = instance
                .try_lock()
                .map_err(|_| "Replay instance still in preparation or poisoned".to_owned())?;
            if !instance.is_physically_retired() {
                return Err("Replay helper has not physically retired".into());
            }
            let shape = shape(instance.plan(), request.width, request.height)?;
            let mut cells = cells_from_replay(
                shape,
                &lease,
                &request.settings,
                Duration::from_secs_f64(clock.time),
            )?;
            overlay_native_text(
                &mut cells,
                shape,
                lease.text().map_err(|error| error.to_string())?.iter(),
            )?;
            let authority = instance
                .frame_authority()
                .ok_or("Original replay activation retired")?;
            let identity = instance
                .active_identity()
                .ok_or("Original replay package identity retired")?;
            let identity = PluginFrameIdentity {
                package_id: identity.id().into(),
                package_digest: identity.digest().into(),
                verified_ilium: identity.is_ilium(),
                instance_id: authority.instance_id,
                revision: request.revision,
                plan_generation: authority.plan_generation,
                authorization_epoch: authority.authorization_epoch,
            };
            let authority = instance
                .retain_frame_authority()
                .map_err(|error| error.to_string())?;
            let resident_bytes = snapshot_cells_bytes(&cells, cells.capacity())
                .and_then(|bytes| {
                    bytes.checked_add(
                        std::mem::size_of::<PluginFrameIdentity>()
                            + identity.package_id.capacity()
                            + identity.package_digest.capacity(),
                    )
                })
                .ok_or("Replay frame size overflow")?;
            return Ok(Some(PluginFrame {
                identity,
                authority,
                cells,
                frames_per_second: instance.plan().fps.ceil().clamp(1., 120.) as u32,
                resident_bytes,
                replay: Some(lease),
            }));
        }
        let Some(presentation) = &mut workflow.presentation else {
            return Ok(None);
        };
        let instance = workflow
            .controller
            .package_instance_mut()
            .ok_or_else(|| "Native permission creation is not accepted".to_owned())?;
        presentation.creation = instance.pump().map_err(|error| error.to_string())?;
        presentation.dispatch_requests(instance)?;
        if presentation.creation == CreateState::Ready {
            self.review
                .set_phase(workflow.revision, ReviewPhase::Ready, None)?;
        }
        presentation.render(instance, request, sequence, stop)
    }
}
/// Convert script-authored error diagnostics into the bounded runtime error
/// channel used for rejected frames. Warnings and progress remain informational.
fn status_error_message(status: &serde_json::Value) -> Option<String> {
    let records = status.get("records")?.as_array()?;
    let messages = records
        .iter()
        .filter_map(|record| {
            (record.get("level").and_then(serde_json::Value::as_str) == Some("error"))
                .then(|| record.get("message").and_then(serde_json::Value::as_str))
                .flatten()
        })
        .collect::<Vec<_>>();
    (!messages.is_empty()).then(|| messages.join("; ").chars().take(240).collect())
}

/// Observations are native outcomes, not authority. Lost/cleanup uncertainty
/// remains retained by NativeHttpHost and prevents physical replacement.
fn observe_http_events(events: Vec<HttpEvent>) -> Result<(), String> {
    for event in events {
        match event.observation {
            HttpObservation::Lost => {
                return Err("Original native HTTP receipt lost; retirement is unproven".into())
            }
            HttpObservation::CleanupFailed => {
                return Err("Original native HTTP ticket cleanup failed; owner retained".into())
            }
            HttpObservation::Refused(code) => {
                tracing::debug!(
                    request_id = event.request_id,
                    code,
                    "Native HTTP delivery withheld"
                );
            }
            HttpObservation::Completion(_) => {}
        }
    }
    Ok(())
}
fn apply_before_creation(invalidation: &Invalidation) -> Result<(), String> {
    // No acquisition/compute/draw owner is constructed before accepted create.
    // A nonempty native operation inventory cannot be discarded as a no-op.
    if !invalidation.operations.is_empty() {
        return Err("Original native operations require their actual retirement owner".into());
    }
    Ok(())
}
impl Presentation {
    fn new(
        instance: &mut PackageInstance,
        request: &RenderRequest,
        quota: &QuotaGroup,
        resources: ilium_ambient::resources::AmbientResources,
        state_root: Arc<PinnedDirectory>,
        selected_audio: Option<QualifiedCaptureBinding>,
        live_mode: bool,
        creation: CreateState,
        actor_wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, String> {
        let request_bytes = instance
            .engine_limits()
            .pending_requests
            .checked_mul(std::mem::size_of::<HostRequest>())
            .and_then(|bytes| bytes.checked_mul(2))
            .and_then(|bytes| bytes.checked_add(512))
            .ok_or("Native request staging size overflow")?;
        let request_storage = quota
            .reserve_external_storage(request_bytes)
            .map_err(|error| format!("Native request staging admission: {error:?}"))?;
        let http = NativeHttpHost::new(
            instance,
            resources.finite().clone(),
            Arc::new(SystemDns),
            None, // No configured native credential backend; never invent/read secrets.
        )
        .map_err(|error| error.to_string())?;
        let source_client = resources.finite().clone();
        let compute = NativeComputeHost::new(
            resources.clone(),
            quota.clone(),
            instance.engine_limits().clone(),
        )
        .map_err(|error| error.to_string())?;
        let sources = NativeSourceOwner::new(source_client, quota.clone())?;

        let shape = shape(instance.plan(), request.width, request.height)?;
        let layout = shape.layout().map_err(|error| error.to_string())?;
        // Two canonical states, binary seed/handoff copies, and nonlocal dither
        // scratch. Immutable UI packing has its separate existing FramePermit.
        let peak = layout
            .canonical_bytes
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(layout.handoff_bytes.checked_mul(2)?))
            .and_then(|bytes| bytes.checked_add(layout.dots.checked_mul(16)?))
            .ok_or_else(|| "Plugin surface admission overflow".to_owned())?;
        let surface_storage = quota
            .reserve_external_storage(peak)
            .map_err(|error| format!("Plugin surface admission: {error:?}"))?;
        let surface = Surface::new(
            instance
                .frame_authority()
                .ok_or("Accepted native authority missing")?
                .instance_id,
            request.revision,
            shape,
        )
        .map_err(|error| error.to_string())?;
        let clock = AnimationClock::new(
            request.elapsed,
            f64::from(request.settings.speed_percent) / 100.,
        )
        .map_err(|error| error.to_string())?;
        let drawing =
            NativeDrawHost::new(quota.clone(), MediaLimits::default(), DrawLimits::default())
                .map_err(|error| error.to_string())?;
        let images = NativeImageHost::new(quota.clone()).map_err(|error| error.to_string())?;
        let assets = NativeAssetHost::new(
            instance,
            resources.finite().clone(),
            quota.clone(),
            state_root,
        )
        .map_err(|error| error.to_string())?;
        let video = NativeVideoHost::new(
            resources.finite().clone(),
            quota.clone(),
            shape,
            if live_mode {
                AnimationMode::Live
            } else {
                AnimationMode::PreRendered
            },
            StopToken::default(),
            actor_wake,
        )
        .map_err(|error| error.to_string())?;
        let inputs =
            NativeFrameInputs::new(instance, quota.clone()).map_err(|error| error.to_string())?;
        let tasks = NativeTaskHost::new(quota.clone(), instance.engine_limits().clone())
            .map_err(|error| error.to_string())?;
        let world_epoch = instance
            .frame_authority()
            .ok_or_else(|| "Accepted native authority missing for world service".to_owned())?
            .authorization_epoch;
        let world_limits = instance.engine_limits().clone();
        // Opening the audio job is the last fallible step. On success the
        // original receipt and reserved physical-close slot enter Presentation.
        let audio = AudioOwner::new(instance, selected_audio, &resources, quota, live_mode)?;
        Ok(Self {
            surface,
            clock,
            compute,
            http,
            sources,
            drawing,
            images,
            assets,
            video,
            inputs,
            audio,
            tasks,
            live_mode,
            world: None,
            world_resources: resources,
            world_epoch,
            world_limits,
            saved_context: None,
            presentation: None,
            creation,
            unhandled: None,
            undispatched: Vec::new(),
            _surface_storage: surface_storage,
            _request_storage: request_storage,
        })
    }
}
/// Cached native observations only. Device audio acquisition requires a separate
/// admitted owner; observer location reuses the already configured native value.
struct WorkerInputProvider<'a> {
    request: &'a RenderRequest,
    audio: &'a mut AudioOwner,
}
impl CachedInputProvider for WorkerInputProvider<'_> {
    fn pointer(&mut self) -> ilium_animation_js::error::Result<Option<PointerObservation>> {
        Ok(self.request.pointer.map(|normalized| PointerObservation {
            normalized,
            buttons: None,
        }))
    }
    fn occlusion(&mut self) -> ilium_animation_js::error::Result<Option<OcclusionObservation>> {
        Ok(self
            .request
            .occupancy
            .as_ref()
            .map(|mask| OcclusionObservation {
                mask: Arc::clone(mask),
                viewport_revision: self.request.revision,
                occupancy_revision: self.request.occupancy_revision,
            }))
    }
    fn clock(
        &mut self,
        civil: bool,
    ) -> ilium_animation_js::error::Result<Option<ClockObservation>> {
        if !civil {
            return Ok(None);
        }
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| {
                ilium_animation_js::error::AnimationError::Runtime(
                    "Civil clock precedes Unix epoch".into(),
                )
            })?
            .as_millis();
        let epoch_ms = i64::try_from(millis).map_err(|_| {
            ilium_animation_js::error::AnimationError::Runtime("Civil clock range".into())
        })?;
        Ok(Some(ClockObservation {
            epoch_ms: Some(epoch_ms),
            timezone: Some("UTC".into()),
        }))
    }
    fn location(&mut self) -> ilium_animation_js::error::Result<Option<LocationObservation>> {
        // Called only for a granted location demand by NativeFrameInputs::prepare.
        // Reuse the native scene observer; no device lookup or IO is needed.
        let location = self.request.settings.ambient.location.normalized();
        Ok(Some(LocationObservation {
            latitude: location.latitude,
            longitude: location.longitude,
            altitude_m: None,
            accuracy_m: None,
        }))
    }
    fn audio(
        &mut self,
        _: &ilium_animation_js::plan::AudioDemand,
    ) -> ilium_animation_js::error::Result<Option<Arc<RetainedAudioSnapshot>>> {
        self.audio.snapshot()
    }
}

impl Presentation {
    fn dispatch_requests(&mut self, instance: &mut PackageInstance) -> Result<(), String> {
        // A completion can synchronously queue the next native request during
        // the one authorized pump. Drain that chain in bounded actor turns so
        // a task next issued after open does not wait for an unrelated frame.
        for _ in 0..64 {
            if self.unhandled.is_some() {
                return Err(
                    "An original native service request is awaiting its actual owner".into(),
                );
            }
            let requests = instance.requests().map_err(|error| error.to_string())?;
            if requests.is_empty() {
                return Ok(());
            }
            let mut requests = requests.into_iter();
            while let Some(request) = requests.next() {
                // Each actual adapter consumes only its own method. Unrelated
                // original requests move intact to the next owner, never copied
                // into JSON/fabricated demand IDs or replaced with fake responses.
                let task = self
                    .tasks
                    .dispatch(instance, request)
                    .map_err(|error| error.to_string());
                let request = match task {
                    Ok(None) => continue,
                    Ok(Some(request)) => request,
                    Err(error) => {
                        self.undispatched = requests.collect();
                        return Err(error);
                    }
                };
                // World operations own their own selected-world registry and
                // frame/source custody.  Route them before the generic asset
                // and media adapters so a world frame can borrow the original
                // draw/asset owners without being mistaken for an image call.
                if request.method.starts_with("worlds.") {
                    if !self.live_mode
                        || (self.world.is_none()
                            && !matches!(request.method.as_str(), "worlds.open" | "worlds.list"))
                    {
                        self.unhandled = Some(request);
                        self.undispatched = requests.collect();
                        return Err("Native world owner is not admitted for this request".into());
                    }
                    if self.world.is_none() {
                        let world = NativeWorldHost::new(
                            self.world_resources.clone(),
                            self.sources.client.quota_group(),
                            self.world_epoch,
                            self.world_limits.clone(),
                            AnimationMode::Live,
                        )
                        .map_err(|error| error.to_string())?;
                        let mut world = world;
                        if let Some(context) = self.saved_context.clone() {
                            world
                                .set_saved_context(context)
                                .map_err(|error| error.to_string())?;
                        }
                        self.world = Some(world);
                    }
                    let world = self.world.as_mut().ok_or_else(|| {
                        "Native world service is available only in live mode".to_owned()
                    })?;
                    let request = world
                        .dispatch(instance, request, &mut self.drawing, Some(&mut self.assets))
                        .map_err(|error| error.to_string())?;
                    if let Some(request) = request {
                        self.unhandled = Some(request);
                        self.undispatched = requests.collect();
                        return Err("Native world adapter left an unhandled request".into());
                    }
                    continue;
                }
                // Presentation is a separate, lazy receipt subscription.  It
                // must never be opened merely because a package has a world
                // capability or because a frame is being rendered.
                if request.method.starts_with("presentation.") {
                    if !self.live_mode {
                        return Err(
                            "Native presentation subscriptions require live animation mode".into(),
                        );
                    }
                    if self.presentation.is_none() && request.method != "presentation.subscribe" {
                        self.unhandled = Some(request);
                        self.undispatched = requests.collect();
                        return Err("Native presentation subscription is not open".into());
                    }
                    if self.presentation.is_none() {
                        let limits = instance.engine_limits().clone();
                        let quota = self.sources.client.quota_group();
                        let receipts = NativePresentationHost::new(instance, quota, limits)
                            .map_err(|error| error.to_string())?;
                        self.presentation = Some(receipts);
                    }
                    let receipts = self
                        .presentation
                        .as_mut()
                        .ok_or("Native presentation owner missing")?;
                    let request = receipts
                        .dispatch(instance, request)
                        .map_err(|error| error.to_string())?;
                    if let Some(request) = request {
                        self.unhandled = Some(request);
                        self.undispatched = requests.collect();
                        return Err("Native presentation adapter left an unhandled request".into());
                    }
                    continue;
                }
                let compute = self
                    .compute
                    .dispatch(instance, request)
                    .map_err(|error| error.to_string());
                let request = match compute {
                    Ok(None) => continue,
                    Ok(Some(request)) => request,
                    Err(error) => {
                        self.undispatched = requests.collect();
                        return Err(error);
                    }
                };
                let http = self
                    .http
                    .dispatch(instance, request)
                    .map_err(|error| error.to_string());
                let request = match http {
                    Ok(None) => continue,
                    Ok(Some(request)) => request,
                    Err(error) => {
                        self.undispatched = requests.collect();
                        return Err(error);
                    }
                };
                let source = self
                    .sources
                    .dispatch(instance, request, &mut self.drawing)
                    .map_err(|error| error.to_string());
                let request = match source {
                    Ok(None) => continue,
                    Ok(Some(request)) => request,
                    Err(error) => {
                        self.undispatched = requests.collect();
                        return Err(error);
                    }
                };
                // Video needs the original asset/HTTP owners and must see its
                // own image-close identities before the generic image owner.
                let video = self
                    .video
                    .dispatch(
                        instance,
                        &self.assets,
                        &mut self.http,
                        &mut self.drawing,
                        request.clone(),
                    )
                    .map_err(|error| error.to_string());
                let request = match video {
                    Ok(None) => continue,
                    Ok(Some(request)) => request,
                    Err(error) => {
                        self.unhandled = Some(request);
                        self.undispatched = requests.collect();
                        return Err(error);
                    }
                };
                let image = self
                    .images
                    .dispatch(instance, &mut self.drawing, request)
                    .map_err(|error| error.to_string());
                let request = match image {
                    Ok(None) => continue,
                    Ok(Some(request)) => request,
                    Err(error) => {
                        self.undispatched = requests.collect();
                        return Err(error);
                    }
                };
                let asset = self
                    .assets
                    .dispatch(instance, request)
                    .map_err(|error| error.to_string());
                let request = match asset {
                    Ok(None) => continue,
                    Ok(Some(request)) => request,
                    Err(error) => {
                        self.undispatched = requests.collect();
                        return Err(error);
                    }
                };
                let message = format!(
                    "Native service adapter is not yet wired: {}",
                    request.method
                );
                self.unhandled = Some(request);
                self.undispatched = requests.collect();
                return Err(message);
            }
            self.creation = instance.pump().map_err(|error| error.to_string())?;
        }
        let remaining = instance.requests().map_err(|error| error.to_string())?;
        if remaining.is_empty() {
            return Ok(());
        }
        self.undispatched = remaining;
        Err("Native request continuation chain exceeded actor turn bound".into())
    }

    fn render(
        &mut self,
        instance: &mut PackageInstance,
        request: &RenderRequest,
        sequence: u64,
        stop: &StopToken,
    ) -> Result<Option<PluginFrame>, String> {
        stopped(stop)?;
        // Retain one accepted native instance through asynchronous create. A
        // pending promise does not become a failure or launch a replacement.
        if self.creation != CreateState::Ready {
            return Ok(None);
        }
        self.sources.begin_due(instance)?;

        self.clock
            .set_speed(
                request.elapsed,
                f64::from(request.settings.speed_percent) / 100.,
            )
            .map_err(|error| error.to_string())?;
        let clock = self
            .clock
            .sample(request.elapsed)
            .map_err(|error| error.to_string())?;
        let seed = self
            .surface
            .begin(sequence)
            .map_err(|error| error.to_string())?;
        let layout = seed.shape.layout().map_err(|error| error.to_string())?;
        let data_kind = if seed.shape.format == Format::Gray32 {
            TypedArrayKind::F32
        } else {
            TypedArrayKind::U8
        };
        let mut seed_planes = BTreeMap::new();
        seed_planes.insert("work_data".into(), encode_data(seed.data));
        let mut seed_arrays = vec![array("work_data", data_kind, layout.elements)];
        if let Some(rgb) = seed.cell_rgb {
            seed_planes.insert("work_cell_rgb".into(), rgb);
            seed_arrays.push(array("work_cell_rgb", TypedArrayKind::U8, layout.cells * 3));
        }
        let native_status = self
            .compute
            .snapshots(instance)
            .map_err(|error| error.to_string())?;
        let source_status = self
            .sources
            .snapshots(instance)
            .map_err(|error| error.to_string())?;
        let task_status = self
            .tasks
            .snapshots(instance)
            .map_err(|error| error.to_string())?;
        let presentation_status = self
            .presentation
            .as_ref()
            .map(|host| host.snapshots(instance))
            .transpose()
            .map_err(|error| error.to_string())?;
        let world_status = self
            .world
            .as_mut()
            .map(|host| host.snapshots(instance))
            .transpose()
            .map_err(|error| error.to_string())?;
        let video_position = Duration::try_from_secs_f64(clock.time).map_err(|_| {
            "Native Video playback position is outside the finite clip bound".to_owned()
        })?;
        let video_status = self
            .video
            .snapshots(instance, &mut self.drawing, video_position)
            .map_err(|error| error.to_string())?;
        let service_copy_bytes = native_status
            .wire_bytes()
            .checked_add(source_status.wire_bytes())
            .and_then(|bytes| bytes.checked_add(task_status.wire_bytes()))
            .and_then(|bytes| {
                bytes.checked_add(
                    presentation_status
                        .as_ref()
                        .map_or(0, |value| value.wire_bytes()),
                )
            })
            .and_then(|bytes| {
                bytes.checked_add(world_status.as_ref().map_or(0, |value| value.wire_bytes()))
            })
            .and_then(|bytes| bytes.checked_add(video_status.wire_bytes()))
            .and_then(|bytes| bytes.checked_add(64 * 1024))
            .ok_or_else(|| "Native service seed size overflow".to_owned())?;
        let _service_seed_storage = self
            .sources
            .client
            .quota_group()
            .reserve_external_storage(service_copy_bytes)
            .map_err(|error| format!("Native service seed admission: {error:?}"))?;
        let mut services = native_status
            .metadata()
            .as_array()
            .ok_or_else(|| "Native compute service snapshot schema".to_owned())?
            .clone();
        services.extend(
            source_status
                .metadata()
                .as_array()
                .ok_or_else(|| "Native source service snapshot schema".to_owned())?
                .iter()
                .cloned(),
        );
        services.extend(
            task_status
                .metadata()
                .as_array()
                .ok_or_else(|| "Native task snapshot schema".to_owned())?
                .iter()
                .cloned(),
        );
        if let Some(status) = &presentation_status {
            services.extend(
                status
                    .metadata()
                    .as_array()
                    .ok_or_else(|| "Native presentation snapshot schema".to_owned())?
                    .iter()
                    .cloned(),
            );
        }
        if let Some(status) = &world_status {
            services.extend(
                status
                    .metadata()
                    .as_array()
                    .ok_or_else(|| "Native world snapshot schema".to_owned())?
                    .iter()
                    .cloned(),
            );
        }
        services.extend(
            video_status
                .metadata()
                .as_array()
                .ok_or_else(|| "Native Video service snapshot schema".to_owned())?
                .iter()
                .cloned(),
        );
        if services.len() > 64 {
            return Err("Native service seed inventory exceeds helper bound".into());
        }
        let input_packet = self
            .inputs
            .prepare(
                instance,
                seed.shape,
                request.revision,
                request.requested_at,
                &mut WorkerInputProvider {
                    request,
                    audio: &mut self.audio,
                },
            )
            .map_err(|error| error.to_string())?;
        let input_bytes = input_packet
            .value
            .planes()
            .values()
            .try_fold(128 * 1024usize, |total, plane| {
                total.checked_add(plane.len())
            })
            .ok_or_else(|| "Input seed storage overflow".to_owned())?;
        // The distinct seed copy is admitted while the immutable producer packet
        // is still retained; original payload aliases never become free storage.
        let _input_seed_storage = instance
            .reserve_input_seed_storage(input_bytes)
            .map_err(|error| error.to_string())?;
        seed_arrays.extend_from_slice(input_packet.value.arrays());
        for (name, bytes) in input_packet.value.planes() {
            seed_planes.insert(name.clone(), bytes.clone());
        }
        let metadata = json!({"frame":{"key":seed.key,"shape":seed.shape,"reset":seed.reset,"invalid_rects":seed.invalid_rects,"input_specs":input_packet.value.metadata()["input_specs"]},"services":services,"random_seed":instance.settings().get("seed").and_then(serde_json::Value::as_u64).unwrap_or(0).to_string()});
        instance
            .seed_frame(&metadata, &seed_arrays, &seed_planes)
            .map_err(|error| error.to_string())?;
        // The trusted seed hook has now applied each native closed task
        // observation. Failed seeds retain the terminal records for retry.
        self.tasks.acknowledge_seeded_snapshots();
        if let Some(presentation) = self.presentation.as_mut() {
            presentation.acknowledge_seeded_snapshots();
        }
        if let Some(world) = self.world.as_mut() {
            world.acknowledge_seeded_snapshots();
        }
        self.video
            .acknowledge_seeded_snapshots(&mut self.drawing)
            .map_err(|error| error.to_string())?;
        let context = json!({"_ilium_frame":{"key":seed.key,"shape":seed.shape},"viewport":{"cell_width":seed.shape.cell_width,"cell_height":seed.shape.cell_height,"dot_width":seed.shape.cell_width*2,"dot_height":seed.shape.cell_height*4,"revision":request.revision},"time":clock.time,"wall":clock.wall,"delta":clock.delta,"wall_delta":clock.wall_delta,"settings":instance.settings(),"visible":true,"render_policy":{"can_skip_occluded":false},"inputs":input_packet.value.metadata()["inputs"]});
        let mut arrays = vec![
            array("work_data", data_kind, layout.elements),
            array("data", data_kind, layout.elements),
            array("work_touch", TypedArrayKind::U8, layout.samples),
            array("touch", TypedArrayKind::U8, layout.samples),
            array("work_order", TypedArrayKind::U32, layout.samples),
            array("order", TypedArrayKind::U32, layout.samples),
        ];
        if seed.shape.cell_rgb {
            for (name, kind, elements) in [
                ("work_cell_rgb", TypedArrayKind::U8, layout.cells * 3),
                ("cell_rgb", TypedArrayKind::U8, layout.cells * 3),
                ("work_colour_touch", TypedArrayKind::U8, layout.cells),
                ("colour_touch", TypedArrayKind::U8, layout.cells),
                ("work_colour_order", TypedArrayKind::U32, layout.cells),
                ("colour_order", TypedArrayKind::U32, layout.cells),
            ] {
                arrays.push(array(name, kind, elements));
            }
        }
        arrays.extend_from_slice(input_packet.value.arrays());
        let retained = instance
            .render(&context, &arrays)
            .map_err(|error| error.to_string())?;
        let (mut output, _retained_storage) = retained.into_parts();
        if let Some(status) = instance.take_status().map_err(|error| error.to_string())? {
            if let Some(message) = status_error_message(&status) {
                instance
                    .accept_frame(false)
                    .map_err(|error| error.to_string())?;
                return Err(format!("Plugin runtime error: {message}"));
            }
        }
        let frame_metadata = FrameMeta::parse(
            &serde_json::to_vec(&output.metadata).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let planes = decode_planes(&mut output.planes, seed.shape.format, seed.shape.cell_rgb)?;
        stopped(stop)?;
        let mut prepared = None;
        let outcome = self
            .drawing
            .finish(
                instance,
                &mut self.surface,
                frame_metadata,
                planes,
                stop,
                |snapshot, _| {
                    prepared = Some(
                        pack_cells(snapshot, &request.settings, request.elapsed)
                            .map_err(|_| ilium_animation_js::surface::SurfaceError::Capacity)?,
                    );
                    Ok(())
                },
            )
            .map_err(|error| error.to_string())?;
        instance
            .accept_frame(outcome.accepted)
            .map_err(|error| error.to_string())?;
        self.dispatch_requests(instance)?;
        if !outcome.accepted {
            return Ok(None);
        }
        stopped(stop)?;
        let cells =
            prepared.ok_or_else(|| "Native plugin publication was not staged".to_owned())?;
        let authority = instance
            .frame_authority()
            .ok_or_else(|| "Animation authority retired before publication".to_owned())?;
        let identity = instance
            .active_identity()
            .ok_or_else(|| "Animation identity retired before publication".to_owned())?;
        let identity = PluginFrameIdentity {
            package_id: identity.id().into(),
            package_digest: identity.digest().into(),
            verified_ilium: identity.is_ilium(),
            instance_id: authority.instance_id,
            revision: request.revision,
            plan_generation: authority.plan_generation,
            authorization_epoch: authority.authorization_epoch,
        };
        let authority = instance
            .retain_frame_authority()
            .map_err(|error| error.to_string())?;
        let bytes = snapshot_cells_bytes(&cells, cells.capacity())
            .and_then(|bytes| {
                bytes.checked_add(
                    std::mem::size_of::<PluginFrameIdentity>()
                        + identity.package_id.capacity()
                        + identity.package_digest.capacity(),
                )
            })
            .ok_or_else(|| "Plugin publication size overflow".to_owned())?;
        Ok(Some(PluginFrame {
            identity,
            authority,
            cells,
            frames_per_second: instance.plan().fps.ceil().clamp(1., 120.) as u32,
            resident_bytes: bytes,
            replay: None,
        }))
    }
    fn render_prepared(
        &mut self,
        instance: &mut PackageInstance,
        request: &RenderRequest,
        sample: ilium_animation_js::replay::ReplaySample,
        preparation: &mut ilium_animation_js::replay::ClipPreparation,
        quota: &QuotaGroup,
        stop: &StopToken,
    ) -> Result<(), String> {
        stopped(stop)?;
        // Retain one accepted native instance through asynchronous create. A
        // pending promise does not become a failure or launch a replacement.
        if self.creation != CreateState::Ready {
            return Err("Replay creation is not ready".into());
        }
        if self.video.pre_render_video_attempted() && !self.video.recording_ready() {
            return Err("Pre-rendered Video has no recorded native FrozenEvidence".into());
        }
        let clock = ilium_animation_js::clock::ClockSample {
            time: sample.time,
            wall: sample.wall,
            delta: sample.delta,
            wall_delta: sample.delta,
            suspended: false,
        };
        let sequence = u64::try_from(sample.index())
            .ok()
            .and_then(|index| index.checked_add(1))
            .ok_or("Replay sequence overflow")?;
        let seed = self
            .surface
            .begin(sequence)
            .map_err(|error| error.to_string())?;
        let layout = seed.shape.layout().map_err(|error| error.to_string())?;
        let data_kind = if seed.shape.format == Format::Gray32 {
            TypedArrayKind::F32
        } else {
            TypedArrayKind::U8
        };
        let mut seed_planes = BTreeMap::new();
        seed_planes.insert("work_data".into(), encode_data(seed.data));
        let mut seed_arrays = vec![array("work_data", data_kind, layout.elements)];
        if let Some(rgb) = seed.cell_rgb {
            seed_planes.insert("work_cell_rgb".into(), rgb);
            seed_arrays.push(array("work_cell_rgb", TypedArrayKind::U8, layout.cells * 3));
        }
        let native_status = self
            .compute
            .snapshots(instance)
            .map_err(|error| error.to_string())?;
        let video_position = Duration::try_from_secs_f64(sample.time)
            .map_err(|_| "Recorded Video playback position is invalid".to_owned())?;
        let video_status = self
            .video
            .snapshots(instance, &mut self.drawing, video_position)
            .map_err(|error| error.to_string())?;
        let service_copy_bytes = native_status
            .wire_bytes()
            .checked_add(video_status.wire_bytes())
            .and_then(|bytes| bytes.checked_add(64 * 1024))
            .ok_or("Recorded Video replay service seed size overflow")?;
        let _service_seed_storage = quota
            .reserve_external_storage(service_copy_bytes)
            .map_err(|error| format!("Recorded Video replay service seed admission: {error:?}"))?;
        let mut services = native_status
            .metadata()
            .as_array()
            .ok_or("Native compute replay snapshot schema")?
            .clone();
        services.extend(
            video_status
                .metadata()
                .as_array()
                .ok_or("Recorded Video replay snapshot schema")?
                .iter()
                .cloned(),
        );
        if services.len() > 64 {
            return Err("Recorded Video replay service inventory exceeds helper bound".into());
        }
        let input_packet = self
            .inputs
            .prepare(
                instance,
                seed.shape,
                request.revision,
                request.requested_at,
                &mut WorkerInputProvider {
                    request,
                    audio: &mut self.audio,
                },
            )
            .map_err(|error| error.to_string())?;
        let input_bytes = input_packet
            .value
            .planes()
            .values()
            .try_fold(128 * 1024usize, |total, plane| {
                total.checked_add(plane.len())
            })
            .ok_or_else(|| "Input seed storage overflow".to_owned())?;
        // The distinct seed copy is admitted while the immutable producer packet
        // is still retained; original payload aliases never become free storage.
        let _input_seed_storage = instance
            .reserve_input_seed_storage(input_bytes)
            .map_err(|error| error.to_string())?;
        seed_arrays.extend_from_slice(input_packet.value.arrays());
        for (name, bytes) in input_packet.value.planes() {
            seed_planes.insert(name.clone(), bytes.clone());
        }
        let metadata = json!({"frame":{"key":seed.key,"shape":seed.shape,"reset":seed.reset,"invalid_rects":seed.invalid_rects,"input_specs":input_packet.value.metadata()["input_specs"]},"services":services,"random_seed":instance.settings().get("seed").and_then(serde_json::Value::as_u64).unwrap_or(0).to_string()});
        instance
            .seed_frame(&metadata, &seed_arrays, &seed_planes)
            .map_err(|error| error.to_string())?;
        self.video
            .acknowledge_seeded_snapshots(&mut self.drawing)
            .map_err(|error| error.to_string())?;
        let context = json!({"_ilium_frame":{"key":seed.key,"shape":seed.shape},"viewport":{"cell_width":seed.shape.cell_width,"cell_height":seed.shape.cell_height,"dot_width":seed.shape.cell_width*2,"dot_height":seed.shape.cell_height*4,"revision":request.revision},"time":clock.time,"wall":clock.wall,"delta":clock.delta,"wall_delta":clock.wall_delta,"settings":instance.settings(),"visible":true,"render_policy":{"can_skip_occluded":false},"inputs":input_packet.value.metadata()["inputs"]});
        let mut arrays = vec![
            array("work_data", data_kind, layout.elements),
            array("data", data_kind, layout.elements),
            array("work_touch", TypedArrayKind::U8, layout.samples),
            array("touch", TypedArrayKind::U8, layout.samples),
            array("work_order", TypedArrayKind::U32, layout.samples),
            array("order", TypedArrayKind::U32, layout.samples),
        ];
        if seed.shape.cell_rgb {
            for (name, kind, elements) in [
                ("work_cell_rgb", TypedArrayKind::U8, layout.cells * 3),
                ("cell_rgb", TypedArrayKind::U8, layout.cells * 3),
                ("work_colour_touch", TypedArrayKind::U8, layout.cells),
                ("colour_touch", TypedArrayKind::U8, layout.cells),
                ("work_colour_order", TypedArrayKind::U32, layout.cells),
                ("colour_order", TypedArrayKind::U32, layout.cells),
            ] {
                arrays.push(array(name, kind, elements));
            }
        }
        arrays.extend_from_slice(input_packet.value.arrays());
        let retained = instance
            .render(&context, &arrays)
            .map_err(|error| error.to_string())?;
        let (mut output, _retained_storage) = retained.into_parts();
        if let Some(status) = instance.take_status().map_err(|error| error.to_string())? {
            if let Some(message) = status_error_message(&status) {
                instance
                    .accept_frame(false)
                    .map_err(|error| error.to_string())?;
                return Err(format!("Plugin runtime error: {message}"));
            }
        }
        let frame_metadata = FrameMeta::parse(
            &serde_json::to_vec(&output.metadata).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let planes = decode_planes(&mut output.planes, seed.shape.format, seed.shape.cell_rgb)?;
        stopped(stop)?;
        let _dither_storage =
            if request.settings.dither.is_error_diffusion() && seed.shape.mode == Mode::Pixels {
                let dots = usize::try_from(seed.shape.cell_width)
                    .ok()
                    .and_then(|width| width.checked_mul(2))
                    .and_then(|width| {
                        usize::try_from(seed.shape.cell_height)
                            .ok()
                            .and_then(|height| height.checked_mul(4))
                            .and_then(|height| width.checked_mul(height))
                    })
                    .ok_or("Replay dither dimensions overflow")?;
                let bytes = dots
                    .checked_mul(std::mem::size_of::<f32>() + std::mem::size_of::<bool>())
                    .and_then(|bytes| bytes.checked_add(8192))
                    .ok_or("Replay dither storage overflow")?;
                Some(
                    quota
                        .reserve_external_storage(bytes)
                        .map_err(|error| format!("Replay dither admission: {error:?}"))?,
                )
            } else {
                None
            };
        let mut sample = Some(sample);
        let mut captured = None;
        let mut pack_error = None;
        let outcome = self
            .drawing
            .finish(
                instance,
                &mut self.surface,
                frame_metadata,
                planes,
                stop,
                |snapshot, _| {
                    let width = snapshot.shape().cell_width as usize * 2;
                    let height = snapshot.shape().cell_height as usize * 4;
                    let density = f32::from(request.settings.density_percent) / 100.;
                    let tone = |value: f32, _: usize, _: usize| {
                        request.settings.appearance.shape_dot(value) * density
                    };
                    let diffused = if request.settings.dither.is_error_diffusion()
                        && snapshot.shape().mode == Mode::Pixels
                    {
                        let mut coverage = vec![0.; width * height];
                        snapshot.pack(tone, |value, x, y| {
                            coverage[y * width + x] = value;
                            false
                        })?;
                        let mut diffused = Vec::new();
                        ilium_ambient::dither::diffuse(
                            &coverage,
                            width,
                            height,
                            1.,
                            request.settings.dither,
                            &mut diffused,
                        );
                        Some(diffused)
                    } else {
                        None
                    };
                    let result = preparation.capture_snapshot(
                        sample
                            .as_ref()
                            .ok_or(ilium_animation_js::surface::SurfaceError::Stale)?,
                        snapshot,
                        tone,
                        |value, x, y| {
                            diffused.as_ref().map_or_else(
                                || {
                                    value
                                        >= ilium_ambient::raster::threshold(
                                            x,
                                            y,
                                            request.settings.dither,
                                        )
                                },
                                |bits| bits[y * width + x],
                            )
                        },
                    );
                    match result {
                        Ok(packed) => captured = Some(packed),
                        Err(error) => {
                            pack_error = Some(error.to_string());
                            return Err(ilium_animation_js::surface::SurfaceError::Capacity);
                        }
                    }
                    Ok(())
                },
            )
            .map_err(|error| pack_error.unwrap_or_else(|| error.to_string()))?;
        instance
            .accept_frame(outcome.accepted)
            .map_err(|error| error.to_string())?;
        self.dispatch_requests(instance)?;
        if self.video.pre_render_video_attempted() && !self.video.recording_ready() {
            return Err("Pre-rendered Video has no recorded native FrozenEvidence".into());
        }
        if self.video.recording_ready() {
            instance.check_recorded_video_replay_requests(self.video.recorded_count())
        } else {
            instance.check_procedural_replay_requests()
        }
        .map_err(|error| error.to_string())?;
        if !outcome.accepted {
            return Err("Replay sample was not presented as a complete viewport".into());
        }
        stopped(stop)?;
        preparation
            .push_captured(
                sample.take().ok_or("Replay sample ticket missing")?,
                captured.take().ok_or("Replay packed sample missing")?,
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}
fn playback_tick(
    origin: Instant,
    previous: Duration,
    requested_at: Instant,
) -> Result<Duration, String> {
    let tick = requested_at
        .checked_duration_since(origin)
        .ok_or("Playback request moved backwards in the monotonic domain")?;
    if tick < previous {
        return Err("Playback request moved backwards after admission".into());
    }
    Ok(tick)
}
fn stopped(stop: &StopToken) -> Result<(), String> {
    if stop.is_stopped() {
        Err("Animation plugin worker stopped".into())
    } else {
        Ok(())
    }
}
fn helper_executable() -> Result<PathBuf, String> {
    let current = std::env::current_exe().map_err(|error| error.to_string())?;
    let path = ilium_platform::animation_sandbox::helper_executable_path(&current)
        .map_err(|error| error.to_string())?;
    if !path.is_file() {
        return Err("Protected animation helper is not installed beside Ilium".into());
    }
    Ok(path)
}
fn shape(
    plan: &ilium_animation_js::plan::AnimationPlan,
    width: u16,
    height: u16,
) -> Result<Shape, String> {
    let output = plan
        .output
        .as_ref()
        .ok_or_else(|| "Plugin plan requires an explicit output shape".to_owned())?;
    serde_json::from_value(json!({"cell_width":width,"cell_height":height,"mode":output.mode,"format":output.format,"update":output.update,"cell_rgb":output.cell_rgb,"colour_space":output.colour_space.as_deref().unwrap_or("srgb")})).map_err(|error|error.to_string())
}
fn array(name: &str, kind: TypedArrayKind, elements: usize) -> ArraySpec {
    ArraySpec {
        name: name.into(),
        kind,
        elements,
    }
}
fn encode_data(data: Data) -> Vec<u8> {
    match data {
        Data::U8(bytes) => bytes,
        Data::F32(values) => values.into_iter().flat_map(f32::to_ne_bytes).collect(),
    }
}
fn take(planes: &mut BTreeMap<String, Vec<u8>>, name: &str) -> Result<Vec<u8>, String> {
    planes
        .remove(name)
        .ok_or_else(|| format!("Missing plugin plane {name}"))
}
fn words(bytes: Vec<u8>) -> Result<Vec<u32>, String> {
    if bytes.len() % 4 != 0 {
        return Err("Malformed plugin u32 plane".into());
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|word| u32::from_ne_bytes([word[0], word[1], word[2], word[3]]))
        .collect())
}
fn decode_planes(
    planes: &mut BTreeMap<String, Vec<u8>>,
    format: Format,
    rgb: bool,
) -> Result<Planes, String> {
    let data = take(planes, "data")?;
    let data = if format == Format::Gray32 {
        if data.len() % 4 != 0 {
            return Err("Malformed plugin float plane".into());
        }
        Data::F32(
            data.chunks_exact(4)
                .map(|word| f32::from_ne_bytes([word[0], word[1], word[2], word[3]]))
                .collect(),
        )
    } else {
        Data::U8(data)
    };
    Ok(Planes {
        data,
        touch: take(planes, "touch")?,
        order: words(take(planes, "order")?)?,
        cell_rgb: if rgb {
            Some(take(planes, "cell_rgb")?)
        } else {
            None
        },
        colour_touch: if rgb {
            Some(take(planes, "colour_touch")?)
        } else {
            None
        },
        colour_order: if rgb {
            Some(words(take(planes, "colour_order")?)?)
        } else {
            None
        },
    })
}
fn pack_cells(
    snapshot: &Snapshot,
    settings: &AnimationSettings,
    elapsed: Duration,
) -> Result<Vec<SnapshotCell>, String> {
    if snapshot.owners().iter().any(Option::is_some) {
        return Err(
            "Prepared native source publication requires its authenticated owner adapter".into(),
        );
    }
    let shape = snapshot.shape();
    let width = shape.cell_width as usize * 2;
    let height = shape.cell_height as usize * 4;
    let density = f32::from(settings.density_percent) / 100.;
    let tone = |value: f32, _: usize, _: usize| settings.appearance.shape_dot(value) * density;
    let packed = if settings.dither.is_error_diffusion() && shape.mode == Mode::Pixels {
        let mut coverage = vec![0.; width * height];
        let scratch = snapshot
            .pack(tone, |value, x, y| {
                coverage[y * width + x] = value;
                false
            })
            .map_err(|error| error.to_string())?;
        drop(scratch);
        let mut diffused = Vec::new();
        ilium_ambient::dither::diffuse(
            &coverage,
            width,
            height,
            1.,
            settings.dither,
            &mut diffused,
        );
        snapshot
            .pack(tone, |_, x, y| diffused[y * width + x])
            .map_err(|error| error.to_string())?
    } else {
        snapshot
            .pack(tone, |value, x, y| {
                value >= ilium_ambient::raster::threshold(x, y, settings.dither)
            })
            .map_err(|error| error.to_string())?
    };
    let mut cells = cells_from_packed(shape, &packed, settings, elapsed)?;
    overlay_native_text(&mut cells, shape, snapshot.text())?;
    Ok(cells)
}
fn snapshot_cells_bytes(cells: &[SnapshotCell], capacity: usize) -> Option<usize> {
    capacity
        .checked_mul(std::mem::size_of::<SnapshotCell>())?
        .checked_add(
            cells
                .iter()
                .map(|cell| cell.article_symbol.as_ref().map_or(0, String::capacity))
                .try_fold(0usize, |sum, bytes| sum.checked_add(bytes))?,
        )
}
fn overlay_native_text<'a>(
    cells: &mut [SnapshotCell],
    shape: Shape,
    text: impl IntoIterator<Item = &'a ilium_animation_js::surface::NativeText>,
) -> Result<(), String> {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;
    let width = shape.cell_width as usize;
    let mut total_bytes = 0usize;
    for span in text {
        total_bytes = total_bytes
            .checked_add(span.text.len())
            .ok_or("Native text byte count overflow")?;
        let glyph_width = UnicodeWidthStr::width(span.text.as_str());
        if total_bytes > ilium_animation_js::surface::MAX_TEXT_BYTES
            || span.text.len() > 256
            || span.text.graphemes(true).count() != 1
            || span.text.chars().any(char::is_control)
            || glyph_width != usize::from(span.width)
            || !matches!(span.width, 1 | 2)
            || span.x >= shape.cell_width
            || span.y >= shape.cell_height
            || u32::from(span.width) > shape.cell_width - span.x
        {
            return Err("Native text cell validation failed".into());
        }
        let index = span.y as usize * width + span.x as usize;
        for offset in 0..glyph_width {
            let cell = cells
                .get_mut(index + offset)
                .ok_or("Native text escaped packed frame")?;
            if cell.article_symbol.is_some() || cell.article_is_continuation {
                return Err("Overlapping native text cells".into());
            }
            cell.color = span.style.rgb.map(|[red, green, blue]| (red, green, blue));
            cell.article_background = span
                .style
                .background
                .map(|[red, green, blue]| (red, green, blue));
            cell.article_style = (span.style.bold, span.style.italic);
            cell.article_underline = span.style.underline;
            if offset == 0 {
                cell.article_symbol = Some(span.text.clone());
            } else {
                cell.article_is_continuation = true;
            }
        }
    }
    Ok(())
}
fn cells_from_packed(
    shape: Shape,
    packed: &ilium_animation_js::surface::PackedSurface,
    settings: &AnimationSettings,
    elapsed: Duration,
) -> Result<Vec<SnapshotCell>, String> {
    if packed.owners.iter().any(Option::is_some) {
        return Err("Protected replay frame requires its original playback lease".into());
    }
    cells_from_validated_packed(shape, packed, settings, elapsed)
}
/// Decode only through the original lease, which validates playback authority,
/// grant lineage, stop state and generation. The caller retains this same lease
/// on PluginFrame for surviving-dot preparation and settlement after flush.
fn cells_from_replay(
    shape: Shape,
    lease: &ilium_animation_js::replay::PlaybackLease,
    settings: &AnimationSettings,
    elapsed: Duration,
) -> Result<Vec<SnapshotCell>, String> {
    let packed = lease.packed().map_err(|error| error.to_string())?;
    cells_from_validated_packed(shape, packed, settings, elapsed)
}
fn cells_from_validated_packed(
    shape: Shape,
    packed: &ilium_animation_js::surface::PackedSurface,
    settings: &AnimationSettings,
    elapsed: Duration,
) -> Result<Vec<SnapshotCell>, String> {
    let layout = shape.layout().map_err(|error| error.to_string())?;
    if packed.masks.len() != layout.cells
        || packed.rgb.len() != layout.cells
        || packed.owners.len() != layout.dots
    {
        return Err("Incomplete packed frame".into());
    }
    let foreground = settings.foreground_rgb();
    Ok(packed
        .masks
        .iter()
        .copied()
        .zip(packed.rgb.iter().copied())
        .enumerate()
        .map(|(index, (mask, rgb))| {
            let x = index % shape.cell_width as usize;
            let y = index / shape.cell_width as usize;
            let fraction = |coordinate: usize, extent: u32| {
                if extent <= 1 {
                    0.5
                } else {
                    coordinate as f32 / (extent - 1) as f32
                }
            };
            let color = settings.appearance.shade(
                [foreground.0, foreground.1, foreground.2],
                rgb,
                &ilium_ambient::style::CellContext {
                    coverage: f32::from(mask.count_ones() as u8) / 8.,
                    x: fraction(x, shape.cell_width),
                    y: fraction(y, shape.cell_height),
                    seconds: elapsed.as_secs_f32(),
                },
            );
            SnapshotCell {
                glyph: char::from_u32(0x2800 + u32::from(mask)).unwrap_or(' '),
                native_glyph: None,
                packed_bits: mask,
                color: Some((color[0], color[1], color[2])),
                article_symbol: None,
                article_is_continuation: false,
                article_style: (false, false),
                article_background: None,
                article_underline: false,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_animation_js::surface::{ColourSpace, Update};

    #[test]
    fn playback_host_tick_rejects_backward_requests() {
        let origin = Instant::now();
        let first = origin + Duration::from_secs(2);
        assert_eq!(
            playback_tick(origin, Duration::ZERO, first).unwrap(),
            Duration::from_secs(2)
        );
        assert!(playback_tick(
            origin,
            Duration::from_secs(2),
            origin + Duration::from_secs(1)
        )
        .is_err());
        assert!(playback_tick(origin, Duration::ZERO, origin - Duration::from_millis(1)).is_err());
    }

    fn accepted(format: Format, data: Data, touch: Vec<u8>) -> Surface {
        let shape = Shape {
            cell_width: 1,
            cell_height: 1,
            mode: if format == Format::Mask8 {
                Mode::Cells
            } else {
                Mode::Pixels
            },
            format,
            update: Update::Retain,
            cell_rgb: false,
            colour_space: ColourSpace::Srgb,
        };
        let mut surface = Surface::new(7, 11, shape).unwrap();
        let seed = surface.begin(1).unwrap();
        surface
            .finish(
                FrameMeta {
                    wire_version: 1,
                    key: seed.key,
                    shape,
                    presented: true,
                    error: None,
                    commands: Vec::new(),
                },
                Planes {
                    data,
                    order: touch.iter().map(|value| u32::from(*value)).collect(),
                    touch,
                    cell_rgb: None,
                    colour_touch: None,
                    colour_order: None,
                },
                &mut NoNativeRenderer,
            )
            .unwrap();
        surface
    }

    #[test]
    fn direct_masks_keep_braille_bit_positions_under_diffusion() {
        let surface = accepted(Format::Mask8, Data::U8(vec![0b1000_0001]), vec![1]);
        let settings = AnimationSettings {
            dither: ilium_ambient::raster::DitherMode::FloydSteinberg,
            ..Default::default()
        };
        let packed = pack_cells(surface.snapshot(), &settings, Duration::ZERO).unwrap();
        assert_eq!(packed.len(), 1);
        assert_eq!(packed[0].packed_bits, 0b1000_0001);
        assert_eq!(packed[0].glyph, '\u{2881}');
        assert!(packed[0].article_symbol.is_none());
    }

    #[test]
    fn inversion_distinguishes_explicit_zero_from_untouched_empty_pixels() {
        let empty = accepted(Format::Gray8, Data::U8(vec![0; 8]), vec![0; 8]);
        let drawn = accepted(Format::Gray8, Data::U8(vec![0; 8]), vec![1; 8]);
        let mut settings = AnimationSettings {
            density_percent: 100,
            ..Default::default()
        };
        settings.appearance.pattern_invert = true;
        for mode in [
            ilium_ambient::raster::DitherMode::Ordered,
            ilium_ambient::raster::DitherMode::FloydSteinberg,
        ] {
            settings.dither = mode;
            assert_eq!(
                pack_cells(empty.snapshot(), &settings, Duration::ZERO).unwrap()[0].packed_bits,
                0
            );
            assert_eq!(
                pack_cells(drawn.snapshot(), &settings, Duration::ZERO).unwrap()[0].packed_bits,
                255
            );
        }
    }

    #[test]
    fn gray32_handoff_decodes_native_words_and_rejects_partial_word() {
        let mut planes = BTreeMap::from([
            ("data".into(), 0.375_f32.to_ne_bytes().to_vec()),
            ("touch".into(), vec![1]),
            ("order".into(), 17_u32.to_ne_bytes().to_vec()),
        ]);
        let decoded = decode_planes(&mut planes, Format::Gray32, false).unwrap();
        let Data::F32(values) = decoded.data else {
            panic!("float plane required")
        };
        assert_eq!(values, vec![0.375]);
        assert_eq!(decoded.order, vec![17]);
        assert!(words(vec![0; 3]).is_err());
        planes.insert("data".into(), vec![0; 3]);
        assert!(decode_planes(&mut planes, Format::Gray32, false).is_err());
    }
    #[test]
    fn native_policy_offers_installed_http_and_inputs_without_gpu_or_local_access() {
        let wire = |id: &str, scope| ilium_animation_js::manifest::Capability {
            id: id.into(),
            scope,
        };
        let policy = native_policy(
            &[
                wire("input.pointer", json!("animation_viewport")),
                wire("screen.occlusion", json!("animation_viewport")),
                wire(
                    "network.http",
                    json!({"origins":["https://example.com"],"methods":["GET"]}),
                ),
                wire("device.gpu", json!({"kernels":["fft"]})),
                wire(
                    "network.local",
                    json!({"origins":["https://127.0.0.1:8080"],"methods":["GET"]}),
                ),
            ],
            false,
        )
        .unwrap();
        assert_eq!(policy.permissions.len(), 3);
        let network = policy
            .permissions
            .iter()
            .find(|right| right.id == Capability::NetworkHttp)
            .unwrap();
        assert_eq!(
            network.scope,
            ilium_animation_js::permissions::Scope::Network {
                origins: std::collections::BTreeSet::from(["https://example.com".into()]),
                methods: std::collections::BTreeSet::from([
                    ilium_animation_js::permissions::HttpMethod::Get
                ]),
            }
        );
        assert!(!policy
            .permissions
            .iter()
            .any(|right| matches!(right.id, Capability::NetworkLocal | Capability::DeviceGpu)));
        assert!(policy
            .permissions
            .iter()
            .any(|right| right.id == Capability::InputPointer));
        assert!(policy
            .permissions
            .iter()
            .any(|right| right.id == Capability::ScreenOcclusion));
    }
    #[test]
    fn native_audio_right_is_offered_only_with_platform_selection_support() {
        let microphone = ilium_animation_js::manifest::Capability {
            id: "audio.microphone".into(),
            scope: json!({"source":"microphone"}),
        };
        assert!(native_policy(std::slice::from_ref(&microphone), false)
            .unwrap()
            .permissions
            .is_empty());
        let offered = native_policy(std::slice::from_ref(&microphone), true).unwrap();
        assert_eq!(offered.permissions.len(), 1);
        assert_eq!(offered.permissions[0].id, Capability::AudioMicrophone);
    }
    #[test]
    fn selector_review_cannot_turn_an_allow_choice_into_a_native_binding() {
        use ilium_animation_js::permissions::{
            PackageIdentity, PermissionBroker, PermissionError, PermissionPlan, PermissionRequest,
            UserChoice, Verdict,
        };
        let wire = ilium_animation_js::manifest::Capability {
            id: "disk.read".into(),
            scope: json!({"selection":"file","access":"read","slot":"terrain"}),
        };
        let policy = native_policy(std::slice::from_ref(&wire), false).unwrap();
        let right = policy.permissions[0].clone();
        let mut broker = PermissionBroker::new(
            PackageIdentity::unverified("fixture".into(), b"native selector fixture").unwrap(),
            policy.clone(),
            policy,
        )
        .unwrap();
        let review = broker
            .prepare(
                1,
                1,
                PermissionPlan {
                    permissions: vec![PermissionRequest {
                        request_id: Some("terrain".into()),
                        id: right.id,
                        scope: right.scope,
                        required: true,
                        reason: "Select terrain input".into(),
                    }],
                    demands: vec![],
                },
                BTreeMap::new(),
            )
            .unwrap();
        assert_eq!(review.items()[0].verdict, Verdict::NeedsSelection);
        let request_id = review.items()[0].request.request_id.clone();
        assert!(matches!(
            broker.resolve(
                review,
                BTreeMap::from([(request_id, UserChoice::AllowSession)])
            ),
            Err(PermissionError::SelectionRequired(_))
        ));
    }
    #[test]
    fn installed_state_persist_policy_reaches_accepted_original_review() {
        use ilium_animation_js::permissions::{
            Demand, PackageIdentity, PermissionBroker, PermissionPlan, PermissionRequest,
            UserChoice, Verdict,
        };
        let wire = ilium_animation_js::manifest::Capability {
            id: "state.persist".into(),
            scope: json!("session"),
        };
        let policy = native_policy(std::slice::from_ref(&wire), false).unwrap();
        assert_eq!(policy.permissions.len(), 1);
        let right = policy.permissions[0].clone();
        assert_eq!(right.id, Capability::StatePersist);
        let mut broker = PermissionBroker::new(
            PackageIdentity::unverified("fixture".into(), b"state policy fixture").unwrap(),
            policy.clone(),
            policy,
        )
        .unwrap();
        let review = broker
            .prepare(
                1,
                1,
                PermissionPlan {
                    permissions: vec![PermissionRequest {
                        request_id: Some("state".into()),
                        id: right.id,
                        scope: right.scope,
                        required: true,
                        reason: "Retain the native cache".into(),
                    }],
                    demands: vec![Demand {
                        demand_id: "state".into(),
                        request_ids: std::collections::BTreeSet::from(["state".into()]),
                    }],
                },
                BTreeMap::new(),
            )
            .unwrap();
        assert_eq!(review.items()[0].verdict, Verdict::Prompt);
        let resolution = broker
            .resolve(
                review,
                BTreeMap::from([("state".into(), UserChoice::AllowSession)]),
            )
            .unwrap();
        let activation = resolution.activation.unwrap();
        let grant = broker.grant(&activation.channel, "state").unwrap().unwrap();
        assert_eq!(grant.right.id, Capability::StatePersist);
        assert!(grant.binding.is_none());
    }
    #[test]
    fn original_native_operation_invalidation_is_not_silently_acknowledged() {
        let invalidation = Invalidation {
            authorization_epoch: 2,
            instance_ids: vec![1],
            all_rights_blocked: true,
            operations: vec![ilium_animation_js::permissions::InvalidatedOperation {
                operation_id: 9,
                may_have_effects: true,
            }],
        };
        assert!(apply_before_creation(&invalidation).is_err());
        assert_eq!(invalidation.operations[0].operation_id, 9);
        assert!(invalidation.operations[0].may_have_effects);
    }
    #[test]
    fn installed_http_policy_does_not_turn_native_denial_into_an_operation() {
        use ilium_animation_js::permissions::{
            CallPhase, Demand, OperationNeed, PackageIdentity, PermissionBroker, PermissionPlan,
            PermissionRequest, UserChoice, Verdict,
        };
        let wire = ilium_animation_js::manifest::Capability {
            id: "network.http".into(),
            scope: json!({"origins":["https://example.org"],"methods":["GET"]}),
        };
        let policy = native_policy(std::slice::from_ref(&wire), false).unwrap();
        let right = policy.permissions[0].clone();
        let mut broker = PermissionBroker::new(
            PackageIdentity::unverified("fixture".into(), b"HTTP native denial fixture").unwrap(),
            policy.clone(),
            policy,
        )
        .unwrap();
        let review = broker
            .prepare(
                1,
                1,
                PermissionPlan {
                    permissions: vec![PermissionRequest {
                        request_id: Some("http".into()),
                        id: right.id,
                        scope: right.scope.clone(),
                        required: false,
                        reason: "Native HTTP denial contract".into(),
                    }],
                    demands: vec![Demand {
                        demand_id: "http".into(),
                        request_ids: std::collections::BTreeSet::from(["http".into()]),
                    }],
                },
                BTreeMap::new(),
            )
            .unwrap();
        assert_eq!(review.items()[0].verdict, Verdict::Prompt);
        let resolution = broker
            .resolve(
                review,
                BTreeMap::from([("http".into(), UserChoice::DenySession)]),
            )
            .unwrap();
        let active = resolution.activation.unwrap(); // optional denial can still create
        assert!(broker.grant(&active.channel, "http").unwrap().is_none());
        assert!(broker
            .dispatch(
                &active.channel,
                CallPhase::Async,
                "http",
                vec![OperationNeed::new(right, None).unwrap()]
            )
            .is_err());
        assert_eq!(broker.pending_operations(), 0); // no effect/IO ticket issued
    }
    #[test]
    fn native_http_lost_and_cleanup_observations_never_report_settlement() {
        assert!(observe_http_events(vec![HttpEvent {
            request_id: 1,
            observation: HttpObservation::Lost,
        }])
        .is_err());
        assert!(observe_http_events(vec![HttpEvent {
            request_id: 2,
            observation: HttpObservation::CleanupFailed,
        }])
        .is_err());
        assert!(observe_http_events(vec![HttpEvent {
            request_id: 3,
            observation: HttpObservation::Completion(
                ilium_animation_js::engine::CompletionState::Cancelled
            ),
        }])
        .is_ok()); // observation alone never sets Ready/is_drained
    }
    #[test]
    fn script_error_status_becomes_bounded_runtime_message() {
        let status = json!({
            "records": [
                {"kind":"log", "level":"warning", "message":"ignored"},
                {"kind":"log", "level":"error", "message":"draw failed"},
                {"kind":"log", "level":"error", "message":"asset missing"}
            ],
            "dropped": 0
        });
        assert_eq!(
            status_error_message(&status).as_deref(),
            Some("draw failed; asset missing")
        );
        assert!(status_error_message(&json!({"records":[],"dropped":0})).is_none());
    }
}

#[cfg(test)]
#[path = "pre_render_custody_tests.rs"]
mod pre_render_custody_tests;
