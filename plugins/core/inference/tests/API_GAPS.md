# API gaps found by the inference feature tests

Each bullet: crate, exact signature, requirement id, why. The tests are written against these items as the manual specifies them.

- ilium-plugin-core-inference: `pub struct InferenceClient;` (`#[ilium_plugin(side = "client", exports = [])]`, `Default`, `Lifecycle`, `Handler`, `Views`). Ids F-084, F-085. The page says the plugin runs on both sides and holds client grants (`ui.panels`, two `settings.write-kernel:*`), but the skeleton has only `InferenceServer`.
- ilium-plugin-core-inference: `impl ilium_plugin_sdk::InferenceProvider for InferenceServer` (`complete(&mut self, ProviderCall) -> Result<(), PluginError>`, `models(&mut self) -> Vec<String>`) and `#[ilium_plugin(side = "server", exports = ["inference-provider", "catalog-provider"])]`. Ids F-084. Sample 1 and 3 of the page.
- ilium-plugin-core-inference: `impl ilium_plugin_sdk::CatalogProvider for InferenceServer` (`query_catalog`). Id F-084. Sample 3.
- ilium-plugin-testkit: `impl FakeHost { pub fn instance_mut<P: NativeEntry + Default + 'static>(&mut self) -> &mut P }`. Returns the activated plugin instance so a test can call its export traits (`complete`, `models`, `query_catalog`) in native mode. Ids F-084.
- ilium-plugin-testkit: `impl FakeHost { pub fn with_settings(self, settings: serde_json::Value) -> FakeHost }` (builder). Sets `Activation.settings_json` before `activate`. Ids F-084.
- ilium-plugin-testkit: `impl FakeHost { pub fn inference(&mut self) -> &mut FakeInference }` with `FakeInference::provider_replies(&self) -> Vec<(u64, Result<Completion, PluginError>)>` (every `provider_reply` the plugin made, in order) and `FakeInference::script_completion(&mut self, request: RequestRef, completion: Completion)` (what `take_completion` returns after `FakeHost::complete`). Ids F-084.
- ilium-plugin-testkit: `impl FakeHttp { pub fn streams(&self) -> Vec<(StreamId, StreamRequest)> }` and `pub fn expect_stream(&mut self, url: &str) -> StreamId`. Records `open_stream` calls and hands out stream ids. Id F-084. Sample 2.
- ilium-plugin-testkit: `impl FakeHost { pub fn topics(&mut self) -> &mut FakeTopics }` with `FakeTopics::published(&self) -> Vec<(String, String)>` (topic, payload_json). Id F-084 (topic `ilium.core.inference/request-finished`).
- ilium-plugin-testkit: `impl FakeHost { pub fn extensions(&mut self) -> &mut FakeExtensions }` with `FakeExtensions::on_hook(&mut self, point: &str, reply: HookReply)` (what a contributor answers), `FakeExtensions::hook_payloads(&self, point: &str) -> Vec<String>` (payload_json of each run), `FakeExtensions::contribute_profile(&mut self, entry_json: &str)` (a `provider-profiles` collect contributor). Ids F-084, SEC-293.
- ilium-plugin-testkit: `impl FakeSettings { pub fn info(&self, id: &str) -> Option<String> }`. Text of an info row written with `settings::set_info`. Id F-084 (Test provider, sample 4).
- ilium-plugin-core-inference: view ids are not named by the manual. The tests render `ViewTarget::SettingsPage("inference")` (tab figure) and `ViewTarget::Popup("test-provider")` (test dialog figure); the implementation must use these ids or the manual must name others.
- manual conflict, manifest.json: the page "Complete manifest" has no `server.exports`, no `contributes.catalogs` and no `client` entry, but the capability page capability-inference-provide.md says the manifest lists `inference-provider` in `server.exports` and sample 3 of the core page needs `contributes.catalogs: [{ "id": "models" }]`. `features::manifest::manifest_050_equals_the_complete_manifest_of_the_page` and `manifest_provide_*` cannot both pass until the page and the capability page agree.
- manual conflict, SEC-293: the core page marks `provider-profiles` risk `standard`; reference-extension-security.md marks it `high`. The test follows the corrected security table.
- manual conflict, hook call: the core page sample reads `call.extension`; the skeleton `HookCall` has `point`. The tests use `point`.
- manifest grants: only `http:api.openai.com` and `http:localhost` are granted, so the Anthropic, OpenRouter and Kilo Gateway providers of src/docs/inference-and-privacy.md cannot reach their hosts. The tests cover OpenAI-compatible and Ollama routing only.
- dev-dependency: none missing (serde_json and proptest are present).

