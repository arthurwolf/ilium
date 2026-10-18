# API gaps: ilium-plugin-host data_migration tests

Every test in this directory drives the migration rig below. None of it exists in the skeleton yet.
The rig is the contract these tests ask the skeleton to provide, in `ilium_plugin_host::testing::migration`.

- ilium-plugin-host: `pub mod testing::migration` with `MigrationRig`, `MigServer`, `PluginSpec`, `Step`, `Bucket`, `MigrateScope`, `MigrateCall`, `ActivateReport`; ids LIF-100..121, WIT migrator.migrate; one in-process rig with a fake clock, fake disk and fake plugin runtime.
- ilium-plugin-host: `MigrationRig::new() -> MigrationRig`, `server(name, project, session) -> MigServer`, `restart_server(&MigServer) -> MigServer`; LIF-104, 110, 112.
- ilium-plugin-host: `MigrationRig::install(&PluginSpec) -> CliOutput`, `update(&PluginSpec, &[&str]) -> CliOutput`, `rollback(&str, &[&str]) -> CliOutput`, `cli(&[&str]) -> CliOutput`, `cli_install_file(&PluginSpec, &[&str]) -> CliOutput`; LIF-100, 107, 111.
- ilium-plugin-host: `PluginSpec::{new(id, version), data_version(u32), data_version_json(&str), migrator(Vec<Step>), migrate_deadline_ms(u64), restore_bytes(Vec<u8>)}`; LIF-100, 103, 105.
- ilium-plugin-host: `Step::{Put(Bucket,String,Vec<u8>), Remove(Bucket,String), WriteFile(String,Vec<u8>), Fail(PluginError), Trap, Hang, KillHost, KillHostFirstRun}`; LIF-105, 108, 110, 116.
- ilium-plugin-host: `MigServer::{activate(&str) -> ActivateReport, upgrade(&str) -> ActivateReport, storage(&str) -> StorageHandle, stamp(&str, Bucket) -> Option<u32>, call_migrate(&str, u32, u32) -> Result<(), PluginError>, kill()}`; LIF-101..121.
- ilium-plugin-host: `ActivateReport::{active_version, rolled_back, killed, restored, deactivation_reason, lock_wait_ms}`; LIF-103, 108, 110, 112.
- ilium-plugin-host: `MigrateCall::{from, to, scope, elapsed_ms, backup_name, lock_held, returned_ok, returned_error}`; LIF-104, 105, 107, 112.
- ilium-plugin-host: `StorageHandle::{put, get, remove, keys, write_batch, has_compare_and_set}`; LIF-115, 117, 119, 120.
- ilium-plugin-host: `MigrationRig::{stamp, data_dir_stamp, stamp_location, plugin_data_dir, data_dir_files, write_data_file, backups, backup_dump, backup_data_files, migrate_calls, log_codes, log_line, failures_in_window, info_json, lock_path, config}`; LIF-101..114.
- ilium-plugin-host: `MigrationRig::{set_clock_utc, advance_clock, set_free_bytes, fill_disk, free_disk, set_plugin_quota, power_loss, read_after_power_loss, unflushed_bytes, crash_during_next_write}`; LIF-107, 109, 115, 118, 119.
- ilium-plugin-host: `PluginSpec::restore_bytes` data is delivered as `ActivateReport::restored`; ambiguity: LIF-103 gives no wire form.
- Assumption: LIF-107 cap 0 is ambiguous in the manual; only the no-backup outcome is asserted.
- Assumption: `attempts` is 0 at first interruption, 1 after the second start, per LIF-110.
- Assumption: free-space tests use a one MiB tolerance around 2 x data + 100 MiB.
- ilium-plugin-host: `MigrationRig::config_text() -> String`, `MigServer::{is_running() -> bool, write_setting(tab, key, value)}`; COR-090.
