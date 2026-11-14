//! What every per-instance import is built from: the identity of the instance, its grants, the
//! request id allocator of the runtime and the queue background workers use to reach the plugin.
//!
//! An import never owns process-wide state. The runtime builds one [`ImportEnv`] per activation
//! (`ImportEnv::for_instance`), every import of that instance holds a clone, and dropping the
//! instance drops every import with the state, workers and children it started. Tests that build
//! an import without a runtime use [`ImportEnv::unrestricted`], which grants everything and owns a
//! private queue.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::ids::RequestId;
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef, RequestResult};
use ilium_plugin_api::grants::Grant;

use crate::runtime::pending_events::{EventSink, PendingEvents, WorkerGuard};
use crate::runtime::InstanceContext;

/// The request and service call id allocator of one runtime. Every import of every instance draws
/// from it, so two requests of one plugin never share an id and a `request-completed` event names
/// exactly one request. Cloning shares the counters.
#[derive(Debug, Clone)]
pub struct RequestIds {
    requests: Arc<AtomicU64>,
    calls: Arc<AtomicU64>,
}

impl Default for RequestIds {
    fn default() -> Self {
        Self {
            requests: Arc::new(AtomicU64::new(1)),
            calls: Arc::new(AtomicU64::new(1)),
        }
    }
}

impl RequestIds {
    /// A fresh allocator; ids start at 1 so zero never names a request.
    pub fn new() -> Self {
        Self::default()
    }

    /// The next request id.
    pub fn next_request(&self) -> RequestId {
        RequestId(self.requests.fetch_add(1, Ordering::Relaxed))
    }

    /// The next service call number.
    pub fn next_call(&self) -> u64 {
        self.calls.fetch_add(1, Ordering::Relaxed)
    }
}

/// The counters of what one session runs at once, shared by every instance of its runtime.
/// Cloning shares the counters.
#[derive(Debug, Clone, Default)]
pub struct SessionCounters {
    processes: Arc<AtomicUsize>,
}

impl SessionCounters {
    /// Fresh counters, all at zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes one process slot when fewer than `cap` are taken; the slot frees itself when dropped.
    pub fn try_acquire_process(&self, cap: usize) -> Option<SessionSlot> {
        self.processes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |taken| {
                (taken < cap).then_some(taken + 1)
            })
            .ok()?;
        Some(SessionSlot {
            counter: Arc::clone(&self.processes),
        })
    }

    /// How many process slots are taken.
    pub fn processes(&self) -> usize {
        self.processes.load(Ordering::Acquire)
    }
}

/// One taken slot of a session counter.
#[derive(Debug)]
pub struct SessionSlot {
    counter: Arc<AtomicUsize>,
}

impl Drop for SessionSlot {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Whether any of the `granted` capabilities allows `wanted`, a full grant string such as
/// `process.run:git` or `http:api.example.com`.
pub fn grants_allow(granted: &[String], wanted: &str) -> bool {
    let Ok(wanted) = Grant::parse(wanted) else {
        return false;
    };
    granted.iter().any(|held| {
        held == wanted.as_str() || Grant::parse(held).is_ok_and(|held| held.covers(&wanted))
    })
}

/// Whether any of the `granted` capabilities belongs to `family`, the part before the `:`.
pub fn grants_hold_family(granted: &[String], family: &str) -> bool {
    granted
        .iter()
        .any(|held| held.split(':').next() == Some(family))
}

/// The shared parts of the imports of one plugin instance.
#[derive(Debug, Clone)]
pub struct ImportEnv {
    plugin: String,
    instance: u64,
    /// `None` grants everything (host-internal use and tests).
    granted: Option<Arc<Vec<String>>>,
    events: PendingEvents,
    ids: RequestIds,
    session: SessionCounters,
    clock_origin: Instant,
}

impl Default for ImportEnv {
    fn default() -> Self {
        Self::unrestricted()
    }
}

impl ImportEnv {
    /// An environment that grants everything and owns a private event queue and id allocator.
    pub fn unrestricted() -> Self {
        Self {
            plugin: "ilium.host.standalone".to_owned(),
            instance: 0,
            granted: None,
            events: PendingEvents::new(),
            ids: RequestIds::new(),
            session: SessionCounters::new(),
            clock_origin: Instant::now(),
        }
    }

    /// The environment of the instance a runtime is activating.
    pub fn for_instance(context: &InstanceContext) -> Self {
        Self {
            plugin: context.plugin.clone(),
            instance: context.instance,
            granted: Some(Arc::new(context.granted.clone())),
            events: context.events.clone(),
            ids: context.ids.clone(),
            session: context.session.clone(),
            clock_origin: context.clock_origin,
        }
    }

