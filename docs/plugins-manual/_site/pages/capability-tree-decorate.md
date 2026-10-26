# tree.decorate

The `tree.decorate` grant lets a plugin add small pieces of information to rows in the project tree: a badge, an icon, a subtitle line or a footer line. It also sorts sibling rows with a sort key, and it adds virtual rows such as "Chatroom". A plugin decorates a row; it never draws the tree itself. The host places each decoration in a slot that the manifest declared, draws the row, and reports the outcome of each intent back to the plugin.

## You will need

- The grant `tree.decorate` in the manifest `capabilities` list of the side that sends the intents. The examples below run on the server side.
- A `decoration_slots` entry for each slot the plugin fills. A slot must be declared before the plugin can use it.
- A `virtual_rows` entry for each virtual row kind the plugin fills, and a `commands` entry for each command a row invokes.
- Either the Rust SDK (`ilium_plugin_sdk`) or the TypeScript SDK (`@ilium/plugin-sdk`). Both expose the same three intents.
- [tree.read](/pages/capability-tree-read/) if the plugin chooses the rows to decorate by their content.

## Grant syntax

- Valid: `tree.decorate`
- Invalid: `tree.decorate:badge`. The slot is named in the manifest and in the intent, not in the grant.

## Intents

The grant opens three intents. Each one completes with an outcome, as described in [requests and outcomes](/pages/requests-and-outcomes/).

| Intent | Payload | Outcome |
|---|---|---|
| `set-decoration` | `node: node-id`, `slot: string`, `content: styled-text`, `icon: option<string>`, `sort-key: option<s64>`, `persist: bool` | `ok` |
| `clear-decoration` | `node: node-id`, `slot: string` | `ok` |
| `set-virtual-rows` | `row-kind: string`, `rows: list<virtual-row>`; a `virtual-row` has `id`, `label`, `icon` and `command` | `ok` |

In Rust the payloads are `SetDecorationSpec`, `DecorationKey` and `SetVirtualRowsSpec` (with `VirtualRow`), all in `ilium_plugin_sdk::types::intents`. In TypeScript they are the objects of the same names in camelCase, for example `sortKey` and `rowKind`.

Field rules:

- `content` is a `styled-text`: a list of spans. Build one span with `Span::plain("2")` in Rust or `span('2')` in TypeScript.
- `icon` is a Tabler icon name, such as `checks` or `messages`. It is not a file path.
- `node` is the id from `tree.read` or from an event. A node keeps its id when it is renamed or moved.
- `set-decoration` on a node and slot that already hold a decoration replaces it. It does not add a second one.
- `set-virtual-rows` replaces every row of that kind that the plugin owns. An empty `rows` list removes them all.
- `command` is the global id of one of the plugin's commands, `<plugin-id>/<id>`. When the user activates the row, the plugin receives `command-invoked` for that command.

## Slots and placements

The manifest declares the slots and the parent of each virtual row:

```json
{
  "id": "com.example.review-agent",
  "capabilities": { "server": ["agent.read", "tree.decorate"] },
  "contributes": {
    "decoration_slots": [
      { "id": "review-count", "placement": "badge", "priority": 10 },
      { "id": "branch-line", "placement": "subtitle", "priority": 20 }
    ],
    "virtual_rows": [
      { "id": "chatroom", "title": "Chatroom", "icon": "messages", "parent": "project" }
    ],
    "commands": [{ "id": "open-chatroom", "title": "Open chatroom" }]
  }
}
```

The four placements are:

| Placement | Where it appears | Size limit |
|---|---|---|
| `icon` | Before the title | 2 cells |
| `badge` | On the right of the title line | 12 cells |
| `subtitle` | Under the title | The width of the row |
| `footer` | The bottom line of the row | The width of the row |

Text wider than its limit is cut and ends with `…`. Widths use the host's display-width rules, so a wide glyph counts as two cells. Use the `display-width` function of the host when you need to measure text before you send it.

The `priority` value orders the slots of one placement: a lower number is drawn first, to the left or higher. Use one slot per kind of information, so that a plugin's badges do not replace each other.

