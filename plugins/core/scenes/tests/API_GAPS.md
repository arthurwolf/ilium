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
- Kernel (G9a, plan `docs/tasks/02-foundation-gaps-plan.md`): no scene-side host exists yet. `tools/gen_plugin_wasm_glue.py` still lists `renderer` in `EXPORTS_WITHOUT_GLUE`; `ilium-plugin-bundle/build.rs` still has `PENDING_SIDES = ["scene"]` and `BUILT_SIDES = ["server", "client"]`; `linker.rs`/`wasm_guest.rs` have no resource table for `surface`, `fonts` and `child`. Wanted: scene-world glue, `BUILT_SIDES` including `scene`, and `BundledPlugin.scene` filled.
- Kernel (G9b): `ilium-ambient/src/external.rs` (trait `ExternalSceneFactory { fn create(&self, kind: &str, env: SceneEnv, controls: &[Control]) -> Option<Box<dyn Scene>> }`), a call to it in `ilium-ambient/src/registry.rs` `create_scene` for unknown kinds, and `ilium-client/src/plugin_host/scene.rs` (`PluginScene: Scene`, forwarding `tick`, `set_palette` and `set_control` to the guest renderer) do not exist. Without them no kernel scene directory (`ilium-ambient/src/scenes/**`) can be deleted: a cut would remove the animation from the product.
- Id clash: `tests/features/render_waves.rs` pins scene `waves` to a plain sine (`tone = sin(0.2 c + 0.1 r - 2 s)`), while the group plan maps the kernel `dithered_waves` scene onto the id `waves`. A move of `dithered_waves` needs its own id (for example `dithered-waves`) or a changed manual sample.
- Imports (G10) the S3 and S4 scenes need on the scene world: `anim/surface`, `anim/fonts` (resource handles), `media`, `codec`, `http`, `location`, `geocode`, `catalogs`, `gpu`. Wired on 2026-11-14 except `catalogs`, which the `ilium:anim/base` world does not import; see "Scene host imports (G10)" below.

## Verified against the tree on 2026-11-14 (visuals group, consumer pass)

- ilium-plugin-testkit `FakeScene` (`ilium-plugin-testkit/src/scene.rs`): the accessors above now exist but are stubs. `FakeScene::new` builds the renderer from `"{}"` (:62-64) and `spec_json()` (:79) adds only the settings and the location, so the scene id never reaches `NativeRenderer::new`; `reconfigure` (:155) is always false, `uses_cell_colors` (:175) and `wants_pointer` (:165) always false, `status` (:180) always `None`. Wanted: `spec_json` carries `"scene": <scene_id>`, and the native contract (`ilium-plugin-sdk-core/src/plugin.rs` `NativeRenderer`) gains `reconfigure(&mut self, &str) -> bool`, `status(&self) -> Option<String>`, `uses_cell_colors`, `wants_pointer` and `wants_occupancy`, with `FakeScene` forwarding to them. Needed by `tests/features/renderer_flags.rs`, `renderer.rs` (unknown scene id) and the stars status test.
- `NativeSurface` (`ilium-plugin-sdk-core/src/plugin.rs:281`): a cell is `(char, u32, u32)`, so the glyph-plane numerals `"12"` and the bold flag asserted by `tests/features/render_clock.rs` cannot be written or read. Wanted: a glyph cell holding a `String` and a modifier set (the `GlyphCell` of page sample 2).
- Kernel G9a and G9b are still absent: `ilium-plugin-bundle/build.rs:40` `PENDING_SIDES = ["scene"]`, `tools/gen_plugin_wasm_glue.py:43` `EXPORTS_WITHOUT_GLUE = ("renderer",)`, no `ilium-ambient/src/external.rs`, no `ilium-client/src/plugin_host/scene.rs`; `ilium-ambient/src/registry.rs:654` and :661 are a closed match over `AmbientKind`. Nothing of `ilium-ambient/src/scenes/**` can be cut. See `docs/tasks/02-cuts/scenes.md` keys S1 to S8.

