# Tree and window chrome

## Tree and window chrome

Ilium draws a tree of nodes on the left, a status bar at the bottom and a frame around every pane. The contract lets a plugin add to each of these places without drawing them itself. A plugin declares a place in its manifest, then fills it with intents. The host draws the result with the same rounded borders, icon slots and colours as its own rows, so a plugin row cannot be told apart from a built-in one except by its content.

This page covers eight places: decorations on tree rows (including trailing and hover-only ones), tree toolbar buttons, hover actions on rows, tree orderings, display modes, virtual rows (including lazy children and drops), status bar segments and pane frame slots. Reading the tree is on [tree](/pages/api-tree/). Pane lifecycle intents are on [pane lifecycle](/pages/api-pane-lifecycle/).

**You will need:**

- `tree.decorate` for decorations, virtual rows, `tree_orderings` and `display_modes`.
- `ui.chrome` for `tree_toolbar`, `row_actions`, `status_segments` and `pane_frame_slots`, and for the intents `set-status-segment`, `clear-status-segment`, `set-frame-slot` and `clear-frame-slot`.
- `tree.write` for `set-expanded` on real nodes.
- Both worlds: the server side owns decorations and virtual rows, the client side draws them. Intents can be submitted from either side. See [worlds and sides](/pages/worlds-and-sides/).
- Grants are explained in [tree.decorate](/pages/capability-tree-decorate/), [tree.write](/pages/capability-tree-write/) and [ui.panels](/pages/capability-ui-panels/). The grant `ui.chrome` is listed in the [capability reference](/pages/reference-capabilities/).

### Where each place is

<figure>
<svg viewBox="0 0 420 230" role="img" aria-label="Layout of the tree sidebar, a pane frame and the status bar with the places a plugin can fill">
  <title>Places in the window that a plugin can fill</title>
  <desc>A sidebar with a toolbar on top, rows with decorations, hover actions and trailing text; a pane frame with header icon, header text, banner and footer row; a status bar with two segments.</desc>
  <rect x="6" y="6" width="150" height="200" rx="8" fill="none" stroke="currentColor"/>
  <text x="14" y="24" font-size="11" fill="currentColor">tree_toolbar</text>
  <line x1="6" y1="32" x2="156" y2="32" stroke="currentColor" stroke-dasharray="3 3"/>
  <text x="14" y="52" font-size="11" fill="currentColor">row: icon, title, badge</text>
  <text x="14" y="72" font-size="11" fill="currentColor">row: hover strip (row_actions)</text>
  <text x="14" y="92" font-size="11" fill="currentColor">row: trailing slot</text>
  <text x="14" y="112" font-size="11" fill="currentColor">virtual row, expandable</text>
  <text x="26" y="132" font-size="11" fill="currentColor">lazy child rows</text>
  <text x="14" y="160" font-size="11" fill="currentColor">Order By: tree_orderings</text>
  <text x="14" y="180" font-size="11" fill="currentColor">Mode: display_modes</text>
  <rect x="170" y="6" width="244" height="200" rx="8" fill="none" stroke="currentColor"/>
  <text x="178" y="24" font-size="11" fill="currentColor">header-icon  header-text</text>
  <line x1="170" y1="32" x2="414" y2="32" stroke="currentColor" stroke-dasharray="3 3"/>
  <text x="178" y="50" font-size="11" fill="currentColor">banner</text>
  <text x="178" y="110" font-size="11" fill="currentColor">pane content (not a plugin slot)</text>
  <line x1="170" y1="182" x2="414" y2="182" stroke="currentColor" stroke-dasharray="3 3"/>
  <text x="178" y="198" font-size="11" fill="currentColor">footer-row</text>
  <rect x="6" y="212" width="408" height="16" rx="4" fill="none" stroke="currentColor"/>
  <text x="14" y="224" font-size="10" fill="currentColor">status_segments: quota  |  cost</text>
