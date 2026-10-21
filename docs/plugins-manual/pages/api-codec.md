---
layout: page
title: codec
permalink: /pages/api-codec/
---

## codec

The `codec` import does the byte-level work that plugins need without shipping their own libraries. It inflates and deflates data, reads image sizes, decodes images to RGBA pixels, computes SHA-256 digests and rasterises text into a grey image. Every function is synchronous and runs in the host's native code, so a plugin never bundles a decoder and never pays for one in WebAssembly.

`codec` does no input or output itself. A plugin gets its bytes from [fs](/pages/api-fs/) or [http](/pages/api-http/), passes them to `codec`, and writes the result to a pane or a file.

**You will need:**

- No grant. `codec` is always granted in the server, client and scene worlds, and in job instances.
- Rust: the `ilium-plugin-sdk` crate, module `ilium_plugin_sdk::host::codec`.
- TypeScript: the `@ilium/plugin-sdk` package, subpath `@ilium/plugin-sdk/host/codec`.
- Inputs of up to 16 MiB per call. Larger work belongs in a job, see [background jobs](/pages/background-jobs/).

### Interface

Interface version: `ilium:host/codec@0.1.0`.

```wit
// Excerpt of ilium:host/codec in reference-wit
enum compression { zlib, gzip, deflate-raw }
record image-info { width: u32, height: u32, frames: u32 }
record decoded-image { width: u32, height: u32, rgba: list<u8> }
record raster-text { text: string, size-px: u16, max-width-px: u32 }
inflate: func(format: compression, data: list<u8>, max-output: u32)
    -> result<list<u8>, plugin-error>;
deflate: func(format: compression, data: list<u8>, level: u8)
    -> result<list<u8>, plugin-error>;
image-info: func(data: list<u8>) -> result<image-info, plugin-error>;
decode-image: func(data: list<u8>, frame: u32, max-pixels: u32)
    -> result<decoded-image, plugin-error>;
sha256: func(data: list<u8>) -> list<u8>;
rasterize-text: func(request: raster-text) -> result<decoded-image, plugin-error>;
```

Names follow the SDK naming rules. In Rust the functions are snake_case and the enum cases are `Compression::Zlib`, `Compression::Gzip` and `Compression::DeflateRaw`. In TypeScript the functions are camelCase (`imageInfo`, `decodeImage`) and the format is the string `'zlib'`, `'gzip'` or `'deflate-raw'`.

### Functions

| Function | Parameters | Returns | What it does |
|---|---|---|---|
| `inflate` | `format`, `data`, `max-output: u32` | bytes | Decompresses zlib, gzip or raw deflate data. Refused if the output is longer than `max-output`, which may be at most 64 MiB. |
| `deflate` | `format`, `data`, `level: u8` | bytes | Compresses data. `level` runs from 0 to 9. Higher levels are slower and produce smaller output. |
| `image-info` | `data` | `image-info` | Reads the width, height and frame count from the header. It does not decode any pixels. |
| `decode-image` | `data`, `frame: u32`, `max-pixels: u32` | `decoded-image` | Decodes one frame, counted from 0, to 8-bit RGBA. Refused if the image has more pixels than `max-pixels`. |
| `sha256` | `data` | 32 bytes | Returns the SHA-256 digest. It cannot fail, so it has no error channel. |
| `rasterize-text` | `request: raster-text` | `decoded-image` | Draws text as white pixels. The alpha channel holds the ink tone: 0 is background, 255 is full ink. Text wraps at word boundaries to `max-width-px`. |

Records:

| Record | Fields | Meaning |
|---|---|---|
| `image-info` | `width`, `height`, `frames` (all `u32`) | Size in pixels and number of frames. A still image has one frame. |
| `decoded-image` | `width`, `height` (`u32`), `rgba` (bytes) | Pixels in RGBA order, 4 bytes per pixel, so `rgba` is `width × height × 4` bytes. |
| `raster-text` | `text` (string), `size-px` (`u16`), `max-width-px` (`u32`) | The text, its font size in pixels and the widest line allowed. |

Each function is synchronous, so Rust and TypeScript code call it directly. Errors arrive as `PluginError`, see [errors](/pages/reference-errors-and-limits/).

The native crates behind the import are flate2, the image decoders, sha2 and cosmic-text. A plugin cannot choose another decoder.

### Limits

| Limit | Value | Refused with |
|---|---|---|
| Input of any function except `sha256` | 16 MiB | `invalid-argument` |
| `inflate` `max-output` | at most 64 MiB | `invalid-argument` |
| Pixels in `decode-image` | `max-pixels`, and never more than 67,108,864 | `invalid-argument` above 67,108,864; `quota-exceeded` for an image over `max-pixels` |
| `rasterize-text` text | 64 KiB | `invalid-argument` |
| `rasterize-text` `size-px` | 1 to 512 | `invalid-argument` |
| `rasterize-text` `max-width-px` | 1 to 16,384 | `invalid-argument` |

