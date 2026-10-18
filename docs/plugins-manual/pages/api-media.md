---
layout: page
title: media
permalink: /pages/api-media/
---

## media

The `media` import decodes a video file into frames. The host decodes the file natively, outside the plugin, and keeps a small buffer of decoded frames ahead of the plugin's requests. The plugin opens a file, asks for the next frame at the size it needs, and writes the returned pixels into a `surface` widget with `ui.write-surface`. The plugin never links a codec and never parses a container format.

Each frame is packed `rgb8`: three bytes per pixel, row by row, top row first. A frame of 320 by 180 pixels is 172 800 bytes (320 × 180 × 3).

**You will need:**

- **The `media.video` grant** on the client side or on the animation (scene) side. The server side cannot import `media`.
- **A readable path.** `open-video` takes an `fs-path`, and the plugin needs `fs.read` for that path's scope. A file the user chose in the picker arrives in a `picked/<n>` scope, so the plugin also needs `fs.pick` and `fs.read:picked` ([capability: fs.pick](/pages/capability-fs-pick/)).
- **A view for the picture.** Showing frames needs the `ui` import, which requires `ui.panels` or `panes.provide-kind`, and a `surface` widget in that view's tree.
- **The `ffmpeg` program on `PATH`.** The host runs `ffmpeg` as its decoder, as the native video scene does. Without it, `open-video` returns `unsupported`.

Import paths for the two SDKs:

| SDK | Import |
|---|---|
| Rust | `ilium_plugin_sdk::host::media` |
| TypeScript | `@ilium/plugin-sdk/host/media` |

### Interface

```wit
// Excerpt of ilium:host/media in reference-wit
record video-info { width: u32, height: u32, frames-per-second: f32, duration-millis: option<u64> }
open-video: func(path: fs-path) -> result<tuple<u64, video-info>, plugin-error>;
next-frame: func(video: u64, width: u32, height: u32) -> result<option<list<u8>>, plugin-error>;
close-video: func(video: u64);
```

### Functions

| Function | Rust | TypeScript | Grant | Sync | Meaning |
|---|---|---|---|---|---|
| `open-video` | `open_video(path: FsPath) -> Result<(u64, VideoInfo), PluginError>` | `openVideo(path: FsPath): [bigint, VideoInfo]` | `media.video`, `fs.read` for the path | sync | Opens the file and returns its video id and its description. |
| `next-frame` | `next_frame(video: u64, width: u32, height: u32) -> Result<Option<Vec<u8>>, PluginError>` | `nextFrame(video: bigint, width: number, height: number): Uint8Array \| undefined` | `media.video` | sync | Returns the next frame as `rgb8`, scaled to exactly `width` by `height`. `None` (`undefined`) at the end of the file. |
| `close-video` | `close_video(video: u64)` | `closeVideo(video: bigint): void` | `media.video` | sync | Releases the decoder and its buffer. The id is invalid afterwards. |

### Types

| Field | Rust | TypeScript | Meaning |
|---|---|---|---|
| `width`, `height` | `u32` | `number` | Size of the video in pixels. |
| `frames-per-second` | `frames_per_second: f32` | `framesPerSecond: number` | The frame rate that the container states. |
| `duration-millis` | `duration_millis: Option<u64>` | `durationMillis: bigint \| undefined` | Length in milliseconds, or none when the container does not state one. |
| video id | `u64` | `bigint` | Identifies one open video. Every `u64` is a `bigint` in TypeScript. |

### Rules

- **Frame rate.** The decoder does not pace the frames. The plugin decides how often it calls `next-frame`, usually from a timer ([timers](/pages/api-clock-and-timers/)).
- **Frame size.** Each dimension must be at least 1, and `width` by `height` may not exceed 3,840 by 2,160. The frame size must match the pixel size of the surface that receives it. The host derives that pixel size from the surface's `min-columns` and `min-rows` and the sub-cell factor of the shared dither mode, so the plugin sets those widget props to give the size it asks for.
- **Buffer.** The host decodes frames ahead, as many as fit in `video_buffer_mib` (64 MiB by default, 256 MiB at most for tier 1, 64 MiB for tiers 2 and 3) for each open video, and always at least one. A call that finds a buffered frame returns at once.
- **Open videos.** A plugin may hold 2 open videos at once, and the ceiling is 8 for tier 1 and 2 for tiers 2 and 3. A third `open-video` returns `quota-exceeded` until one is closed.
- **Closing.** Call `close-video` as soon as the plugin no longer needs the video. Each open video holds its decoder and its frame buffer until then.

