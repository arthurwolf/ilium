---
layout: page
title: "Capability: media.video"
permalink: /pages/capability-media-video/
---

## media.video

The `media.video` grant lets a client-side plugin or a scene open a video file and read its frames as pixels. The host decodes the file natively, using the `ffmpeg` program that it finds on `PATH`. The plugin therefore ships no decoder and never parses a container format. It asks for one frame at a time, at the size it needs, and draws the result, usually into a `surface` widget with `ui.write-surface`.

The grant has normal risk. It is not a high-risk capability. Bundled T1 plugins receive it automatically up to their ceiling. T2 and T3 packages list it checked by default in the review dialog.

### What you need

- **Grant.** `media.video` in `capabilities.client` or in `capabilities.scene`. The server side cannot import `media`. The grant takes no argument:

| Grant | Valid | Reason |
|---|---|---|
| `media.video` | yes | the only form |
| `media.video:h264` | no | the grant takes no argument; the host chooses the decoder |
| `media.decode` | no | the grant name is `media.video` |

- **A readable path.** `open-video` takes an `fs-path` and needs `fs.read` for that path's scope. A file the user chose arrives in a `picked/<n>` scope. A client plugin therefore also needs `fs.pick` and `fs.read:picked` ([fs.pick](/pages/capability-fs-pick/)). A scene does not declare `fs.pick`. It declares `fs.read:picked` and takes its path from a settings field that uses the `picker` keyword ([settings screens](/pages/settings-screens/)).
- **A view.** Drawing frames needs the `ui` import, which requires `ui.panels` or `panes.provide-kind`, and a `surface` widget in the view's tree.
- **The `ffmpeg` program on `PATH`.** Without it, `open-video` returns `unsupported` and names the missing program in the message.
- **Language.** Rust with the `ilium_plugin_sdk` crate, or TypeScript with `@ilium/plugin-sdk`. The import paths are `ilium_plugin_sdk::host::media` and `@ilium/plugin-sdk/host/media`.

### Full surface

#### Functions

| Function | Rust | TypeScript | Meaning |
|---|---|---|---|
| `open-video` | `media::open_video(path: FsPath) -> Result<(u64, VideoInfo), PluginError>` | `openVideo(path: FsPath): [bigint, VideoInfo]` | Opens the file and returns its video id and description. Synchronous. Throws in TypeScript. |
| `next-frame` | `media::next_frame(video: u64, width: u32, height: u32) -> Result<Option<Vec<u8>>, PluginError>` | `nextFrame(video: bigint, width: number, height: number): Uint8Array \| undefined` | Returns the next frame as `rgb8`, scaled to exactly `width` by `height`. `None` (`undefined`) at the end of the video. Synchronous. |
| `close-video` | `media::close_video(video: u64)` | `closeVideo(video: bigint): void` | Releases the decoder and its frame buffer. The id is invalid afterwards. |

#### Types

```wit
// Excerpt of ilium:host/media in reference-wit
record video-info {
    width: u32,
    height: u32,
    frames-per-second: f32,
    duration-millis: option<u64>,
}
open-video: func(path: fs-path) -> result<tuple<u64, video-info>, plugin-error>;
next-frame: func(video: u64, width: u32, height: u32) -> result<option<list<u8>>, plugin-error>;
close-video: func(video: u64);
```

| Field | Rust | TypeScript | Meaning |
|---|---|---|---|
| `width`, `height` | `u32` | `number` | Size of the video in pixels. |
| `frames-per-second` | `frames_per_second: f32` | `framesPerSecond: number` | Frame rate that the container states. |
| `duration-millis` | `duration_millis: Option<u64>` | `durationMillis: bigint \| undefined` | Length in milliseconds, or none when the container does not state one. |
| video id | `u64` | `bigint` | Identifies one open video. Every `u64` is a `bigint` in TypeScript. |

The host does not pace frames. The plugin decides how often it calls `next-frame`, usually from a timer ([timers](/pages/api-clock-and-timers/)).

### Frame format and limits

- **Layout.** Each frame is packed `rgb8`: three bytes per pixel, row by row, top row first. The byte count is `width * height * 3`. A 320 by 180 frame is 172 800 bytes, and a 640 by 360 frame is 691 200 bytes.
- **Size.** Each dimension must be at least 1. A frame larger than 3,840 by 2,160 pixels is refused with `invalid-argument`. The frame size must match the pixel size of the receiving `surface`. The host derives that size from the widget's `min-columns` and `min-rows` and from the sub-cell factor of the dither mode.
- **Buffer.** The host decodes frames ahead, as many as fit in the buffer of each open video, and always at least one. A call that finds a buffered frame returns at once.
- **Open videos.** A plugin may hold open videos up to a count (below). A further `open-video` returns `quota-exceeded` until one is closed.

