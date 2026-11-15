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
| `fs` | `roots`, `metadata`, `list-dir`, `read`, `read-all`, `scan`, `write`, `write-atomic`, `read-with-revision`, `tail`, `create-dir`, `remove`, `rename`, `lock`, `unlock`, `take-bytes`, `take-entries`, `take-lock`, `take-revisioned`, `take-written` | `fs.read:<scope>` / `fs.write:<scope>` | private `package` (read only), `data`, `cache` under `<data>/plugins/fs/<plugin>/`; `minecraft-saves` read only at `<minecraft java dir>/saves` (`host.rs:1822`, `scene.rs:134`). Read-only `picked/<n>`: the paths of the scene's `picker` settings rows, lent per instance in row order on `fs.read:picked` (scenehost pass below). No `project`, agent scopes, `watch`/`unwatch`, `pick` or `propose-agent-config` (not scene grants). |
| `http` | `fetch`, `take-response`, `open-stream`, `close-stream`, `open-websocket`, `send-websocket` | `http:<host>` | render instance: `take-response` never waits (`scene.rs:197`); job instances: waiting `take-response` (blocking fetch) |
| `jobs` | `start`, `cancel`, `take-output`, `wait-request`; inside a job `report-progress`, `is-cancelled` | `jobs.run` + `contributes.jobs` | `SceneBackground::start_queued_jobs` (`scene.rs:349`) runs each job in a fresh instance of the same component on its own thread with the job's deadline and memory cap, called before every frame by `SceneGuest::pump_background` (`scene_guest.rs:980`, `plugin_scenes.rs:448`); dropping the scene cancels its jobs (`scene.rs:464`). `wait-request` in the render instance blocks the frame up to its timeout, so a scene should poll `take-output` instead. |
| `audio` | `devices`, `open-capture`, `open-loopback`, `close-capture`, `play` | `audio.capture` / `audio.loopback` / `audio.playback` | `ClientAudio` |
| `media` | `open-video`, `open-video-bytes`, `open-video-range`, `next-frame`, `seek`, `close-video` | `media.video` (+ `fs.read:<scope>` for the file) | `FfmpegMedia`: one ffmpeg child per open video. This is the only way a scene reaches ffmpeg. |
| `location` | `current` | `location.observer` | the shared location row (`ObserverLocation`, `imports/location.rs:48`; `plugin_scenes.rs:248`). Without the grant the spec carries no `location` either. |
| `geocode` | `search`, `reverse`, `take-places` | `location.geocode` | `GeocodeImport::with_backend(ClientGeocoder)`: OSM address search (Photon) then Open-Meteo place search, on a worker thread; `reverse` has no provider and completes without places (scenehost pass below) |
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

- (Resolved in the scenehost pass, 2026-11-14: `ilium:anim/base` has the optional fragment
  `with-handler`, so a scene listing `handler` in its manifest `exports` receives audio frames.)
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
- Re-check (cluster scenes3, round 10, edits only): the dependency is two-way. `voxel_landscape/settings.rs` and `surface_binding.rs` name `minecraft::` (5 references), and 21 files under `ilium-ambient/src/minecraft/` name `voxel_landscape`, so the two trees are one scene kind (`registry.rs` picks the saved-map or the procedural scene under `AmbientKind::VoxelLandscape`). The path-order quarter for scenes3 is 55 `.rs` files (52,327 lines, `assets/metadata.rs` through `tree_decoration_profiles.rs`); `minecraft/` sorts into an earlier quarter and `tree_decorations.rs` onward into a later one, so no single quarter can delete its share. No other cluster had started the chain at this check.
- The voxel engine needs a worker (`engine.rs` `Worker`, `resources`, `cache_dir`), 5 call sites of `ilium_platform::secure_fs` in `assets/{source,directory_snapshot}.rs`, `pack_sources.rs`, `pack_registry.rs`, and `std::thread` in `surface_retirement.rs`; its clip output (`GeneratedViewportSession`) has to become serialisable job output.


Satellite clouds (cluster scenes1, 2026-11-14, edits only, uncompiled): ported to `src/clouds/` and removed from the kernel. The scene owns no thread: `clouds/refresh.rs` is a per-frame state machine over the host `http` import (one decode per poll, per-request deadline from `Frame.now`). Frames are kept in memory only; the kernel's disk cache of tiles is gone. The setting key `history_hours` became the string choice `history` (`latest`, `6h`, `12h`, `24h`).

