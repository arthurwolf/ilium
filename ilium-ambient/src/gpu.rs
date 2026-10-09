//! Optional GPU backend: the pure side of the contract.
//!
//! This crate contains no GPU code. It defines the vocabulary the host and
//! the `ilium-gpu` adapter share: why a GPU cannot be used
//! ([`GpuUnavailable`]), the process-wide availability the host publishes
//! ([`set_gpu_availability`]), one compute request ([`GpuJob`]), the trait a
//! real device implements ([`GpuRunner`]) and [`GpuFrameWorker`], which keeps
//! the blocking device call off the render path.
//!
//! Scenes never block on the GPU: they submit the newest job and draw the
//! newest finished frame, falling back to their software renderer until one
//! arrives.

use crate::control::{Control, ControlValue};
use crate::resources::{AmbientResources, Stored, WorkerCost};
use crate::scene::SceneEnv;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, RwLock};

/// Why the GPU option cannot be used. Pure data; the text helpers give the UI
/// its strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuUnavailable {
    /// Built without `--features gpu`.
    NotCompiled,
    /// The probe is still running.
    Checking,
    /// `libvulkan` / the platform loader was not found.
    NoVulkanLoader,
    /// The loader is present but there is no ICD or no adapter.
    NoDriver,
    /// Only llvmpipe / lavapipe / CPU adapters exist.
    SoftwareOnly { adapter: String },
    /// `/dev/dri` exists but this user cannot open the render node.
    NoDeviceAccess,
    /// Device or queue creation (or another step) failed.
    Failed(String),
}

impl GpuUnavailable {
    /// One short line for the help/status line.
    pub fn summary(&self) -> String {
        match self {
            Self::NotCompiled => "GPU support is not compiled into this build.".to_owned(),
            Self::Checking => "Checking for a usable GPU...".to_owned(),
            Self::NoVulkanLoader => "No Vulkan loader was found.".to_owned(),
            Self::NoDriver => "No GPU driver (Vulkan ICD) was found.".to_owned(),
            Self::SoftwareOnly { adapter } => {
                format!("Only a software rasterizer ({adapter}) is available.")
            }
            Self::NoDeviceAccess => "This user cannot access the GPU render node.".to_owned(),
            Self::Failed(message) => format!("GPU initialisation failed: {message}"),
        }
    }

    /// Multi-sentence "what to do", shown in the hover popover.
    pub fn fix(&self) -> String {
        match self {
            Self::NotCompiled => "Rebuild with `cargo build --release --features gpu` (needs the wgpu dependency), then reinstall.".to_owned(),
            Self::Checking => "Wait a moment; the check runs once in the background.".to_owned(),
            Self::NoVulkanLoader => "The Vulkan loader `libvulkan.so.1` was not found. Install it: `libvulkan1` on Debian/Ubuntu, `vulkan-icd-loader` on Arch, `vulkan-loader` on Fedora. On macOS and Windows, update the OS or the graphics driver.".to_owned(),
            Self::NoDriver => "The Vulkan loader is installed but no GPU driver was found. Install the vendor Vulkan driver (the NVIDIA proprietary driver, or `mesa-vulkan-drivers` for AMD and Intel) and verify it with `vulkaninfo --summary`.".to_owned(),
            Self::SoftwareOnly { adapter } => format!(
                "Only a CPU rasterizer ({adapter}) was found, which is no faster than the built-in renderer. Install a real GPU driver to use this option; software rendering is what the Software option already does."
            ),
            Self::NoDeviceAccess => "`/dev/dri/renderD*` exists but this user cannot open it. Add your user to the `render` (or `video`) group, then log out and back in.".to_owned(),
            Self::Failed(message) => format!(
                "The GPU could not be initialised: {message}. Update the graphics driver and try again."
            ),
        }
    }
}

/// What the host learned about the GPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuAvailability {
    Ready { adapter: String },
    Unavailable(GpuUnavailable),
}

static AVAILABILITY: RwLock<GpuAvailability> =
    RwLock::new(GpuAvailability::Unavailable(GpuUnavailable::Checking));

