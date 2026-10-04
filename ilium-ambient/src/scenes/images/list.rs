//! Custody of an original discovered list across worker/publication/scene.
//! Legacy discovery's prepublication allocation phase remains separately open.
use super::worker::Entry;
use crate::resources::{AmbientResources, Stored};
use ilium_execution::RejectReason;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug)]
pub(super) struct DiscoveredList {
    pub entries: Vec<Entry>,
    pub notes: Vec<String>,
}
pub(super) type SharedList = Arc<Stored<DiscoveredList>>;

impl DiscoveredList {
    fn storage_bytes(&self) -> Option<usize> {
        let Self { entries, notes } = self;
        let mut bytes = entries
            .capacity()
            .checked_mul(std::mem::size_of::<Entry>())?
            .checked_add(
                notes
                    .capacity()
                    .checked_mul(std::mem::size_of::<String>())?,
            )?
            .checked_add(std::mem::size_of::<Stored<Self>>())?
            .checked_add(std::mem::size_of::<ilium_execution::StorageAdmission>())?
            .checked_add(4 * std::mem::size_of::<usize>())?;
        for entry in entries {
            let Entry { source, name } = entry;
            let source_bytes = match source {
                super::worker::EntrySource::File(path) => path.capacity(),
                super::worker::EntrySource::Url(url) => url.capacity(),
            };
            bytes = bytes
                .checked_add(source_bytes)?
                .checked_add(name.capacity())?;
        }
        for note in notes {
            bytes = bytes.checked_add(note.capacity())?;
        }
        Some(bytes)
    }
}

/// Refusal returns BOTH original vectors and their exact original allocations.
fn try_retain(
    value: DiscoveredList,
    resources: &AmbientResources,
) -> Result<SharedList, (RejectReason, DiscoveredList)> {
    let Some(bytes) = value.storage_bytes() else {
        return Err((RejectReason::InvalidCost, value));
    };
    if bytes
        > resources
            .finite()
            .quota_group()
            .snapshot()
            .limits
            .worker_bytes
    {
        return Err((RejectReason::InvalidCost, value));
    }
    match resources.reserve_storage(bytes) {
        Ok(storage) => Ok(Arc::new(Stored::new(value, storage))),
        Err(reason) => Err((reason, value)),
    }
}

