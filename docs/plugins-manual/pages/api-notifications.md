---
layout: page
title: Notifications and sounds
permalink: /pages/api-notifications/
---

## Notifications and sounds

A plugin tells the user about an event in two ways. The `notify` intent shows a desktop notice, and the `play-sound` intent plays one sound from the sound catalogue. Both are submitted from the `handler` export, as the outcome of an event, and both are checked by the kernel before anything reaches the desktop or the speakers. The kernel checks the grant, the rate limit and the dedup window in that order. A repeat of a keyed submission inside the window completes as a duplicate and does nothing.

### What you need

- **Grants.** `notify.desktop` for `notify` and `sound.play` for `play-sound`. Reading the sound catalogue with `sound.catalog` needs no grant. Request the grants per side in the manifest. Both intents are available on the `server` and `client` sides.
- **Language.** Rust plugins use the `ilium-plugin-sdk` crate. TypeScript plugins use `@ilium/plugin-sdk`. See the [Rust SDK](/pages/rust-sdk/) and the [TypeScript SDK](/pages/typescript-sdk/).
- **Export.** The plugin exports `handler`, which receives events and returns submissions. See [Events](/pages/api-events/).
- **Sound files.** Each sound ships inside the package under `sounds/` and is declared in the manifest under `contributes.sounds`, as `{ id, file }`.

```json
"capabilities": { "server": ["agent.read", "notify.desktop", "sound.play"] },
"contributes": { "sounds": [{ "id": "ask", "file": "sounds/ask.wav" }] }
```

### Surface

| Name | Rust | TypeScript | Grant | Kind | Meaning |
|---|---|---|---|---|---|
| `sound.catalog` | `host::sounds::catalog() -> Vec<String>` | `catalog(): string[]` from `@ilium/plugin-sdk/host/sounds` | none | sync | Lists the bundled sound ids and the sound ids the plugins contribute. Never fails. |
| `notify` | `Intent::Notify(NotifySpec)` | `{ tag: 'notify', val: NotifySpec }` | `notify.desktop` | submission | Shows a desktop notice. Completes `ok` when shown. |
| `play-sound` | `Intent::PlaySound(String)` | `{ tag: 'play-sound', val: string }` | `sound.play` | submission | Plays one sound id. Completes `ok` when played. |

The `NotifySpec` record has four fields:

| Field | Type | Meaning |
|---|---|---|
| `title` | string | The first line of the notice. |
| `body` | option of string | The second line of the notice. `None` or `undefined` shows a title-only notice. |
| `urgency` | `low`, `normal` or `high` | Sets the priority on each operating system (see the table below). Rust `Urgency::High`, TypeScript `'high'`. |
| `sound` | option of string | A sound id from the catalogue, played with the notice. Use `None` or `undefined` for a silent notice. |

Urgency maps to the native priority of each platform:

| Urgency | Linux (freedesktop) | Windows (toast) | macOS |
|---|---|---|---|
| `low` | urgency 0 | silent | passive |
| `normal` | urgency 1 | default toast | active |
| `high` | urgency 2 (critical) | `urgent` scenario | time-sensitive |

### Dedup keys

Every submission may carry a `dedup-key`. Rust sets it with `Submission::keyed(intent, key)`. `Submission::new(intent)` sends no key. TypeScript sets the field `dedupKey` on the submission literal.

- **Format.** From 1 to 128 characters from `A-Z`, `a-z`, `0-9`, `.`, `_`, `:`, `/` and `-`. A key over 128 characters or with any other character is refused as `refused(invalid-argument)` for the lowest-index offender of the batch; every other submission completes as `refused(cancelled)` with the message `batch refused: submission <index>` ([ABI-009]).
- **Scope.** The pair of plugin id and key. The server and each client keep separate tables, so a server notice and a client notice with the same key do not suppress each other.
- **Window.** 10 minutes from the moment the key is reserved. The table is kept in memory, so a restart clears it, except that from Phase 5 the keys of `send-input` and `agent-action` are journaled in the snapshot and survive.
- **Outcome.** A repeat completes as `duplicate(original)`, where `original` names the submission that reserved the key. Nothing is shown or played. A duplicate is not an error.
- **Reservation.** A key is reserved only after the submission passes its checks. A refused submission does not reserve its key, so a retry with the same key is valid.
- **Batches.** The first submission in a batch that reserves a key blocks every later submission in the same batch with that key. Use a different key for the notice and for the sound, as the examples do.

The key is optional for `notify` and `play-sound`. It is required only for `send-input` and `agent-action`.