/// Publish the probe result. Called by the host (`ilium-gpu`).
pub fn set_gpu_availability(value: GpuAvailability) {
    // A poisoned lock still holds a whole value: overwrite it.
    let mut guard = AVAILABILITY
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = value;
}

/// The last published availability; `Unavailable(Checking)` until set.
pub fn gpu_availability() -> GpuAvailability {
    AVAILABILITY
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Which compute kernel a job runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuKernel {
    FbmClouds,
    DitheredWaves,
    DithrPatterns,
}

/// One compute request. The output is `width * height` f32 dot intensities
/// `0.0..=1.0`, row-major: the same field the software path writes into
/// `Frame::raster`.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuJob {
    pub kernel: GpuKernel,
    pub width: u32,
    pub height: u32,
    /// Kernel parameters; the layout is documented per scene in
    /// `scenes/<scene>/gpu.rs` and mirrored by the WGSL uniform struct.
    pub uniforms: Vec<f32>,
}

/// A device that can execute [`GpuJob`]s.
pub trait GpuRunner: Send + Sync {
    fn adapter_name(&self) -> String;
    /// Blocking; only ever called from a worker thread, never from
    /// `Scene::render`. `out` is `width * height` long.
    fn run(&self, job: &GpuJob, out: &mut [f32]) -> Result<(), String>;
}

/// A finished GPU frame.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuFrame {
    pub width: u32,
    pub height: u32,
    pub dots: Vec<f32>,
    /// 1 for the first finished frame, increasing by one per frame.
    pub sequence: u64,
}

#[derive(Default)]
struct WorkerState {
    pending: Option<GpuJob>,
    latest: Option<Arc<Stored<GpuFrame>>>,
    error: Option<String>,
    sequence: u64,
    shutdown: bool,
}

struct WorkerShared {
    state: Mutex<WorkerState>,
    wake: Condvar,
}

const MAX_GPU_PIXELS: usize = 4_000_000;
const MAX_GPU_UNIFORM_VALUES: usize = 64;
const GPU_WORKER_STACK_BYTES: usize = 2 * 1024 * 1024;
const GPU_WORKER_RESIDENT_BYTES: usize = GPU_WORKER_STACK_BYTES;

impl WorkerShared {
    fn lock(&self) -> MutexGuard<'_, WorkerState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Owns one admitted background worker that runs jobs on a [`GpuRunner`]. `submit`
/// never blocks and replaces any older pending job; `latest` returns the
/// newest finished frame. Physical joining remains with the platform supervisor.
pub struct GpuFrameWorker {
    shared: Arc<WorkerShared>,
    worker: Option<crate::source::Worker>,
}

impl GpuFrameWorker {
    pub fn new(runner: Arc<dyn GpuRunner>, resources: &AmbientResources) -> Result<Self, String> {
        let reservation = resources
            .reserve_worker(WorkerCost {
                threads: 1,
                resident_bytes: GPU_WORKER_RESIDENT_BYTES,
            })
            .map_err(|error| format!("GPU worker admission refused: {error:?}"))?;
        let shared = Arc::new(WorkerShared {
            state: Mutex::new(WorkerState::default()),
            wake: Condvar::new(),
        });
        let worker_shared = Arc::clone(&shared);
        let worker_resources = resources.clone();
        let worker = crate::source::Worker::start_admitted_with_stack(
            "gpu-frames",
            reservation,
            Some(GPU_WORKER_STACK_BYTES),
            move |stop| worker_loop(&worker_shared, runner.as_ref(), &stop, worker_resources),
        )
        .map_err(|error| format!("could not start the GPU worker: {error}"))?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }

    /// Queue `job`, dropping any job that has not started yet. Never blocks
    /// on the device.
    pub fn submit(&self, job: GpuJob) {
        if job.uniforms.len() > MAX_GPU_UNIFORM_VALUES {
            self.shared.lock().error = Some(format!(
                "GPU job exceeds the {MAX_GPU_UNIFORM_VALUES}-value uniform limit"
            ));
            return;
        }
        let replaced = {
            let mut state = self.shared.lock();
            if state.shutdown {
                return;
            }
            state.pending.replace(job)
        };
        drop(replaced);
        self.shared.wake.notify_one();
    }

