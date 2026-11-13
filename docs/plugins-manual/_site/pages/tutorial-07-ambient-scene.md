# 7. Ambient scene

## 7. Ambient scene

This step adds an animated background to the agent tools plugin. A scene is a third component. It runs in the animation world `ilium:anim/base@0.1.0`, apart from the server and client worlds. The host creates one renderer for the scene, reads its frame rate and calls `render` once per frame. The scene writes tone values into a surface and calls `present`. The host then applies the shared look, the palette and the dither mode. The scene has no colour, palette or dithering controls of its own.

The scene draws rings that move outward from the centre of the panel. It has one setting, the ring count.

### What you need

- [Tutorial step 6](/pages/tutorial-06-notes-pane/) finished. The scene is a new component in the same project. The `server/` and `client/` components do not change.
- A `scene` component in a new crate `scene/`, built against `ilium:anim/base@0.1.0`.
- Rust stable 1.90 or later with the `wasm32-wasip2` target, using `ilium-plugin-sdk`. Or Bun 1.2 or later with `@ilium/plugin-sdk`, for the TypeScript version in this step.
- Grants: none. The scene side always has `log`, `clock`, its own `storage`, `codec`, and the `fs` scopes `package`, `data` and `cache`. This scene reads no file and makes no network call, so `capabilities.scene` is an empty list.
- Manifest values: `family` is `procedural` and `modes` is `["live"]`. The family names are `natural`, `procedural`, `geometry`, `simulation`, `live-data`, `maps-and-space`, `saved-world`, `media` and `other`. The mode names are `live` and `pre-rendered`. The first mode in the list is the default.

### What changed from step 6

- `core/src/rings.rs` is new. It holds the tone function and makes no host calls.
- `scene/` is a new crate. Its `lib.rs` implements the `renderer` resource.
- `manifest.json` adds the `scene` object and the `scenes` contribution.
- `server/` and `client/` are unchanged.

### The frame loop

<figure>
<svg viewBox="0 0 400 206" role="img" aria-label="Frame loop between the host and the scene renderer">
  <defs>
    <marker id="s7-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="10" width="120" height="34" rx="4" fill="none" stroke="currentColor"/>
  <text x="70" y="31" font-size="11" text-anchor="middle" fill="currentColor">Host</text>
  <rect x="270" y="10" width="120" height="34" rx="4" fill="none" stroke="currentColor" stroke-dasharray="5 3"/>
  <text x="330" y="31" font-size="11" text-anchor="middle" fill="currentColor">Scene renderer</text>
  <line x1="130" y1="70" x2="268" y2="70" stroke="currentColor" marker-end="url(#s7-arrow)"/>
  <text x="200" y="64" font-size="11" text-anchor="middle" fill="currentColor">new(spec), once</text>
  <line x1="268" y1="100" x2="132" y2="100" stroke="currentColor" marker-end="url(#s7-arrow)"/>
  <text x="200" y="94" font-size="11" text-anchor="middle" fill="currentColor">frames-per-second, once</text>
  <line x1="130" y1="130" x2="268" y2="130" stroke="currentColor" marker-end="url(#s7-arrow)"/>
  <text x="200" y="124" font-size="11" text-anchor="middle" fill="currentColor">render(frame, surface)</text>
  <line x1="268" y1="160" x2="132" y2="160" stroke="currentColor" marker-end="url(#s7-arrow)"/>
  <text x="200" y="154" font-size="11" text-anchor="middle" fill="currentColor">tone values, then present()</text>
  <rect x="80" y="178" width="240" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="200" y="195" font-size="11" text-anchor="middle" fill="currentColor">Host: look, dither, send to panel</text>
</svg>
<figcaption>The host owns the clock. The scene answers each frame with tone values, and the host adds the look and the dither.</figcaption>
</figure>

The host reads `frames-per-second` once after the constructor returns, and again after each `reconfigure` that returns `true`. It never reads it per frame. A palette change reaches the scene through `set-palette`, between frames.

### Renderer surface

The scene implements the `renderer` resource. Rust names come from `ilium_plugin_sdk::anim::{surface, renderer}`, and TypeScript names from `@ilium/plugin-sdk/anim/surface` and `@ilium/plugin-sdk/anim/renderer`.

