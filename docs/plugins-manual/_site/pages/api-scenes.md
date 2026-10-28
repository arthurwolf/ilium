# Scenes

## Scenes (`ilium:anim`)

A scene is an animated background. The host runs each scene in its own animation world, separate from the server and client worlds. The host creates one `renderer` for the scene, reads its frame rate, and calls `render` once per frame with a `frame-surface`. The scene writes tone or cell colours into that surface and calls `present`. The host then applies the shared look: colour mode, palette, colour filters and dithering. A scene never adds its own brightness, contrast, palette or dither controls.

Use a scene for an animated background the user can choose and tune. Use a client view or pane for anything interactive.

### What you need

- **Package:** a `scene` component (`scene.wasm`) with the world `ilium:anim/base@0.1.0`, and a `contributes.scenes` entry for each scene.
- **Language:** Rust stable 1.90 or later with the `wasm32-wasip2` target, using `ilium-plugin-sdk`; or TypeScript with Bun 1.2 or later and `@ilium/plugin-sdk`. The SDK pins the binding generator versions, so `bun install` is the only install step.
- **Imports:** `ilium:anim/base` allows `log`, `clock`, `storage`, `fs`, `codec`, `jobs`, `http`, `audio`, `media`, `location` and the surface. Nothing else is linked.
- **Exports:** `lifecycle` and `renderer` are required, and the manifest `exports` list never names a required export, so `"exports": []` is correct for a scene with only those two. The optional `job-runner` export needs `jobs.run`.
- **Grants:** the scene side always has `log`, `clock`, `storage` (own buckets), `codec`, and the `fs` scopes `package`, `data` and `cache`. Every other grant goes in `capabilities.scene`, for example `jobs.run`, `http:api.example.com` or `media.video`. `audio.capture` and `location.observer` are high risk and need an explicit user grant.

### Manifest contributions

Each scene is declared in `contributes.scenes`. The host reads the entry before it loads the component.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `id` | string | none | Scene id, unique within the plugin. The global id is `<plugin-id>:<id>`. |
| `title` | string | none | Name shown in the animation settings list. |
| `family` | string | none | One of `natural`, `procedural`, `geometry`, `simulation`, `live-data`, `maps-and-space`, `saved-world`, `media`, `other`. |
| `modes` | list of strings | none | Any of `live` and `pre-rendered`. The first value is the default mode. |
| `settings_schema` | JSON schema object | none | The scene's own settings, in the same subset used by settings pages. |
| `frame_rate_cap` | integer 1 to 60 | 30 | Upper bound for the frame rate the scene may ask for. A value above 60 is clamped to 60 and logged. |
| `clip_seconds` | integer 1 to 30 | 10 | Length of the clip in `pre-rendered` mode. |

### Renderer surface

The renderer is a guest resource. The SDK names below are the ones a plugin writes. The WIT names are the same in kebab-case.

| Member | Rust | TypeScript | Default | Meaning |
|---|---|---|---|---|
| constructor | `fn new(spec: SceneSpec) -> Self` | `constructor(spec: SceneSpec)` | none | Called when the scene is shown. |
| readiness | `fn readiness(&self) -> Readiness` | `readiness()` | `Ready` | `Ready`, `Preparing` or `Failed`. |
| frames-per-second | `fn frames_per_second(&self) -> u32` | `framesPerSecond()` | 30 | Requested rate, capped by `frame_rate_cap`. |
| wants-pointer | `fn wants_pointer(&self) -> bool` | `wantsPointer()` | false | Enables `pointer()` on the surface. |
| wants-occupancy | `fn wants_occupancy(&self) -> bool` | `wantsOccupancy()` | false | Enables `occupancy()` on the surface. |
| uses-cell-colors | `fn uses_cell_colors(&self) -> bool` | `usesCellColors()` | false | Enables `write_cell_colors`. |
| render | `fn render(&mut self, frame: FrameInput, surface: &FrameSurface)` | `render(frame, surface)` | required | Draws one frame. |
| set-palette | `fn set_palette(&mut self, palette_rgb: Vec<u32>)` | `setPalette(palette)` | ignored | Called when the palette changes. |
| reconfigure | `fn reconfigure(&mut self, settings_json: String) -> bool` | `reconfigure(settings)` | `false` | Applies new settings. `false` means restart. |
| status | `fn status(&self) -> Option<String>` | `status()` | none | Short text shown beside the scene title while set. |

