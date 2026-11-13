# tree

## tree

The `tree` interface reads the project tree: the containers, folders and panes, with their titles, parents, working directories, agent state and lock flags. Every node has a `node-id`, a number that stays the same for the life of the node, including across moves and server restarts. A plugin can also keep its own small byte map on any node, for example a bookmark flag or the id of a note. No other plugin can read that map.

All reads are synchronous. They answer at once from the copy of the tree that the plugin's side holds, so a plugin does not await them. Changes to the tree go through intents, not through this interface: see [intents](/pages/api-intents/).

**You will need:**

- `tree.read` for `get`, `children`, `roots` and `find`, and for the `tree-changed` and `node-removed` events.
- `tree.metadata` for `metadata` and `set-metadata`.
- `agent.read` if the `agent` field of a node must be filled in. Without it, `agent` is always none for this plugin.
- Language: Rust with `ilium-plugin-sdk`, or TypeScript with `@ilium/plugin-sdk`. Both are available on the server and client sides. The two sides of one plugin share one metadata map per node.
- Writes to the tree need `tree.write` (rename, move, lock, close) or `tree.decorate` (badges and rows). Those are intents, not calls on this interface.

### Surface

Functions, all in the `tree` interface. Every function is synchronous and returns no request id.

| Rust (`host::tree`) | TypeScript (`host/tree`) | Grant | Returns | Meaning |
|---|---|---|---|---|
| `get(node)` | `get(node)` | `tree.read` | `Option<NodeView>`, `NodeView` or `undefined` | The view of one node, or none when it does not exist. |
| `children(node)` | `children(node)` | `tree.read` | `Vec<NodeId>`, `bigint[]` | The child ids in tree order. Empty for a pane or an unknown node. |
| `roots()` | `roots()` | `tree.read` | `Vec<NodeId>`, `bigint[]` | The top-level node ids in tree order. |
| `find(query)` | `find(query)` | `tree.read` | `Vec<NodeId>`, `bigint[]` | The ids of nodes that match every field set in the query. |
| `metadata(node)` | `metadata(node)` | `tree.metadata` | `Option<Vec<u8>>`, `Uint8Array` or `undefined` | The plugin's own bytes on that node, or none. |
| `set_metadata(node, bytes)` | `setMetadata(node, bytes)` | `tree.metadata` | `Result<(), PluginError>`, `void` (throws) | Replaces the plugin's own bytes on that node. |

The records and their fields:

| Record | Field | Rust type | TypeScript type | Meaning |
|---|---|---|---|---|
| `NodeView` | `id` | `NodeId` (`u64`) | `bigint` | The node id. |
| `NodeView` | `parent` | `Option<NodeId>` | `bigint \| undefined` | The parent node. None for a top-level node. |
| `NodeView` | `class` | `NodeClass` | `'container' \| 'folder' \| 'pane'` | The kind of node. |
| `NodeView` | `content_kind` | `Option<String>` | `string \| undefined` | Such as `ilium.terminal`, `ilium.editor`, `ilium.board` or `<plugin-id>.<kind>`. |
| `NodeView` | `title` | `String` | `string` | The title the user sees. |
| `NodeView` | `cwd` | `Option<String>` | `string \| undefined` | A display string. It grants no file access. |
| `NodeView` | `agent` | `Option<AgentView>` | `AgentView \| undefined` | Present only for a pane that runs an agent, and only with `agent.read`. |
| `NodeView` | `locked` | `bool` | `boolean` | True when the user or a plugin locked the node. |
| `NodeView` | `revision` | `u64` | `bigint` | Rises whenever a field above changes. Compare it to detect a change cheaply. |
| `NodeQuery` | `content_kind` | `Option<String>` | `contentKind?: string` | Match nodes of this content kind. |
| `NodeQuery` | `signature` | `Option<String>` | `signature?: string` | Match panes whose agent has this signature, such as `claude`. |
| `NodeQuery` | `under` | `Option<NodeId>` | `under?: bigint` | Match strict descendants of this node, at any depth. |
| `AgentView` | `signature` | `String` | `string` | The detected agent, such as `claude` or `codex`. |
| `AgentView` | `activity` | `AgentActivity` | `AgentActivity` | The current activity, see below. |
| `AgentView` | `objective` | `Option<String>` | `string \| undefined` | The objective the agent reports, when it reports one. |
| `AgentView` | `goal` | `Option<String>` | `string \| undefined` | The active goal, when one is set. |

