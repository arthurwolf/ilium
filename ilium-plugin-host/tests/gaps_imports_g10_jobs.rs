//! G10 `jobs`: a plugin starts only the kinds its manifest declares in `contributes.jobs`, the
//! runtime takes the queued jobs and finishes them, progress and completion reach the plugin as
//! events, and one plugin can neither cancel nor read the job of another (api-jobs.md). The
//! book is the real shared queue the runtime thread drains; the finishing side here plays the
//! part of the runtime's worker.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use ilium_plugin_api::generated::host::jobs::HostJobs;
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::ids::RequestId;
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
use ilium_plugin_host::imports::env::{ImportEnv, RequestIds};
use ilium_plugin_host::imports::jobs::{JobBook, JobDeclaration, JobsImport};
use ilium_plugin_host::runtime::pending_events::PendingEvents;

fn declared() -> Vec<JobDeclaration> {
    vec![JobDeclaration {
        kind: "index".to_owned(),
        deadline_ms: 5_000,
        memory_mib: 96,
    }]
}

/// One runtime's shared parts: the job book, the event queue and the request id allocator.
struct Rig {
    book: JobBook,
    events: PendingEvents,
    ids: RequestIds,
}

impl Rig {
    fn new() -> Self {
        Self {
            book: JobBook::new(),
            events: PendingEvents::new(),
            ids: RequestIds::new(),
        }
    }

    fn env(&self, plugin: &str) -> ImportEnv {
        ImportEnv::unrestricted()
            .with_identity(plugin, 1)
            .with_events(self.events.clone())
            .with_ids(self.ids.clone())
    }

    /// The import of one plugin instance.
    fn instance(&self, plugin: &str) -> JobsImport {
        JobsImport::new(self.env(plugin), self.book.clone(), declared())
    }

    /// The import of the job instance that runs `request` for `plugin`.
    fn runner(&self, plugin: &str, request: RequestId) -> JobsImport {
        JobsImport::for_job(self.env(plugin), self.book.clone(), request)
    }
}

fn completed(events: &PendingEvents, request: RequestId) -> Option<Outcome> {
    events
        .drain()
        .into_iter()
        .find_map(|queued| match queued.event {
            Event::RequestCompleted(result) if result.request == RequestRef::Call(request) => {
                Some(result.outcome)
            }
            _ => None,
        })
}

fn failure(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_owned(),
        retry_after_ms: None,
    }
}

/// REQ: API-002
#[test]
fn only_declared_kinds_start_and_the_job_waits_in_the_book_with_its_limits() {
    let rig = Rig::new();
    let book = rig.book.clone();
    let mut jobs = rig.instance("test.jobs");
    let refused = jobs.start("unknown".to_owned(), Vec::new());
    assert_eq!(
        refused.expect_err("undeclared").code,
        ErrorCode::InvalidArgument
    );
    let request = jobs
        .start("index".to_owned(), b"input".to_vec())
        .expect("declared kind");
    let queued = book.take_queued();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].request, request);
    assert_eq!(queued[0].plugin, "test.jobs");
    assert_eq!(queued[0].kind, "index");
    assert_eq!(queued[0].input, b"input");
    assert_eq!(queued[0].deadline_ms, 5_000);
    assert_eq!(queued[0].memory_mib, 96);
    assert!(book.take_queued().is_empty(), "a job starts once");
}

/// REQ: API-002
#[test]
fn a_manifest_entry_without_limits_gets_the_defaults() {
    let bare = JobDeclaration::from_manifest(&serde_json::json!({"kind": "work"}))
        .expect("a kind is enough");
    assert_eq!(bare.kind, "work");
    assert!(bare.deadline_ms > 0 && bare.memory_mib > 0);
    let tuned = JobDeclaration::from_manifest(
        &serde_json::json!({"kind": "work", "deadline_ms": 2500, "memory_mib": 80}),
    )
    .expect("declared");
    assert_eq!((tuned.deadline_ms, tuned.memory_mib), (2_500, 80));
    assert!(JobDeclaration::from_manifest(&serde_json::json!({"title": "no kind"})).is_none());
}

/// REQ: API-002
#[test]
fn a_plugin_holds_at_most_eight_unfinished_jobs() {
    let rig = Rig::new();
    let mut jobs = rig.instance("test.jobs");
    for _ in 0..8 {
        jobs.start("index".to_owned(), Vec::new())
            .expect("within the quota");
    }
    let ninth = jobs.start("index".to_owned(), Vec::new());
    assert_eq!(ninth.expect_err("ninth job").code, ErrorCode::Busy);
    let mut other = rig.instance("test.other");
    other
        .start("index".to_owned(), Vec::new())
        .expect("another plugin has its own quota");
}

