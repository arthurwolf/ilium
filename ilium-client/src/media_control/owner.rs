//! One ordered owner for acknowledged media effects. Desired state is bounded;
//! actual paused-player custody never leaves this OS thread.
use ilium_execution::QuotaGroup;
use ilium_platform::owned_worker::{spawn_owned, OwnedWorker, StopToken, WorkerKind};
use std::{
    io,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Default)]
struct Demand {
    active: [bool; 2],
    revision: u64,
    restart: u64,
    closing: bool,
}
struct Shared {
    demand: Mutex<Demand>,
    changed: Condvar,
}
impl Shared {
    fn close(&self) {
        self.demand
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .closing = true;
        self.changed.notify_all();
    }
}

pub(crate) struct MediaOwner {
    shared: Arc<Shared>,
    owner: Option<OwnedWorker>,
}
#[derive(Clone)]
pub(crate) struct MediaLease {
    shared: Arc<Shared>,
    index: usize,
}
impl MediaLease {
    #[cfg(test)]
    pub(crate) fn inactive_fixture() -> Self {
        Self {
            shared: Arc::new(Shared {
                demand: Mutex::new(Demand::default()),
                changed: Condvar::new(),
            }),
            index: 0,
        }
    }

    pub(crate) fn restart(&self) {
        let mut demand = self.shared.demand.lock().unwrap_or_else(|e| e.into_inner());
        if demand.closing {
            return;
        }
        let Some(revision) = demand.revision.checked_add(1) else {
            tracing::error!("media ownership revision exhausted");
            return;
        };
        demand.active[self.index] = true;
        demand.revision = revision;
        demand.restart = revision;
        self.shared.changed.notify_one();
    }
    pub(crate) fn is_requested(&self) -> bool {
        self.shared
            .demand
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .active[self.index]
    }
    /// Publishes an intention, never a claim that D-Bus acknowledged an effect.
    pub(crate) fn request(&self, active: bool) {
        let mut demand = self.shared.demand.lock().unwrap_or_else(|e| e.into_inner());
        if demand.closing || demand.active[self.index] == active {
            return;
        }
        let Some(revision) = demand.revision.checked_add(1) else {
            tracing::error!("media ownership revision exhausted");
            return;
        };
        demand.active[self.index] = active;
        demand.revision = revision;
        self.shared.changed.notify_one();
    }
}

