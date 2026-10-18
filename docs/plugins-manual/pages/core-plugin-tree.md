---
layout: page
title: Tree
permalink: /pages/core-plugin-tree/
---

## Tree

The Tree plugin (`ilium.core.tree`, side both) owns the project tree as the user sees it: groups, folders and top-level destinations, moving, renaming and closing entries, the tree order, bookmarks, locks and the automatic removal of empty groups. The tree itself is a kernel structure. The plugin reads it with the [`tree` import](/pages/api-tree/) and changes it only through intents, so this page shows how a plugin drives a kernel surface without drawing it.

### What it covers

- The project tree: groups, folders and panes in one outline (Panes and layout, The tree).
- Groups, folders and top-level destinations (Panes and layout, Groups).
- Moving, reordering and renaming entries, and rearranging without stopping any process (Panes and layout, Moving, ordering and renaming).
- Tree ordering (Settings, User Interface tab).
- Bookmarks, locks and automatic removal of empty groups (Panes and layout, Groups and Closing).
- Closing panes and groups (Panes and layout, Closing).

### See it

<figure><img src="/assets/demos/02-pane-tree.gif" alt="The pane tree: groups open and close, a pane is dragged to a new group" loading="lazy"><figcaption>The pane tree in use. Entries move between groups while their processes keep running.</figcaption></figure>

### What the user sees

<figure>
<pre class="term">╭─ Tree ───────────────────────────────────────╮
│<span class="t-hi">▼ 📂 Ilium</span>                                    │
│<span class="t-dim">  ▼ 📂 Deploy</span>                                 │
│<span class="t-ok">     🖥️ ★ api-deploy</span>                          │
│<span class="t-dim">     🖥️   web-deploy</span>                          │
│<span class="t-dim">  ▶ 📁 docs</span>                                   │
│<span class="t-warn">  🔒 🖥️ release-build</span>                         │
│<span class="t-dim">     🖥️   notes</span>                               │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative: glyphs depend on the icon set. Two groups, one folder and five panes. The star marks the bookmarked pane and the lock marks the pane that close-node refuses to close.</figcaption>
</figure>

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs ilium.core.tree
<span class="t-dim">2026-10-12 09:14:02 info  server ilium.core.tree rename-node node=14</span>
<span class="t-warn">2026-10-12 09:14:02 warn  server ilium.core.tree request 7 refused(conflict)</span>
<span class="t-dim">2026-10-12 09:14:03 info  server ilium.core.tree keeping manual title</span>

╭─ Tree ───────────────────────────────────────╮
│<span class="t-dim">  ▼ 📂 Deploy</span>                                 │
│<span class="t-hi">     🖥️ ★ api-deploy</span>                          │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A refused rename: the tree row keeps the manual name and the log line shows <code>refused(conflict)</code>.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.tree",
  "name": "Tree",
  "version": "0.1.0",
  "publisher": "Ilium",
  "capabilities": {
    "server": ["tree.read", "tree.write", "tree.metadata", "tree.decorate", "tree.restructure"],
    "client": ["tree.read"]
  }
}
```

`tree.restructure` is a high risk grant; see [tree.restructure](/pages/capability-tree-restructure/). The bundled plugin carries it by default.

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.tree",
  "name": "Tree",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "The Tree plugin (ilium.core.tree, side both) owns the project tree as the user sees it: groups, folders and top-level destinations, moving, renaming and closing entries, the tree order, bookmarks, locks and the automatic removal of empty gr",
  "license": "MIT",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0"
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0"
  },
  "capabilities": {
    "server": [
      "tree.read",
      "tree.write",
      "tree.metadata",
      "tree.decorate",
      "tree.restructure"
    ],
    "client": [
      "tree.read"
    ]
  },
  "activation": [
    "startup"
  ],
  "limits_tier": "first-party"
}
```

### Samples

#### Order the tree

A server handler reads the tree on each coalesced `tree-changed` event and submits `set-decoration` with only a `sort-key` for nodes whose key changed. It uses [api-tree](/pages/api-tree/), the [tree.read](/pages/capability-tree-read/) and [tree.decorate](/pages/capability-tree-decorate/) grants, [events and intents](/pages/events-and-intents/), the [event list](/pages/api-events/) and [tree chrome](/pages/api-tree-chrome/).

