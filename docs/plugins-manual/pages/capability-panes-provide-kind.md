---
layout: page
title: "panes.provide-kind"
permalink: /pages/capability-panes-provide-kind/
---

## Overview

The `panes.provide-kind` grant lets a client-side plugin add a new kind of pane. A kind is a
named type of pane, such as a file explorer, a board or a Markdown view. The plugin declares the
kind in the manifest, and the host creates panes of that kind when the user or another plugin asks
for them.

The pane is a basic pane. The host draws its frame: the title bar, the status line in the footer,
and the focus border. The plugin draws only the body, through the `views` export, which returns a
UI tree for each pane of its kind. The plugin keeps its own state, either in node metadata or in
storage, so a restored pane can show the same content after a restart.

<figure>
<svg viewBox="0 0 400 200" role="img" aria-label="A basic pane: the plugin draws the body through views.render, and the host draws the frame, title and status line">
  <defs>
    <marker id="pk-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M 0 0 L 10 5 L 0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <text x="200" y="24" text-anchor="middle" fill="currentColor" font-size="11">Pane kind: example.tree-explorer.files</text>
  <rect x="10" y="40" width="150" height="110" rx="6" fill="none" stroke="currentColor"/>
  <text x="85" y="62" text-anchor="middle" fill="currentColor" font-size="12">Plugin</text>
  <text x="85" y="84" text-anchor="middle" fill="currentColor" font-size="10">views.render</text>
  <text x="85" y="102" text-anchor="middle" fill="currentColor" font-size="10">set-title, set-status</text>
  <text x="85" y="120" text-anchor="middle" fill="currentColor" font-size="10">Node metadata</text>
  <rect x="240" y="40" width="150" height="110" rx="6" fill="none" stroke="currentColor"/>
  <text x="315" y="62" text-anchor="middle" fill="currentColor" font-size="12">Host frame</text>
  <text x="315" y="84" text-anchor="middle" fill="currentColor" font-size="10">Title and status line</text>
  <text x="315" y="102" text-anchor="middle" fill="currentColor" font-size="10">Focus and frame</text>
  <text x="315" y="120" text-anchor="middle" fill="currentColor" font-size="10">Body area</text>
  <line x1="162" y1="80" x2="236" y2="80" stroke="currentColor" marker-end="url(#pk-arrow)"/>
  <text x="200" y="72" text-anchor="middle" fill="currentColor" font-size="10">ui tree</text>
  <line x1="238" y1="110" x2="164" y2="110" stroke="currentColor" marker-end="url(#pk-arrow)"/>
  <text x="200" y="130" text-anchor="middle" fill="currentColor" font-size="10">key-pressed</text>
  <text x="200" y="180" text-anchor="middle" fill="currentColor" font-size="10">Grant: panes.provide-kind (client side)</text>
</svg>
<figcaption>A basic pane. The plugin draws the body and handles keys. The host draws the frame,
the title and the footer status line, which the plugin sets with the basic primitive.</figcaption>
</figure>

## What you need

- The grant `panes.provide-kind`, on the client side only. Its risk is not marked high.
- A `pane_kinds` entry in the manifest for each kind the plugin provides.
- A client build of the plugin with a `handler` export, which is required on the client side and
  receives `key-pressed` events when the kind wants keys, and a `views` export, which renders the
  body. The manifest lists `"views"` in its `exports` array, because it is an optional export.
- The SDK for the language you write in. The Rust SDK is `ilium_plugin_sdk`; the TypeScript SDK is
  `@ilium/plugin-sdk`. Both expose the basic primitive as one module (see the table below).
- [ui.panels](/pages/capability-ui-panels/) only if the body opens panels or popups as well.
- Read access for the data the body shows. A basic pane has no access to the file system or the
  terminal by itself. Its reads need their own grants, such as `fs.read:project`.

`ui.invalidate` for the views the plugin contributes is always granted, so a basic pane can redraw
its body without asking for more.

## Manifest contribution

The grant allows the `pane_kinds` contribution. Each entry declares one kind:

| Field | Type | Meaning |
|---|---|---|
| `id` | string | Local id. The global kind id is `<plugin-id>.<id>`. |
| `title` | string | Display name of the kind in menus. |
| `icon` | string | A Tabler icon name. |
| `primitive` | `"basic"` | The primitive the kind uses. Only `basic` is valid for this grant. |
| `wants_keys` | bool | When `true`, keys the focused widget does not consume arrive as `key-pressed`. |
| `persist` | `"metadata"`, `"storage"` or `"none"` | How the pane's state survives a restart (see below). |

```json
{
  "contributes": {
    "pane_kinds": [
      {
        "id": "files",
        "title": "Files",
        "icon": "folder",
        "primitive": "basic",
        "wants_keys": true,
        "persist": "metadata"
      }
    ]
  },
  "capabilities": {
    "client": ["panes.provide-kind", "fs.read:project"]
  }
}
```

With this manifest, the global kind id of the example is `example.tree-explorer.files` when the
plugin id is `example.tree-explorer`.

### The persist values

