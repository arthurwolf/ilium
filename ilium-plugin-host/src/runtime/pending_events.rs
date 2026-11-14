//! Events that background workers of the imports hand to the runtime.
//!
//! A child process reader, an http stream, a job runner or a file watch runs on a thread the
//! import owns. It must not call a guest, so it queues the event here; the runtime thread drains
//! the queue at the start of every `pump` round and delivers each event to its plugin. Every
//! queued event names the plugin instance it belongs to, so an event of an instance that was
//! deactivated or replaced meanwhile is dropped instead of reaching the next instance.
//!
//! Workers also register with [`PendingEvents::worker`]: while any worker lives, the runtime
//! thread wakes at a short interval and pumps, so a background event is delivered even when the
//! host posts nothing.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use ilium_plugin_api::generated::types::events::Event;

/// Most events kept before the oldest is dropped; a plugin that never pumps cannot grow the host
/// without bound.
const QUEUE_CAPACITY: usize = 4096;

/// How often the runtime thread looks at the queue while a worker lives.
pub const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// One queued event.
#[derive(Debug, Clone, PartialEq)]
pub struct QueuedEvent {
    /// The plugin that receives the event.
    pub plugin: String,
    /// The instance number of the plugin when the event was produced.
    pub instance: u64,
    /// The event.
    pub event: Event,
}

#[derive(Debug, Default)]
struct Shared {
    queue: Mutex<VecDeque<QueuedEvent>>,
    workers: AtomicUsize,
}

/// The queue of background events of one runtime. Cloning shares the queue.
#[derive(Debug, Clone, Default)]
pub struct PendingEvents {
    shared: Arc<Shared>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Every writer pushes or pops one item, so the data stays consistent after a panic.
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

impl PendingEvents {
    /// An empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues one event for an instance of a plugin.
    pub fn push(&self, plugin: &str, instance: u64, event: Event) {
        let mut queue = lock(&self.shared.queue);
        if queue.len() >= QUEUE_CAPACITY {
            queue.pop_front();
        }
        queue.push_back(QueuedEvent {
            plugin: plugin.to_owned(),
            instance,
            event,
        });
    }

    /// Takes every queued event in arrival order.
    pub fn drain(&self) -> Vec<QueuedEvent> {
        lock(&self.shared.queue).drain(..).collect()
    }

    /// Whether no event waits.
    pub fn is_empty(&self) -> bool {
        lock(&self.shared.queue).is_empty()
    }

    /// How many events wait.
    pub fn len(&self) -> usize {
        lock(&self.shared.queue).len()
    }

    /// Drops what waits for a plugin (it was deactivated or failed).
    pub fn forget_plugin(&self, plugin: &str) {
        lock(&self.shared.queue).retain(|queued| queued.plugin != plugin);
    }

    /// Registers a background worker; the returned guard unregisters it when dropped.
    pub fn worker(&self) -> WorkerGuard {
        self.shared.workers.fetch_add(1, Ordering::AcqRel);
        WorkerGuard {
            shared: Arc::clone(&self.shared),
        }
    }

    /// How many workers live.
    pub fn workers(&self) -> usize {
        self.shared.workers.load(Ordering::Acquire)
    }

    /// A handle that queues events for one instance of one plugin.
    pub fn sink(&self, plugin: &str, instance: u64) -> EventSink {
        EventSink {
            events: self.clone(),
            plugin: plugin.to_owned(),
            instance,
        }
    }
}

/// Keeps the runtime thread polling while a background worker lives.
#[derive(Debug)]
pub struct WorkerGuard {
    shared: Arc<Shared>,
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        self.shared.workers.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Queues events for one instance of one plugin. Safe to move to a worker thread.
#[derive(Debug, Clone)]
pub struct EventSink {
    events: PendingEvents,
    plugin: String,
    instance: u64,
}

impl EventSink {
    /// Queues one event.
    pub fn send(&self, event: Event) {
        self.events.push(&self.plugin, self.instance, event);
    }

    /// The plugin this sink delivers to.
    pub fn plugin(&self) -> &str {
        &self.plugin
    }

    /// The instance number this sink delivers to.
    pub fn instance(&self) -> u64 {
        self.instance
    }

    /// How many events wait in the queue; a worker that must not lose output waits while this is
    /// high.
    pub fn backlog(&self) -> usize {
        self.events.len()
    }

    /// Registers a worker of the runtime this sink belongs to.
    pub fn worker(&self) -> WorkerGuard {
        self.events.worker()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_keep_their_order_and_name_their_instance() {
        let events = PendingEvents::new();
        events.push("a.plugin", 1, Event::ThemeChanged);
        events.push("b.plugin", 2, Event::KeymapChanged);
        let drained = events.drain();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].plugin, "a.plugin");
        assert_eq!(drained[0].instance, 1);
        assert_eq!(drained[1].event, Event::KeymapChanged);
        assert!(events.is_empty());
    }

    #[test]
    fn the_oldest_event_goes_first_when_the_queue_is_full() {
        let events = PendingEvents::new();
        for _ in 0..QUEUE_CAPACITY + 3 {
            events.push("a.plugin", 1, Event::ThemeChanged);
        }
        assert_eq!(events.len(), QUEUE_CAPACITY);
    }

    #[test]
    fn forgetting_a_plugin_keeps_the_others() {
        let events = PendingEvents::new();
        events.push("a.plugin", 1, Event::ThemeChanged);
        events.push("b.plugin", 1, Event::ThemeChanged);
        events.forget_plugin("a.plugin");
        let drained = events.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].plugin, "b.plugin");
    }

    #[test]
    fn a_worker_guard_counts_until_it_is_dropped() {
        let events = PendingEvents::new();
        assert_eq!(events.workers(), 0);
        let first = events.worker();
        let sink = events.sink("a.plugin", 3);
        let second = sink.worker();
        assert_eq!(events.workers(), 2);
        drop(first);
        assert_eq!(events.workers(), 1);
        drop(second);
        assert_eq!(events.workers(), 0);
        sink.send(Event::ThemeChanged);
        assert_eq!(events.drain()[0].instance, 3);
    }
}
