---
layout: page
title: gpu
permalink: /pages/api-gpu/
---

## gpu

<div class="warn">

This interface is `@unstable(feature = gpu-kernels)`. It exists only in development builds, the grant `gpu.kernels` appears only there, and its shape can change before it is stable (D18).

</div>

The `gpu` import lets a scene run a small fixed set of image kernels on the graphics card: blur, threshold, palette mapping, ordered dithering and scaling. The kernels belong to the host. A plugin never ships shader code, and it never sees a graphics device; it hands over pixels and gets pixels back. Without a graphics card, every kernel runs on a CPU fallback and returns the same result, only slower. It exists so animation scenes such as clouds, dithered waves and pattern fields can offload their heaviest per-pixel step.

### What you need

- The grant `gpu.kernels`, in the `capabilities.scene` list of a scene plugin. The interface is imported by the scene world (`ilium:anim/base`) only.
- A host build with the feature `gpu-kernels` enabled. A package that asks for the grant installs on such a build only.
- Nothing else: the call needs no filesystem or network access.

```json
"capabilities": { "scene": ["gpu.kernels"] }
```

### Signatures

```wit
// Excerpt of ilium:host/gpu in reference-wit
enum gpu-kernel { blur, threshold, palette-map, dither-ordered, scale }
run: func(kernel: gpu-kernel, input: list<u8>, width: u32, height: u32, params-json: string) -> result<list<u8>, plugin-error>;
available: func() -> bool;
```

| Rust (`ilium_plugin_sdk::host::gpu`) | TypeScript (`@ilium/plugin-sdk/host/gpu`) | Meaning |
|---|---|---|
| `gpu::available()` | `available()` | `true` when a graphics device runs the kernels. `false` means the CPU fallback is used; calls still work. |
| `gpu::run(kernel, &input, width, height, &params_json)?` | `run(kernel, input, width, height, paramsJson)` | Runs one kernel and returns the output bytes. Synchronous. |
| `GpuKernel::{Blur, Threshold, PaletteMap, DitherOrdered, Scale}` | `'blur' \| 'threshold' \| 'palette-map' \| 'dither-ordered' \| 'scale'` | The kernels the host offers. |

The call is synchronous and runs inside the frame budget of the scene's `renderer` export, so it should be used for a frame step, not for batch work. For heavy offline work, use a [job](/pages/api-jobs/).

### Kernels

| Kernel | What it does | Typical use |
|---|---|---|
| `blur` | Softens the image. | Cloud edges, glow. |
| `threshold` | Turns tone into two levels. | Masks, coastlines from a height field. |
| `palette-map` | Maps tone onto a palette. | Colouring a field by the shared palette. |
| `dither-ordered` | Ordered (matrix) dithering. | Preparing a tone raster for cells. |
| `scale` | Resamples to another size. | Fitting a pre-rendered field to the surface. |

| Kernel | `params-json` keys (defaults) |
|---|---|
| `blur` | `radius` (integer cells of pixels, 1 to 64, default 2) |
| `threshold` | `level` (integer 0 to 255, default 128) |
| `palette-map` | `palette` (array of 2 to 256 `[r,g,b]` triples, required) |
| `dither-ordered` | `matrix` (`"bayer2"`, `"bayer4"` or `"bayer8"`, default `"bayer4"`), `levels` (integer 2 to 256, default 2) |
| `scale` | `out-width` and `out-height` (integers 1 to 8192, required) |

The kernel parameters travel in `params-json`, a JSON object whose keys depend on the kernel, listed below. Unknown keys are ignored; a missing key takes the default shown. A kernel the host does not offer returns `unsupported`. Bad dimensions or malformed parameters return `invalid-argument`.

The shared look, dithering and frame pacing of the surface stay in the kernel and apply to whatever the scene draws, so a scene should use `dither-ordered` only for its own intermediate step, not to replace the global dither setting.