The frame input holds these fields:

| Field | Rust | TypeScript | Meaning |
|---|---|---|---|
| time | `time_millis: u64` | `timeMillis: bigint` | Milliseconds since the scene started. |
| visible time | `wall_millis: u64` | `wallMillis: bigint` | Milliseconds since the scene was last made visible. |
| clock | `now_unix_millis: u64` | `nowUnixMillis: bigint` | Wall-clock time in Unix milliseconds. |
| size | `width: u32`, `height: u32` | `width`, `height` | Surface size in pixels (cells times the dither sub-cell factor). |
| palette | `palette_rgb: Vec<u32>` | `paletteRgb: number[]` | Current palette as `0xRRGGBB` values. Empty in greyscale and monotone modes. |

The surface methods are `size`, `write_rows` (`writeRows`), `write_cell_colors` (`writeCellColors`), `occupancy`, `pointer` and `present`. The SDK adds two helpers: `write_tones` (`writeTones`) writes `gray8` rows, and `write_rgb` (`writeRgb`) writes `rgb8` rows.

| Pixel format | Bytes per pixel | Use |
|---|---|---|
| `gray8` | 1 | One tone per pixel. The host colours it with the shared look. |
| `rgb8` | 3 | Natural colour, in R, G, B order. The host still applies the shared look. |
| `rgba8` | 4 | As `rgb8` with a fourth byte, in R, G, B, A order. |

### Lifecycle of a renderer

The host constructs the renderer when the scene is shown, and again after a restart. The spec is a JSON object with `scene` (the scene id), `settings`, `palette` and `mode` (`live` or `pre-rendered`). Rust receives it as `SceneSpec`. TypeScript receives it parsed, with `settings` typed `unknown`, so the scene checks it before use.

The host reads `readiness` before each frame. `Preparing` frames are not presented, and `Failed` stops drawing. The frame rate is read once after construction and after each `reconfigure` that returns `true`, never per frame. A palette change calls `set_palette` before the next frame. A setting change is validated against `settings_schema` first, then sent to `reconfigure`.

In `pre-rendered` mode the host calls `render` for `clip_seconds` of frames, stores the clip in host memory and loops it without calling `render` again. A change to the palette or the settings rebuilds the clip. A single clip is limited to 64 MiB (the one clip size limit); the 256 MiB figure in [host rendering](/pages/reference-host-rendering/) is the separate total clip memory the host holds across clips. A longer clip is shortened to fit, with a minimum of one second.

<figure>
<svg viewBox="0 0 400 200" role="img" aria-label="Scene frame flow from host to scene and back to the panel">
  <defs>
    <marker id="scene-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="20" y="12" width="170" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="105" y="34" font-size="11" text-anchor="middle" fill="currentColor">Host calls render(frame)</text>
  <line x1="105" y1="48" x2="105" y2="74" stroke="currentColor" marker-end="url(#scene-arrow)"/>
  <rect x="20" y="76" width="170" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="105" y="98" font-size="11" text-anchor="middle" fill="currentColor">Scene writes rows</text>
  <line x1="105" y1="112" x2="105" y2="138" stroke="currentColor" marker-end="url(#scene-arrow)"/>
  <rect x="20" y="140" width="170" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="105" y="162" font-size="11" text-anchor="middle" fill="currentColor">Scene calls present</text>
  <line x1="190" y1="158" x2="238" y2="158" stroke="currentColor" marker-end="url(#scene-arrow)"/>
  <rect x="240" y="140" width="150" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="315" y="162" font-size="11" text-anchor="middle" fill="currentColor">Host applies the look</text>
  <line x1="315" y1="138" x2="315" y2="116" stroke="currentColor" marker-end="url(#scene-arrow)"/>
  <rect x="240" y="76" width="150" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="315" y="98" font-size="11" text-anchor="middle" fill="currentColor">Panel shows the frame</text>
