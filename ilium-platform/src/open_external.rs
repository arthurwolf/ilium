//! Handing a URL or filesystem path to the operating system's own default
//! handler -- the browser for a URL, the registered application for a file,
//! the file manager for a folder. Every code path spawns through an argument
//! array or `ShellExecuteW`'s own parameters, never through a shell, so a
//! metacharacter in a URL or path (this crate's callers feed it terminal and
//! editor text, which can be attacker- or agent-influenced) is never
//! interpreted as a shell operator.
//!
//! Callers must restrict a URL to an allowed scheme before calling
//! [`ExternalOpenHandle::open_url`] -- this module does not re-validate it, matching every other
//! primitive in this crate that trusts its caller to enforce policy before
//! reaching the OS boundary.

use std::io;
use std::path::Path;

use crate::owned_worker::{reserve_owned_worker, OwnedWorker, StopToken, WorkerKind, WorkerTicket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// Includes openings still inside the OS spawn/ShellExecute call.
pub const MAX_EXTERNAL_CHILDREN: usize = 32;
pub const EXTERNAL_REAPER_STACK_BYTES: usize = 2 * 1024 * 1024;
const REAP_INTERVAL: Duration = Duration::from_millis(100);

/// Requested resident bookkeeping, excluding native allocator/OS overhead.
/// The caller separately admits one native worker with EXTERNAL_REAPER_STACK_BYTES requested stack.
pub fn external_opener_declared_bytes() -> usize {
    std::mem::size_of::<Shared<std::process::Child>>()
        + MAX_EXTERNAL_CHILDREN * std::mem::size_of::<Slot<std::process::Child>>()
        + crate::owned_worker::worker_retained_state_bytes()
        + 4096
}

trait ReapChild {
    fn try_reap(&mut self) -> io::Result<Option<bool>>;
}
impl ReapChild for std::process::Child {
    fn try_reap(&mut self) -> io::Result<Option<bool>> {
        self.try_wait()
            .map(|status| status.map(|status| status.success()))
    }
}
enum Slot<C> {
    Free,
    Reserved,
    Running { child: C, error_reported: bool },
}
struct Children<C> {
    slots: Vec<Slot<C>>,
}
impl<C: ReapChild> Children<C> {
    fn new() -> Self {
        Self {
            slots: std::iter::repeat_with(|| Slot::Free)
                .take(MAX_EXTERNAL_CHILDREN)
                .collect(),
        }
    }
    fn reserve(&mut self) -> io::Result<usize> {
        let index = self
            .slots
            .iter()
            .position(|slot| matches!(slot, Slot::Free))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "external opener child capacity exhausted",
                )
            })?;
        self.slots[index] = Slot::Reserved;
        Ok(index)
    }
    fn sweep(&mut self) -> usize {
        let mut occupied = 0;
        for slot in &mut self.slots {
            match slot {
                Slot::Free => {}
                Slot::Reserved => occupied += 1,
                Slot::Running {
                    child,
                    error_reported,
                } => match child.try_reap() {
                    Ok(Some(success)) => {
                        if !success {
                            tracing::warn!("external opener child exited unsuccessfully");
                        }
                        *slot = Slot::Free;
                    }
                    Ok(None) => occupied += 1,
                    Err(error) => {
                        // Error is not evidence of exit. Keep the original Child
                        // and its slot, retry later; never detach or kill it.
                        occupied += 1;
                        if !*error_reported {
                            tracing::warn!("external opener child reap failed: {error}");
                            *error_reported = true;
                        }
                    }
                },
            }
        }
        occupied
    }
}
struct Shared<C> {
    closed: AtomicBool,
    children: Mutex<Children<C>>,
    changed: Condvar,
    _custody: Option<Arc<Mutex<Box<dyn Send>>>>,
}
struct Opening<C: ReapChild> {
    shared: Arc<Shared<C>>,
    index: usize,
    pending: bool,
}
impl<C: ReapChild> Opening<C> {
    fn admit(shared: &Arc<Shared<C>>) -> io::Result<Self> {
        let mut children = shared.children.try_lock().map_err(|_| {
            io::Error::new(io::ErrorKind::WouldBlock, "external opener registry busy")
        })?;
        if shared.closed.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "external opener admission closed",
            ));
        }
        let index = children.reserve()?;
        Ok(Self {
            shared: shared.clone(),
            index,
            pending: true,
        })
    }
    fn finish(mut self, child: Option<C>) {
        // This sole permit owns the reserved index. Sweeps retain Reserved,
        // close never removes children, and no other caller can modify it.
        let mut children = self
            .shared
            .children
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        children.slots[self.index] = match child {
            Some(child) => Slot::Running {
                child,
                error_reported: false,
            },
            None => Slot::Free,
        };
        self.pending = false;
        self.shared.changed.notify_all();
    }
}
impl<C: ReapChild> Drop for Opening<C> {
    fn drop(&mut self) {
        if self.pending {
            let mut children = self
                .shared
                .children
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            children.slots[self.index] = Slot::Free;
            self.shared.changed.notify_all();
        }
    }
}

