//! Text-preserving edits of the agent CLIs' own config files (core-plugin-agent-setup.md,
//! "Compaction window" setup step; port of `ilium-client/src/agent_setup/config_text.rs`).
//!
//! Skeleton of the tests-first phase: every body is `unimplemented!`.

/// Which agent setting a config edit targets (core-plugin-agent-setup.md "Compaction window").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigTarget {
    /// `autoCompactWindow` in Claude Code's `settings.json`.
    ClaudeAutoCompactWindow,
    /// `auto_compact_token_limit` in Codex's `config.toml`.
    CodexAutoCompactTokenLimit,
}

/// What to do with the targeted key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyChange {
    /// Set the key to this value.
    Set(u64),
    /// Remove the key.
    Remove,
}

/// Applies `change` to `target` inside `text`, keeping every other byte of the file.
pub fn edit_config_text(
    _target: ConfigTarget,
    _text: &str,
    _change: KeyChange,
) -> Result<String, String> {
    unimplemented!("ilium.core.agent-setup: config_writer::edit_config_text")
}

/// Applies `change` to the top-level `key` of a JSON document, keeping formatting and comments.
pub fn edit_json_text(_text: &str, _key: &str, _change: KeyChange) -> Result<String, String> {
    unimplemented!("ilium.core.agent-setup: config_writer::edit_json_text")
}

/// Reads the current value of `target` from `text`; `Ok(None)` when the key is absent.
pub fn parse_setting(_target: ConfigTarget, _text: &str) -> Result<Option<u64>, String> {
    unimplemented!("ilium.core.agent-setup: config_writer::parse_setting")
}

/// Reads the top-level integer `key` of a TOML document; `Ok(None)` when absent.
pub fn parse_toml_setting(_text: &str, _key: &str) -> Result<Option<u64>, String> {
    unimplemented!("ilium.core.agent-setup: config_writer::parse_toml_setting")
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// safety rules: "text outside the markers is kept". Unrelated keys keep their exact bytes.
    #[test]
    fn json_set_keeps_unrelated_keys_verbatim() {
        let text = "{\n  \"model\": \"opus\",\n  \"theme\":   \"dark\"\n}\n";
        let edited = edit_json_text(text, "autoCompactWindow", KeyChange::Set(250_000)).unwrap();
        assert!(edited.contains("\"model\": \"opus\""));
        assert!(edited.contains("\"theme\":   \"dark\""));
        assert_eq!(
            parse_setting(ConfigTarget::ClaudeAutoCompactWindow, &edited).unwrap(),
            Some(250_000)
        );
    }

    /// the top-level key. An existing key is replaced in place, not duplicated.
    #[test]
    fn json_set_replaces_existing_value_without_duplicating_key() {
        let text = "{\n  \"autoCompactWindow\": 100\n}\n";
        let edited = edit_json_text(text, "autoCompactWindow", KeyChange::Set(200)).unwrap();
        assert_eq!(edited.matches("autoCompactWindow").count(), 1);
        assert_eq!(
            parse_setting(ConfigTarget::ClaudeAutoCompactWindow, &edited).unwrap(),
            Some(200)
        );
    }

    /// the top-level key, and the other keys stay.
    #[test]
    fn json_remove_drops_key_and_keeps_other_keys() {
        let text = "{\n  \"autoCompactWindow\": 100,\n  \"model\": \"opus\"\n}\n";
        let edited = edit_json_text(text, "autoCompactWindow", KeyChange::Remove).unwrap();
        assert_eq!(
            parse_setting(ConfigTarget::ClaudeAutoCompactWindow, &edited).unwrap(),
            None
        );
        assert!(edited.contains("\"model\": \"opus\""));
    }

    /// top-level key is targeted. A nested key of the same name keeps its value and text.
    #[test]
    fn json_set_does_not_touch_nested_key_of_same_name() {
        let text = "{\n  \"env\": {\"autoCompactWindow\": 1}\n}\n";
        let edited = edit_json_text(text, "autoCompactWindow", KeyChange::Set(5)).unwrap();
        assert!(edited.contains("\"env\": {\"autoCompactWindow\": 1}"));
        assert_eq!(
            parse_setting(ConfigTarget::ClaudeAutoCompactWindow, &edited).unwrap(),
            Some(5)
        );
    }

    /// the key is absent.
    #[test]
    fn parse_setting_absent_key_is_none() {
        let text = "{\n  \"model\": \"opus\"\n}\n";
        assert_eq!(
            parse_setting(ConfigTarget::ClaudeAutoCompactWindow, text).unwrap(),
            None
        );
    }

    /// settings object (no keys yet) accepts a `Set` and reads it back.
    #[test]
    fn config_text_set_on_empty_object_adds_claude_key() {
        let edited = edit_config_text(
            ConfigTarget::ClaudeAutoCompactWindow,
            "{}",
            KeyChange::Set(1),
        )
        .unwrap();
        assert_eq!(
            parse_setting(ConfigTarget::ClaudeAutoCompactWindow, &edited).unwrap(),
            Some(1)
        );
    }

    /// top-level integer key of a TOML document.
    #[test]
    fn parse_toml_reads_top_level_integer() {
        let text = "model = \"x\"\nauto_compact_token_limit = 180000\n\n[profiles.fast]\nauto_compact_token_limit = 1\n";
        assert_eq!(
            parse_toml_setting(text, "auto_compact_token_limit").unwrap(),
            Some(180_000)
        );
    }

    /// top-level table counts. A same-named key inside `[profiles.fast]` is not the setting.
    #[test]
    fn parse_toml_ignores_table_scoped_key() {
        let text = "[profiles.fast]\nauto_compact_token_limit = 1\n";
        assert_eq!(
            parse_toml_setting(text, "auto_compact_token_limit").unwrap(),
            None
        );
    }

    /// `u64`, so the parse returns `Err` instead of a made-up number.
    #[test]
    fn parse_toml_non_integer_value_is_error() {
        let text = "auto_compact_token_limit = \"big\"\n";
        assert!(parse_toml_setting(text, "auto_compact_token_limit").is_err());
    }

    /// absent key.
    #[test]
    fn parse_setting_malformed_json_is_error() {
        assert!(parse_setting(ConfigTarget::ClaudeAutoCompactWindow, "{ not json").is_err());
    }
}