    /// The same environment holding only these grants.
    #[must_use]
    pub fn with_grants(mut self, granted: Vec<String>) -> Self {
        self.granted = Some(Arc::new(granted));
        self
    }

    /// The same environment named as another plugin instance.
    #[must_use]
    pub fn with_identity(mut self, plugin: &str, instance: u64) -> Self {
        self.plugin = plugin.to_owned();
        self.instance = instance;
        self
    }

    /// The same environment drawing ids from `ids`.
    #[must_use]
    pub fn with_ids(mut self, ids: RequestIds) -> Self {
        self.ids = ids;
        self
    }

    /// The same environment queueing events on `events`.
    #[must_use]
    pub fn with_events(mut self, events: PendingEvents) -> Self {
        self.events = events;
        self
    }

    /// The plugin id.
    pub fn plugin(&self) -> &str {
        &self.plugin
    }

    /// The instance number.
    pub fn instance(&self) -> u64 {
        self.instance
    }

    /// The instant the monotonic clock of the host counts from.
    pub fn clock_origin(&self) -> Instant {
        self.clock_origin
    }

    /// The granted capabilities; empty when the environment is unrestricted.
    pub fn granted(&self) -> &[String] {
        match &self.granted {
            Some(granted) => granted.as_slice(),
            None => &[],
        }
    }

    /// Whether the plugin holds a grant that allows `wanted` (`process.run:git`).
    pub fn holds(&self, wanted: &str) -> bool {
        match &self.granted {
            None => true,
            Some(granted) => grants_allow(granted, wanted),
        }
    }

    /// Whether the plugin holds any grant of `family` (`fs.read`).
    pub fn holds_family(&self, family: &str) -> bool {
        match &self.granted {
            None => true,
            Some(granted) => grants_hold_family(granted, family),
        }
    }

    /// The id allocator of the runtime.
    pub fn ids(&self) -> &RequestIds {
        &self.ids
    }

    /// The next request id.
    pub fn next_request(&self) -> RequestId {
        self.ids.next_request()
    }

    /// The counters of what the session runs at once.
    pub fn session(&self) -> &SessionCounters {
        &self.session
    }

    /// The same environment counting against `session`.
    #[must_use]
    pub fn with_session(mut self, session: SessionCounters) -> Self {
        self.session = session;
        self
    }

    /// The event queue of the runtime.
    pub fn events(&self) -> &PendingEvents {
        &self.events
    }

    /// A handle for a worker thread that queues events for this instance.
    pub fn sink(&self) -> EventSink {
        self.events.sink(&self.plugin, self.instance)
    }

    /// Queues one event for this instance.
    pub fn emit(&self, event: Event) {
        self.events.push(&self.plugin, self.instance, event);
    }

    /// Queues `request-completed` for a request of this instance.
    pub fn complete(&self, request: RequestId, outcome: Outcome) {
        self.emit(completion_event(request, outcome));
    }

    /// Registers a background worker; the runtime thread polls while the guard lives.
    pub fn worker(&self) -> WorkerGuard {
        self.events.worker()
    }
}

/// The `request-completed` event of a call request.
pub fn completion_event(request: RequestId, outcome: Outcome) -> Event {
    Event::RequestCompleted(RequestResult {
        request: RequestRef::Call(request),
        outcome,
    })
}

/// A flag the workers of one import poll; it is set once the import is gone. Cloning shares the
/// flag.
#[derive(Debug, Clone, Default)]
pub struct CancelFlag(Arc<std::sync::atomic::AtomicBool>);

impl CancelFlag {
    /// Whether the owner asked its workers to stop.
    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    /// Asks every worker holding this flag to stop.
    pub fn set(&self) {
        self.0.store(true, Ordering::Release);
    }
}

/// Sets its [`CancelFlag`] when dropped. An import keeps one in an `Arc`, so the flag is set when
/// the last clone of the import (the instance) goes away and its workers stop.
#[derive(Debug, Default)]
pub struct CancelOnDrop(CancelFlag);

impl CancelOnDrop {
    /// The flag this guard sets.
    pub fn flag(&self) -> CancelFlag {
        self.0.clone()
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.set();
    }
}

/// Bounds how many workers of one import run at once. Cloning shares the bound.
#[derive(Debug, Clone)]
pub struct WorkerGate {
    running: Arc<(std::sync::Mutex<usize>, std::sync::Condvar)>,
    limit: usize,
}

/// One running slot of a [`WorkerGate`]; dropping it lets the next waiting worker run.
#[derive(Debug)]
pub struct GateSlot {
    running: Arc<(std::sync::Mutex<usize>, std::sync::Condvar)>,
}

impl Drop for GateSlot {
    fn drop(&mut self) {
        let (count, freed) = &*self.running;
        let mut count = count.lock().unwrap_or_else(|poison| poison.into_inner());
        *count = count.saturating_sub(1);
        freed.notify_one();
    }
}

impl WorkerGate {
    /// A gate letting at most `limit` workers (at least one) run at once.
    pub fn new(limit: usize) -> Self {
        Self {
            running: Arc::default(),
            limit: limit.max(1),
        }
    }

