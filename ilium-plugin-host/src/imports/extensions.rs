//! Host implementation of `ilium:host/extensions`.
//!
//! The import keeps the points the host has declared and the entries contributed to them. The
//! import is a unit struct, so this state is per thread (one host instance per thread). The host
//! declares points with [`declare_point`] when it installs an owner, and names the calling plugin
//! with [`bind_contributor`] before it runs the plugin's code.
//!
//! Handlers live in the plugin instances, so `run-hook` here validates the call, rejects re-entry
//! and completes the chain with the host-side result only: no registered handler answered, so
//! `take-reply` yields nothing. The engine that owns the instances replaces that completion with
//! the real chain result.

use std::cell::RefCell;
use std::collections::HashMap;

use ilium_plugin_api::generated::host::extensions::{
    ExtensionEntry, ExtensionError, HostExtensions,
};
use ilium_plugin_api::generated::types::ids::{PluginId, RequestId};

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
    next_request: u64,
    replies: HashMap<u64, Option<String>>,
    /// Points on the current chain stack, outermost first (EXT re-entry rule).
    active_chain: Vec<String>,
}

thread_local! {
    static STATE: RefCell<ExtensionsState> = RefCell::new(ExtensionsState::default());
}

fn with_state<R>(action: impl FnOnce(&mut ExtensionsState) -> R) -> R {
    STATE.with(|cell| action(&mut cell.borrow_mut()))
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

/// Declares an extension point of the current host thread. `dynamic` points accept `register`.
///
/// Returns [`ExtensionError::UnknownPoint`] when the name is not `<owner>/<point-id>`.
pub fn declare_point(point: &str, dynamic: bool) -> Result<(), ExtensionError> {
    if !is_well_formed_point(point) {
        return Err(ExtensionError::UnknownPoint);
    }
    with_state(|state| {
        state
            .points
            .entry(point.to_owned())
            .or_default()
            .dynamic = dynamic;
    });
    Ok(())
}

/// Names the plugin whose code is calling the import, so entries carry the right contributor.
pub fn bind_contributor(contributor: PluginId) {
    with_state(|state| state.contributor = Some(contributor));
}

fn contributor_of(state: &ExtensionsState) -> PluginId {
    state
        .contributor
        .clone()
        .unwrap_or_else(|| PluginId(UNBOUND_CONTRIBUTOR.to_owned()))
}

/// Host side of the `extensions` interface.
#[derive(Debug)]
pub struct ExtensionsImport;

impl HostExtensions for ExtensionsImport {
    fn list(&mut self, point: String) -> Result<Vec<ExtensionEntry>, ExtensionError> {
        with_state(|state| {
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
        with_state(|state| {
            if !state.points.contains_key(&point) {
                return Err(ExtensionError::UnknownPoint);
            }
            if state.active_chain.contains(&point) {
                return Err(ExtensionError::Reentrant);
            }
            if serde_json::from_str::<serde_json::Value>(&payload_json).is_err() {
                return Err(ExtensionError::SchemaMismatch);
            }
            state.next_request += 1;
            let request = state.next_request;
            state.replies.insert(request, None);
            Ok(RequestId(request))
        })
    }

    fn take_reply(&mut self, request: RequestId) -> Option<String> {
        with_state(|state| state.replies.remove(&request.0)).flatten()
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
        with_state(|state| {
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
        with_state(|state| {
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
        let mut import = ExtensionsImport;
        assert_eq!(
            import.list(UNDECLARED_POINT.to_string()),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-024
    #[test]
    fn list_empty_point_name_fails_with_unknown_point() {
        let mut import = ExtensionsImport;
        assert_eq!(
            import.list(String::new()),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-024
    #[test]
    fn list_point_id_over_64_characters_fails_with_unknown_point() {
        let mut import = ExtensionsImport;
        assert_eq!(
            import.list(overlong_point_name()),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-024
    #[test]
    fn run_hook_undeclared_point_fails_with_unknown_point() {
        let mut import = ExtensionsImport;
        assert_eq!(
            import.run_hook(UNDECLARED_POINT.to_string(), "{}".to_string()),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-024
    #[test]
    fn run_hook_empty_payload_on_undeclared_point_fails_with_unknown_point() {
        let mut import = ExtensionsImport;
        assert_eq!(
            import.run_hook(UNDECLARED_POINT.to_string(), String::new()),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-004
    #[test]
    fn run_hook_payload_one_byte_over_64_kib_fails_with_cap_exceeded() {
        let mut import = ExtensionsImport;
        let oversized_payload = "a".repeat(PAYLOAD_CAP_BYTES + 1);
        assert_eq!(
            import.run_hook(UNDECLARED_POINT.to_string(), oversized_payload),
            Err(ExtensionError::CapExceeded)
        );
    }

    /// REQ: EXT-023
    #[test]
    fn run_hook_payload_exactly_64_kib_is_not_rejected_for_size() {
        let mut import = ExtensionsImport;
        let payload_at_cap = "a".repeat(PAYLOAD_CAP_BYTES);
        assert_eq!(
            import.run_hook(UNDECLARED_POINT.to_string(), payload_at_cap),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-024
    #[test]
    fn register_undeclared_point_fails_with_unknown_point() {
        let mut import = ExtensionsImport;
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
        let mut import = ExtensionsImport;
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
        let mut import = ExtensionsImport;
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
        let mut import = ExtensionsImport;
        assert_eq!(
            import.unregister(UNDECLARED_POINT.to_string(), "project-key".to_string()),
            Err(ExtensionError::UnknownPoint)
        );
    }

    /// REQ: EXT-013
    #[test]
    fn take_reply_for_unknown_request_returns_none() {
        let mut import = ExtensionsImport;
        assert_eq!(import.take_reply(RequestId(0)), None);
    }

    /// REQ: EXT-013
    #[test]
    fn take_reply_for_maximum_request_id_returns_none() {
        let mut import = ExtensionsImport;
        assert_eq!(import.take_reply(RequestId(u64::MAX)), None);
    }
}