### Example 1: decode a PNG into a surface

The plugin reads a picture that the user picked, checks its size from the header, and decodes the first frame. The RGBA bytes then go to a `surface` in the plugin's pane. The `fs.read` call returns at most 1 MiB, so this example handles pictures up to that size. Read a larger file with `fs.read-all`, see [fs](/pages/api-fs/).

**Grants:** `fs.read:picked`

**Rust**

```rust
use ilium_plugin_sdk::host::codec::{self, DecodedImage, ImageInfo};
use ilium_plugin_sdk::host::{fs, ui};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ui::{PixelFormat, ViewTarget};

const MAX_PIXELS: u32 = 4_000_000;

fn load_picture(path: FsPath) -> Result<DecodedImage, PluginError> {
    let bytes = fs::read(&path, 0, 1_048_576)?;
    let info: ImageInfo = codec::image_info(&bytes)?;
    if u64::from(info.width) * u64::from(info.height) > u64::from(MAX_PIXELS) {
        return Err(PluginError {
            code: ErrorCode::InvalidArgument,
            message: "the picture has more than 4 megapixels".to_string(),
            retry_after_ms: None,
        });
    }
    codec::decode_image(&bytes, 0, MAX_PIXELS)
}

fn show_picture(view: ViewTarget, picture: &DecodedImage) -> Result<(), PluginError> {
    ui::write_surface(&view, "picture", 0, PixelFormat::Rgba8, &picture.rgba)
}
```

**TypeScript**

```ts
import { PluginError } from '@ilium/plugin-sdk';
import * as codec from '@ilium/plugin-sdk/host/codec';
import * as fs from '@ilium/plugin-sdk/host/fs';
import * as ui from '@ilium/plugin-sdk/host/ui';
import type { FsPath, ViewTarget } from '@ilium/plugin-sdk/types';

const MAX_PIXELS = 4_000_000;

export function loadPicture(path: FsPath): codec.DecodedImage {
  const bytes = fs.read(path, 0n, 1_048_576);
  const info = codec.imageInfo(bytes);
  if (info.width * info.height > MAX_PIXELS) {
    throw new PluginError('invalid-argument', 'the picture has more than 4 megapixels');
  }
  return codec.decodeImage(bytes, 0, MAX_PIXELS);
}

export function showPicture(view: ViewTarget, picture: codec.DecodedImage): void {
  ui.writeSurface(view, 'picture', 0, 'rgba8', picture.rgba);
}
```

For a 640 × 480 picture, `image_info` returns `width 640, height 480, frames 1`. The decoded image holds 640 × 480 × 4 = 1,228,800 bytes. The size cap of 4,000,000 pixels gives 16,000,000 bytes of RGBA, which fits the 16 MiB limit of `ui.write-surface`.

<figure>
<pre class="term"><span class="t-dim">╭  ≡ ● · picture.png</span> <span class="t-dim">640 x 480 ──────────────────────────╮</span>
<span class="t-dim">│</span>· - - = = ≡ ≡ ≣ ≣ █ █ █ █ ≣ ≣ ≡ ≡ = = - - ·        <span class="t-dim">     │</span>
<span class="t-dim">│</span>- - = = ≡ ≡ ≣ ≣ █ █ █ █ █ █ ≣ ≣ ≡ ≡ = = - -      <span class="t-dim">       │</span>
<span class="t-dim">│</span>= = ≡ ≡ ≣ ≣ █ █ █ █ █ █ █ █ █ █ ≣ ≣ ≡ ≡ = =      <span class="t-dim">       │</span>
<span class="t-dim">│</span>≡ ≡ ≣ ≣ █ █ █ █ █ █ █ █ █ █ █ █ ≣ ≣ ≡ ≡ =        <span class="t-dim">       │</span>
<span class="t-dim">│</span>· - = ≡ ≣ █ █ █ █ █ █ █ █ █ █ █ ≣ ≡ = - ·        <span class="t-dim">       │</span>
<span class="t-dim">│</span>frame 1 of 1                                      <span class="t-dim">      │</span>
<span class="t-dim">╰────────────────────────────────────────────────────    ╯</span>
<span class="t-err">error</span> picture.png 2,400 x 2,000 has more than 4 megapixels, nothing was drawn
</pre>
<figcaption>Illustrative, not generated. The rounded border and the chrome title (the three icons, then the label) follow the client's panel code. The tone ramp is the client's minimap density ramp; the picture itself is invented.</figcaption>
</figure>

### Example 2: inflate a saved file and hash the result

The plugin reads a zlib-compressed file that the user picked, inflates it with a hard output cap, and computes the SHA-256 of the plain data. The cap guards against a small file that expands to a very large output.

