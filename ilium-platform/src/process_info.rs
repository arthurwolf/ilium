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
//! Platform coverage, and how each answer is obtained:
//!
//! | | working directory | open files |
//! |---|---|---|
//! | Linux | `/proc/<pid>/cwd` | `/proc/<pid>/fd` |
//! | macOS | `proc_pidinfo` | `proc_pidinfo` + `proc_pidfdinfo` |
//! | Windows | PEB via `ReadProcessMemory` | system handle table |
//!
//! macOS answers both through the `proc_*info` family. The descriptor path
//! needs two declarations `libc` omits, so the reply's byte count is checked
//! against the size passed in: a layout that did not match would read
//! misaligned memory rather than fail, and this turns that into no answer
//! instead of a fabricated one.
//!
//! Windows has no ordinary API for either question, but both are answerable
//! without elevation for the processes ilium cares about -- the agent CLIs it
//! spawned itself, which are its own descendants running as the same user.
//! Open files come from `NtQuerySystemInformation`'s system-wide handle table,
//! filtered to the target pid; the working directory is read out of the
//! target's PEB. Both use undocumented reply layouts declared here, so both
//! check the kernel's reported sizes rather than trusting the shape, and both
//! are covered by unit tests that run against the test process itself -- a
//! wrong offset shows up as a failing test, not as a plausible wrong answer.

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

/// Windows: read out of the target process's own PEB.
///
/// A process's current directory lives only in its user-mode
/// `RTL_USER_PROCESS_PARAMETERS`, so there is nothing to query -- it has to be
/// read out of the process's address space.
/// `NtQueryInformationProcess(ProcessBasicInformation)` gives the PEB address,
/// and three `ReadProcessMemory` hops (PEB -> parameters -> the string's own
/// buffer) reach the text.
///
/// Only `PROCESS_QUERY_INFORMATION | PROCESS_VM_READ` is requested, which the
/// same user's own descendants grant unelevated. Every hop is checked, so a
/// process that exits midway through yields `None` rather than a partial or
/// fabricated path.
///
/// Deliberately no 32-bit-process handling: ilium ships 64-bit, and reading a
/// WoW64 child's 32-bit PEB needs an entirely different layout. Such a process
/// answers `None`, which callers already treat as "fall back to the project
/// root".
#[cfg(windows)]
pub fn working_directory(process_id: u32) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;

    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    /// Byte offset of `ProcessParameters` within `PEB` on x64.
    const PROCESS_PARAMETERS_OFFSET: usize = 0x20;
    /// Byte offset of `CurrentDirectory.DosPath` within
    /// `RTL_USER_PROCESS_PARAMETERS` on x64. `CurrentDirectory` is a
    /// `CURDIR { UNICODE_STRING DosPath; HANDLE Handle; }`, so the
    /// `UNICODE_STRING` sits at the front of it.
    const CURRENT_DIRECTORY_OFFSET: usize = 0x38;
    /// A current directory longer than this is not a real one; the cap keeps a
    /// corrupt or racing read from asking for an enormous allocation.
    const MAXIMUM_PATH_BYTES: u16 = 0x8000;

    // SAFETY: plain FFI call with no pointer arguments; failure is a null
    // handle, checked immediately.
    let process =
        unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, process_id) };
    if process.is_null() {
        return None;
    }
    let result = (|| {
        let peb_address = peb_address(process)?;
        let parameters: usize =
            read_process_value(process, peb_address.wrapping_add(PROCESS_PARAMETERS_OFFSET))?;
        if parameters == 0 {
            return None;
        }
        // `UNICODE_STRING { USHORT Length; USHORT MaximumLength; PWSTR Buffer; }`
        // -- read as its three fields rather than as one struct so the
        // padding before `Buffer` cannot be got wrong.
        let length: u16 =
            read_process_value(process, parameters.wrapping_add(CURRENT_DIRECTORY_OFFSET))?;
        let buffer: usize = read_process_value(
            process,
            parameters.wrapping_add(CURRENT_DIRECTORY_OFFSET + 8),
        )?;
        if length == 0 || length > MAXIMUM_PATH_BYTES || buffer == 0 {
            return None;
        }
        let mut utf16 = vec![0u16; usize::from(length) / 2];
        read_process_slice(process, buffer, &mut utf16)?;
        let text = std::ffi::OsString::from_wide(&utf16);
        let path = PathBuf::from(text);
        // Windows stores it with a trailing separator; every caller compares
        // against paths that have none. A drive root ("C:\") is the one
        // exception: stripping its separator would leave "C:", which means
        // "the current directory on drive C" rather than the root, so the
        // separator stays on a bare drive letter.
        let text = path.to_string_lossy();
        let trimmed = text.trim_end_matches('\\');
        let normalized = if trimmed.len() == 2 && trimmed.ends_with(':') {
            text.into_owned()
        } else {
            trimmed.to_string()
        };
        Some(PathBuf::from(normalized))
    })();

    // SAFETY: `process` came from the successful `OpenProcess` above and is
    // not used after this call.
    unsafe { CloseHandle(process) };
    result
}

