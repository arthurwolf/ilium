# API gaps of the lane extensions-services (ilium-plugin-host/tests/extensions)

Every item below is missing from the skeleton. The tests are written against these signatures,
all of them referenced only from `support/mod.rs` and the test files through `use support::*`.
If the compile stage chooses another shape, change `support/mod.rs` (the re-exports and the
builder helpers) first. Part 2 of the lane extends this file as `API_GAPS_p2.md` if needed.

## Build wiring

- ilium-plugin-host dev-dependency: `ilium-plugin-host = { path = ".", features = ["testing"] }`
  (the test binary `tests/extensions/main.rs` needs `ilium_plugin_host::testing`; REQ: all).
- ilium-plugin-host dev-dependency: `tokio` feature `macros` is NOT needed; tests are plain
  `#[test]` and never use a runtime.

## ExtWorld: a multi-plugin world on the host (module `ilium_plugin_host::testing::ext_world`, re-exported from `ilium_plugin_host::testing`)

`HostHarness` runs one guest and has no manifest and no host-call path, so no extension point,
service or topic can be exercised with it (EXT-001..056, SVC-001..030, HST-470..478,
SEC-280..315). `ExtWorld` is the smallest world that can: it installs manifests (real install
checks), keeps a manual clock, runs scripted guests, and exposes the observation channels of the
manual (return values, completions, log lines, topics received, audit records, notices).

