# API gaps: lifecycle-ops

The skeleton exposes `cli::run`, `Installer`, `LifecycleManager` and the registry types, but no
deterministic way to drive whole install, update, session and dev-reload flows. These tests use
one new test module, `ilium_plugin_host::testing::ops`, behind the `testing` feature. Nothing
else is invented. All time is virtual: `OpsRig::advance` moves the clock and runs due work.

## Build configuration

- ilium-plugin-host dev-dependencies: `ilium-plugin-host` itself with feature `testing` (self dev-dependency), `serde_json`, `proptest`. The test target needs `required-features = ["testing"]`.
- `ilium_plugin_host::testing::ops::compile_cache_key(component_hash, wasmtime_version, cpu_features) -> String`, pure; used by LIF-176.

## OpsRig (temp data, state, config and cache dirs; virtual clock; fake network; fake disk)

- `OpsRig::new()`, `cli(&[&str]) -> CliOutput` (stdin closed, no TTY), `cli_tty(&[&str], answers: &[&str]) -> CliOutput` (scripted prompt answers), `dev_cli(&[&str]) -> CliOutput` (the `plugin dev` entry, returns when it exits).
- `write_package(&PackageSpec) -> PathBuf`, `scratch_dir()`, `plugins_root()`, `state_dir()`, `cache_dir()`, `data_dir(id)`, `compile_cache_dir()`.
- `registry_bytes()`, `registry_json()`, `set_registry_bytes(&[u8])`, `grants_json()`.
- `inject(Fault)`, with `Fault::{RegistryWrite(RegistryStep), InstallFails(u8), KillInstaller(u8), KillUninstall(UninstallStep)}`; `trace() -> Vec<String>` of named steps (`registry.temp-written`, `lock.acquire`, `uninstall.state-pending`, `disk.check`, `staging.create ...`, `activate.begin <id>` ...).
- Lock: `hold_lock() -> LockHold`, `hold_lock_for(Duration)`, `LockHold::kill()`, `lock_polls()`, `virtual_elapsed()`, `advance(Duration)`.
- Servers: `start_server(session)` (applies pending work at once), `start_server_manual(session)` (call `apply_pending()`), `start_server_polling(session)` (no file watcher), `start_server_or_same(&ServerHandle)`, `server_running(session)`.
- Prompts: `prompt_count()`, `last_review() -> Vec<String>`.
- Network and disk: `block_network()`, `network_attempts()`, `set_free_bytes(u64)`, `required_free_bytes(&PackageSpec)`, `fill_disk()`, `free_disk()`, `make_data_read_only(id)`, `corrupt_file(&Path, &[u8])`, `storage_db_path(id)`, `delete_compile_cache()`, `make_compile_cache_unwritable()`.
- Planting: `plant_data(id, stamp) -> PathBuf`, `plant_settings`, `plant_backup`, `plant_cache`, `plant_shared_bucket`, `plant_first_use_answer`, `plant_durable_timer`, `plant_snapshot_metadata(session, node, id, key, bytes)`, `make_staging(name, pid, age)`, `make_store_orphan(id, version, age)`, `age_store_entries`, `truncate_first_store_file`, `delete_cached_package`, `delete_backups`.
- Reading: `bucket_stamp(id) -> Option<u32>`, `setting(id, key)`, `shared_bucket_exists`, `first_use_answers`, `durable_timers`, `snapshot_metadata_keys(session, id)`, `snapshot_bytes(session)`, `make_snapshot_dir_read_only/writable(session)`.
- Trust and policy: `trust_publisher(key_id)`, `revoke_key(key_id, reason)`, `registry_takedown(id, version)`, `write_policy(toml)`, `write_project_plugins_toml(text)`, `set_enabled_levels(id, session, Option<bool> x3)`.
- Matrix: `set_all_markers(id, &ServerHandle)`, `marker_present(id, Marker) -> bool`, `apply_event(&ServerHandle, id, LifeEvent)`.
- Core and bundled: `install_replacement_of_core(&PackageSpec, core_id)`, `install_bundled(&PackageSpec)`, `update_bundled(&PackageSpec)`.
- Dev: `dev_start(&PackageSpec, &[&str]) -> DevSession`.
- Installer wrapper: `cli("install ...")` is used for installs; there is no direct `Installer` call.

## PackageSpec (builder for a signed or unsigned `.ilpkg`)

- `new(id, version)`, `grant`, `high_risk_grant`, `first_use_prompt`, `signed(key)`, `resigned(note)`, `same_publisher_as(key)`, `bad_manifest()`, `bad_signature()`, `content_marker(&str)`, `padding(bytes)`, `client_side()`.
- Behaviour switches of the NativeGuest-backed fixture: `activate_error()`, `activate_traps()`, `activate_misses_deadline()`, `start_writes_file(&Path)`, `restore_bytes(len)`, `data_version(u32)`, `raw_data_version(&str)`, `migrator_both()`, `migrator_server_only()`, `migrator_listed_but_not_exported()`.
- Contracts: `provides_service`, `requires_service`, `wait_ms(u32)`, `extension_point`, `extension_contribution`, `activation_trigger`.

