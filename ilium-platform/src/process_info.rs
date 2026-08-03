//! What a live process is doing: its working directory and its open files.
//!
//! Agent detection uses both. The working directory tells a pane which project
//! a shell has wandered into; the open-file list is how a running Claude Code
//! or Codex process is matched to the transcript file it is currently writing,
//! which is what turns "some agent is running" into "this specific session".
//!
//! Every function here answers "unknown" rather than failing. A process can
//! exit between the caller's decision to inspect it and the inspection itself,
//! and on a platform with no unprivileged way to ask, the honest answer is
//! also "unknown". Callers already treat that as ordinary: a pane falls back
//! to the project root, and detection identifies the agent without pinning it
//! to a specific session.
//!
//! Platform coverage is uneven, and the gaps are deliberate rather than
//! pending:
//!
//! | | working directory | open files |
//! |---|---|---|
//! | Linux | `/proc/<pid>/cwd` | `/proc/<pid>/fd` |
//! | macOS | `proc_pidinfo` | unavailable |
//! | Windows | unavailable | unavailable |
//!
//! macOS can answer the working directory through `proc_pidinfo`, whose
//! structures `libc` declares, so there is no hand-written struct layout to
//! get wrong. Mapping a *descriptor* to a path additionally needs
//! `vnode_fdinfowithpath` and its flavor constant, which `libc` does not
//! declare; hand-writing that layout risks silently reading misaligned bytes,
//! which is worse than reporting nothing. Session identity there falls back to
//! parsing the agent's own command line, which `ilium-detect` already does.
//!
//! Windows has no unprivileged equivalent of either. Enumerating another
//! process's handles means `NtQueryInformationProcess` with
//! `SystemHandleInformation`, an unstable interface that generally requires
//! elevation.

use std::path::PathBuf;

/// The process's current working directory, or `None` if it cannot be read.
#[cfg(target_os = "linux")]
pub fn working_directory(process_id: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{process_id}/cwd")).ok()
}

#[cfg(target_os = "macos")]
pub fn working_directory(process_id: u32) -> Option<PathBuf> {
    use std::ffi::c_void;

    let process_id = i32::try_from(process_id).ok()?;
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as i32;
    // SAFETY: `proc_pidinfo` writes at most `size` bytes into `info`, which is
    // a live, correctly sized local of exactly the type this flavor returns.
    // The layout comes from `libc`, so it tracks the SDK rather than being
    // restated here.
    let written = unsafe {
        libc::proc_pidinfo(
            process_id,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            std::ptr::addr_of_mut!(info).cast::<c_void>(),
            size,
        )
    };
    // A short read means the kernel did not populate the whole structure, so
    // the path field cannot be trusted.
    if written < size {
        return None;
    }
    // `vip_path` is a fixed 1024-byte buffer that `libc` declares as nested
    // arrays to stay compatible with older compilers; flatten it and stop at
    // the terminator.
    let path_bytes: Vec<u8> = info
        .pvi_cdir
        .vip_path
        .iter()
        .flatten()
        .take_while(|byte| **byte != 0)
        .map(|byte| *byte as u8)
        .collect();
    if path_bytes.is_empty() {
        return None;
    }
    Some(PathBuf::from(
        String::from_utf8_lossy(&path_bytes).into_owned(),
    ))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn working_directory(process_id: u32) -> Option<PathBuf> {
    let _ = process_id;
    None
}

/// The executable a running process was started from.
///
/// Used to prove a restarted client is running the same binary as before, not
/// a stale one. `None` where the process is gone or the platform cannot say.
#[cfg(target_os = "linux")]
pub fn executable_path(process_id: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{process_id}/exe")).ok()
}

#[cfg(target_os = "macos")]
pub fn executable_path(process_id: u32) -> Option<PathBuf> {
    use std::ffi::c_void;

    let process_id = i32::try_from(process_id).ok()?;
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: `proc_pidpath` writes at most `buffersize` bytes into `buffer`,
    // which is a live allocation of exactly that length.
    let written = unsafe {
        libc::proc_pidpath(
            process_id,
            buffer.as_mut_ptr().cast::<c_void>(),
            buffer.len() as u32,
        )
    };
    if written <= 0 {
        return None;
    }
    buffer.truncate(written as usize);
    Some(PathBuf::from(String::from_utf8_lossy(&buffer).into_owned()))
}

#[cfg(windows)]
pub fn executable_path(process_id: u32) -> Option<PathBuf> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: integers in, handle out; closed on every path below.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if handle.is_null() {
        return None;
    }
    let mut buffer = vec![0u16; 32_768];
    let mut length = buffer.len() as u32;
    // SAFETY: `handle` is valid, and the callee writes at most `length` UTF-16
    // units into `buffer` while updating `length` to what it wrote.
    let queried =
        unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) };
    // SAFETY: same handle, closed exactly once.
    unsafe { CloseHandle(handle) };
    if queried == 0 {
        return None;
    }
    buffer.truncate(length as usize);
    Some(PathBuf::from(String::from_utf16_lossy(&buffer)))
}

