//! Client-side plugin management over the server's lifecycle transaction.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ilium_ipc::plugin::{ControlBody, EnvelopeBody};
use ilium_plugin_api::generated::host::plugins::{HealthState, HostPlugins, PackageState, PluginInfo, SideState, InstanceState};
use ilium_plugin_api::generated::types::{ErrorCode, Outcome, PluginError, PluginId, RequestId};
use ilium_plugin_host::imports::env::{completion_event, ImportEnv};
use ilium_plugin_host::runtime::pending_events::{EventSink, WorkerGuard};
use ilium_plugin_host::runtime::InstanceContext;
use ilium_plugin_host::wiring;

use super::{Output, Shared, OUTBOX_LIMIT};

const PENDING_LIMIT: usize = 64;
const WAIT: Duration = Duration::from_secs(40);

pub(super) struct PendingChange {
    owner_token: u64,
    target: String,
    request: RequestId,
    sink: EventSink,
    deadline: Instant,
    _worker: WorkerGuard,
}

pub(super) struct ClientManagement {
    shared: Arc<Shared>,
    owner: String,
    owner_token: u64,
    is_job: bool,
    env: ImportEnv,
}

impl ClientManagement {
    pub(super) fn new(shared: Arc<Shared>, context: &InstanceContext) -> Self {
        let owner_token = shared.management_owner_next.fetch_add(1, Ordering::Relaxed);
        Self { shared, owner: context.plugin.clone(), owner_token, is_job: context.job_requests.is_some(), env: ImportEnv::for_instance(context) }
    }
}

impl Drop for ClientManagement {
    fn drop(&mut self) {
        if let Ok(mut pending) = self.shared.management.lock() {
            pending.retain(|_, change| change.owner_token != self.owner_token);
        }
    }
}

impl HostPlugins for ClientManagement {
    fn list(&mut self) -> Vec<PluginInfo> {
        let Ok(states) = self.shared.management_states.lock() else { return Vec::new() };
        let Ok(disabled) = self.shared.disabled.lock() else { return Vec::new() };
        wiring::plugin_rows(&self.shared.management_catalogue, &disabled, &states).into_iter().map(|row| {
            let manifest = self.shared.management_catalogue.iter().find(|item| item.id == row.id)
                .and_then(|item| serde_json::from_str::<serde_json::Value>(&item.manifest).ok());
            let field = |name: &str| manifest.as_ref().and_then(|value| value.get(name)).and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
            let enabled = !wiring::is_disabled_state(&row.state);
            let failure = row.state.strip_prefix("failed:").map(str::to_owned);
            let instance_state = if failure.is_some() { InstanceState::Failed } else if row.state == "running" { InstanceState::Active } else if row.state == "stopped" { InstanceState::Stopped } else { InstanceState::Inactive };
            PluginInfo {
                id: PluginId(row.id), name: row.title, version: row.version, tier: field("tier"), enabled,
                failed: failure.is_some(),
                state: if enabled { PackageState::InstalledEnabled } else { PackageState::InstalledDisabledByUser },
                side_states: vec![SideState { side: "server".into(), scope: "server".into(), state: instance_state, failures_in_window: 0, last_error_code: failure.clone(), restart_at_unix_ms: None }],
                scope: "global".into(), disabled_reason: row.essential.then(|| "This plugin is required by the host".to_owned()),
                health: if failure.is_some() { HealthState::Failed } else if !enabled { HealthState::Disabled } else if row.state == "running" { HealthState::Healthy } else { HealthState::Dormant },
                publisher: field("publisher"), publisher_key: None, source: String::new(), channel: String::new(), pinned: false,
                update_available: None, deprecated: false, yanked: false, held_until: None,
            }
        }).collect()
    }

