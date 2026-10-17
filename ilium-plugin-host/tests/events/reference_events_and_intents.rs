//! Event delivery, queue rules, payloads, intents and batch refusal
//! (manual page reference-events-and-intents.md; EVT-001 .. EVT-050).

use ilium_plugin_api::generated::types::{
    ActionKind, ActionValue, AgentActivity, ChildExitedEvent, ChildId, DeactivationReason,
    DroppedCount, ErrorCode, Event, InputSource, InputSubmittedEvent, Intent, NodeClass, NodeId,
    Outcome, PaneLifecycleEvent, PaneLifecycleState, PaneTitleChangedEvent, PluginId, PluginSide,
    SettingsChange, SettingsOrigin, TitleOrigin, TopicPublishedEvent, UiAction, UserLineEvent,
    ViewTarget,
};
use ilium_plugin_api::{Side, Tier};
use proptest::prelude::*;

use crate::common::*;

const READ: &[&str] = &[
    "tree.read",
    "panes.read",
    "agent.read",
    "terminal.output-filter",
];

fn read_setup() -> Setup {
    Setup::new(READ)
}

// ---- EVT-001 -------------------------------------------------------------------------

/// REQ: EVT-001
#[test]
fn evt_001_three_events_arrive_in_one_batch_in_order() {
    let env = read_setup().build();
    env.deliver(vec![pane_exited(7), pane_exited(3), pane_exited(5)]);
    let batches = lock(&env.probe.batches).clone();
    let with_exits: Vec<Vec<Event>> = batches
        .into_iter()
        .filter(|batch| batch.iter().any(|event| event.case_name() == "pane-exited"))
        .collect();
    assert_eq!(with_exits.len(), 1, "one batch holds the three events");
    assert_eq!(
        with_exits[0],
        vec![pane_exited(7), pane_exited(3), pane_exited(5)]
    );
}

/// REQ: EVT-001
#[test]
fn evt_001_batch_holds_at_most_256_events_and_backlog_is_split() {
    let env = read_setup().build();
    env.deliver(exits(300));
    let sizes: Vec<usize> = lock(&env.probe.batches)
        .iter()
        .map(|batch| batch.len())
        .collect();
    assert!(
        sizes.iter().all(|size| *size <= 256),
        "batch sizes {sizes:?}"
    );
    assert_eq!(sizes.iter().sum::<usize>(), 300);
    assert_eq!(sizes[0], 256, "the first batch is full");
    assert_eq!(sizes[1], 44, "the rest follows in a second batch");
}

/// REQ: EVT-001
#[test]
fn evt_001_exactly_256_events_fit_one_batch() {
    let env = read_setup().build();
    env.deliver(exits(256));
    let sizes: Vec<usize> = lock(&env.probe.batches)
        .iter()
        .map(|batch| batch.len())
        .collect();
    assert_eq!(sizes, vec![256]);
}

/// REQ: EVT-001
#[test]
fn evt_001_257_events_need_two_batches() {
    let env = read_setup().build();
    env.deliver(exits(257));
    let sizes: Vec<usize> = lock(&env.probe.batches)
        .iter()
        .map(|batch| batch.len())
        .collect();
    assert_eq!(sizes, vec![256, 1]);
}

/// REQ: EVT-001
#[test]
fn evt_001_order_of_different_kinds_is_emission_order() {
    let env = read_setup().build();
    let sent = vec![
        pane_exited(4),
        activity_changed(2, 1),
        Event::PaneFocused(Some(NodeId(2))),
        output_matched(1),
        pane_exited(9),
    ];
    env.deliver(sent.clone());
    assert_eq!(env.probe.events(), sent);
}

/// REQ: EVT-001
#[test]
fn evt_001_per_node_order_created_exited_removed() {
    let env = read_setup().build();
    let created = Event::PaneCreated(ilium_plugin_api::generated::types::PaneInfo {
        node: NodeId(8),
        content_kind: "ilium.terminal".to_owned(),
        primitive: ilium_plugin_api::generated::types::PrimitiveKind::Terminal,
        columns: 80,
        rows: 24,
        focused: false,
        title: "t".to_owned(),
        is_restored: false,
    });
    env.deliver(vec![created.clone(), pane_exited(8), node_removed(8)]);
    let kinds: Vec<&str> = env
        .probe
        .events()
        .iter()
        .map(|event| event.case_name())
        .collect();
    assert_eq!(kinds, vec!["pane-created", "pane-exited", "node-removed"]);
}

/// REQ: EVT-001
#[test]
fn evt_001_agent_identified_precedes_activity_changed() {
    let env = read_setup().build();
    env.deliver(vec![
        Event::AgentIdentified(ilium_plugin_api::generated::types::AgentIdentifiedEvent {
            node: NodeId(2),
            signature: "claude-code".to_owned(),
        }),
        activity_changed(2, 1),
    ]);
    let kinds: Vec<&str> = env
        .probe
        .events()
        .iter()
        .map(|event| event.case_name())
        .collect();
    assert_eq!(kinds, vec!["agent-identified", "agent-activity-changed"]);
}

/// REQ: EVT-001
#[test]
fn evt_001_one_call_at_a_time_batches_are_sequential() {
    let env = read_setup().build();
    env.deliver(exits(600));
    let sequence = lock(&env.probe.sequence).clone();
    let calls = sequence
        .iter()
        .filter(|name| name.as_str() == "on-events")
        .count();
    assert_eq!(
        calls, 3,
        "600 events are three sequential calls: {sequence:?}"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]

    // REQ: EVT-001
    #[test]
    fn evt_001_any_backlog_is_delivered_in_order_in_batches_of_at_most_256(count in 1u64..700) {
        let env = read_setup().build();
        env.deliver(exits(count));
        let received = env.probe.events();
        prop_assert_eq!(received, exits(count));
        let max = lock(&env.probe.batches).iter().map(|batch| batch.len()).max().unwrap_or(0);
        prop_assert!(max <= 256);
    }
}

// ---- EVT-002 -------------------------------------------------------------------------

/// REQ: EVT-002
#[test]
fn evt_002_without_panes_read_no_pane_events_arrive() {
    let env = Setup::new(&[]).build();
    env.deliver(vec![
        pane_exited(2),
        pane_resized(2, 80),
        Event::PaneFocused(None),
    ]);
    assert_eq!(env.probe.count_of("pane-exited"), 0);
    assert_eq!(env.probe.count_of("pane-resized"), 0);
    assert_eq!(env.probe.count_of("pane-focused"), 0);
}

/// REQ: EVT-002
#[test]
fn evt_002_with_panes_read_pane_events_arrive() {
    let env = Setup::new(&["panes.read"]).build();
    env.deliver(vec![pane_exited(2)]);
    assert_eq!(env.probe.count_of("pane-exited"), 1);
}

/// REQ: EVT-002
#[test]
fn evt_002_other_grant_does_not_open_output_matched() {
    let env = Setup::new(&["panes.read"]).build();
    env.deliver(vec![output_matched(1)]);
    assert_eq!(env.probe.count_of("output-matched"), 0);
}

/// REQ: EVT-002
#[test]
fn evt_002_own_events_need_no_grant() {
    let env = Setup::new(&[]).build();
    env.deliver(vec![trigger()]);
    assert_eq!(env.probe.count_of("timer-fired"), 1);
}

/// REQ: EVT-002
#[test]
fn evt_002_agent_events_need_agent_read() {
    let env = Setup::new(&["panes.read"]).build();
    env.deliver(vec![activity_changed(2, 1)]);
    assert_eq!(env.probe.count_of("agent-activity-changed"), 0);
}

/// REQ: EVT-002
#[test]
fn evt_002_prompt_events_need_agent_read_prompts_not_agent_read() {
    let env = Setup::new(&["agent.read"]).build();
    env.deliver(vec![Event::InputSubmitted(InputSubmittedEvent {
        node: NodeId(2),
        text: "secret".to_owned(),
        source: InputSource::User,
        epoch: 1,
    })]);
    assert_eq!(env.probe.count_of("input-submitted"), 0);
}

// ---- EVT-003 -------------------------------------------------------------------------

