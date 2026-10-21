---
layout: page
title: Voice
permalink: /pages/core-plugin-voice/
---

## Voice

Voice is the plugin form of Ilium's voice control. Hold F8 or press the voice button, speak, and Ilium either runs one of its own commands or types a prompt into an agent, asking for confirmation when the action is destructive. Its id is `ilium.core.voice` and it runs on both sides: the client side owns the microphone, the key and the dialogs, and the server side adds the `ilium voice say` command. Capturing audio from the operating system is a kernel service; the plugin drives it through the audio import.

### What it covers

- **Voice session.** F8 and the voice control button start and stop listening (Using F8 and the voice control button in the voice guide).
- **Confirmations and safety.** Destructive commands wait for a confirmation (Confirmations and safety).
- **ilium voice say.** Typed voice input from the command line, with JSONL output (Typed voice input; the `ilium voice say` entry of the CLI reference).
- **Onboarding.** The steps for turning voice on (Step by step: turning voice on).
- **Device selection and reconnect.** Choose a microphone and recover when it returns (Audio devices).

### See it

<figure><img src="/assets/demos/02-voice-control.gif" alt="A voice command is spoken and Ilium runs it after the transcript appears" loading="lazy"><figcaption>Voice control: speak, read the transcript, run the command.</figcaption></figure>

### What the user sees

The first render is the voice strip while listening, with a level meter and the partial transcript. The second is the confirmation dialog before a voice command closes a pane.

<figure>
<pre class="term">╭─ Voice ──────────────────────────────────────╮
│ ● Listening   ▁▂▃▅▆▇▅▃▂▁▁▂▃▄                 │
│<span class="t-dim"> close the pane called docs</span>                   │
│<span class="t-dim"> F8 release to send · Esc cancel</span>              │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The voice strip while the key is held; the meter glyphs depend on the icon set.</figcaption>
</figure>

<figure>
<pre class="term">╭─ Close pane? ────────────────────────────────╮
│ You said: close the pane called docs         │
│ Pane docs and its terminal will close.       │
│                                              │
│    Esc Cancel                Enter Close pane│
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A confirmation before a destructive command; the dialog uses kind <code>danger-confirm</code>.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.voice",
  "capabilities": {
    "server": ["cli.contribute"],
    "client": ["audio.capture", "keys.intercept", "commands.invoke:*",
               "inference", "jobs.run", "settings.write-kernel:voice-control"]
  }
}
```

`commands.invoke:*` lets voice run any command, so the user confirms it at install and the plugin still asks before a destructive one.

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.voice",
  "name": "Voice",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "Voice is the plugin form of Ilium's voice control.",
  "license": "MIT",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0"
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0"
  },
  "capabilities": {
    "server": [
      "cli.contribute"
    ],
    "client": [
      "audio.capture",
      "keys.intercept",
      "commands.invoke:*",
      "inference",
      "jobs.run",
      "settings.write-kernel:voice-control",
      "extensions.provide:ilium.core.voice/transcript",
      "extensions.provide:ilium.core.voice/voice-intents"
    ]
  },
  "activation": [
    "command:voice.toggle",
    "cli:voice"
  ],
  "limits_tier": "first-party",
  "contributes": {
    "commands": [
      { "id": "voice.toggle", "title": "Toggle voice control" }
    ],
    "cli": [{
      "name": "voice",
      "summary": "Voice control: say",
      "output": "jsonl",
      "requires_server": true
    }],
    "jobs": [
      { "kind": "transcribe", "deadline_ms": 30000, "memory_mib": 512 }
    ],
    "extension_points": [
      {
        "id": "transcript",
        "kind": "hook",
        "mode": "transform",
        "title": "Correct the transcript before commands are matched",
        "payload_schema": {
          "type": "object",
          "properties": {
            "text": { "type": "string" },
            "confidence": { "type": "number" },
            "language": { "type": "string" }
          },
          "required": ["text", "confidence", "language"]
        },
        "reply_schema": {
          "type": "object",
          "properties": { "text": { "type": "string" } },
          "required": ["text"]
        },
        "risk": "high",
        "deadline_ms": 20,
        "max_handlers": 8,
        "version": "0.1.0"
      },
      {
        "id": "voice-intents",
        "kind": "collect",
        "title": "Add a spoken phrase that runs a command",
        "entry_schema": {
          "type": "object",
          "properties": {
            "id": { "type": "string" },
            "phrases": { "type": "array", "items": { "type": "string" } },
            "command": { "type": "string" },
            "confirm": { "type": "boolean" }
          },
          "required": ["id", "phrases", "command", "confirm"]
        },
        "risk": "standard",
        "max_entries": 64,
        "dynamic": false,
        "version": "0.1.0"
      }
    ]
  }
}
```