<figure>
<svg viewBox="0 0 440 150" role="img" aria-label="Scene calls a host kernel which runs on GPU or CPU fallback">
<title>gpu.run path</title>
<desc>The scene calls run with pixels and parameters. The host picks the GPU when available, else the CPU fallback, and returns pixels. The scene writes them to its surface.</desc>
<defs>
<marker id="gpu-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker>
</defs>
<rect x="8" y="50" width="100" height="40" rx="6" fill="none" stroke="currentColor"/>
<text x="58" y="74" font-size="12" text-anchor="middle" fill="currentColor">Scene</text>
<line x1="108" y1="70" x2="152" y2="70" stroke="currentColor" marker-end="url(#gpu-arrow)"/>
<text x="130" y="62" font-size="10" text-anchor="middle" fill="currentColor">run</text>
<rect x="154" y="50" width="100" height="40" rx="6" fill="none" stroke="currentColor"/>
<text x="204" y="74" font-size="12" text-anchor="middle" fill="currentColor">Host kernel</text>
<line x1="254" y1="60" x2="300" y2="30" stroke="currentColor" marker-end="url(#gpu-arrow)"/>
<line x1="254" y1="80" x2="300" y2="110" stroke="currentColor" marker-end="url(#gpu-arrow)"/>
<rect x="302" y="12" width="128" height="34" rx="6" fill="none" stroke="currentColor"/>
<text x="366" y="33" font-size="11" text-anchor="middle" fill="currentColor">GPU, when available</text>
<rect x="302" y="94" width="128" height="34" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
<text x="366" y="115" font-size="11" text-anchor="middle" fill="currentColor">CPU fallback, same result</text>
<text x="8" y="144" font-size="10" fill="currentColor">The plugin sends pixels and parameters. It never sends shader code.</text>
</svg>
<figcaption>Only the host's own kernels run. The result is the same on the GPU and on the CPU fallback.</figcaption>
</figure>

### Example: blur a cloud field when a GPU is present

The scene builds a tone field on the CPU, blurs it with the host kernel and draws the result. When `available` is false it still calls `run`, because the CPU fallback gives the same pixels; it only lowers the field size to keep the frame time. `field_to_surface` and `build_params` are the scene's own code.

**Grants:** `gpu.kernels`

**Rust**

```rust
use ilium_plugin_sdk::host::gpu::{self, GpuKernel};
use ilium_plugin_sdk::prelude::*;

struct CloudScene {
    width: u32,
    height: u32,
    field: Vec<u8>,
    params_json: String,
}

impl CloudScene {
    fn soften(&mut self) -> Result<Vec<u8>, PluginError> {
        let (width, height) = if gpu::available() {
            (self.width, self.height)
        } else {
            (self.width / 2, self.height / 2)
        };
        let input = self.field_at(width, height);
        match gpu::run(GpuKernel::Blur, &input, width, height, &self.params_json) {
            Ok(output) => Ok(output),
            Err(error) if error.code == ErrorCode::Unsupported => Ok(input),
            Err(error) => Err(error),
        }
    }
}
```

**TypeScript**

```ts
import { PluginError } from '@ilium/plugin-sdk';
import { available, run } from '@ilium/plugin-sdk/host/gpu';

export function soften(
  field: (width: number, height: number) => Uint8Array,
  width: number,
  height: number,
  paramsJson: string,
): Uint8Array {
  const [w, h] = available() ? [width, height] : [width >> 1, height >> 1];
  const input = field(w, h);
  try {
    return run('blur', input, w, h, paramsJson);
  } catch (error) {
    if (error instanceof PluginError && error.code === 'unsupported') {
      return input;
    }
    throw error;
  }
}
```

### What the user sees

Nothing separate. The scene looks the same with or without a graphics card; only the frame rate differs. The user can tell a development build apart in the plugin list, where the review dialog shows the grant in plain words.

<pre class="term"><span class="t-prompt">$</span> ilium plugin list
<span class="t-ok">ok</span>   example.clouds 0.1.0   scene  gpu.kernels
<span class="t-prompt">$</span> ilium plugin logs example.clouds --tail 2
<span class="t-dim">11:20:04</span> <span class="t-ok">info</span>  gpu.available false, cpu fallback, field 160x45
<span class="t-dim">11:20:04</span> <span class="t-ok">info</span>  gpu.run blur 160x45 in 3 ms
</pre>

Illustrative, not generated. A scene on a host without a graphics device: the call works and the plugin lowers its field size.

### Errors and limits

| Code | When |
|---|---|
| `not-permitted` | The grant `gpu.kernels` is missing, declined or revoked. |
| `unsupported` | The kernel is not one the host offers. |
| `invalid-argument` | `width` or `height` is zero, `input` does not match the size, or `params-json` is not valid for the kernel. |

`available` never fails. If the graphics device faults during `run`, the host reruns the kernel on the CPU fallback and returns the same result; if that also fails, `run` returns `internal` and the scene keeps its previous frame. There is no new requestable limit; the call counts against the scene's frame budget and the usual CPU budget of the plugin.

### See also

- [Scenes](/pages/api-scenes/) for the `renderer` export and the surface the result is drawn on.
- [media](/pages/api-media/) for decoded video frames, another source of pixels.
- [codec](/pages/api-codec/) for decoding images before a kernel runs on them.
- [jobs](/pages/api-jobs/) for work that is too heavy for a frame.
- [Errors and limits](/pages/reference-errors-and-limits/) for the shared codes.