/// REQ: ABI-012
#[test]
fn a_finished_job_completes_through_an_event_and_hands_its_output_over_once() {
    let rig = Rig::new();
    let (book, events) = (rig.book.clone(), rig.events.clone());
    let mut jobs = rig.instance("test.jobs");
    let request = jobs.start("index".to_owned(), Vec::new()).expect("starts");
    let queued = book.take_queued();
    assert_eq!(queued.len(), 1);
    book.finish(
        request,
        Ok(b"result".to_vec()),
        &events.sink("test.jobs", 1),
    );
    assert_eq!(completed(&events, request), Some(Outcome::Ok));
    jobs.wait_request(request, 100).expect("finished");
    assert_eq!(jobs.take_output(request), Some(b"result".to_vec()));
    assert_eq!(jobs.take_output(request), None, "output is taken once");
}

/// REQ: ABI-012
#[test]
fn a_deadline_or_memory_failure_reaches_the_plugin_as_the_failure_that_ended_the_job() {
    let rig = Rig::new();
    let (book, events) = (rig.book.clone(), rig.events.clone());
    let mut jobs = rig.instance("test.jobs");
    let slow = jobs.start("index".to_owned(), Vec::new()).expect("starts");
    let heavy = jobs.start("index".to_owned(), Vec::new()).expect("starts");
    book.take_queued();
    let sink = events.sink("test.jobs", 1);
    book.finish(slow, Err(failure(ErrorCode::TimedOut, "deadline")), &sink);
    book.finish(
        heavy,
        Err(failure(ErrorCode::QuotaExceeded, "memory")),
        &sink,
    );
    let queued: Vec<Event> = events
        .drain()
        .into_iter()
        .map(|entry| entry.event)
        .collect();
    let outcome_of = |request: RequestId| {
        queued.iter().find_map(|event| match event {
            Event::RequestCompleted(result) if result.request == RequestRef::Call(request) => {
                Some(result.outcome.clone())
            }
            _ => None,
        })
    };
    assert!(
        matches!(outcome_of(slow), Some(Outcome::Failed(error)) if error.code == ErrorCode::TimedOut)
    );
    assert!(
        matches!(outcome_of(heavy), Some(Outcome::Failed(error)) if error.code == ErrorCode::QuotaExceeded)
    );
    assert_eq!(
        jobs.wait_request(slow, 10).expect_err("failed job").code,
        ErrorCode::TimedOut
    );
    assert_eq!(jobs.take_output(slow), None, "a failed job has no output");
}

/// REQ: RQO-006
#[test]
fn cancelling_a_queued_job_completes_it_as_cancelled_and_it_never_starts() {
    let rig = Rig::new();
    let (book, events) = (rig.book.clone(), rig.events.clone());
    let mut jobs = rig.instance("test.jobs");
    let request = jobs.start("index".to_owned(), Vec::new()).expect("starts");
    jobs.cancel(request);
    match completed(&events, request) {
        Some(Outcome::Failed(error)) => assert_eq!(error.code, ErrorCode::Cancelled),
        other => panic!("expected a cancelled failure, got {other:?}"),
    }
    assert!(
        book.take_queued().is_empty(),
        "a cancelled job is not started"
    );
    assert_eq!(
        jobs.wait_request(request, 10).expect_err("cancelled").code,
        ErrorCode::Cancelled
    );
}

/// REQ: RQO-006
#[test]
fn a_running_job_sees_its_cancel_flag_and_ends_cancelled_whatever_it_returns() {
    let rig = Rig::new();
    let (book, events) = (rig.book.clone(), rig.events.clone());
    let mut owner = rig.instance("test.jobs");
    let request = owner.start("index".to_owned(), Vec::new()).expect("starts");
    let queued = book.take_queued();
    let mut runner = rig.runner("test.jobs", request);
    assert!(!runner.is_cancelled());
    owner.cancel(request);
    assert!(
        runner.is_cancelled(),
        "the job instance sees the plugin's cancel"
    );
    assert!(
        queued[0].cancel.load(Ordering::Acquire),
        "the worker's own flag is set too"
    );
    book.finish(request, Ok(b"late".to_vec()), &events.sink("test.jobs", 1));
    assert_eq!(
        owner.take_output(request),
        None,
        "a cancelled job yields no output"
    );
    match completed(&events, request) {
        Some(Outcome::Failed(error)) => assert_eq!(error.code, ErrorCode::Cancelled),
        other => panic!("expected a cancelled failure, got {other:?}"),
    }
}