### Kernel tabs this plugin owns

This plugin is the single owner of the following built-in Settings tabs. It holds `settings.write-kernel:<tab>` for each one (T1 only, one holder per tab, see [kernel settings](/pages/api-kernel-settings-and-keymap/#writing-a-kernel-tab)). The values stay in the same `config.toml` tables as before, so no user setting moves. [COR-030] [COR-033]

| Tab id | Label | Config table | Readers |
|---|---|---|---|
| `voice-control` | Voice control | `[voice]` | none |

If the plugin is disabled, the tab stays in the rail with its stored values shown read-only and the notice "Voice is disabled. Values are kept." The kernel keeps reading the keys it consumes itself (listed in the Consumer column of the key table in [config and data migration](/pages/reference-config-and-data-migration/)). Behaviour that only this plugin provides is off. The exact result for this plugin is its row in [kernel and boot](/pages/reference-kernel-and-boot/). [COR-022]

### Samples

#### 1. Push to talk on F8

Holding F8 records and releasing it stops. The plugin grabs the key with [keys](/pages/api-keys/) and the [keys.intercept grant](/pages/capability-keys-intercept/), starts capture on `key-pressed`, stops on `key-released`, and releases the grab when voice is turned off. See [events](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::host::keys::{self, GrabSpec};
use ilium_plugin_sdk::prelude::*;

// manifest: "capabilities": { "client": ["keys.intercept"] }
#[ilium_plugin(side = "client")]
#[derive(Default)]
struct Voice {
    grab: Option<u64>,
    listening: bool,
}

impl Lifecycle for Voice {}

impl Handler for Voice {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::KeyPressed(key) if key.grab == self.grab && !key.repeat => {
                    self.listening = true; // start capture, see the next sample
                }
                Event::KeyReleased(key) if key.grab == self.grab => {
                    self.listening = false; // stop capture, start transcription
                }
                Event::SettingsChanged(_) if !voice_enabled() => self.release_f8(),
                Event::SettingsChanged(_) if self.grab.is_none() => self.grab_f8(),
                _ => {}
            }
        }
        Vec::new()
    }
}

impl Voice {
    fn grab_f8(&mut self) {
        // Release reports need the kitty keyboard protocol in the terminal.
        let spec = GrabSpec { chords: vec!["F8".into()], pane: None, until_release: true };
        self.grab = keys::grab(&spec).ok();
    }

    fn release_f8(&mut self) {
        if let Some(grab) = self.grab.take() {
            keys::release(grab);
        }
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { grab, release } from '@ilium/plugin-sdk/host/keys';

// manifest: "capabilities": { "client": ["keys.intercept"] }
let f8: bigint | undefined;
let listening = false;

function grabF8(): void {
  // Release reports need the kitty keyboard protocol in the terminal.
  f8 = grab({ chords: ['F8'], pane: undefined, untilRelease: true });
}

function releaseF8(): void {
  if (f8 !== undefined) release(f8);
  f8 = undefined;
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'key-pressed' && event.val.grab === f8 && !event.val.repeat) {
        listening = true; // start capture, see the next sample
      } else if (event.tag === 'key-released' && event.val.grab === f8) {
        listening = false; // stop capture, start transcription
      } else if (event.tag === 'settings-changed') {
        if (!voiceEnabled()) releaseF8();
        else if (f8 === undefined) grabF8();
      }
    }
    return [];
  },
});
```

#### 2. Capture microphone frames

Audio frames feed the speech model in a job. The plugin opens a 16 kHz capture with [audio](/pages/api-audio/) and the [audio grant](/pages/capability-audio/), buffers `audio-frames` events, and starts a transcription [job](/pages/api-jobs/) when the key is released.

**Rust**

```rust
use ilium_plugin_sdk::host::{audio, jobs, log::{self, Level}};
use ilium_plugin_sdk::prelude::*;

