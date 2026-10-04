//! Read-only saved-world scene. Blocking catalog, pack, and model work lives on
//! one owned worker; only immutable bank-matched surfaces reach presentation.
//! Native liquid, waterlogged, and built-in block meshes remain explicit
//! prerequisites; retained receipts cannot credit a different raster's ids.
use super::{
    evidence::{Confidence, MapId},
    history_store::{BoundMap, Repository},
    history_writer::Writer,
    native_assets,
    paint_owners::FrameOwners,
    pipeline, projected_route,
    saved_runtime::{Gate, SavedRuntime},
    session_catalog,
    settings::SavedMapsSettings,
    tours::{
        self, Choice, Clock, Controller, Envelope, History, IssuedView, Motion, Plan, Policy,
        PreparedMap, RouteKey, Ticket,
    },
};
use crate::{
    control::SceneSettings,
    raster::PaintedOwner,
    resources::{AmbientResources, WorkerCost},
    scene::{Frame, FrameReceiptId, Scene, SceneEnv, MAX_SCENE_RECEIPT_SLOTS},
    source::Worker,
    style::ScenePalette,
    voxel_landscape::{
        assets::budget::{ByteBudget, Cancel, Reservation},
        composite_selected,
        surface_raster::{self, DirectionalLight, RasterFrame, RasterLimits},
        Retirement, VoxelLandscapeScene, VoxelLandscapeSettings,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex, TryLockError,
    },
    time::{Duration, Instant},
};

const SCENE_ACCOUNT: u64 = 1024 * 1024 * 1024;
#[path = "saved_display.rs"]
mod display;
// Additional native stack/decompression scratch outside the retained account.
// One synchronous preparation/planning thread; no library helper threads.
const PREPARATION_WORKER_BYTES: usize = 64 * 1024 * 1024;

struct PreparationResources {
    budget: ByteBudget,
    host: AmbientResources,
}
// Four default loaded windows cost at most 4 * 32 MiB by loader admission.
// The remaining conservative logical charge covers retained target/catalog
// trees and transient catalog preparation, within the one scene account.
const CATALOG_CHARGE: u64 = 512 * 1024 * 1024;
const TOUR_WORK: u64 = 16_000_000;
// The worker's4096 line queries reserve at most4096*(257*25+64)
// =26,578,944 probes for the1024-block/radius16 policy. Map/target/top-cell
// scans add under1million at the admitted16-map/128-chunk/2304-target caps.
// This finite worker budget covers that declared search; UI/history operations
// retain their separate16million cap.
const PLANNER_WORK: u64 = 32_000_000;

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);
static RASTER_STOP: AtomicBool = AtomicBool::new(false);

struct Bundle {
    history: History,
    maps: Vec<Arc<PreparedMap>>,
    bindings: Vec<BoundMap>,
    world_seeds: BTreeMap<MapId, Option<i64>>,
    root: PathBuf,
    jar: PathBuf,
    warnings: Vec<String>,
    budget: ByteBudget,
    _catalog_reservation: Arc<Reservation>,
}
#[derive(Clone, Copy)]
struct PlanRequest {
    sequence: u64,
    ticket: Ticket,
    history: History,
    policy: Policy,
    size: [usize; 2],
    scale: f64,
}
struct PlanResponse {
    sequence: u64,
    size: [usize; 2],
    outcome: Result<Option<(Plan, Arc<projected_route::PreparedRoute>)>, String>,
}
struct FrameReceipt {
    issued: IssuedView,
    owners: FrameOwners,
    accounted_bytes: usize,
    _reservation: Reservation,
}

type PreparedBundleOutput = Arc<Mutex<Option<Result<Arc<Bundle>, String>>>>;

pub struct SavedScene {
    settings: VoxelLandscapeSettings,
    palette: ScenePalette,
    runtime: Arc<SavedRuntime>,
    output: PreparedBundleOutput,
    plan_requests: Arc<Mutex<Option<PlanRequest>>>,
    plan_results: Arc<Mutex<Option<PlanResponse>>>,
    desired_plan: Arc<AtomicU64>,
    terminal_history: SyncSender<History>,
    retired: Arc<Retirement<Arc<Bundle>>>,
    retired_plans: Arc<Retirement<Plan>>,
    retired_routes: Arc<Retirement<Arc<projected_route::PreparedRoute>>>,
    worker: Option<Worker>,
    bundle: Option<Arc<Bundle>>,
    controller: Option<Controller>,
    plan: Option<Plan>,
    route: Option<Arc<projected_route::PreparedRoute>>,
    plan_pending: Option<(u64, [usize; 2])>,
    active_size: Option<[usize; 2]>,
    unavailable_size: Option<[usize; 2]>,
    plan_sequence: u64,
    pending_retired_plans: Vec<Plan>,
    pending_retired_routes: Vec<Arc<projected_route::PreparedRoute>>,
    /// Current raster's heavy provenance is worker-owned. The three sealed
    /// slots are reused only after their matching snapshot Arc is gone.
    receipt: Option<Arc<FrameReceipt>>,
    display: Option<display::Display>,
    receipt_slots: [Option<(FrameReceiptId, Arc<FrameReceipt>)>; MAX_SCENE_RECEIPT_SLOTS as usize],
    pending_history: Option<History>,
    last_clock: Option<Clock>,
    status: Option<String>,
}