**Rust**

```rust
use std::collections::HashMap;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::tree;
use ilium_plugin_sdk::types::intents::SetDecorationSpec;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct TreeOrder {
    keys: HashMap<NodeId, i64>,
}

impl Lifecycle for TreeOrder {}

impl Handler for TreeOrder {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::TreeChanged(nodes) = event else { continue };
            for node in nodes {
                let Some(view) = tree::get(node) else { continue };
                let key = sort_key(&view.title);
                if self.keys.get(&node) == Some(&key) { continue; }
                self.keys.insert(node, key);
                submissions.push(Submission::new(Intent::SetDecoration(SetDecorationSpec {
                    sort_key: Some(key),
                    ..SetDecorationSpec::text(node, "order", "")
                })));
            }
        }
        submissions
    }
}

// Alphabetical by the first four bytes of the lower-case title.
fn sort_key(title: &str) -> i64 {
    title.to_lowercase().bytes().take(4).fold(0, |key, byte| key * 256 + i64::from(byte))
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import * as tree from '@ilium/plugin-sdk/host/tree';
import type { Event, Submission } from '@ilium/plugin-sdk/types';

const keys = new Map<bigint, bigint>();
let next = 1n;

// Alphabetical by the first four bytes of the lower-case title.
const sortKey = (title: string): bigint =>
  [...new TextEncoder().encode(title.toLowerCase()).slice(0, 4)]
    .reduce((key, byte) => key * 256n + BigInt(byte), 0n);

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'tree-changed') continue;
      for (const node of event.val) {
        const view = tree.get(node);
        if (view === undefined) continue;
        const key = sortKey(view.title);
        if (keys.get(node) === key) continue;
        keys.set(node, key);
        submissions.push({
          intent: { tag: 'set-decoration', val: {
            node, slot: 'order', content: [], icon: undefined, sortKey: key, persist: false,
          } },
          correlation: next++,
          dedupKey: undefined,
        });
      }
    }
    return submissions;
  },
});
```

#### New group or folder

The client command `New group` cannot create nodes itself. It sends a message to its own server side, which submits `create-group` or `create-folder` and logs the outcome. Messages between the two sides of one plugin need no grant. Pages: [tree structure intents](/pages/api-intent-tree-structure/), [tree.write](/pages/capability-tree-write/), [tree.restructure](/pages/capability-tree-restructure/), [commands](/pages/api-commands/), [worlds and sides](/pages/worlds-and-sides/), [events](/pages/api-events/) and [messages and topics](/pages/api-messages-and-topics/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{
    CreateFolderSpec, CreateGroupSpec, MessageSpec, MessageTarget, PluginSide,
};
use ilium_plugin_sdk::types::tree::Placement;
use ilium_plugin_sdk::host::log::{self, Level};

// Client side: the command forwards the selected node and the wanted kind.
fn forward(selected: NodeId, kind: &str) -> Submission {
    Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget { plugin: "ilium.core.tree".into(), side: PluginSide::Server, client: None },
        channel: kind.into(),
        payload: selected.to_le_bytes().to_vec(),
    }))
}

