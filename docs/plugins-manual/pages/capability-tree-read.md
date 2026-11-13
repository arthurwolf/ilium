---
layout: page
title: "tree.read"
permalink: /pages/capability-tree-read/
---

The `tree.read` grant lets a plugin read the project tree. With it, a plugin can list the nodes, learn the kind of each node, see how the nodes are nested, and see which agent runs in a pane. It is the base grant for most tree features. Without it the plugin cannot look up any node, and it receives no tree events.

The grant allows reads only. Changing the tree needs [tree.write](/pages/capability-tree-write/). Storing the plugin's own bytes on a node needs [tree.metadata](/pages/capability-tree-metadata/). Badges and rows need [tree.decorate](/pages/capability-tree-decorate/).

## What you need

| Requirement | Detail |
|---|---|
| Grant | `tree.read`, on the `server` side, the `client` side, or both |
| Always-granted set | [Always granted](/pages/capability-always-granted/): log, clock, timers, storage and codec |
| `agent.read` | Only if the plugin reads `node-view.agent` or filters with `signature` |
| Language | Rust with `ilium-plugin-sdk`, or TypeScript with `@ilium/plugin-sdk` |

Grant strings take no argument. `tree.read` is valid. `tree.read:*` and `tree.read:project` are not. An install with such a string fails with the error `manifest.grant-argument` and the text `capability "tree.read" takes no argument (got "tree.read:x")`.

## Full surface

All four functions are synchronous. They return at once and never return a request id. None of them returns a `plugin-error`, so a read has no error code to handle.

| Function (WIT, `ilium:host/tree`) | Returns | Meaning |
|---|---|---|
| `get: func(node: node-id) -> option<node-view>` | `none` for a missing node | The view of one node |
| `children: func(node: node-id) -> list<node-id>` | empty list | Direct children, in tree order |
| `roots: func() -> list<node-id>` | empty list | Top-level nodes, in tree order |
| `find: func(query: node-query) -> list<node-id>` | empty list | Nodes that match the query |

The types are declared in `ilium:types/tree` (`node-view`, `node-class`, `agent-view`,
`agent-activity`) and in the host interface (`node-query`).

| Type | Fields or cases | Meaning |
|---|---|---|
| `node-id` | `u64` | Stable id. It stays the same when the node moves or is renamed |
| `node-query` | `content-kind`, `signature`, `under`, all `option` | Filter for `find` |
| `node-view` | `id`, `parent`, `class`, `content-kind`, `title`, `cwd`, `agent`, `locked`, `revision` | Snapshot of one node |
| `node-class` | `container`, `pane`, `folder` | Kind of node |
| `agent-view` | `signature`, `activity`, `objective`, `goal` | The agent in a pane |
| `agent-activity` | `none`, `working`, `idle`, `awaiting-approval`, `done`, `menu` | Agent state |

Two events come from this grant. The host delivers them in batches to `on-events`.

| Event | Payload | Delivery |
|---|---|---|
| `tree-changed` | `list<node-id>` | Coalesced: pending lists merge into one event whose list holds each node once, in the order each node was first seen in the pending events |
| `node-removed` | `node-id` | Never dropped |

Rules that the reads follow:

- `children`, `roots` and `find` return ids in tree order. A parent comes before its children,
  and siblings keep their order.
- `under` matches every strict descendant of a node, at any depth. The node itself is not in the
  result. A query with every field unset returns every node.
- `content-kind` and `signature` match by exact string. A query combines its fields with AND.
- `node-view.agent` is `none` unless the plugin holds `agent.read`. A `signature` filter matches
  no pane without that grant. The grant is checked on every call, so a revoked grant takes effect
  on the next call.
- `tree-changed` names each node whose `node-view` fields changed, including nodes that were just
  created. `revision` moves with the same changes. Writing the node's metadata changes neither the
  revision nor the events.

### SDK names

