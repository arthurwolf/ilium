//! Lowering a background worker thread's scheduling priority.
//!
//! ilium runs CPU-heavy work off the interactive path -- the detection tick's
//! process scan, local file scans, embedding inference -- and none of it may
//! compete on equal footing with keystroke handling and rendering when the
//! machine is loaded. Only the *worker* thread is lowered, never the process,
//! so input and drawing keep their normal priority.
//!
//! Every platform expresses this differently, and only one of them through
//! niceness: Linux narrows `setpriority` to the calling thread, Darwin has
//! quality-of-service classes because its `setpriority` is process-wide, and
//! Windows has thread priority classes. Callers pick an intent and this module
//! maps it -- picking the mechanism at a call site would eventually reach for
//! `setpriority` on Darwin and slow the whole interface down.

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
/// `setpriority(PRIO_PROCESS, 0, _)` targets the calling *thread* on Linux,
/// which is the exception rather than the rule: POSIX defines `PRIO_PROCESS`
/// as process-wide, and Darwin implements it that way. Using it there would
/// deprioritise the interactive UI along with the worker -- the precise
/// opposite of this module's purpose -- so Linux is the only platform that
/// takes this path.
///
/// The value set is absolute rather than relative on purpose: worker threads
/// come from pools that get reused across many calls, so a relative adjustment
/// would compound on every reuse instead of settling at a fixed, correct value.
#[cfg(target_os = "linux")]
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

/// Darwin's per-thread mechanism is a quality-of-service class, not niceness.
///
/// `setpriority` would apply to the whole process here (see the Linux arm), so
/// it cannot be used: the scheduler is told what kind of work the thread is
/// doing instead, and derives priority, timer coalescing, and I/O throttling
/// from that.
#[cfg(target_os = "macos")]
pub fn lower_current_thread(priority: WorkerPriority) {
    // From `<pthread/qos.h>`. Declared here because `libc` does not expose the
    // QoS family; both the function and these values are stable public ABI.
    const QOS_CLASS_UTILITY: u32 = 0x11;
    const QOS_CLASS_BACKGROUND: u32 = 0x09;
    unsafe extern "C" {
        fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
    }

    let qos_class = match priority {
        // Long-running, not user-initiated, but its progress still matters.
        WorkerPriority::BelowNormal => QOS_CLASS_UTILITY,
        // Explicitly throttled, including for I/O; nothing waits on it.
        WorkerPriority::Lowest => QOS_CLASS_BACKGROUND,
    };
    // SAFETY: takes two integers, affects only the calling thread's scheduling
    // class, and touches no caller memory.
    let result = unsafe { pthread_set_qos_class_self_np(qos_class, 0) };
    if result != 0 {
        tracing::warn!(
            "failed to lower worker thread quality-of-service class: {}; continuing at \
             default scheduling priority",
            std::io::Error::from_raw_os_error(result)
        );
    }
}

/// Every other Unix defines `PRIO_PROCESS` as process-wide and offers no
/// portable per-thread equivalent, so a worker simply runs at normal priority
/// rather than dragging the interface down with it.
#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
pub fn lower_current_thread(priority: WorkerPriority) {
    let _ = priority;
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
    ///
    /// Reads process-wide priority, which is exactly the trap being guarded
    /// against: Darwin's `setpriority(PRIO_PROCESS, 0, _)` would move this
    /// number and take the whole interface down with the worker.
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