**Grants:** `fs.read:picked`

**Rust**

```rust
use ilium_plugin_sdk::host::codec::{self, Compression};
use ilium_plugin_sdk::host::fs;
use ilium_plugin_sdk::prelude::*;

const MAX_INFLATED: u32 = 16_777_216;

/// Returns the inflated bytes and their 32-byte SHA-256 digest.
fn inflate_and_hash(path: FsPath) -> Result<(Vec<u8>, Vec<u8>), PluginError> {
    let packed = fs::read(&path, 0, 1_048_576)?;
    let plain = codec::inflate(Compression::Zlib, &packed, MAX_INFLATED)?;
    let digest = codec::sha256(&plain);
    Ok((plain, digest))
}
```

**TypeScript**

```ts
import * as codec from '@ilium/plugin-sdk/host/codec';
import * as fs from '@ilium/plugin-sdk/host/fs';
import type { FsPath } from '@ilium/plugin-sdk/types';

const MAX_INFLATED = 16_777_216;

export function inflateAndHash(path: FsPath): { plain: Uint8Array; digest: Uint8Array } {
  const packed = fs.read(path, 0n, 1_048_576);
  const plain = codec.inflate('zlib', packed, MAX_INFLATED);
  const digest = codec.sha256(plain); // always 32 bytes
  return { plain, digest };
}
```

### Where codec output goes

The `codec` interface (`ilium:host/codec@0.1.0`) has several consumers of its output. A decoded image can be written to a `surface` widget with `write-surface`, drawn in a scene, or kept as a frame of a pre-rendered loop. `rasterize-text` returns a decoded image whose alpha channel holds the ink tone, ready for `write-surface`. A font that ships in the package is rasterized by the scene's `font` resource instead (see [scenes](/pages/api-scenes/)); `rasterize-text` is for text drawn with the host's default font. Video frames come from [media](/pages/api-media/), not from `codec`.

<figure>
<svg viewBox="0 0 440 110" role="img" aria-label="Bytes flow from fs or http through codec to a surface">
<title>Codec in a pipeline</title>
<desc>Bytes from fs or http go to codec, which returns RGBA pixels or a digest. The pixels go to a surface widget or a scene.</desc>
<defs>
<marker id="codec-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker>
</defs>
<rect x="6" y="14" width="90" height="34" rx="6" fill="none" stroke="currentColor"/>
<text x="51" y="35" font-size="11" text-anchor="middle" fill="currentColor">fs read</text>
<rect x="6" y="62" width="90" height="34" rx="6" fill="none" stroke="currentColor"/>
<text x="51" y="83" font-size="11" text-anchor="middle" fill="currentColor">http fetch</text>
<line x1="96" y1="31" x2="150" y2="52" stroke="currentColor" marker-end="url(#codec-arrow)"/>
<line x1="96" y1="79" x2="150" y2="58" stroke="currentColor" marker-end="url(#codec-arrow)"/>
<rect x="152" y="36" width="110" height="38" rx="6" fill="none" stroke="currentColor"/>
<text x="207" y="52" font-size="11" text-anchor="middle" fill="currentColor">codec</text>
<text x="207" y="66" font-size="9" text-anchor="middle" fill="currentColor">sync, 16 MiB in</text>
<line x1="262" y1="55" x2="316" y2="55" stroke="currentColor" marker-end="url(#codec-arrow)"/>
<text x="289" y="48" font-size="9" text-anchor="middle" fill="currentColor">rgba, digest</text>
<rect x="318" y="14" width="116" height="34" rx="6" fill="none" stroke="currentColor"/>
<text x="376" y="35" font-size="11" text-anchor="middle" fill="currentColor">surface widget</text>
<rect x="318" y="62" width="116" height="34" rx="6" fill="none" stroke="currentColor"/>
<text x="376" y="83" font-size="11" text-anchor="middle" fill="currentColor">scene renderer</text>
</svg>
<figcaption>The plugin moves bytes in and out. The codec call is a pure function of its input.</figcaption>
</figure>

A plugin log shows what a decode job did. The digest is the first eight bytes of the SHA-256 in hexadecimal:

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs example.viewer --tail 3
<span class="t-dim">12:04:10</span> <span class="t-ok">info</span>  decode picture.png 640x480 frame 0
<span class="t-dim">12:04:10</span> <span class="t-ok">info</span>  rgba 1228800 bytes, sha256 9f2c61aa..
<span class="t-dim">12:04:11</span> <span class="t-err">error</span> too big.png: invalid-argument
</pre>

Illustrative, not generated. The numbers follow the 640 by 480 example above.

### Errors

`codec` raises only the codes below. Every function may also return `not-permitted`, `revoked` and `internal`, which apply to all host calls.