| Value | Where the state lives | After a restart |
|---|---|---|
| `metadata` | Node metadata: the plugin's own byte map on the node, at most 64 KiB per node. | The node is restored with its metadata, and the view can read it. |
| `storage` | The plugin's storage buckets. Keys under `pane/<node-id>/` are deleted when the pane is closed. | The node is restored empty, as with `none`. The plugin reads its own storage. |
| `none` | Nothing is kept. | The node is recreated empty, and the view starts without state. |

### Grant syntax and tiers

- Valid: `panes.provide-kind`.
- Invalid: `panes.provide-kind:explorer`. The kind is declared in `pane_kinds`, not in the grant.
- A T1 bundled plugin receives the grant automatically, up to its manifest ceiling.
- A T2 or T3 plugin needs an explicit grant in the review dialog, and the grant is remembered.

## The basic primitive

The `basic` primitive gives the plugin four calls for each of its panes. The plugin passes the
node id of the pane, which the host gives it in `views.render` and in events.

| Function | Rust (`ilium_plugin_sdk::primitives::basic`) | TypeScript (`@ilium/plugin-sdk/primitives/basic`) | Returns | Meaning |
|---|---|---|---|---|
| `set-title` | `set_title(pane: NodeId, title: &str)` | `setTitle(pane: bigint, title: string)` | `Result<(), PluginError>` | Sets the title in the frame. At most 256 characters. |
| `set-status` | `set_status(pane: NodeId, status: StyledText)` | `setStatus(pane: bigint, status: StyledText)` | `Result<(), PluginError>` | Sets one line of styled text in the footer. |
| `request-focus` | `request_focus(pane: NodeId)` | `requestFocus(pane: bigint)` | `Result<(), PluginError>` | Asks the kernel to focus the pane. |
| `size` | `size(pane: NodeId) -> Option<(u16, u16)>` | `size(pane: bigint): [number, number] \| undefined` | option | Inner columns and rows of the body. `None` for a node that is not a pane of the plugin's kinds. |

The body is drawn by the `views` export. Its Rust trait method is
`fn render(&mut self, view: ViewTarget, area: Area) -> UiTree`, and its TypeScript form is
`render(view, area)`. The host calls it when the view first opens, after `ui.invalidate` for the
pane, and on resize. It is not called for each frame. The render deadline is 10 ms on the client
by default. A render that runs over the deadline traps.

A key the focused widget does not consume arrives as an event for the pane:

| Event | Payload | Delivery |
|---|---|---|
| `key-pressed` | `grab: option<u64>`, `pane: option<node-id>`, `chord`, `repeat` | Only when `wants_keys` is `true`. Never dropped. |
| `key-released` | Same fields as `key-pressed`. | Same rule. |

Rust receives it as `Event::KeyPressed(key)`, and TypeScript as `{ tag: 'key-pressed', val: ... }`.

## Worked example in Rust

The example is a file explorer pane. Its body lists the rows of a folder, its title shows the
folder, and its footer shows the row count. The cursor row is kept in node metadata, so it survives
a restart with `persist: "metadata"`.

**Rust**

```rust
use ilium_plugin_sdk::host::{tree, ui};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::basic;
use ilium_plugin_sdk::types::layout::Area;
use ilium_plugin_sdk::types::ui::{UiTree, ViewTarget};
use ilium_plugin_sdk::ui::build::Tree;

const ROW_COUNT: u32 = 5;

#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
struct TreeExplorer;

impl Lifecycle for TreeExplorer {}

impl Handler for TreeExplorer {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::KeyPressed(key) = event else { continue };
            let Some(node) = key.pane else { continue };
            let cursor = (read_cursor(node) + 1) % ROW_COUNT;
            // Example only: a production plugin handles these errors.
            let _ = write_cursor(node, cursor);
            let _ = basic::set_title(node, "Files: src");
            let _ = basic::set_status(node, vec![Span::plain(&format!("{ROW_COUNT} items"))]);
            let _ = ui::invalidate(ViewTarget::Pane(node));
        }
        Vec::new()
    }
}

impl Views for TreeExplorer {
    fn render(&mut self, view: ViewTarget, _area: Area) -> UiTree {
        // Only pane views are registered for this kind, so any other target is a bug.
        let ViewTarget::Pane(node) = view else { unreachable!("only pane views are registered") };
        let line = format!("Cursor at row {} of {ROW_COUNT}", read_cursor(node) + 1);
        let mut builder = Tree::new();
        let body = builder.paragraph(vec![vec![Span::plain(&line)]]);
        let root = builder.block().title("Files").child(body);
        builder.finish(root)
    }
}

fn read_cursor(node: NodeId) -> u32 {
    tree::metadata(node)
        .and_then(|bytes| <[u8; 4]>::try_from(bytes.as_slice()).ok())
        .map_or(0, u32::from_le_bytes)
}

fn write_cursor(node: NodeId, cursor: u32) -> Result<(), PluginError> {
    tree::set_metadata(node, cursor.to_le_bytes().to_vec())
}
```

The `Lifecycle` implementation uses the default `activate` and `deactivate`, so it needs no body.
The `handler` and `views` exports both run in the client instance of the plugin.

## Worked example in TypeScript