Earth at night (cluster scenes2, 2026-11-14, edits only, uncompiled): ported to `src/night_lights/` and removed from the kernel (`ilium-ambient/src/scenes/night_lights*` and `animation_services::satellite` are gone, `AmbientKind::NightLights` and `AnimationKind::NightLights` too; a saved `night_lights` selection loads as the plugin scene kind through a serde alias). The scene owns no thread: `loader.rs` is a per-frame state machine over the host `http` import (probe, collect, one decode per step, banded assembly through `kit::geo_tiles::fill_grid_rows`). Tiles are cached in the plugin's shared storage bucket (rebuildable, values of at most 1 MiB); the kernel's disk cache with a size budget and the worker-panic and admission-refusal paths do not exist in the guest. The manifest grants `http:gibs.earthdata.nasa.gov`.

## Scenehost pass (2026-11-14, edits only, uncompiled)

Host APIs added so the remaining kernel scenes can move; none of the scenes was moved in this pass.

- Handler export on the anim world (spectrum). `reference-wit.md` adds `world with-handler { export
  ilium:exports/handler@0.1.0; }` beside `with-jobs`; `wit/`, `generated/*`, `glue.rs`,
  `generated_glue.rs` (handler now lists the `scene` side), `generated_dispatch.rs`,
  `host_handle.rs` and `bridge_defaults.rs` were regenerated by `tools/extract_wit.py` and
  `tools/gen_plugin_api.py`. The manifest schema accepts `"exports": ["job-runner", "handler"]` on a
  scene (`validate.rs`, `reference-manifest-schema.md`, `reference-manifest.md`,
  `worlds-and-sides.md`, `api-exports.md`). The host already delivered events to a handler
  (`SceneGuest::pump_background`). The audio capture import was already wired (`ClientAudio`).
  Open: the SDK side of `scene_body` (`ilium-plugin-sdk-macros/src/component.rs`, owned by another
  pass) dispatches the handler export to the default lifecycle value, not to the scene's
  renderer program, so an SDK scene cannot yet read the frames in its renderer.
- Folders and files for images and video. A scene settings field with the `picker` keyword
  (`"file"` or `"directory"`) is lent, in row order, as the read-only scope `picked/<n>` on
  `fs.read:picked` (`scene_controls.rs::picked_paths`, `plugin_scenes.rs` `build`,
  `wiring/scene.rs::lent_picks`, `imports/fs.rs::extra_scope_root`). `list-dir` lists a folder, a
  file is read on the empty path, writes are refused. Changing the field rebuilds the scene.
  Documented in `capability-fs-pick.md`. The row is a typed path: the client has no file dialog
  on scene settings rows yet. Image decoding is the existing `codec` import; video frames are the
  existing `media` import (`FfmpegMedia`), which now reaches a picked file.
- Geocode. `GeocodeImport` takes a `GeocodeBackend`; requests run on a worker thread, answers wait
  for `take-places`, a failure leaves `none`, one provider request per second per instance
  (`rate-limited` with `retry-after-ms`). The client lends `ClientGeocoder`
  (`ilium-client/src/plugin_host/geocode_backend.rs`): OSM address search (Photon, default
  address settings) then the Open-Meteo place search. Open: no reverse provider; the per-plugin
  consent prompt of `capability-location-geocode.md` is not implemented; `near` and `language`
  are not passed to the providers; the manual's Nominatim fallback needs a user endpoint and is
  not used. No `request-completed` outcome is posted, so a failed request (and every `reverse`)
  looks like a pending one to a polling scene. The OSM address search shares the kernel location
  picker's process-wide admission gate (one request at a time, 60 s cooldown after a 429 or 503),
  so a scene that geocodes can make the user's picker answer "already running" or a cooldown.
- Not done:
  - The location picker is still special-cased on `AnimationKind::OpenStreetMap`
    (`location_picker.rs`, `ui.rs`, `animation_settings_ui.rs`), because the OpenStreetMap scene is
    still a kernel scene. `ExternalSceneFactory::uses_location` already lets a plugin scene ask
    for the shared location row.
  - Saved-world host API (minecraft, voxel landscape). `SavedRuntime` is a history-writer fence
    that has to outlive one scene instance, while a scene's background (jobs) is cancelled when
    the instance drops. The kernel scenes also use `SceneEnv::resources`, `SceneEnv::cache_dir`,
    `secure_fs`, the kernel `Worker` and `std::thread`. The scene world has none of these; only the
    per-instance `cache` scope and `jobs` exist. Moving them needs a host-owned saved-world
    service that outlives instances, plus a design decision in the manual. That was not attempted
    here.

## Scenes1 pass (ilium-ambient infrastructure and `minecraft/`), 2026-11-14, edits only, uncompiled

