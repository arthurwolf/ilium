//! Host-owned history-writer drain fence across disposable SavedScene instances.
//! All caller-side locks use `try_lock`; disk I/O belongs to the writer worker.
use super::{
    history_writer::{self, Writer},
    tours::History,
};
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Mutex, TryLockError,
};

// The UI may start a final-history handoff without taking a worker mutex.
// An overlapping gate/install/retire/reload operation makes that handoff fail
// sticky instead of allowing a successor to consume an older repository view.
const IDLE: u8 = 0;
const HANDOFF: u8 = 1;
const FAILED: u8 = 2;
const HANDOFF_FAILED: u8 = 3;
const OPERATION: u8 = 4;
const OPERATION_FAILED: u8 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// A new catalog worker may now perform its authoritative load/bind.
    Ready,
    /// A writer still owns accepted transactions; the next scene must wait.
    Draining,
    /// A writer failed or conflicted; an authoritative reload is mandatory.
    ReloadRequired,
    Busy,
    /// A panicked runtime cannot be treated as temporary lock contention.
    Poisoned,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("saved history runtime is busy")]
    Busy,
    #[error("saved history runtime lock poisoned")]
    Poisoned,
    #[error("saved history writer is not installed")]
    NoWriter,
    #[error("saved history writer is draining")]
    Draining,
    #[error("saved history requires authoritative reload")]
    ReloadRequired,
    #[error("saved history writer serial exhausted")]
    Serial,
    #[error(transparent)]
    Writer(#[from] history_writer::Error),
}

#[derive(Default)]
struct Inner {
    writer: Option<Writer>,
    serial: u64,
    retiring: bool,
    reload_required: bool,
}