- `pub struct ExtWorld;` with `Debug`.
- `pub fn ExtWorld::new() -> ExtWorld` : empty host, manual clock at 0 ms, no core plugin installed. (all)
- `pub fn ExtWorld::with_core_plugins() -> ExtWorld` : the 30 bundled core plugins installed and enabled, T1. (EXT-052, EXT-051, EXT-053, SEC-300..306)
- `pub fn install(&mut self, tier: Tier, manifest: serde_json::Value, handlers: Vec<(String, HandlerScript)>) -> Result<(), ilium_plugin_api::Diagnostic>` : runs the install checks of the manifest (codes `manifest.*`, `policy.*`), installs, enables and activates. `handlers` maps a handler name (the manifest `handler` value, or `"<service>#<method>"` for a service provider) to its script. (all install-error rows)
- `pub fn update(&mut self, tier: Tier, manifest: serde_json::Value, handlers: Vec<(String, HandlerScript)>) -> Result<(), Diagnostic>` (EXT-008)
- `pub fn uninstall(&mut self, plugin: &str)`; `pub fn set_enabled(&mut self, plugin: &str, enabled: bool)`; `pub fn restart(&mut self, plugin: &str)` (EXT-008, EXT-021, EXT-027, SVC-008)
- `pub fn advance_ms(&mut self, ms: u64)` and `pub fn now_ms(&self) -> u64` : the manual clock; due timers fire (EXT-005, EXT-026, SVC-007)
- `pub fn run_hook(&mut self, caller: &str, point: &str, payload_json: &str) -> Result<RequestId, PluginError>` : `ilium:host/extensions.run-hook` as plugin `caller` (WIT:ilium:host/extensions.run-hook)
- `pub fn settle(&mut self)` : runs queued chains, handlers, deliveries and virtual time until nothing is pending; bounded by 10_000 internal steps, then returns (all)
- `pub fn outcome(&self, caller: &str, request: RequestId) -> Option<ilium_plugin_api::generated::types::requests::Outcome>` : the `request-completed` outcome delivered to `caller`
- `pub fn take_reply(&mut self, caller: &str, request: RequestId) -> Option<String>` (WIT:ilium:host/extensions.take-reply)
- `pub fn list(&self, caller: &str, point: &str) -> Result<Vec<ExtensionEntry>, PluginError>` (WIT:ilium:host/extensions.list); `ExtensionEntry` from `ilium_plugin_api::generated::host::extensions`
- `pub fn register(&mut self, caller: &str, point: &str, extension: &str, priority: u8, entry_json: &str) -> Result<(), PluginError>` and `pub fn unregister(&mut self, caller: &str, point: &str, extension: &str) -> Result<(), PluginError>` (WIT:ilium:host/extensions.register, .unregister). The `PluginError.message` starts with the extension code and ": ".
- `pub fn handler_runs(&self) -> Vec<HandlerRun>` : every handler call in start order. `pub struct HandlerRun { pub plugin: String, pub handler: String, pub point: String, pub start_ms: u64, pub payload_json: String, pub depth: u8 }` (EXT-005, EXT-007, EXT-011, EXT-012, HST-471)
- `pub fn all_logs(&self) -> Vec<ilium_plugin_host::telemetry::LogLine>` : every captured log line of every plugin and of the kernel, `plugin` field = emitting plugin or `"kernel"` (EXT-005, EXT-010, EXT-014, EXT-020, EXT-048, SEC-284..292)
- `pub fn topics_received(&self, plugin: &str) -> Vec<(String, String)>` : (topic, payload_json) of every `topic-published` event delivered to `plugin` (EXT-026, SVC-020..)
- `pub fn dormant_reason(&self, plugin: &str, point: &str, extension: &str) -> Option<String>` : `None` when active, `Some("dormant")`-style text otherwise; for a full point exactly `point full (<n>)` (EXT-006, EXT-027, EXT-030)
- `pub fn is_active(&self, plugin: &str, point: &str, extension: &str) -> bool`
- `pub fn set_user_order(&mut self, point: &str, order: Vec<(String, String)>)` : the stored (plugin id, extension id) list (EXT-008, SEC-288)
- `pub fn reset_extension(&mut self, plugin: &str, point: &str, extension: &str)` : the Reset button (EXT-021)
- `pub fn notices(&self) -> Vec<String>` : kernel notices drawn for the user (EXT-021, SEC-290, EXT-051, EXT-053)
- `pub fn audit(&self) -> Vec<ilium_plugin_host::audit::AuditRecord>` (SEC-290, SEC-291, SEC-306)
- `pub fn user_started(&mut self, ...)` is NOT assumed: user-started chains are marked by the payload field `source` (SEC-291).
- `pub fn answer_prompt(&mut self, yes: bool)` and `pub fn prompts(&self) -> Vec<String>` : kernel yes/no prompts such as `A guard did not answer. Continue anyway?` and `<plugin> refused: <reason>. Continue anyway?` (SEC-287, SEC-291, EXT-053)
- Services: `pub fn call_service(&mut self, caller: &str, service: &str, method: &str, payload_json: &str) -> Result<RequestId, PluginError>` (WIT:ilium:host/services.call); `pub fn service_take_reply(&mut self, caller: &str, request: RequestId) -> Option<String>` (WIT:ilium:host/services.take-reply); `pub fn service_reply(&mut self, provider: &str, call: u64, outcome: Result<String, PluginError>) -> Result<(), PluginError>` (WIT:ilium:host/services.reply); `pub fn service_calls_seen(&self, provider: &str) -> Vec<(u64, String, String, String)>` (call id, service, method, payload_json).
- Topics: `pub fn publish(&mut self, plugin: &str, topic: &str, payload_json: &str) -> Result<(), PluginError>` (WIT:ilium:host/topics.publish)
- Commands: `pub fn set_command_state(&mut self, plugin: &str, command: &str, enabled: bool, checked: Option<bool>) -> Result<(), PluginError>` and `pub fn invoke_command(&mut self, caller: &str, command: &str, arguments_json: &str) -> Result<RequestId, PluginError>` (WIT:ilium:host/commands.set-state, .invoke)
- `pub struct HandlerScript { pub elapsed_ms: u32, pub action: HandlerAction }` and `pub enum HandlerAction { Reply(HookReply), Fail(PluginError), Trap, RunHook { point: String, payload_json: String }, ServiceDone(String), ServiceDeferred, ServiceNever }` : the scripted behaviour of one handler or service method; `elapsed_ms` is virtual time spent before the action. `HookReply` is `ilium_plugin_api::generated::exports::hook_handler::HookReply`. (all hook rows)