/// REQ: EVT-003
#[test]
fn evt_003_blocking_100_ms_in_on_events_traps_deadline() {
    let env = read_setup().build();
    *lock(&env.probe.block_millis) = 100;
    env.deliver(vec![pane_exited(2)]);
    let failures = env.host.failures();
    assert_eq!(failures.len(), 1, "one breach is one failure: {failures:?}");
    assert_eq!(failures[0].code.as_str(), "trap.deadline");
    assert_eq!(failures[0].function, "on-events");
}

/// REQ: EVT-003
#[test]
fn evt_003_a_fast_call_does_not_trap() {
    let env = read_setup().build();
    *lock(&env.probe.block_millis) = 1;
    env.deliver(vec![pane_exited(2)]);
    assert!(env.host.failures().is_empty());
}

/// REQ: EVT-003
#[test]
fn evt_003_one_breach_counts_as_one_failure_not_one_per_event() {
    let env = read_setup().build();
    *lock(&env.probe.block_millis) = 100;
    env.deliver(exits(10));
    assert_eq!(env.host.failures().len(), 1);
}

// ---- EVT-004 -------------------------------------------------------------------------

/// REQ: EVT-004
#[test]
fn evt_004_overflow_with_output_matches_puts_events_dropped_first_with_a_count() {
    let env = read_setup().build();
    env.deliver(outputs(1100));
    let batches = lock(&env.probe.batches).clone();
    let first = &batches[0];
    match &first[0] {
        Event::EventsDropped(records) => assert_eq!(
            records,
            &vec![DroppedCount {
                event: "output-matched".to_owned(),
                source: Some("f".to_owned()),
                count: 76,
            }]
        ),
        other => panic!("first event was {other:?}"),
    }
}

/// REQ: EVT-004
#[test]
fn evt_004_the_oldest_drop_oldest_event_is_evicted() {
    let env = read_setup().build();
    env.deliver(outputs(1100));
    let rows: Vec<u32> = env
        .probe
        .events_of("output-matched")
        .into_iter()
        .filter_map(|event| match event {
            Event::OutputMatched(matched) => Some(matched.row),
            _ => None,
        })
        .collect();
    assert_eq!(rows.len(), 1024);
    assert_eq!(rows[0], 76, "rows 0..=75 were evicted");
    assert_eq!(rows[1023], 1099);
}

/// REQ: EVT-004
#[test]
fn evt_004_exactly_full_queue_drops_nothing() {
    let env = read_setup().build();
    env.deliver(outputs(1024));
    assert_eq!(env.probe.count_of("events-dropped"), 0);
    assert_eq!(env.probe.count_of("output-matched"), 1024);
}

/// REQ: EVT-004
#[test]
fn evt_004_one_event_over_drops_exactly_one() {
    let env = read_setup().build();
    env.deliver(outputs(1025));
    let records = dropped_records(&env.probe.events());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].count, 1);
}

/// REQ: EVT-004
#[test]
fn evt_004_eviction_takes_the_oldest_drop_oldest_event_of_any_kind() {
    let env = Setup::new(&["terminal.output-filter", "topics.subscribe"]).build();
    let mut events = vec![Event::TopicPublished(TopicPublishedEvent {
        topic: "build".to_owned(),
        from: PluginId("other.plugin".to_owned()),
        payload_json: "{}".to_owned(),
    })];
    events.extend(outputs(1023));
    events.push(output_matched(5000));
    env.deliver(events);
    let records = dropped_records(&env.probe.events());
    assert_eq!(
        records,
        vec![DroppedCount {
            event: "topic-published".to_owned(),
            source: Some("build".to_owned()),
            count: 1,
        }]
    );
}

/// REQ: EVT-004
#[test]
fn evt_004_incoming_drop_oldest_is_discarded_and_counted_when_none_is_pending() {
    let env = read_setup().build();
    let mut events = exits(1024);
    events.push(output_matched(1));
    env.deliver(events);
    let records = dropped_records(&env.probe.events());
    assert_eq!(
        records,
        vec![DroppedCount {
            event: "output-matched".to_owned(),
            source: Some("f".to_owned()),
            count: 1,
        }]
    );
    assert_eq!(env.probe.count_of("output-matched"), 0);
    assert_eq!(
        env.probe.activation_count(),
        1,
        "no restart for a droppable event"
    );
}

/// REQ: EVT-004
#[test]
fn evt_004_a_coalesced_event_with_a_pending_key_never_evicts() {
    let env = read_setup().build();
    let mut events = vec![pane_resized(9, 80)];
    events.extend(outputs(1023));
    events.push(pane_resized(9, 100));
    env.deliver(events);
    assert_eq!(env.probe.count_of("events-dropped"), 0);
    assert_eq!(env.probe.count_of("output-matched"), 1023);
}

/// REQ: EVT-004
#[test]
fn evt_004_never_dropped_event_on_a_full_queue_without_droppable_restarts_the_instance() {
    let env = read_setup().tree(flat_tree(2)).build();
    let before = env.probe.activation_count();
    let mut events = exits(1024);
    events.push(pane_exited(2000));
    env.deliver(events);
    assert_eq!(
        env.probe.activation_count(),
        before + 1,
        "the instance restarted"
    );
}

/// REQ: EVT-004
#[test]
fn evt_004_the_new_instance_gets_tree_changed_for_every_node_after_the_restart() {
    let env = read_setup().tree(flat_tree(2)).build();
    let mut events = exits(1024);
    events.push(pane_exited(2000));
    env.deliver(events);
    env.deliver(vec![trigger()]);
    let lists = tree_changed_lists(&env.probe.events());
    assert!(
        lists.len() >= 2,
        "initial and restart tree-changed: {lists:?}"
    );
    assert_eq!(lists.last().cloned(), Some(vec![1, 2, 3]));
}

// ---- EVT-005 -------------------------------------------------------------------------

fn lifecycle_event(state: PaneLifecycleState, by: Option<&str>) -> Event {
    Event::PaneLifecycleChanged(PaneLifecycleEvent {
        pane: NodeId(2),
        state,
        by: by.map(|id| PluginId(id.to_owned())),
    })
}

/// REQ: EVT-005
#[test]
fn evt_005_pane_lifecycle_changed_carries_pane_state_and_by() {
    let env = read_setup().build();
    let sent = lifecycle_event(PaneLifecycleState::Frozen, Some("demo.guard"));
    env.deliver(vec![sent.clone()]);
    assert_eq!(env.probe.events_of("pane-lifecycle-changed"), vec![sent]);
}

/// REQ: EVT-005
#[test]
fn evt_005_pane_lifecycle_changed_is_never_dropped_on_a_full_queue() {
    let env = read_setup().build();
    let mut events = outputs(1030);
    events.push(lifecycle_event(PaneLifecycleState::Exited, None));
    env.deliver(events);
    assert_eq!(env.probe.count_of("pane-lifecycle-changed"), 1);
    assert_eq!(env.probe.activation_count(), 1);
}

/// REQ: EVT-005
#[test]
fn evt_005_freezing_a_pane_produces_one_event_with_state_frozen_and_by_set() {
    let env = Setup::new(&["panes.read", "panes.lifecycle"])
        .tree(flat_tree(1))
        .build();
    env.run(vec![sub(1, freeze(2))]);
    env.settle();
    let events = env.probe.events_of("pane-lifecycle-changed");
    assert_eq!(events.len(), 1);
    match &events[0] {
        Event::PaneLifecycleChanged(changed) => {
            assert_eq!(changed.state, PaneLifecycleState::Frozen);
            assert_eq!(changed.pane, NodeId(2));
            assert_eq!(changed.by, Some(env.plugin()));
        }
        other => panic!("unexpected {other:?}"),
    }
}

/// REQ: EVT-005
#[test]
fn evt_005_every_lifecycle_state_is_carried_unchanged() {
    let env = read_setup().build();
    let states = [
        PaneLifecycleState::Running,
        PaneLifecycleState::Frozen,
        PaneLifecycleState::Restarting,
        PaneLifecycleState::Exited,
    ];
    let sent: Vec<Event> = states
        .iter()
        .map(|state| lifecycle_event(*state, None))
        .collect();
    env.deliver(sent.clone());
    assert_eq!(env.probe.events_of("pane-lifecycle-changed"), sent);
}