| Item | Rust | TypeScript |
|---|---|---|
| Functions | `ilium_plugin_sdk::host::tree::{get, children, roots, find}` | `@ilium/plugin-sdk/host/tree` |
| Query type | `host::tree::NodeQuery { content_kind, signature, under }` | `{ contentKind, signature, under }` |
| View types | `ilium_plugin_sdk::types::tree::{NodeView, NodeClass, AgentView, AgentActivity}` | `NodeView` and the others from `@ilium/plugin-sdk/types` |
| `tree-changed` | `Event::TreeChanged(Vec<NodeId>)` | `{ tag: 'tree-changed', val: bigint[] }` |
| `node-removed` | `Event::NodeRemoved(NodeId)` | `{ tag: 'node-removed', val: bigint }` |

Node ids are `u64` in Rust and `bigint` in TypeScript. Field names change case as the SDK rules
require: `content_kind` in Rust and `contentKind` in TypeScript.

<figure>
<svg viewBox="0 0 420 170" role="img" aria-label="Plugin calls find and get on the kernel tree; the kernel sends tree events back">
  <defs>
    <marker id="tree-read-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="110" y="10" width="200" height="50" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="30" text-anchor="middle" font-size="11" fill="currentColor">Plugin, tree.read granted</text>
  <text x="210" y="46" text-anchor="middle" font-size="10" fill="currentColor">activate and on-events</text>
  <rect x="110" y="110" width="200" height="50" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="130" text-anchor="middle" font-size="11" fill="currentColor">Kernel tree</text>
  <text x="210" y="146" text-anchor="middle" font-size="10" fill="currentColor">get, children, roots, find</text>
  <path d="M150 62 L150 106" stroke="currentColor" stroke-width="1.5" fill="none" marker-end="url(#tree-read-arrow)"/>
  <text x="140" y="86" text-anchor="end" font-size="10" fill="currentColor">find, get</text>
  <path d="M270 106 L270 64" stroke="currentColor" stroke-width="1.5" fill="none" marker-end="url(#tree-read-arrow)"/>
  <text x="280" y="82" font-size="10" fill="currentColor">tree-changed</text>
  <text x="280" y="96" font-size="10" fill="currentColor">node-removed</text>
</svg>
<figcaption>The plugin reads nodes with find and get. The kernel sends tree-changed and node-removed back in batches.</figcaption>
</figure>

<div class="note">

The `cwd` field of a `node-view` is a display string. Reading it does not open the directory.
File access needs a file scope such as `fs.read:project`. See [file access](/pages/file-access/).

</div>

## Risk and tiers

- The grant is not marked high risk. It appears in the review dialog and is checked by default.
- A T1 bundled plugin receives it automatically, up to the ceiling in its manifest.
- A T2 or T3 plugin gets it when the user leaves the row checked in the review dialog. A user
  can also remove it later in Settings > Plugins.
- Grants are per side and per plugin. A plugin that reads the tree only on the server should
  request `tree.read` under `server` and nothing under `client`.
- Revoking the grant: Settings > Plugins > the plugin > Capabilities, or
  `ilium plugin grants <id> --revoke tree.read`.

## What the user sees

Before a T2 or T3 plugin is installed, the review dialog shows one row for the grant. The row text
is fixed: "Read the workspace tree".

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.tree 0.1.0 by Example Ltd                                      │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Read the workspace tree <span class="t-dim">……………………………………………………………………………………………</span> tree.read │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests tree.read, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

If the grant is declined, the plugin keeps running. Its log shows one warning, and the plugin
continues with empty answers:

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs com.example.agent-index
2026-10-11 12:04:11 <span class="t-warn">warn</span>  refused tree.read in find (declined)
2026-10-11 12:04:11 <span class="t-ok">info</span>  0 codex panes at activation
</pre>
<figcaption>Illustrative, not generated. The plugin log after the user declined tree.read.</figcaption>
</figure>

## Declined, revoked and missing grants

Because the reads have no error channel, a missing grant gives an empty answer, not an error code.

- **Declined at install or on the prompt.** `get` returns `none`. `children`, `roots` and `find`
  return empty lists. The first refused call of each plugin instance writes one warn line that
  names `tree.read` and the reason (`declined`). No tree events are delivered.
- **Revoked while running.** The next read returns `none` or an empty list (reads have no error channel, so this is the read-side form of the revoked outcome in ABI-043). The plugin receives
  `deactivate(revoked)`, and it is activated again with the reduced grant set. The restart does not
  count as a failure.
- **Granted later.** The plugin receives `deactivate(granted)` and is activated again with the new
  grant set. Events start arriving after that.
