//! Bounded, cancellation-safe dialog snapshots; never used for launch admission.
use std::{
    collections::HashMap,
    future::Future,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, Semaphore};
type Key = (PathBuf, PathBuf);
type Snapshot<T> = Option<(u64, Instant, Result<T, String>)>;
type Slot<T> = Arc<Mutex<Snapshot<T>>>;
/// At most 32 resident/in-flight keys and four Git snapshots across connections.
pub(crate) struct FactsCache<T> {
    entries: std::sync::Mutex<HashMap<Key, Slot<T>>>,
    generation: AtomicU64,
    workers: Semaphore,
}
impl<T> Default for FactsCache<T> {
    fn default() -> Self {
        Self {
            entries: Default::default(),
            generation: AtomicU64::new(0),
            workers: Semaphore::new(4),
        }
    }
}
impl<T: Clone> FactsCache<T> {
    pub(crate) fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }
    pub(crate) async fn get<F>(&self, key: Key, compute: F) -> Result<T, String>
    where
        F: Future<Output = Result<T, String>>,
    {
        let slot = {
            let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            if !entries.contains_key(&key) && entries.len() >= 32 {
                // Never evict a flight: that would create duplicate producers.
                let victim = entries
                    .iter()
                    .find(|(_, slot)| Arc::strong_count(slot) == 1)
                    .map(|(key, _)| key.clone());
                if let Some(victim) = victim {
                    entries.remove(&victim);
                } else {
                    return Err("repository snapshot capacity reached; retry shortly".into());
                }
            }
            entries.entry(key).or_default().clone()
        };
        // Cancellation drops the guard and permit; another waiter can compute.
        let mut snapshot = slot.lock().await;
        let generation = self.generation.load(Ordering::Acquire);
        if let Some((saved_generation, expires, result)) = &*snapshot {
            if *saved_generation == generation && *expires > Instant::now() {
                return result.clone();
            }
        }
        let _permit = self
            .workers
            .acquire()
            .await
            .map_err(|_| "repository snapshots shut down".to_string())?;
        let result = compute.await;
        let lifetime = if result.is_ok() {
            Duration::from_secs(1)
        } else {
            Duration::from_millis(100)
        };
        *snapshot = Some((generation, Instant::now() + lifetime, result.clone()));
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn non_repository_discovery_errors_are_coalesced_and_invalidated() {
        let directory = tempfile::tempdir().unwrap();
        let cache = FactsCache::<ilium_git::Repository>::default();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let key = (directory.path().to_path_buf(), PathBuf::new());
        for _ in 0..2 {
            assert!(cache
                .get(key.clone(), async {
                    calls.fetch_add(1, Ordering::Relaxed);
                    ilium_git::discover(directory.path())
                        .await
                        .map_err(|error| error.to_string())
                })
                .await
                .is_err());
        }
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        cache.invalidate();
        assert!(cache
            .get(key, async {
                calls.fetch_add(1, Ordering::Relaxed);
                ilium_git::discover(directory.path())
                    .await
                    .map_err(|error| error.to_string())
            })
            .await
            .is_err());
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }
    #[tokio::test]
    async fn active_flights_are_not_evicted_and_capacity_is_finite() {
        let cache = FactsCache::<usize>::default();
        let mut active = Vec::new();
        {
            let mut entries = cache.entries.lock().unwrap();
            for index in 0..32 {
                let slot: Slot<usize> = Arc::new(Mutex::new(None));
                entries.insert(
                    (PathBuf::from(index.to_string()), PathBuf::new()),
                    slot.clone(),
                );
                active.push(slot);
            }
        }
        assert!(cache
            .get((PathBuf::from("overflow"), PathBuf::new()), async {
                Ok(99)
            })
            .await
            .is_err());
        drop(active);
        assert_eq!(
            cache
                .get((PathBuf::from("overflow"), PathBuf::new()), async {
                    Ok(99)
                })
                .await
                .unwrap(),
            99
        );
    }
    #[tokio::test]
    async fn expires_success_and_error_and_bounds_entries() {
        let cache = FactsCache::<usize>::default();
        let key = (PathBuf::from("/project"), PathBuf::from("/repo"));
        assert!(cache
            .get(key.clone(), async { Err("temporary".into()) })
            .await
            .is_err());
        {
            let entries = cache.entries.lock().unwrap();
            let mut slot = entries.get(&key).unwrap().try_lock().unwrap();
            slot.as_mut().unwrap().1 = Instant::now();
        }
        assert_eq!(cache.get(key.clone(), async { Ok(3) }).await.unwrap(), 3);
        {
            let entries = cache.entries.lock().unwrap();
            let mut slot = entries.get(&key).unwrap().try_lock().unwrap();
            slot.as_mut().unwrap().1 = Instant::now();
        }
        assert_eq!(cache.get(key, async { Ok(4) }).await.unwrap(), 4);
        for index in 0..64 {
            cache
                .get((PathBuf::from(index.to_string()), PathBuf::new()), async {
                    Ok(index)
                })
                .await
                .unwrap();
        }
        assert_eq!(cache.entries.lock().unwrap().len(), 32);
    }
    #[tokio::test]
    async fn concurrent_waiters_share_one_producer() {
        let cache = Arc::new(FactsCache::<usize>::default());
        let key = (PathBuf::from("/project"), PathBuf::from("/repo"));
        let owner = cache.clone();
        let first_key = key.clone();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (finish, finished) = tokio::sync::oneshot::channel();
        let first = tokio::spawn(async move {
            owner
                .get(first_key, async {
                    let _ = started.send(());
                    let _ = finished.await;
                    Ok(5)
                })
                .await
        });
        ready.await.unwrap();
        let owner = cache.clone();
        let second =
            tokio::spawn(
                async move { owner.get(key, async { panic!("duplicate producer") }).await },
            );
        finish.send(()).unwrap();
        assert_eq!(first.await.unwrap().unwrap(), 5);
        assert_eq!(second.await.unwrap().unwrap(), 5);
    }
    #[tokio::test]
    async fn coalesces_and_invalidates() {
        let cache = FactsCache::<usize>::default();
        let key = (PathBuf::from("/project"), PathBuf::from("/repo"));
        assert_eq!(cache.get(key.clone(), async { Ok(7) }).await.unwrap(), 7);
        assert_eq!(
            cache
                .get(key.clone(), async { panic!("must reuse snapshot") })
                .await
                .unwrap(),
            7
        );
        cache.invalidate();
        assert_eq!(cache.get(key, async { Ok(8) }).await.unwrap(), 8);
    }
    #[tokio::test]
    async fn cancelled_leader_does_not_strand_waiters() {
        let cache = Arc::new(FactsCache::<usize>::default());
        let key = (PathBuf::from("/project"), PathBuf::from("/repo"));
        let (started, ready) = tokio::sync::oneshot::channel();
        let owner = cache.clone();
        let leader_key = key.clone();
        let leader = tokio::spawn(async move {
            owner
                .get(leader_key, async {
                    let _ = started.send(());
                    std::future::pending().await
                })
                .await
        });
        ready.await.unwrap();
        leader.abort();
        let _ = leader.await;
        assert_eq!(cache.get(key, async { Ok(9) }).await.unwrap(), 9);
    }
}