</svg>
<figcaption>The scene draws tone or colour and presents the frame. The host adds the palette, colour filters and dithering.</figcaption>
</figure>

### Surface rules

- A row is `width` bytes in `gray8`, `3 x width` bytes in `rgb8`, and `4 x width` bytes in `rgba8`. `write_rows` writes whole rows only. When the data length is an exact multiple of the row size, every row is written. When it is not, the call writes the whole rows, drops the partial trailing row and writes a warn line at most once per second, and it does not return an error. `invalid-argument` is only for a format the scene did not declare, for empty data, or for `first-row` at or past `height`.
- Rows at or past `height` are ignored.
- `write_cell_colors` takes one packed `0xRRGGBB` value per cell, row-major. It is ignored unless `uses-cell-colors` is `true`.
- `occupancy()` returns an `occupancy-grid` of cells (`columns`, `rows`, and one `bool` per cell, row-major): `true` where pane text is drawn. It returns `none` unless `wants-occupancy` is `true`.
- `pointer()` returns the column and row in cells, counted from the top left of the scene area. It returns `none` when the pointer is outside that area, or when `wants-pointer` is `false`.
- A `render` call that returns without `present` drops that frame, and a warn line is written once per second.

### Example 1: a gradient in Rust

The scene draws a vertical grey gradient at 24 frames per second. It needs no settings and no palette. The host adds the shared look, so the gradient follows the user's colour mode and dithering without any code in the scene.

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::anim::renderer::{FrameInput, Renderer, SceneSpec};
use ilium_plugin_sdk::anim::surface::FrameSurface;

#[ilium_plugin(side = "scene")]
#[derive(Default)]
struct Gradient;

impl Lifecycle for Gradient {}

impl Renderer for Gradient {
    fn new(_spec: SceneSpec) -> Self {
        Gradient
    }

    fn frames_per_second(&self) -> u32 {
        24
    }

    fn render(&mut self, frame: FrameInput, surface: &FrameSurface) {
        let mut tones = Vec::with_capacity((frame.width * frame.height) as usize);
        for row in 0..frame.height {
            let tone = (row * 255 / frame.height.max(1)) as u8;
            tones.extend(std::iter::repeat(tone).take(frame.width as usize));
        }
        surface.write_tones(0, &tones);
        surface.present();
    }
}
```

The manifest declares the scene and its rate:

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.gradient",
  "name": "Gradient",
  "version": "0.1.0",
  "publisher": "Example Developer",
  "scene": { "entry": "scene.wasm", "world": "ilium:anim/base@0.1.0", "exports": [] },
  "capabilities": { "scene": [] },
  "contributes": {
    "scenes": [
      {
        "id": "gradient",
        "title": "Gradient",
        "family": "procedural",
        "modes": ["live"],
        "frame_rate_cap": 24,
        "settings_schema": { "type": "object", "properties": {} }
      }
    ]
  }
}
```

### Example 2: a settings-driven scene in TypeScript

The scene draws concentric rings. The `ring_count` setting comes from `settings_schema`. The `reconfigure` method accepts a new count without a restart, and returns `false` for a value it cannot read, which makes the host restart the scene. The manifest adds the `pre-rendered` mode with a 12-second clip.

```ts
import { defineScene } from '@ilium/plugin-sdk';
import type { FrameInput, SceneSpec } from '@ilium/plugin-sdk/anim/renderer';
import type { FrameSurface } from '@ilium/plugin-sdk/anim/surface';

interface RingSettings {
  ring_count: number;
}

function readRingSettings(value: unknown): RingSettings | undefined {
  if (typeof value !== 'object' || value === null) return undefined;
  const ringCount = (value as { ring_count?: unknown }).ring_count;
  return typeof ringCount === 'number' ? { ring_count: ringCount } : undefined;
}

function ringTone(column: number, row: number, frame: FrameInput, ringCount: number): number {
  const distance = Math.hypot(column - frame.width / 2, (row - frame.height / 2) * 2);
  const phase = Number(frame.timeMillis) / 1000;
  const wave = Math.sin(distance * ringCount * 0.1 - phase * 2);
  return Math.round(((wave + 1) / 2) * 255);
}

class Rings {
  private settings: RingSettings = { ring_count: 3 };

  constructor(spec: SceneSpec) {
    this.settings = readRingSettings(spec.settings) ?? this.settings;
  }

  framesPerSecond(): number {
    return 24;
  }

  render(frame: FrameInput, surface: FrameSurface): void {
    const tones = new Uint8Array(frame.width * frame.height);
    for (let row = 0; row < frame.height; row++) {
      for (let column = 0; column < frame.width; column++) {
        tones[row * frame.width + column] = ringTone(column, row, frame, this.settings.ring_count);
      }
    }
    surface.writeTones(0, tones);
    surface.present();
  }

  reconfigure(settings: unknown): boolean {
    const next = readRingSettings(settings);
    if (next === undefined) return false;
    this.settings = next;
    return true;
  }
}

export const { lifecycle, renderer } = defineScene(Rings);
```