/// One explicitly composed opener service. No process-static opener registry.
/// Closing never kills a browser/editor, waits on the caller, or releases the
/// physical reaper charge while any opening or native child remains alive.
pub struct ExternalOpener {
    handle: ExternalOpenHandle,
    worker: OwnedWorker,
}
#[derive(Clone)]
pub struct ExternalOpenHandle {
    shared: Arc<Shared<std::process::Child>>,
}
impl ExternalOpener {
    /// Call at the composition root after admitting this service's physical
    /// worker/storage. Custody survives actual join and the last handle.
    /// Holding a closed handle conservatively retains the whole service lease.
    pub fn start_with_custody<C: Send + 'static>(custody: C) -> io::Result<Self> {
        let custody: Arc<Mutex<Box<dyn Send>>> = Arc::new(Mutex::new(Box::new(custody)));
        let shared = Arc::new(Shared {
            closed: AtomicBool::new(false),
            children: Mutex::new(Children::new()),
            changed: Condvar::new(),
            _custody: Some(custody.clone()),
        });
        let wake_shared = Arc::downgrade(&shared);
        let body_shared = shared.clone();
        let worker = reserve_owned_worker(Some(EXTERNAL_REAPER_STACK_BYTES), custody)?.spawn(
            "ilium-external-reaper",
            WorkerKind::Cooperative,
            StopToken::default(),
            move || {
                if let Some(shared) = wake_shared.upgrade() {
                    shared.closed.store(true, Ordering::Release);
                    shared.changed.notify_all();
                }
            },
            move |stop| run_reaper(body_shared, stop),
        )?;
        Ok(Self {
            handle: ExternalOpenHandle { shared },
            worker,
        })
    }
    pub fn handle(&self) -> ExternalOpenHandle {
        self.handle.clone()
    }
    /// Nonblocking admission closure. In-flight admitted OS calls can finish
    /// spawning and must install their children even after close. On Unix,
    /// a user application can keep the charged worker alive indefinitely.
    pub fn close(&self) {
        self.handle.shared.closed.store(true, Ordering::Release);
        self.handle.shared.changed.notify_all();
    }
    /// Optional actual-exit observation; join_until is IO-thread-only and a
    /// deadline does not change ownership or terminate external applications.
    pub fn ticket(&self) -> WorkerTicket {
        self.worker.ticket()
    }
}
impl Drop for ExternalOpener {
    fn drop(&mut self) {
        self.close();
        // OwnedWorker cancellation closes/wakes but the body deliberately
        // keeps reaping until every admitted opening/child actually retires.
    }
}
impl ExternalOpenHandle {
    /// Synchronous OS call: invoke only from bounded shared IO jobs. Callers
    /// must first restrict URLs to their allowed schemes.
    pub fn open_url(&self, url: &str) -> io::Result<()> {
        self.open_argument(std::ffi::OsStr::new(url))
    }
    /// Non-UTF8 and dash-prefixed filenames retain the existing byte contract.
    pub fn open_path(&self, path: &Path) -> io::Result<()> {
        #[cfg(unix)]
        {
            self.open_argument(&opener_path_argument(path))
        }
        #[cfg(windows)]
        {
            self.open_argument(path.as_os_str())
        }
    }
    fn open_argument(&self, argument: &std::ffi::OsStr) -> io::Result<()> {
        if argument.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "external opener target is empty",
            ));
        }
        // The reservation covers the OS call itself: no child can exist
        // before its fixed slot and persistent reaper have been admitted.
        let opening = Opening::admit(&self.shared)?;
        #[cfg(unix)]
        {
            let child = std::process::Command::new(OPEN_COMMAND)
                .arg(argument)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
            opening.finish(Some(child));
            Ok(())
        }
        #[cfg(windows)]
        {
            // ShellExecuteW transfers handler custody to the OS, returning
            // no Child handle. The slot covers this call, then retires.
            let result = shell_execute_open(argument);
            opening.finish(None);
            result
        }
    }
}
fn run_reaper<C: ReapChild>(shared: Arc<Shared<C>>, stop: StopToken) {
    loop {
        let mut children = shared
            .children
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if stop.is_stopped() {
            shared.closed.store(true, Ordering::Release);
        }
        let occupied = children.sweep();
        if shared.closed.load(Ordering::Acquire) && occupied == 0 {
            return;
        }
        // Nonblocking try_wait only, once per sweep. The condvar releases
        // the mutex; no spawn, child.wait, termination, or unbounded queue.
        // Timed wake also covers a notify arriving before wait begins.
        let _wait = shared
            .changed
            .wait_timeout(children, REAP_INTERVAL)
            .unwrap_or_else(|error| error.into_inner());
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
const OPEN_COMMAND: &str = "xdg-open";

#[cfg(target_os = "macos")]
const OPEN_COMMAND: &str = "open";

/// Makes `path` unmistakably a filename rather than an option.
///
/// Both `xdg-open` and macOS's `open` parse a leading `-` as the start of an
/// option, so a relative path whose first character is `-` -- a perfectly
/// legal filename, and one a user can click straight out of terminal output --
/// would be rejected as bad syntax by `xdg-open` or swallowed as a flag by
/// `open` instead of being opened. Prefixing `./` names the very same file
/// while removing the leading `-`. An absolute path can never begin with one,
/// so the common case borrows and allocates nothing.
///
/// A URL needs no equivalent guard: callers must already have restricted it
/// to an allowed scheme, and a scheme has to start with a letter.
#[cfg(unix)]
fn opener_path_argument(path: &Path) -> std::borrow::Cow<'_, std::ffi::OsStr> {
    use std::borrow::Cow;
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    // Inspected as bytes rather than through `to_str`: a non-UTF-8 filename is
    // still a filename and has to open exactly like any other.
    let bytes = path.as_os_str().as_bytes();
    if bytes.first() != Some(&b'-') {
        return Cow::Borrowed(path.as_os_str());
    }
    let mut prefixed: Vec<u8> = Vec::with_capacity(bytes.len() + 2);
    prefixed.extend_from_slice(b"./");
    prefixed.extend_from_slice(bytes);
    Cow::Owned(OsString::from_vec(prefixed))
}

/// `cmd /C start` is unsafe here: `cmd.exe` re-parses the whole command line
/// itself and treats `&`, `|`, `^`, `<`, `>` as its own operators regardless
/// of how the argument array was assembled. `ShellExecuteW` takes the target
/// as a single parameter with no such re-parsing step, so it is the only safe
/// way to invoke the OS's default handler for attacker-influenced text.
#[cfg(windows)]
fn shell_execute_open(target: &std::ffi::OsStr) -> io::Result<()> {
    use std::ffi::OsStr;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    // Same contract as the Unix path: an empty target is a caller mistake, not
    // a shell-level failure, and both platforms must reject it identically so
    // callers never have to branch on the operating system.
    if target.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "refusing to hand ShellExecuteW an empty target",
        ));
    }

    // Encoded straight from the caller's `OsStr` -- never round-tripped
    // through `str`, which would silently mangle a path containing an
    // unpaired UTF-16 surrogate (valid as `OsStr` on Windows, not valid
    // Unicode) into the lossy replacement character.
    let operation: Vec<u16> = OsStr::new("open").encode_wide().chain(once(0)).collect();
    let file: Vec<u16> = target.encode_wide().chain(once(0)).collect();

    // SAFETY: `operation` and `file` are NUL-terminated UTF-16 buffers kept
    // alive for the duration of this call; the remaining pointer arguments
    // are explicitly null, which `ShellExecuteW` accepts.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // A return value greater than 32 means success; anything else is one of
    // `Shell32`'s own small `SE_ERR_*` codes. Those are NOT `GetLastError`
    // codes, so they must never go through `io::Error::from_raw_os_error`,
    // which would relabel e.g. `SE_ERR_NOASSOC` (31) as the unrelated Win32
    // `ERROR_GEN_FAILURE`.
    if (result as usize) > 32 {
        Ok(())
    } else {
        Err(shell_execute_error(result as usize))
    }
}