/// The PEB base address of an already-open process.
#[cfg(windows)]
fn peb_address(process: windows_sys::Win32::Foundation::HANDLE) -> Option<usize> {
    use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation};

    /// `PROCESS_BASIC_INFORMATION` on x64. Declared here for the same reason
    /// the handle-table layout is: `windows-sys` binds the call but not the
    /// reply.
    #[repr(C)]
    struct ProcessBasicInformationReply {
        exit_status: i32,
        _padding: i32,
        peb_base_address: usize,
        affinity_mask: usize,
        base_priority: i32,
        _padding_two: i32,
        unique_process_id: usize,
        inherited_from_unique_process_id: usize,
    }

    let mut reply = ProcessBasicInformationReply {
        exit_status: 0,
        _padding: 0,
        peb_base_address: 0,
        affinity_mask: 0,
        base_priority: 0,
        _padding_two: 0,
        unique_process_id: 0,
        inherited_from_unique_process_id: 0,
    };
    let mut written = 0u32;
    // SAFETY: `reply` is a live, correctly sized allocation described to the
    // kernel as exactly its own size.
    let status = unsafe {
        NtQueryInformationProcess(
            process,
            ProcessBasicInformation,
            std::ptr::addr_of_mut!(reply).cast(),
            std::mem::size_of::<ProcessBasicInformationReply>() as u32,
            &mut written,
        )
    };
    // The size is checked, not assumed: a reply shorter than the declared
    // layout would leave `peb_base_address` holding whatever was there before.
    if status < 0 || written as usize != std::mem::size_of::<ProcessBasicInformationReply>() {
        return None;
    }
    (reply.peb_base_address != 0).then_some(reply.peb_base_address)
}

/// Reads one `T` out of another process's address space.
#[cfg(windows)]
fn read_process_value<T: Copy + Default>(
    process: windows_sys::Win32::Foundation::HANDLE,
    address: usize,
) -> Option<T> {
    let mut value = T::default();
    let mut slice = std::slice::from_mut(&mut value);
    read_process_slice(process, address, &mut slice).map(|()| value)
}