The manifest entry for this scene:

```json
{
  "id": "rings",
  "title": "Rings",
  "family": "procedural",
  "modes": ["live", "pre-rendered"],
  "frame_rate_cap": 24,
  "clip_seconds": 12,
  "settings_schema": {
    "type": "object",
    "properties": {
      "ring_count": { "type": "integer", "title": "Rings", "default": 3,
                      "minimum": 1, "maximum": 12, "step": 1 }
    }
  }
}
```

### What the user sees

The scene appears in the animation settings list under its title, next to the shared rows: colour mode, palette, filters, dithering, panel placement, frame-rate cap, speed, density and background. The scene's own settings come from `settings_schema`. A `status` string appears beside the title while it is set. After a failure the plugin log records the reason, and the third failure in 10 minutes disables the scene side. Settings > Plugins then shows the notice described in [Lifecycle](/pages/lifecycle/).

<pre class="term">
<span class="t-prompt">$</span> ilium plugin logs example.rings --tail 3
2026-10-11 12:04:09 error example.rings scene trapped, memory over 32 MiB
2026-10-11 12:04:10 warn  example.rings restart in 1 s (failure 1 of 3)
2026-10-11 12:04:11 info  example.rings renderer built, 24 frames per second
</pre>

Illustrative, not generated. The rings scene traps once, restarts after one second and draws again.

### Edge cases and errors

