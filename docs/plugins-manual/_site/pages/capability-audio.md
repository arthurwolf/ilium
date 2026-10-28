# Capability: audio

## audio.capture, audio.loopback and audio.playback

The three audio grants let a plugin record from a microphone, record the sound the computer is playing, and play samples through an output device. Each direction has its own grant. A plugin that only plays a sound never receives a recording path, and a plugin that only records never receives a playback path.

Capture and loopback are high-risk (**H**) grants. Capture can record everything said in the room. Loopback can record everything the desktop plays, including audio from other applications. Playback is a normal grant.

**You will need:**

- One grant per direction, declared in the manifest under the side that uses it. Client plugins use `capabilities.client`, and animation scenes use `capabilities.scene`.
- The client world (`ilium:client/base`) or the animation world (`ilium:anim/base`). Both accept the three grants (`C A` in section 3 of the API).
- A manifest with `"api": "ilium:plugin@0.1.0"`.
- The SDK import for the audio functions: `ilium_plugin_sdk::host::audio` in Rust, `@ilium/plugin-sdk/host/audio` in TypeScript. The [audio API page](/pages/api-audio/) lists the full import surface.

### Grant syntax

| Grant | Valid | Meaning |
|---|---|---|
| `audio.capture` | yes | records from an input device (H) |
| `audio.loopback` | yes | records the system output (H) |
| `audio.playback` | yes | plays samples through an output device |
| `audio.microphone` | no | the old name, replaced by `audio.capture` |
| `audio` | no | the direction is missing |
| `audio.capture:default` | no | the grant takes no argument; choose the device in `open-capture` |

### Surface

Every function below is in the client world. A grant listed in the second column is needed for the call. Errors are described in the edge cases table.

| Rust | TypeScript | Grant | Returns | Meaning |
|---|---|---|---|---|
| `audio::devices()` | `audio.devices()` | any audio grant | `Vec<AudioDevice>` | Lists the devices the grants allow. |
| `audio::open_capture(device, sample_rate, channels)` | `audio.openCapture(...)` | `audio.capture` | `Result<u64, PluginError>` | Opens an input device and returns a capture id. `None` selects the default input. |
| `audio::open_loopback(sample_rate, channels)` | `audio.openLoopback(...)` | `audio.loopback` | `Result<u64, PluginError>` | Opens a capture of the desktop output. |
| `audio::close_capture(capture)` | `audio.closeCapture(id)` | the grant that opened it | nothing | Stops the stream and releases the device. |
| `audio::play(samples, sample_rate, channels).await` | `await audio.play(...)` | `audio.playback` | `Result<(), PluginError>` | Plays samples. The WIT function returns `result<request-id, plugin-error>`: a request id at once, and the host delivers `request-completed` with that id when playback ends. The SDK `await` resolves at that completion with this result type. |
| `Event::AudioFrames(AudioFramesEvent)` | `{ tag: 'audio-frames', val }` | audio capture stream open | event | One chunk of 20 ms of captured audio. |

The records and the event as WIT declares them:

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

In Rust the device fields are `id`, `name`, `is_input` and `is_default`. In TypeScript they are `isInput` and `isDefault`. A capture id is a `u64`, which is a `bigint` in TypeScript.

### Stream flow

Capture and loopback share one flow. The plugin opens a stream, receives `audio-frames` events until it closes the stream, and the host closes the stream when the plugin deactivates.

