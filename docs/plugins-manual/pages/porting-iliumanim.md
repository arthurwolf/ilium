---
layout: page
title: Moving from .iliumanim to .iliumpkg
permalink: /pages/porting-iliumanim/
---

## Moving from .iliumanim to .iliumpkg

Ilium replaces the `.iliumanim` animation package with the plugin package `.iliumpkg`. There is no compatibility layer. The new loader refuses an `.iliumanim` file, so each animation is rebuilt as a `.iliumpkg` that holds a `scene.wasm` component. The archive format carries over: the package is still a deterministic ZIP32 archive with stored entries, sorted paths and fixed timestamps. What changes is the names, the component layout, the rights, the limits and the render code. The contract also provides scene features that the old format had no way to express: a host-cached pre-rendered loop, a text layer over the pixels, owner ids for hit tests and fonts from package assets. This page shows every code listing in Rust and in TypeScript, so you can port a scene in either language.

### What you need

- An existing `.iliumanim` project folder or file. The porting command accepts either.
- For Rust scenes: Rust stable 1.90 or later with the `wasm32-wasip2` target.
- For TypeScript scenes: Bun 1.2 or later. Run `bun install` in the plugin folder. It installs the pinned `@ilium/plugin-sdk`, `@bytecodealliance/componentize-js` and `@bytecodealliance/jco`. Nothing is installed globally.
- No grant is needed to port a scene. The new manifest requests rights under `capabilities.scene`, as in the rights table below.
- Reference: [Manifest](/pages/reference-manifest/), [Capabilities](/pages/reference-capabilities/), [Scenes API](/pages/api-scenes/), [Rust SDK](/pages/rust-sdk/), [TypeScript SDK](/pages/typescript-sdk/) and [Toolchain](/pages/getting-started-toolchain/).

## What changes

<figure>
<svg viewBox="0 0 360 200" role="img" aria-label="Mapping from an animation package to a plugin package">
  <title>Old and new package layout</title>
  <desc>An animation package with a script and assets is rebuilt as a plugin package with a manifest, a scene component, the assets, and a file inventory.</desc>
  <defs>
    <marker id="port-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="8" y="16" width="130" height="168" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="73" y="38" text-anchor="middle" fill="currentColor" font-size="12">.iliumanim</text>
  <text x="20" y="66" fill="currentColor" font-size="11">manifest.json</text>
  <text x="20" y="92" fill="currentColor" font-size="11">script (V8)</text>
  <text x="20" y="118" fill="currentColor" font-size="11">assets/</text>
  <rect x="222" y="16" width="130" height="168" rx="6" fill="none" stroke="currentColor"/>
  <text x="287" y="38" text-anchor="middle" fill="currentColor" font-size="12">.iliumpkg</text>
  <text x="230" y="66" fill="currentColor" font-size="11">manifest.json</text>
  <text x="230" y="92" fill="currentColor" font-size="11">scene.wasm</text>
  <text x="230" y="118" fill="currentColor" font-size="11">assets/</text>
  <text x="230" y="144" fill="currentColor" font-size="11">files, signature</text>
  <line x1="142" y1="100" x2="218" y2="100" stroke="currentColor" fill="none" marker-end="url(#port-arrow)"/>
  <text x="180" y="92" text-anchor="middle" fill="currentColor" font-size="10">rebuild</text>
</svg>
<figcaption>The script becomes a WebAssembly component. The assets carry over as read-only files under the package scope. The file inventory and the signature are new.</figcaption>
</figure>

| Part | `.iliumanim` | `.iliumpkg` |
|---|---|---|
| Script | JavaScript run by the V8 engine | `scene.wasm`, a component for `ilium:anim/base@0.1.0` |
| Manifest | Old keys, no `publisher` | snake_case keys, `publisher` required, unknown keys refused |
| File inventory | `files` with `sha256` and `bytes` | `files` with `path`, `length` and `sha256` |
| Rights | `capabilities` as `{id, scope}` objects | `capabilities.scene` as grant strings |
| Limits | `heap_bytes`, `render_ms` and others | `memory_mib`, `frame_deadline_ms` and others |
| Pre-rendered clips | `modes: pre-rendered` with `clip_seconds` | `clip_seconds`, and the `prerender` object |
| Text over pixels | none | `glyph_plane: true` |
| Fonts, large assets | none | font assets up to 4 MiB each, package assets up to 64 MiB (256 MiB for T1) |

