//! Ordered external application launches on the existing bounded I/O bank.
//!
//! Launch acknowledgement means the OS accepted the request. It never means
//! the browser/file manager exited successfully or that a document loaded.
use crate::open_target::OpenTarget;
use ilium_execution::{
    Client, ClientLimits, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt,
    RejectReason, Retained, Retention,
};
use ilium_platform::open_external::{ExternalOpenHandle, ExternalOpener};
use std::{collections::VecDeque, io, sync::Arc};
use tokio::sync::Notify;

const MAX_REQUESTS: usize = 8;
const MAX_TARGET_BYTES: usize = 64 * 1024;
const REQUEST_OVERHEAD: usize = 4096;
const MAX_ERROR_BYTES: usize = 8192;
// Lossy path display can expand each source byte threefold. The display and
// final status temporarily coexist, as can error text and its formatted copy.
const IO_RESULT_BYTES: usize = MAX_TARGET_BYTES * 6 + MAX_ERROR_BYTES * 2 + REQUEST_OVERHEAD;
// The platform owner requests a 2 MiB native stack; this declaration also
// covers its bounded child registry and synchronization metadata.
pub(crate) const REAPER_BYTES: usize = 3 * 1024 * 1024;

pub(crate) fn limits() -> ClientLimits {
    ClientLimits {
        // Eight original requests, one separately admitted I/O callback, and
        // the UI's last retained acknowledgement must coexist. Excluding the
        // acknowledgement would deadlock dispatch behind its own old result.
        jobs: MAX_REQUESTS + 2,
        service_jobs: 0,
        input_bytes: (MAX_REQUESTS + 2) * (MAX_TARGET_BYTES + REQUEST_OVERHEAD),
        // A completed UI status and its successor callback must both fit,
        // alongside the original-custody reservations of eight requests.
        result_bytes: IO_RESULT_BYTES * 2 + MAX_REQUESTS * REQUEST_OVERHEAD,
    }
}

struct Pending {
    target: OpenTarget,
    // A terminal-link leaf keeps the original source/result charge until launch.
    source_hold: Option<Arc<ilium_execution::StorageAdmission>>,
}

struct OpenJob {
    opener: OpenHandle,
    pending: Retained<Pending>,
}

#[derive(Clone)]
enum OpenHandle {
    Platform(ExternalOpenHandle),
    #[cfg(test)]
    Fixture(Arc<dyn Fn(&OpenTarget) -> io::Result<()> + Send + Sync>),
}

impl OpenHandle {
    fn open(&self, target: &OpenTarget) -> io::Result<()> {
        match self {
            Self::Platform(handle) => match target {
                OpenTarget::Url(url) => handle.open_url(url),
                OpenTarget::File(path) | OpenTarget::Directory(path) => handle.open_path(path),
            },
            #[cfg(test)]
            Self::Fixture(open) => open(target),
        }
    }
}

impl Job for OpenJob {
    type Output = String;
    type Error = String;

    fn run(self, _context: JobContext) -> Result<String, String> {
        // Accepted user actions are semantic operations. The owner drains them
        // before closing; newer clicks do not cancel a preceding launch.
        let pending = self.pending.view();
        let result = self.opener.open(&pending.target);
        let display = match &pending.target {
            OpenTarget::Url(url) => url.clone(),
            OpenTarget::File(path) | OpenTarget::Directory(path) => path.display().to_string(),
        };
        // Keep source custody explicit through the synchronous OS boundary.
        let _source_hold = &pending.source_hold;
        match result {
            Ok(()) => Ok(format!("Opening {display}")),
            Err(error) => {
                let mut error = error.to_string();
                let mut end = error.len().min(MAX_ERROR_BYTES);
                while !error.is_char_boundary(end) {
                    end -= 1;
                }
                error.truncate(end);
                Err(format!("Could not open {display}: {error}"))
            }
        }
    }
}

pub(crate) struct Completion {
    pub message: String,
    // The UI moves the original message alongside this guard; no extra clone.
    pub storage: Retention,
}

