---
layout: page
title: Layout and containers
permalink: /pages/widgets-layout-and-containers/
---

## Layout and containers

Containers decide where their children go and draw the frames around them. This family has ten widgets. `layout` splits an area into segments, `block` frames one child, `tabs` shows one child per tab, `popup` and `dialog` float above the view, `clear` erases an area, `scrollbar` shows the offset of another widget, `split` divides an area with a draggable divider, `separator` draws a rule, and `scroll-view` scrolls one tall child.

The plugin never sees screen rectangles. It declares the tree, and the host runs the constraint solver, draws the frames and paints them with the user's theme. `views.render` runs when a view opens, after `ui.invalidate`, when the view's area changes and when the theme changes. It does not run on every frame.

## You need

- The `ui.panels` grant ([Panels](/pages/capability-ui-panels/)), or `panes.provide-kind` for a pane kind ([Basic panes](/pages/capability-panes-provide-kind/)). Both are client-side (`side = "client"`).
- Rust with the `ilium-plugin-sdk` crate, or TypeScript with `@ilium/plugin-sdk`. The builders fill every default listed on this page.
- The layout types `Area`, `Constraint`, `Direction`, `Flex`, `Spacing` and `Margin`. [Layout constraints](/pages/layout/) explains them.
- The [widget catalogue](/pages/widgets-index/) for the rules that all widgets share: keys, host-owned state and `ui-action` events.

Text values in the examples are plain strings. In WIT they are `styled-text`. Use `span()` in TypeScript or `Span::styled` in Rust for a fragment with its own colour.

## The shape of a block

<figure>
<svg viewBox="0 0 360 220" role="img" aria-label="A block has a border with titles, padding and one inner area">
  <title>The parts of a block</title>
  <desc>A rounded border with a title on its top line, then padding, then one inner area for the child.</desc>
  <defs>
    <marker id="arrow-block" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="20" y="50" width="240" height="150" rx="10" fill="none" stroke="currentColor"/>
  <rect x="36" y="66" width="208" height="118" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <rect x="52" y="82" width="176" height="86" fill="none" stroke="currentColor"/>
  <text x="34" y="44" font-size="11" fill="currentColor">Build</text>
  <text x="140" y="129" font-size="11" text-anchor="middle" fill="currentColor">inner area (one child)</text>
  <line x1="284" y1="50" x2="258" y2="50" stroke="currentColor" marker-end="url(#arrow-block)"/>
  <text x="290" y="54" font-size="11" fill="currentColor">Titles</text>
  <line x1="284" y1="100" x2="262" y2="100" stroke="currentColor" marker-end="url(#arrow-block)"/>
  <text x="290" y="104" font-size="11" fill="currentColor">Border</text>
  <line x1="284" y1="130" x2="230" y2="130" stroke="currentColor" marker-end="url(#arrow-block)"/>
  <text x="290" y="134" font-size="11" fill="currentColor">Inner area</text>
  <line x1="284" y1="170" x2="246" y2="170" stroke="currentColor" marker-end="url(#arrow-block)"/>
  <text x="290" y="174" font-size="11" fill="currentColor">Padding</text>
</svg>
<figcaption>Outside in: the border with its titles on the border line, the padding, then the one child in the inner area.</figcaption>
</figure>

## The ten widgets

| Widget | Mirrors | Children | Sends |
|---|---|---|---|
| `layout` | `ratatui::layout::Layout` | one per constraint, in order | nothing |
| `block` | `ratatui::widgets::Block` | zero or one | nothing |
| `tabs` | `ratatui::widgets::Tabs` | one per title; only the selected one draws | `selected` |
| `popup` | `Clear` plus `Block` | one | `closed` |
| `clear` | `ratatui::widgets::Clear` | none | nothing |
| `scrollbar` | `ratatui::widgets::Scrollbar` | none | `scrolled` |
| `split` | Ilium pane split geometry | exactly two | `resized` |
| `separator` | Ilium `─` rule | none | nothing |
| `dialog` | Ilium dialog frame and button row | zero or one (the body) | `activated`, `closed` |
| `scroll-view` | Ilium scroll regions | one | `scrolled` |

Keys, mouse input and theme roles follow the [widget input map](/pages/reference-widget-input-map/). Each widget below lists them again.

## layout

`layout` runs the constraint solver. It takes one child per constraint, in order. [Layout constraints](/pages/layout/) shows six renders and the rules of the solver.

