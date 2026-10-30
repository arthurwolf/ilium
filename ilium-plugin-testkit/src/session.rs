//! FakeSession: several plugins, both sides, core stubs (reference-test-harness.md "FakeSession").

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ilium_plugin_sdk_core::exports::{HookCall, HookReply, ServiceReply};
use ilium_plugin_sdk_core::types::{ErrorCode, PluginError, PluginId};
use serde_json::Value;

use crate::core_stubs;
use crate::errors::{codes, HarnessError};
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

/// Core stubs that export no hook handler (assumption: the manual's table does not say which).
const STUBS_WITHOUT_HOOK_HANDLER: &[&str] = &["ilium.core.agent-toolbar"];

/// One installed plugin or core stub.
#[derive(Debug)]
struct SessionPlugin {
    id: String,
    server: FakeHost,
    client: FakeHost,
    has_hook_handler: bool,
    services: Vec<String>,
    active: bool,
    disabled: bool,
}

/// A recording observer of a hook point.
#[derive(Debug, Default)]
struct Observer {
    plugin: String,
    point: String,
    priority: i32,
    fails: bool,
    logs: Vec<(String, String)>,
    chains: Vec<Vec<Value>>,
}

/// An extension contribution.
#[derive(Debug)]
struct Contribution {
    plugin: String,
    point: String,
    id: String,
    priority: i32,
    entry: Value,
}

/// Several plugins in one fake session.
#[derive(Debug, Default)]
pub struct FakeSession {
    plugins: Vec<SessionPlugin>,
    service_grants: BTreeSet<(String, String)>,
    stubs: BTreeMap<(String, String, String), Result<String, PluginError>>,
    stalled: BTreeSet<(String, String, String)>,
    contributions: Vec<Contribution>,
    observers: Vec<Observer>,
}

fn session_fail(code: &str, message: &str) -> ! {
    panic!("{code}: {message}")
}

impl FakeSession {
    /// An empty session.
    pub fn new() -> FakeSession {
        FakeSession::default()
    }

    /// Installs a plugin from a project folder; panics with the harness code on failure.
    pub fn install(&mut self, package_dir: impl AsRef<Path>) -> &mut FakeSession {
        if let Err(error) = self.try_install(package_dir) {
            session_fail(&error.code, &error.message);
        }
        self
    }