`AgentActivity` has six values: `none`, `working`, `idle`, `awaiting-approval`, `done` and `menu`. In Rust they are `AgentActivity::Working` and so on. In TypeScript they are the strings `'working'` and `'awaiting-approval'`.

Events that concern the tree, delivered in batches to `on-events`:

| Event | Payload | Grant | Delivery |
|---|---|---|---|
| `tree-changed` | list of node ids | `tree.read` | Coalesced: pending events merge into one event whose list is the union of node ids in first-seen order. |
| `node-removed` | node id | `tree.read` | Never dropped. |
| `agent-identified`, `agent-activity-changed` | node and agent details | `agent.read` | Never dropped. |

The WIT declaration of the interface, for reference:

```wit
enum node-class { container, pane, folder }
record node-view {
  id: node-id, parent: option<node-id>, class: node-class,
  content-kind: option<string>, title: string, cwd: option<string>,
  agent: option<agent-view>, locked: bool, revision: u64,
}
record node-query {
  content-kind: option<string>, signature: option<string>, under: option<node-id>,
}
get: func(node: node-id) -> option<node-view>;                       // tree.read
children: func(node: node-id) -> list<node-id>;                      // tree.read
roots: func() -> list<node-id>;                                      // tree.read
find: func(query: node-query) -> list<node-id>;                      // tree.read
metadata: func(node: node-id) -> option<list<u8>>;                   // tree.metadata
set-metadata: func(node: node-id, bytes: list<u8>) -> result<_, plugin-error>;
```

### Import paths

Rust:

```rust
use ilium_plugin_sdk::prelude::*;                 // NodeId, PluginError
use ilium_plugin_sdk::host::tree::{self, NodeQuery};
use ilium_plugin_sdk::types::tree::{NodeView, AgentActivity};
```

TypeScript:

```ts
import * as tree from '@ilium/plugin-sdk/host/tree';
import type { NodeView, AgentActivity } from '@ilium/plugin-sdk/types';
```

### Example 1: find Claude panes and bookmark them

The plugin finds every terminal pane that runs the `claude` agent. It keeps a bookmark flag in its own metadata: the byte `1` means on, and an empty list means off.

**Grants:** `tree.read` `tree.metadata`

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::tree::{self, NodeQuery};

const BOOKMARK_ON: u8 = 1;

fn claude_panes() -> Vec<NodeId> {
    tree::find(NodeQuery {
        content_kind: Some("ilium.terminal".to_string()),
        signature: Some("claude".to_string()),
        under: None,
    })
}

fn set_bookmark(node: NodeId, on: bool) -> Result<(), PluginError> {
    let bytes = if on { vec![BOOKMARK_ON] } else { Vec::new() };
    tree::set_metadata(node, bytes)
}

fn is_bookmarked(node: NodeId) -> bool {
    tree::metadata(node).is_some_and(|bytes| bytes == [BOOKMARK_ON])
}
```

**TypeScript**

```ts
import * as tree from '@ilium/plugin-sdk/host/tree';

const BOOKMARK_ON = 1;

export function claudePanes(): bigint[] {
  return tree.find({
    contentKind: 'ilium.terminal',
    signature: 'claude',
    under: undefined,
  });
}

export function setBookmark(node: bigint, on: boolean): void {
  tree.setMetadata(node, new Uint8Array(on ? [BOOKMARK_ON] : []));
}

