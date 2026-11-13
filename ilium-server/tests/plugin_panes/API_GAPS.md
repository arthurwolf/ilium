# API gaps: ilium-server/tests/plugin_panes

Format: crate, signature, requirement id, why.

- ilium-server: `ServerKernel::for_test_world(plugin: &str, grants: &[&str]) -> ServerKernel` (unit struct today) - all PAN ids - a kernel with panes 10 running, 11 restored (title "restored title", 100x30), 12 exited, 13 neighbour (min 10x3).
- ilium-server: `ServerKernel` observers `lifecycle_events`, `lifecycle_event_causes`, `event_names_for`, `plugin_log`, `pane_info`, `pane_identity`, `pane_cwd`, `tree_fingerprint`, `node_revision`, `node_exists`, `pane_shows_exit_status`, `kill_was_sent_after_ms`, `last_restructure_token`, `last_spawned_program`, `pane_count` - PAN-001..032 - observable channels.
- ilium-server: `ServerKernel` drivers `pretend_platform_windows`, `make_process_ignore_terminate`, `set_manual_icon`, `restore_pane_without_saved_size`, `start_restored_process`, `create_pane_for_test`, `set_environment` - PAN-003, 005, 010, 012, 025, 031.
- ilium-server: `ilium_server::plugin_host` must be public (module is private in lib.rs today) - all.
- ilium-server: detector loop hooks `run_detection_tick`, `run_detection_tick_with_fixture_detector`, `set_detector_block_ms(_for)`, `detector_thread_names`, `detection_loop_thread_name`, `detector_calls_in_last_tick`, `wait_for_detector_call` - HST-450, 451, 455.
- ilium-plugin-api: `grants::is_high_risk(&str) -> bool` - PAN-032 - high-risk lookup by grant name.
- ilium-plugin-api: `Intent::CloseNode(NodeId)` assumed - PAN-008 - close a pane.
