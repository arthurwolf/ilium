//! Host implementation of `ilium:host/jobs` (api-jobs).
//!
//! A job is a unit of background work a plugin declares in `contributes.jobs` (`kind`,
//! `deadline_ms`, `memory_mib`). `start` queues it in the runtime's [`JobBook`]; the runtime thread
//! starts a fresh job instance of the same component for it (`runtime/jobs.rs`) and runs the
//! `job-runner` export on a worker thread under the job's deadline and memory cap. The worker
//! queues `job-progress` for every `report-progress` call and `request-completed` when the job
//! ends; the output waits for `take-output`. Everything is keyed by the plugin that started the
//! job: another plugin can neither cancel nor read it. A stopped plugin loses its jobs.

use ilium_plugin_api::generated::host::jobs::HostJobs;
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::events::{Event, JobProgressEvent};
use ilium_plugin_api::generated::types::ids::RequestId;
use ilium_plugin_api::generated::types::Outcome;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use super::env::{completion_event, ImportEnv};
use crate::runtime::pending_events::EventSink;

/// Default `job_io_mib` limit for a job's input and output: 16 MiB.
const JOB_IO_LIMIT_BYTES: usize = 16 * 1024 * 1024;
/// Jobs of one plugin that are queued or running at once before `busy`.
const JOBS_PER_PLUGIN: usize = 8;
/// Default `deadline_ms` of a job kind that declares none.
pub const DEFAULT_JOB_DEADLINE_MS: u64 = 30_000;
/// Default `memory_mib` of a job kind that declares none.
pub const DEFAULT_JOB_MEMORY_MIB: u64 = 64;

fn error(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_string(),
        retry_after_ms: None,
    }
}

/// One entry of `contributes.jobs` of a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobDeclaration {
    /// The job kind a plugin passes to `start`.
    pub kind: String,
    /// The most time one run may take.
    pub deadline_ms: u64,
    /// The most memory one run may use.
    pub memory_mib: u64,
}

impl JobDeclaration {
    /// Reads the manifest object of a `contributes.jobs` entry; `None` when it names no kind.
    pub fn from_manifest(entry: &serde_json::Value) -> Option<Self> {
        let kind = entry.get("kind")?.as_str()?.to_owned();
        let number = |key: &str, default: u64| {
            entry
                .get(key)
                .and_then(serde_json::Value::as_u64)
                .filter(|value| *value > 0)
                .unwrap_or(default)
        };
        Some(Self {
            kind,
            deadline_ms: number("deadline_ms", DEFAULT_JOB_DEADLINE_MS),
            memory_mib: number("memory_mib", DEFAULT_JOB_MEMORY_MIB),
        })
    }
}

/// Where a job is in its life.
#[derive(Debug, Clone, PartialEq)]
enum JobStatus {
    Queued,
    Running,
    Finished(Result<(), PluginError>),
}

#[derive(Debug)]
struct JobRecord {
    plugin: String,
    instance: u64,
    kind: String,
    input: Vec<u8>,
    deadline_ms: u64,
    memory_mib: u64,
    cancel: Arc<AtomicBool>,
    progress: f32,
    status: JobStatus,
    output: Option<Vec<u8>>,
}

#[derive(Debug, Default)]
struct BookState {
    jobs: HashMap<u64, JobRecord>,
    queue: VecDeque<u64>,
}

/// A job the runtime thread has to start.
#[derive(Debug)]
pub struct QueuedJob {
    /// The request id `start` returned.
    pub request: RequestId,
    /// The plugin that started it.
    pub plugin: String,
    /// The instance of the plugin that started it; the job's events belong to it.
    pub instance: u64,
    /// The job kind.
    pub kind: String,
    /// The input bytes.
    pub input: Vec<u8>,
    /// The deadline of the run.
    pub deadline_ms: u64,
    /// The memory cap of the run.
    pub memory_mib: u64,
    /// Set when the plugin cancels the job.
    pub cancel: Arc<AtomicBool>,
}

/// The jobs of one runtime. Cloning shares the book.
#[derive(Debug, Clone, Default)]
pub struct JobBook {
    shared: Arc<(Mutex<BookState>, Condvar)>,
}

