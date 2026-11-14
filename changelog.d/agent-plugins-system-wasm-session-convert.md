### Changed
- "Convert to Codex" and "Convert to Claude Code" now live in the `session-convert` core plugin (`plugins/core/session-convert`): the context-menu entries, the progress dialog, the Codex-to-Claude translator and the Claude-to-Codex import via `codex app-server` are plugin code. Disabling the plugin removes the feature.

### Removed
- The built-in session converter: the `ilium-session-convert` crate, `ilium-client/src/session_conversion.rs`, the `ContextMenuAction::ConvertTo` menu entry and the `Mode::ConvertSession` dialog mode.