| Situation | Result |
|---|---|
| `render` traps, or the scene exceeds `memory_mib` (32 MiB by default) | Trap. The frame is skipped and the plugin follows the [restart and disable rule](/pages/reference-errors-and-limits/#restart-and-disable-rule). A scene failure counts as a failure of the whole plugin on that side [API-045]. |
| A frame passes `frame_deadline_ms` (the frame budget, at most 50 ms) | The frame is skipped. Thirty breaches in a row trap. |
| `readiness` returns `Failed`, or stays `Preparing` for more than 10 s | Counts as one failure. The host stops calling `render` until the restart. |
| Settings fail `settings_schema` | The host refuses the change before the scene sees it. The previous values stay. |
| `reconfigure` returns `false` | The host restarts the renderer with the new spec. This is not a failure. |
| A call to an import whose grant the manifest never requested | Trap, because that is a plugin bug. |
| A call to an import whose grant the user declined or revoked, or `http.fetch` outside the grant | `not-permitted`, synchronously. |
| `location.current` with no position set or a declined grant | `none`, not an error. |
| `codec.decode-image` with an unsupported format | `unsupported`. |
| `jobs.start` with the job queue full | `busy`. Retry later. |
| `storage.put` over 1 MiB, or a bucket over 16 MiB | `invalid-argument` or `quota-exceeded`. |
| Revocation of a grant | The renderer restarts with the reduced grants. Not a failure. |

Limits that apply to a scene, from the limits table:

| Limit | Default | Ceiling (T1 / T2 and T3) | Error on breach |
|---|---|---|---|
| `memory_mib` | 32 | 256 / 128 | trap |
| `table_elements` | 10,000 | 50,000 / 20,000 | trap |
| `frame_deadline_ms` | frame budget | 50 / 50 | frame skipped, trap after 30 in a row |
| `storage_bucket_mib` | 16 | 256 / 16 | `quota-exceeded` |

### Glyph plane and loops

Scenes can now use a glyph plane for text, owner ids and frame receipts for hit tests, fonts, and pre-rendered loops. They are described in [glyph plane](/pages/api-surface-glyph-plane/). The manifest keys are `glyph_plane: true` and `prerender: { frames, fps, cache_mib }`; a scene that declares `wants-pointer` cannot also declare `prerender` (`manifest.scene-prerender`).

| Need | Use |
|---|---|
| Labels over the picture | `write-glyphs` (131,072 cells per frame) |
| Which object is under the pointer | `set-owner-ids` and `last-receipt().pointer-owner` |
| Text as pixels | `Font::rasterize` into `mask8` |
| A loop that costs no plugin calls | `prerender` (up to 3,600 frames at 30 fps, within the 64 MiB clip limit) |

**Rust**

```rust
let owner = surface.last_receipt().and_then(|receipt| receipt.pointer_owner);
```

**TypeScript**

```ts
const owner = surface.lastReceipt()?.pointerOwner;
```

The shared look still applies: palette, dithering and appearance are the host's, and `PALETTE (future plugin contract)` scenes follow the palette the host passes in the constructor.

Requirement ids for this page: [API-045], [RND-002], [RND-003], [RND-004], [RND-005], [RND-006], [RND-007], [RND-008], [RND-009], [RND-010], [RND-011], [RND-012], [RND-013], [RND-014]. See the [requirements index](/pages/reference-requirements/).

### WIT

The contract below is an excerpt. The owner is the [WIT reference](/pages/reference-wit/), and that page wins if the two differ.

```wit
// Excerpt of ilium:anim/surface in reference-wit
/// An occupancy value per cell, set by the host from the pane text: true where text is drawn.
record occupancy-grid { columns: u16, rows: u16, cells: list<bool> }
record scene-pointer { column: u16, row: u16, sub-x: u8, sub-y: u8, pressed: bool }
record glyph-cell { column: u16, row: u16, text: string, fg: option<u32>, bg: option<u32>, add: modifiers }
/// pointer-owner is the owner id under the pointer at the presented frame.
record frame-receipt { frame: u64, presented-at-millis: u64, dropped-before: u32, pointer-owner: option<u16> }
resource frame-surface {
  /// Width and height in pixels.
  size: func() -> tuple<u32, u32>;
  /// Rows are top to bottom. Rows that fall at or past the height are ignored.
  /// Errors: invalid-argument for a format the scene did not declare, empty data, or first-row at or past the height. A partial trailing row is dropped, not an error.
  write-rows: func(first-row: u32, format: pixel-format, data: list<u8>) -> result<_, plugin-error>;
  /// Needs uses-cell-colors. One 0xRRGGBB value per cell, row by row.
  write-cell-colors: func(first-row: u32, colors: list<u32>) -> result<_, plugin-error>;
  /// Needs wants-occupancy.
  occupancy: func() -> option<occupancy-grid>;
  /// Needs wants-pointer.
  pointer: func() -> option<scene-pointer>;
  /// Ends the frame. A render call that returns without present drops the frame.
  present: func();
  /// Needs glyph_plane true. At most 131,072 cells per frame.
  write-glyphs: func(cells: list<glyph-cell>) -> result<_, plugin-error>;
  clear-glyphs: func();
  /// One id per pixel, 0 means none.
  set-owner-ids: func(first-row: u32, ids: list<u16>) -> result<_, plugin-error>;
  last-receipt: func() -> option<frame-receipt>;
}
```

### See also

- [Plugin exports](/pages/api-exports/) for the `renderer` export and `job-runner`.
- [Storage](/pages/api-storage/), [File access](/pages/api-fs/) and [Codec](/pages/api-codec/) for the scene's data and images.
- [Background jobs](/pages/api-jobs/), [HTTP](/pages/api-http/), [Location](/pages/api-location/), [Audio](/pages/api-audio/) and [Media](/pages/api-media/) for the other imports.
- [Errors and limits](/pages/reference-errors-and-limits/) for the full error codes.
- [Tutorial 7: ambient scene](/pages/tutorial-07-ambient-scene/) for a step-by-step scene plugin.