    /// Waits for a free slot; `None` when `cancel` is set first.
    pub fn enter(&self, cancel: &CancelFlag) -> Option<GateSlot> {
        let (count, freed) = &*self.running;
        let mut held = count.lock().unwrap_or_else(|poison| poison.into_inner());
        loop {
            if cancel.is_set() {
                return None;
            }
            if *held < self.limit {
                *held += 1;
                return Some(GateSlot {
                    running: Arc::clone(&self.running),
                });
            }
            held = freed
                .wait_timeout(held, std::time::Duration::from_millis(50))
                .unwrap_or_else(|poison| poison.into_inner())
                .0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cancel_guard_sets_its_flag_when_the_last_owner_drops() {
        let guard = Arc::new(CancelOnDrop::default());
        let flag = guard.flag();
        let other_owner = Arc::clone(&guard);
        drop(guard);
        assert!(!flag.is_set());
        drop(other_owner);
        assert!(flag.is_set());
    }

    #[test]
    fn a_worker_gate_admits_up_to_its_limit_and_refuses_after_cancel() {
        let gate = WorkerGate::new(1);
        let cancel = CancelFlag::default();
        let first = gate.enter(&cancel).expect("first slot");
        cancel.set();
        assert!(gate.enter(&cancel).is_none());
        drop(first);
        assert!(gate.enter(&CancelFlag::default()).is_some());
    }

    #[test]
    fn request_ids_are_unique_across_clones() {
        let ids = RequestIds::new();
        let other = ids.clone();
        let first = ids.next_request();
        let second = other.next_request();
        assert_ne!(first, second);
        assert_eq!(first.0, 1);
        assert_eq!(ids.next_call(), 1);
        assert_eq!(other.next_call(), 2);
    }

    #[test]
    fn two_allocators_do_not_share_ids() {
        let first = RequestIds::new();
        let second = RequestIds::new();
        first.next_request();
        assert_eq!(second.next_request().0, 1);
    }

    #[test]
    fn grants_are_matched_by_family_and_argument() {
        let granted = vec![
            "process.run:git".to_owned(),
            "http:*.example.com".to_owned(),
            "fs.read:project".to_owned(),
        ];
        assert!(grants_allow(&granted, "process.run:git"));
        assert!(!grants_allow(&granted, "process.run:sh"));
        assert!(grants_allow(&granted, "http:api.example.com"));
        assert!(!grants_allow(&granted, "http:example.org"));
        assert!(grants_hold_family(&granted, "fs.read"));
        assert!(!grants_hold_family(&granted, "fs.write"));
    }

    #[test]
    fn a_restricted_environment_refuses_what_it_does_not_hold() {
        let env = ImportEnv::unrestricted().with_grants(vec!["jobs.run".to_owned()]);
        assert!(env.holds("jobs.run"));
        assert!(!env.holds("process.long-lived"));
        assert!(ImportEnv::unrestricted().holds("process.long-lived"));
    }

    #[test]
    fn a_session_slot_frees_itself_and_the_cap_holds() {
        let session = SessionCounters::new();
        let first = session.try_acquire_process(2).expect("first slot");
        let _second = session.try_acquire_process(2).expect("second slot");
        assert!(session.try_acquire_process(2).is_none(), "the cap holds");
        drop(first);
        assert_eq!(session.processes(), 1);
        assert!(session.try_acquire_process(2).is_some());
    }

    #[test]
    fn a_completion_is_queued_for_the_instance() {
        let env = ImportEnv::unrestricted().with_identity("a.plugin", 4);
        let request = env.next_request();
        env.complete(request, Outcome::Ok);
        let queued = env.events().drain();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].plugin, "a.plugin");
        assert_eq!(queued[0].instance, 4);
        assert_eq!(queued[0].event, completion_event(request, Outcome::Ok));
    }
}
