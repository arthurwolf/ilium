---
layout: page
title: "Capability: ui.chrome"
permalink: /pages/capability-ui-chrome/
---

## ui.chrome

The `ui.chrome` capability lets a plugin put its own controls into the frame of the client: a button in the tree toolbar, an icon in the hover strip of tree rows, a segment of the status bar, and a slot in pane frames (header icon, header text, footer row or banner). The plugin declares each place in its manifest and fills it at run time with intents.

The grant covers decoration of the chrome only. Whole panels need [ui.panels](/pages/capability-ui-panels/), and tree decorations need [tree.decorate](/pages/capability-tree-decorate/).

**You will need:**

- A `ui.chrome` grant. No argument.
- The matching `contributes` keys: `tree_toolbar` (cap 4), `row_actions` (cap 4), `status_segments` (cap 2) and `pane_frame_slots` (cap 4).
- Commands for the buttons to run: see [Commands](/pages/api-commands/).

<figure>
<svg viewBox="0 0 420 252" role="img" aria-label="Chrome contributions are declared in the manifest and filled by intents">
  <defs><marker id="arr" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="27" text-anchor="middle" fill="currentColor" font-size="12">Manifest declares a status segment, toolbar button, row action or frame slot</text>
  <text x="210" y="43" text-anchor="middle" fill="currentColor" font-size="10">above the cap: manifest.contribution-limit</text>
  <line x1="210" y1="52" x2="210" y2="72" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="72" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="91" text-anchor="middle" fill="currentColor" font-size="12">Plugin submits set-status-segment or set-frame-slot</text>
  <text x="210" y="107" text-anchor="middle" fill="currentColor" font-size="10">at most 10 per second per slot</text>
  <line x1="210" y1="116" x2="210" y2="136" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="136" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="155" text-anchor="middle" fill="currentColor" font-size="12">Client draws the content in the declared place</text>
  <text x="210" y="171" text-anchor="middle" fill="currentColor" font-size="10">clicks arrive as command-invoked</text>
  <line x1="210" y1="180" x2="210" y2="200" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="110" y="224" text-anchor="middle" fill="currentColor" font-size="11">ok, drawn in the chrome</text>
  <rect x="220" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="310" y="224" text-anchor="middle" fill="currentColor" font-size="11">refused(invalid-argument, rate-limited)</text>
</svg>
<figcaption>The place is declared once in the manifest. The content changes at run time through intents. A click on a toolbar button or row action runs the declared command.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `ui.chrome` | yes | the only form |
| `ui.chrome:status` | no | no argument; all four places share one grant |
| `ui.frame` | no | unknown grant name |

Normal risk. The host owns the layout: content is rich text of bounded length, a segment is at most 40 cells wide, and a plugin cannot hide or restyle kernel chrome.

### What it unlocks

| Name | Kind | Meaning |
|---|---|---|
| `tree_toolbar` | contribution | `{ id, icon, label, command, caption?, popup_view?, when? }`. `label` at most 24 characters. |
| `row_actions` | contribution | `{ id, icon, label, command, applies_to, when?, order? }`. The click arrives as `command-invoked` with origin `row-action`. |
| `status_segments` | contribution | `{ id, priority, max_width? }`. `priority` 0 to 100, `max_width` 4 to 40 cells. |
| `pane_frame_slots` | contribution | `{ id, placement, applies_to }`. Placement `header-icon`, `header-text`, `footer-row` or `banner`. |
| `set-status-segment`, `clear-status-segment` | intents | Fill or clear an own segment: `segment` (the own `status_segments` id), `content: rich-text`, `tooltip`, `tone`. `clear-status-segment` takes `segment` only. |
| `set-frame-slot`, `clear-frame-slot` | intents | Fill or clear an own slot for one pane: `pane`, `slot` (the own `pane_frame_slots` id), `content: rich-text`, `icon`, `tooltip`. `clear-frame-slot` takes `pane` and `slot`. |

Contribution keys are in [Manifest reference](/pages/reference-manifest/); the intents are on [Intents](/pages/api-intents/) and [UI](/pages/api-ui/).

### Manifest

The grant goes into `manifest.json` under `capabilities`, in the list of the side that uses it (server or client).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.quota",
  "capabilities": {
    "client": [
      "ui.chrome"
    ]
  }
}
```

### Worked example

This plugin declares a status bar segment `quota` in its manifest (`"status_segments": [{ "id": "quota", "priority": 60 }]`) and fills it with a warning text when the command `com.example.quota/update` runs.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::intents::{Intent, Submission};
use ilium_plugin_sdk::types::intents::SetStatusSegmentSpec;
use ilium_plugin_sdk::types::text::{RichText, Tone};

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct Quota;

impl Handler for Quota {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::CommandInvoked(invoked) = event else { continue };
            if invoked.command != "com.example.quota/update" { continue; }
            submissions.push(Submission::new(Intent::SetStatusSegment(SetStatusSegmentSpec {
                segment: "quota".into(),
                content: RichText::plain("Quota 82%"),
                tooltip: None,
                tone: Some(Tone::Warning),
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
      if (event.tag !== 'command-invoked' || event.val.command !== 'com.example.quota/update') continue;
      submissions.push({
        intent: {
          tag: 'set-status-segment',
          val: { segment: 'quota', content: richText('Quota 82%'), tooltip: undefined, tone: 'warning' },
        },
        correlation: 1n,
        dedupKey: undefined,
      });
    }
    return submissions;
  },
});
```

The client draws the segment with the real status bar separators, and drops segments when the terminal is narrow, in ascending `priority` order (lowest first; segments of equal priority drop the one whose plugin id sorts last, then the later segment id). A dropped segment is hidden whole, never truncated below its `max_width` floor of 4 cells. Test: with segments of priority 10, 50 and 90 and room for two, only the priority 10 segment is hidden.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Grant missing or declined | `refused(not-permitted)`; declared contributions are not drawn | Keep the information in an own panel. |
| Segment or slot id not declared | `refused(not-found)` | Declare it in the manifest. |
| Pane does not match `applies_to` | `refused(invalid-argument)` | Check the pane kind first. |
| More than 10 updates per second to one segment or slot | `refused(rate-limited)` | Coalesce updates. |
| Contribution cap exceeded | install refused with `manifest.contribution-limit` | Declare fewer places. |
| Unknown `when` key | install refused with `manifest.when-key` | Use a key from the context grammar. |

When a plugin is disabled, its segments, slots, buttons and row actions disappear at once.

### What the user sees

The review dialog lists the grant when a package is installed or first activated. The line states that the plugin adds controls to the client frame.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.quota 0.1.0 by Example Ltd                                     │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ [x] Adds buttons and status text to the frame <span class="t-dim">………………………………………………</span> ui.chrome │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests ui.chrome, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar.</figcaption>
</figure>

Listing the grants of an installed plugin from the terminal:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.quota
<span class="t-dim">Grant                             Risk     State</span>
ui.chrome                         normal   <span class="t-ok">granted</span>
</pre>

<figcaption>Output of the grant listing for this plugin. A high-risk grant that the user left unchecked shows as declined.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.quota > Capabilities. A revocation re-activates the plugin with the reduced grant set, and the stubs of the declined imports return `not-permitted`.

### Related pages

- [ui.panels](/pages/capability-ui-panels/).
- [tree.decorate](/pages/capability-tree-decorate/).
- [UI](/pages/api-ui/).
- [Manifest reference](/pages/reference-manifest/).
