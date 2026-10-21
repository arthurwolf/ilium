---
layout: page
title: Scenes
permalink: /pages/core-plugin-scenes/
---

## Scenes

The Scenes plugin (id `ilium.core.scenes`, side: scene) is the plugin form of the animated scenes themselves. A scene is a renderer that draws frames into a surface. The host runs it in its own animation world, applies the shared look from the [Ambient look plugin](/pages/core-plugin-ambient/) (palette, filters, dithering), and puts the result behind the panes. A scene never adds colour, brightness or dithering controls of its own.

The scene host, the cached loops and the frame pacing are part of the client kernel. The plugin supplies the renderers: natural, procedural, geometry, simulations and games, live data, maps and space, media scenes, packaged animations and the optional GPU path.

### What it covers

- **Natural cached-loop scenes.** See Built-in scenes with cached loops in the nature and abstract animations page of the user documentation.
- **Procedural quiet scenes and machines.** See Quiet procedural scenes and Machines and clocks.
- **Geometry scenes, pipes and wind.** See Geometry and mathematics, 3D pipes and Wind.
- **Simulations and games.** Galactic empires, Hex expedition and Vector TD, described in the media and games animations page.
- **Live-data scenes.** See Live data scenes in the maps, space and live data page. They fetch data over HTTP and need an `http:<host>` grant.
- **Maps, space, Wikipedia and voxel saved worlds.** See Stars overhead, OpenStreetMap and Voxel landscape.
- **Media scenes.** Video, audio spectrum and images, in the media and games page.
- **Packaged animations.** `.iliumanim` files, described under Animation packages. See also [porting an .iliumanim package](/pages/porting-iliumanim/).
- **Optional GPU backend.** Described under Background animations in How it works. The scene-side calls are an unstable development-build feature.

### See it

<figure><img src="/assets/demos/26-animations.gif" alt="Animated scenes behind the panes, switched from the Animations tab" loading="lazy"><figcaption>Scenes behind the panes. The shared look comes from the Animations tab, not from the scene.</figcaption></figure>

### What the user sees

<figure><pre class="term">╭─ Waves ──────────────────────────────────────╮
│ <span class="t-ok">⣀⣠⣤⣴⣶⣾⣿⣷⣶⣦⣤⣄⣀⡀⣀⣠⣤⣴⣶⣾⣿⣷⣶⣦⣤⣄⣀⡀⣀⣠⣤⣴⣶⣾</span>           │
│ <span class="t-ok">⠀⠈⠙⠛⠿⢿⣿⡿⠿⠟⠛⠋⠁⠀⠀⠈⠙⠛⠿⢿⣿⡿⠿⠟⠛⠋⠁⠀⠀⠈⠙⠛⠿⢿</span>           │
│ <span class="t-ok">⣀⣠⣤⣴⣶⣾⣿⣷⣶⣦⣤⣄⣀⡀⣀⣠⣤⣴⣶⣾⣿⣷⣶⣦⣤⣄⣀⡀⣀⣠⣤⣴⣶⣾</span>           │
│ <span class="t-hi">Waves</span>  <span class="t-dim">24 fps · live</span>                         │
│                                              │
╰──────────────────────────────────────────────╯</pre><figcaption>Rendered at 48 columns. Illustrative. A procedural scene drawn with Braille cells (two columns by four rows of dots per cell) and a text overlay from the glyph plane. The glyphs depend on the icon set.</figcaption></figure>
<figure><pre class="term">╭─ Stars overhead ─────────────────────────────╮
│ <span class="t-dim">⠀⠀⠀⠀⠀⠀⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠂⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀</span>   │
│ <span class="t-hi">⠀⠀⠀⠀⠀⠀⠀⠀⢀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀</span>   │
│ <span class="t-dim">⠀⠀⠂⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠐⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀</span>   │
│ <span class="t-hi">Vega</span>   <span class="t-dim">alt 62°  az 118°</span>                      │
│ <span class="t-dim">Observer 38.72 N, 9.14 W</span>                     │
│                                              │
╰──────────────────────────────────────────────╯</pre><figcaption>Rendered at 48 columns. Illustrative. Stars above the horizon for the observer location, drawn as dots. The caption names the star under the pointer.</figcaption></figure>
### Scene catalogue and single owners

