### Changed
- The compaction optimizer (transcript scan, statistics, replay and trigger recommendation) moved out of the kernel into the `ilium.core.compaction` plugin. It opens from the "Analyze compaction settings" command as a panel (S scans the saved Claude Code or Codex transcripts through read-only host file access); disabling the plugin removes it.

### Removed
- The built-in Settings > Optimization tab and its trace cache. The panel scans again on every request.

### Changed
- Remote compaction (summarize a Claude Code or Codex transcript with the Inference model and resume the same session) moved out of the kernel into the `ilium.core.compaction` plugin: the Compact command, the progress dialog, the automatic trigger, the transcript rewrite with its backup and the Settings > Remote compaction tab are now the plugin's. Disabling the plugin removes all of it, and the agent toolbar's Compact button then sends the agent's own `/compact`.
- The old `[remote_compaction]` table of `config.toml` is no longer read; the plugin keeps its own settings.

### Removed
- The `ilium-remote-compaction` and `ilium-compaction-analysis` crates and the compaction prompt templates of `ilium-prompts` (the plugin carries its own copies).

### Added
- The compaction optimizer panel applies the recommended auto-compact threshold (A) and reverts the last apply (R) from the `ilium.core.compaction` plugin: it shows the change first, writes only the one key of Claude Code's `settings.json` or Codex's `config.toml` (every other byte is kept) and refuses to overwrite or revert a file that changed in between. The host accepts such a settings-file change through `agent-config.propose` (replace with the revision read, or create when missing).

### Removed
- The kernel's apply and revert machinery for the auto-compact thresholds in `ilium-client/src/agent_config_writer.rs` (targets, plan, apply, revert, revert record and file lock); the model status-line bridges keep its text editor.