</svg>
<figcaption>The places that the contract opens. Dashed lines separate the host's fixed parts from the plugin's slots.</figcaption>
</figure>

### Decorations on tree rows

A decoration is text and an icon that a plugin attaches to one node, in one of its own `decoration_slots`. The intent `set-decoration` takes these optional fields. A missing field keeps the plain behaviour.

| Field | Type | Meaning |
|---|---|---|
| `node` | node id | The row. |
| `slot` | string | An own `decoration_slots` id. |
| `content` | styled text | The text, with a tone per run. |
| `icon` | optional string | A glyph or an icon catalogue id. |
| `sort-key` | optional `s64` | Used by `tree_orderings`. |
| `persist` | bool | Keep the decoration across restarts. |
| `spans` | optional rich text | Styled spans with their own tooltip and click target, replacing `content`. |
| `tooltip` | optional tooltip | Up to 200 characters, shown after the hover delay. |
| `frames` | list of styled text, 2 to 16 | Animation frames. |
| `frame-ms` | `u16`, at least 90 | Time per frame. |
| `trailing` | bool | Right-aligned after the title, for cost columns. |
| `visibility` | `always` or `on-hover` | A hover-only decoration appears only on the row under the pointer. |
| `tone` | optional tone | Default colour for the whole decoration. |

The host steps the frames itself. The plugin makes one call, not one per frame. When the user turns on reduced motion, the host shows the first frame only.

The samples on this page and in the core plugin pages call `baseDecoration(node, slot, text)`. It is a helper each sample plugin defines for itself, not host API: it returns a `set-decoration` value with the given `node` and `slot`, a `content` of one run of `text` in the default tone, and every other field at its plain default (no icon, `persist` false, no frames, `always` visibility). The caller then spreads it and overrides the fields it needs, such as `sortKey` or `trailing`.

Manifest, declared once per plugin:

```json
"decoration_slots": [
  { "id": "cost",  "placement": "badge",    "priority": 40, "trailing": true },
  { "id": "queue", "placement": "icon",     "priority": 60 },
  { "id": "hint",  "placement": "subtitle", "priority": 10, "visibility": "on-hover" }
]
```

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{Intent, SetDecorationSpec, DecorationVisibility};