<figure>
<svg viewBox="0 0 400 240" role="img" aria-label="Notification submissions pass the dedup window before the notifier and the sound player">
<title>Notification and sound flow</title>
<desc>A plugin submits notify and play-sound with dedup keys. The kernel checks each key against a 10 minute window. A first submission reaches the desktop notifier or the sound player. A repeat completes as a duplicate and reaches neither.</desc>
<defs>
<marker id="arrow-notify" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="130" y="8" width="140" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="200" y="30" font-size="12" text-anchor="middle" fill="currentColor">Plugin handler</text>
<line x1="200" y1="44" x2="200" y2="68" stroke="currentColor" marker-end="url(#arrow-notify)"/>
<text x="208" y="60" font-size="10" fill="currentColor">notify, play-sound</text>
<rect x="40" y="70" width="320" height="50" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="200" y="90" font-size="12" text-anchor="middle" fill="currentColor">Dedup window, 10 minutes</text>
<text x="200" y="108" font-size="10" text-anchor="middle" fill="currentColor">keyed by plugin id and dedup-key</text>
<line x1="95" y1="120" x2="95" y2="158" stroke="currentColor" marker-end="url(#arrow-notify)"/>
<line x1="305" y1="120" x2="305" y2="158" stroke="currentColor" marker-end="url(#arrow-notify)"/>
<text x="103" y="142" font-size="10" fill="currentColor">first: ok</text>
<text x="297" y="142" font-size="10" text-anchor="end" fill="currentColor">repeat: duplicate</text>
<rect x="20" y="158" width="150" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="95" y="180" font-size="12" text-anchor="middle" fill="currentColor">Desktop notice</text>
<rect x="230" y="158" width="150" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="305" y="180" font-size="12" text-anchor="middle" fill="currentColor">Sound player</text>
<text x="200" y="222" font-size="11" text-anchor="middle" fill="currentColor">A repeat within 10 minutes reaches neither intent.</text>
</svg>
<figcaption>The dedup window sits before both intents. The first submission with a key reaches the notifier or the sound player. A repeat with the same key reaches neither.</figcaption>
</figure>

### Example 1: an approval notice with a sound

The plugin announces an agent pane that starts waiting for approval. Each pane is announced at most once in 10 minutes. The notice and the sound carry different keys, so both are delivered on the first change. The notice has no `sound` field, because the separate `play-sound` submission already plays the sound. Passing both would play it twice.

**Grants:** `agent.read`, `notify.desktop`, `sound.play`. **Side:** `server`.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::intents::{NotifySpec, Urgency};
use ilium_plugin_sdk::types::tree::AgentActivity;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct ApprovalNotice;

impl Handler for ApprovalNotice {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::AgentActivityChanged(AgentActivityEvent { node, current, .. }) =
                event
            else {
                continue;
            };
            if current != AgentActivity::AwaitingApproval {
                continue;
            }
            submissions.push(Submission::keyed(
                Intent::Notify(NotifySpec {
                    title: "Agent needs approval".to_string(),
                    body: Some(format!("Pane {node} is waiting for you")),
                    urgency: Urgency::High,
                    sound: None,
                }),
                format!("approval-notice-{node}"),
            ));
            submissions.push(Submission::keyed(
                Intent::PlaySound("ask".to_string()),
                format!("approval-sound-{node}"),
            ));
        }
        submissions
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'agent-activity-changed') continue;
      const { node, current } = event.val;
      if (current !== 'awaiting-approval') continue;
      submissions.push({
        intent: {
          tag: 'notify',
          val: {
            title: 'Agent needs approval',
            body: `Pane ${node} is waiting for you`,
            urgency: 'high',
            sound: undefined,
          },
        },
        correlation: 1n,
        dedupKey: `approval-notice-${node}`,
      });
      submissions.push({
        intent: { tag: 'play-sound', val: 'ask' },
        correlation: 2n,
        dedupKey: `approval-sound-${node}`,
      });
    }
    return submissions;
  },
});
```

Outcome: the first change of a pane to `awaiting-approval` completes both submissions as `ok`. A second change of the same pane within 10 minutes completes both as `duplicate(original)`. The user hears one sound and sees one notice.

### Example 2: a client command that picks a catalogue sound

A client plugin adds a command that plays a sound for testing. It checks the catalogue first and falls back to the first available sound. The command id is the plugin's own, so it needs no extra grant.

**Grants:** `sound.play`. **Side:** `client`.

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { catalog } from '@ilium/plugin-sdk/host/sounds';

const PREFERRED_SOUND = 'ask';
const TEST_COMMAND = 'com.example.approval/test-sound';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'command-invoked') continue;
      if (event.val.command !== TEST_COMMAND) continue;
      const available = catalog();
      const soundId = available.includes(PREFERRED_SOUND) ? PREFERRED_SOUND : available[0];
      if (soundId === undefined) continue;
      submissions.push({
        intent: { tag: 'play-sound', val: soundId },
        correlation: BigInt(submissions.length + 1),
        dedupKey: undefined,
      });
    }
    return submissions;
  },
});
```

### Errors and limits