<div class="warn">
<p>Call `next-frame` once for each timer event. A handler that runs longer than the 20 ms `on-events` deadline for a third-party plugin traps the instance. Decoding runs in the host ahead of the request, so one call per event normally stays well inside the deadline.</p>
</div>

### Example: play a picked video into a surface (Rust)

The plugin keeps one video open at a time. When the user picks a file, the plugin calls `start`. A timer fires every 100 ms, so the preview shows 10 frames per second. At the end of the file, the plugin stops the timer and closes the decoder.

**Grants:** `media.video`, `fs.pick`, `fs.read:picked`, `ui.panels`. The timer needs no grant.

The panel tree from the plugin's `views` export contains a `surface` widget with the key `frame`, format `rgb8`, and enough minimum size for a 320 by 180 pixel picture.

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::{media, timers, ui};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ui::{PixelFormat, ViewTarget};

const SURFACE_WIDTH: u32 = 320;
const SURFACE_HEIGHT: u32 = 180;
const FRAME_MILLIS: u32 = 100;

#[derive(Default)]
pub struct Preview {
    video: Option<u64>,
    timer: Option<TimerId>,
}

impl Preview {
    /// Called with the path that `take_picked` returns after the user chose a file.
    pub fn start(&mut self, picked: FsPath) -> Result<(), PluginError> {
        self.stop();
        let (video, info) = media::open_video(picked)?;
        log::log(Level::Info, &format!(
            "opened {}x{} at {:.1} frames per second",
            info.width, info.height, info.frames_per_second));
        self.video = Some(video);
        self.timer = Some(timers::set(FRAME_MILLIS, Some(FRAME_MILLIS))?);
        Ok(())
    }

    /// Writes one frame. `None` from the decoder means the end of the file.
    fn show_next_frame(&mut self) -> Result<(), PluginError> {
        let Some(video) = self.video else {
            return Ok(());
        };
        match media::next_frame(video, SURFACE_WIDTH, SURFACE_HEIGHT)? {
            Some(pixels) => ui::write_surface(
                ViewTarget::Panel("preview".into()),
                "frame",
                0,
                PixelFormat::Rgb8,
                &pixels,
            ),
            None => {
                log::log(Level::Info, "end of video, decoder closed");
                self.stop();
                Ok(())
            }
        }
    }

    fn stop(&mut self) {
        if let Some(timer) = self.timer.take() {
            timers::cancel(timer);
        }
        if let Some(video) = self.video.take() {
            media::close_video(video);
        }
    }
}

impl Handler for Preview {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::TimerFired(timer) = event else {
                continue;
            };
            if Some(timer) != self.timer {
                continue;
            }
            if let Err(error) = self.show_next_frame() {
                log::log(Level::Warn, &format!("preview stopped: {}", error.message));
                self.stop();
            }
        }
        Vec::new()
    }
}
```

The `#[ilium_plugin]` attribute and the registration of `Preview` follow [exports](/pages/api-exports/). The `take_picked` call and the `fs.pick` request follow [fs](/pages/api-fs/).

### Example: the same preview (TypeScript)

**Grants:** the same as the Rust example.

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';
import * as media from '@ilium/plugin-sdk/host/media';
import * as timers from '@ilium/plugin-sdk/host/timers';
import * as ui from '@ilium/plugin-sdk/host/ui';
import type { Event, FsPath } from '@ilium/plugin-sdk/types';

const SURFACE_WIDTH = 320;
const SURFACE_HEIGHT = 180;
const FRAME_MILLIS = 100;

let video: bigint | undefined;
let timer: bigint | undefined;