## Scene host imports (G10), verified against the tree on 2026-11-14

Every plugin scene instance is now equipped by `ilium-plugin-host/src/wiring/scene.rs`
(`SceneImportServices::equip`, :169; shared part `base_set`, :226), built by the client at
`ilium-client/src/plugin_host/host.rs:1811` and used for each scene at
`ilium-client/src/plugin_host/plugin_scenes.rs:262`. Only grants of `capabilities.scene` whose
catalogue row allows the scene side count (`scene_grants`, :283); only the 15 interfaces of
`ilium:anim/base` are ever linked (`keep_scene_interfaces`, :295); then everything the grants do
not unlock is removed, again inside `SceneGuest::instantiate` (`scene_guest.rs:738`), and a
component importing an ungranted interface does not start. `fs`, `http`, `audio`, `media`, `gpu`
and `location` also check their grant on every call; `jobs` and `geocode` rely on the link-time
removal only. What a scene guest can call now:

| Interface | Functions | Needs | Backed by |
|---|---|---|---|
| `log`, `clock`, `codec`, `anim/fonts` | all (`codec`: `inflate`, `deflate`, `get-image-info`, `decode-image`, `sha256`, `hmac-sha256`, `constant-time-equal`, `rasterize-text`) | nothing | `SinkLog` to the client log; `ClockImport`; `CodecImport`; `FontsImport` |
| `anim/surface` | all | nothing | `SceneSurface` (unchanged) |
| `storage` | `get`, `put`, `remove`, `keys`, `write-batch`, `lease` | nothing | the plugin's own scene-side buckets under `<data>/plugins/storage` (`wiring::import_provider`) |
| `fs` | `roots`, `metadata`, `list-dir`, `read`, `read-all`, `scan`, `write`, `write-atomic`, `read-with-revision`, `tail`, `create-dir`, `remove`, `rename`, `lock`, `unlock`, `take-bytes`, `take-entries`, `take-lock`, `take-revisioned`, `take-written` | `fs.read:<scope>` / `fs.write:<scope>` | private `package` (read only), `data`, `cache` under `<data>/plugins/fs/<plugin>/`; `minecraft-saves` read only at `<minecraft java dir>/saves` (`host.rs:1822`, `scene.rs:134`). No `project`, `picked`, agent scopes, `watch`/`unwatch`, `pick` or `propose-agent-config` (not scene grants). |
| `http` | `fetch`, `take-response`, `open-stream`, `close-stream`, `open-websocket`, `send-websocket` | `http:<host>` | render instance: `take-response` never waits (`scene.rs:197`); job instances: waiting `take-response` (blocking fetch) |
| `jobs` | `start`, `cancel`, `take-output`, `wait-request`; inside a job `report-progress`, `is-cancelled` | `jobs.run` + `contributes.jobs` | `SceneBackground::start_queued_jobs` (`scene.rs:349`) runs each job in a fresh instance of the same component on its own thread with the job's deadline and memory cap, called before every frame by `SceneGuest::pump_background` (`scene_guest.rs:980`, `plugin_scenes.rs:448`); dropping the scene cancels its jobs (`scene.rs:464`). `wait-request` in the render instance blocks the frame up to its timeout, so a scene should poll `take-output` instead. |
| `audio` | `devices`, `open-capture`, `open-loopback`, `close-capture`, `play` | `audio.capture` / `audio.loopback` / `audio.playback` | `ClientAudio` |
| `media` | `open-video`, `open-video-bytes`, `open-video-range`, `next-frame`, `seek`, `close-video` | `media.video` (+ `fs.read:<scope>` for the file) | `FfmpegMedia`: one ffmpeg child per open video. This is the only way a scene reaches ffmpeg. |
| `location` | `current` | `location.observer` | the shared location row (`ObserverLocation`, `imports/location.rs:48`; `plugin_scenes.rs:248`). Without the grant the spec carries no `location` either. |
| `geocode` | `search`, `reverse`, `take-places` | `location.geocode` | `GeocodeImport` (answers no places yet; no geocoding provider is wired) |
| `gpu` | `run`, `available` | `gpu.kernels` (unstable) | `GpuImport` CPU kernels |
| `telemetry` | - | - | not equipped: calls trap with `host.engine-error` |

