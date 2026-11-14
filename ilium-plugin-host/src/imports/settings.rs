//! Host implementation of `ilium:host/settings`.
//!
//! Two implementations share the argument checks:
//!
//! - `SettingsImport` has no loaded schema, so no row key is declared: every row call validates
//!   its arguments first (`invalid-argument`) and then reports an undeclared row as
//!   `not-found`, and a merge patch may only be empty. Other plugins are not installed here, so
//!   reading or writing one is `not-found`. Tests that need no plugin use it.
//! - `StoredSettingsImport` is the one a running plugin gets: it holds the plugin's compiled
//!   `contributes.settings` schema and the `config.toml` store, returns defaults plus stored
//!   values, validates and stores merge patches, and tells the runtime which keys changed so
//!   the plugin receives one `settings-changed` event (manual: `api-settings.md`).

use crate::config_settings::{schema_defaults, PluginSettingsError, PluginSettingsStore};
use ilium_plugin_api::generated::host::settings::{
    ChoiceOption, HostSettings, ItemState, SettingsRef,
};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::events::SettingsOrigin;
use ilium_plugin_api::generated::types::ids::PluginId;
use ilium_plugin_api::generated::types::text::RichText;
use ilium_plugin_api::generated::types::ui::{CatalogItem, MeterSegment, RowState};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Largest merge patch, in bytes.
const PATCH_LIMIT_BYTES: usize = 256 * 1024;
/// Longest info text, in characters.
const INFO_TEXT_LIMIT_CHARS: usize = 120;
/// Longest row note, in characters.
const ROW_NOTE_LIMIT_CHARS: usize = 120;
/// Longest row state message, in characters.
const ROW_STATE_MESSAGE_LIMIT_CHARS: usize = 80;
/// Most options of a dynamic choice row.
const CHOICE_OPTIONS_MAX: usize = 64;
/// Most segments of a meter.
const METER_SEGMENTS_MAX: usize = 8;

/// Host side of the `settings` interface.
#[derive(Debug)]
pub struct SettingsImport;

fn error(code: ErrorCode, message: impl Into<String>) -> PluginError {
    PluginError {
        code,
        message: message.into(),
        retry_after_ms: None,
    }
}

fn invalid(message: &str) -> PluginError {
    error(ErrorCode::InvalidArgument, message)
}

/// The error for a row key that no schema declares.
fn undeclared_row(key: &str) -> PluginError {
    let shown: String = key.chars().take(64).collect();
    error(
        ErrorCode::NotFound,
        format!("the settings schema declares no row `{shown}`"),
    )
}

fn check_chars(text: &str, limit: usize, what: &str) -> Result<(), PluginError> {
    if text.chars().count() > limit {
        return Err(invalid(&format!(
            "{what} is longer than {limit} characters"
        )));
    }
    Ok(())
}

/// Parses a merge patch: one JSON object of at most 256 KiB.
fn parse_patch(patch: &str) -> Result<serde_json::Map<String, serde_json::Value>, PluginError> {
    if patch.len() > PATCH_LIMIT_BYTES {
        return Err(invalid("the patch is larger than 256 KiB"));
    }
    match serde_json::from_str::<serde_json::Value>(patch) {
        Ok(serde_json::Value::Object(map)) => Ok(map),
        Ok(_) => Err(invalid("the patch must be one JSON object")),
        Err(_) => Err(invalid("the patch is not valid JSON")),
    }
}

/// Applies a patch against the (empty) schema: only an empty patch has no undeclared key.
fn apply_patch(patch: &str) -> Result<(), PluginError> {
    let map = parse_patch(patch)?;
    if let Some(key) = map.keys().next() {
        let shown: String = key.chars().take(64).collect();
        return Err(invalid(&format!("the schema does not declare `{shown}`")));
    }
    Ok(())
}

impl HostSettings for SettingsImport {
    fn get_json(&mut self) -> String {
        "{}".to_owned()
    }

    fn set_json(&mut self, patch: String) -> Result<(), PluginError> {
        apply_patch(&patch)
    }

    fn set_info(&mut self, key: String, text: String) -> Result<(), PluginError> {
        check_chars(&text, INFO_TEXT_LIMIT_CHARS, "the info text")?;
        Err(undeclared_row(&key))
    }