Every entry other than `manifest.json` and `signature` must appear in `files`. The `api` value in the manifest must equal the `ilium:api-version` section of `scene.wasm`, which is `ilium:plugin@0.1.0`.

## Porting steps

1. Create the project from the old package. The porting command writes the new manifest, copies the assets and creates a scene skeleton. Choose the language with `--lang`.

   **Rust**

   ```bash
   ilium plugin new aurora --side scene --lang rust \
     --id com.example.aurora --from-iliumanim ./aurora.iliumanim
   ```

   **TypeScript**

   ```bash
   ilium plugin new aurora --side scene --lang typescript \
     --id com.example.aurora --from-iliumanim ./aurora.iliumanim
   ```

   The command translates keys and copies assets. It does not translate render code: there is no runtime shim for the old JavaScript API.

2. Set `publisher`. The old format had no such key and the new manifest requires it.
3. Port the render code to the renderer contract below. Each old `entry.mjs` function maps to one renderer member.
4. Replace each old right with the grant string or scene flag in the rights table.
5. Check the limits against the limits table and remove any key that has no counterpart.
6. Build and pack. For TypeScript, run `bun install` and `bun test` first. `ilium plugin build` compiles the component and `ilium plugin pack` writes `dist/<id>-<version>.iliumpkg`. The pack step fills in `files`, so a source manifest may omit it.
7. Install and read the log. Install verifies the inventory, the `api` value and the signature, if any.

   ```bash
   ilium plugin install dist/com.example.aurora-0.1.0.iliumpkg --allow-unsigned
   ilium plugin logs com.example.aurora
   ```

   `--allow-unsigned` installs a local build as tier T3. For a shared package, sign it with `ilium plugin sign` first.

## Manifest mapping

Each old key maps to one new key. The table covers every key of the old format.

| Old key | New key |
|---|---|
| `api_version` (1) | `api` (`"ilium:plugin@0.1.0"`) |
| `id` (lowercase, hyphen, at most 80) | `id` (reverse DNS, for example `com.example.wave`) |
| `name`, `version`, `description` | `name`, `version`, `description` |
| none | `publisher` (required) |
| `entry` (`entry.mjs`) | `scene.entry` (`scene.wasm`) and `scene.world` (`ilium:anim/base@0.1.0`) |
| `modes` | `contributes.scenes[].modes` |
| `capabilities` (`[{id, scope}]`) | `capabilities.scene` (grant strings) |
| `settings` (JSON schema) | `contributes.scenes[].settings_schema` |
| `assets` | files under `assets/`, listed in `files` |
| `files` (`[{path, sha256, bytes}]`) | `files` (`[{path, length, sha256}]`) |
| `limits.heap_bytes` | `limits.memory_mib` (rounded up) |
| `limits.frame_bytes` | removed, because the host sizes the surface |
| `limits.render_ms` | `limits.frame_deadline_ms` |
| `limits.preparation_ms` | `contributes.jobs` with `{ "kind": "prepare", "deadline_ms": ... }` |
| `limits.clip_seconds` | `contributes.scenes[].clip_seconds` (at most 30) |

Each `contributes.scenes` entry has `id`, `title`, `family`, `modes`, `settings_schema` and the optional `frame_rate_cap` (1 to 60, default 30) and `clip_seconds` (default 10, at most 30). `family` is one of `natural`, `procedural`, `geometry`, `simulation`, `live-data`, `maps-and-space`, `saved-world`, `media` or `other`. `modes` is `live` or `pre-rendered`, and the first one is the default. Colour, palette and dithering are user settings, so a scene never declares them.

The contract adds two optional fields to a `contributes.scenes` entry:

| Field | Value | Meaning |
|---|---|---|
| `prerender` | `{ "frames": 1 to 3600, "fps": 1 to 30, "cache_mib": 1 to 128 }` | The host calls `render` for that many frames once, caches them and loops them with no further plugin calls. A change of settings or palette discards the cache. Refused with `manifest.scene-prerender` when a value is out of range. A scene that answers `true` from `wants_pointer` is played from the cache with no pointer: `pointer` returns `none` and the host logs a warning. |
| `glyph_plane` | `true` or `false` | Lets the scene write text cells over its pixels with `write-glyphs`, at most 131,072 cells per frame. The glyph colours still pass through the shared look. |

The old `clip_seconds` value converts to frames as `clip_seconds` times the frame rate. For a 12-second clip at 24 fps that is `{ "frames": 288, "fps": 24, "cache_mib": 64 }`.

