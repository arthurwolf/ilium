//! Host implementation of `ilium:host/settings` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostSettings` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::settings::{
    ChoiceOption, HostSettings, ItemState, SettingsRef,
};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::PluginId;
use ilium_plugin_api::generated::types::text::RichText;
use ilium_plugin_api::generated::types::ui::{CatalogItem, MeterSegment, RowState};

/// Host side of the `settings` interface.
#[derive(Debug)]
pub struct SettingsImport;

impl HostSettings for SettingsImport {
    fn get_json(&mut self) -> String {
        unimplemented!("host::imports::settings::get_json")
    }

    fn set_json(&mut self, _patch: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::set_json")
    }

    fn set_info(&mut self, _key: String, _text: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::set_info")
    }

    fn set_choices(
        &mut self,
        _key: String,
        _options: Vec<ChoiceOption>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::set_choices")
    }

    fn set_row_note(&mut self, _key: String, _note: Option<String>) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::set_row_note")
    }

    fn set_row_state(&mut self, _key: String, _state: RowState) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::set_row_state")
    }

    fn set_item_states(&mut self, _key: String, _items: Vec<ItemState>) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::set_item_states")
    }

    fn set_choice_items(
        &mut self,
        _key: String,
        _items: Vec<CatalogItem>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::set_choice_items")
    }

    fn set_info_rich(&mut self, _key: String, _text: RichText) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::set_info_rich")
    }

    fn set_meter(&mut self, _id: String, _segments: Vec<MeterSegment>) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::set_meter")
    }

    fn open(&mut self, _target: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::open")
    }

    fn list_settings(&mut self) -> Vec<SettingsRef> {
        unimplemented!("host::imports::settings::list_settings")
    }

    fn get_plugin_json(&mut self, _plugin: PluginId) -> Result<String, PluginError> {
        unimplemented!("host::imports::settings::get_plugin_json")
    }

    fn set_plugin_json(&mut self, _plugin: PluginId, _patch: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::settings::set_plugin_json")
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