<figure>
<svg viewBox="0 0 420 300" role="img" aria-label="Capture stream from grant to close">
  <defs>
    <marker id="aud-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="20" y="8" width="380" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="29" text-anchor="middle" font-size="11" fill="currentColor">Manifest requests audio.capture (H)</text>
  <line x1="210" y1="42" x2="210" y2="62" stroke="currentColor" marker-end="url(#aud-arrow)"/>
  <rect x="20" y="62" width="380" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="83" text-anchor="middle" font-size="11" fill="currentColor">First activation: user ticks the HIGH row</text>
  <line x1="210" y1="96" x2="210" y2="116" stroke="currentColor" marker-end="url(#aud-arrow)"/>
  <rect x="20" y="116" width="380" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="137" text-anchor="middle" font-size="11" fill="currentColor">Grant stored per plugin and per side</text>
  <line x1="210" y1="150" x2="210" y2="170" stroke="currentColor" marker-end="url(#aud-arrow)"/>
  <rect x="20" y="170" width="380" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="191" text-anchor="middle" font-size="11" fill="currentColor">open-capture(device, 16000, 1) returns id 7</text>
  <line x1="210" y1="204" x2="210" y2="224" stroke="currentColor" marker-end="url(#aud-arrow)"/>
  <rect x="20" y="224" width="380" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="245" text-anchor="middle" font-size="11" fill="currentColor">audio-frames every 20 ms until close-capture</text>
  <text x="210" y="284" text-anchor="middle" font-size="10" fill="currentColor">A declined grant makes open-capture return not-permitted.</text>
</svg>
<figcaption>The grant gates the call. Frames then arrive as events until the stream closes.</figcaption>
</figure>

### Sample format and frame size

Samples are signed 16-bit integers (`s16`) from -32,768 to 32,767. Zero is silence. Samples are interleaved: each frame holds one sample per channel, left before right. Stereo at 48 kHz therefore alternates left and right values. The same layout applies to `audio-frames` events and to `play`.

Each `audio-frames` event holds exactly 20 ms of audio. The sample count is `sample_rate / 50 * channels`: 320 samples at 16 kHz mono, and 1,920 samples at 48 kHz stereo.

Any sample rate from 8,000 to 192,000 Hz is accepted. The host resamples to and from the device rate, so a plugin never needs a fallback rate. One or two channels are accepted.

### Device rules

- `devices()` filters the list by grant. Input devices appear with `audio.capture`, output devices with `audio.playback`, and the default monitor source with `audio.loopback`.
- With every audio grant declined, `devices()` returns an empty list.
- With no audio grant in the manifest at all, `devices()` traps and the plugin fails. Declare at least one audio grant, even one the user may decline.
- `device` takes the `id` string from `devices()`. `None` or `undefined` selects the default input. An id that no longer exists is `not-found`.
- If the device of an open stream is removed, the host closes that stream at once. No further `audio-frames` events arrive for its id, and `close-capture` on that id does nothing. The plugin sees the change by calling `devices()` again and opens the replacement with a new call.

### Risk and tier rules

| Grant | Risk | T1 bundled plugin | T2 and T3 install |
|---|---|---|---|
| `audio.capture` | H | asked once at first activation | unchecked, marked HIGH |
| `audio.loopback` | H | asked once at first activation | unchecked, marked HIGH |
| `audio.playback` | normal | granted up to the manifest ceiling | checked by default |

A T1 bundled plugin with H capabilities starts without them. The first activation shows one prompt per plugin, listing each H capability with a checkbox. The user can decline and be asked again at the next server start, at most three times. Settings > Plugins grants a capability at any time. There is no prompt per call. A grant stays until it is revoked.

The install dialog for a T2 or T3 plugin lists every grant, one row each, with the wording from the review table:

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.audio 0.1.0 by Example Ltd                                     │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ [x] Plays sound through the speakers <span class="t-dim">…………………………………………………………</span> audio.playback │
│ <span class="t-warn">! [ ] Records from the microphone</span> <span class="t-dim">……………………………………………………………………</span> <span class="t-warn">audio.capture</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

Illustrative, not generated. The install dialog for a voice plugin from a signed T2 publisher. The publisher, fingerprint and plugin id are placeholders.

### When the grant is declined, revoked or missing

| Situation | What the call returns | What happens to streams |
|---|---|---|
| The user declined the grant | `not-permitted` from `open-capture`, `open-loopback` and `play` | none opened |
| The grant was revoked while a stream was open | `revoked` for calls in flight | the stream closes, the plugin gets `deactivate(revoked)` and activates again with the reduced grant set |
| The manifest never requested the grant | the call traps | the plugin fails |

