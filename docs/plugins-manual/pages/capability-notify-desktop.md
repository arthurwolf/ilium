---
layout: page
title: "Capability: notify.desktop"
permalink: /pages/capability-notify-desktop/
---

## notify.desktop

The `notify.desktop` capability lets a plugin show a desktop notification. The plugin submits the intent `notify` with a title, a body, an urgency and an optional sound. The host turns the intent into a notice, picks the desktop form that matches the urgency, and reports `ok` once the notice is queued. The host does not wait for the user to read it.

A notice is an optional extra. A plugin should keep its own in-app status line working, because the user can decline the grant at any time.

**You will need:**

- A `notify.desktop` grant in the manifest of the plugin. The grant takes no argument.
- An SDK that exposes the `notify` intent. The Rust path is `ilium_plugin_sdk::types::intents`, and the TypeScript import is `@ilium/plugin-sdk/types`.
- A sound id from `sound.catalog()` if you want a sound. See [sound.play](/pages/capability-sound-play/).
- A `dedup-key` if the same notice could be submitted twice, as described in the section on duplicates below.

<figure>
<svg viewBox="0 0 400 260" role="img" aria-label="A notify intent passes the batch checks, then becomes a desktop notice or a footer notice, and the plugin receives ok or a refusal">
  <defs>
    <marker id="arr-notify" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="20" y="8" width="360" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="200" y="30" text-anchor="middle" fill="currentColor" font-size="12">Plugin submits the notify intent</text>
  <line x1="200" y1="44" x2="200" y2="70" stroke="currentColor" marker-end="url(#arr-notify)"/>
  <rect x="20" y="70" width="360" height="46" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="200" y="90" text-anchor="middle" fill="currentColor" font-size="11">Checks: grant, dedup key, title and body size, rate</text>
  <text x="200" y="106" text-anchor="middle" fill="currentColor" font-size="10">A failed check refuses the submission, nothing is shown</text>
  <line x1="170" y1="116" x2="110" y2="144" stroke="currentColor" marker-end="url(#arr-notify)"/>
  <line x1="230" y1="116" x2="290" y2="144" stroke="currentColor" marker-end="url(#arr-notify)"/>
  <rect x="20" y="144" width="170" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="105" y="168" text-anchor="middle" fill="currentColor" font-size="11">Desktop notice</text>
  <rect x="210" y="144" width="170" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="295" y="168" text-anchor="middle" fill="currentColor" font-size="11">Client footer notice</text>
  <line x1="105" y1="184" x2="105" y2="212" stroke="currentColor" marker-end="url(#arr-notify)"/>
  <line x1="295" y1="184" x2="295" y2="212" stroke="currentColor" marker-end="url(#arr-notify)"/>
  <rect x="20" y="212" width="360" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="234" text-anchor="middle" fill="currentColor" font-size="11">Plugin receives ok, or refused(code)</text>
</svg>
<figcaption>A notify intent passes the checks. A desktop notice is shown, unless an attached client reports terminal focus, in which case the notice goes to the client footer. Both paths return ok.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `notify.desktop` | yes | the only form |
| `notify.desktop:all` | no | the grant takes no argument; the manifest fails with `manifest.grant-argument` |
| `notify` | no | `notify` is the intent name, not the grant |

The grant is a normal risk capability. It is not marked high risk.

### Surface

The grant unlocks one intent and one related host function from the sound module.

| Name | Kind | Type and meaning |
|---|---|---|
| `notify` | intent | Submits a notice. Payload is `notify-spec`. Rust: `Intent::Notify(NotifySpec)`. |
| `notify-spec.title` | field | `string`, 1 to 256 characters. The first line of the notice. |
| `notify-spec.body` | field | `option<string>`, 0 to 2,048 characters when present. The text below the title; none shows a title-only notice. Rust: `Option<String>`. TypeScript: `string \| undefined`. |
| `notify-spec.urgency` | field | `enum urgency`: `low`, `normal` or `high`. Rust: `Urgency::Low` and so on. TypeScript: `'low'`, `'normal'`, `'high'`. |
| `notify-spec.sound` | field | `option<string>`, a sound id, or none. Rust: `Option<String>`. TypeScript: `string \| undefined`. |
| `sound.catalog` | host function | `func() -> list<string>`. Returns the bundled and contributed sound ids. |
| `ok` | outcome | The notice is queued. |

