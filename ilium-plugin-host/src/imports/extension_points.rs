//! `ilium:host/extensions` of the running host (foundation contract section 20.5).
//!
//! A point exists while its owner is enabled: it is the owner's `extension_points` entry in
//! the contribution registry, so a disabled owner takes its points with it. Entries that
//! plugins register at run time live in one book the runtime shares between its plugins.
//! Each plugin gets its own import, which knows the plugin and its grants.
//!
//! `run-hook` validates the call and queues it as a [`PendingHook`]. The runtime's hook pump
//! (`runtime/hooks.rs`) takes the queue, runs the handler chain one handler after another and
//! stores the folded reply with [`ExtensionBook::finish`]; `take-reply` returns it.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use ilium_plugin_api::generated::host::extensions::{
    ExtensionEntry, ExtensionError, HostExtensions,
};
use ilium_plugin_api::generated::types::ids::{PluginId, RequestId};
use serde_json::Value;

use crate::contributions::ContributionRegistry;

use super::env::RequestIds;

/// Largest payload of a hook chain in bytes (EXT-023).
const PAYLOAD_CAP_BYTES: usize = 64 * 1024;
/// Largest contributor entry in bytes (EXT-023).
const ENTRY_CAP_BYTES: usize = 16 * 1024;
/// Highest priority an entry may carry (EXT-007).
const PRIORITY_MAX: u8 = 100;
/// The priority of a manifest entry that names none.
const PRIORITY_DEFAULT: u8 = 50;
/// Entries a point holds when its manifest names no `max_entries`.
const ENTRIES_DEFAULT_MAX: usize = 64;
/// Hook requests of one plugin whose reply was not taken yet.
const PENDING_REPLIES_MAX: usize = 256;
/// Deepest chain nesting: a `run-hook` from a handler at this depth fails (EXT-046).
const CHAIN_DEPTH_MAX: usize = 3;
/// Deadline of one handler when the point names none, in milliseconds.
const HANDLER_DEADLINE_DEFAULT_MS: u64 = 20;
/// Range a point's `deadline_ms` is clamped into, in milliseconds.
const HANDLER_DEADLINE_RANGE_MS: (u64, u64) = (5, 50);
/// Handlers a hook point runs when its manifest names no `max_handlers`.
const HANDLERS_DEFAULT_MAX: usize = 8;
/// `run-hook` calls one point accepts per second (EXT-048).
const RUN_HOOK_PER_SECOND: usize = 200;

/// A `run-hook` call that waits for its handler chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingHook {
    /// The plugin that owns the point and called `run-hook`.
    pub caller: String,
    /// The request id `run-hook` returned.
    pub request: u64,
    /// The global point name `<owner>/<point>`.
    pub point: String,
    /// The payload the owner passed.
    pub payload_json: String,
    /// The points of the chains this call was started inside, outermost first. Empty for an
    /// outermost chain (depth 1).
    pub stack: Vec<String>,
}

/// What plugins registered at run time, for every point.
#[derive(Debug, Default)]
pub struct BookState {
    /// Entries by global point name `<owner>/<point>`.
    entries: BTreeMap<String, Vec<ExtensionEntry>>,
    /// Hook requests the pump did not run yet, oldest first.
    queue: VecDeque<PendingHook>,
    /// The chains the handler now running sits inside, outermost first; empty between chains.
    stack: Vec<String>,
    /// Completed hook requests by (calling plugin, request id).
    replies: HashMap<(String, u64), Option<String>>,
    /// When `run-hook` calls on each point were accepted in the last second.
    accepted: HashMap<String, VecDeque<Instant>>,
    /// The request ids of the runtime; hook requests share them with every other request kind.
    ids: RequestIds,
}

/// The book of run-time extension entries, shared by the plugins of one runtime.
#[derive(Debug, Clone, Default)]
pub struct ExtensionBook(Arc<Mutex<BookState>>);

impl ExtensionBook {
    /// A book whose hook request ids come from `ids`, the allocator of the runtime.
    pub fn with_ids(ids: RequestIds) -> Self {
        Self(Arc::new(Mutex::new(BookState {
            ids,
            ..BookState::default()
        })))
    }

    fn state(&self) -> MutexGuard<'_, BookState> {
        // Every writer does one insert or remove, so the data is consistent after a panic.
        self.0.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    /// Drops what a plugin registered, the entries of the points it owns and its pending
    /// hook replies. Called when the plugin is disabled: its points are gone with it.
    pub fn forget_plugin(&self, plugin: &str) {
        let owned = format!("{plugin}/");
        self.state()
            .entries
            .retain(|point, _| !point.starts_with(&owned));
        self.forget_instance(plugin);
    }