Every scene below belongs to the Scenes plugin. The [Ambient look](/pages/core-plugin-ambient/) plugin owns only the shared controls (colour, palette, filters, dithering, display) and the shared observer location row. The kernel owns the scene host, the raster and the look painter. [COR-010] [COR-011]

| Family | Scenes | User documentation | Wave | Grants a scene package needs | Single owner of the feature |
|---|---|---|---|---|---|
| Natural cached loops | Shoreline, Moonlit water, Sleeping ridge, Windy hillside, Tea steam, Kelp, Stone caustics, Cloudlets, Two ripples, Lily pads | animations-nature-and-abstract | 4c | none | Scenes |
| Quiet procedural | Aurora, Pollen, Fireflies, Window sunlight, Frost, Lighthouse | animations-nature-and-abstract | 4c | none | Scenes |
| Geometry and mathematics | Paper-fold trace, Embroidery orbit, Prime constellations, Turning wallpaper, Unfinished circle, Threads through a needle, Ink that hesitates, Crop circles, Delayed reflection, Almost touching, Hidden wheel | animations-nature-and-abstract | 4c | none | Scenes |
| Pipes, wind, growth | 3D pipes, Wind, Growth | animations-nature-and-abstract | 4c | none | Scenes |
| Dithered | Dithered water, Atlantic dusk, Dithered fBm clouds, Dithered waves, Dithr patterns | animations-nature-and-abstract | 4c | none | Scenes (dithering algorithms stay shared) |
| Machines and clocks | Cube clock, Box machine, Machine screen | animations-nature-and-abstract | 4c | none | Scenes |
| Carpet simulations | Mouse hunters, Autonomous Snake, Slow Life, chess (automated and Lichess TV), DVD ball, Planetary orbits, clocks | animations-nature-and-abstract | 4c; Lichess TV in 4d | `http:lichess.org` for Lichess TV | Scenes |
| Games | Galactic empires, Hex expedition, Vector TD | animations-media-and-games | 4c | none | Scenes |
| Video | Custom series, Germination series | animations-media-and-games | 4d | `media.video`, `fs.pick` | Scenes. Ambient only shows the preview clip |
| Audio spectrum | Audio spectrum | animations-media-and-games | 4d | `audio.loopback` | Scenes |
| Images | Images | animations-media-and-games | 4d | `fs.pick`, `codec` | Scenes |
| Wikipedia | Wikipedia | animations-media-and-games | 4d | `http:*.wikipedia.org` | Scenes |
| Maps and space | Stars overhead, Satellite clouds, Earth at night, Solar system, Topographic maps, OpenStreetMap | animations-maps-space-and-live-data | 4d | `http`, `cache`, shared location | Scenes. The location row belongs to Ambient |
| Live data | Earthquakes, aircraft, boats, live graphs | animations-maps-space-and-live-data | 4d | `http:<host>` per source | Scenes |
| Saved worlds | Voxel landscape | animations-maps-space-and-live-data | 4e | `minecraft-saves` scope, `codec.inflate`, jobs | Scenes |

[COR-011] covers the rule that a scene package follows the shared look. A scene that adds its own brightness, contrast, hue, palette or dithering control is refused by the check rules.

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

