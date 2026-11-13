# Retained mode

## Retained mode

Plugins do not draw on every frame. A plugin describes the state it wants, such as a badge on a tree row, a virtual row in the tree or the enabled state of an agent toolbar button. The kernel stores that state, draws it on every frame and sends it to every attached client. A plugin changes the state only when its meaning changes.

Views work differently. A panel, a pane body or a popup is a widget tree that the plugin returns from `views.render`. The kernel calls that export when the view first opens, after `ui.invalidate` and on resize. It never calls it per frame. This page covers the three kinds of retained output, the intents and calls that set them, how long each one lives and what happens when a plugin fails.

## You will need

- The grants `tree.decorate` (decorations and virtual rows) and `agent.control` (toolbar state). Reading agent activity, as the examples do, needs `agent.read`.
- Declarations in the manifest `contributes` block: `decoration_slots` for each slot, `virtual_rows` for each row kind, `toolbar_buttons` for each button and `commands` for each command a row invokes. The kernel rejects an undeclared slot or row kind.
- The Rust SDK (`ilium_plugin_sdk`) or the TypeScript SDK (`@ilium/plugin-sdk`). The examples run on the server side.
- A working knowledge of [events and intents](/pages/events-and-intents/) and [requests and outcomes](/pages/requests-and-outcomes/). Each intent completes exactly once, with one outcome.

## What is retained

| Output | Set by | Cleared or replaced by | Grant | Where it appears |
|---|---|---|---|---|
| Decoration | `set-decoration` intent | `clear-decoration` intent, removal of the node, disable | `tree.decorate` | A declared slot on a tree row |
| Virtual rows | `set-virtual-rows` intent | Another `set-virtual-rows` call with the same row kind; an empty `rows` list | `tree.decorate` | Rows under the project or session, named by `virtual_rows` |
| Toolbar state | `set-toolbar-state` function of the agent primitive | Another call; a plugin restart resets it to the manifest state | `agent.control` | The toolbar of an agent pane |

Views are not retained. A plugin that changes a view calls `ui.invalidate` and the kernel asks it for a new tree on its next render.

## Surface

### Intents

| Intent | Fields | Outcome |
|---|---|---|
| `set-decoration` | `node: node-id`, `slot: string`, `content: styled-text`, `icon: option<string>`, `sort-key: option<s64>`, `persist: bool` | `ok` |
| `clear-decoration` | `node: node-id`, `slot: string` | `ok`, also on an empty slot |
| `set-virtual-rows` | `row-kind: string`, `rows: list<virtual-row>` | `ok` |

A `virtual-row` has `id: string`, `label: string` (plain text), `icon: option<string>` and `command: option<string>`. The `command` is the global id `<plugin-id>/<id>` of a command the plugin declares. An `icon` is a Tabler icon name, not a file path. An unknown name draws no icon.

`content` is a `styled-text`: a list of `span` records. Each span has `text` and a `style` with `fg`, `bg`, `add` and `remove`. Colours can be theme roles (`foreground`, `muted`, `accent`, `success`, `warning`, `danger`, `info`, `selection`, `border`), so a decoration follows the theme.

In Rust the payloads are `SetDecorationSpec`, `DecorationKey` and `SetVirtualRowsSpec` (with `VirtualRow`) in `ilium_plugin_sdk::types::intents`. In TypeScript the same objects use camelCase fields, such as `sortKey` and `rowKind`.

### Extended decoration

`set-decoration` has optional fields: `spans` (rich text), `tooltip` (`{ lines, delay-ms }`), `frames` (2 to 16 frames) with `frame-ms` (at least 90), `trailing` and `visibility` (`always` or `on-hover`). A decoration with `frames` animates in the host with no plugin call. `set-virtual-rows` rows gain `parent`, `expandable`, `accepts-drop`, `tooltip`, `label` and `trailing`. The intents `set-expanded`, `set-bookmarked` and `set-inferred-icon` change retained node state in the same way. [Tutorial 1](/pages/tutorial-01-agent-badge/) shows the record in Rust and TypeScript.

### Calls

| Call | Rust | TypeScript | Result |
|---|---|---|---|
| Set the state of a toolbar button | `primitives::agent::set_toolbar_state(button, pane, enabled, label)` | `setToolbarState(button, pane, enabled, label)` from `@ilium/plugin-sdk/primitives/agent` | `Result<(), PluginError>` |

`button` is the local id from `toolbar_buttons`. `pane` is `None` (or `none`) for every agent pane that shows the button, or a node id for one pane. `label` is `None` (or `none`) to keep the current label.

### Placements

| Placement | Position on the row | Size limit |
|---|---|---|
| `icon` | Before the title | 2 cells |
| `badge` | Right of the title | 12 cells |
| `subtitle` | Under the title | Width of the row |
| `footer` | Bottom line of the row | Width of the row |

Text wider than the limit is cut and ends with `…`. Widths use the host's display-width rules, so a wide glyph counts as two cells. The `priority` in the manifest orders slots of one placement; a lower number is drawn first. Give each kind of information its own slot, so that two badges do not replace each other.

## Setting a decoration in Rust