<figure>
<svg viewBox="0 0 340 230" role="img" aria-label="Decoration intent passes the kernel check to the tree row">
  <title>Decoration flow</title>
  <desc>A plugin sends set-decoration. The kernel checks the tree.decorate grant and the declared slot, writes the decoration onto the row, and answers with request-completed.</desc>
  <defs>
    <marker id="decorate-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="20" y="10" width="200" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="120" y="34" text-anchor="middle" font-size="11" fill="currentColor">Plugin: review-agent</text>
  <rect x="20" y="95" width="200" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="120" y="119" text-anchor="middle" font-size="10" fill="currentColor">Kernel: grant and slot check</text>
  <rect x="20" y="180" width="200" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="120" y="204" text-anchor="middle" font-size="10" fill="currentColor">Tree row shows the decoration</text>
  <line x1="80" y1="50" x2="80" y2="93" stroke="currentColor" marker-end="url(#decorate-arrow)"/>
  <text x="90" y="76" font-size="10" fill="currentColor">set-decoration</text>
  <line x1="80" y1="135" x2="80" y2="178" stroke="currentColor" marker-end="url(#decorate-arrow)"/>
  <text x="90" y="160" font-size="10" fill="currentColor">applied</text>
  <line x1="200" y1="93" x2="200" y2="52" stroke="currentColor" marker-end="url(#decorate-arrow)"/>
  <text x="208" y="76" font-size="10" fill="currentColor">request-completed</text>
</svg>
<figcaption>A set-decoration intent passes the grant and slot check, then the row shows the decoration. The plugin gets its outcome back as request-completed.</figcaption>
</figure>

## Sorting and virtual rows

A `sort-key` orders a row among its siblings, lowest first. A row with several decorations uses its lowest key. Rows without a key come after the keyed rows, in their usual order. The feature that sorts the tree by cost uses this field.

A virtual row is a row that the plugin owns, not a node of the workspace. The manifest sets its parent to `project` (a row under the project) or `session` (a row under each session). The row kind in `row-kind` must match the `id` of a `virtual_rows` entry.

## Persistence and restarts

`persist: true` stores the decoration in the snapshot, so it survives a restart of the server. After a restart it is shown in the muted colour, marked as stale, until the plugin sets or clears it again after its activation. `persist: false` decorations are dropped at shutdown.

A crash restart keeps every decoration of the plugin, marked as stale. Disabling or uninstalling the plugin removes all of its decorations. Closing a node removes the decorations on that node.

## Risk and tier rules

- This grant is not marked high risk.
- A T1 bundled plugin receives it automatically, up to its manifest ceiling.
- A T2 or T3 plugin needs an explicit grant in the review dialog. The grant is remembered.
- A virtual row kind is declared in the manifest with its parent. Only the declared kinds can be filled.

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.tree 0.1.0 by Example Ltd                                      │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Adds icons and labels to tree rows <span class="t-dim">………………………………………………………</span> tree.decorate │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests tree.decorate, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

## Limits

<div class="note">

A plugin may hold 4,096 decorations on T2 and T3 and 16,384 on the T1 ceiling. All plugins together may hold 65,536 decorations per session. A plugin that writes the same decoration again should use the same slot and node, so that it replaces the old one rather than adding a second.

</div>

## What the user sees

<figure>
<pre class="term"><span class="t-dim">Project tree</span>
▾ api-service
  <span class="t-ok">◉</span> review-agent                              <span class="t-hi">[done]</span>
      feature/parser-rewrite, 3 files changed
      <span class="t-dim">14 min   0.42 EUR</span>
  ▸ Chatroom
</pre>
<figcaption>Illustrative, not generated. One project with an agent row and a virtual row. The icon is before the title, the badge on the right, the subtitle under the title and the footer at the bottom. Exact positions depend on the host layout.</figcaption>
</figure>

A stale decoration looks the same, but in the muted colour. A decoration of a plugin that has been stopped disappears with the plugin.

## Worked example in Rust

