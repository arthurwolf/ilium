//! Unit tests for public ilium-core items that previously had no direct coverage.

use std::collections::HashMap;
use std::error::Error;
use std::mem::size_of;
use std::path::PathBuf;

use ilium_core::allocation::AllocationSize;
use ilium_core::animation_recommendation::{
    valid_animation_identifier, AnimationParameter, AnimationRecommendation, AnimationValue,
    ResourcePolicy, ANIMATION_RECOMMENDATION_VERSION, MAX_ANIMATION_PARAMETERS,
};
use ilium_core::progress_timing::{
    EtaEstimate, ProgressCheck, ProgressCheckOutcome, ProgressSample, ProgressTiming,
    STALL_MINIMUM_MILLIS, STALL_PROBE_COUNT,
};
use ilium_core::{
    task_progress_bucket, validate_branch_name, AgentAvailability, AgentClass, AgentExitOutcome,
    AgentProvider, BranchNameError, BuiltinAgentProvider, ContainerKind, ContainerNode,
    NodeActivityUpdate, NodeId, NodeKind, PaneContentKind, PaneProgress, PaneTitleSource,
    ProgressAttention, ProgressMonitorHealth, ProgressTaskReport, ProgressTaskStatus,
    ProgressValidationError, PromptQueueDelivery, QueuedPrompt, ScheduledPaneInput,
    SessionIdentityTransitionRule, SplitOrientation, StructureSource, TaskSignal, Tree, TreeError,
    MAXIMUM_PROGRESS_ERROR_BYTES, MAXIMUM_PROGRESS_ETA_SECONDS, MAXIMUM_PROGRESS_JOB_ID_BYTES,
    MAXIMUM_PROGRESS_MESSAGE_BYTES, MAXIMUM_PROGRESS_MONITOR_ERROR_BYTES, ROOT_ID,
    TASK_PROGRESS_BUCKETS,
};

type TestResult = Result<(), Box<dyn Error>>;

const SESSION_ID: &str = "123e4567-e89b-12d3-a456-426614174000";

fn report(
    job_id: &str,
    status: ProgressTaskStatus,
    percent: f32,
    error: Option<&str>,
) -> Result<ProgressTaskReport, ProgressValidationError> {
    ProgressTaskReport::new(
        job_id.to_string(),
        status,
        percent,
        "Compiling the crate".to_string(),
        String::new(),
        error.map(str::to_string),
    )
}

fn report_with_message(message: String) -> Result<ProgressTaskReport, ProgressValidationError> {
    ProgressTaskReport::new(
        "job-1".to_string(),
        ProgressTaskStatus::Running,
        10.0,
        message,
        String::new(),
        None,
    )
}

fn pane_progress(
    status: ProgressTaskStatus,
    percent: f32,
    error: Option<&str>,
    health: ProgressMonitorHealth,
) -> Result<PaneProgress, Box<dyn Error>> {
    let mut progress = PaneProgress::new(1, report("job-1", status, percent, error)?, 1_000)?;
    progress.monitor_health = health;
    Ok(progress)
}

fn task_signal(
    status: ProgressTaskStatus,
    percent: f32,
    error: Option<&str>,
    health: ProgressMonitorHealth,
    acknowledged: bool,
) -> Result<TaskSignal, Box<dyn Error>> {
    let mut progress = pane_progress(status, percent, error, health)?;
    if acknowledged {
        progress.attention = ProgressAttention::Acknowledged;
    }
    Ok(TaskSignal::from_progress(&progress))
}

fn degraded() -> ProgressMonitorHealth {
    ProgressMonitorHealth::Degraded {
        consecutive_failures: 2,
        last_error: "probe timed out".to_string(),
    }
}

fn failed() -> ProgressMonitorHealth {
    ProgressMonitorHealth::Failed {
        consecutive_failures: 3,
        last_error: "probe timed out".to_string(),
    }
}

fn timing_with_probe_interval(interval_millis: u64) -> ProgressTiming {
    let mut timing = ProgressTiming::started(0, 0.0);
    timing.probe_interval_millis = Some(interval_millis);
    timing
}

fn pane_in_group() -> Result<(Tree, NodeId), TreeError> {
    let mut tree = Tree::new();
    let group = tree.add_group(ROOT_ID, "Work")?;
    let pane = tree.add_pane(group, "Shell", PaneContentKind::Terminal)?;
    Ok((tree, pane))
}

fn prompt_queue_len(tree: &Tree, pane: NodeId) -> Option<usize> {
    match &tree.get(pane)?.kind {
        NodeKind::Pane { prompt_queue, .. } => Some(prompt_queue.len()),
        _ => None,
    }
}

