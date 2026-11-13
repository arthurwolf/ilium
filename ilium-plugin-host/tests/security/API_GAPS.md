# API gaps: lane security, ilium-plugin-host (part 1)

Items the security tests use that the skeleton lacks. Signatures are as the tests call them. All of them are reached through `support.rs` (harness side) or directly (modules `audit`, `secrets`, `review`, `cli`). Part 2 appends below.

## Harness (ilium-plugin-host::testing, feature `testing`)

- ilium-plugin-host::testing: `HostHarness::{state_dir(PathBuf), net(FakeNet), offline(bool), system_policy(&str, bool /*root-owned and not group/other writable*/), user_policy(&str), effective_uid(u32), secret(&str, &str), setting(&str, serde_json::Value), option(&str, serde_json::Value)}` -> `HostHarness`. One state directory holds every store, `audit/`, `privacy/` and `secrets.json`. Ids: SEC-084, 180..199, 220..250. Why: policy, secrets and the scratch state must be injectable without a real /etc.
- ilium-plugin-host::testing: `HostHarness::option(&str, Value)` keys read by the tests: `declined` (list of grants), `safe-mode`, `kernel-secret:<key>`, `keyring` (bool), `system-policy-unreadable`, `provider` ({name, local}), `trapping-guest`, `replacement-inference-plugin`, `accepts-messages-from` (list), `tampered-t1-bundle`, `platform` (`linux|macos|windows`), `sandbox-unavailable` (prerequisite text). Ids: SEC-003..014, 187, 222, 241, 244, 264, 265. Why: one catch-all keeps the builder small.
- ilium-plugin-host::testing: `HarnessHandle::{call_import(&str, Value) -> ImportResult, call_import_value(&str, Value) -> Result<Value, PluginError>}` and `enum ImportResult { Ok, Err(PluginError), Trap(Trap) }` (same as the grants lane). Ids: SEC-001, 013, 070..086. Why: tests call imports by WIT name.
- ilium-plugin-host::testing: `HarnessHandle::{revoke(&str), grant_now(&str), flush_audit_summaries(), persist_all_stores(), audit_path() -> PathBuf, notices() -> Vec<String>, policy_show() -> Vec<serde_json::Value>, load_error() -> Option<String>, load_error_message() -> Option<String>, isolation() -> Isolation, isolation_attempted() -> Option<Isolation>, guest_process_id() -> u32, guest_clock_nanos() -> u64}` and `enum Isolation { InProcess, HelperProcess }`. Ids: SEC-003, 011, 013, 014, 012, 182, 187, 191, 197, 220, 225. Why: observable channels for load decisions, policy and audit.
- ilium-plugin-host::testing: `HarnessHandle::{audit_append(&str /*event*/, &str /*detail*/), audit_fill_to(u64 /*bytes*/), audit_limits() -> (u32, u32), fail_audit_writes(bool), audit_write_attempts() -> u32, audit_queue_len() -> usize, audit_queue_events() -> Vec<String>, audit_dropped_count() -> u64, system_log_messages() -> Vec<SystemLogMessage{identifier: String, text: String}>, edit_store_file_externally(&str)}`. Ids: SEC-222, 224, 225, 228. Why: rotation, disk-full queue and sinks need controlled input.
- ilium-plugin-host::testing: `HarnessHandle::{install_check(&str /*id*/, Tier) -> Result<(), String /*code*/>, install_check_from_url(&str, Tier), install_check_with_grants(&str, Tier, &[&str]), install_check_signed_by(&str, &str /*fingerprint*/), install_check_replacing(&str, &str), install_from_system_dir(&Path, &str) -> Result<PathBuf, PluginError>, user_install_dir() -> PathBuf, is_installed(&str) -> bool}`. Ids: SEC-005, 181, 184, 195. Why: policy codes at install without building packages.
- ilium-plugin-host::testing: `HarnessHandle::{review_rows(&[&str]) -> Vec<ReviewRow>, review_text_for(&[&str]) -> String, review_text_with_data_use(&[Value]) -> String, details_text_with_contact(&str) -> String, try_check_row(&str) -> bool, consent_prompt_text_for_url(&str) -> String}` and `struct ReviewRow { grant: String, checked: bool, locked: bool, checkable: bool, toxic: bool, warning_style: bool, risk: String, note: Option<String>, text: String, detail: String }`. Ids: SEC-004, 007, 185, 247, 264, 267, 268. Why: dialog content without a terminal.
- ilium-plugin-host::testing: `HarnessHandle::{write_plugin_data(&str, &[u8]), read_plugin_data(&str) -> Option<Vec<u8>>, replace_system_policy_file(&str), trust_project(&str), write_project_value(&str, &str, &str, Value), effective_setting(&str) -> Option<Value>, user_set_setting(&str, Value) -> Result<(), PluginError>, runtime_dir() -> PathBuf, add_session(&str, &str /*owner*/)}`. Ids: SEC-186, 188, 190, 192, 194, 196, 242. Why: data, project values and multi-user layout.
- ilium-plugin-host::testing: `HarnessHandle::{log_from_plugin(&str), error_string_for(&str) -> String, trap_message_for(&str) -> String, set_secret_now(&str, &str), stored_secret_text() -> String, secret_value(&str) -> Option<String>, keyring_value(&str, &str) -> Option<String>, forget_secrets(Option<&str>), secrets_screen_text() -> String, review_text() -> String, cli_json_text() -> String, config_text() -> Vec<String>, snapshot_text() -> Option<String>, export_text() -> Vec<String>, journal_text() -> Vec<String>, restore_snapshot(&str), channel_payload_text(&str) -> String, diagnostics_report_with_section(&str) -> DiagnosticsSection{shown: String, saved: String}, delivered_events_text() -> String}`. Ids: SEC-240..250. Why: every place a secret could land must be readable.
- ilium-plugin-host::testing: `HarnessHandle::{peer_messages() -> Vec<PeerMessage{sender: String, payload: Vec<u8>}>, deliver_peer_message(&str, &[u8]), inject_helper_message(&[u8]), plugin_environment() -> Vec<(String, String)>, plugin_arguments() -> Vec<String>}`. Ids: SEC-002, 007, 098. Why: inter-plugin and helper channels.
- ilium-plugin-host::testing: `HarnessHandle::{http_api_bind_address() -> SocketAddr, http_api_token() -> String, http_api_token_path() -> PathBuf, http_api_request(&str, &[(&str, &str)]) -> u16, host_egress(&str /*feature*/, &str /*url*/) -> Result<(), PluginError>, stream_close_reason(u64) -> Option<String>, stream_bytes_delivered(u64) -> u64, stream_largest_read(u64) -> u64}`. Ids: SEC-082, 086, 097..099. Why: listener and host-initiated features.
- ilium-plugin-host::testing: `HarnessHandle::{privacy_log_fill_to(u64)}`. Id: SEC-265. Why: size retention boundary.
- ilium-plugin-host::testing: `HarnessHandle::advance_clock(&mut self, Duration)` exists; the tests call it on a `&mut` binding and on shared `Rig` fields.