    fn set_choices(&mut self, key: String, options: Vec<ChoiceOption>) -> Result<(), PluginError> {
        if options.is_empty() || options.len() > CHOICE_OPTIONS_MAX {
            return Err(invalid("a dynamic choice row takes 1 to 64 options"));
        }
        Err(undeclared_row(&key))
    }

    fn set_row_note(&mut self, key: String, note: Option<String>) -> Result<(), PluginError> {
        if let Some(note) = note {
            check_chars(&note, ROW_NOTE_LIMIT_CHARS, "the row note")?;
        }
        Err(undeclared_row(&key))
    }

    fn set_row_state(&mut self, key: String, state: RowState) -> Result<(), PluginError> {
        if let Some(message) = state.message {
            check_chars(
                &message,
                ROW_STATE_MESSAGE_LIMIT_CHARS,
                "the row state message",
            )?;
        }
        Err(undeclared_row(&key))
    }

    fn set_item_states(&mut self, key: String, items: Vec<ItemState>) -> Result<(), PluginError> {
        for item in items {
            if let Some(message) = item.state.message {
                check_chars(
                    &message,
                    ROW_STATE_MESSAGE_LIMIT_CHARS,
                    "the item state message",
                )?;
            }
        }
        Err(undeclared_row(&key))
    }

    fn set_choice_items(
        &mut self,
        key: String,
        items: Vec<CatalogItem>,
    ) -> Result<(), PluginError> {
        if items.is_empty() {
            return Err(invalid("a catalog row takes at least one item"));
        }
        Err(undeclared_row(&key))
    }

    fn set_info_rich(&mut self, key: String, _text: RichText) -> Result<(), PluginError> {
        Err(undeclared_row(&key))
    }

    fn set_meter(&mut self, id: String, segments: Vec<MeterSegment>) -> Result<(), PluginError> {
        if segments.len() > METER_SEGMENTS_MAX {
            return Err(invalid("a meter takes at most 8 segments"));
        }
        Err(undeclared_row(&id))
    }

    fn open(&mut self, _target: String) -> Result<(), PluginError> {
        Ok(())
    }

    fn list_settings(&mut self) -> Vec<SettingsRef> {
        Vec::new()
    }

    fn get_plugin_json(&mut self, plugin: PluginId) -> Result<String, PluginError> {
        Err(error(
            ErrorCode::NotFound,
            format!("plugin `{}` is not installed", plugin.0),
        ))
    }

    fn set_plugin_json(&mut self, plugin: PluginId, patch: String) -> Result<(), PluginError> {
        parse_patch(&patch)?;
        Err(error(
            ErrorCode::NotFound,
            format!("plugin `{}` is not installed", plugin.0),
        ))
    }
}

/// Row updates a plugin may make per second, all kinds together (manual: `api-settings.md`,
/// "Dynamic content").
const ROW_CALLS_PER_SECOND: usize = 10;
/// Longest message of a `plugin-error`, in characters.
const ERROR_MESSAGE_LIMIT_CHARS: usize = 256;

/// What a plugin told the kernel to show on one row at run time. The client reads it to draw
/// the row; the plugin's own `settings-changed` is not involved.
#[derive(Debug, Clone, Default)]
pub struct RowOverlay {
    /// The live text of an `info` row.
    pub info: Option<String>,
    /// The rich live text of an `info` row.
    pub info_rich: Option<RichText>,
    /// The dim note under the row.
    pub note: Option<String>,
    /// The state of the row (idle, loading, failed, ...).
    pub state: Option<RowState>,
    /// The options of a dynamic `choice` row.
    pub choices: Option<Vec<ChoiceOption>>,
    /// The state of each item of a catalog or list row.
    pub item_states: Vec<ItemState>,
    /// The items of a catalog `choice` row.
    pub catalog: Vec<CatalogItem>,
    /// The segments of a `meter` row.
    pub meter: Vec<MeterSegment>,
}

/// The overlays of one plugin's rows by key or id; shared with the client.
pub type SharedRowOverlays = Arc<Mutex<BTreeMap<String, RowOverlay>>>;

