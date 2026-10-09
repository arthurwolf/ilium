use super::*;
use crate::app::{App, PaneRuntime};
use ilium_execution::{
    ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaLimits, ShutdownMode,
};
use std::{cell::RefCell, time::Instant};

pub(crate) struct Bank(Execution);

impl Drop for Bank {
    fn drop(&mut self) {
        self.0.request_shutdown(ShutdownMode::Cancel);
        assert_eq!(
            self.0
                .join_until_background(Instant::now() + Duration::from_secs(5))
                .unwrap()
                .remaining_workers,
            0
        );
    }
}

pub(crate) fn view(app: &App, id: NodeId) -> &TerminalView {
    let Some(PaneRuntime::Terminal(view)) = app.panes.get(&id) else {
        panic!("missing terminal {id:?}");
    };
    view
}

#[test]
fn pending_parser_retries_prioritize_displayed_panes_and_keep_hidden_work() {
    let ids = [NodeId(5), NodeId(3), NodeId(7), NodeId(4)];
    let order = crate::app::terminal_parser_retry_order(
        ids,
        &[NodeId(7), NodeId(3), NodeId(7), NodeId(99)],
    );

    assert_eq!(&order[..2], &[NodeId(7), NodeId(3)]);
    let mut hidden = order[2..].to_vec();
    hidden.sort_unstable();
    assert_eq!(hidden, [NodeId(4), NodeId(5)]);
}

fn view_mut(app: &mut App, id: NodeId) -> &mut TerminalView {
    let Some(PaneRuntime::Terminal(view)) = app.panes.get_mut(&id) else {
        panic!("missing terminal {id:?}");
    };
    view
}