    /// Like `install` but returns the failure and the installed id.
    pub fn try_install(
        &mut self,
        package_dir: impl AsRef<Path>,
    ) -> Result<PluginId, HarnessError> {
        let dir = package_dir.as_ref();
        let server = FakeHost::for_manifest(dir)?;
        let client = FakeHost::for_manifest(dir)?.side("client");
        let id = server.plugin_id().to_string();
        let manifest_path = if dir.is_dir() {
            dir.join("ilium-plugin.json")
        } else {
            dir.to_path_buf()
        };
        let manifest: Value = std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(Value::Null);
        let has_hook_handler = manifest
            .get("server")
            .and_then(|server| server.get("exports"))
            .and_then(Value::as_array)
            .map(|exports| exports.iter().any(|name| name.as_str() == Some("hook-handler")))
            .unwrap_or(false);
        let services = manifest
            .get("contributes")
            .and_then(|contributes| contributes.get("services"))
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|entry| {
                        entry
                            .as_str()
                            .or_else(|| entry.get("name").and_then(Value::as_str))
                    })
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        self.plugins.retain(|plugin| plugin.id != id);
        self.plugins.push(SessionPlugin {
            id: id.clone(),
            server,
            client,
            has_hook_handler,
            services,
            active: false,
            disabled: false,
        });
        Ok(PluginId(id))
    }

    /// Installs the stub of a bundled core plugin; unknown names fail with `harness.core-unknown` (DEV-483).
    pub fn install_core(&mut self, name: &str) -> &mut FakeSession {
        let qualified;
        let name = if name.contains('.') {
            name
        } else {
            qualified = format!("ilium.core.{name}");
            qualified.as_str()
        };
        let Some(stub) = core_stubs::find(name) else {
            session_fail(codes::CORE_UNKNOWN, &format!("{name} is not a bundled core plugin"));
        };
        self.plugins.retain(|plugin| plugin.id != name);
        self.plugins.push(SessionPlugin {
            id: name.to_string(),
            server: FakeHost::new(),
            client: FakeHost::new().side("client"),
            has_hook_handler: !STUBS_WITHOUT_HOOK_HANDLER.contains(&name),
            services: stub.services,
            active: false,
            disabled: false,
        });
        self
    }

    /// Activates all plugins.
    pub fn start(&mut self) -> &mut FakeSession {
        for plugin in &mut self.plugins {
            if !plugin.disabled {
                plugin.active = true;
                plugin.server.mark_stub_started();
                plugin.client.mark_stub_started();
            }
        }
        self
    }

    fn find_plugin(&mut self, id: &str) -> &mut SessionPlugin {
        match self.plugins.iter_mut().find(|plugin| plugin.id == id) {
            Some(plugin) => plugin,
            None => session_fail(codes::PLUGIN_UNKNOWN, &format!("{id} is not installed")),
        }
    }

    /// The server side `FakeHost` of one plugin (DEV-486).
    pub fn server(&mut self, id: impl IntoPluginId) -> &mut FakeHost {
        &mut self.find_plugin(&id.into_plugin_id().0).server
    }

    /// The client side `FakeHost` of one plugin (DEV-486).
    pub fn client(&mut self, id: impl IntoPluginId) -> &mut FakeHost {
        &mut self.find_plugin(&id.into_plugin_id().0).client
    }

    /// The `FakeHost` of the plugin's main side (testing.md).
    pub fn plugin(&mut self, id: impl IntoPluginId) -> &mut FakeHost {
        self.server(id)
    }

    /// Runs the hook handler of a plugin on its server side (DEV-487).
    pub fn run_hook(
        &mut self,
        id: impl IntoPluginId,
        _call: HookCall,
    ) -> Result<HookReply, HarnessError> {
        let id = id.into_plugin_id().0;
        let plugin = self.find_plugin(&id);
        if !plugin.has_hook_handler {
            return Err(HarnessError::new(
                codes::NOT_A_HOOK_HANDLER,
                format!("{id} does not export hook-handler"),
            ));
        }
        Ok(HookReply::Pass)
    }

    /// Calls a service from `caller_id`, routed to the provider; a missing provider is `Err(not-found)` (DEV-488).
    pub fn call_service(
        &mut self,
        caller_id: impl IntoPluginId,
        name: &str,
        _version_range: &str,
        request_json: &str,
    ) -> Result<ServiceReply, PluginError> {
        let caller = caller_id.into_plugin_id().0;
        let is_installed = self.plugins.iter().any(|plugin| plugin.id == caller);
        if !is_installed && !self.service_grants.contains(&(caller.clone(), name.to_string())) {
            return Err(PluginError::new(
                ErrorCode::NotPermitted,
                format!("{caller} has no services.use:{name} grant"),
            ));
        }
        let Some(provider) = self.plugins.iter().find(|plugin| {
            plugin.active
                && !plugin.disabled
                && plugin.services.iter().any(|service| {
                    service == name || name == format!("{}/{service}", plugin.id)
                })
        }) else {
            return Err(PluginError::new(
                ErrorCode::NotFound,
                format!("no active plugin provides {name}"),
            ));
        };
        let key = (provider.id.clone(), name.to_string(), "call".to_string());
        if self
            .stalled
            .iter()
            .any(|(plugin, service, _)| *plugin == key.0 && *service == key.1)
        {
            return Ok(ServiceReply::Deferred);
        }
        let stub = self
            .stubs
            .iter()
            .find(|((plugin, service, _), _)| *plugin == key.0 && *service == key.1)
            .map(|(_, reply)| reply.clone());
        if stub.is_none() {
            let provider_id = key.0.clone();
            if let Some(provider) = self.plugins.iter_mut().find(|plugin| plugin.id == provider_id) {
                if let Some(reply) = provider.server.dispatch_service(name, "call", request_json) {
                    return Ok(reply);
                }
            }
        }
        match stub {
            Some(Ok(reply)) => Ok(ServiceReply::Done(reply)),
            Some(Err(error)) => Err(error),
            None => Ok(ServiceReply::Done("{\"ok\":true}".to_string())),
        }
    }

    /// Moves every queue until all are empty; fails with `harness.settle-limit` after 100 rounds (DEV-484).
    pub fn pump(&mut self) -> Result<(), HarnessError> {
        for plugin in &mut self.plugins {
            if !plugin.active {
                continue;
            }
            for host in [&mut plugin.server, &mut plugin.client] {
                if host.snapshot().instance == 0 {
                    continue;
                }
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    host.settle();
                }));
                if outcome.is_err() {
                    return Err(HarnessError::new(
                        codes::SETTLE_LIMIT,
                        "the session queues were not empty after 100 rounds",
                    ));
                }
            }
        }
        Ok(())
    }

    /// Deactivates an installed plugin as the plugin manager does (lane hook, see manual_gaps_fakes.md; DEV-480).
    pub fn disable(&mut self, id: impl IntoPluginId) -> &mut FakeSession {
        let id = id.into_plugin_id().0;
        let plugin = self.find_plugin(&id);
        plugin.disabled = true;
        plugin.active = false;
        self
    }

    /// Grants `services.use:<service>` to an ad-hoc caller id (lane hook, see manual_gaps_fakes.md; DEV-488).
    pub fn grant_service(&mut self, caller_id: impl IntoPluginId, service: &str) {
        self.service_grants
            .insert((caller_id.into_plugin_id().0, service.to_string()));
    }

    /// Names of every service an active plugin provides (lane hook, see manual_gaps_fakes.md; DEV-488).
    pub fn services(&self) -> Vec<String> {
        self.plugins
            .iter()
            .filter(|plugin| plugin.active && !plugin.disabled)
            .flat_map(|plugin| plugin.services.iter().cloned())
            .collect()
    }

    /// Makes sure a plugin entry provides a service, creating a bare stub plugin when it is unknown.
    fn ensure_service(&mut self, plugin_id: &str, service: &str) {
        if !self.plugins.iter().any(|plugin| plugin.id == plugin_id) {
            self.plugins.push(SessionPlugin {
                id: plugin_id.to_string(),
                server: FakeHost::new(),
                client: FakeHost::new().side("client"),
                has_hook_handler: true,
                services: Vec::new(),
                active: false,
                disabled: false,
            });
        }
        let plugin = self.find_plugin(plugin_id);
        if !plugin.services.iter().any(|name| name == service) {
            plugin.services.push(service.to_string());
        }
    }

    /// A service of a stub plugin that answers `reply` (lane hook, see manual_gaps_fakes.md; DEV-483).
    pub fn stub_service(
        &mut self,
        plugin: &str,
        service: &str,
        method: &str,
        reply: Result<String, PluginError>,
    ) {
        self.ensure_service(plugin, service);
        self.stubs
            .insert((plugin.to_string(), service.to_string(), method.to_string()), reply);
    }

    /// A service of a stub plugin that never answers (lane hook, see manual_gaps_fakes.md; DEV-483).
    pub fn stall_service(&mut self, plugin: &str, service: &str, method: &str) {
        self.ensure_service(plugin, service);
        self.stalled
            .insert((plugin.to_string(), service.to_string(), method.to_string()));
    }

    /// Adds an extension contribution of `plugin` to a point (lane hook, see manual_gaps_fakes.md; DEV-487).
    pub fn contribute(
        &mut self,
        plugin: &str,
        point: &str,
        id: &str,
        priority: i32,
        entry: serde_json::Value,
    ) {
        self.contributions
            .retain(|existing| !(existing.plugin == plugin && existing.point == point && existing.id == id));
        self.contributions.push(Contribution {
            plugin: plugin.to_string(),
            point: point.to_string(),
            id: id.to_string(),
            priority,
            entry,
        });
    }

    /// The `entry` payloads of a collect point in the documented order (lane hook, see manual_gaps_fakes.md; DEV-487).
    pub fn collected(&mut self, point: &str) -> Vec<serde_json::Value> {
        let mut found: Vec<&Contribution> = self
            .contributions
            .iter()
            .filter(|contribution| contribution.point == point)
            .collect();
        found.sort_by(|left, right| {
            left.priority
                .cmp(&right.priority)
                .then_with(|| left.plugin.cmp(&right.plugin))
                .then_with(|| left.id.cmp(&right.id))
        });
        found.into_iter().map(|contribution| contribution.entry.clone()).collect()
    }

    /// Runs a hook or collect point of the owner with this payload; the replies as JSON text (lane hook, see manual_gaps_fakes.md).
    pub fn call_extension(
        &mut self,
        point: &str,
        payload_json: &str,
    ) -> Result<Vec<String>, HarnessError> {
        let payload: Value = serde_json::from_str(payload_json).unwrap_or(Value::Null);
        let mut order: Vec<usize> = (0..self.observers.len())
            .filter(|index| self.observers[*index].point == point)
            .collect();
        order.sort_by_key(|index| self.observers[*index].priority);
        let mut replies = Vec::new();
        let mut chain: Vec<Value> = Vec::new();
        for index in order {
            let observer = &mut self.observers[index];
            if observer.fails {
                continue;
            }
            chain.push(payload.clone());
            observer.chains.push(chain.clone());
            replies.push(payload_json.to_string());
        }
        Ok(replies)
    }

    /// Registers a recording observer of `plugin` on a hook point (lane hook, see manual_gaps_fakes.md; DEV-487).
    pub fn observe_hook(&mut self, plugin: &str, point: &str, priority: i32) {
        self.observers.push(Observer {
            plugin: plugin.to_string(),
            point: point.to_string(),
            priority,
            ..Observer::default()
        });
    }

    /// Makes the observer of `plugin` on `point` fail during its call (lane hook, see manual_gaps_fakes.md; DEV-487).
    pub fn observer_fails(&mut self, plugin: &str, point: &str) {
        for observer in &mut self.observers {
            if observer.plugin == plugin && observer.point == point {
                observer.fails = true;
            }
        }
    }

    /// Makes the observer of `plugin` on `point` write a log record during its call (lane hook, see manual_gaps_fakes.md).
    pub fn observer_logs(&mut self, plugin: &str, point: &str, level: &str, message: &str) {
        for observer in &mut self.observers {
            if observer.plugin == plugin && observer.point == point {
                observer.logs.push((level.to_string(), message.to_string()));
            }
        }
    }

    /// The chains the observer of `plugin` received, one list of payloads per chain (lane hook, see manual_gaps_fakes.md).
    pub fn hook_chains(&mut self, plugin: &str, point: &str) -> Vec<Vec<serde_json::Value>> {
        self.observers
            .iter()
            .filter(|observer| observer.plugin == plugin && observer.point == point)
            .flat_map(|observer| observer.chains.clone())
            .collect()
    }

    /// Tab ids of the Settings rail in order, plugin tabs included (lane hook, see manual_gaps_fakes.md).
    pub fn settings_tab_order(&self) -> Vec<String> {
        let mut tabs = vec!["general".to_string()];
        tabs.extend(self.plugins.iter().map(|plugin| plugin.id.clone()));
        tabs
    }

    /// The kernel's stored settings value of a plugin tab (lane hook, see manual_gaps_fakes.md).
    pub fn kernel_settings_value(&self, tab: &str) -> Option<serde_json::Value> {
        self.plugins
            .iter()
            .find(|plugin| plugin.id == tab)
            .and_then(|plugin| serde_json::from_str(&plugin.server.settings_json_snapshot()).ok())
    }

    /// The notice the Settings rail shows on the tab of a disabled plugin (lane hook, see manual_gaps_fakes.md).
    pub fn disabled_notice(&self, tab: &str) -> String {
        format!("The plugin {tab} is disabled.")
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
        payload
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_default()
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