Handle a declined grant in `activate` and keep the plugin working without audio. The example below does this, and it is the pattern to copy for any optional grant.

### Worked example: voice input that survives a declined grant (Rust)

The plugin opens the default microphone at 16 kHz mono on activation. If the user declined `audio.capture`, the plugin stays active and counts nothing. It counts chunks whose peak level passes a threshold, and it adds up the frames that the host dropped.

Manifest excerpt:

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.voice",
  "capabilities": { "client": ["audio.capture"] }
}
```

Plugin source:

**Rust**

```rust
use ilium_plugin_sdk::exports::{Handler, Lifecycle};
use ilium_plugin_sdk::host::audio;
use ilium_plugin_sdk::prelude::*;

const CAPTURE_RATE: u32 = 16_000;
const SPEECH_PEAK: u16 = 4_000;

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct VoiceInput {
    capture: Option<u64>,
    speech_chunks: u32,
    dropped_frames: u32,
}

impl Lifecycle for VoiceInput {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        match audio::open_capture(None, CAPTURE_RATE, 1) {
            Ok(capture) => self.capture = Some(capture),
            // The user declined audio.capture. Stay active without voice input.
            Err(error) if error.code == ErrorCode::NotPermitted => {}
            Err(error) => return Err(error),
        }
        Ok(())
    }

    fn deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        if let Some(capture) = self.capture.take() {
            audio::close_capture(capture);
        }
        None
    }
}

impl Handler for VoiceInput {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::AudioFrames(frames) if Some(frames.capture) == self.capture => {
                    let peak = frames.samples.iter().map(|sample| sample.unsigned_abs()).max();
                    if peak.unwrap_or(0) > SPEECH_PEAK {
                        self.speech_chunks += 1;
                    }
                }
                Event::EventsDropped(counts) => {
                    self.dropped_frames += counts
                        .iter()
                        .filter(|count| count.event == "audio-frames")
                        .map(|count| count.count)
                        .sum::<u32>();
                }
                _ => {}
            }
        }
        Vec::new()
    }
}
```

Any other error is returned from `activate` unchanged. A `not-permitted` error is not a failure of the plugin, so the plugin does not return it.

### Worked example: loopback recorder (TypeScript)

The plugin records five seconds of desktop output when the user runs its command. It records 250 chunks of 20 ms, closes the stream, and logs the result. It also closes the stream if the plugin deactivates first. The manifest declares `audio.loopback` under `capabilities.client`, and a `contributes.commands` entry for the command id.

**TypeScript**

```ts
import { definePlugin, PluginError, type Event, type Submission } from '@ilium/plugin-sdk';
import * as audio from '@ilium/plugin-sdk/host/audio';
import { log } from '@ilium/plugin-sdk/host/log';

const RECORD_COMMAND = 'com.example.clip/record-output';
const SAMPLE_RATE = 48_000;
const CHANNELS = 2;
const CHUNKS_TO_RECORD = 250; // 250 chunks of 20 ms is five seconds

let recording: bigint | undefined;
let chunksSeen = 0;

function startRecording(): void {
  if (recording !== undefined) {
    return;
  }
  try {
    recording = audio.openLoopback(SAMPLE_RATE, CHANNELS);
    chunksSeen = 0;
    log('info', `loopback ${recording} opened`);
  } catch (error) {
    const code = error instanceof PluginError ? error.code : 'unknown';
    log('warn', `loopback not opened: ${code}`);
  }
}

function stopRecording(reason: string): void {
  if (recording === undefined) {
    return;
  }
  audio.closeCapture(recording);
  log('info', `loopback closed (${reason}) after ${chunksSeen} chunks`);
  recording = undefined;
}

