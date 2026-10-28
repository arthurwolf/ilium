//! One estimate model for the resource detail and native wait strip.
use super::tree::ordered;
use ilium_core::resource_queue::*;

pub(super) struct QueuePresentation {
    pub weights: Vec<u64>,
    pub snapshot: Option<ResourceQueueSnapshot>,
    pub uncertainty: Option<String>,
}
pub(super) fn project(
    record: &ResourceRecord,
    clock: Option<&ResourceClock>,
    observation_error: Option<&str>,
) -> QueuePresentation {
    let snapshot = clock.map(|clock| project_resource_queue(record, clock));
    let uncertainty = observation_error
        .map(str::to_owned)
        .or_else(|| {
            snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.observation_reason.clone())
        })
        .or_else(|| {
            clock
                .is_none()
                .then(|| "Authority clock unavailable".into())
        });
    let weights = ordered(record)
        .into_iter()
        .map(|reservation| {
            if !reservation.state.occupied() {
                return reservation.estimate_seconds.max(1);
            }
            // The core projection supplies the authority's uncertainty. This subtraction
            // is only geometry and cannot produce a grant or a completed countdown.
            clock
                .zip(reservation.acquired_at.as_ref())
                .filter(|(clock, acquired)| clock.boot_id == acquired.boot_id)
                .and_then(|(clock, acquired)| {
                    clock
                        .monotonic_millis
                        .checked_sub(acquired.monotonic_millis)
                })
                .and_then(|elapsed| reservation.estimate_seconds.checked_sub(elapsed / 1000))
                .unwrap_or(1)
                .max(1)
        })
        .collect();
    QueuePresentation {
        weights,
        snapshot,
        uncertainty,
    }
}
