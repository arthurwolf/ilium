//! Pure scheduling and queue rules ported from ilium-client/src/scheduled_input.rs,
//! ilium-client/src/prompt_queue.rs, ilium-server/src/scheduled_input.rs,
//! ilium-server/src/prompt_queue.rs, ilium-server/src/detection_effects.rs and the queue
//! methods of ilium-core, run against the plugin's `schedule`, `queue` and `effects` modules.

use ilium_plugin_core_scheduler::effects::Requests;
use ilium_plugin_core_scheduler::queue::{
    cycle_delivery, next_send, normalize_paste, step_times, validated_request, Activity,
    CompletionEpoch, Delivery, PromptQueue, QueueError, QueuedPrompt, Turn,
    MAXIMUM_PROMPT_QUEUE_LEN,
};
use ilium_plugin_core_scheduler::schedule::{
    accepts_numeric_key, clear_if_matches, deadline_from_delay, due_inputs, nearest_deadline,
    next_redraw_unix_millis, remaining_seconds, DialogFocus, Draft, Pending, ScheduledInput,
};

fn draft(hours: &str, minutes: &str, seconds: &str, text: &str, send_enter: bool) -> Draft {
    Draft {
        hours: hours.to_string(),
        minutes: minutes.to_string(),
        seconds: seconds.to_string(),
        text: text.to_string(),
        send_enter,
    }
}

fn prompt(text: &str, delivery: Delivery) -> QueuedPrompt {
    QueuedPrompt {
        text: text.to_string(),
        delivery,
        attempted_delivery: false,
    }
}

fn pending(pane_id: u64, at: u64, available: bool) -> Pending {
    Pending {
        pane_id,
        input: ScheduledInput {
            execute_at_unix_millis: at,
            text: "go".to_string(),
            send_enter: true,
        },
        available,
    }
}

#[test]
fn a_fresh_draft_waits_thirty_seconds_and_sends_enter() {
    let fresh = Draft::default();
    assert_eq!(fresh.seconds, "30");
    assert!(fresh.send_enter);
    assert_eq!(fresh.delay_seconds(), Ok(30));
}

#[test]
fn rejects_zero_invalid_clock_fields_and_empty_payload() {
    assert_eq!(
        draft("", "", "0", "x", true).delay_seconds(),
        Err("Choose a delay of at least one second".to_string())
    );
    assert_eq!(
        draft("", "60", "", "x", true).delay_seconds(),
        Err("Minutes must be between 0 and 59".to_string())
    );
    assert_eq!(
        draft("", "", "60", "x", true).delay_seconds(),
        Err("Seconds must be between 0 and 59".to_string())
    );
    assert_eq!(
        draft("-1", "", "5", "x", true).delay_seconds(),
        Err("Hours must be a whole number".to_string())
    );
    assert_eq!(
        draft("", "1.5", "", "x", true).delay_seconds(),
        Err("Minutes must be a whole number".to_string())
    );
    assert_eq!(
        draft("", "", "5", "", false).validated_request(),
        Err("Enter text, enable Send Enter, or use both".to_string())
    );
}

#[test]
fn validates_text_only_text_and_enter_and_enter_only() {
    let text_only = draft("", "", "5", "go", false)
        .validated_request()
        .expect("text only");
    assert_eq!(
        (text_only.text.as_str(), text_only.send_enter),
        ("go", false)
    );
    let both = draft("", "", "5", "go", true)
        .validated_request()
        .expect("text and enter");
    assert!(both.send_enter && both.text == "go");
    let enter_only = draft("", "", "5", "", true)
        .validated_request()
        .expect("enter only");
    assert_eq!(
        (enter_only.text.as_str(), enter_only.send_enter),
        ("", true)
    );
}

#[test]
fn duration_uses_checked_units_and_reports_overflow() {
    assert_eq!(draft("1", "2", "3", "x", true).delay_seconds(), Ok(3723));
    let huge = u64::MAX.to_string();
    assert_eq!(
        draft(&huge, "", "1", "x", true).delay_seconds(),
        Err("The duration is too large".to_string())
    );
}

#[test]
fn numeric_fields_accept_only_digits() {
    assert!(accepts_numeric_key('7'));
    assert!(!accepts_numeric_key('x'));
    assert!(!accepts_numeric_key('-'));
}

#[test]
fn dialog_focus_tabs_forward_and_back_through_every_control() {
    let mut focus = DialogFocus::Hours;
    let mut seen = Vec::new();
    for _ in 0..6 {
        seen.push(focus);
        focus = focus.next();
    }
    assert_eq!(focus, DialogFocus::Hours);
    assert_eq!(seen.len(), 6);
    assert_eq!(DialogFocus::Hours.previous(), DialogFocus::ScheduleButton);
    assert_eq!(DialogFocus::Text.previous(), DialogFocus::Seconds);
    for focus in seen {
        assert_eq!(focus.next().previous(), focus);
    }
}