/// Translates a `ShellExecuteW` failure return value (<= 32) into an
/// `io::Error` that names the actual shell-level failure. The `SE_ERR_*`
/// numbering only coincides with Win32 error codes for a handful of values,
/// so each known code is mapped explicitly instead of being reinterpreted as
/// an OS error number.
#[cfg(windows)]
fn shell_execute_error(code: usize) -> io::Error {
    use io::ErrorKind;

    let (kind, description) = match code {
        0 => (ErrorKind::OutOfMemory, "out of memory or resources"),
        2 => (ErrorKind::NotFound, "file not found (SE_ERR_FNF)"),
        3 => (ErrorKind::NotFound, "path not found (SE_ERR_PNF)"),
        5 => (
            ErrorKind::PermissionDenied,
            "access denied (SE_ERR_ACCESSDENIED)",
        ),
        8 => (ErrorKind::OutOfMemory, "out of memory (SE_ERR_OOM)"),
        // Documented alongside the `SE_ERR_*` codes, and returned for a target
        // that resolves to a malformed executable. Without this arm it fell
        // into the catch-all and was reported as an unknown failure.
        11 => (
            ErrorKind::InvalidData,
            "the executable file is invalid (ERROR_BAD_FORMAT)",
        ),
        26 => (ErrorKind::Other, "sharing violation (SE_ERR_SHARE)"),
        27 => (
            ErrorKind::Other,
            "incomplete or invalid file association (SE_ERR_ASSOCINCOMPLETE)",
        ),
        28 => (
            ErrorKind::TimedOut,
            "DDE transaction timed out (SE_ERR_DDETIMEOUT)",
        ),
        29 => (ErrorKind::Other, "DDE transaction failed (SE_ERR_DDEFAIL)"),
        30 => (ErrorKind::Other, "DDE server busy (SE_ERR_DDEBUSY)"),
        31 => (
            ErrorKind::Unsupported,
            "no application is associated with this file type (SE_ERR_NOASSOC)",
        ),
        32 => (
            ErrorKind::NotFound,
            "required shared library not found (SE_ERR_DLLNOTFOUND)",
        ),
        _ => (ErrorKind::Other, "unknown ShellExecuteW failure"),
    };
    io::Error::new(
        kind,
        format!("ShellExecuteW failed: {description} (code {code})"),
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_path_starting_with_a_dash_is_rewritten_so_it_cannot_read_as_an_option() {
        assert_eq!(
            &*opener_path_argument(Path::new("-weird name.txt")),
            std::ffi::OsStr::new("./-weird name.txt")
        );
    }

    #[test]
    fn an_ordinary_path_reaches_the_opener_byte_for_byte() {
        assert_eq!(
            &*opener_path_argument(Path::new("/home/user/notes.md")),
            std::ffi::OsStr::new("/home/user/notes.md")
        );
    }

    #[test]
    fn dash_prefixed_non_utf8_path_keeps_all_original_bytes() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let input = std::ffi::OsString::from_vec(vec![b'-', 0xff, b' ', b'&', b';']);
        let prepared = opener_path_argument(Path::new(&input));
        assert_eq!(
            prepared.as_bytes(),
            &[b'.', b'/', b'-', 0xff, b' ', b'&', b';']
        );
    }

    #[test]
    fn ordinary_non_utf8_path_and_shell_characters_are_not_rewritten() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let bytes = vec![b'/', b't', b'm', b'p', b'/', 0xff, b' ', b'&', b';'];
        let input = std::ffi::OsString::from_vec(bytes.clone());
        assert_eq!(opener_path_argument(Path::new(&input)).as_bytes(), bytes);
    }

    #[test]
    fn an_empty_target_fails_instead_of_reporting_a_launch_that_cannot_happen() {
        let shared = Arc::new(Shared {
            closed: AtomicBool::new(false),
            children: Mutex::new(Children::new()),
            changed: Condvar::new(),
            _custody: None,
        });
        let error = ExternalOpenHandle { shared }
            .open_url("")
            .expect_err("an empty target must never reach the system opener");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}