## Other gaps

- `ilium_plugin_host::instance::ExportCall` has only `Hook`, `Job`, `Migrate`; the exports `annotator`, `token-provider`, `catalog-provider`, `menu-provider`, `virtual-rows-provider` and `service-provider` cannot be called. Needed by the WIT tests of `tests/extensions/reference_wit_exports.rs`:
  `ExportCall::{Annotate { buffer: BufferId, revision: u64, first_line: u32, lines: Vec<String> }, Tokens { buffer: BufferId, first_line: u32, lines: Vec<String>, state_in: Option<Vec<u8>> }, QueryCatalog(CatalogQuery), MenuItems(MenuContext), ListVirtualChildren { row_kind: String, parent: String, cursor: Option<String> }, ServiceHandle(ServiceCall)}` and the matching `ExportReply` variants `Annotations(Vec<LineAnnotation>)`, `Tokens(Vec<Token>, Option<Vec<u8>>)`, `CatalogPage(Result<CatalogPage, PluginError>)`, `MenuEntries(Vec<MenuEntry>)`, `VirtualChildren(Result<(Vec<VirtualRowChild>, Option<String>), PluginError>)`, `ServiceReply(ServiceReply)`. (WIT:ilium:exports/annotator.annotate, token-provider.tokens, catalog-provider.query-catalog, menu-provider.menu-items, virtual-rows-provider.list-virtual-children, service-provider.handle)
- `ilium_plugin_host::testing::HarnessHandle::call_export(&self, call: ExportCall) -> Result<ExportReply, Trap>` : drives one export of the guest in the harness (same WIT rows).
- `ExportReply::Hook(HookReply)` cannot express the error return `result<hook-reply, plugin-error>` of `hook-handler.handle-hook` (reference-wit.md section 8.14, EXT-020): `ExportReply::HookResult(Result<HookReply, PluginError>)`. (WIT:ilium:exports/hook-handler.handle-hook)
- `ilium_plugin_api::generated::exports::hook_handler::HookCall` has no `extension` field although api-extension-points.md says `HookCall.extension` is the manifest `handler` name (the WIT in reference-wit.md has `call`, `point`, `payload-json` only). The tests dispatch on `point` and the handler name given at install; the manual pages disagree (documentation defect, not an implementation gap).
- `ilium_plugin_host::extensions::ChainMode` lacks `Observe` and `First` and `ExtensionRegistry` has no constructor, `list`, `register`; the tests do not use `ExtensionRegistry` directly. (EXT-010..013)
- Menu: `pub struct MenuHarness` in `ilium_plugin_host::testing` : `pub fn new() -> MenuHarness`, `pub fn add_provider(&mut self, plugin: &str, answer_after_ms: u32, entries: Vec<MenuEntry>)`, `pub fn add_stopped_provider(&mut self, plugin: &str, activation_ms: u32, entries: Vec<MenuEntry>)`, `pub fn static_entries(&mut self, entries: Vec<MenuEntry>)`, `pub fn open(&mut self, context: MenuContext) -> MenuOpening`; `MenuOpening { pub frames: Vec<Vec<String>> /* entry ids per drawn frame */, pub opened_at_ms: u64, pub provider_calls: Vec<(String, u64)> /* plugin, start ms */, pub activations_waited_for: Vec<String> }`; `MenuEntry`/`MenuContext` from `ilium_plugin_api::generated::exports::menu_provider`. (HST-476, HST-477, HST-478)
- Manual path helper: tests read the manual from `$CARGO_MANIFEST_DIR/../docs/plugins-manual/pages` (override `ILIUM_PLUGIN_MANUAL_DIR`) through a local helper in `support/mod.rs`; no gap.

## Additional ExtWorld methods used by tests added later

