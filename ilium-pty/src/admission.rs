//! Server PTY worker capacity is reserved before a child process is started.
//! Each native owner retains a clone of one shared debit until physical join.

use ilium_execution::QuotaGroup;
use ilium_platform::owned_worker::{reserve_owned_worker, WorkerReservation};
use std::{io, sync::Arc};

/// Explicit stack size of every PTY worker thread. This is a virtual
/// reservation that the kernel backs with memory only as a thread actually
/// touches it; 4 MiB doubles Rust's 2 MiB default so deep terminal parsing
/// never needs to be the reason a pane fails.
pub const PTY_WORKER_STACK_BYTES: usize = 4 * 1024 * 1024;
/// Persistent OS workers one PTY session owns until its physical join.
pub const PTY_WORKERS_PER_SESSION: usize = if cfg!(windows) { 6 } else { 5 };
const WORKER_RESIDENT_BYTES: usize = PTY_WORKER_STACK_BYTES;
const PTY_WORKER_COUNT: usize = PTY_WORKERS_PER_SESSION;

type Custody = Arc<dyn Send + Sync>;
pub(crate) type PtyWorkerReservation = WorkerReservation<Custody>;

pub(crate) struct PtyWorkerReservations {
    pub(crate) child_reaper: PtyWorkerReservation,
    pub(crate) owner: PtyOwnerReservations,
}

pub(crate) struct PtyOwnerReservations {
    pub(crate) expiry: PtyWorkerReservation,
    pub(crate) writer: PtyWorkerReservation,
    pub(crate) reader: PtyWorkerReservation,
    pub(crate) owner: PtyWorkerReservation,
    pub(crate) native_writer: Option<PtyWorkerReservation>,
}

impl PtyWorkerReservations {
    pub(crate) fn new(quota: Option<&QuotaGroup>) -> io::Result<Self> {
        let (custody, count, stack_bytes) = if let Some(quota) = quota {
            let count = PTY_WORKER_COUNT;
            let resident = count * WORKER_RESIDENT_BYTES;
            let admission = quota
                .reserve_external_worker(count, resident)
                .map_err(|reason| {
                    io::Error::other(format!("PTY worker admission rejected: {reason:?}"))
                })?;
            (
                Arc::new(admission) as Custody,
                count,
                Some(WORKER_RESIDENT_BYTES),
            )
        } else {
            (Arc::new(()) as Custody, PTY_WORKER_COUNT, None)
        };
        let mut reservations = Vec::with_capacity(count);
        for _ in 0..count {
            reservations.push(reserve_owned_worker(stack_bytes, Arc::clone(&custody))?);
        }
        let mut reservations = reservations.into_iter();
        Ok(Self {
            child_reaper: reservations.next().unwrap(),
            owner: PtyOwnerReservations {
                expiry: reservations.next().unwrap(),
                writer: reservations.next().unwrap(),
                reader: reservations.next().unwrap(),
                owner: reservations.next().unwrap(),
                native_writer: cfg!(windows).then(|| reservations.next().unwrap()),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_execution::QuotaLimits;

    fn quota(worker_threads: usize, worker_bytes: usize) -> QuotaGroup {
        QuotaGroup::new(QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads,
            worker_bytes,
        })
    }

    #[test]
    fn pty_workers_reserve_and_release_the_complete_physical_set() {
        let quota = quota(PTY_WORKER_COUNT, PTY_WORKER_COUNT * WORKER_RESIDENT_BYTES);
        let reservations = PtyWorkerReservations::new(Some(&quota)).unwrap();
        assert_eq!(quota.snapshot().worker_threads, PTY_WORKER_COUNT);
        assert_eq!(
            quota.snapshot().worker_bytes,
            PTY_WORKER_COUNT * WORKER_RESIDENT_BYTES
        );
        drop(reservations);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn pty_worker_overload_refuses_before_reserving_a_partial_set() {
        let quota = quota(
            PTY_WORKER_COUNT - 1,
            (PTY_WORKER_COUNT - 1) * WORKER_RESIDENT_BYTES,
        );
        assert!(PtyWorkerReservations::new(Some(&quota)).is_err());
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}