First quarter of the inventory's surviving scene files: `ilium-ambient/examples/scene_probe.rs`, `src/{animation_services,control,debug,dither,geocode,gpu,lib,location,prepared_cache,raster,registry,resources,scene}.rs` and all 52 files of `src/minecraft/` (39,675 lines).

- Deleted: `ilium-ambient/src/animation_services.rs` (the `osm` re-export module) and its `pub mod` line. Nothing in the workspace named it, so no call site changed.
- Kept by design (a kernel duty or a live kernel consumer): `control`, `dither`, `raster`, `registry`, `scene`, `gpu`, `lib` (scene contract, shared look, GPU adapter); `location` and `geocode` (`ilium-client/src/location_picker.rs:242` and the plugin `ClientGeocoder`); `resources` (`ilium-client/src/background_animation/cache.rs`, `ilium-wikipedia`); `debug` and `examples/scene_probe.rs` (the headless probe that still drives the kernel images, video, openstreetmap and voxel scenes; its pixel tests are mapped to this plugin in `tools/test_port_map.tsv` and stay until those scenes move).
- Kept, blocked: `prepared_cache.rs` is used only by `minecraft/` (`saved_scene.rs:957`, `projected_route.rs:76`), and all of `minecraft/` imports `scenes::voxel_landscape::{assets, catalog, surface_*}` while `ilium-ambient/src/scene.rs` (`SavedWorldSource`, `SavedWorldFrameEvidence`, `Scene::saved_world_*`) and `ilium-client/src/background_animation/host.rs` (`SavedRuntime`) are typed against both. The chain `minecraft` + `voxel_landscape` + `prepared_cache` moves as one piece or not at all, and it needs the host-owned saved-world service described above (history-writer fence that outlives scene instances, a `Worker`, `secure_fs`, `std::thread` replacements). A partial move would leave a half-ported feature, so the kernel copy is untouched.

## Scenes2 pass (Images moved), 2026-11-15, edits only, uncompiled

Second quarter of the inventory's surviving scene files. Only the `images` scene could move; the
other scene directories stay in the kernel for the reasons below. Nothing was compiled or tested.

- Moved: `ilium-ambient/src/scenes/images/` (19 files) to `plugins/core/scenes/src/images/`
  (settings, motion, slideshow, adjust, picture, source, loader, lru, render, tests), registered
  as scene `images` in `manifest.json` with grants `fs.read:picked`, `http:creator.nightcafe.studio`
  and `http:images.unsplash.com`. The kernel `AmbientKind::Images`, `ImagesSettings`,
  `AnimationKind::Images`, the `project_config` static-image migration, the AN-17 settings-help row
  mapping, the semantic recommender's Images branches and `fdeflate` are gone. A saved `images`
  selection loads as the plugin scene kind (serde alias); its old per-scene values are not carried
  over (no migration shim, as with spectrum).
- Features that changed on the move, because the scene world cannot express them:
  - Colour rows (preset, brightness, contrast, saturation, hue tint, intensity, opacity): dropped,
    the shared look owns colour for every scene.
  - URL and URL-list modes and custom image URLs: dropped. A `http:<host>` grant names one fixed
    host, so only the four built-in pictures (two hosts) are reachable.
  - `;`-separated folders and globs: replaced by three directory picker rows
    (`folder`, `folder_2`, `folder_3`) lent as read-only `picked/<n>` scopes. Glob patterns have
    no equivalent.
  - The on-disk download cache and the 32 MiB / 20 000 image scan limits: the scene keeps decoded
    pictures in an in-memory LRU and limits a picture to 8 MiB encoded and 16 megapixels.
  - One picture is loaded at a time (no worker pool): the scene steps a loader slice by slice
    each frame.
- Manifest limit used: `visible_when` names one controlling key, so `recursive`, `order`,
  `shuffle_seed` and `transition_seconds` key on `mode` alone, and `motion_strength_percent` and
  `easing` stay visible when Motion is None.
- Kept in the kernel, with the blocker:
  - Voxel landscape and `minecraft/`: tied to `SavedRuntime`, `SceneEnv::resources`/`cache_dir`
    and the kernel `Worker` (see the saved-world item above).
  - Video: the `media` import opens local files only (`fs.read` scope), with no URL, glob or
    series playlist, and `http` responses are capped at 8 MiB, so Germination and URL playback
    cannot be reproduced.
  - OpenStreetMap: the location picker and the shared location row are special-cased on
    `AnimationKind::OpenStreetMap` (see "Not done" above), and the scene reads extracts and
    Overpass answers larger than the `http` cap.
- Semantic recommendations can no longer name Images; authored resources are Video only.