fn pump(app: &mut App, ready: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.collect_terminal_parsing();
        if ready(app) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "App parser did not make progress"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

pub(crate) fn initialized_app(
    count: usize,
    budget_mib: u32,
    select: fn(&mut App, NodeId),
) -> (Bank, App, Vec<NodeId>) {
    let disabled = LaneConfig {
        threads: 0,
        queue_slots: 0,
        priority: None,
        resident_bytes_per_thread: 0,
    };
    let execution = Execution::start(
        QuotaGroup::new(QuotaLimits {
            clients: 1,
            jobs: 1,
            service_jobs: 1,
            input_bytes: 128 * MIB,
            result_bytes: 2 * MIB,
            // The service thread reserves 128 MiB; keep separate headroom for
            // the execution bank's retained metadata and worker bookkeeping.
            worker_threads: 1,
            worker_bytes: 132 * MIB,
        }),
        ExecutionConfig {
            cpu: disabled,
            io: disabled,
            service: LaneConfig {
                threads: 1,
                queue_slots: 1,
                priority: None,
                resident_bytes_per_thread: 128 * MIB,
            },
        },
    )
    .unwrap();
    let client = execution
        .client(ClientLimits {
            jobs: 1,
            service_jobs: 1,
            input_bytes: 128 * MIB,
            result_bytes: 2 * MIB,
        })
        .unwrap();
    let parsing = TerminalParsing::start(client, budget_mib).unwrap();
    let bank = Bank(execution);
    let mut app = App::new("parser-regression".into(), std::env::temp_dir());
    app.terminal_settings.engine_memory_budget_mib = budget_mib;
    let group = app.tree.add_group(ilium_core::ROOT_ID, "work").unwrap();
    let mut ids = Vec::new();
    for index in 0..count {
        let label = format!("pane-{index:03}");
        let id = app
            .tree
            .add_pane(group, &label, ilium_core::PaneContentKind::Terminal)
            .unwrap();
        let mut terminal = TerminalView::with_scrollback_budget_mib(
            24,
            80,
            app.terminal_settings.scrollback_budget_mib,
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while parsing.attach(id, &mut terminal).is_err() {
            assert!(Instant::now() < deadline, "registration stalled");
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut command = PaneCommand::Output {
            first: 1,
            sequence: 1,
            bytes: label.into_bytes(),
            track: true,
        };
        loop {
            match terminal.frontend.as_mut().unwrap().submit(command) {
                Ok(_) => break,
                Err(returned) => command = returned,
            }
            assert!(Instant::now() < deadline, "output admission stalled");
            std::thread::sleep(Duration::from_millis(1));
        }
        app.panes
            .insert(id, PaneRuntime::Terminal(Box::new(terminal)));
        ids.push(id);
    }
    app.terminal_parsing = Some(parsing);
    select(&mut app, ids[0]);
    app.set_screen_area(ratatui::layout::Rect::new(0, 0, 120, 40));
    pump(&mut app, |app| {
        ids.iter().enumerate().all(|(index, id)| {
            let terminal = view(app, *id);
            terminal.can_evict_parser()
                && terminal.with_screen(|screen| screen.contents()) == format!("pane-{index:03}")
        })
    });
    pump(&mut app, |app| {
        app.terminal_parsing.as_ref().unwrap().pending_work() == Some((0, 0))
    });
    (bank, app, ids)
}

#[derive(Debug, PartialEq, Eq)]
enum ProbeEvent {
    Accepted(u8),
    Applied(u64),
}

thread_local! {
    static TRACE: RefCell<Option<Vec<ProbeEvent>>> = const { RefCell::new(None) };
}

fn record(event: ProbeEvent) {
    TRACE.with(|trace| {
        if let Some(events) = trace.borrow_mut().as_mut() {
            assert!(events.len() < 128);
            events.push(event);
        }
    });
}

pub(crate) fn accepted_result(result: &ParseResult) {
    record(ProbeEvent::Accepted(match result {
        ParseResult::Published { .. } => 0,
        ParseResult::Acknowledged { .. } => 1,
        ParseResult::Error { .. } => 2,
        ParseResult::InputBarrier { .. } => 3,
        ParseResult::Capture { .. } => 4,
        ParseResult::CaptureFailed { .. } => 5,
    }));
}

pub(crate) fn applied_ordinal(ordinal: u64) {
    record(ProbeEvent::Applied(ordinal));
}

struct Probe;

impl Probe {
    fn start() -> Self {
        TRACE.with(|trace| assert!(trace.borrow_mut().replace(Vec::new()).is_none()));
        Self
    }

    fn finish(self) -> Vec<ProbeEvent> {
        TRACE.with(|trace| trace.borrow_mut().take().unwrap())
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        TRACE.with(|trace| {
            trace.borrow_mut().take();
        });
    }
}

fn all_variants(target: PaneTarget, ordinal: u64) -> Vec<ParseResult> {
    let mut state = TerminalState::new(24, 80);
    state.feed(b"stale picture");
    vec![
        ParseResult::Published {
            target: target.clone(),
            ordinal,
            snapshot: Arc::new(state.publish()),
            evidence: None,
        },
        ParseResult::Acknowledged {
            target: target.clone(),
            ordinal,
            evidence: None,
        },
        ParseResult::Error {
            target: target.clone(),
            message: "stale error".into(),
        },
        ParseResult::InputBarrier {
            target: target.clone(),
            generation: 77,
            ordinal,
            sequence: 99,
            mouse: true,
            paste: true,
        },
        ParseResult::Capture {
            target: target.clone(),
            generation: 77,
            snapshot: state.with_screen(crate::smart_copy::SmartCopySnapshot::capture),
        },
        ParseResult::CaptureFailed {
            target,
            generation: 77,
            message: "stale capture".into(),
        },
    ]
}

pub(crate) fn stale_domain_attachment_and_dead_target_never_reach_any_app_result_consumer(
    select: fn(&mut App, NodeId),
) {
    let (_bank, mut app, ids) = initialized_app(1, 0, select);
    let id = ids[0];
    let old = view(&app, id).frontend.as_ref().unwrap().target.clone();
    assert!(view_mut(&mut app, id).evict_parser());
    app.collect_terminal_parsing();
    pump(&mut app, |app| view(app, id).can_evict_parser());
    let current = view(&app, id).frontend.as_ref().unwrap().target.clone();
    assert!(Arc::ptr_eq(&old.identity, &current.identity));
    assert!(!Arc::ptr_eq(&old.alive, &current.alive));
    let mut wrong_domain = current.clone();
    wrong_domain.identity = Arc::new(());
    let mut wrong_attachment = current.clone();
    wrong_attachment.alive = Arc::new(AtomicBool::new(true));
    let shared = app.terminal_parsing.as_ref().unwrap().shared.clone();
    let _queue = shared.queue.lock().unwrap();
    let ordinal = view(&app, id).applied_ordinal;
    let screen = view(&app, id).with_screen(|screen| screen.contents());
    app.status_message = Some("preserved status".into());
    view_mut(&mut app, id).admission_error = Some("preserved error".into());
    for (target, dead_current) in [
        (old, false),
        (wrong_domain, false),
        (wrong_attachment, false),
        (current.clone(), true),
    ] {
        if dead_current {
            current.alive.store(false, Ordering::Release);
        }
        assert!(!view(&app, id).accepts_parser_target(&target));
        shared
            .results
            .lock()
            .unwrap()
            .extend(all_variants(target, ordinal + 100));
        let probe = Probe::start();
        assert!(!app.collect_terminal_parsing());
        assert_eq!(probe.finish(), Vec::<ProbeEvent>::new());
        assert_eq!(view(&app, id).applied_ordinal, ordinal);
        assert_eq!(
            view(&app, id).with_screen(|screen| screen.contents()),
            screen
        );
        assert_eq!(
            view(&app, id).admission_error.as_deref(),
            Some("preserved error")
        );
        assert_eq!(app.status_message.as_deref(), Some("preserved status"));
        current.alive.store(true, Ordering::Release);
    }
    shared
        .results
        .lock()
        .unwrap()
        .extend(all_variants(current, ordinal + 100));
    let probe = Probe::start();
    app.collect_terminal_parsing();
    let accepted: Vec<_> = probe
        .finish()
        .into_iter()
        .filter_map(|event| match event {
            ProbeEvent::Accepted(kind) => Some(kind),
            _ => None,
        })
        .collect();
    assert_eq!(accepted, [0, 1, 2, 3, 4, 5]);
}

pub(crate) fn app_completes_each_ordinal_once_and_ack_preserves_picture_and_error(
    select: fn(&mut App, NodeId),
) {
    let (_bank, mut app, ids) = initialized_app(1, 0, select);
    let id = ids[0];
    let target = view(&app, id).frontend.as_ref().unwrap().target.clone();
    let old = view(&app, id).applied_ordinal;
    let shared = app.terminal_parsing.as_ref().unwrap().shared.clone();
    let _queue = shared.queue.lock().unwrap();
    let mut state = TerminalState::new(24, 80);
    state.feed(b"next picture");
    let snapshot = Arc::new(state.publish());
    view_mut(&mut app, id).admission_error = Some("retained diagnostic".into());
    let probe = Probe::start();
    for ordinal in [old - 1, old, old + 1, old + 1] {
        shared
            .results
            .lock()
            .unwrap()
            .push_back(ParseResult::Acknowledged {
                target: target.clone(),
                ordinal,
                evidence: None,
            });
    }
    app.collect_terminal_parsing();
    assert_eq!(view(&app, id).applied_ordinal, old + 1);
    assert_eq!(
        view(&app, id).with_screen(|screen| screen.contents()),
        "pane-000"
    );
    assert_eq!(
        view(&app, id).admission_error.as_deref(),
        Some("retained diagnostic")
    );
    for ordinal in [old, old + 1, old + 2, old + 2, old + 1] {
        shared
            .results
            .lock()
            .unwrap()
            .push_back(ParseResult::Published {
                target: target.clone(),
                ordinal,
                snapshot: snapshot.clone(),
                evidence: None,
            });
    }
    app.collect_terminal_parsing();
    let applied: Vec<_> = probe
        .finish()
        .into_iter()
        .filter_map(|event| match event {
            ProbeEvent::Applied(ordinal) => Some(ordinal),
            _ => None,
        })
        .collect();
    assert_eq!(applied, [old + 1, old + 2]);
    assert_eq!(view(&app, id).applied_ordinal, old + 2);
    assert_eq!(
        view(&app, id).with_screen(|screen| screen.contents()),
        "next picture"
    );
    assert!(view(&app, id).admission_error.is_none());
}

pub(crate) fn pooled_app_protects_displayed_and_busy_panes_and_waits_for_retiring_claims(
    select: fn(&mut App, NodeId),
) {
    let (_bank, mut app, ids) = initialized_app(5, 256, select);
    let displayed = ids[0];
    let shared = app.terminal_parsing.as_ref().unwrap().shared.clone();
    let queue = shared.queue.lock().unwrap();
    let displayed_target = view(&app, displayed)
        .frontend
        .as_ref()
        .unwrap()
        .target
        .clone();
    for (budget, pressure) in [
        (0, ParserPressure::StatePool),
        (0, ParserPressure::ProcessStorage),
        (256, ParserPressure::PaneLimit),
        (256, ParserPressure::StorageBusy),
    ] {
        app.terminal_parsing
            .as_ref()
            .unwrap()
            .set_budget_mib(budget);
        displayed_target
            .pressure
            .store(pressure as usize, Ordering::Release);
        app.collect_terminal_parsing();
        assert!(ids.iter().all(|id| view(&app, *id).frontend.is_some()));
        if budget == 0 {
            let requests = app.take_admitted_outbound_requests();
            let queued_discard = requests.iter().any(|request| {
                matches!(
                    request.view(),
                    ilium_ipc::ClientRequest::DiscardTerminalDelivery { .. }
                )
            });
            for request in requests {
                app.enqueue_admitted_request(request);
            }
            assert!(
                !queued_discard,
                "pool Off must not queue a discard that can delay visible-pane replay"
            );
        }
    }
    let mut hidden = ids[1..].to_vec();
    hidden.sort_by_key(|id| (app.tree.get(*id).unwrap().last_focus_activity_revision, *id));
    let busy = hidden[0];
    let victim = hidden[1];
    let frontend = view_mut(&mut app, busy).frontend.as_mut().unwrap();
    frontend.pending.push_back(PaneCommand::Bottom);
    frontend.output_backpressure = true;
    let pin = view(&app, victim).painted_source().pinned().unwrap();
    let old_text = pin.with_screen(|screen| screen.contents());
    let identity = view(&app, victim).identity.clone();
    let retiring = view(&app, victim).frontend.as_ref().unwrap().target.clone();
    let old_alive = retiring.alive.clone();
    displayed_target
        .pressure
        .store(ParserPressure::StatePool as usize, Ordering::Release);
    app.collect_terminal_parsing();
    assert!(view(&app, victim).frontend.is_none());
    assert!(view(&app, displayed).frontend.is_some());
    assert!(view(&app, busy).frontend.is_some());
    assert_eq!(shared.engine_claims.load(Ordering::Acquire), ids.len());
    for _ in 0..3 {
        app.collect_terminal_parsing();
        assert_eq!(
            ids.iter()
                .filter(|id| view(&app, **id).frontend.is_none())
                .count(),
            1
        );
    }
    assert_eq!(pin.with_screen(|screen| screen.contents()), old_text);
    displayed_target.pressure.store(0, Ordering::Release);
    let frontend = view_mut(&mut app, busy).frontend.as_mut().unwrap();
    frontend.pending.clear();
    frontend.output_backpressure = false;
    drop(queue);
    drop(retiring);
    pump(&mut app, |_| {
        shared.engine_claims.load(Ordering::Acquire) == ids.len() - 1
    });
    assert_eq!(pin.with_screen(|screen| screen.contents()), old_text);
    select(&mut app, victim);
    app.collect_terminal_parsing();
    assert!(Arc::ptr_eq(&identity, &view(&app, victim).identity));
    assert!(!Arc::ptr_eq(
        &old_alive,
        &view(&app, victim).frontend.as_ref().unwrap().target.alive
    ));
    let event = ilium_ipc::ServerEvent::TerminalReplay {
        pane_id: victim,
        through_sequence: 7,
        bytes: b"replayed victim".to_vec(),
        is_complete: true,
    };
    assert!(app.submit_terminal_event(event).is_ok());
    pump(&mut app, |app| {
        view(app, victim).can_evict_parser() && view(app, victim).last_output_sequence() == 7
    });
    assert_eq!(
        view(&app, victim).with_screen(|screen| screen.contents()),
        "replayed victim"
    );
    assert_eq!(pin.with_screen(|screen| screen.contents()), old_text);
}

pub(crate) fn invalid_exact_pool_setter_rejects_before_mutating_any_live_setting(
    select: fn(&mut App, NodeId),
) {
    let (_bank, mut app, _) = initialized_app(1, 0, select);
    let before = app.terminal_settings;
    for budget in [1, 255, 16385, u32::MAX] {
        assert!(app.set_terminal_engine_memory_budget_mib(budget).is_err());
        assert_eq!(app.terminal_settings, before);
        assert!(!app.terminal_parsing.as_ref().unwrap().pool_enabled());
    }
}

pub(crate) fn custom_pool_values_reach_existing_app_consumers(select: fn(&mut App, NodeId)) {
    let (_bank, mut app, ids) = initialized_app(2, 0, select);
    let shared = app.terminal_parsing.as_ref().unwrap().shared.clone();
    let attachments: Vec<_> = ids
        .iter()
        .map(|id| {
            view(&app, *id)
                .frontend
                .as_ref()
                .unwrap()
                .target
                .alive
                .clone()
        })
        .collect();
    let before = app.terminal_settings;
    let storage_bytes = shared.storage.snapshot().worker_bytes;
    for budget in [0, 256, 257, 16384, 0] {
        let settings = crate::config::TerminalSettings {
            engine_memory_budget_mib: budget,
            ..before
        };
        app.apply_terminal_settings(settings);
        assert_eq!(app.terminal_settings, settings);
        assert_eq!(
            shared.state_limit.load(Ordering::Acquire),
            budget as usize * MIB
        );
        assert_eq!(
            shared.snapshot_limit.load(Ordering::Acquire),
            if budget == 0 {
                0
            } else {
                (budget as usize + 256) * MIB
            }
        );
        assert_eq!(shared.storage.snapshot().worker_bytes, storage_bytes);
        for (index, id) in ids.iter().enumerate() {
            let current = view(&app, *id);
            assert!(Arc::ptr_eq(
                &attachments[index],
                &current.frontend.as_ref().unwrap().target.alive
            ));
            assert_eq!(
                current.with_screen(|screen| screen.contents()),
                format!("pane-{index:03}")
            );
        }
    }
}