impl MediaOwner {
    pub(crate) fn start() -> io::Result<Self> {
        Self::start_with(crate::execution::process_quota(), || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            Ok(DbusBackend(runtime))
        })
    }
    fn start_with<B: Backend>(
        quota: QuotaGroup,
        backend: impl FnOnce() -> io::Result<B> + Send + 'static,
    ) -> io::Result<Self> {
        // zbus's locked wire limit is128 MiB. max_queued1 bounds its queues;
        // this conservative declaration covers current/queued/reply buffers
        // plus runtime and bounded name parsing. It is not an allocator cap.
        let admission = Arc::new(
            quota
                .reserve_external_worker(1, 400 * 1024 * 1024)
                .map_err(|e| io::Error::other(format!("media owner admission: {e:?}")))?,
        );
        let shared = Arc::new(Shared {
            demand: Mutex::new(Demand::default()),
            changed: Condvar::new(),
        });
        let wake_shared = Arc::clone(&shared);
        let worker_shared = Arc::clone(&shared);
        let owner = spawn_owned(
            "ilium-media",
            WorkerKind::Cooperative,
            StopToken::default(),
            move || {
                // The wake owner retains admission through actual join/TLS teardown.
                let _admission = &admission;
                wake_shared.close();
            },
            move |_| {
                ilium_platform::thread_priority::lower_current_thread(
                    ilium_platform::thread_priority::WorkerPriority::BelowNormal,
                );
                match backend() {
                    Ok(mut backend) => run_owner(worker_shared, &mut backend),
                    Err(error) => {
                        tracing::error!(%error, "media runtime could not start");
                        worker_shared.close();
                    }
                }
            },
        )?;
        Ok(Self {
            shared,
            owner: Some(owner),
        })
    }
    pub(crate) fn normal(&self) -> MediaLease {
        MediaLease {
            shared: Arc::clone(&self.shared),
            index: 0,
        }
    }
    pub(crate) fn cancel(&self) {
        self.shared.close();
    }
    pub(crate) fn demonstration(&self) -> MediaLease {
        MediaLease {
            shared: Arc::clone(&self.shared),
            index: 1,
        }
    }
    pub(crate) async fn shutdown(mut self) -> io::Result<()> {
        self.shared.close();
        let Some(owner) = self.owner.take() else {
            return Ok(());
        };
        let ticket = owner.ticket();
        drop(owner);
        let exit = tokio::task::spawn_blocking(move || {
            ticket.join_until(Instant::now() + Duration::from_secs(5))
        })
        .await
        .map_err(io::Error::other)?
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                format!("media restoration still owned: {e:?}"),
            )
        })?;
        if exit == ilium_platform::owned_worker::WorkerExit::Panicked {
            return Err(io::Error::other(
                "media owner panicked; restoration unverified",
            ));
        }
        Ok(())
    }
}
impl Drop for MediaOwner {
    fn drop(&mut self) {
        self.shared.close();
    }
}
trait Backend: Send + 'static {
    fn pause(&mut self, should_stop: &(dyn Fn() -> bool + Sync)) -> Vec<String>;
    fn resume(&mut self, players: Vec<String>);
}
struct DbusBackend(tokio::runtime::Runtime);
impl Backend for DbusBackend {
    fn pause(&mut self, should_stop: &(dyn Fn() -> bool + Sync)) -> Vec<String> {
        self.0
            .block_on(super::pause_playing_players_until(should_stop))
    }
    fn resume(&mut self, players: Vec<String>) {
        self.0.block_on(super::resume_players(players));
    }
}
fn run_owner(shared: Arc<Shared>, backend: &mut impl Backend) {
    let mut observed = 0;
    let mut observed_restart = 0;
    let mut paused = Vec::new();
    let mut was_active = false;
    loop {
        let desired = {
            let mut demand = shared.demand.lock().unwrap_or_else(|e| e.into_inner());
            while !demand.closing && demand.revision == observed {
                demand = shared
                    .changed
                    .wait(demand)
                    .unwrap_or_else(|e| e.into_inner());
            }
            *demand
        };
        let active = !desired.closing && desired.active.iter().any(|active| *active);
        let restart = active && was_active && desired.restart != observed_restart;
        // A handoff between normal/demo retains one aggregate ownership lease.
        // A pending pause still finishes into `paused`, then the latest demand
        // drives compensation; submitting Stop cannot lose its acknowledged set.
        if was_active && (!active || restart) {
            backend.resume(std::mem::take(&mut paused));
        }
        if active && (!was_active || restart) {
            paused = backend.pause(&|| {
                let current = shared.demand.lock().unwrap_or_else(|e| e.into_inner());
                current.closing
                    || !current.active.iter().any(|active| *active)
                    || current.restart != desired.restart
            });
        }
        was_active = active;
        observed = desired.revision;
        observed_restart = desired.restart;
        if desired.closing {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_execution::{QuotaLimits, RejectReason};
    use std::sync::mpsc;

    #[derive(Debug, PartialEq)]
    enum Event {
        PauseEntered,
        PauseFinished,
        Restored(Vec<String>),
    }
    struct Controlled {
        events: mpsc::Sender<Event>,
        release: Option<mpsc::Receiver<()>>,
    }
    impl Backend for Controlled {
        fn pause(&mut self, _should_stop: &(dyn Fn() -> bool + Sync)) -> Vec<String> {
            self.events.send(Event::PauseEntered).unwrap();
            if let Some(release) = self.release.take() {
                release.recv().unwrap();
            }
            self.events.send(Event::PauseFinished).unwrap();
            vec!["org.mpris.MediaPlayer2.synthetic-owned-player".into()]
        }
        fn resume(&mut self, players: Vec<String>) {
            self.events.send(Event::Restored(players)).unwrap();
        }
    }
    fn quota() -> QuotaGroup {
        QuotaGroup::new(QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 1,
            worker_bytes: 400 * 1024 * 1024,
        })
    }
    fn controlled(quota: QuotaGroup) -> (MediaOwner, mpsc::Receiver<Event>, mpsc::Sender<()>) {
        let (events_tx, events_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let owner = MediaOwner::start_with(quota, move || {
            Ok(Controlled {
                events: events_tx,
                release: Some(release_rx),
            })
        })
        .unwrap();
        (owner, events_rx, release_tx)
    }
    fn next(events: &mpsc::Receiver<Event>) -> Event {
        events.recv_timeout(Duration::from_secs(2)).unwrap()
    }
    fn joined(owner: MediaOwner) {
        let ticket = owner.owner.as_ref().unwrap().ticket();
        drop(owner);
        assert_eq!(
            ticket
                .join_until(Instant::now() + Duration::from_secs(2))
                .unwrap(),
            ilium_platform::owned_worker::WorkerExit::Joined
        );
    }

    #[test]
    fn cancellation_stops_new_player_effects_but_restores_in_flight_acknowledgement() {
        struct Inventory {
            events: mpsc::Sender<Event>,
            release: mpsc::Receiver<()>,
        }
        impl Backend for Inventory {
            fn pause(&mut self, should_stop: &(dyn Fn() -> bool + Sync)) -> Vec<String> {
                let mut acknowledged = Vec::new();
                for player in ["first", "second"] {
                    if should_stop() {
                        break;
                    }
                    self.events.send(Event::PauseEntered).unwrap();
                    self.release.recv_timeout(Duration::from_secs(2)).unwrap();
                    acknowledged.push(player.to_owned());
                }
                self.events.send(Event::PauseFinished).unwrap();
                acknowledged
            }
            fn resume(&mut self, players: Vec<String>) {
                self.events.send(Event::Restored(players)).unwrap();
            }
        }
        let (events_tx, events) = mpsc::channel();
        let (release, waiting) = mpsc::channel();
        let owner = MediaOwner::start_with(quota(), move || {
            Ok(Inventory {
                events: events_tx,
                release: waiting,
            })
        })
        .unwrap();
        owner.normal().request(true);
        assert_eq!(next(&events), Event::PauseEntered);
        owner.cancel();
        release.send(()).unwrap();
        assert_eq!(next(&events), Event::PauseFinished);
        assert_eq!(next(&events), Event::Restored(vec!["first".into()]));
        joined(owner);
    }

    #[test]
    fn stop_during_blocked_pause_restores_exact_acknowledged_set() {
        let (owner, events, release) = controlled(quota());
        let lease = owner.normal();
        lease.request(true);
        assert_eq!(next(&events), Event::PauseEntered);
        // Pause is still blocked. Submitting Stop touches only scalar state.
        lease.request(false);
        assert!(!lease.is_requested());
        release.send(()).unwrap();
        assert_eq!(next(&events), Event::PauseFinished);
        assert_eq!(
            next(&events),
            Event::Restored(vec!["org.mpris.MediaPlayer2.synthetic-owned-player".into()])
        );
        joined(owner);
    }

    #[test]
    fn overlapping_leases_do_not_restore_media_before_last_release() {
        let (owner, events, release) = controlled(quota());
        let normal = owner.normal();
        let demo = owner.demonstration();
        normal.request(true);
        assert_eq!(next(&events), Event::PauseEntered);
        demo.request(true);
        normal.request(false);
        release.send(()).unwrap();
        assert_eq!(next(&events), Event::PauseFinished);
        let early = events.recv_timeout(Duration::from_millis(30));
        demo.request(false);
        assert!(matches!(early, Err(mpsc::RecvTimeoutError::Timeout)));
        assert!(matches!(next(&events), Event::Restored(_)));
        joined(owner);
    }

    #[test]
    fn restart_restores_old_set_before_another_pause() {
        let (owner, events, release) = controlled(quota());
        let lease = owner.normal();
        lease.request(true);
        assert_eq!(next(&events), Event::PauseEntered);
        lease.restart();
        release.send(()).unwrap();
        assert_eq!(next(&events), Event::PauseFinished);
        assert!(matches!(next(&events), Event::Restored(_)));
        assert_eq!(next(&events), Event::PauseEntered);
        assert_eq!(next(&events), Event::PauseFinished);
        joined(owner);
        assert!(matches!(next(&events), Event::Restored(_)));
    }

    #[test]
    fn blocked_drop_retains_physical_admission_until_actual_join() {
        let quota = quota();
        let (owner, events, release) = controlled(quota.clone());
        owner.normal().request(true);
        assert_eq!(next(&events), Event::PauseEntered);
        let ticket = owner.owner.as_ref().unwrap().ticket();
        drop(owner);
        assert!(matches!(
            quota.reserve_external_worker(1, 0),
            Err(RejectReason::WorkerLimit)
        ));
        assert!(ticket.exit().is_none());
        release.send(()).unwrap();
        assert_eq!(next(&events), Event::PauseFinished);
        assert!(matches!(next(&events), Event::Restored(_)));
        assert_eq!(
            ticket
                .join_until(Instant::now() + Duration::from_secs(2))
                .unwrap(),
            ilium_platform::owned_worker::WorkerExit::Joined
        );
        drop(ticket);
        assert!(quota.reserve_external_worker(1, 0).is_ok());
    }
}