// ---- EVT-006 -------------------------------------------------------------------------

fn ui_action(key: &str, kind: ActionKind, value: Option<ActionValue>) -> Event {
    Event::UiAction(UiAction {
        view: ViewTarget::Panel("main".to_owned()),
        key: key.to_owned(),
        action_id: "act".to_owned(),
        kind,
        value,
    })
}

/// REQ: EVT-006
#[test]
fn evt_006_click_on_save_button_gives_ui_action_with_key_save() {
    let env = Setup::new(&[]).build();
    let sent = ui_action("save", ActionKind::Activated, None);
    env.deliver(vec![sent.clone()]);
    assert_eq!(env.probe.events_of("ui-action"), vec![sent]);
}

/// REQ: EVT-006
#[test]
fn evt_006_value_kinds_match_the_widget_kind() {
    let env = Setup::new(&[]).build();
    let sent = vec![
        ui_action(
            "name",
            ActionKind::Changed,
            Some(ActionValue::Text("abc".to_owned())),
        ),
        ui_action("flag", ActionKind::Toggled, Some(ActionValue::Flag(true))),
        ui_action("list", ActionKind::Selected, Some(ActionValue::Index(2))),
        ui_action("size", ActionKind::Changed, Some(ActionValue::Number(1.5))),
        ui_action(
            "pick",
            ActionKind::Selected,
            Some(ActionValue::Ids(vec!["a".to_owned()])),
        ),
    ];
    env.deliver(sent.clone());
    assert_eq!(env.probe.events_of("ui-action"), sent);
}

/// REQ: EVT-006
#[test]
fn evt_006_ui_action_is_never_dropped_on_a_full_queue() {
    let env = Setup::new(&["terminal.output-filter"]).build();
    let mut events = outputs(1030);
    events.push(ui_action("save", ActionKind::Activated, None));
    env.deliver(events);
    assert_eq!(env.probe.count_of("ui-action"), 1);
}

/// REQ: EVT-006
#[test]
fn evt_006_scrolled_actions_for_one_key_coalesce_to_the_newest() {
    let env = Setup::new(&[]).build();
    env.deliver(vec![
        ui_action("log", ActionKind::Scrolled, Some(ActionValue::Index(1))),
        ui_action("log", ActionKind::Scrolled, Some(ActionValue::Index(2))),
        ui_action("log", ActionKind::Scrolled, Some(ActionValue::Index(3))),
    ]);
    assert_eq!(
        env.probe.events_of("ui-action"),
        vec![ui_action(
            "log",
            ActionKind::Scrolled,
            Some(ActionValue::Index(3))
        )]
    );
}

// ---- EVT-010 -------------------------------------------------------------------------

/// REQ: EVT-010
#[test]
fn evt_010_a_send_input_submission_gets_one_request_result() {
    let env = Setup::new(&["terminal.write"]).tree(flat_tree(1)).build();
    env.run(vec![keyed(7, "k:1", send_input(2, "ls\n"))]);
    let results = env.outcomes();
    assert_eq!(count_for(&results, 7), 1);
}

/// REQ: EVT-010
#[test]
fn evt_010_the_result_reaches_the_guest_as_request_completed() {
    let env = Setup::new(&["terminal.write"]).tree(flat_tree(1)).build();
    env.run(vec![keyed(7, "k:1", send_input(2, "ls\n"))]);
    env.settle();
    let completed = env.probe.events_of("request-completed");
    assert_eq!(completed.len(), 1);
}

/// REQ: EVT-010
#[test]
fn evt_010_no_wit_host_interface_exposes_an_intent_as_a_function() {
    let host_dir = workspace_root().join("wit/host");
    let entries = std::fs::read_dir(&host_dir)
        .map(|dir| dir.count())
        .unwrap_or(0);
    assert!(entries > 0, "wit/host is readable");
    for case in Intent::CASE_NAMES {
        for entry in std::fs::read_dir(&host_dir).expect("wit/host").flatten() {
            let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
            let declaration = format!("{case}: func(");
            let is_submit_only = entry.file_name().to_string_lossy() == "intents-host.wit";
            assert!(
                !text.contains(&declaration) || is_submit_only,
                "{} declares the intent {case} as a function",
                entry.path().display()
            );
        }
    }
}

/// REQ: EVT-010
#[test]
fn evt_010_the_only_host_function_taking_a_submission_is_intents_host_submit() {
    let text = wit_file("host/intents-host.wit");
    assert!(text.contains("submit: func(submission: submission)"));
    let functions = text.lines().filter(|line| line.contains(": func(")).count();
    assert_eq!(functions, 1);
}

/// REQ: EVT-010
#[test]
fn evt_010_ten_intents_give_ten_results() {
    let env = Setup::new(&["tree.write"]).tree(flat_tree(10)).build();
    let submissions = (0..10)
        .map(|index| sub(index + 1, rename(index + 2, "t")))
        .collect();
    env.run(submissions);
    assert_eq!(env.outcomes().len(), 10);
}

// ---- EVT-011 -------------------------------------------------------------------------

/// REQ: EVT-011
#[test]
fn evt_011_freeze_pane_with_a_non_pane_node_id_is_refused_invalid_argument() {
    let mut nodes = flat_tree(1);
    nodes.push(folder(9, Some(1)));
    let env = Setup::new(&["panes.lifecycle"]).tree(nodes).build();
    env.run(vec![sub(1, freeze(9))]);
    assert!(
        is_refused(&env.outcome(1), ErrorCode::InvalidArgument),
        "{:?}",
        env.outcome(1)
    );
}

/// REQ: EVT-011
#[test]
fn evt_011_an_invalid_value_refuses_only_that_submission() {
    let mut nodes = flat_tree(2);
    nodes.push(folder(9, Some(1)));
    let env = Setup::new(&["panes.lifecycle", "tree.write"])
        .tree(nodes)
        .build();
    env.run(vec![sub(1, freeze(9)), sub(2, rename(2, "ok"))]);
    assert!(is_refused(&env.outcome(1), ErrorCode::InvalidArgument));
    assert!(
        is_ok(&env.outcome(2)),
        "the sibling is applied: {:?}",
        env.outcome(2)
    );
}

/// REQ: EVT-011
#[test]
fn evt_011_malformed_shape_is_the_lowest_index_offender_and_siblings_are_cancelled() {
    let env = Setup::new(&["tree.write", "terminal.write"])
        .tree(flat_tree(2))
        .build();
    env.run(vec![
        sub(1, rename(2, "a")),
        sub(2, send_input(2, "x")),
        sub(3, send_input(3, "y")),
    ]);
    assert!(is_refused(&env.outcome(2), ErrorCode::InvalidArgument));
    assert!(is_refused(&env.outcome(1), ErrorCode::Cancelled));
    assert!(is_refused(&env.outcome(3), ErrorCode::Cancelled));
    assert!(
        env.applied().is_empty(),
        "nothing of a refused batch is applied"
    );
}

/// REQ: EVT-011
#[test]
fn evt_011_siblings_complete_as_refused_cancelled_never_as_cancelled() {
    let env = Setup::new(&["tree.write", "terminal.write"])
        .tree(flat_tree(2))
        .build();
    env.run(vec![sub(1, rename(2, "a")), sub(2, send_input(2, "x"))]);
    assert!(!matches!(env.outcome(1), Some(Outcome::Cancelled)));
    assert!(is_refused(&env.outcome(1), ErrorCode::Cancelled));
}