The manifest below is the port of the example scene in this guide. The Rust port and the TypeScript port use the same manifest, because the manifest is JSON and does not depend on the language.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.aurora",
  "name": "Aurora",
  "version": "0.1.0",
  "publisher": "Example Publisher",
  "scene": {
    "entry": "scene.wasm",
    "world": "ilium:anim/base@0.1.0"
  },
  "contributes": {
    "scenes": [
      {
        "id": "aurora",
        "title": "Aurora",
        "family": "natural",
        "modes": ["live"],
        "settings_schema": {
          "type": "object",
          "properties": {
            "speed": { "type": "number", "minimum": 0.1, "maximum": 4, "default": 1 }
          }
        }
      }
    ]
  }
}
```

The `scene` object lists `exports` only for optional exports, such as `job-runner`. This source manifest omits `files`, which `ilium plugin pack` writes.

## Rights mapping

The old right names are not grant strings. Each one maps as follows.

| Old right | New form | Notes |
|---|---|---|
| `network.http` | `http:<host>` | Name each host, for example `http:api.example.com`. Wildcards such as `*.openstreetmap.org` are allowed. |
| `audio.microphone` | `audio.capture` | High risk. Always needs an explicit user grant. |
| `audio.loopback` | `audio.loopback` | Unchanged. |
| `location.observer` | `location.observer` | Unchanged. High risk. |
| `input.pointer` | `wants-pointer` renderer method | Not a grant string. The scene answers `true` from `wants_pointer`. |
| `screen.occlusion` | `wants-occupancy` renderer method | Not a grant string. The scene answers `true` from `wants_occupancy`. |
| `state.persist` | always-granted `storage` | No grant needed. Use the `storage` import. |
| `device.gpu` | none | No plugin equivalent. The grant `gpu.kernels` is `@unstable` and appears only in development builds. It serves host kernels only and gives a plugin no shaders (decision D18). A scene that needs a GPU cannot be ported yet. |

<div class="warn">

<p>A scene that depends on <code>device.gpu</code> is not portable. Do not work around it with a native helper process inside the package. The install refuses the grant with <code>manifest.grant-unknown</code>.</p>

</div>

## Limits

The table lists the scene-relevant keys with their defaults and the ceilings for tier T2 and T3 packages. A runtime scene limit above its ceiling is clamped and logged. The two install-time limits, package assets and the `prerender` cache, are refused at install instead. The user can lower any value under `[plugins.limits]` in `config.toml`.

| Key | Default | Ceiling (T2 and T3) | Effect when exceeded |
|---|---|---|---|
| `memory_mib` | 32 | 128 | Trap |
| `table_elements` | 10,000 | 20,000 | Trap |
| `frame_deadline_ms` | the frame budget | 50 | Frame skipped. Thirty breaches in a row trap. |
| Package assets (`package_assets_mib`) | 64 | 64 (256 for T1) | Install refused with `manifest.asset-size` |
| `prerender` cache | none | 3,600 frames, 30 fps, 128 MiB | Install refused with `manifest.scene-prerender` |
| Glyph plane cells | none | 131,072 per frame | The write is refused |
| `activate_deadline_ms` | 2,000 | 2,000 | Trap |
| `deactivate_deadline_ms` | 500 | 500 | Instance dropped without restore bytes |

The frame budget is 1,000 divided by the effective frame rate, rounded down to whole milliseconds. At the default 30 frames per second it is 33 ms.

## Renderer contract

The old `entry.mjs` functions become the members of the `renderer` resource. The table gives the Rust and TypeScript names and the default when a member is omitted. [Scenes API](/pages/api-scenes/) has the full surface.

| Member | Rust | TypeScript | Default |
|---|---|---|---|
| Constructor | `fn new(spec: SceneSpec) -> Self` | `constructor(spec)` | none |
| Readiness | `fn readiness(&self) -> Readiness` | `readiness()` | `Ready` |
| Frame rate | `fn frames_per_second(&self) -> u32` | `framesPerSecond()` | 30 |
| Pointer | `fn wants_pointer(&self) -> bool` | `wantsPointer()` | `false` |
| Occupancy | `fn wants_occupancy(&self) -> bool` | `wantsOccupancy()` | `false` |
| Draw | `fn render(&mut self, frame: FrameInput, surface: &FrameSurface)` | `render(frame, surface)` | required |
| Palette | `fn set_palette(&mut self, palette_rgb: Vec<u32>)` | `setPalette(palette)` | ignored |
| Settings | `fn reconfigure(&mut self, settings_json: String) -> bool` | `reconfigure(settings)` | `false` |
| Status text | `fn status(&self) -> Option<String>` | `status()` | none |

The host reads `frames_per_second`, `wants_pointer` and `wants_occupancy` once after the constructor and after each `reconfigure` that returns `true`. These flags replace the old `input.pointer` and `screen.occlusion` rights.

## Example 1: a live scene

This port draws a drifting grey band at the default 30 frames per second and skips cells covered by a panel.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::anim::renderer::{FrameInput, Renderer, SceneSpec};
use ilium_plugin_sdk::anim::surface::FrameSurface;

/// Port of the old Aurora scene: a drifting grey band in live mode.
#[ilium_plugin(side = "scene")]
#[derive(Default)]
struct Aurora;

impl Lifecycle for Aurora {}

impl Renderer for Aurora {
    fn new(_spec: SceneSpec) -> Self {
        Aurora
    }

    fn wants_occupancy(&self) -> bool {
        true
    }

    fn render(&mut self, frame: FrameInput, surface: &FrameSurface) {
        let width = frame.width as usize;
        let height = frame.height as usize;
        let phase = frame.time_millis as f32 / 1000.0;
        let covered = surface.occupancy();
        let mut tones = vec![0u8; width * height];
        for row in 0..height {
            let band = ((row as f32 / height as f32) * 6.0 + phase).sin();
            let tone = ((band * 0.5 + 0.5) * 255.0) as u8;
            for column in 0..width {
                let index = row * width + column;
                let is_covered = covered.as_ref().is_some_and(|map| map[index] != 0);
                tones[index] = if is_covered { 0 } else { tone };
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

/** Port of the old Aurora scene: a drifting grey band in live mode. */
class Aurora {
  constructor(_spec: SceneSpec) {}

  wantsOccupancy(): boolean {
    return true;
  }

  render(frame: FrameInput, surface: FrameSurface): void {
    const tones = new Uint8Array(frame.width * frame.height);
    const phase = Number(frame.timeMillis) / 1000;
    const covered = surface.occupancy(); // Uint8Array | undefined
    for (let row = 0; row < frame.height; row++) {
      const band = Math.sin((row / frame.height) * 6 + phase) * 0.5 + 0.5;
      const tone = Math.round(band * 255);
      for (let column = 0; column < frame.width; column++) {
        const index = row * frame.width + column;
        tones[index] = covered !== undefined && covered[index] !== 0 ? 0 : tone;
      }
    }
    surface.writeTones(0, tones);
    surface.present();
  }
}

export const { lifecycle, renderer } = defineScene(Aurora);
```