impl JobBook {
    /// An empty book.
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> MutexGuard<'_, BookState> {
        // Every writer leaves the maps consistent between statements.
        self.shared
            .0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Takes the jobs waiting to start, oldest first, and marks them running.
    pub fn take_queued(&self) -> Vec<QueuedJob> {
        let mut state = self.state();
        let ids: Vec<u64> = state.queue.drain(..).collect();
        let mut started = Vec::new();
        for id in ids {
            let Some(record) = state.jobs.get_mut(&id) else {
                continue;
            };
            if record.status != JobStatus::Queued {
                continue;
            }
            record.status = JobStatus::Running;
            started.push(QueuedJob {
                request: RequestId(id),
                plugin: record.plugin.clone(),
                instance: record.instance,
                kind: record.kind.clone(),
                input: std::mem::take(&mut record.input),
                deadline_ms: record.deadline_ms,
                memory_mib: record.memory_mib,
                cancel: Arc::clone(&record.cancel),
            });
        }
        started
    }

    /// Ends a job with its result and queues `request-completed` for the plugin. A job that was
    /// cancelled ends as `cancelled` whatever it returned.
    pub fn finish(
        &self,
        request: RequestId,
        result: Result<Vec<u8>, PluginError>,
        sink: &EventSink,
    ) {
        let outcome = {
            let mut state = self.state();
            let Some(record) = state.jobs.get_mut(&request.0) else {
                return;
            };
            if matches!(record.status, JobStatus::Finished(_)) {
                return;
            }
            let result = if record.cancel.load(Ordering::Acquire) {
                Err(error(ErrorCode::Cancelled, "the job was cancelled"))
            } else {
                match result {
                    Ok(output) if output.len() > JOB_IO_LIMIT_BYTES => Err(error(
                        ErrorCode::QuotaExceeded,
                        "job output exceeds the job_io_mib limit",
                    )),
                    other => other,
                }
            };
            match result {
                Ok(output) => {
                    record.output = Some(output);
                    record.status = JobStatus::Finished(Ok(()));
                    Outcome::Ok
                }
                Err(failure) => {
                    record.status = JobStatus::Finished(Err(failure.clone()));
                    Outcome::Failed(failure)
                }
            }
        };
        self.shared.1.notify_all();
        sink.send(completion_event(request, outcome));
    }

    /// Drops every job of a plugin that stopped and tells their workers to stop.
    pub fn forget_plugin(&self, plugin: &str) {
        let mut state = self.state();
        state.jobs.retain(|_, record| {
            if record.plugin == plugin {
                record.cancel.store(true, Ordering::Release);
                false
            } else {
                true
            }
        });
        let jobs = &state.jobs;
        let live: HashSet<u64> = jobs.keys().copied().collect();
        state.queue.retain(|id| live.contains(id));
        drop(state);
        self.shared.1.notify_all();
    }

    /// How many jobs the book holds (queued, running and finished but not yet collected).
    pub fn len(&self) -> usize {
        self.state().jobs.len()
    }

    /// Whether the book holds no job.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The last reported progress of a job, 0 to 100.
    pub fn progress_of(&self, request: RequestId) -> Option<f32> {
        self.state()
            .jobs
            .get(&request.0)
            .map(|record| record.progress)
    }
}

/// Host side of the `jobs` interface of one plugin instance (or of one job instance).
#[derive(Debug)]
pub struct JobsImport {
    env: ImportEnv,
    book: JobBook,
    declared: Vec<JobDeclaration>,
    /// The job this import runs inside, for `report-progress` and `is-cancelled`.
    current: Option<RequestId>,
    /// Requests the plugin asked to cancel, known or not.
    cancelled: HashSet<u64>,
}

impl Default for JobsImport {
    /// A standalone import with a private book and no declared kind.
    fn default() -> Self {
        Self::new(ImportEnv::unrestricted(), JobBook::new(), Vec::new())
    }
}

impl JobsImport {
    /// The import of a plugin instance: it may start the `declared` kinds.
    pub fn new(env: ImportEnv, book: JobBook, declared: Vec<JobDeclaration>) -> Self {
        Self {
            env,
            book,
            declared,
            current: None,
            cancelled: HashSet::new(),
        }
    }

    /// The import of the job instance that runs `job`: it reports progress and sees the cancel
    /// flag of that job.
    pub fn for_job(env: ImportEnv, book: JobBook, job: RequestId) -> Self {
        Self {
            env,
            book,
            declared: Vec::new(),
            current: Some(job),
            cancelled: HashSet::new(),
        }
    }

    /// The book this import queues its jobs in.
    pub fn book(&self) -> &JobBook {
        &self.book
    }
}