impl SavedScene {
    // PALETTE (future plugin contract): `env.palette` is the shared look's current
    // palette. When animations become plugins, the plugin constructor receives the
    // current palette and MUST follow it: scenes with natural colours shift them
    // onto it (`ScenePalette::recolor`/`at`), and `Scene::set_palette` delivers later
    // changes. This scene follows it natively: `composite_selected` moves every
    // pixel onto the palette colour of equal brightness before cell averaging,
    // so `PaletteScene` skips its generic remap.
    pub fn new(
        saved: &SavedMapsSettings,
        settings: &VoxelLandscapeSettings,
        env: &SceneEnv,
    ) -> Self {
        let output = Arc::new(Mutex::new(None));
        let plan_requests = Arc::new(Mutex::new(None));
        let plan_results = Arc::new(Mutex::new(None));
        let desired_plan = Arc::new(AtomicU64::new(0));
        // Exactly one final full History may be transferred on scene drop.
        let (terminal_history, final_receiver) = mpsc::sync_channel(1);
        let retired = Arc::new(Retirement::new());
        let retired_plans = Arc::new(Retirement::new());
        let retired_routes = Arc::new(Retirement::new());
        let runtime = Arc::clone(&env.saved_runtime);
        let settings = settings.normalized();
        let mut scene = Self {
            settings: settings.clone(),
            palette: env.palette.clone(),
            runtime: Arc::clone(&runtime),
            output: Arc::clone(&output),
            plan_requests: Arc::clone(&plan_requests),
            plan_results: Arc::clone(&plan_results),
            desired_plan: Arc::clone(&desired_plan),
            terminal_history,
            retired: Arc::clone(&retired),
            retired_plans: Arc::clone(&retired_plans),
            retired_routes: Arc::clone(&retired_routes),
            worker: None,
            bundle: None,
            controller: None,
            plan: None,
            route: None,
            plan_pending: None,
            active_size: None,
            unavailable_size: None,
            plan_sequence: 0,
            pending_retired_plans: Vec::with_capacity(3),
            pending_retired_routes: Vec::with_capacity(3),
            receipt: None,
            display: None,
            receipt_slots: std::array::from_fn(|_| None),
            pending_history: None,
            last_clock: None,
            status: Some("Preparing saved Java worlds…".into()),
        };
        let root = match saved.saves_root() {
            Ok(root) => root,
            Err(error) => {
                scene.status = Some(error);
                return scene;
            }
        };
        let storage = env.cache_dir.join("minecraft-saved-history");
        if !storage.is_absolute() {
            scene.status = Some("Saved history storage root must be absolute".into());
            return scene;
        }
        let jar = match native_assets::jar_path("") {
            Ok(jar) => jar,
            Err(error) => {
                scene.status = Some(error.to_string());
                return scene;
            }
        };
        let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        if generation == 0 {
            scene.status = Some("Saved scene generation exhausted".into());
            return scene;
        }
        // One physical lease covers this shared account, including map/bank
        // reservations retained by emitted frames after the worker retires.
        let physical_storage = match env.resources.reserve_storage(SCENE_ACCOUNT as usize) {
            Ok(storage) => storage,
            Err(error) => {
                scene.status = Some(format!("Saved scene storage admission rejected: {error:?}"));
                return scene;
            }
        };
        let budget = match ByteBudget::with_storage(SCENE_ACCOUNT, physical_storage) {
            Ok(budget) => budget,
            Err(error) => {
                scene.status = Some(error.to_string());
                return scene;
            }
        };
        let admission = match env.resources.reserve_worker(WorkerCost {
            threads: 1,
            resident_bytes: PREPARATION_WORKER_BYTES,
        }) {
            Ok(admission) => admission,
            Err(error) => {
                scene.status = Some(format!("Saved preparation admission rejected: {error:?}"));
                return scene;
            }
        };
        let preparation = PreparationResources {
            budget,
            host: env.resources.clone(),
        };
        let worker = Worker::start_admitted("saved-native-scene", admission, move |stop| {
            ilium_platform::thread_priority::lower_current_thread(
                ilium_platform::thread_priority::WorkerPriority::BelowNormal,
            );
            let result =
                prepare_bundle(root, storage, jar, generation, preparation, &runtime, &stop)
                    .map(Arc::new);
            let planner_bundle = result.as_ref().ok().cloned();
            let published = match output.lock() {
                Ok(mut slot) => {
                    *slot = Some(result);
                    true
                }
                Err(_) => false,
            };
            if !published {
                tracing::error!("saved scene result handoff poisoned");
            }
            while !stop.load(Ordering::Relaxed) {
                drop(retired.drain());
                drop(retired_plans.drain());
                drop(retired_routes.drain());
                if let Some(bundle) = planner_bundle.as_ref() {
                    let request = match plan_requests.lock() {
                        Ok(mut slot) => slot.take(),
                        Err(_) => None,
                    };
                    if let Some(request) = request {
                        let outcome =
                            prepare_selection(bundle, &request, &settings, &stop, &desired_plan);
                        if !stop.load(Ordering::Relaxed)
                            && desired_plan.load(Ordering::Acquire) == request.sequence
                        {
                            if let Ok(mut slot) = plan_results.lock() {
                                *slot = Some(PlanResponse {
                                    sequence: request.sequence,
                                    size: request.size,
                                    outcome,
                                });
                            }
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            drop(retired.drain());
            drop(retired_plans.drain());
            drop(retired_routes.drain());
            if let Ok(mut slot) = output.lock() {
                drop(slot.take());
            }
            let pending = final_receiver.try_recv().ok();
            finish_worker_handoff(&runtime, pending);
        });
        match worker {
            Ok(worker) => scene.worker = Some(worker),
            Err(error) => {
                scene.status = Some(format!("Could not start saved preparation: {error}"))
            }
        }
        scene
    }

    fn receive(&mut self) {
        if self.bundle.is_some() {
            return;
        }
        let result = match self.output.try_lock() {
            Ok(mut slot) => slot.take(),
            Err(TryLockError::WouldBlock) => return,
            Err(TryLockError::Poisoned(_)) => {
                self.status = Some("Saved preparation handoff poisoned".into());
                return;
            }
        };
        match result {
            Some(Ok(bundle)) => {
                let generation = bundle.maps.first().map(|map| map.source().generation);
                let Some(generation) = generation else {
                    self.status = Some(format!(
                        "No native-bindable saved windows: {}",
                        bundle.warnings.join("; ")
                    ));
                    self.bundle = Some(bundle);
                    return;
                };
                match Controller::new(generation, bundle.history) {
                    Ok(controller) => {
                        self.controller = Some(controller);
                        self.status = bundle.warnings.first().cloned();
                        self.bundle = Some(bundle);
                    }
                    Err(error) => self.status = Some(format!("Saved tour history: {error}")),
                }
            }
            Some(Err(error)) => self.status = Some(error),
            None => {}
        }
    }

    fn receive_plan(&mut self) {
        let response = match self.plan_results.try_lock() {
            Ok(mut slot) => slot.take(),
            Err(TryLockError::WouldBlock) => return,
            Err(TryLockError::Poisoned(_)) => {
                self.status = Some("Saved planner handoff poisoned".into());
                return;
            }
        };
        let Some(response) = response else {
            return;
        };
        if self.plan_pending != Some((response.sequence, response.size)) {
            if let Ok(Some((plan, route))) = response.outcome {
                self.retire_plan(plan);
                self.retire_route(route);
            }
            return;
        }
        self.plan_pending = None;
        match response.outcome {
            Ok(outcome) => match outcome {
                Some((plan, route)) => {
                    if let Some(old) = self.route.replace(route) {
                        self.retire_route(old);
                    }
                    if let Some(old) = self.plan.replace(plan) {
                        self.retire_plan(old);
                    }
                    self.active_size = Some(response.size);
                    self.unavailable_size = None;
                }
                None => {
                    self.unavailable_size = Some(response.size);
                    self.status = Some("No eligible saved tour in finite survey".into());
                }
            },
            Err(error) => {
                self.unavailable_size = Some(response.size);
                self.status = Some(format!("Saved tour planner: {error}"));
            }
        }
    }

    fn request_plan(&mut self, policy: Policy, size: [usize; 2], scale: f64) {
        if self.unavailable_size == Some(size)
            || self
                .plan_pending
                .is_some_and(|(_, requested)| requested == size)
        {
            return;
        }
        let Ok(mut slot) = self.plan_requests.try_lock() else {
            return;
        };
        let Some(controller) = self.controller.as_mut() else {
            return;
        };
        let cancelled = || false;
        let budget = tours::Budget::new(TOUR_WORK, &cancelled);
        let ticket = match controller.request(&budget) {
            Ok(ticket) => ticket,
            Err(error) => {
                self.status = Some(error.to_string());
                return;
            }
        };
        let Some(sequence) = self.plan_sequence.checked_add(1) else {
            self.status = Some("Saved planner serial exhausted".into());
            return;
        };
        self.plan_sequence = sequence;
        self.desired_plan.store(sequence, Ordering::Release);
        *slot = Some(PlanRequest {
            sequence,
            ticket,
            history: controller.history(),
            policy,
            size,
            scale,
        });
        self.plan_pending = Some((sequence, size));
        self.status = Some("Selecting a saved camera route…".into());
    }
    fn retire_plan(&mut self, plan: Plan) {
        if let Err(plan) = self.retired_plans.try_retire(plan) {
            self.pending_retired_plans.push(plan);
        }
    }
    fn retire_route(&mut self, route: Arc<projected_route::PreparedRoute>) {
        if let Err(route) = self.retired_routes.try_retire(route) {
            self.pending_retired_routes.push(route);
        }
    }
    fn retry_retired_plans(&mut self) {
        let mut pending = Vec::new();
        for plan in self.pending_retired_plans.drain(..) {
            if let Err(plan) = self.retired_plans.try_retire(plan) {
                pending.push(plan);
            }
        }
        self.pending_retired_plans = pending;
        let mut routes = Vec::new();
        for route in self.pending_retired_routes.drain(..) {
            if let Err(route) = self.retired_routes.try_retire(route) {
                routes.push(route);
            }
        }
        self.pending_retired_routes = routes;
    }

    fn policy(size: [usize; 2], scale: f64) -> Result<Policy, String> {
        if size.contains(&0) || !scale.is_finite() || scale <= 0.0 {
            return Err("Invalid saved viewport or zoom".into());
        }
        // Sixteen blocks only proves an initial route inside the catalog's
        // saved core. It is NOT the painted viewport radius. The worker's
        // inverse projected Request covers the actual full 2D viewport, full
        // block-Y domain and selected-model reach before publishing a route.
        Ok(Policy {
            envelope: Envelope {
                cell_heights: [-64, 319],
                viewport_radius: 16.0,
                horizontal_halo: 0.0,
                upward_overhang: 9.0,
                eye_y: 512.0,
            },
            minimum_length: 48.0,
            maximum_length: 1024.0,
            max_line_queries: 4096,
            minimum_confidence: Confidence::Supported,
            minimum_pixels: 1,
        })
    }
    fn clock(settings: &VoxelLandscapeSettings, time: Duration) -> Clock {
        let speed = f64::from(settings.pan_speed_percent.max(0)) / 100.0;
        Clock {
            time,
            local_speed: speed.min(64.0),
            frozen: speed == 0.0,
        }
    }
    fn persist(&mut self, history: History) {
        self.pending_history = Some(history);
        self.retry_persist();
    }
    fn retry_persist(&mut self) {
        let Some(history) = self.pending_history else {
            return;
        };
        match self.runtime.submit(history) {
            Ok(_) => self.pending_history = None,
            Err(super::saved_runtime::Error::Busy)
            | Err(super::saved_runtime::Error::Writer(super::history_writer::Error::Busy)) => {}
            Err(error) => self.status = Some(format!("Saved history requires recovery: {error}")),
        }
    }
}

impl Scene for SavedScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        self.receive();
        self.receive_plan();
        self.retry_retired_plans();
        self.retry_persist();
        frame.raster.dots.fill(0.0);
        frame.raster.owner_ids.fill(0);
        frame.cell_colors.fill([0; 3]);
        self.receipt = None;
        let Some(bundle) = self.bundle.as_ref() else {
            return;
        };
        if self.controller.is_none() {
            return;
        }
        let scale = f64::from(VoxelLandscapeScene::scale(&self.settings));
        let size = [frame.raster.width, frame.raster.height];
        if self
            .display
            .as_ref()
            .is_some_and(|display| !display.matches(frame, scale))
        {
            self.display = None;
        }
        if self.display.is_none() {
            // Admit the small presentation bridge before submitting the next
            // heavy route. It shares the unchanged scene account with models.
            match display::Display::new(frame, scale, &bundle.budget, Cancel::new(&RASTER_STOP)) {
                Ok(display) => self.display = Some(display),
                Err(error) => {
                    self.status = Some(format!("Saved display: {error}"));
                }
            }
        }
        let policy = match Self::policy(size, scale) {
            Ok(policy) => policy,
            Err(error) => {
                self.status = Some(error);
                return;
            }
        };
        if self.plan.is_none() {
            self.request_plan(policy, size, scale);
            if self.plan_pending.is_some() {
                if let Some(display) = self.display.as_ref() {
                    display.replay(frame, scale);
                }
            }
            return;
        }
        if self.active_size.is_some_and(|admitted| size != admitted) {
            let cancelled = || false;
            let budget = tours::Budget::new(TOUR_WORK, &cancelled);
            let retired = self
                .controller
                .as_mut()
                .and_then(|controller| controller.cancel_viewport(&budget).ok())
                .flatten();
            if let Some(retired) = retired {
                self.retire_plan(retired);
            }
            if let Some(retired) = self.plan.take() {
                self.retire_plan(retired);
            }
            if let Some(retired) = self.route.take() {
                self.retire_route(retired);
            }
            self.active_size = None;
            self.last_clock = None;
            self.plan_pending = None;
            self.unavailable_size = None;
            self.desired_plan.fetch_add(1, Ordering::AcqRel);
            self.request_plan(policy, size, scale);
            return;
        }
        let Some(plan) = self.plan.as_ref() else {
            return;
        };
        let Some(route) = self.route.as_ref() else {
            self.status = Some("Saved route lost its exact prepared source".into());
            return;
        };
        let Some(controller) = self.controller.as_mut() else {
            return;
        };
        let cancelled = || false;
        let budget = tours::Budget::new(TOUR_WORK, &cancelled);
        let clock = Self::clock(&self.settings, frame.time);
        let view = match controller.view() {
            Some(_) => controller.advance(plan.ticket(), clock, &budget),
            None => {
                match controller.start_projected(plan, Arc::clone(&route.display), clock, &budget) {
                    Ok(Some(view)) => Ok(view),
                    Ok(None) => return,
                    Err(error) => Err(error),
                }
            }
        };
        let view = match view {
            Ok(view) => view,
            Err(error) => {
                self.status = Some(error.to_string());
                return;
            }
        };
        let Some(issued) = controller
            .issued_view()
            .filter(|issued| issued.view() == view)
        else {
            self.status = Some("Saved controller did not seal its rendered view".into());
            return;
        };
        if route.map.source() != plan.source()
            || !route
                .request
                .matches_view(plan.line(), route.camera_height, size, scale)
        {
            self.status = Some("Saved route projection/source changed; reprepare required".into());
            return;
        }
        match paint(
            route,
            bundle,
            issued,
            scale,
            frame,
            &self.settings,
            &self.palette,
        ) {
            Ok(receipt) => {
                if let Some(display) = self.display.as_mut() {
                    display.capture(frame, scale);
                }
                self.receipt = Some(Arc::new(receipt));
                self.last_clock = Some(clock);
            }
            Err(error) => self.status = Some(format!("Saved raster: {error}")),
        }
    }
    fn receipt_bytes(&self) -> usize {
        self.receipt
            .as_ref()
            .map_or(0, |receipt| receipt.accounted_bytes)
    }
    fn seal_frame(&mut self, id: FrameReceiptId) {
        // This runs on the animation worker after its slot admission. The
        // displaced slot Arc drops here, never on the UI presentation thread.
        self.receipt_slots[id.slot()] = self
            .receipt
            .as_ref()
            .map(|receipt| (id, Arc::clone(receipt)));
    }
    fn presented_frame(&mut self, id: FrameReceiptId, painted: &[PaintedOwner]) {
        let Some((stored_id, receipt)) = self.receipt_slots[id.slot()].as_ref() else {
            return;
        };
        if *stored_id != id {
            return;
        }
        let receipt = Arc::clone(receipt);
        let Some(controller) = self.controller.as_mut() else {
            return;
        };
        let displayed = match receipt.owners.displayed(painted) {
            Ok(displayed) => displayed,
            Err(error) => {
                self.status = Some(error.to_string());
                return;
            }
        };
        let cancelled = || false;
        let mut budget = tours::Budget::new(TOUR_WORK, &cancelled);
        let presentation =
            match controller.presented_issued(&receipt.issued, &displayed, &mut budget) {
                Ok(presentation) => presentation,
                Err(tours::Error::Stale) => return,
                Err(error) => {
                    self.status = Some(error.to_string());
                    return;
                }
            };
        let mut changed = (presentation.credited_categories > 0).then(|| controller.history());
        if let (Some(clock), Some(plan)) = (self.last_clock, self.plan.as_ref()) {
            if controller
                .view()
                .is_some_and(|view| view.motion == Motion::Endpoint)
            {
                match controller.finish(plan.ticket(), clock, &budget) {
                    Ok(finished) => {
                        changed = Some(controller.history());
                        self.retire_plan(finished.retired);
                        if let Some(retired) = self.plan.take() {
                            self.retire_plan(retired);
                        }
                        if let Some(retired) = self.route.take() {
                            self.retire_route(retired);
                        }
                        self.active_size = None;
                        self.unavailable_size = None;
                    }
                    Err(tours::Error::NotComplete | tours::Error::Frozen) => {}
                    Err(error) => self.status = Some(error.to_string()),
                }
            }
        }
        if let Some(history) = changed {
            self.persist(history);
        }
    }
    fn uses_cell_colors(&self) -> bool {
        true
    }
    fn set_palette(&mut self, palette: &ScenePalette) {
        // The host delivers this even when unchanged. Invalidate only actual
        // palette changes, or every frame would erase the preparation bridge.
        if self.palette != *palette {
            self.palette = palette.clone();
            if let Some(display) = self.display.as_mut() {
                display.invalidate();
            }
        }
    }
    fn follows_palette(&self) -> bool {
        true
    }
    fn frames_per_second(&self) -> u32 {
        12
    }
    fn status(&self) -> Option<String> {
        self.status.clone()
    }
}
impl Drop for SavedScene {
    fn drop(&mut self) {
        if let Some(history) = self.pending_history.take() {
            match self.runtime.begin_handoff() {
                Ok(()) => {
                    if let Err(error) = self.terminal_history.try_send(history) {
                        self.runtime.finish_handoff(false);
                        tracing::error!(%error, "final saved history worker handoff failed");
                    }
                }
                Err(error) => {
                    tracing::error!(%error, "final saved history could not begin host fence");
                }
            }
        }
        if let Some(bundle) = self.bundle.take() {
            self.retired.retire_final(bundle);
        }
        if let Some(plan) = self.plan.take() {
            self.retired_plans.retire_final(plan);
        }
        if let Some(route) = self.route.take() {
            self.retired_routes.retire_final(route);
        }
        for plan in self.pending_retired_plans.drain(..) {
            self.retired_plans.retire_final(plan);
        }
        for route in self.pending_retired_routes.drain(..) {
            self.retired_routes.retire_final(route);
        }
        if let Some(worker) = self.worker.take() {
            worker.stop_in_background();
        } else if let Err(error) = self.runtime.retire() {
            tracing::warn!(%error, "saved history retirement deferred to successor gate");
        }
    }
}

fn finish_worker_handoff(runtime: &SavedRuntime, pending: Option<History>) {
    if let Some(history) = pending {
        let deadline = Instant::now() + Duration::from_secs(10);
        let accepted = loop {
            match runtime.submit(history) {
                Ok(_) => break true,
                Err(super::saved_runtime::Error::Busy)
                | Err(super::saved_runtime::Error::Writer(super::history_writer::Error::Busy))
                    if Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(error) => {
                    tracing::error!(%error, "final saved history was not accepted; authoritative reload required");
                    break false;
                }
            }
        };
        runtime.finish_handoff(accepted);
    }
    if let Err(error) = runtime.retire() {
        // The successor's gate retains and drains the same host-owned writer.
        tracing::warn!(%error, "saved history close deferred to successor gate");
    }
}

/// Catalog preparation is independent of raster size. It retains exact
/// initial evidence/route windows and their bound directories; no model bank
/// or clipped map is prepared until an emitted viewport requests a route.
fn prepare_bundle(
    root: PathBuf,
    storage: PathBuf,
    jar: PathBuf,
    generation: u64,
    preparation: PreparationResources,
    runtime: &SavedRuntime,
    stop: &AtomicBool,
) -> Result<Bundle, String> {
    let mut reload = false;
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err("Saved scene cancelled".into());
        }
        match runtime.gate() {
            Gate::Ready => break,
            Gate::ReloadRequired => {
                reload = true;
                break;
            }
            Gate::Poisoned => return Err("Saved history runtime lock poisoned".into()),
            Gate::Busy | Gate::Draining => std::thread::sleep(Duration::from_millis(25)),
        }
    }
    let cancelled = || stop.load(Ordering::Relaxed);
    let budget = preparation.budget;
    let catalog_reservation = budget
        .reserve(CATALOG_CHARGE, Cancel::new(stop))
        .map_err(|error| error.to_string())?;
    let catalog = session_catalog::prepare(
        &root,
        &storage,
        generation,
        pipeline::Limits::default(),
        &cancelled,
    )
    .map_err(|error| error.to_string())?;
    if reload {
        runtime
            .acknowledge_authoritative_reload()
            .map_err(|error| error.to_string())?;
    }
    let history = catalog.snapshot.history();
    let revision = catalog.snapshot.revision();
    if !catalog.maps.is_empty() {
        let writer = Writer::start(
            Repository::new(storage).map_err(|error| error.to_string())?,
            revision,
            &preparation.host,
        )
        .map_err(|error| error.to_string())?;
        runtime.install(writer).map_err(|error| error.to_string())?;
    }
    let mut warnings = Vec::new();
    if reload {
        warnings.push("Saved history was reloaded after an unaccepted or failed writer receipt; earlier final display credit is uncertain".into());
    }
    if catalog.maps.is_empty() {
        return Err(format!(
            "No qualified saved windows ({:?}, {} map reports)",
            catalog.availability,
            catalog.reports.len()
        ));
    }
    let mut bundle = Bundle {
        history,
        maps: catalog.maps,
        bindings: catalog.bindings,
        world_seeds: catalog.world_seeds,
        root,
        jar,
        warnings,
        budget,
        _catalog_reservation: Arc::new(catalog_reservation),
    };
    drop(catalog.metadata);
    drop(catalog.reports);
    drop(catalog.snapshot);
    let retained = bundle
        .retained_catalog_charge()
        .ok_or("Saved catalog retained storage charge overflow or missing loader accounting")?;
    Arc::get_mut(&mut bundle._catalog_reservation)
        .ok_or("Saved catalog storage was shared before publication")?
        .shrink_to(retained as u64)
        .map_err(|error| error.to_string())?;
    for map in &mut bundle.maps {
        Arc::get_mut(map)
            .ok_or("Saved catalog map was shared before storage admission")?
            .retain_catalog_charge(Arc::clone(&bundle._catalog_reservation))
            .map_err(|error| error.to_string())?;
    }
    Ok(bundle)
}

