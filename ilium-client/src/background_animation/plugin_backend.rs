//! Worker-local procedural plugin adapter. Package evaluation runs only in the
//! protected helper; catalogue descriptors never become frame identities.
use super::{
    worker::{RenderRequest, SnapshotCell},
    AnimationSettings,
};
use crate::{
    animation_plugins::{
        review_bridge::{ReviewBridge, ReviewPhase},
        review_controller::{self, IntentOutcome},
        PluginCatalogue,
    },
    filesystem::{
        plugin_permission_controller::{
            ActivationUpdate, PermissionCancellation, PluginPermissionController,
        },
        plugin_permissions::PluginPermissionFiles,
    },
};
#[cfg(test)]
use ilium_animation_js::surface::NoNativeRenderer;
use ilium_animation_js::{
    clock::AnimationClock,
    engine::HostRequest,
    engine::{ArraySpec, CreateState, TypedArrayKind},
    helper::HelperLimits,
    http::SystemDns,
    manifest::AnimationMode,
    native_audio::RetainedAudioSnapshot,
    native_compute_host::NativeComputeHost,
    native_draw::DrawLimits,
    native_draw_host::NativeDrawHost,
    native_frame_inputs::{
        CachedInputProvider, ClockObservation, LocationObservation, NativeFrameInputs,
        OcclusionObservation, PointerObservation,
    },
    native_http_host::{HttpEvent, HttpObservation, NativeHttpHost},
    native_media::MediaLimits,
    permissions::{Capability, Ceiling, Invalidation},
    runtime::VerifiedPreparation,
    runtime::{InstancePreparation, PackageInstance},
    surface::{Data, Format, FrameMeta, Mode, Planes, Shape, Snapshot, Surface},
};
use ilium_execution::{
    Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, QuotaGroup, Receipt, Retention,
    StorageAdmission,
};
use ilium_platform::owned_worker::StopToken;
use ilium_platform::{animation_files::PinnedDirectory, secure_fs::NoFollowDirectory};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
const ARCHIVE_BYTES: usize = 32 * 1024 * 1024;
static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);

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
    pub cells: Vec<SnapshotCell>,
    pub frames_per_second: u32,
    pub resident_bytes: usize,
}
struct Presentation {
    surface: Surface,
    clock: AnimationClock,
    compute: NativeComputeHost,
    http: NativeHttpHost,
    drawing: NativeDrawHost,
    inputs: NativeFrameInputs,
    creation: CreateState,
    unhandled: Option<HostRequest>,
    undispatched: Vec<HostRequest>,
    _surface_storage: StorageAdmission,
    _request_storage: StorageAdmission,
}
struct PreparedSelection {
    verified: VerifiedPreparation,
    root: Arc<PinnedDirectory>,
    request: RenderRequest,
}
struct SetupJob {
    request: RenderRequest,
    quota: QuotaGroup,
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
        if selection.mode != AnimationMode::Live {
            return Err("Pre-rendered plugin playback is awaiting the native clip owner".into());
        }
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
        let environment = json!({"viewport":{"cell_width":request.width,"cell_height":request.height,"dot_width":u32::from(request.width)*2,"dot_height":u32::from(request.height)*4,"revision":request.revision},"available":{"pointer":true,"audio":false,"gpu":false,"location":true}});
        let verified = PackageInstance::verify(InstancePreparation {
            archive: &bytes,
            verifier: &verifier,
            helper_executable: &helper,
            trusted_bootstrap: ilium_animation_js::TRUSTED_BOOTSTRAP,
            settings: &selection.settings,
            mode: selection.mode,
            environment: &environment,
            host_policy: native_policy(&descriptor.manifest.capabilities)?,
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
        stopped(&stop)?;
        Ok(PreparedSelection {
            verified,
            root,
            request: self.request,
        })
    }
}
// Pointer/occlusion have actual native input owners. Selector-dependent rights
// are REVIEWABLE only: the broker returns NeedsSelection until a real native
// resource picker supplies a binding. This backend supplies NO bindings.
// Normalized NetworkHttp is reviewable because an actual original native HTTP
// owner is always constructed before dispatch. NetworkLocal is deliberately
// excluded until its genuine dependency projection is implemented. State,
// observer and GPU owners must extend policy only when actually composed.
fn native_policy(
    capabilities: &[ilium_animation_js::manifest::Capability],
) -> Result<Ceiling, String> {
    let mut permissions = Vec::new();
    for capability in capabilities {
        let right = ilium_animation_js::permission_projection::right(capability)
            .map_err(|error| error.to_string())?;
        if matches!(
            right.id,
            Capability::InputPointer
                | Capability::ScreenOcclusion
                | Capability::DiskRead
                | Capability::DiskWrite
                | Capability::AudioLoopback
                | Capability::AudioMicrophone
                | Capability::NetworkHttp
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
    presentation: Option<Presentation>,
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
}
impl PluginBackend {
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
        if let Some(presentation) = &mut workflow.presentation {
            presentation.compute.revoke();
            presentation.http.cancel_all();
            presentation.drawing.revoke();
            presentation.surface.abort();
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
            presentation: None,
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
        if let IntentOutcome::Cancelled(cancellation) = outcome {
            workflow.cancellation = Some(cancellation);
            workflow.halted = true;
        }
        self.advance_controller()
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
                    creation,
                )?);
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
                if presentation.creation == CreateState::Ready {
                    ReviewPhase::Ready
                } else {
                    ReviewPhase::Creating
                },
                None,
            )?;
            // Resolution has been applied, but retain it with controller ledger
            // receipts through real retirement instead of discarding the inventory.
        }
        Ok(())
    }
    /// Genuine finite completion wake only: collects native setup/ledger jobs,
    /// then native compute. UI intents and time frames never poll those receipts.
    pub(super) fn on_native_completion(&mut self) -> Result<(), String> {
        self.collect_setup()?;
        self.advance_controller()?;
        let Some(workflow) = self.workflow.as_mut() else {
            return Ok(());
        };
        if workflow.halted {
            return self.settle_retirement(true);
        }
        let Some(presentation) = &mut workflow.presentation else {
            return Ok(());
        };
        let instance = workflow
            .controller
            .package_instance_mut()
            .ok_or_else(|| "Accepted native instance unavailable".to_owned())?;
        // Observe BOTH independent original owners on this genuine finite wake
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
        compute?;
        http?;
        presentation.creation = instance.pump().map_err(|error| error.to_string())?;
        presentation.dispatch_requests(instance)?;
        if presentation.creation == CreateState::Ready {
            self.review
                .set_phase(workflow.revision, ReviewPhase::Ready, None)?;
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
        if on_wake {
            let ledger = workflow.controller.collect_retirement_on_wake();
            let (compute, http) = if let Some(presentation) = &mut workflow.presentation {
                presentation.http.cancel_all();
                let compute = presentation
                    .compute
                    .collect_retirement_on_wake()
                    .map_err(|error| error.to_string());
                let http = workflow
                    .controller
                    .collect_http_retirement_on_wake(&mut presentation.http)
                    .and_then(observe_http_events);
                (compute, http)
            } else {
                (Ok(()), Ok(()))
            };
            // Independent original inventories all receive the SAME real wake.
            // Every failure stays retained; no helper ACK or replacement grant.
            ledger?;
            compute?;
            http?;
        }
        if let Some(presentation) = &mut workflow.presentation {
            presentation.compute.revoke();
            presentation.http.cancel_all();
            presentation.drawing.revoke();
            presentation.surface.abort();
        }
        // No source/HTTP/audio owner is composed here. Any future native owner
        // MUST join this original retirement inventory and physical predicate.
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
        if !workflow.controller.is_physically_settled()
            || workflow
                .presentation
                .as_ref()
                .is_some_and(|p| !p.compute.is_drained() || !p.http.is_drained())
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
        creation: CreateState,
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
        let compute =
            NativeComputeHost::new(resources, quota.clone(), instance.engine_limits().clone())
                .map_err(|error| error.to_string())?;

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
        let inputs =
            NativeFrameInputs::new(instance, quota.clone()).map_err(|error| error.to_string())?;
        Ok(Self {
            surface,
            clock,
            compute,
            http,
            drawing,
            inputs,
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
        Ok(None)
    }
}

impl Presentation {
    fn dispatch_requests(&mut self, instance: &mut PackageInstance) -> Result<(), String> {
        if self.unhandled.is_some() {
            return Err("An original native service request is awaiting its actual owner".into());
        }
        let requests = instance.requests().map_err(|error| error.to_string())?;
        let had_requests = !requests.is_empty();
        let mut requests = requests.into_iter();
        while let Some(request) = requests.next() {
            // Each actual adapter consumes only its own method. Unrelated
            // original requests move intact to the next owner, never copied
            // into JSON/fabricated demand IDs or replaced with fake responses.
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
            let message = format!(
                "Native service adapter is not yet wired: {}",
                request.method
            );
            self.unhandled = Some(request);
            self.undispatched = requests.collect();
            return Err(message);
        }
        if had_requests {
            self.creation = instance.pump().map_err(|error| error.to_string())?;
        }
        Ok(())
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
        let input_packet = self
            .inputs
            .prepare(
                instance,
                seed.shape,
                request.revision,
                request.requested_at,
                &mut WorkerInputProvider { request },
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
        let metadata = json!({"frame":{"key":seed.key,"shape":seed.shape,"reset":seed.reset,"invalid_rects":seed.invalid_rects,"input_specs":input_packet.value.metadata()["input_specs"]},"services":native_status.metadata(),"random_seed":instance.settings().get("seed").and_then(serde_json::Value::as_u64).unwrap_or(0).to_string()});
        instance
            .seed_frame(&metadata, &seed_arrays, &seed_planes)
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
        let bytes = cells
            .capacity()
            .checked_mul(std::mem::size_of::<SnapshotCell>())
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
            cells,
            frames_per_second: instance.plan().fps.ceil().clamp(1., 120.) as u32,
            resident_bytes: bytes,
        }))
    }
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
    if snapshot.owners().iter().any(Option::is_some) || snapshot.text().next().is_some() {
        return Err(
            "Prepared native source/text publication requires its authenticated owner adapter"
                .into(),
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
    let foreground = settings.foreground_rgb();
    Ok(packed
        .masks
        .into_iter()
        .zip(packed.rgb)
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
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_animation_js::surface::{ColourSpace, Update};

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
        let policy = native_policy(&[
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
        ])
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
    fn selector_review_cannot_turn_an_allow_choice_into_a_native_binding() {
        use ilium_animation_js::permissions::{
            PackageIdentity, PermissionBroker, PermissionError, PermissionPlan, PermissionRequest,
            UserChoice, Verdict,
        };
        let wire = ilium_animation_js::manifest::Capability {
            id: "disk.read".into(),
            scope: json!({"selection":"file","access":"read","slot":"terrain"}),
        };
        let policy = native_policy(std::slice::from_ref(&wire)).unwrap();
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
        let policy = native_policy(std::slice::from_ref(&wire)).unwrap();
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
}
