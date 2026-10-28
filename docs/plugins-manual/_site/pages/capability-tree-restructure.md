# Capability: tree.restructure

## tree.restructure

The `tree.restructure` capability lets a plugin create groups, folders, projects and splits, and reorganise many nodes in one atomic step that the user can revert. It is high risk (**H**) because it changes how the user's whole workspace is arranged.

**You will need:**

- A `tree.restructure` grant (explicit, high risk).
- Node ids from [tree.read](/pages/capability-tree-read/).
- A label for the undo entry: `restructure` takes a human-readable `label`.

<figure>
<svg viewBox="0 0 420 252" role="img" aria-label="A restructure intent is validated as a whole and applied as one undoable step">
  <defs><marker id="arr" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="27" text-anchor="middle" fill="currentColor" font-size="12">Plugin submits restructure with up to 256 operations</text>
  <text x="210" y="43" text-anchor="middle" fill="currentColor" font-size="10">move, create-group, ungroup, rename</text>
  <line x1="210" y1="52" x2="210" y2="72" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="72" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="91" text-anchor="middle" fill="currentColor" font-size="12">Host validates every operation first</text>
  <text x="210" y="107" text-anchor="middle" fill="currentColor" font-size="10">all or nothing</text>
  <line x1="210" y1="116" x2="210" y2="136" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="136" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="155" text-anchor="middle" fill="currentColor" font-size="12">Host applies them as one undoable step</text>
  <text x="210" y="171" text-anchor="middle" fill="currentColor" font-size="10">an undo token is returned</text>
  <line x1="210" y1="180" x2="210" y2="200" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="110" y="224" text-anchor="middle" fill="currentColor" font-size="11">ok-node with undo token</text>
  <rect x="220" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="310" y="224" text-anchor="middle" fill="currentColor" font-size="11">refused(conflict) with index</text>
</svg>
<figcaption>If one operation fails, none is applied. The refusal names the index of the failing operation.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `tree.restructure` | yes | the only form |
| `tree.restructure:groups` | no | no argument is accepted |
| `tree.reorganize` | no | unknown grant name |

High risk. Even a bundled T1 plugin needs an explicit grant. Simple renames, locks and moves of single nodes need only [tree.write](/pages/capability-tree-write/).

### What it unlocks

| Name | Kind | Meaning |
|---|---|---|
| `create-group` | intent | `parent`, `title`, `placement`, `children`. Outcome `ok-node`. |
| `create-folder` | intent | `parent`, `title`, optional `path`. Outcome `ok-node`. |
| `create-project` | intent | `title`, `root`. Outcome `ok-node`. |
| `create-split` | intent | `panes` (2 to 4), `direction`. Outcome `ok-node` for the split container. |
| `restructure` | intent | `operations` (at most 256), `label`. Outcome `ok-node` carrying an undo token (0 when none). |
| `revert-restructure` | intent | `token`. `refused(conflict)` when a later change touched the same nodes. |

See [Tree](/pages/api-tree/) and [Intents](/pages/api-intents/).

### Manifest

The grant goes into `manifest.json` under `capabilities`, in the list of the side that uses it (server or client).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.organizer",
  "capabilities": {
    "server": [
      "tree.read",
      "tree.restructure"
    ]
  }
}
```

### Worked example

This plugin groups two panes into a new group called Review when the command `com.example.organizer/group` runs, and keeps the undo token for a later revert.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::intents::{Intent, Submission};
use ilium_plugin_sdk::types::intents::CreateGroupSpec;
use ilium_plugin_sdk::types::tree::Placement;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Organizer;

impl Handler for Organizer {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::CommandInvoked(invoked) = event else { continue };
            if invoked.command != "com.example.organizer/group" { continue; }
            let Some(parent) = invoked.parent else { continue };
            submissions.push(Submission::new(Intent::CreateGroup(CreateGroupSpec {
                parent,
                title: "Review".into(),
                placement: Placement::InsideLast,
                children: invoked.selection.clone(),
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
export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'command-invoked') continue;
      if (event.val.command !== 'com.example.organizer/group' || event.val.parent === undefined) continue;
      submissions.push({
        intent: {
          tag: 'create-group',
          val: { parent: event.val.parent, title: 'Review', placement: 'inside-last', children: event.val.selection },
        },
        correlation: 1n,
        dedupKey: undefined,
      });
    }
    return submissions;
  },
});
```

The outcome `ok-node` carries the new node id. For `restructure` it carries the undo token as a node id, which a later `revert-restructure` takes.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Grant missing or declined | `refused(not-permitted)` | Fall back to single moves with `tree.write` if granted. |
| One of many operations fails | `refused(conflict)` with the operation index | Fix that operation and resubmit the batch. |
| More than 256 operations | `refused(invalid-argument)` | Split into several restructures. |
| Revert after a later change | `refused(conflict)` | Tell the user that the layout changed since. |
| `create-split` with a pane count outside 2 to 4 | `refused(invalid-argument)` | Pass 2, 3 or 4 panes. |
| `revert-restructure` with an unknown or expired undo token | `refused(not-found)` | Treat the layout as not revertable. |
| Locked node in the batch | Applied: the lock flag only blocks `close-node` in `tree.write`, and a restructure never closes a node | None needed. |
| Unknown node | `refused(not-found)` | Re-read the tree. |

The attention policy and the user's manual order are kernel concerns. A restructure never changes a title that the user set by hand.

### What the user sees

The review dialog lists the grant when a package is installed or first activated. The line is high risk and starts unchecked.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.organizer 0.1.0 by Example Ltd                                 │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ <span class="t-warn">! [ ] Creates groups and reorganises the tree</span> <span class="t-dim">……………………………</span> <span class="t-warn">tree.restructure</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests tree.restructure, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar.</figcaption>
</figure>

Listing the grants of an installed plugin from the terminal:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.organizer
<span class="t-dim">Grant                             Risk     State</span>
tree.restructure                  high     <span class="t-warn">declined</span>
</pre>

<figcaption>Output of the grant listing for this plugin. A high-risk grant that the user left unchecked shows as declined.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.organizer > Capabilities. A revocation re-activates the plugin with the reduced grant set, and the stubs of the declined imports return `not-permitted`.

### Related pages

- [tree.write](/pages/capability-tree-write/).
- [tree.read](/pages/capability-tree-read/).
- [tree.attention](/pages/capability-tree-attention/).
- [Tree](/pages/api-tree/).
