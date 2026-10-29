# API gaps of the notifications port twins (svc01)

Twins are in `tests/port/svc_server.rs` and `svc_sounds.rs`. Ported by observable behaviour only (submissions, completion outcomes, settings, logs); the host-internal bank, admission and byte charge are not visible to a plugin.

Skipped (no plugin-API channel):
- `ilium-client/src/render_cache.rs::sound_preview_status_changes_only_after_server_completion`: the plugin API has no preview request or status line.
- `ilium-client/src/tree_ui.rs::stopped_agent_tree_row_keeps_recovery_indicator_in_normal_and_attention_modes`: the agent tree row and its recovery indicator are kernel rendering.
- `ilium-client/src/ui.rs::attention_mode_hides_tooltips_for_status_slots_without_a_glyph`: kernel tooltip rendering.

Assumptions: `FakeHost::pending_requests/complete` order requests by submission; a failed receipt writes one log line containing "sound" (port_041); an oversized pane name yields an attention reason of at most 80 characters (port_033, manual limit).