A scene package has a `scene` entry and no server or client side. Each scene listed under `contributes.scenes` appears in the Animations tab. The `capabilities.scene` list holds the grants for media, the observer location, the GPU and a live-data host.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.scenes",
  "name": "Scenes",
  "version": "0.1.0",
  "publisher": "Ilium",
  "scene": { "entry": "scene.wasm", "world": "ilium:anim/base@0.1.0", "exports": [] },
  "capabilities": {
    "scene": ["media.video", "location.observer", "gpu.kernels",
              "http:services.swpc.noaa.gov"]
  },
  "contributes": {
    "scenes": [
      { "id": "waves", "title": "Waves", "family": "procedural",
        "modes": ["live", "pre-rendered"], "frame_rate_cap": 24, "clip_seconds": 12,
        "settings_schema": { "type": "object", "properties": {} } },
      { "id": "clock", "title": "Clock", "family": "procedural",
        "modes": ["live"], "frame_rate_cap": 2, "glyph_plane": true,
        "settings_schema": { "type": "object", "properties": {} } },
      { "id": "stars-overhead", "title": "Stars overhead", "family": "maps-space",
        "modes": ["live"], "frame_rate_cap": 10,
        "settings_schema": { "type": "object", "properties": {} } }
    ]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

The `capabilities` list of this manifest is the grant set of one sample scene package, the one that holds the video, location, GPU and space-weather scenes. The grant table above names the grants of every scene family, including `http:lichess.org`, `http:*.wikipedia.org`, `minecraft-saves`, `codec.inflate` and `jobs`. A scene package declares only the grants of the scenes it contains, so those other grants belong to the packages that ship those scenes and are not in this manifest.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.scenes",
  "name": "Scenes",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "The Scenes plugin (id ilium.core.scenes, side: scene) is the plugin form of the animated scenes themselves.",
  "license": "MIT",
  "scene": {
    "entry": "scene.wasm",
    "world": "ilium:anim/base@0.1.0",
    "exports": []
  },
  "capabilities": {
    "scene": [
      "media.video",
      "location.observer",
      "gpu.kernels",
      "http:services.swpc.noaa.gov",
      "extensions.provide:ilium.core.scenes/scene-packs",
      "extensions.provide:ilium.core.scenes/scene-data-sources"
    ]
  },
  "activation": [
    "scene:*"
  ],
  "contributes": {
    "extension_points": [
      {
        "id": "scene-packs",
        "kind": "collect",
        "title": "Scene packs",
        "entry_schema": {
          "type": "object",
          "required": ["id", "title", "scenes"],
          "properties": {
            "id": { "type": "string" },
            "title": { "type": "string" },
            "scenes": {
              "type": "array",
              "items": {
                "type": "object",
                "required": ["scene"],
                "properties": {
                  "scene": { "type": "string" },
                  "controls": { "type": "object" }
                }
              }
            }
          }
        },
        "risk": "standard",
        "max_entries": 64,
        "dynamic": false,
        "version": "0.1.0"
      },
      {
        "id": "scene-data-sources",
        "kind": "collect",
        "title": "Scene data sources",
        "entry_schema": {
          "type": "object",
          "required": ["id", "title", "service"],
          "properties": {
            "id": { "type": "string" },
            "title": { "type": "string" },
            "service": { "type": "string" }
          }
        },
        "risk": "standard",
        "max_entries": 64,
        "dynamic": false,
        "version": "0.1.0"
      }
    ],
    "scenes": [
      {
        "id": "waves",
        "title": "Waves",
        "family": "procedural",
        "modes": [
          "live",
          "pre-rendered"
        ],
        "frame_rate_cap": 24,
        "clip_seconds": 12,
        "settings_schema": {
          "type": "object",
          "properties": {}
        }
      },
      {
        "id": "clock",
        "title": "Clock",
        "family": "procedural",
        "modes": [
          "live"
        ],
        "frame_rate_cap": 2,
        "glyph_plane": true,
        "settings_schema": {
          "type": "object",
          "properties": {}
        }
      },
      {
        "id": "stars-overhead",
        "title": "Stars overhead",
        "family": "maps-space",
        "modes": [
          "live"
        ],
        "frame_rate_cap": 10,
        "settings_schema": {
          "type": "object",
          "properties": {}
        }
      }
    ]
  },
  "limits_tier": "first-party"
}
```

### Samples

#### 1. A procedural scene

The smallest scene is a renderer with `new(spec)`, `frames_per_second` and `render`. It writes tones, and the host applies the palette, filters and dithering. The renderer also keeps the palette that the host passes in the spec and in `set_palette`, as every scene should. See the [scenes API](/pages/api-scenes/) and [always-granted capabilities](/pages/capability-always-granted/).

**Rust**

```rust
use ilium_plugin_sdk::anim::renderer::{FrameInput, Renderer, SceneSpec};
use ilium_plugin_sdk::anim::surface::FrameSurface;
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "scene")]
#[derive(Default)]
struct Waves {
    palette: Vec<u32>,
}

impl Lifecycle for Waves {}

impl Renderer for Waves {
    fn new(spec: SceneSpec) -> Self {
        Waves { palette: spec.palette }
    }

    fn frames_per_second(&self) -> u32 {
        24 // the host caps it at frame_rate_cap from the manifest
    }

