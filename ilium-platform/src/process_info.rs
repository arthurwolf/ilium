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
//! | macOS | `proc_pidinfo` | `proc_pidinfo` + `proc_pidfdinfo` |
//! | Windows | unavailable | unavailable |
//!
//! macOS answers both through the `proc_*info` family. The descriptor path
//! needs two declarations `libc` omits, so the reply's byte count is checked
//! against the size passed in: a layout that did not match would read
//! misaligned memory rather than fail, and this turns that into no answer
//! instead of a fabricated one.
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

/// Darwin's descriptor table, read through `proc_pidinfo`/`proc_pidfdinfo`.
///
/// `libc` declares every piece except the file-info header and the flavor
/// constant, which [`darwin`] supplies. A wrong layout there would read
/// misaligned memory rather than fail, so every call checks the byte count the
/// kernel reports against the size it was given and discards anything that
/// disagrees -- a mismatch yields no path instead of a fabricated one.
#[cfg(target_os = "macos")]
pub fn open_file_paths(process_id: u32) -> Vec<PathBuf> {
    use std::ffi::c_void;

    /// Where the descriptor table is assumed to start when the kernel will not
    /// say, and how far it is allowed to grow. A process holding more open
    /// files than this is not one of the agent CLIs being looked for.
    const INITIAL_DESCRIPTOR_CAPACITY: usize = 256;
    const MAXIMUM_DESCRIPTOR_CAPACITY: usize = 16_384;

    let Ok(process_id) = i32::try_from(process_id) else {
        return Vec::new();
    };
    let descriptor_size = std::mem::size_of::<libc::proc_fdinfo>();

    // The sizing form -- a null buffer and zero size -- is documented to report
    // how many bytes the table needs, but it is only a hint and not every
    // kernel answers it: some return zero, which would otherwise be read as
    // "this process has no open files" and silently end the search. Treat any
    // non-positive answer as "unknown" and grow instead.
    // SAFETY: a null buffer with zero size is the documented sizing form.
    let hinted = unsafe {
        libc::proc_pidinfo(
            process_id,
            libc::PROC_PIDLISTFDS,
            0,
            std::ptr::null_mut(),
            0,
        )
    };
    let mut count = if hinted > 0 {
        hinted as usize / descriptor_size
    } else {
        INITIAL_DESCRIPTOR_CAPACITY
    };

    loop {
        let mut descriptors: Vec<libc::proc_fdinfo> = vec![unsafe { std::mem::zeroed() }; count];
        let Ok(capacity) = i32::try_from(count * descriptor_size) else {
            return Vec::new();
        };
        // SAFETY: `descriptors` owns `capacity` bytes and is written at most
        // that far; the kernel reports how much it actually used.
        let written = unsafe {
            libc::proc_pidinfo(
                process_id,
                libc::PROC_PIDLISTFDS,
                0,
                descriptors.as_mut_ptr().cast::<c_void>(),
                capacity,
            )
        };
        if written <= 0 {
            return Vec::new();
        }
        let used = written as usize;
        // A completely filled buffer is indistinguishable from a truncated
        // one -- the call reports bytes written, not bytes needed -- so grow
        // and ask again rather than searching a table that may be cut short of
        // the descriptor being looked for.
        if used >= count * descriptor_size && count * 2 <= MAXIMUM_DESCRIPTOR_CAPACITY {
            count *= 2;
            continue;
        }
        descriptors.truncate(used / descriptor_size);
        return descriptors
            .iter()
            .filter(|descriptor| descriptor.proc_fdtype == libc::PROX_FDTYPE_VNODE as u32)
            .filter_map(|descriptor| darwin_vnode_path(process_id, descriptor.proc_fd))
            .collect();
    }
}

/// The filesystem path behind one vnode descriptor, or `None` when the kernel
/// declines or answers with an unexpected size.
#[cfg(target_os = "macos")]
fn darwin_vnode_path(process_id: i32, descriptor: i32) -> Option<PathBuf> {
    use std::ffi::c_void;

    let mut info: darwin::VnodeFdInfoWithPath = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<darwin::VnodeFdInfoWithPath>() as i32;
    // SAFETY: `info` is a live local of exactly the type this flavor returns,
    // and `size` is its real size.
    let written = unsafe {
        libc::proc_pidfdinfo(
            process_id,
            descriptor,
            darwin::PROC_PIDFDVNODEPATHINFO,
            std::ptr::addr_of_mut!(info).cast::<c_void>(),
            size,
        )
    };
    // Anything short means the structure was not filled as declared -- either
    // the descriptor vanished or this layout is wrong. Either way the bytes
    // are not a path.
    if written != size {
        return None;
    }
    let path_bytes: Vec<u8> = info
        .vnode
        .vip_path
        .iter()
        .flatten()
        .take_while(|byte| **byte != 0)
        .map(|byte| *byte as u8)
        .collect();
    (!path_bytes.is_empty())
        .then(|| PathBuf::from(String::from_utf8_lossy(&path_bytes).into_owned()))
}

/// The two declarations `libc` is missing, from `<sys/proc_info.h>`.
///
/// Both have been stable since the interface was introduced. `vnode_info_path`
/// itself comes from `libc`, so the part most likely to drift with the SDK is
/// not restated here.
#[cfg(target_os = "macos")]
mod darwin {
    /// `PROC_PIDFDVNODEPATHINFO`.
    pub const PROC_PIDFDVNODEPATHINFO: i32 = 2;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct ProcFileInfo {
        pub open_flags: u32,
        pub status: u32,
        pub offset: i64,
        pub file_type: i32,
        pub guard_flags: u32,
    }

    #[repr(C)]
    pub struct VnodeFdInfoWithPath {
        pub file_info: ProcFileInfo,
        pub vnode: libc::vnode_info_path,
    }
}

/// Windows has no unprivileged equivalent; see the module comment.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn open_file_paths(process_id: u32) -> Vec<PathBuf> {
    let _ = process_id;
    Vec::new()
}

/// Whether this platform can enumerate a process's open files, so callers can
/// distinguish "this process has none" from "this platform cannot say".
pub const fn open_files_are_observable() -> bool {
    cfg!(any(target_os = "linux", target_os = "macos"))
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

    /// Also the proof that the hand-written Darwin layout is right: a wrong
    /// one yields no path, so this failing on macOS means the structures need
    /// revisiting rather than the feature being unavailable.
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

    /// Where open files cannot be observed, the answer must be an honest empty
    /// list rather than a wrong one, and must agree with the capability flag.
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
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