The manifest declares the slot and the capability:

```json
{
  "capabilities": { "server": ["agent.read", "tree.decorate"] },
  "contributes": {
    "decoration_slots": [{ "id": "review-count", "placement": "badge", "priority": 10 }]
  }
}
```

The plugin shows a badge while an agent works and clears it when the agent leaves that state:

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::intents::{DecorationKey, SetDecorationSpec};
use ilium_plugin_sdk::types::tree::AgentActivity;

const SLOT: &str = "review-count";

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct ReviewBadge;

impl Lifecycle for ReviewBadge {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        let message = format!("review badge active in {}", context.plugin);
        log::log(Level::Info, &message);
        Ok(())
    }
}

impl Handler for ReviewBadge {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::AgentActivityChanged(change) = event else { continue };
            let intent = if change.current == AgentActivity::Working {
                Intent::SetDecoration(SetDecorationSpec {
                    node: change.node,
                    slot: SLOT.into(),
                    content: vec![Span::plain("reviewing")],
                    icon: Some("checks".into()),
                    sort_key: None,
                    persist: false,
                })
            } else {
                Intent::ClearDecoration(DecorationKey { node: change.node, slot: SLOT.into() })
            };
            submissions.push(Submission::new(intent));
        }
        submissions
    }
}
```

`Submission::new` takes the next correlation number and no dedup key. `Submission::keyed(intent, key)` adds a key. The handler sends one submission per event, and the kernel checks the batch as a whole.

## Virtual rows in TypeScript

The manifest declares the row kind and the command that the row invokes:

```json
{
  "contributes": {
    "virtual_rows": [{ "id": "review-queue", "title": "Reviews", "icon": "checks", "parent": "project" }],
    "commands": [{ "id": "open-review", "title": "Open review" }]
  }
}
```

The plugin keeps the finished panes in memory and sends the whole list each time it changes. Each `set-virtual-rows` call replaces every row of that kind:

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';

const ROW_KIND = 'review-queue';
const OPEN_REVIEW = 'com.example.review-tools/open-review';
const finishedPanes = new Map<bigint, string>();
let nextCorrelation = 1n;

function rowsSubmission(): Submission {
  const rows = [...finishedPanes].map(([node, label]) => ({
    id: String(node), label, icon: 'checks', command: OPEN_REVIEW,
  }));
  return {
    intent: { tag: 'set-virtual-rows', val: { rowKind: ROW_KIND, rows } },
    correlation: nextCorrelation++,
    dedupKey: undefined,
  };
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate(context) {
    log('info', `review queue active for ${context.plugin}`);
  },
  onEvents(batch: Event[]): Submission[] {
    let changed = false;
    for (const event of batch) {
      if (event.tag === 'agent-activity-changed') {
        const { node, current } = event.val;
        if (current === 'done') finishedPanes.set(node, `Review ready in pane ${node}`);
        else finishedPanes.delete(node);
        changed = true;
      } else if (event.tag === 'node-removed' && finishedPanes.delete(event.val)) {
        changed = true;
      }
    }
    return changed ? [rowsSubmission()] : [];
  },
});
```

The `node-removed` branch matters. Without it the row would keep pointing at a pane that no longer exists.

## Toolbar state

Toolbar state is a direct call, not an intent. The SDK returns its result at once, because the function has no request id. This sketch disables a button named `review` on one pane while that pane has no review to run:

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::agent;