/// Every path the process currently holds open.
///
/// Descriptors with no filesystem path (sockets, pipes, the terminal itself)
/// resolve to synthetic targets that simply fail to match any transcript path
/// later, so they are left in rather than filtered by type.
#[cfg(target_os = "linux")]
pub fn open_file_paths(process_id: u32) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(format!("/proc/{process_id}/fd")) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| std::fs::read_link(entry.path()).ok())
        .collect()
}

/// See the module comment: the structures needed to turn a macOS descriptor
/// into a path are not declared by `libc`, and guessing their layout would
/// risk reading misaligned memory rather than merely returning less.
#[cfg(not(target_os = "linux"))]
pub fn open_file_paths(process_id: u32) -> Vec<PathBuf> {
    let _ = process_id;
    Vec::new()
}

/// Whether this platform can enumerate a process's open files, so callers can
/// distinguish "this process has none" from "this platform cannot say".
pub const fn open_files_are_observable() -> bool {
    cfg!(target_os = "linux")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Platforms that can answer must answer correctly for the test process
    /// itself, which is the one process whose truth the test already knows.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn the_current_process_working_directory_is_readable() {
        let expected = std::env::current_dir().expect("current dir");

        let reported = working_directory(std::process::id()).expect("cwd is readable");

        assert_eq!(
            reported.canonicalize().ok(),
            expected.canonicalize().ok(),
            "reported cwd should match the process's real cwd"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn an_open_file_appears_in_the_process_open_file_list() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("held-open.txt");
        std::fs::write(&path, b"content").expect("write");
        let held = std::fs::File::open(&path).expect("open");

        let open_paths = open_file_paths(std::process::id());

        let canonical = path.canonicalize().expect("canonicalize");
        assert!(
            open_paths
                .iter()
                .any(|open| open.canonicalize().ok().as_deref() == Some(canonical.as_path())),
            "expected {canonical:?} among {open_paths:?}"
        );
        drop(held);
    }

    /// Where open files cannot be observed, the answer must be an honest empty
    /// list rather than a wrong one, and must agree with the capability flag.
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn open_files_are_reported_as_unobservable() {
        assert!(!open_files_are_observable());
        assert!(open_file_paths(std::process::id()).is_empty());
    }

    #[test]
    fn the_current_process_executable_is_readable() {
        let expected = std::env::current_exe().expect("current exe");

        let reported = executable_path(std::process::id()).expect("executable is readable");

        assert_eq!(
            reported.canonicalize().ok(),
            expected.canonicalize().ok(),
            "reported executable should match the running test binary"
        );
    }

    /// A process id that cannot exist must be reported as unknown rather than
    /// panicking or blocking, because detection races real process exits.
    #[test]
    fn an_absent_process_is_reported_as_unknown() {
        // 0 is never a normal user process on any supported platform.
        assert_eq!(working_directory(0), None);
        assert_eq!(executable_path(0), None);
        assert!(open_file_paths(0).is_empty());
    }
}
