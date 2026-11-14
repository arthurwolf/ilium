### Changed
- The compaction optimizer (transcript scan, statistics, replay and trigger recommendation) moved out of the kernel into the `ilium.core.compaction` plugin. It opens from the "Analyze compaction settings" command as a panel (S scans the saved Claude Code or Codex transcripts through read-only host file access); disabling the plugin removes it.

### Removed
- The built-in Settings > Optimization tab and its trace cache. The panel scans again on every request, and applying or reverting the recommended threshold from the panel is not available yet.