`surface.occupancy()` returns `None` in Rust and `undefined` in TypeScript when `wants_occupancy` is `false`. A `render` call that returns without `present` drops the frame.

## Example 2: a scene with a setting

This port reads the `speed` setting from `settings_schema`. The host validates new settings before it calls `reconfigure`, so an invalid value never reaches the scene. The guard below narrows the `unknown` type and keeps the previous value when the shape is wrong.

**TypeScript**

```ts
import { defineScene } from '@ilium/plugin-sdk';
import type { FrameInput, SceneSpec } from '@ilium/plugin-sdk/anim/renderer';
import type { FrameSurface } from '@ilium/plugin-sdk/anim/surface';

interface AuroraSettings {
  speed: number;
}

function readSettings(value: unknown): AuroraSettings | undefined {
  if (typeof value !== 'object' || value === null) return undefined;
  const speed = (value as { speed?: unknown }).speed;
  return typeof speed === 'number' ? { speed } : undefined;
}

class Aurora {
  private speed = 1;

  constructor(spec: SceneSpec) {
    this.speed = readSettings(spec.settings)?.speed ?? this.speed;
  }

  framesPerSecond(): number {
    return 30;
  }

  render(frame: FrameInput, surface: FrameSurface): void {
    const tones = new Uint8Array(frame.width * frame.height);
    const phase = (Number(frame.timeMillis) / 1000) * this.speed;
    for (let row = 0; row < frame.height; row++) {
      const band = Math.sin((row / frame.height) * 6 + phase) * 0.5 + 0.5;
      const start = row * frame.width;
      tones.fill(Math.round(band * 255), start, start + frame.width);
    }
    surface.writeTones(0, tones);
    surface.present();
  }

  reconfigure(settings: unknown): boolean {
    const next = readSettings(settings);
    if (next === undefined) return false;
    this.speed = next.speed;
    return true;
  }
}

export const { lifecycle, renderer } = defineScene(Aurora);
```

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::anim::renderer::{FrameInput, Renderer, SceneSpec};
use ilium_plugin_sdk::anim::surface::FrameSurface;
use serde::Deserialize;