// Server side: Event::Message arrives with the same channel and payload.
fn create(channel: &str, payload: &[u8]) -> Option<Submission> {
    let parent = NodeId::from_le_bytes(payload.try_into().ok()?);
    let intent = match channel {
        "new-group" => Intent::CreateGroup(CreateGroupSpec {
            parent, title: "New group".into(), placement: Placement::InsideLast, children: vec![],
        }),
        "new-folder" => Intent::CreateFolder(CreateFolderSpec { parent, title: "New folder".into(), path: None }),
        _ => return None,
    };
    Some(Submission::keyed(intent, format!("{channel}-{parent}")))
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk/types';

// Client side: the command forwards the selected node and the wanted kind.
const forward = (selected: bigint, kind: string): Submission => ({
  intent: { tag: 'message', val: {
    to: { plugin: 'ilium.core.tree', side: 'server', client: undefined },
    channel: kind,
    payload: new Uint8Array(new BigUint64Array([selected]).buffer),
  } },
  correlation: 1n,
  dedupKey: undefined,
});

// Server side: the message event carries the same channel and payload.
function create(channel: string, payload: Uint8Array): Submission | undefined {
  const parent = new DataView(payload.buffer, payload.byteOffset).getBigUint64(0, true);
  const intent = channel === 'new-group'
    ? { tag: 'create-group', val: { parent, title: 'New group', placement: 'inside-last', children: [] } } as const
    : channel === 'new-folder'
      ? { tag: 'create-folder', val: { parent, title: 'New folder', path: undefined } } as const
      : undefined;
  return intent && { intent, correlation: 2n, dedupKey: `${channel}-${parent}` };
}
```

#### Move and rename without overriding the user

A move keeps placement with `inside-last`. A rename carries `expected-revision` and the source `inferred`, so a name the user typed always wins. On `refused(conflict)` the plugin stops and does not retry. Pages: [intents](/pages/api-intents/) and [requests and outcomes](/pages/requests-and-outcomes/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{MoveNodeSpec, RenameNodeSpec};
use ilium_plugin_sdk::types::tree::{Placement, TitleSource};

fn move_and_rename(node: NodeId, target: NodeId, title: &str, revision: u64) -> Vec<Submission> {
    vec![
        Submission::new(Intent::MoveNode(MoveNodeSpec {
            node, target, placement: Placement::InsideLast,
        })),
        Submission::new(Intent::RenameNode(RenameNodeSpec {
            node,
            title: title.into(),
            source: TitleSource::Plugin,
            expected_revision: Some(revision),
        })),
    ]
}

fn on_completed(result: RequestResult) {
    if let Outcome::Refused(error) = result.outcome {
        if error.code == ErrorCode::Conflict {
            log::log(Level::Info, "manual name kept; not retrying");
        }
    }
}
```

**TypeScript**

```ts
import { log } from '@ilium/plugin-sdk/host/log';
import type { Submission, RequestResult } from '@ilium/plugin-sdk/types';

const moveAndRename = (node: bigint, target: bigint, title: string, revision: bigint): Submission[] => [
  { intent: { tag: 'move-node', val: { node, target, placement: 'inside-last' } },
    correlation: 10n, dedupKey: undefined },
  { intent: { tag: 'rename-node', val: { node, title, source: 'inferred', expectedRevision: revision } },
    correlation: 11n, dedupKey: undefined },
];

function onCompleted(result: RequestResult): void {
  if (result.outcome.tag === 'refused' && result.outcome.val.code === 'conflict') {
    log('info', 'manual name kept; not retrying');
  }
}
```

#### Bookmarks and locks

Bookmark and lock flags survive restarts and protect nodes from closing. The plugin keeps its own bookmark flag with `tree.set-metadata`, mirrors it with `set-bookmarked` and locks with `set-node-locked`, so a later `close-node` is refused with `refused(conflict)`. Pages: [tree.metadata](/pages/capability-tree-metadata/), [api-tree](/pages/api-tree/), [tree structure intents](/pages/api-intent-tree-structure/) and [intents](/pages/api-intents/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::tree;
use ilium_plugin_sdk::types::intents::{SetBookmarkedSpec, SetNodeLockedSpec};

fn protect(node: NodeId) -> Result<Vec<Submission>, PluginError> {
    tree::set_metadata(node, vec![1])?; // 1 = bookmarked, empty = not
    Ok(vec![
        Submission::new(Intent::SetBookmarked(SetBookmarkedSpec { node, bookmarked: true })),
        Submission::new(Intent::SetNodeLocked(SetNodeLockedSpec { node, locked: true })),
        // Refused with conflict while the lock is set:
        Submission::new(Intent::CloseNode(node)),
    ])
}
```

**TypeScript**

```ts
import * as tree from '@ilium/plugin-sdk/host/tree';
import type { Submission } from '@ilium/plugin-sdk/types';

function protect(node: bigint): Submission[] {
  tree.setMetadata(node, new Uint8Array([1])); // 1 = bookmarked, empty = not
  return [
    { intent: { tag: 'set-bookmarked', val: { node, bookmarked: true } }, correlation: 20n, dedupKey: undefined },
    { intent: { tag: 'set-node-locked', val: { node, locked: true } }, correlation: 21n, dedupKey: undefined },
    // Refused with conflict while the lock is set:
    { intent: { tag: 'close-node', val: { node } }, correlation: 22n, dedupKey: undefined },
  ];
}
```

#### Remove empty groups

A group that loses its last child disappears unless it is locked or bookmarked. On `node-removed` the plugin looks up the former parent from its own map, and if it has no children and no flag it submits `close-node`. Pages: [api-tree](/pages/api-tree/), [lifecycle](/pages/lifecycle/), [events](/pages/api-events/) and [intents](/pages/api-intents/).

**Rust**

```rust
use std::collections::HashMap;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::tree;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct EmptyGroups {
    parents: HashMap<NodeId, NodeId>, // child -> parent, refreshed on tree-changed
}

impl Lifecycle for EmptyGroups {}

impl Handler for EmptyGroups {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::TreeChanged(nodes) => {
                    for node in nodes {
                        if let Some(parent) = tree::get(node).and_then(|view| view.parent) {
                            self.parents.insert(node, parent);
                        }
                    }
                }
                Event::NodeRemoved(node) => {
                    let Some(parent) = self.parents.remove(&node) else { continue };
                    let Some(view) = tree::get(parent) else { continue };
                    let flagged = view.locked || tree::metadata(parent).is_some_and(|b| b == [1]);
                    if tree::children(parent).is_empty() && !flagged {
                        submissions.push(Submission::new(Intent::CloseNode(parent)));
                    }
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
import * as tree from '@ilium/plugin-sdk/host/tree';
import type { Event, Submission } from '@ilium/plugin-sdk/types';

const parents = new Map<bigint, bigint>(); // child -> parent, refreshed on tree-changed
let next = 1n;

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'tree-changed') {
        for (const node of event.val) {
          const parent = tree.get(node)?.parent;
          if (parent !== undefined) parents.set(node, parent);
        }
      } else if (event.tag === 'node-removed') {
        const parent = parents.get(event.val);
        parents.delete(event.val);
        const view = parent === undefined ? undefined : tree.get(parent);
        if (parent === undefined || view === undefined) continue;
        const flagged = view.locked || tree.metadata(parent)?.[0] === 1;
        if (tree.children(parent).length === 0 && !flagged) {
          submissions.push({ intent: { tag: 'close-node', val: { node: parent } }, correlation: next++, dedupKey: undefined });
        }
      }
    }
    return submissions;
  },
});
```

#### Right-click menu for a node

A right click on a tree row sends `context-requested`. The plugin remembers the node and renders a `menu` from [collections](/pages/widgets-collections/) with the actions that apply to it. Choosing an item sends the matching intent.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ui::{ActionKind, ActionValue};
use ilium_plugin_sdk::ui::build::Tree;

impl Handler for TreeMenu {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::UiAction(action) = event else { continue };
            let Some(node) = action.key.strip_prefix("row-") else { continue };
            match (action.kind, action.value) {
                (ActionKind::ContextRequested, _) => self.menu_node = Some(node.to_string()),
                (ActionKind::Activated, ActionValue::Text(item)) if item == "lock" => {
                    return vec![Submission::new(Intent::SetNodeLocked(SetNodeLockedSpec {
                        node: node.parse().unwrap_or_default(), locked: true,
                    }))];
                }
                _ => {}
            }
        }
        Vec::new()
    }
}

// In Views::render, when a node is selected for the menu:
fn menu(tree: &mut Tree, node: &str) -> NodeId {
    tree.menu("Node actions").items(vec![
        MenuItem::new("rename", "Rename"),
        MenuItem::new("lock", "Lock"),
        MenuItem::new("close", "Close").danger(),
    ]).key(format!("menu-{node}"))
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';

let menuNode: string | undefined;

export const { handler } = definePlugin({
  handler: {
    onEvents(batch: Event[]): Submission[] {
      const out: Submission[] = [];
      for (const event of batch) {
        if (event.tag !== 'ui-action' || !event.val.key.startsWith('row-')) continue;
        const { kind, value, key } = event.val;
        const node = key.slice('row-'.length);
        if (kind === 'context-requested') menuNode = node;
        if (kind === 'activated' && value.tag === 'text' && value.val === 'lock') {
          out.push({ intent: { tag: 'set-node-locked', val: { node, locked: true } }, correlation: 1n });
        }
      }
      return out;
    },
  },
});

// In the render function, when menuNode is set:
// tree.menu('Node actions', [{ id: 'rename', label: 'Rename' }, { id: 'lock', label: 'Lock' }, { id: 'close', label: 'Close', danger: true }]);
```