/// REQ: EVT-011
#[test]
fn evt_011_the_offender_is_at_index_zero() {
    let env = Setup::new(&["tree.write", "terminal.write"])
        .tree(flat_tree(2))
        .build();
    env.run(vec![sub(1, send_input(2, "x")), sub(2, rename(2, "a"))]);
    assert!(is_refused(&env.outcome(1), ErrorCode::InvalidArgument));
    assert!(is_refused(&env.outcome(2), ErrorCode::Cancelled));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10))]

    // REQ: EVT-011
    #[test]
    fn evt_011_only_the_offender_is_invalid_argument_others_cancelled(count in 2u64..8, offender in 0u64..8) {
        let offender = offender % count;
        let env = Setup::new(&["tree.write", "terminal.write"]).tree(flat_tree(2)).build();
        let submissions = (0..count)
            .map(|index| {
                if index == offender { sub(index + 1, send_input(2, "x")) } else { sub(index + 1, rename(2, "a")) }
            })
            .collect();
        env.run(submissions);
        for index in 0..count {
            let outcome = env.outcome(index + 1);
            if index == offender {
                prop_assert!(is_refused(&outcome, ErrorCode::InvalidArgument));
            } else {
                prop_assert!(is_refused(&outcome, ErrorCode::Cancelled));
            }
        }
    }
}

// ---- EVT-012 -------------------------------------------------------------------------

/// REQ: EVT-012
#[test]
fn evt_012_freeze_pane_without_panes_lifecycle_is_refused_not_permitted() {
    let env = Setup::new(&["tree.write"]).tree(flat_tree(1)).build();
    env.run(vec![sub(1, freeze(2))]);
    assert!(
        is_refused(&env.outcome(1), ErrorCode::NotPermitted),
        "{:?}",
        env.outcome(1)
    );
}

/// REQ: EVT-012
#[test]
fn evt_012_siblings_of_a_grant_offender_are_refused_cancelled_and_nothing_is_applied() {
    let env = Setup::new(&["tree.write"]).tree(flat_tree(2)).build();
    env.run(vec![
        sub(1, rename(2, "a")),
        sub(2, freeze(3)),
        sub(3, rename(3, "b")),
    ]);
    assert!(is_refused(&env.outcome(2), ErrorCode::NotPermitted));
    assert!(is_refused(&env.outcome(1), ErrorCode::Cancelled));
    assert!(is_refused(&env.outcome(3), ErrorCode::Cancelled));
    assert!(env.applied().is_empty());
}

/// REQ: EVT-012
#[test]
fn evt_012_with_the_grant_the_same_intent_is_applied() {
    let env = Setup::new(&["panes.lifecycle"]).tree(flat_tree(1)).build();
    env.run(vec![sub(1, freeze(2))]);
    assert!(is_ok(&env.outcome(1)), "{:?}", env.outcome(1));
}

/// REQ: EVT-012
#[test]
fn evt_012_the_lowest_index_grant_offender_is_reported() {
    let env = Setup::new(&[]).tree(flat_tree(2)).build();
    env.run(vec![sub(1, rename(2, "a")), sub(2, rename(3, "b"))]);
    assert!(is_refused(&env.outcome(1), ErrorCode::NotPermitted));
    assert!(is_refused(&env.outcome(2), ErrorCode::Cancelled));
}

/// REQ: EVT-012
#[test]
fn evt_012_a_shape_offender_at_a_lower_index_is_reported_before_a_grant_offender() {
    let env = Setup::new(&["tree.write"]).tree(flat_tree(2)).build();
    env.run(vec![sub(1, send_input(2, "x")), sub(2, freeze(3))]);
    assert!(is_refused(&env.outcome(1), ErrorCode::InvalidArgument));
    assert!(is_refused(&env.outcome(2), ErrorCode::Cancelled));
}

// ---- EVT-013 -------------------------------------------------------------------------

/// REQ: EVT-013
#[test]
fn evt_013_rename_of_a_missing_node_is_failed_not_found() {
    let env = Setup::new(&["tree.write"]).tree(flat_tree(1)).build();
    env.run(vec![sub(1, rename(99, "x"))]);
    assert!(
        is_failed(&env.outcome(1), ErrorCode::NotFound),
        "{:?}",
        env.outcome(1)
    );
}

/// REQ: EVT-013
#[test]
fn evt_013_set_node_locked_of_a_missing_node_is_refused_not_found() {
    let env = Setup::new(&["tree.write"]).tree(flat_tree(1)).build();
    env.run(vec![sub(1, set_locked(99, true))]);
    assert!(
        is_refused(&env.outcome(1), ErrorCode::NotFound),
        "{:?}",
        env.outcome(1)
    );
}

/// REQ: EVT-013
#[test]
fn evt_013_a_pane_intent_for_a_removed_node_reports_not_found() {
    let env = Setup::new(&["panes.lifecycle"]).tree(flat_tree(2)).build();
    env.kernel.set_tree(flat_tree(1));
    env.run(vec![sub(1, freeze(3))]);
    assert_eq!(
        code_of(&env.outcome(1)),
        Some(ErrorCode::NotFound),
        "{:?}",
        env.outcome(1)
    );
}

/// REQ: EVT-013
#[test]
fn evt_013_an_existing_node_is_not_reported_missing() {
    let env = Setup::new(&["tree.write"]).tree(flat_tree(1)).build();
    env.run(vec![sub(1, rename(2, "x"))]);
    assert!(is_ok(&env.outcome(1)));
}

// ---- EVT-014 -------------------------------------------------------------------------

/// REQ: EVT-014
#[test]
fn evt_014_outcome_has_exactly_the_seven_cases_in_order() {
    assert_eq!(
        Outcome::CASE_NAMES,
        &[
            "ok",
            "ok-node",
            "duplicate",
            "refused",
            "failed",
            "cancelled",
            "ok-epoch"
        ]
    );
}

/// REQ: EVT-014
#[test]
fn evt_014_wit_outcome_variant_lists_the_seven_cases() {
    let wit = wit_file("types/ilium-types.wit");
    let start = wit
        .find("variant outcome")
        .expect("variant outcome in the WIT");
    let body = &wit[start..];
    let end = body.find('}').expect("end of variant");
    let block = &body[..end];
    for case in [
        "ok",
        "ok-node",
        "duplicate",
        "refused",
        "failed",
        "cancelled",
        "ok-epoch",
    ] {
        let has_case = block.lines().skip(1).any(|line| {
            let trimmed = line.trim().trim_start_matches('/').trim();
            trimmed.starts_with(case)
                && trimmed[case.len()..].starts_with(|c: char| c == ',' || c == '(' || c == ' ')
        });
        assert!(has_case, "case {case} missing in: {block}");
    }
}

/// REQ: EVT-014
#[test]
fn evt_014_case_names_round_trip_through_case_name() {
    let samples = vec![
        Outcome::Ok,
        Outcome::OkNode(NodeId(1)),
        Outcome::Cancelled,
        Outcome::OkEpoch(3),
    ];
    for sample in samples {
        assert!(Outcome::CASE_NAMES.contains(&sample.case_name()));
    }
}

// ---- EVT-020 .. EVT-022 ----------------------------------------------------------------

/// REQ: EVT-020
#[test]
fn evt_020_send_input_to_a_restored_pane_is_refused_pane_not_ready() {
    let env = Setup::new(&["terminal.write"]).tree(flat_tree(1)).build();
    env.kernel.set_pane_restored(NodeId(2), true);
    env.run(vec![keyed(1, "k:1", send_input(2, "x"))]);
    assert!(
        is_refused(&env.outcome(1), ErrorCode::PaneNotReady),
        "{:?}",
        env.outcome(1)
    );
}

/// REQ: EVT-020
#[test]
fn evt_020_send_input_to_a_started_pane_is_accepted() {
    let env = Setup::new(&["terminal.write"]).tree(flat_tree(1)).build();
    env.kernel.set_pane_restored(NodeId(2), false);
    env.run(vec![keyed(1, "k:1", send_input(2, "x"))]);
    assert!(is_ok(&env.outcome(1)));
}

/// REQ: EVT-020
#[test]
fn evt_020_signal_pane_to_a_restored_pane_is_also_pane_not_ready() {
    let env = Setup::new(&["panes.signal"]).tree(flat_tree(1)).build();
    env.kernel.set_pane_restored(NodeId(2), true);
    let signal = Intent::SignalPane(ilium_plugin_api::generated::types::SignalPaneSpec {
        pane: NodeId(2),
        signal: ilium_plugin_api::generated::types::Signal::Interrupt,
    });
    env.run(vec![sub(1, signal)]);
    assert!(is_refused(&env.outcome(1), ErrorCode::PaneNotReady));
}

