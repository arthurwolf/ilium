//! Lowering a background worker thread's scheduling priority.
//!
//! ilium runs CPU-heavy work off the interactive path -- the detection tick's
//! process scan, local file scans, embedding inference -- and none of it may
//! compete on equal footing with keystroke handling and rendering when the
//! machine is loaded. Only the *worker* thread is lowered, never the process,
//! so input and drawing keep their normal priority.
//!
//! Both supported families can express this, by unrelated mechanisms: POSIX
//! niceness through `setpriority`, and Windows thread priority classes through
//! `SetThreadPriority`. Callers pick an intent and this module maps it.

/// How aggressively a worker thread should yield to interactive work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerPriority {
    /// Still schedules promptly; yields only when something else wants the
    /// CPU. Suited to short, repeated work whose latency the user notices
    /// indirectly, such as a detection tick.
    BelowNormal,
    /// Yields aggressively. Suited to long bulk work whose completion time
    /// nobody is watching, such as a full local scan or embedding inference.
    Lowest,
}

/// Lowers the *calling thread's* priority. Never fails the caller: a worker
/// that could not be niced must still run, just at default priority.
///
/// On Unix this is `setpriority(PRIO_PROCESS, 0, _)`, which despite its name
/// targets the calling thread alone when given a pid of `0`, and is always
/// permitted -- only *raising* priority needs `CAP_SYS_NICE`. The value set is
/// absolute rather than relative on purpose: worker threads come from pools
/// that get reused across many calls, so a relative adjustment would compound
/// on every reuse instead of settling at a fixed, correct value.
#[cfg(unix)]
pub fn lower_current_thread(priority: WorkerPriority) {
    let niceness = match priority {
        WorkerPriority::BelowNormal => 10,
        WorkerPriority::Lowest => 15,
    };
    // SAFETY: `setpriority` takes no pointers, and `PRIO_PROCESS` with a pid
    // of 0 restricts its effect to the calling thread.
    let result = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, niceness) };
    if result == -1 {
        // The target values above are never a "already there, returned -1 by
        // coincidence" case, so -1 here is a genuine failure.
        let error = std::io::Error::last_os_error();
        tracing::warn!(
            "failed to lower worker thread priority to niceness {niceness}: {error}; \
             continuing at default scheduling priority"
        );
    }
}

/// Windows has no niceness; the equivalent is a per-thread priority class.
#[cfg(windows)]
pub fn lower_current_thread(priority: WorkerPriority) {
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL, THREAD_PRIORITY_LOWEST,
    };

    let level = match priority {
        WorkerPriority::BelowNormal => THREAD_PRIORITY_BELOW_NORMAL,
        WorkerPriority::Lowest => THREAD_PRIORITY_LOWEST,
    };
    // SAFETY: `GetCurrentThread` returns a pseudo-handle that needs no
    // release and is always valid for the calling thread; `SetThreadPriority`
    // takes it plus an integer and touches no caller memory.
    let result = unsafe { SetThreadPriority(GetCurrentThread(), level) };
    if result == 0 {
        let error = std::io::Error::last_os_error();
        tracing::warn!(
            "failed to lower worker thread priority: {error}; continuing at default \
             scheduling priority"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The contract is "never fails the caller", so the observable behaviour
    /// worth asserting is that both intents return normally on a worker
    /// thread and leave it running.
    #[test]
    fn lowering_a_worker_thread_returns_normally_for_every_intent() {
        for priority in [WorkerPriority::BelowNormal, WorkerPriority::Lowest] {
            let worker = std::thread::spawn(move || {
                lower_current_thread(priority);
                // Prove the thread still runs afterwards rather than merely
                // that the call returned.
                42
            });
            assert_eq!(worker.join().expect("worker thread"), 42);
        }
    }

    /// Lowering must not disturb the interactive thread that spawned the
    /// worker, which is the entire reason this is per-thread.
    #[cfg(unix)]
    #[test]
    fn lowering_a_worker_leaves_the_spawning_thread_alone() {
        // SAFETY: `getpriority` takes no pointers; pid 0 reads the caller.
        let before = unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) };

        std::thread::spawn(|| lower_current_thread(WorkerPriority::Lowest))
            .join()
            .expect("worker thread");

        let after = unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) };
        assert_eq!(
            before, after,
            "spawning thread's priority must be untouched"
        );
    }
}