    /// The newest finished frame, or `None` until the first one arrives.
    pub fn latest(&self) -> Option<Arc<Stored<GpuFrame>>> {
        self.shared.lock().latest.as_ref().map(Arc::clone)
    }

    /// The most recent failure, cleared by the next successful frame.
    pub fn last_error(&self) -> Option<String> {
        self.shared.lock().error.clone()
    }
}

impl Drop for GpuFrameWorker {
    fn drop(&mut self) {
        {
            let mut state = self.shared.lock();
            state.shutdown = true;
            state.pending = None;
        }
        self.shared.wake.notify_all();
        // Worker Drop requests cancellation; its admitted physical owner stays
        // with the bounded platform join supervisor until a blocked device call exits.
        drop(self.worker.take());
    }
}

fn worker_loop(
    shared: &WorkerShared,
    runner: &dyn GpuRunner,
    stop: &std::sync::atomic::AtomicBool,
    resources: AmbientResources,
) {
    loop {
        let job = {
            let mut state = shared.lock();
            loop {
                if state.shutdown || stop.load(std::sync::atomic::Ordering::Acquire) {
                    return;
                }
                if let Some(job) = state.pending.take() {
                    break job;
                }
                state = shared
                    .wake
                    .wait(state)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        };
        let outcome = run_job(runner, &job, &resources);
        let mut state = shared.lock();
        if state.shutdown || stop.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        match outcome {
            Ok((dots, storage)) => {
                state.sequence += 1;
                let sequence = state.sequence;
                let previous = state.latest.replace(Arc::new(Stored::new(
                    GpuFrame {
                        width: job.width,
                        height: job.height,
                        dots,
                        sequence,
                    },
                    storage,
                )));
                state.error = None;
                drop(state);
                drop(previous);
            }
            Err(message) => state.error = Some(message),
        }
    }
}

fn run_job(
    runner: &dyn GpuRunner,
    job: &GpuJob,
    resources: &AmbientResources,
) -> Result<(Vec<f32>, Arc<ilium_execution::StorageAdmission>), String> {
    if job.uniforms.len() > MAX_GPU_UNIFORM_VALUES {
        return Err(format!(
            "GPU job exceeds the {MAX_GPU_UNIFORM_VALUES}-value uniform limit"
        ));
    }
    let pixels = u64::from(job.width) * u64::from(job.height);
    if pixels > MAX_GPU_PIXELS as u64 {
        return Err(format!(
            "GPU frame exceeds the {MAX_GPU_PIXELS}-pixel limit"
        ));
    }
    let length = usize::try_from(pixels).map_err(|_| "GPU frame is too large".to_owned())?;
    if length == 0 {
        return Err("GPU frame has no pixels".to_owned());
    }
    let storage = resources
        .reserve_storage(length.saturating_mul(std::mem::size_of::<f32>()))
        .map_err(|error| format!("GPU frame storage admission refused: {error:?}"))?;
    let mut dots = vec![0.0_f32; length];
    runner.run(job, &mut dots)?;
    Ok((dots, storage))
}

/// Index of the GPU option in every scene's `render_backend` row.
pub(crate) const GPU_OPTION_INDEX: usize = 1;

/// Shown only while a GPU job is submitted but the shader is not ported.
const NOT_PORTED_STATUS: &str = "GPU kernel not ported yet; using software";

/// Apply the current availability to a `render_backend` row: disable the GPU
/// option with the fix text when unusable, name the adapter when ready.
pub(crate) fn decorate_backend_row(row: Control) -> Control {
    match gpu_availability() {
        GpuAvailability::Ready { adapter } => {
            row.with_help_detail(format!("GPU adapter: {adapter}."))
        }
        GpuAvailability::Unavailable(reason) => {
            row.with_disabled_option(GPU_OPTION_INDEX, reason.fix())
        }
    }
}

/// `Err(summary)` when `value` selects the GPU option while it is unusable.
pub(crate) fn reject_unavailable_choice(value: &ControlValue) -> Result<(), String> {
    if *value != ControlValue::Index(GPU_OPTION_INDEX) {
        return Ok(());
    }
    match gpu_availability() {
        GpuAvailability::Ready { .. } => Ok(()),
        GpuAvailability::Unavailable(reason) => Err(reason.summary()),
    }
}

/// Per-scene GPU state: whether the user asked for the GPU, the host runner
/// and the lazily created frame worker.
pub(crate) struct GpuBackend {
    requested: bool,
    runner: Option<Arc<dyn GpuRunner>>,
    resources: AmbientResources,
    worker: Option<GpuFrameWorker>,
    worker_error: Option<String>,
}

impl GpuBackend {
    pub(crate) fn new(requested: bool, env: &SceneEnv) -> Self {
        Self {
            requested,
            runner: env.gpu.clone(),
            resources: env.resources.clone(),
            worker: None,
            worker_error: None,
        }
    }

    /// Submit this frame's job when the GPU is selected and usable. The worker
    /// is created on first use, so a probe that finishes after the scene was
    /// built is still picked up.
    pub(crate) fn drive(&mut self, build_job: impl FnOnce() -> GpuJob) {
        if !self.requested {
            return;
        }
        if self.worker.is_none() && matches!(gpu_availability(), GpuAvailability::Ready { .. }) {
            if let Some(runner) = &self.runner {
                match GpuFrameWorker::new(Arc::clone(runner), &self.resources) {
                    Ok(worker) => {
                        self.worker = Some(worker);
                        self.worker_error = None;
                    }
                    Err(error) => self.worker_error = Some(error),
                }
            }
        }
        if let Some(worker) = &self.worker {
            worker.submit(build_job());
        }
    }

    /// Status line for the Settings panel; `None` when software was chosen.
    pub(crate) fn status(&self) -> Option<String> {
        if !self.requested {
            return None;
        }
        match gpu_availability() {
            GpuAvailability::Unavailable(reason) => Some(reason.summary()),
            GpuAvailability::Ready { .. } => match &self.worker {
                Some(worker) => Some(match worker.last_error() {
                    Some(error) => format!("GPU error: {error}; using software"),
                    None => NOT_PORTED_STATUS.to_owned(),
                }),
                None => Some(self.worker_error.clone().unwrap_or_else(|| {
                    "No GPU device was provided by the host; using software".to_owned()
                })),
            },
        }
    }

    /// The newest finished GPU frame, `None` until the first arrives or when
    /// the GPU is not in use.
    pub(crate) fn latest_frame(&self) -> Option<Arc<Stored<GpuFrame>>> {
        self.worker.as_ref().and_then(GpuFrameWorker::latest)
    }

    /// Status line for a scene whose kernel is ported: `is_displaying` says
    /// the scene is currently drawing GPU frames.
    pub(crate) fn ported_status(&self, is_displaying: bool) -> Option<String> {
        if !self.requested {
            return None;
        }
        if let GpuAvailability::Unavailable(reason) = gpu_availability() {
            return Some(reason.summary());
        }
        let (Some(worker), Some(runner)) = (&self.worker, &self.runner) else {
            return Some(self.worker_error.clone().unwrap_or_else(|| {
                "No GPU device was provided by the host; using software".to_owned()
            }));
        };
        if let Some(error) = worker.last_error() {
            return Some(format!("GPU error: {error}; using software"));
        }
        if is_displaying {
            let adapter = runner.adapter_name();
            return Some(format!("Rendering on GPU ({adapter})"));
        }
        Some("Starting the GPU renderer; using software until the first frame".to_owned())
    }
}

/// Serialises tests that touch the process-wide availability.
#[cfg(test)]
pub(crate) mod test_support {
    use super::{gpu_availability, set_gpu_availability, GpuAvailability, GpuUnavailable};
    use super::{GpuJob, GpuRunner};
    use std::sync::{Arc, Mutex, MutexGuard};

    /// Runner for scene tests: fails with "fake kernel not ported" (like a
    /// real runner given an empty uniform vector) or succeeds with zeros.
    pub(crate) struct ScriptedRunner {
        pub(crate) fail: bool,
    }

    impl GpuRunner for ScriptedRunner {
        fn adapter_name(&self) -> String {
            "Scripted".to_owned()
        }

        fn run(&self, _job: &GpuJob, out: &mut [f32]) -> Result<(), String> {
            if self.fail {
                return Err("fake kernel not ported".to_owned());
            }
            out.fill(0.0);
            Ok(())
        }
    }

    pub(crate) fn scripted_runner(fail: bool) -> Arc<dyn GpuRunner> {
        Arc::new(ScriptedRunner { fail })
    }

    static GLOBAL_STATE: Mutex<()> = Mutex::new(());

    /// Holds the global lock; restores `Unavailable(Checking)` on drop.
    pub(crate) struct AvailabilityGuard {
        _lock: MutexGuard<'static, ()>,
    }

    impl AvailabilityGuard {
        pub(crate) fn set(value: GpuAvailability) -> Self {
            let lock = GLOBAL_STATE
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            set_gpu_availability(value);
            Self { _lock: lock }
        }

        pub(crate) fn ready() -> Self {
            Self::set(GpuAvailability::Ready {
                adapter: "Test GPU".to_owned(),
            })
        }

        pub(crate) fn unavailable(reason: GpuUnavailable) -> Self {
            Self::set(GpuAvailability::Unavailable(reason))
        }
    }

    impl Drop for AvailabilityGuard {
        fn drop(&mut self) {
            set_gpu_availability(GpuAvailability::Unavailable(GpuUnavailable::Checking));
        }
    }

    #[test]
    fn guard_restores_the_default() {
        {
            let _guard = AvailabilityGuard::ready();
            assert!(matches!(gpu_availability(), GpuAvailability::Ready { .. }));
        }
        let _guard = AvailabilityGuard::unavailable(GpuUnavailable::Checking);
        assert_eq!(
            gpu_availability(),
            GpuAvailability::Unavailable(GpuUnavailable::Checking)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::AvailabilityGuard;
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// Fills the output with `uniforms[0]`; optionally fails or blocks.
    struct FakeRunner {
        fail: bool,
        gate: Option<Mutex<mpsc::Receiver<()>>>,
        runs: AtomicUsize,
        dropped: Arc<AtomicBool>,
        _flag: DropFlag,
    }

    /// Sets the shared flag when the runner is finally released.
    struct DropFlag(Arc<AtomicBool>);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    impl FakeRunner {
        fn new() -> Self {
            let dropped = Arc::new(AtomicBool::new(false));
            Self {
                fail: false,
                gate: None,
                runs: AtomicUsize::new(0),
                _flag: DropFlag(Arc::clone(&dropped)),
                dropped,
            }
        }
    }

    impl GpuRunner for FakeRunner {
        fn adapter_name(&self) -> String {
            "Fake".to_owned()
        }

        fn run(&self, job: &GpuJob, out: &mut [f32]) -> Result<(), String> {
            if let Some(gate) = &self.gate {
                let receiver = gate.lock().map_err(|_| "gate poisoned".to_owned())?;
                // Bounded: a scene dropped while a later job waits would
                // otherwise deadlock its worker join.
                receiver
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .map_err(|_| "gate closed".to_owned())?;
            }
            self.runs.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err("device lost".to_owned());
            }
            let value = job.uniforms.first().copied().unwrap_or(0.0);
            out.fill(value);
            Ok(())
        }
    }

    fn job(width: u32, height: u32, value: f32) -> GpuJob {
        GpuJob {
            kernel: GpuKernel::FbmClouds,
            width,
            height,
            uniforms: vec![value],
        }
    }

    fn wait_for<T>(mut probe: impl FnMut() -> Option<T>) -> Option<T> {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Some(found) = probe() {
                return Some(found);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        None
    }

    fn admitted_worker(runner: Arc<dyn GpuRunner>) -> GpuFrameWorker {
        GpuFrameWorker::new(runner, &crate::resources::test_resources())
            .expect("test GPU worker admission")
    }

    #[test]
    fn texts_match_the_contract() {
        assert_eq!(
            GpuUnavailable::NotCompiled.summary(),
            "GPU support is not compiled into this build."
        );
        assert_eq!(
            GpuUnavailable::NotCompiled.fix(),
            "Rebuild with `cargo build --release --features gpu` (needs the wgpu dependency), then reinstall."
        );
        assert_eq!(
            GpuUnavailable::Checking.summary(),
            "Checking for a usable GPU..."
        );
        assert_eq!(
            GpuUnavailable::Checking.fix(),
            "Wait a moment; the check runs once in the background."
        );
        let loader = GpuUnavailable::NoVulkanLoader.fix();
        for needle in [
            "libvulkan.so.1",
            "libvulkan1",
            "vulkan-icd-loader",
            "vulkan-loader",
        ] {
            assert!(loader.contains(needle), "{needle}");
        }
        let driver = GpuUnavailable::NoDriver.fix();
        for needle in ["NVIDIA", "mesa-vulkan-drivers", "vulkaninfo --summary"] {
            assert!(driver.contains(needle), "{needle}");
        }
        let software = GpuUnavailable::SoftwareOnly {
            adapter: "llvmpipe".to_owned(),
        };
        assert!(software.summary().contains("llvmpipe"));
        assert!(software.fix().contains("llvmpipe"));
        let access = GpuUnavailable::NoDeviceAccess.fix();
        assert!(access.contains("/dev/dri/renderD*") && access.contains("render"));
        let failed = GpuUnavailable::Failed("boom".to_owned());
        assert!(failed.summary().contains("boom"));
        assert!(failed.fix().contains("boom") && failed.fix().contains("driver"));
    }

    #[test]
    fn availability_defaults_to_checking_and_round_trips() {
        let _guard = AvailabilityGuard::unavailable(GpuUnavailable::Checking);
        assert_eq!(
            gpu_availability(),
            GpuAvailability::Unavailable(GpuUnavailable::Checking)
        );
        set_gpu_availability(GpuAvailability::Ready {
            adapter: "X".to_owned(),
        });
        assert_eq!(
            gpu_availability(),
            GpuAvailability::Ready {
                adapter: "X".to_owned()
            }
        );
    }

    #[test]
    fn worker_delivers_a_frame_with_increasing_sequence() {
        let worker = admitted_worker(Arc::new(FakeRunner::new()));
        assert!(worker.latest().is_none());
        worker.submit(job(3, 2, 0.25));
        let first = wait_for(|| worker.latest());
        let first = first.expect("first frame");
        assert_eq!(
            (
                first.view().width,
                first.view().height,
                first.view().sequence
            ),
            (3, 2, 1)
        );
        assert_eq!(first.view().dots, vec![0.25; 6]);
        let retained = worker.latest().expect("retained latest frame");
        assert!(Arc::ptr_eq(&first, &retained));
        worker.submit(job(3, 2, 0.75));
        let second = wait_for(|| worker.latest().filter(|frame| frame.view().sequence == 2));
        assert_eq!(second.expect("second frame").view().dots, vec![0.75; 6]);
        assert_eq!(worker.last_error(), None);
    }

    #[test]
    fn rejects_gpu_frames_over_the_bounded_pixel_budget_before_calling_the_device() {
        // Four million f32 pixels cap output storage at 16 MiB; one column
        // beyond the square boundary must be refused before allocating or invoking GPU code.
        let runner = FakeRunner::new();
        let result = run_job(
            &runner,
            &job(2049, 2048, 0.25),
            &crate::resources::test_resources(),
        );
        assert!(
            result
                .as_ref()
                .is_err_and(|error| error.contains("pixel limit")),
            "an oversized frame must be refused explicitly"
        );
        assert_eq!(runner.runs.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn rejects_gpu_jobs_over_the_bounded_uniform_budget_before_calling_the_device() {
        let runner = FakeRunner::new();
        let mut oversized = job(1, 1, 0.25);
        oversized.uniforms.resize(MAX_GPU_UNIFORM_VALUES + 1, 0.0);
        let result = run_job(&runner, &oversized, &crate::resources::test_resources());
        assert!(result
            .as_ref()
            .is_err_and(|error| error.contains("uniform limit")));
        assert_eq!(runner.runs.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn submit_never_blocks_and_keeps_only_the_newest_pending_job() {
        let (release, gate) = mpsc::channel();
        let runner = Arc::new(FakeRunner {
            gate: Some(Mutex::new(gate)),
            ..FakeRunner::new()
        });
        let worker = admitted_worker(runner.clone());
        worker.submit(job(1, 1, 0.1));
        // Wait until the worker holds job 1 inside the (blocked) runner.
        std::thread::sleep(Duration::from_millis(50));
        let started = Instant::now();
        for value in [0.2, 0.3, 0.4] {
            worker.submit(job(1, 1, value));
        }
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(worker.latest().is_none());
        // Release job 1 and the single surviving pending job.
        assert!(release.send(()).is_ok());
        assert!(release.send(()).is_ok());
        let last = wait_for(|| worker.latest().filter(|frame| frame.view().sequence == 2));
        assert_eq!(last.expect("newest frame").view().dots, vec![0.4]);
        assert_eq!(runner.runs.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn dropping_a_scene_does_not_wait_for_a_blocked_device_call() {
        struct BlockingRunner {
            entered: mpsc::SyncSender<()>,
            release: Mutex<mpsc::Receiver<()>>,
            dropped: Arc<AtomicBool>,
        }
        impl GpuRunner for BlockingRunner {
            fn adapter_name(&self) -> String {
                "Blocking fake".to_owned()
            }

            fn run(&self, _job: &GpuJob, out: &mut [f32]) -> Result<(), String> {
                self.entered
                    .send(())
                    .map_err(|_| "test receiver closed".to_owned())?;
                self.release
                    .lock()
                    .map_err(|_| "test gate poisoned".to_owned())?
                    .recv()
                    .map_err(|_| "test gate closed".to_owned())?;
                out.fill(0.5);
                Ok(())
            }
        }
        impl Drop for BlockingRunner {
            fn drop(&mut self) {
                self.dropped.store(true, Ordering::Release);
            }
        }

        let (entered, entered_rx) = mpsc::sync_channel(1);
        let (release, release_rx) = mpsc::channel();
        let dropped = Arc::new(AtomicBool::new(false));
        let worker = admitted_worker(Arc::new(BlockingRunner {
            entered,
            release: Mutex::new(release_rx),
            dropped: Arc::clone(&dropped),
        }));
        worker.submit(job(1, 1, 0.5));
        entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("device call started");

        let release_thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            release.send(()).unwrap();
        });
        let started = Instant::now();
        drop(worker);
        let drop_elapsed = started.elapsed();
        release_thread.join().unwrap();

        assert!(
            drop_elapsed < Duration::from_millis(100),
            "dropping a scene waited {drop_elapsed:?} for device work"
        );
        assert!(wait_for(|| dropped.load(Ordering::Acquire).then_some(())).is_some());
    }

    #[test]
    fn errors_surface_and_clear_on_the_next_good_frame() {
        let worker = admitted_worker(Arc::new(FakeRunner {
            fail: true,
            ..FakeRunner::new()
        }));
        worker.submit(job(2, 2, 1.0));
        let error = wait_for(|| worker.last_error());
        assert_eq!(error.as_deref(), Some("device lost"));
        assert!(worker.latest().is_none());

        let worker = admitted_worker(Arc::new(FakeRunner::new()));
        worker.submit(job(0, 4, 1.0));
        let error = wait_for(|| worker.last_error());
        assert_eq!(error.as_deref(), Some("GPU frame has no pixels"));
        worker.submit(job(2, 2, 1.0));
        assert!(wait_for(|| worker.latest()).is_some());
        assert_eq!(worker.last_error(), None);
    }

    #[test]
    fn dropping_the_worker_requests_shutdown_and_supervisor_joins_the_thread() {
        let runner = Arc::new(FakeRunner::new());
        let dropped = Arc::clone(&runner.dropped);
        let worker = admitted_worker(runner.clone());
        worker.submit(job(2, 2, 1.0));
        assert!(wait_for(|| worker.latest()).is_some());
        drop(runner);
        assert!(
            !dropped.load(Ordering::SeqCst),
            "worker still owns the runner"
        );
        drop(worker);
        // The supervisor retains the physical owner until the runner returns.
        assert!(wait_for(|| dropped.load(Ordering::SeqCst).then_some(())).is_some());
    }

    #[test]
    fn dropping_an_idle_worker_does_not_hang() {
        let started = Instant::now();
        drop(admitted_worker(Arc::new(FakeRunner::new())));
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