impl Bundle {
    /// Every field moved into Bundle is covered. Temporary metadata, reports
    /// and snapshot drop at return. Decode keeps the full 512 MiB reservation;
    /// only the retained payload shrinks. This is logical accounting, not RSS.
    fn retained_catalog_charge(&self) -> Option<usize> {
        let mut charge = std::mem::size_of::<Self>().checked_add(1 << 20)?;
        for bytes in [
            self.maps
                .capacity()
                .checked_mul(std::mem::size_of::<Arc<PreparedMap>>())?,
            self.bindings
                .capacity()
                .checked_mul(std::mem::size_of::<BoundMap>())?,
            self.world_seeds.len().checked_mul(4096)?,
            self.root.capacity(),
            self.jar.capacity(),
            self.warnings
                .capacity()
                .checked_mul(std::mem::size_of::<String>())?,
        ] {
            charge = charge.checked_add(bytes)?;
        }
        for map in &self.maps {
            charge = charge.checked_add(map.retained_catalog_charge()?)?;
        }
        for binding in &self.bindings {
            charge = charge.checked_add(binding.retained_heap_charge()?)?;
        }
        for warning in &self.warnings {
            charge = charge.checked_add(warning.capacity())?;
        }
        Some(charge)
    }
}

/// Reserve a finite survey for novelty first, then progressively weaker tour
/// choices. The original maximum is still tried, while smaller caps allow
/// complete source windows that cannot support the longest line.
fn qualification_tiers(maximum_length: f64) -> [(Choice, f64, usize); 14] {
    [
        (Choice::NovelAppearance, maximum_length, 1),
        (Choice::NovelAppearance, 512.0, 1),
        (Choice::NovelAppearance, 256.0, 2),
        (Choice::NovelAppearance, 160.0, 2),
        (Choice::NovelAppearance, 128.0, 1),
        (Choice::NovelAppearance, 64.0, 1),
        (Choice::RepeatedAppearance, 256.0, 1),
        (Choice::RepeatedAppearance, 160.0, 1),
        (Choice::RepeatedAppearance, 128.0, 1),
        (Choice::RepeatedAppearance, 64.0, 1),
        (Choice::OtherKind, 256.0, 1),
        (Choice::OtherKind, 64.0, 1),
        (Choice::SavedSurface, 256.0, 1),
        (Choice::SavedSurface, 64.0, 1),
    ]
}

