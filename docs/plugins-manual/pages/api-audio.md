---
layout: page
title: audio
permalink: /pages/api-audio/
---

## audio

The `audio` import lists the sound devices, opens a capture stream from a microphone or from a loopback of the desktop output, and plays samples. Captured audio arrives as `audio-frames` events, one event for each 20 ms chunk. A plugin that captures audio hears everything the device records, so capture is a high-risk capability (marked H) and the user must allow it explicitly.

Use this import in client plugins and scenes. The server world has no audio import.

### What you need

- **Grants.** `audio.capture` opens a microphone (H). `audio.loopback` captures what the computer plays (H). `audio.playback` plays samples. Listing devices has no error code, because `devices` returns a plain list. It lists devices only while you hold at least one audio grant; with none granted it returns an empty list.
- **Worlds.** The import is in the client world (`ilium:client/base`) and the animation world (`ilium:anim/base`).
- **Manifest.** Declare each grant under the side that uses it in `manifest.json`:

```json
{
  "capabilities": { "client": ["audio.capture", "ui.panels"] }
}
```

- **Consent.** The user answers one prompt per plugin at first activation. Each H capability is listed unchecked, and the user must tick it. A declined grant makes every call that needs it return `not-permitted`.

### Surface

| Rust (`ilium_plugin_sdk::host::audio`) | TypeScript (`@ilium/plugin-sdk/host/audio`) | Grant | Call | Returns | Meaning |
|---|---|---|---|---|---|
| `audio::devices()` | `audio.devices()` | any audio grant | sync | `Vec<AudioDevice>` | Lists the devices the grants allow. |
| `audio::open_capture(device, sample_rate, channels)` | `audio.openCapture(device, sampleRate, channels)` | `audio.capture` | sync | `Result<u64, PluginError>` | Opens a microphone and returns a capture id. `None` or `undefined` selects the default input. |
| `audio::open_loopback(sample_rate, channels)` | `audio.openLoopback(sampleRate, channels)` | `audio.loopback` | sync | `Result<u64, PluginError>` | Opens a capture of the desktop output. |
| `audio::close_capture(capture)` | `audio.closeCapture(capture)` | the grant that opened it | sync | nothing | Stops the capture and releases the device. |
| `audio::play(samples, sample_rate, channels).await` | `await audio.play(samples, sampleRate, channels)` | `audio.playback` | async | `Result<(), PluginError>` | Plays samples. The WIT function returns `result<request-id, plugin-error>`: a request id at once, and the host delivers `request-completed` with that id when playback ends. The SDK `await` resolves at that completion with this result type. |

Types and events:

| Name | Rust | TypeScript | Fields |
|---|---|---|---|
| Device record | `AudioDevice` | `AudioDevice` | `id: String`, `name: String`, `is_input: bool`, `is_default: bool` (TypeScript: `isInput`, `isDefault`) |
| Capture event | `Event::AudioFrames(AudioFramesEvent)` | `{ tag: 'audio-frames', val }` | `capture: u64` (TypeScript `bigint`), `samples: Vec<i16>` (TypeScript `Int16Array`) |

The WIT form, for reference:

```wit
record audio-device { id: string, name: string, is-input: bool, is-default: bool }
record audio-frames-event { capture: u64, samples: list<s16> }
devices: func() -> list<audio-device>;
open-capture: func(device: option<string>, sample-rate: u32, channels: u8)
  -> result<u64, plugin-error>;
open-loopback: func(sample-rate: u32, channels: u8) -> result<u64, plugin-error>;
close-capture: func(capture: u64);
play: func(samples: list<s16>, sample-rate: u32, channels: u8)
  -> result<request-id, plugin-error>;
```

### Sample format

Samples are signed 16-bit integers (`s16`) from -32,768 to 32,767, with 0 as silence. They are interleaved: each frame holds one sample per channel, left before right. The same layout applies to `audio-frames` and to `play`.

Each `audio-frames` event holds 20 ms of audio. The number of samples is `sample_rate / 50 * channels`. At 16 kHz mono, one event carries 320 samples. At 48 kHz stereo, one event carries 1,920 samples.

The host resamples natively between your requested rate and the device rate, so you never need a fallback rate. Any rate from 8,000 to 192,000 Hz is accepted.

### Delivery and backpressure

`audio-frames` uses the policy *drop oldest, counted*. A slow handler loses old frames and never blocks the device. When frames are dropped, the next batch starts with an `events-dropped` event whose `event` field is `audio-frames` and whose `source` field is the capture id. The count tells you how much audio you missed.

