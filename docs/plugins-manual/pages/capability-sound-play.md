---
layout: page
title: "Capability: sound.play"
permalink: /pages/capability-sound-play/
---

## sound.play

The `sound.play` capability lets a plugin ask the Ilium client to play a short sound. The plugin does not open an audio device. It submits the intent `play-sound` with a sound id. The host checks the grant and the sound catalogue, and only an accepted id reaches the audio output. The sound comes either from the set bundled with Ilium or from the plugin's own `contributes.sounds` list.

<figure>
<svg viewBox="0 0 420 230" role="img" aria-label="A plugin submits play-sound; the host checks the grant and the catalogue, then plays the sound or refuses it">
  <defs><marker id="arr-sound-play" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="240" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="140" y="30" text-anchor="middle" fill="currentColor" font-size="12">Plugin submits play-sound</text>
  <line x1="140" y1="44" x2="140" y2="66" stroke="currentColor" marker-end="url(#arr-sound-play)"/>
  <text x="152" y="60" fill="currentColor" font-size="11">sound id and dedup-key</text>
  <rect x="20" y="66" width="240" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="140" y="88" text-anchor="middle" fill="currentColor" font-size="12">Grant check: sound.play</text>
  <line x1="140" y1="102" x2="140" y2="122" stroke="currentColor" marker-end="url(#arr-sound-play)"/>
  <text x="152" y="116" fill="currentColor" font-size="11">granted</text>
  <rect x="20" y="122" width="240" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="140" y="144" text-anchor="middle" fill="currentColor" font-size="12">Catalogue check: id in catalog</text>
  <line x1="140" y1="158" x2="140" y2="178" stroke="currentColor" marker-end="url(#arr-sound-play)"/>
  <text x="152" y="172" fill="currentColor" font-size="11">id found</text>
  <rect x="20" y="178" width="240" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="140" y="200" text-anchor="middle" fill="currentColor" font-size="12">Client audio output plays it</text>
  <line x1="260" y1="84" x2="290" y2="84" stroke="currentColor" marker-end="url(#arr-sound-play)"/>
  <line x1="260" y1="140" x2="290" y2="140" stroke="currentColor" marker-end="url(#arr-sound-play)"/>
  <rect x="290" y="64" width="110" height="100" rx="6" fill="none" stroke="currentColor"/>
  <text x="345" y="86" text-anchor="middle" fill="currentColor" font-size="11">Refused:</text>
  <text x="345" y="106" text-anchor="middle" fill="currentColor" font-size="11">not-permitted</text>
  <text x="345" y="124" text-anchor="middle" fill="currentColor" font-size="11">not-found</text>
  <text x="345" y="142" text-anchor="middle" fill="currentColor" font-size="11">rate-limited</text>
  <text x="345" y="160" text-anchor="middle" fill="currentColor" font-size="11">nothing plays</text>
</svg>
<figcaption>A <code>play-sound</code> submission passes the grant check and the catalogue check before it reaches the audio output. A failed check refuses that submission and plays nothing.</figcaption>
</figure>

### What you need

- **A grant.** `sound.play` on the side that submits the intent. Client plugins request it under `capabilities.client`. It is a normal-risk grant, not a high-risk one. T1 bundled plugins receive it up to their manifest ceiling. T2 and T3 packages list it in the review dialog, checked by default.
- **A language.** Rust with `ilium_plugin_sdk`, or TypeScript with `@ilium/plugin-sdk`. The toolchain is described in [Toolchain](/pages/getting-started-toolchain/).
- **A sound id.** Either a bundled id or an id from your own `contributes.sounds` list.

### Grant syntax

The grant takes no argument.

| Grant | Valid | Reason |
|---|---|---|
| `sound.play` | yes | the only form |
| `sound.play:bell` | no | the grant takes no argument; name the sound in the intent |
| `sound` | no | the grant name is incomplete |

### The surface