    fn set_enabled(&mut self, plugin: PluginId, enabled: bool, scope: String) -> Result<RequestId, PluginError> {
        if self.is_job {
            return Err(PluginError::new(ErrorCode::Unsupported, "plugin changes are not available in jobs"));
        }
        if self.shared.stopping.load(Ordering::Acquire) {
            return Err(PluginError::new(ErrorCode::Cancelled, "the plugin host is stopping"));
        }
        if !matches!(scope.as_str(), "global" | "project" | "session") {
            return Err(PluginError::new(ErrorCode::InvalidArgument, "invalid plugin scope"));
        }
        if scope != "global" {
            return Err(PluginError::new(ErrorCode::Unsupported, "only global plugin scope is implemented"));
        }
        if !self.shared.management_catalogue.iter().any(|item| item.id == plugin.0) {
            return Err(PluginError::new(ErrorCode::NotFound, "plugin is not installed"));
        }
        let mut pending = self.shared.management.lock().map_err(|_| PluginError::new(ErrorCode::Internal, "plugin management state is unavailable"))?;
        if pending.len() >= PENDING_LIMIT {
            return Err(PluginError::new(ErrorCode::Busy, "too many plugin changes are pending"));
        }
        let mut outbox = self.shared.outbox.lock().map_err(|_| PluginError::new(ErrorCode::Internal, "plugin request queue is unavailable"))?;
        if outbox.len() >= OUTBOX_LIMIT {
            return Err(PluginError::new(ErrorCode::Busy, "the plugin request queue is full"));
        }
        let (sequence, wire) = self.shared.envelope(&self.owner, EnvelopeBody::Control(ControlBody::SetEnabled { plugin: plugin.0.clone(), enabled }));
        let request = self.env.next_request();
        pending.insert(sequence, PendingChange { owner_token: self.owner_token, target: plugin.0, request, sink: self.env.sink(), deadline: Instant::now() + WAIT, _worker: self.env.worker() });
        outbox.push_back(Output::Request(wire));
        drop(outbox);
        drop(pending);
        self.shared.notification.notify_one();
        Ok(request)
    }
}

pub(super) fn complete(shared: &Shared, sequence: u64, target: &str, state: &str) {
    let change = shared.management.lock().ok().and_then(|mut pending| {
        if pending.get(&sequence).is_some_and(|change| change.target == target) { pending.remove(&sequence) } else { None }
    });
    if let Some(change) = change {
        let outcome = match state.strip_prefix("refused:") {
            Some("host.timeout") => Outcome::Failed(PluginError::new(ErrorCode::TimedOut, "The server did not confirm the change; it may still be applied")),
            Some("host.stopping") => Outcome::Cancelled,
            Some("plugin.unknown") => Outcome::Refused(PluginError::new(ErrorCode::NotFound, "plugin.unknown")),
            Some("plugin.essential") => Outcome::Refused(PluginError::new(ErrorCode::NotPermitted, "plugin.essential")),
            Some(code) => Outcome::Failed(PluginError::new(ErrorCode::Internal, code)),
            None if state.starts_with("failed:") || state == "stopped" => Outcome::Failed(PluginError::new(ErrorCode::Internal, format!("Plugin did not start: {state}"))),
            None => Outcome::Ok,
        };
        change.sink.send(completion_event(change.request, outcome));
    }
}

pub(super) fn expire(shared: &Shared) {
    let now = Instant::now();
    let expired = shared.management.lock().map(|mut pending| {
        let keys: Vec<_> = pending.iter().filter_map(|(key, change)| (change.deadline <= now).then_some(*key)).collect();
        keys.into_iter().filter_map(|key| pending.remove(&key)).collect::<Vec<_>>()
    }).unwrap_or_default();
    for change in expired {
        change.sink.send(completion_event(change.request, Outcome::Failed(PluginError::new(ErrorCode::TimedOut, "The server did not confirm the change; it may still be applied"))));
    }
}

pub(super) fn cancel_all(shared: &Shared) {
    if let Ok(mut pending) = shared.management.lock() {
        pending.clear();
    }
}