### Extension points

Other plugins change what `ilium.core.tree` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.tree/<point>`). Contributors need `extensions.contribute:ilium.core.tree/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `before-move` | hook (veto) | Refuse a drag or move of a node before the tree applies it. | node, from_parent, to_parent, index, source (mouse, keys, command, plugin) | standard |
| `before-close` | hook (veto) | Refuse closing a pane or group, for example while work is unsaved. | node, kind, has_children | standard |
| `node-sorts` | collect | Add a sort order to the tree Order By choice. | entry: id, title, slot (a decoration slot whose sort-key orders siblings) | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.tree/bookmarks`: list, add, remove: read and edit the project bookmarks

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.tree/changed`: { node, change } after any structural change

Decoration slots, row actions and display modes of every plugin are drawn by the tree.

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.tree"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Refuses to close the pane that runs the build, with the reason `Build still running`

A package adds a `veto` handler. Handlers run until one refuses.

```json
{
  "api": "ilium:plugin@0.1.0",
  "exports": [
    "hook-handler"
  ],
  "capabilities": { "server": ["extensions.contribute:ilium.core.tree/before-close"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.tree/before-close",
        "id": "keep-build-pane",
        "priority": 50,
        "handler": "keep_build_pane"
      }
    ]
  }
}
```

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;

