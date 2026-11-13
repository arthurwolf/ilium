# API gaps found by the sdk-fakehost lane, part 1 (DEV-400 to DEV-517)

Each bullet: crate, exact signature or item, requirement id, one line why. Tests behind a cfg feature
name that feature; the implementation phase declares the feature in `ilium-plugin-testkit/Cargo.toml`
when the item exists.

- ilium-plugin-sdk-core: `pub trait HostApi: HostCalls + HostFs + HostHttp + HostProcess + HostStorage + HostTimers + HostJobs + HostCli` (today `HostApi` is an empty trait); DEV-416, DEV-450, DEV-455, DEV-456, DEV-460, DEV-462, DEV-463, DEV-464, DEV-465, DEV-466, DEV-472, DEV-473: a native plugin cannot call a host import, so every row driven by a plugin call (timers firing, scope checks, unexpected-call, quota, jobs, `import-not-requested`) cannot be written. Gated module `host_calls` (feature `plugin_host_calls`) holds DEV-455 and DEV-463.
- ilium-plugin-testkit: Cargo feature `plugin_host_calls = []`; DEV-455, DEV-463: gates `reference_test_harness_gated::host_calls`.
- ilium-plugin-testkit: Cargo feature `plugin_views = []`; DEV-441, DEV-442, DEV-444, DEV-445, DEV-446: gates the render-content tests in `reference_test_harness_views.rs`.
- ilium-plugin-testkit: Cargo feature `plugin_session = []`; DEV-480, DEV-481, DEV-482: gates `reference_test_harness_gated::session_routing`.
- ilium-plugin-sdk-core: `pub trait Views { fn render(&mut self, target: ViewTarget, columns: u16, rows: u16) -> Screen; }` reachable from `NativeEntry` (for example `const VIEWS: bool` plus a `native_render` method); DEV-441, DEV-442, DEV-444, DEV-445: `FakeHost::render` has no plugin method to call, so screen content cannot be asserted.
- ilium-plugin-testkit: `pub fn FakeSession::install_native<P: NativeEntry + Default>(&mut self, package_dir: impl AsRef<Path>) -> &mut FakeSession`; DEV-480, DEV-481, DEV-482, DEV-486, DEV-487, DEV-488: `install(package_dir)` has no way to bind a native plugin type to a package, so routing between plugins cannot be exercised.
- ilium-plugin-testkit: `pub fn conformance::assert_within_budget(...)` (signature per reference-test-harness.md "Modes"); DEV-404: not present in the skeleton.
- ilium-plugin-testkit: a built component fixture (`dist/server.wasm`) for the component-mode rows; DEV-401, DEV-402, DEV-403, DEV-423: only the native half is tested.
- ilium-plugin-testkit: builder methods fail with a panic that carries the dotted code, there is no `Result` channel (`FakeHost::grant`, `seed`, `with_storage_snapshot`, `with_data_version`, `fs().write`, `inject_trap`, `set_clock`); DEV-410 to DEV-417, DEV-458, DEV-495, DEV-497: tests use `catch_unwind` and read the panic text. A `try_` variant returning `Result<_, HarnessError>` would remove that.
- ilium-plugin-testkit: `pub fn FakeHost::clock_start(self, text: &str) -> FakeHost` (the builder for the start instant, format `YYYY-MM-DD HH:mm:ss` UTC); DEV-414, DEV-451, DEV-510: only `set_clock(u64)` exists, so "the builder wins over the fixture clock" cannot be tested; the grants variant is used instead.
- ilium-plugin-testkit: `FakeHost::mode(self, Mode)` is the builder and `current_mode(&self)` the reader; the manual calls the reader `mode()`; DEV-400 to DEV-404.
- ilium-plugin-testkit: `FakeHost::send_action` takes `ActionValue`, not `Option<ActionValue>`; DEV-443: an action without a value cannot be sent.
- ilium-plugin-sdk-core: `events::activity(node, prev, cur)` is a free function with no host, so the per-host revision counter of DEV-447 cannot be tied to a host.
- ilium-plugin-sdk-core: `ProcessExit.signal` is `Option<String>` in the generated type; reference-test-harness.md "Fake process" shows `Option<i32>`; DEV-460.
- ilium-plugin-sdk-core: `ViewTarget` has no scene-id variant; DEV-446 covers five targets only.
- ilium-plugin-testkit: `FakeHost::deactivate` has no default-reason overload (`deactivate()` meaning `shutdown`); DEV-424.
- ilium-plugin-testkit: a PluginError with an error-code name outside the 15-value enum cannot be built, so the negative case of `inject_failure` (DEV-493) tests a code outside the import's catalogue instead of an unknown name; an `inject_failure_named(&str import, &str code, &str message)` would cover the manual's wording.
- ilium-plugin-testkit: `assert_snapshot` writes to `tests/snapshots/` relative to the working directory; DEV-504: the tests write `sdk_fakehost_504_*.json` there; the lane owner should add that folder to the cleanup list or give `assert_snapshot` a base-dir override.
- ilium-plugin-testkit: dev-dependencies are enough (proptest, serde_json via normal deps); no missing dev-dependency.