impl HostJobs for JobsImport {
    fn start(&mut self, kind: String, input: Vec<u8>) -> Result<RequestId, PluginError> {
        if kind.is_empty() {
            return Err(error(ErrorCode::InvalidArgument, "job kind is empty"));
        }
        if input.len() > JOB_IO_LIMIT_BYTES {
            return Err(error(
                ErrorCode::InvalidArgument,
                "job input exceeds the job_io_mib limit",
            ));
        }
        let Some(declaration) = self.declared.iter().find(|declared| declared.kind == kind) else {
            return Err(error(
                ErrorCode::InvalidArgument,
                "job kind is not declared in contributes.jobs",
            ));
        };
        let plugin = self.env.plugin().to_owned();
        let mut state = self.book.state();
        let unfinished = state
            .jobs
            .values()
            .filter(|record| {
                record.plugin == plugin && !matches!(record.status, JobStatus::Finished(_))
            })
            .count();
        if unfinished >= JOBS_PER_PLUGIN {
            return Err(error(
                ErrorCode::Busy,
                "8 jobs of this plugin are already pending",
            ));
        }
        let request = self.env.next_request();
        state.jobs.insert(
            request.0,
            JobRecord {
                plugin,
                instance: self.env.instance(),
                kind,
                input,
                deadline_ms: declaration.deadline_ms,
                memory_mib: declaration.memory_mib,
                cancel: Arc::new(AtomicBool::new(false)),
                progress: 0.0,
                status: JobStatus::Queued,
                output: None,
            },
        );
        state.queue.push_back(request.0);
        Ok(request)
    }

