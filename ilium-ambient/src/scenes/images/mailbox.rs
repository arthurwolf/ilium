//! Fixed inline FIFO custody. The slots bound metadata; each variable payload
//! still needs its own independently qualified allocation owner.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, TryLockError};
use std::time::Duration;

const RETRY: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Refusal {
    Busy,
    Full,
    Closed,
    Poisoned,
}
#[derive(Debug)]
pub(super) struct Rejected<T> {
    pub reason: Refusal,
    pub value: T,
}

struct Queue<T, const N: usize> {
    slots: [Option<T>; N],
    head: usize,
    len: usize,
    closed: bool,
}
impl<T, const N: usize> Queue<T, N> {
    fn push(&mut self, value: T) {
        self.slots[(self.head + self.len) % N] = Some(value);
        self.len += 1;
    }
    fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let value = self.slots[self.head].take();
        self.head = (self.head + 1) % N;
        self.len -= 1;
        value
    }
}
pub(super) struct Mailbox<T, const N: usize> {
    queue: Mutex<Queue<T, N>>,
    changed: Condvar,
    // Last: the same admitted mailbox allocations remain charged through every
    // scene/callback Arc and through destruction of all queued original values.
    _storage: Arc<ilium_execution::StorageAdmission>,
}
impl<T, const N: usize> Mailbox<T, N> {
    pub fn allocation_bytes() -> usize {
        std::mem::size_of::<Self>() + 2 * std::mem::size_of::<usize>()
    }
    pub fn new(storage: Arc<ilium_execution::StorageAdmission>) -> Arc<Self> {
        // All production instantiations have statically positive capacity.
        assert!(N > 0);
        Arc::new(Self {
            queue: Mutex::new(Queue {
                slots: std::array::from_fn(|_| None),
                head: 0,
                len: 0,
                closed: false,
            }),
            changed: Condvar::new(),
            _storage: storage,
        })
    }
    pub fn try_send(&self, value: T) -> Result<(), Rejected<T>> {
        let mut queue = match self.queue.try_lock() {
            Ok(queue) => queue,
            Err(TryLockError::WouldBlock) => {
                return Err(Rejected {
                    reason: Refusal::Busy,
                    value,
                })
            }
            Err(TryLockError::Poisoned(_)) => {
                return Err(Rejected {
                    reason: Refusal::Poisoned,
                    value,
                })
            }
        };
        let reason = if queue.closed {
            Some(Refusal::Closed)
        } else if queue.len == N {
            Some(Refusal::Full)
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(Rejected { reason, value });
        }
        queue.push(value);
        self.changed.notify_all();
        Ok(())
    }
    /// Background only. Refusal/cancellation returns the exact original owner.
    pub fn send(&self, mut value: T, stop: &AtomicBool) -> Result<(), Rejected<T>> {
        loop {
            if stop.load(Ordering::Acquire) {
                return Err(Rejected {
                    reason: Refusal::Closed,
                    value,
                });
            }
            match self.try_send(value) {
                Ok(()) => return Ok(()),
                Err(rejected) if matches!(rejected.reason, Refusal::Busy | Refusal::Full) => {
                    value = rejected.value
                }
                Err(rejected) => return Err(rejected),
            }
            self.wait();
        }
    }
    pub fn try_recv(&self) -> Option<T> {
        let mut queue = self.queue.try_lock().ok()?;
        let value = queue.pop();
        if value.is_some() {
            self.changed.notify_all();
        }
        value
    }
    pub fn recv(&self, stop: &AtomicBool) -> Option<T> {
        loop {
            if stop.load(Ordering::Acquire) {
                return None;
            }
            let mut queue = self.queue.lock().ok()?;
            if let Some(value) = queue.pop() {
                self.changed.notify_all();
                return Some(value);
            }
            if queue.closed {
                return None;
            }
            drop(self.changed.wait_timeout(queue, RETRY));
        }
    }
    fn wait(&self) {
        if let Ok(queue) = self.queue.lock() {
            drop(self.changed.wait_timeout(queue, RETRY));
        }
    }
    pub fn close(&self) {
        let mut queue = self
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        queue.closed = true;
        // Scene retirement explicitly cancels every queued original value.
        while queue.pop().is_some() {}
        self.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_mailbox_returns_original_allocation_and_retains_fifo() {
        let resources = crate::resources::test_resources();
        let mailbox = Mailbox::<String, 2>::new(
            resources
                .reserve_storage(Mailbox::<String, 2>::allocation_bytes())
                .unwrap(),
        );
        mailbox.try_send("first".to_owned()).unwrap();
        mailbox.try_send("second".to_owned()).unwrap();
        let original = String::from("third");
        let pointer = original.as_ptr();
        let rejected = mailbox.try_send(original).unwrap_err();
        assert_eq!(rejected.reason, Refusal::Full);
        assert_eq!(rejected.value.as_ptr(), pointer);
        assert_eq!(mailbox.try_recv().as_deref(), Some("first"));
        mailbox.try_send(rejected.value).unwrap();
        assert_eq!(mailbox.try_recv().as_deref(), Some("second"));
        assert_eq!(mailbox.try_recv().as_deref(), Some("third"));
        mailbox.close();
        assert_eq!(
            mailbox.try_send("retired".to_owned()).unwrap_err().reason,
            Refusal::Closed
        );
    }
    #[test]
    fn closing_full_mailbox_returns_original_to_retrying_publisher() {
        let resources = crate::resources::test_resources();
        let mailbox = Mailbox::<String, 1>::new(
            resources
                .reserve_storage(Mailbox::<String, 1>::allocation_bytes())
                .unwrap(),
        );
        mailbox.try_send("queued".to_owned()).unwrap();
        let original = String::from("blocked");
        let pointer = original.as_ptr() as usize;
        let producer = mailbox.clone();
        let (entered, arrival) = std::sync::mpsc::sync_channel(0);
        let worker = std::thread::spawn(move || {
            let rejected = producer.try_send(original).unwrap_err();
            assert_eq!(rejected.reason, Refusal::Full);
            entered.send(()).unwrap();
            producer.send(rejected.value, &AtomicBool::new(false))
        });
        arrival.recv_timeout(Duration::from_secs(2)).unwrap();
        mailbox.close();
        let rejected = worker.join().unwrap().unwrap_err();
        assert_eq!(rejected.reason, Refusal::Closed);
        assert_eq!(rejected.value.as_ptr() as usize, pointer);
        assert!(mailbox.try_recv().is_none());
    }
}