/// Pressure does not turn a complete list into a partial/empty successful list.
/// Native collector owns the original on retry; no list clone or second bank.
pub(super) fn retain(
    mut value: DiscoveredList,
    resources: &AmbientResources,
    stop: &AtomicBool,
) -> Result<SharedList, (RejectReason, DiscoveredList)> {
    loop {
        if stop.load(Ordering::Acquire) {
            return Err((RejectReason::Closed, value));
        }
        match try_retain(value, resources) {
            Ok(shared) => return Ok(shared),
            Err((RejectReason::Busy | RejectReason::WorkerBytes, original)) => {
                value = original;
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(rejected) => return Err(rejected),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
    };
    fn isolated() -> (Execution, AmbientResources, QuotaGroup) {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 1,
            jobs: 1,
            service_jobs: 0,
            input_bytes: 1024,
            result_bytes: 1024,
            worker_threads: 1,
            worker_bytes: 65536,
        });
        let empty = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        let execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: LaneConfig {
                    threads: 1,
                    queue_slots: 1,
                    priority: None,
                    resident_bytes_per_thread: 1024,
                },
                io: empty,
                service: empty,
            },
        )
        .unwrap();
        let resources = AmbientResources::new(
            execution
                .client(ClientLimits {
                    jobs: 1,
                    service_jobs: 0,
                    input_bytes: 1024,
                    result_bytes: 1024,
                })
                .unwrap(),
        );
        (execution, resources, quota)
    }
    fn original() -> DiscoveredList {
        let mut entries = Vec::with_capacity(16);
        let mut path = std::path::PathBuf::from("original.bmp");
        path.reserve(4096);
        let mut entry = Entry::file(path);
        entry.name.reserve(256);
        entries.push(entry);
        let mut notes = Vec::with_capacity(8);
        let mut note = "original note".to_owned();
        note.reserve(512);
        notes.push(note);
        DiscoveredList { entries, notes }
    }
    #[test]
    fn original_list_and_spare_capacity_follow_every_last_consumer_after_bank_exit() {
        let (mut execution, resources, quota) = isolated();
        let value = original();
        let entry_pointer = value.entries.as_ptr();
        let note_pointer = value.notes[0].as_ptr();
        let declared = value.storage_bytes().unwrap();
        assert!(
            declared > 4096 + 256 + 512,
            "spare containers and strings count"
        );
        let before = quota.snapshot().worker_bytes;
        let shared = try_retain(value, &resources).unwrap();
        assert_eq!(quota.snapshot().worker_bytes - before, declared);
        assert_eq!(shared.view().entries.as_ptr(), entry_pointer);
        assert_eq!(shared.view().notes[0].as_ptr(), note_pointer);
        let scene = shared.clone();
        let queued_event = shared.clone();
        execution.request_shutdown(ShutdownMode::Drain);
        execution
            .join_until_background(std::time::Instant::now() + std::time::Duration::from_secs(5))
            .unwrap();
        drop(resources);
        drop(execution);
        drop(shared);
        drop(queued_event);
        assert_eq!(quota.snapshot().worker_bytes, declared);
        assert_eq!(scene.view().entries.as_ptr(), entry_pointer);
        drop(scene);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn storage_pressure_returns_original_complete_list_until_real_retry() {
        let (mut execution, resources, quota) = isolated();
        let pressure = resources
            .reserve_storage(65536 - quota.snapshot().worker_bytes)
            .unwrap();
        let original = original();
        let pointer = original.entries.as_ptr();
        let (reason, original) = try_retain(original, &resources).unwrap_err();
        assert_eq!(reason, RejectReason::WorkerBytes);
        assert_eq!(original.entries.as_ptr(), pointer);
        assert_eq!(original.entries.len(), 1);
        assert_eq!(original.notes.len(), 1);
        drop(pressure);
        let shared = try_retain(original, &resources).unwrap();
        assert_eq!(shared.view().entries.as_ptr(), pointer);
        execution.request_shutdown(ShutdownMode::Drain);
        execution
            .join_until_background(std::time::Instant::now() + std::time::Duration::from_secs(5))
            .unwrap();
        drop(shared);
        drop(resources);
        drop(execution);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn canceled_list_publication_returns_original_and_never_a_successful_empty_list() {
        let (mut execution, resources, quota) = isolated();
        let original = original();
        let pointer = original.entries.as_ptr();
        let (reason, original) = retain(original, &resources, &AtomicBool::new(true)).unwrap_err();
        assert_eq!(reason, RejectReason::Closed);
        assert_eq!(original.entries.as_ptr(), pointer);
        assert_eq!(original.entries.len(), 1);
        drop(original);
        execution.request_shutdown(ShutdownMode::Drain);
        execution
            .join_until_background(std::time::Instant::now() + std::time::Duration::from_secs(5))
            .unwrap();
        drop(resources);
        drop(execution);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn impossible_declared_list_returns_original_without_endless_pressure_retry() {
        let (mut execution, resources, quota) = isolated();
        let mut original = original();
        original.entries[0].name.reserve(131072);
        let pointer = original.entries.as_ptr();
        let (reason, original) = retain(original, &resources, &AtomicBool::new(false)).unwrap_err();
        assert_eq!(reason, RejectReason::InvalidCost);
        assert_eq!(original.entries.as_ptr(), pointer);
        assert_eq!(original.entries.len(), 1);
        drop(original);
        execution.request_shutdown(ShutdownMode::Drain);
        execution
            .join_until_background(std::time::Instant::now() + std::time::Duration::from_secs(5))
            .unwrap();
        drop(resources);
        drop(execution);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}