hook_errors() -> Vec<(String, PluginError)>; breach_count(plugin, point, extension) -> u32; set_script(plugin, handler, HandlerScript); set_instance_busy(plugin, bool); set_instance_activating(plugin, bool); set_helper_round_trip_ms(u32); set_cpu_ms_per_call(plugin, u32); plugin_active(plugin) -> bool; deactivate(plugin); restart_count(plugin) -> u32; queue_capacity(plugin) -> usize; queue_len(plugin) -> usize; block_delivery(plugin, bool); events_dropped(plugin) -> Vec<u32>; first_event_of_next_batch(plugin) -> Option<String>; send_message(from, to, channel, &[u8]) -> Result<RequestId, PluginError>; messages_received(plugin) -> Vec<(String, String, Vec<u8>)>; event_order(plugin) -> Vec<String>; fill_queue_with_never_dropped_events(plugin); fill_queue_with_drop_oldest_events(plugin); deliver_never_dropped_event(plugin). Inline-object `schema` in manifest `contributes.services.provides`.

## ExtWorld methods used by commands, menu, core replacement, security and export tests

- Commands: set_command_state(plugin, command, enabled, visible), invoke_command(caller, command, args_json) -> Result<RequestId, PluginError>, script_command(command, HandlerScript), outcome(plugin, RequestId) -> Option<Outcome>, install_on(tier, side, manifest, scripts), advance_ms, settle.
- Replacement: ExtWorld::with_core_plugins(), ReplacementShape and replacement_manifest(id, core, shape), core_surface(core), add_core_surface_item, update_ilium_with_surface_item, set_enabled, plugin_enabled, answer_prompt, prompts, notices, uninstall, granted, declares, contributions_of, set_policy, service_provider, plugin_installed, palette_labels_of, is_active, plugins_screen_lines, owner_of_kernel_function, audit() -> records with action, data_folder_exists, can_read_data_folder, list_points().
- Core points: emit_log, chains_started, log_records_delivered_to, batch_sizes, type_key_into_pane, paste_into_pane, plugin_write_input, last_chain_payload, register_snapshot_section, set_snapshot_reply, take_snapshot, restore_snapshot, stored_snapshot_section, restore_event_order, set_restore_duration_ms.
- Security: point_risk, review_row, review_lines, entries_of, seen_payload, initial_payload, guard_ran_after_transforms, HookShape builders (transform, veto, with_final_check, high), Contrib::new/guard, veto_overridable, open_project, trust_prompts, answer_trust, revoke_trust, project_trusted, set_project_value(_of_type), get_setting (field source), ignored_project_values_notice, first_use_prompt, answer_first_use, sdk_project_trusted.
- Exports: ExtWorld::call_export(plugin, ExportCall), script_export(plugin, export, HandlerScript), CatalogQuery, ServiceCall, ServiceReply, BufferId.
- MenuHarness: add_provider(id, answer_ms, entries), add_stopped_provider, static_entries, open(context) -> MenuOpening { opened_at_ms, frames, provider_calls, activations_waited_for }, advance_ms.

Known doc conflicts: SVC-026 (full queue restarts) vs SVC-030 (refused busy, no restart); tests follow SVC-030. HookCall has no `extension` field though the manual names one.

## Added by the compile stage (round 1)

- ilium-plugin-host testing: `HandlerAction` also needs the variants used by the chain and export tests: `Pass`, `Replace(serde_json::Value)`, `Refuse(String)`, `ErrorReturn`, `Sleep(u32)`, `LogWarn(String)` (EXT-020, SEC-284, SEC-287, SEC-292).
- ilium-plugin-host testing: `ExtWorld::last_veto_reason(&self) -> Option<String>` (the sanitised reason of the last refusal, SEC-292) and `ExtWorld::last_veto_ui_text(&self) -> String` (the text drawn to the user, prefixed with the contributor display name, SEC-292).
- ilium-plugin-host testing: `ExtWorld::install_on(&mut self, tier: Tier, side: Side, manifest: serde_json::Value, handlers: Vec<(String, HandlerScript)>) -> Result<(), Diagnostic>` (WIT exports tests).
- `ReplacementShape`, `HookShape`, `Contrib::new/guard` and `ChainResult` are now local test helpers (support/mod.rs, reference_extension_security.rs), no longer gaps.