    // PALETTE (future plugin contract): colour scenes shift onto this palette.
    // A tone-only scene needs no code; the host colours the tones.
    fn set_palette(&mut self, palette_rgb: Vec<u32>) {
        self.palette = palette_rgb;
    }

    fn render(&mut self, frame: FrameInput, surface: &FrameSurface) {
        let seconds = frame.time_millis as f32 / 1000.0;
        let mut tones = Vec::with_capacity((frame.width * frame.height) as usize);
        for row in 0..frame.height {
            for column in 0..frame.width {
                let wave = (column as f32 * 0.2 + row as f32 * 0.1 - seconds * 2.0).sin();
                tones.push(((wave + 1.0) * 127.5) as u8);
            }
        }
        surface.write_tones(0, &tones);
        surface.present();
    }
}
```

**TypeScript**

```ts
import { defineScene } from '@ilium/plugin-sdk';
import type { FrameInput, SceneSpec } from '@ilium/plugin-sdk/anim/renderer';
import type { FrameSurface } from '@ilium/plugin-sdk/anim/surface';

class Waves {
  private palette: number[];

  constructor(spec: SceneSpec) {
    this.palette = spec.palette;
  }

  framesPerSecond(): number {
    return 24; // the host caps it at frame_rate_cap from the manifest
  }

  // PALETTE (future plugin contract): colour scenes shift onto this palette.
  // A tone-only scene needs no code; the host colours the tones.
  setPalette(palette: number[]): void {
    this.palette = palette;
  }

  render(frame: FrameInput, surface: FrameSurface): void {
    const seconds = Number(frame.timeMillis) / 1000;
    const tones = new Uint8Array(frame.width * frame.height);
    for (let row = 0; row < frame.height; row++) {
      for (let column = 0; column < frame.width; column++) {
        const wave = Math.sin(column * 0.2 + row * 0.1 - seconds * 2);
        tones[row * frame.width + column] = Math.round(((wave + 1) / 2) * 255);
      }
    }
    surface.writeTones(0, tones);
    surface.present();
  }
}

export const { lifecycle, renderer } = defineScene(Waves);
```

#### 2. Text over pixels

A scene with `"glyph_plane": true` can draw text over its pixels. The clock face and hands go into the pixel plane, and the numbers go into the glyph plane, so the text stays sharp under dithering. See the [Glyph plane and frame receipts](/pages/api-surface-glyph-plane/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;

// Assumption of this sample: 2 pixels by 4 pixels per cell. The real factor depends
// on the dither mode, so a real scene derives it from the surface size.
const PIXELS_PER_CELL_X: u32 = 2;
const PIXELS_PER_CELL_Y: u32 = 4;

impl Clock {
    fn draw_numbers(&self, frame: &FrameInput, surface: &FrameSurface) -> Result<(), PluginError> {
        let columns = (frame.width / PIXELS_PER_CELL_X) as u16;
        let rows = (frame.height / PIXELS_PER_CELL_Y) as u16;
        surface.clear_glyphs();
        let mark = |text: &str, column: u16, row: u16| GlyphCell {
            column, row, text: text.into(), fg: Some(0xffffff), bg: None, add: Modifiers::BOLD,
        };
        surface.write_glyphs(&[
            mark("12", columns / 2 - 1, 0),
            mark("3", columns - 2, rows / 2),
            mark("6", columns / 2, rows - 1),
            mark("9", 1, rows / 2),
        ])
    }
}
// In render: draw the face ring and hands as tones with write_tones, then
// self.draw_numbers(&frame, surface) for the numbers, then surface.present().
```

**TypeScript**

```ts
import type { FrameInput } from '@ilium/plugin-sdk/anim/renderer';
import type { FrameSurface } from '@ilium/plugin-sdk/anim/surface';

// Assumption of this sample: 2 pixels by 4 pixels per cell. The real factor depends
// on the dither mode, so a real scene derives it from the surface size.
const PIXELS_PER_CELL_X = 2;
const PIXELS_PER_CELL_Y = 4;

export function drawNumbers(frame: FrameInput, surface: FrameSurface): void {
  const columns = Math.floor(frame.width / PIXELS_PER_CELL_X);
  const rows = Math.floor(frame.height / PIXELS_PER_CELL_Y);
  surface.clearGlyphs();
  const mark = (text: string, column: number, row: number) =>
    ({ column, row, text, fg: 0xffffff, bg: undefined, add: { bold: true } });
  surface.writeGlyphs([
    mark('12', Math.floor(columns / 2) - 1, 0),
    mark('3', columns - 2, Math.floor(rows / 2)),
    mark('6', Math.floor(columns / 2), rows - 1),
    mark('9', 1, Math.floor(rows / 2)),
  ]);
}
// In render: draw the face ring and hands as tones with writeTones, then
// drawNumbers(frame, surface) for the numbers, then surface.present().
```

