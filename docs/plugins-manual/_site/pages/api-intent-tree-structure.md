# intents: tree structure and restructure

## Tree structure intents

The session tree is made of groups, folders, projects, splits and panes. A plugin can build and reorganise it with intents: create the container kinds, bookmark and expand nodes, and apply a whole reorganisation as one atomic, undoable batch. The tree model, the layout solver and the drawing of splits stay in the kernel, so a plugin only asks and the kernel checks.

**You need:** `tree.restructure` (high risk) for `create-group`, `create-folder`, `create-project`, `create-split`, `restructure` and `revert-restructure`. `tree.write` for `set-bookmarked` and `set-expanded` on nodes. `tree.read` to learn node ids and to receive `tree-changed`. A plugin that expands its own virtual rows needs the `virtual_rows` entry in its manifest instead. See [tree](/pages/api-tree/) for the read side, [tree.write](/pages/capability-tree-write/) and [tree.read](/pages/capability-tree-read/) for the grants, and [intents](/pages/api-intents/) for batches.

```json
"capabilities": { "server": ["tree.read", "tree.write", "tree.restructure"] }
```

### Records

```wit
variant placement { before, after, inside-first, inside-last }           // as for move-node

record create-group-spec   { parent: node-id, title: string, placement: placement, children: list<node-id> }
record create-folder-spec  { parent: node-id, title: string, path: option<fs-path> }
record create-project-spec { title: string, root: fs-path }
record create-split-spec   { panes: list<node-id>, direction: split-direction }   // 2 to 4 panes
record set-bookmarked-spec { node: node-id, bookmarked: bool }
variant expand-target { node(node-id), virtual-row(string) }
record set-expanded-spec   { target: expand-target, expanded: bool }
record restructure-spec    { operations: list<restructure-op>, label: string }    // at most 256 operations
// restructure-op is a variant with the cases move, create-group, ungroup and rename
// revert-restructure carries the undo token (u64)
```

### Intents

| Intent | Effect | Outcome |
|---|---|---|
| `create-group` | Creates a group under `parent` at `placement` and moves `children` into it. | `ok-node` with the new group |
| `create-folder` | Creates a folder node, optionally bound to a directory `path`. | `ok-node` |
| `create-project` | Creates a project node rooted at `root`. | `ok-node` |
| `create-split` | Joins 2 to 4 panes into one split container. | `ok-node` with the split container |
| `set-bookmarked` | Sets or clears the bookmark star on a node. | `ok` |
| `set-expanded` | Expands or collapses a node, or one of the plugin's own virtual rows. | `ok` |
| `restructure` | Applies up to 256 operations (move, create-group, ungroup, rename) as one change with a `label` for the undo. | `ok-node` carrying an undo token node id (0 when nothing changed) |
| `revert-restructure` | Undoes the restructure with that token. | `ok`; `refused(conflict)` when a later change touched the same nodes |

`restructure` is all or nothing. One failing operation refuses the whole intent with `refused(conflict)` and the index of that operation in the message, and the tree is unchanged. An undo token does not survive a server restart.

<figure>
<svg viewBox="0 0 420 150" role="img" aria-label="A restructure batch is applied atomically and can be reverted">
  <defs>
    <marker id="treestruct-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M 0 0 L 10 5 L 0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="6" y="50" width="110" height="46" rx="6" fill="none" stroke="currentColor"/>
  <text x="61" y="70" font-size="11" text-anchor="middle" fill="currentColor">restructure</text>
  <text x="61" y="85" font-size="10" text-anchor="middle" fill="currentColor">N operations</text>
  <rect x="160" y="14" width="120" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="220" y="38" font-size="11" text-anchor="middle" fill="currentColor">all apply: ok-node(token)</text>
  <rect x="160" y="92" width="120" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="220" y="108" font-size="10" text-anchor="middle" fill="currentColor">one fails: refused(conflict)</text>
  <text x="220" y="122" font-size="10" text-anchor="middle" fill="currentColor">index in message, no change</text>
  <rect x="320" y="14" width="94" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="367" y="38" font-size="10" text-anchor="middle" fill="currentColor">revert-restructure</text>
  <line x1="116" y1="64" x2="158" y2="38" stroke="currentColor" marker-end="url(#treestruct-arrow)"/>
  <line x1="116" y1="82" x2="158" y2="108" stroke="currentColor" marker-end="url(#treestruct-arrow)"/>
  <line x1="280" y1="34" x2="318" y2="34" stroke="currentColor" marker-end="url(#treestruct-arrow)"/>
</svg>
<figcaption>The token returned by a successful restructure is the only handle for the undo.</figcaption>
</figure>

### Example: group two panes and bookmark the group

The first submission creates the group and its result `ok-node` gives the new node id. A later batch uses that id, because a node created in a batch is not yet known to the plugin when it builds the batch.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{CreateGroupSpec, SetBookmarkedSpec};
use ilium_plugin_sdk::types::tree::Placement;

