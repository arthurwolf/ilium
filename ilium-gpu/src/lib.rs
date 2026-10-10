//! Optional GPU backend for the ambient scenes.
//!
//! The crate is always linked, but `wgpu` (and with it `naga` and every WGSL
//! shader) is compiled only with `--features gpu`. Without the feature,
//! [`start_probe`] just reports `NotCompiled`. With it, one admitted ambient
//! worker enumerates adapters, classifies them with the pure
//! [`diagnose::diagnose`], creates a device and publishes the result through
//! `ilium_ambient::gpu::set_gpu_availability`.

pub mod diagnose;
pub mod facts;

#[cfg(feature = "gpu")]
mod shaders;
#[cfg(feature = "gpu")]
mod wgpu_backend;

use ilium_ambient::gpu::{set_gpu_availability, GpuAvailability, GpuRunner, GpuUnavailable};
use ilium_ambient::resources::AmbientResources;
#[cfg(any(feature = "gpu", test))]
use ilium_ambient::resources::WorkerCost;
use ilium_ambient::source::Worker;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
#[cfg(feature = "gpu")]
use std::time::Instant;

static PROBE_STARTED: AtomicBool = AtomicBool::new(false);
static RUNNER: OnceLock<Arc<dyn GpuRunner>> = OnceLock::new();
static RUNNER_SOURCE: OnceLock<Arc<dyn GpuRunner>> = OnceLock::new();
#[cfg(any(feature = "gpu", test))]
const GPU_PROBE_STACK_BYTES: usize = 2 * 1024 * 1024;

/// Owns the admitted GPU capability probe for the client session.
#[must_use]
pub struct GpuProbeOwner(Option<Worker>);

impl GpuProbeOwner {
    /// Request cooperative cancellation and transfer physical-exit custody to
    /// a receipt that the shutdown path can join without blocking its runtime.
    pub fn request_stop(mut self) -> Option<ilium_ambient::source::WorkerRetirement> {
        self.0.take().and_then(Worker::request_stop)
    }
}

/// Starts the one-time GPU probe on an admitted worker. The returned owner
/// requests cancellation on drop; the client should join its retirement
/// receipt before shutting down its shared execution bank. Never blocks.
pub fn start_probe(resources: AmbientResources) -> Option<GpuProbeOwner> {
    if PROBE_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return None;
    }

    #[cfg(feature = "gpu")]
    {
        match spawn_probe_worker(&resources, run_probe) {
            Ok(owner) => Some(owner),
            Err(error) => {
                PROBE_STARTED.store(false, Ordering::Release);
                tracing::warn!(%error, "could not admit the GPU probe worker");
                set_gpu_availability(GpuAvailability::Unavailable(GpuUnavailable::Failed(
                    format!("could not start the GPU probe worker: {error}"),
                )));
                None
            }
        }
    }
    #[cfg(not(feature = "gpu"))]
    {
        let _ = resources;
        set_gpu_availability(GpuAvailability::Unavailable(GpuUnavailable::NotCompiled));
        None
    }
}

#[cfg(any(feature = "gpu", test))]
fn spawn_probe_worker(
    resources: &AmbientResources,
    task: impl FnOnce(Arc<AtomicBool>) + Send + 'static,
) -> Result<GpuProbeOwner, String> {
    let reservation = resources
        .reserve_worker(WorkerCost {
            threads: 1,
            resident_bytes: GPU_PROBE_STACK_BYTES,
        })
        .map_err(|error| format!("GPU probe worker admission refused: {error:?}"))?;
    let worker = Worker::start_admitted_with_stack(
        "gpu-probe",
        reservation,
        Some(GPU_PROBE_STACK_BYTES),
        task,
    )
    .map_err(|error| format!("GPU probe worker creation failed: {error}"))?;
    Ok(GpuProbeOwner(Some(worker)))
}

/// The shared runner source. While availability is `Checking`, this is a
/// stable proxy that begins forwarding to the eventual device after publication.
pub fn runner() -> Option<Arc<dyn GpuRunner>> {
    match ilium_ambient::gpu::gpu_availability() {
        GpuAvailability::Ready { .. } => RUNNER.get().cloned(),
        GpuAvailability::Unavailable(GpuUnavailable::Checking) => Some(Arc::clone(
            RUNNER_SOURCE.get_or_init(|| Arc::new(DeferredGpuRunner)),
        )),
        GpuAvailability::Unavailable(_) => None,
    }
}

struct DeferredGpuRunner;

impl GpuRunner for DeferredGpuRunner {
    fn adapter_name(&self) -> String {
        RUNNER.get().map_or_else(
            || "GPU probe is still checking".to_owned(),
            |runner| runner.adapter_name(),
        )
    }

    fn run(&self, job: &ilium_ambient::gpu::GpuJob, out: &mut [f32]) -> Result<(), String> {
        RUNNER
            .get()
            .ok_or_else(|| "GPU probe has not published a runner".to_owned())?
            .run(job, out)
    }
}

#[cfg(feature = "gpu")]
fn run_probe(stop: Arc<AtomicBool>) {
    let outcome = wgpu_backend::probe();
    if stop.load(Ordering::Acquire) {
        return;
    }
    if let Some(device_runner) = outcome.runner {
        // Only the probe thread ever sets this, once; a failed set would mean
        // a runner is already published, which is equally fine.
        let shared: Arc<dyn GpuRunner> = device_runner;
        let _ = RUNNER.set(shared);
    }
    tracing::info!(availability = ?outcome.availability, "GPU probe finished");
    set_gpu_availability(outcome.availability);
}