// manifest: "capabilities": { "client": ["audio.capture", "jobs.run",
//   "inference"] }, "contributes": { "jobs": [{ "kind": "transcribe",
//   "deadline_ms": 30000, "memory_mib": 512 }] }
impl Voice {
    fn start_capture(&mut self) {
        // 16 kHz, one channel: the format the speech model expects.
        self.capture = audio::open_capture(None, 16_000, 1).ok();
        self.samples.clear();
    }

    fn on_audio(&mut self, event: Event) {
        match event {
            Event::AudioFrames(frames) if Some(frames.capture) == self.capture => {
                self.samples.extend_from_slice(&frames.samples);
            }
            _ => {}
        }
    }

    fn stop_and_transcribe(&mut self) {
        if let Some(capture) = self.capture.take() {
            audio::close_capture(capture);
        }
        let pcm: Vec<u8> = self.samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        task::spawn(async move {
            match jobs::start("transcribe", &pcm).await {
                Ok(text) => log::log(Level::Info, &format!("heard {} bytes of text", text.len())),
                Err(error) => log::log(Level::Warn, &format!("transcription failed: {error:?}")),
            }
        });
    }
}
```

**TypeScript**

```ts
import type { Event } from '@ilium/plugin-sdk';
import * as audio from '@ilium/plugin-sdk/host/audio';
import { start } from '@ilium/plugin-sdk/host/jobs';
import { log } from '@ilium/plugin-sdk/host/log';

// manifest: "capabilities": { "client": ["audio.capture", "jobs.run",
//   "inference"] }, "contributes": { "jobs": [{ "kind": "transcribe",
//   "deadline_ms": 30000, "memory_mib": 512 }] }
let capture: bigint | undefined;
let samples: number[] = [];

export function startCapture(): void {
  // 16 kHz, one channel: the format the speech model expects.
  capture = audio.openCapture(undefined, 16_000, 1);
  samples = [];
}

export function onAudio(event: Event): void {
  if (event.tag === 'audio-frames' && event.val.capture === capture) {
    samples.push(...event.val.samples);
  }
}

export async function stopAndTranscribe(): Promise<void> {
  if (capture !== undefined) audio.closeCapture(capture);
  capture = undefined;
  const pcm = new Uint8Array(new Int16Array(samples).buffer);
  try {
    const text = await start('transcribe', pcm);
    log('info', `heard ${text.length} bytes of text`);
  } catch (error) {
    log('warn', `transcription failed: ${String(error)}`);
  }
}
```

#### 3. Voice runs Ilium commands

A recognised phrase runs a command after confirmation. The plugin maps the transcript to a command id, shows a `dialog` from [layout and containers](/pages/widgets-layout-and-containers/) for destructive ones, and then submits the `invoke-command` [intent](/pages/api-intents/). It needs the [commands.invoke grant](/pages/capability-commands-invoke/); see also [commands](/pages/api-commands/).

**Rust**

```rust
use ilium_plugin_sdk::host::ui;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::InvokeCommandSpec;
use ilium_plugin_sdk::types::layout::Constraint;
use ilium_plugin_sdk::types::ui::PopupAnchor;

// manifest: "capabilities": { "client": ["commands.invoke:*", "ui.panels"] }
// `command_for` is the plugin's own phrase table: transcript -> command id.
impl Voice {
    fn on_transcript(&mut self, transcript: &str) -> Vec<Submission> {
        let Some(command) = command_for(transcript) else { return Vec::new() };
        if is_destructive(&command) {
            // A dialog (rendered with dialog kind danger-confirm) asks first.
            self.waiting = Some(command);
            let anchor = PopupAnchor::Centered((Constraint::Length(52), Constraint::Length(7)));
            let _ = ui::open_popup("confirm-voice", anchor);
            return Vec::new();
        }
        vec![invoke(command)]
    }

    fn on_dialog(&mut self, key: &str, button: &str) -> Vec<Submission> {
        match (key, button, self.waiting.take()) {
            ("confirm-voice", "ok", Some(command)) => vec![invoke(command)],
            _ => Vec::new(),
        }
    }
}

