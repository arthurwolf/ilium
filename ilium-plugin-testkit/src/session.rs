//! FakeSession: several plugins, both sides, core stubs (reference-test-harness.md "FakeSession").

use std::path::Path;

use ilium_plugin_sdk_core::exports::{HookCall, HookReply, ServiceReply};
use ilium_plugin_sdk_core::types::{PluginError, PluginId};

use crate::errors::HarnessError;
use crate::host::FakeHost;

/// A plugin identifier given as text or as a `PluginId`.
pub trait IntoPluginId {
    /// Converts to a `PluginId`.
    fn into_plugin_id(self) -> PluginId;
}

impl IntoPluginId for PluginId {
    fn into_plugin_id(self) -> PluginId {
        self
    }
}
impl IntoPluginId for &PluginId {
    fn into_plugin_id(self) -> PluginId {
        self.clone()
    }
}
impl IntoPluginId for String {
    fn into_plugin_id(self) -> PluginId {
        PluginId(self)
    }
}
impl IntoPluginId for &String {
    fn into_plugin_id(self) -> PluginId {
        PluginId(self.clone())
    }
}
impl IntoPluginId for &str {
    fn into_plugin_id(self) -> PluginId {
        PluginId(self.to_string())
    }
}

/// Several plugins in one fake session.
#[derive(Debug)]
pub struct FakeSession {
    _private: (),
}

impl FakeSession {
    /// An empty session.
    pub fn new() -> FakeSession {
        FakeSession { _private: () }
    }

    /// Installs a plugin from a project folder; panics with the harness code on failure.
    pub fn install(&mut self, _package_dir: impl AsRef<Path>) -> &mut FakeSession {
        unimplemented!("DEV-480")
    }

    /// Like `install` but returns the failure and the installed id.
    pub fn try_install(&mut self, _package_dir: impl AsRef<Path>) -> Result<PluginId, HarnessError> {
        unimplemented!("DEV-480")
    }

    /// Installs the stub of a bundled core plugin; unknown names fail with `harness.core-unknown` (DEV-483).
    pub fn install_core(&mut self, _name: &str) -> &mut FakeSession {
        unimplemented!("DEV-483")
    }

    /// Activates all plugins in dependency order.
    pub fn start(&mut self) -> &mut FakeSession {
        unimplemented!("DEV-480")
    }

    /// The server side `FakeHost` of one plugin (DEV-486).
    pub fn server(&mut self, _id: impl IntoPluginId) -> &mut FakeHost {
        unimplemented!("DEV-486")
    }

    /// The client side `FakeHost` of one plugin (DEV-486).
    pub fn client(&mut self, _id: impl IntoPluginId) -> &mut FakeHost {
        unimplemented!("DEV-486")
    }

    /// The `FakeHost` of the plugin's main side (testing.md).
    pub fn plugin(&mut self, _id: impl IntoPluginId) -> &mut FakeHost {
        unimplemented!("DEV-486")
    }

    /// Runs the hook handler of a plugin on its server side (DEV-487).
    pub fn run_hook(&mut self, _id: impl IntoPluginId, _call: HookCall) -> Result<HookReply, HarnessError> {
        unimplemented!("DEV-487")
    }

    /// Calls a service from `caller_id`, routed to the provider; a missing provider is `Err(not-found)` (DEV-488).
    pub fn call_service(
        &mut self,
        _caller_id: impl IntoPluginId,
        _name: &str,
        _version_range: &str,
        _request_json: &str,
    ) -> Result<ServiceReply, PluginError> {
        unimplemented!("DEV-488")
    }

    /// Moves every queue until all are empty; fails with `harness.settle-limit` after 100 rounds (DEV-484).
    pub fn pump(&mut self) -> Result<(), HarnessError> {
        unimplemented!("DEV-484")
    }
}

impl Default for FakeSession {
    fn default() -> FakeSession {
        FakeSession::new()
    }
}