Events: audio frames, request completions and job progress reach a scene only if it exports
`ilium:exports/handler@0.1.0` (`SceneGuest::deliver_events`, `scene_guest.rs:1006`); otherwise
they are counted as dropped and the scene polls (`take-response`, `take-output`, media frames).

Tests: `ilium-plugin-host/src/wiring/scene.rs` (grant filtering, world restriction, private fs
scopes per plugin, `minecraft-saves` with and without its grant, jobs launch and cancel on drop),
`ilium-plugin-host/src/imports/location.rs` (grant per call, range), `ilium-plugin-host/tests/scene_guest_boundaries.rs`
(an equipped scene pumps and draws; a WAT scene whose `status` calls `location.current`),
`ilium-client/src/plugin_host/plugin_scenes.rs:782` and :804 (the same through the client factory).
Compiled and run: not yet (edits only in this pass).

Still not reachable, by design of the manual or the WIT, not by missing host code:

- `ilium:anim/base` exports no `handler`: audio frames (spectrum) need either that export added
  to the world (WIT change in `reference-wit.md`) or a polling `audio` function.
- No `process` import in the scene world and `process.run` is not a scene grant: ffmpeg is reached
  only through `media`; a scene cannot spawn processes.
- `SavedRuntime` (the history-writer fence of the kernel minecraft scenes) is host-internal; a
  scene gets the same data through `fs.read:minecraft-saves` + `codec.inflate` + `jobs`.
- Shared live data (Graph, Earthquakes, Aircraft, Boats) and the clouds providers of
  `ilium_ambient::animation_services`: no `topics`/`services`/`catalogs` import in the scene
  world; a scene fetches with `http:<host>` and keeps results in its `cache` scope and storage.
  The provider algorithms (`animation_services::satellite`, tile URLs) are pure and can be
  compiled into the guest.

Live data (Graph, Earthquakes, Aircraft, Boats) now runs inside this plugin (`src/live_data/`),
fetching with `http:<host>` grants and polling one non-blocking step per frame. What the scene
world cannot do, and what the scenes therefore do instead:

- No stream import: the Wikipedia graph sources (edit rate, bot share) cannot read the SSE feed.
  The aggregator and its tests are kept; the scene reports `STREAM_UNAVAILABLE` in its status.
- Host http bodies are capped at 8 MiB; the OpenSeaFeed fleet snapshot can be larger (the kernel
  allowed 32 MB). The scene reports the size error; it does not silently switch provider.
- No shared disk snapshot cache across processes and no worker threads: the request floors are
  kept in the plugin's shared storage (`live-data/<name>`), and fleet markers are prepared in the
  frame when the data or the viewport changed.

Voxel landscape: the port was reverted; the scene stays in the kernel (`ilium-ambient/src/scenes/voxel_landscape`) because the saved-world module `ilium-ambient/src/minecraft` depends on it. A guest port needs the gaps below first:

- A scene component cannot export `job-runner` and cannot call `ilium_plugin_sdk::host()`:
  `ilium-plugin-sdk-macros/src/component.rs::scene_body` rejects exports and
  `__ilium_install_host()` is a no-op for scenes. The voxel engine needs a background job
  (generate a clip with `GeneratedViewportSession`, poll `jobs.take_output`) because a frame has
  a 33-50 ms deadline and wasm32-wasip2 has no threads.
