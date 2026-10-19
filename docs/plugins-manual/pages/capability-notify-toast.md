---
layout: page
title: "Capability: notify.toast"
permalink: /pages/capability-notify-toast/
---

## notify.toast

The `notify.toast` capability lets a plugin show a short in-app notice in the client, called a toast, with a tone, a title, an optional body and up to two action buttons. A toast stays inside Ilium and disappears by itself, unlike a desktop notice from [notify.desktop](/pages/capability-notify-desktop/). A plugin can replace a toast by reusing its `id` and remove it with `dismiss-toast`.

**You will need:**

- A `notify.toast` grant. No argument.
- Commands for the action buttons, if any (declared in `contributes.commands`).

<figure>
<svg viewBox="0 0 420 252" role="img" aria-label="A toast intent is rate limited, shown with a countdown and removed by timeout, action or dismiss">
  <defs><marker id="arr" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="27" text-anchor="middle" fill="currentColor" font-size="12">Plugin submits toast(id, tone, title, body, actions)</text>
  <text x="210" y="43" text-anchor="middle" fill="currentColor" font-size="10">title at most 60 characters</text>
  <line x1="210" y1="52" x2="210" y2="72" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="72" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="91" text-anchor="middle" fill="currentColor" font-size="12">Host checks the grant and the rate</text>
  <text x="210" y="107" text-anchor="middle" fill="currentColor" font-size="10">3 per 10 s per plugin</text>
  <line x1="210" y1="116" x2="210" y2="136" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="136" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="155" text-anchor="middle" fill="currentColor" font-size="12">Client shows the toast with a countdown bar</text>
  <text x="210" y="171" text-anchor="middle" fill="currentColor" font-size="10">an action runs its command</text>
  <line x1="210" y1="180" x2="210" y2="200" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="110" y="224" text-anchor="middle" fill="currentColor" font-size="11">ok</text>
  <rect x="220" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="310" y="224" text-anchor="middle" fill="currentColor" font-size="11">refused(rate-limited, invalid-argument)</text>
</svg>
<figcaption>A toast with an id that is already shown replaces that toast.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `notify.toast` | yes | the only form |
| `notify.toast:danger` | no | no argument; the tone is a field |
| `notify.desktop` | separate | desktop notices |

Normal risk. Toasts have fixed size limits, at most two actions, and a rate limit of 3 per 10 s per plugin, so a plugin cannot flood the screen.

### What it unlocks

| Name | Kind | Meaning |
|---|---|---|
| `toast` | intent | `id?`, `tone`, `title` (at most 60), `body?` (at most 200 characters), `actions` (at most 2; `{ label, command }`), `duration-ms?` (1,000 to 30,000). |
| `dismiss-toast` | intent | `id`. Outcome `ok`. |

The default duration is 5,000 ms, and a `danger` toast stays until dismissed while a client is attached; a queued toast of any level, `danger` included, is discarded after its duration when no client attaches. See [Notifications](/pages/api-notifications/) and [Intents](/pages/api-intents/).

### Manifest

The grant goes into `manifest.json` under `capabilities`, in the list of the side that uses it (server or client).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.exporter",
  "capabilities": {
    "client": [
      "notify.toast"
    ]
  }
}
```

### Worked example

This plugin shows a success toast with one action button when the command `com.example.exporter/done` runs.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::intents::{Intent, Submission};
use ilium_plugin_sdk::types::intents::{ToastAction, ToastSpec};
use ilium_plugin_sdk::types::text::{RichText, Tone};

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct Exporter;

impl Handler for Exporter {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::CommandInvoked(invoked) = event else { continue };
            if invoked.command != "com.example.exporter/done" { continue; }
            submissions.push(Submission::new(Intent::Toast(ToastSpec {
                id: Some("export-done".into()),
                tone: Tone::Success,
                title: "Export finished".into(),
                body: Some(RichText::plain("12 notes written.")),
                actions: vec![ToastAction { label: "Open".into(), command: "com.example.exporter/open".into() }],
                duration_ms: None,
            })));
        }
        submissions
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { richText } from '@ilium/plugin-sdk/ui/build';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'command-invoked' || event.val.command !== 'com.example.exporter/done') continue;
      submissions.push({
        intent: {
          tag: 'toast',
          val: {
            id: 'export-done',
            tone: 'success',
            title: 'Export finished',
            body: richText('12 notes written.'),
            actions: [{ label: 'Open', command: 'com.example.exporter/open' }],
            durationMs: undefined,
          },
        },
        correlation: 1n,
        dedupKey: undefined,
      });
    }
    return submissions;
  },
});
```

The toast uses the client's real toast stack, with a countdown bar drawn with `━` and `─`.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Grant missing or declined | `refused(not-permitted)` | Use the plugin's own panel. |
| More than 3 toasts in 10 s | `refused(rate-limited)` | Merge messages or reuse the same `id`. |
| Title above 60 or body above 200 characters | `refused(invalid-argument)` | Shorten the text. |
| More than 2 actions | `refused(invalid-argument)` | Keep two. |
| Action command not declared | `refused(not-found)` | Declare it in the manifest. |
| No client attached | `ok`; the toast is shown on the first client attach only if it was queued less than its duration (default 5 s) earlier; an older toast, `danger` included, is discarded and never shown | Use desktop notices for important news. |

Toast text goes through the same rich text rules as other labels. Reduced motion shows no animation.

### What the user sees

The review dialog lists the grant when a package is installed or first activated. The line states that the plugin shows notices inside Ilium.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.exporter 0.1.0 by Example Ltd                                  │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ [x] Shows short notices inside Ilium <span class="t-dim">………………………………………………………………</span> notify.toast │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests notify.toast, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar.</figcaption>
</figure>

Listing the grants of an installed plugin from the terminal:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.exporter
<span class="t-dim">Grant                             Risk     State</span>
notify.toast                      normal   <span class="t-ok">granted</span>
</pre>

<figcaption>Output of the grant listing for this plugin. A high-risk grant that the user left unchecked shows as declined.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.exporter > Capabilities. A revocation re-activates the plugin with the reduced grant set, and the stubs of the declined imports return `not-permitted`.

### Toasts are never the only signal

A toast expires. An event the user must act on or learn about MUST also be available after the toast is gone.

| ID | Rule | Exact value | Source |
|---|---|---|---|
| CAP-883 | Persistent twin of a toast | A toast for an error or an action-needed event MUST be paired with a persistent text signal (panel row or status line) that remains after the toast duration. A toast with no persistent twin for such an event is refused by the plugin check with `a11y.toast-only` | `src/docs/notifications.md` |

### Related pages

- [notify.desktop](/pages/capability-notify-desktop/).
- [ui.chrome](/pages/capability-ui-chrome/).
- [Notifications](/pages/api-notifications/).
- [Widgets: data display](/pages/widgets-data-display/).
