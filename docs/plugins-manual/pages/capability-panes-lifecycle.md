---
layout: page
title: "Capability: panes.lifecycle"
permalink: /pages/capability-panes-lifecycle/
---

## panes.lifecycle

The `panes.lifecycle` capability lets a plugin control the process inside a pane: freeze and unfreeze it, replace its command, terminate it and resize the pane. It also delivers `pane-lifecycle-changed` events. Because a plugin that can stop or replace a process can destroy the user's work, the grant is high risk (**H**) and is server-side only.

**You will need:**

- A `panes.lifecycle` grant on the server side (explicit, high risk).
- A pane id from [panes.read](/pages/capability-panes-read/).
- A way to show the user that a pane is frozen: the host draws the frozen mark itself.

<figure>
<svg viewBox="0 0 420 252" role="img" aria-label="Lifecycle intents pass checks and act on the pane process">
  <defs><marker id="arr" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="27" text-anchor="middle" fill="currentColor" font-size="12">Plugin submits freeze-pane, replace-pane-command, terminate-pane-process or resize-pane</text>
  <text x="210" y="43" text-anchor="middle" fill="currentColor" font-size="10">one intent per action</text>
  <line x1="210" y1="52" x2="210" y2="72" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="72" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="91" text-anchor="middle" fill="currentColor" font-size="12">Host checks the grant, the pane and the platform</text>
  <text x="210" y="107" text-anchor="middle" fill="currentColor" font-size="10">freeze is refused on Windows</text>
  <line x1="210" y1="116" x2="210" y2="136" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="136" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="155" text-anchor="middle" fill="currentColor" font-size="12">Host acts on the pane process</text>
  <text x="210" y="171" text-anchor="middle" fill="currentColor" font-size="10">durable resume bindings stay with the kernel</text>
  <line x1="210" y1="180" x2="210" y2="200" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="110" y="224" text-anchor="middle" fill="currentColor" font-size="11">ok</text>
  <rect x="220" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="310" y="224" text-anchor="middle" fill="currentColor" font-size="11">refused(conflict, unsupported, not-permitted)</text>
</svg>
<figcaption>Each lifecycle action is one intent with its own outcome. The kernel keeps ownership of the durable session resume binding.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `panes.lifecycle` | yes | the only form |
| `panes.lifecycle:freeze` | no | no argument; all five actions share one grant |
| `panes.control` | no | unknown grant name |

High risk and server-side only; the client side cannot request it. A bundled plugin still needs an explicit grant.

### What it unlocks

| Name | Kind | Meaning |
|---|---|---|
| `freeze-pane` | intent | `pane`. Stops the process (SIGSTOP on Unix). `refused(unsupported)` on Windows. |
| `unfreeze-pane` | intent | `pane`. `refused(conflict)` when the pane is not frozen. |
| `replace-pane-command` | intent | `pane`, `command: list<string>`, `cwd`, `keep-scrollback: bool`. `ok` once the new process runs. |
| `terminate-pane-process` | intent | `pane`, `grace-ms` at most 10,000. `ok` when the process has exited; the pane stays open with its exit status. |
| `resize-pane` | intent | `pane`, optional `columns` and `rows`. `refused(conflict)` when the layout cannot give the size. |
| `pane-lifecycle-changed` | event | Reports freeze, exit and replace transitions. |

Intents are described on [Intents](/pages/api-intents/) and pane state on [Panes](/pages/api-panes/).

### Manifest

The grant goes into `manifest.json` under `capabilities`, in the list of the side that uses it (server only).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.panecontrol",
  "capabilities": {
    "server": [
      "panes.read",
      "panes.lifecycle"
    ]
  }
}
```

### Worked example

This plugin freezes a pane when the command `com.example.panecontrol/freeze` runs. The dedup key stops a double click from submitting twice.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::intents::{Intent, Submission};
use ilium_plugin_sdk::types::intents::FreezePaneSpec;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct PaneControl;

impl Handler for PaneControl {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::CommandInvoked(invoked) = event else { continue };
            if invoked.command != "com.example.panecontrol/freeze" { continue; }
            let Some(pane) = invoked.context.pane else { continue };
            submissions.push(Submission::keyed(
                Intent::FreezePane(FreezePaneSpec { pane }),
                format!("freeze-{pane}"),
            ));
        }
        submissions
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'command-invoked') continue;
      if (event.val.command !== 'com.example.panecontrol/freeze' || event.val.context.pane === undefined) continue;
      const pane = event.val.context.pane;
      submissions.push({
        intent: { tag: 'freeze-pane', val: { pane } },
        correlation: 1n,
        dedupKey: `freeze-${pane}`,
      });
    }
    return submissions;
  },
});
```

Pair every freeze with a visible way to unfreeze. A frozen pane shows the frozen mark in the tree and the pane frame, and the user can unfreeze it by hand.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Grant missing or declined | `refused(not-permitted)` | Hide the feature. |
| Freeze on Windows | `refused(unsupported)` | Check the platform before offering the action. |
| Unfreeze of a running pane | `refused(conflict)` | Ignore. |
| Resize that the layout cannot give | `refused(conflict)`, the limiting neighbour in the message | Ask for a smaller size. |
| `grace-ms` above 10,000 | `refused(invalid-argument)` | Use a shorter grace period. |
| Pane not found | `refused(not-found)` | Drop the pane id. |

A process frozen when the plugin stops stays frozen: the pane keeps the frozen mark so the user can resume it. Plugins SHOULD unfreeze on deactivation. Observable criterion: a test that freezes a pane, then stops the plugin, asserts that the pane state is still `frozen` and shows the frozen mark; a plugin that unfroze in `deactivate` leaves the pane `running`.

### What the user sees

The review dialog lists the grant when a package is installed or first activated. The line is high risk and starts unchecked.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.panecontrol 0.1.0 by Example Ltd                               │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ <span class="t-warn">! [ ] Freezes, replaces and ends pane processes</span> <span class="t-dim">…………………………</span> <span class="t-warn">panes.lifecycle</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests panes.lifecycle, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar.</figcaption>
</figure>

Listing the grants of an installed plugin from the terminal:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.panecontrol
<span class="t-dim">Grant                             Risk     State</span>
panes.lifecycle                   high     <span class="t-warn">declined</span>
</pre>

<figcaption>Output of the grant listing for this plugin. A high-risk grant that the user left unchecked shows as declined.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.panecontrol > Capabilities. A revocation re-activates the plugin with the reduced grant set, and the stubs of the declined imports return `not-permitted`.

### Related pages

- [panes.signal](/pages/capability-panes-signal/) for sending a signal.
- [panes.create](/pages/capability-panes-create/).
- [Panes](/pages/api-panes/).
- [Intents](/pages/api-intents/).

### Security

**[SEC-041]** A `panes.lifecycle` restart with a new command uses the same program grant and the same confirmation as starting that program ([Sandbox reference](/pages/reference-sandbox/)). Separately, closing or moving a pane that another plugin created is a tree intent checked against `tree.write`, and signalling its process needs `panes.signal` (see [panes.signal](/pages/capability-panes-signal/)); each refusal is `not-permitted`. See [Sandbox reference](/pages/reference-sandbox/).
