---
layout: page
title: "panes.read"
permalink: /pages/capability-panes-read/
---

The `panes.read` grant lets a plugin see the panes that exist: their size, title, kind, focus and whether a restored pane is still waiting for its process. It also delivers the pane lifecycle events, so a plugin learns when a pane is created, exits, gains focus, resizes or changes its title. The grant reads pane state only. It does not type into a pane, start one or stop one.

Two calls read state on demand: `info` returns the record of one pane, and `focused` returns the focused pane. Events report changes as they happen. A plugin should react to the events and call `info` only when it needs a pane's current record. It should not call the functions in a loop.

<figure>
<svg viewBox="0 0 360 170" role="img" aria-label="A plugin asks the kernel for pane state and receives pane events">
  <defs><marker id="arr-panes-read" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker></defs>
  <rect x="8" y="30" width="100" height="110" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="58" y="88" text-anchor="middle" fill="currentColor" font-size="12">Plugin</text>
  <text x="58" y="104" text-anchor="middle" fill="currentColor" font-size="10">needs panes.read</text>
  <rect x="252" y="30" width="100" height="110" rx="6" fill="none" stroke="currentColor"/>
  <text x="302" y="88" text-anchor="middle" fill="currentColor" font-size="12">Pane state</text>
  <text x="302" y="104" text-anchor="middle" fill="currentColor" font-size="10">server or client</text>
  <line x1="108" y1="60" x2="252" y2="60" stroke="currentColor" marker-end="url(#arr-panes-read)"/>
  <text x="180" y="54" text-anchor="middle" fill="currentColor" font-size="11">info(pane), focused()</text>
  <line x1="252" y1="100" x2="108" y2="100" stroke="currentColor" marker-end="url(#arr-panes-read)"/>
  <text x="180" y="94" text-anchor="middle" fill="currentColor" font-size="11">pane-info, node id</text>
  <line x1="252" y1="128" x2="108" y2="128" stroke="currentColor" marker-end="url(#arr-panes-read)"/>
  <text x="180" y="122" text-anchor="middle" fill="currentColor" font-size="11">pane events</text>
</svg>
<figcaption>Pane state reaches the plugin in two ways: info and focused return pane records on request, and pane events such as pane-exited arrive as they happen. The grant does not start or type into a pane.</figcaption>
</figure>

## You will need

- The grant `panes.read` on each side that calls the functions or receives the events. A server-side plugin lists it under `capabilities.server`, and a client-side plugin under `capabilities.client`.
- A Rust plugin built with `ilium-plugin-sdk` 0.1 or a TypeScript plugin built with `@ilium/plugin-sdk`. Both need the toolchain of the SDK release (Rust 1.90 or later, or Bun 1.2 or later).
- [tree.read](/pages/capability-tree-read/) if the plugin also needs the node that holds each pane, for example to walk from a pane to its folder.

## Grant syntax

- Valid: `panes.read`
- Invalid: `panes.read:terminal`. The grant takes no argument. The primitive kind is part of the pane information the plugin reads, not of the grant.

## Functions

Both functions are synchronous. They return at once, without a request id, and they have no error code (see the edge cases below).

| Function | Rust | TypeScript | Returns | Meaning |
|---|---|---|---|---|
| `info(pane)` | `panes::info(node)` | `info(node)` | `option<pane-info>` | The record of one pane. `none` for a node that is not a pane or no longer exists. |
| `focused()` | `panes::focused()` | `focused()` | `option<node-id>` | The focused pane, or `none` when no pane has focus. |

Rust names come from `ilium_plugin_sdk::host::panes`. TypeScript names come from `@ilium/plugin-sdk/host/panes`. A node id is a `u64` in Rust and a `bigint` in TypeScript.

## Records and types

`pane-info` is a record in `ilium:types/tree`. Its fields are:

| Field | WIT type | Rust | TypeScript | Meaning |
|---|---|---|---|---|
| `node` | `node-id` | `NodeId` | `bigint` | The tree node of the pane. |
| `content-kind` | `string` | `String` | `string` | `ilium.terminal`, `ilium.editor`, or `<plugin-id>.<kind>` for a pane kind a plugin provides. |
| `primitive` | `primitive-kind` | `PrimitiveKind` | `'terminal' \| 'editor' \| 'basic'` | The kind of body the pane has. |
| `columns` | `u16` | `u16` | `number` | Inner width in cells. |
| `rows` | `u16` | `u16` | `number` | Inner height in cells. |
| `focused` | `bool` | `bool` | `boolean` | The pane has focus. |
| `title` | `string` | `String` | `string` | The current title. |
| `is-restored` | `bool` | `bool` | `boolean` | A restored pane is waiting for its process to start. |

The WIT declarations, with the grant on each line:

```wit
// ilium:types/tree
record pane-info {
    node: node-id, content-kind: string, primitive: primitive-kind,
    columns: u16, rows: u16, focused: bool, title: string, is-restored: bool,
}
enum primitive-kind { terminal, editor, basic }

// ilium:host/panes (panes.read)
info: func(pane: node-id) -> option<pane-info>;
focused: func() -> option<node-id>;
```

## Events

The grant opens five events. Each arrives in the plugin's batch, in kernel order, through `on-events`.

| Event | Payload | Delivery | Rust variant | TypeScript tag and value |
|---|---|---|---|---|
| `pane-created` | `pane-info` | never dropped | `Event::PaneCreated(PaneInfo)` | `'pane-created'`, a `PaneInfo` |
| `pane-exited` | `node`, `status`, `signal` | never dropped | `Event::PaneExited(PaneExitedEvent)` | `'pane-exited'`, `{ node, status, signal }` |
| `pane-focused` | `option<node-id>` | coalesced | `Event::PaneFocused(Option<NodeId>)` | `'pane-focused'`, `bigint \| undefined` |
| `pane-resized` | `node`, `columns`, `rows` | coalesced by node | `Event::PaneResized(PaneResizedEvent)` | `'pane-resized'`, `{ node, columns, rows }` |
| `pane-title-changed` | `node`, `title`, `source` | coalesced by node | `Event::PaneTitleChanged(PaneTitleChangedEvent)` | `'pane-title-changed'`, `{ node, title, source }` |

- `status` is `option<s32>`: the exit code of the process. `signal` is `option<string>`: the name of the signal that ended it, such as `SIGKILL`. For a terminal pane whose end the kernel recorded, exactly one of the two is set. Editor and basic panes have no process, so they never send `pane-exited`.
- `coalesced` means that when several events of the same kind reach the queue before the plugin runs, only the newest is delivered. `pane-exited` and `pane-created` are never merged.
- `source` is `osc` (the program set the title with an escape sequence), `manual` (the user renamed the pane) or `inferred` (the kernel chose the title from the program name). Rust `TitleOrigin::{Osc, Manual, Inferred}`; TypeScript `'osc' | 'manual' | 'inferred'`.
- A restored pane reports its saved size until the client lays it out. Then `pane-resized` arrives with the real size. A size is never zero.

## Worked example: Rust, server side

This plugin logs every pane exit with the title and the way the process ended. It needs only `panes.read`.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::panes;
use ilium_plugin_sdk::types::events::PaneExitedEvent;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct ExitWatch;

impl Handler for ExitWatch {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::PaneExited(PaneExitedEvent { node, status, signal }) = event else {
                continue;
            };
            let title = panes::info(node)
                .map(|info| info.title)
                .unwrap_or_else(|| String::from("(closed)"));
            let outcome = match (status, signal) {
                (Some(code), _) => format!("exit status {code}"),
                (None, Some(name)) => format!("killed by {name}"),
                (None, None) => String::from("ended, status unknown"),
            };
            log::log(Level::Info, &format!("pane {node} \"{title}\" {outcome}"));
        }
        Vec::new()
    }
}
```

The manifest excerpt for this plugin:

```json
{
  "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": [] },
  "capabilities": { "server": ["panes.read"] }
}
```

## Worked example: TypeScript, client side

This client plugin logs the focused pane each time focus moves. It also handles the case where no pane has focus.

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { info } from '@ilium/plugin-sdk/host/panes';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate(context) {
    log('info', `focus watch started in ${context.plugin}`);
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'pane-focused') continue;
      const node = event.val;
      if (node === undefined) {
        log('info', 'no pane has focus');
        continue;
      }
      const pane = info(node);
      if (pane === undefined) continue;
      log('info', `focus on "${pane.title}" (${pane.primitive}, ${pane.columns}x${pane.rows})`);
    }
    return [];
  },
});
```