    /// Drops what one instance of a plugin registered and its pending hook replies. Called
    /// when the plugin fails or gets a new instance; the points it owns keep the entries
    /// other plugins registered.
    pub fn forget_instance(&self, plugin: &str) {
        let mut state = self.state();
        for entries in state.entries.values_mut() {
            entries.retain(|entry| entry.contributor.0 != plugin);
        }
        state.replies.retain(|(caller, _), _| caller != plugin);
        state.queue.retain(|hook| hook.caller != plugin);
    }

    /// Takes every queued hook request, oldest first. The runtime's pump runs them.
    pub(crate) fn take_queue(&self) -> Vec<PendingHook> {
        self.state().queue.drain(..).collect()
    }

    /// Sets the chains the handler about to run sits inside; `run-hook` calls made by that
    /// handler are queued one level deeper and refused when they name a point on it.
    pub(crate) fn set_stack(&self, stack: Vec<String>) {
        self.state().stack = stack;
    }

    /// Stores the folded reply of a finished chain for `take-reply`.
    pub(crate) fn finish(&self, caller: &str, request: u64, reply: Option<String>) {
        self.state()
            .replies
            .insert((caller.to_owned(), request), reply);
    }

    /// The entries of a point in run order: priority, contributor, extension id.
    pub fn entries_of(&self, point: &str) -> Vec<ExtensionEntry> {
        let mut entries = self.state().entries.get(point).cloned().unwrap_or_default();
        entries.sort_by(|left, right| {
            (left.priority, &left.contributor.0, &left.extension).cmp(&(
                right.priority,
                &right.contributor.0,
                &right.extension,
            ))
        });
        entries
    }
}

/// What the manifest of the owner says about one point.
struct Declared {
    dynamic: bool,
    max_entries: usize,
    payload_schema: Option<Value>,
}

/// `ilium:host/extensions` of one plugin.
pub struct PluginExtensions {
    plugin: String,
    granted: Vec<String>,
    registry: Arc<Mutex<ContributionRegistry>>,
    book: ExtensionBook,
}

impl PluginExtensions {
    /// The import of one plugin instance over the runtime's registry and book.
    pub fn new(
        plugin: &str,
        granted: &[String],
        registry: Arc<Mutex<ContributionRegistry>>,
        book: ExtensionBook,
    ) -> Self {
        Self {
            plugin: plugin.to_owned(),
            granted: granted.to_vec(),
            registry,
            book,
        }
    }

    fn declared(&self, point: &str) -> Result<Declared, ExtensionError> {
        let registry = self
            .registry
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let entry = registry
            .find_global("extension_points", point)
            .ok_or(ExtensionError::UnknownPoint)?;
        Ok(Declared {
            dynamic: entry
                .payload
                .get("dynamic")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            max_entries: entry
                .payload
                .get("max_entries")
                .and_then(Value::as_u64)
                .and_then(|max| usize::try_from(max).ok())
                .unwrap_or(ENTRIES_DEFAULT_MAX),
            payload_schema: entry.payload.get("payload_schema").cloned(),
        })
    }

    /// The entries enabled plugins declare for a point in their manifests.
    fn manifest_entries(&self, point: &str) -> Vec<ExtensionEntry> {
        let registry = self
            .registry
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        registry
            .entries("extensions")
            .into_iter()
            .filter(|entry| entry.payload.get("point").and_then(Value::as_str) == Some(point))
            .filter_map(|entry| {
                let payload = entry.payload.get("entry")?;
                Some(ExtensionEntry {
                    extension: entry.name.clone(),
                    contributor: PluginId(entry.plugin.clone()),
                    priority: entry
                        .payload
                        .get("priority")
                        .and_then(Value::as_u64)
                        .and_then(|priority| u8::try_from(priority).ok())
                        .unwrap_or(PRIORITY_DEFAULT),
                    entry_json: payload.to_string(),
                })
            })
            .collect()
    }

    fn owns(&self, point: &str) -> bool {
        point
            .split_once('/')
            .is_some_and(|(owner, _)| owner == self.plugin)
    }

    fn holds(&self, family: &str, point: &str) -> bool {
        let wanted = format!("{family}:{point}");
        self.granted.iter().any(|grant| *grant == wanted)
    }

    /// The owner reads and runs its point; `extensions.provide` names the same right.
    fn provides(&self, point: &str) -> bool {
        self.owns(point) || self.holds("extensions.provide", point)
    }
}