| Code | Raised by | When |
|---|---|---|
| `invalid-argument` | every function except `sha256` | Input over 16 MiB. |
| `invalid-argument` | `inflate` | `max-output` over 64 MiB, or corrupt data. |
| `quota-exceeded` | `inflate` | Output longer than `max-output`. |
| `invalid-argument` | `deflate` | `level` over 9. |
| `invalid-argument` | `image-info`, `decode-image` | Corrupt data, `frame` out of range, or `max-pixels` over 67,108,864. |
| `quota-exceeded` | `decode-image` | Width times height is more than `max-pixels`. |
| `invalid-argument` | `rasterize-text` | Text over 64 KiB, `size-px` of 0 or over 512, or `max-width-px` of 0 or over 16,384. |
| `unsupported` | `image-info`, `decode-image` | The format is not PNG, JPEG, GIF, WebP or BMP. |

Corrupt compressed or image data is always `invalid-argument`. The code `io` never comes from `codec`, because `io` is reserved for operating system I/O. `sha256` has no codec-specific error.

<div class="warn">
<p>Call <code>image_info</code> before <code>decode_image</code>. The header check costs almost nothing, and it refuses an oversized picture before the decoder allocates its pixel buffer.</p>
</div>

### Decode path

The plugin supplies bytes and receives pixels. Every step between those two points runs in host code.

<figure>
<svg viewBox="0 0 400 124" role="img" aria-label="Decode path from file bytes through checks to a surface">
<defs>
<marker id="codec-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="6" y="36" width="84" height="44" rx="6" fill="none" stroke="currentColor"/>
<text x="48" y="56" font-size="11" text-anchor="middle" fill="currentColor">File bytes</text>
<text x="48" y="72" font-size="10" text-anchor="middle" fill="currentColor">fs.read</text>
<rect x="116" y="36" width="84" height="44" rx="6" fill="none" stroke="currentColor"/>
<text x="158" y="56" font-size="11" text-anchor="middle" fill="currentColor">Image info</text>
<text x="158" y="72" font-size="10" text-anchor="middle" fill="currentColor">size check</text>
<rect x="226" y="36" width="84" height="44" rx="6" fill="none" stroke="currentColor"/>
<text x="268" y="56" font-size="11" text-anchor="middle" fill="currentColor">Decode</text>
<text x="268" y="72" font-size="10" text-anchor="middle" fill="currentColor">RGBA8</text>
<rect x="336" y="36" width="58" height="44" rx="6" fill="none" stroke="currentColor"/>
<text x="365" y="56" font-size="11" text-anchor="middle" fill="currentColor">Surface</text>
<text x="365" y="72" font-size="10" text-anchor="middle" fill="currentColor">pane</text>
<line x1="90" y1="58" x2="116" y2="58" stroke="currentColor" marker-end="url(#codec-arrow)"/>
<line x1="200" y1="58" x2="226" y2="58" stroke="currentColor" marker-end="url(#codec-arrow)"/>
<line x1="310" y1="58" x2="336" y2="58" stroke="currentColor" marker-end="url(#codec-arrow)"/>
<text x="6" y="108" font-size="11" fill="currentColor">Refusals happen before any pixel is decoded.</text>
</svg>
<figcaption>Decode path for Example 1. The size check runs on the header, and the decoder runs only when the size passes.</figcaption>
</figure>

### Edge cases

- **Animated formats.** `image-info` reports every frame. `decode-image` decodes one frame at a time, so a plugin that plays an animation calls it once per frame and never asks for all frames in one call.
- **Frame index.** Frames are counted from 0. A `frame` at or above `frames` returns `invalid-argument`.
- **Pixel budget.** `max-pixels` is an upper bound that the plugin chooses. The host never allows more than 67,108,864 pixels, whatever value the plugin passes.
- **Size of the output.** A decoded picture is `width × height × 4` bytes. Check the product before you write it to a surface, because `ui.write-surface` accepts at most 16 MiB per call.
- **Jobs.** A job instance calls the same functions with the same results. Jobs are the right place for work near the 16 MiB input limit.

### What the user sees

`codec` has no screen output of its own. The user sees a result only through what the plugin shows: the decoded picture in its surface, or the message the plugin writes when a call fails. The host does not display a codec error by itself.

### Used by

- Saved-world scenes use `inflate` to read compressed chunks. See [scenes](/pages/api-scenes/).
- Map and Wikipedia scenes use `decode-image` for the image tiles they fetch. See [http](/pages/api-http/).
- Media scenes use `decode-image` for picture files. See [media](/pages/api-media/).

### See also

- [fs](/pages/api-fs/) for reading the bytes.
- [ui](/pages/api-ui/) for `write-surface`.
- [background jobs](/pages/background-jobs/) for work near the input limit.
- [always-granted capabilities](/pages/capability-always-granted/) for the grant rules.
- [errors and limits](/pages/reference-errors-and-limits/) for the error codes.
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/) for project setup.