### Worked example: level meter (Rust)

The plugin opens the default microphone on activation, computes the peak level of each 20 ms chunk, and keeps the last 60 levels. A panel can draw them as a sparkline (see [UI](/pages/api-ui/)).

```rust
use ilium_plugin_sdk::exports::{Activation, DeactivationReason, Handler, Lifecycle};
use ilium_plugin_sdk::host::audio;
use ilium_plugin_sdk::prelude::*;

const LEVEL_HISTORY: usize = 60;

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct LevelMeter {
    capture: Option<u64>,
    levels: Vec<u8>,
}

impl Lifecycle for LevelMeter {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        self.capture = Some(audio::open_capture(None, 16_000, 1)?);
        Ok(())
    }

    fn deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        if let Some(capture) = self.capture.take() {
            audio::close_capture(capture);
        }
        None
    }
}

impl Handler for LevelMeter {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::AudioFrames(frames) = event else { continue };
            if Some(frames.capture) != self.capture {
                continue;
            }
            let peak = frames.samples.iter().map(|sample| sample.unsigned_abs()).max();
            let level = (u32::from(peak.unwrap_or(0)) * 100 / 32_768).min(100) as u8;
            if self.levels.len() == LEVEL_HISTORY {
                self.levels.remove(0);
            }
            self.levels.push(level);
        }
        Vec::new()
    }
}
```

The `audio::open_capture` call is synchronous, so `activate` can return its error at once. A `not-permitted` error means the user declined `audio.capture`, and the plugin should keep working without the meter.

### Worked example: tone playback (TypeScript)

The plugin plays a one-second 440 Hz tone when the user runs its command. The command is declared in the manifest. The `play` call is async: `await` resolves when the sound has finished.

```ts
import { definePlugin, PluginError, type Event, type Submission } from '@ilium/plugin-sdk';
import * as audio from '@ilium/plugin-sdk/host/audio';
import { log } from '@ilium/plugin-sdk/host/log';

const SAMPLE_RATE = 16_000;
const TONE_HZ = 440;
const TONE_SECONDS = 1;
const PLAY_COMMAND = 'com.example.tone/play';

function makeTone(): Int16Array {
  const samples = new Int16Array(SAMPLE_RATE * TONE_SECONDS);
  for (let index = 0; index < samples.length; index++) {
    const phase = (2 * Math.PI * TONE_HZ * index) / SAMPLE_RATE;
    samples[index] = Math.round(Math.sin(phase) * 8_000);
  }
  return samples;
}

async function playTone(): Promise<void> {
  try {
    await audio.play(makeTone(), SAMPLE_RATE, 1);
    log('info', 'tone finished');
  } catch (error) {
    const code = error instanceof PluginError ? error.code : 'unknown';
    log('warn', `tone not played: ${code}`);
  }
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {
    log('info', 'tone plugin ready');
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === PLAY_COMMAND) {
        void playTone();
      }
    }
    return [];
  },
});
```

The `playTone` promise is not awaited inside `onEvents`. The SDK drains pending promises before it returns from each export call, so the play request finishes on its own. Errors are caught here because an unhandled rejection would be a trap.

### What the user sees

The first activation shows the grant prompt. The user ticks the H rows they want:

<pre class="term"><span class="t-dim">╭  ≡ ● · level-meter asks for ───────────────────────────╮</span>
<span class="t-dim">│</span> [ ] audio.capture   Record from the microphone <span class="t-warn">(H)</span>     <span class="t-dim">│</span>
<span class="t-dim">│</span>                                                        <span class="t-dim">│</span>
<span class="t-dim">│</span>      Esc Not now                 Enter Allow selected  <span class="t-dim">│</span>
<span class="t-dim">│</span> Esc cancel · Enter allow                               <span class="t-dim">│</span>
<span class="t-dim">╰────────────────────────────────────────────────────────╯</span>
</pre>

Illustrative, not generated. The prompt for a plugin that asks for one H capability, drawn with the client's rounded border, chrome title, key-hint buttons and dim hint line. The wording is the host's.