fn invoke(command: String) -> Submission {
    Submission::new(Intent::InvokeCommand(InvokeCommandSpec {
        command,
        arguments_json: "{}".into(),
    }))
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk';
import { openPopup } from '@ilium/plugin-sdk/host/ui';
import { length } from '@ilium/plugin-sdk/ui/build';

// manifest: "capabilities": { "client": ["commands.invoke:*", "ui.panels"] }
// commandFor is the plugin's own phrase table: transcript -> command id.
let waiting: string | undefined;

function invoke(command: string): Submission {
  return { intent: { tag: 'invoke-command', val: { command, argumentsJson: '{}' } }, correlation: 1n, dedupKey: undefined };
}

export function onTranscript(transcript: string): Submission[] {
  const command = commandFor(transcript);
  if (command === undefined) return [];
  if (isDestructive(command)) {
    // A dialog (rendered with kind danger-confirm) asks first.
    waiting = command;
    openPopup('confirm-voice', { tag: 'centered', val: [length(52), length(7)] });
    return [];
  }
  return [invoke(command)];
}

export function onDialog(key: string, button: string): Submission[] {
  const command = waiting;
  waiting = undefined;
  return key === 'confirm-voice' && button === 'ok' && command !== undefined ? [invoke(command)] : [];
}
```

#### 4. ilium voice say

Typed voice input from the command line reaches a client. On `cli-invoked` the server side calls `client_requests.request`, and the client answers its `client-request` with the JSONL result. See [client requests](/pages/api-client-requests/), the [command line](/pages/api-cli/) and [events](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::host::{cli, client_requests};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::{CliInvokedEvent, ClientRequestEvent};

// server manifest: "capabilities": { "server": ["cli.contribute"] },
//   "contributes": { "cli": [{ "name": "voice", "output": "jsonl", ... }] }
// client manifest: "client_requests": [{ "id": "voice-say", "timeout_ms": 5000 }]
impl Handler for VoiceServer {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::ClientAttached(client) => self.client = Some(client),
                Event::CliInvoked(CliInvokedEvent { invocation, argv, .. }) => {
                    let payload = serde_json::json!({ "text": argv.join(" ") }).to_string();
                    match self.client.map(|c| client_requests::request(c, "voice-say", &payload, 5_000)) {
                        Some(Ok(request)) => self.pending.push((request, invocation)),
                        _ => cli::finish(invocation, 1),
                    }
                }
                Event::RequestCompleted(done) => {
                    let Some(at) = self.pending.iter().position(|(r, _)| *r == done.request) else { continue };
                    let (_, invocation) = self.pending.remove(at);
                    let line = client_requests::take_answer(done.request).unwrap_or_default();
                    let _ = cli::write_line(invocation, &line);
                    cli::finish(invocation, if done.outcome.is_ok() { 0 } else { 1 });
                }
                _ => {}
            }
        }
        Vec::new()
    }
}

// Client side: answer the request with the JSONL result.
fn answer(event: &Event) {
    if let Event::ClientRequest(ClientRequestEvent { call, kind, payload_json }) = event {
        if kind == "voice-say" {
            client_requests::respond(*call, Ok(run_typed_voice_input(payload_json)));
        }
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import * as cli from '@ilium/plugin-sdk/host/cli';
import * as clientRequests from '@ilium/plugin-sdk/host/client-requests';

// server manifest: "capabilities": { "server": ["cli.contribute"] },
//   "contributes": { "cli": [{ "name": "voice", "output": "jsonl", ... }] }
// client manifest: "client_requests": [{ "id": "voice-say", "timeout_ms": 5000 }]
let client: bigint | undefined;
const pending = new Map<bigint, bigint>(); // request id -> invocation

export const serverSide = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'client-attached') {
        client = event.val;
      } else if (event.tag === 'cli-invoked') {
        const payload = JSON.stringify({ text: event.val.argv.join(' ') });
        if (client === undefined) { cli.finish(event.val.invocation, 1); continue; }
        pending.set(clientRequests.request(client, 'voice-say', payload, 5_000), event.val.invocation);
      } else if (event.tag === 'request-completed' && pending.has(event.val.request)) {
        const invocation = pending.get(event.val.request)!;
        pending.delete(event.val.request);
        cli.writeLine(invocation, clientRequests.takeAnswer(event.val.request) ?? '{}');
        cli.finish(invocation, event.val.outcome.tag === 'ok' ? 0 : 1);
      }
    }
    return [];
  },
});

// Client side: answer the request with the JSONL result.
export function answer(event: Event): void {
  if (event.tag === 'client-request' && event.val.kind === 'voice-say') {
    clientRequests.respond(event.val.call, { tag: 'ok', val: runTypedVoiceInput(event.val.payloadJson) });
  }
}
```

### Extension points

Other plugins change what `ilium.core.voice` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.voice/<point>`). Contributors need `extensions.contribute:ilium.core.voice/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `transcript` | hook (transform) | Correct the transcript before commands are matched. | text, confidence, language; reply: text | high |
| `voice-intents` | collect | Add a spoken phrase that runs a command. | entry: id, phrases, command, confirm | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.voice/say`: type: type a sentence into the live voice session

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.voice/heard`: { text, command }

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.voice"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Adds the phrase `open the board`

A package adds this entry in its manifest. It needs no code.

```json
{
  "api": "ilium:plugin@0.1.0",
  "capabilities": { "server": ["extensions.contribute:ilium.core.voice/voice-intents"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.voice/voice-intents",
        "id": "open-board",
        "priority": 50,
        "entry": {
          "phrases": [
            "open the board"
          ],
          "command": "open-board",
          "confirm": false
        }
      }
    ]
  }
}
```

### Extension points this plugin owns

The table gives the values that [EXT-052] requires for each point this plugin owns. The page does not state them, so the values come from the defaults in [Extension points](/pages/api-extension-points/) and are marked (default). The complete manifest holds an `extensions.provide:ilium.core.voice/<point>` grant for each point in its client `capabilities` list and declares each point in `contributes.extension_points`.

| Point | Side | Version | Deadline | Max | Dynamic |
|---|---|---|---|---|---|
| `ilium.core.voice/transcript` | client | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) |
| `ilium.core.voice/voice-intents` | client | 0.1.0 (default) | none (default) | 64 entries (default) | no (default) |