fn group_panes(parent: NodeId, panes: Vec<NodeId>) -> Submission {
    Submission::keyed(
        Intent::CreateGroup(CreateGroupSpec {
            parent,
            title: "Deploy".into(),
            placement: Placement::InsideLast,
            children: panes,
        }),
        format!("group-deploy-{parent}"),
    )
}

// in on_events: Event::RequestCompleted with Outcome::OkNode(group)
fn bookmark(group: NodeId) -> Submission {
    Submission::new(Intent::SetBookmarked(SetBookmarkedSpec { node: group, bookmarked: true }))
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk/types';

const groupPanes = (parent: bigint, panes: bigint[]): Submission => ({
  intent: { tag: 'create-group', val: { parent, title: 'Deploy', placement: 'inside-last', children: panes } },
  correlation: 1n,
  dedupKey: `group-deploy-${parent}`,
});

// in onEvents: event.tag === 'request-completed' && outcome.tag === 'ok-node'
const bookmark = (group: bigint): Submission => ({
  intent: { tag: 'set-bookmarked', val: { node: group, bookmarked: true } },
  correlation: 2n,
  dedupKey: undefined,
});
```

### Example: a split, a project and an expanded virtual row

**Rust**

```rust
let split = Intent::CreateSplit(CreateSplitSpec { panes: vec![left, right], direction: SplitDirection::Vertical });
let project = Intent::CreateProject(CreateProjectSpec { title: "api".into(), root: "/work/api".into() });
let expand = Intent::SetExpanded(SetExpandedSpec { target: ExpandTarget::VirtualRow("builds".into()), expanded: true });
```

**TypeScript**

```ts
const split = { tag: 'create-split', val: { panes: [left, right], direction: 'vertical' } } as const;
const project = { tag: 'create-project', val: { title: 'api', root: '/work/api' } } as const;
const expand = { tag: 'set-expanded', val: { target: { tag: 'virtual-row', val: 'builds' }, expanded: true } } as const;
```

### Example: apply a restructure and keep the undo token

The operations list is built by the plugin's own code (move, create-group, ungroup and rename operations). The token arrives as the node id of `ok-node`; 0 means that nothing changed and there is nothing to undo.

**Rust**

```rust
let submission = Submission::new(Intent::Restructure(RestructureSpec {
    operations: self.plan_operations(),          // at most 256
    label: "Group panes by project".into(),
}));

// later, on Event::RequestCompleted
if let Outcome::OkNode(token) = result.outcome {
    if token != 0 { self.undo_token = Some(token); }
}
// to undo: Intent::RevertRestructure(token)
```

**TypeScript**

```ts
const submission: Submission = {
  intent: { tag: 'restructure', val: { operations: planOperations(), label: 'Group panes by project' } },
  correlation: 5n,
  dedupKey: undefined,
};
// on request-completed: if (outcome.tag === 'ok-node' && outcome.val !== 0n) undoToken = outcome.val;
// to undo: { tag: 'revert-restructure', val: undoToken }
```

### Errors and limits

| Condition | Result |
|---|---|
| Missing `tree.restructure` or `tree.write` | batch refused, `not-permitted` |
| More than 256 operations, a split with fewer than 2 or more than 4 panes | `refused(invalid-argument)` |
| One operation of a restructure fails | whole intent `refused(conflict)`, index in the message |
| Revert after a later change touched the same nodes | `refused(conflict)` |
| Token from before a server restart | `refused(not-found)` or `refused(conflict)`; tokens do not survive restarts |
| A node that does not exist or a locked node that must move | `refused(not-found)`, `refused(conflict)` |

### What the user sees

The tree changes at once. A group shows as an expanded node, a split as the split glyph of the client, a bookmark as the star. The restructure appears as one entry in the kernel's undo, labelled with `label`.

<pre class="term"><span class="t-dim">▼ 📂 Ilium</span>
<span class="t-dim">› ▼ 📂 Deploy</span>
<span class="t-hi">› ›   🖥️ ★ api-deploy</span>
<span class="t-dim">› ›   🖥️    web-deploy</span>
<span class="t-dim">› ▶ ▥ left | right</span>
</pre>

Illustrative, not generated: composed from the glyph inventory of the client tree (expand arrows, folder, bookmark star, split glyph, indent marks per depth); the split row label is invented.

### See also

- [Tree](/pages/api-tree/), [intents](/pages/api-intents/) and [events](/pages/api-events/) (`tree-changed`, `node-removed`).
- [tree.write](/pages/capability-tree-write/), [tree.read](/pages/capability-tree-read/) and [tree.decorate](/pages/capability-tree-decorate/).
- [Chrome intents](/pages/api-intent-chrome/) for titles, icons and attention marks.