/// REQ: EVT-021
#[test]
fn evt_021_send_input_without_a_key_refuses_the_batch() {
    let env = Setup::new(&["terminal.write", "tree.write"])
        .tree(flat_tree(2))
        .build();
    env.run(vec![sub(1, send_input(2, "x")), sub(2, rename(3, "a"))]);
    assert!(is_refused(&env.outcome(1), ErrorCode::InvalidArgument));
    assert!(is_refused(&env.outcome(2), ErrorCode::Cancelled));
}

/// REQ: EVT-021
#[test]
fn evt_021_agent_action_without_a_key_refuses_the_batch() {
    let env = Setup::new(&["agent.control", "tree.write"])
        .tree(flat_tree(2))
        .build();
    env.run(vec![
        sub(1, agent_action(2, "approve")),
        sub(2, rename(3, "a")),
    ]);
    assert!(is_refused(&env.outcome(1), ErrorCode::InvalidArgument));
    assert!(is_refused(&env.outcome(2), ErrorCode::Cancelled));
}

/// REQ: EVT-021
#[test]
fn evt_021_send_input_with_a_key_is_accepted() {
    let env = Setup::new(&["terminal.write"]).tree(flat_tree(1)).build();
    env.run(vec![keyed(1, "send:1", send_input(2, "x"))]);
    assert!(is_ok(&env.outcome(1)));
}

/// REQ: EVT-021
#[test]
fn evt_021_other_intents_need_no_key() {
    let env = Setup::new(&["tree.write"]).tree(flat_tree(1)).build();
    env.run(vec![sub(1, rename(2, "x"))]);
    assert!(is_ok(&env.outcome(1)));
}

/// REQ: EVT-022
#[test]
fn evt_022_journal_entry_is_written_before_the_keys_reach_the_pane() {
    let env = Setup::new(&["terminal.write"]).tree(flat_tree(1)).build();
    env.run(vec![keyed(1, "send:1", send_input_journaled(2, "x"))]);
    let steps = env.kernel.step_log();
    let journal = steps.iter().position(|step| step == "journal:send:1");
    let write = steps
        .iter()
        .position(|step| step.starts_with("pane-write:2"));
    assert!(journal.is_some() && write.is_some(), "steps {steps:?}");
    assert!(journal < write, "journal before write: {steps:?}");
}

/// REQ: EVT-022
#[test]
fn evt_022_restart_between_journal_and_outcome_gives_input_uncertain_and_no_replay() {
    let mut env = Setup::new(&["terminal.write"]).tree(flat_tree(1)).build();
    env.kernel.interrupt_after_journal(NodeId(2), "send:1");
    env.run(vec![keyed(1, "send:1", send_input_journaled(2, "x"))]);
    let writes_before = env
        .kernel
        .step_log()
        .iter()
        .filter(|s| s.starts_with("pane-write"))
        .count();
    env.host.restart_server();
    env.settle();
    assert_eq!(env.probe.count_of("input-uncertain"), 1);
    let writes_after = env
        .kernel
        .step_log()
        .iter()
        .filter(|s| s.starts_with("pane-write"))
        .count();
    assert_eq!(writes_before, writes_after, "no replay");
}

/// REQ: EVT-022
#[test]
fn evt_022_without_journal_true_no_journal_entry_is_written() {
    let env = Setup::new(&["terminal.write"]).tree(flat_tree(1)).build();
    env.run(vec![keyed(1, "send:1", send_input(2, "x"))]);
    let steps = env.kernel.step_log();
    assert!(
        !steps.iter().any(|step| step.starts_with("journal:")),
        "{steps:?}"
    );
}

// ---- EVT-030, EVT-031 ------------------------------------------------------------------

fn settings_changed(keys: &[&str], origin: SettingsOrigin) -> Event {
    Event::SettingsChanged(SettingsChange {
        keys: keys.iter().map(|key| (*key).to_owned()).collect(),
        origin,
        side: PluginSide::Server,
    })
}

/// REQ: EVT-030
#[test]
fn evt_030_two_changes_in_one_window_give_one_event_with_both_keys() {
    let env = Setup::new(&[]).build();
    env.host.deliver(vec![
        settings_changed(&["width"], SettingsOrigin::User),
        settings_changed(&["color"], SettingsOrigin::User),
    ]);
    env.settle();
    let events = env.probe.events_of("settings-changed");
    assert_eq!(events.len(), 1);
    match &events[0] {
        Event::SettingsChanged(change) => {
            let mut keys = change.keys.clone();
            keys.sort();
            assert_eq!(keys, vec!["color".to_owned(), "width".to_owned()]);
        }
        other => panic!("unexpected {other:?}"),
    }
}

/// REQ: EVT-030
#[test]
fn evt_030_the_last_origin_is_kept() {
    let env = Setup::new(&[]).build();
    env.host.deliver(vec![
        settings_changed(&["a"], SettingsOrigin::User),
        settings_changed(&["b"], SettingsOrigin::Plugin),
    ]);
    env.settle();
    match &env.probe.events_of("settings-changed")[0] {
        Event::SettingsChanged(change) => assert_eq!(change.origin, SettingsOrigin::Plugin),
        other => panic!("unexpected {other:?}"),
    }
}

/// REQ: EVT-030
#[test]
fn evt_030_a_change_after_delivery_is_a_new_event() {
    let mut env = Setup::new(&[]).build();
    env.deliver(vec![settings_changed(&["a"], SettingsOrigin::User)]);
    env.host.advance_clock(std::time::Duration::from_secs(5));
    env.deliver(vec![settings_changed(&["b"], SettingsOrigin::User)]);
    assert_eq!(env.probe.count_of("settings-changed"), 2);
}

/// REQ: EVT-030
#[test]
fn evt_030_a_repeated_key_is_listed_once() {
    let env = Setup::new(&[]).build();
    env.host.deliver(vec![
        settings_changed(&["a"], SettingsOrigin::User),
        settings_changed(&["a"], SettingsOrigin::User),
    ]);
    env.settle();
    match &env.probe.events_of("settings-changed")[0] {
        Event::SettingsChanged(change) => assert_eq!(change.keys, vec!["a".to_owned()]),
        other => panic!("unexpected {other:?}"),
    }
}

/// REQ: EVT-031
#[test]
fn evt_031_cli_cancelled_is_a_distinct_event_from_a_child_exit_with_a_signal() {
    let env = Setup::new(&["cli.contribute", "process.long-lived"]).build();
    env.deliver(vec![
        Event::CliCancelled(5),
        Event::ChildExited(ChildExitedEvent {
            child: ChildId(1),
            status: None,
            signal: Some("SIGINT".to_owned()),
        }),
    ]);
    assert_eq!(env.probe.count_of("cli-cancelled"), 1);
    assert_eq!(env.probe.count_of("child-exited"), 1);
    assert_eq!(
        env.probe.events_of("cli-cancelled"),
        vec![Event::CliCancelled(5)]
    );
}

/// REQ: EVT-031
#[test]
fn evt_031_cli_cancelled_is_never_dropped() {
    let env = Setup::new(&["cli.contribute", "terminal.output-filter"]).build();
    let mut events = outputs(1030);
    events.push(Event::CliCancelled(9));
    env.deliver(events);
    assert_eq!(env.probe.count_of("cli-cancelled"), 1);
}

/// REQ: EVT-031
#[test]
fn evt_031_cli_cancelled_needs_cli_contribute() {
    let env = Setup::new(&[]).build();
    env.deliver(vec![Event::CliCancelled(5)]);
    assert_eq!(env.probe.count_of("cli-cancelled"), 0);
}

// ---- EVT-040 .. EVT-042 -----------------------------------------------------------------