#### 3. Video scene

The host decodes the video. The scene opens it from bytes bundled in the package, pulls one frame per tick scaled to the surface, and writes it as RGB. The `codec` import reads the poster image size without decoding it. See the [media API](/pages/api-media/), the [media.video grant](/pages/capability-media-video/) and the [codec API](/pages/api-codec/). A clip may be at most 16 MiB when opened from bytes.

**Rust**

```rust
use ilium_plugin_sdk::anim::renderer::{FrameInput, Renderer, SceneSpec};
use ilium_plugin_sdk::anim::surface::FrameSurface;
use ilium_plugin_sdk::host::{codec, media};
use ilium_plugin_sdk::prelude::*;

static CLIP: &[u8] = include_bytes!("../assets/loop.mp4");
static POSTER: &[u8] = include_bytes!("../assets/poster.png");

struct VideoScene {
    video: Option<u64>,
}

impl Renderer for VideoScene {
    fn new(_spec: SceneSpec) -> Self {
        let video = media::open_video_bytes(CLIP, "video/mp4").ok().map(|(id, _info)| id);
        // The poster only supplies a size here; its pixels are not decoded.
        if let Ok(info) = codec::image_info(POSTER) {
            let _aspect = info.width as f32 / info.height as f32;
        }
        VideoScene { video }
    }

    fn render(&mut self, frame: FrameInput, surface: &FrameSurface) {
        let Some(video) = self.video else { return };
        if let Ok(Some(pixels)) = media::next_frame(video, frame.width, frame.height) {
            surface.write_rgb(0, &pixels);
            surface.present();
        }
    }
}
```

**TypeScript**

```ts
import { defineScene } from '@ilium/plugin-sdk';
import { imageInfo } from '@ilium/plugin-sdk/host/codec';
import { nextFrame, openVideoBytes } from '@ilium/plugin-sdk/host/media';
import type { FrameInput, SceneSpec } from '@ilium/plugin-sdk/anim/renderer';
import type { FrameSurface } from '@ilium/plugin-sdk/anim/surface';

declare const CLIP: Uint8Array;   // bundled with the package
declare const POSTER: Uint8Array; // bundled with the package

class VideoScene {
  private video: bigint | undefined;

  constructor(_spec: SceneSpec) {
    try {
      this.video = openVideoBytes(CLIP, 'video/mp4')[0];
    } catch {
      this.video = undefined;
    }
    // The poster only supplies a size here; its pixels are not decoded.
    const info = imageInfo(POSTER);
    void (info.width / info.height);
  }

  render(frame: FrameInput, surface: FrameSurface): void {
    if (this.video === undefined) return;
    const pixels = nextFrame(this.video, frame.width, frame.height);
    if (pixels === undefined) return;
    surface.writeRgb(0, pixels);
    surface.present();
  }
}

export const { lifecycle, renderer } = defineScene(VideoScene);
```

#### 4. Stars overhead

The scene reads the observer position once with `location.current`, converts each star to altitude and azimuth for the current time, and plots the ones above the horizon. The position never leaves the scene. See the [location API](/pages/api-location/) and the [location.observer grant](/pages/capability-location-observer/). The sidereal-time formula is a simple approximation, good enough for a background.

**Rust**

