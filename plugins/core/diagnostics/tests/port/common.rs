//! Helpers of the `port` target: a host for each side of the plugin, with the manifest grants.

use ilium_plugin_core_diagnostics::{DiagnosticsClient, DiagnosticsServer};
use ilium_plugin_sdk::testing::FakeHost;

use crate::session_support::manifest_path;
pub use crate::session_support::{agent_debug, session};

/// A server-side host with the manifest grants and the plugin activated.
pub fn server_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<DiagnosticsServer>();
    host
}

/// A client-side host with the manifest grants and the plugin activated.
pub fn client_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build();
    host.activate::<DiagnosticsClient>();
    host
}