## Fake network (ilium-plugin-host::testing)

- ilium-plugin-host::testing: `FakeNet::{new(), dns(&str, &[&str]), dns_then(&str, &[&str], &[&str] /*second resolution*/), serve(&str, FakeResponse), redirect(&str, u16, &str), fail_tls(&str), set_tls_version(&str, &str), require_root(&str, &str), trust_bundle(&str, &str), reserve_ilium_port(u16), reserve_ilium_port_of_other_session(u16), seen() -> Vec<SeenRequest>, resolve_count(&str) -> usize, resolutions() -> Vec<String>}` and `Clone`. Ids: SEC-070..087.
- ilium-plugin-host::testing: `struct FakeResponse` with `ok(Vec<u8>)`, `sized(u64 /*virtual body length*/)`, `header(&str, &str) -> FakeResponse`; `struct SeenRequest { url: String, addr: IpAddr, headers: Vec<(String, String)>, body_len: u64, via_proxy: Option<String>, tls_version: Option<String> }`. Ids: SEC-072..083.

## Other crates

- ilium-plugin-host::audit: `pub const EVENT_NAMES: &[&str]` (the catalogue of SEC-222). Id: SEC-222.
- ilium-plugin-host::secrets: `redact(&str, &[String]) -> String`, `base64_standard(&[u8]) -> String`, `base64_url(&[u8]) -> String`, `percent_encode(&str) -> String`, `hex_lower(&[u8]) -> String`, `masked_row(&str, u8 /*reveal_last*/) -> String`. Ids: SEC-223, 243, 248.
- ilium-plugin-host::review: `grant_group(&str) -> Option<GrantGroup>` and `enum GrantGroup { DataRead, Egress }` (same module the grants lane names). Id: SEC-006.
- ilium-plugin-host::cli::run: subcommands `plugin audit [--verify|--plugin|--since|--event|--limit]`, `plugin policy show|reload`, `privacy log [--since] [--plugin]`, `config set <key> <value>`, `--version --verbose`. The tests read the audit file and the privacy log below `CliEnv.state_dir` (`audit/audit.jsonl`, `privacy/inference.jsonl`). Ids: SEC-188, 190, 226, 227, 265, 320.
- ilium-plugin-api::manifest: the keys `data_use`, `security_contact` and settings rows with `secret` are not in the `Manifest` struct. The tests only call `validate_json` and expect `manifest.secret-scope`, `manifest.unknown-key` (absent for these keys) and the length limits of SEC-267/268 to produce errors. Ids: SEC-242, 267, 268.
- Dev-dependencies of ilium-plugin-host: `sha2`, `serde_json` is a normal dependency, `tempfile` and `proptest` exist.

## Manual conflicts noticed

- SEC-082 gives 32 MiB for a buffered response; api-http.md `fetch` says 8 MiB by default. The test `sec_082_default_response_cap_applies_to_a_plain_fetch` follows SEC-082; `sec_082_buffered_response_over_32_mib_...` raises the limit through `limits.http_response_bytes` for a T1 plugin. The manual should name which value is the default.
- SEC-004 names `notify channels` as an egress group without a grant string; the tests use `notify.desktop`.