impl HookHandler for Plugin {
    fn handle_hook(&mut self, call: HookCall) -> HookReply {
        match call.extension.as_str() {
            "keep_build_pane" => {
                // call.payload_json holds: node, kind, has_children
                let _payload: serde_json::Value = match serde_json::from_str(&call.payload_json) {
                    Ok(value) => value,
                    Err(_) => return HookReply::Pass,
                };
                HookReply::Refuse("reason".into())
            }
            _ => HookReply::Pass,
        }
    }
}
```

**TypeScript**

```ts
definePlugin({
  handleHook(call) {
    if (call.extension !== 'keep_build_pane') return { tag: 'pass' };
    // call.payloadJson holds: node, kind, has_children
    const payload = JSON.parse(call.payloadJson);
    void payload;
    return { tag: 'refuse', val: 'reason' };
  },
});
```

A handler that is late, traps or returns a reply the mode does not allow counts as `pass`. See [Extension points](/pages/api-extension-points/#hooks).

### ABI used

- [Tree import](/pages/api-tree/), [tree.read](/pages/capability-tree-read/), [tree.write](/pages/capability-tree-write/), [tree.metadata](/pages/capability-tree-metadata/), [tree.decorate](/pages/capability-tree-decorate/), [tree.restructure](/pages/capability-tree-restructure/)
- [Tree structure intents](/pages/api-intent-tree-structure/), [Intents](/pages/api-intents/), [Tree chrome](/pages/api-tree-chrome/)
- [Events](/pages/api-events/), [Events and intents](/pages/events-and-intents/), [Requests and outcomes](/pages/requests-and-outcomes/)
- [Commands](/pages/api-commands/), [Messages and topics](/pages/api-messages-and-topics/), [Worlds and sides](/pages/worlds-and-sides/), [Lifecycle](/pages/lifecycle/)
- [Menu widget](/pages/widgets-collections/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Layout plugin](/pages/core-plugin-layout/)
- [Sessions plugin](/pages/core-plugin-sessions/)

### Security

Risk marks of this plugin's extension points follow the corrected table in [Extension security](/pages/reference-extension-security/) ([SEC-293]).
