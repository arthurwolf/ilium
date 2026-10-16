//! Helpers shared by the host-sched test modules: ids, event builders, a scripted native guest
//! and scheduler drivers. Every wait is bounded.
#![allow(dead_code)]

pub use std::sync::{Arc, Mutex};
pub use std::time::{Duration, Instant};

pub use ilium_plugin_api::generated::types::events::Event;
pub use ilium_plugin_api::ids::{NodeId, PluginId};
pub use ilium_plugin_host::calls::{CallClass, ResolvedLimits};
pub use ilium_plugin_host::scheduler::{CallKind, Scheduler};
pub use ilium_plugin_host::testing::{HarnessHandle, HostHarness};

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::thread::ThreadId;

use ilium_plugin_api::generated::types::events::{
    CliInvokedEvent, ClientRequestEvent, CommandContext, CommandInvokedEvent, CommandOrigin,
    DurableTimerEvent, HttpCalledEvent, InputUncertainEvent, KeyEvent, MessageEvent,
    PointerEvent,
};
use ilium_plugin_api::generated::types::ids::TimerId;
use ilium_plugin_api::generated::types::intents::{Intent, PluginSide, Submission};
use ilium_plugin_api::generated::types::tree::{NodeClass, NodeView};
use ilium_plugin_host::testing::FakeKernel;
use ilium_plugin_api::ProcessKind;
use ilium_plugin_api::generated::types::lifecycle::{Activation, DeactivationReason};
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef, RequestResult};
use ilium_plugin_api::generated::types::ui::{
    ActionKind, PointerButton, PointerKind, PointerModifiers, UiAction, ViewTarget,
};
use ilium_plugin_api::limits::LimitKey;
use ilium_plugin_api::{Code, PluginError};
use ilium_plugin_host::calls::Trap;
use ilium_plugin_host::testing::NativeGuest;

pub fn plugin(name: &str) -> PluginId {
    PluginId(name.to_owned())
}

pub fn at(base: Instant, ms: u64) -> Instant {
    base + Duration::from_millis(ms)
}

pub type Work = Box<dyn FnOnce() + Send>;

pub fn noop() -> Work {
    Box::new(|| {})
}

pub fn sleeper(ms: u64) -> Work {
    Box::new(move || std::thread::sleep(Duration::from_millis(ms)))
}

/// Runs `ticks` scheduler ticks 10 ms apart, starting 10 ms after `base`.
pub fn drain(scheduler: &Scheduler, base: Instant, ticks: u64) {
    for index in 0..ticks {
        scheduler.tick(at(base, (index + 1) * 10));
    }
}

pub fn kinds_run(scheduler: &Scheduler) -> Vec<CallKind> {
    scheduler.executed().iter().map(|call| call.kind).collect()
}

pub fn executed_count(scheduler: &Scheduler, who: &PluginId) -> usize {
    scheduler
        .executed()
        .iter()
        .filter(|call| &call.plugin == who)
        .count()
}

/// The deadline keys with the defaults of the call-class table.
pub fn default_limits() -> ResolvedLimits {
    let mut map = BTreeMap::new();
    map.insert(LimitKey::EventDeadlineMs, 20);
    map.insert(LimitKey::CommandDeadlineMs, 50);
    map.insert(LimitKey::RenderDeadlineMs, 10);
    map.insert(LimitKey::AnnotateDeadlineMs, 10);
    map.insert(LimitKey::FrameDeadlineMs, 50);
    map.insert(LimitKey::DetectorDeadlineMs, 20);
    map.insert(LimitKey::ActivateDeadlineMs, 2_000);
    map.insert(LimitKey::DeactivateDeadlineMs, 500);
    map.insert(LimitKey::MigrateDeadlineMs, 30_000);
    map.insert(LimitKey::JobDeadlineMs, 2_000);
    map.insert(LimitKey::JobWallMs, 20_000);
    ResolvedLimits(map)
}

pub fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

// ---- events -------------------------------------------------------------------------------

pub fn theme() -> Event {
    Event::ThemeChanged
}

pub fn keymap() -> Event {
    Event::KeymapChanged
}