Voxel landscape and saved-world scene, round 11 (cluster scenes3, edits only). Not moved: the kernel copy is intact and the plugin tree holds no voxel code (the line in `changelog.d/agent-plugins-system-wasm-scenes6.md` that said otherwise was wrong and is removed). The scenes3 quarter is the 56 `.rs` files `assets/models.rs` through `tree_geometry.rs` of `ilium-ambient/src/scenes/voxel_landscape`, but the kernel consumers outside the quarters are typed against the whole chain and cannot be redirected by one quarter: `ilium-ambient/src/{registry,scene,lib}.rs` and `scenes/mod.rs` (`VoxelLandscapeScene`, `SavedScene`, `SavedWorld*` types), `ilium-ambient/tests/unit_gaps.rs`, the examples `voxel_landscape_probe.rs` and `saved_scene_capture_probe.rs`, and `ilium-client/src/background_animation/{mod,host}.rs` (`SavedRuntime`). Deleting a quarter without the other three leaves `mod.rs` naming missing files. A port needs, in one step by one owner: (1) the whole `voxel_landscape/` plus `minecraft/` trees (about 113,000 lines) under `plugins/core/scenes/src/`, rewritten over the guest `fs` scopes `minecraft-saves` and `picked/<n>` instead of `ilium_platform::secure_fs`; (2) the engine `Worker` and `std::thread` retirement replaced by a `job-runner` export plus `jobs.take_output`, with the generated clip as serialisable job output; (3) the disk caches (`prepared_cache`, `minecraft/tile_cache`) moved to the plugin `cache` scope; (4) `AmbientKind::VoxelLandscape`, `SavedRuntime` and the `Scene::saved_world_*` hooks removed from the kernel and the saved settings alias-loaded as a plugin scene kind. It needs a compiler in the loop, so it was not attempted blind.

## Scenes1 pass, round 11 (ilium-ambient infrastructure and `minecraft/`), edits only

Re-checked the first quarter (58 surviving files). Nothing could be deleted: the kernel `AmbientKind` now has only `Video`, `Plugin`, `VoxelLandscape` and `OpenStreetMap`, and everything left in this quarter is either a kernel duty (`control`, `dither`, `gpu`, `lib`, the shared `location`/`geocode` used by `location_picker.rs`, `debug`) or part of the `minecraft/` + `voxel_landscape` chain. That chain is still tied to a saved-world service that outlives scene instances (history-writer fence), the kernel `Worker`/`std::thread`, `secure_fs` and `SceneEnv::resources`/`cache_dir`; none of these exists on the scene world, and the chain spans quarters 1 to 3 (`minecraft/` 38k lines, `voxel_landscape/` 52k+ lines), so it moves as one piece or not at all. Video and OpenStreetMap belong to quarter 2. Only fix made: the stale usage line of `ilium-ambient/examples/scene_probe.rs`.

## Scenes4 pass, round 11 (last quarter of the scenes inventory, and ilium-ambient), edits only

Nothing could be deleted from the 59 surviving files of the last quarter or from the remaining ambient files, and nothing was moved. The files are the voxel and `minecraft/` chain (see the scenes3 and scenes1 notes above), the shared look and settings tab of `ilium-ambient` that the kernel Settings UI and draw path call synchronously, the gpu adapter, and the location picker with `source`, `worldmap` and `geocode`, which `Video` and `OpenStreetMap` still use.

The one self-contained feature in the quarter is Wikipedia (`ilium-wikipedia` crate plus `ilium-client/src/background_animation/wikipedia/`). A port was started and withdrawn, because it cannot be finished without a compiler: (1) `render.rs` (about 2,000 lines of cosmic-text and swash layout) must become a resumable builder that lays out a few blocks per frame, since a frame has a 50 ms hard deadline and wasm32-wasip2 has no threads; (2) the guest has only the bundled Cascadia font, so the layout needs a re-tuned font set; (3) it is unverified that `cosmic-text` 0.14 (`default-features = false`, `std` and `swash`) builds for wasm32-wasip2, and html5ever's parser is not `Send`; (4) the loader needs a state machine over `http.start` and `take-response` with the redirect, rate-limit and cooldown policy of `ilium-wikipedia/src/fetch.rs` (the host never follows redirects), and the cached-article offline fallback of `loader.rs` has no guest equivalent beyond the 1 MiB shared storage bucket; (5) the kernel side to delete afterwards is `AnimationKind::Wikipedia` (to become a serde alias of `PluginScene`), `AnimationSettings::wikipedia`, the `is_wikipedia` and `WikipediaPresentation` frame fields, the overlay in `background_composition.rs`, and the sites in `animation_rows.rs`, `animation_settings_ui.rs`, `animation_visibility.rs`, `semantic_animation.rs`, `app_semantic_animation.rs`, `introduction.rs`, `settings_help` and the client tests. The grants would be `http:en.wikipedia.org`, `http:upload.wikimedia.org` and `http:thumb.wikimedia.org`. The kernel copy is untouched.