/// A spinner that the host animates on a pane while the plugin indexes it.
fn indexing_decoration(node: NodeId) -> Submission {
    Submission::keyed(
        Intent::SetDecoration(SetDecorationSpec {
            node,
            slot: "queue".into(),
            content: StyledText::plain("indexing"),
            icon: None,
            sort_key: None,
            persist: false,
            spans: None,
            tooltip: Some(Tooltip::new("Building the search index")),
            frames: ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
                .iter().map(|glyph| StyledText::plain(glyph)).collect(),
            frame_ms: 100,
            trailing: false,
            visibility: DecorationVisibility::Always,
            tone: Some(Tone::Info),
        }),
        format!("index-{node}"),
    )
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk';

const SPINNER = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/** A spinner that the host animates on a pane while the plugin indexes it. */
export function indexingDecoration(node: bigint): Submission {
  return {
    intent: {
      tag: 'set-decoration',
      val: {
        node,
        slot: 'queue',
        content: { text: 'indexing', tone: undefined },
        icon: undefined,
        sortKey: undefined,
        persist: false,
        spans: undefined,
        tooltip: { text: 'Building the search index' },
        frames: SPINNER.map((glyph) => ({ text: glyph, tone: undefined })),
        frameMs: 100,
        trailing: false,
        visibility: 'always',
        tone: 'info',
      },
    },
    correlation: 1n,
    dedupKey: `index-${node}`,
  };
}
```

Outcome: `ok`. Errors: `invalid-argument` when `frames` has fewer than 2 or more than 16 entries, or `frame-ms` is below 90; `not-found` for an unknown node; `not-permitted` without `tree.decorate`; `invalid-argument` for a slot that is not one of the plugin's own. Clear a decoration with `clear-decoration { node, slot }`.

<figure>
<pre class="term">▼ 📂 Ilium
›   🦀 <span class="t-hi">⠋</span> Fix auth                <span class="t-dim">$0.42</span>
›   🐢 ● Docs pass               <span class="t-dim">$0.08</span>
›   🖥️    Shell
</pre>
<figcaption>A tree with two decorations. The spinner in the now column comes from a <code>frames</code> decoration in an <code>icon</code> slot. The cost at the right edge is a <code>trailing</code> decoration in the <code>cost</code> slot. Drawn after the tree rows of the real client: an indent marker, then the identity glyph, then the status column.</figcaption>
</figure>

### Tree toolbar buttons

`tree_toolbar` adds buttons to the strip above the tree. The built-in strip holds the search `🔎`, restructure `♻️` and settings `🎚️` buttons. At most 4 entries per plugin.

```json
"tree_toolbar": [
  { "id": "board", "icon": "📉", "label": "Open board", "command": "open-board",
    "caption": "Board" },
  { "id": "quota", "icon": "⌛", "label": "Quota", "popup_view": "quota-popover" }
]
```

| Field | Rule |
|---|---|
| `icon` | A glyph or an icon catalogue id. |
| `label` | At most 24 characters. The tooltip and the accessible name. |
| `caption` | At most 12 cells, shown beside the icon when the sidebar is wide. |
| `command` | Runs when clicked. Arrives as `command-invoked` with origin `tree-toolbar`. |
| `popup_view` | An own `panels` id. The button opens it as a popover anchored at the button instead of running `command`. |
| `when` | A context expression, for example `display_mode == "attention"`. An unknown key is refused with `manifest.when-key`. |

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::{CommandInvokedEvent, CommandOrigin};

fn on_command(invoked: &CommandInvokedEvent) -> Vec<Submission> {
    if invoked.origin == CommandOrigin::TreeToolbar && invoked.command == "open-board" {
        return vec![Submission::new(Intent::CreatePane(CreatePaneSpec::kind("ilium.board")))];
    }
    Vec::new()
}
```

**TypeScript**

```ts
import type { CommandInvokedEvent, Submission } from '@ilium/plugin-sdk';

export function onCommand(invoked: CommandInvokedEvent): Submission[] {
  if (invoked.origin === 'tree-toolbar' && invoked.command === 'open-board') {
    return [{ intent: { tag: 'create-pane', val: { kind: 'ilium.board' } }, correlation: 1n, dedupKey: undefined }];
  }
  return [];
}
```

### Hover actions on rows

`row_actions` places icons in the strip that appears on the row under the pointer. At most 4 entries per plugin. The built-in strip shows rename `✏️`, move up `🔼`, move down `🔽` and close `🚫`; plugin icons come after them, ordered by `order` (0 to 1000, default 500).

```json
"row_actions": [
  { "id": "bookmark", "icon": "★", "label": "Bookmark", "command": "toggle-bookmark",
    "applies_to": ["pane", "agent"], "order": 300 }
]
```

`applies_to` lists any of `pane`, `group`, `folder`, `project`, `agent` and `virtual:<row-kind>`. A click arrives as `command-invoked` with origin `row-action`; `arguments-json` is `{"node": <id>}` or, for a virtual row, `{"row": "<virtual row id>"}`. A target name that is none of these is refused at install with `manifest.contribution-target`.

**Rust**

```rust
fn on_row_action(invoked: &CommandInvokedEvent) -> Vec<Submission> {
    if invoked.origin != CommandOrigin::RowAction || invoked.command != "toggle-bookmark" {
        return Vec::new();
    }
    let Some(node) = invoked.context.node else { return Vec::new() };
    let on = !is_bookmarked(node);
    vec![Submission::new(Intent::SetBookmarked(SetBookmarkedSpec { node, bookmarked: on }))]
}
```

**TypeScript**

```ts
export function onRowAction(invoked: CommandInvokedEvent): Submission[] {
  if (invoked.origin !== 'row-action' || invoked.command !== 'toggle-bookmark') return [];
  const node = invoked.context.node;
  if (node === undefined) return [];
  return [{
    intent: { tag: 'set-bookmarked', val: { node, bookmarked: !isBookmarked(node) } },
    correlation: 1n,
    dedupKey: undefined,
  }];
}
```

<figure>
<pre class="term">▼ 📂 Ilium
›   🦀 ⠋ Fix auth                       <span class="t-hi">✏️ 🔼 🔽 🚫 ★</span>
›   🐢 ● Docs pass
›   🖥️    Shell
</pre>
<figcaption>The hover strip on the first row. The first four glyphs are the built-in actions; the star is a plugin <code>row_actions</code> entry. The strip is visible only on the row under the pointer.</figcaption>
</figure>

### Tree orderings and display modes

`tree_orderings` adds a choice to the tree's Order By list. Siblings sort by the `sort-key` of the plugin's decoration in the named slot, ascending. Nodes without a key keep their manual order, after the keyed ones. At most 4.

`display_modes` adds a mode next to Normal and Attention. In the mode, the tree shows the decorations of the slot first, and with `hide_unmarked: true` it hides panes that have none. At most 2. The host sends `display-mode-changed` (payload: the mode's global id, or none for Normal) to the plugin that owns the mode.

```json
"decoration_slots": [ { "id": "cost", "placement": "badge", "priority": 40 } ],
"tree_orderings": [ { "id": "by-cost", "title": "Cost, highest first", "slot": "cost" } ],
"display_modes": [ { "id": "costly", "title": "Costly", "icon": "¤", "slot": "cost", "hide_unmarked": true } ]
```

To sort highest first, send a negative `sort-key`. A `slot` that is not the plugin's own is refused with `manifest.contribution-target`.

**Rust**

```rust
fn publish_cost(node: NodeId, cents: i64) -> Submission {
    Submission::new(Intent::SetDecoration(SetDecorationSpec {
        sort_key: Some(-cents), // ascending order puts the highest cost first
        ..SetDecorationSpec::text(node, "cost", format!("${:.2}", cents as f64 / 100.0))
    }))
}
```

**TypeScript**

```ts
export function publishCost(node: bigint, cents: number): Submission {
  return {
    intent: {
      tag: 'set-decoration',
      val: { ...baseDecoration(node, 'cost', `$${(cents / 100).toFixed(2)}`), sortKey: BigInt(-cents) },
    },
    correlation: 1n,
    dedupKey: undefined,
  };
}
```

### Virtual rows

A virtual row is a row in the tree that does not belong to a pane or group: a plugin's own list, such as saved searches or quota buckets. The plugin sends them with `set-virtual-rows { row-kind, rows }`. A row also has `parent`, `expandable`, `accepts-drop`, `tooltip`, a rich-text `label` and `trailing`. The manifest entry `virtual_rows` also has `expandable`, `children`, `accepts_drop` and `drop_command`.

| Setting | Values | Meaning |
|---|---|---|
| `children` | `"static"` | The plugin sends child rows itself with `parent` set. |
| `children` | `"provider"` | The host calls the `virtual-rows-provider` export when the user expands a row. |
| `accepts_drop` | list of `pane`, `file`, `text` | What the row accepts. A drop runs `drop_command` with origin `virtual-row-drop`. |

The provider export:

```wit
// Excerpt of ilium:exports/virtual-rows-provider in reference-wit
record virtual-row-child { id: string, label: rich-text, icon: option<string>, expandable: bool, command: option<string>,
                           tooltip: option<tooltip>, accepts-drop: bool }
list-virtual-children: func(row-kind: string, parent: string, cursor: option<string>)
    -> result<tuple<list<virtual-row-child>, option<string>>, plugin-error>;
```

The deadline is 20 ms and a page holds at most 256 children. While a page is pending the host shows `…` with a spinner; with a returned cursor it shows a `Load more` row. Expansion state belongs to the host. A plugin changes it with `set-expanded { virtual-row, expanded }`, which needs no grant beyond the plugin's own `virtual_rows` entry (real nodes need `tree.write`).

**Rust**

```rust
use ilium_plugin_sdk::exports::VirtualRowsProvider;

struct Searches;

impl VirtualRowsProvider for Searches {
    fn list_virtual_children(
        &mut self, row_kind: &str, parent: &str, cursor: Option<String>,
    ) -> Result<(Vec<VirtualRowChild>, Option<String>), PluginError> {
        if row_kind != "saved-searches" {
            return Ok((Vec::new(), None));
        }
        let page = load_page(parent, cursor.as_deref())?; // must answer within 20 ms: read from a cache
        let rows = page.items.iter().map(|item| VirtualRowChild {
            id: item.id.clone(),
            label: RichText::plain(&item.title),
            icon: Some("🔎".into()),
            expandable: false,
            command: Some("run-search".into()),
            tooltip: None,
            accepts_drop: false,
        }).collect();
        Ok((rows, page.next_cursor))
    }
}
```

**TypeScript**

```ts
import type { VirtualRowsProvider, VirtualRowChild } from '@ilium/plugin-sdk';

export const virtualRows: VirtualRowsProvider = {
  listVirtualChildren(rowKind, parent, cursor) {
    if (rowKind !== 'saved-searches') return [[], undefined];
    const page = loadPage(parent, cursor); // must answer within 20 ms: read from a cache
    const rows: VirtualRowChild[] = page.items.map((item) => ({
      id: item.id,
      label: [{ text: item.title }],
      icon: '🔎',
      expandable: false,
      command: 'run-search',
      tooltip: undefined,
      acceptsDrop: false,
    }));
    return [rows, page.nextCursor];
  },
};
```

<figure>
<pre class="term">▼ 📂 Ilium
›   🦀 ⠋ Fix auth
› ▼ 🔎 Saved searches
› ›   🔎 TODO in src
› ›   🔎 failing tests
› ›   <span class="t-dim">…</span>
</pre>
<figcaption>A virtual row <code>Saved searches</code>, expanded. Its children come from the provider; the last row stands for a page that is still loading. Indent markers are one <code>›</code> per level, in dark grey in the real client.</figcaption>
</figure>

### Status bar segments

The status bar shows the mode, then segments separated by a long dash with spaces around it. A plugin declares up to 2 segments and fills them with intents.

```json
"status_segments": [ { "id": "quota", "priority": 70, "max_width": 20 } ]
```

`priority` is 0 to 100; when the bar is too narrow, higher priority survives. `max_width` is 4 to 40 cells, default 24. Text longer than that is cut with `…`.

| Intent | Fields | Notes |
|---|---|---|
| `set-status-segment` | `segment`, `content` (rich text), `tooltip`, `tone` | At most 10 per second per slot (the segment id is the slot); faster calls are refused with `rate-limited`. |
| `clear-status-segment` | `segment` | Removes the segment. |

A click on the segment arrives as `command-invoked` with origin `status-segment`.

**Rust**

```rust
fn show_quota(percent_used: u32) -> Submission {
    let tone = if percent_used >= 90 { Tone::Warning } else { Tone::Info };
    Submission::new(Intent::SetStatusSegment(SetStatusSegmentSpec {
        segment: "quota".into(),
        content: RichText::plain(format!("quota {percent_used}%")),
        tooltip: Some(Tooltip::new("Tokens used in this 5 hour window")),
        tone: Some(tone),
    }))
}
```

**TypeScript**

```ts
export function showQuota(percentUsed: number): Submission {
  return {
    intent: {
      tag: 'set-status-segment',
      val: {
        segment: 'quota',
        content: [{ text: `quota ${percentUsed}%` }],
        tooltip: { text: 'Tokens used in this 5 hour window' },
        tone: percentUsed >= 90 ? 'warning' : 'info',
      },
    },
    correlation: 1n,
    dedupKey: undefined,
  };
}
```

<figure>
<pre class="term"><span class="t-hi">ℹ Tree</span>  -  Ctrl+B menu  -  <span class="t-warn">quota 92%</span>  -  <span class="t-dim">3 panes</span>
</pre>
<figcaption>The status bar with one plugin segment. The mode prefix is <code>ℹ</code>. The real bar is drawn in the accent colours with a rounded cap at each end; a segment takes the tone colour of its intent.</figcaption>
</figure>

### Pane frame slots

`pane_frame_slots` reserve a place in the frame of matching panes. At most 4 per plugin. `placement` is `header-icon`, `header-text`, `footer-row` or `banner`. `applies_to` is `all`, `agents`, `terminals`, `pane:<kind>`, or a list of agent signature ids. The intents are `set-frame-slot { pane, slot, content, icon, tooltip }` and `clear-frame-slot { pane, slot }`, at most 10 per second per slot. Filling a slot on a pane that does not match `applies_to` is refused with `invalid-argument`.

```json
"pane_frame_slots": [ { "id": "model", "placement": "header-text", "applies_to": ["claude", "codex"] } ]
```

**Rust**

```rust
fn show_model(pane: NodeId, model: &str) -> Submission {
    Submission::new(Intent::SetFrameSlot(SetFrameSlotSpec {
        pane,
        slot: "model".into(),
        content: RichText::plain(model),
        icon: Some("🧩".into()),
        tooltip: None,
    }))
}
```

**TypeScript**

```ts
export function showModel(pane: bigint, model: string): Submission {
  return {
    intent: {
      tag: 'set-frame-slot',
      val: { pane, slot: 'model', content: [{ text: model }], icon: '🧩', tooltip: undefined },
    },
    correlation: 1n,
    dedupKey: undefined,
  };
}
```

<figure>
<pre class="term">╭──≡ ● · Fix auth  🧩 sonnet──────────────╮
│ Reading ilium-client/src/ui.rs          │
│                                         │
╰─────────────────────────────────────────╯
</pre>
<figcaption>A pane frame with a <code>header-text</code> slot. The title notch and the three icons <code>≡ ● ·</code> belong to the host; only the text after the title is the plugin's.</figcaption>
</figure>

### Errors and limits

| Situation | Result |
|---|---|
| A contribution key over its cap (`status_segments` 2, `pane_frame_slots` 4, `tree_toolbar` 4, `row_actions` 4, `tree_orderings` 4, `display_modes` 2) | Install refused with `manifest.contribution-limit` |
| `applies_to` or `slot` names an unknown target or another plugin's slot | `manifest.contribution-target` |
| Unknown `when` key | `manifest.when-key` |
| Intent without `ui.chrome` or `tree.decorate` | `not-permitted` |
| More than 10 updates per second to one segment or slot | `rate-limited` |
| Decoration frames below 2 or above 16, or `frame-ms` below 90 | `invalid-argument` |
| Tooltip over 200 characters | Truncated with `…` |
| Provider answers after 20 ms | Deadline breach counted; the host shows `…` until the next page |
| Virtual-row page above 256 children | Clamped |

### See also

- [tree](/pages/api-tree/) for reading nodes.
- [Pane lifecycle](/pages/api-pane-lifecycle/) for freeze, replace and restructure intents.
- [UI](/pages/api-ui/) for popovers opened from a toolbar button and for the menu model.
- [Intents](/pages/api-intents/) and [events](/pages/api-events/) for the shared shapes.
- [UI model](/pages/ui-model/) for how the host draws rows and widgets.

### Security

Tree chrome text is sanitised ([SEC-380]).