    fn cancel(&mut self, job: RequestId) {
        let plugin = self.env.plugin().to_owned();
        let finished_now = {
            let mut state = self.book.state();
            // The job of another plugin is none of this plugin's business; an id nobody holds is
            // remembered so a later wait on it reports `cancelled`.
            if state
                .jobs
                .get(&job.0)
                .is_none_or(|record| record.plugin == plugin)
            {
                self.cancelled.insert(job.0);
            }
            match state.jobs.get_mut(&job.0) {
                Some(record) if record.plugin == plugin => {
                    record.cancel.store(true, Ordering::Release);
                    if record.status == JobStatus::Queued {
                        let failure = error(ErrorCode::Cancelled, "the job was cancelled");
                        record.status = JobStatus::Finished(Err(failure.clone()));
                        Some(failure)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        };
        if let Some(failure) = finished_now {
            self.book.shared.1.notify_all();
            self.env.complete(job, Outcome::Failed(failure));
        }
    }

    fn take_output(&mut self, job: RequestId) -> Option<Vec<u8>> {
        let mut state = self.book.state();
        let record = state.jobs.get_mut(&job.0)?;
        if record.plugin != self.env.plugin() || record.cancel.load(Ordering::Acquire) {
            return None;
        }
        record.output.take()
    }

    fn report_progress(&mut self, percent: f32) {
        let Some(job) = self.current else {
            return;
        };
        let clamped = if percent.is_nan() {
            0.0
        } else {
            percent.clamp(0.0, 100.0)
        };
        if let Some(record) = self.book.state().jobs.get_mut(&job.0) {
            record.progress = clamped;
        }
        self.env.emit(Event::JobProgress(JobProgressEvent {
            job,
            percent: clamped,
        }));
    }

    fn is_cancelled(&mut self) -> bool {
        let Some(job) = self.current else {
            return false;
        };
        self.book
            .state()
            .jobs
            .get(&job.0)
            .is_none_or(|record| record.cancel.load(Ordering::Acquire))
    }

    fn wait_request(&mut self, request: RequestId, timeout_millis: u32) -> Result<(), PluginError> {
        if self.cancelled.contains(&request.0) {
            return Err(error(ErrorCode::Cancelled, "request was cancelled"));
        }
        let plugin = self.env.plugin().to_owned();
        let (lock, ready) = &*self.book.shared;
        let state = lock.lock().unwrap_or_else(|poison| poison.into_inner());
        if !state
            .jobs
            .get(&request.0)
            .is_some_and(|record| record.plugin == plugin)
        {
            return Err(error(ErrorCode::TimedOut, "request did not finish in time"));
        }
        let (state, _) = ready
            .wait_timeout_while(
                state,
                Duration::from_millis(u64::from(timeout_millis)),
                |held| {
                    held.jobs
                        .get(&request.0)
                        .is_some_and(|record| !matches!(record.status, JobStatus::Finished(_)))
                },
            )
            .unwrap_or_else(|poison| poison.into_inner());
        match state.jobs.get(&request.0).map(|record| &record.status) {
            Some(JobStatus::Finished(Ok(()))) => Ok(()),
            Some(JobStatus::Finished(Err(failure))) => Err(failure.clone()),
            _ => Err(error(ErrorCode::TimedOut, "request did not finish in time")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::JobsImport;
    use ilium_plugin_api::generated::host::jobs::HostJobs;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::ids::RequestId;

    /// Default `job_io_mib` limit for a job's input and for its output: 16 MiB.
    const DEFAULT_JOB_IO_BYTES: usize = 16 * 1024 * 1024;

    /// A job kind that no `contributes.jobs` entry declares in these unit tests.
    const UNDECLARED_KIND: &str = "undeclared-test-kind";

    /// Asserts that a start call was refused with the expected error code.
    fn assert_start_refused(result: Result<RequestId, PluginError>, expected: ErrorCode) {
        match result {
            Ok(request) => panic!(
                "start must be refused with {expected:?}, but it returned request {}",
                request.0
            ),
            Err(error) => assert_eq!(
                error.code, expected,
                "wrong error code, message: {}",
                error.message
            ),
        }
    }

    /// REQ: API-002
    #[test]
    fn start_with_undeclared_kind_is_refused_as_invalid_argument() {
        let mut jobs = JobsImport::default();
        let result = jobs.start(UNDECLARED_KIND.to_string(), b"input".to_vec());
        assert_start_refused(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-002
    #[test]
    fn start_with_empty_kind_is_refused_as_invalid_argument() {
        let mut jobs = JobsImport::default();
        let result = jobs.start(String::new(), Vec::new());
        assert_start_refused(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-002
    #[test]
    fn start_with_input_over_job_io_limit_is_refused_as_invalid_argument() {
        let mut jobs = JobsImport::default();
        let oversized_input = vec![0u8; DEFAULT_JOB_IO_BYTES + 1];
        let result = jobs.start(UNDECLARED_KIND.to_string(), oversized_input);
        assert_start_refused(result, ErrorCode::InvalidArgument);
    }

    /// REQ: ABI-012
    #[test]
    fn take_output_of_unknown_request_returns_none() {
        let mut jobs = JobsImport::default();
        assert_eq!(jobs.take_output(RequestId(1)), None);
    }

    /// REQ: ABI-012
    #[test]
    fn take_output_of_request_zero_returns_none() {
        let mut jobs = JobsImport::default();
        assert_eq!(jobs.take_output(RequestId(0)), None);
    }

    /// REQ: ABI-012
    #[test]
    fn take_output_of_largest_request_id_returns_none() {
        let mut jobs = JobsImport::default();
        assert_eq!(jobs.take_output(RequestId(u64::MAX)), None);
    }

    /// REQ: API-074
    #[test]
    fn cancel_of_unknown_request_is_a_silent_no_op() {
        let mut jobs = JobsImport::default();
        jobs.cancel(RequestId(u64::MAX));
        assert_eq!(jobs.take_output(RequestId(u64::MAX)), None);
    }

    /// REQ: RQO-006
    #[test]
    fn cancelled_job_has_no_output_to_take() {
        let mut jobs = JobsImport::default();
        jobs.cancel(RequestId(7));
        assert_eq!(jobs.take_output(RequestId(7)), None);
    }

    /// REQ: HST-210
    #[test]
    fn is_cancelled_is_false_when_no_cancel_was_requested() {
        let mut jobs = JobsImport::default();
        assert!(!jobs.is_cancelled());
    }

    /// REQ: WID-023
    #[test]
    fn report_progress_accepts_nan_and_out_of_range_values_without_panicking() {
        let mut jobs = JobsImport::default();
        jobs.report_progress(f32::NAN);
        jobs.report_progress(-5.0);
        jobs.report_progress(0.0);
        jobs.report_progress(100.0);
        jobs.report_progress(150.0);
    }

    /// REQ: HST-499
    #[test]
    fn wait_request_with_zero_timeout_on_unfinished_request_times_out() {
        let mut jobs = JobsImport::default();
        match jobs.wait_request(RequestId(42), 0) {
            Ok(()) => panic!("an unfinished request must not complete within a zero timeout"),
            Err(error) => assert_eq!(
                error.code,
                ErrorCode::TimedOut,
                "message: {}",
                error.message
            ),
        }
    }
}