The core payload is a record with these four fields. The WIT form is `record notify-spec { title: string, body: option<string>, urgency: urgency, sound: option<string> }` (the full record also carries the optional coalescing fields described in [notifications](/pages/api-notifications/)).

### Urgency

The urgency sets the form that the desktop uses for the notice. The host does not change the text.

| Urgency | Linux (freedesktop) | Windows (toast) | macOS |
|---|---|---|---|
| `low` | urgency 0 | silent | passive |
| `normal` | urgency 1 | default toast | active |
| `high` | urgency 2 (critical) | `urgent` scenario | time-sensitive |

Use `high` only for something the user must see soon, such as a failed export that blocks further work. Overusing `high` makes the desktop treat every notice as urgent.

### Focus and visibility

A notice is shown in one of two places:

- The desktop notification, when no attached client reports terminal focus.
- The client footer, when an attached client reports terminal focus (focus reporting, DEC mode 1004). The desktop notification is then replaced by a footer notice in the Ilium client.

`notify-spec` has no pane field, so the host cannot tell which pane a notice concerns. The rule therefore applies to every notice. A plugin cannot choose the place.

The host passes `title` and `body` to the desktop unchanged. How a desktop shortens a long line is up to that desktop.

### Sound

The `sound` field names a sound id. The id must be in the catalogue, which is the set returned by `sound.catalog()`. The sound plays with the notice in either place, and it needs only `notify.desktop`. A plugin does not also need `sound.play` for a sound inside a notice. An unknown id refuses the whole submission with `refused(not-found)`, so no notice is shown.

### Duplicates and dedup keys

The submission carries a `dedup-key`. The key is optional for `notify`, but a plugin that can submit the same notice twice should set one.

- Format: 1 to 128 characters from `[A-Za-z0-9._:/-]`. Any other character makes the submission malformed.
- Scope: the pair of plugin id and key, in the kernel that applies the intent.
- Window: 10 minutes from the moment the key is reserved.
- Duplicate: a submission whose key is reserved completes as `duplicate(original)` and shows nothing. `original` is the `request-ref` of the submission that reserved the key.
- A refused submission does not reserve its key, so a retry with the same key is correct after the cause is fixed.

In Rust, `dedup::key_for(&["notes-export", "2026-10-11"])` joins the parts with `:` and returns `notes-export:2026-10-11`. TypeScript has no helper, so write the string.

### Worked example: manifest

The manifest grants the capability to the client side. Notifications come from the client.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.notes",
  "capabilities": { "client": ["notify.desktop"] }
}
```

### Worked example: Rust

This client plugin shows a notice when its export command runs. The example fixes the date in the key. A real plugin uses an id for each export run.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::dedup;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::intents::{Intent, NotifySpec, Submission, Urgency};

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct NotesNotifier;

impl Handler for NotesNotifier {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::CommandInvoked(invoked) = event else { continue };
            if invoked.command != "com.example.notes/export" { continue; }
            submissions.push(Submission::keyed(
                Intent::Notify(NotifySpec {
                    title: "Export finished".into(),
                    body: Some("12 notes written to the archive folder.".into()),
                    urgency: Urgency::Normal,
                    sound: Some("done".into()),
                }),
                dedup::key_for(&["notes-export", "2026-10-11"]),
            ));
        }
        submissions
    }
}
```

### Worked example: TypeScript

The same behaviour in TypeScript. The sound is used only when the catalogue contains it.

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { catalog } from '@ilium/plugin-sdk/host/sounds';