| Name | Kind | Rust | TypeScript | Meaning |
|---|---|---|---|---|
| `play-sound` | intent, grant `sound.play` | `Intent::PlaySound(String)` | `{ tag: 'play-sound', val: 'done' }` | Asks the client to play the sound with that id. The outcome is in the request result. |
| `catalog` | host function, always granted | `ilium_plugin_sdk::host::sounds::catalog() -> Vec<String>` | `catalog(): string[]` from `@ilium/plugin-sdk/host/sounds` | Returns the sound ids this plugin may play. It is synchronous and has no error code. |
| `contributes.sounds` | manifest key | not applicable | not applicable | A list of `{ "id", "file" }` entries. Each file lives in the `sounds/` folder of the package. |
| `dedup-key` | submission field | `Submission::keyed(intent, key)` | `dedupKey`, an optional string | An optional key that stops a repeat within 10 minutes. |

The Rust prelude brings in `Intent` and `Submission`. Import `sound` from `ilium_plugin_sdk::host` explicitly.

### Sound ids and files

- **Bundled ids.** Ilium ships one bundled sound: `chirping`. It is embedded in the client, so it plays with no sound package installed.
- **Contributed ids.** Each `contributes.sounds` entry adds one id. The id is the plain string you write, such as `done`.
- **Catalogue scope.** `catalog()` and `play-sound` see the bundled ids and the plugin's own ids. A plugin cannot play another plugin's sounds. Such an id is refused with `not-found`.
- **Id rules.** An id must be unique within the manifest, and it must not be `chirping`. A repeated id or a clash with `chirping` fails the package check at install with `manifest.sound-id-taken`.
- **File format.** A sound file must be WAV with PCM samples, or OGG Vorbis. A file that is missing or in another format fails the package check at install with `manifest.sound-format`, and the plugin is not installed.

The manifest for both examples below:

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.notes",
  "capabilities": { "client": ["sound.play"] },
  "contributes": {
    "sounds": [ { "id": "done", "file": "sounds/done.ogg" } ],
    "commands": [ { "id": "finish", "title": "Finish note" } ]
  }
}
```

### Worked example 1: Rust, client side

The plugin plays `done` when the user runs its `finish` command. It checks the catalogue first, so it never submits an id that the host would refuse.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::sounds;
use ilium_plugin_sdk::types::events::CommandInvokedEvent;

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct NotesSound;

impl Handler for NotesSound {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::CommandInvoked(CommandInvokedEvent { command, .. }) = event else {
                continue;
            };
            if command != "com.example.notes/finish" {
                continue;
            }
            if !sounds::catalog().iter().any(|id| id == "done") {
                continue;
            }
            submissions.push(Submission::keyed(
                Intent::PlaySound("done".to_string()),
                "notes-done-2026-10-11",
            ));
        }
        submissions
    }
}
```

The `Submission::keyed` call sets the dedup key. Use a key that names one logical event, such as one note and its revision. A fixed key, as here, only suppresses repeats within the 10-minute window.

### Worked example 2: TypeScript, client side