## ServerHandle (an in-process server for one session)

- Lifecycle: `apply_pending()`, `pause_at(UpdateStage)`, `crash()`, `shutdown()`, `is_running()`, `drop_socket()`, `snapshot_now()`, `kernel_call_log()`, `log_codes()`, `received_messages()`, `toasts()`, `notices()`.
- Instances: `active_version(id)`, `activations(id) -> Vec<String>`, `deactivations(id)`, `state_of(id, Side)`, `inactive_reason(id)`, `refusal(id)`, `failure_count(id, Side)`, `restart_delay_ms(id, Side)`, `fail(id, Side)`, `fail_activation_of_version(v)`, `is_disabled_by_failure(id)`, `is_enabled(id)`, `disable(id)`, `enable(id)`, `dependency_retry_times(id)`, `call_plugin`, `call_service`.
- State: `granted(id)`, `last_restored_len(id)`, `stored_client_restore(id)`, `attach_client(name) -> ClientHandle` (`detach_with_restore`, `restored_bytes`, `deactivations`), `set_node_metadata`, `node_metadata_as(reader, node, owner, key)`, `remove_node`, `add_decoration`, `decorations`, `write_storage`, `write_storage_checked`, `read_storage`, `write_setting`, `write_data_file(_checked)`, `read_data_file`, `send_pending_request(id) -> PendingRequest` (`outcome`, `id`), `deliveries_of`.
- Races and prompts: `race_disable_and_update`, `race_disable_of_two`, `answer_first_use`, `trigger_first_use`, `not_now_count`, `prompts_for`, `reload_policy`.
- UI: `render_plugins_screen()` (`row_for`, `rows`; row has `state`, `health`, `text`, `has_switch`), `render_plugin_detail(id).tabs()`, `press_space_on`, `press_recovery_enable`, `press_reset_failures`.
- Extensions and effects: `breach_extension`, `extension_stopped`, `call_extension`, `extension_events`, `contributions`, `triggers`, `replaces`, `hold_effects(id)` / `effects_of(id)` / `effects_of_instance(n)` (`input_lock`, `stream_open`, `child_running`, `any()`), `has_dev_package(id)`.

## DevSession

- `attached_session()`, `exit_code()`, `end()`, `kill_without_cleanup()`, `close_connection()`, `pause_renewals()`, `lease_renewals()`, `touch_source()`, `build_count()`, `reload(&PackageSpec)`, `reload_failing_build()`, `reload_log()`, `output_lines()`, `prompts()`, `answer_prompts(&str)`.

## Spec conflicts and assumptions

- LIF-072: the table says `install.same-version-different-content`; the page and the catalogue say `package.version-reused` (the old name is an alias). Tests assert `package.version-reused`; a doc_ test pins the alias row.
- LIF-150 and the persistence matrix: the last column says `--delete-data`; the CLI flag is `--purge`. Tests use `--purge`.
- LIF-193: the row says `denied_ids`; the policy page uses `plugins.deny_ids`. Tests use `deny_ids`.
- Matrix cells marked `S` (session bucket and failure window on server restart) conflict with LIF-037 and are not asserted.
- LIF-077: re-trying a blocked candidate is modelled by `install --reinstall`.
- LIF-162 and LIF-281: the manual gives no `plugins.toml` format; tests use `[plugins."<id>"]` tables and assert only that nothing is installed, granted or raised.
- Codes assumed from the catalogue pattern, not found verbatim: `restore.dropped-size`, `storage.corrupt-moved`, `storage.disk-full`, `install.disk-full`, `policy.denied`.
- Exact `session-result` fields (`session`, `ok`) and the `changed` field of enable and disable results are assumed.
- `ilium_plugin_host::testing::ops::LifeEvent` enum, variants `PluginRestart, DisableEnable, Update, ServerRestart, ClientReconnect, SessionDelete, Uninstall, UninstallPurge` (derive Clone, Copy, Debug, PartialEq, Eq); LIF-150..157 (keep-and-lose matrix columns).
- `ilium_plugin_host::testing::ops::Marker` enum, variants `LinearMemory, PendingRequest, Settings, RestoreServer, RestoreClient, GlobalBucket, ProjectBucket, SessionBucket, DataFolder, CacheFolder, Grants, NodeMetadata, DurableTimers` (derive Clone, Copy, Debug, PartialEq, Eq); LIF-150..157 (matrix rows). The E0408/E0425 errors in reference_persistence_and_restart.rs cascade from these.