#[test]
fn deadline_from_delay_rejects_zero_and_overflow() {
    assert_eq!(deadline_from_delay(10_000, 5), Ok(15_000));
    assert_eq!(
        deadline_from_delay(10_000, 0),
        Err("scheduled input delay must be at least one second".to_string())
    );
    assert_eq!(
        deadline_from_delay(0, u64::MAX),
        Err("scheduled input delay is too large".to_string())
    );
    assert_eq!(
        deadline_from_delay(u64::MAX, 1),
        Err("scheduled input deadline is too large".to_string())
    );
}

#[test]
fn the_executor_sleeps_to_the_nearest_available_deadline_only() {
    let panes = [
        pending(1, 900, false),
        pending(2, 500, true),
        pending(3, 300, true),
    ];
    assert_eq!(nearest_deadline(&panes), Some(300));
    assert_eq!(nearest_deadline(&[pending(1, 5, false)]), None);
    assert_eq!(nearest_deadline(&[]), None);
}

#[test]
fn due_inputs_skip_unavailable_panes_and_run_oldest_first() {
    let panes = [
        pending(1, 400, true),
        pending(2, 100, true),
        pending(3, 50, false),
        pending(4, 900, true),
    ];
    let due: Vec<u64> = due_inputs(&panes, 400)
        .iter()
        .map(|entry| entry.pane_id)
        .collect();
    assert_eq!(due, vec![2, 1]);
    assert!(due_inputs(&panes, 10).is_empty());
}

#[test]
fn a_replacement_survives_the_compare_and_clear_of_a_finished_executor() {
    let original = ScheduledInput {
        execute_at_unix_millis: 1,
        text: "a".to_string(),
        send_enter: true,
    };
    let replacement = ScheduledInput {
        text: "b".to_string(),
        ..original.clone()
    };
    let mut slot = Some(replacement.clone());
    assert!(!clear_if_matches(&mut slot, &original));
    assert_eq!(slot, Some(replacement.clone()));
    assert!(clear_if_matches(&mut slot, &replacement));
    assert_eq!(slot, None);
}

#[test]
fn the_countdown_label_rounds_up_and_redraws_on_the_exact_boundary() {
    assert_eq!(remaining_seconds(10_000, 9_001), 1);
    assert_eq!(remaining_seconds(10_000, 10_000), 0);
    assert_eq!(remaining_seconds(10_000, 20_000), 0);
    // 2.5 s left shows "3"; the label drops to "2" when exactly 2 s remain.
    assert_eq!(next_redraw_unix_millis(10_000, 7_500), Some(8_000));
    // Exactly 2 s left: the next change is a full second away.
    assert_eq!(next_redraw_unix_millis(10_000, 8_000), Some(9_000));
    assert_eq!(next_redraw_unix_millis(10_000, 10_000), None);
}

#[test]
fn enqueue_rejects_blank_prompts_zero_runs_and_a_full_queue() {
    let mut queue = PromptQueue::new();
    assert_eq!(
        queue.enqueue(prompt("  ", Delivery::Once)),
        Err(QueueError::EmptyPrompt)
    );
    assert_eq!(
        queue.enqueue(prompt("x", Delivery::Times { remaining_runs: 0 })),
        Err(QueueError::EmptyPrompt)
    );
    for index in 0..MAXIMUM_PROMPT_QUEUE_LEN {
        queue
            .enqueue(prompt(&format!("p{index}"), Delivery::Once))
            .expect("room");
    }
    assert_eq!(
        queue.enqueue(prompt("one more", Delivery::Once)),
        Err(QueueError::Full(MAXIMUM_PROMPT_QUEUE_LEN))
    );
    queue.clear();
    assert!(queue.is_empty());
}

#[test]
fn acknowledging_a_one_shot_removes_it_and_a_recurring_entry_rotates_to_the_tail() {
    let mut queue = PromptQueue::new();
    let forever = prompt("loop", Delivery::Forever);
    let twice = prompt("twice", Delivery::Times { remaining_runs: 2 });
    let once = prompt("once", Delivery::Once);
    for entry in [&forever, &twice, &once] {
        queue.enqueue(entry.clone()).expect("queued");
    }
    assert!(queue.acknowledge(&forever));
    let texts: Vec<&str> = queue
        .prompts()
        .iter()
        .map(|entry| entry.text.as_str())
        .collect();
    assert_eq!(texts, ["twice", "once", "loop"]);
    assert!(queue.acknowledge(&twice));
    assert_eq!(
        queue.prompts().last().map(|entry| &entry.delivery),
        Some(&Delivery::Times { remaining_runs: 1 })
    );
    let head = queue.next().cloned().expect("head");
    assert!(queue.acknowledge(&head));
    assert_eq!(queue.len(), 3);
    // A stale head is refused and changes nothing.
    assert!(!queue.acknowledge(&prompt("nobody", Delivery::Once)));
    assert_eq!(queue.len(), 3);
}

#[test]
fn the_head_is_marked_attempted_once_and_a_cleared_replacement_cannot_be_marked() {
    let mut queue = PromptQueue::new();
    let head = prompt("a", Delivery::Once);
    queue.enqueue(head.clone()).expect("queued");
    assert!(queue.mark_attempted(&head));
    assert!(!queue.mark_attempted(&head));
    assert!(queue.next().is_some_and(|entry| entry.attempted_delivery));
    queue.clear();
    queue.enqueue(prompt("b", Delivery::Once)).expect("queued");
    assert!(!queue.mark_attempted(&head));
}