- **Requested but not in the manifest.** A call to an import the manifest never requested is a
  trap, not a refusal. The plugin instance is restarted. Always list `tree.read` in the manifest.

A plugin should treat an empty answer as a normal result. It should turn off the feature that needs
the tree. It should not retry in a loop.

## Worked example 1: Rust, server side

This plugin counts the Codex panes at activation and logs every node change. The manifest excerpt
asks for both grants on the server side.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.agent-index",
  "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": [] },
  "capabilities": { "server": ["tree.read", "agent.read"] }
}
```

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::tree::{self, NodeQuery};

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct AgentIndex;

impl Lifecycle for AgentIndex {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        let codex_panes = tree::find(NodeQuery {
            content_kind: Some("ilium.terminal".into()),
            signature: Some("codex".into()),
            under: None,
        });
        log::log(Level::Info, &format!("{} codex panes at activation", codex_panes.len()));
        log::log(Level::Info, &format!("plugin {} active", context.plugin));
        Ok(())
    }
}

impl Handler for AgentIndex {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::TreeChanged(nodes) => {
                    for node in nodes {
                        if let Some(view) = tree::get(node) {
                            let line = format!("node {} is now {}", view.id, view.title);
                            log::log(Level::Info, &line);
                        }
                    }
                }
                Event::NodeRemoved(node) => {
                    log::log(Level::Info, &format!("node {node} removed"));
                }
                _ => {}
            }
        }
        Vec::new()
    }
}
```

## Worked example 2: TypeScript, client side

The same query on the client. The `signature` filter needs `agent.read` on the client side as well,
so the manifest asks for both grants under `client`.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.agent-index-ui",
  "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": [] },
  "capabilities": { "client": ["tree.read", "agent.read"] }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { find, get } from '@ilium/plugin-sdk/host/tree';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate(context) {
    const codexPanes = find({
      contentKind: 'ilium.terminal',
      signature: 'codex',
      under: undefined,
    });
    log('info', `${codexPanes.length} codex panes at activation of ${context.plugin}`);
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'tree-changed') {
        for (const node of event.val) {
          const view = get(node);
          if (view) log('info', `node ${view.id} is now "${view.title}"`);
        }
      }
      if (event.tag === 'node-removed') log('info', `node ${event.val} removed`);
    }
    return [];
  },
});
```

## Edge cases

| Case | Result |
|---|---|
| Node id that does not exist | `get` returns `none`. `children` returns an empty list. Not an error |
| Node removed after an event was queued | `get` returns `none`. Act on the `node-removed` event |
| `find` with `signature` and no `agent.read` | Empty list. Panes without an agent are not matched |
| `under` naming a node that has no children | Empty list |
| Query with every field unset | Every node in tree order |
| Many changes to one node in one batch | One `tree-changed` entry for that node |
| Restored project after a restart | One `tree-changed` batch covers all restored nodes, once |

## Limits

- Event batches hold at most 256 events. The event queue per plugin holds 1,024 events by default for every tier; a T1
  plugin may request up to 4,096 in `limits`. A full queue on a never-dropped event such as `node-removed` restarts the plugin.
- Each `on-events` call has a default deadline of 20 ms for every tier; a T1 plugin may request up to 50 ms in `limits`. Keep the handler
  short, and do the slow work in a job.

## First-party features that request it

These rows of [feature traceability](/pages/reference-feature-traceability/) list `tree.read`: feature 5 (tree ordering), feature 6 (bookmarks,
locks and removal of empty groups), feature 9 (workspace search), feature 42 (session naming),
feature 43 (tree organisation) and feature 109 (the `ilium panes` command).

## See also

- [tree.write](/pages/capability-tree-write/) for the intents that change the tree.
- [tree.metadata](/pages/capability-tree-metadata/) for the plugin's own bytes on a node.
- [tree.decorate](/pages/capability-tree-decorate/) for badges and rows.
- [agent.read](/pages/capability-agent-read/) for the agent data in `node-view.agent`.
- [Errors and limits](/pages/reference-errors-and-limits/) for the error codes and the trap rules.
- [Events and intents](/pages/events-and-intents/) for batching and delivery order.

Checked against the API version `ilium:plugin@0.1.0`.