```rust
use ilium_plugin_sdk::anim::renderer::{FrameInput, Renderer, SceneSpec};
use ilium_plugin_sdk::anim::surface::FrameSurface;
use ilium_plugin_sdk::host::location;

/// A few bright stars as (right ascension in hours, declination in degrees).
const STARS: [(f64, f64); 6] = [
    (2.53, 89.26), (5.28, 46.0), (5.92, 7.41), (6.75, -16.72), (14.26, 19.18), (18.62, 38.78),
];

struct StarsOverhead {
    observer: Option<(f64, f64)>, // read once, latitude and longitude in degrees
}

impl Renderer for StarsOverhead {
    fn new(_spec: SceneSpec) -> Self {
        StarsOverhead { observer: location::current() }
    }

    fn render(&mut self, frame: FrameInput, surface: &FrameSurface) {
        let mut tones = vec![0u8; (frame.width * frame.height) as usize];
        if let Some((latitude, longitude)) = self.observer {
            let days = frame.now_unix_millis as f64 / 86_400_000.0 - 10_957.5; // since J2000
            let sidereal = (280.46061837 + 360.98564736629 * days + longitude) % 360.0;
            for (hours, declination) in STARS {
                let hour_angle = (sidereal - hours * 15.0).to_radians();
                let (lat, dec) = (latitude.to_radians(), declination.to_radians());
                let sin_alt = dec.sin() * lat.sin() + dec.cos() * lat.cos() * hour_angle.cos();
                if sin_alt <= 0.0 { continue } // below the horizon
                let altitude = sin_alt.asin();
                let azimuth = (-dec.cos() * hour_angle.sin())
                    .atan2(dec.sin() * lat.cos() - dec.cos() * lat.sin() * hour_angle.cos());
                let radius = 1.0 - altitude / std::f64::consts::FRAC_PI_2;
                let x = ((azimuth.sin() * radius + 1.0) / 2.0 * (frame.width - 1) as f64) as u32;
                let y = ((1.0 - azimuth.cos() * radius) / 2.0 * (frame.height - 1) as f64) as u32;
                tones[(y * frame.width + x) as usize] = 255;
            }
        }
        surface.write_tones(0, &tones);
        surface.present();
    }
}
```

**TypeScript**

```ts
import { defineScene } from '@ilium/plugin-sdk';
import * as location from '@ilium/plugin-sdk/host/location';
import type { FrameInput, SceneSpec } from '@ilium/plugin-sdk/anim/renderer';
import type { FrameSurface } from '@ilium/plugin-sdk/anim/surface';

/** A few bright stars as [right ascension in hours, declination in degrees]. */
const STARS: Array<[number, number]> = [
  [2.53, 89.26], [5.28, 46.0], [5.92, 7.41], [6.75, -16.72], [14.26, 19.18], [18.62, 38.78],
];
const radians = (degrees: number) => (degrees * Math.PI) / 180;

class StarsOverhead {
  private observer = location.current(); // read once: [latitude, longitude] in degrees

  constructor(_spec: SceneSpec) {}

  render(frame: FrameInput, surface: FrameSurface): void {
    const tones = new Uint8Array(frame.width * frame.height);
    if (this.observer !== undefined) {
      const [latitude, longitude] = this.observer;
      const days = Number(frame.nowUnixMillis) / 86_400_000 - 10_957.5; // since J2000
      const sidereal = (280.46061837 + 360.98564736629 * days + longitude) % 360;
      for (const [hours, declination] of STARS) {
        const hourAngle = radians(sidereal - hours * 15);
        const lat = radians(latitude);
        const dec = radians(declination);
        const sinAlt = Math.sin(dec) * Math.sin(lat) + Math.cos(dec) * Math.cos(lat) * Math.cos(hourAngle);
        if (sinAlt <= 0) continue; // below the horizon
        const altitude = Math.asin(sinAlt);
        const azimuth = Math.atan2(
          -Math.cos(dec) * Math.sin(hourAngle),
          Math.sin(dec) * Math.cos(lat) - Math.cos(dec) * Math.sin(lat) * Math.cos(hourAngle),
        );
        const radius = 1 - altitude / (Math.PI / 2);
        const x = Math.floor(((Math.sin(azimuth) * radius + 1) / 2) * (frame.width - 1));
        const y = Math.floor(((1 - Math.cos(azimuth) * radius) / 2) * (frame.height - 1));
        tones[y * frame.width + x] = 255;
      }
    }
    surface.writeTones(0, tones);
    surface.present();
  }
}

export const { lifecycle, renderer } = defineScene(StarsOverhead);
```

#### 5. GPU kernels

