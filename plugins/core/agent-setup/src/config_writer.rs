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
