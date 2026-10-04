//! Admission for the two process-persistent source decoder baselines.
//! Their native OnceLock/interner allocations never retire on package closure,
//! so the first genuine process quota and its lease remain retained for process
//! lifetime. This is logical admission, not a measured allocator/RSS bound.
use crate::error::{AnimationError, Result};
use ilium_execution::{QuotaGroup, StorageAdmission};
use std::sync::Mutex;

struct RetainedBaseline {
    quota: QuotaGroup,
    _admission: StorageAdmission,
}
#[derive(Default)]
struct Baselines {
    stars: Option<RetainedBaseline>,
    atoms: Option<RetainedBaseline>,
}
// Inline slots avoid allocating an uncharged registry during lazy admission.
static BASELINES: Mutex<Baselines> = Mutex::new(Baselines {
    stars: None,
    atoms: None,
});
impl Baselines {
    fn admit(&mut self, quota: &QuotaGroup, key: &str, bytes: usize) -> Result<()> {
        let (slot, required) = match key {
            "yale_bright_stars" => (&mut self.stars, 4 * 1024 * 1024),
            "html5ever_atoms" => (
                &mut self.atoms,
                ilium_wikipedia::ARTICLE_ATOM_BASELINE_BYTES,
            ),
            _ => {
                return Err(AnimationError::Runtime(
                    "unknown native source baseline".into(),
                ))
            }
        };
        if bytes != required {
            return Err(AnimationError::Budget(
                "native source baseline size mismatch".into(),
            ));
        }
        if let Some(retained) = slot {
            return if retained.quota.shares_root(quota) {
                Ok(())
            } else {
                Err(AnimationError::Budget(
                    "native source baseline belongs to another process quota".into(),
                ))
            };
        }
        let admission = quota.reserve_external_storage(required).map_err(|error| {
            AnimationError::Budget(format!("native source process baseline: {error:?}"))
        })?;
        *slot = Some(RetainedBaseline {
            quota: quota.clone(),
            _admission: admission,
        });
        Ok(())
    }
}
/// Call before entering the corresponding native catalogue/parser. The caller
/// still owns actual operation authorization and finite execution admission;
/// possession of this baseline lease does not grant either capability.
pub(crate) fn admit_process_baseline(quota: &QuotaGroup, key: &str, bytes: usize) -> Result<()> {
    BASELINES
        .lock()
        .map_err(|_| AnimationError::Runtime("native source baseline owner poisoned".into()))?
        .admit(quota, key, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn quota(bytes: usize) -> QuotaGroup {
        QuotaGroup::new(ilium_execution::QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 0,
            worker_bytes: bytes,
        })
    }
    #[test]
    fn denied_admission_has_no_slot_or_debit_and_can_retry() {
        let small = quota(1);
        let real = quota(4 * 1024 * 1024);
        let mut baselines = Baselines::default();
        assert!(baselines
            .admit(&small, "yale_bright_stars", 4 * 1024 * 1024)
            .is_err());
        assert!(baselines.stars.is_none());
        assert_eq!(small.snapshot().worker_bytes, 0);
        baselines
            .admit(&real, "yale_bright_stars", 4 * 1024 * 1024)
            .unwrap();
        assert_eq!(real.snapshot().worker_bytes, 4 * 1024 * 1024);
        assert!(baselines
            .admit(&small, "yale_bright_stars", 4 * 1024 * 1024)
            .is_err());
        assert!(baselines.admit(&real, "yale_bright_stars", 1).is_err());
        assert!(baselines.admit(&real, "guest_selected_key", 1).is_err());
        assert_eq!(real.snapshot().worker_bytes, 4 * 1024 * 1024);
        drop(baselines);
        assert_eq!(real.snapshot().worker_bytes, 0);
    }
    #[test]
    fn concurrent_same_original_root_admits_once() {
        let original = quota(4 * 1024 * 1024);
        let baselines = Mutex::new(Baselines::default());
        std::thread::scope(|scope| {
            let mut workers = Vec::new();
            for _ in 0..8 {
                let root = original.clone();
                let owner = &baselines;
                workers.push(scope.spawn(move || {
                    owner
                        .lock()
                        .unwrap()
                        .admit(&root, "yale_bright_stars", 4 * 1024 * 1024)
                }));
            }
            for worker in workers {
                worker.join().unwrap().unwrap();
            }
        });
        assert_eq!(original.snapshot().worker_bytes, 4 * 1024 * 1024);
        drop(baselines);
        assert_eq!(original.snapshot().worker_bytes, 0);
    }
}