| Limit (manifest key) | Default | Ceiling T1 | Ceiling T2 and T3 | Effect when reached |
|---|---|---|---|---|
| Open videos (`open_videos`) | 2 | 8 | 2 | `quota-exceeded` from `open-video` |
| Frame buffer per video (`video_buffer_mib`) | 64 MiB | 256 MiB | 64 MiB | Decode-ahead shortens, to a minimum of one frame |

A manifest may request other values under `limits`. The host clamps each request to the ceiling of the tier and logs the clamp.

### Frame loop

<figure>
<svg viewBox="0 0 420 220" role="img" aria-label="Video calls: open, then next-frame on each timer event, then close">
  <defs>
    <marker id="mv-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="8" width="400" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="30" text-anchor="middle" font-size="11" fill="currentColor">open-video(path) returns the video id and its info</text>
  <line x1="210" y1="46" x2="210" y2="60" stroke="currentColor" marker-end="url(#mv-arrow)"/>
  <rect x="10" y="62" width="400" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="84" text-anchor="middle" font-size="11" fill="currentColor">next-frame(video, width, height) on each timer event</text>
  <line x1="210" y1="100" x2="210" y2="114" stroke="currentColor" marker-end="url(#mv-arrow)"/>
  <rect x="10" y="116" width="400" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="138" text-anchor="middle" font-size="11" fill="currentColor">none at the end of the video; a decoder failure gives io</text>
  <line x1="210" y1="154" x2="210" y2="168" stroke="currentColor" marker-end="url(#mv-arrow)"/>
  <rect x="10" y="170" width="400" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="192" text-anchor="middle" font-size="11" fill="currentColor">close-video(video) releases the decoder and its buffer</text>
</svg>
<figcaption>The frame loop runs once for each timer event. The decoder keeps running until the plugin calls close-video, or until the video ends or its owner stops.</figcaption>
</figure>

### Manifest excerpt

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.preview",
  "capabilities": {
    "client": ["media.video", "fs.pick", "fs.read:picked", "ui.panels"]
  }
}
```

### Worked example: Rust

The plugin plays one picked video at a time. A 100 ms timer shows 10 frames per second in a surface with the key `frame`, inside the panel `preview`. At the end of the file, the plugin stops the timer and closes the decoder. The `start` method receives the path that `fs::pick` returns. Run `choose_video` in a task that the plugin spawns with `task::spawn`, then call `start` with its result.

**Rust**

```rust
use ilium_plugin_sdk::host::fs::{self, PickKind, PickSpec};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::{media, timers, ui};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ids::TimerId;
use ilium_plugin_sdk::types::ui::{PixelFormat, ViewTarget};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;
const FRAME_MILLIS: u32 = 100; // 10 frames per second

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct Preview {
    video: Option<u64>,
    timer: Option<TimerId>,
}

/// Asks the user for a video file. Resolves to a picked/<n> path with an empty path field.
async fn choose_video() -> Result<FsPath, PluginError> {
    fs::pick(PickSpec {
        kind: PickKind::OpenFile,
        title: "Choose a video".to_string(),
        extensions: vec!["mp4".to_string(), "mkv".to_string(), "webm".to_string()],
    })
    .await
}

impl Preview {
    /// Starts playback of a path that the picker returned.
    fn start(&mut self, picked: FsPath) -> Result<(), PluginError> {
        self.stop();
        let (video, info) = media::open_video(picked)?;
        log::log(Level::Info, &format!("opened {}x{}", info.width, info.height));
        self.video = Some(video);
        self.timer = Some(timers::set(FRAME_MILLIS, Some(FRAME_MILLIS))?);
        Ok(())
    }