impl HostExtensions for PluginExtensions {
    fn list(&mut self, point: String) -> Result<Vec<ExtensionEntry>, ExtensionError> {
        self.declared(&point)?;
        if !self.provides(&point) && !self.holds("extensions.contribute", &point) {
            return Err(ExtensionError::NotGranted);
        }
        // Entries come from two places: the `contributes.extensions` lists of the enabled
        // plugins' manifests, and what plugins registered at run time.
        let mut entries = self.manifest_entries(&point);
        entries.extend(self.book.entries_of(&point));
        entries.sort_by(|left, right| {
            (left.priority, &left.contributor.0, &left.extension).cmp(&(
                right.priority,
                &right.contributor.0,
                &right.extension,
            ))
        });
        Ok(entries)
    }

    fn run_hook(
        &mut self,
        point: String,
        payload_json: String,
    ) -> Result<RequestId, ExtensionError> {
        if payload_json.len() > PAYLOAD_CAP_BYTES {
            return Err(ExtensionError::CapExceeded);
        }
        let declared = self.declared(&point)?;
        if !self.provides(&point) {
            return Err(ExtensionError::NotGranted);
        }
        let Ok(payload) = serde_json::from_str::<Value>(&payload_json) else {
            return Err(ExtensionError::SchemaMismatch);
        };
        if let Some(schema) = &declared.payload_schema {
            if !conforms(schema, &payload) {
                return Err(ExtensionError::SchemaMismatch);
            }
        }
        let mut state = self.book.state();
        // A chain already running on this point, directly or through other points, cannot
        // start again (EXT-022), and a handler at depth 3 cannot start a deeper one (EXT-046).
        if state.stack.iter().any(|running| *running == point) {
            return Err(ExtensionError::Reentrant);
        }
        if state.stack.len() >= CHAIN_DEPTH_MAX {
            return Err(ExtensionError::Reentrant);
        }
        let queued = state
            .queue
            .iter()
            .filter(|hook| hook.caller == self.plugin)
            .count();
        let answered = state
            .replies
            .keys()
            .filter(|(caller, _)| *caller == self.plugin)
            .count();
        if queued + answered >= PENDING_REPLIES_MAX {
            return Err(ExtensionError::CapExceeded);
        }
        // More than 200 calls in one second on one point are refused (EXT-048). The WIT
        // `extension-error` has no rate-limited case, so the closest case reports it.
        let now = Instant::now();
        let window = state.accepted.entry(point.clone()).or_default();
        while window
            .front()
            .is_some_and(|accepted| now.duration_since(*accepted) >= Duration::from_secs(1))
        {
            window.pop_front();
        }
        if window.len() >= RUN_HOOK_PER_SECOND {
            return Err(ExtensionError::CapExceeded);
        }
        window.push_back(now);
        let request = state.ids.next_request().0;
        let stack = state.stack.clone();
        state.queue.push_back(PendingHook {
            caller: self.plugin.clone(),
            request,
            point,
            payload_json,
            stack,
        });
        Ok(RequestId(request))
    }

    fn take_reply(&mut self, request: RequestId) -> Option<String> {
        self.book
            .state()
            .replies
            .remove(&(self.plugin.clone(), request.0))
            .flatten()
    }

    fn register(
        &mut self,
        point: String,
        extension: String,
        priority: u8,
        entry_json: String,
    ) -> Result<(), ExtensionError> {
        if entry_json.len() > ENTRY_CAP_BYTES {
            return Err(ExtensionError::CapExceeded);
        }
        if priority > PRIORITY_MAX {
            return Err(ExtensionError::SchemaMismatch);
        }
        let declared = self.declared(&point)?;
        if !self.owns(&point) && !self.holds("extensions.contribute", &point) {
            return Err(ExtensionError::NotGranted);
        }
        if !declared.dynamic {
            return Err(ExtensionError::NotDynamic);
        }
        if extension.is_empty() || serde_json::from_str::<Value>(&entry_json).is_err() {
            return Err(ExtensionError::SchemaMismatch);
        }
        let contributor = PluginId(self.plugin.clone());
        let entry = ExtensionEntry {
            extension: extension.clone(),
            contributor: contributor.clone(),
            priority,
            entry_json,
        };
        let mut state = self.book.state();
        let entries = state.entries.entry(point).or_default();
        match entries
            .iter_mut()
            .find(|held| held.extension == extension && held.contributor == contributor)
        {
            Some(held) => *held = entry,
            None => {
                if entries.len() >= declared.max_entries {
                    return Err(ExtensionError::CapExceeded);
                }
                entries.push(entry);
            }
        }
        Ok(())
    }

