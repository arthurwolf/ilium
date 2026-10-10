use super::*;

use std::time::{Duration, Instant};

use ilium_execution::{
    Client, ClientLimits, Execution, ExecutionConfig, Job, JobCost, JobPoll, Lane, LaneConfig,
    QuotaGroup, QuotaLimits, ShutdownMode,
};

use crate::text_prompt::TextPromptState;

const MIB: usize = 1024 * 1024;

fn owned_bank(preview_limits: ClientLimits) -> (Execution, QuotaGroup, Client, Client) {
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 8,
        jobs: 16,
        service_jobs: 0,
        input_bytes: 128 * MIB,
        result_bytes: 64 * MIB,
        worker_threads: 1,
        worker_bytes: 128 * MIB,
    });

    let disabled = LaneConfig {
        threads: 0,
        queue_slots: 0,
        priority: None,
        resident_bytes_per_thread: 0,
    };

    let execution = Execution::start(
        quota.clone(),
        ExecutionConfig {
            cpu: LaneConfig {
                threads: 1,
                queue_slots: 8,
                priority: None,
                resident_bytes_per_thread: MIB,
            },
            io: disabled,
            service: disabled,
        },
    )
    .expect("isolated preview CPU bank");

    let preview_client = execution.client(preview_limits).expect("preview client");

    let blocker_client = execution
        .client(ClientLimits {
            jobs: 4,
            service_jobs: 0,
            input_bytes: 16 * MIB,
            result_bytes: 16 * MIB,
        })
        .expect("blocker client");

    (execution, quota, preview_client, blocker_client)
}

fn state_with(regexp: &str, message: &str, lines: &[&str]) -> TextTriggerDialogState {
    let mut state = TextTriggerDialogState::new(None);

    state.regexp = TextPromptState::new(regexp);

    state.message = TextPromptState::new(message);

    state.sample = ratatui_textarea::TextArea::from(
        lines
            .iter()
            .map(|line| (*line).to_owned())
            .collect::<Vec<_>>(),
    );

    state
}

fn replace_preview_inputs(
    state: &mut TextTriggerDialogState,
    regexp: &str,
    message: &str,
    lines: &[&str],
) {
    state.regexp = TextPromptState::new(regexp);

    state.message = TextPromptState::new(message);

    state.sample = ratatui_textarea::TextArea::from(
        lines
            .iter()
            .map(|line| (*line).to_owned())
            .collect::<Vec<_>>(),
    );

    state.mark_preview_dirty();
}

fn wait_ready(owner: &mut TextTriggerPreview, state: &mut TextTriggerDialogState) {
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        owner.request(state);

        if state.preview_is_current() {
            return;
        }

        if let Some(issue) = state.preview_issue() {
            panic!("preview became unavailable while waiting: {issue:?}");
        }

        assert!(Instant::now() < deadline, "preview did not complete");

        std::thread::sleep(Duration::from_millis(1));
    }
}

fn wait_issue(
    owner: &mut TextTriggerPreview,
    state: &mut TextTriggerDialogState,
    expected: TextTriggerPreviewIssue,
) {
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        owner.request(state);

        if state.preview_issue() == Some(expected) {
            return;
        }

        assert!(Instant::now() < deadline, "preview issue did not arrive");

        std::thread::sleep(Duration::from_millis(1));
    }
}

fn wait_receipt<J: Job>(receipt: &mut ilium_execution::Receipt<J>) {
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        match receipt.try_take() {
            JobPoll::Pending => {
                assert!(Instant::now() < deadline, "receipt did not settle");

                std::thread::sleep(Duration::from_millis(1));
            }

            JobPoll::Ready(outcome) => {
                drop(outcome);
                return;
            }

            JobPoll::Lost | JobPoll::Taken => {
                panic!("test receipt lost")
            }
        }
    }
}