## Scenes2 pass, round 12 (Video guest scene), edits only

Video now exists as a scene of the Scenes plugin (`plugins/core/scenes/src/video/`, manifest entry `video`, registry id `video`, grants `media.video`, `fs.read:picked` and `http:upload.wikimedia.org`). It is written but was not compiled or run (no cargo in this pass); the unit tests in `video/*.rs` and `video/tests.rs` are the first thing a compiler round must run. The kernel copy (`ilium-ambient/src/scenes/video/`, `AmbientKind::Video`, `AnimationKind::Video`, `AmbientSettings::video`) is still in place, so nothing was deleted: the scene picker shows two Videos until the kernel cut is made.

Host gap closed in this pass: a scene must not block inside a frame, but `next-frame` waited up to the decoder timeout. `FrameSource` gained `poll_frame` (`FramePoll::{Frame, End, Pending}`), `MediaImport::without_waiting()` makes `next-frame` answer `timed-out` ("no frame ready yet") without stopping the decoder, `wave_two.rs` applies it to the scene side only, and `ilium-client`'s `FfmpegRun` implements `poll_frame` with `try_recv`. No WIT change: `timed-out` on `next-frame` means "ask again next frame" for scenes.

What the guest scene gives up compared with the kernel Video (each is a platform limit, not a choice):
- URL playback, `;`-separated lists and globs: the `media` import opens `picked/<n>` files and clips held in memory only. Sources are one picked file or up to three picked folders.
- Germination: 45 of 51 clips. `open-video-bytes` takes at most 16 MiB and `http` answers at most 8 MiB, so the scene downloads in 4 MiB `Range` requests and keeps only clips of at most 16 MiB (`assets/germination.json`, hashes pinned and checked through `codec.sha256`); the six larger clips are dropped. A clip is downloaded again each time it plays (no cache).
- The brightness, contrast, gamma and invert rows: colour belongs to the shared look.
- ffmpeg-side filtering (`fps`, `setpts` slowing, `unsharp`): the host decodes at the clip's own rate, the scene thins frames by the clock, slows by stretching due times, and sharpens in the guest (a 3x3 unsharp mask, so Detail costs guest CPU).
- Random scenes seek with `seek` and stop by frame count; the kernel's ffmpeg `-ss/-t` restart is not needed.

Why the kernel cut was not made in this pass (kept for the round that has a compiler):
- `AnimationKind::Video` is the stand-in kind of about 45 client tests (`background_animation/{tests,host,surface,worker}.rs`, `background_composition.rs`, `animation_settings_ui.rs`, `location_picker.rs`, `project_config.rs`, `settings_help/tests.rs`, `app.rs`, `ui_openstreetmap_tests.rs`, `background_acceptance.rs`, `semantic_animation_tests.rs`), several of which read `settings.video.*` fields; each needs a different hosted stand-in (only `OpenStreetMap` and `PluginScene` remain) and the assertions that depend on Video-only behaviour rewritten.
- The semantic recommender has a Video-only branch: `ResourcePolicy::Authored`, `authored_capabilities`, `verify_resources`, the `axes` and `notes` entries and the catalogue text in `ilium-client/src/semantic_animation.rs`, plus the server and `ilium/tests/semantic_animation_tui.rs` that exercise it. With Video gone the authored-resource policy has no user; that is a protocol decision, not a mechanical edit.
- Mechanical part, once decided: remove `AmbientKind::Video` and `AmbientSettings::video` in `ilium-ambient/src/registry.rs`, the `scenes::video` module and `pub use scenes::video::VideoSettings` in `lib.rs`, `AnimationKind::Video` (array `ALL` 5 to 4, `ambient()`, `introduction.rs`, `animation_rows.rs` AN-15, `animation_settings_ui.rs` line 1569), add `alias = "video"` on `PluginScene`, delete `ilium-ambient/src/scenes/video/` and `ilium-ambient/assets/germination.json`, then drop the cargo dependencies only Video used and the `tools/test_port_map.tsv` rows.

Still blocked and untouched: OpenStreetMap (location picker and shared location row special-case `AnimationKind::OpenStreetMap`; Overpass answers over the `http` cap) and the voxel landscape chain (see the notes above).
