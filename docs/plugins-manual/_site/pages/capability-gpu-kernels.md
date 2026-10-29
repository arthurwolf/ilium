# Capability: gpu.kernels

## gpu.kernels

The `gpu.kernels` capability lets an animation scene run the host's own image kernels on pixel data: blur, threshold, palette map, ordered dither and scale. A plugin never ships shader code (decision D18). Without a GPU every kernel runs on the CPU with the same result. The capability is unstable: it is marked `@unstable(feature = gpu-kernels)` and appears only in development builds.

**You will need:**

- A development build of Ilium with the `gpu-kernels` feature.
- A scene-side component (world `ilium:anim/base`).
- A `gpu.kernels` grant on the `scene` side.

<figure>
<svg viewBox="0 0 420 252" role="img" aria-label="A scene sends pixels to a host kernel and receives processed pixels">
  <defs><marker id="arr" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="27" text-anchor="middle" fill="currentColor" font-size="12">Scene calls gpu.available()</text>
  <text x="210" y="43" text-anchor="middle" fill="currentColor" font-size="10">tells whether a GPU is used</text>
  <line x1="210" y1="52" x2="210" y2="72" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="72" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="91" text-anchor="middle" fill="currentColor" font-size="12">Scene calls gpu.run(kernel, pixels, width, height, params)</text>
  <text x="210" y="107" text-anchor="middle" fill="currentColor" font-size="10">host kernels only</text>
  <line x1="210" y1="116" x2="210" y2="136" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="136" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="155" text-anchor="middle" fill="currentColor" font-size="12">Host returns the processed pixels</text>
  <text x="210" y="171" text-anchor="middle" fill="currentColor" font-size="10">CPU fallback gives the same result</text>
  <line x1="210" y1="180" x2="210" y2="200" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="110" y="224" text-anchor="middle" fill="currentColor" font-size="11">pixel bytes</text>
  <rect x="220" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="310" y="224" text-anchor="middle" fill="currentColor" font-size="11">unsupported in release builds</text>
</svg>
<figcaption>Only kernels from the host list run. The scene passes parameters as JSON.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `gpu.kernels` | yes | the only form, development builds only |
| `gpu.kernels:blur` | no | no argument |
| `device.gpu` | no | a name that never existed; see [porting iliumanim](/pages/porting-iliumanim/) |

Normal risk, unstable. In release builds the grant is unknown and the manifest is refused with `manifest.grant-unknown`.

### What it unlocks

| Name | Kind | Meaning |
|---|---|---|
| `gpu.run` | function | `run(kernel, input, width, height, params-json) -> result<list<u8>, plugin-error>`. |
| `gpu.available` | function | `available() -> bool`. |
| `gpu-kernel` | enum | `blur`, `threshold`, `palette-map`, `dither-ordered`, `scale`. |

See [Scenes](/pages/api-scenes/). Colour and dithering stay global in the shared look: a scene does not add its own palette controls.

### Manifest

The grant goes into `manifest.json` under `capabilities`, in the list of the side that uses it (scene only).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.glowscene",
  "capabilities": {
    "scene": [
      "gpu.kernels"
    ]
  }
}
```

### Worked example

This scene blurs a frame buffer with a radius of 2 cells.

**Rust**

```rust
use ilium_plugin_sdk::host::gpu::{self, GpuKernel};

fn blur_frame(pixels: &[u8], width: u32, height: u32) -> Vec<u8> {
    if !gpu::available() { log::log(Level::Debug, "gpu: cpu fallback"); }
    gpu::run(GpuKernel::Blur, pixels, width, height, r#"{"radius":2}"#)
        .unwrap_or_else(|_| pixels.to_vec())
}
```

**TypeScript**

```ts
import { run, available } from '@ilium/plugin-sdk/host/gpu';
import { log } from '@ilium/plugin-sdk/host/log';

export function blurFrame(pixels: Uint8Array, width: number, height: number): Uint8Array {
  if (!available()) log('debug', 'gpu: cpu fallback');
  try {
    return run('blur', pixels, width, height, '{"radius":2}');
  } catch {
    return pixels;
  }
}
```

Call the helper from the scene's frame function. Keep the original buffer: a failed kernel must not blank the animation.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Release build | grant unknown, manifest refused | Gate the feature behind a development build. |
| Grant declined | `not-permitted` | Draw without the effect. |
| Unknown kernel name | `invalid-argument` | Use the enum. |
| Pixel buffer size mismatch | `invalid-argument` | Match width, height and format (4 bytes per pixel, RGBA, 8 bits per channel, so the buffer is exactly `width * height * 4` bytes). |
| Buffer too large | `quota-exceeded` | Process tiles. |
| No GPU | CPU fallback with the same result | Nothing. |

Because the feature is unstable, kernels and parameters may change between development builds.

### What the user sees

The review dialog lists the grant when a package is installed or first activated. The grant appears only in development builds.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.glowscene 0.1.0 by Example Ltd                                 │
│                                                                            │
│ <span class="t-hi">Scene side</span>                                                                 │
│ [x] Runs image effects on the GPU <span class="t-dim">…………………………………………………………………………</span> gpu.kernels │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests gpu.kernels, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar.</figcaption>
</figure>

Listing the grants of an installed plugin from the terminal:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.glowscene
<span class="t-dim">Grant                             Risk     State</span>
gpu.kernels                       normal   <span class="t-ok">granted</span>
</pre>

<figcaption>Output of the grant listing for this plugin. A high-risk grant that the user left unchecked shows as declined.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.glowscene > Capabilities. A revocation re-activates the plugin with the reduced grant set, and the stubs of the declined imports return `not-permitted`.

### Related pages

- [Scenes](/pages/api-scenes/).
- [Media](/pages/api-media/).
- [media.video](/pages/capability-media-video/).
- [Capabilities and grants](/pages/capabilities/).