| Prop | Type | Meaning |
|---|---|---|
| `direction` | `horizontal` or `vertical` | The axis of the split. |
| `constraints` | `list<constraint>` | One entry per child. |
| `flex` | `legacy`, `start`, `end`, `center`, `space-between`, `space-around` or `space-evenly` | Where leftover cells go. Default `start`. |
| `spacing` | `space(u16)` or `overlap(u16)` | The gap, or the overlap, between segments. Default `space(0)`. |
| `margin` | `horizontal`, `vertical` (both `u16`) | Cells removed from the area before the split. Default 0 by 0. |

Actions: none. Keys: none. Theme roles: none. It draws nothing itself. A `popup` or `dialog` child occupies its slot as empty space; give it `length(0)` to take none.

<figure>
<pre class="term"><span class="t-dim">0         1         2         3         4         5
012345678901234567890123456789012345678901234567890123456789</span>
╭─ Sidebar ────────╮╭─ Body ───────────────────────────────╮
│Agents            ││Select a session to read it.          │
│Sessions          ││                                      │
│                  ││                                      │
╰──────────────────╯╰──────────────────────────────────────╯
</pre>
<figcaption>Rendered at 60 columns. A horizontal layout with <code>length(20)</code> and <code>fill(1)</code>. The sidebar keeps 20 cells and the body takes the other 40.</figcaption>
</figure>

**Rust**

```rust
let root = tree
    .layout(Direction::Horizontal, [Constraint::Length(20), Constraint::Fill(1)])
    .child(sidebar)
    .child(body);
```

**TypeScript**

```ts
const root = layout({
  direction: 'horizontal',
  constraints: [length(20), fill(1)],
}, [sidebar, body]);
```

## block

`block` draws its border and titles around one child. A title sits on the top or bottom line and aligns left, centre or right.

| Prop | Type | Meaning |
|---|---|---|
| `borders` | flags `top`, `right`, `bottom`, `left` | Edges that draw a line. Default: all four. |
| `border-type` | `plain`, `rounded`, `double`, `thick`, `quadrant-inside` or `quadrant-outside` | The line set. Default `rounded`. |
| `titles` | `list<block-title>` | Each title has `text`, `position` (`top` or `bottom`) and `alignment`. Default: none. |
| `padding` | `left`, `right`, `top`, `bottom` (all `u16`) | Cells between the border and the child. Default 0. |
| `style`, `border-style` | `style` record | Colours and modifiers. An empty style inherits the theme. |

The border uses the `border` role. The focused block is the innermost block whose area contains the focused node. Its border and title use the `accent` role, and its title is also bold. Actions: none.

### Block titles

- A left-aligned title draws starting at x+1 as `─`, a space, the title, a space.
- A right-aligned title ends at the last cell before the corner, mirrored: the title, a space, then `─` against the corner.
- A centred title is centred on the inner width, with an odd remainder going right.
- A title wider than the inner width minus 4 is cut to that width minus 1, plus `…`.
- Several titles at the same position are joined by one `─`.

### Border sets

The host frame is always rounded. A plugin `block` defaults to rounded and may use any of the six `border-type` cases. Each case draws these corners and edges.

| `border-type` | Top left, top right, bottom left, bottom right | Horizontal edge (top, bottom) | Vertical edge (left, right) |
|---|---|---|---|
| `plain` | `┌ ┐ └ ┘` | `─` `─` | `│` `│` |
| `rounded` | `╭ ╮ ╰ ╯` | `─` `─` | `│` `│` |
| `double` | `╔ ╗ ╚ ╝` | `═` `═` | `║` `║` |
| `thick` | `┏ ┓ ┗ ┛` | `━` `━` | `┃` `┃` |
| `quadrant-inside` | `▗ ▖ ▝ ▘` | `▄` `▀` | `▐` `▌` |
| `quadrant-outside` | `▛ ▜ ▙ ▟` | `▀` `▄` | `▌` `▐` |

<figure>
<pre class="term">┌────┐ ╭────╮ ╔════╗
│    │ │    │ ║    ║
└────┘ ╰────╯ ╚════╝
┏━━━━┓ ▗▄▄▄▄▖ ▛▀▀▀▀▜
┃    ┃ ▐    ▌ ▌    ▐
┗━━━━┛ ▝▀▀▀▀▘ ▙▄▄▄▄▟
</pre>
<figcaption>The six border types at 6 by 3 cells: plain, rounded, double, thick, quadrant-inside and quadrant-outside.</figcaption>
</figure>

### Host frame

The host draws a frame around every panel, popup and dialog. It is not a plugin `block`. The top edge is, from the left: `╭`, two spaces, `≡` (the frame menu, a click target, `accent` role), one space, `●` (the view state: `success` role when live, `warning` role when stale), one space, `·` (`muted`), one space, the title (`foreground`, bold when focused), then `─` up to the corner `╮`. In stable-glyph mode `≡` becomes `=`, `●` becomes `*` and `·` becomes `-`. The title is cut with `…` so the edge keeps at least one `─`. A plugin `block` title instead draws as `╭─ Title ─…╮`.

