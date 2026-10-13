//! Spawning and talking to `ilium-plugin-helper` for T2 and T3 plugins (reference-host-architecture.md).

use crate::error::HostError;

/// A connection to one helper process.
#[derive(Debug)]
pub struct HelperClient {
    helper_path: std::path::PathBuf,
}

impl HelperClient {
    /// Spawns the helper through `ilium-platform` and waits for `Ready`.
    pub fn spawn(helper_path: std::path::PathBuf) -> Result<HelperClient, HostError> {
        unimplemented!("host::helper_client::HelperClient::spawn")
    }

    /// Sends one framed request and waits for the reply.
    pub fn request(&mut self, payload: &[u8]) -> Result<Vec<u8>, HostError> {
        unimplemented!("host::helper_client::HelperClient::request")
    }
}
