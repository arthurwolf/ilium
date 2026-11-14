### Changed
- The attention queue (kinds, ordering, snooze and visit, the instant derivation from agent state, the Turbo limiter and the LLM analysis) moved out of the kernel into the `ilium.core.attention` plugin. Its panel and overview open from plugin commands, the server-side `analyse` job reads each changed agent's screen and asks the host inference call, and disabling the plugin removes the queue, its status segment and its shortcuts.

### Removed
- The built-in Settings > Attention tab, the `[attention]` config table, the `attention.*` control paths, the "Next: needs you" and overview buttons in the tree panel, and the `Ctrl+B i`, `Ctrl+B I` and `Ctrl+B O` built-in bindings.
- Shared mode (attention analysis piggybacking on the Sort call) and the attention model override: the plugin always analyses with its own inference job.
- The briefing box drawn on the pane; reaching an agent from the queue now shows the briefing as a toast. Change detection uses state-based fingerprints instead of screen hashes.
