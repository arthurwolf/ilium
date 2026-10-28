//! Host implementation of `ilium:host/settings`.
//!
//! This import has no loaded schema, so no row key is declared: every row call validates its
//! arguments first (`invalid-argument`) and then reports an undeclared row as `not-found`, and
//! a merge patch may only be empty. Other plugins are not installed here, so reading or
//! writing one is `not-found`.

use ilium_plugin_api::generated::host::settings::{
    ChoiceOption, HostSettings, ItemState, SettingsRef,
};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::PluginId;
use ilium_plugin_api::generated::types::text::RichText;
use ilium_plugin_api::generated::types::ui::{CatalogItem, MeterSegment, RowState};

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
        return Err(invalid(&format!("{what} is longer than {limit} characters")));
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

    fn set_choices(
        &mut self,
        key: String,
        options: Vec<ChoiceOption>,
    ) -> Result<(), PluginError> {
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
            check_chars(&message, ROW_STATE_MESSAGE_LIMIT_CHARS, "the row state message")?;
        }
        Err(undeclared_row(&key))
    }

    fn set_item_states(&mut self, key: String, items: Vec<ItemState>) -> Result<(), PluginError> {
        for item in items {
            if let Some(message) = item.state.message {
                check_chars(&message, ROW_STATE_MESSAGE_LIMIT_CHARS, "the item state message")?;
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
}