/// The ambient host owns one Arc for its entire lifetime and passes clones to
/// scene environments. Rebuilding or hiding a scene cannot discard a draining
/// accepted writer and cannot let a successor race its repository revision.
#[derive(Default)]
pub struct SavedRuntime {
    inner: Mutex<Inner>,
    handoff: AtomicU8,
}
impl SavedRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fence the departing scene's latest complete History until its owned
    /// worker has either admitted it or recorded a terminal failure.
    pub fn begin_handoff(&self) -> Result<(), Error> {
        match self
            .handoff
            .compare_exchange(IDLE, HANDOFF, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => Ok(()),
            Err(HANDOFF | OPERATION) => {
                self.handoff.fetch_or(FAILED, Ordering::AcqRel);
                Err(Error::Draining)
            }
            Err(FAILED | HANDOFF_FAILED | OPERATION_FAILED) => Err(Error::ReloadRequired),
            Err(_) => Err(Error::ReloadRequired),
        }
    }

    pub fn finish_handoff(&self, accepted: bool) {
        loop {
            let old = self.handoff.load(Ordering::Acquire);
            let next = match old {
                HANDOFF if accepted => IDLE,
                HANDOFF | HANDOFF_FAILED => FAILED,
                _ => return,
            };
            if self
                .handoff
                .compare_exchange(old, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return;
            }
        }
    }

    fn begin_operation(&self) -> Result<(), Error> {
        match self
            .handoff
            .compare_exchange(IDLE, OPERATION, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => Ok(()),
            Err(HANDOFF | HANDOFF_FAILED) => Err(Error::Draining),
            Err(FAILED) => Err(Error::ReloadRequired),
            Err(OPERATION | OPERATION_FAILED) => Err(Error::Busy),
            Err(_) => Err(Error::ReloadRequired),
        }
    }

    /// Return false when a concurrent scene attempted to hand off final credit
    /// while this operation held the fence. That credit was not accepted.
    fn finish_operation(&self) -> bool {
        loop {
            let old = self.handoff.load(Ordering::Acquire);
            let (next, accepted) = match old {
                OPERATION => (IDLE, true),
                OPERATION_FAILED => (FAILED, false),
                _ => return false,
            };
            if self
                .handoff
                .compare_exchange(old, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return accepted;
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn with_test_lock(&self, action: impl FnOnce()) {
        let _guard = self.inner.lock().unwrap();
        action();
    }

    /// Close admission immediately; accepted snapshots continue off-thread.
    /// Scene drop and host release both call this, but never wait for disk.
    pub fn retire(&self) -> Result<(), Error> {
        self.begin_operation()?;
        let result = (|| {
            let mut inner = self.inner.try_lock().map_err(lock_error)?;
            if !inner.retiring {
                if let Some(writer) = inner.writer.as_mut() {
                    writer.close();
                }
                inner.retiring = inner.writer.is_some();
            }
            Ok(())
        })();
        if self.finish_operation() {
            result
        } else {
            Err(Error::ReloadRequired)
        }
    }

    /// One nonblocking successor check. Calling this ends admission by any
    /// installed predecessor; use it only when opening a replacement catalog.
    /// A scene must not bind a catalog until `Ready` or after a fresh reload.
    pub fn gate(&self) -> Gate {
        // A failed handoff still has to drain the old writer before exposing
        // ReloadRequired. Its sticky bit survives this operation.
        match self
            .handoff
            .compare_exchange(IDLE, OPERATION, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => {}
            Err(FAILED) => {
                if self
                    .handoff
                    .compare_exchange(
                        FAILED,
                        OPERATION_FAILED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_err()
                {
                    return Gate::Busy;
                }
            }
            Err(HANDOFF | HANDOFF_FAILED) => return Gate::Draining,
            Err(_) => return Gate::Busy,
        }
        let result = self.gate_inner();
        if self.finish_operation() {
            result
        } else if result == Gate::Poisoned {
            Gate::Poisoned
        } else {
            match result {
                // A failed handoff is sticky, but lock contention and accepted
                // writer drain still precede the authoritative reload.
                Gate::Busy | Gate::Draining | Gate::Poisoned => result,
                Gate::Ready | Gate::ReloadRequired => Gate::ReloadRequired,
            }
        }
    }

    fn gate_inner(&self) -> Gate {
        let mut inner = match self.inner.try_lock() {
            Ok(inner) => inner,
            Err(TryLockError::WouldBlock) => return Gate::Busy,
            Err(TryLockError::Poisoned(_)) => return Gate::Poisoned,
        };
        if inner.writer.is_some() && !inner.retiring {
            if let Some(writer) = inner.writer.as_mut() {
                writer.close();
            }
            inner.retiring = true;
        }
        if inner.retiring {
            if inner
                .writer
                .as_ref()
                .is_some_and(|writer| !writer.is_drained())
            {
                return Gate::Draining;
            }
            if let Some(writer) = inner.writer.as_ref() {
                match writer.take_receipt() {
                    Ok(Some(receipt))
                        if receipt.status == history_writer::Status::ReloadRequired =>
                    {
                        inner.reload_required = true;
                    }
                    Ok(_) => {}
                    Err(history_writer::Error::Busy) => return Gate::Busy,
                    Err(_) => inner.reload_required = true,
                }
            }
            inner.writer.take();
            inner.retiring = false;
            inner.serial = 0;
        }
        if inner.reload_required {
            Gate::ReloadRequired
        } else {
            Gate::Ready
        }
    }

    /// A worker constructs `Writer` only from a freshly loaded Snapshot revision;
    /// installation is rejected while an older writer can still commit.
    pub fn install(&self, writer: Writer) -> Result<(), Error> {
        self.begin_operation()?;
        let result = (|| {
            let mut inner = self.inner.try_lock().map_err(lock_error)?;
            if inner.reload_required {
                return Err(Error::ReloadRequired);
            }
            if inner.writer.is_some() || inner.retiring {
                return Err(Error::Draining);
            }
            inner.writer = Some(writer);
            inner.serial = 0;
            Ok(())
        })();
        if self.finish_operation() {
            result
        } else {
            Err(Error::ReloadRequired)
        }
    }

    /// Called only after a fresh repository load after a failed writer. The
    /// caller must discard the old Controller/History and use the loaded value.
    pub fn acknowledge_authoritative_reload(&self) -> Result<(), Error> {
        // A failed final handoff remains failed until AFTER a new authoritative
        // repository load. The caller supplies the fresh read, then this clears
        // the fence; an active handoff can never be acknowledged away.
        if self.handoff.load(Ordering::Acquire) == FAILED {
            let mut inner = self.inner.try_lock().map_err(lock_error)?;
            if inner.writer.is_some() || inner.retiring {
                return Err(Error::Draining);
            }
            inner.reload_required = false;
            self.handoff
                .compare_exchange(FAILED, IDLE, Ordering::AcqRel, Ordering::Acquire)
                .map_err(|_| Error::Draining)?;
            return Ok(());
        }
        self.begin_operation()?;
        let result = (|| {
            let mut inner = self.inner.try_lock().map_err(lock_error)?;
            if inner.writer.is_some() || inner.retiring {
                return Err(Error::Draining);
            }
            inner.reload_required = false;
            Ok(())
        })();
        if self.finish_operation() {
            result
        } else {
            Err(Error::ReloadRequired)
        }
    }

    /// Admission is nonblocking. A Busy return does not consume the serial; a
    /// retry submits the caller's latest full History under that same serial.
    pub fn submit(&self, history: History) -> Result<history_writer::Admission, Error> {
        let mut inner = self.inner.try_lock().map_err(lock_error)?;
        if inner.reload_required {
            return Err(Error::ReloadRequired);
        }
        if inner.retiring {
            return Err(Error::Draining);
        }
        let serial = inner.serial.checked_add(1).ok_or(Error::Serial)?;
        let writer = inner.writer.as_ref().ok_or(Error::NoWriter)?;
        match writer.submit(serial, history) {
            Ok(admission) => {
                inner.serial = serial;
                Ok(admission)
            }
            Err(history_writer::Error::ReloadRequired) => {
                inner.reload_required = true;
                if let Some(writer) = inner.writer.as_mut() {
                    writer.close();
                }
                inner.retiring = true;
                Err(Error::ReloadRequired)
            }
            Err(error) => Err(error.into()),
        }
    }
}

fn lock_error<T>(error: TryLockError<T>) -> Error {
    match error {
        TryLockError::WouldBlock => Error::Busy,
        TryLockError::Poisoned(_) => Error::Poisoned,
    }
}

#[cfg(test)]
#[path = "saved_runtime_tests.rs"]
mod tests;