fn join_bank(mut execution: Execution) {
    execution.request_shutdown(ShutdownMode::Drain);

    let report = execution
        .join_until_background(Instant::now() + Duration::from_secs(5))
        .expect("join preview bank");

    assert!(
        report.shutdown_complete,
        "preview bank did not physically drain: {report:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn worker_preserves_exact_preview_semantics_and_regex_work_is_off_ui() {
    let (execution, _quota, preview_client, blocker_client) = owned_bank(limits());

    let mut owner = TextTriggerPreview::new(preview_client);

    let (worker_sender, worker_receiver) = std::sync::mpsc::channel();

    owner.set_worker_probe(worker_sender);

    let ui_thread = std::thread::current().id();

    let mut state = state_with("b+", "reply", &["abba", "xxx"]);

    wait_ready(&mut owner, &mut state);

    let worker_thread = worker_receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("worker thread probe");

    assert_ne!(
        worker_thread, ui_thread,
        "regex/output preparation executed on interactive test thread"
    );

    assert_eq!(state.preview_text(), Some("✓ a[bb]a\n· xxx"));

    replace_preview_inputs(&mut state, "^", "reply", &["alpha", "beta"]);

    wait_ready(&mut owner, &mut state);

    assert_eq!(
        state.preview_text(),
        Some(
            "✓ alpha  ⟪zero-width match⟫\n\
             ✓ beta  ⟪zero-width match⟫\n\
             ⚠ Reply also matches this regexp; echoed input can loop."
        )
    );

    replace_preview_inputs(&mut state, "(", "", &["ignored"]);

    wait_ready(&mut owner, &mut state);

    // Intentionally invalid literal: clippy's invalid_regex lint would reject it, but the test needs the real parser error text.
    #[allow(clippy::invalid_regex)]
    let expected_invalid = format!(
        "Invalid regexp: {}",
        regex::Regex::new("(").expect_err("invalid test regexp"),
    );

    assert_eq!(state.preview_text(), Some(expected_invalid.as_str()));

    replace_preview_inputs(&mut state, "", "anything", &["anything"]);

    wait_ready(&mut owner, &mut state);

    assert_eq!(
        state.preview_text(),
        Some("Enter a regexp to preview matching sample lines.")
    );

    // Worker panic has no heap error payload on the UI and the captured
    // source destructor remains on the CPU callback.
    let (drop_sender, drop_receiver) = std::sync::mpsc::channel();

    owner.set_source_drop_probe(drop_sender);
    owner.panic_next_job();

    replace_preview_inputs(&mut state, "panic", "", &["panic"]);

    wait_issue(
        &mut owner,
        &mut state,
        TextTriggerPreviewIssue::WorkerPanicked,
    );

    let destruction_thread = drop_receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("panic source destruction");

    assert_ne!(
        destruction_thread, ui_thread,
        "panicking worker returned captured-source destruction to UI"
    );

    state.release_preview();
    settle_closed(&mut owner);

    drop(owner);
    drop(blocker_client);

    join_bank(execution);
}

#[tokio::test(flavor = "current_thread")]
async fn newer_revision_cancels_queued_old_original_and_old_result_never_installs() {
    let (execution, _quota, preview_client, blocker_client) = owned_bank(limits());

    let (entered_sender, entered_receiver) = std::sync::mpsc::sync_channel(1);

    let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(0);

    let mut blocker = bounded_submit(
        &blocker_client,
        Lane::Cpu,
        JobCost {
            input_bytes: 1024,
            result_bytes: 1024,
        },
        move |_| {
            entered_sender.send(()).expect("blocker entered");

            release_receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("release blocker");

            Ok::<(), ()>(())
        },
    );

    entered_receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("blocker running");

    let mut owner = TextTriggerPreview::new(preview_client);

    let (drop_sender, drop_receiver) = std::sync::mpsc::channel();

    owner.set_source_drop_probe(drop_sender);

    let ui_thread = std::thread::current().id();

    let mut state = state_with("old", "", &["old"]);

    wait_submitted(&mut owner, &mut state, 1);

    assert_eq!(
        owner.running.len(),
        1,
        "old revision must be accepted behind blocker"
    );

    replace_preview_inputs(&mut state, "new", "", &["new"]);

    wait_submitted(&mut owner, &mut state, 2);

    assert_eq!(state.preview_revision(), 1);

    assert_eq!(
        owner.running.len(),
        2,
        "new revision should coexist only with one retiring old receipt"
    );

    release_sender.send(()).expect("release CPU");

    wait_receipt(&mut blocker);
    wait_ready(&mut owner, &mut state);

    assert_eq!(state.preview_text(), Some("✓ [new]"));

    let first_drop = drop_receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("first source destruction");

    let second_drop = drop_receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("second source destruction");

    assert_ne!(first_drop, ui_thread);
    assert_ne!(second_drop, ui_thread);

    state.release_preview();
    settle_closed(&mut owner);

    drop(owner);
    drop(blocker_client);

    join_bank(execution);
}

#[tokio::test(flavor = "current_thread")]
async fn transient_job_refusal_retries_without_capturing_authored_draft() {
    let narrow_limits = ClientLimits {
        jobs: 1,
        service_jobs: 0,
        input_bytes: JOB_COST.input_bytes,
        result_bytes: JOB_COST.result_bytes,
    };

    let (execution, _quota, preview_client, blocker_client) = owned_bank(narrow_limits);

    // Use the same tenant as preview so one blocker exhausts its one job.
    let occupied_client = preview_client.clone();

    let (entered_sender, entered_receiver) = std::sync::mpsc::sync_channel(1);

    let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(0);

    let mut blocker = bounded_submit(
        &occupied_client,
        Lane::Cpu,
        JobCost {
            input_bytes: 1024,
            result_bytes: 1024,
        },
        move |_| {
            entered_sender.send(()).expect("blocker entered");

            release_receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("release blocker");

            Ok::<(), ()>(())
        },
    );

    entered_receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("blocker running");

    let mut owner = TextTriggerPreview::new(preview_client);

    let (drop_sender, drop_receiver) = std::sync::mpsc::channel();

    owner.set_source_drop_probe(drop_sender);

    let mut state = state_with("authored", "", &["authored"]);

    owner.request(&mut state);

    assert!(
        owner.pending.is_none(),
        "admission refusal must happen before source capture"
    );

    assert!(
        owner.running.is_empty(),
        "refused preview must not become an accepted receipt"
    );

    assert!(
        owner.retry_at.is_some(),
        "transient refusal must schedule one bounded retry"
    );

    assert!(
        drop_receiver.try_recv().is_err(),
        "no CapturedDraft should exist before job admission"
    );

    release_sender.send(()).expect("release preview slot");

    wait_receipt(&mut blocker);
    // A completed receipt continues to retain its job debit until the receipt
    // itself is dropped. Release that original admission before retrying.
    drop(blocker);

    // Avoid sleeping merely to cross the policy deadline in this unit test.
    owner.retry_at = Some(Instant::now());

    wait_ready(&mut owner, &mut state);

    assert_eq!(state.preview_text(), Some("✓ [authored]"));

    state.release_preview();
    settle_closed(&mut owner);

    drop(owner);
    drop(blocker_client);

    join_bank(execution);
}

#[tokio::test(flavor = "current_thread")]
async fn installed_result_keeps_job_and_storage_debit_until_physical_cpu_retirement() {
    let (mut execution, quota, preview_client, blocker_client) = owned_bank(limits());

    let baseline_worker_bytes = quota.snapshot().worker_bytes;

    let mut owner = TextTriggerPreview::new(preview_client);

    let mut state = state_with("owned", "", &["owned"]);

    wait_ready(&mut owner, &mut state);

    assert_eq!(
        owner.client.usage().jobs,
        1,
        "installed result must retain the producing job debit"
    );

    assert!(
        quota.snapshot().worker_bytes >= baseline_worker_bytes + OUTPUT_STORAGE_BYTES,
        "installed result must retain output storage admission"
    );

    let (entered_sender, entered_receiver) = std::sync::mpsc::sync_channel(1);

    let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(0);

    let mut blocker = bounded_submit(
        &blocker_client,
        Lane::Cpu,
        JobCost {
            input_bytes: 1024,
            result_bytes: 1024,
        },
        move |_| {
            entered_sender.send(()).expect("blocker entered");

            release_receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("release blocker");

            Ok::<(), ()>(())
        },
    );

    entered_receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("blocker running");

    let completed_before =
        execution.monitor().health().lanes[Lane::Cpu as usize].retirement_completed;

    state.release_preview();

    let parked = execution.monitor().health();

    assert!(
        parked.lanes[Lane::Cpu as usize].retirement_queued >= 1,
        "final preview destruction must be queued behind occupied CPU"
    );

    assert_eq!(
        parked.lanes[Lane::Cpu as usize].retirement_completed,
        completed_before,
        "preview retirement completed while CPU was blocked"
    );

    assert_eq!(
        owner.client.usage().jobs,
        1,
        "job debit released before physical result retirement"
    );

    // No active receipt remains, so owner shutdown may finish while the
    // Retiring result is still queued. Execution shutdown owns the physical
    // retirement proof.
    settle_closed(&mut owner);

    release_sender.send(()).expect("release CPU");

    wait_receipt(&mut blocker);

    execution.request_shutdown(ShutdownMode::Drain);

    let report = execution
        .join_until_background(Instant::now() + Duration::from_secs(5))
        .expect("join preview bank");

    assert!(report.shutdown_complete);

    assert_eq!(report.health.lanes[Lane::Cpu as usize].retirement_live, 0,);

    assert!(
        report.health.lanes[Lane::Cpu as usize].retirement_completed > completed_before,
        "physical preview retirement was not observed"
    );

    assert_eq!(
        owner.client.usage().jobs,
        0,
        "producing job debit survived physical destruction"
    );

    drop(owner);
    drop(blocker_client);
}
fn bounded_submit<J: Job>(
    client: &Client,
    lane: Lane,
    cost: JobCost,
    mut job: J,
) -> ilium_execution::Receipt<J> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match client.try_submit(lane, cost, job) {
            Ok(receipt) => return receipt,
            Err(rejected) => {
                assert!(
                    matches!(
                        rejected.reason,
                        RejectReason::Busy | RejectReason::QueueFull
                    ),
                    "intrinsic fixture admission: {:?}",
                    rejected.reason
                );
                job = rejected.value;
                assert!(Instant::now() < deadline, "fixture admission deadline");
                std::thread::yield_now();
            }
        }
    }
}
fn wait_submitted(
    owner: &mut TextTriggerPreview,
    state: &mut TextTriggerDialogState,
    count: usize,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while owner.running.len() < count {
        owner.request(state);
        assert!(Instant::now() < deadline, "capture never submitted");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn settle_closed(owner: &mut TextTriggerPreview) {
    owner.close();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !owner.is_settled() {
        owner.collect();
        assert!(
            Instant::now() < deadline,
            "cancelled receipt did not settle"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn sample_limit_with_one_to_three_bytes_before_unicode_refuses_without_livelock() {
    for remaining in 1..=3 {
        let (execution, _quota, client, blocker) = owned_bank(limits());
        let mut owner = TextTriggerPreview::new(client);
        // The separating LF consumes one byte; the next whole scalar cannot
        // fit the global bound even when another capture turn is available.
        let prefix = "a".repeat(MAX_SAMPLE_BYTES - remaining - 1);
        let mut state = state_with("z", "reply", &[&prefix, "🦀"]);
        wait_issue(
            &mut owner,
            &mut state,
            TextTriggerPreviewIssue::CaptureLimit,
        );
        assert_eq!(state.sample.lines()[1], "🦀");
        assert_eq!(state.regexp.buf, "z");
        assert!(owner.pending.is_none());
        assert!(owner.retry_at.is_none());
        settle_closed(&mut owner);
        drop(owner);
        drop(blocker);
        join_bank(execution);
    }
}

#[test]
fn exact_sample_limit_preserves_unicode_and_separating_lf() {
    let (execution, _quota, client, blocker) = owned_bank(limits());
    let mut owner = TextTriggerPreview::new(client);
    let prefix = "a".repeat(MAX_SAMPLE_BYTES - 5);
    let mut state = state_with("🦀", "reply", &[&prefix, "🦀"]);
    wait_ready(&mut owner, &mut state);
    let text = state.preview_text().unwrap();
    assert!(text.starts_with("· a"));
    assert!(text.ends_with("\n✓ [🦀]"));
    assert_eq!(text.len(), MAX_SAMPLE_BYTES + "· ".len() + "✓ []".len());
    state.release_preview();
    settle_closed(&mut owner);
    drop(owner);
    drop(blocker);
    join_bank(execution);
}

#[test]
fn intrinsic_cpu_limit_reports_unavailable_before_copy_instead_of_retrying() {
    let narrow = ClientLimits {
        input_bytes: JOB_COST.input_bytes - 1,
        ..limits()
    };
    let (execution, _quota, client, blocker) = owned_bank(narrow);
    let mut owner = TextTriggerPreview::new(client);
    let mut state = state_with("unchanged", "reply", &["original"]);
    wait_issue(
        &mut owner,
        &mut state,
        TextTriggerPreviewIssue::Admission(RejectReason::InvalidCost),
    );
    assert!(owner.pending.is_none());
    assert!(owner.running.is_empty());
    assert!(owner.retry_delay(Instant::now()).is_none());
    assert_eq!(state.sample.lines()[0], "original");
    drop(owner);
    drop(blocker);
    join_bank(execution);
}

#[test]
fn close_does_not_wait_for_blocked_cpu_and_keeps_real_queued_receipt() {
    let (execution, _quota, client, blocker_client) = owned_bank(limits());
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let mut blocker = bounded_submit(
        &blocker_client,
        Lane::Cpu,
        JobCost {
            input_bytes: 1024,
            result_bytes: 1024,
        },
        move |_| {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok::<(), ()>(())
        },
    );
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut owner = TextTriggerPreview::new(client);
    let mut state = state_with("held", "", &["held"]);
    wait_submitted(&mut owner, &mut state, 1);
    owner.close();
    assert!(
        !owner.is_settled(),
        "queued original must remain in custody"
    );
    assert_eq!(owner.running.len(), 1);
    assert!(owner.retry_delay(Instant::now()).is_none());
    release_tx.send(()).unwrap();
    wait_receipt(&mut blocker);
    settle_closed(&mut owner);
    assert!(
        state.preview_text().is_none(),
        "closing must not install cancelled output"
    );
    assert_eq!(state.sample.lines()[0], "held");
    drop(owner);
    drop(blocker_client);
    join_bank(execution);
}
