---
layout: page
title: "tree.metadata"
permalink: /pages/capability-tree-metadata/
---

## Overview

The `tree.metadata` grant lets a plugin keep its own bytes on a workspace tree node. Each plugin
has a separate byte map on each node, so no plugin can read or write another plugin's entry
through this grant. The host saves the maps in the session snapshot and restores them when the
server starts. A node keeps its metadata when it is moved or renamed, and the host removes the
metadata when the node closes.

Use metadata for small state that belongs to one node: a bookmark flag, a schedule, a position in
a queue. Use [storage](/pages/api-storage/) for anything larger, or for state that is not tied to a
node.

## You will need

- The grant `tree.metadata` on each side that calls the functions. Grants are per side, so a
  server-side call needs a server grant and a client-side call needs a client grant.
- The Rust SDK `ilium-plugin-sdk` or the TypeScript SDK `@ilium/plugin-sdk`, both at the versions
  pinned for the release. Plugins built with either SDK use the same imports.
- The grant [tree.read](/pages/capability-tree-read/) when the plugin also needs to find the nodes
  that carry metadata, for example with `find`.

## Grant syntax and risk

- Valid: `tree.metadata`.
- Invalid: `tree.metadata:own`. The grant takes no argument. The manifest fails with
  `manifest.grant-argument` and the text `capability "tree.metadata" takes no argument (got
  "tree.metadata:own")`.
- Risk: `normal`. The grant is not marked high risk, so it is never off by default.
- A T1 bundled plugin receives the grant automatically, up to its manifest ceiling.
- A T2 or T3 plugin lists the grant in the install review dialog, checked by default. The user
  confirms the install. The choice is recorded per plugin and per side.

## Surface

| Function | Rust | TypeScript | Returns | Meaning |
|---|---|---|---|---|
| `metadata` | `tree::metadata(node: NodeId) -> Option<Vec<u8>>` | `metadata(node: bigint): Uint8Array \| undefined` | option | The plugin's own bytes for the node. `None` or `undefined` when nothing is stored. |
| `set-metadata` | `tree::set_metadata(node: NodeId, bytes: Vec<u8>) -> Result<(), PluginError>` | `setMetadata(node: bigint, bytes: Uint8Array): void` | result | Replaces the plugin's bytes for the node. Throws `PluginError` in TypeScript. |

Both functions live in the `tree` interface, so the Rust module is `ilium_plugin_sdk::host::tree`
and the TypeScript import is `@ilium/plugin-sdk/host/tree`. The WIT signatures are:

```wit
// Excerpt of ilium:host/tree in reference-wit
// ilium:host/tree
metadata: func(node: node-id) -> option<list<u8>>;                  // tree.metadata
set-metadata: func(node: node-id, bytes: list<u8>) -> result<_, plugin-error>;  // tree.metadata
```

`node-id` is a `u64`. In TypeScript it is a `bigint`, including every node id that comes from an
event. `list<u8>` is `Vec<u8>` in Rust and `Uint8Array` in TypeScript.

An empty list clears the entry. The host has no delete call. After an empty write, `metadata`
returns an empty list, not `none`. Treat an empty list and `none` as "no value" in your code.

### Size limits

The limit is per node, per plugin, and it is the manifest key `node_metadata_kib`:

| Tier | Default | Ceiling | Bytes at the ceiling |
|---|---|---|---|
| T1 bundled | 64 KiB | 256 KiB | 262,144 |
| T2 and T3 | 64 KiB | 64 KiB | 65,536 |

A manifest request above the ceiling is clamped and logged, never refused. The user can set the
same key under `[plugins.limits]` in `config.toml`.

Metadata bytes are opaque to the host. The host never reads them, so a plugin that stores
structured data must choose its own format and write a version byte first. A later version of
your plugin can then tell old bytes from new ones.

