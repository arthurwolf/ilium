# The UI model

## The UI model

A plugin describes its interface as a flat list of widgets. The kernel lays the list out, draws it with the Ilium theme, keeps the state of each widget and reports what the user did. The plugin never draws, and it never stores a scroll position, a selection, a hover state or a focus. Each render call returns a fresh description, and the host merges it with the state it holds under each widget key.

This page covers the render function, the flat tree and its flags, the fields a node can carry (tooltip, tab index and disabled reason), host-owned state, the 17 action kinds and 12 value cases, the pointer model, hover and tooltips, popups and their anchors, focus order, drag and drop, the widened widget set of settings views and the theme extras. Every code example is given in Rust and in TypeScript.

## What you need

- A client-side component. The `views` export exists only on the client side.
- The `ui.panels` grant, or `panes.provide-kind` for pane views. See [UI panels](/pages/capability-ui-panels/).
- The `views` export in the manifest. Every component also has a `handler` export.
- Rust: the `ilium-plugin-sdk` crate. TypeScript: `@ilium/plugin-sdk`.

Read [Retained mode](/pages/retained-mode/) and [Accessibility and theming](/pages/accessibility-and-theming/) before you build a view.

## The render function

| Language | Signature |
|---|---|
| WIT | `render: func(view: view-target, area: area) -> ui-tree;` |
| Rust | `fn render(&mut self, view: ViewTarget, area: Area) -> UiTree` |
| TypeScript | `render(view: ViewTarget, area: Area): UiTree` inside `definePlugin` |

The kernel calls `render` when the view first opens, after `ui.invalidate` for that view, when the view's area changes, and after a theme or font change. It never runs on every frame. Every change of the view's area triggers one render with the new area, coalesced to at most one per 16 ms; until it returns, the host lays out the previous tree in the new area through the same constraint solver. `area` gives the `columns` and `rows` the view may use. The host clips every widget to that area. [Layout constraints](/pages/layout/) shows how a plugin uses `area`.

### View targets

| Case | Payload | Where the view appears |
|---|---|---|
| `panel` | `string`, the panel id | A workspace panel |
| `pane` | `node-id` | The body of a pane whose kind the plugin provides |
| `popup` | `string`, the popup id | A popup opened with `open-popup` |
| `settings-page` | `string`, the page id | A Settings page |
| `onboarding-step` | `string`, the step id | A first-run onboarding step |

In Rust these are `ViewTarget::Panel(String)`, `ViewTarget::Pane(NodeId)`, `ViewTarget::Popup(String)`, `ViewTarget::SettingsPage(String)` and `ViewTarget::OnboardingStep(String)`. In TypeScript they are tagged objects such as `{ tag: 'panel', val: 'notes' }`.

## The ui-tree

WIT has no recursive types, so a tree is a flat list of nodes, and each node names its children by index.

```wit
// Excerpt of ilium:types/ui in reference-wit
record ui-node {
  key: option<string>, widget: widget, children: list<u32>, %flags: node-flags,
  tooltip: option<tooltip>,          // hover help
  tab-index: option<s16>,            // focus order
  disabled-reason: option<string>,   // at most 40 characters
}
flags node-flags { focusable, draggable, drop-target, hidden, hover-events, pointer-events }
record ui-tree { version: u32, nodes: list<ui-node>, root: u32 }
```

- `root` must be 0. The field stays for the shape of later versions and must equal 0. Every child index must be greater than its parent's index and less than `nodes.len()`. Every node except the root must appear exactly once, in exactly one `children` list. A nonzero root, an out-of-range index, an unreachable node, a node with two parents or a backward index traps the instance with `ui.invalid-tree`.
- Each `key` must be unique across the whole view, not only among siblings. A duplicate key anywhere in one `ui-tree` traps with `ui.duplicate-key`. The host matches nodes by key across the whole view, so a keyed node that moves to another parent keeps its host-owned state.
- The depth is at most 32, and the root is depth 1. A node at depth 33 traps with `ui.too-deep`.
- A `layout` needs one child per constraint, and a `ratio` with denominator 0 is invalid (`ui.invalid-tree`).
- `views.render` returns a `ui-tree`, not a `result`, so it cannot return `invalid-argument`. There is one outcome for every invalid tree: the host keeps the previous tree (marked stale), logs one error, and the instance traps with `trap.invalid-ui-tree`, whose logged detail reason is a `ui.*` code. The trap counts as one failure under the restart rule (see [Errors and limits](/pages/reference-errors-and-limits/)). The codes are `ui.invalid-tree` (structure, a prop out of range, more than 256 check-list items, more items than `max-items`, a bad validation regex), `ui.duplicate-key` (two nodes with the same key, or two items with the same item `id` in one widget), `ui.too-deep`, `ui.too-large` (more than 4,096 nodes or 1 MiB) and `ui.widget-not-allowed` (a `menu`, `list-editor` or other input widget in a settings page). A render over its deadline traps with `ui.render-timeout`.

The Rust and TypeScript builders fill `version`, the widget defaults and the child indices. A wrong layout stops the render before the tree leaves the plugin, and the message names the layout key.

| Limit | Default, every tier | T1 ceiling (requested in `limits`) |
|---|---|---|
| Nodes in one tree | 4,096 | 16,384 |
| Encoded size | 1 MiB | 4 MiB |