## Repository files the tests expect

- `SECURITY.md` at the repository root (SEC-320, 321, 323, 324) and `.github/SECURITY_ADVISORY_TEMPLATE.md` (SEC-322). Both are missing in the worktree, so those tests are red for a real reason.

# Part 2 (SEC-325 .. SEC-381)

## Harness (`ilium-plugin-host::testing::HarnessHandle`)

- `plugin_disabled(&self, &str) -> bool`, `package_sha256(&self) -> String`, `publisher_fingerprint(&self) -> String`, `uninstall(&mut self, &str) -> Result<(), String>`, `enable(&mut self, &str) -> Result<(), String>`, `replacement_active(&self, &str) -> bool` (SEC-325..331, blocklist and advisory effects).
- `status_bar_text(&self) -> String`, `plugins_screen_text(&self) -> String` (SEC-326, 377: SAFE badge and plugin list text).
- `run_hostile_probe(&self, who: &str, attempt: &str) -> String`, `helper_responsive(&self) -> bool`, `inject_failure_count(&self) -> u32` (SEC-340..348 hostile-guest and fault-injection checks).
- `render_dialog(&self, kind: &str, columns: u16, dark: bool) -> Vec<String>`, `screen_reader_text(&self, kind: &str) -> String` (SEC-346, 378).
- `guest_wasi_clock_nanos(&self) -> u64` (SEC-370: virtualised wasi clock).
- `pane_output(&mut self, &str)`, `set_pane_echo(&mut self, bool)`, `screen_changed_count(&self) -> usize`, `screen_changed_payloads(&self) -> Vec<String>` (SEC-371, 379).
- `trap_message_for(&self, &str) -> String`, `error_string_for(&self, &str) -> String` (SEC-374).
- `write_other_plugin_data(&self, plugin: &str, key: &str, bytes: &[u8])` (SEC-375).
- Kernel dialogs: `open_kernel_dialog(&self, &str)`, `visible_dialogs(&self) -> Vec<String>`, `queued_dialogs(&self) -> Vec<String>`, `dialog_is_topmost(&self, &str) -> bool`, `dialog_cells(&self, &str) -> Vec<String>`, `composited_screen(&self) -> Vec<String>`, `set_user_typing_in_terminal(&self, bool)`, `press_dialog_key(&self)`, `type_keys(&self, &str)`, `dialog_received_keys(&self) -> String` (SEC-377, 379).
- Plugin UI: `plugin_draw_rows(&self, &[&str]) -> Result<(), String>`, `plugin_status_segment(&self, &str)`, `plugin_screen_capture(&self) -> String`, `render_plugin_chrome(&self, kind: &str, text: &str) -> String`, `plugin_display_name(&self) -> String`, `render_plugin_text(&self, field: &str, text: &str) -> String` (SEC-377..380).
- `install_check_with_name(&self, id: &str, tier: Tier, name: &str) -> Result<(), String>` (SEC-381, install-time `manifest.display-name`).
- Options read: `blocklist` (list of `{kind: package|id|publisher-key, value, versions, reason}`), `runtime-advisory`, `installed-plugins` (list of ids), `fine-clock`, `safe-mode` (true or "all-optional").

## Other crates

- `ilium_plugin_host::lifecycle::resolve_safe_mode(args: &[String], env_value: Option<&str>) -> Result<SafeMode, UsageError>`; `enum SafeMode { Off, ThirdParty, AllOptional, NoPlugins }`; `struct UsageError { exit_code: i32, code: String }` with Debug, PartialEq, Eq (SEC-328).
- `ilium_plugin_host::render::sanitize_plugin_text(&str, limit: usize) -> String` (SEC-380).
- `ilium_plugin_host::sync::constant_time_eq(&[u8], &[u8]) -> bool` (SEC-376).

## Repository files the tests expect (all missing today)

- `fuzz/fuzz_targets/<stem>.rs`, non-empty `fuzz/corpus/<stem>/`, and `fuzz/regressions/` for: iliumpkg_reader, manifest_parser, grant_matcher, path_scope, settings_schema, payload_schema, wit_lifting, dispatcher_protocol, codecs, http_api_request, transcript_parsers, detector_screens, regex_compile, audit_verify (SEC-340..345).
- `.github/workflows/*` with `-max_total_time=600`, `-max_total_time=14400`, address and undefined sanitizers, a `paths:` filter containing `ilium-plugin`, and no `continue-on-error: true`.
- `tests/security-fixtures/{archives,manifests,paths,spoofing,patterns}/`, non-empty, and a `security-fixtures` mention in `ilium-plugin-sdk/Cargo.toml` or its `README.md`.

## Notes

- The meta tests for SEC-347 check that tests tagged SEC-370/371/373/374 exist; they are repository-structure checks and pass once `reference_threat_model_p2.rs` is built.
- The skeleton `Manifest` has no `description` or `security_contact` field.
- SEC-346 is a manual row, covered by render and page doc tests.
