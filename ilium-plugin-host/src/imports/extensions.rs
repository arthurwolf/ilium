//! Host implementation of `ilium:host/extensions`.
//!
//! The import keeps the points the host has declared and the entries contributed to them. The
//! state belongs to the import value (one per host instance, shared by its clones). The host
//! declares points with [`ExtensionsImport::declare_point`] when it installs an owner, and names
//! the calling plugin with [`ExtensionsImport::bind_contributor`] before it runs the plugin's
//! code.
//!
//! Handlers live in the plugin instances, so `run-hook` here validates the call, rejects re-entry
//! and completes the chain with the host-side result only: no registered handler answered, so
//! `take-reply` yields nothing. The engine that owns the instances replaces that completion with
//! the real chain result.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use ilium_plugin_api::generated::host::extensions::{
    ExtensionEntry, ExtensionError, HostExtensions,
};
use ilium_plugin_api::generated::types::ids::{PluginId, RequestId};

use super::env::RequestIds;

/// Largest payload, reply or chain result in bytes (EXT-023).
const PAYLOAD_CAP_BYTES: usize = 64 * 1024;
/// Largest contributor entry in bytes (EXT-023).
const ENTRY_CAP_BYTES: usize = 16 * 1024;
/// Highest priority a manifest may declare (EXT-007).
const MAX_MANIFEST_PRIORITY: u8 = 100;
/// Longest point id after the owner prefix.
const POINT_ID_MAX_CHARS: usize = 64;
/// Longest owner plugin id.
const OWNER_ID_MAX_CHARS: usize = 128;
/// Contributor assumed when the host has not bound one.
const UNBOUND_CONTRIBUTOR: &str = "ilium.host.unbound";

/// One declared extension point.
#[derive(Debug, Default)]
struct PointState {
    dynamic: bool,
    entries: Vec<ExtensionEntry>,
}

#[derive(Debug, Default)]
struct ExtensionsState {
    points: HashMap<String, PointState>,
    contributor: Option<PluginId>,
    replies: HashMap<u64, Option<String>>,
    /// Points on the current chain stack, outermost first (EXT re-entry rule).
    active_chain: Vec<String>,
}

/// True when `point` looks like `<owner plugin id>/<point id>` with the documented character sets.
fn is_well_formed_point(point: &str) -> bool {
    let Some((owner, point_id)) = point.split_once('/') else {
        return false;
    };
    let owner_ok = !owner.is_empty()
        && owner.chars().count() <= OWNER_ID_MAX_CHARS
        && owner
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-');
    let point_ok = !point_id.is_empty()
        && point_id.chars().count() <= POINT_ID_MAX_CHARS
        && point_id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    owner_ok && point_ok
}

fn contributor_of(state: &ExtensionsState) -> PluginId {
    state
        .contributor
        .clone()
        .unwrap_or_else(|| PluginId(UNBOUND_CONTRIBUTOR.to_owned()))
}

/// Host side of the `extensions` interface of one host instance. Cloning shares the points,
/// entries and replies (a test harness keeps one instance and hands out clones).
#[derive(Debug, Clone, Default)]
pub struct ExtensionsImport {
    state: Arc<Mutex<ExtensionsState>>,
    ids: RequestIds,
}

impl ExtensionsImport {
    /// An import drawing request ids from `ids`, the allocator of the runtime.
    pub fn new(ids: RequestIds) -> Self {
        Self {
            state: Arc::default(),
            ids,
        }
    }

    fn with_state<R>(&self, action: impl FnOnce(&mut ExtensionsState) -> R) -> R {
        // Every writer does one insert or remove, so the state is consistent after a panic.
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        action(&mut state)
    }

    /// Declares an extension point. `dynamic` points accept `register`.
    ///
    /// Returns [`ExtensionError::UnknownPoint`] when the name is not `<owner>/<point-id>`.
    pub fn declare_point(&self, point: &str, dynamic: bool) -> Result<(), ExtensionError> {
        if !is_well_formed_point(point) {
            return Err(ExtensionError::UnknownPoint);
        }
        self.with_state(|state| {
            state
                .points
                .entry(point.to_owned())
                .or_default()
                .dynamic = dynamic;
        });
        Ok(())
    }

    /// Names the plugin whose code is calling the import, so entries carry the right contributor.
    pub fn bind_contributor(&self, contributor: PluginId) {
        self.with_state(|state| state.contributor = Some(contributor));
    }
}