Plugin logs show what the plugin and the host did. `ilium plugin logs` prints them:

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs level-meter
<span class="t-dim">2026-10-11 09:14:02</span> <span class="t-ok">info</span>  capture 4 opened: 16000 Hz, 1 channel
<span class="t-dim">2026-10-11 09:14:03</span> <span class="t-warn">warn</span>  audio-frames: 2 oldest frames dropped (capture 4)
<span class="t-dim">2026-10-11 09:14:09</span> <span class="t-ok">info</span>  capture 4 closed by deactivate
<span class="t-prompt">$</span> <span class="t-dim">3 lines shown, 0 dropped</span>
</pre>

Illustrative, not generated. The log of the level meter while the user speaks, then closes its panel.

<figure>
<svg viewBox="0 0 400 150" role="img" aria-label="Audio capture flow from device to plugin">
<title>Audio capture flow</title>
<defs>
<marker id="audio-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="8" y="20" width="110" height="40" rx="6" fill="none" stroke="currentColor"/>
<text x="63" y="45" font-size="12" text-anchor="middle" fill="currentColor">Device</text>
<line x1="120" y1="40" x2="144" y2="40" stroke="currentColor" marker-end="url(#audio-arrow)"/>
<rect x="146" y="20" width="110" height="40" rx="6" fill="none" stroke="currentColor"/>
<text x="201" y="45" font-size="12" text-anchor="middle" fill="currentColor">Capture stream</text>
<line x1="258" y1="40" x2="282" y2="40" stroke="currentColor" marker-end="url(#audio-arrow)"/>
<rect x="284" y="20" width="108" height="40" rx="6" fill="none" stroke="currentColor"/>
<text x="338" y="45" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
<text x="63" y="84" font-size="10" text-anchor="middle" fill="currentColor">Mic or loopback</text>
<text x="201" y="84" font-size="10" text-anchor="middle" fill="currentColor">20 ms per chunk</text>
<text x="338" y="84" font-size="10" text-anchor="middle" fill="currentColor">audio-frames event</text>
<text x="8" y="118" font-size="11" fill="currentColor">Queue full: oldest frame dropped and counted.</text>
<text x="8" y="134" font-size="11" fill="currentColor">A slow handler loses old audio, never the device.</text>
</svg>
<figcaption>Frames reach the plugin in 20 ms events. A full queue drops the oldest frame.</figcaption>
</figure>

### What the level meter looks like

The meter plugin from the Rust example draws its 60 levels as a sparkline in a panel. The client's sparkline and "Blocks" progress preset use the block ramp from U+2581 to U+2588, so a plugin that wants the same look uses those characters:

<pre class="term"><span class="t-dim">╭  ≡ ● · Level meter ────────────────────────────────────╮</span>
<span class="t-dim">│</span> <span class="t-ok">▁▁▂▂▃▅▆▇█▇▅▃▂▂▁▁▁▂▃▅▇██▆▄▂▁▁▁▁▂▄▆▇▇▅▃▂▁▁▁▁▁▂▃▅▆▇▅▃▂▁▁▁</span> <span class="t-dim">│</span>
<span class="t-dim">│</span> Mic: default input · 16000 Hz · 1 channel · peak 78%   <span class="t-dim">│</span>
<span class="t-dim">╰────────────────────────────────────────────────────────╯</span>
</pre>

Illustrative, not generated. The panel chrome and the block ramp follow the client code; the levels are invented.

### Sequence of capture and playback