pub fn tree_changed(ids: &[u64]) -> Event {
    Event::TreeChanged(ids.iter().map(|id| NodeId(*id)).collect())
}

pub fn key_pressed() -> Event {
    Event::KeyPressed(KeyEvent {
        chord: "ctrl+k".to_owned(),
        ..KeyEvent::default()
    })
}

pub fn key_released() -> Event {
    Event::KeyReleased(KeyEvent {
        chord: "ctrl+k".to_owned(),
        ..KeyEvent::default()
    })
}

pub fn command_invoked() -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: "demo.run".to_owned(),
        arguments_json: "{}".to_owned(),
        origin: CommandOrigin::Palette,
        context: CommandContext::default(),
    })
}

pub fn ui_action() -> Event {
    Event::UiAction(UiAction {
        view: ViewTarget::Panel("demo".to_owned()),
        key: "k".to_owned(),
        action_id: "a".to_owned(),
        kind: ActionKind::Activated,
        value: None,
    })
}

pub fn pointer() -> Event {
    Event::Pointer(PointerEvent {
        node: NodeId(1),
        row: 0,
        column: 0,
        kind: PointerKind::Down,
        button: PointerButton::Left,
        modifiers: PointerModifiers(0),
        sub_x: 0,
        sub_y: 0,
    })
}

pub fn message() -> Event {
    Event::Message(MessageEvent {
        from: plugin("demo.peer"),
        from_side: PluginSide::Server,
        from_client: None,
        channel: "c".to_owned(),
        payload: vec![1],
    })
}

pub fn request_completed() -> Event {
    Event::RequestCompleted(RequestResult {
        request: RequestRef::Submission(1),
        outcome: Outcome::Ok,
    })
}

pub fn durable_timer() -> Event {
    Event::DurableTimerFired(DurableTimerEvent {
        timer: TimerId(1),
        name: "t".to_owned(),
        late_by_millis: 0,
    })
}

pub fn input_uncertain() -> Event {
    Event::InputUncertain(InputUncertainEvent {
        pane: NodeId(1),
        dedup_key: "k".to_owned(),
    })
}

pub fn cli_invoked(invocation: u64) -> Event {
    Event::CliInvoked(CliInvokedEvent {
        invocation,
        name: "demo".to_owned(),
        argv: Vec::new(),
        cwd: "/".to_owned(),
    })
}

pub fn http_called(call: u64) -> Event {
    Event::HttpCalled(HttpCalledEvent {
        call,
        method: "GET".to_owned(),
        path: "/".to_owned(),
        headers: Vec::new(),
        body: Vec::new(),
    })
}

pub fn client_request(call: u64) -> Event {
    Event::ClientRequest(ClientRequestEvent {
        call,
        kind: "k".to_owned(),
        payload_json: "{}".to_owned(),
    })
}

pub fn contains_kind(batches: &[Vec<Event>], wanted: &Event) -> bool {
    batches
        .iter()
        .flatten()
        .any(|event| std::mem::discriminant(event) == std::mem::discriminant(wanted))
}

pub fn count_kind(batches: &[Vec<Event>], wanted: &Event) -> usize {
    batches
        .iter()
        .flatten()
        .filter(|event| std::mem::discriminant(*event) == std::mem::discriminant(wanted))
        .count()
}

// ---- scripted guest -----------------------------------------------------------------------

/// What the guest observed; shared with the test.
#[derive(Default)]
pub struct Shared {
    pub batches: Mutex<Vec<Vec<Event>>>,
    pub threads: Mutex<Vec<(String, String, ThreadId)>>,
    pub starts: Mutex<Vec<Instant>>,
}

impl Shared {
    pub fn new() -> Arc<Shared> {
        Arc::new(Shared::default())
    }

    pub fn batches(&self) -> Vec<Vec<Event>> {
        self.batches.lock().expect("batches lock").clone()
    }

    pub fn threads(&self) -> Vec<(String, String, ThreadId)> {
        self.threads.lock().expect("threads lock").clone()
    }

