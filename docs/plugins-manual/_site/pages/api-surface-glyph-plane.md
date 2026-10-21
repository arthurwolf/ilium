# Glyph plane and frame receipts

## Glyph plane and frame receipts

The glyph plane and frame receipts are part of the surface of the single contract `ilium:plugin@0.1.0`. There is no second contract version. The surface is the extended pixel surface that ambient scenes draw into (see [scenes](/pages/api-scenes/)). It adds a glyph plane for text over the pixels, owner ids so a scene can learn which of its objects is under the pointer, a receipt for every presented frame, fonts rasterized by the host, pre-rendered loops, and larger package assets. It does not change the rule that the shared look, dithering and the palette are applied by the host: glyph colours go through the shared look too, so a scene never escapes the global appearance.

### What you need

- **Grant.** None for the surface. Video from bytes needs `media.video` (see [media](/pages/api-media/)).
- **Manifest.** In `contributes.scenes`: `glyph_plane: true` for the glyph plane, and `prerender: { frames, fps, cache_mib }` for a looped scene. `prerender` and `wants-pointer` exclude each other (`manifest.scene-prerender`).
- **Sides.** The scene (A) world. Fonts are always available there.

```json
"contributes": { "scenes": [{ "id": "lanes", "title": "Lanes", "glyph_plane": true }] }
```

### Interface

```wit
// Excerpt of ilium:anim/surface in reference-wit
record glyph-cell { column: u16, row: u16, text: string, fg: option<u32>, bg: option<u32>, add: modifiers }
write-glyphs: func(cells: list<glyph-cell>) -> result<_, plugin-error>;       // glyph_plane: true; <= 131,072 cells per frame
clear-glyphs: func();
set-owner-ids: func(first-row: u32, ids: list<u16>) -> result<_, plugin-error>;   // one id per pixel, 0 = none
record frame-receipt { frame: u64, presented-at-millis: u64, dropped-before: u32, pointer-owner: option<u16> }
last-receipt: func() -> option<frame-receipt>;
```

```wit
// Excerpt of ilium:anim/fonts in reference-wit
resource font {
  constructor(asset: string);
  rasterize: func(text: string, size-px: u16) -> result<tuple<u32, u32, list<u8>>, plugin-error>;
}
```

| Method | Rust (`Surface`) | TypeScript | Notes |
|---|---|---|---|
| `write-glyphs` | `write_glyphs(&[GlyphCell])` | `writeGlyphs(cells)` | Text cells drawn over the pixels after dithering. Icon widths apply. |
| `clear-glyphs` | `clear_glyphs()` | `clearGlyphs()` | Removes every glyph cell. |
| `set-owner-ids` | `set_owner_ids(first_row, &[u16])` | `setOwnerIds(firstRow, ids)` | One id per pixel from `first_row`, 0 means none. |
| `last-receipt` | `last_receipt()` | `lastReceipt()` | None before the first frame is presented. |
| `font` | `Font::new(asset)`, `.rasterize(text, size_px)` | `new Font(asset)`, `.rasterize(text, sizePx)` | Returns width, height and `gray8` bytes. |

### The glyph plane

A glyph cell is one terminal cell of text with optional foreground, background (RGB `0xRRGGBB`) and modifiers. The host draws it over the dithered pixels with the same glyph widths the tree uses, so a wide icon takes two cells. Glyph colours are recoloured by the shared look, brightness, contrast and palette included. Use the plane for labels, scores and axes; keep pictures in the pixels.

<figure>
<pre class="term">⠀⠀⣀⣤⣶⣿⣿⣶⣤⣀⠀⠀⠀⠀⣀⣤⣶⣿⣿⣶⣤⣀⠀⠀⠀⠀
⠀⣴⣿⣿⣿⣿⣿⣿⣿⣿⣦⠀⣴⣿⣿⣿⣿⣿⣿⣿⣿⣦⠀⠀⠀
⠀⠻⣿⣿⣿⣿⣿⣿⣿⣿⠟⠀⠻⣿⣿⣿ Lane 2 ⣿⣿⠟⠀⠀⠀
⠀⠀⠈⠛⠿⣿⣿⠿⠛⠁⠀⠀⠀⠈⠛⠿⣿⣿⠿⠛⠁⠀⠀⠀⠀
</pre>
<figcaption>Pixels shown as Braille cells (two columns by four rows of dots per cell, as the client draws scenes). The words "Lane 2" are glyph cells over them.</figcaption>
</figure>

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;