export function isBookmarked(node: bigint): boolean {
  const bytes = tree.metadata(node);
  return bytes !== undefined && bytes.length === 1 && bytes[0] === BOOKMARK_ON;
}
```

For the tree in the render below, `claude_panes()` returns:

```text
[ 14 ]
```

Pane 22 runs `codex`, so the signature filter leaves it out.

### Example 2: breadcrumb from the parent links

The plugin builds a path such as `project / backend / claude: api fix` by following `parent` from a node up to its top-level ancestor. The walk stops when a node has no parent.

**Grants:** `tree.read`

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::tree;

/// Titles from the top-level node down to `node`, joined with " / ".
/// Returns None when `node` or one of its ancestors does not exist.
fn breadcrumb(node: NodeId) -> Option<String> {
    let mut titles = Vec::new();
    let mut current = Some(node);
    while let Some(node_id) = current {
        let view = tree::get(node_id)?;
        titles.push(view.title);
        current = view.parent;
    }
    titles.reverse();
    Some(titles.join(" / "))
}
```

**TypeScript**

```ts
import * as tree from '@ilium/plugin-sdk/host/tree';

/** Titles from the top-level node down to `node`, joined with " / ". */
export function breadcrumb(node: bigint): string | undefined {
  const titles: string[] = [];
  let current: bigint | undefined = node;
  while (current !== undefined) {
    const view = tree.get(current);
    if (view === undefined) {
      return undefined;
    }
    titles.unshift(view.title);
    current = view.parent;
  }
  return titles.join(' / ');
}
```

`breadcrumb(14)` returns `Some("project / backend / claude: api fix")` for the tree below. Pane 14 has parent 4, and folder 4 has parent 1.

### Simulated render: tree with node ids



<figure>
<pre class="term"><span class="t-hi">▼ project</span>                               <span class="t-dim">id 1    container</span>
›  <span class="t-hi">▼ backend</span>                          <span class="t-dim">id 4    folder</span>
› ›   <span class="t-ok">⠋ claude: api fix</span>             <span class="t-dim">id 14   pane, agent claude</span>
› ›         shell                        <span class="t-dim">id 15   pane</span>
›  <span class="t-hi">▶ notes</span>                            <span class="t-dim">id 9    folder, locked</span>
</pre>
<figcaption>Illustrative, not generated. The project tree as a plugin sees it, with node ids. A find for the claude signature returns id 14 only. The agent activity appears in the agent field of each pane.</figcaption>
</figure>

### Diagram: node classes and parent links

<figure>
<svg viewBox="0 0 400 200" role="img" aria-label="Containers, folders and panes linked by parent arrows">
  <title>Node classes and parent links</title>
  <desc>A container holds two folders. Folder 4 holds panes 14 and 15, folder 9 holds pane 22. Arrows point from parent to child.</desc>
  <defs>
    <marker id="api-tree-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="140" y="8" width="120" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="30" font-size="11" text-anchor="middle" fill="currentColor">container, id 1</text>
  <rect x="30" y="78" width="120" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="90" y="100" font-size="11" text-anchor="middle" fill="currentColor">folder, id 4</text>
  <rect x="250" y="78" width="120" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="310" y="100" font-size="11" text-anchor="middle" fill="currentColor">folder, id 9</text>
  <rect x="8" y="150" width="110" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="63" y="172" font-size="11" text-anchor="middle" fill="currentColor">pane, id 14</text>
  <rect x="140" y="150" width="110" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="195" y="172" font-size="11" text-anchor="middle" fill="currentColor">pane, id 15</text>
  <rect x="270" y="150" width="120" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="330" y="172" font-size="11" text-anchor="middle" fill="currentColor">pane, id 22</text>
  <line x1="200" y1="42" x2="90" y2="78" stroke="currentColor" marker-end="url(#api-tree-arrow)"/>
  <line x1="200" y1="42" x2="310" y2="78" stroke="currentColor" marker-end="url(#api-tree-arrow)"/>
  <line x1="90" y1="112" x2="63" y2="150" stroke="currentColor" marker-end="url(#api-tree-arrow)"/>
  <line x1="90" y1="112" x2="195" y2="150" stroke="currentColor" marker-end="url(#api-tree-arrow)"/>
  <line x1="310" y1="112" x2="330" y2="150" stroke="currentColor" marker-end="url(#api-tree-arrow)"/>
  <text x="10" y="198" font-size="10" fill="currentColor">Arrows run from parent to child. A pane has no children.</text>