#[derive(Deserialize)]
struct AuroraSettings {
    speed: f32,
}

/// Port of the old Aurora scene with a `speed` setting.
#[ilium_plugin(side = "scene")]
#[derive(Default)]
struct Aurora {
    speed: f32,
}

impl Lifecycle for Aurora {}

impl Renderer for Aurora {
    fn new(spec: SceneSpec) -> Self {
        // A spec that cannot be read keeps the default speed of 1.
        let speed = serde_json::from_value::<AuroraSettings>(spec.settings.clone())
            .map(|settings| settings.speed)
            .unwrap_or(1.0);
        Aurora { speed }
    }

    fn render(&mut self, frame: FrameInput, surface: &FrameSurface) {
        let width = frame.width as usize;
        let phase = (frame.time_millis as f32 / 1000.0) * self.speed;
        let mut tones = vec![0u8; width * frame.height as usize];
        for row in 0..frame.height as usize {
            let band = ((row as f32 / frame.height as f32) * 6.0 + phase).sin() * 0.5 + 0.5;
            tones[row * width..(row + 1) * width].fill((band * 255.0).round() as u8);
        }
        surface.write_tones(0, &tones);
        surface.present();
    }

    fn reconfigure(&mut self, settings_json: String) -> bool {
        match serde_json::from_str::<AuroraSettings>(&settings_json) {
            Ok(next) => {
                self.speed = next.speed;
                true
            }
            Err(_) => false,
        }
    }
}
```

Returning `false` from `reconfigure` is not a failure. The host discards the renderer, builds a new one from a new spec and logs a restart.

## Example 3: a pre-rendered loop and a text layer

An old `pre-rendered` scene with `clip_seconds` becomes a scene with a `prerender` object. The scene code does not change: `render` is called once for each cached frame, and the host loops the result. The manifest fragment for a 12-second loop at 24 frames per second with a caption layer:

```json
{
  "contributes": {
    "scenes": [
      {
        "id": "aurora",
        "title": "Aurora",
        "family": "natural",
        "modes": ["live", "pre-rendered"],
        "prerender": { "frames": 288, "fps": 24, "cache_mib": 64 },
        "glyph_plane": true,
        "settings_schema": { "type": "object", "properties": {} }
      }
    ]
  }
}
```

The surface then has these methods, in WIT:

```wit
// Excerpt of ilium:anim/surface in reference-wit
write-glyphs: func(cells: list<glyph-cell>) -> result<_, plugin-error>;
clear-glyphs: func();
set-owner-ids: func(first-row: u32, ids: list<u16>) -> result<_, plugin-error>;
last-receipt: func() -> option<frame-receipt>;
```

In Rust these are the methods `write_glyphs`, `clear_glyphs`, `set_owner_ids` and `last_receipt` of `FrameSurface`. In TypeScript they are `writeGlyphs`, `clearGlyphs`, `setOwnerIds` and `lastReceipt`, with `u64` values as `bigint`. A `prerender` scene cannot declare `wants-pointer`, because a cached loop cannot react to the pointer. A live scene that needs hit tests keeps `wants_pointer` and uses the owner ids and the frame receipt instead of its own copy of the frame. Fonts come from package assets (TTF or OTF, at most 4 MiB each) through the `font` resource, which rasterizes text into a tone mask.

## Edge cases and errors

| Situation | Result |
|---|---|
| An `.iliumanim` file passed to the new loader | Refused with `package.old-format`. Logged as `old format, not loaded`. |
| A manifest key with no counterpart, including a `wants-pointer` key in a manifest or a key in `limits` | Refused at install with `manifest.unknown-key` |
| A file missing, extra or different from its inventory entry | Refused with `manifest.file-unlisted`, `manifest.file-missing`, `manifest.file-length` or `manifest.file-hash`, depending on the difference. No code from the package runs. |
| An `api` value or `ilium:api-version` section that differs from `ilium:plugin@0.1.0` | Refused with `manifest.api-version` when the section differs from the manifest `api`, and with `package.api-unsupported` when the version is outside the window the host loads |
| Old right names, or `device.gpu`, in `capabilities.scene` | Refused with `manifest.grant-unknown` |
| A `frame_rate_cap` above 60, or a runtime scene limit above its ceiling | Clamped to the ceiling and logged. Not refused. `package_assets_mib` and the `prerender` cache above their ceilings are refused at install |
| `settings` that fail `settings_schema` | Never reach the scene. The previous values stay in force. |
| `readiness` stays `Preparing` for more than 10 s | Counts as one failure. A `Failed` readiness stops `render` until the renderer restarts. |
| A `render` call that returns without `present` | The frame is dropped. A warn line appears at most once per second. |
| `prerender` values out of range | Refused at install with `manifest.scene-prerender` |
| `prerender` together with a scene that answers `true` from `wants_pointer` | Not refused. Played from the cache, `pointer` returns `none`, a warning is logged |
| Package assets over 64 MiB (256 MiB for T1), or a font over 4 MiB | Refused at install with `manifest.asset-size` |
| A glyph plane write without `glyph_plane: true` | The call returns `unsupported` ([glyph plane errors](/pages/api-surface-glyph-plane/)) |

Failures and clamps are written to the plugin log. Read them with `ilium plugin logs <id>`.

## What the user sees

The scene appears in the animation settings list under its `title`, with its `status` text beside the title while it is set. The shared rows (colour mode, palette, speed, background, placement) apply to it without any code in the scene. The install review dialog lists the scene's rights before any code runs.

<figure>
<pre class="term"><span class="t-hi">╭  ≡ ● · Animations────────────────────────────────────────────╮</span>
│ Scene <span class="t-dim">………………………………………………………………………………………………………</span> ← Aurora … → + │
│   <span class="t-dim">aurora 0.1.0, 30 fps</span>                                       │
│ Colour mode <span class="t-dim">…………………………………………………………………………………………</span> ← Color … → + │
│ Palette <span class="t-dim">…………………………………………………………………………………………………</span> ← Sunset … → + │
│ Speed        ──────────────<span class="t-hi">●</span>─────────────────  - 1 + *       │
│ Show background <span class="t-dim">…………………………………………………………………………………………………</span> [ On ] │
│ Panel placement <span class="t-dim">…………………………………………………………………………………</span> ← Both … → + │
│ <span class="t-dim">↑/↓ navigate · ←/→ change · Enter edit · Esc close</span>           │
╰──────────────────────────────────────────────────────────────╯
</pre>
<figcaption>Illustrative, not generated. The animation settings panel with the ported scene selected. It uses the choice row buttons <code>←</code> <code>→</code> <code>+</code>, the slider and stepper, the bracketed toggle and the grey <code>…</code> leaders of the real client.</figcaption>
</figure>

The terminal session for the install looks like this:

<pre class="term"><span class="t-prompt">$</span> ilium plugin install dist/com.example.aurora-0.1.0.iliumpkg \
  --allow-unsigned
<span class="t-ok">ok</span>   com.example.aurora 0.1.0: 3 files match the inventory
<span class="t-warn">warn</span> unsigned package, tier T3 (dev badge shown)
<span class="t-ok">ok</span>   scene aurora registered in the animation settings list
<span class="t-prompt">$</span> ilium plugin logs com.example.aurora
<span class="t-dim">2026-10-11 11:30:08</span> <span class="t-err">err</span>  aurora.iliumanim: package.old-format, not loaded
<span class="t-dim">2026-10-11 11:30:09</span> <span class="t-warn">warn</span> device.gpu: manifest.grant-unknown, not installed
</pre>

Illustrative, not generated. The first block shows a successful install of a rebuilt scene. The log shows the old file refused and a port that still asks for `device.gpu`.

## Engine during the migration

The existing V8 engine keeps running the `.iliumanim` packages that are already installed, until the animation migration ends (Phase 4c, see the [roadmap](/pages/roadmap/)). That engine is an internal detail of Ilium, not a second package format. A new `.iliumpkg` is always a new build, and once a scene is rebuilt, its component runs under the single Wasmtime host.

## See also

- [Manifest reference](/pages/reference-manifest/)
- [Capability table](/pages/reference-capabilities/)
- [Scenes API](/pages/api-scenes/)
- [Rust SDK](/pages/rust-sdk/)
- [TypeScript SDK](/pages/typescript-sdk/)
- [Packages](/pages/packages/)
- [Design decisions](/pages/design-decisions/)
- [Tutorial step 7](/pages/tutorial-07-ambient-scene/) builds a scene from scratch.
- [Tutorial step 9](/pages/tutorial-09-package-and-share/) packs and signs the result.

Checked against the API version `ilium:plugin@0.1.0`.
