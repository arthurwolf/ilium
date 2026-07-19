//! Best-effort scheduling priority for CPU-heavy client background work.
//!
//! Input dispatch and terminal rendering remain at the process's normal
//! priority. Local scans and embedding inference lower only their own worker
//! thread on Linux so they yield promptly when the interactive path needs CPU.

#[cfg(target_os = "linux")]
pub fn lower_current_thread() {
    const BACKGROUND_NICENESS: i32 = 15;
    // SAFETY: `setpriority` receives no pointers. `who = 0` with
    // `PRIO_PROCESS` applies to the calling Linux thread only.
    let result = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, BACKGROUND_NICENESS) };
    if result == -1 {
        tracing::warn!(
            "failed to lower background worker priority: {}",
            std::io::Error::last_os_error()
        );
    }
}

#[cfg(not(target_os = "linux"))]
pub fn lower_current_thread() {}