<figure>
<svg viewBox="0 0 360 200" role="img" aria-label="Metadata flows from the plugin into the host map and then into the snapshot file">
<title>Where node metadata lives</title>
<desc>A plugin writes bytes to the host map keyed by node and plugin id. The host saves the map in the snapshot file and restores it at boot. Another plugin cannot read the entry.</desc>
<defs>
<marker id="arr-tree-metadata" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
<path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="8" y="30" width="104" height="54" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="60" y="52" text-anchor="middle" font-size="12" fill="currentColor">Plugin</text>
<text x="60" y="70" text-anchor="middle" font-size="11" fill="currentColor">set-metadata</text>
<rect x="128" y="30" width="104" height="54" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="180" y="52" text-anchor="middle" font-size="12" fill="currentColor">Host map</text>
<text x="180" y="70" text-anchor="middle" font-size="11" fill="currentColor">node + plugin id</text>
<rect x="128" y="132" width="104" height="54" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="180" y="154" text-anchor="middle" font-size="12" fill="currentColor">Snapshot</text>
<text x="180" y="172" text-anchor="middle" font-size="11" fill="currentColor">saved to disk</text>
<rect x="248" y="30" width="104" height="54" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="300" y="52" text-anchor="middle" font-size="12" fill="currentColor">Other plugin</text>
<text x="300" y="70" text-anchor="middle" font-size="11" fill="currentColor">no entry</text>
<line x1="112" y1="57" x2="126" y2="57" stroke="currentColor" marker-end="url(#arr-tree-metadata)"/>
<line x1="248" y1="57" x2="234" y2="57" stroke="currentColor" stroke-dasharray="3 3" marker-end="url(#arr-tree-metadata)"/>
<line x1="160" y1="86" x2="160" y2="128" stroke="currentColor" marker-end="url(#arr-tree-metadata)"/>
<line x1="200" y1="128" x2="200" y2="88" stroke="currentColor" marker-end="url(#arr-tree-metadata)"/>
<text x="166" y="112" font-size="11" fill="currentColor">save</text>
<text x="206" y="112" font-size="11" fill="currentColor">restore</text>
</svg>
<figcaption>The host keeps one entry for each node and plugin. The snapshot holds the same entries, and the other plugin has no path to them.</figcaption>
</figure>

## Rules that affect your code

- **Client and server share one map.** Both sides of a plugin read and write the same map for
  each node. The server holds it and replicates it to the client. A value written on one side is
  visible on the other.
- **Revision and events do not change.** `set-metadata` does not change the node's `revision` and
  emits no `tree-changed` event. A plugin that caches metadata must update its cache from its own
  writes, or re-read the value when it needs it.
- **Restore at boot.** Metadata is part of the snapshot. After a server restart, `metadata` returns
  the saved bytes before the plugin writes anything new.
- **No cross-plugin access.** Another plugin's entry is invisible. The grant gives no way to list
  the keys of other plugins.

## Worked example, Rust (server side)

The manifest requests the grant on the server side and caps the size at 16 KiB, which is well
under the T2 ceiling. The module stores a bookmark flag on a node.

```json
{
  "capabilities": {
    "server": ["tree.metadata", "tree.read"]
  },
  "limits": {
    "node_metadata_kib": 16
  }
}
```

**Rust**

```rust
use ilium_plugin_sdk::host::tree;
use ilium_plugin_sdk::prelude::*;

/// Version byte written first, so a later format can be told apart from this one.
const BOOKMARK_FORMAT_VERSION: u8 = 1;
const BOOKMARKED_FLAG: u8 = 1;

/// True when the node carries the bookmark. Absent metadata, a cleared flag and a declined
/// grant (which returns `None`) all read as "not bookmarked".
pub fn is_bookmarked(node: NodeId) -> bool {
    tree::metadata(node).is_some_and(|bytes| bytes == [BOOKMARK_FORMAT_VERSION, BOOKMARKED_FLAG])
}

/// Sets or clears the bookmark. Clearing writes an empty list, which leaves an empty entry.
pub fn set_bookmark(node: NodeId, bookmarked: bool) -> Result<(), PluginError> {
    let bytes = if bookmarked {
        vec![BOOKMARK_FORMAT_VERSION, BOOKMARKED_FLAG]
    } else {
        Vec::new()
    };
    tree::set_metadata(node, bytes)
}

/// Flips the bookmark and returns the new state. Errors come back to the caller with `?`.
pub fn toggle_bookmark(node: NodeId) -> Result<bool, PluginError> {
    let now_bookmarked = !is_bookmarked(node);
    set_bookmark(node, now_bookmarked)?;
    Ok(now_bookmarked)
}
```

Call `toggle_bookmark` from the handler that reacts to the user's bookmark command. The module
needs no export of its own. It is ordinary code called from the `Handler` implementation.

## Worked example, TypeScript (client side)

The manifest requests the grant on the client side. The helpers use the same byte format as the
Rust example, so a server and a client can read each other's flag.