Heavy scenes can hand a kernel to the graphics device. The GPU API is unstable and exists in development builds only, so every call needs a CPU path: check `available` first, and fall back when the call returns `unsupported`. See the [GPU API](/pages/api-gpu/) and the [gpu.kernels grant](/pages/capability-gpu-kernels/).

**Rust**

```rust
use ilium_plugin_sdk::host::gpu::{self, GpuKernel};
use ilium_plugin_sdk::prelude::*;

// Unstable: the GPU API is for development builds only and may change.
impl CloudScene {
    fn soften(&self, field: &[u8], width: u32, height: u32) -> Vec<u8> {
        if !gpu::available() {
            return self.soften_on_cpu(field, width, height); // portable fallback
        }
        match gpu::run(GpuKernel::Blur, field, width, height, &self.params_json) {
            Ok(output) => output,
            Err(error) if error.code == ErrorCode::Unsupported => {
                self.soften_on_cpu(field, width, height)
            }
            Err(_) => field.to_vec(),
        }
    }
}
```

**TypeScript**

```ts
import { PluginError } from '@ilium/plugin-sdk';
import { available, run } from '@ilium/plugin-sdk/host/gpu';

// Unstable: the GPU API is for development builds only and may change.
export function soften(
  field: Uint8Array, width: number, height: number, paramsJson: string,
  softenOnCpu: (field: Uint8Array, width: number, height: number) => Uint8Array,
): Uint8Array {
  if (!available()) return softenOnCpu(field, width, height); // portable fallback
  try {
    return run('blur', field, width, height, paramsJson);
  } catch (error) {
    if (error instanceof PluginError && error.code === 'unsupported') {
      return softenOnCpu(field, width, height);
    }
    return field;
  }
}
```

### Extension points

Other plugins change what `ilium.core.scenes` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.scenes/<point>`). Contributors need `extensions.contribute:ilium.core.scenes/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `scene-packs` | collect | Offer a bundle of scenes with preset controls. | entry: id, title, scenes (list of scene id and control values) | standard |
| `scene-data-sources` | collect | Provide live data to data scenes, such as a feed. | entry: id, title, service (returns data points) | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.scenes/catalog`: list: list scenes and their controls

`contributes.scenes` entries add scenes. This plugin hosts the built-in ones.

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.scenes"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Bundles a slow cloud scene

A package adds this entry in its manifest. It needs no code.

```json
{
  "api": "ilium:plugin@0.1.0",
  "capabilities": { "scene": ["extensions.contribute:ilium.core.scenes/scene-packs"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.scenes/scene-packs",
        "id": "calm",
        "priority": 50,
        "entry": {
          "title": "Calm",
          "scenes": [
            {
              "scene": "clouds",
              "controls": {
                "speed": 0.2
              }
            }
          ]
        }
      }
    ]
  }
}
```

### Extension points this plugin owns

The table gives the values that the core extension points table requires for each point this plugin owns. The page does not state the version, deadline, max or dynamic values, so they come from the defaults in [Extension points](/pages/api-extension-points/) and are marked (default). The side is `scene` because the page states that the Scenes plugin runs on the scene side. The complete manifest grants `extensions.provide:ilium.core.scenes/<point>` in its `capabilities.scene` list and declares both points in `contributes.extension_points`.

| Point | Side | Version | Deadline | Max | Dynamic |
|---|---|---|---|---|---|
| `ilium.core.scenes/scene-packs` | scene | 0.1.0 (default) | none (default) | 64 entries (default) | no (default) |
| `ilium.core.scenes/scene-data-sources` | scene | 0.1.0 (default) | none (default) | 64 entries (default) | no (default) |

### ABI used

- [Scenes API](/pages/api-scenes/) and [always-granted capabilities](/pages/capability-always-granted/)
- [Glyph plane](/pages/api-surface-glyph-plane/)
- [Media API](/pages/api-media/), [media.video](/pages/capability-media-video/) and [codec API](/pages/api-codec/)
- [Location API](/pages/api-location/) and [location.observer](/pages/capability-location-observer/)
- [GPU API](/pages/api-gpu/) (unstable) and [gpu.kernels](/pages/capability-gpu-kernels/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Ambient look plugin](/pages/core-plugin-ambient/), the shared look every scene gets
- [Tutorial: an ambient scene](/pages/tutorial-07-ambient-scene/)