    /// Draws one frame. `None` from the decoder means the end of the video.
    fn show_next_frame(&mut self) -> Result<(), PluginError> {
        let Some(video) = self.video else {
            return Ok(());
        };
        match media::next_frame(video, WIDTH, HEIGHT)? {
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

### Worked example: TypeScript

The TypeScript version behaves the same way. `definePlugin` returns the `handler` export that the manifest lists. The picker call returns a promise.

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, FsPath, Submission } from '@ilium/plugin-sdk/types';
import * as fs from '@ilium/plugin-sdk/host/fs';
import { log } from '@ilium/plugin-sdk/host/log';
import * as media from '@ilium/plugin-sdk/host/media';
import * as timers from '@ilium/plugin-sdk/host/timers';
import * as ui from '@ilium/plugin-sdk/host/ui';

const WIDTH = 320;
const HEIGHT = 180;
const FRAME_MILLIS = 100; // 10 frames per second

let video: bigint | undefined;
let timer: bigint | undefined;

/** Asks the user for a video file and resolves to the picked scope path. */
export function chooseVideo(): Promise<FsPath> {
  return fs.pick({
    kind: 'open-file',
    title: 'Choose a video',
    extensions: ['mp4', 'mkv', 'webm'],
  });
}

/** Starts playback of a path that the picker returned. Throws PluginError on failure. */
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
  const pixels = media.nextFrame(video, WIDTH, HEIGHT);
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

export const { handler } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
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
});
```

### What the user sees

When the user installs the plugin, the review dialog shows the video grant as one row. The row text is "Decode video files", and the grant string is on the right.

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.media 0.1.0 by Example Ltd                                     │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ [x] Decode video files <span class="t-dim">………………………………………………………………………………………………………</span> media.video │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests media.video, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

The plugin writes its own log lines. They appear in `ilium plugin logs`:

<figure>
<pre class="term">
<span class="t-prompt">$</span> ilium plugin logs com.example.preview
2026-10-11 12:04:11 <span class="t-hi">info</span>  opened 640x360 at 25.0 fps
2026-10-11 12:04:44 <span class="t-hi">info</span>  end of video
2026-10-11 12:06:02 <span class="t-warn">warn</span>  stopped, decoder exit 1
</pre>
<figcaption>Illustrative, not generated. Log lines of a preview plugin for one video.</figcaption>
</figure>

### Edge cases and errors

| Situation | Result | What the plugin should do |
|---|---|---|
| Grant declined or revoked | `open-video` returns `not-permitted` | Draw the fallback image. |
| Path outside the granted scopes | `not-permitted` | Ask the user again through the picker. |
| No `ffmpeg` on `PATH` | `unsupported`, message names `ffmpeg` | Show the message in the view. |
| Extension or codec that `ffmpeg` cannot decode | `unsupported` | Choose another file. |
| File does not exist | `not-found` from `open-video` | Re-pick the file. |
| Third open video (default limit 2) | `quota-exceeded` | Close a video first. |
| Zero width or height, or over 3,840 by 2,160 | `invalid-argument` from `next-frame` | Fix the size in the call. |
| Video id that is closed or was never opened | `not-found` from `next-frame` | Stop calling it. |
| Decoder fails in the middle of a file | `io` from `next-frame`; the video stays open | Close the video and report the message. |
| Plugin restarts, is disabled or loses the grant | Open video ids end; later calls return `not-found` | Open the video again after activation. |

Keep each `on-events` call short. One `next-frame` call per timer event normally fits the handler deadline, because the host decodes ahead.

### Photosensitivity

Flashing or strobing pictures can trigger seizures in people with photosensitive epilepsy. The host does not measure what a plugin draws, so the flash limit is the plugin's duty. The warning and the off switch are the host's duty to the user.

- **Flash limit.** A plugin that draws video MUST keep its output within WCAG 2.3.1 ("Three Flashes or Below Threshold"). In practice that means no more than three flashes in any one-second period. Flashing areas that add up to more than a quarter of any 1,024 by 768 pixel area are over the limit. Saturated red flashes follow the stricter red-flash rule of the same criterion. The Harding Flash and Pattern Analyzer is the reference check for a new scene or video treatment.
- **Source video.** A user-picked file can strobe even when the plugin draws nothing extra. The plugin MUST either hold, average or blur frames so that the output stays within the limit, or stop drawing and show the fallback image. The `frames-per-second` value in `VideoInfo` is the source rate, not the output rate, so a plugin must not treat a low frame rate as a safe one.
- **Warning.** The review dialog MUST show one line for a package that requests `media.video` or `gpu.kernels`: "Can show flashing images. Photosensitive viewers may be affected." The line is a warning. It does not change the grant or its risk tier.
- **Off switch.** The user turns the plugin off at once in Settings > Plugins, or with `ilium plugin disable <id>` ([install and manage](/pages/user-install-and-manage/)). Revoking the grant in Settings > Plugins > <plugin> > Capabilities, or with `ilium plugin grants <id> --revoke media.video`, stops the video at once. Open video ids then end, and later calls return `not-found` ([edge cases](#edge-cases-and-errors)). An administrator can switch off every third-party plugin with `ilium plugin disable --all-third-party --reason <text>` ([enterprise deployment](/pages/guide-enterprise-deployment/)).
- **GPU kernels.** The same limit, warning and switch apply to a scene that calls `gpu.kernels`, because a kernel can produce the same flashing output. See [gpu.kernels](/pages/capability-gpu-kernels/).

### Who requests it

The native video scene becomes the bundled scene plugin `ilium.video` in Phase 4d. It is the first first-party user of this capability ([api-media](/pages/api-media/)).

### See also

- [api-media](/pages/api-media/) for the binding reference and the complete rules.
- [fs.pick](/pages/capability-fs-pick/) for the picked file scope.
- [fs.read](/pages/capability-fs-read/) for the read grant that `open-video` needs.
- [audio](/pages/capability-audio/) for the audio half of a media scene.
- [Media and embedded widgets](/pages/widgets-media-and-embedded/) for the `surface` widget.
- [Errors and limits](/pages/reference-errors-and-limits/) for the error codes and the limit model.