/** Called with the path that `takePicked` returns after the user chose a file. */
export function start(picked: FsPath): void {
  stop();
  const [id, info] = media.openVideo(picked);
  log('info', `opened ${info.width}x${info.height} at ${info.framesPerSecond.toFixed(1)} fps`);
  video = id;
  timer = timers.set(FRAME_MILLIS, FRAME_MILLIS);
}

function showNextFrame(): void {
  if (video === undefined) {
    return;
  }
  const pixels = media.nextFrame(video, SURFACE_WIDTH, SURFACE_HEIGHT);
  if (pixels === undefined) {
    log('info', 'end of video, decoder closed');
    stop();
    return;
  }
  ui.writeSurface({ tag: 'panel', val: 'preview' }, 'frame', 0, 'rgb8', pixels);
}

function stop(): void {
  if (timer !== undefined) {
    timers.cancel(timer);
    timer = undefined;
  }
  if (video !== undefined) {
    media.closeVideo(video);
    video = undefined;
  }
}

export default definePlugin({
  handler: {
    onEvents(batch: Event[]) {
      for (const event of batch) {
        if (event.tag !== 'timer-fired' || event.timerId !== timer) {
          continue;
        }
        try {
          showNextFrame();
        } catch (error) {
          log('warn', `preview stopped: ${String(error)}`);
          stop();
        }
      }
      return [];
    },
  },
});
```

### What the user sees

After the user picks a file, the preview panel fills with the picture and updates 10 times each second. The surface converts the pixels to cells with the shared colour settings and dithering, so the picture follows the look that the user has chosen in Settings. When the file ends, the picture stops and the plugin's log records the end.

<pre class="term">$ ilium plugin logs example.video-preview --tail 2
2026-10-11 10:12:01 <span class="t-ok">info</span> opened 1920x1080 at 25.0 fps
2026-10-11 10:12:09 <span class="t-ok">info</span> end of video, decoder closed
</pre>

Illustrative, not generated. The log of a short preview, ending when the decoder reports no more frames.

<figure>
<svg viewBox="0 0 400 200" role="img" aria-label="Frame path from the plugin through the native decoder to a surface widget">
<title>Frame path</title>
<desc>The plugin asks the host decoder for the next frame at a set size. The decoder returns packed rgb8 pixels. The plugin writes them into a surface widget with ui.write-surface.</desc>
<defs>
<marker id="arr-media" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="40" y="10" width="320" height="34" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
<text x="200" y="31" font-size="12" text-anchor="middle" fill="currentColor">Plugin: show_frame on timer event</text>
<line x1="120" y1="44" x2="120" y2="86" stroke="currentColor" marker-end="url(#arr-media)"/>
<text x="130" y="66" font-size="11" fill="currentColor">media.next-frame</text>
<line x1="340" y1="86" x2="340" y2="44" stroke="currentColor" marker-end="url(#arr-media)"/>
<text x="330" y="66" font-size="11" text-anchor="end" fill="currentColor">rgb8 frame</text>
<rect x="40" y="86" width="320" height="34" rx="6" fill="none" stroke="currentColor"/>
<text x="200" y="107" font-size="12" text-anchor="middle" fill="currentColor">Native decoder (ffmpeg in the host)</text>
<line x1="120" y1="120" x2="120" y2="150" stroke="currentColor" marker-end="url(#arr-media)"/>
<text x="130" y="138" font-size="11" fill="currentColor">ui.write-surface</text>
<rect x="40" y="150" width="320" height="34" rx="6" fill="none" stroke="currentColor"/>
<text x="200" y="171" font-size="12" text-anchor="middle" fill="currentColor">Surface widget in the preview panel</text>
</svg>
<figcaption>The plugin never sees the codec. It asks the host for a frame and writes the pixels it gets back.</figcaption>
</figure>

### Edge cases and errors

Every error is a `plugin-error` with one of these codes. Rust returns it as `Err(PluginError)`. TypeScript throws it.

| Code | When | Retry |
|---|---|---|
| `not-permitted` | No `media.video` grant, or no read grant for the path's scope. | No. Request the grant. |
| `not-found` | `open-video`: the path does not exist. `next-frame`: the video id is unknown or already closed. `close-video` has no error channel: closing an unknown or already closed id is a silent no-op. | No. |
| `invalid-argument` | `next-frame` with a zero width or height, or a size over 3,840 by 2,160. | No. Fix the size. |
| `unsupported` | No `ffmpeg` on `PATH` (the message names the program), or the file's extension, the `mime` type given to `open-video-bytes`, or the codec is not supported. Accepted mime types are `video/mp4`, `video/x-matroska`, `video/x-msvideo`, `video/quicktime`, `video/webm`, `video/mpeg`, `video/x-ms-wmv`, `video/x-flv` and `video/mp2t`. | No. |
| `quota-exceeded` | The plugin already has 2 open videos (8 at most for tier 1, 2 for tiers 2 and 3). | Yes, after a `close-video`. |
| `io` | The operating system or the decoder failed to read the file. | Maybe. |
| `revoked` | The authorization changed during the call. The plugin is activated again. | No. |

Accepted extensions are `mp4`, `mkv`, `avi`, `mov`, `webm`, `m4v`, `mpg`, `mpeg`, `wmv`, `flv` and `ts`. The codecs are whatever the host's `ffmpeg` decodes. A file with another extension returns `unsupported`, even when `ffmpeg` could read it.

<div class="note">
<p>An end-of-file is not an error. `next-frame` returns `none` (`undefined` in TypeScript) and keeps the video open until the plugin calls `close-video`.</p>
</div>

### Used by

No first-party feature uses `media` yet. The native video scene becomes the bundled scene plugin `ilium.video` in Phase 4d, and that plugin is the first first-party user.

### Video from bytes and ranges

With the `media.video` grant, three calls add sources and seeking.

```wit
// Excerpt of ilium:host/media in reference-wit
open-video-bytes: func(bytes: list<u8>, mime: string) -> result<tuple<u64, video-info>, plugin-error>;   // <= 16 MiB
open-video-range: func(path: fs-path, start-millis: u64, end-millis: option<u64>) -> result<tuple<u64, video-info>, plugin-error>;
seek: func(video: u64, millis: u64) -> result<_, plugin-error>;
```

| Call | Rust | TypeScript | Error |
|---|---|---|---|
| `open-video-bytes` | `media::open_video_bytes(&bytes, "video/mp4")` | `openVideoBytes(bytes, 'video/mp4')` | `unsupported` (mime type or codec), `invalid-argument` (empty bytes, or more than 16 MiB) |
| `open-video-range` | `media::open_video_range(path, start, end)` | `openVideoRange(path, start, end)` | `io`, `not-permitted` |
| `seek` | `media::seek(video, millis)` | `seek(video, millis)` | `invalid-argument` |

The `video` widget plays the same sources inside views: `source` is `asset(string)`, `fs(fs-path)` or `range(fs-path, u64, option<u64>)`, with `autoplay`, `loop`, `muted`, `controls` and `fit`. It is always muted unless `audio.playback` is granted, and the shared look and dithering apply. The `image` widget gains `asset(string)` and `bytes(list<u8>, string)` sources (PNG, JPEG, GIF or WebP up to 16 MiB). Settings views do not allow `video`.

**Rust**

```rust
let (video, info) = media::open_video_range(path, 5_000, Some(15_000))?;
media::seek(video, 0)?;
```

**TypeScript**

```ts
const [video, info] = openVideoRange(path, 5000n, 15000n);
seek(video, 0n);
```

See [glyph plane](/pages/api-surface-glyph-plane/) for fonts, package assets and pre-rendered scene loops, and [capability media video](/pages/capability-media-video/).

### See also

- [ui](/pages/api-ui/) for `write-surface` and the view that holds the surface.
- [widgets: media and embedded](/pages/widgets-media-and-embedded/) for the `surface` widget and how its pixels become cells.
- [capability: media.video](/pages/capability-media-video/) for the grant and its risk.
- [codec](/pages/api-codec/) for decoding still images.
- [fs](/pages/api-fs/) and [capability: fs.pick](/pages/capability-fs-pick/) for files the user chooses.
- [timers](/pages/api-clock-and-timers/) for the frame rate.
- [errors and limits](/pages/reference-errors-and-limits/) for the general error model.