function onFrames(capture: bigint): void {
  if (capture !== recording) {
    return;
  }
  chunksSeen += 1;
  if (chunksSeen >= CHUNKS_TO_RECORD) {
    stopRecording('complete');
  }
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {},
  deactivate() {
    stopRecording('deactivate');
    return undefined;
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === RECORD_COMMAND) {
        startRecording();
      } else if (event.tag === 'audio-frames') {
        onFrames(event.val.capture);
      } else if (event.tag === 'events-dropped') {
        for (const count of event.val) {
          if (count.event === 'audio-frames') {
            log('warn', `${count.count} loopback frames dropped`);
          }
        }
      }
    }
    return [];
  },
});
```

The `openLoopback` call throws a `PluginError` when the grant is declined or the platform has no monitor source. The plugin catches it and logs the code, so it never traps. A real recorder would copy the samples of each `audio-frames` event into a buffer.

### Edge cases and error codes

| Call | Code | Cause and response |
|---|---|---|
| `open-capture`, `open-loopback`, `play` | `not-permitted` | grant declined or revoked; keep working without audio |
| `open-capture` | `not-found` | the device id is not in `devices()`; list devices again |
| `open-capture`, `open-loopback` | `invalid-argument` | rate outside 8,000 to 192,000 Hz, or zero channels |
| `open-capture`, `open-loopback` | `unsupported` | more than two channels, or loopback with no monitor source |
| `open-capture` | `busy` | the device is held exclusively by another application |
| `open-capture`, `open-loopback` | `quota-exceeded` | too many open streams; the default is 2, and the ceiling is 4 for T1 and 2 for T2 and T3 |
| `play` | `invalid-argument` | longer than 30 s, larger than 16 MiB, or a sample count that is not a multiple of `channels` |
| `play` | `io` (completion) | the output device failed during playback |
| `audio-frames` | `events-dropped` | the handler was too slow; the event names `audio-frames` and the capture id |
| `close-capture` | none | an unknown or already closed id does nothing |

Stream rules that the plugin can rely on:

- A stream closes when its plugin deactivates, is disabled or is uninstalled. No `audio-frames` event arrives after that.
- Frames that do not fit the queue are dropped oldest first. The loss is counted in `events-dropped`, and the drop never blocks the device.
- A `play` call completes when playback ends, not when it starts. The Rust function returns after playback, and the TypeScript `await` resolves to `undefined` at the same point.

### What the user sees

The host shows the install dialog for a T2 or T3 plugin and the first-activation prompt for a T1 plugin. The plugin's own log shows what it did with the streams. `ilium plugin logs` prints it:

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs voice-input
<span class="t-dim">2026-10-11 09:14:02</span> <span class="t-ok">info</span>  capture 4 opened: 16000 Hz, 1 channel
<span class="t-dim">2026-10-11 09:14:07</span> <span class="t-warn">warn</span>  device usb-mic-2 is gone, reopening the default input
<span class="t-dim">2026-10-11 09:14:07</span> <span class="t-ok">info</span>  capture 5 opened: 16000 Hz, 1 channel
<span class="t-dim">2026-10-11 09:14:09</span> <span class="t-warn">warn</span>  2 audio-frames events dropped (capture 5)
<span class="t-prompt">$</span> <span class="t-dim">4 lines shown, 0 dropped</span>
</pre>

Illustrative, not generated. The log of a voice plugin that reopens its microphone after the device is removed. The plugin writes the reopen lines, and the device name is a placeholder.

### Who requests it

First-party plugins from [feature traceability](/pages/reference-feature-traceability/):

- 50 Voice session: `audio.capture`.
- 53 Voice onboarding demo: `audio.capture` and `audio.playback`.
- 54 Voice device selection and reconnect: `audio.capture`.
- 101 Media scenes: `audio.loopback` and `audio.capture`.

### See also

- [audio](/pages/api-audio/) for the SDK import, the sample format and the level meter example.
- [media.video](/pages/capability-media-video/) for the other media decoder.
- [messages and topics](/pages/api-messages-and-topics/) for the voice plugin's messages to its own client side, which need no grant.
- [capabilities](/pages/capabilities/) for the grant model, the H rule and the review dialog.
- [errors and limits](/pages/reference-errors-and-limits/) for the error codes that every import uses.

Checked against `ilium:plugin@0.1.0` (API draft of 2026-10-11).
