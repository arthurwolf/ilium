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
//! to the project root, and detection reports the agent without a session id.
//!
//! Platform coverage differs, and deliberately so:
//!
//! - Linux reads `/proc/<pid>/cwd` and `/proc/<pid>/fd`, which are cheap,
//!   unprivileged for one's own processes, and exact.
//! - macOS has no procfs; `libproc`'s `proc_pidinfo` family answers the same
//!   two questions for processes the caller owns.
//! - Windows has no equivalent that works unprivileged. Enumerating another
//!   process's handles means `NtQueryInformationProcess` with
//!   `SystemHandleInformation`, an unstable interface that generally requires
//!   elevation, so both functions report "unknown" and agent detection
//!   degrades to identity-without-session rather than pretending.

use std::path::PathBuf;

/// The process's current working directory, or `None` if it cannot be read.
#[cfg(target_os = "linux")]
pub fn working_directory(process_id: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{process_id}/cwd")).ok()
}

#[cfg(target_os = "macos")]
pub fn working_directory(process_id: u32) -> Option<PathBuf> {
    use libproc::libproc::proc_pid;

    let process_id = i32::try_from(process_id).ok()?;
    proc_pid::pidcwd(process_id).ok()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn working_directory(process_id: u32) -> Option<PathBuf> {
    let _ = process_id;
    None
}

/// Every regular file the process currently holds open.
///
/// Entries that are not regular files (sockets, pipes, the terminal itself)
/// are omitted: the only caller matches these paths against transcript files
/// on disk, so a descriptor with no path is noise rather than information.
#[cfg(target_os = "linux")]
pub fn open_file_paths(process_id: u32) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(format!("/proc/{process_id}/fd")) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        // `/proc/<pid>/fd/<n>` is a symlink to the opened path; a descriptor
        // pointing at a socket or pipe resolves to a synthetic `socket:[...]`
        // target, which simply fails to match any transcript path later.
        .filter_map(|entry| std::fs::read_link(entry.path()).ok())
        .collect()
}

#[cfg(target_os = "macos")]
pub fn open_file_paths(process_id: u32) -> Vec<PathBuf> {
    use libproc::libproc::file_info::{pidfdinfo, ListFDs, ProcFDType};
    use libproc::libproc::proc_pid::listpidinfo;

    let Ok(process_id) = i32::try_from(process_id) else {
        return Vec::new();
    };
    // `listpidinfo` needs an upper bound on how many descriptors to retrieve.
    // Agent CLIs hold a modest number; a generous cap costs one allocation and
    // avoids a second round trip to size the list exactly.
    const MAX_INSPECTED_DESCRIPTORS: usize = 1024;
    let Ok(descriptors) = listpidinfo::<ListFDs>(process_id, MAX_INSPECTED_DESCRIPTORS) else {
        return Vec::new();
    };
    descriptors
        .iter()
        .filter(|descriptor| ProcFDType::from(descriptor.proc_fdtype) == ProcFDType::VNode)
        .filter_map(|descriptor| {
            let info: libproc::libproc::file_info::VnodeFdInfoWithPath =
                pidfdinfo(process_id, descriptor.proc_fd).ok()?;
            let path = info
                .pvip
                .vip_path
                .iter()
                .take_while(|byte| **byte != 0)
                .map(|byte| *byte as u8)
                .collect::<Vec<u8>>();
            if path.is_empty() {
                return None;
            }
            Some(PathBuf::from(String::from_utf8_lossy(&path).into_owned()))
        })
        .collect()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn open_file_paths(process_id: u32) -> Vec<PathBuf> {
    let _ = process_id;
    Vec::new()
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

    #[cfg(any(target_os = "linux", target_os = "macos"))]
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

    /// A process id that cannot exist must be reported as unknown rather than
    /// panicking or blocking, because detection races real process exits.
    #[test]
    fn an_absent_process_is_reported_as_unknown() {
        // 0 is never a normal user process on any supported platform.
        assert_eq!(working_directory(0), None);
        assert!(open_file_paths(0).is_empty());
    }
}
