# API gaps: config-migration lane, server crate tests

Rig contract in `ilium_plugin_host::testing::config` (does not exist in the skeleton). Ids COR-030..033.

- ilium-plugin-host: `testing::config::{ConfigRig, PluginSpec, InstallResult{ok: bool, codes: Vec<String>}, SnapshotFile, SectionHandle}` (COR-030..033).
- ilium-plugin-host: `ConfigRig::{new(), with_core_plugins(), install(&PluginSpec) -> InstallResult, uninstall(&str) -> bool, is_installed(&str) -> bool, kernel_tabs() -> Vec<String>, write_owners(&str) -> Vec<String>, risk_of(&str) -> Option<String>}` (COR-030, 031).
- ilium-plugin-host: `PluginSpec::{new(id, tier: &str), grants(&[&str]), snapshot_section(name, schema_version: u32, max_bytes: usize)}` (COR-031, 032).
- ilium-plugin-host: `ConfigRig::{section(plugin, name, schema_version, max_bytes) -> SectionHandle, write_section(&SectionHandle, &[u8]) -> Result<(), PluginError>, write_snapshot() -> Result<(), PluginError>, snapshot() -> SnapshotFile, crash_plugin(&str), plugin_stores_in_storage(&str, &str, &[u8]), log_codes() -> Vec<String>}`; `SnapshotFile::{section(&str) -> Option<Vec<u8>>, schema_version(&str) -> Option<u32>, plugin_keys() -> Vec<String>}` (COR-032).
- ilium-plugin-host: `ConfigRig::{write_config(&str), read_config() -> String, set_setting(&str, &str), write_project_config(&str), read_project_config() -> String, set_project_setting(&str, &str), config_key_paths() -> Vec<String>, run_plugin_build()}` (COR-033).
- Assumption: COR-032 "max_bytes at most 256 KiB" is enforced at install; the refusing code is not named, so the test asserts only failure.
- Assumption: COR-032 oversize write returns Err and logs a code containing "snapshot".