    fn unregister(&mut self, point: String, extension: String) -> Result<(), ExtensionError> {
        let declared = self.declared(&point)?;
        if !declared.dynamic {
            return Err(ExtensionError::NotDynamic);
        }
        let mut state = self.book.state();
        let entries = state
            .entries
            .get_mut(&point)
            .ok_or(ExtensionError::NotFound)?;
        let position = entries
            .iter()
            .position(|held| held.extension == extension && held.contributor.0 == self.plugin)
            .ok_or(ExtensionError::NotFound)?;
        entries.remove(position);
        Ok(())
    }
}

/// True when `value` satisfies `schema` (the data schema subset). A schema whose root is not
/// an object, or a value the validator cannot compare with an object schema, is checked for
/// its JSON type only: the subset validator takes object roots.
pub(crate) fn conforms(schema: &Value, value: &Value) -> bool {
    let is_object_schema = schema.get("type").and_then(Value::as_str) == Some("object");
    if !is_object_schema {
        return true;
    }
    if !value.is_object() {
        return false;
    }
    crate::settings::validate_settings(&schema.to_string(), &value.to_string()).is_ok()
}

/// How a hook point folds the replies of its handlers (api-extension-points.md, hook modes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookMode {
    /// Every handler runs; replies are ignored.
    Observe,
    /// Each handler gets the output of the previous one.
    Transform,
    /// The first `refuse` ends the chain.
    Veto,
    /// The first `answer` ends the chain.
    First,
}

/// What the owner's manifest says about a hook point.
#[derive(Debug, Clone)]
pub(crate) struct HookPoint {
    pub mode: HookMode,
    pub payload_schema: Option<Value>,
    pub reply_schema: Option<Value>,
    /// The deadline of one handler call in milliseconds, clamped to 5..=50.
    pub deadline_ms: u64,
    pub max_handlers: usize,
    /// Whether a handler that fails refuses the action (`on_failure`, SEC-287).
    pub refuse_on_failure: bool,
    /// Whether the guards run once after a transform chain.
    pub final_check: bool,
}

