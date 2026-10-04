//! Original failure text custody. Allocation of legacy provider/codec errors is
//! still audited separately; this owner prevents publication from releasing the
//! admitted immutable original while scene/cache consumers retain it.
use crate::resources::{AmbientResources, Stored};
use ilium_execution::{RejectReason, Retention, StorageAdmission};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

pub(super) const FALLBACK: &str = "Image failure could not be published: resource owner retired";
pub(super) fn fallback_bytes() -> usize {
    FALLBACK.len() + std::mem::size_of::<Stored<String>>() + 2 * std::mem::size_of::<usize>()
}
pub(super) fn fallback(storage: Arc<StorageAdmission>) -> Arc<Stored<String>> {
    Arc::new(Stored::new(FALLBACK.to_owned(), storage))
}
fn bytes(message: &String) -> Option<usize> {
    message
        .capacity()
        .checked_add(std::mem::size_of::<Stored<String>>())?
        .checked_add(std::mem::size_of::<StorageAdmission>())?
        .checked_add(4 * std::mem::size_of::<usize>())
}
pub(super) fn retain(
    message: String,
    resources: &AmbientResources,
    stop: &AtomicBool,
    emergency: &Arc<Stored<String>>,
    retention: Option<Retention>,
) -> Arc<Stored<String>> {
    // A finite callback receipt, when supplied, stays alive through storage
    // admission. A cloned guard never authorizes a heap clone of its message.
    let _retention = retention;
    let Some(bytes) = bytes(&message) else {
        tracing::error!("image failure storage cost overflow");
        return emergency.clone();
    };
    loop {
        if stop.load(Ordering::Acquire) {
            return emergency.clone();
        }
        match resources.reserve_storage(bytes) {
            Ok(storage) => return Arc::new(Stored::new(message, storage)),
            Err(RejectReason::WorkerBytes | RejectReason::Busy) => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(reason) => {
                tracing::error!(?reason, "image failure publication admission refused");
                return emergency.clone();
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use ilium_execution::ShutdownMode;
    fn isolated() -> (
        ilium_execution::Execution,
        AmbientResources,
        ilium_execution::QuotaGroup,
    ) {
        use ilium_execution::{
            ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
        };
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 1,
            jobs: 2,
            service_jobs: 0,
            input_bytes: 65536,
            result_bytes: 65536,
            worker_threads: 1,
            worker_bytes: 3 * 1024 * 1024,
        });
        let lane = LaneConfig {
            threads: 1,
            queue_slots: 2,
            priority: None,
            resident_bytes_per_thread: 1024,
        };
        let empty = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        let execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: lane,
                io: empty,
                service: empty,
            },
        )
        .unwrap();
        let resources = AmbientResources::new(
            execution
                .client(ClientLimits {
                    jobs: 2,
                    service_jobs: 0,
                    input_bytes: 65536,
                    result_bytes: 65536,
                })
                .unwrap(),
        );
        (execution, resources, quota)
    }
    #[test]
    fn original_failure_pointer_follows_scene_and_last_error_consumers() {
        let (mut execution, resources, quota) = isolated();
        let emergency = fallback(resources.reserve_storage(fallback_bytes()).unwrap());
        let baseline = quota.snapshot().worker_bytes;
        let message = "original decoder failure".to_owned();
        let pointer = message.as_ptr();
        let original = retain(
            message,
            &resources,
            &AtomicBool::new(false),
            &emergency,
            None,
        );
        assert_eq!(original.view().as_ptr(), pointer);
        let retained = quota.snapshot().worker_bytes;
        assert!(retained > baseline);
        let scene = original.clone();
        let last_error = original.clone();
        drop(original);
        drop(scene);
        assert_eq!(last_error.view().as_ptr(), pointer);
        assert_eq!(last_error.view(), "original decoder failure");
        assert_eq!(quota.snapshot().worker_bytes, retained);
        drop(last_error);
        assert_eq!(quota.snapshot().worker_bytes, baseline);
        drop(emergency);
        execution.request_shutdown(ShutdownMode::Drain);
        execution
            .join_until_background(std::time::Instant::now() + Duration::from_secs(5))
            .unwrap();
        drop(resources);
        drop(execution);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn storage_refusal_keeps_finite_message_credit_until_original_publication() {
        let (mut execution, resources, quota) = isolated();
        let emergency = fallback(resources.reserve_storage(fallback_bytes()).unwrap());
        let claim = resources
            .finite()
            .try_reserve_external(ilium_execution::JobCost {
                input_bytes: 1024,
                result_bytes: 1024,
            })
            .unwrap();
        let original = claim.retain("finite original error".to_owned()).unwrap();
        let (message, retention) = original.into_parts();
        let pointer = message.as_ptr() as usize;
        let pressure = resources
            .reserve_storage(quota.snapshot().limits.worker_bytes - quota.snapshot().worker_bytes)
            .unwrap();
        let worker_resources = resources.clone();
        let fallback = emergency.clone();
        let (publish, receive) = std::sync::mpsc::sync_channel(1);
        let producer = std::thread::spawn(move || {
            let outcome = retain(
                message,
                &worker_resources,
                &AtomicBool::new(false),
                &fallback,
                Some(retention),
            );
            publish.send(outcome).unwrap();
        });
        assert!(
            matches!(
                receive.recv_timeout(Duration::from_millis(200)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ),
            "full physical storage must prevent publication"
        );
        assert_eq!(quota.snapshot().jobs, 1);
        assert_eq!(quota.snapshot().input_bytes, 1024);
        assert_eq!(quota.snapshot().result_bytes, 1024);
        drop(pressure);
        let published = receive.recv_timeout(Duration::from_secs(5)).unwrap();
        producer.join().unwrap();
        assert_eq!(published.view().as_ptr() as usize, pointer);
        assert_eq!(published.view(), "finite original error");
        assert_eq!(quota.snapshot().jobs, 0);
        execution.request_shutdown(ShutdownMode::Drain);
        execution
            .join_until_background(std::time::Instant::now() + Duration::from_secs(5))
            .unwrap();
        drop(emergency);
        drop(resources);
        drop(execution);
        assert!(quota.snapshot().worker_bytes > 0);
        drop(published);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}
