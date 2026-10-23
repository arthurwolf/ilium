# API gaps: host-production lane (ilium-plugin-host/tests/production)

The lane's tests call the items below through `unimplemented!`, so each stays red until the item exists. Tests that call an existing item are listed as "real" and do not need a gap. The lane needs no new dev-dependencies and no Cargo.toml change.

- `ilium_plugin_host::host_capacity::machine_disk_ceiling`: one machine-wide disk ceiling of 8,192 MiB over `data` plus `cache` of all plugins, refusing with `quota-exceeded` / `host-disk-ceiling` (HST-126, HST-127).
- `ilium_plugin_host::host_capacity::machine_cpu_bucket`: one machine-wide CPU token bucket of 4,000 ms held and refilled at 4,000 ms per second, applied in addition to the per-side group (HST-128).
- `ilium_plugin_host::host_capacity::machine_ceilings_with_low_disk_steps`: the machine ceilings never skip the low-disk steps of HST-661 or the reserve of EXT-064 (HST-954).
- Machine-bucket refusal for HST-953 (`quota-exceeded` with `retry-after-ms` from the API-072 formula, no trap, no failure count). Real but partial: `ilium_plugin_host::limits_rules::retry_after_ms(balance_ms: i64) -> i64` exists as the per-process HST-496 hint; the machine-bucket refusal does not exist and is not tested by a real call here.
- `ilium_plugin_host::telemetry::host_line` e-mail and absolute-path redaction to `[redacted-email]` and `[redacted-path]`, with the count in the support bundle summary (HST-129).
- `ilium_plugin_host::telemetry::SpanRecorder::record_custom_counter` personal-data guard (HST-130). Missing.
- `ilium_plugin_host::telemetry::truncate_message`: EXISTS but does not match HST-131. It cuts at `LOG_MESSAGE_MAX_BYTES` = 16,384 bytes and appends `…[truncated]`. HST-131 requires the first 4 KiB (4,096 bytes) and the marker `[truncated]`. Test `hst_131_a_log_line_over_4_kib_keeps_its_first_4_kib_and_ends_with_truncated` is red until the limit and marker change. Test `hst_131_a_log_line_of_exactly_4_kib_is_not_cut` passes today.
- `ilium_plugin_host::telemetry::trace_file::prune_at_server_start`: deletes `<state>/plugin-trace.jsonl` spans older than 7 days at server start (HST-979).
- `ilium_plugin_host::cli::erase_data_trace_rewrite`: `ilium plugin erase-data` rewrites the trace file by a new file renamed over the old one and keeps other plugins' spans (HST-980).
- `ilium_client::render::nearest_terminal_colour`: 16-colour fallback that starts the client and maps `indexed` and `rgb` colours without failing the plugin call (HST-390).
- `ilium_plugin_host::install::refuse_macos_below_14`: installer refusal of macOS releases before 14 with `install.unsupported-platform` (HST-956).
- `ilium_plugin_host::install::refusal_before_any_write`: refusal of unsupported platforms before any file is written, exit 1, no half-installed host (HST-958).
- `ilium_plugin_host::instance::stop_after_five_failures_in_24_hours`: `stopped-by-failure` at the fifth counted failure in a 24 hour sliding window, no restart scheduled (HST-950).
- Restore of failure times across server restarts: `installed.json` holds the wall-clock failure times, reloaded at start, dropping times older than 24 hours (HST-951). The test for it is in the reference page check; the behaviour item is `ilium_plugin_host::instance::failure_times_reload_at_server_start`, not yet called by a test.
- `ilium_plugin_host::trust::restored_store_without_mac`: a restored `trust.json` without a verifiable MAC is renamed `trust.json.tampered-<YYYYMMDD-HHmmss>` and T2/T3 plugins are refused with `trust.store-tampered` (HST-959).
- `ilium_plugin_host::cli::backup_space_precheck`: `ilium plugin backup` refuses with `backup.no-space` before any write when free space is below the `du` bytes plus 512 MiB (HST-132).
- `ilium_plugin_host::cli::backup_disk_full_cleanup`: disk-full during backup returns `io` / `no-space` and removes `<file>.partial` (HST-133). The doc test covers the no-change rule for an existing archive.
- `ilium_plugin_host::cli::backup_keeps_older_archive_without_force`: backup never deletes an archive, including one at the same path, without `--force` (HST-971).
- `ilium_plugin_host::cli::restore_refuses_live_root_with_server`: restore into a live root while a session server runs is refused with `restore.server-running` (HST-974). The doc test covers the drill rule.
- `ilium_plugin_host::cli::restore_timing_on_reference_machine`: a 1 GiB single-plugin restore completes within 15 minutes on the reference machine (HST-976). A release timing check; no in-tree item.
- `ilium_plugin_host::cli::uninstall_purge_keeps_exports`: `uninstall --purge` never deletes a whole-host archive or single-plugin export (HST-977).
- `ilium_plugin_host::cli::plugin_metrics_opens_no_socket`: `ilium plugin metrics` writes to stdout only and opens no listening socket (HST-991).
- `ilium_plugin_host::policy::forwarding_key_rejected`: an admin policy file with a log or metrics forwarding key is rejected (HST-993).
- The backup-archive, restore-precheck and disk-full behaviour (HST-132..135, HST-960..963) is documented in the manual; the rig `ilium_plugin_host::testing::ops::OpsRig` exists as a skeleton whose methods are all `unimplemented!`, so behaviour tests that need it are not written here.