fn queued_prompt(text: &str) -> QueuedPrompt {
    QueuedPrompt {
        text: text.to_string(),
        delivery: PromptQueueDelivery::Once,
        attempted_delivery: false,
    }
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn recommendation(parameter_count: usize) -> AnimationRecommendation {
    AnimationRecommendation {
        version: ANIMATION_RECOMMENDATION_VERSION,
        kind: "wave".to_string(),
        resources: ResourcePolicy::Catalog,
        parameters: (0..parameter_count)
            .map(|index| AnimationParameter {
                id: format!("p{index}"),
                value: AnimationValue::Bool(true),
            })
            .collect(),
    }
}

// AgentAvailability::explanation

#[test]
fn availability_explanation_is_non_empty_for_every_variant() {
    let variants = [
        AgentAvailability::Unverified,
        AgentAvailability::ShellForeground,
        AgentAvailability::Exited(AgentExitOutcome::ExitCode(0)),
        AgentAvailability::Exited(AgentExitOutcome::ExitCode(1)),
        AgentAvailability::Exited(AgentExitOutcome::Signal),
        AgentAvailability::Exited(AgentExitOutcome::Unknown),
    ];
    for variant in variants {
        assert!(
            !variant.explanation().trim().is_empty(),
            "{variant:?} has an empty explanation"
        );
    }
}

#[test]
fn availability_explanation_states_the_meaning_of_each_variant() {
    assert!(AgentAvailability::Unverified
        .explanation()
        .contains("did not establish"));
    assert!(AgentAvailability::Unverified
        .explanation()
        .contains("no crash or clean exit is inferred"));
    assert!(AgentAvailability::ShellForeground
        .explanation()
        .contains("shell owns the terminal"));
    assert!(AgentAvailability::Exited(AgentExitOutcome::ExitCode(0))
        .explanation()
        .contains("normal exit"));
    assert!(AgentAvailability::Exited(AgentExitOutcome::ExitCode(2))
        .explanation()
        .contains("nonzero status"));
    assert!(AgentAvailability::Exited(AgentExitOutcome::Signal)
        .explanation()
        .contains("signal termination"));
    assert!(AgentAvailability::Exited(AgentExitOutcome::Unknown)
        .explanation()
        .contains("disappeared"));
}

// AllocationSize::retained_bytes and heap_bytes

#[test]
fn retained_bytes_equals_heap_bytes_plus_inline_size_for_string() {
    let text = String::with_capacity(40);
    assert_eq!(text.heap_bytes(), text.capacity());
    assert_eq!(text.retained_bytes(), size_of::<String>() + text.capacity());
}

#[test]
fn empty_string_and_vec_report_zero_heap_bytes() {
    assert_eq!(String::new().heap_bytes(), 0);
    assert_eq!(Vec::<u32>::new().heap_bytes(), 0);
    assert_eq!(Vec::<u32>::new().retained_bytes(), size_of::<Vec<u32>>());
}

#[test]
fn tree_holding_only_its_root_reports_heap_for_the_root_node() {
    // A tree is never empty: the root node is always stored in its map.
    let tree = Tree::new();
    assert!(tree.heap_bytes() > 0, "root node storage must be counted");
    assert!(tree.retained_bytes() > size_of::<Tree>());
}

#[test]
fn nested_option_box_and_vec_count_their_children() {
    let inner = String::with_capacity(32);
    let inner_capacity = inner.capacity();
    let items: Vec<Option<Box<String>>> = vec![Some(Box::new(inner)), None];
    let expected =
        items.capacity() * size_of::<Option<Box<String>>>() + size_of::<String>() + inner_capacity;
    assert_eq!(items.heap_bytes(), expected);
}

#[test]
fn nested_result_reports_heap_of_the_active_variant_only() {
    let ok_value = String::with_capacity(16);
    let ok_capacity = ok_value.capacity();
    let ok: Result<String, Vec<u8>> = Ok(ok_value);
    assert_eq!(ok.heap_bytes(), ok_capacity);

    let err_value: Vec<u8> = Vec::with_capacity(24);
    let err_capacity = err_value.capacity();
    let err: Result<String, Vec<u8>> = Err(err_value);
    assert_eq!(err.heap_bytes(), err_capacity);
}

#[test]
fn nested_hash_map_counts_bucket_storage_and_children() {
    let empty: HashMap<String, Vec<u8>> = HashMap::new();
    assert_eq!(empty.heap_bytes(), 0);

    let key = String::with_capacity(20);
    let key_capacity = key.capacity();
    let value: Vec<u8> = Vec::with_capacity(5);
    let value_capacity = value.capacity();
    let mut map: HashMap<String, Vec<u8>> = HashMap::new();
    map.insert(key, value);

    let buckets = map
        .capacity()
        .saturating_mul(2)
        .checked_next_power_of_two()
        .unwrap_or(usize::MAX);
    let expected =
        buckets * (size_of::<(String, Vec<u8>)>() + 1) + 64 + key_capacity + value_capacity;
    assert_eq!(map.heap_bytes(), expected);
}

// animation_recommendation

#[test]
fn exactly_the_maximum_parameter_count_is_accepted() -> TestResult {
    recommendation(MAX_ANIMATION_PARAMETERS).validate_shape()?;
    assert_eq!(MAX_ANIMATION_PARAMETERS, 128);
    Ok(())
}

#[test]
fn one_parameter_over_the_maximum_is_rejected() {
    let over = recommendation(MAX_ANIMATION_PARAMETERS + 1);
    assert_eq!(
        over.validate_shape(),
        Err("Too many animation parameters".to_string())
    );
}

#[test]
fn identifier_rejects_the_empty_string() {
    assert!(!valid_animation_identifier(""));
}

#[test]
fn identifier_accepts_maximum_length_and_rejects_one_over() {
    assert!(valid_animation_identifier(&"a".repeat(80)));
    assert!(!valid_animation_identifier(&"a".repeat(81)));
}

#[test]
fn identifier_rejects_invalid_characters() {
    assert!(!valid_animation_identifier("Wave"));
    assert!(!valid_animation_identifier("wave-sweep"));
    assert!(!valid_animation_identifier("wave sweep"));
    assert!(!valid_animation_identifier("wave.sweep"));
    assert!(valid_animation_identifier("wave_sweep_2"));
}

#[test]
fn identifier_rejects_leading_digit_and_leading_separator() {
    assert!(!valid_animation_identifier("1wave"));
    assert!(!valid_animation_identifier("_wave"));
    assert!(valid_animation_identifier("w1"));
    assert!(valid_animation_identifier("a"));
}

#[test]
fn identifier_rejects_non_ascii_input() {
    assert!(!valid_animation_identifier("é"));
    assert!(!valid_animation_identifier("aé"));
}

// progress text and ETA limits (lib.rs)

#[test]
fn job_id_accepts_exactly_the_byte_limit_and_rejects_one_over() -> TestResult {
    let at_limit = "j".repeat(MAXIMUM_PROGRESS_JOB_ID_BYTES);
    report(&at_limit, ProgressTaskStatus::Running, 10.0, None)?;

    let over_limit = "j".repeat(MAXIMUM_PROGRESS_JOB_ID_BYTES + 1);
    assert!(matches!(
        report(&over_limit, ProgressTaskStatus::Running, 10.0, None),
        Err(ProgressValidationError::TextTooLong {
            field: "job_id",
            maximum_bytes: 256
        })
    ));
    Ok(())
}

#[test]
fn job_id_byte_limit_counts_multibyte_characters_as_bytes() -> TestResult {
    // Each "é" is two UTF-8 bytes: 128 of them fill the 256-byte limit exactly.
    let at_limit = "é".repeat(MAXIMUM_PROGRESS_JOB_ID_BYTES / 2);
    report(&at_limit, ProgressTaskStatus::Running, 10.0, None)?;

    let over_limit = format!("{}a", "é".repeat(MAXIMUM_PROGRESS_JOB_ID_BYTES / 2));
    assert!(report(&over_limit, ProgressTaskStatus::Running, 10.0, None).is_err());
    Ok(())
}

#[test]
fn message_accepts_exactly_the_byte_limit_and_rejects_one_over() -> TestResult {
    report_with_message("m".repeat(MAXIMUM_PROGRESS_MESSAGE_BYTES))?;
    assert!(matches!(
        report_with_message("m".repeat(MAXIMUM_PROGRESS_MESSAGE_BYTES + 1)),
        Err(ProgressValidationError::TextTooLong {
            field: "message",
            maximum_bytes: 2048
        })
    ));
    Ok(())
}

#[test]
fn message_byte_limit_counts_multibyte_text_at_the_boundary() -> TestResult {
    // 1024 "é" characters are exactly 2048 bytes.
    report_with_message("é".repeat(1024))?;
    assert!(report_with_message(format!("{}a", "é".repeat(1024))).is_err());
    Ok(())
}

#[test]
fn empty_message_is_accepted() -> TestResult {
    report_with_message(String::new())?;
    Ok(())
}

#[test]
fn error_detail_accepts_exactly_the_byte_limit_and_rejects_one_over() -> TestResult {
    let at_limit = "e".repeat(MAXIMUM_PROGRESS_ERROR_BYTES);
    report("job-1", ProgressTaskStatus::Error, 10.0, Some(&at_limit))?;

    let over_limit = "e".repeat(MAXIMUM_PROGRESS_ERROR_BYTES + 1);
    assert!(matches!(
        report("job-1", ProgressTaskStatus::Error, 10.0, Some(&over_limit)),
        Err(ProgressValidationError::TextTooLong {
            field: "error",
            maximum_bytes: 4096
        })
    ));
    Ok(())
}

#[test]
fn error_detail_is_present_only_for_error_status() {
    assert!(matches!(
        report("job-1", ProgressTaskStatus::Running, 10.0, Some("boom")),
        Err(ProgressValidationError::UnexpectedTaskError)
    ));
    assert!(matches!(
        report("job-1", ProgressTaskStatus::Done, 100.0, Some("boom")),
        Err(ProgressValidationError::UnexpectedTaskError)
    ));
    assert!(matches!(
        report("job-1", ProgressTaskStatus::Error, 10.0, None),
        Err(ProgressValidationError::MissingTaskError)
    ));
    // An empty error string is normalized to "absent", which an error status rejects.
    assert!(matches!(
        report("job-1", ProgressTaskStatus::Error, 10.0, Some("")),
        Err(ProgressValidationError::MissingTaskError)
    ));
}

#[test]
fn monitor_error_accepts_exactly_the_byte_limit_and_rejects_one_over() -> TestResult {
    let mut progress = pane_progress(
        ProgressTaskStatus::Running,
        5.0,
        None,
        ProgressMonitorHealth::Healthy,
    )?;
    progress.monitor_health = ProgressMonitorHealth::Degraded {
        consecutive_failures: 1,
        last_error: "x".repeat(MAXIMUM_PROGRESS_MONITOR_ERROR_BYTES),
    };
    progress.validate()?;

    progress.monitor_health = ProgressMonitorHealth::Failed {
        consecutive_failures: 1,
        last_error: "x".repeat(MAXIMUM_PROGRESS_MONITOR_ERROR_BYTES + 1),
    };
    assert!(matches!(
        progress.validate(),
        Err(ProgressValidationError::TextTooLong {
            field: "monitor error",
            maximum_bytes: 2048
        })
    ));
    Ok(())
}

#[test]
fn eta_none_leaves_the_estimate_unset() -> TestResult {
    let progress_report =
        report("job-1", ProgressTaskStatus::Running, 5.0, None)?.with_eta_seconds(None)?;
    assert_eq!(progress_report.eta_seconds, None);
    Ok(())
}

#[test]
fn eta_accepts_zero_and_the_exact_maximum() -> TestResult {
    let zero =
        report("job-1", ProgressTaskStatus::Running, 5.0, None)?.with_eta_seconds(Some(0))?;
    assert_eq!(zero.eta_seconds, Some(0));

    let maximum = report("job-1", ProgressTaskStatus::Running, 5.0, None)?
        .with_eta_seconds(Some(MAXIMUM_PROGRESS_ETA_SECONDS))?;
    assert_eq!(maximum.eta_seconds, Some(315_360_000));
    Ok(())
}

#[test]
fn eta_above_the_maximum_returns_a_validation_error() -> TestResult {
    let result = report("job-1", ProgressTaskStatus::Running, 5.0, None)?
        .with_eta_seconds(Some(MAXIMUM_PROGRESS_ETA_SECONDS + 1));
    assert!(matches!(
        result,
        Err(ProgressValidationError::InvalidTiming(_))
    ));
    Ok(())
}

#[test]
fn eta_can_be_attached_to_a_non_running_status() -> TestResult {
    let done = report("job-1", ProgressTaskStatus::Done, 100.0, None)?.with_eta_seconds(Some(5))?;
    assert_eq!(done.eta_seconds, Some(5));
    Ok(())
}

// ProgressTaskStatus, PaneProgress

#[test]
fn done_and_error_statuses_are_terminal() {
    assert!(ProgressTaskStatus::Done.is_terminal());
    assert!(ProgressTaskStatus::Error.is_terminal());
}

#[test]
fn not_started_and_running_statuses_are_not_terminal() {
    assert!(!ProgressTaskStatus::NotStartedYet.is_terminal());
    assert!(!ProgressTaskStatus::Running.is_terminal());
}

#[test]
fn running_monitor_is_live() -> TestResult {
    let progress = pane_progress(
        ProgressTaskStatus::Running,
        20.0,
        None,
        ProgressMonitorHealth::Healthy,
    )?;
    assert!(progress.is_live());
    assert!(!progress.is_terminal());
    Ok(())
}

#[test]
fn terminal_monitor_is_not_live() -> TestResult {
    let done = pane_progress(
        ProgressTaskStatus::Done,
        100.0,
        None,
        ProgressMonitorHealth::Healthy,
    )?;
    let error = pane_progress(
        ProgressTaskStatus::Error,
        50.0,
        Some("exit 1"),
        ProgressMonitorHealth::Healthy,
    )?;
    assert!(!done.is_live());
    assert!(!error.is_live());
    Ok(())
}

#[test]
fn failed_monitor_is_not_live_but_degraded_monitor_is() -> TestResult {
    let failed_running = pane_progress(ProgressTaskStatus::Running, 20.0, None, failed())?;
    let degraded_running = pane_progress(ProgressTaskStatus::Running, 20.0, None, degraded())?;
    assert!(!failed_running.is_live());
    assert!(degraded_running.is_live());
    Ok(())
}

#[test]
fn unacknowledged_terminal_outcome_is_unread() -> TestResult {
    let progress = pane_progress(
        ProgressTaskStatus::Done,
        100.0,
        None,
        ProgressMonitorHealth::Healthy,
    )?;
    assert!(progress.has_unread_outcome());
    Ok(())
}

#[test]
fn acknowledged_terminal_outcome_is_not_unread() -> TestResult {
    let mut progress = pane_progress(
        ProgressTaskStatus::Done,
        100.0,
        None,
        ProgressMonitorHealth::Healthy,
    )?;
    progress.attention = ProgressAttention::Acknowledged;
    assert!(!progress.has_unread_outcome());
    Ok(())
}

#[test]
fn running_monitor_has_no_unread_outcome() -> TestResult {
    let progress = pane_progress(
        ProgressTaskStatus::Running,
        20.0,
        None,
        ProgressMonitorHealth::Healthy,
    )?;
    assert!(!progress.has_unread_outcome());
    Ok(())
}

#[test]
fn failed_monitor_without_acknowledgement_has_unread_outcome() -> TestResult {
    let progress = pane_progress(ProgressTaskStatus::Running, 20.0, None, failed())?;
    assert!(progress.has_unread_outcome());
    Ok(())
}

#[test]
fn pane_progress_is_terminal_maps_each_status() -> TestResult {
    let not_started = pane_progress(
        ProgressTaskStatus::NotStartedYet,
        0.0,
        None,
        ProgressMonitorHealth::Healthy,
    )?;
    let running = pane_progress(
        ProgressTaskStatus::Running,
        40.0,
        None,
        ProgressMonitorHealth::Healthy,
    )?;
    let done = pane_progress(
        ProgressTaskStatus::Done,
        100.0,
        None,
        ProgressMonitorHealth::Healthy,
    )?;
    let error = pane_progress(
        ProgressTaskStatus::Error,
        40.0,
        Some("exit 1"),
        ProgressMonitorHealth::Healthy,
    )?;
    assert!(!not_started.is_terminal());
    assert!(!running.is_terminal());
    assert!(done.is_terminal());
    assert!(error.is_terminal());
    Ok(())
}

#[test]
fn pane_progress_is_terminal_ignores_monitor_health() -> TestResult {
    let done_failed = pane_progress(ProgressTaskStatus::Done, 100.0, None, failed())?;
    let running_failed = pane_progress(ProgressTaskStatus::Running, 40.0, None, failed())?;
    let running_degraded = pane_progress(ProgressTaskStatus::Running, 40.0, None, degraded())?;
    assert!(done_failed.is_terminal());
    assert!(!running_failed.is_terminal());
    assert!(!running_degraded.is_terminal());
    Ok(())
}

// TaskSignal and buckets

#[test]
fn task_progress_bucket_boundaries() {
    assert_eq!(TASK_PROGRESS_BUCKETS, 12);
    assert_eq!(task_progress_bucket(0.0), 0);
    assert_eq!(task_progress_bucket(f32::NAN), 0);
    assert_eq!(task_progress_bucket(-1.0), 0);
    assert_eq!(task_progress_bucket(0.001), 1);
    assert_eq!(task_progress_bucket(8.0), 1);
    assert_eq!(task_progress_bucket(9.0), 2);
    assert_eq!(task_progress_bucket(83.0), 10);
    assert_eq!(task_progress_bucket(91.0), 11);
    assert_eq!(task_progress_bucket(99.9), 12);
    assert_eq!(task_progress_bucket(100.0), TASK_PROGRESS_BUCKETS);
    assert_eq!(task_progress_bucket(150.0), TASK_PROGRESS_BUCKETS);
}

#[test]
fn task_signal_maps_each_status_to_its_signal() -> TestResult {
    assert_eq!(
        task_signal(
            ProgressTaskStatus::NotStartedYet,
            0.0,
            None,
            ProgressMonitorHealth::Healthy,
            false
        )?,
        TaskSignal::Pending
    );
    assert_eq!(
        task_signal(
            ProgressTaskStatus::Done,
            100.0,
            None,
            ProgressMonitorHealth::Healthy,
            false
        )?,
        TaskSignal::Done { unread: true }
    );
    assert_eq!(
        task_signal(
            ProgressTaskStatus::Error,
            40.0,
            Some("exit 1"),
            ProgressMonitorHealth::Healthy,
            false
        )?,
        TaskSignal::Error { unread: true }
    );
    Ok(())
}

#[test]
fn task_signal_running_bucket_is_zero_at_zero_percent_and_last_at_hundred() -> TestResult {
    assert_eq!(
        task_signal(
            ProgressTaskStatus::Running,
            0.0,
            None,
            ProgressMonitorHealth::Healthy,
            false
        )?,
        TaskSignal::Running {
            bucket: 0,
            degraded: false
        }
    );
    assert_eq!(
        task_signal(
            ProgressTaskStatus::Running,
            100.0,
            None,
            ProgressMonitorHealth::Healthy,
            false
        )?,
        TaskSignal::Running {
            bucket: TASK_PROGRESS_BUCKETS,
            degraded: false
        }
    );
    Ok(())
}

#[test]
fn task_signal_running_bucket_follows_percent_and_degraded_health() -> TestResult {
    assert_eq!(
        task_signal(ProgressTaskStatus::Running, 25.0, None, degraded(), false)?,
        TaskSignal::Running {
            bucket: task_progress_bucket(25.0),
            degraded: true
        }
    );
    Ok(())
}

#[test]
fn task_signal_failed_monitor_and_error_precedence() -> TestResult {
    assert_eq!(
        task_signal(ProgressTaskStatus::Running, 30.0, None, failed(), false)?,
        TaskSignal::MonitorFailed { unread: true }
    );
    assert_eq!(
        task_signal(ProgressTaskStatus::Running, 30.0, None, failed(), true)?,
        TaskSignal::MonitorFailed { unread: false }
    );
    assert_eq!(
        task_signal(
            ProgressTaskStatus::Error,
            30.0,
            Some("exit 1"),
            failed(),
            false
        )?,
        TaskSignal::Error { unread: true }
    );
    Ok(())
}

#[test]
fn task_signal_done_respects_acknowledgement() -> TestResult {
    assert_eq!(
        task_signal(
            ProgressTaskStatus::Done,
            100.0,
            None,
            ProgressMonitorHealth::Healthy,
            true
        )?,
        TaskSignal::Done { unread: false }
    );
    Ok(())
}

// Title source, structure source, scheduled input, prompt queue

#[test]
fn only_user_specified_titles_are_user_specified() {
    assert!(PaneTitleSource::UserSpecified.is_user_specified());
    assert!(!PaneTitleSource::Automatic.is_user_specified());
}

#[test]
fn structure_source_labels_are_non_empty_and_distinct() {
    let manual = StructureSource::Manual.prompt_label();
    let restructure = StructureSource::LlmRestructure.prompt_label();
    assert!(!manual.is_empty());
    assert!(!restructure.is_empty());
    assert_ne!(manual, restructure);
}

#[test]
fn scheduled_input_requires_text_or_enter() {
    let empty = ScheduledPaneInput {
        execute_at_unix_millis: 1_000,
        text: String::new(),
        send_enter: false,
    };
    let enter_only = ScheduledPaneInput {
        execute_at_unix_millis: 1_000,
        text: String::new(),
        send_enter: true,
    };
    let text_only = ScheduledPaneInput {
        execute_at_unix_millis: 1_000,
        text: "ls".to_string(),
        send_enter: false,
    };
    assert!(!empty.has_input());
    assert!(enter_only.has_input());
    assert!(text_only.has_input());
}

#[test]
fn whitespace_only_scheduled_text_counts_as_input() {
    // The schedule sends the text as typed, so only an empty string is a no-op.
    let whitespace = ScheduledPaneInput {
        execute_at_unix_millis: 1_000,
        text: "   ".to_string(),
        send_enter: false,
    };
    assert!(whitespace.has_input());
}

#[test]
fn prompt_queue_delivery_rejects_zero_remaining_runs_only() {
    assert!(PromptQueueDelivery::Once.is_valid());
    assert!(PromptQueueDelivery::Forever.is_valid());
    assert!(PromptQueueDelivery::Times { remaining_runs: 1 }.is_valid());
    assert!(!PromptQueueDelivery::Times { remaining_runs: 0 }.is_valid());
}

#[test]
fn queued_prompt_rejects_blank_text() {
    let empty = queued_prompt("");
    let blank = queued_prompt(" \t ");
    assert!(!empty.is_valid());
    assert!(!blank.is_valid());
    assert!(queued_prompt("run the tests").is_valid());
}

#[test]
fn queued_prompt_propagates_invalid_delivery() {
    let prompt = QueuedPrompt {
        text: "run the tests".to_string(),
        delivery: PromptQueueDelivery::Times { remaining_runs: 0 },
        attempted_delivery: false,
    };
    assert!(!prompt.is_valid());
}

// ContainerKind, ContainerNode

#[test]
fn container_factories_construct_each_kind() {
    let path = PathBuf::from("/work/app");
    assert_eq!(
        ContainerNode::project(path.clone()).kind,
        ContainerKind::Project { path }
    );
    assert_eq!(ContainerNode::group().kind, ContainerKind::Group);
    assert_eq!(
        ContainerNode::split_view(SplitOrientation::Horizontal).kind,
        ContainerKind::SplitView {
            orientation: SplitOrientation::Horizontal
        }
    );
}

#[test]
fn container_kinds_are_distinct_and_equal_to_themselves() {
    assert_ne!(
        ContainerKind::Group,
        ContainerKind::SplitView {
            orientation: SplitOrientation::Vertical,
        }
    );
    assert_ne!(
        ContainerKind::SplitView {
            orientation: SplitOrientation::Vertical,
        },
        ContainerKind::SplitView {
            orientation: SplitOrientation::Horizontal,
        }
    );
    assert_eq!(ContainerKind::Group, ContainerKind::Group);
}

#[test]
fn project_and_group_containers_accept_normal_children() {
    assert!(ContainerNode::project(PathBuf::from("/work/app")).accepts_normal_children());
    assert!(ContainerNode::group().accepts_normal_children());
}

#[test]
fn split_view_does_not_accept_normal_children() {
    assert!(!ContainerNode::split_view(SplitOrientation::Vertical).accepts_normal_children());
}

// validate_branch_name

#[test]
fn branch_name_rejects_empty_input() {
    assert_eq!(validate_branch_name(""), Err(BranchNameError::Empty));
}

#[test]
fn branch_name_rejects_invalid_sequences() {
    assert_eq!(
        validate_branch_name("a..b"),
        Err(BranchNameError::InvalidSequence)
    );
    assert_eq!(
        validate_branch_name("a@{b"),
        Err(BranchNameError::InvalidSequence)
    );
    assert_eq!(
        validate_branch_name("@"),
        Err(BranchNameError::InvalidSequence)
    );
    assert_eq!(
        validate_branch_name("a//b"),
        Err(BranchNameError::InvalidSequence)
    );
}

#[test]
fn branch_name_rejects_forbidden_characters() {
    for branch in [
        "a b", "a~b", "a^b", "a:b", "a?b", "a*b", "a[b", "a\\b", "a\tb",
    ] {
        assert_eq!(
            validate_branch_name(branch),
            Err(BranchNameError::ForbiddenCharacter),
            "branch {branch:?}"
        );
    }
}

#[test]
fn branch_name_rejects_invalid_components() {
    for branch in ["-main", "main/", "main.", "a/.hidden", "a.lock", "a/b.lock"] {
        assert_eq!(
            validate_branch_name(branch),
            Err(BranchNameError::InvalidComponent),
            "branch {branch:?}"
        );
    }
}

#[test]
fn branch_name_accepts_a_plain_slash_separated_name() {
    assert_eq!(validate_branch_name("feature/plugin-host"), Ok(()));
}

// AgentClass, BuiltinAgentProvider

#[test]
fn agent_class_type_sort_ranks_are_unique_per_variant() {
    let ranks = [
        AgentClass::Claude.type_sort_rank(),
        AgentClass::Codex.type_sort_rank(),
        AgentClass::Antigravity.type_sort_rank(),
        AgentClass::Other("Gemini".to_string()).type_sort_rank(),
    ];
    for (index, rank) in ranks.iter().enumerate() {
        assert!(
            !ranks[index + 1..].contains(rank),
            "rank {rank} is shared by two variants"
        );
    }
    assert_eq!(AgentClass::Other("Gemini".to_string()).type_sort_rank(), 6);
}

#[test]
fn builtin_provider_type_sort_ranks_are_distinct_and_order_the_registry() {
    let mut ranks: Vec<u8> = BuiltinAgentProvider::ALL
        .iter()
        .map(|provider| provider.type_sort_rank())
        .collect();
    ranks.sort_unstable();
    ranks.dedup();
    assert_eq!(ranks.len(), BuiltinAgentProvider::ALL.len());

    let mut ordered = BuiltinAgentProvider::ALL;
    ordered.sort_by_key(|provider| provider.type_sort_rank());
    assert_eq!(ordered, BuiltinAgentProvider::ALL);
    assert_eq!(
        BuiltinAgentProvider::Claude.type_sort_rank(),
        BuiltinAgentProvider::Claude.type_sort_rank()
    );
}

#[test]
fn stepped_wraps_forward_and_backward_across_the_registry() {
    assert_eq!(
        BuiltinAgentProvider::Claude.stepped(1),
        BuiltinAgentProvider::Codex
    );
    assert_eq!(
        BuiltinAgentProvider::Codex.stepped(1),
        BuiltinAgentProvider::Antigravity
    );
    assert_eq!(
        BuiltinAgentProvider::Antigravity.stepped(1),
        BuiltinAgentProvider::Claude
    );
    assert_eq!(
        BuiltinAgentProvider::Claude.stepped(-1),
        BuiltinAgentProvider::Antigravity
    );
    assert_eq!(
        BuiltinAgentProvider::Codex.stepped(-1),
        BuiltinAgentProvider::Claude
    );
}

#[test]
fn stepped_with_zero_delta_returns_the_same_provider() {
    for provider in BuiltinAgentProvider::ALL {
        assert_eq!(provider.stepped(0), provider);
    }
}

#[test]
fn stepped_handles_large_positive_and_negative_deltas() {
    assert_eq!(
        BuiltinAgentProvider::Claude.stepped(1_000_000),
        BuiltinAgentProvider::Codex
    );
    assert_eq!(
        BuiltinAgentProvider::Claude.stepped(i32::MAX),
        BuiltinAgentProvider::Codex
    );
    assert_eq!(
        BuiltinAgentProvider::Claude.stepped(i32::MIN),
        BuiltinAgentProvider::Antigravity
    );
}

#[test]
fn from_config_name_maps_each_known_name() {
    assert_eq!(
        BuiltinAgentProvider::from_config_name("claude"),
        Some(BuiltinAgentProvider::Claude)
    );
    assert_eq!(
        BuiltinAgentProvider::from_config_name("codex"),
        Some(BuiltinAgentProvider::Codex)
    );
    assert_eq!(
        BuiltinAgentProvider::from_config_name("antigravity"),
        Some(BuiltinAgentProvider::Antigravity)
    );
    assert_eq!(
        BuiltinAgentProvider::from_config_name("antimatter"),
        Some(BuiltinAgentProvider::Antigravity)
    );
}

#[test]
fn from_config_name_ignores_case_and_surrounding_whitespace() {
    assert_eq!(
        BuiltinAgentProvider::from_config_name("  CODEX  "),
        Some(BuiltinAgentProvider::Codex)
    );
    assert_eq!(
        BuiltinAgentProvider::from_config_name("Claude"),
        Some(BuiltinAgentProvider::Claude)
    );
}

#[test]
fn from_config_name_returns_none_for_unknown_and_empty_names() {
    assert_eq!(BuiltinAgentProvider::from_config_name("gemini"), None);
    assert_eq!(BuiltinAgentProvider::from_config_name("agy"), None);
    assert_eq!(BuiltinAgentProvider::from_config_name(""), None);
    assert_eq!(BuiltinAgentProvider::from_config_name("   "), None);
}

#[test]
fn from_command_line_matches_each_exact_fresh_launch_command() {
    assert_eq!(
        BuiltinAgentProvider::from_command_line("claude"),
        Some(BuiltinAgentProvider::Claude)
    );
    assert_eq!(
        BuiltinAgentProvider::from_command_line("codex"),
        Some(BuiltinAgentProvider::Codex)
    );
    assert_eq!(
        BuiltinAgentProvider::from_command_line("agy"),
        Some(BuiltinAgentProvider::Antigravity)
    );
}

#[test]
fn from_command_line_rejects_non_agent_and_empty_commands() {
    assert_eq!(BuiltinAgentProvider::from_command_line("bash"), None);
    assert_eq!(
        BuiltinAgentProvider::from_command_line("claude --resume x"),
        None
    );
    assert_eq!(
        BuiltinAgentProvider::from_command_line("/usr/bin/claude"),
        None
    );
    assert_eq!(BuiltinAgentProvider::from_command_line(""), None);
    assert_eq!(BuiltinAgentProvider::from_command_line("   "), None);
}

#[test]
fn unknown_agent_class_falls_back_safely() {
    let unknown = AgentClass::Other("Gemini".to_string());
    assert_eq!(unknown.provider(), None);
    assert_eq!(unknown.label(), "Gemini");
    assert_eq!(BuiltinAgentProvider::from_config_name("gemini"), None);
}

// AgentProvider trait

#[test]
fn each_builtin_provider_implements_class_and_labels() {
    assert_eq!(BuiltinAgentProvider::Claude.class(), AgentClass::Claude);
    assert_eq!(BuiltinAgentProvider::Codex.class(), AgentClass::Codex);
    assert_eq!(
        BuiltinAgentProvider::Antigravity.class(),
        AgentClass::Antigravity
    );
    assert_eq!(BuiltinAgentProvider::Claude.label(), "Claude");
    assert_eq!(BuiltinAgentProvider::Codex.label(), "Codex");
    assert_eq!(BuiltinAgentProvider::Antigravity.label(), "Antigravity");
}

#[test]
fn each_builtin_provider_has_its_command_and_process_names() {
    assert_eq!(BuiltinAgentProvider::Claude.command_line(), "claude");
    assert_eq!(BuiltinAgentProvider::Codex.command_line(), "codex");
    assert_eq!(BuiltinAgentProvider::Antigravity.command_line(), "agy");
    assert!(BuiltinAgentProvider::Claude
        .process_name_substrings()
        .contains(&"claude"));
    assert!(BuiltinAgentProvider::Codex
        .process_name_substrings()
        .contains(&"codex"));
    let antigravity = BuiltinAgentProvider::Antigravity.process_name_substrings();
    assert!(antigravity.contains(&"agy"));
    assert!(antigravity.contains(&"antigravity"));
    assert!(antigravity.contains(&"antimatter"));
}

#[test]
fn each_builtin_provider_builds_its_resume_command() {
    assert_eq!(
        BuiltinAgentProvider::Claude.resume_command("'abc'"),
        "claude --resume 'abc'"
    );
    assert_eq!(
        BuiltinAgentProvider::Codex.resume_command("'abc'"),
        "codex resume 'abc'"
    );
    assert_eq!(
        BuiltinAgentProvider::Antigravity.resume_command("'abc'"),
        "agy --conversation 'abc'"
    );
}

#[test]
fn session_id_is_parsed_from_claude_arguments_with_and_without_flags() {
    assert_eq!(
        BuiltinAgentProvider::Claude
            .session_id_from_arguments(&args(&["claude", "--resume", SESSION_ID])),
        Some(SESSION_ID.to_string())
    );
    assert_eq!(
        BuiltinAgentProvider::Claude
            .session_id_from_arguments(&args(&["claude", "-r", SESSION_ID])),
        Some(SESSION_ID.to_string())
    );
    let equals_form = format!("--resume={SESSION_ID}");
    assert_eq!(
        BuiltinAgentProvider::Claude
            .session_id_from_arguments(&args(&["claude", equals_form.as_str()])),
        Some(SESSION_ID.to_string())
    );
    assert_eq!(
        BuiltinAgentProvider::Claude.session_id_from_arguments(&args(&["claude"])),
        None
    );
    assert_eq!(
        BuiltinAgentProvider::Claude.session_id_from_arguments(&args(&[
            "claude",
            "--fork-session",
            "--resume",
            SESSION_ID
        ])),
        None
    );
    assert_eq!(
        BuiltinAgentProvider::Claude
            .session_id_from_arguments(&args(&["claude", "--", "--resume", SESSION_ID])),
        None
    );
}

#[test]
fn session_id_is_parsed_from_codex_arguments() {
    assert_eq!(
        BuiltinAgentProvider::Codex.session_id_from_arguments(&args(&[
            "/usr/local/bin/codex",
            "resume",
            SESSION_ID
        ])),
        Some(SESSION_ID.to_string())
    );
    assert_eq!(
        BuiltinAgentProvider::Codex.session_id_from_arguments(&args(&[
            "codex.exe",
            "resume",
            SESSION_ID
        ])),
        Some(SESSION_ID.to_string())
    );
    assert_eq!(
        BuiltinAgentProvider::Codex
            .session_id_from_arguments(&args(&["codex", "--model", "x", "resume", SESSION_ID])),
        None
    );
    assert_eq!(
        BuiltinAgentProvider::Codex.session_id_from_arguments(&args(&["codex"])),
        None
    );
}

#[test]
fn session_id_is_parsed_from_antigravity_arguments_with_and_without_flags() {
    assert_eq!(
        BuiltinAgentProvider::Antigravity.session_id_from_arguments(&args(&[
            "agy",
            "--conversation",
            SESSION_ID
        ])),
        Some(SESSION_ID.to_string())
    );
    let equals_form = format!("--conversation={SESSION_ID}");
    assert_eq!(
        BuiltinAgentProvider::Antigravity
            .session_id_from_arguments(&args(&["agy", equals_form.as_str()])),
        Some(SESSION_ID.to_string())
    );
    assert_eq!(
        BuiltinAgentProvider::Antigravity.session_id_from_arguments(&args(&["agy"])),
        None
    );
}

#[test]
fn shared_new_session_commands_transition_every_builtin_provider() {
    for provider in BuiltinAgentProvider::ALL {
        for command in ["/resume abc", "/branch", "/new"] {
            assert_eq!(
                provider.session_identity_transition_rule(command),
                Some(SessionIdentityTransitionRule::SharedNewSessionCommand),
                "{provider:?} with {command:?}"
            );
        }
    }
}

#[test]
fn clear_starts_a_fresh_session_for_claude_and_codex_only() {
    assert_eq!(
        BuiltinAgentProvider::Claude.session_identity_transition_rule("/clear"),
        Some(SessionIdentityTransitionRule::ClaudeOrCodexClearStartsFreshSession)
    );
    assert_eq!(
        BuiltinAgentProvider::Codex.session_identity_transition_rule("  /clear  "),
        Some(SessionIdentityTransitionRule::ClaudeOrCodexClearStartsFreshSession)
    );
    assert_eq!(
        BuiltinAgentProvider::Antigravity.session_identity_transition_rule("/clear"),
        None
    );
}

#[test]
fn ordinary_and_near_miss_commands_do_not_transition_identity() {
    assert_eq!(
        BuiltinAgentProvider::Claude.session_identity_transition_rule("ls"),
        None
    );
    assert_eq!(
        BuiltinAgentProvider::Claude.session_identity_transition_rule("/resumed"),
        None
    );
    assert_eq!(
        BuiltinAgentProvider::Claude.session_identity_transition_rule("/clear now"),
        None
    );
    assert!(!BuiltinAgentProvider::Codex.invalidates_session_identity("ls"));
    assert!(BuiltinAgentProvider::Codex.invalidates_session_identity("/new"));
}

#[test]
fn transition_rule_explanations_are_non_empty_and_name_their_command() {
    let shared = SessionIdentityTransitionRule::SharedNewSessionCommand.explanation();
    let clear = SessionIdentityTransitionRule::ClaudeOrCodexClearStartsFreshSession.explanation();
    assert!(!shared.is_empty());
    assert!(!clear.is_empty());
    assert!(shared.contains("/resume"));
    assert!(clear.contains("/clear"));
    assert!(clear.contains("fresh"));
}

// Tree: activity, presentation, ordering, capacity, prompt queue

#[test]
fn node_activity_update_fields_are_populated_and_revision_increments() -> TestResult {
    let mut tree = Tree::new();
    let first: NodeActivityUpdate = tree.record_node_activity(ROOT_ID)?;
    assert_eq!(first.activity_revision, 1);
    assert!(first.became_unrestructured);
    assert!(first.became_unread_since_focus);

    let second = tree.record_node_activity(ROOT_ID)?;
    assert_eq!(second.activity_revision, 2);
    assert!(!second.became_unrestructured);
    assert!(!second.became_unread_since_focus);
    Ok(())
}

#[test]
fn retained_node_capacity_starts_at_the_root_baseline() {
    let tree = Tree::new();
    assert!(tree.retained_node_capacity() >= 1);
}

#[test]
fn retained_node_capacity_grows_with_added_nodes() -> TestResult {
    let mut tree = Tree::new();
    let before = tree.retained_node_capacity();
    for index in 0..100 {
        tree.add_group(ROOT_ID, format!("Group {index}"))?;
    }
    assert!(tree.retained_node_capacity() >= 101);
    assert!(tree.retained_node_capacity() > before);
    Ok(())
}

#[test]
fn retained_node_capacity_remains_after_removals() -> TestResult {
    let mut tree = Tree::new();
    let mut groups = Vec::new();
    for index in 0..100 {
        groups.push(tree.add_group(ROOT_ID, format!("Group {index}"))?);
    }
    let full_capacity = tree.retained_node_capacity();
    for group in groups {
        tree.remove_node(group)?;
    }
    assert_eq!(tree.retained_node_capacity(), full_capacity);
    Ok(())
}

#[test]
fn merge_activity_revision_applies_newer_and_ignores_older() -> TestResult {
    let mut tree = Tree::new();
    let group = tree.add_group(ROOT_ID, "Work")?;
    tree.merge_node_activity_revision(group, 5)?;
    assert_eq!(tree.get(group).map(|node| node.activity_revision), Some(5));
    tree.merge_node_activity_revision(group, 3)?;
    assert_eq!(tree.get(group).map(|node| node.activity_revision), Some(5));
    Ok(())
}

#[test]
fn merge_activity_revision_unknown_node_is_an_error() {
    let mut tree = Tree::new();
    assert!(matches!(
        tree.merge_node_activity_revision(NodeId(9_999), 1),
        Err(TreeError::NodeNotFound(NodeId(9_999)))
    ));
}

#[test]
fn merge_focus_checkpoint_applies_newer_checkpoint() -> TestResult {
    let mut tree = Tree::new();
    let group = tree.add_group(ROOT_ID, "Work")?;
    tree.merge_node_focus_checkpoint(group, 7)?;
    let node = tree.get(group).ok_or("group missing")?;
    assert_eq!(node.activity_revision, 7);
    assert_eq!(node.last_focus_activity_revision, Some(7));
    Ok(())
}

#[test]
fn stale_focus_checkpoint_never_rewinds_the_activity_generation() -> TestResult {
    // Only the activity generation uses max semantics; the checkpoint itself
    // is recorded exactly as given (see the doc comment on the method).
    let mut tree = Tree::new();
    let group = tree.add_group(ROOT_ID, "Work")?;
    tree.merge_node_focus_checkpoint(group, 4)?;
    tree.merge_node_focus_checkpoint(group, 2)?;
    assert_eq!(tree.get(group).map(|node| node.activity_revision), Some(4));
    Ok(())
}

#[test]
fn merge_focus_checkpoint_unknown_node_is_an_error() {
    let mut tree = Tree::new();
    assert!(matches!(
        tree.merge_node_focus_checkpoint(NodeId(9_999), 1),
        Err(TreeError::NodeNotFound(NodeId(9_999)))
    ));
}

#[test]
fn merge_restructure_checkpoint_applies_newer_checkpoint() -> TestResult {
    let mut tree = Tree::new();
    let group = tree.add_group(ROOT_ID, "Work")?;
    tree.merge_node_restructure_checkpoint(group, 6)?;
    let node = tree.get(group).ok_or("group missing")?;
    assert_eq!(node.activity_revision, 6);
    assert_eq!(node.last_restructure_activity_revision, Some(6));
    Ok(())
}

#[test]
fn stale_restructure_checkpoint_never_rewinds_the_activity_generation() -> TestResult {
    let mut tree = Tree::new();
    let group = tree.add_group(ROOT_ID, "Work")?;
    tree.merge_node_restructure_checkpoint(group, 9)?;
    tree.merge_node_restructure_checkpoint(group, 3)?;
    assert_eq!(tree.get(group).map(|node| node.activity_revision), Some(9));
    Ok(())
}

#[test]
fn merge_restructure_checkpoint_unknown_node_is_an_error() {
    let mut tree = Tree::new();
    assert!(matches!(
        tree.merge_node_restructure_checkpoint(NodeId(9_999), 1),
        Err(TreeError::NodeNotFound(NodeId(9_999)))
    ));
}

#[test]
fn pane_ids_in_tree_order_is_empty_for_a_tree_without_panes() {
    assert!(Tree::new().pane_ids_in_tree_order().is_empty());
}

#[test]
fn pane_ids_in_tree_order_is_depth_first_through_nested_groups() -> TestResult {
    let mut tree = Tree::new();
    let group = tree.add_group(ROOT_ID, "A")?;
    let first = tree.add_pane(group, "p1", PaneContentKind::Terminal)?;
    let nested = tree.add_group(group, "B")?;
    let second = tree.add_pane(nested, "p2", PaneContentKind::Terminal)?;
    let third = tree.add_pane(group, "p3", PaneContentKind::Terminal)?;
    assert_eq!(tree.pane_ids_in_tree_order(), vec![first, second, third]);
    Ok(())
}

#[test]
fn pane_order_is_stable_after_a_move() -> TestResult {
    let mut tree = Tree::new();
    let group = tree.add_group(ROOT_ID, "A")?;
    let first = tree.add_pane(group, "p1", PaneContentKind::Terminal)?;
    let nested = tree.add_group(group, "B")?;
    let second = tree.add_pane(nested, "p2", PaneContentKind::Terminal)?;
    let third = tree.add_pane(group, "p3", PaneContentKind::Terminal)?;
    tree.move_node(first, nested, None)?;
    assert_eq!(tree.pane_ids_in_tree_order(), vec![second, first, third]);
    Ok(())
}

#[test]
fn pane_ids_in_subtree_of_a_leaf_pane_is_the_pane_itself() -> TestResult {
    let (tree, pane) = pane_in_group()?;
    assert_eq!(tree.pane_ids_in_subtree(pane), vec![pane]);
    Ok(())
}

#[test]
fn pane_ids_in_subtree_of_a_group_returns_only_its_panes() -> TestResult {
    let mut tree = Tree::new();
    let group = tree.add_group(ROOT_ID, "A")?;
    let first = tree.add_pane(group, "p1", PaneContentKind::Terminal)?;
    let second = tree.add_pane(group, "p2", PaneContentKind::Terminal)?;
    let other = tree.add_group(ROOT_ID, "B")?;
    tree.add_pane(other, "q", PaneContentKind::Terminal)?;
    assert_eq!(tree.pane_ids_in_subtree(group), vec![first, second]);
    Ok(())
}

#[test]
fn pane_ids_in_subtree_of_an_unknown_root_is_empty() {
    assert!(Tree::new().pane_ids_in_subtree(NodeId(9_999)).is_empty());
}

#[test]
fn invalidate_presentation_advances_an_existing_node_revision() -> TestResult {
    let mut tree = Tree::new();
    let group = tree.add_group(ROOT_ID, "Work")?;
    assert_eq!(
        tree.get(group).map(|node| node.presentation_revision),
        Some(0)
    );
    tree.invalidate_presentation(group)?;
    assert_eq!(
        tree.get(group).map(|node| node.presentation_revision),
        Some(1)
    );
    Ok(())
}

#[test]
fn invalidate_presentation_handles_a_project_node() -> TestResult {
    let mut tree = Tree::new();
    let project = tree.add_project(PathBuf::from("/work/app"))?;
    tree.invalidate_presentation(project)?;
    assert_eq!(
        tree.get(project).map(|node| node.presentation_revision),
        Some(1)
    );
    Ok(())
}

#[test]
fn invalidate_presentation_unknown_node_is_an_error() {
    let mut tree = Tree::new();
    assert!(matches!(
        tree.invalidate_presentation(NodeId(9_999)),
        Err(TreeError::NodeNotFound(NodeId(9_999)))
    ));
}

#[test]
fn clear_prompt_queue_on_an_empty_queue_is_a_no_op() -> TestResult {
    let (mut tree, pane) = pane_in_group()?;
    tree.clear_prompt_queue(pane)?;
    assert_eq!(prompt_queue_len(&tree, pane), Some(0));
    Ok(())
}

#[test]
fn clear_prompt_queue_empties_queued_entries() -> TestResult {
    let (mut tree, pane) = pane_in_group()?;
    tree.enqueue_prompt(pane, queued_prompt("one"))?;
    tree.enqueue_prompt(pane, queued_prompt("two"))?;
    assert_eq!(prompt_queue_len(&tree, pane), Some(2));
    tree.clear_prompt_queue(pane)?;
    assert_eq!(prompt_queue_len(&tree, pane), Some(0));
    Ok(())
}

#[test]
fn clear_prompt_queue_unknown_id_is_an_error() {
    let mut tree = Tree::new();
    assert!(matches!(
        tree.clear_prompt_queue(NodeId(9_999)),
        Err(TreeError::NodeNotFound(NodeId(9_999)))
    ));
}

#[test]
fn node_accepts_normal_children_is_true_for_containers_only() -> TestResult {
    let mut tree = Tree::new();
    let group = tree.add_group(ROOT_ID, "Work")?;
    let pane = tree.add_pane(group, "Shell", PaneContentKind::Terminal)?;
    assert_eq!(
        tree.get(group).map(|node| node.accepts_normal_children()),
        Some(true)
    );
    assert_eq!(
        tree.get(pane).map(|node| node.accepts_normal_children()),
        Some(false)
    );
    Ok(())
}

#[test]
fn node_project_path_returns_the_path_only_for_projects() -> TestResult {
    let mut tree = Tree::new();
    let path = PathBuf::from("/work/app");
    let project = tree.add_project(path.clone())?;
    let group = tree.add_group(ROOT_ID, "Work")?;
    assert_eq!(
        tree.get(project).and_then(|node| node.project_path()),
        Some(path.as_path())
    );
    assert_eq!(tree.get(group).and_then(|node| node.project_path()), None);
    Ok(())
}

#[test]
fn node_is_container_for_groups_projects_and_split_views_only() -> TestResult {
    let mut tree = Tree::new();
    let project = tree.add_project(PathBuf::from("/work/app"))?;
    let group = tree.add_group(project, "Work")?;
    let first = tree.add_pane(group, "p1", PaneContentKind::Terminal)?;
    let second = tree.add_pane(group, "p2", PaneContentKind::Terminal)?;
    let split =
        tree.create_split_view(group, "Split", SplitOrientation::Vertical, &[first, second])?;
    assert_eq!(tree.get(group).map(|node| node.is_container()), Some(true));
    assert_eq!(
        tree.get(project).map(|node| node.is_container()),
        Some(true)
    );
    assert_eq!(tree.get(split).map(|node| node.is_container()), Some(true));
    assert_eq!(tree.get(first).map(|node| node.is_container()), Some(false));
    Ok(())
}

#[test]
fn node_is_pane_for_panes_only() -> TestResult {
    let (tree, pane) = pane_in_group()?;
    let group = tree.parent_of(pane).ok_or("pane has no group")?;
    assert_eq!(tree.get(pane).map(|node| node.is_pane()), Some(true));
    assert_eq!(tree.get(group).map(|node| node.is_pane()), Some(false));
    Ok(())
}

#[test]
fn node_is_folder_for_folders_only() -> TestResult {
    let mut tree = Tree::new();
    let group = tree.add_group(ROOT_ID, "Work")?;
    let folder = tree.add_folder(group, PathBuf::from("/data/notes"))?;
    let project = tree.add_project(PathBuf::from("/work/app"))?;
    let pane = tree.add_pane(group, "Shell", PaneContentKind::Terminal)?;
    assert_eq!(tree.get(folder).map(|node| node.is_folder()), Some(true));
    assert_eq!(tree.get(project).map(|node| node.is_folder()), Some(false));
    assert_eq!(tree.get(pane).map(|node| node.is_folder()), Some(false));
    Ok(())
}

#[test]
fn tree_default_equals_new_and_validates() -> TestResult {
    let defaulted = Tree::default();
    assert_eq!(defaulted, Tree::new());
    defaulted.validate()?;
    Ok(())
}

// progress_timing: stall, quiet time, samples, checks, rate, ETA

#[test]
fn stall_constants_match_the_documented_policy() {
    assert_eq!(STALL_PROBE_COUNT, 10);
    assert_eq!(STALL_MINIMUM_MILLIS, 10 * 60 * 1000);
}

#[test]
fn nine_probe_intervals_of_quiet_time_are_not_stalled() {
    let timing = timing_with_probe_interval(60_000);
    assert!(!timing.is_stalled(9 * 60_000));
    assert!(!timing.is_stalled(STALL_MINIMUM_MILLIS - 1));
}

#[test]
fn ten_probe_intervals_and_minimum_time_mark_a_stall() {
    let timing = timing_with_probe_interval(60_000);
    assert!(timing.is_stalled(STALL_MINIMUM_MILLIS));

    let slow = timing_with_probe_interval(120_000);
    assert_eq!(slow.stall_threshold_millis(), STALL_PROBE_COUNT * 120_000);
    assert!(!slow.is_stalled(1_199_999));
    assert!(slow.is_stalled(1_200_000));
}

#[test]
fn stall_threshold_is_never_below_the_minimum_without_an_interval() {
    let timing = ProgressTiming::started(0, 0.0);
    assert_eq!(timing.stall_threshold_millis(), STALL_MINIMUM_MILLIS);
}

#[test]
fn progress_sample_percent_must_be_finite_and_in_range() {
    let mut nan = ProgressTiming::started(0, 50.0);
    nan.samples.push(ProgressSample {
        unix_millis: 1,
        percent: f32::NAN,
    });
    assert!(nan.validate().is_err());

    let mut above = ProgressTiming::started(0, 50.0);
    above.samples.push(ProgressSample {
        unix_millis: 1,
        percent: 100.5,
    });
    assert!(above.validate().is_err());

    let mut valid = ProgressTiming::started(0, 50.0);
    valid.samples.push(ProgressSample {
        unix_millis: 1,
        percent: 100.0,
    });
    assert!(valid.validate().is_ok());
}

#[test]
fn unchanged_percent_is_recorded_as_a_heartbeat_check() {
    let mut timing = ProgressTiming::started(0, 10.0);
    timing.observe(1_000, 10.0, false);
    let check = timing.checks.last().copied();
    assert_eq!(
        check.map(|check| check.outcome),
        Some(ProgressCheckOutcome::Heartbeat)
    );
    assert_eq!(timing.last_progress_unix_millis, Some(0));
}

#[test]
fn rising_percent_or_changed_message_is_recorded_as_progress() {
    let mut timing = ProgressTiming::started(0, 10.0);
    timing.observe(1_000, 12.0, false);
    assert_eq!(
        timing.checks.last().map(|check| check.outcome),
        Some(ProgressCheckOutcome::Progress)
    );
    assert_eq!(timing.last_progress_unix_millis, Some(1_000));

    timing.observe(2_000, 12.0, true);
    assert_eq!(
        timing.checks.last().map(|check| check.outcome),
        Some(ProgressCheckOutcome::Progress)
    );
    assert_eq!(timing.last_progress_unix_millis, Some(2_000));
}

#[test]
fn failed_observation_is_recorded_without_refreshing_progress_time() {
    let mut timing = ProgressTiming::started(0, 10.0);
    timing.record_failed_check(2_000);
    let check = timing.checks.last().copied();
    assert_eq!(
        check.map(|check| check.outcome),
        Some(ProgressCheckOutcome::Failed)
    );
    assert_eq!(check.and_then(|check| check.percent), None);
    assert_eq!(timing.last_progress_unix_millis, Some(0));
}

#[test]
fn progress_check_validation_rejects_invalid_fields() {
    let mut failed_with_percent = ProgressTiming::started(0, 10.0);
    failed_with_percent.checks.push(ProgressCheck {
        unix_millis: 1,
        outcome: ProgressCheckOutcome::Failed,
        percent: Some(5.0),
    });
    assert!(failed_with_percent.validate().is_err());

    let mut progress_without_percent = ProgressTiming::started(0, 10.0);
    progress_without_percent.checks.push(ProgressCheck {
        unix_millis: 1,
        outcome: ProgressCheckOutcome::Progress,
        percent: None,
    });
    assert!(progress_without_percent.validate().is_err());

    let mut infinite_percent = ProgressTiming::started(0, 10.0);
    infinite_percent.checks.push(ProgressCheck {
        unix_millis: 1,
        outcome: ProgressCheckOutcome::Heartbeat,
        percent: Some(f32::INFINITY),
    });
    assert!(infinite_percent.validate().is_err());

    let mut valid_failure = ProgressTiming::started(0, 10.0);
    valid_failure.record_failed_check(1);
    assert!(valid_failure.validate().is_ok());
}

#[test]
fn rate_is_percent_per_minute_over_positive_elapsed_time() -> TestResult {
    let mut timing = ProgressTiming::started(0, 0.0);
    timing.observe(60_000, 10.0, false);
    let rate = timing.rate(10.0).ok_or("rate missing")?;
    assert!((rate.percent_per_minute - 10.0).abs() < 1e-6);
    assert!(rate.recent_percent_per_minute.is_some());
    assert!(rate.whole_run_percent_per_minute.is_some());
    Ok(())
}

#[test]
fn zero_elapsed_time_yields_no_rate() {
    let timing = ProgressTiming::started(5_000, 0.0);
    assert!(timing.rate(0.0).is_none());
    assert!(timing.rate(5.0).is_none());
}

#[test]
fn eta_range_is_reported_only_when_the_paces_differ_enough() {
    let wide = EtaEstimate {
        expected_millis: 5_000,
        fastest_millis: 4_000,
        slowest_millis: 6_000,
    };
    assert_eq!(wide.range_millis(), Some((4_000, 6_000)));

    let single = EtaEstimate {
        expected_millis: 5_000,
        fastest_millis: 5_000,
        slowest_millis: 5_000,
    };
    assert_eq!(single.range_millis(), None);

    let narrow = EtaEstimate {
        expected_millis: 10_000,
        fastest_millis: 10_000,
        slowest_millis: 10_500,
    };
    assert_eq!(narrow.range_millis(), None);
}

#[test]
fn quiet_time_is_unknown_without_a_recorded_observation() {
    assert_eq!(ProgressTiming::default().quiet_millis(5_000), None);
}

#[test]
fn quiet_time_grows_after_the_last_observation() {
    let timing = ProgressTiming::started(1_000, 0.0);
    assert_eq!(timing.quiet_millis(4_000), Some(3_000));
}

#[test]
fn quiet_time_saturates_to_zero_when_now_precedes_the_observation() {
    let timing = ProgressTiming::started(1_000, 0.0);
    assert_eq!(timing.quiet_millis(500), Some(0));
}

// PaneProgress::new with ProgressTiming and monitor health, plus sample-free checks

#[test]
fn pane_progress_new_records_the_first_observation_as_unread_and_healthy() -> TestResult {
    let progress = PaneProgress::new(
        1,
        report("job-1", ProgressTaskStatus::Running, 5.0, None)?,
        1_000,
    )?;
    assert_eq!(progress.attention, ProgressAttention::Unread);
    assert_eq!(progress.monitor_health, ProgressMonitorHealth::Healthy);
    assert_eq!(progress.timing.started_at_unix_millis, Some(1_000));
    Ok(())
}

#[test]
fn pane_progress_monitor_id_zero_is_rejected() -> TestResult {
    let result = PaneProgress::new(
        0,
        report("job-1", ProgressTaskStatus::Running, 5.0, None)?,
        1_000,
    );
    assert!(matches!(
        result,
        Err(ProgressValidationError::InvalidMonitorId)
    ));
    Ok(())
}

// option_compatible_list and PaneStatus serde are intentionally not tested here:
// ilium-core has no serialization format crate available to tests, and the
// Cargo manifest may not be changed for this task.
