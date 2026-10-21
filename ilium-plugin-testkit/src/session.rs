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
    pub fn try_install(
        &mut self,
        _package_dir: impl AsRef<Path>,
    ) -> Result<PluginId, HarnessError> {
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
    pub fn run_hook(
        &mut self,
        _id: impl IntoPluginId,
        _call: HookCall,
    ) -> Result<HookReply, HarnessError> {
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

    /// Deactivates an installed plugin as the plugin manager does (lane hook, see manual_gaps_fakes.md; DEV-480).
    pub fn disable(&mut self, _id: impl IntoPluginId) -> &mut FakeSession {
        unimplemented!("DEV-480")
    }

    /// Grants `services.use:<service>` to an ad-hoc caller id (lane hook, see manual_gaps_fakes.md; DEV-488).
    pub fn grant_service(&mut self, _caller_id: impl IntoPluginId, _service: &str) {
        unimplemented!("DEV-488")
    }

    /// Names of every service an active plugin provides (lane hook, see manual_gaps_fakes.md; DEV-488).
    pub fn services(&self) -> Vec<String> {
        unimplemented!("DEV-488")
    }

    /// A service of a stub plugin that answers `reply` (lane hook, see manual_gaps_fakes.md; DEV-483).
    pub fn stub_service(
        &mut self,
        _plugin: &str,
        _service: &str,
        _method: &str,
        _reply: Result<String, PluginError>,
    ) {
        unimplemented!("DEV-483")
    }

    /// A service of a stub plugin that never answers (lane hook, see manual_gaps_fakes.md; DEV-483).
    pub fn stall_service(&mut self, _plugin: &str, _service: &str, _method: &str) {
        unimplemented!("DEV-483")
    }

    /// Adds an extension contribution of `plugin` to a point (lane hook, see manual_gaps_fakes.md; DEV-487).
    pub fn contribute(
        &mut self,
        _plugin: &str,
        _point: &str,
        _id: &str,
        _priority: i32,
        _entry: serde_json::Value,
    ) {
        unimplemented!("DEV-487")
    }

    /// The `entry` payloads of a collect point in the documented order (lane hook, see manual_gaps_fakes.md; DEV-487).
    pub fn collected(&mut self, _point: &str) -> Vec<serde_json::Value> {
        unimplemented!("DEV-487")
    }

    /// Runs a hook or collect point of the owner with this payload; the replies as JSON text (lane hook, see manual_gaps_fakes.md).
    pub fn call_extension(
        &mut self,
        _point: &str,
        _payload_json: &str,
    ) -> Result<Vec<String>, HarnessError> {
        unimplemented!("DEV-487")
    }

    /// Registers a recording observer of `plugin` on a hook point (lane hook, see manual_gaps_fakes.md; DEV-487).
    pub fn observe_hook(&mut self, _plugin: &str, _point: &str, _priority: i32) {
        unimplemented!("DEV-487")
    }

    /// Makes the observer of `plugin` on `point` fail during its call (lane hook, see manual_gaps_fakes.md; DEV-487).
    pub fn observer_fails(&mut self, _plugin: &str, _point: &str) {
        unimplemented!("DEV-487")
    }

    /// Makes the observer of `plugin` on `point` write a log record during its call (lane hook, see manual_gaps_fakes.md).
    pub fn observer_logs(&mut self, _plugin: &str, _point: &str, _level: &str, _message: &str) {
        unimplemented!("DEV-487")
    }

    /// The chains the observer of `plugin` received, one list of payloads per chain (lane hook, see manual_gaps_fakes.md).
    pub fn hook_chains(&mut self, _plugin: &str, _point: &str) -> Vec<Vec<serde_json::Value>> {
        unimplemented!("DEV-487")
    }

    /// Tab ids of the Settings rail in order, plugin tabs included (lane hook, see manual_gaps_fakes.md).
    pub fn settings_tab_order(&self) -> Vec<String> {
        unimplemented!("DEV-480")
    }

    /// The kernel's stored settings value of a plugin tab (lane hook, see manual_gaps_fakes.md).
    pub fn kernel_settings_value(&self, _tab: &str) -> Option<serde_json::Value> {
        unimplemented!("DEV-480")
    }

    /// The notice the Settings rail shows on the tab of a disabled plugin (lane hook, see manual_gaps_fakes.md).
    pub fn disabled_notice(&self, _tab: &str) -> String {
        unimplemented!("DEV-480")
    }
}

impl Default for FakeSession {
    fn default() -> FakeSession {
        FakeSession::new()
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use crate::errors::codes;
    use ilium_plugin_sdk_core::types::ErrorCode;

    /// Runs an action that must panic and returns the panic text.
    fn panic_message<F: FnOnce()>(action: F) -> String {
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action))
            .expect_err("the call was expected to fail");
        if let Some(text) = payload.downcast_ref::<&str>() {
            return text.to_string();
        }
        payload.downcast_ref::<String>().cloned().unwrap_or_default()
    }

    /// REQ: DEV-480
    #[test]
    fn core_stub_service_is_listed_once_active() {
        let mut session = FakeSession::new();
        session.install_core("ilium.core.tree").start();
        assert!(session
            .services()
            .iter()
            .any(|name| name.as_str() == "bookmarks"));
    }

    /// REQ: DEV-483
    #[test]
    fn core_stub_exposes_only_its_own_services() {
        let mut session = FakeSession::new();
        session.install_core("ilium.core.tree").start();
        assert_eq!(session.services(), vec!["bookmarks".to_string()]);
    }

    /// REQ: DEV-483
    #[test]
    fn install_core_rejects_unknown_core_name() {
        let mut session = FakeSession::new();
        let message = panic_message(|| {
            session.install_core("ilium.core.does-not-exist");
        });
        assert!(
            message.contains(codes::CORE_UNKNOWN),
            "panic message was: {message}"
        );
    }

    /// REQ: DEV-484
    #[test]
    fn pump_on_empty_session_succeeds() {
        let mut session = FakeSession::new();
        assert_eq!(session.pump(), Ok(()));
    }

    /// REQ: DEV-486
    #[test]
    fn server_of_uninstalled_plugin_fails_with_plugin_unknown() {
        let mut session = FakeSession::new();
        let message = panic_message(|| {
            session.server("ilium.test.absent");
        });
        assert!(
            message.contains(codes::PLUGIN_UNKNOWN),
            "panic message was: {message}"
        );
    }

    /// REQ: DEV-487
    #[test]
    fn run_hook_returns_pass_from_core_stub() {
        let mut session = FakeSession::new();
        session.install_core("ilium.core.tree").start();
        let call = HookCall {
            call: 1,
            point: "before-move".to_string(),
            extension: String::new(),
            payload_json: "{}".to_string(),
        };
        let reply = session
            .run_hook("ilium.core.tree", call)
            .expect("the stub hook runs");
        assert_eq!(reply, HookReply::Pass);
    }

    /// REQ: DEV-487
    #[test]
    fn run_hook_on_plugin_without_hook_handler_fails() {
        let mut session = FakeSession::new();
        session.install_core("ilium.core.agent-toolbar").start();
        let call = HookCall {
            call: 2,
            point: "toolbar-actions".to_string(),
            extension: String::new(),
            payload_json: "{}".to_string(),
        };
        let error = session
            .run_hook("ilium.core.agent-toolbar", call)
            .expect_err("the stub has no hook handler");
        assert_eq!(error.code, codes::NOT_A_HOOK_HANDLER);
    }

    /// REQ: DEV-488
    #[test]
    fn call_service_returns_stub_provider_reply() {
        let mut session = FakeSession::new();
        session.install_core("ilium.core.tree").start();
        session.grant_service("ilium.test.caller", "bookmarks");
        let reply = session.call_service("ilium.test.caller", "bookmarks", "*", "{}");
        match reply {
            Ok(ServiceReply::Done(payload)) => {
                let value: serde_json::Value =
                    serde_json::from_str(&payload).expect("the reply is JSON");
                assert_eq!(value, serde_json::json!({"ok": true}));
            }
            other => panic!("expected a done reply, got {other:?}"),
        }
    }

    /// REQ: DEV-488
    #[test]
    fn call_service_without_provider_is_not_found() {
        let mut session = FakeSession::new();
        session.grant_service("ilium.test.caller", "no-such-service");
        let error = session
            .call_service("ilium.test.caller", "no-such-service", "*", "{}")
            .expect_err("no provider is installed");
        assert_eq!(error.code, ErrorCode::NotFound);
    }
}