enum SurveyStep<C> {
    Candidate(C),
    EmptyTier,
    StopSurvey,
}

/// Qualify a selected candidate before searching another slot or weaker tier.
/// The schedule, per-tier cap deduplication, and early return are shared by
/// production and the ordering regression; no phase can preempt a qualified
/// candidate in an earlier phase.
fn qualify_in_tier_order<C, R, E>(
    minimum_length: f64,
    maximum_length: f64,
    mut survey: impl FnMut(Choice, f64) -> Result<SurveyStep<C>, E>,
    mut qualify: impl FnMut(C) -> Result<Option<R>, E>,
) -> Result<Option<R>, E> {
    let mut last_choice = None;
    let mut surveyed_caps = BTreeSet::<u64>::new();
    for (choice, cap, route_slots) in qualification_tiers(maximum_length) {
        if last_choice != Some(choice) {
            surveyed_caps.clear();
            last_choice = Some(choice);
        }
        if cap < minimum_length || cap > maximum_length || !surveyed_caps.insert(cap.to_bits()) {
            continue;
        }
        for _ in 0..route_slots {
            match survey(choice, cap)? {
                SurveyStep::Candidate(candidate) => {
                    if let Some(qualified) = qualify(candidate)? {
                        return Ok(Some(qualified));
                    }
                }
                SurveyStep::EmptyTier => break,
                SurveyStep::StopSurvey => return Ok(None),
            }
        }
    }
    Ok(None)
}

