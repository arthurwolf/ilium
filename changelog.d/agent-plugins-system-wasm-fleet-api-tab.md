### Changed
- The API settings tab (the loopback HTTP API port) is now contributed by the Fleet plugin as a settings page bound to the same `[api]` table in `config.toml`, instead of being built into the client. Disabling the Fleet plugin removes the tab; the server still reads `[api] port`, so no stored setting moves.
- Ask for update again appends the "Ask for update instructions" text (Settings, LLM Instructions) to the fixed status request, inside a `<custom-instructions purpose="ask-for-update">` block, when the Fleet plugin sends it. Text typed into the Send message dialog is never changed.

### Removed
- The dead `Tree::is_agent_pane_idle_for_update` and `Tree::panes_eligible_for_update` helpers in `ilium-core`; the Fleet plugin owns the idle-agent rule.
