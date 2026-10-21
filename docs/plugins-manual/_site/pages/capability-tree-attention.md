# Capability: tree.attention

## tree.attention

The `tree.attention` capability lets a plugin mark a node as needing the user's attention and acknowledge that mark, and receive `attention-changed` events. The mark appears in the tree and in the Attention display mode. The policy for the done prefix and the sound stays with the kernel; the plugin only supplies the node and a short reason.

**You will need:**

- A `tree.attention` grant. No argument.
- A node id from [tree.read](/pages/capability-tree-read/) or an event.

<figure>
<svg viewBox="0 0 420 252" role="img" aria-label="mark-attention shows a mark in the tree and ack-attention clears it">
  <defs><marker id="arr" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="27" text-anchor="middle" fill="currentColor" font-size="12">Plugin submits mark-attention(node, reason)</text>
  <text x="210" y="43" text-anchor="middle" fill="currentColor" font-size="10">reason at most 80 characters</text>
  <line x1="210" y1="52" x2="210" y2="72" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="72" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="91" text-anchor="middle" fill="currentColor" font-size="12">Host checks the grant and the node</text>
  <text x="210" y="107" text-anchor="middle" fill="currentColor" font-size="10">the kernel owns sound and prefix</text>
  <line x1="210" y1="116" x2="210" y2="136" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="136" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="155" text-anchor="middle" fill="currentColor" font-size="12">Tree shows the mark; attention-changed is sent</text>
  <text x="210" y="171" text-anchor="middle" fill="currentColor" font-size="10">ack-attention clears it</text>
  <line x1="210" y1="180" x2="210" y2="200" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="110" y="224" text-anchor="middle" fill="currentColor" font-size="11">ok</text>
  <rect x="220" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="310" y="224" text-anchor="middle" fill="currentColor" font-size="11">refused(not-found, invalid-argument)</text>
</svg>
<figcaption>The plugin supplies the node and the reason. The kernel decides how the mark looks and whether a sound plays.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `tree.attention` | yes | the only form |
| `tree.attention:pane` | no | no argument |
| `attention` | no | unknown grant name |

Normal risk. A plugin can only add a mark and a reason of at most 80 characters, so the grant cannot hide or change anything else in the tree.

### What it unlocks

| Name | Kind | Meaning |
|---|---|---|
| `mark-attention` | intent | `node`, `reason` (at most 80 characters). Outcome `ok`. |
| `ack-attention` | intent | `node`. Outcome `ok`. |
| `attention-changed` | event | Reports a node whose attention state changed, from any source. |

See [Tree](/pages/api-tree/) and [Events](/pages/api-events/).

### Manifest

The grant goes into `manifest.json` under `capabilities`, in the list of the side that uses it (server or client).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.attention",
  "capabilities": {
    "server": [
      "tree.read",
      "tree.attention"
    ]
  }
}
```

### Worked example

This plugin marks the pane of a finished build and clears the mark when the user acknowledges it elsewhere.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::intents::{Intent, Submission};
use ilium_plugin_sdk::types::intents::MarkAttentionSpec;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Attention;

impl Handler for Attention {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::CommandInvoked(invoked) if invoked.command == "com.example.attention/build-done" => {
                    if let Some(node) = invoked.pane {
                        submissions.push(Submission::new(Intent::MarkAttention(MarkAttentionSpec {
                            node, reason: "Build finished".into(),
                        })));
                    }
                }
                Event::AttentionChanged(changed) if !changed.marked => {
                    log::log(Level::Debug, &format!("node {} acknowledged", changed.node));
                }
                _ => {}
            }
        }
        submissions
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === 'com.example.attention/build-done' && event.val.pane !== undefined) {
        submissions.push({
          intent: { tag: 'mark-attention', val: { node: event.val.pane, reason: 'Build finished' } },
          correlation: 1n,
          dedupKey: undefined,
        });
      } else if (event.tag === 'attention-changed' && !event.val.isMarked) {
        log('debug', `node ${event.val.node} acknowledged`);
      }
    }
    return submissions;
  },
});
```

A plugin never needs to clear a mark that the user acknowledged: the kernel clears it and sends `attention-changed`.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Grant missing or declined | `refused(not-permitted)` | Show the status in the plugin's own panel. |
| `reason` above 80 characters | `refused(invalid-argument)` | Shorten the reason. |
| Node not found | `refused(not-found)` | Re-read the tree. |
| Marking too often | `refused(rate-limited)` | Coalesce marks per node. |
| Ack of an unmarked node | `ok` (no change) | Nothing. |
| Grant revoked later | later submissions `refused(not-permitted)`; existing marks stay | Nothing. |

Marks set by a plugin are removed when the plugin is disabled, so a disabled plugin cannot leave stale marks.

### What the user sees

The review dialog lists the grant when a package is installed or first activated. The line is a normal-risk grant and starts checked.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.attention 0.1.0 by Example Ltd                                 │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Marks tree nodes that need attention <span class="t-dim">………………………………………………</span> tree.attention │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests tree.attention, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar.</figcaption>
</figure>

Listing the grants of an installed plugin from the terminal:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.attention
<span class="t-dim">Grant                             Risk     State</span>
tree.attention                    normal   <span class="t-ok">granted</span>
</pre>

<figcaption>Output of the grant listing for this plugin. A high-risk grant that the user left unchecked shows as declined.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.attention > Capabilities. A revocation re-activates the plugin with the reduced grant set, and the stubs of the declined imports return `not-permitted`.

### Related pages

- [tree.decorate](/pages/capability-tree-decorate/).
- [tree.read](/pages/capability-tree-read/).
- [notify.toast](/pages/capability-notify-toast/).
- [Tree](/pages/api-tree/).
