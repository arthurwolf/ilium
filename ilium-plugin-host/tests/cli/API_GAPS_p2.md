# API gaps part 2: install failures, uninstall, enable/disable, list, grants, dev, logs, log-level

All of part 1's gaps (CliEnv::new/with_*, CliOutput stdout_raw/stdout_text, MemorySession core methods, DevEvent::{Change, Interrupt}) apply too.

- ilium-plugin-host `cli::DevEvent::WriteFile { at_ms: u64, path: String, bytes: Vec<u8> }` (writes the file in the project, then counts as a change); DEV-184. Why: a rebuilt manifest that adds grants.
- ilium-plugin-host `cli::DevEvent::Calls { at_ms: u64, side: String, export: String, duration_ms: Vec<u64> }`; DEV-196. Why: plugin calls whose timings `dev --timings` reports.
- ilium-plugin-host `cli::DevEvent::Log { at_ms: u64, side: String, level: String, message: String }`; DEV-190. Why: a log line that arrives while `logs --follow` runs.
- ilium-plugin-host `cli::CliEnv::with_build_duration_ms(self, u64) -> CliEnv` (virtual duration of each build in the dev script clock); DEV-186. Why: changes must arrive while a build runs.
- ilium-plugin-host `cli::MemorySession::decline_review(&self, bool)` and `decline_trust(&self, bool)`; DEV-143, DEV-172. Why: the user answers the dialogs with "no".
- ilium-plugin-host `cli::MemorySession::plugin_version(&self, id: &str) -> Option<String>`, `activations(&self, id: &str) -> usize`, `deactivations(&self, id: &str) -> Vec<String>` (reason of each deactivation, in order), `restarts(&self, id: &str) -> usize`; DEV-143, 144, 155, 160, 161, 171, 187, 189.
- ilium-plugin-host `cli::MemorySession::package_installed(id) -> bool`, `add_contribution(id, kind: &str, needs_grant: &str)`, `contribution_count(id) -> usize`; DEV-155, DEV-171.
- ilium-plugin-host `cli::MemorySession::seed_plugin_data(id)` (creates a data folder, a cache folder, one storage bucket, one settings value and a log), `data_folder_exists(id) -> bool`, `cache_folder_exists(id) -> bool`, `storage_bucket_count(id) -> usize`, `settings_value_count(id) -> usize`, `log_kept(id) -> bool`, `keep_log(id, lines: &[(&str, &str, &str)])` (side, level, message of a log kept for an uninstalled id); DEV-155, 156, 160, 189, 191.
- ilium-plugin-host `cli::MemorySession::set_failure_count(id, usize)`, `failure_count(id) -> usize`; DEV-160.
- ilium-plugin-host `cli::MemorySession::add_plugin_with_grants(id, version, tier, state, dev: bool, grants: &[(&str, &str, &str)])` with (grant, risk, state) triples where state is granted|declined|revoked; `grant_state(id, grant) -> Option<String>`; DEV-165, 170, 171, 172. The risk words in the tests ("low", "medium", "high") are only echoed by the fake; the real vocabulary comes from the grants page.
- ilium-plugin-host `cli::MemorySession::install_history() -> Vec<InstallRecord>` with `InstallRecord { id: String, version: String, tier: String, dev: bool, data_dir: PathBuf, grants: Vec<String> }`; `state_history(id, dev: bool) -> Vec<String>` (states of the dev or non-dev copy over time); `review_dialog_grants() -> Vec<Vec<String>>` (grants shown by each dialog); `attach_clients(n: usize)`; `set_restore_bytes(id, usize)`; `activation_restore_bytes(id) -> Vec<usize>`; DEV-181, 183, 184, 187, 188.
- ilium-plugin-host `cli::MemorySession::log_level(id) -> String` (default "info"), `set_default_log_level(&str)`; DEV-195.
- Constant `EXTRA_LOW_RISK_GRANT = "workspace.read"` (common_p2.rs) and the grant ids `clipboard.write` and `workspace.read` used in grants tests must exist as low/medium-risk ids in the grants catalogue; adjust to the grants page. HIGH_RISK_GRANT is as in part 1.
- `builder_builds_of_one_dev_build() == 2` in reference_plugin_cli_dev.rs assumes one compile stage per side of the `both` template; adjust if the pipeline counts differently.
- ilium (bin): `ilium plugin dev|logs|log` wired as in reference-plugin-cli.md; DEV-180, DEV-192 process tests.