| Flag | Meaning |
|---|---|
| `focusable` | The node can take keyboard focus. Widgets in [Inherently focusable widgets](#inherently-focusable-widgets) take it without the flag; every other widget, such as a `surface` that reports clicks, needs it. |
| `draggable` | The node can be dragged onto a `drop-target`. |
| `drop-target` | The node accepts a drop and receives one `dropped` action. |
| `hidden` | The node is not drawn. Its state is kept. |
| `hover-events` | The node sends `hover-enter` and `hover-leave`. |
| `pointer-events` | The node sends `pointer` actions. It also needs `focusable`. |

| Node field | Type | Meaning |
|---|---|---|
| `tooltip` | `option<tooltip>` | Help shown after a delay on hover, or on focus with F1. It needs no flag. |
| `tab-index` | `option<s16>` | Focus order, see [Focus order](#focus-order). Values from 1 come first, in ascending order. None and 0 follow in tree order. A negative value is skipped by Tab. |
| `disabled-reason` | `option<string>` | The node is drawn muted and ignores input. It stays in the focus order. The reason, at most 40 characters, shows as its tooltip. |

**Rust**

```rust
let search = tree
    .text_input("Search")
    .key("search")
    .tab_index(1)
    .tooltip("Filters the list as you type")
    .hover_events(true);
let export = tree
    .button("Export")
    .key("export")
    .disabled_reason("No sessions selected");
```

**TypeScript**

```ts
const search = textInput('Search', {
  key: 'search',
  tabIndex: 1,
  tooltip: 'Filters the list as you type',
  flags: { hoverEvents: true },
});
const exportButton = button('Export', {
  key: 'export',
  disabledReason: 'No sessions selected',
});
```

## Widgets

Each node carries one `widget` variant. There are 51 widgets in seven families. The [widget catalogue](/pages/widgets-index/) lists all of them with their pages. The table lists what each family sends back; a blank cell means it sends nothing.

| Family | Widgets | Sends to the plugin | Page |
|---|---|---|---|
| Layout and containers | `layout`, `block`, `clear`, `separator` | nothing | [Layout and containers](/pages/widgets-layout-and-containers/) |
| Layout and containers | `tabs` | `selected` with `index(u32)` | [Layout and containers](/pages/widgets-layout-and-containers/) |
| Layout and containers | `popup` | `closed` with `none` | [Layout and containers](/pages/widgets-layout-and-containers/) |
| Layout and containers | `split` | `resized` with `number(f64)` | [Layout and containers](/pages/widgets-layout-and-containers/) |
| Layout and containers | `dialog` | `activated` with `text(button id)`, `closed` | [Layout and containers](/pages/widgets-layout-and-containers/) |
| Layout and containers | `scroll-view`, `scrollbar` | `scrolled` with `index(u32)` | [Layout and containers](/pages/widgets-layout-and-containers/) |
| Text | `paragraph` | `scrolled` when scrollable; `activated` for clickable spans | [Text](/pages/widgets-text/) |
| Text | `markdown` | `activated` with `text(target)` without `link-command` | [Text](/pages/widgets-text/) |
| Text | `leader-row`, `key-hints` | nothing; `key-hints` runs its command on the host | [Text](/pages/widgets-text/) |
| Text | `diff-view` | `selected`, `activated` with `index(u32)` (hunk or line) | [Text](/pages/widgets-text/) |
| Collections | `%list`, `table`, `tree-list`, `menu`, `check-list`, `list-editor` | `selected`, `activated`, `toggled`, `changed`, `context-requested`, `closed` | [Collections](/pages/widgets-collections/) |
| Data display | `gauge`, `line-gauge`, `sparkline`, `bar-chart`, `canvas`, `calendar`, `minimap` | nothing | [Data display](/pages/widgets-data-display/) |
| Data display | `chart`, `progress-row`, `meter` | `selected` (focusable chart), `activated` (progress row) | [Data display](/pages/widgets-data-display/) |
| Input | `button`, `toggle`, `choice`, `number`, `text-input`, `text-area`, `key-capture`, `color-picker`, `path-field`, `location-picker` | `activated`, `changed`, `submitted`, `filter-changed` | [Input](/pages/widgets-input/) |
| Input | `schema-form` | nothing; the owning plugin receives `settings-changed` | [Input](/pages/widgets-input/) |
| Status and feedback | `badge`, `spinner`, `notice`, `icon` | nothing; `notice` may send `closed` | [Widget catalogue](/pages/widgets-index/) |
| Media and embedded | `surface`, `terminal-view`, `code-view` | `activated` with `cell`, `pointer`, `changed` | [Media and embedded](/pages/widgets-media-and-embedded/) |
| Media and embedded | `image`, `video` | nothing | [Media and embedded](/pages/widgets-media-and-embedded/) |

The [widget input map](/pages/reference-widget-input-map/) gives the exact focus behaviour, keys, mouse actions, events, colour roles and stable-glyph substitutes of every widget. The family pages repeat it for their widgets.

## Keys and host-owned state

The host keeps each widget's state under its key. A key must stay the same across renders. Without one, a widget loses its state on every render: a text cursor jumps to the start and a list drops its selection.

| Widget | Host-owned state | `widget-state` case |
|---|---|---|
| `%list`, `table` | selected row, scroll offset | `selected(option<u32>)`, `offset(u32)` |
| `%list`, `table`, `tree-list` | multi-selection | `ids(list<string>)` |
| `tree-list` | selected item | read as `ids(list<string>)` with one id; set with `ids([id])` |
| `choice` | selected option | `selected(option<u32>)`; a catalogue choice is set with `text(item id)` and `selected(n)` on it returns `invalid-argument` |
| `tabs` | active tab | `tab(u32)` |
| `text-input`, `text-area` | text and cursor | `text(string)` |
| `tree-list` | expanded items | `expanded(list<string>)` (read and set) |
| `toggle` | on or off | `checked(bool)` |
| `check-list`, tri-state `toggle` | the state of each item | `tri(list<tuple<string, tri-state>>)` |
| `location-picker` | the chosen point | `geo(option<geo-point>)` |
| `number` (also the slider) | the value | `number(f64)` |
| `split` | the divider position in per mille | `split(u16)` |

Read the state with `ui.widget-state(view, key)` when `render` needs it, and change it with `ui.set-widget-state(view, key, state)`. Setting state for an unknown key returns `not-found`. A state kind that does not fit the widget returns `invalid-argument`.

**Rust**

```rust
// Select the third row of a list and open the second tab.
ui::set_widget_state(view.clone(), "notes-list", WidgetState::Selected(Some(2)))?;
ui::set_widget_state(view, "views", WidgetState::Tab(1))?;
```

**TypeScript**

```ts
ui.setWidgetState(view, 'notes-list', { tag: 'selected', val: 2 });
ui.setWidgetState(view, 'views', { tag: 'tab', val: 1 });
```

## Actions back to the plugin

When the user acts on a widget, the kernel sends one `ui-action` event with the payload `UiAction { view, key, action-id, kind, value }`. In 0.1.0 `action-id` is always a copy of `key`; no widget has a separate action id prop. The `key` is the widget that sent it, and for a drop it is the drop target. A widget without a key sends nothing.

There are 17 kinds: `activated`, `changed`, `submitted`, `selected`, `toggled`, `scrolled`, `dropped`, `context-requested`, `closed`, `focus-gained`, `focus-lost`, `hover-enter`, `hover-leave`, `pointer`, `drag-over`, `resized` and `filter-changed`. The last six (`hover-enter`, `hover-leave`, `pointer`, `drag-over`, `resized`, `filter-changed`) report hover, pointer, drag, resize and filter changes. Every focusable widget also sends `focus-gained` and `focus-lost`.

| `action-value` case | Used by |
|---|---|
| `none` | `button` (`activated`), `popup`, `dialog`, `menu` and `key-capture` (`closed`), hover and focus kinds. The docs write `none` for the option case `none` of `value: option<action-value>` |
| `text(string)` | `text-input`, `text-area`, `markdown`, `tree-list` (item id), a catalogue `choice` (item id), `menu`, `dialog` (button id), `filter-changed` |
| `index(u32)` | `%list`, `table`, `tabs`, `choice`, `scrolled` of every scrolling widget (always the first visible row) |
| `number(f64)` | `number`, `split` (`resized`) |
| `flag(bool)` | `toggle` |
| `cell(cell-position)` | `surface` (column and row, relative to the surface) |
| `drop(drop-value)` | `drop-target` nodes (`dropped`) |
| `pointer(view-pointer)` | nodes with `pointer-events` |
| `drag-over(drag-over-value)` | `drop-target` nodes during a drag |
| `item-state(tuple<string, tri-state>)` | `check-list`, tri-state `toggle` (state read with the `tri` case) |
| `geo(geo-point)` | `location-picker` |
| `ids(list<string>)` | multi-selection in `%list`, `table`, `tree-list` |

The host coalesces `scrolled`, `pointer` and `drag-over`. The others are never dropped.

Typing never reaches the plugin key by key. A `text-input` sends `changed` after 600 ms. A `text-area` with `debounce-ms` unset waits 600 ms while its current text is at most 10,000 bytes (UTF-8) and 1,000 ms above that, re-evaluated at each keystroke; an explicit `debounce-ms` (0 to 2,000) always wins. This rule covers `text-area` widgets in plugin views; a text-area row of a settings tab is written to the config 1 s after the last keystroke whatever its size, per [validation and saving](/pages/reference-settings-schema/#validation-and-saving). 64 KiB stays only as the `text-input` `max-length` cap. A `text-input` with `submit-on-enter` sends `submitted` on Enter. A `toggle` and a `choice` send `changed` at once. `filter-changed` is debounced by 150 ms.

The handler changes its own state and calls `ui.invalidate` for the view. The next `render` reads the new state.

### Context requests

`context-requested` is sent for a right click on any node that has a key, and for Shift+F10 or the Menu key on the focused node. The node does not need `pointer-events`. A mouse trigger carries `pointer` data with the cell relative to the node, button `right` and `clicks` 1. A keyboard trigger carries `pointer` data with column 0, row 0, button `none` and `clicks` 0. In a collection the item under the pointer (or the selected item, for the keyboard) is selected first, and the `selected` event is delivered before `context-requested` in the same batch.

## The pointer model

By default the host turns mouse input into the widget's own actions: a click on a button sends `activated`, a click on a tab sends `selected`. A plugin that draws its own surface can ask for raw pointer input. Set the `pointer-events` and `focusable` flags on the node. The node then sends `pointer` actions whose value is a `view-pointer`.

| Field | Meaning |
|---|---|
| `kind` | `down`, `up`, `move`, `drag`, `wheel-up`, `wheel-down`, `wheel-left` or `wheel-right` |
| `button` | `none`, `left`, `middle` or `right` |
| `modifiers` | flags `shift`, `control`, `alt` |
| `column`, `row` | The cell inside the widget's area, 0-based |
| `sub-x`, `sub-y` | The position inside the cell, 0 to 255, from left and top to right and bottom. The value is 128 when the terminal reports cells only |
| `clicks` | 1, 2 or 3 for `down` events; 0 for the others |

`move` is sent only while a button is held, and the host coalesces it to the frame rate. A plain hover does not produce `move`; use `hover-events` for that.

<figure>
<svg viewBox="0 0 400 190" role="img" aria-label="A pointer position inside a cell: column, row and the sub-cell offset from 0 to 255">
  <title>A view-pointer inside one cell</title>
  <desc>A grid of cells. The pointer is in column 3, row 1, at sub-x 192 and sub-y 64, that is three quarters from the left and a quarter from the top of the cell.</desc>
  <g fill="none" stroke="currentColor" stroke-opacity="0.5">
    <rect x="20" y="20" width="40" height="40"/><rect x="60" y="20" width="40" height="40"/><rect x="100" y="20" width="40" height="40"/><rect x="140" y="20" width="40" height="40"/>
    <rect x="20" y="60" width="40" height="40"/><rect x="60" y="60" width="40" height="40"/><rect x="100" y="60" width="40" height="40"/><rect x="140" y="60" width="40" height="40" stroke-opacity="1" stroke-width="2"/>
    <rect x="20" y="100" width="40" height="40"/><rect x="60" y="100" width="40" height="40"/><rect x="100" y="100" width="40" height="40"/><rect x="140" y="100" width="40" height="40"/>
  </g>
  <circle cx="170" cy="70" r="4" fill="currentColor"/>
  <text x="22" y="160" font-size="11" fill="currentColor">columns 0 to 3, rows 0 to 2</text>
  <text x="210" y="50" font-size="12" fill="currentColor">column = 3, row = 1</text>
  <text x="210" y="72" font-size="12" fill="currentColor">sub-x = 192 (three quarters)</text>
  <text x="210" y="94" font-size="12" fill="currentColor">sub-y = 64 (one quarter)</text>
  <text x="210" y="116" font-size="12" fill="currentColor">clicks = 1, kind = down</text>
</svg>
<figcaption>The pointer marks column 3 and row 1. The sub-cell fields tell a plugin where in the cell the pointer is, which matters for the half-block and Braille graphics of a canvas or surface.</figcaption>
</figure>

**Rust**

```rust
// ilium_plugin_sdk::types::ui::{ActionKind, ActionValue, PointerKind}
for event in batch {
    let Event::UiAction(action) = event else { continue };
    if action.key != "plot" || action.kind != ActionKind::Pointer {
        continue;
    }
    let ActionValue::Pointer(pointer) = action.value else { continue };
    if pointer.kind == PointerKind::Down && pointer.clicks == 2 {
        // Double click: zoom to the cell, using the sub-cell offset for half-cell accuracy.
        let x = f64::from(pointer.column) + f64::from(pointer.sub_x) / 255.0;
        let y = f64::from(pointer.row) + f64::from(pointer.sub_y) / 255.0;
        self.zoom_to(x, y);
        ui::invalidate(view.clone());
    }
}
```

**TypeScript**

```ts
for (const event of batch) {
  if (event.tag !== 'ui-action') continue;
  const action = event.val;
  if (action.key !== 'plot' || action.kind !== 'pointer') continue;
  if (action.value.tag !== 'pointer') continue;
  const pointer = action.value.val;
  if (pointer.kind === 'down' && pointer.clicks === 2) {
    // Double click: zoom to the cell, using the sub-cell offset for half-cell accuracy.
    const x = pointer.column + pointer.subX / 255;
    const y = pointer.row + pointer.subY / 255;
    zoomTo(x, y);
    ui.invalidate(view);
  }
}
```

The node that draws the plot sets the flags: `focusable` and `pointer-events` (`flags: { focusable: true, pointerEvents: true }` in TypeScript, `.focusable(true).pointer_events(true)` in Rust).

## Hover and tooltips

Hover tracking belongs to the host. A node with `hover-events` sends `hover-enter` and `hover-leave`. Tooltips need no flag: any node with a `tooltip` field shows it.

<figure>
<svg viewBox="0 0 420 130" role="img" aria-label="Hover timeline: enter, 500 milliseconds to the tooltip, leave, 150 milliseconds grace">
  <title>The hover timeline</title>
  <desc>The pointer enters a node and hover-enter is sent. After 500 ms the tooltip appears. When the pointer leaves, hover-leave is sent and a popup with dismiss-on-leave closes after a 150 ms grace time.</desc>
  <defs>
    <marker id="hover-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker>
  </defs>
  <line x1="20" y1="70" x2="400" y2="70" stroke="currentColor" marker-end="url(#hover-arrow)"/>
  <circle cx="40" cy="70" r="4" fill="currentColor"/><text x="40" y="52" font-size="11" text-anchor="middle" fill="currentColor">enter</text><text x="40" y="95" font-size="10" text-anchor="middle" fill="currentColor">hover-enter</text>
  <circle cx="190" cy="70" r="4" fill="currentColor"/><text x="190" y="52" font-size="11" text-anchor="middle" fill="currentColor">500 ms</text><text x="190" y="95" font-size="10" text-anchor="middle" fill="currentColor">tooltip shows</text>
  <circle cx="290" cy="70" r="4" fill="currentColor"/><text x="290" y="52" font-size="11" text-anchor="middle" fill="currentColor">leave</text><text x="290" y="95" font-size="10" text-anchor="middle" fill="currentColor">hover-leave</text>
  <circle cx="365" cy="70" r="4" fill="currentColor"/><text x="365" y="52" font-size="11" text-anchor="middle" fill="currentColor">150 ms</text><text x="365" y="95" font-size="10" text-anchor="middle" fill="currentColor">popup closes</text>
</svg>
<figcaption>The host owns the timing. A tooltip delay of 500 ms is the default and a plugin may choose 0 to 2000 ms. The 150 ms leave grace applies to a popup with dismiss-on-leave.</figcaption>
</figure>

A tooltip is text of at most 200 Unicode scalar values in total, given as `styled-lines`. Beyond that the host cuts it to 199 plus `…`. The box is a rounded border with an inner width of min(40, view width - 4) cells, word-wrapped, and at most 8 inner lines (the 8th line is cut with `…`). On hover its top-left corner sits one row below the pointer cell. For F1 it sits one row below the node's top-left cell. It flips above when it does not fit below, then shifts left to stay inside the view. The text uses the foreground role on the info background role. It shows after the tooltip delay (default 500 ms, 0 to 2,000 ms) and hides when the pointer leaves the node or on any key. Keyboard users get the same text with F1 while the node has focus. A disabled node shows its `disabled-reason` as the tooltip.

<figure>
<pre class="term">Threshold too low. <span class="t-hi">Apply 180k</span><span class="t-dim">  (hover)</span>
            ╭──────────────────────────────╮
            │ Writes the recommended       │
            │ threshold                    │
            ╰──────────────────────────────╯
</pre>
<figcaption>Rendered below the hovered span after 500 ms. A tooltip is a rounded block with one cell of horizontal padding, placed one row below the pointer cell, or above it when there is no room.</figcaption>
</figure>

**Rust**

```rust
let apply = tree
    .button("Apply 180k")
    .key("apply")
    .tooltip(Tooltip::new("Writes the recommended threshold").delay_ms(300));
```

**TypeScript**

```ts
const apply = button('Apply 180k', {
  key: 'apply',
  tooltip: { lines: [[span('Writes the recommended threshold')]], delayMs: 300 },
});
```

Inside text, a `rich-span` can carry its own `tooltip`, and a span with a `target` becomes clickable (see [Text](/pages/widgets-text/)). To react to hover, set the `hover-events` flag and handle the two kinds:

**Rust**

```rust
match (action.kind, action.key.as_str()) {
    (ActionKind::HoverEnter, "session-row") => self.preview_open = true,
    (ActionKind::HoverLeave, "session-row") => self.preview_open = false,
    _ => {}
}
```

**TypeScript**

```ts
if (action.key === 'session-row') {
  if (action.kind === 'hover-enter') previewOpen = true;
  if (action.kind === 'hover-leave') previewOpen = false;
}
```

## Popups and anchors

A `popup` node is drawn only while `ui.open-popup` or `ui.open-popup-with` has it open. The popup id is the `key` of the popup or dialog node. Popup ids must be unique across all open views of one plugin instance. `open-popup` searches the last committed tree of each open view. It returns `not-found` when it finds no match and `invalid-argument` when it finds two. A view may have at most 4 popups open (`quota-exceeded` for a fifth). `close-popup` on an id that is not open is a no-op that returns `ok`, and a popup whose node disappears from the next tree closes and sends `closed`. Open popups form a stack: the topmost takes input first, Escape closes it and sends `closed`, and focus moves into an opened popup and returns to the anchor on close.

| Anchor | Placement |
|---|---|
| `centered((width, height))` | In the middle of the view. Its size overrides the spec width and height. |
| `at-cell((column, row))` | Top-left corner on that cell. |
| `below-key("key")` | Under the widget with that key. |
| `at-key({ key, side })` | Next to a widget, on the `below`, `above`, `right` or `left` side. |
| `at-tree-node(node)` | Next to a tree row. Used for toolbar popovers and row actions. |
| `at-pointer` | At the last pointer position. Used for context menus in own views. |

| Option | Default | Effect |
|---|---|---|
| `modal` | true | Input stays in the popup, and the view behind takes none. Options passed to `open-popup-with` override the spec fields `modal` and `close-on-escape`. |
| `pinnable` | false | Adds a pin glyph to the title. A pinned popup stays open until Escape or a click on the pin. |
| `dismiss-on-leave` | false | The popup closes when the pointer leaves both the popup and its anchor for longer than 150 ms. |
| `close-on-escape` | true | Escape closes the popup. |

Size and placement follow the rules in [Popup size and placement](/pages/widgets-layout-and-containers/#popup-size-and-placement).

**Rust**

```rust
// ilium_plugin_sdk::types::ui::{PopupAnchor, PopupOptions, KeyAnchor, AnchorSide}; host::log::{self, Level}
let anchor = PopupAnchor::AtKey(KeyAnchor { key: "stats-button".into(), side: AnchorSide::Below });
let options = PopupOptions { modal: false, pinnable: true, dismiss_on_leave: true, close_on_escape: true };
if let Err(error) = ui::open_popup_with("session-stats", anchor, options) {
    log::log(Level::Warn, &format!("open_popup_with failed: {error:?}"));
}
```

**TypeScript**

```ts
try {
  ui.openPopupWith(
    'session-stats',
    { tag: 'at-key', val: { key: 'stats-button', side: 'below' } },
    { modal: false, pinnable: true, dismissOnLeave: true, closeOnEscape: true },
  );
} catch (error) {
  log('warn', String(error));
}
```

<figure>
<pre class="term">▼ Ilium
›   ⠋ Fix auth
›     Shell
    ╭─ Session stats ──────────────────╮
    │ <span class="t-hi">Overview</span>  Tokens  Activity       │
    │                                  │
    │ Tokens today <span class="t-dim">………………</span> 41,200       │
    │ Cost today <span class="t-dim">…………………</span> $4.20         │
    ╰──────────────────────────────────╯
</pre>
<figcaption>Rendered below a tree row. A non-modal popup anchored <code>at-tree-node</code>. With <code>dismiss-on-leave</code> it closes when the pointer leaves both the popup and its anchor; with <code>pinnable</code> the user can pin it open, and Escape then closes it.</figcaption>
</figure>

A context menu in an own view is a `menu` widget in a popup opened `at-pointer`. The host draws it with the same layout as its own menus.

<figure>
<pre class="term">  Meeting notes
  Shopping list
  ╭──────────────────────╮
  │                      │
  │   Open               │
  │   Rename             │
  │ <span class="t-dim">────────────────────</span> │
  │ ✓ Pin to top         │
  │   Move to          ▸ │
  │   <span class="t-err">Delete</span>             │
  │                      │
  ╰──────────────────────╯
</pre>
<figcaption>A context menu opened <code>at-pointer</code> on a right click. The host draws one padding row above and below, a separator, the checked marker <code>✓</code> and the submenu marker <code>▸</code>.</figcaption>
</figure>

## Focus

Left and Right, Up and Down and the other keys belong to the focused widget. Focus moves into an opened popup and returns to the anchor when it closes.

### Focus order

Tab order works as follows.

1. First come the focusable nodes with `tab-index` of 1 or more, in ascending value. Ties are broken by pre-order position.
2. Then come the nodes with `tab-index` none or 0, in pre-order.
3. Nodes with a negative `tab-index` take focus by click and by `set-focus` but are skipped by Tab and Shift+Tab.
4. Tab wraps from the last node to the first within the view, and Shift+Tab wraps the other way.
5. Hidden nodes are skipped. A disabled node (`disabled-reason` set) stays in the focus order but ignores input.

### Inherently focusable widgets

These widgets take focus without the `focusable` flag.

| Widget | Condition |
|---|---|
| `button`, `toggle`, `choice`, `number`, `text-input`, `text-area`, `key-capture`, `color-picker`, `path-field`, `location-picker` | always |
| `%list`, `table`, `tree-list`, `menu`, `check-list`, `list-editor`, `tabs`, `scroll-view` | always |
| `split` divider | `resizable` is true |
| `paragraph` | `scroll` is set |
| `markdown` | the text contains a link or a task item |
| `diff-view`, `code-view` | always |
| `terminal-view` | `interactive` is set |
| `schema-form` | one stop per row |

Every other widget is focusable only with `flags.focusable`.

### Escape

Escape is resolved in this order, and the first step that applies consumes it.

1. A focused `key-capture` that is capturing cancels the capture and sends `closed`.
2. A focused text widget with a non-empty selection clears the selection.
3. The topmost open popup or dialog in the view closes (`close-on-escape` true), sending `closed` or the cancel activation of a dialog.
4. A modal panel closes.
5. A non-modal panel or pane view returns focus to the session tree.

<figure>
<pre class="term">╭────────────────╮  ╭────────────────╮  ╭────────────────╮
│<span class="t-hi"> Search  [1]    </span>│  │ Filter  [2]    │  │ List  [3]      │
╰────────────────╯  ╰────────────────╯  ╰────────────────╯
<span class="t-dim">Tab moves 1, 2, 3 and back. Shift+Tab goes the other way.</span>
</pre>
<figcaption>Focus order with <code>tab-index</code> 1, 2 and 3. The focused widget is drawn in the highlight colours; the numbers are shown only to explain the order.</figcaption>
</figure>

The host draws focus on the widget: a focused frame is cyan and bold, a focused row uses the selection colours. The plugin hears `focus-gained` and `focus-lost`.

**Rust**

```rust
let search = tree.text_input("Search").key("search").tab_index(1);
let filter = tree.choice(["All", "Running", "Done"]).key("filter").tab_index(2);
let list = tree.list(items).key("list").tab_index(3);
```

**TypeScript**

```ts
const search = textInput('Search', { key: 'search', tabIndex: 1 });
const filter = choice(['All', 'Running', 'Done'], { key: 'filter', tabIndex: 2 });
const list = listWidget(items, { key: 'list', tabIndex: 3 });
```

## Drag and drop

A `draggable` node dropped on a `drop-target` node produces one `dropped` action. Its value is `drop(drop-value)` (a record with `source-key`, `target-key` and `index`), with `index` as the insertion position. The event `key` is the target key. The host does not move data; the plugin moves its own.

While a drag hovers a drop target, the host sends `drag-over` with `drag-over-value { source-key, target-key, index, accept }` once each time the target changes. The host decides the position from the pointer row inside the target's area, and `accept` carries the result: `before`, `after`, `inside` or `none`. For a `tree-item` with `expandable` set, the top third is `before`, the middle third is `inside` and the bottom third is `after`, with rows rounded down. For any other target, the top half is `before` and the bottom half is `after`. A target with `drop-target` unset accepts nothing. A drop sends `dropped` to the target key with the position in the `drop` value, and for a `tree-list` the target item id as `text`. The host draws the insertion line or the inside highlight, and scrolls the list when the pointer is near an edge.

## Settings views

A `settings-page` tree uses a subset of the widgets. The subset is: `block`, `paragraph`, `tabs`, `separator`, `leader-row`, `%list`, `table`, `button` and `schema-form`, and also `layout`, `split`, `scroll-view`, `markdown`, `key-hints`, `diff-view`, `tree-list`, `check-list`, `gauge`, `line-gauge`, `sparkline`, `bar-chart`, `chart`, `canvas`, `progress-row`, `meter`, `minimap`, `badge`, `spinner`, `notice`, `icon`, `image`, `surface`, `location-picker` and `code-view` with `read-only: true`.

These trap the instance in a settings page with `ui.widget-not-allowed`, and the host keeps the previous tree and logs one error: `popup`, `dialog`, `menu`, `video`, `terminal-view`, an editable `code-view`, and the input widgets other than `button`, `schema-form` and `location-picker`. Values are still edited through `schema-form`, and the host validates and saves them.

## Theme extras

`ui.theme-extras()` returns the motion, density and icon settings that a view should respect, next to the colours of `ui.theme()`.

| Field | Meaning |
|---|---|
| `reduced-motion` | When true, a view should avoid animation. The host also stops its own spinners. |
| `density` | `compact`, `normal` or `comfortable`. Use it to choose margins and spacing. |
| `icon-mode` | `normal` or `stable`. Stable icons are used only when the user opted in. |
| `info`, `selection`, `border` | RGB colours for the roles that `theme-tokens` lacks. |

**Rust**

```rust
let extras = ui::theme_extras();
let margin = match extras.density {
    Density::Compact => 0,
    Density::Normal => 1,
    Density::Comfortable => 2,
};
let animate = !extras.reduced_motion;
```

**TypeScript**

```ts
const extras = ui.themeExtras();
const margin = { compact: 0, normal: 1, comfortable: 2 }[extras.density];
const animate = !extras.reducedMotion;
```

## Worked example in Rust: a filtered notes panel

This component renders a panel with a filter field and a list. It filters the rows when the filter text changes. The host keeps the text, the cursor and the selected row.

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::ui;
use ilium_plugin_sdk::types::layout::{Area, Constraint, Direction};
use ilium_plugin_sdk::types::ui::{ActionKind, ActionValue, UiTree, ViewTarget};
use ilium_plugin_sdk::ui::build::{ListItem, Tree};

const PANEL: &str = "notes";
const FILTER_KEY: &str = "filter";
const LIST_KEY: &str = "notes-list";
const NOTES: [&str; 3] = ["Meeting notes", "Shopping list", "Release checklist"];

#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
struct NotesPanel {
    filter: String,
}

impl Handler for NotesPanel {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::UiAction(action) = event else { continue };
            if action.key != FILTER_KEY {
                continue;
            }
            if let (ActionKind::Changed, ActionValue::Text(text)) = (action.kind, action.value) {
                self.filter = text;
                ui::invalidate(ViewTarget::Panel(PANEL.to_string()));
            }
        }
        Vec::new()
    }
}

impl Views for NotesPanel {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let needle = self.filter.to_lowercase();
        let items: Vec<ListItem> = NOTES
            .iter()
            .filter(|name| name.to_lowercase().contains(&needle))
            .map(|name| ListItem::plain(*name))
            .collect();

        let mut tree = Tree::new();
        let filter = tree.text_input("Filter").key(FILTER_KEY).tab_index(1);
        let notes = tree.list(items).key(LIST_KEY).tab_index(2);
        let body = tree
            .layout(Direction::Vertical, [Constraint::Length(1), Constraint::Min(1)])
            .child(filter)
            .child(notes);
        let root = tree.block().title("Notes").child(body);
        tree.finish(root)
    }
}
```

The panel id `notes` must be declared in the manifest. The plugin keeps the filter text only because it needs that text to filter.

<figure>
<pre class="term">╭─ Notes ────────────────────────────────────╮
│ Filter                                     │
│<span class="t-hi">› Meeting notes                             </span>│
│  Shopping list                             │
│  Release checklist                         │
╰────────────────────────────────────────────╯
<span class="t-dim">Enter opens a note. Esc closes the panel.</span>
</pre>
<figcaption>The notes panel before a filter is typed. The host selects the first row and draws the selected row in the selection colours.</figcaption>
</figure>

<figure>
<pre class="term">╭─ Notes ────────────────────────────────────╮
│ Filter meeting                             │
│<span class="t-hi">› Meeting notes                             </span>│
╰────────────────────────────────────────────╯
<span class="t-dim">1 of 3 notes shown. Enter opens a note.</span>
</pre>
<figcaption>The same panel after the plugin filtered the rows on a <code>changed</code> action. The selection stayed on the same row.</figcaption>
</figure>

## Worked example in TypeScript: reordering rows

Each note is a draggable row. A drop moves the dragged row to the insertion index, and `drag-over` tells the host where the row would land.

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import * as ui from '@ilium/plugin-sdk/host/ui';
import { block, layout, paragraph, span, tree } from '@ilium/plugin-sdk/ui/build';

const PANEL = 'notes';
const ROW_PREFIX = 'row:';
let order = ['Meeting notes', 'Shopping list', 'Release checklist'];

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  activate() {},
  deactivate() { return undefined; },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'ui-action') continue;
      const action = event.val;
      if (action.kind !== 'dropped' || action.value.tag !== 'drop') continue;
      const { sourceKey, index } = action.value.val;
      const moved = sourceKey.slice(ROW_PREFIX.length);
      order = order.filter((name) => name !== moved);
      order.splice(index, 0, moved);
      ui.invalidate({ tag: 'panel', val: PANEL });
    }
    return [];
  },
  render() {
    const rows = order.map((name) =>
      paragraph([[span(name)]], {
        key: ROW_PREFIX + name,
        flags: { draggable: true, dropTarget: true },
      }),
    );
    const body = layout(
      { direction: 'vertical', constraints: rows.map(() => ({ tag: 'length', val: 1 })) },
      rows,
    );
    return tree(block({ titles: [{ text: 'Notes', position: 'top', alignment: 'left' }] }, [body]));
  },
});
```

The row keys carry the note name, so the drop handler reads the source from `sourceKey` without a lookup.

The same example in Rust marks the row as `draggable` and `drop_target`:

**Rust**

```rust
let row = tree
    .paragraph(vec![vec![Span::plain(name)]])
    .key(format!("row:{name}"))
    .draggable(true)
    .drop_target(true);
```

## Cell width

The host measures every plugin string in cells, and plugins never pass widths. The width is the sum, over extended grapheme clusters (UAX #29, Unicode 15.1), of these values.

- 2 when the base of the cluster has East Asian Width W or F, when the cluster contains U+FE0F, or when it is an emoji ZWJ sequence.
- 0 for a cluster made only of controls, ZWJ or default-ignorable code points.
- 1 otherwise. This includes East Asian Ambiguous characters.

Nerd Font private-use glyphs count as 1. Text that overflows a cell boundary at the right edge is replaced by spaces, never by half of a wide glyph. `ui.display-width` returns this value.

## Text sanitising

Before layout, the host removes the following from every plugin string. It applies to labels, spans, titles, tooltips and items alike.

1. The complete ECMA-48 sequences: `ESC [` up to its final byte, `ESC ]`, `ESC P`, `ESC X`, `ESC ^` and `ESC _` up to a terminating BEL or `ESC \` (or the end of the string), and two-byte `ESC Fe` sequences.
2. The remaining C0 controls except TAB and LF, DEL, and the C1 controls U+0080 to U+009F.

TAB expands to spaces up to the next multiple of 4 cells. LF splits lines in multi-line widgets. In single-line fields (labels, titles, items and the first line of a tooltip) CR, LF and CRLF each become one space. Each view logs at most one warn line per render with the code `ui.text-sanitized`. [RND-019]

## Host functions

These live in `ilium_plugin_sdk::host::ui` (Rust) and `@ilium/plugin-sdk/host/ui` (TypeScript).

| Function | Signature (WIT) | Errors |
|---|---|---|
| `invalidate` | `(view: view-target)` | none; a view the plugin does not declare is ignored with one warn line |
| `open-panel` | `(panel: string) -> result` | `not-found` (panel not declared) |
| `close-panel` | `(panel: string)` | none |
| `open-popup` | `(popup: string, anchor: popup-anchor) -> result` | `not-found` (no such `popup` in the last committed tree of any open view), `invalid-argument` (two matches), `quota-exceeded` |
| `open-popup-with` | `(popup: string, anchor: popup-anchor, options: popup-options) -> result` | as `open-popup` |
| `close-popup` | `(popup: string)` | none; an id that is not open is a no-op |
| `widget-state` | `(view, key: string) -> option<widget-state>` | none; `none` for an unknown key |
| `set-widget-state` | `(view, key, state) -> result` | `not-found`, `invalid-argument` |
| `display-width` | `(text: string) -> u16` | none |
| `theme` | `() -> theme-tokens` | none |
| `theme-extras` | `() -> theme-extras` | none |
| `write-surface` | `(view, key, first-row: u32, format, data: list<u8>) -> result` | `not-found`, `invalid-argument` (format mismatch, partial row, over 16 MiB) |

## Edge cases and errors

- **Escape** is reserved by the host and follows the order in [Escape](#escape). A modal popup cannot refuse the close, and the plugin receives `closed`.
- **Popups** draw only while an `open-popup` call has them open. A fifth open popup in one view fails with `quota-exceeded`.
- **Traps.** A duplicate key, a bad child index, a broken layout, a tree over a limit or a widget outside the allowed subset traps the instance with `trap.invalid-ui-tree` (detail reason a `ui.*` code) (see [The ui-tree](#the-ui-tree)).
- **Permissions.** A `code-view` on a buffer the plugin did not create needs `editor.read`. A `terminal-view` needs `terminal.read-screen`, and an `interactive` one also needs `terminal.raw-input`. Without the grant the widget draws an empty frame with the text `not permitted`.
- **Pointer without focusable.** A node with `pointer-events` but not `focusable` sends no pointer actions.
- **Render-time caps.** `views.render` has no result type, so a widget outside the allowed subset, or a `check-list` or `list-editor` over its item cap, cannot return `invalid-argument`. The host traps the instance (one failure under the restart rule) with `ui.invalid-tree` or `ui.widget-not-allowed` and logs the widget key [WID-014].
- **Tooltip length.** A tooltip is limited to 200 Unicode scalar values. A longer text is cut at 199 characters and ends with `…`; it is not an error.
- **Settings views.** A widget outside the allowed subset traps with `ui.widget-not-allowed`.

## What the user sees

The user sees the widgets drawn by the host. The plugin sees the filtered list only after the debounce, when the host asks for a new render. The two panels above show the notes panel before and after a filter is typed. The diagram below shows the split between plugin and host: the plugin returns keyed content, the host draws it and keeps the state, and user actions return as events.

<figure>
<svg viewBox="0 0 400 200" role="img" aria-label="Plugin returns a flat ui-tree, the host keeps widget state and sends ui-action events back">
  <title>Plugin and host</title>
  <desc>The plugin returns a flat list of keyed nodes. The host keeps the state of each node and returns each user action as an event.</desc>
  <defs>
    <marker id="ui-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="20" width="150" height="160" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="85" y="40" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
  <text x="22" y="62" font-size="10" fill="currentColor">0 block [1]</text>
  <text x="22" y="78" font-size="10" fill="currentColor">1 layout [2, 3]</text>
  <text x="22" y="94" font-size="10" fill="currentColor">2 text-input</text>
  <text x="22" y="110" font-size="10" fill="currentColor">3 %list</text>
  <text x="22" y="150" font-size="10" fill="currentColor">content, no state</text>
  <rect x="240" y="20" width="150" height="160" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="315" y="40" font-size="12" text-anchor="middle" fill="currentColor">Host</text>
  <text x="252" y="62" font-size="10" fill="currentColor">filter: "meet"</text>
  <text x="252" y="78" font-size="10" fill="currentColor">notes: selected 0</text>
  <text x="252" y="94" font-size="10" fill="currentColor">notes: offset 0</text>
  <text x="252" y="110" font-size="10" fill="currentColor">focus, hover, popups</text>
  <text x="252" y="150" font-size="10" fill="currentColor">state, keyed by key</text>
  <line x1="162" y1="90" x2="236" y2="90" stroke="currentColor" marker-end="url(#ui-arrow)"/>
  <text x="200" y="82" font-size="10" text-anchor="middle" fill="currentColor">render</text>
  <line x1="238" y1="130" x2="164" y2="130" stroke="currentColor" marker-end="url(#ui-arrow)"/>
  <text x="200" y="122" font-size="10" text-anchor="middle" fill="currentColor">ui-action</text>
</svg>
<figcaption>The plugin returns keyed, indexed content. The host draws it, keeps the state of each widget and returns each user action as an event.</figcaption>
</figure>

## Common mistakes

- **Colour as the only signal.** Add a word or glyph to every status. See [Accessibility and theming](/pages/accessibility-and-theming/). A view MUST show state in text as well as in colour. [AUT-507]
- **Missing keys on interactive widgets.** State is lost on every render, and actions cannot be matched.
- **Expecting a per-key event.** A text input sends `changed` after the debounce, not once per key press.
- **Forgetting `ui.invalidate`.** The host does not render again after a state change. Call `invalidate` for every view whose content changed.
- **Pointer events without focus.** Set both `pointer-events` and `focusable`.
- **Storing hover state in the tree.** Hover belongs to the host. Keep only what the plugin needs to react to.

Requirement ids for this page: [WID-002], [WID-003], [WID-017], [WID-018], [WID-019], [WID-008], [WID-009], [WID-010], [WID-011], [WID-012], [WID-013], [WID-014], [WID-015], [WID-016]. See the [requirements index](/pages/reference-requirements/).

### WIT

The `widget` variant has 51 cases, and the order is the ABI. The owner is the [WIT reference](/pages/reference-wit/).

```wit
// Excerpt of ilium:types/ui in reference-wit
variant widget {
  // layout (10)
  layout(layout-spec), block(block-spec), tabs(tabs-spec), popup(popup-spec), clear,
  scrollbar(scrollbar-spec), split(split-spec), separator(separator-spec), dialog(dialog-spec),
  scroll-view(scroll-view-spec),
  // text (5)
  paragraph(paragraph-spec), markdown(markdown-spec), leader-row(leader-spec), key-hints(list<key-hint>),
  diff-view(diff-view-spec),
  // collections (6)
  %list(list-spec), table(table-spec), tree-list(tree-list-spec), menu(menu-spec),
  check-list(check-list-spec), list-editor(list-editor-spec),
  // data (10)
  gauge(gauge-spec), line-gauge(line-gauge-spec), sparkline(sparkline-spec), bar-chart(bar-chart-spec),
  chart(chart-spec), canvas(canvas-spec),
  // @unstable(feature = ui-calendar)
  calendar(calendar-spec),
  progress-row(progress-row-spec), meter(meter-spec), minimap(minimap-spec),
  // input (11)
  button(button-spec), toggle(toggle-spec), choice(choice-spec), number(number-spec),
  text-input(text-input-spec), text-area(text-area-spec), schema-form(schema-form-spec),
  key-capture(key-capture-spec), color-picker(color-picker-spec), path-field(path-field-spec),
  location-picker(location-picker-spec),
  // status (4)
  badge(badge-spec), spinner(spinner-spec), notice(notice-spec), icon(icon-spec),
  // media (5)
  image(image-spec), surface(surface-spec), code-view(code-view-spec), terminal-view(terminal-view-spec),
  video(video-spec),
}
```

## See also

- [Layout constraints](/pages/layout/)
- [Retained mode](/pages/retained-mode/)
- [Widget catalogue](/pages/widgets-index/)
- [Layout and containers](/pages/widgets-layout-and-containers/)
- [Text](/pages/widgets-text/)
- [Accessibility and theming](/pages/accessibility-and-theming/)
- [UI panels](/pages/capability-ui-panels/)
- [Tutorial: a panel](/pages/tutorial-03-panel/)
- [Tutorial: a notes pane](/pages/tutorial-06-notes-pane/)

API version: `ilium:plugin@0.1.0`.

### Security

Kernel security UI is always on top and never imitated ([SEC-377]); plugin text is sanitised ([SEC-380]). See [Extension security](/pages/reference-extension-security/).

Requirements for this page: [Rendering requirements](/pages/reference-requirements-rendering/).
