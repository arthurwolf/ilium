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

### Accessibility

The host draws the toast. The plugin sends text, a tone and optional buttons, and gets back only the outcome of the submission and the invocation of a button's command. The plugin cannot set an accessible label, cannot read the toast back and cannot choose how it is announced. The manual does not document a screen-reader announcement for toasts, so a plugin MUST NOT assume that one is made. Put the meaning in the `title` and `body`, and treat the toast as a short extra notice. The general rules for host widgets, such as the linear screen-reader mode and focus order, are in [Theming and readable output](/pages/accessibility-and-theming/).

**Colour and words.** The `tone` is a colour role, so it is not a signal on its own. The title or body MUST say what happened, for example `Export failed: disk full`, and not only `danger`. The countdown bar is drawn with `━` and `─`, so its filled and empty parts differ in shape, not only in colour. It shows how much time is left, but it carries no meaning of its own.

**Reduced motion.** When the user turns on reduced motion, the countdown is not drawn as a shrinking bar. The client shows the fixed text `5 s left`, updated once per second, as [api-toast](/pages/api-toast/) describes. The plugin does nothing to switch this on and off. A toast with no animation has the same timeout as one with a bar.

**Time to read and act.** The plugin chooses the time with `duration-ms`, from 1,000 to 30,000 ms, and the default is 5,000 ms. The host does not extend a toast for a slow reader, and the manual documents no pause while the pointer or the keyboard is on a toast. A user who reads slowly, or who uses a screen reader, may miss the toast, so:

- For an error or an action that needs the user, set `duration-ms` to the longest value the message needs, up to 30,000 ms, or use a `danger` tone, which stays until the user dismisses it while a client is attached.
- Pair the toast with a persistent text signal, as [CAP-883](#toasts-are-never-the-only-signal) requires. That signal is the only way to guarantee that the user can still read the event later.

**Keyboard.** A toast never takes keyboard focus away from the pane the user types in, so the user is never pulled out of their work. The buttons answer to a click, and the manual documents no keyboard route to them. A plugin MUST NOT put the only way to run an action in a toast button. Put the same command in a panel or a settings row whose controls the user can reach with the keyboard, as [USR-303](/pages/accessibility-and-theming/) requires for host-drawn plugin screens. A toast needs no keyboard dismissal: it leaves by itself when its time runs out, and the plugin can remove it with `dismiss-toast`.

**Motion and attention.** The rate limit of 3 toasts per 10 s keeps a plugin from pulling the user's attention again and again. A plugin that replaces a toast by its `id` restarts the countdown, so it SHOULD replace a toast for progress only at a useful moment, not on every tick.

### Toast timing for slow readers, keyboard focus and the linear reader mode

This section states what the host does with a toast's timer, where a closed toast can still be read, and how a toast reaches a user in the linear reader mode. It overrides the cautious wording above where the two differ: the host does pause and does announce, as described here. The plugin controls none of it.

| ID | Rule | Exact value | Source |
|---|---|---|---|
| USR-088 | Stack limit | At most 3 toasts are alive at once per the rate limit of 3 per 10 s per plugin. A toast beyond the rate is refused with `rate-limited`; a toast that fits the rate but would be a fourth on screen replaces the oldest one | `src/docs/notifications.md` |
| USR-305 | Linear reader mode covers toasts | With `[ui] screen_reader = true` a toast is a logical element of the linear mode, rendered without box-drawing, bar or spinner characters, and inserted at the top of the focus order | `src/docs/notifications.md` |
| CAP-884 | Channels are independent | A toast, its history entry and any desktop notice of the same event stay separate; losing one leaves the text of the others unchanged | `src/docs/notifications.md` |

**Pause on pointer or focus.** The countdown of a toast stops while the pointer is over the toast or while keyboard focus is on it, and it continues with the time that was left when the pointer leaves or focus moves away. A paused toast does not shrink its bar and does not change its `N s left` text. Pausing never extends `duration-ms` beyond what is left, and a toast that is replaced by its `id` restarts its countdown in full, paused or not.

**Reader modes.** When `[ui] screen_reader = true` or reduced motion is on (see [Settings plugin](/pages/core-plugin-settings/)), the minimum time rules change:

| Toast | Default mode | Screen reader or reduced motion |
|---|---|---|
| Plain toast (no buttons) | `duration-ms`, 1,000 to 30,000 ms, default 5,000 ms | At least 10 s: a smaller `duration-ms` is raised to 10 s, a larger one is kept |
| Toast with one or two buttons | `duration-ms` | Stays until the user dismisses it or runs a button |
| `danger` toast | Stays until dismissed while a client is attached | Same |
| Toast queued with no client attached | Discarded after its duration | Same; the raised minimum applies only once a client shows it |

The host applies these values after it accepts the intent. The plugin gets `ok` for the same submission in every mode and cannot ask which mode is on.

**Notification history.** Every shown toast is also written to the notification history, with its plugin, tone, title, body and button labels, so a reader can open it after it closes, expires or is replaced. A toast that was refused (`rate-limited`, `invalid-argument`, `not-permitted`) is not written. The history is the host's list described in [Notifications of the core plugin](/pages/core-plugin-notifications/); the plugin cannot read or edit it. This makes the history, not the toast, the place a slow reader can always reach, and it does not replace the persistent twin that CAP-883 requires for errors.

**Linear reader mode.** With `[ui] screen_reader = true` the client announces a toast as one line, in this exact form, and inserts that element at the top of the focus order so the cursor moves to it:

```
Notice from <plugin>: <title>. <body>
```

`<plugin>` is the plugin's display name, `<title>` and `<body>` are the plain text of the fields (rich text spans are read as their text), and the `. <body>` part is left out when there is no body. Buttons follow as separate lines with the role word `button`. When the toast leaves, focus returns to the element that had it. Because the toast is announced as text, the tone is not spoken, so the title MUST carry the status word, as the colour rule above says.

**What the plugin can observe.** Only the outcome of the `toast` intent (`ok`, or `refused` with `not-permitted`, `rate-limited`, `invalid-argument` or `not-found`) and the invocation of a button's command. The plugin cannot see whether the toast was paused, whether it was announced, how long it stayed, whether it was dropped as the oldest of a full stack, or whether it is in the history. A plugin MUST therefore not time its own follow-up on the assumption that the toast is gone after `duration-ms`.

**Failure modes.**

| Situation | What happens |
|---|---|
| A fourth toast arrives while 3 are alive | The oldest toast is dropped from the screen at once, even if it was paused or had buttons; its history entry stays |
| Pointer rests on a toast for a long time | The toast stays; the next toasts queue behind the stack limit and replace the oldest one only after it leaves or is replaced |
| `screen_reader = true` and a toast with buttons that the user ignores | It stays until dismissed; the persistent twin of CAP-883 still has to exist |
| Two toasts share an `id` | The second replaces the first in place and restarts the countdown |

### Related pages

- [notify.desktop](/pages/capability-notify-desktop/).
- [ui.chrome](/pages/capability-ui-chrome/).
- [Notifications](/pages/api-notifications/).
- [Widgets: data display](/pages/widgets-data-display/).