/// REQ: WID-023
#[test]
fn progress_arrives_in_order_clamped_to_zero_to_one_hundred() {
    let rig = Rig::new();
    let (book, events) = (rig.book.clone(), rig.events.clone());
    let mut owner = rig.instance("test.jobs");
    let request = owner.start("index".to_owned(), Vec::new()).expect("starts");
    book.take_queued();
    let mut runner = rig.runner("test.jobs", request);
    for value in [10.0_f32, 55.5, 150.0, -3.0, f32::NAN] {
        runner.report_progress(value);
    }
    let reported: Vec<f32> = events
        .drain()
        .into_iter()
        .filter_map(|queued| match queued.event {
            Event::JobProgress(progress) => {
                assert_eq!(progress.job, request);
                Some(progress.percent)
            }
            _ => None,
        })
        .collect();
    assert_eq!(reported, vec![10.0, 55.5, 100.0, 0.0, 0.0]);
    assert_eq!(book.progress_of(request), Some(0.0));
}

/// REQ: API-074
#[test]
fn another_plugin_can_neither_cancel_nor_read_a_job_it_did_not_start() {
    let rig = Rig::new();
    let (book, events) = (rig.book.clone(), rig.events.clone());
    let mut owner = rig.instance("test.owner");
    let mut stranger = rig.instance("test.stranger");
    let request = owner.start("index".to_owned(), Vec::new()).expect("starts");
    let queued = book.take_queued();
    stranger.cancel(request);
    assert!(
        !queued[0].cancel.load(Ordering::Acquire),
        "a stranger's cancel is ignored"
    );
    book.finish(
        request,
        Ok(b"secret".to_vec()),
        &events.sink("test.owner", 1),
    );
    assert_eq!(stranger.take_output(request), None);
    assert_eq!(
        stranger
            .wait_request(request, 10)
            .expect_err("not the owner")
            .code,
        ErrorCode::TimedOut
    );
    assert_eq!(owner.take_output(request), Some(b"secret".to_vec()));
}

/// REQ: HST-499
#[test]
fn waiting_returns_as_soon_as_the_job_finishes_on_another_thread() {
    let rig = Rig::new();
    let (book, events) = (rig.book.clone(), rig.events.clone());
    let mut jobs = rig.instance("test.jobs");
    let request = jobs.start("index".to_owned(), Vec::new()).expect("starts");
    book.take_queued();
    let worker_book = book.clone();
    let sink = events.sink("test.jobs", 1);
    let worker = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        worker_book.finish(request, Ok(Vec::new()), &sink);
    });
    let started = Instant::now();
    jobs.wait_request(request, 10_000)
        .expect("finished while waiting");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the wait ended on the finish"
    );
    worker.join().expect("worker thread");
    let early = jobs.start("index".to_owned(), Vec::new()).expect("starts");
    assert_eq!(
        jobs.wait_request(early, 0).expect_err("unfinished").code,
        ErrorCode::TimedOut
    );
}

/// REQ: API-074
#[test]
fn a_stopped_plugin_loses_its_jobs_and_its_workers_are_told_to_stop() {
    let rig = Rig::new();
    let (book, events) = (rig.book.clone(), rig.events.clone());
    let mut jobs = rig.instance("test.jobs");
    let mut survivor = rig.instance("test.survivor");
    let running = jobs.start("index".to_owned(), Vec::new()).expect("starts");
    jobs.start("index".to_owned(), Vec::new()).expect("starts");
    let kept = survivor
        .start("index".to_owned(), Vec::new())
        .expect("starts");
    let started: Vec<_> = book.take_queued();
    assert_eq!(started.len(), 3);
    book.forget_plugin("test.jobs");
    let flag_of = |request: RequestId| {
        started
            .iter()
            .find(|job| job.request == request)
            .map(|job| job.cancel.load(Ordering::Acquire))
    };
    assert_eq!(
        flag_of(running),
        Some(true),
        "the worker of a stopped plugin is cancelled"
    );
    assert_eq!(flag_of(kept), Some(false), "other plugins keep their jobs");
    assert_eq!(book.len(), 1);
    book.finish(running, Ok(Vec::new()), &events.sink("test.jobs", 1));
    assert!(events.is_empty(), "a forgotten job completes nothing");
}