/// The route policy certifies an initial position in the original evidence
/// window. ProjectedRoute separately certifies the ENTIRE actual viewport and
/// complete block-Y domain before any plan is published to the controller.
fn prepare_selection(
    bundle: &Bundle,
    request: &PlanRequest,
    settings: &VoxelLandscapeSettings,
    stop: &AtomicBool,
    desired_plan: &AtomicU64,
) -> Result<Option<(Plan, Arc<projected_route::PreparedRoute>)>, String> {
    // Header-only feasibility and an independent full decoder probe found
    // three 256-block source windows. Neither proves native render admission;
    // each selected route below still requires full projected qualification.
    const MAX_ROUTE_QUALIFICATIONS: usize = 16;
    const MAX_SELECTION_WORK: u64 = PLANNER_WORK * MAX_ROUTE_QUALIFICATIONS as u64;
    let cancelled =
        || stop.load(Ordering::Relaxed) || desired_plan.load(Ordering::Acquire) != request.sequence;
    let cancel = Cancel::for_revision(stop, desired_plan, request.sequence);
    let maps = bundle
        .maps
        .iter()
        .filter(|map| {
            bundle
                .world_seeds
                .get(&map.source().map)
                .copied()
                .flatten()
                .is_some()
                && bundle
                    .bindings
                    .iter()
                    .any(|bound| bound.map == map.source().map)
        })
        .cloned()
        .collect::<Vec<_>>();
    if maps.is_empty() {
        return Err("No saved map has canonical signed world seed and bound source".into());
    }
    let mut attempts_by_map = BTreeMap::<MapId, usize>::new();
    for map in &maps {
        attempts_by_map.insert(map.source().map, 0);
    }
    let mut excluded = BTreeSet::<RouteKey>::new();
    let mut previous_tier = None;
    let mut selection_passes = 0;
    let mut selection_work_used = 0_u64;
    let mut selection_limit = None;
    let mut query_limited_tiers = 0;
    let mut failures = Vec::<String>::new();
    let mut attempted = 0;
    let qualified: Option<(Plan, Arc<projected_route::PreparedRoute>, f64)> =
        qualify_in_tier_order(
            request.policy.minimum_length,
            request.policy.maximum_length,
            |choice, maximum_length| {
                let tier = (choice, maximum_length.to_bits());
                if previous_tier != Some(tier) {
                    excluded.clear();
                    previous_tier = Some(tier);
                }
                cancel.check().map_err(|error| error.to_string())?;
                let remaining = MAX_SELECTION_WORK.saturating_sub(selection_work_used);
                if remaining == 0 {
                    selection_limit = Some("cumulative planner work");
                    return Ok(SurveyStep::StopSurvey);
                }
                let policy = Policy {
                    maximum_length,
                    ..request.policy
                };
                let mut work = tours::Budget::new(PLANNER_WORK.min(remaining), &cancelled);
                // Every selector pass receives ALL maps; attempt count ranks
                // maps only within the required appearance phase.
                let selection = tours::select_diverse_excluding(
                    request.ticket,
                    &maps,
                    &request.history,
                    policy,
                    tours::CandidateSurvey {
                        excluded: &excluded,
                        attempted_maps: &attempts_by_map,
                        required_choice: Some(choice),
                    },
                    &mut work,
                );
                selection_passes += 1;
                selection_work_used += work.used();
                let selection = match selection {
                    Ok(selection) => selection,
                    Err(tours::Error::Limit("work")) => {
                        selection_limit = Some(if remaining < PLANNER_WORK {
                            "cumulative planner work"
                        } else {
                            "per-selection planner work"
                        });
                        tracing::warn!(
                            ?choice,
                            cap = maximum_length,
                            selection_passes,
                            selection_work_used,
                            "saved route candidate survey hit a finite work limit"
                        );
                        return Ok(if remaining < PLANNER_WORK {
                            SurveyStep::StopSurvey
                        } else {
                            SurveyStep::EmptyTier
                        });
                    }
                    Err(error) => return Err(error.to_string()),
                };
                if selection.audit.query_limited {
                    query_limited_tiers += 1;
                    tracing::warn!(
                        ?choice,
                        cap = maximum_length,
                        "saved route candidate tier reached its finite line-query cap"
                    );
                }
                let Some(plan) = selection.plan else {
                    return Ok(SurveyStep::EmptyTier);
                };
                // The exclusion and map-attempt count belong to the selected
                // candidate even if its projected source later fails.
                excluded.insert(plan.route());
                *attempts_by_map.entry(plan.source().map).or_default() += 1;
                Ok(SurveyStep::Candidate((maximum_length, plan)))
            },
            |(maximum_length, plan)| {
                cancel.check().map_err(|error| error.to_string())?;
                attempted += 1;
                let Some(initial_map) = maps.iter().find(|map| map.source() == plan.source())
                else {
                    return Err("Selected saved route lost its initial map".into());
                };
                let Some(bound) = bundle
                    .bindings
                    .iter()
                    .find(|bound| bound.map == plan.source().map)
                else {
                    return Err("Selected saved route lost its bound directory".into());
                };
                let Some(seed) = bundle
                    .world_seeds
                    .get(&plan.source().map)
                    .copied()
                    .flatten()
                else {
                    return Err("Selected saved route lost its canonical seed".into());
                };
                let selected = (!settings.pack_path.is_empty()).then_some(settings);
                match projected_route::prepare(projected_route::Inputs {
                    plan: &plan,
                    initial_map,
                    bound,
                    saves_root: &bundle.root,
                    jar: &bundle.jar,
                    selected,
                    world_seed: seed,
                    blend_radius: 2,
                    fancy_leaves: true,
                    viewport: request.size,
                    scale: request.scale,
                    account: &bundle.budget,
                    cancel,
                    cancelled: &cancelled,
                }) {
                    Ok(route) => Ok(Some((plan, Arc::new(route), maximum_length))),
                    Err(error) => {
                        if cancelled() {
                            return Err("Saved route preparation cancelled".into());
                        }
                        tracing::warn!(
                            map = ?plan.source().map,
                            cap = maximum_length,
                            length = plan.line().length(),
                            choice = ?plan.choice(),
                            route = ?plan.route(),
                            %error,
                            "saved route failed projected source or native qualification"
                        );
                        if failures.len() < 8 {
                            failures.push(format!(
                                "map {:?}, choice {:?}, cap {maximum_length}, length {:.1}, {:?}: {error}",
                                plan.source().map,
                                plan.choice(),
                                plan.line().length(),
                                plan.route()
                            ));
                        }
                        Ok(None)
                    }
                }
            },
        )?;
    if let Some((plan, route, maximum_length)) = qualified {
        tracing::info!(
            map = ?plan.source().map,
            cap = maximum_length,
            length = plan.line().length(),
            choice = ?plan.choice(),
            route = ?plan.route(),
            selection_passes,
            selection_work_used,
            query_limited_tiers,
            "saved route passed full projected source and native qualification"
        );
        return Ok(Some((plan, route)));
    }
    if attempted == 0 {
        if let Some(limit) = selection_limit {
            return Err(format!(
                "Saved route candidate survey reached {limit} after {selection_passes} selections and {selection_work_used}/{MAX_SELECTION_WORK} work units without a candidate"
            ));
        }
        if query_limited_tiers != 0 {
            return Err(format!(
                "Saved route candidate survey reached the finite line-query cap in {query_limited_tiers} tiers and found no candidate"
            ));
        }
        return Ok(None);
    }
    Err(format!(
        "{attempted}/{MAX_ROUTE_QUALIFICATIONS} finite saved routes failed full source/pack qualification after {selection_passes} selections and {selection_work_used}/{MAX_SELECTION_WORK} planner work units (survey limit: {}, query-limited tiers: {query_limited_tiers}): {}",
        selection_limit.unwrap_or("none"),
        failures.join("; ")
    ))
}

