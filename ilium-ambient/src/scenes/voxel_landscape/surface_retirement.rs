//! Bounded ownership transfer for expensive world/model destruction. Only the
//! preparation worker drains this queue; a busy worker makes the scene retain
//! its current snapshot instead of destroying one on the presentation thread.
use std::sync::Mutex;

pub struct Retirement<T> {
    items: Mutex<Vec<T>>,
}

impl<T> Retirement<T> {
    pub fn new() -> Self {
        Self {
            items: Mutex::new(Vec::with_capacity(3)),
        }
    }

    pub fn try_retire(&self, item: T) -> Result<(), T> {
        let Ok(mut items) = self.items.try_lock() else {
            return Err(item);
        };
        if items.len() >= 2 {
            return Err(item);
        }
        items.push(item);
        Ok(())
    }

    /// Called once during scene teardown, before raising the worker stop flag.
    /// The worker only holds this lock to transfer up to three pointers, never
    /// while preparing or destroying their contents.
    pub fn retire_final(&self, item: T) {
        let mut items = self
            .items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        items.push(item);
    }

    /// Worker-only: release the lock before dropping the returned snapshots.
    pub fn drain(&self) -> Vec<T> {
        let mut items = self
            .items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::replace(&mut *items, Vec::with_capacity(3))
    }
}

impl<T> Default for Retirement<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Arc};

    #[test]
    fn busy_worker_bounds_retirement_and_returns_ownership_without_dropping() {
        let queue = Retirement::new();
        assert!(queue.try_retire("first").is_ok());
        assert!(queue.try_retire("second").is_ok());
        assert_eq!(queue.try_retire("current"), Err("current"));
        queue.retire_final("current");
        assert_eq!(queue.drain(), vec!["first", "second", "current"]);
        assert!(queue.drain().is_empty());
    }

    #[test]
    fn snapshots_are_destroyed_by_the_worker_after_lock_release() {
        struct Snapshot(mpsc::Sender<std::thread::ThreadId>);
        impl Drop for Snapshot {
            fn drop(&mut self) {
                self.0.send(std::thread::current().id()).unwrap();
            }
        }
        let (sender, receiver) = mpsc::channel();
        let queue = Arc::new(Retirement::new());
        assert!(queue.try_retire(Snapshot(sender)).is_ok());
        assert!(receiver.try_recv().is_err());
        let worker_queue = Arc::clone(&queue);
        let worker = std::thread::spawn(move || {
            let items = worker_queue.drain();
            assert!(worker_queue.items.try_lock().is_ok());
            drop(items);
            std::thread::current().id()
        });
        assert_eq!(receiver.recv().unwrap(), worker.join().unwrap());
    }
}