pub(crate) struct ExternalOpenService {
    client: Client,
    notification: Arc<Notify>,
    owner: Option<ExternalOpener>,
    handle: OpenHandle,
    pending: VecDeque<Retained<Pending>>,
    active: Option<Receipt<OpenJob>>,
    closing: bool,
    fault: Option<RejectReason>,
}

impl ExternalOpenService {
    pub(crate) fn start(client: Client) -> io::Result<Self> {
        let custody = crate::execution::process_quota()
            .reserve_external_worker(1, REAPER_BYTES)
            .map_err(|reason| io::Error::other(format!("external opener admission: {reason:?}")))?;
        let owner = ExternalOpener::start_with_custody(custody)?;
        let handle = OpenHandle::Platform(owner.handle());
        let notification = Arc::new(Notify::new());
        let wake = Arc::clone(&notification);
        Ok(Self {
            client: client.with_completion_wake(move || wake.notify_one()),
            notification,
            owner: Some(owner),
            handle,
            pending: VecDeque::with_capacity(MAX_REQUESTS),
            active: None,
            closing: false,
            fault: None,
        })
    }

    pub(crate) fn notification(&self) -> Arc<Notify> {
        Arc::clone(&self.notification)
    }

    pub(crate) fn submit(
        &mut self,
        target: OpenTarget,
        source_hold: Option<Arc<ilium_execution::StorageAdmission>>,
    ) -> Result<(), (OpenTarget, String)> {
        let bytes = match &target {
            OpenTarget::Url(url) => url.capacity(),
            OpenTarget::File(path) | OpenTarget::Directory(path) => path.capacity(),
        };
        if self.closing
            || self.fault.is_some()
            || self.pending.len() + usize::from(self.active.is_some()) >= MAX_REQUESTS
        {
            return Err((
                target,
                "External opening queue is closed or full; retry".into(),
            ));
        }
        if bytes > MAX_TARGET_BYTES {
            return Err((target, "External opening target exceeds 64 KiB".into()));
        }
        let reservation = match self.client.try_reserve_external(JobCost {
            input_bytes: bytes + REQUEST_OVERHEAD,
            result_bytes: REQUEST_OVERHEAD,
        }) {
            Ok(reservation) => reservation,
            Err(reason) => return Err((target, format!("External opening admission: {reason:?}"))),
        };
        match reservation.retain(Pending {
            target,
            source_hold,
        }) {
            Ok(pending) => self.pending.push_back(pending),
            Err(rejected) => {
                return Err((
                    rejected.value.target,
                    format!("External opening custody: {:?}", rejected.reason),
                ));
            }
        }
        self.dispatch();
        Ok(())
    }

    fn dispatch(&mut self) {
        if self.active.is_some() || self.pending.is_empty() || self.fault.is_some() {
            return;
        }
        // Reserve before removing the original FIFO head. Saturation leaves
        // every accepted action and its source custody in this ordered queue.
        let reservation = match self.client.try_reserve(
            Lane::Io,
            JobCost {
                input_bytes: REQUEST_OVERHEAD,
                result_bytes: IO_RESULT_BYTES,
            },
        ) {
            Ok(reservation) => reservation,
            Err(reason) => {
                if matches!(
                    reason,
                    RejectReason::Closed
                        | RejectReason::InvalidCost
                        | RejectReason::AccountingPoisoned
                ) {
                    self.fault = Some(reason);
                }
                return;
            }
        };
        let Some(pending) = self.pending.pop_front() else {
            return;
        };
        let job = OpenJob {
            opener: self.handle.clone(),
            pending,
        };
        match reservation.submit(job) {
            Ok(receipt) => self.active = Some(receipt),
            Err(rejected) => {
                if matches!(
                    rejected.reason,
                    RejectReason::Closed
                        | RejectReason::InvalidCost
                        | RejectReason::AccountingPoisoned
                ) {
                    self.fault = Some(rejected.reason);
                }
                self.pending.push_front(rejected.value.pending);
            }
        }
    }