#[cfg(all(test, not(feature = "gpu")))]
mod tests {
    use super::*;
    use ilium_ambient::resources::{AmbientResources, WorkerCost};
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
    };
    use std::sync::mpsc;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    static GPU_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct PublishedRunner;

    impl GpuRunner for PublishedRunner {
        fn adapter_name(&self) -> String {
            "published-test-device".to_owned()
        }

        fn run(&self, _job: &ilium_ambient::gpu::GpuJob, out: &mut [f32]) -> Result<(), String> {
            out.fill(0.75);
            Ok(())
        }
    }

    fn isolated_resources() -> (Execution, AmbientResources) {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 2,
            jobs: 2,
            service_jobs: 0,
            input_bytes: 4096,
            result_bytes: 4096,
            worker_threads: 4,
            worker_bytes: 8 * 1024 * 1024,
        });
        let lane = LaneConfig {
            threads: 1,
            queue_slots: 2,
            priority: None,
            resident_bytes_per_thread: 1024,
        };
        let execution = Execution::start(
            quota,
            ExecutionConfig {
                cpu: lane,
                io: lane,
                service: LaneConfig {
                    threads: 0,
                    queue_slots: 0,
                    priority: None,
                    resident_bytes_per_thread: 0,
                },
            },
        )
        .unwrap();
        let client = execution
            .client(ClientLimits {
                jobs: 2,
                service_jobs: 0,
                input_bytes: 4096,
                result_bytes: 4096,
            })
            .unwrap();
        (execution, AmbientResources::new(client))
    }

    #[test]
    fn default_build_reports_not_compiled_and_has_no_runner() {
        let _guard = GPU_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let (mut execution, resources) = isolated_resources();
        let probe = start_probe(resources);
        drop(probe);
        assert_eq!(
            ilium_ambient::gpu::gpu_availability(),
            GpuAvailability::Unavailable(GpuUnavailable::NotCompiled)
        );
        assert!(runner().is_none());
        execution.request_shutdown(ShutdownMode::Cancel);
        execution
            .join_until_background(Instant::now() + Duration::from_secs(2))
            .unwrap();
    }

    #[test]
    fn scene_runner_source_resolves_device_published_after_scene_construction() {
        let _guard = GPU_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        set_gpu_availability(GpuAvailability::Unavailable(GpuUnavailable::Checking));
        let source = runner().expect("checking scenes retain the stable runner source");
        assert_eq!(source.adapter_name(), "GPU probe is still checking");
        assert!(Arc::ptr_eq(
            &source,
            &runner().expect("repeated lookup keeps the same source")
        ));
        if RUNNER.set(Arc::new(PublishedRunner)).is_err() {
            panic!("isolated test unexpectedly found an already-published runner");
        }
        set_gpu_availability(GpuAvailability::Ready {
            adapter: "published-test-device".to_owned(),
        });
        assert_eq!(source.adapter_name(), "published-test-device");
        let job = ilium_ambient::gpu::GpuJob {
            kernel: ilium_ambient::gpu::GpuKernel::FbmClouds,
            width: 1,
            height: 1,
            uniforms: Vec::new(),
        };
        let mut output = [0.0];
        source.run(&job, &mut output).unwrap();
        assert_eq!(output, [0.75]);
        set_gpu_availability(GpuAvailability::Unavailable(GpuUnavailable::NotCompiled));
    }

    #[test]
    fn admitted_probe_runs_on_a_named_worker_and_shutdown_joins_it() {
        let (mut execution, resources) = isolated_resources();
        let (started, wait_started) = mpsc::sync_channel(1);
        let owner = spawn_probe_worker(&resources, move |stop| {
            started
                .send(std::thread::current().name().unwrap().to_owned())
                .unwrap();
            while !stop.load(std::sync::atomic::Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(2));
            }
        })
        .unwrap();
        let thread_name = wait_started.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(thread_name.starts_with("ilium-ambient-gpu-probe"));
        let retirement = owner.request_stop().unwrap();
        retirement
            .join_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
        execution.request_shutdown(ShutdownMode::Cancel);
        execution
            .join_until_background(Instant::now() + Duration::from_secs(2))
            .unwrap();
    }

    #[test]
    fn probe_admission_refusal_does_not_start_a_worker() {
        let (mut execution, resources) = isolated_resources();
        let quota = resources.finite().quota_group();
        let mut reservations = Vec::new();
        loop {
            let snapshot = quota.snapshot();
            if snapshot.worker_threads == snapshot.limits.worker_threads
                || snapshot.worker_bytes == snapshot.limits.worker_bytes
            {
                break;
            }
            let Ok(reservation) = resources.reserve_worker(WorkerCost {
                threads: 1,
                resident_bytes: 1,
            }) else {
                break;
            };
            reservations.push(reservation);
        }

        let result = spawn_probe_worker(&resources, |_| {});
        assert!(matches!(result, Err(error) if error.contains("admission refused")));
        drop(reservations);
        execution.request_shutdown(ShutdownMode::Cancel);
        execution
            .join_until_background(Instant::now() + Duration::from_secs(2))
            .unwrap();
    }
}