#[test]
fn delivery_cycles_through_all_choices_and_never_overflows() {
    let mut choice = Delivery::Once;
    choice = cycle_delivery(&choice, 1);
    assert_eq!(choice, Delivery::Times { remaining_runs: 2 });
    choice = cycle_delivery(&choice, 1);
    assert_eq!(choice, Delivery::Forever);
    choice = cycle_delivery(&choice, 1);
    assert_eq!(choice, Delivery::Once);
    assert_eq!(cycle_delivery(&Delivery::Once, -1), Delivery::Forever);
    for direction in [i8::MIN, i8::MAX, 0] {
        let _ = cycle_delivery(&Delivery::Forever, direction);
    }
}

#[test]
fn the_dialog_requires_text_and_a_positive_whole_run_count() {
    assert!(validated_request("  ", &Delivery::Once, "2").is_err());
    assert_eq!(
        validated_request("go", &Delivery::Times { remaining_runs: 2 }, "5"),
        Ok(("go".to_string(), Delivery::Times { remaining_runs: 5 }))
    );
    assert_eq!(
        validated_request("go", &Delivery::Times { remaining_runs: 2 }, "0"),
        Err("Run count must be a positive whole number".to_string())
    );
    assert!(validated_request("go", &Delivery::Times { remaining_runs: 2 }, "").is_err());
    assert_eq!(
        validated_request("go", &Delivery::Forever, "junk"),
        Ok(("go".to_string(), Delivery::Forever))
    );
}

#[test]
fn repeat_count_steps_and_saturates_and_rejects_an_invalid_draft() {
    assert_eq!(step_times("2", 1), Ok("3".to_string()));
    assert_eq!(step_times("1", -1), Ok("1".to_string()));
    assert_eq!(step_times("4294967295", 1), Ok("4294967295".to_string()));
    assert!(step_times("", 1).is_err());
    assert!(step_times("0", 1).is_err());
    assert!(step_times("4294967296", -1).is_err());
}

#[test]
fn pasted_crlf_becomes_single_newlines() {
    assert_eq!(normalize_paste("a\r\nb\rc\nd"), "a\nb\nc\nd");
}

#[test]
fn only_an_idle_pane_with_an_unattempted_head_and_an_open_epoch_may_send() {
    let mut queue = PromptQueue::new();
    let head = prompt("go", Delivery::Once);
    queue.enqueue(head.clone()).expect("queued");
    let epoch = CompletionEpoch::default();
    let idle = Some(Activity { turn: Turn::Idle });
    let working = Some(Activity {
        turn: Turn::Working,
    });
    assert_eq!(
        next_send(&queue, idle, &epoch, epoch.current()),
        Some(&head)
    );
    assert_eq!(next_send(&queue, working, &epoch, epoch.current()), None);
    assert_eq!(next_send(&queue, None, &epoch, epoch.current()), None);
    assert_eq!(next_send(&queue, idle, &epoch, epoch.current() + 1), None);
    queue.mark_attempted(&head);
    assert_eq!(next_send(&queue, idle, &epoch, epoch.current()), None);
    assert_eq!(
        next_send(&PromptQueue::new(), idle, &epoch, epoch.current()),
        None
    );
}

#[test]
fn repeated_completion_requests_authorize_only_one_queued_prompt_per_turn() {
    let mut epoch = CompletionEpoch::default();
    let completed = epoch.current();
    assert!(epoch.claim(completed));
    assert!(epoch.is_claimed_current(completed));
    assert!(!epoch.claim(completed));
    epoch.retire_submission();
    assert!(!epoch.is_claimed_current(completed));
    assert!(!epoch.claim(epoch.current()));
    epoch.invalidate();
    assert!(!epoch.claim(completed));
    assert!(epoch.claim(epoch.current()));
}

#[test]
fn a_stale_completion_cannot_claim_a_newer_turn() {
    let mut epoch = CompletionEpoch::default();
    let captured = epoch.current();
    epoch.invalidate();
    assert!(!epoch.claim(captured));
    assert!(!epoch.is_claimed_current(captured));
    assert!(epoch.is_available(epoch.current()));
}

#[test]
fn a_busy_mailbox_coalesces_both_request_kinds_without_losing_the_latest() {
    let mut requests = Requests::default();
    requests.request(true, false, 7);
    let in_flight = requests;
    for _ in 0..10_000 {
        requests.request(true, false, 7);
    }
    requests.request(false, true, 7);
    requests.request(true, false, 7);
    assert_eq!(requests.title_checks, 10_002);
    assert_eq!(requests.completions, 1);
    assert_eq!(requests.completion_epoch, 7);
    let outstanding = requests.outstanding(&in_flight);
    assert!(outstanding.title_check && outstanding.completion);
    assert_eq!(outstanding.completion_epoch, 7);
    let caught_up = requests.outstanding(&requests);
    assert!(!caught_up.title_check && !caught_up.completion);
}