<figure>
<pre class="term">╭─ Build ──────────────────────────────────────────────────╮
│                                                          │
│  Compiled 14 crates                                      │
│  Warnings are listed below                               │
│                                                          │
╰───────────────────────────────────────────── 3 warnings ─╯
</pre>
<figcaption>Rendered at 60 columns. A rounded block with padding 1 by 2, a top title at the left and a bottom title at the right. The border uses the <code>border</code> role, or <code>accent</code> and bold while the block has focus.</figcaption>
</figure>

**Rust**

```rust
let frame = tree
    .block()
    .title("Build")
    .padding(Padding { left: 2, right: 2, top: 1, bottom: 1 })
    .child(report);
```

**TypeScript**

```ts
const frame = block({
  titles: [
    { text: 'Build', position: 'top', alignment: 'left' },
    { text: '3 warnings', position: 'bottom', alignment: 'right' },
  ],
  padding: { left: 2, right: 2, top: 1, bottom: 1 },
}, [report]);
```

## tabs

`tabs` shows every title in a strip. The host keeps the selected index as the widget state `tab(u32)`. Give each tab one child, in title order; only the selected child draws. The number of titles MUST equal the number of children, otherwise the instance traps with `ui.invalid-tree`. There is no action-id prop: the widget's `key` names the tabs in `ui-action` events.

| Prop | Type | Meaning |
|---|---|---|
| `titles` | `list<styled-text>` | One title per tab. |
| `divider` | string | The text between titles. Default `│`. |
| `padding-left`, `padding-right` | `u16` | Cells around each title. Default 1 and 1. |
| `highlight-style` | `style` | The style of the selected title. Default: the selection colours. |

The strip takes row 0 of the area, and the selected child gets rows 1 to the end. The strip draws, for each tab, the left padding, the title and the right padding, with the divider (`│`, `muted`) between tabs only. Titles that overflow are cut from the right, with `…` as the last cell. The selected title uses `highlight-style` (default: the `selection` role).

Actions: `selected` with `index(u32)`, the newly selected tab. Keys: Left and Right step tabs without wrapping, Home and End, and Ctrl+PageUp and Ctrl+PageDown from inside a child. Mouse: a click on a title selects it. Roles: `muted` for other titles, `selection` for the selected one, `border` for the divider.

<figure>
<pre class="term"> Agents <span class="t-dim">│</span><span class="t-hi"> Sessions </span><span class="t-dim">│</span> Costs
╭──────────────────────────────────────────────────────────╮
│  12 sessions in this project                             │
│  Sorted by last activity                                 │
│                                                          │
╰──────────────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 60 columns. The selected title is drawn with the selection colours (lavender background in the dark theme). Only the child of the selected tab is drawn.</figcaption>
</figure>

**Rust**

```rust
let root = tree
    .tabs(["Agents", "Sessions", "Costs"])
    .key("views")
    .child(agents)
    .child(sessions)
    .child(costs);
```

**TypeScript**

```ts
const root = tabs(['Agents', 'Sessions', 'Costs'], { key: 'views' }, [agents, sessions, costs]);
```

To pick the tab from code, call `ui.set-widget-state` with `tab(1)` on the key `views`. The tab count must match the child count.

## popup

`popup` is drawn only while `ui.open-popup` has it open, and `ui.close-popup` hides it again. Open popups form a stack, and the topmost one takes input first. The `id` is the name both calls use. It is the `key` of the popup node and is unique across the open views of one plugin instance. A view may have at most 4 popups open.