| Situation | Completion | Effect on the batch |
|---|---|---|
| `notify` without `notify.desktop`, or `play-sound` without `sound.play` | `refused(not-permitted)` for that submission | The other submissions complete as `refused(cancelled)`. |
| Sound id not in the catalogue, in `play-sound` or in `notify.sound` | `refused(not-found)` for that submission | The other submissions run. |
| Dedup key with a disallowed character or over 128 characters | `refused(invalid-argument)` for the lowest-index offender | Every other submission completes as `refused(cancelled)` with the message `batch refused: submission <index>` ([ABI-009]). |
| More than 6 `notify` submissions per minute per plugin | `refused(rate-limited)` with `retry-after-ms` | Only that submission. |
| More than 4 `play-sound` submissions per second per plugin | `refused(rate-limited)` with `retry-after-ms` | Only that submission. |
| Repeat of a reserved key within 10 minutes | `duplicate(original)` | None. Not an error. |
| Manifest sound file missing or in another format | Package check fails at install with `manifest.sound-format` | The plugin is not installed. |

The sound id check happens per submission, so one bad id never stops a notice from showing. A refused submission does not reserve its key, so the plugin can fix the id and retry with the same key.

### What the user sees

When the terminal pane the notice is about has focus in an attached client, the desktop notice is replaced by an in-app notice in the client footer. When no attached client reports focus, the desktop notice is shown. A sound plays either way.

<figure>
<pre class="term"><span class="t-dim">╭  ≡ ● · Notice────────────────────────────────╮</span>
<span class="t-dim">│</span> <span class="t-warn">!</span> <span class="t-hi">Agent needs approval</span>                       <span class="t-dim">│</span>
<span class="t-dim">│</span> Pane 14 is waiting for you                   <span class="t-dim">│</span>
<span class="t-dim">│</span>                                              <span class="t-dim">│</span>
<span class="t-dim">│</span>                              Esc Dismiss     <span class="t-dim">│</span>
<span class="t-dim">╰──────────────────────────────────────────────╯</span>
<span class="t-hi">▼ 📂 backend</span>
›   🦀 <span class="t-warn">✋ claude: api fix</span>   <span class="t-warn">awaiting approval</span>
<span class="t-dim">  1 notice shown, 1 sound played, 0 duplicates</span>
</pre>
<figcaption>Illustrative, not generated. A desktop notice above the tree footer, dark theme. The footer counts one notice and one sound, and no duplicate.</figcaption>
</figure>

### Used by

- Sound alerts (feature 47) play a sound when an agent changes activity or a pane exits.
- Desktop notifications (feature 48) show a notice for the same events.
- Coalescing of redundant alerts (feature 49) uses `dedup-key` on both intents.
- Text triggers (feature 56) play a sound and show a notice when an output filter matches.

### Coalescing and toasts

The `notify` intent gains two fields: `coalesce-key` is optional, and `coalesce-window-ms` is a plain `u32` that is `0` when unused. Notifications with the same `coalesce-key` inside `coalesce-window-ms` replace each other, so a burst of "build finished" events shows one notice with the latest text.

```wit
// Excerpt of ilium:types/intents in reference-wit
record notify-spec {
  title: string, body: option<string>, urgency: urgency, sound: option<string>,
  coalesce-key: option<string>,
  coalesce-window-ms: u32,           // at most 60,000; 0 or no key: no coalescing
}
```

Coalescing works with the dedup key described above: the dedup key suppresses a repeated submission, while the coalesce key merges different notices. Rate limits apply to what is shown, not to the replaced submissions.

**Rust**

```rust
let notice = NotifySpec {
    title: "Build finished".into(),
    body: Some(format!("{ok} passed, {failed} failed")),
    coalesce_key: Some("build".into()),
    coalesce_window_ms: 5_000,
    ..NotifySpec::default()
};
Submission::new(Intent::Notify(notice))
```

**TypeScript**

```ts
const notice = {
  title: 'Build finished',
  body: `${ok} passed, ${failed} failed`,
  coalesceKey: 'build',
  coalesceWindowMs: 5000,
};
return [{ intent: { tag: 'notify', val: notice }, correlation: 1n, dedupKey: undefined }];
```

When the user is in the Ilium window, a desktop notice is often the wrong tool. The `toast` intent draws a message inside Ilium with the `notify.toast` grant; see [toast](/pages/api-toast/). A notice in a settings view uses the `notice` row or widget. Desktop notices keep the `notify.desktop` grant and the host's rules for focus.

| Surface | Intent or widget | Grant | Lives |
|---|---|---|---|
| Desktop notice | `notify` | `notify.desktop` | until the desktop dismisses it |
| In-app toast | `toast` | `notify.toast` | 1 to 30 seconds |
| Settings or panel notice | `notice` | none | until dismissed or removed |
| Sound | `play-sound` | `sound.play` | the length of the sound |

### See also

- [Intents](/pages/api-intents/) for the full intent catalogue and outcomes.
- [Events](/pages/api-events/) for `agent-activity-changed` and `pane-exited`.
- [Desktop notifications capability](/pages/capability-notify-desktop/) for the per-platform behaviour.
- [Sound capability](/pages/capability-sound-play/) for the `sound.play` grant.
- [Errors and limits](/pages/reference-errors-and-limits/) for the full error code table.

Checked against `ilium:plugin@0.1.0`.