</svg>
<figcaption>Parent links run downwards. A pane is a leaf, so children returns an empty list for it.</figcaption>
</figure>

### Reading the tree on the client

A plugin half on a client reads the tree from the **client mirror**, not from the server. The mirror holds every node with its kind, title, status, attention and the metadata keys that a plugin may read. The server sends changes as `tree-changed` events and the client applies them to the mirror before it delivers the event to the plugin. The read functions are synchronous and answer from the mirror, so they never wait for the server.

- **Staleness.** The mirror lags the server by the delivery time of one delta, 5 ms at the 99th percentile on a local connection. A read right after a submission can still show the old tree. Wait for the `request-completed` of your own submission: the server sends the tree change before that outcome, so the mirror already shows the effect when you hear it.
- **After a reconnect.** A client that attaches again receives a full snapshot of the tree, and then a `tree-changed` event for every node.
- **Detached.** While the connection is down the mirror is frozen. Reads succeed on the old data, and submissions are queued.
- **Servers.** A server plugin half reads the tree itself and always sees the current state.

- **[HST-493]** A client plugin half MUST read the tree from the client mirror, and the mirror MUST be updated before the matching `tree-changed` event is delivered.
- **[HST-494]** The tree change caused by a forwarded submission MUST reach the client mirror before the `request-completed` of that submission.

### Intents and events around the read functions

Intents and events surround the six read functions. Nothing in this section is a new call on the `tree` interface.

| Addition | Where | Grant |
|---|---|---|
| `create-group`, `create-folder`, `create-project`, `create-split` | [pane lifecycle](/pages/api-pane-lifecycle/) | `tree.restructure` |
| `restructure` and `revert-restructure` (batches with undo) | [pane lifecycle](/pages/api-pane-lifecycle/) | `tree.restructure` |
| `set-bookmarked`, `set-expanded` | below | `tree.write` |
| `rename-node` with `source` and `expected-revision`, `set-inferred-icon` | [pane lifecycle](/pages/api-pane-lifecycle/) | `tree.write`, `tree.decorate` |
| `mark-attention`, `ack-attention`, event `attention-changed` | [pane lifecycle](/pages/api-pane-lifecycle/) | `tree.attention` |
| Decorations with spans, tooltips, frames, trailing and hover-only slots | [tree chrome](/pages/api-tree-chrome/) | `tree.decorate` |
| `tree_toolbar`, `row_actions`, `tree_orderings`, `display_modes`, virtual rows with children and drops | [tree chrome](/pages/api-tree-chrome/) | `ui.chrome`, `tree.decorate` |
| Event `display-mode-changed` | [tree chrome](/pages/api-tree-chrome/) | own `display_modes` |

`set-bookmarked { node, bookmarked }` sets or clears the star that the user sees on a row; the outcome is `ok`. `set-expanded { node, expanded }` folds or unfolds a real node and needs `tree.write`; for a plugin's own virtual row, pass `virtual-row` instead of `node`. A plugin that wants its own flag, not the user's star, still keeps it in `tree.metadata` as in Example 1.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{Intent, SetBookmarkedSpec, SetExpandedSpec, ExpandTarget};