const EXPORT_COMMAND = 'com.example.notes/export';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    const soundIds = catalog();
    for (const event of batch) {
      if (event.tag !== 'command-invoked' || event.val.command !== EXPORT_COMMAND) continue;
      submissions.push({
        intent: {
          tag: 'notify',
          val: {
            title: 'Export finished',
            body: '12 notes written to the archive folder.',
            urgency: 'normal',
            sound: soundIds.includes('done') ? 'done' : undefined,
          },
        },
        correlation: 2n,
        dedupKey: 'notes-export:2026-10-11',
      });
    }
    return submissions;
  },
});
```

The `correlation` value is a `bigint` in TypeScript. It names the submission, so the plugin can match the outcome to the batch that caused it.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Grant missing, or declined by the user | `refused(not-permitted)`; the rest of the batch is `refused(cancelled)` | Keep the in-app status line. Do not retry. |
| Sound id not in the catalogue | `refused(not-found)`; no notice is shown | Call `sound.catalog()` first, or omit the sound. |
| `title` empty or over 256 characters | `refused(invalid-argument)` | Fix the text. |
| `body` over 2,048 characters | `refused(invalid-argument)` | Shorten the text. |
| `dedup-key` with a disallowed character or over 128 characters | `refused(invalid-argument)` for the lowest-index offender; every other submission completes as `refused(cancelled)` ([ABI-009]) | Fix the key. |
| Same key within 10 minutes | `duplicate(original)`; no second notice | Nothing. The first notice stands. |
| More than 6 notices per minute | `refused(rate-limited)` for the extra ones | Wait a minute, or merge the notices. |
| Grant revoked later | Later submissions are `refused(not-permitted)`; queued notices stay | Re-read the grant on activation. |

A notice already queued is not recalled if the grant is revoked afterwards. The desktop may also hide a notice under its own focus or do-not-disturb rules. The plugin receives `ok` in both cases, because `ok` means the notice was queued, not that the user saw it.

### What the user sees

The review dialog lists the grant when a T2 or T3 package is installed. The T1 bundled plugins receive the grant automatically up to their ceiling.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.notify 0.1.0 by Example Ltd                                    │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ [x] Shows desktop notifications <span class="t-dim">………………………………………………………………………</span> notify.desktop │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests notify.desktop, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.notes > Capabilities, or from the terminal.

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.notes
<span class="t-dim">Grant             Risk     State</span>
notify.desktop     normal   <span class="t-ok">granted</span>
<span class="t-prompt">$</span> ilium plugin grants com.example.notes --revoke notify.desktop
<span class="t-ok">ok</span>   notify.desktop revoked for com.example.notes
<span class="t-warn">warn</span> plugin re-activated with the reduced grant set
</pre>

<figcaption>Illustrative, not generated. Listing the grants of a plugin and revoking one.</figcaption>
</figure>

A revocation re-activates the plugin with the reduced grant set. The plugin receives `deactivate(revoked)` and its activation does not count as a failure.

### Desktop notices are never the only signal

The desktop may hide a notice (focus or do-not-disturb rules) and `ok` only means it was queued.

| ID | Rule | Exact value | Source |
|---|---|---|---|
| CAP-882 | In-app twin of a desktop notice | Every `notify` intent MUST be accompanied by an in-app text signal of the same event that stays until the user dismisses it or the event ends. Dismissal is the user closing or clearing the row, and the event ends when the condition the notice reports stops being true (for example the agent leaves the waiting state), after which the row is removed within 1 second. The desktop notice alone is not a valid signal | `src/docs/notifications.md` |

### Risk and tier rules

- The grant has the normal risk. The review dialog lists it, and it is checked by default.
- A notice is visible to anyone who looks at the screen. Do not put private transcript text, file paths with personal names or secrets into the title or body.
- T1 bundled plugins receive the grant automatically up to their manifest ceiling. T2 and T3 packages show it in the review dialog, and the user keeps or removes it there.

### Who requests it

First-party plugins from [feature traceability](/pages/reference-feature-traceability/) use the grant for:

- 48: desktop notifications.
- 56: text triggers, which often finish with a notice. See [the terminal output filter](/pages/capability-terminal-output-filter/).
- 59: the progress monitor, which notifies when a job finishes.

### See also

- [sound.play](/pages/capability-sound-play/) for the sound catalogue and the `play-sound` intent.
- [agent.read](/pages/capability-agent-read/) for the agent activity that usually triggers an alert.
- [Requests and outcomes](/pages/requests-and-outcomes/) for the `ok`, `refused` and `duplicate` outcomes.
- [Errors and limits](/pages/reference-errors-and-limits/) for the error codes and the rate rules.
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/) for the import paths.

Checked against `ilium:plugin@0.1.0` (API draft of 2026-10-11).

### Security

**[SEC-045]** Desktop notification text is sanitised ([SEC-380]) and rate limited. See [Extension security](/pages/reference-extension-security/).