fn label(surface: &mut Surface, column: u16, row: u16) -> Result<(), PluginError> {
    surface.clear_glyphs();
    let cells: Vec<GlyphCell> = "Lane 2".chars().enumerate().map(|(index, ch)| GlyphCell {
        column: column + index as u16, row, text: ch.to_string(),
        fg: Some(0xffffff), bg: None, add: Modifiers::BOLD,
    }).collect();
    surface.write_glyphs(&cells)
}
```

**TypeScript**

```ts
import type { Surface } from '@ilium/plugin-sdk';

export function label(surface: Surface, column: number, row: number): void {
  surface.clearGlyphs();
  const cells = [...'Lane 2'].map((text, index) => ({
    column: column + index, row, text, fg: 0xffffff, bg: undefined, add: { bold: true },
  }));
  surface.writeGlyphs(cells);
}
```

### Owner ids and receipts

`set-owner-ids` tells the host which logical object each pixel belongs to (a lane, a ship, a button). The receipt of the last presented frame names `pointer-owner`, the owner under the pointer, so a scene with `wants-pointer` hit-tests without keeping its own copy of the frame. `dropped-before` counts frames the host skipped to keep pace, which lets a scene advance its clock by real time.

<figure>
<svg viewBox="0 0 440 130" role="img" aria-label="Owner ids and the frame receipt">
<title>Owner ids</title>
<desc>The scene writes pixels and owner ids. The host presents the frame, finds the owner under the pointer and returns it in the receipt.</desc>
<defs><marker id="s2" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker></defs>
<rect x="8" y="14" width="130" height="40" rx="6" fill="none" stroke="currentColor"/><text x="73" y="38" font-size="12" text-anchor="middle" fill="currentColor">render: pixels + ids</text>
<line x1="138" y1="34" x2="180" y2="34" stroke="currentColor" marker-end="url(#s2)"/>
<rect x="180" y="14" width="120" height="40" rx="6" fill="none" stroke="currentColor"/><text x="240" y="38" font-size="12" text-anchor="middle" fill="currentColor">host presents</text>
<line x1="300" y1="34" x2="340" y2="34" stroke="currentColor" marker-end="url(#s2)"/>
<rect x="340" y="14" width="92" height="40" rx="6" fill="none" stroke="currentColor"/><text x="386" y="38" font-size="12" text-anchor="middle" fill="currentColor">receipt</text>
<line x1="386" y1="54" x2="386" y2="92" stroke="currentColor" marker-end="url(#s2)"/>
<text x="240" y="108" font-size="11" text-anchor="middle" fill="currentColor">next render reads last-receipt: pointer-owner, dropped-before</text>
</svg>
<figcaption>The pointer is resolved by the host against the ids the scene wrote.</figcaption>
</figure>

**Rust**

```rust
fn hover_lane(surface: &Surface) -> Option<u16> {
    surface.last_receipt().and_then(|receipt| receipt.pointer_owner)
}
fn mark_lanes(surface: &mut Surface, width: u32, height: u32) -> Result<(), PluginError> {
    let ids: Vec<u16> = (0..width * height).map(|i| 1 + ((i % width) * 4 / width) as u16).collect();
    surface.set_owner_ids(0, &ids)
}
```

**TypeScript**

```ts
export const hoverLane = (surface: Surface): number | undefined => surface.lastReceipt()?.pointerOwner;
export function markLanes(surface: Surface, width: number, height: number): void {
  const ids = Array.from({ length: width * height }, (_, i) => 1 + Math.floor(((i % width) * 4) / width));
  surface.setOwnerIds(0, ids);
}
```

### Fonts

Fonts are package assets (TTF or OTF, at most 4 MiB each; `manifest.asset-size` otherwise). `rasterize` returns `(width, height, gray8)`, which a scene blends into its raster with its own colour. Package assets as a whole may be up to 64 MiB.

### Pre-rendered loops

With `prerender: { frames, fps, cache_mib }` the host calls `render` for the declared frame count once, caches the frames (at most 3,600 frames, 30 fps, 128 MiB) and plays them in a loop with no further plugin calls. A change to the scene's settings or palette invalidates the cache and the host renders again.

### Errors and limits

| Case | Error |
|---|---|
| Cell outside the surface, id rows outside | `invalid-argument` |
| More than 131,072 glyph cells per frame | `quota-exceeded` |
| `write-glyphs` without `glyph_plane` | `unsupported` |
| Font asset over 4 MiB | `manifest.asset-size` |
| `prerender` out of range or with `wants-pointer` | `manifest.scene-prerender` |

### See also

- [scenes](/pages/api-scenes/), [media](/pages/api-media/), [pointer](/pages/api-pointer/), [Capabilities reference](/pages/reference-capabilities/) and [Errors and limits](/pages/reference-errors-and-limits/).