| Prop | Type | Meaning |
|---|---|---|
| `id` | string | Names the popup. It equals the node key. Unique across the open views of the plugin. |
| `title` | `option<styled-text>` | Text on the top border. Default: none. |
| `modal` | `bool` | When true, input stays in the popup. Default: true. |
| `width`, `height` | `constraint` | The size, see [Popup size and placement](#popup-size-and-placement). Default: 60% each. |

`open-popup-with` takes an anchor and options. The anchors are `centered((width, height))`, `at-cell((column, row))`, `below-key("key")`, `at-key({ key, side })`, `at-tree-node(node)` and `at-pointer`. The options are `modal`, `pinnable`, `dismiss-on-leave` and `close-on-escape`. [The UI model](/pages/ui-model/) explains each anchor.

Actions: `closed` with `none` when the host closes the popup, for example after Escape. Escape follows the order in [Escape](/pages/ui-model/#escape), and a modal popup cannot refuse the close. A click outside closes a non-modal popup. `close-popup` sends nothing. Roles: `border`, and `accent` for the title.

### Popup size and placement

- **Size.** `centered(w, h)` overrides the spec width and height. Every other anchor uses the spec width and height. A percentage is relative to the view area. The default is `percentage(60)` by `percentage(60)`. The result is clamped to [3, view width] by [3, view height] cells.
- **Placement.** `below-key` places the popup at the row under the key's area, with the left edge aligned. `at-key`, `at-cell`, `at-tree-node` and `at-pointer` place its top-left corner at the anchor cell.
- **Fit.** If the popup does not fit below, it flips to above. It then shifts left or up to stay inside the view.
- **Options.** Options passed to `open-popup-with` override the spec fields `modal` and `close-on-escape`.
- **Layout.** A popup or dialog child takes no space in its parent's layout. The slot still gets its constraint's cells and is drawn blank. The popup draws only while open, at its anchor, above all content.

<figure>
<pre class="term">╭─ Build output ───────────────────────────────────────────────╮
│compiling ilium-core                                          │
│compiling ilium-detect                                        │
│linking ilium╭  ≡ ● · Delete 3 files?───────────╮             │
│running 14 te│                                  │             │
│finished in 4│  <span class="t-dim">Esc Cancel</span>   <span class="t-err">Enter Delete</span>       │             │
│             │                                  │             │
│Build finishe╰──────────────────────────────────╯             │
│                                                              │
╰──────────────────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 64 columns. A modal popup centred over a view. The popup clears its cells first, so no text of the view shows inside it. The danger button uses the <code>danger</code> role.</figcaption>
</figure>

**Rust**

```rust
// Declare the popup in the tree, then open it from an event handler.
let confirm = tree.popup("confirm-delete").title("Delete 3 files?").modal(true).child(buttons);
ui::open_popup("confirm-delete", PopupAnchor::Centered((Constraint::Length(36), Constraint::Length(5))))?;
```

**TypeScript**

```ts
const confirm = popup('confirm-delete', { title: 'Delete 3 files?', modal: true }, [buttons]);
openPopup('confirm-delete', { tag: 'centered', val: [length(36), length(5)] });
```

A non-modal popup anchored to a hovered row is the usual way to show details:

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

## clear

`clear` erases every cell of its area. It has no props and no children. Use it when a notice must hide content drawn earlier in the same view.

<figure>
<pre class="term">╭──────────────────────────────────────────────────────────────╮
│The quick brown fox jumps over the lazy dog 0123456789        │
│Second row of bac╭─ Notice ─────────────────╮to the right     │
│Third row of back│  Saved                   │o the right      │
│Fourth row of bac╰──────────────────────────╯to the right     │
│Fifth row of background text that continues to the right      │
╰──────────────────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 64 columns. The notice draws a <code>clear</code> first, so the text behind it is erased inside the frame and stays visible outside it.</figcaption>
</figure>

**Rust**

```rust
let erase = tree.clear();
```

**TypeScript**

```ts
const erase = clear();
```

Actions: none. Keys and mouse: none.

## scrollbar

`scrollbar` has no content of its own. It draws the scroll offset of the widget named by `target-key`, so the thumb always shows where that widget is.

| Prop | Type | Meaning |
|---|---|---|
| `orientation` | `vertical-right`, `vertical-left`, `horizontal-bottom` or `horizontal-top` | Where the bar draws. Default `vertical-right`. |
| `target-key` | string | The key of a scrollable widget in the same view. |

The track is the last column of the scrolling node's own area (the first column with position left) and spans the node's full height. Let T be the track length, V the viewport rows and C the content rows. When C <= V, no bar is drawn and the column belongs to the content. Otherwise `thumb_len = max(1, floor(T*V/C))` and `thumb_start = round((T-thumb_len)*offset/(C-V))`. The track draws `│` (`muted`) and the thumb draws `┃` (`accent` when the node is focused, else `foreground`). There are no arrows. In stable-glyph mode the track is `|` and the thumb is `#`. While the user drags the thumb it uses `accent`. A drag moves the target; a click on the track pages. The scrollable widget sends `scrolled` with `index(u32)`, the new top row offset. The host coalesces bursts of wheel events.

<figure>
<pre class="term">╭─ Log ────────────────────────────────────────────────────╮
│Line 03: compiling ilium-core                            <span class="t-dim">│</span>│
│Line 04: compiling ilium-detect                          <span class="t-hi">┃</span>│
│Line 05: compiling ilium-server                          <span class="t-hi">┃</span>│
│Line 06: compiling ilium-client                          <span class="t-hi">┃</span>│
│Line 07: compiling ilium-ipc                             <span class="t-dim">│</span>│
│Line 08: compiling ilium-pty                             <span class="t-dim">│</span>│
│Line 09: linking                                         <span class="t-dim">│</span>│
│Line 10: done                                            <span class="t-dim">│</span>│
╰──────────────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 60 columns. A <code>vertical-right</code> scrollbar drawn inside the border. The thumb <code>┃</code> sits on the track <code>│</code>; the log holds 20 rows and 8 are visible, scrolled to offset 2. With T=8, V=8 and C=20, `thumb_len` is 3 and `thumb_start` is 1.</figcaption>
</figure>

**Rust**

```rust
let bar = tree.scrollbar(ScrollbarOrientation::VerticalRight, "log").key("log-bar");
```

**TypeScript**

```ts
const bar = scrollbar('log', { orientation: 'vertical-right', key: 'log-bar' });
```

## split

`split` divides its area between exactly two children and lets the user move the divider. It mirrors the pane split of the Ilium tree. The divider position is host state `widget-state::split(u16)`, in per mille; the host keeps it under the key, so it survives a re-render.

| Prop | Type | Meaning |
|---|---|---|
| `direction` | `horizontal` or `vertical` | `horizontal` puts the children side by side. |
| `position` | `u16` | The initial divider position in per mille, 50 to 950. Default 500. |
| `min-first`, `min-second` | `u16` | Minimum size of each child, in cells. |
| `resizable` | `bool` | Whether the user can move the divider. Default true. |

Let A be the area length along the split axis. `first = floor((A-1)*position/1000)`, the divider takes 1 cell, and `second = A-1-first`. Then `first` is clamped to [`min-first`, A-1-`min-second`]; if both minimums cannot fit, `first` gets `min-first` and `second` gets the rest. The divider draws `│` for a horizontal split and `─` for a vertical one (`muted`; `accent` when focused or dragged). It is focusable and draggable only when `resizable` is true. During a drag the grip `┃` or `━` shows.

Actions: `resized` with `number(f64)`, the new position in per mille (the value is `split(position)` as host state), sent when a drag or a key press ends. Keys: with the divider focused, Left and Right (or Up and Down) move `position` by 50, and Home or End move it to the minimum bounds. Mouse: drag the divider.

<figure>
<pre class="term">╭──────────────────────────────────┬───────────────────────╮
│Explorer                          <span class="t-hi">┃</span>Preview                │
│src/                              <span class="t-hi">┃</span>fn main() {            │
│  main.rs                         <span class="t-hi">┃</span>  run();               │
│  lib.rs                          <span class="t-hi">┃</span>}                      │
│tests/                            <span class="t-hi">┃</span>                       │
╰──────────────────────────────────┴───────────────────────╯
</pre>
<figcaption>Rendered at 60 columns. A horizontal <code>split</code> at position 600 per mille. The divider is <code>│</code> at rest; while it is hovered or dragged the host draws the grip <code>┃</code> in the accent colour (shown here). The two panels share the divider column.</figcaption>
</figure>

**Rust**

```rust
let root = tree
    .split(Direction::Horizontal)
    .key("main-split")
    .position(600)
    .min_first(20)
    .min_second(16)
    .child(explorer)
    .child(preview);
```

**TypeScript**

```ts
const root = split({
  direction: 'horizontal',
  key: 'main-split',
  position: 600,
  minFirst: 20,
  minSecond: 16,
}, [explorer, preview]);
```

## separator

`separator` draws a one-cell rule, with an optional label.

| Prop | Type | Meaning |
|---|---|---|
| `direction` | `horizontal` or `vertical` | A horizontal rule is `─`, a vertical rule is `│`. |
| `label` | `option<styled-text>` | Text centred on a horizontal rule. |
| `style` | `style` | Overrides the `border` role. |

Actions, keys and mouse: none. It is allowed in settings views.

<figure>
<pre class="term">  src/main.rs
<span class="t-dim">── Selected file ─────────────────────────────────────────</span>
  fn main() {}
<span class="t-dim">──────────────────────────────────────────────────────────</span>
</pre>
<figcaption>Rendered at 58 columns. A horizontal <code>separator</code> with a label, and one without. A vertical separator is one column of <code>│</code>.</figcaption>
</figure>

**Rust**

```rust
let rule = tree.separator(Direction::Horizontal).label("Selected file");
```

**TypeScript**

```ts
const rule = separator({ direction: 'horizontal', label: 'Selected file' });
```

## dialog

`dialog` is a modal frame with a title, an optional body and a button row. It is opened like a popup, with `open-popup` and its key as popup id, and it is always modal.

| Prop | Type | Meaning |
|---|---|---|
| `title` | `styled-text` | Text on the top border. |
| `kind` | `plain`, `confirm` or `danger-confirm` | `confirm` and `danger-confirm` add two buttons: `cancel` (`is-cancel`, label Cancel) and `ok` (primary, label OK, or Delete for `danger-confirm`). `danger-confirm` styles the frame and `ok` with the `danger` role and focuses Cancel first. |
| `buttons` | `list<dialog-button>` | Each has `id`, `label`, `role` (`primary`, `secondary` or `danger`) and `is-cancel`. |
| `width`, `height` | `constraint` | The size of the frame. |
| `message` | `option<styled-lines>` | Text above the buttons. |

The body is zero or one child. The dialog's action key is its node key. Actions: `activated` with `text(button id)` when a button runs, and `closed` with `none` when the dialog closes without a button. Keys: Escape sends `activated` with `text(id of the is-cancel button)`, or `closed` when there is no `is-cancel` button. Enter on a focused button activates that button, and Enter while focus is in the body activates the primary button. Left and Right move between buttons. Mouse: click a button. Roles: `border`, `accent` for the primary button, and `danger` for a danger button and for a `danger-confirm` frame.

The host draws the hint line `Esc cancel · Enter <primary label>` in the `muted` role on the last inner row. The buttons draw on the row above it, centred, as their labels separated by three spaces.

<figure>
<pre class="term">╭  ≡ ● · Delete file?──────────────────────────────╮
│  notes-2026-10.md will be removed.               │
│  This cannot be undone.                          │
│                                                  │
│                 <span class="t-dim">Cancel</span>   <span class="t-err">Delete</span>                  │
│<span class="t-dim"> Esc cancel · Enter Delete                        </span>│
╰──────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 52 columns. A <code>danger-confirm</code> dialog. The frame is drawn with the <code>danger</code> role, the Cancel button is neutral, the Delete button uses the <code>danger</code> role, and the dim hint line repeats the keys. Cancel has focus first.</figcaption>
</figure>

A `plain` dialog can hold a body widget such as a `text-input`:

<figure>
<pre class="term">╭  ≡ ● · New session───────────────────────────────╮
│ ╭  ≡ ● · Name──────────────────────────────╮     │
│ │Fix auth                                      │ │
│ ╰──────────────────────────────────────────────╯ │
│                 <span class="t-dim">Cancel</span>   <span class="t-hi">Create</span>                  │
│<span class="t-dim"> Esc cancel · Enter Create                        </span>│
╰──────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 52 columns. A <code>plain</code> dialog with a <code>text-input</code> as its body and two buttons. The primary button uses the <code>accent</code> role, and Enter in the text field activates it.</figcaption>
</figure>

**Rust**

```rust
let dialog = tree
    .dialog("delete-file", "Delete file?")
    .kind(DialogKind::DangerConfirm)
    .message("notes-2026-10.md will be removed.\nThis cannot be undone.")
    .width(Constraint::Length(52))
    .height(Constraint::Length(7));
ui::open_popup("delete-file", PopupAnchor::Centered((Constraint::Length(52), Constraint::Length(7))))?;
```

**TypeScript**

```ts
const dlg = dialog('delete-file', {
  title: 'Delete file?',
  kind: 'danger-confirm',
  message: 'notes-2026-10.md will be removed.\nThis cannot be undone.',
  width: length(52),
  height: length(7),
});
openPopup('delete-file', { tag: 'centered', val: [length(52), length(7)] });
```

## scroll-view

`scroll-view` lays out its one child at `content-rows` height and clips it to its area. The child can be taller than the view. The host keeps the first visible row as `widget-state::offset(u32)`.

| Prop | Type | Meaning |
|---|---|---|
| `content-rows` | `u32` | The height at which the child is laid out. |
| `show-scrollbar` | `bool` | Draw the scrollbar. Default true. |
| `follow-tail` | `bool` | Keep the last row visible while the user is at the bottom. |

Keys go to the focused node first. Up, Down, PageUp, PageDown, Home and End that the focused node does not consume bubble to the nearest ancestor `scroll-view`. Up and Down scroll 1 row, the Page keys scroll V-1 rows, and the mouse wheel scrolls 3 rows. When focus moves to a descendant, the `scroll-view` adjusts `offset` so that the descendant's area is fully visible, aligning to its top. The child is laid out at width A-1 when the scrollbar is visible and at width A otherwise; its height is `content-rows`. The scrollbar follows the [scrollbar drawing rule](#scrollbar).

Actions: `scrolled` with `index(u32)`, the first visible row. Keys: Up and Down, PageUp and PageDown, Home and End. Mouse: the wheel, and a drag of the scrollbar. Roles: `border` and `muted`.

<figure>
<pre class="term">╭─ Job log, following the tail ────────────────────────────╮
│ 07 12:01:44 indexed 412 sessions                        <span class="t-dim">│</span>│
│ 08 12:01:45 wrote cost table                            <span class="t-dim">│</span>│
│ 09 12:01:45 wrote token table                           <span class="t-dim">│</span>│
│ 10 12:01:46 checking quotas                             <span class="t-hi">┃</span>│
│ 11 12:01:47 quota ok                                    <span class="t-hi">┃</span>│
│ 12 12:01:47 done                                        <span class="t-hi">┃</span>│
╰──────────────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 60 columns. A <code>scroll-view</code> with <code>follow-tail</code>. With V=6 and C=12 the thumb has length 3 and sits at the bottom, so the newest rows are visible. If the user scrolls up, following stops until the thumb returns to the bottom.</figcaption>
</figure>

**Rust**

```rust
let log = tree.scroll_view(240).key("job-log").follow_tail(true).child(log_lines);
```

**TypeScript**

```ts
const log = scrollView({ contentRows: 240, key: 'job-log', followTail: true }, [logLines]);
```

## Hover and tooltips on containers

Any node can carry a `tooltip` of at most 200 Unicode scalar values, and the `hover-events` and `pointer-events` flags. Containers are no exception: a `block` with a tooltip shows it when the pointer rests on the block for 500 ms. The tooltip box, its size and its placement follow [Hover and tooltips](/pages/ui-model/#hover-and-tooltips).

<figure>
<pre class="term">Threshold too low. <span class="t-hi">Apply 180k</span><span class="t-dim">  (hover)</span>
            ╭──────────────────────────────╮
            │ Writes the recommended       │
            │ threshold                    │
            ╰──────────────────────────────╯
</pre>
<figcaption>Rendered below the hovered span after 500 ms. A tooltip is a rounded block with one cell of horizontal padding. It is placed one row below the pointer cell, or above it when there is no room.</figcaption>
</figure>

## Paint order and overlap

Nodes paint in pre-order, so a later sibling overdraws an earlier one cell by cell. With `overlap(n)` between bordered blocks, shared cells use the box-drawing merge rule. In a horizontal layout the shared vertical edge becomes `┬` at the top and `┴` at the bottom. In a vertical layout the shared horizontal edge becomes `├` at the left and `┤` at the right. Rounded corners that become joints use the plain joints. When the two border types differ, the later sibling's glyph wins. Popups, then tooltips, paint after all content.

## Builder defaults

The WIT records have no defaults. The SDK builders fill these, so a plugin sets only what differs.

| Widget | Defaults |
|---|---|
| `layout` | `flex` start, `spacing` `space(0)`, `margin` 0 by 0 |
| `block` | all four borders, `border-type` rounded, no titles, padding 0, empty styles |
| `tabs` | divider `│`, padding 1 and 1, highlight style = selection colours |
| `popup` | no title, `modal` true, width and height 60% |
| `scrollbar` | `orientation` vertical-right |
| `split` | `position` 500, `resizable` true |
| `dialog` | `kind` plain, no message |
| `scroll-view` | `show-scrollbar` true, `follow-tail` false |

## Worked example in Rust

A client view with a split between a list and a log, the log in a scroll view, under a two-tab strip. A Delete button opens a danger dialog.

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::layout::{Area, Constraint, Direction};
use ilium_plugin_sdk::types::ui::{DialogKind, UiTree, ViewTarget};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::ui::build::Tree;

#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
struct BuildPanel;

impl Lifecycle for BuildPanel {}

impl Handler for BuildPanel {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::UiAction(action) = event else { continue };
            if action.key == "delete" {
                let anchor = PopupAnchor::Centered((Constraint::Length(52), Constraint::Length(7)));
                if let Err(error) = ui::open_popup("delete-file", anchor) {
                    log::log(Level::Warn, &format!("open_popup failed: {error:?}"));
                }
            }
        }
        Vec::new()
    }
}

impl Views for BuildPanel {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let files = tree.paragraph(vec![vec![Span::plain("src/")]]).key("files");
        let log_text = tree.paragraph(vec![vec![Span::plain("Line 01: starting the build")]]);
        let log = tree.scroll_view(240).key("log").follow_tail(true).child(log_text);
        let split = tree.split(Direction::Horizontal).key("main-split").position(400).child(files).child(log);
        let costs = tree.paragraph(vec![vec![Span::plain("Costs this week")]]);
        let tabs = tree.tabs(["Log", "Costs"]).key("views").child(split).child(costs);
        let delete = tree.button("Delete").key("delete");
        let dialog = tree
            .dialog("delete-file", "Delete file?")
            .kind(DialogKind::DangerConfirm)
            .message("This cannot be undone.");
        let root = tree
            .layout(Direction::Vertical, [Constraint::Length(1), Constraint::Fill(1), Constraint::Length(0)])
            .child(delete)
            .child(tabs)
            .child(dialog);
        tree.finish(root)
    }
}
```

The dialog sits in a `length(0)` slot, so it takes no space until `open-popup` shows it.

## Worked example in TypeScript

The same view.

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { log } from '@ilium/plugin-sdk/host/log';
import { openPopup } from '@ilium/plugin-sdk/host/ui';
import {
  button, dialog, fill, layout, length, paragraph, scrollView, span, split, tabs, tree,
} from '@ilium/plugin-sdk/ui/build';

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'ui-action' || event.val.key !== 'delete') continue;
      try {
        openPopup('delete-file', { tag: 'centered', val: [length(52), length(7)] });
      } catch (error) {
        log('warn', String(error));
      }
    }
    return [];
  },
  render() {
    const files = paragraph([[span('src/')]], { key: 'files' });
    const logText = paragraph([[span('Line 01: starting the build')]]);
    const logView = scrollView({ contentRows: 240, key: 'log', followTail: true }, [logText]);
    const main = split({ direction: 'horizontal', key: 'main-split', position: 400 }, [files, logView]);
    const costs = paragraph([[span('Costs this week')]]);
    const views = tabs(['Log', 'Costs'], { key: 'views' }, [main, costs]);
    const confirm = dialog('delete-file', {
      title: 'Delete file?',
      kind: 'danger-confirm',
      message: 'This cannot be undone.',
    });
    return tree(layout({
      direction: 'vertical',
      constraints: [length(1), fill(1), length(0)],
    }, [button('Delete', { key: 'delete' }), views, confirm]));
  },
});
```