/// Keys of a plugin that changed, handed to the runtime to queue `settings-changed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsNotice {
    /// The plugin whose values changed.
    pub plugin: String,
    /// The keys whose value changed (never empty).
    pub keys: Vec<String>,
    /// Who changed them.
    pub origin: SettingsOrigin,
}

/// Receives one notice per accepted write that changed a value.
pub type SettingsChangeSink = Arc<dyn Fn(SettingsNotice) + Send + Sync>;

/// The other plugins a settings screen may read and write, by plugin id, with their schemas.
pub type PeerSchemas = BTreeMap<String, String>;

/// Host side of the `settings` interface for a plugin that has a schema and a store.
pub struct StoredSettingsImport {
    plugin: String,
    schema_json: String,
    declared: BTreeSet<String>,
    store: Arc<PluginSettingsStore>,
    on_change: SettingsChangeSink,
    overlays: SharedRowOverlays,
    peers: PeerSchemas,
    read_all: bool,
    write_all: bool,
    listing: Vec<SettingsRef>,
    recent_row_calls: VecDeque<Instant>,
}

impl StoredSettingsImport {
    /// The import of `plugin`, whose side checks writes against `schema_json` and whose rows
    /// (value keys, info ids and meter ids) are `declared`.
    pub fn new(
        plugin: impl Into<String>,
        schema_json: impl Into<String>,
        declared: BTreeSet<String>,
        store: Arc<PluginSettingsStore>,
        on_change: SettingsChangeSink,
    ) -> Self {
        Self {
            plugin: plugin.into(),
            schema_json: schema_json.into(),
            declared,
            store,
            on_change,
            overlays: SharedRowOverlays::default(),
            peers: PeerSchemas::new(),
            read_all: false,
            write_all: false,
            listing: Vec::new(),
            recent_row_calls: VecDeque::new(),
        }
    }

    /// Shares the row overlays with the client instead of keeping them private.
    #[must_use]
    pub fn with_overlays(mut self, overlays: SharedRowOverlays) -> Self {
        self.overlays = overlays;
        self
    }

    /// Lets a settings screen plugin read (`settings.read-all`) and write
    /// (`settings.write-all`) the values of the plugins in `peers`.
    #[must_use]
    pub fn with_peers(mut self, peers: PeerSchemas, read_all: bool, write_all: bool) -> Self {
        self.peers = peers;
        self.read_all = read_all;
        self.write_all = write_all;
        self
    }

    /// The tabs and sections `list-settings` returns.
    #[must_use]
    pub fn with_listing(mut self, listing: Vec<SettingsRef>) -> Self {
        self.listing = listing;
        self
    }

    fn values_of(&self, plugin: &str, schema_json: &str) -> Result<String, PluginError> {
        let mut values = schema_defaults(schema_json);
        values.extend(self.store.read(plugin).map_err(from_facade)?);
        Ok(serde_json::Value::Object(values).to_string())
    }

    fn write(
        &self,
        plugin: &str,
        schema_json: &str,
        patch: &str,
        origin: SettingsOrigin,
    ) -> Result<(), PluginError> {
        let patch = parse_patch(patch)?;
        if patch.is_empty() {
            return Ok(());
        }
        let changed = self
            .store
            .apply_patch(plugin, schema_json, &patch)
            .map_err(|failure| match failure {
                PluginSettingsError::Invalid(message) => {
                    invalid(&truncate(&message, ERROR_MESSAGE_LIMIT_CHARS))
                }
                PluginSettingsError::Store(failure) => from_facade(failure),
            })?;
        if !changed.is_empty() {
            (self.on_change)(SettingsNotice {
                plugin: plugin.to_owned(),
                keys: changed,
                origin,
            });
        }
        Ok(())
    }

    /// Admits one row update, at most ten per second.
    fn admit_row_call(&mut self) -> Result<(), PluginError> {
        let now = Instant::now();
        while self
            .recent_row_calls
            .front()
            .is_some_and(|call| now.duration_since(*call) >= Duration::from_secs(1))
        {
            self.recent_row_calls.pop_front();
        }
        if self.recent_row_calls.len() >= ROW_CALLS_PER_SECOND {
            return Err(PluginError {
                code: ErrorCode::RateLimited,
                message: "at most 10 row updates per second".to_owned(),
                retry_after_ms: Some(1000),
            });
        }
        self.recent_row_calls.push_back(now);
        Ok(())
    }