#[cfg(test)]
mod ownership_tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct FakeChild {
        exited: Arc<AtomicBool>,
        errors: Arc<AtomicUsize>,
        drops: Arc<AtomicUsize>,
    }
    impl ReapChild for FakeChild {
        fn try_reap(&mut self) -> io::Result<Option<bool>> {
            if self.errors.load(Ordering::Acquire) != 0 {
                self.errors.fetch_sub(1, Ordering::AcqRel);
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "fake reap interrupted",
                ));
            }
            Ok(self.exited.load(Ordering::Acquire).then_some(true))
        }
    }
    impl Drop for FakeChild {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::AcqRel);
        }
    }
    fn fake() -> (
        FakeChild,
        Arc<AtomicBool>,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
    ) {
        let exited = Arc::new(AtomicBool::new(false));
        let errors = Arc::new(AtomicUsize::new(0));
        let drops = Arc::new(AtomicUsize::new(0));
        (
            FakeChild {
                exited: exited.clone(),
                errors: errors.clone(),
                drops: drops.clone(),
            },
            exited,
            errors,
            drops,
        )
    }
    fn shared() -> Arc<Shared<FakeChild>> {
        Arc::new(Shared {
            closed: AtomicBool::new(false),
            children: Mutex::new(Children::new()),
            changed: Condvar::new(),
            _custody: None,
        })
    }

    #[test]
    fn child_limit_counts_unfinished_spawn_calls_before_any_child_exists() {
        let registry = shared();
        let mut permits = Vec::new();
        for _ in 0..MAX_EXTERNAL_CHILDREN {
            permits.push(Opening::admit(&registry).unwrap());
        }
        assert_eq!(
            Opening::admit(&registry).err().unwrap().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(
            registry.children.lock().unwrap().sweep(),
            MAX_EXTERNAL_CHILDREN
        );
        drop(permits.pop()); // Failed OS spawn releases exactly its original slot.
        let retry = Opening::admit(&registry).unwrap();
        assert_eq!(
            registry.children.lock().unwrap().slots.len(),
            MAX_EXTERNAL_CHILDREN
        );
        drop(retry);
        drop(permits);
        assert_eq!(registry.children.lock().unwrap().sweep(), 0);
    }

    #[test]
    fn close_refuses_new_requests_but_preserves_previously_admitted_spawn_and_child() {
        let registry = shared();
        let permit = Opening::admit(&registry).unwrap();
        registry.closed.store(true, Ordering::Release);
        assert_eq!(
            Opening::admit(&registry).err().unwrap().kind(),
            io::ErrorKind::BrokenPipe
        );
        assert_eq!(registry.children.lock().unwrap().sweep(), 1); // Cannot end reaper during spawn.
        let (child, exited, _, drops) = fake();
        permit.finish(Some(child)); // Already-admitted OS spawn returned after close.
        assert_eq!(registry.children.lock().unwrap().sweep(), 1);
        assert_eq!(drops.load(Ordering::Acquire), 0);
        exited.store(true, Ordering::Release);
        assert_eq!(registry.children.lock().unwrap().sweep(), 0);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[test]
    fn reap_error_retains_original_child_and_slot_until_observed_exit() {
        let registry = shared();
        let (child, exited, errors, drops) = fake();
        errors.store(1, Ordering::Release);
        Opening::admit(&registry).unwrap().finish(Some(child));
        assert_eq!(registry.children.lock().unwrap().sweep(), 1);
        assert_eq!(registry.children.lock().unwrap().sweep(), 1);
        assert_eq!(drops.load(Ordering::Acquire), 0);
        exited.store(true, Ordering::Release);
        assert_eq!(registry.children.lock().unwrap().sweep(), 0);
        assert_eq!(drops.load(Ordering::Acquire), 1);
        assert!(Opening::admit(&registry).is_ok());
    }

    #[test]
    fn registry_busy_is_retryable_without_allocating_or_starting_an_opener() {
        let registry = shared();
        let _held = registry.children.lock().unwrap();
        assert_eq!(
            Opening::admit(&registry).err().unwrap().kind(),
            io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn cheap_handle_storage_keeps_custody_until_last_strong_owner_drops() {
        struct Custody(Arc<AtomicBool>);
        impl Drop for Custody {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let retired = Arc::new(AtomicBool::new(false));
        let custody: Arc<Mutex<Box<dyn Send>>> =
            Arc::new(Mutex::new(Box::new(Custody(retired.clone()))));
        let shared = Arc::new(Shared::<FakeChild> {
            closed: AtomicBool::new(false),
            children: Mutex::new(Children::new()),
            changed: Condvar::new(),
            _custody: Some(custody.clone()),
        });
        let handle = shared.clone();
        let wake = Arc::downgrade(&shared);
        drop(custody); // Record's original copy released after hypothetical join.
        drop(shared);
        assert!(!retired.load(Ordering::Acquire));
        drop(handle);
        assert!(retired.load(Ordering::Acquire));
        assert!(wake.upgrade().is_none()); // Observer wake does not retain the slab.
    }

    #[test]
    fn closing_owner_does_not_drop_charged_worker_before_fake_child_exit() {
        struct Custody(Arc<AtomicBool>);
        impl Drop for Custody {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let registry = shared();
        let (child, exited, _, drops) = fake();
        Opening::admit(&registry).unwrap().finish(Some(child));
        let retired = Arc::new(AtomicBool::new(false));
        let wake = registry.clone();
        let body = registry.clone();
        let worker =
            reserve_owned_worker(Some(EXTERNAL_REAPER_STACK_BYTES), Custody(retired.clone()))
                .unwrap()
                .spawn(
                    "fake-external-reaper",
                    WorkerKind::Cooperative,
                    StopToken::default(),
                    move || {
                        wake.closed.store(true, Ordering::Release);
                        wake.changed.notify_all();
                    },
                    move |stop| run_reaper(body, stop),
                )
                .unwrap();
        let ticket = worker.ticket();
        drop(worker); // Cancels logical owner, never fake child.
        assert!(ticket.join_until(std::time::Instant::now()).is_err());
        assert!(!retired.load(Ordering::Acquire));
        assert_eq!(drops.load(Ordering::Acquire), 0);
        exited.store(true, Ordering::Release);
        registry.changed.notify_all();
        assert_eq!(
            ticket
                .join_until(std::time::Instant::now() + Duration::from_secs(5))
                .unwrap(),
            crate::owned_worker::WorkerExit::Joined
        );
        assert!(retired.load(Ordering::Acquire));
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }
}

#[cfg(all(test, windows))]
mod windows_contract_tests {
    use super::*;

    #[test]
    fn shell_failures_keep_shell_error_names_and_categories() {
        for (code, kind, label) in [
            (5, io::ErrorKind::PermissionDenied, "SE_ERR_ACCESSDENIED"),
            (11, io::ErrorKind::InvalidData, "ERROR_BAD_FORMAT"),
            (28, io::ErrorKind::TimedOut, "SE_ERR_DDETIMEOUT"),
            (31, io::ErrorKind::Unsupported, "SE_ERR_NOASSOC"),
        ] {
            let error = shell_execute_error(code);
            assert_eq!(error.kind(), kind);
            assert!(error.to_string().contains(label));
        }
    }
}

#[cfg(all(test, unix))]
mod real_child_tests {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    #[test]
    fn closing_reaper_preserves_real_child_until_child_exits_and_then_joins() {
        // This task-owned child blocks on its own pipe, never on the user's
        // terminal. EOF also ends the child if an assertion fails early.
        let mut child = Command::new("/bin/sh")
            .args(["-c", "read line"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let owner = ExternalOpener::start_with_custody(()).unwrap();
        Opening::admit(&owner.handle.shared)
            .unwrap()
            .finish(Some(child));
        let ticket = owner.ticket();
        owner.close();
        drop(owner);
        assert!(
            ticket.join_until(Instant::now()).is_err(),
            "closing discarded a still-live real child"
        );
        input.write_all(b"finish\n").unwrap();
        drop(input);
        assert_eq!(
            ticket
                .join_until(Instant::now() + Duration::from_secs(5))
                .unwrap(),
            crate::owned_worker::WorkerExit::Joined
        );
    }
}