    fn note(&self, phase: &str) {
        let current = std::thread::current();
        self.threads.lock().expect("threads lock").push((
            phase.to_owned(),
            current.name().unwrap_or("").to_owned(),
            current.id(),
        ));
    }
}

pub type TrapRule = Box<dyn Fn(usize, &[Event]) -> bool + Send>;

/// A native guest that records its calls and traps by rule.
pub struct ScriptedGuest {
    pub shared: Arc<Shared>,
    pub on_events_sleep_ms: u64,
    pub deactivate_sleep_ms: u64,
    pub restore: Option<Vec<u8>>,
    pub submissions: Vec<Submission>,
    pub trap_when: TrapRule,
}

impl ScriptedGuest {
    pub fn new(shared: Arc<Shared>) -> ScriptedGuest {
        ScriptedGuest {
            shared,
            on_events_sleep_ms: 0,
            deactivate_sleep_ms: 0,
            restore: None,
            submissions: Vec::new(),
            trap_when: Box::new(|_, _| false),
        }
    }

    pub fn sleeping(mut self, on_events_sleep_ms: u64) -> ScriptedGuest {
        self.on_events_sleep_ms = on_events_sleep_ms;
        self
    }

    pub fn trapping(mut self, rule: TrapRule) -> ScriptedGuest {
        self.trap_when = rule;
        self
    }
}

pub fn deadline_trap(function: &str) -> Trap {
    Trap {
        code: Code(Cow::Borrowed("trap.deadline")),
        interface: "native-guest".to_owned(),
        function: function.to_owned(),
    }
}

impl NativeGuest for ScriptedGuest {
    fn activate(&mut self, _activation: Activation) -> Result<(), PluginError> {
        self.shared.note("activate");
        Ok(())
    }

    fn deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        self.shared.note("deactivate");
        if self.deactivate_sleep_ms > 0 {
            std::thread::sleep(ms(self.deactivate_sleep_ms));
        }
        self.restore.clone()
    }

    fn on_events(&mut self, batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        self.shared.note("on-events");
        self.shared.starts.lock().expect("starts lock").push(Instant::now());
        let call_number = {
            let mut batches = self.shared.batches.lock().expect("batches lock");
            batches.push(batch.clone());
            batches.len()
        };
        if self.on_events_sleep_ms > 0 {
            std::thread::sleep(ms(self.on_events_sleep_ms));
        }
        if (self.trap_when)(call_number, &batch) {
            return Err(deadline_trap("on-events"));
        }
        Ok(self.submissions.clone())
    }
}

pub fn build(guest: ScriptedGuest) -> HarnessHandle {
    HostHarness::new()
        .with_native_guest(Box::new(guest))
        .build()
        .expect("harness builds")
}

pub fn build_with(builder: HostHarness, guest: ScriptedGuest) -> HarnessHandle {
    builder
        .with_native_guest(Box::new(guest))
        .build()
        .expect("harness builds")
}

impl ScriptedGuest {
    pub fn submitting(mut self, submissions: Vec<Submission>) -> ScriptedGuest {
        self.submissions = submissions;
        self
    }
}

pub fn play_sound_submission(correlation: u64) -> Submission {
    Submission {
        intent: Intent::PlaySound("ding".to_owned()),
        correlation,
        dedup_key: None,
    }
}

pub fn pane_node(id: u64) -> NodeView {
    NodeView {
        id: NodeId(id),
        parent: None,
        class: NodeClass::Pane,
        content_kind: None,
        title: format!("pane {id}"),
        cwd: None,
        agent: None,
        locked: false,
        revision: 1,
    }
}

pub fn kernel_with_nodes(count: u64) -> Arc<FakeKernel> {
    let kernel = Arc::new(FakeKernel::new(ProcessKind::Server));
    kernel.set_tree((1..=count).map(pane_node).collect());
    kernel
}

/// Pumps and moves the manual clock forward in bounded rounds so a restart can finish.
pub fn settle(handle: &mut HarnessHandle) {
    for _ in 0..6 {
        handle.pump(16);
        handle.advance_clock(Duration::from_secs(30));
    }
    handle.pump(16);
}