    pub(crate) fn collect(&mut self) -> Option<Completion> {
        self.dispatch();
        if self.active.is_none() {
            if let Some(reason) = self.fault {
                let pending = self.pending.pop_front()?;
                let (_, storage) = pending.into_parts();
                return Some(Completion {
                    message: format!("External opening did not start: {reason:?}"),
                    storage,
                });
            }
        }
        let receipt = self.active.as_mut()?;
        let result = match receipt.try_take() {
            JobPoll::Pending => return None,
            JobPoll::Ready(outcome) => {
                let (outcome, storage) = outcome.into_parts();
                let message = match outcome {
                    JobOutcome::Finished(Ok(message) | Err(message)) => message,
                    JobOutcome::NotStarted { .. } => "External opening did not start".into(),
                    JobOutcome::Panicked => {
                        "External opening worker failed; launch outcome unknown".into()
                    }
                };
                Completion { message, storage }
            }
            JobPoll::Lost | JobPoll::Taken => Completion {
                message: "External opening acknowledgement lost; launch outcome unknown".into(),
                storage: receipt.retention(),
            },
        };
        self.active = None;
        self.dispatch();
        Some(result)
    }

    pub(crate) async fn shutdown(mut self) -> io::Result<()> {
        self.closing = true;
        let notification = Arc::clone(&self.notification);
        while self.active.is_some() || !self.pending.is_empty() {
            let ready = notification.notified();
            let admission = crate::execution::admission_notification();
            let available = admission.notified();
            tokio::pin!(ready, available);
            ready.as_mut().enable();
            available.as_mut().enable();
            while let Some(completion) = self.collect() {
                tracing::info!("{}", completion.message);
            }
            if self.active.is_none() && self.pending.is_empty() {
                break;
            }
            if !self.client.is_open() {
                return Err(io::Error::other(
                    "External opening bank closed with accepted requests pending",
                ));
            }
            tokio::select! {
                _ = ready => {},
                _ = available => {},
            }
        }
        // Close admissions, never kill a browser/file manager. The charged
        // platform owner keeps reaping accepted children until actual exit.
        if let Some(owner) = &self.owner {
            owner.close();
        }
        match self.fault {
            Some(reason) => Err(io::Error::other(format!(
                "External opening bank failed: {reason:?}"
            ))),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Condvar, Mutex};
    use std::time::Duration;

    fn fixture(
        open: impl Fn(&OpenTarget) -> io::Result<()> + Send + Sync + 'static,
    ) -> ExternalOpenService {
        let client = crate::execution::test_client().child(limits()).unwrap();
        let notification = Arc::new(Notify::new());
        let wake = Arc::clone(&notification);
        ExternalOpenService {
            client: client.with_completion_wake(move || wake.notify_one()),
            notification,
            owner: None,
            handle: OpenHandle::Fixture(Arc::new(open)),
            pending: VecDeque::with_capacity(MAX_REQUESTS),
            active: None,
            closing: false,
            fault: None,
        }
    }

    async fn completion(service: &mut ExternalOpenService) -> Completion {
        let notification = service.notification();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let ready = notification.notified();
                tokio::pin!(ready);
                ready.as_mut().enable();
                if let Some(completion) = service.collect() {
                    return completion;
                }
                ready.await;
            }
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn blocked_launch_keeps_fifo_and_refuses_original_ninth_target() {
        let caller = std::thread::current().id();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let opened = Arc::clone(&observed);
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let release = Arc::clone(&gate);
        let (started, wait_started) = mpsc::sync_channel(1);
        let mut service = fixture(move |target| {
            assert_ne!(std::thread::current().id(), caller);
            opened.lock().unwrap().push(target.clone());
            if matches!(target, OpenTarget::Url(url) if url == "0") {
                started.send(()).unwrap();
                let (lock, changed) = &*release;
                let result = changed
                    .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(5), |released| {
                        !*released
                    })
                    .unwrap();
                assert!(*result.0, "fixture gate was not released");
            }
            Ok(())
        });
        service.submit(OpenTarget::Url("-1".into()), None).unwrap();
        let preceding_acknowledgement = completion(&mut service).await;
        service.submit(OpenTarget::Url("0".into()), None).unwrap();
        wait_started.recv_timeout(Duration::from_secs(5)).unwrap();
        for index in 1..MAX_REQUESTS {
            service
                .submit(OpenTarget::Url(index.to_string()), None)
                .unwrap();
        }
        let rejected = String::from("original ninth target");
        let original_pointer = rejected.as_ptr();
        let (target, _) = service.submit(OpenTarget::Url(rejected), None).unwrap_err();
        assert!(matches!(target, OpenTarget::Url(url) if url.as_ptr() == original_pointer));
        assert!(service.collect().is_none());
        assert_eq!(observed.lock().unwrap().len(), 2);
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        for index in 0..MAX_REQUESTS {
            assert_eq!(
                completion(&mut service).await.message,
                format!("Opening {index}")
            );
        }
        let expected: Vec<_> = std::iter::once(OpenTarget::Url("-1".into()))
            .chain((0..MAX_REQUESTS).map(|index| OpenTarget::Url(index.to_string())))
            .collect();
        assert_eq!(*observed.lock().unwrap(), expected);
        assert_eq!(preceding_acknowledgement.message, "Opening -1");
        service.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn launch_failure_is_acknowledged_and_shutdown_drains_following_actions() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let opened = Arc::clone(&observed);
        let mut service = fixture(move |target| {
            opened.lock().unwrap().push(target.clone());
            if matches!(target, OpenTarget::Url(url) if url == "failure") {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "fixture refusal",
                ));
            }
            Ok(())
        });
        service
            .submit(OpenTarget::Url("failure".into()), None)
            .unwrap();
        service
            .submit(OpenTarget::Url("second".into()), None)
            .unwrap();
        service
            .submit(OpenTarget::Url("third".into()), None)
            .unwrap();
        assert_eq!(
            completion(&mut service).await.message,
            "Could not open failure: fixture refusal"
        );
        service.shutdown().await.unwrap();
        assert_eq!(
            *observed.lock().unwrap(),
            vec![
                OpenTarget::Url("failure".into()),
                OpenTarget::Url("second".into()),
                OpenTarget::Url("third".into())
            ]
        );
    }

    #[tokio::test]
    async fn permanently_closed_bank_reports_every_accepted_unstarted_action() {
        let mut service = fixture(|_| Ok(()));
        // Retain accepted originals while no callback is dispatched. This is
        // the same between-job state reached after an earlier launch finishes.
        for index in 0..3 {
            let reservation = service
                .client
                .try_reserve_external(JobCost {
                    input_bytes: REQUEST_OVERHEAD,
                    result_bytes: REQUEST_OVERHEAD,
                })
                .unwrap();
            service.pending.push_back(
                reservation
                    .retain(Pending {
                        target: OpenTarget::Url(index.to_string()),
                        source_hold: None,
                    })
                    .unwrap(),
            );
        }
        service.fault = Some(RejectReason::Closed);
        for _ in 0..3 {
            assert_eq!(
                service.collect().unwrap().message,
                "External opening did not start: Closed"
            );
        }
        assert!(service.pending.is_empty());
        assert!(service.shutdown().await.is_err());
    }

    #[test]
    fn oversized_target_returns_original_before_any_launch() {
        let mut service = fixture(|_| panic!("oversized target reached OS adapter"));
        let target = "x".repeat(MAX_TARGET_BYTES + 1);
        let original_pointer = target.as_ptr();
        let (target, error) = service.submit(OpenTarget::Url(target), None).unwrap_err();
        assert!(matches!(target, OpenTarget::Url(url) if url.as_ptr() == original_pointer));
        assert_eq!(error, "External opening target exceeds 64 KiB");
        assert!(service.pending.is_empty());
        assert!(service.active.is_none());
    }
}
