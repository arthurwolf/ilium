//! Owned preparation and bounded handoff; the animation thread performs no I/O.
use super::loader;
use crate::{
    resources::{AmbientResources, WorkerCost},
    source::Worker,
};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, AtomicU8, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex, TryLockError,
    },
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct Request {
    /// Scene-owned monotonic epoch, including changes A -> B -> A.
    pub generation: u64,
    pub region_directory: PathBuf,
    pub chunks: BTreeSet<[i32; 2]>,
    pub limits: loader::Limits,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Accepted,
    Busy,
    Stale,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid saved preparation request")]
    InvalidRequest,
    #[error("saved preparation handoff busy")]
    Busy,
    #[error("saved preparation lock poisoned")]
    Poisoned,
    #[error("saved preparation worker stopped")]
    Stopped,
    #[error("saved preparation worker panicked")]
    Panicked,
}
pub struct Prepared<T> {
    pub generation: u64,
    pub result: Result<T, String>,
}
struct Pending<T> {
    request: Option<Request>,
    retired_request: Option<Request>,
    retired_output: Option<Arc<Prepared<T>>>,
}
struct Shared<T> {
    pending: Mutex<Pending<T>>,
    latest: Mutex<Option<Arc<Prepared<T>>>>,
    desired: AtomicU64,
    fault: AtomicU8,
}
pub struct PreparationWorker<T: Send + Sync + 'static> {
    worker: Option<Worker>,
    shared: Arc<Shared<T>>,
    wake: SyncSender<()>,
}
impl<T: Send + Sync + 'static> PreparationWorker<T> {
    pub fn start_with(
        resources: &AmbientResources,
        cost: WorkerCost,
        mut prepare: impl FnMut(&Request, &dyn Fn() -> bool) -> Result<T, String> + Send + 'static,
    ) -> Result<Self, String> {
        let reservation = resources
            .reserve_worker(cost)
            .map_err(|error| format!("saved preparation admission rejected: {error:?}"))?;
        let shared = Arc::new(Shared {
            pending: Mutex::new(Pending {
                request: None,
                retired_request: None,
                retired_output: None,
            }),
            latest: Mutex::new(None),
            desired: AtomicU64::new(0),
            fault: AtomicU8::new(0),
        });
        let state = Arc::clone(&shared);
        let (wake, receiver) = mpsc::sync_channel(1);
        let worker = Worker::start_admitted("minecraft-saved", reservation, move |stop| {
            ilium_platform::thread_priority::lower_current_thread(
                ilium_platform::thread_priority::WorkerPriority::BelowNormal,
            );
            while !stop.load(Ordering::Relaxed) {
                match receiver.recv_timeout(Duration::from_millis(25)) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                while receiver.try_recv().is_ok() {}
                let (request, retired_request, retired_output) = match state.pending.lock() {
                    Ok(mut pending) => (
                        pending.request.take(),
                        pending.retired_request.take(),
                        pending.retired_output.take(),
                    ),
                    Err(poisoned) => {
                        state.fault.store(1, Ordering::Release);
                        let mut pending = poisoned.into_inner();
                        (
                            pending.request.take(),
                            pending.retired_request.take(),
                            pending.retired_output.take(),
                        )
                    }
                };
                drop(retired_request);
                drop(retired_output);
                // A preparation failure disables new work, but this owner stays
                // alive to retire the scene's already-published output safely.
                if state.fault.load(Ordering::Acquire) != 0 {
                    continue;
                }
                let Some(request) = request else { continue };
                let cancelled = || {
                    stop.load(Ordering::Relaxed)
                        || state.desired.load(Ordering::Acquire) != request.generation
                };
                if cancelled() {
                    continue;
                }
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    prepare(&request, &cancelled)
                }));
                let Ok(result) = result else {
                    state.fault.store(3, Ordering::Release);
                    continue;
                };
                if cancelled() {
                    continue;
                }
                let prepared = Arc::new(Prepared {
                    generation: request.generation,
                    result: result.map_err(|error| error.chars().take(512).collect()),
                });
                let retired = match state.latest.lock() {
                    Ok(mut latest) => {
                        if cancelled() {
                            None
                        } else {
                            latest.replace(prepared)
                        }
                    }
                    Err(_) => {
                        state.fault.store(1, Ordering::Release);
                        continue;
                    }
                };
                drop(retired);
            }
            state
                .fault
                .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
                .ok();
            let retired = {
                let mut pending = state
                    .pending
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                (
                    pending.request.take(),
                    pending.retired_request.take(),
                    pending.retired_output.take(),
                )
            };
            drop(retired);
            let output = state
                .latest
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take();
            drop(output);
        })
        .map_err(|error| format!("saved preparation worker failed to start: {error}"))?;
        Ok(Self {
            worker: Some(worker),
            shared,
            wake,
        })
    }

    fn health(&self) -> Result<(), Error> {
        match self.shared.fault.load(Ordering::Acquire) {
            0 => Ok(()),
            1 => Err(Error::Poisoned),
            2 => Err(Error::Stopped),
            _ => Err(Error::Panicked),
        }
    }

    /// Busy admission retains caller ownership. Retry the SAME generation until
    /// accepted; every distinct map/pack/view request must get a new generation.
    pub fn submit(&self, request: &Request) -> Result<Admission, Error> {
        if request.generation == 0
            || request.chunks.is_empty()
            || request.chunks.len() > request.limits.max_chunks.min(128)
            || request.region_directory.as_os_str().len() > 4096
        {
            return Err(Error::InvalidRequest);
        }
        self.health()?;
        self.shared
            .desired
            .fetch_max(request.generation, Ordering::AcqRel);
        if self.shared.desired.load(Ordering::Acquire) != request.generation {
            return Ok(Admission::Stale);
        }
        {
            let mut pending = match self.shared.pending.try_lock() {
                Ok(pending) => pending,
                Err(TryLockError::WouldBlock) => return Ok(Admission::Busy),
                Err(TryLockError::Poisoned(_)) => return Err(Error::Poisoned),
            };
            self.health()?;
            if self.shared.desired.load(Ordering::Acquire) != request.generation {
                return Ok(Admission::Stale);
            }
            if pending.retired_request.is_some() {
                return Ok(Admission::Busy);
            }
            pending.retired_request = pending.request.replace(request.clone());
        }
        match self.wake.try_send(()) {
            Ok(()) | Err(mpsc::TrySendError::Full(())) => Ok(Admission::Accepted),
            Err(mpsc::TrySendError::Disconnected(())) => Err(Error::Stopped),
        }
    }

    /// Exchange on the presentation thread without waiting. Old output is moved
    /// to the worker's retirement slot before replacing the caller's Arc.
    pub fn update(&self, current: &mut Option<Arc<Prepared<T>>>) -> Result<bool, Error> {
        self.health()?;
        let latest = match self.shared.latest.try_lock() {
            Ok(latest) => latest,
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Poisoned(_)) => return Err(Error::Poisoned),
        };
        let Some(latest) = latest.as_ref() else {
            return Ok(false);
        };
        if latest.generation != self.shared.desired.load(Ordering::Acquire)
            || current
                .as_ref()
                .is_some_and(|value| value.generation == latest.generation)
        {
            return Ok(false);
        }
        let mut pending = match self.shared.pending.try_lock() {
            Ok(pending) => pending,
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Poisoned(_)) => return Err(Error::Poisoned),
        };
        if pending.retired_output.is_some()
            || latest.generation != self.shared.desired.load(Ordering::Acquire)
        {
            return Ok(false);
        }
        pending.retired_output = current.replace(Arc::clone(latest));
        let _ = self.wake.try_send(());
        Ok(true)
    }

    /// Remains usable after a preparation fault: the owned worker disables
    /// preparation while continuing retirement until this handle is dropped.
    pub fn retire_current(&self, current: &mut Option<Arc<Prepared<T>>>) -> Result<(), Error> {
        let mut pending = match self.shared.pending.try_lock() {
            Ok(pending) => pending,
            Err(TryLockError::WouldBlock) => return Err(Error::Busy),
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        };
        if pending.retired_output.is_some() {
            return Err(Error::Busy);
        }
        pending.retired_output = current.take();
        let _ = self.wake.try_send(());
        Ok(())
    }
}

impl PreparationWorker<loader::LoadedWindow> {
    pub fn start_loader(resources: &AmbientResources, cost: WorkerCost) -> Result<Self, String> {
        Self::start_with(resources, cost, |request, cancelled| {
            loader::load_window(
                &request.region_directory,
                &request.chunks,
                request.limits,
                cancelled,
            )
            .map_err(|error| error.to_string())
        })
    }
}

impl<T: Send + Sync + 'static> Drop for PreparationWorker<T> {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop_in_background();
        }
    }
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