impl HostExtensions for ExtensionsImport {
    fn list(&mut self, point: String) -> Result<Vec<ExtensionEntry>, ExtensionError> {
        self.with_state(|state| {
            let declared = state.points.get(&point).ok_or(ExtensionError::UnknownPoint)?;
            let mut entries = declared.entries.clone();
            // Run order without user order or tier data: priority, plugin id, extension id.
            entries.sort_by(|left, right| {
                (left.priority, &left.contributor.0, &left.extension).cmp(&(
                    right.priority,
                    &right.contributor.0,
                    &right.extension,
                ))
            });
            Ok(entries)
        })
    }

    fn run_hook(
        &mut self,
        point: String,
        payload_json: String,
    ) -> Result<RequestId, ExtensionError> {
        if payload_json.len() > PAYLOAD_CAP_BYTES {
            return Err(ExtensionError::CapExceeded);
        }
        self.with_state(|state| {
            if !state.points.contains_key(&point) {
                return Err(ExtensionError::UnknownPoint);
            }
            if state.active_chain.contains(&point) {
                return Err(ExtensionError::Reentrant);
            }
            if serde_json::from_str::<serde_json::Value>(&payload_json).is_err() {
                return Err(ExtensionError::SchemaMismatch);
            }
            let request = self.ids.next_request();
            state.replies.insert(request.0, None);
            Ok(request)
        })
    }

    fn take_reply(&mut self, request: RequestId) -> Option<String> {
        self.with_state(|state| state.replies.remove(&request.0)).flatten()
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
        if priority > MAX_MANIFEST_PRIORITY {
            return Err(ExtensionError::SchemaMismatch);
        }
        self.with_state(|state| {
            let contributor = contributor_of(state);
            let declared = state
                .points
                .get_mut(&point)
                .ok_or(ExtensionError::UnknownPoint)?;
            if !declared.dynamic {
                return Err(ExtensionError::NotDynamic);
            }
            if extension.is_empty() || serde_json::from_str::<serde_json::Value>(&entry_json).is_err()
            {
                return Err(ExtensionError::SchemaMismatch);
            }
            let entry = ExtensionEntry {
                extension: extension.clone(),
                contributor: contributor.clone(),
                priority,
                entry_json,
            };
            match declared
                .entries
                .iter_mut()
                .find(|held| held.extension == extension && held.contributor == contributor)
            {
                Some(held) => *held = entry,
                None => declared.entries.push(entry),
            }
            Ok(())
        })
    }