This server-side plugin puts a `done` badge on each agent row when the agent finishes. It removes the badge when the agent starts working again. The badge persists across restarts.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::intents::{DecorationKey, SetDecorationSpec};
use ilium_plugin_sdk::types::tree::AgentActivity;

const REVIEW_SLOT: &str = "review-count";

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct ReviewBadge;

impl Handler for ReviewBadge {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::AgentActivityChanged(AgentActivityEvent { node, current, .. })
                = event
            else {
                continue;
            };
            match current {
                AgentActivity::Done => {
                    submissions.push(Submission::new(Intent::SetDecoration(SetDecorationSpec {
                        node,
                        slot: REVIEW_SLOT.into(),
                        content: vec![Span::plain("done")],
                        icon: Some("checks".into()),
                        sort_key: Some(0),
                        persist: true,
                    })));
                }
                AgentActivity::Working => {
                    submissions.push(Submission::new(Intent::ClearDecoration(DecorationKey {
                        node,
                        slot: REVIEW_SLOT.into(),
                    })));
                }
                _ => {}
            }
        }
        submissions
    }
}
```

The `Event` and `Submission` names come from the prelude. Each submission gets the next correlation number when `Submission::new` is called. Use `Submission::keyed(intent, key)` when a repeated submission must not be applied twice.

## Worked example in TypeScript

This server-side plugin declares a "Chatroom" virtual row under the project. The row runs the plugin's `open-chatroom` command. The row is set again on every `tree-changed` batch. Setting the same rows again is safe, because the intent replaces every row of the kind.

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';

const CHATROOM_ROW_KIND = 'chatroom';
const OPEN_CHATROOM_COMMAND = 'com.example.review-agent/open-chatroom';

let nextCorrelation = 1n;

export const { handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'tree-changed') continue;
      submissions.push({
        intent: {
          tag: 'set-virtual-rows',
          val: {
            rowKind: CHATROOM_ROW_KIND,
            rows: [
              {
                id: 'chatroom',
                label: 'Chatroom',
                icon: 'messages',
                command: OPEN_CHATROOM_COMMAND,
              },
            ],
          },
        },
        correlation: nextCorrelation++,
        dedupKey: undefined,
      });
    }
    return submissions;
  },
});
```

The TypeScript intent objects use the `tag` and `val` shape of the event and intent variants. The `correlation` value is a `bigint`, so the counter starts at `1n`.

## Edge cases and errors

| Case | Result |
|---|---|
| Grant declined | Each intent completes `refused(not-permitted)`. Decorations on screen stay, but the plugin cannot change them. |
| Grant revoked while running | Pending calls complete `revoked`. The decorations are withdrawn and the plugin is re-activated with the reduced grants. |
| Slot not declared in the manifest | `refused(invalid-argument)` |
| Row kind not declared in the manifest | `refused(invalid-argument)` from `set-virtual-rows` |
| Node does not exist, or was removed | `refused(not-found)` from `set-decoration` and `clear-decoration` |
| Quota full | `refused(quota-exceeded)`. Clear decorations you no longer need, then retry. |
| `clear-decoration` on a slot with no decoration | `ok`. Nothing changes. |
| Icon name not in the icon set | The icon is not drawn; the content still shows. |
| Text wider than the placement | Cut to the size limit and ends with `…`. |
| Plugin traps or misses its deadline | Its decorations stay on screen, marked stale, until the plugin restarts and sets them again. |

A plugin that cannot decorate should still work through its other features, such as commands and panels.

## See also

- [tree.read](/pages/capability-tree-read/) for choosing the rows to decorate.
- [tree.metadata](/pages/capability-tree-metadata/) for data that the plugin keeps on a row without drawing it.
- [panes.read](/pages/capability-panes-read/) for the pane state that a status badge usually shows.
- [agent.read](/pages/capability-agent-read/) for the agent activity events that drive most badges.
- [Retained mode](/pages/retained-mode/) for how decorations and virtual rows are replicated to the client.
- [Tutorial: agent badge](/pages/tutorial-01-agent-badge/) for a complete first plugin that uses a badge.

Checked against the API version `ilium:plugin@0.1.0`.