fn paint(
    route: &projected_route::PreparedRoute,
    bundle: &Bundle,
    issued: IssuedView,
    scale: f64,
    frame: &mut Frame<'_>,
    settings: &VoxelLandscapeSettings,
    palette: &ScenePalette,
) -> Result<FrameReceipt, String> {
    let look_at = issued.view().look_at;
    let cancel = Cancel::new(&RASTER_STOP);
    let size = [frame.raster.width, frame.raster.height];
    if size != route.viewport || route.scale.to_bits() != scale.to_bits() {
        return Err("Saved frame differs from certified viewport and zoom".into());
    }
    let mut pixels = RasterFrame::new(size, RasterLimits::default(), &bundle.budget, cancel)
        .map_err(|error| error.to_string())?;
    let camera = [look_at[0], look_at[2], route.camera_height];
    // One raster retains all tile depth, translucent layers and owners. Model
    // geometry goes first across every tile, then all fluids, so later tiles
    // cannot clear a preceding tile or let behind-fluid exhaust blend layers.
    for tile in &route.tiles {
        if tile.bank_epoch != route.bank_epoch
            || tile.mesh.bank != route.bank_epoch
            || tile.bank().identity() != route.bank_epoch
            || !Arc::ptr_eq(&tile.map, &route.map)
        {
            return Err("Saved tile model and selected bank differ".into());
        }
        surface_raster::draw_mesh_layer(
            &tile.mesh,
            tile.bank(),
            camera,
            scale,
            frame.time,
            DirectionalLight::default(),
            &mut pixels,
            cancel,
        )
        .map_err(|error| error.to_string())?;
    }
    for tile in &route.tiles {
        if let Some(fluid) = tile.fluid.as_ref() {
            if fluid.bank != route.bank_epoch {
                return Err("Saved fluid and selected bank differ".into());
            }
            // The native-specific UV path has no generated river drift. The
            // selected frame animation and scene directional light remain.
            surface_raster::draw_fluid_mesh(
                fluid,
                tile.bank(),
                camera,
                scale,
                frame.time,
                DirectionalLight::default(),
                &mut pixels,
                cancel,
            )
            .map_err(|error| error.to_string())?;
        }
    }
    let count = size[0]
        .checked_mul(size[1])
        .ok_or("Saved viewport size overflow")?;
    // MAX_OWNERS is 8192. Four MiB conservatively covers its Vec and
    // BTreeMap nodes at 256 bytes per entry plus frame-local bookkeeping;
    // the per-pixel term covers temporary color/coverage vectors. The same
    // retained charge is reported to the worker's frame-byte admission.
    let charge = 4_u64 * 1024 * 1024 + count as u64 * 8;
    let reservation = bundle
        .budget
        .reserve(charge, cancel)
        .map_err(|error| error.to_string())?;
    let accounted_bytes =
        usize::try_from(charge).map_err(|_| "Saved receipt byte charge overflow")?;
    let mut colors = Vec::new();
    let mut covered = Vec::new();
    colors
        .try_reserve_exact(count)
        .map_err(|error| error.to_string())?;
    covered
        .try_reserve_exact(count)
        .map_err(|error| error.to_string())?;
    for y in 0..size[1] {
        for x in 0..size[0] {
            let pixel = pixels.pixel(x, y).map_err(|error| error.to_string())?;
            let visible = pixel.color.alpha() > 0.0;
            colors.push(if visible {
                pixel
                    .color
                    .straight()
                    .map(surface_raster::linear_to_srgb_byte)
            } else {
                [0; 3]
            });
            covered.push(visible);
        }
    }
    composite_selected(&colors, &covered, frame, settings, palette);
    let mut owners = FrameOwners::new(Arc::clone(&route.map));
    for y in 0..size[1] {
        for x in 0..size[0] {
            let index = y * size[0] + x;
            if frame.raster.dots.get(index).is_some_and(|dot| *dot > 0.0) {
                let pixel = pixels.pixel(x, y).map_err(|error| error.to_string())?;
                let contributors = pixel
                    .contributors
                    .map(|part| part.map(|owner| owner.position));
                frame.raster.owner_ids[index] = owners.register(&contributors, true);
            }
        }
    }
    Ok(FrameReceipt {
        issued,
        owners,
        accounted_bytes,
        _reservation: reservation,
    })
}

#[cfg(test)]
mod palette_tests {
    use super::*;

    #[test]
    fn follows_palette_natively_and_stores_updates() {
        let mut env = SceneEnv::for_test(
            std::env::temp_dir().join("saved-scene-palette-test"),
            crate::resources::test_resources(),
        );
        let saved = SavedMapsSettings {
            source: crate::minecraft::settings::WorldSource::SavedMaps,
            saves_folder: "relative-folder".into(),
        };
        let none = SavedScene::new(&saved, &VoxelLandscapeSettings::default(), &env);
        assert!(none.follows_palette() && !none.palette.is_provided());
        env.palette = ScenePalette {
            stops: vec![[0, 0, 0], [255, 0, 0]],
            reverse: false,
            shift_percent: 0,
        };
        let mut scene = SavedScene::new(&saved, &VoxelLandscapeSettings::default(), &env);
        assert!(scene.palette.is_provided());
        scene.set_palette(&ScenePalette::default());
        assert!(!scene.palette.is_provided());
    }
}

#[cfg(test)]
#[path = "saved_scene_tests.rs"]
mod tests;