| Member | Rust | TypeScript | Default | Meaning |
|---|---|---|---|---|
| constructor | `fn new(spec: SceneSpec) -> Self` | `constructor(spec)` | none | Called when the scene is shown and after a restart. |
| readiness | `fn readiness(&self) -> Readiness` | `readiness()` | `Ready` | `Ready`, `Preparing` or `Failed`. |
| frames per second | `fn frames_per_second(&self) -> u32` | `framesPerSecond()` | 30 | Requested rate, capped by `frame_rate_cap`. |
| pointer | `fn wants_pointer(&self) -> bool` | `wantsPointer()` | false | Enables `pointer` on the surface. |
| occupancy | `fn wants_occupancy(&self) -> bool` | `wantsOccupancy()` | false | Enables `occupancy` on the surface. |
| cell colours | `fn uses_cell_colors(&self) -> bool` | `usesCellColors()` | false | Enables `write_cell_colors`. |
| render | `fn render(&mut self, frame: FrameInput, surface: &FrameSurface)` | `render(frame, surface)` | required | Draws one frame. |
| palette | `fn set_palette(&mut self, palette_rgb: Vec<u32>)` | `setPalette(palette)` | ignored | Called when the palette changes. |
| settings | `fn reconfigure(&mut self, settings_json: String) -> bool` | `reconfigure(settings)` | `false` | Applies new settings. `false` makes the host restart the scene. |
| status | `fn status(&self) -> Option<String>` | `status()` | none | Short text shown beside the scene title while set. |

`FrameInput` holds the frame time and the surface size:

| Field | Rust | TypeScript | Meaning |
|---|---|---|---|
| time | `time_millis: u64` | `timeMillis: bigint` | Milliseconds since the scene started. |
| visible time | `wall_millis: u64` | `wallMillis: bigint` | Milliseconds since the scene was last made visible. |
| clock | `now_unix_millis: u64` | `nowUnixMillis: bigint` | Wall-clock time in Unix milliseconds. |
| size | `width: u32`, `height: u32` | `width`, `height` | Surface size in pixels: cells times the dither sub-cell factor (the number of pixels that one cell holds along each axis, set by the user's active dither mode). |
| palette | `palette_rgb: Vec<u32>` | `paletteRgb: number[]` | Current palette as `0xRRGGBB` values. Empty in greyscale and monotone modes. |

`FrameSurface` takes the tone rows. `write_tones(first_row, &[u8])` (`writeTones`) writes `gray8` rows, one byte per pixel. The other methods are `write_rows`, `write_cell_colors`, `occupancy`, `pointer` and `present`. A `render` call that returns without `present` drops that frame, and a warning is logged once per second.

The dither modes are `ordered`, `stippled`, `bayer2`, `bayer4`, `bayer16`, `blue-noise`, `gradient`, `halftone`, `lines`, `diagonal`, `crosshatch`, `white-noise`, `floyd-steinberg`, `atkinson` and `sierra-lite`. The user picks one in the shared Dither row. A scene never names a mode.

### Manifest

The manifest adds a `scene` object next to `server` and `client`. Required exports such as `renderer` are never listed in `exports`, so the list is empty. The `scenes` contribution declares the scene. The host reads it before it loads the component.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.agent-tools",
  "name": "Agent tools",
  "version": "0.1.0",
  "publisher": "Example Developer",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": ["job-runner"]
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": ["views"]
  },
  "scene": {
    "entry": "scene.wasm",
    "world": "ilium:anim/base@0.1.0",
    "exports": []
  },
  "capabilities": {
    "server": [
      "agent.read", "agent.control", "tree.read", "tree.decorate",
      "notify.desktop", "sound.play", "fs.read:project", "jobs.run"
    ],
    "client": [
      "agent.read", "tree.read", "tree.metadata", "ui.panels", "editor.edit",
      "topics.subscribe:example.agent-tools/*", "panes.create", "panes.provide-kind"
    ],
    "scene": []
  },
  "contributes": {
    "scenes": [
      {
        "id": "rings",
        "title": "Rings",
        "family": "procedural",
        "modes": ["live"],
        "frame_rate_cap": 30,
        "settings_schema": {
          "type": "object",
          "properties": {
            "ring_count": {
              "type": "integer",
              "title": "Rings",
              "default": 3,
              "minimum": 1,
              "maximum": 12,
              "step": 1
            }
          }
        }
      }
    ]
  }
}
```

The other contributions from step 6 (commands, keybindings, panels, pane kinds, sounds, jobs, topics, settings) stay in the file. They are omitted here to keep the block short.

- `id` is local to the plugin. The global scene id is `example.agent-tools:rings`.
- `frame_rate_cap` is an integer from 1 to 60. The host caps the rate that the scene asks for at this value.
- `settings_schema` uses the JSON-Schema subset of the settings pages. The host checks new settings against it before the scene sees them.
- `clip_seconds` is used only in `pre-rendered` mode, which this step does not declare.

### The pure tone function

The drawing rule is a pure function with no host calls. The TypeScript scene mirrors it below, because a TypeScript plugin has no Rust core crate. For each cell it returns a tone from 0 (dark) to 255 (bright). The rings are cosine waves of the distance from the centre. A terminal cell is about twice as tall as it is wide, so the vertical gap counts double. The frame number moves the rings outward.

**Rust**

```rust
// core/src/rings.rs. Tone of one cell, no host calls.