    fn unregister(&mut self, point: String, extension: String) -> Result<(), ExtensionError> {
        self.with_state(|state| {
            let contributor = contributor_of(state);
            let declared = state
                .points
                .get_mut(&point)
                .ok_or(ExtensionError::UnknownPoint)?;
            if !declared.dynamic {
                return Err(ExtensionError::NotDynamic);
            }
            let position = declared
                .entries
                .iter()
                .position(|held| held.extension == extension && held.contributor == contributor)
                .ok_or(ExtensionError::NotFound)?;
            declared.entries.remove(position);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ExtensionsImport;
    use ilium_plugin_api::generated::host::extensions::{ExtensionError, HostExtensions};
    use ilium_plugin_api::generated::types::ids::RequestId;

    /// Largest payload, reply or chain result in bytes (EXT-023, "Payloads").
    const PAYLOAD_CAP_BYTES: usize = 64 * 1024;

    /// Largest contributor entry in bytes (EXT-023).
    const ENTRY_CAP_BYTES: usize = 16 * 1024;

    /// Highest priority a manifest may declare; `register` takes a `u8` but applies the same range.
    const MAX_MANIFEST_PRIORITY: u8 = 100;

    /// A global point name that no installed owner declares in a fresh import.
    const UNDECLARED_POINT: &str = "ilium.core.tree/before-close";

    /// A point name built from a 65-character id, one over the 64-character point id limit.
    fn overlong_point_name() -> String {
        format!("{}/before-close", "a".repeat(65))
    }

    /// REQ: EXT-024
    #[test]
    fn list_undeclared_point_fails_with_unknown_point() {
        let mut import = ExtensionsImport::default();
        assert_eq!(
            import.list(UNDECLARED_POINT.to_string()),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-024
    #[test]
    fn list_empty_point_name_fails_with_unknown_point() {
        let mut import = ExtensionsImport::default();
        assert_eq!(
            import.list(String::new()),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-024
    #[test]
    fn list_point_id_over_64_characters_fails_with_unknown_point() {
        let mut import = ExtensionsImport::default();
        assert_eq!(
            import.list(overlong_point_name()),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-024
    #[test]
    fn run_hook_undeclared_point_fails_with_unknown_point() {
        let mut import = ExtensionsImport::default();
        assert_eq!(
            import.run_hook(UNDECLARED_POINT.to_string(), "{}".to_string()),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-024
    #[test]
    fn run_hook_empty_payload_on_undeclared_point_fails_with_unknown_point() {
        let mut import = ExtensionsImport::default();
        assert_eq!(
            import.run_hook(UNDECLARED_POINT.to_string(), String::new()),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-004
    #[test]
    fn run_hook_payload_one_byte_over_64_kib_fails_with_cap_exceeded() {
        let mut import = ExtensionsImport::default();
        let oversized_payload = "a".repeat(PAYLOAD_CAP_BYTES + 1);
        assert_eq!(
            import.run_hook(UNDECLARED_POINT.to_string(), oversized_payload),
            Err(ExtensionError::CapExceeded)
        );
    }

    /// REQ: EXT-023
    #[test]
    fn run_hook_payload_exactly_64_kib_is_not_rejected_for_size() {
        let mut import = ExtensionsImport::default();
        let payload_at_cap = "a".repeat(PAYLOAD_CAP_BYTES);
        assert_eq!(
            import.run_hook(UNDECLARED_POINT.to_string(), payload_at_cap),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-024
    #[test]
    fn register_undeclared_point_fails_with_unknown_point() {
        let mut import = ExtensionsImport::default();
        assert_eq!(
            import.register(
                UNDECLARED_POINT.to_string(),
                "project-key".to_string(),
                50,
                "{}".to_string(),
            ),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-023
    #[test]
    fn register_entry_one_byte_over_16_kib_fails_with_cap_exceeded() {
        let mut import = ExtensionsImport::default();
        let oversized_entry = "a".repeat(ENTRY_CAP_BYTES + 1);
        assert_eq!(
            import.register(
                UNDECLARED_POINT.to_string(),
                "project-key".to_string(),
                50,
                oversized_entry,
            ),
            Err(ExtensionError::CapExceeded)
        );
    }

    /// REQ: EXT-007
    #[test]
    fn register_priority_above_manifest_range_fails_with_schema_mismatch() {
        let mut import = ExtensionsImport::default();
        assert_eq!(
            import.register(
                UNDECLARED_POINT.to_string(),
                "project-key".to_string(),
                MAX_MANIFEST_PRIORITY + 1,
                "{}".to_string(),
            ),
            Err(ExtensionError::SchemaMismatch)
        );
    }

    /// REQ: EXT-024
    #[test]
    fn unregister_undeclared_point_fails_with_unknown_point() {
        let mut import = ExtensionsImport::default();
        assert_eq!(
            import.unregister(UNDECLARED_POINT.to_string(), "project-key".to_string()),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-013
    #[test]
    fn take_reply_for_unknown_request_returns_none() {
        let mut import = ExtensionsImport::default();
        assert_eq!(import.take_reply(RequestId(0)), None);
    }

    /// REQ: EXT-013
    #[test]
    fn take_reply_for_maximum_request_id_returns_none() {
        let mut import = ExtensionsImport::default();
        assert_eq!(import.take_reply(RequestId(u64::MAX)), None);
    }

    #[test]
    fn declared_points_and_entries_belong_to_the_import() {
        let mut owner = ExtensionsImport::default();
        let mut stranger = ExtensionsImport::default();
        owner
            .declare_point("ilium.core.tree/before-close", true)
            .expect("declared");
        owner
            .register(
                "ilium.core.tree/before-close".to_string(),
                "project-key".to_string(),
                50,
                "{}".to_string(),
            )
            .expect("registered");
        assert_eq!(
            owner
                .list("ilium.core.tree/before-close".to_string())
                .expect("listed")
                .len(),
            1
        );
        assert_eq!(
            stranger.list("ilium.core.tree/before-close".to_string()),
            Err(ExtensionError::UnknownPoint),
            "another import never declared the point"
        );
    }

    #[test]
    fn a_hook_reply_is_asked_for_on_the_instance_that_ran_the_hook() {
        let mut owner = ExtensionsImport::default();
        owner
            .declare_point("ilium.core.tree/before-close", false)
            .expect("declared");
        let first = owner
            .run_hook("ilium.core.tree/before-close".to_string(), "{}".to_string())
            .expect("accepted");
        let second = owner
            .run_hook("ilium.core.tree/before-close".to_string(), "{}".to_string())
            .expect("accepted");
        assert_ne!(first, second, "request ids never repeat");
    }
}