The same behaviour in TypeScript. The submission is a literal, and `correlation` is a number below 2^63 that the plugin chooses. Every `u64` is a `bigint`.

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { catalog } from '@ilium/plugin-sdk/host/sounds';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    const out: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'command-invoked') continue;
      if (event.val.command !== 'com.example.notes/finish') continue;
      if (!catalog().includes('done')) continue;
      out.push({
        intent: { tag: 'play-sound', val: 'done' },
        correlation: 1n,
        dedupKey: 'notes-done-2026-10-11',
      });
    }
    return out;
  },
});
```

The host answers each submission with a `request-completed` event. The SDK passes that event to `onEvents` unless a future is waiting for it. Check the outcome when a sound matters to the user; a refusal is not an error that stops the plugin.

### Errors and edge cases

| Outcome | When it happens | What the plugin does |
|---|---|---|
| `ok` | The id is in the catalogue and the sound is sent to the audio output. | Nothing. |
| `refused(not-permitted)` | The grant is missing or declined, or was revoked before the call started. Every other submission in the same batch completes as `refused(cancelled)`. | Do not retry. Tell the user the grant is needed. |
| `refused(not-found)` | The id is not in `catalog()`. Only this submission is refused. | Check `catalog()` first. Do not retry the same id. |
| `refused(rate-limited)` | More than 4 `play-sound` submissions per second from this plugin. The result carries `retry-after-ms`. | Wait for `retry-after-ms`, or drop the sound. The key is not reserved, so a retry with the same key is valid. |
| `duplicate(original)` | The same key was reserved within the last 10 minutes. The `original` field names the earlier submission. | Nothing. The earlier sound was already accepted. |
| `failed(io)` | The client has no audio output, or the device fails. | Log it. A retry of a failed play needs a new key. |
| `refused(invalid-argument)` | The `dedup-key` is malformed. | Fix the key. The malformed submission refuses its whole batch. |

A key is 1 to 128 characters from `A-Z`, `a-z`, `0-9`, `.`, `_`, `:`, `/` and `-`.

Edge cases:

- A plugin that submits 5 `play-sound` intents inside one second gets `ok` for 4 and `refused(rate-limited)` for the fifth.
- A sound file that is missing or not WAV or OGG Vorbis fails the install. It never fails at play time.
- A `play-sound` on a client without an audio device completes as `failed(io)`. The plugin keeps running and no failure is counted.
- A grant revoked while a batch is in flight refuses the current submission with `revoked` and the later ones with `refused(cancelled)`. The plugin is re-activated with the reduced grants.

### Rate and dedup rules

- **Rate.** Each plugin may submit 4 `play-sound` intents per second. Its `notify` intents have a separate budget of 6 per minute. Sounds that repeat often should use a dedup key rather than a retry loop.
- **Dedup window.** A key stays reserved for 10 minutes from the moment it is accepted. A refused submission does not reserve its key. A submission that was accepted keeps its key, even when playback later fails.
- **Retries.** Do not retry a refusal in a loop. Retry `rate-limited` after `retry-after-ms`. Do not retry `not-permitted` at all until the user grants the capability.

### What the user sees

The review dialog shows one row for the capability. For a T2 package it looks like this:

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.sound 0.1.0 by Example Ltd                                     │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Plays sounds <span class="t-dim">…………………………………………………………………………………………………………………………</span> sound.play │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests sound.play, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

The row text is "Play sounds", and the grant string follows it on the right. A declined row is shown unchecked and dimmed. The plugin then receives no `sound.play` grant, and its calls are refused as described above. The sound itself comes out of the audio output of the Ilium client, and the plugin shows no notice of its own.

To change the grant later, open Settings > Plugins > the plugin > Capabilities, or run `ilium plugin grants com.example.notes --revoke sound.play`. Calls in flight then return `revoked`, and the plugin is re-activated with the reduced set.

Each refusal with `not-permitted` is written to the plugin log at level `warn`. Read it with `ilium plugin logs`:

<figure>
<pre class="term">
<span class="t-prompt">$</span> ilium plugin logs com.example.notes
2026-10-11 09:14:02 warn  sound.play refused: not-permitted
</pre>
<figcaption>Illustrative, not generated. A refused <code>play-sound</code> in the plugin log.</figcaption>
</figure>

### Sound is never the only signal

A sound cannot be heard by every user, on every terminal or in every room. A plugin that plays a sound MUST also show the same event as text.

| ID | Rule | Exact value | Source |
|---|---|---|---|
| CAP-880 | Visible pair for every sound | Every `play-sound` for an event MUST be accompanied, in the same batch, by a visible text signal for that event (panel row, status line or toast). A batch with a `play-sound` and no visible signal for the event is rejected by the plugin check with `a11y.sound-only` | `src/docs/notifications.md` |
| CAP-881 | Failed sound keeps the visible signal | `failed(io)`, `refused(rate-limited)` and a muted sound setting MUST NOT suppress or delay the paired visible signal | `src/docs/notifications.md` |

### Who requests it

First-party plugins from the feature mapping:

- 47 Sound alerts, which play a sound when an agent finishes or needs approval.
The Text triggers and Progress monitor plugins do not hold `sound.play` and play no sounds.

### WIT

The contract below is an excerpt. The owner is the [WIT reference](/pages/reference-wit/), and that page wins if the two differ.

```wit
// Excerpt of ilium:host/sounds in reference-wit
/// Bundled and contributed sound ids.
catalog: func() -> list<string>;
```

### See also

- [notify.desktop](/pages/capability-notify-desktop/) shows a desktop notification. Its `sound` field plays a sound with the notice and needs only `notify.desktop`.
- [fs.read](/pages/capability-fs-read/) reads the sound files that a plugin reads for itself.
- [Events and intents](/pages/events-and-intents/) describes the intent batch and the outcome model.
- [Errors and limits](/pages/reference-errors-and-limits/) lists the error codes and the rate budgets.

Contract: `ilium:plugin@0.1.0`.