/// Returns the tone of the cell at (`x`, `y`) for a frame. Zero is dark and 255 is bright.
/// Rings move outward as `frame` grows.
pub fn ring_tone(x: u32, y: u32, width: u32, height: u32, frame: u32, ring_count: u32) -> u8 {
    if width == 0 || height == 0 {
        return 0;
    }
    let centre_x = (width as f32 - 1.0) / 2.0;
    let centre_y = (height as f32 - 1.0) / 2.0;
    let delta_x = x as f32 - centre_x;
    // A terminal cell is about twice as tall as it is wide, so vertical distance counts double.
    let delta_y = (y as f32 - centre_y) * 2.0;
    let distance = (delta_x * delta_x + delta_y * delta_y).sqrt();
    let period = width.max(height) as f32 / ring_count.max(1) as f32;
    let angle = std::f32::consts::TAU * (distance / period - frame as f32 * 0.05);
    ((angle.cos() + 1.0) * 0.5 * 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centre_is_brightest_at_frame_zero() {
        assert_eq!(ring_tone(2, 2, 5, 5, 0, 3), 255);
    }

    #[test]
    fn zero_size_is_dark() {
        assert_eq!(ring_tone(0, 0, 0, 0, 0, 3), 0);
    }

    #[test]
    fn rings_move_outward_between_frames() {
        assert_ne!(ring_tone(0, 0, 22, 6, 0, 3), ring_tone(0, 0, 22, 6, 8, 3));
    }
}
```

The `as u8` cast is safe. The value of `(cos + 1) * 0.5 * 255` stays between 0 and 255, and Rust's float-to-integer cast saturates in any case.

### The scene component in Rust

The scene implements the `renderer` resource. The constructor reads the spec: the settings arrive as JSON text, and the palette as a list of `0xRRGGBB` values. `render` writes one tone per cell and presents the frame. `reconfigure` takes new settings without a restart.

**Rust**

```rust
// scene/src/lib.rs. The rings scene, step 7.
use agent_tools_core::rings::ring_tone;
use ilium_plugin_sdk::anim::renderer::{FrameInput, Renderer, SceneSpec};
use ilium_plugin_sdk::anim::surface::FrameSurface;
use ilium_plugin_sdk::prelude::*;
use serde::Deserialize;

/// The tone function counts frames at this rate. The frame number comes from the scene clock,
/// so the ring speed does not change when the host runs the scene at a lower rate.
const RING_FRAMES_PER_SECOND: u64 = 30;

#[derive(Deserialize, Clone, Copy)]
#[serde(default)]
struct RingSettings {
    ring_count: u32,
}

impl Default for RingSettings {
    fn default() -> Self {
        Self { ring_count: 3 }
    }
}

#[ilium_plugin(side = "scene")]
#[derive(Default)]
struct RingRenderer {
    settings: RingSettings,
    palette_rgb: Vec<u32>,
}

impl Lifecycle for RingRenderer {}

impl Renderer for RingRenderer {
    fn new(spec: SceneSpec) -> Self {
        Self {
            settings: serde_json::from_str(&spec.settings).unwrap_or_default(),
            palette_rgb: spec.palette,
        }
    }

    fn frames_per_second(&self) -> u32 {
        30
    }

    fn render(&mut self, frame: FrameInput, surface: &FrameSurface) {
        let frame_number = (frame.time_millis * RING_FRAMES_PER_SECOND / 1000) as u32;
        let mut tones = Vec::with_capacity((frame.width * frame.height) as usize);
        for row in 0..frame.height {
            for column in 0..frame.width {
                tones.push(ring_tone(
                    column,
                    row,
                    frame.width,
                    frame.height,
                    frame_number,
                    self.settings.ring_count,
                ));
            }
        }
        surface.write_tones(0, &tones);
        surface.present();
    }

    fn set_palette(&mut self, palette_rgb: Vec<u32>) {
        self.palette_rgb = palette_rgb;
    }

    /// Returns `false` for settings it cannot read. The host then restarts the scene.
    fn reconfigure(&mut self, settings_json: String) -> bool {
        match serde_json::from_str::<RingSettings>(&settings_json) {
            Ok(settings) => {
                self.settings = settings;
                true
            }
            Err(_) => false,
        }
    }

    fn status(&self) -> Option<String> {
        Some(format!("{} rings", self.settings.ring_count))
    }
}
```

Notes on the code:

- The scene keeps `palette_rgb` for the palette contract. It does not draw colour, because the host maps tones to colour. Until a later change reads the field, the compiler reports it as never read. That is expected in this step.
- `impl Lifecycle for RingRenderer {}` is required because the scene exports `lifecycle` as well as `renderer`. The default methods are enough.
- `render` takes `&mut self`, as the SDK trait does. It takes the surface by reference.

### The same scene in TypeScript

The TypeScript version has the same behaviour. `defineScene` takes the class, and the spec arrives already parsed. `timeMillis` is a `bigint`, so the code converts it with `Number`. The `ringTone` function mirrors the Rust one line for line, so both builds draw the same frame.

**TypeScript**

```ts
// scene/src/scene.ts. The rings scene, step 7, in TypeScript.
import { defineScene } from '@ilium/plugin-sdk';
import type { FrameInput, SceneSpec } from '@ilium/plugin-sdk/anim/renderer';
import type { FrameSurface } from '@ilium/plugin-sdk/anim/surface';

const RING_FRAMES_PER_SECOND = 30;

interface RingSettings {
  ring_count: number;
}

function readRingSettings(value: unknown): RingSettings | undefined {
  if (typeof value !== 'object' || value === null) return undefined;
  const ringCount = (value as { ring_count?: unknown }).ring_count;
  return typeof ringCount === 'number' ? { ring_count: ringCount } : undefined;
}

// Mirrors ring_tone in core/src/rings.rs.
function ringTone(
  x: number, y: number, width: number, height: number, frame: number, ringCount: number,
): number {
  if (width === 0 || height === 0) return 0;
  const deltaX = x - (width - 1) / 2;
  const deltaY = (y - (height - 1) / 2) * 2;
  const distance = Math.sqrt(deltaX * deltaX + deltaY * deltaY);
  const period = Math.max(width, height) / Math.max(ringCount, 1);
  const angle = 2 * Math.PI * (distance / period - frame * 0.05);
  return Math.floor(((Math.cos(angle) + 1) * 0.5) * 255);
}

class Rings {
  private settings: RingSettings = { ring_count: 3 };

  constructor(spec: SceneSpec) {
    this.settings = readRingSettings(spec.settings) ?? this.settings;
  }

  framesPerSecond(): number {
    return 30;
  }

  render(frame: FrameInput, surface: FrameSurface): void {
    const frameNumber = Math.floor((Number(frame.timeMillis) * RING_FRAMES_PER_SECOND) / 1000);
    const tones = new Uint8Array(frame.width * frame.height);
    for (let row = 0; row < frame.height; row++) {
      for (let column = 0; column < frame.width; column++) {
        tones[row * frame.width + column] = ringTone(
          column, row, frame.width, frame.height, frameNumber, this.settings.ring_count,
        );
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

  status(): string {
    return `${this.settings.ring_count} rings`;
  }
}

export const { lifecycle, renderer } = defineScene(Rings);
```

A scene test draws a frame through `FakeScene`, because a scene is not a view. In Rust, `FakeScene::new::<RingRenderer>("rings", 16, 4)` makes a scene of 16 columns and 4 rows, and `scene.frame(FrameInput::default())` returns a `Screen`. In TypeScript, `createFakeScene(renderer, 'rings', 16, 4)` does the same. Compare `screen.text()` with a golden file through `assert_golden`. See [Test harness reference](/pages/reference-test-harness/#scenes).

### Build and test

Run the tone tests on the development machine, then build the package and start the dev loop:

```bash
cargo test -p agent-tools-core rings
ilium plugin build
ilium plugin dev
```

`ilium plugin build` writes `dist/scene.wasm` and prints JSONL, one object per line. `ilium plugin dev` rebuilds on change and reloads the scene in place. A successful build ends like this:

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin build
<span class="t-dim">{"type":"progress","step":"build","message":"Building scene.wasm"}</span>
<span class="t-dim">{"type":"artifact","path":"dist/scene.wasm","bytes":184320}</span>
<span class="t-ok">{"type":"result","ok":true}</span>
<span class="t-dim">{"type":"summary","duration_ms":4120,"warnings":0,"errors":0}</span>
</pre>
<figcaption>Illustrative, not generated. The JSONL lines are abbreviated: the real artifact line also carries a `sha256` field.</figcaption>
</figure>

### Errors and edge cases

The renderer returns no `plugin-error`, so a failure in the scene appears as a trap, a deadline breach, a dropped frame or a log line. The rules come from the frame budget, the restart rule and the settings check.

| Case | What happens |
|---|---|
| `ring_count` is 13 | The host refuses the value before the scene sees it, and the previous count stays. The message is `ring_count: 13 is above the maximum of 12`. |
| `reconfigure` returns `false` | The host discards the renderer and builds a new one from a new spec. This is a restart, not a failure. |
| `render` runs past the frame budget | The frame budget is 33 ms at 30 fps. The frame is skipped. The default `frame_deadline_ms` is that budget, with a ceiling of 50 ms. 30 breaches in a row trap. |
| `render` returns without `present` | The frame is dropped and a warning is logged once per second. |
| `write_tones` gets the wrong length | Only whole rows are written. A remainder is dropped and a warning is logged once per second. |
| `readiness` stays `Preparing` for over 10 s | Counts as `Failed`. `render` is not called until the renderer restarts. |
| A trap or a failed readiness | Counts as one failure. The restart waits 1 s after the first failure, 5 s after the second. The third failure within 10 minutes disables the scene side of the plugin. |
| Ring count 12 on a small panel | Rings are closer together than a cell, so the pattern aliases. This is not an error. |

### What you see

Open Settings, then the Animations tab. The scene appears in the list as `Rings`, with the status text that `status()` returns. Its one setting is a shared number row, drawn with the grey dotted leader, the decrement and increment buttons and the edit button:

<figure>
<pre class="term"><span class="t-hi">╭──≡ ● · Rings</span><span class="t-hi">─────────────────────────────────╮</span>
<span class="t-hi">│</span> Rings <span class="t-dim">………………………………………………………………………………</span> - 3 + * <span class="t-hi">│</span>
<span class="t-hi">╰──────────────────────────────────────────────╯</span>
</pre>
<figcaption>Illustrative, not generated. The row comes from the <code>ring_count</code> property of <code>settings_schema</code>. The colour mode, palette, dither, panel placement and frame-rate cap rows above and below it are the shared rows, which the scene does not draw.</figcaption>
</figure>

The panels below show the tone values at frame 0 and frame 16, which is about half a second later. The rows are 24 by 6 cells. The characters map tone to brightness, from space (dark) to at-sign (bright). The host's palette supplies the colour on screen.

<figure>
<pre class="term"><span class="t-hi">╭──≡ ● · Frame 0─────────╮</span>
<span class="t-hi">│  :+%@@#+-:..:-+#@@%+:  │</span>
<span class="t-hi">│ .+%@%+:   ..   :+%@%+. │</span>
<span class="t-hi">│ :*@@#-  :+%%+:  -#@@*: │</span>
<span class="t-hi">│ :*@@#-  :+%%+:  -#@@*: │</span>
<span class="t-hi">│ .+%@%+:   ..   :+%@%+. │</span>
<span class="t-hi">│  :+%@@#+-:..:-+#@@%+:  │</span>
<span class="t-hi">╰────────────────────────╯</span>
<span class="t-ok">╭──≡ ● · Frame 16────────╮</span>
<span class="t-ok">│+:  :+%@@%####%@@%+:  :+│</span>
<span class="t-ok">│-  :*@@%+-.  .-+%@@*:  -│</span>
<span class="t-ok">│. .=%@%+:  ::  :+%@%=. .│</span>
<span class="t-ok">│. .=%@%+:  ::  :+%@%=. .│</span>
<span class="t-ok">│-  :*@@%+-.  .-+%@@*:  -│</span>
<span class="t-ok">│+:  :+%@@%####%@@%+:  :+│</span>
<span class="t-ok">╰────────────────────────╯</span>
</pre>
<figcaption>Illustrative, not generated. The top panel is frame 0 and the lower one is frame 16, both with three rings. The rings move outward from the centre between the two frames.</figcaption>
</figure>

### Scene additions

A scene can ask for more than tones. These additions are optional and the rings scene does not need them, but they are how a scene such as a map or a chess board draws text and answers the pointer.

| Addition | Where | Rust | TypeScript | Meaning |
|---|---|---|---|---|
| Glyph plane | manifest `glyph_plane: true` | `surface.write_glyphs(&cells)` | `surface.writeGlyphs(cells)` | Text cells drawn over the pixels after dithering. At most 131,072 cells per frame. Glyph colours also go through the shared look. |
| Clear glyphs | surface | `surface.clear_glyphs()` | `surface.clearGlyphs()` | Removes every glyph cell. |
| Owner ids | surface | `surface.set_owner_ids(first_row, &ids)` | `surface.setOwnerIds(firstRow, ids)` | One `u16` per pixel, 0 for none. Maps pixels to logical objects. |
| Frame receipt | surface | `surface.last_receipt()` | `surface.lastReceipt()` | `frame`, `presented_at_millis`, `dropped_before` and the `pointer_owner` under the pointer. |
| Pointer | renderer | `fn wants_pointer(&self) -> bool` | `wantsPointer()` | A runtime answer, read after the constructor and after each `reconfigure`. `pointer` gives the cell, or none outside the scene. |
| Pre-rendered loop | manifest `prerender` | none | none | `{ frames, fps, cache_mib }`. The host calls `render` for the frames once, caches them (at most 128 MiB, 30 fps) and plays the loop. |
| Fonts | scene world | `Font::new(asset)` then `rasterize(text, size_px)` | `new Font(asset)` | TTF or OTF from package assets, at most 4 MiB, rasterized to `mask8`. |

A glyph cell is `column`, `row`, `text`, optional `fg` and `bg` colours as `0xRRGGBB`, and text modifiers. This draws the ring count in the centre of the panel:

**Rust**

```rust
// scene/src/lib.rs. In render, after surface.write_tones and before present.
// Needs "glyph_plane": true on the scene in the manifest.
let label = GlyphCell {
    column: (frame.width / 2) as u16,
    row: (frame.height / 2) as u16,
    text: self.settings.ring_count.to_string(),
    fg: None,
    bg: None,
    add: Modifiers::default(),
};
surface.write_glyphs(&[label]);
```

**TypeScript**

```ts
// scene/src/scene.ts. In render, after surface.writeTones and before present.
// Needs "glyph_plane": true on the scene in the manifest.
surface.writeGlyphs([{
  column: Math.floor(frame.width / 2),
  row: Math.floor(frame.height / 2),
  text: String(this.settings.ring_count),
  fg: undefined,
  bg: undefined,
  add: {},
}]);
```

The glyph is drawn after the shared look, so it follows the palette and the colour mode like the rings do. The surface size in `frame.width` and `frame.height` is in pixels. A glyph cell uses cell coordinates, and the scene API does not give a scene the dither sub-cell factor. A scene therefore cannot convert `frame.width` into a cell column exactly. The example above uses half the pixel size as the cell position, which is exact only where one cell holds one pixel; with a larger factor the label sits right of and below the centre. A scene that needs exact cell positions has to avoid the glyph plane until the SDK exposes the factor.

A pre-rendered scene declares `modes: ["pre-rendered"]` and `prerender` in the manifest. The host runs the frames once and plays them with no further calls. Values out of range are the manifest error `manifest.scene-prerender`. A pre-rendered scene that answers `wants_pointer` with `true` is not refused: `pointer` returns `none` and the host logs a warning.

```json
{
  "id": "rings-loop", "title": "Rings loop", "family": "procedural",
  "modes": ["pre-rendered"], "clip_seconds": 8,
  "prerender": { "frames": 240, "fps": 30, "cache_mib": 32 }
}
```

### See also

- [Scenes](/pages/api-scenes/) lists every renderer member, the surface formats and the readiness rules.
- [Settings screens](/pages/settings-screens/) explains the JSON-Schema subset that `settings_schema` uses.
- [Manifest reference](/pages/reference-manifest/) lists every manifest key.
- [Errors and limits](/pages/reference-errors-and-limits/) lists the frame budget and the restart rule.
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/) cover the shared SDK conventions.
- [Tutorial step 6](/pages/tutorial-06-notes-pane/) is the step this one builds on, and [tutorial step 8](/pages/tutorial-08-typescript/) ports the earlier steps to TypeScript.

API checked against `ilium:plugin@0.1.0`, including the scene additions (AX15).