## What the user sees

The grant changes no pane. The user sees the plugin's log lines with `ilium plugin logs`:

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs exit-watch
<span class="t-dim">2026-10-11 14:02:07</span> <span class="t-ok">info</span>  exit-watch pane 42 "review-agent" exit status 0
<span class="t-dim">2026-10-11 14:05:31</span> <span class="t-ok">info</span>  exit-watch pane 57 "build-agent" killed by SIGKILL
<span class="t-dim">2026-10-11 14:09:12</span> <span class="t-ok">info</span>  exit-watch pane 61 "(closed)" ended, status unknown
<span class="t-dim">2026-10-11 14:11:40</span> <span class="t-warn">warn</span>  exit-watch panes.read declined, pane events are off
</pre>
<figcaption>Illustrative, not generated. A plugin log for the Rust example, with one warn line from a session where the grant was declined.</figcaption>
</figure>

## Risk and tier rules

- This grant is not marked high risk.
- A T1 bundled plugin receives it automatically up to its manifest ceiling.
- A T2 or T3 plugin needs an explicit grant in the review dialog, and the grant is remembered.
- The grant does not allow starting, stopping or typing into a pane. Those need `panes.create`, `panes.signal` or `terminal.write`.

## What the review dialog says

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.panes 0.1.0 by Example Ltd                                     │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Reads pane information <span class="t-dim">………………………………………………………………………………………………</span> panes.read │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests panes.read, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

## Edge cases and errors

- **Grant declined or revoked.** `info` and `focused` return `none`, because their signatures have no error channel. The first refused call of each plugin instance writes one warn line to the plugin log, naming `panes.read` and the reason (`declined`). Pane events are not delivered. A grant revoked while the plugin runs stops events from the next batch on.
- **Node that is not a pane.** `info` returns `none` for a folder, a container, a removed node or an unknown id.
- **Exited pane.** A pane node stays in the tree after its process exits, until it is closed. `info` still returns it, with its last title and size. `is-restored` is `false`.
- **Restored pane.** `is-restored` is `true` while a restored pane waits for its first process start. Input to such a pane is refused with `pane-not-ready` by the sending grant, such as `terminal.write`. Re-check `is-restored` before each write.
- **New pane.** `pane-created` arrives before any other event for that node, and before its process starts.
- **Error codes.** No function in this grant returns a code. The universal codes of the error table do not apply, because the calls have no result.
- **Event overflow.** Pane events are never dropped. A plugin whose queue overflows is treated as stuck and restarted. The restart counts toward the disable rule (after three failures in ten minutes the plugin is disabled). The new instance re-reads state with `info` and `focused`.

## First-party features that request it

- Feature 17, screen transfer between panes.
- Feature 18, pane title from the terminal.
- Feature 47, sound alerts, through `pane-exited`.
- Feature 109, the `ilium panes` command.

## See also

- [api-panes](/pages/api-panes/) for the full `panes` import and its record.
- [Events](/pages/api-events/) for the event catalogue and delivery rules.
- [panes.create](/pages/capability-panes-create/) for starting panes.
- [panes.signal](/pages/capability-panes-signal/) for stopping them.
- [terminal.read-screen](/pages/capability-terminal-read-screen/) for what is shown on a terminal pane.
- [errors-and-limits](/pages/reference-errors-and-limits/) for the error codes and limits shared by all grants.

Checked against the API version `ilium:plugin@0.1.0`.
