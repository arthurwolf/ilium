//! Framed requests between the host and the helper (reference-host-architecture.md).

use serde::{Deserialize, Serialize};

/// The version of this framing; a mismatch fails loudly.
pub const HELPER_PROTOCOL_VERSION: u32 = 1;

/// Sent once by the helper when it is ready for requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ready {
    /// The helper protocol version.
    pub protocol_version: u32,
    /// The process id of the helper.
    pub helper_process_id: u32,
}

/// A request from the host to the helper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HelperRequest {
    /// Load a compiled or source component for a plugin side.
    Load {
        /// The plugin id.
        plugin: String,
        /// The component bytes.
        component: Vec<u8>,
    },
    /// Call an export with encoded arguments.
    Call {
        /// The plugin id.
        plugin: String,
        /// The export name.
        export: String,
        /// The encoded arguments.
        payload: Vec<u8>,
    },
    /// Unload a plugin side.
    Unload {
        /// The plugin id.
        plugin: String,
    },
    /// Stop the helper.
    Shutdown,
}

/// A reply from the helper to the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HelperReply {
    /// The request succeeded.
    Ok {
        /// The encoded result.
        payload: Vec<u8>,
    },
    /// The request failed with a namespaced code.
    Failed {
        /// The code, for example `trap.deadline`.
        code: String,
        /// A short message.
        message: String,
    },
    /// An import the helper cannot serve itself; the host answers it and the helper resumes.
    ForwardImport {
        /// The interface name.
        interface: String,
        /// The function name.
        function: String,
        /// The encoded arguments.
        payload: Vec<u8>,
    },
}