/// Star every Claude pane and fold the group that holds them.
fn star_and_fold(panes: &[NodeId], group: NodeId) -> Vec<Submission> {
    let mut submissions: Vec<Submission> = panes.iter().map(|&node| {
        Submission::new(Intent::SetBookmarked(SetBookmarkedSpec { node, bookmarked: true }))
    }).collect();
    submissions.push(Submission::new(Intent::SetExpanded(SetExpandedSpec {
        target: ExpandTarget::Node(group),
        expanded: false,
    })));
    submissions
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk';

/** Star every Claude pane and fold the group that holds them. */
export function starAndFold(panes: bigint[], group: bigint): Submission[] {
  const stars: Submission[] = panes.map((node, index) => ({
    intent: { tag: 'set-bookmarked', val: { node, bookmarked: true } },
    correlation: BigInt(index + 1),
    dedupKey: undefined,
  }));
  stars.push({
    intent: { tag: 'set-expanded', val: { target: { tag: 'node', val: group }, expanded: false } },
    correlation: BigInt(panes.length + 1),
    dedupKey: undefined,
  });
  return stars;
}
```

<figure>
<pre class="term"><span class="t-dim">before</span>                          <span class="t-dim">after star_and_fold</span>
▼ Ilium                      ▶ Ilium
›   ⠋ Fix auth               <span class="t-dim">(group folded; the stars are inside)</span>
›   ● Review
</pre>
<figcaption>Folding hides the starred rows but does not remove them. A <code>find</code> still returns them, because the plugin reads the tree, not the screen.</figcaption>
</figure>

The new intents are refused with `not-permitted` without their grant and with `not-found` for a node that is gone. Batch limits and the full error table of these intents are on [pane lifecycle](/pages/api-pane-lifecycle/#errors-and-limits).

### What the user sees

Reading the tree shows nothing on its own. The user sees what the plugin does with the data: a decoration, a sorted list, a notice or a log line. Metadata is never shown unless the plugin shows it. A bookmark flag from Example 1 appears only if the plugin draws it, for example as a badge with `tree.decorate`. Log lines from a refused read appear in `ilium plugin logs <plugin-id>`.

### Errors and limits

A read of a missing node never fails: it gives none or an empty list. Reads can still fail for a missing grant or a revoked authorization, as the rows below show:

| Situation | Result |
|---|---|
| `get`, `metadata` for a node that does not exist | `None` or `undefined` |
| `children`, `roots`, `find` with no match, or `under` naming an unknown node | Empty list |
| `tree.read` or `tree.metadata` not granted, or declined by the user | `not-permitted`, written to the plugin log |
| `set-metadata` for a node that is absent or removed during the call | `not-found` |
| `set-metadata` past the per-node limit | `quota-exceeded` |
| Call while the authorization changed | `revoked`; the plugin is re-activated |

Limit: the key `node_metadata_kib` sets the metadata that one plugin may keep on one node. The default is 64 KiB. The T1 ceiling is 256 KiB. A manifest request above the ceiling is clamped and logged, and the user can lower the value in `config.toml` under `[plugins.limits]`.

<div class="warn">
<p>A write that passes the limit is refused whole. The old bytes stay in place, so store a version number in the first byte if the format may change.</p>
</div>

<div class="note">
<p>Metadata survives restarts because it is saved in the session snapshot. It is removed when its node is closed. It is not removed when the node is only moved or renamed.</p>
</div>

### Used by

- Tree ordering reads the tree and sets a `sort-key` on decorations; see [tree.decorate](/pages/capability-tree-decorate/).
- Bookmark and lock plugins store their flags in `tree.metadata`, as in Example 1.
- Workspace search and session statistics read the tree and the agent state.
- Tree toolbar, hover actions, orderings and virtual rows build on this interface; see [tree chrome](/pages/api-tree-chrome/).

### See also

- [Tree and window chrome](/pages/api-tree-chrome/) and [pane lifecycle](/pages/api-pane-lifecycle/) for these intents.
- [tree.read](/pages/capability-tree-read/) and [tree.metadata](/pages/capability-tree-metadata/) for the grants.
- [tree.write](/pages/capability-tree-write/) for rename, move, lock and close.
- [panes](/pages/api-panes/) for pane details, which `tree` does not give.
- [agent state](/pages/primitives-agent/) for the agent fields in more depth.
- [Events](/pages/api-events/) for `tree-changed` and `node-removed`.
- [Tutorial 6: notes pane](/pages/tutorial-06-notes-pane/), which uses metadata on both sides.
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/).
