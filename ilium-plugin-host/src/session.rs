//! Session-wide limits shared by every plugin of a session (reference-errors-and-limits.md HST-40x).

/// The session limits after applying the user's settings (reference-errors-and-limits.md HST-406).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLimits {
    /// Memory of every plugin instance together, in MiB.
    pub session_memory_mib: u64,
    /// Helper processes.
    pub session_helpers: u64,
    /// Child processes.
    pub session_processes: u64,
    /// Open HTTP streams.
    pub session_http_streams: u64,
    /// Decorations.
    pub session_decorations: u64,
    /// Pending requests.
    pub session_pending_requests: u64,
    /// Key grabs.
    pub session_key_grabs: u64,
    /// Virtual children.
    pub session_children: u64,
}

impl Default for SessionLimits {
    /// The documented defaults.
    fn default() -> SessionLimits {
        unimplemented!("host::session::SessionLimits::default")
    }
}

impl SessionLimits {
    /// The defaults overridden by the user's `(key, value)` settings; higher values are clamped.
    pub fn from_user(_settings: &[(&str, u64)]) -> SessionLimits {
        unimplemented!("host::session::SessionLimits::from_user")
    }
}
