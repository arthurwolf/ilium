# API gaps found by the scenes feature tests

Format: crate, signature, requirement id, why.

- ilium-plugin-testkit: `impl FakeScene { pub fn with_location(self, latitude: f64, longitude: f64) -> FakeScene }` - F-100 - stars needs the `location.current` import fake (no location fake exists).
- ilium-plugin-testkit: `impl FakeScene { pub fn with_settings(self, settings_json: &str) -> FakeScene }` - COR-011 - spec settings for a scene.
- ilium-plugin-testkit: `impl FakeScene { pub fn status(&self) -> Option<String> }` - F-100 - stars status strings.
- ilium-plugin-testkit: `impl FakeHost { pub fn extensions(&mut self) -> Extensions }` with `contribute(&mut self, point: &str, id: &str, priority: u8, entry_json: &str)` and `collect(&mut self, point: &str) -> Vec<serde_json::Value>` - COR-010 - extension point tests.
- ilium-plugin-core-scenes: `impl SceneRenderer for ScenesScene` (the type FakeScene::new needs) - F-095..F-101 - skeleton renderer impl.
- ilium-plugin-sdk (testing): `Screen::cell(..).bold` is used for glyph numerals; exists. `assert_golden` takes a path relative to the crate; golden files under tests/features/golden do not exist yet.
- Manual gap: reply shape of `ilium.core.scenes/catalog` `list` and how the method is selected in the request are not specified; tests assume request `{"method":"list"}` and reply `{"scenes":[{id,title,family}],"packs":[...]}`.
- Manual gap: the page sample (no dithering) conflicts with the implementation where scenes also take a `mode` and `palette` in the spec; tests follow the page.
- ilium-plugin-testkit: `impl FakeScene { pub fn frames_per_second(&self) -> u32; pub fn wants_pointer(&self) -> bool; pub fn wants_occupancy(&self) -> bool; pub fn uses_cell_colors(&self) -> bool; pub fn reconfigure(&mut self, settings_json: &str) -> bool }` - COR-011 - renderer query and reconfigure methods (tests/features/renderer_flags.rs).
- ilium-plugin-testkit (ports): the port twins in tests/port reuse tests/features/common.rs and the accessors listed above (frames_per_second, wants_pointer, wants_occupancy, uses_cell_colors, reconfigure, with_settings, status); no further item is needed - PORT:* originals about host/client internals (quota, worker, surface, wikipedia layout) are twinned at the scene-renderer boundary only.