fn set_review_button(pane: u64, has_review: bool) -> Result<(), PluginError> {
    agent::set_toolbar_state("review", Some(pane), has_review, Some("Review now".into()))
}
```

The state lasts until the plugin changes it. A label over 32 cells is refused.

## What you see

The tree row below is the result of the Rust example above. Only the row whose agent changed is different.

<figure>
<pre class="term">╭  ≡ ● · Sessions ───────────────────────────────╮
│ ▼ 📂 shop                                      │
│ › ▼ 📂 api                                     │
│ ››   🦀 <span class="t-ok">⠋</span> Fix login redirect <span class="t-warn">reviewing</span>         │
│ ››   🖥️    Shell                               │
│                                                │
╰────────────────────────────────────────────────╯
</pre>
<figcaption>Illustrative. The badge slot shows the text of the decoration in the warning colour, after the title.</figcaption>
</figure>

After a server restart a persisted decoration is shown in the muted colour until the plugin sets or clears it again:

<figure>
<pre class="term">╭  ≡ ● · Sessions ───────────────────────────────╮
│ ▼ 📂 shop                                      │
│ › ▼ 📂 api                                     │
│ ››   🦀 <span class="t-ok">⠋</span> Fix login redirect <span class="t-dim">reviewing</span>         │
│ ››   🖥️    Shell                               │
│                                                │
╰────────────────────────────────────────────────╯
</pre>
<figcaption>Illustrative. The muted badge marks a decoration that the kernel kept from the previous run.</figcaption>
</figure>

The virtual row appears under the project as a separate entry:

<figure>
<pre class="term">╭  ≡ ● · Sessions ───────────────────────────────╮
│ ▼ 📂 shop                                      │
│ ››   🖥️    Shell                               │
│ ▼ <span class="t-hi">Reviews</span>                                      │
│ ›   Review ready in pane 42 <span class="t-ok">open</span>               │
│                                                │
╰────────────────────────────────────────────────╯
</pre>
<figcaption>Illustrative. Activating the row invokes the command named in the row.</figcaption>
</figure>

The diagram shows the path of one decoration from the plugin to the clients:

<figure>
<svg viewBox="0 0 400 200" role="img" aria-label="Decoration path from plugin to clients">
  <title>Retained decoration state</title>
  <desc>The plugin sends a set-decoration intent to the kernel. The kernel keeps the state and sends it to two clients, which draw it on every frame.</desc>
  <defs>
    <marker id="retained-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="8" y="20" width="130" height="48" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="73" y="40" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
  <text x="73" y="56" font-size="10" text-anchor="middle" fill="currentColor">sends set-decoration</text>
  <line x1="73" y1="68" x2="73" y2="110" stroke="currentColor" marker-end="url(#retained-arrow)"/>
  <text x="82" y="92" font-size="10" fill="currentColor">intent</text>
  <rect x="8" y="110" width="170" height="56" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="93" y="130" font-size="12" text-anchor="middle" fill="currentColor">Kernel</text>
  <text x="93" y="150" font-size="10" text-anchor="middle" fill="currentColor">retained state per node</text>
  <line x1="178" y1="128" x2="240" y2="44" stroke="currentColor" marker-end="url(#retained-arrow)"/>
  <text x="210" y="100" font-size="10" fill="currentColor">delta</text>
  <line x1="178" y1="150" x2="240" y2="154" stroke="currentColor" marker-end="url(#retained-arrow)"/>
  <rect x="240" y="20" width="150" height="48" rx="6" fill="none" stroke="currentColor"/>
  <text x="315" y="40" font-size="12" text-anchor="middle" fill="currentColor">Client A</text>
  <text x="315" y="56" font-size="10" text-anchor="middle" fill="currentColor">draws each frame</text>
  <rect x="240" y="130" width="150" height="48" rx="6" fill="none" stroke="currentColor"/>
  <text x="315" y="150" font-size="12" text-anchor="middle" fill="currentColor">Client B</text>
  <text x="315" y="166" font-size="10" text-anchor="middle" fill="currentColor">draws each frame</text>
</svg>
<figcaption>The plugin changes retained state in the kernel. The kernel sends each change to every client, and each client draws it.</figcaption>
</figure>

## Edge cases and errors

| Case | Result |
|---|---|
| `set-decoration` or `clear-decoration` on a node that does not exist | `refused(not-found)` |
| Slot not declared in `decoration_slots` | `refused(invalid-argument)` |
| More than 4,096 decorations for one plugin at the default limit | `refused(quota-exceeded)` |
| `set-virtual-rows` with a row kind not declared in `virtual_rows` | `refused(invalid-argument)` |
| `clear-decoration` on an empty slot | `ok` |
| `set-decoration` on a slot that already holds a decoration | Replaces it; the slot never holds two |
| Toolbar call for a button that is not the plugin's own, or for a missing pane | `not-found` |
| Toolbar label over 32 cells | `invalid-argument` |
| A malformed submission in a batch | The whole batch is refused, so nothing is half-applied |
| Repeat of a submission with the same `dedup-key` within 10 minutes | `duplicate(original)`, with no effect |
| Node removed | Its decorations and its rows are removed with it |

Wide content is cut, as described under placements. Limits apply per plugin, and the host limit for all plugins together is 65,536 decorations.

## Lifetime and failure

- `persist: true` stores the decoration in the snapshot. After a server restart it is shown in the muted colour, and it stays that way until the plugin sets or clears it after its activation.
- `persist: false` decorations are dropped at server shutdown.
- A trap, a deadline breach or a crash keeps the plugin's decorations, marked stale in the muted colour. The plugin's virtual rows are withdrawn until it sets them again. The toolbar state returns to the manifest state on restart.
- Disable and uninstall remove all of the plugin's retained state.
- A revoked `tree.decorate` or `agent.control` grant withdraws the matching output.

A plugin that fails three times in 10 minutes is disabled, and the tree footer shows a notice. See [Lifecycle](/pages/lifecycle/).

## Rules for retained state

- Set a decoration when its meaning changes, not on every event. A repeated intent costs one submission each time and changes nothing on screen.
- Keep slot ids, row kinds and row ids stable across plugin versions. The kernel stores them as strings in the snapshot.
- Give an intent a `dedup-key` when a repeat would be harmful, for example a notice that must not appear twice.
- Send the complete row list on every `set-virtual-rows` call. The call replaces the rows of its kind; it does not add to them.

## See also

- [The UI model](/pages/ui-model/)
- [Events and intents](/pages/events-and-intents/)
- [Requests and outcomes](/pages/requests-and-outcomes/)
- [Capability: tree.decorate](/pages/capability-tree-decorate/)
- [Capability: agent.control](/pages/capability-agent-control/)
- [Lifecycle](/pages/lifecycle/)

API version checked: `ilium:plugin@0.1.0`.