- Pack and world file reads (`assets/source.rs`, `directory_snapshot.rs`, `pack_sources.rs`,
  `pack_registry.rs`) still use `ilium_platform::secure_fs::NoFollowDirectory`; they need a shim
  over the scope-based guest `fs` (`FsPath{scope,path}`, reads of at most 1 MiB, scopes
  `minecraft-saves` and the new `minecraft-java`).
- `engine.rs` still uses the kernel `Worker`, `env.resources` and `cache_dir`, which the kit
  `SceneEnv` (palette, location) does not carry.

Voxel landscape, chain check (cluster scenes3, 2026-11-14, edits only). Nothing was moved or deleted; the kernel copy is intact. The slice is 66 files, 52,371 lines (`voxel_landscape/assets/compatibility.rs` through `surface_viewport.rs`), and it cannot leave the kernel alone, because the rest of the chain still holds it:

- `ilium-ambient/src/minecraft/**` (38,512 lines, another quarter) imports `voxel_landscape::assets`, `catalog`, `surface_*`; `ilium-ambient/src/scene.rs` (`SavedWorldFrameEvidence`, `SavedWorldSource`, `Scene::saved_world_*`, `readiness`) and `ilium-client/src/background_animation/{host,mod}.rs` (`SavedRuntime`) are typed against both. The chain moves as one piece or not at all.
- (Fixed by cluster scenes1, 2026-11-14, unverified: `scene_body` now emits per-grant host-call macros and installs the backend with `__ilium_install_host()`, so `host()` works in a real scene component; the text below describes the state before the fix.) `ilium-plugin-sdk-macros/src/component.rs::scene_body` emitted `fn __ilium_install_host() {}` and no host-call macros, although `generated_glue.rs` and `wasm/glue.rs` already carry `(scene $host)` arms and `job-runner` lists the `scene` side. So `ilium_plugin_sdk::host()` has no backend inside a real scene component: the `http`/`storage` calls of `live_data` and `live_chess` return the absent answer there (they only work under FakeHost). The fix mirrors `body()` (grants of side `scene`, `HOST_CALLS` loop, `set_host`, optional `job-runner` export). It must be done together with a check that every grant of this manifest (`gpu.kernels`, `media.video`, `location.observer`, the `http:` hosts) is linked by `ilium-plugin-host/src/wiring/scene.rs`, otherwise instantiation of the whole scenes component fails.
- The voxel engine needs a worker (`engine.rs` `Worker`, `resources`, `cache_dir`), 5 call sites of `ilium_platform::secure_fs` in `assets/{source,directory_snapshot}.rs`, `pack_sources.rs`, `pack_registry.rs`, and `std::thread` in `surface_retirement.rs`; its clip output (`GeneratedViewportSession`) has to become serialisable job output.


Satellite clouds (cluster scenes1, 2026-11-14, edits only, uncompiled): ported to `src/clouds/` and removed from the kernel. The scene owns no thread: `clouds/refresh.rs` is a per-frame state machine over the host `http` import (one decode per poll, per-request deadline from `Frame.now`). Frames are kept in memory only; the kernel's disk cache of tiles is gone. The setting key `history_hours` became the string choice `history` (`latest`, `6h`, `12h`, `24h`).

Earth at night (cluster scenes2, 2026-11-14, edits only, uncompiled): ported to `src/night_lights/` and removed from the kernel (`ilium-ambient/src/scenes/night_lights*` and `animation_services::satellite` are gone, `AmbientKind::NightLights` and `AnimationKind::NightLights` too; a saved `night_lights` selection loads as the plugin scene kind through a serde alias). The scene owns no thread: `loader.rs` is a per-frame state machine over the host `http` import (probe, collect, one decode per step, banded assembly through `kit::geo_tiles::fill_grid_rows`). Tiles are cached in the plugin's shared storage bucket (rebuildable, values of at most 1 MiB); the kernel's disk cache with a size budget and the worker-panic and admission-refusal paths do not exist in the guest. The manifest grants `http:gibs.earthdata.nasa.gov`.