/// The hook declaration of a global point name, `None` when no enabled owner declares it.
pub(crate) fn hook_point(registry: &ContributionRegistry, point: &str) -> Option<HookPoint> {
    let entry = registry.find_global("extension_points", point)?;
    let payload = &entry.payload;
    let mode = match payload.get("mode").and_then(Value::as_str) {
        Some("transform") => HookMode::Transform,
        Some("veto") => HookMode::Veto,
        Some("first") => HookMode::First,
        _ => HookMode::Observe,
    };
    let high_risk = payload.get("risk").and_then(Value::as_str) == Some("high");
    // `on_failure` matters on a high-risk veto or transform point only, and defaults to
    // `refuse` there (SEC-287); everywhere else a failing handler counts as `pass`.
    let refuse_on_failure = high_risk
        && matches!(mode, HookMode::Veto | HookMode::Transform)
        && payload.get("on_failure").and_then(Value::as_str) != Some("pass");
    let (shortest, longest) = HANDLER_DEADLINE_RANGE_MS;
    Some(HookPoint {
        mode,
        payload_schema: payload.get("payload_schema").cloned(),
        reply_schema: payload.get("reply_schema").cloned(),
        deadline_ms: payload
            .get("deadline_ms")
            .and_then(Value::as_u64)
            .unwrap_or(HANDLER_DEADLINE_DEFAULT_MS)
            .clamp(shortest, longest),
        max_handlers: payload
            .get("max_handlers")
            .and_then(Value::as_u64)
            .and_then(|max| usize::try_from(max).ok())
            .unwrap_or(HANDLERS_DEFAULT_MAX),
        refuse_on_failure,
        final_check: payload
            .get("final_check")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// One handler a contributor offers a hook point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HookHandler {
    /// The contributing plugin.
    pub plugin: String,
    /// The contribution id (`ExtensionEntry.extension`), the identity of the entry.
    pub extension: String,
    /// The `handler` name `HookCall.extension` carries.
    pub handler: String,
    pub priority: u8,
    /// A guard runs once after a transform chain with `final_check`.
    pub guard: bool,
}

/// The handlers of a point from enabled manifests and from run-time registrations, in run
/// order (priority, contributor, id). Entries without a `handler` are data, not handlers.
pub(crate) fn hook_handlers(
    registry: &ContributionRegistry,
    book: &ExtensionBook,
    point: &str,
) -> Vec<HookHandler> {
    let mut handlers: Vec<HookHandler> = registry
        .entries("extensions")
        .into_iter()
        .filter(|entry| {
            entry
                .payload
                .get("point")
                .and_then(Value::as_str)
                .is_some_and(|named| named.split('@').next() == Some(point))
        })
        .filter_map(|entry| {
            let handler = entry.payload.get("handler")?.as_str()?.to_owned();
            Some(HookHandler {
                plugin: entry.plugin.clone(),
                extension: entry.name.clone(),
                handler,
                priority: entry
                    .payload
                    .get("priority")
                    .and_then(Value::as_u64)
                    .and_then(|priority| u8::try_from(priority).ok())
                    .unwrap_or(PRIORITY_DEFAULT),
                guard: entry
                    .payload
                    .get("guard")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        })
        .collect();
    for entry in book.entries_of(point) {
        let Ok(body) = serde_json::from_str::<Value>(&entry.entry_json) else {
            continue;
        };
        let Some(handler) = body.get("handler").and_then(Value::as_str) else {
            continue;
        };
        handlers.push(HookHandler {
            plugin: entry.contributor.0.clone(),
            extension: entry.extension.clone(),
            handler: handler.to_owned(),
            priority: entry.priority,
            guard: body.get("guard").and_then(Value::as_bool).unwrap_or(false),
        });
    }
    handlers.sort_by(|left, right| {
        (left.priority, &left.plugin, &left.extension).cmp(&(
            right.priority,
            &right.plugin,
            &right.extension,
        ))
    });
    handlers
}

#[cfg(test)]
mod tests {
    use super::{ExtensionBook, PluginExtensions};
    use crate::contributions::ContributionRegistry;
    use ilium_plugin_api::generated::host::extensions::{ExtensionError, HostExtensions};
    use std::sync::{Arc, Mutex};

    const OWNER: &str = "ilium.core.search";
    const POINT: &str = "ilium.core.search/sources";

    fn registry() -> Arc<Mutex<ContributionRegistry>> {
        let mut registry = ContributionRegistry::new();
        let manifest = serde_json::json!({
            "id": OWNER,
            "contributes": {"extension_points": [
                {"id": "sources", "kind": "collect", "dynamic": true, "max_entries": 1},
                {"id": "fixed", "kind": "collect", "dynamic": false}
            ]}
        });
        registry
            .load_manifest(OWNER, &manifest)
            .expect("the manifest loads");
        Arc::new(Mutex::new(registry))
    }

    fn import(plugin: &str, grants: &[&str], book: &ExtensionBook) -> PluginExtensions {
        let granted: Vec<String> = grants.iter().map(|grant| (*grant).to_owned()).collect();
        PluginExtensions::new(plugin, &granted, registry(), book.clone())
    }

    #[test]
    fn a_contributor_registers_and_the_owner_lists_the_entry() {
        let book = ExtensionBook::default();
        let mut contributor = import(
            "com.example.notes",
            &["extensions.contribute:ilium.core.search/sources"],
            &book,
        );
        contributor
            .register(POINT.to_owned(), "notes".to_owned(), 50, "{}".to_owned())
            .expect("registered");
        let mut owner = import(OWNER, &[], &book);
        let entries = owner.list(POINT.to_owned()).expect("listed");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].contributor.0, "com.example.notes");

        book.forget_plugin("com.example.notes");
        assert!(owner.list(POINT.to_owned()).expect("listed").is_empty());
    }

    #[test]
    fn grants_caps_and_static_points_are_enforced() {
        let book = ExtensionBook::default();
        let mut stranger = import("com.example.other", &[], &book);
        assert_eq!(
            stranger.register(POINT.to_owned(), "x".to_owned(), 1, "{}".to_owned()),
            Err(ExtensionError::NotGranted)
        );
        assert_eq!(
            stranger.list("ilium.core.search/none".to_owned()),
            Err(ExtensionError::UnknownPoint)
        );
        let mut owner = import(OWNER, &[], &book);
        assert_eq!(
            owner.register(
                "ilium.core.search/fixed".to_owned(),
                "x".to_owned(),
                1,
                "{}".to_owned()
            ),
            Err(ExtensionError::NotDynamic)
        );
        owner
            .register(POINT.to_owned(), "one".to_owned(), 1, "{}".to_owned())
            .expect("the first entry fits");
        assert_eq!(
            owner.register(POINT.to_owned(), "two".to_owned(), 1, "{}".to_owned()),
            Err(ExtensionError::CapExceeded)
        );
    }
}