/// REQ: EVT-040
#[test]
fn evt_040_a_plugin_sent_prompt_fires_input_submitted_only() {
    let env = Setup::new(&["terminal.write", "agent.read-prompts"])
        .tree(vec![
            node(1, None, NodeClass::Container, "root"),
            agent_pane(2, Some(1), AgentActivity::Idle),
        ])
        .build();
    env.run(vec![keyed(1, "p:1", send_input(2, "do it\n"))]);
    env.settle();
    let submitted = env.probe.events_of("input-submitted");
    assert_eq!(submitted.len(), 1);
    match &submitted[0] {
        Event::InputSubmitted(prompt) => assert_eq!(prompt.source, InputSource::Plugin),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(env.probe.count_of("user-line-submitted"), 0);
}

/// REQ: EVT-040
#[test]
fn evt_040_a_user_typed_prompt_fires_input_submitted_then_user_line_submitted() {
    let env = Setup::new(&["agent.read-prompts"])
        .tree(vec![
            node(1, None, NodeClass::Container, "root"),
            agent_pane(2, Some(1), AgentActivity::Idle),
        ])
        .build();
    env.kernel
        .emit_event(Event::InputSubmitted(InputSubmittedEvent {
            node: NodeId(2),
            text: "hi".to_owned(),
            source: InputSource::User,
            epoch: 1,
        }));
    env.kernel
        .emit_event(Event::UserLineSubmitted(UserLineEvent {
            pane: NodeId(2),
            text: "hi".to_owned(),
            at: ilium_plugin_api::generated::types::EpochMs(1),
        }));
    env.settle();
    let kinds: Vec<&str> = env
        .probe
        .events()
        .iter()
        .map(|event| event.case_name())
        .collect();
    assert_eq!(kinds, vec!["input-submitted", "user-line-submitted"]);
}

/// REQ: EVT-040
#[test]
fn evt_040_scheduled_and_voice_prompts_fire_input_submitted_only() {
    let env = Setup::new(&["agent.read-prompts"]).build();
    for source in [InputSource::Scheduled, InputSource::Voice] {
        env.deliver(vec![Event::InputSubmitted(InputSubmittedEvent {
            node: NodeId(2),
            text: "t".to_owned(),
            source,
            epoch: 1,
        })]);
    }
    assert_eq!(env.probe.count_of("input-submitted"), 2);
    assert_eq!(env.probe.count_of("user-line-submitted"), 0);
}

/// REQ: EVT-041
#[test]
fn evt_041_pending_tree_changed_loses_the_removed_node() {
    let env = read_setup().build();
    env.host
        .deliver(vec![tree_changed(&[5, 3]), node_removed(5)]);
    env.settle();
    let lists = tree_changed_lists(&env.probe.events());
    assert!(lists.iter().all(|list| !list.contains(&5)), "{lists:?}");
    let kinds: Vec<&str> = env
        .probe
        .events()
        .iter()
        .map(|event| event.case_name())
        .collect();
    assert!(kinds.contains(&"node-removed"));
}

/// REQ: EVT-041
#[test]
fn evt_041_no_tree_changed_follows_node_removed_for_that_node() {
    let env = read_setup().build();
    env.host.deliver(vec![node_removed(5), tree_changed(&[3])]);
    env.settle();
    let events = env.probe.events();
    let removed = events
        .iter()
        .position(|event| event.case_name() == "node-removed")
        .expect("node-removed");
    for event in &events[removed..] {
        if let Event::TreeChanged(ids) = event {
            assert!(!ids.contains(&NodeId(5)));
        }
    }
}

/// REQ: EVT-041
#[test]
fn evt_041_pending_coalesced_events_keyed_by_the_node_are_discarded() {
    let env = read_setup().build();
    env.host.deliver(vec![
        pane_resized(5, 100),
        Event::PaneTitleChanged(PaneTitleChangedEvent {
            node: NodeId(5),
            title: "t".to_owned(),
            source: TitleOrigin::Osc,
        }),
        pane_resized(6, 100),
        node_removed(5),
    ]);
    env.settle();
    let resized: Vec<Event> = env.probe.events_of("pane-resized");
    assert_eq!(resized, vec![pane_resized(6, 100)]);
    assert_eq!(env.probe.count_of("pane-title-changed"), 0);
}

/// REQ: EVT-041
#[test]
fn evt_041_other_nodes_keep_their_pending_events() {
    let env = read_setup().build();
    env.host
        .deliver(vec![tree_changed(&[3, 4]), node_removed(4)]);
    env.settle();
    assert_eq!(tree_changed_lists(&env.probe.events()), vec![vec![3]]);
}

/// REQ: EVT-042
#[test]
fn evt_042_a_title_set_by_the_plugin_is_delivered_back_to_it() {
    let env = Setup::new(&["panes.read", "tree.write"])
        .tree(flat_tree(1))
        .build();
    env.run(vec![sub(1, rename(2, "renamed"))]);
    env.settle();
    let titles = env.probe.events_of("pane-title-changed");
    assert_eq!(titles.len(), 1);
    match &titles[0] {
        Event::PaneTitleChanged(changed) => {
            assert_eq!(changed.node, NodeId(2));
            assert_eq!(changed.title, "renamed");
        }
        other => panic!("unexpected {other:?}"),
    }
}

/// REQ: EVT-042
#[test]
fn evt_042_a_node_removed_by_the_plugin_is_reported_to_it() {
    let env = Setup::new(&["tree.read", "tree.write"])
        .tree(flat_tree(1))
        .build();
    env.run(vec![sub(1, close(2))]);
    env.settle();
    assert_eq!(env.probe.events_of("node-removed"), vec![node_removed(2)]);
}

/// REQ: EVT-042
#[test]
fn evt_042_a_freeze_by_the_plugin_is_reported_to_it() {
    let env = Setup::new(&["panes.read", "panes.lifecycle"])
        .tree(flat_tree(1))
        .build();
    env.run(vec![sub(1, freeze(2))]);
    env.settle();
    assert_eq!(env.probe.count_of("pane-lifecycle-changed"), 1);
}

// ---- EVT-043 .. EVT-045 -----------------------------------------------------------------

/// REQ: EVT-043
#[test]
fn evt_043_2000_screen_changed_for_one_pane_give_one_event_with_the_newest_revision() {
    let env = Setup::new(&["terminal.read-cells"]).build();
    env.deliver(
        (0..2000)
            .map(|revision| screen_changed(2, revision))
            .collect(),
    );
    let events = env.probe.events_of("screen-changed");
    assert_eq!(events, vec![screen_changed(2, 1999)]);
    assert_eq!(env.probe.count_of("events-dropped"), 0);
}

/// REQ: EVT-043
#[test]
fn evt_043_the_merged_event_sits_at_the_position_of_the_newest_occurrence() {
    let env = Setup::new(&["terminal.read-cells", "panes.read"]).build();
    env.deliver(vec![
        screen_changed(2, 1),
        pane_exited(9),
        screen_changed(2, 2),
    ]);
    assert_eq!(
        env.probe.events(),
        vec![pane_exited(9), screen_changed(2, 2)]
    );
}

/// REQ: EVT-043
#[test]
fn evt_043_different_keys_do_not_merge() {
    let env = Setup::new(&["terminal.read-cells"]).build();
    env.deliver(vec![
        screen_changed(2, 1),
        screen_changed(3, 1),
        screen_changed(2, 2),
    ]);
    assert_eq!(
        env.probe.events(),
        vec![screen_changed(3, 1), screen_changed(2, 2)]
    );
}

/// REQ: EVT-043
#[test]
fn evt_043_merging_happens_whatever_the_queue_fill() {
    let env = Setup::new(&["terminal.read-cells", "terminal.output-filter"]).build();
    let mut events = vec![screen_changed(2, 1)];
    events.extend(outputs(1023));
    events.push(screen_changed(2, 2));
    env.deliver(events);
    assert_eq!(
        env.probe.events_of("screen-changed"),
        vec![screen_changed(2, 2)]
    );
    assert_eq!(env.probe.count_of("events-dropped"), 0);
}

/// REQ: EVT-044
#[test]
fn evt_044_agent_activity_changed_is_never_dropped_1024_all_arrive() {
    let env = Setup::new(&["agent.read"]).build();
    env.deliver(
        (0..1024)
            .map(|revision| activity_changed(2, revision))
            .collect(),
    );
    assert_eq!(env.probe.count_of("agent-activity-changed"), 1024);
    assert_eq!(env.probe.count_of("events-dropped"), 0);
}

/// REQ: EVT-044
#[test]
fn evt_044_1500_agent_activity_changed_never_drop_and_the_instance_restarts() {
    let env = Setup::new(&["agent.read"]).build();
    env.deliver(
        (0..1500)
            .map(|revision| activity_changed(2, revision))
            .collect(),
    );
    let dropped = dropped_records(&env.probe.events());
    assert!(
        dropped
            .iter()
            .all(|record| record.event != "agent-activity-changed"),
        "{dropped:?}"
    );
    assert!(env.probe.activation_count() >= 2, "the instance restarted");
}

/// REQ: EVT-044
#[test]
fn evt_044_one_over_the_queue_restarts_the_instance() {
    let env = Setup::new(&["agent.read"]).build();
    env.deliver(
        (0..1025)
            .map(|revision| activity_changed(2, revision))
            .collect(),
    );
    assert_eq!(env.probe.activation_count(), 2);
}

/// REQ: EVT-045
#[test]
fn evt_045_dropping_output_matched_twice_gives_one_record_with_count_two() {
    let env = read_setup().build();
    env.deliver(outputs(1026));
    let events = env.probe.events();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.case_name() == "events-dropped")
            .count(),
        1
    );
    assert_eq!(
        dropped_records(&events),
        vec![DroppedCount {
            event: "output-matched".to_owned(),
            source: Some("f".to_owned()),
            count: 2
        }]
    );
}