    /// Checks the row is declared, admits the call and changes its overlay.
    fn update_row(
        &mut self,
        key: &str,
        change: impl FnOnce(&mut RowOverlay),
    ) -> Result<(), PluginError> {
        if !self.declared.contains(key) {
            return Err(undeclared_row(key));
        }
        self.admit_row_call()?;
        let mut overlays = self.overlays.lock().unwrap_or_else(PoisonError::into_inner);
        change(overlays.entry(key.to_owned()).or_default());
        Ok(())
    }
}

fn truncate(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn from_facade(failure: ilium_plugin_api::kernel::FacadeError) -> PluginError {
    error(
        failure.code,
        truncate(&failure.message, ERROR_MESSAGE_LIMIT_CHARS),
    )
}

impl HostSettings for StoredSettingsImport {
    fn get_json(&mut self) -> String {
        self.values_of(&self.plugin, &self.schema_json)
            .unwrap_or_else(|_| {
                serde_json::Value::Object(schema_defaults(&self.schema_json)).to_string()
            })
    }

    fn set_json(&mut self, patch: String) -> Result<(), PluginError> {
        self.write(
            &self.plugin,
            &self.schema_json,
            &patch,
            SettingsOrigin::Plugin,
        )
    }

    fn set_info(&mut self, key: String, text: String) -> Result<(), PluginError> {
        check_chars(&text, INFO_TEXT_LIMIT_CHARS, "the info text")?;
        self.update_row(&key, |row| row.info = Some(text))
    }

    fn set_choices(&mut self, key: String, options: Vec<ChoiceOption>) -> Result<(), PluginError> {
        if options.is_empty() || options.len() > CHOICE_OPTIONS_MAX {
            return Err(invalid("a dynamic choice row takes 1 to 64 options"));
        }
        self.update_row(&key, |row| row.choices = Some(options))
    }

    fn set_row_note(&mut self, key: String, note: Option<String>) -> Result<(), PluginError> {
        if let Some(note) = &note {
            check_chars(note, ROW_NOTE_LIMIT_CHARS, "the row note")?;
        }
        self.update_row(&key, |row| row.note = note)
    }

    fn set_row_state(&mut self, key: String, state: RowState) -> Result<(), PluginError> {
        if let Some(message) = &state.message {
            check_chars(
                message,
                ROW_STATE_MESSAGE_LIMIT_CHARS,
                "the row state message",
            )?;
        }
        self.update_row(&key, |row| row.state = Some(state))
    }

    fn set_item_states(&mut self, key: String, items: Vec<ItemState>) -> Result<(), PluginError> {
        for item in &items {
            if let Some(message) = &item.state.message {
                check_chars(
                    message,
                    ROW_STATE_MESSAGE_LIMIT_CHARS,
                    "the item state message",
                )?;
            }
        }
        self.update_row(&key, |row| row.item_states = items)
    }

    fn set_choice_items(
        &mut self,
        key: String,
        items: Vec<CatalogItem>,
    ) -> Result<(), PluginError> {
        if items.is_empty() {
            return Err(invalid("a catalog row takes at least one item"));
        }
        self.update_row(&key, |row| row.catalog = items)
    }

    fn set_info_rich(&mut self, key: String, text: RichText) -> Result<(), PluginError> {
        self.update_row(&key, |row| row.info_rich = Some(text))
    }

    fn set_meter(&mut self, id: String, segments: Vec<MeterSegment>) -> Result<(), PluginError> {
        if segments.len() > METER_SEGMENTS_MAX {
            return Err(invalid("a meter takes at most 8 segments"));
        }
        self.update_row(&id, |row| row.meter = segments)
    }

    fn open(&mut self, _target: String) -> Result<(), PluginError> {
        Ok(())
    }

    fn list_settings(&mut self) -> Vec<SettingsRef> {
        self.listing.clone()
    }

    fn get_plugin_json(&mut self, plugin: PluginId) -> Result<String, PluginError> {
        if !self.read_all {
            return Err(error(
                ErrorCode::NotPermitted,
                "reading other plugins' values needs settings.read-all",
            ));
        }
        let Some(schema) = self.peers.get(&plugin.0) else {
            return Err(not_installed(&plugin));
        };
        self.values_of(&plugin.0, schema)
    }

    fn set_plugin_json(&mut self, plugin: PluginId, patch: String) -> Result<(), PluginError> {
        if !self.write_all {
            return Err(error(
                ErrorCode::NotPermitted,
                "writing other plugins' values needs settings.write-all",
            ));
        }
        let Some(schema) = self.peers.get(&plugin.0) else {
            parse_patch(&patch)?;
            return Err(not_installed(&plugin));
        };
        self.write(&plugin.0, schema, &patch, SettingsOrigin::OtherPlugin)
    }
}

fn not_installed(plugin: &PluginId) -> PluginError {
    error(
        ErrorCode::NotFound,
        format!("plugin `{}` is not installed", plugin.0),
    )
}

#[cfg(test)]
mod tests {
    use super::SettingsImport;
    use ilium_plugin_api::generated::host::settings::{ChoiceOption, HostSettings};
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::ids::PluginId;
    use ilium_plugin_api::generated::types::text::Tone;
    use ilium_plugin_api::generated::types::ui::{MeterSegment, RowState, RowStateKind};

    /// Size cap of one `set-json` or `set-plugin-json` patch, in bytes (manual: api-settings, "Errors": over 256 KiB).
    const PATCH_LIMIT_BYTES: usize = 256 * 1024;
    /// Text of a live `info` row, in characters (manual: api-settings, "Dynamic content": at most 120).
    const INFO_TEXT_LIMIT_CHARS: usize = 120;
    /// Dim note under a row, in characters (manual: api-settings, "Dynamic content": at most 120).
    const ROW_NOTE_LIMIT_CHARS: usize = 120;
    /// Message of a row state, in characters (manual: api-settings, "Dynamic content": at most 80).
    const ROW_STATE_MESSAGE_LIMIT_CHARS: usize = 80;
    /// Options of a dynamic `choice` row (manual: api-settings, "Dynamic content": 1 to 64).
    const CHOICE_OPTIONS_MAX: usize = 64;
    /// Segments of a `meter` row (manual: api-settings, "Dynamic content": up to 8).
    const METER_SEGMENTS_MAX: usize = 8;
    /// Plain-text message of a `PluginError` (generated types: at most 256 characters).
    const PLUGIN_ERROR_MESSAGE_LIMIT_CHARS: usize = 256;

    /// A plugin id that no test fixture installs, so lookups by it must report `not-found`.
    const ABSENT_PLUGIN_ID: &str = "ilium.settings-test-absent";
    /// A row key that no manifest declares, so calls naming it must report `not-found`.
    const UNDECLARED_ROW_KEY: &str = "settings_test_undeclared_row";

    fn assert_error_code<T: std::fmt::Debug>(result: Result<T, PluginError>, expected: ErrorCode) {
        match result {
            Err(error) => assert_eq!(error.code, expected, "error message: {}", error.message),
            Ok(value) => panic!("expected error {expected:?}, got Ok({value:?})"),
        }
    }

    fn text_of_chars(count: usize) -> String {
        "x".repeat(count)
    }

    fn choice_options(count: usize) -> Vec<ChoiceOption> {
        (0..count)
            .map(|index| ChoiceOption {
                value: format!("value-{index}"),
                title: format!("Title {index}"),
            })
            .collect()
    }

    fn meter_segments(count: usize) -> Vec<MeterSegment> {
        (0..count)
            .map(|index| MeterSegment {
                label: format!("segment-{index}"),
                value: index as f64,
                tone: Tone::Success,
            })
            .collect()
    }

    /// A JSON object whose serialised text is strictly larger than the patch cap.
    fn oversized_patch() -> String {
        let filler = "a".repeat(PATCH_LIMIT_BYTES);
        format!("{{\"filler\":\"{filler}\"}}")
    }

    /// Manual: api-settings, "Surface" (`get-json` is always granted, returns one JSON object).
    /// REQ: CAP-011
    #[test]
    fn get_json_returns_a_json_object_without_any_grant() {
        let mut settings = SettingsImport;
        let values = settings.get_json();
        let parsed: serde_json::Value =
            serde_json::from_str(&values).expect("get_json must return valid JSON text");
        assert!(
            parsed.is_object(),
            "get_json must return one JSON object, got: {values}"
        );
    }

    /// Manual: api-settings, "Writing values" (an empty merge patch changes nothing and succeeds).
    /// REQ: CAP-011
    #[test]
    fn set_json_empty_object_patch_is_accepted() {
        let mut settings = SettingsImport;
        let result = settings.set_json("{}".to_string());
        assert!(
            result.is_ok(),
            "an empty merge patch must succeed, got {result:?}"
        );
    }

    #[test]
    fn set_json_rejects_each_non_object_patch_with_invalid_argument() {
        let non_object_patches = ["", "null", "42", "\"text\"", "true", "[]", "[{}]"];
        for patch in non_object_patches {
            let mut settings = SettingsImport;
            let result = settings.set_json(patch.to_string());
            assert_error_code(result, ErrorCode::InvalidArgument);
        }
    }

    #[test]
    fn set_json_rejects_malformed_json_with_invalid_argument() {
        let malformed_patches = ["{", "{\"a\":", "{\"a\" 1}"];
        for patch in malformed_patches {
            let mut settings = SettingsImport;
            let result = settings.set_json(patch.to_string());
            assert_error_code(result, ErrorCode::InvalidArgument);
        }
    }

    #[test]
    fn set_json_rejects_a_key_no_schema_declares_with_invalid_argument() {
        let mut settings = SettingsImport;
        let patch = format!("{{\"{UNDECLARED_ROW_KEY}\": true}}");
        let result = settings.set_json(patch);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// The one-byte-over boundary is the first size that must fail; the error message must fit PluginError's 256-character cap.
    #[test]
    fn set_json_rejects_a_patch_over_256_kib_with_a_short_message() {
        let patch = oversized_patch();
        assert!(
            patch.len() > PATCH_LIMIT_BYTES,
            "fixture must exceed the cap"
        );
        let mut settings = SettingsImport;
        let result = settings.set_json(patch);
        match result {
            Err(error) => {
                assert_eq!(error.code, ErrorCode::InvalidArgument);
                assert!(
                    error.message.chars().count() <= PLUGIN_ERROR_MESSAGE_LIMIT_CHARS,
                    "error message is {} characters",
                    error.message.chars().count()
                );
            }
            Ok(()) => panic!("a patch over 256 KiB must be refused"),
        }
    }

    #[test]
    fn get_plugin_json_for_an_uninstalled_plugin_returns_not_found() {
        let mut settings = SettingsImport;
        let result = settings.get_plugin_json(PluginId(ABSENT_PLUGIN_ID.to_string()));
        assert_error_code(result, ErrorCode::NotFound);
    }

    #[test]
    fn set_plugin_json_for_an_uninstalled_plugin_returns_not_found() {
        let mut settings = SettingsImport;
        let result =
            settings.set_plugin_json(PluginId(ABSENT_PLUGIN_ID.to_string()), "{}".to_string());
        assert_error_code(result, ErrorCode::NotFound);
    }

    #[test]
    fn set_row_state_for_an_undeclared_row_key_returns_not_found() {
        let mut settings = SettingsImport;
        let state = RowState {
            kind: RowStateKind::Idle,
            message: None,
        };
        let result = settings.set_row_state(UNDECLARED_ROW_KEY.to_string(), state);
        assert_error_code(result, ErrorCode::NotFound);
    }

    /// The test assumes the key check does not take precedence over the length check; see the unresolved note.
    #[test]
    fn set_info_text_one_character_over_120_is_invalid_argument() {
        let mut settings = SettingsImport;
        let result = settings.set_info(
            UNDECLARED_ROW_KEY.to_string(),
            text_of_chars(INFO_TEXT_LIMIT_CHARS + 1),
        );
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    #[test]
    fn set_choices_with_zero_options_is_invalid_argument() {
        let mut settings = SettingsImport;
        let result = settings.set_choices(UNDECLARED_ROW_KEY.to_string(), choice_options(0));
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    #[test]
    fn set_choices_with_one_more_than_64_options_is_invalid_argument() {
        let mut settings = SettingsImport;
        let result = settings.set_choices(
            UNDECLARED_ROW_KEY.to_string(),
            choice_options(CHOICE_OPTIONS_MAX + 1),
        );
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    #[test]
    fn set_row_note_one_character_over_120_is_invalid_argument() {
        let mut settings = SettingsImport;
        let result = settings.set_row_note(
            UNDECLARED_ROW_KEY.to_string(),
            Some(text_of_chars(ROW_NOTE_LIMIT_CHARS + 1)),
        );
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    #[test]
    fn set_row_state_message_one_character_over_80_is_invalid_argument() {
        let mut settings = SettingsImport;
        let state = RowState {
            kind: RowStateKind::Loading,
            message: Some(text_of_chars(ROW_STATE_MESSAGE_LIMIT_CHARS + 1)),
        };
        let result = settings.set_row_state(UNDECLARED_ROW_KEY.to_string(), state);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    #[test]
    fn set_meter_with_one_more_than_8_segments_is_invalid_argument() {
        let mut settings = SettingsImport;
        let result = settings.set_meter(
            UNDECLARED_ROW_KEY.to_string(),
            meter_segments(METER_SEGMENTS_MAX + 1),
        );
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    mod stored {
        use crate::config_settings::PluginSettingsStore;
        use crate::imports::settings::{
            PeerSchemas, SettingsChangeSink, SettingsNotice, StoredSettingsImport,
        };
        use ilium_plugin_api::generated::host::settings::HostSettings;
        use ilium_plugin_api::generated::types::errors::ErrorCode;
        use ilium_plugin_api::generated::types::events::SettingsOrigin;
        use ilium_plugin_api::generated::types::ids::PluginId;
        use std::sync::{Arc, Mutex};

        const SCHEMA: &str = r#"{"type":"object","properties":{
            "volume":{"type":"integer","minimum":0,"maximum":10,"default":5},
            "name":{"type":"string","default":"ilium"}}}"#;

        struct Rig {
            import: StoredSettingsImport,
            notices: Arc<Mutex<Vec<SettingsNotice>>>,
            store: Arc<PluginSettingsStore>,
            _directory: tempfile::TempDir,
        }

        fn rig() -> Rig {
            let directory = tempfile::tempdir().expect("temporary directory");
            let store = Arc::new(PluginSettingsStore::at(
                directory.path().join("config.toml"),
            ));
            let notices = Arc::new(Mutex::new(Vec::new()));
            let sink: SettingsChangeSink = {
                let notices = Arc::clone(&notices);
                Arc::new(move |notice| notices.lock().expect("notices").push(notice))
            };
            let declared = ["volume", "name", "status"]
                .into_iter()
                .map(str::to_owned)
                .collect();
            let import =
                StoredSettingsImport::new("acme.demo", SCHEMA, declared, Arc::clone(&store), sink);
            Rig {
                import,
                notices,
                store,
                _directory: directory,
            }
        }

        #[test]
        fn get_json_returns_the_defaults_until_a_value_is_stored() {
            let mut rig = rig();
            let values: serde_json::Value =
                serde_json::from_str(&rig.import.get_json()).expect("JSON");
            assert_eq!(values["volume"], 5);
            assert_eq!(values["name"], "ilium");
            rig.import
                .set_json(r#"{"volume":8}"#.to_owned())
                .expect("stored");
            let values: serde_json::Value =
                serde_json::from_str(&rig.import.get_json()).expect("JSON");
            assert_eq!(values["volume"], 8);
            assert_eq!(values["name"], "ilium");
        }

        #[test]
        fn a_change_is_stored_and_reported_once_with_the_changed_keys() {
            let mut rig = rig();
            rig.import
                .set_json(r#"{"volume":8,"name":"x"}"#.to_owned())
                .expect("stored");
            let notices = rig.notices.lock().expect("notices").clone();
            assert_eq!(notices.len(), 1);
            assert_eq!(notices[0].plugin, "acme.demo");
            assert_eq!(notices[0].origin, SettingsOrigin::Plugin);
            let mut keys = notices[0].keys.clone();
            keys.sort();
            assert_eq!(keys, ["name", "volume"]);
            assert_eq!(rig.store.read("acme.demo").expect("read")["volume"], 8);
        }

        #[test]
        fn a_patch_that_changes_nothing_succeeds_and_reports_nothing() {
            let mut rig = rig();
            rig.import.set_json("{}".to_owned()).expect("empty patch");
            rig.import
                .set_json(r#"{"volume":8}"#.to_owned())
                .expect("stored");
            rig.import
                .set_json(r#"{"volume":8}"#.to_owned())
                .expect("same value");
            assert_eq!(rig.notices.lock().expect("notices").len(), 1);
        }

        #[test]
        fn an_invalid_patch_stores_nothing_and_reports_nothing() {
            let mut rig = rig();
            for bad in [
                r#"{"volume":99}"#,
                r#"{"nope":1}"#,
                r#"{"volume":2,"name":3}"#,
            ] {
                let failure = rig.import.set_json(bad.to_owned()).expect_err(bad);
                assert_eq!(failure.code, ErrorCode::InvalidArgument, "{bad}");
                assert!(failure.message.chars().count() <= 256);
            }
            assert!(rig.store.read("acme.demo").expect("read").is_empty());
            assert!(rig.notices.lock().expect("notices").is_empty());
        }

        #[test]
        fn null_resets_a_key_to_its_default() {
            let mut rig = rig();
            rig.import
                .set_json(r#"{"volume":8}"#.to_owned())
                .expect("stored");
            rig.import
                .set_json(r#"{"volume":null}"#.to_owned())
                .expect("reset");
            let values: serde_json::Value =
                serde_json::from_str(&rig.import.get_json()).expect("JSON");
            assert_eq!(values["volume"], 5);
            assert_eq!(rig.notices.lock().expect("notices").len(), 2);
        }

        #[test]
        fn a_declared_row_takes_live_content_and_an_undeclared_one_is_not_found() {
            let mut rig = rig();
            rig.import
                .set_info("status".to_owned(), "syncing".to_owned())
                .expect("declared row");
            let failure = rig
                .import
                .set_info("ghost".to_owned(), "x".to_owned())
                .expect_err("undeclared row");
            assert_eq!(failure.code, ErrorCode::NotFound);
        }

        #[test]
        fn row_updates_are_limited_to_ten_per_second() {
            let mut rig = rig();
            for _ in 0..10 {
                rig.import
                    .set_row_note("volume".to_owned(), Some("n".to_owned()))
                    .expect("within the limit");
            }
            let failure = rig
                .import
                .set_row_note("volume".to_owned(), None)
                .expect_err("the eleventh call in a second");
            assert_eq!(failure.code, ErrorCode::RateLimited);
        }

        #[test]
        fn other_plugins_need_their_grants_and_a_known_schema() {
            let mut rig = rig();
            let other = PluginId("acme.other".to_owned());
            let denied = rig
                .import
                .get_plugin_json(other.clone())
                .expect_err("no read-all");
            assert_eq!(denied.code, ErrorCode::NotPermitted);
            let denied = rig
                .import
                .set_plugin_json(other.clone(), "{}".to_owned())
                .expect_err("no write-all");
            assert_eq!(denied.code, ErrorCode::NotPermitted);

            let mut peers = PeerSchemas::new();
            peers.insert("acme.other".to_owned(), SCHEMA.to_owned());
            let mut screen = rig_with_peers(peers, true, true);
            let values: serde_json::Value = serde_json::from_str(
                &screen
                    .import
                    .get_plugin_json(other.clone())
                    .expect("granted read"),
            )
            .expect("JSON");
            assert_eq!(values["volume"], 5);
            screen
                .import
                .set_plugin_json(other.clone(), r#"{"volume":2}"#.to_owned())
                .expect("granted write");
            let notices = screen.notices.lock().expect("notices").clone();
            assert_eq!(notices.len(), 1);
            assert_eq!(notices[0].plugin, "acme.other");
            assert_eq!(notices[0].origin, SettingsOrigin::OtherPlugin);
            let unknown = screen
                .import
                .get_plugin_json(PluginId("acme.absent".to_owned()))
                .expect_err("unknown plugin");
            assert_eq!(unknown.code, ErrorCode::NotFound);
        }

        fn rig_with_peers(peers: PeerSchemas, read_all: bool, write_all: bool) -> Rig {
            let rig = rig();
            Rig {
                import: rig.import.with_peers(peers, read_all, write_all),
                notices: rig.notices,
                store: rig.store,
                _directory: rig._directory,
            }
        }
    }
}