- Guessed command id `ilium.core.inference/load-models` (model refresh); the manual names no command id for it.
- `FakeHost::extensions().on_hook_trap(point)` and `HookReply::{Replace,Pass,Refuse,Answer}` are assumed testkit items.
- `FakeSession::{install_package_dir, services, grant_service, call_service, disable, kernel_settings_value, disabled_notice}` and `FakeSettings::{writes, seed, section_value}` are assumed testkit items.
- `FakeHttp::expect_stream` / `streams()` returning (StreamId, request) pairs.

Part ports (tests/port):
- ilium-plugin-testkit: `impl FakeSession { pub fn settings_tab_order(&self) -> Vec<String> }` (tab ids of the Settings rail in order, plugin tabs included). Id PORT:ilium-client/src/app.rs::settings_tabs_cycle_through_inference_and_every_existing_tab (F-084).
- ilium-plugin-testkit: `FakeSettings::set` / `get` must accept a nested table key (`openai`, `ollama`, `kilo_gateway`) and `get("model")` must return the value written by a `ui-action` on the model choice row; `FakeHost::deliver_one` must start a task without finishing it until `settle()` (used for the in-flight ABA twins).
- manual gap: key and value form of the model choice action (`ActionValue::Text(model name)` vs `Index`) is not stated; the twin uses Text. The kilo gateway host (`api.kilo.ai`) is not in the manifest grants, so the kilo twins fail until the page grants it.
- manual gap: the status text "Could not load Ollama (local) models: connection refused" and the spinner of the original ollama/test-lifecycle tests have no stated channel in the page; the twins assert only list and info-row effects.

Retention tests (features/retention.rs, COR-950 to COR-952):
- ilium-plugin-testkit: `FakeHost::audit_records(&self) -> Vec<AuditRecord>` (the audit log entries the host kept for the plugin). COR-950. The test body is `unimplemented!`.
- ilium-plugin-testkit: `FakeHost::privacy_log(&self) -> Vec<String>` (the privacy log records as JSON text, one per admitted request, with the plugin, purpose, provider, model, token counts and `local` flag). COR-950. The test body is `unimplemented!`.
- ilium-plugin-testkit: `FakeHost::logs()` exists but its body is `unimplemented!("DEV-499")`; the plugin-log tests call it as the only channel for the plugin log. They panic until DEV-499 lands.
- manual gap: the provider entry field that holds the retention statement is not named. The tests assume a `retention` string inside the provider's settings table (`openai.retention`, `ollama.retention`); the "retention unknown" text is taken verbatim from COR-951.
- manual gap: the Inference tab text that carries the statement is not on the page golden (`inference_tab_56.txt` has no retention line). The tests search the rendered tab for the statement text, at 100 columns.
- manual gap: the retention confirmation (COR-952) has no stated view, popup id, command id or settings key. The tests assume a per-provider confirmation kept in the plugin state, and that a remote request is held until it is given. `FakeSession::confirm_retention(provider: &str)` and `FakeSession::retention_confirmations(&self) -> Vec<String>` are assumed testkit items; both test bodies are `unimplemented!`.
- manual gap: the error code returned to `complete` while a remote request waits for confirmation is not stated. The COR-952 negative tests ignore the returned result and check only that no HTTP request was sent.