/// REQ: EVT-045
#[test]
fn evt_045_records_are_per_event_and_source_in_order_of_first_drop() {
    let env = read_setup().build();
    let mut events = Vec::new();
    for index in 0..600 {
        events.push(output_matched_for("g", index));
    }
    for index in 0..600 {
        events.push(output_matched_for("f", index));
    }
    env.deliver(events);
    let records = dropped_records(&env.probe.events());
    assert_eq!(
        records.len(),
        1,
        "all drops are of the older g events: {records:?}"
    );
    assert_eq!(records[0].source, Some("g".to_owned()));
    assert_eq!(records[0].count, 176);
}

/// REQ: EVT-045
#[test]
fn evt_045_events_dropped_sits_at_the_head_of_the_next_batch() {
    let env = read_setup().build();
    env.deliver(outputs(1030));
    let batches = lock(&env.probe.batches).clone();
    assert_eq!(batches[0][0].case_name(), "events-dropped");
    assert!(batches[1..]
        .iter()
        .all(|batch| batch[0].case_name() != "events-dropped"));
}

/// REQ: EVT-045
#[test]
fn evt_045_events_dropped_is_never_dropped_itself() {
    let env = read_setup().build();
    let mut events = outputs(1100);
    events.extend(exits(10));
    env.deliver(events);
    assert_eq!(env.probe.count_of("events-dropped"), 1);
    assert_eq!(env.probe.count_of("pane-exited"), 10);
}

// ---- EVT-046 ---------------------------------------------------------------------------

fn received_outputs_after(env: &Env, count: u32) -> usize {
    env.deliver(outputs(count));
    env.probe.count_of("output-matched")
}

/// REQ: EVT-046
#[test]
fn evt_046_t2_asking_for_4096_gets_1024_and_one_clamp_log_line() {
    let env = Setup::new(&["terminal.output-filter"])
        .tier(Tier::T2)
        .limit("event_queue", 4096)
        .build();
    let received = received_outputs_after(&env, 5000);
    assert_eq!(received, 1024);
    assert_eq!(log_lines_containing(&env, "event_queue").len(), 1);
}

/// REQ: EVT-046
#[test]
fn evt_046_t1_asking_for_4096_gets_4096_and_no_clamp_line() {
    let env = Setup::new(&["terminal.output-filter"])
        .tier(Tier::T1)
        .limit("event_queue", 4096)
        .build();
    let received = received_outputs_after(&env, 5000);
    assert_eq!(received, 4096);
    assert!(log_lines_containing(&env, "event_queue").is_empty());
}

/// REQ: EVT-046
#[test]
fn evt_046_t1_asking_for_4097_is_clamped_to_4096_and_logged() {
    let env = Setup::new(&["terminal.output-filter"])
        .tier(Tier::T1)
        .limit("event_queue", 4097)
        .build();
    let received = received_outputs_after(&env, 5000);
    assert_eq!(received, 4096);
    assert_eq!(log_lines_containing(&env, "event_queue").len(), 1);
}

/// REQ: EVT-046
#[test]
fn evt_046_t3_asking_for_1025_is_clamped_to_1024() {
    let env = Setup::new(&["terminal.output-filter"])
        .limit("event_queue", 1025)
        .build();
    let received = received_outputs_after(&env, 2000);
    assert_eq!(received, 1024);
    assert_eq!(log_lines_containing(&env, "event_queue").len(), 1);
}

/// REQ: EVT-046
#[test]
fn evt_046_the_default_queue_is_1024_without_a_request() {
    let env = Setup::new(&["terminal.output-filter"])
        .tier(Tier::T1)
        .build();
    let received = received_outputs_after(&env, 2000);
    assert_eq!(received, 1024);
    assert!(log_lines_containing(&env, "event_queue").is_empty());
}

// ---- EVT-047 .. EVT-049 -------------------------------------------------------------------

/// REQ: EVT-047
#[test]
fn evt_047_activation_with_tree_read_and_3_nodes_gives_one_tree_changed_with_3_ids() {
    let env = Setup::new(&["tree.read"]).tree(flat_tree(2)).build();
    env.deliver(vec![trigger()]);
    let batches = lock(&env.probe.batches).clone();
    assert_eq!(batches[0][0], tree_changed(&[1, 2, 3]));
    assert_eq!(tree_changed_lists(&env.probe.events()).len(), 1);
}

/// REQ: EVT-047
#[test]
fn evt_047_without_tree_read_nothing_is_sent() {
    let env = Setup::new(&[]).tree(flat_tree(2)).build();
    env.deliver(vec![trigger()]);
    assert_eq!(env.probe.count_of("tree-changed"), 0);
}

/// REQ: EVT-047
#[test]
fn evt_047_the_list_is_in_pre_order_not_in_id_order() {
    let nodes = vec![
        node(1, None, NodeClass::Container, "root"),
        node(5, Some(1), NodeClass::Container, "a"),
        pane(3, Some(5)),
        pane(2, Some(1)),
    ];
    let env = Setup::new(&["tree.read"]).tree(nodes).build();
    env.deliver(vec![trigger()]);
    assert_eq!(
        tree_changed_lists(&env.probe.events()),
        vec![vec![1, 5, 3, 2]]
    );
}

/// REQ: EVT-047
#[test]
fn evt_047_300_nodes_arrive_in_a_single_event_whatever_the_count() {
    let env = Setup::new(&["tree.read"]).tree(flat_tree(299)).build();
    env.deliver(vec![trigger()]);
    let lists = tree_changed_lists(&env.probe.events());
    assert_eq!(lists.len(), 1);
    assert_eq!(lists[0].len(), 300);
}

/// REQ: EVT-047
#[test]
fn evt_047_every_re_activation_starts_with_one_tree_changed() {
    let mut env = Setup::new(&["tree.read"]).tree(flat_tree(2)).build();
    env.deliver(vec![trigger()]);
    env.host.restart_instance(DeactivationReason::CrashRestart);
    env.deliver(vec![trigger()]);
    assert_eq!(tree_changed_lists(&env.probe.events()).len(), 2);
}

/// REQ: EVT-048
#[test]
fn evt_048_events_produced_while_activating_arrive_after_activate_returned_ok() {
    let setup = Setup::new(&["panes.read"]);
    let kernel = setup.kernel.clone();
    *lock(&setup.probe.activate_hook) = Some(Box::new(move || kernel.emit_event(pane_exited(4))));
    let env = setup.build();
    env.settle();
    assert_eq!(env.probe.events_of("pane-exited"), vec![pane_exited(4)]);
    let sequence = lock(&env.probe.sequence).clone();
    assert_eq!(sequence.first().map(String::as_str), Some("activate"));
}