/// Fills `destination` from `address` in another process, or answers `None`
/// unless every byte arrived.
#[cfg(windows)]
fn read_process_slice<T: Copy>(
    process: windows_sys::Win32::Foundation::HANDLE,
    address: usize,
    destination: &mut [T],
) -> Option<()> {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;

    let bytes = std::mem::size_of_val(destination);
    let mut read = 0usize;
    // SAFETY: `destination` is a live allocation of exactly `bytes` bytes, and
    // is described to the kernel as that. `address` is only ever a value the
    // kernel itself reported; an invalid one fails the call rather than
    // touching this process's memory.
    let ok = unsafe {
        ReadProcessMemory(
            process,
            address as *const core::ffi::c_void,
            destination.as_mut_ptr().cast(),
            bytes,
            &mut read,
        )
    };
    (ok != 0 && read == bytes).then_some(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
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
    // Round the hint up to whole descriptors, and never let it reach zero: a
    // zero-descriptor buffer would re-run the sizing form (which reports a
    // positive byte count), trip the "buffer full, grow" branch, and double
    // zero forever.
    let mut count = if hinted > 0 {
        (hinted as usize).div_ceil(descriptor_size).max(1)
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

/// Windows: the system-wide handle table, filtered to this process.
///
/// There is no per-process handle enumeration API, so the only route is
/// `NtQuerySystemInformation(SystemExtendedHandleInformation)`, which returns
/// every handle on the machine at once. Each entry carries the owning pid, so
/// the ones belonging to `process_id` can be picked out, duplicated into this
/// process, and resolved to a path.
///
/// Two details are load-bearing rather than incidental:
///
/// - `GetFileType` is checked *before* `GetFinalPathNameByHandle`. Resolving
///   the name of a handle to a synchronous pipe whose other end is idle blocks
///   forever, and a pane's own ConPTY handles are exactly that. Filtering to
///   `FILE_TYPE_DISK` first keeps this call bounded, which matters because it
///   runs inside the detection loop.
/// - Only `PROCESS_DUP_HANDLE` is requested, which the same user's own
///   descendants grant without elevation. This is not an elevated or
///   privileged interface for the processes ilium actually inspects -- the
///   agent CLIs it spawned itself.
#[cfg(windows)]
pub fn open_file_paths(process_id: u32) -> Vec<PathBuf> {
    use windows_sys::Win32::Foundation::{CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS};
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, PROCESS_DUP_HANDLE,
    };

    let Some(snapshot) = system_handle_table() else {
        return Vec::new();
    };
    // SAFETY: a plain FFI call with no pointer arguments; a failure is
    // reported as a null handle, which is checked immediately.
    let target = unsafe { OpenProcess(PROCESS_DUP_HANDLE, 0, process_id) };
    if target.is_null() {
        return Vec::new();
    }

    let mut paths = Vec::new();
    for entry in snapshot.entries() {
        if entry.unique_process_id as u32 != process_id {
            continue;
        }
        let mut duplicated = std::ptr::null_mut();
        // SAFETY: `target` is a live handle opened above with
        // `PROCESS_DUP_HANDLE`; `entry.handle_value` is a handle value the
        // kernel just reported for that process. A stale value (the process
        // closed it in between) fails the call rather than doing anything
        // unsafe, which is why the result is checked instead of assumed.
        let duplicated_ok = unsafe {
            DuplicateHandle(
                target,
                entry.handle_value as _,
                GetCurrentProcess(),
                &mut duplicated,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if duplicated_ok == 0 {
            continue;
        }
        if let Some(path) = path_of_disk_handle(duplicated) {
            paths.push(path);
        }
        // SAFETY: `duplicated` was produced by the successful `DuplicateHandle`
        // above and is not used again after this point.
        unsafe { CloseHandle(duplicated) };
    }

    // SAFETY: `target` came from a successful `OpenProcess` and is dead after
    // this call; nothing above retains it.
    unsafe { CloseHandle(target) };
    paths
}

/// The final path of `handle`, but only when it names something on disk.
///
/// `None` for pipes, consoles, character devices and anything whose name
/// cannot be resolved -- see [`open_file_paths`] for why the type check has to
/// come first.
#[cfg(windows)]
fn path_of_disk_handle(handle: windows_sys::Win32::Foundation::HANDLE) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;

    use windows_sys::Win32::Storage::FileSystem::{
        GetFileType, GetFinalPathNameByHandleW, FILE_NAME_NORMALIZED, FILE_TYPE_DISK,
        VOLUME_NAME_DOS,
    };

    // SAFETY: `handle` is a live handle owned by the caller for the duration
    // of this function.
    if unsafe { GetFileType(handle) } != FILE_TYPE_DISK {
        return None;
    }

    // Sized by asking: the first call with a zero-length buffer returns the
    // length required (including the terminator), so no path length is
    // assumed.
    // SAFETY: a null buffer with length 0 is the documented way to ask for the
    // required size.
    let required = unsafe {
        GetFinalPathNameByHandleW(
            handle,
            std::ptr::null_mut(),
            0,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    };
    if required == 0 {
        return None;
    }
    let mut buffer = vec![0u16; required as usize];
    // SAFETY: `buffer` is a live allocation of exactly `buffer.len()` UTF-16
    // units, which is what is passed as the capacity.
    let written = unsafe {
        GetFinalPathNameByHandleW(
            handle,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    };
    // A second call returning zero, or a length that no longer fits, means the
    // answer changed underneath us; no answer beats a truncated one.
    if written == 0 || written as usize >= buffer.len() + 1 {
        return None;
    }
    let path = std::ffi::OsString::from_wide(&buffer[..written as usize]);
    let path = PathBuf::from(path);

    // `GetFinalPathNameByHandleW` returns the `\\?\` extended-length form.
    // Callers compare these against paths they built themselves, which never
    // carry the prefix, so it is stripped here rather than at every comparison.
    Some(strip_extended_length_prefix(path))
}

/// Removes the `\\?\` (or `\\?\UNC\`) prefix Windows adds to a resolved path.
#[cfg(windows)]
fn strip_extended_length_prefix(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path.clone(),
    }
}

/// An owned snapshot of the system-wide handle table.
#[cfg(windows)]
struct SystemHandleTable {
    /// `u64`-backed so the allocation is 8-aligned, which the entry layout
    /// requires; a `Vec<u8>` carries no such guarantee.
    buffer: Vec<u64>,
    handle_count: usize,
}

#[cfg(windows)]
impl SystemHandleTable {
    fn entries(&self) -> &[SystemHandleTableEntry] {
        // SAFETY: `buffer` holds a `SYSTEM_HANDLE_INFORMATION_EX` the kernel
        // filled in, whose `handle_count` entries follow its two-word header.
        // `handle_count` was taken from that same reply and is bounded below
        // by the byte count the kernel reported writing.
        unsafe {
            let header = self.buffer.as_ptr().cast::<SystemHandleTableHeader>();
            let first = std::ptr::addr_of!((*header).first_entry);
            std::slice::from_raw_parts(first, self.handle_count)
        }
    }
}

/// `SYSTEM_HANDLE_INFORMATION_EX`. Declared here because `windows-sys` binds
/// `NtQuerySystemInformation` but not this (undocumented) reply layout.
#[cfg(windows)]
#[repr(C)]
struct SystemHandleTableHeader {
    number_of_handles: usize,
    reserved: usize,
    first_entry: SystemHandleTableEntry,
}

/// `SYSTEM_HANDLE_TABLE_ENTRY_INFO_EX`.
#[cfg(windows)]
#[repr(C)]
struct SystemHandleTableEntry {
    object: *mut core::ffi::c_void,
    unique_process_id: usize,
    handle_value: usize,
    granted_access: u32,
    creator_back_trace_index: u16,
    object_type_index: u16,
    handle_attributes: u32,
    reserved: u32,
}

/// Queries the whole handle table, growing the buffer until it fits.
#[cfg(windows)]
fn system_handle_table() -> Option<SystemHandleTable> {
    use windows_sys::Wdk::System::SystemInformation::NtQuerySystemInformation;
    use windows_sys::Win32::Foundation::STATUS_INFO_LENGTH_MISMATCH;

    /// `SystemExtendedHandleInformation`. Not in `windows-sys`'s generated
    /// class list, and stable in practice across every supported release.
    const SYSTEM_EXTENDED_HANDLE_INFORMATION: i32 = 64;
    /// Enough for a lightly loaded machine on the first try.
    const INITIAL_BYTES: usize = 1 << 20;
    /// A machine with a genuinely enormous handle table is not worth an
    /// unbounded allocation; the caller degrades to "no answer".
    const MAXIMUM_BYTES: usize = 256 << 20;

    let mut bytes = INITIAL_BYTES;
    while bytes <= MAXIMUM_BYTES {
        let mut buffer = vec![0u64; bytes / std::mem::size_of::<u64>()];
        let mut written = 0u32;
        // SAFETY: `buffer` is a live allocation of `bytes` bytes and is
        // described to the kernel as exactly that; `written` is a live `u32`.
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_EXTENDED_HANDLE_INFORMATION,
                buffer.as_mut_ptr().cast(),
                bytes as u32,
                &mut written,
            )
        };
        if status == STATUS_INFO_LENGTH_MISMATCH {
            bytes *= 2;
            continue;
        }
        if status < 0 {
            return None;
        }

        // SAFETY: a successful reply begins with the header.
        let handle_count = unsafe {
            let header = buffer.as_ptr().cast::<SystemHandleTableHeader>();
            (*header).number_of_handles
        };
        // Trust the byte count over the stated handle count: a reply claiming
        // more entries than it delivered would make `entries()` read past the
        // allocation.
        let header_bytes = std::mem::size_of::<usize>() * 2;
        let entry_bytes = std::mem::size_of::<SystemHandleTableEntry>();
        let deliverable = (bytes.saturating_sub(header_bytes)) / entry_bytes;
        return Some(SystemHandleTable {
            buffer,
            handle_count: handle_count.min(deliverable),
        });
    }
    None
}

/// Platforms with no implementation here answer honestly rather than wrongly;
/// [`open_files_are_observable`] tells callers which case they are in.
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn open_file_paths(process_id: u32) -> Vec<PathBuf> {
    let _ = process_id;
    Vec::new()
}

/// Whether this platform can enumerate a process's open files, so callers can
/// distinguish "this process has none" from "this platform cannot say".
pub const fn open_files_are_observable() -> bool {
    cfg!(any(target_os = "linux", target_os = "macos", windows))
}

/// Whether `process_id` currently has at least one live child process.
///
/// Exists for one question: on Windows, "is this shell sitting at its prompt,
/// or is a command it launched running?" -- which Unix answers with the
/// terminal's foreground process group, and ConPTY has no equivalent of. A
/// `cmd.exe` with no children is at its prompt.
///
/// `None` where the platform cannot say. Unix deliberately answers `None`
/// rather than implementing this: its process-group answer is strictly better,
/// because it distinguishes a *foreground* child from a backgrounded one,
/// which a child count cannot.
#[cfg(windows)]
pub fn has_live_child(process_id: u32) -> Option<bool> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    // SAFETY: plain FFI call with no pointer arguments.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

    // SAFETY: `snapshot` is live, and `entry` is a correctly sized, live
    // `PROCESSENTRY32W` whose `dwSize` was set as the API requires.
    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    let mut found_child = false;
    while has_entry {
        if entry.th32ParentProcessID == process_id && entry.th32ProcessID != process_id {
            found_child = true;
            break;
        }
        // SAFETY: same invariants as `Process32FirstW` above.
        has_entry = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }

    // SAFETY: `snapshot` came from the successful call above and is not used
    // after this point.
    unsafe { CloseHandle(snapshot) };
    Some(found_child)
}

#[cfg(not(windows))]
pub fn has_live_child(process_id: u32) -> Option<bool> {
    let _ = process_id;
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Platforms that can answer must answer correctly for the test process
    /// itself, which is the one process whose truth the test already knows.
    ///
    /// On Windows this is also the proof that the hand-written PEB layout and
    /// its offsets are right: reading the wrong offset yields a wrong path or
    /// none, so a failure here means the layout needs revisiting rather than
    /// the feature being unavailable.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
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

    /// Also the proof that the hand-written Darwin and Windows layouts are
    /// right: a wrong one yields no path, so this failing means the structures
    /// need revisiting rather than the feature being unavailable.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
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
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    #[test]
    fn open_files_are_reported_as_unobservable() {
        assert!(!open_files_are_observable());
        assert!(open_file_paths(std::process::id()).is_empty());
    }

    /// The extended-length prefix Windows puts on a resolved path is stripped
    /// before callers ever see it, because they compare against paths they
    /// built themselves, which never carry one.
    #[cfg(windows)]
    #[test]
    fn resolved_paths_lose_their_extended_length_prefix() {
        assert_eq!(
            strip_extended_length_prefix(PathBuf::from(r"\\?\C:\Users\x\transcript.jsonl")),
            PathBuf::from(r"C:\Users\x\transcript.jsonl")
        );
        assert_eq!(
            strip_extended_length_prefix(PathBuf::from(r"\\?\UNC\server\share\file.jsonl")),
            PathBuf::from(r"\\server\share\file.jsonl")
        );
        // Already-plain paths pass through untouched rather than losing a
        // leading character to an over-eager strip.
        assert_eq!(
            strip_extended_length_prefix(PathBuf::from(r"C:\plain\path")),
            PathBuf::from(r"C:\plain\path")
        );
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