<figure>
<svg viewBox="0 0 440 270" role="img" aria-label="Sequence of audio capture and playback calls">
<title>Audio capture and playback</title>
<desc>The plugin opens a capture and receives an id. The host sends audio-frames events every 20 milliseconds. The plugin closes the capture. For playback, the plugin calls play, receives a request id, and request-completed arrives when the sound ends.</desc>
<defs>
<marker id="aud-seq-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker>
</defs>
<text x="90" y="16" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
<text x="350" y="16" font-size="12" text-anchor="middle" fill="currentColor">Host and device</text>
<line x1="90" y1="24" x2="90" y2="262" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="350" y1="24" x2="350" y2="262" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="90" y1="42" x2="348" y2="42" stroke="currentColor" marker-end="url(#aud-seq-arrow)"/>
<text x="96" y="36" font-size="10" fill="currentColor">open-capture(none, 16000, 1)</text>
<line x1="348" y1="60" x2="92" y2="60" stroke="currentColor" marker-end="url(#aud-seq-arrow)"/>
<text x="344" y="54" font-size="10" text-anchor="end" fill="currentColor">capture id 4</text>
<line x1="348" y1="84" x2="92" y2="84" stroke="currentColor" marker-end="url(#aud-seq-arrow)"/>
<text x="344" y="78" font-size="10" text-anchor="end" fill="currentColor">audio-frames (20 ms, 320 samples)</text>
<line x1="348" y1="102" x2="92" y2="102" stroke="currentColor" marker-end="url(#aud-seq-arrow)"/>
<text x="344" y="96" font-size="10" text-anchor="end" fill="currentColor">audio-frames ...</text>
<line x1="90" y1="128" x2="348" y2="128" stroke="currentColor" marker-end="url(#aud-seq-arrow)"/>
<text x="96" y="122" font-size="10" fill="currentColor">close-capture(4)</text>
<line x1="90" y1="168" x2="348" y2="168" stroke="currentColor" marker-end="url(#aud-seq-arrow)"/>
<text x="96" y="162" font-size="10" fill="currentColor">play(samples, 16000, 1)</text>
<line x1="348" y1="186" x2="92" y2="186" stroke="currentColor" marker-end="url(#aud-seq-arrow)"/>
<text x="344" y="180" font-size="10" text-anchor="end" fill="currentColor">request id</text>
<text x="354" y="216" font-size="10" fill="currentColor">sound plays</text>
<line x1="348" y1="240" x2="92" y2="240" stroke="currentColor" marker-end="url(#aud-seq-arrow)"/>
<text x="344" y="234" font-size="10" text-anchor="end" fill="currentColor">request-completed (ok or failed(io))</text>
</svg>
<figcaption>Capture is a stream of events that the plugin closes. Playback is one request that completes when the sound ends.</figcaption>
</figure>

### Audio in scenes and video

The animation world has the same `audio` import, so a scene can open a capture or a loopback for a spectrum scene; the grants are listed under `capabilities.scene`. The `video` widget and the media import play with the sound muted unless the plugin also holds `audio.playback`. Frame pacing, dithering and the shared look still apply to anything the scene draws from audio. See [media](/pages/api-media/).

**Rust**

```rust
// In a scene: loopback for a spectrum, 20 ms chunks arrive as audio-frames events.
let capture = audio::open_loopback(48_000, 2)?;
// Closing is the plugin's job; the host also closes it on deactivate or revoke.
audio::close_capture(capture);
```

**TypeScript**

```ts
const capture = audio.openLoopback(48000, 2);
audio.closeCapture(capture);
```

### Limits

| Limit | Value | Over the limit |
|---|---|---|
| Channels per stream | 1 or 2 | `unsupported` above 2; `invalid-argument` for 0 |
| Sample rate | 8,000 to 192,000 Hz | `invalid-argument` outside the range |
| Open captures per plugin | 2 by default, 4 at the T1 ceiling, 2 at the T2 and T3 ceiling | `quota-exceeded` |
| `audio.play` per call | 30 s or 16 MiB of samples | `invalid-argument` |
| Samples per `play` call | a multiple of `channels` | `invalid-argument` |

### Errors

| Call | Code | When |
|---|---|---|
| `devices` | none | Never fails. Returns an empty list when no audio grant is held (none declared, all declined or revoked); with a grant held, an empty list means the computer has no audio device. |
| `open-capture` | `not-permitted` | `audio.capture` is missing, declined or revoked. |
| `open-capture` | `not-found` | The device id does not exist. |
| `open-capture` | `busy` | Another client holds the device exclusively. |
| `open-capture` | `unsupported` | More than 2 channels. |
| `open-loopback` | `unsupported` | The platform has no monitor source for the desktop output. |
| `play` | `io` | The output device failed during playback. |

A grant revoked while a stream is open makes its frames stop, and `not-permitted` is returned by the next call that needs the grant. Streams close on their own when the plugin deactivates, is disabled or is uninstalled, so a plugin does not leak devices if it forgets `close-capture`.

### Used by

- A voice session that opens a capture and reads `audio-frames`, then sends the audio to its provider with `http.fetch`.
- A voice onboarding demo that captures a short sample and plays it back with `audio.play`.
- A device picker that lists devices with `audio.devices` and offers the choice in a `choice` widget (see [Input widgets](/pages/widgets-input/)).

### See also

- [Capability: audio](/pages/capability-audio/) for the grant rules and the review dialog.
- [Events](/pages/api-events/) for the `audio-frames` delivery policy and `events-dropped`.
- [Requests and outcomes](/pages/requests-and-outcomes/) for how `play` completes.
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/) for the generated names.
- [Media](/pages/api-media/) for video decoding.
- [Errors and limits](/pages/reference-errors-and-limits/) for the error codes.