/// REQ: EVT-048
#[test]
fn evt_048_events_of_a_failing_activate_are_discarded_without_events_dropped() {
    let setup = Setup::new(&["terminal.output-filter"]);
    let kernel = setup.kernel.clone();
    *lock(&setup.probe.activate_hook) = Some(Box::new(move || {
        for event in outputs(1500) {
            kernel.emit_event(event);
        }
    }));
    *lock(&setup.probe.activate_error) = Some(ilium_plugin_api::PluginError {
        code: ErrorCode::Internal,
        message: "boom".to_owned(),
        retry_after_ms: None,
    });
    let (built, probe, _kernel) = setup.try_build();
    drop(built);
    *lock(&probe.activate_error) = None;
    *lock(&probe.activate_hook) = None;
    assert_eq!(probe.count_of("events-dropped"), 0);
    assert_eq!(probe.count_of("output-matched"), 0);
}

/// REQ: EVT-048
#[test]
fn evt_048_a_later_successful_activation_gets_no_events_dropped_report() {
    let setup = Setup::new(&["terminal.output-filter"]);
    let kernel = setup.kernel.clone();
    *lock(&setup.probe.activate_hook) = Some(Box::new(move || {
        for event in outputs(1500) {
            kernel.emit_event(event);
        }
    }));
    *lock(&setup.probe.activate_error) = Some(ilium_plugin_api::PluginError {
        code: ErrorCode::Internal,
        message: "boom".to_owned(),
        retry_after_ms: None,
    });
    let probe = setup.probe.clone();
    let (built, _probe, kernel) = setup.try_build();
    let mut host = match built {
        Ok(host) => host,
        Err(_) => return_failed_activation_then_reactivate(&probe, &kernel),
    };
    host.deliver(vec![trigger()]);
    host.pump(64);
    assert_eq!(probe.count_of("events-dropped"), 0);
}

fn return_failed_activation_then_reactivate(
    probe: &std::sync::Arc<Shared>,
    kernel: &std::sync::Arc<ilium_plugin_host::testing::FakeKernel>,
) -> ilium_plugin_host::testing::HarnessHandle {
    *lock(&probe.activate_error) = None;
    *lock(&probe.activate_hook) = None;
    let setup = Setup {
        probe: probe.clone(),
        kernel: kernel.clone(),
        ..Setup::new(&["terminal.output-filter"])
    };
    setup.build().host
}

/// REQ: EVT-048
#[test]
fn evt_048_the_activation_that_breaches_its_deadline_discards_its_events() {
    let setup = Setup::new(&["panes.read"]);
    let kernel = setup.kernel.clone();
    *lock(&setup.probe.activate_hook) = Some(Box::new(move || {
        kernel.emit_event(pane_exited(4));
        std::thread::sleep(std::time::Duration::from_millis(1500));
    }));
    let (built, probe, _kernel) = setup.try_build();
    drop(built);
    assert_eq!(probe.count_of("pane-exited"), 0);
    assert_eq!(probe.count_of("events-dropped"), 0);
}

/// REQ: EVT-049
#[test]
fn evt_049_changes_of_nodes_5_and_3_merge_into_one_event_3_5() {
    let nodes = vec![
        node(1, None, NodeClass::Container, "root"),
        pane(3, Some(1)),
        pane(5, Some(1)),
    ];
    let env = Setup::new(&["tree.read"]).tree(nodes).build();
    env.host
        .deliver(vec![tree_changed(&[5]), tree_changed(&[3])]);
    env.settle();
    let lists = tree_changed_lists(&env.probe.events());
    assert_eq!(lists.last().cloned(), Some(vec![3, 5]));
}

/// REQ: EVT-049
#[test]
fn evt_049_duplicates_are_removed_from_the_union() {
    let nodes = vec![
        node(1, None, NodeClass::Container, "root"),
        pane(3, Some(1)),
        pane(5, Some(1)),
    ];
    let env = Setup::new(&["tree.read"]).tree(nodes).build();
    env.host.deliver(vec![
        tree_changed(&[5, 3]),
        tree_changed(&[3]),
        tree_changed(&[5]),
    ]);
    env.settle();
    assert_eq!(
        tree_changed_lists(&env.probe.events()).last().cloned(),
        Some(vec![3, 5])
    );
}

/// REQ: EVT-049
#[test]
fn evt_049_pending_tree_changed_events_merge_into_exactly_one_event() {
    let env = Setup::new(&["tree.read"]).tree(flat_tree(4)).build();
    env.settle();
    let before = tree_changed_lists(&env.probe.events()).len();
    env.host
        .deliver((2..6).map(|id| tree_changed(&[id])).collect());
    env.settle();
    assert_eq!(tree_changed_lists(&env.probe.events()).len(), before + 1);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10))]

    // REQ: EVT-049
    #[test]
    fn evt_049_merged_list_is_sorted_by_pre_order_without_duplicates(ids in proptest::collection::vec(2u64..8, 1..10)) {
        let env = Setup::new(&["tree.read"]).tree(flat_tree(6)).build();
        env.settle();
        let before = tree_changed_lists(&env.probe.events()).len();
        env.host.deliver(ids.iter().map(|id| tree_changed(&[*id])).collect());
        env.settle();
        let lists = tree_changed_lists(&env.probe.events());
        let merged = lists.last().cloned().unwrap_or_default();
        let mut expected = ids.clone();
        expected.sort();
        expected.dedup();
        prop_assert_eq!(lists.len(), before + 1);
        prop_assert_eq!(merged, expected);
    }
}

// ---- EVT-050 ---------------------------------------------------------------------------

/// REQ: EVT-050
#[test]
fn evt_050_25_refused_send_inputs_take_no_token_and_the_next_valid_one_is_accepted() {
    let env = Setup::new(&["terminal.write"]).tree(flat_tree(1)).build();
    env.kernel.set_pane_restored(NodeId(2), true);
    let refused: Vec<_> = (1..=25)
        .map(|n| keyed(n, &format!("r:{n}"), send_input(2, "x")))
        .collect();
    env.run(refused);
    for n in 1..=25 {
        assert!(
            is_refused(&env.outcome(n), ErrorCode::PaneNotReady),
            "send {n}: {:?}",
            env.outcome(n)
        );
    }
    env.kernel.set_pane_restored(NodeId(2), false);
    env.run(vec![keyed(100, "valid:1", send_input(2, "x"))]);
    assert!(is_ok(&env.outcome(100)), "{:?}", env.outcome(100));
}

/// REQ: EVT-050
#[test]
fn evt_050_20_valid_send_inputs_per_second_are_accepted() {
    let env = Setup::new(&["terminal.write"]).tree(flat_tree(1)).build();
    let batch: Vec<_> = (1..=20)
        .map(|n| keyed(n, &format!("v:{n}"), send_input(2, "x")))
        .collect();
    env.run(batch);
    for n in 1..=20 {
        assert!(is_ok(&env.outcome(n)), "send {n}: {:?}", env.outcome(n));
    }
}

/// REQ: EVT-050
#[test]
fn evt_050_the_21st_valid_send_input_in_one_second_is_rate_limited() {
    let env = Setup::new(&["terminal.write"]).tree(flat_tree(1)).build();
    let batch: Vec<_> = (1..=21)
        .map(|n| keyed(n, &format!("v:{n}"), send_input(2, "x")))
        .collect();
    env.run(batch);
    assert!(
        is_refused(&env.outcome(21), ErrorCode::RateLimited),
        "{:?}",
        env.outcome(21)
    );
}

/// REQ: EVT-050
#[test]
fn evt_050_20_valid_after_25_refused_in_the_same_second_are_all_accepted() {
    let env = Setup::new(&["terminal.write"]).tree(flat_tree(1)).build();
    env.kernel.set_pane_restored(NodeId(2), true);
    env.run(
        (1..=25)
            .map(|n| keyed(n, &format!("r:{n}"), send_input(2, "x")))
            .collect(),
    );
    env.kernel.set_pane_restored(NodeId(2), false);
    env.run(
        (101..=120)
            .map(|n| keyed(n, &format!("v:{n}"), send_input(2, "x")))
            .collect(),
    );
    for n in 101..=120 {
        assert!(is_ok(&env.outcome(n)), "send {n}: {:?}", env.outcome(n));
    }
}

#[allow(dead_code)]
fn unused_side_marker() -> Side {
    Side::Server
}
