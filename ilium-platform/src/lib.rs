//! Operating-system primitives ilium needs on every supported platform.
//!
//! Every other crate is written against this crate's vocabulary instead of
//! against `std::os::unix`, `libc`, or procfs directly. The point is not to
//! hide the operating system -- it is to keep exactly one implementation of
//! each OS-specific decision, so adding a platform means editing the modules
//! here rather than hunting `#[cfg]` branches through the whole workspace.
//!
//! Module map:
//!
//! - [`secure_fs`] -- creating directories and files only the current user can
//!   read, which the log file, the session snapshot, and the start lock all
//!   need for the same reason.
//! - [`file_lock`] -- one cross-process exclusive lock, used to serialize
//!   competing first-attach processes and chatroom appends.
//! - [`process_control`] -- stopping a server process and asking whether one
//!   is still alive, the CLI's fallback when a graceful IPC shutdown cannot be
//!   delivered.
//! - [`process_info`] -- a live process's working directory and open files,
//!   which agent detection uses to identify a running CLI's session.
//! - [`runtime_dir`] -- where sockets and debug logs live, kept short enough
//!   that a Unix-domain socket path cannot overflow `sockaddr_un`.
//! - [`detached`] -- spawning a server process that outlives its parent.
//! - [`thread_priority`] -- lowering one background worker thread so CPU-heavy
//!   work never competes with keystrokes and rendering.
//!
//! Where a platform genuinely cannot answer a question ([`process_info`] on
//! Windows, which has no cheap unprivileged equivalent of `/proc/<pid>/fd`),
//! the API returns "unknown" rather than an error, and callers already treat
//! that as a normal condition instead of a failure.

pub mod detached;
pub mod file_lock;
pub mod process_control;
pub mod process_info;
pub mod runtime_dir;
pub mod secure_fs;
pub mod thread_priority;