## Edge cases and errors

| Situation | Result |
|---|---|
| A `layout` has a different number of children from constraints | `ui.invalid-tree`. The instance traps. The builders stop the render first and name the layout key. |
| A `ratio` has denominator 0 | `ui.invalid-tree`. The instance traps. |
| The tree has more than 4,096 nodes or 1 MiB | `ui.too-large`. The instance traps. |
| Two nodes anywhere in one tree share a `key` | `ui.duplicate-key`. The instance traps. |
| A `split` has a number of children other than two | `ui.invalid-tree`. The instance traps. |
| A `tabs` has a different number of titles and children | `ui.invalid-tree`. The instance traps. |
| A `split` position outside 50 to 950 | The host clamps it. |
| `min-first` and `min-second` need more cells than the area has | The host shrinks both, and the divider cannot move. |
| A constraint asks for more cells than the area has, or the area has zero columns or rows | The solver clips it, or nothing is drawn. No error. |
| `ui.open-popup` names an id missing from the view's last tree | `not-found`. |
| A fifth popup is opened in one view | `quota-exceeded`. The limit is 4 popups per view. |
| Two open views of the plugin hold a popup with the same key | `invalid-argument` from `open-popup`. |
| `ui.close-popup` names a popup that is not open | Nothing happens. No error. |
| `ui.set-widget-state` names an unknown key | `not-found`. |
| `ui.set-widget-state` gives a state the widget does not take, such as `tab(1)` on a `paragraph` | `invalid-argument`. |
| A `scrollbar` names a `target-key` that is not in the view | Nothing is drawn. No error. |
| A `popup` or `dialog` sits in a `layout` | The slot gets its constraint's cells and is drawn blank. Use `length(0)` to take none. |
| A `popup` or `dialog` in a settings page | `ui.widget-not-allowed`. The instance traps and the previous tree stays. |

## What the user sees

- Host frames are always rounded. A plugin `block` is rounded unless `border-type` says otherwise. The focused frame is cyan and bold in the default theme.
- A popup draws over the view, which stays visible behind it. A modal popup blocks input to that view.
- A resize triggers one new `render` with the new area, at most one per 16 ms. Until it returns, the host lays out the previous tree in the new area, so the screen never shows a stale size. The scrollbar thumb follows the target's offset.
- The divider of a `split` highlights when the pointer is on it, and keeps its position when the plugin redraws.

## See also

- [Widget catalogue](/pages/widgets-index/)
- [Layout constraints](/pages/layout/)
- [Text](/pages/widgets-text/)
- [Collections](/pages/widgets-collections/)
- [Views and the ui import](/pages/api-ui/)
- [The UI model](/pages/ui-model/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Tutorial: a panel with a table and a layout](/pages/tutorial-03-panel/)

API version: `ilium:plugin@0.1.0`.