Side basis: `ilium.core.voice/transcript` is client because the manifest's server capabilities hold only `cli.contribute`, and the transcript comes from the client-side `transcribe` job (`audio.capture`, `jobs.run`, `inference`) before the client matches commands in its `on_transcript` handler.
Side basis: `ilium.core.voice/voice-intents` is client because the manifest grants `extensions.provide` for it on the client side, and the phrase-to-command matching that consumes these entries runs in the client's `on_transcript` handler, which holds the `commands.invoke:*` grant.

### ABI used

- [keys](/pages/api-keys/) and [keys.intercept](/pages/capability-keys-intercept/)
- [audio](/pages/api-audio/) and [the audio grant](/pages/capability-audio/)
- [jobs](/pages/api-jobs/)
- [commands](/pages/api-commands/), [commands.invoke](/pages/capability-commands-invoke/) and [intents](/pages/api-intents/)
- [layout and containers](/pages/widgets-layout-and-containers/)
- [client requests](/pages/api-client-requests/) and [command line](/pages/api-cli/)
- [events](/pages/api-events/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Inference providers](/pages/core-plugin-inference/)
- [Security](/pages/security/)

### Data and privacy

Voice handles two kinds of data: the audio you speak and the transcript made from it.

- Audio stays in memory. It is never written to disk, a log or a crash report, and the buffer is released when the transcription job ends, fails or is cancelled. [COR-080]
- With a local transcription provider nothing leaves your machine. With a remote provider the audio is sent to the host you configured, and the voice strip shows a "remote" marker while it is. [COR-081]
- Settings names the provider and whether it is local or remote before the first capture. A failing provider never makes Voice switch to a remote one on its own. [COR-082]
- A transcript lives in memory for the current voice session, is not written to the log at Info level or above, and is dropped when the session ends. [COR-083]
- Audio and transcripts are not published on a topic, sent in an event or shown to other plugins. Only the command id a transcript maps to leaves the plugin. [COR-084]

See [Plugin privacy](/pages/user-privacy/#core-plugins-and-ai-providers).

### Security

Risk marks of this plugin's extension points follow the corrected table in [Extension security](/pages/reference-extension-security/) ([SEC-293]).