The same pane in TypeScript. The entry module exports the three objects that `definePlugin`
returns: `lifecycle`, `handler` and `views`.

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import * as basic from '@ilium/plugin-sdk/primitives/basic';
import * as treeHost from '@ilium/plugin-sdk/host/tree';
import * as ui from '@ilium/plugin-sdk/host/ui';
import { block, paragraph, span, tree as buildTree } from '@ilium/plugin-sdk/ui/build';

const ROW_COUNT = 5;

function readCursor(node: bigint): number {
  const bytes = treeHost.metadata(node);
  if (!bytes || bytes.length !== 4) return 0;
  return new DataView(bytes.buffer, bytes.byteOffset, 4).getUint32(0, true);
}

function writeCursor(node: bigint, cursor: number): void {
  const bytes = new Uint8Array(4);
  new DataView(bytes.buffer).setUint32(0, cursor, true);
  treeHost.setMetadata(node, bytes);
}

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  activate() {},
  deactivate() { return undefined; },
  onEvents(batch) {
    for (const event of batch) {
      if (event.tag !== 'key-pressed' || event.val.pane === undefined) continue;
      const node = event.val.pane;
      writeCursor(node, (readCursor(node) + 1) % ROW_COUNT);
      basic.setTitle(node, 'Files: src');
      basic.setStatus(node, [span(`${ROW_COUNT} items`)]);
      ui.invalidate({ tag: 'pane', val: node });
    }
    return [];
  },
  render(view) {
    // Only pane views are registered for this kind.
    const node = view.val;
    const line = `Cursor at row ${readCursor(node) + 1} of ${ROW_COUNT}`;
    const body = paragraph([[span(line)]]);
    return buildTree(block({ title: 'Files' }, [body]));
  },
});
```

Both examples keep the state in node metadata. The TypeScript `bigint` node id comes from the
`pane` field of the event, and TypeScript receives the variant `view-target` as `{ tag, val }`.

## What the user sees

The user opens a pane of the kind from the pane menu or from a command. The host draws the frame,
the plugin's title and the footer status. The body is the UI tree the plugin returns. Pressing a
key moves the cursor, and the view redraws without a restart.

<figure>
<pre class="term"><span class="t-hi">╭─ Files: src ───────────────────────────────────────────────────────╮</span>
│ ▸ app/                                                             │
│     lib.rs              1.2 KB                                     │
│     main.rs             0.4 KB                                     │
│ <span class="t-dim">▸ tests/                                                          </span> │
│     smoke.rs            2.9 KB                                     │
│                                                                    │
│ <span class="t-hi">Cursor at row 3 of 5                                              </span> │
<span class="t-dim">├────────────────────────────────────────────────────────────────────┤</span>
│ <span class="t-ok">5 items</span>                                                            │
╰────────────────────────────────────────────────────────────────────╯</pre>
<figcaption>Illustrative, not generated. A basic pane with a body drawn by the plugin and a
footer set by `set-status`. The file names are placeholders.</figcaption>
</figure>

On the review dialog for a T2 or T3 plugin, the grant appears as "Add a pane kind drawn by the
plugin". The grant is remembered once the user accepts it.

## Errors and edge cases

| Situation | Result |
|---|---|
| The grant is declined or revoked | The `pane_kinds` contribution is not registered, so the kind is not offered. The `basic` calls return `not-permitted`. |
| `set-title` on a node that is not a pane of one of the plugin's kinds | `not-permitted`. |
| `set-title` with a title over 256 characters | `invalid-argument`. |
| `set-status`, `request-focus` on a foreign node | `not-permitted`. |
| `size` on a foreign node | `None`. The function has no result, so it has no error code. |
| Grant revoked at run time | A call that is already running when the revoke lands returns `revoked`. A call that starts after the revoke returns `not-permitted`. |
| Basic panes and restore | A basic pane has no process, so `pane-not-ready` never applies to it. Calls work at once after a restore. |
| `views.render` over 10 ms | The plugin traps. Three failures in 10 minutes disable the plugin (see [lifecycle](/pages/lifecycle/)). |
| `wants_keys` is `false` | No `key-pressed` or `key-released` events reach the pane. |

Only the codes listed here are specific to the basic primitive. Every call that has a result can also return
`revoked` and `internal`; `size` returns `None` instead and has no error code.

## First-party features that request it

- Feature 22, file explorer and picker, which provides the `files` kind.
- Feature 24, kanban boards, in Phase 7. In v1 the board is native.
- Feature 21, Markdown source and rendered views, in Phase 7.

## See also

- [ui.panels](/pages/capability-ui-panels/) for the panels and popups a basic pane can open.
- [fs.read](/pages/capability-fs-read/) for the project files the explorer lists.
- [Basic primitive](/pages/primitives-basic/) for the interface of the basic primitive.
- [Tree metadata](/pages/capability-tree-metadata/) for the per-node byte map that holds the state.
- [Storage](/pages/api-storage/) for the storage buckets that `persist: "storage"` uses.
- [Retained mode](/pages/retained-mode/) for the persist rules that apply after a restart.

Checked against the API version `ilium:plugin@0.1.0`.