```json
{
  "capabilities": {
    "client": ["tree.metadata", "tree.read"]
  }
}
```

**TypeScript**

```ts
import { metadata, setMetadata } from '@ilium/plugin-sdk/host/tree';

/** Version byte written first, matching the Rust example so both sides read one format. */
const BOOKMARK_FORMAT_VERSION = 1;
const BOOKMARKED_FLAG = 1;

export function isBookmarked(node: bigint): boolean {
  const bytes = metadata(node);
  return bytes !== undefined
    && bytes.length === 2
    && bytes[0] === BOOKMARK_FORMAT_VERSION
    && bytes[1] === BOOKMARKED_FLAG;
}

/** Sets or clears the flag. Throws `PluginError` when the host refuses the write. */
export function setBookmark(node: bigint, bookmarked: boolean): void {
  const bytes = bookmarked
    ? new Uint8Array([BOOKMARK_FORMAT_VERSION, BOOKMARKED_FLAG])
    : new Uint8Array();
  setMetadata(node, bytes);
}
```

Node ids arrive as `bigint` values in the event payloads, so pass them through unchanged.

## Edge cases and errors

| Situation | Result |
|---|---|
| `set-metadata` with more bytes than the per-node limit | `quota-exceeded`. Nothing is written. The old value stays. |
| Node id not in the tree | `metadata` returns `none`. `set-metadata` returns `not-found`. |
| Node removed during the `set-metadata` call | `not-found`. |
| Node moved or renamed | Metadata moves with the node, and the node keeps its id. |
| Node closed | The host removes its metadata with it. There is no delete call. |
| Grant declined at install | `set-metadata` returns `not-permitted`. `metadata` returns `none`. |
| Grant revoked while running | Calls in flight return `revoked`. The plugin is re-activated without the grant and then behaves as declined. |
| Host bug or trap inside the call | `internal`. |

The `not-permitted` rule follows the rule for `api-location`. The `metadata` function returns an
option, so it has no error code, and a declined grant makes it return `none`.

A plugin that stores per-node state without the grant must keep it in its own storage bucket,
keyed by node id. It must also remove those keys when it receives `node-removed` for that node.

<div class="note">
<p>Metadata is saved with the session snapshot, which lives in the project's `.ilium/sessions/`
folder. Anyone who can read that folder can read the bytes, so do not store secrets there.</p>
</div>

<div class="warn">
<p>Do not treat a missing entry as an error. A new node, a cleared flag and a declined grant all
read as "no value". Code that throws on `none` breaks as soon as the user declines the grant.</p>
</div>

## What the user sees

The install review dialog lists the grant for the side that requests it. Settings > Plugins >
[plugin] > Capabilities shows the same grant later, and `ilium plugin grants <id> --revoke
tree.metadata` removes it.

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.tree 0.1.0 by Example Ltd                                      │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Stores notes on tree nodes <span class="t-dim">……………………………………………………………………………</span> tree.metadata │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests tree.metadata, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

After the user declines the grant on one side, the grants listing shows it as declined, and that
side's calls return the values described above:

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin grants example-bookmarks
<span class="t-ok">ok</span>   server  tree.metadata  granted   normal
<span class="t-warn">warn</span> client  tree.metadata  declined  set-metadata returns not-permitted
</pre>
<figcaption>Illustrative, not generated. The grants listing after the user declined the client
side. The layout of the command output is simplified.</figcaption>
</figure>

## First-party features that use it

- Feature 6, bookmarks and locks, stores the bookmark flag in metadata.
- Feature 55, scheduled input, keeps the schedule in node metadata.
- Feature 57, prompt queue, keeps the queue in node metadata.
- Pane kinds with `persist: "metadata"` keep their state in node metadata. See
  [panes.provide-kind](/pages/capability-panes-provide-kind/).

## See also

- [tree.read](/pages/capability-tree-read/) to find the nodes that carry metadata.
- [tree.write](/pages/capability-tree-write/) to move, rename or close the nodes.
- [tree.decorate](/pages/capability-tree-decorate/) to show a flag next to the node.
- [api-tree](/pages/api-tree/) for the full `tree` interface.
- [api-storage](/pages/api-storage/) for data that is not tied to a node.
- [terminal.write](/pages/capability-terminal-write/) for the input that scheduled and queued work
  sends.

Checked against the API version `ilium:plugin@0.1.0`.
