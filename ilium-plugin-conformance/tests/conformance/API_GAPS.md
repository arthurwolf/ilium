# API gaps found by the conformance lane

Each bullet: crate, exact signature, requirement id, why. Tests reach each item through a shim in
`ilium-plugin-conformance/tests/conformance/gap.rs` that panics with `api.gap` until the item exists.

- ilium-plugin-conformance `Cargo.toml`: no `ilium-plugin-sdk-core` dependency, so no `NativeEntry` plugin can be activated on `FakeHost`; add it as a dev-dependency (all SDK-subject cases, LIF-279).
- ilium-plugin-testkit `conformance::DEFAULT_SEED: u64` (DEV-624): the default seed the case host uses is not named anywhere.
- ilium-plugin-testkit `conformance::case(id: &str, title: &str, body: impl FnOnce(&mut FakeHost))` (DEV-624): the project case entry point does not exist.
- ilium-plugin-testkit `conformance::case_in(dir: &Path, id: &str, title: &str, body: impl FnOnce(&mut FakeHost))` (DEV-624): needed to run a case against a manifest.json in a chosen folder.
- ilium-plugin-testkit `conformance::try_case_in(dir: &Path, id: &str, title: &str, body: impl FnOnce(&mut FakeHost)) -> Result<(), HarnessError>` (DEV-624): non-panicking form to assert `harness.manifest-missing`.
- ilium-plugin-testkit `conformance::sample_batch(n: usize) -> Vec<Event>` (DEV-621): not present.
- ilium-plugin-testkit `conformance::assert_within_budget(host: &FakeHost, kind: &str, millis: u32)` (DEV-622): not present.
- ilium-plugin-testkit `conformance::check_within_budget(host: &FakeHost, kind: &str, millis: u32) -> Result<Option<String>, HarnessError>` (DEV-622): non-panicking form so the `needs-component` skip and the bad-kind error can be asserted.
- ilium-plugin-testkit `conformance::is_project_case_id(id: &str) -> bool` (DEV-620): the `PROJ-` prefix rule has no function to call.
- ilium-plugin-testkit `fixture::ui_tree_with_nodes(count: usize) -> UiTree` (CONF-VIEW-03): builds a valid tree of exactly `count` nodes for the 4,096 and 4,097 boundary.
- ilium-plugin-host `testing::HostHarness::with_component(self, bytes: Vec<u8>) -> HostHarness` (DEV-607, CONF-LIFE-02/03, CONF-EVT-02, CONF-MEM-01): the probe is a built component, only `with_wat` and a native guest exist.
- ilium-plugin-host `testing::HostHarness::settings_json(self, json: String) -> HostHarness` (DEV-607): the probe reads its burn plan from the activation settings.
- ilium-plugin-host `testing::HarnessHandle::deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>>` (CONF-LIFE-03, CONF-LIFE-05): no way to deactivate a running harness.
- ilium-plugin-host `testing::HarnessHandle::restore_bytes(&self) -> Option<Vec<u8>>` (CONF-LIFE-03): the kept restore bytes are not readable.
- ilium-plugin-host `testing::HarnessHandle::revoke(&mut self, grant: &str)` (CONF-GRANT-02, CONF-EVT-05): grants can only be set at build time.
- ilium-plugin-host `testing::HarnessHandle::render(&mut self, view: ViewTarget, area: Area) -> Result<UiTree, Trap>` (CONF-VIEW-01..03): views cannot be rendered through the harness.
- ilium-plugin-host `testing::HarnessHandle::click_action(&mut self, target: ViewTarget, key: &str)` (CONF-VIEW-04): no UI action injection.
- ilium-plugin-host `cli::CliEnv::new(root: &Path) -> CliEnv` (DEV-600..643): tests build the struct literal, which breaks when a field is added.
- ilium-plugin-sdk-core `host()` (empty trait): a native plugin cannot call imports, so import-based cases (GRANT-01/03/05, STORE-01 on the SDK subject) run only through the probe component.
- Fixture components not yet built: `conformance-probe`, `conformance-subject`, `conformance-slow`, `conformance-inert`, `conformance-verbose`, `conformance-debug` (contract in `plugins/fixtures/conformance-probe/CONTRACT.txt`); tests fail red with `fixture.missing`.
