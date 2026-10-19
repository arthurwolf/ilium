# Layout constraints

## Layout constraints

A plugin never works out where a widget goes. It describes the sizes it wants with constraints, and the host runs the solver of ratatui 0.29. This page gives the constraint kinds, the flex, spacing and margin rules, the order in which the solver works, and how a plugin reads the space it is given. It shows six layouts with renders drawn at 60 columns, the layout parts of other widgets (`split`, `scroll-view`, `popup`, `dialog` and table columns), the errors a layout can raise, and a responsive view in Rust and in TypeScript.

## You will need

- Grants: none for the layout widget itself. The view that holds it needs `ui.panels` for a panel ([Panels](/pages/capability-ui-panels/)) or `panes.provide-kind` for a basic pane ([Basic panes](/pages/capability-panes-provide-kind/)).
- A language: Rust with the `ilium-plugin-sdk` crate, or TypeScript with `@ilium/plugin-sdk` (built with Bun 1.2 or later).
- Reading: [The UI model](/pages/ui-model/) for the flat tree and the `render` export, and [Retained mode](/pages/retained-mode/) for when a view is redrawn.

## The layout widget

A `layout` widget has a direction, a list of constraints, a flex rule, a spacing rule and a margin. It has one child for each constraint, in the same order: the child at index 0 gets the first segment, the child at index 1 the second, and so on. A different number of children and constraints makes the tree invalid.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `direction` | `horizontal` or `vertical` | required | The axis that the segments divide. |
| `constraints` | `list<constraint>` | required | One constraint for each child. |
| `flex` | `flex` | `start` | Where the space that no segment uses goes. |
| `spacing` | `spacing` | `space(0)` | A gap between segments, or an overlap. |
| `margin` | `margin` | 0 by 0 | Cells removed from the area before the segments are placed. |

The constraint kinds follow ratatui:

| Constraint | Argument | Meaning |
|---|---|---|
| `length` | `u16` | An exact number of cells. |
| `min` | `u16` | A segment at least this large. |
| `max` | `u16` | A segment at most this large. |
| `percentage` | `u16` | A share of the available space, in per cent. |
| `ratio` | `tuple<u32, u32>` | A fraction of the available space: numerator and denominator. The denominator must not be 0. |
| `fill` | `u16` | A weight. Fill segments share the space that is left, in proportion to their weights. |

**Rust**

```rust
// ilium_plugin_sdk::types::layout::{Constraint, Direction, Flex, Spacing, Margin}
let row = tree
    .layout(Direction::Horizontal, [
        Constraint::Length(20),          // exactly 20 cells
        Constraint::Min(30),             // at least 30
        Constraint::Max(10),             // at most 10
        Constraint::Percentage(25),      // 25 % of the area
        Constraint::Ratio(1, 3),         // one third
        Constraint::Fill(2),             // twice the weight of Fill(1)
    ])
    .flex(Flex::SpaceBetween)
    .spacing(Spacing::Space(1))
    .margin(Margin { horizontal: 1, vertical: 0 });
```

**TypeScript**

```ts
import { layout, length, fill } from '@ilium/plugin-sdk/ui/build';

const row = layout({
  direction: 'horizontal',
  constraints: [
    length(20),                        // exactly 20 cells
    { tag: 'min', val: 30 },           // at least 30
    { tag: 'max', val: 10 },           // at most 10
    { tag: 'percentage', val: 25 },    // 25 % of the area
    { tag: 'ratio', val: [1, 3] },     // one third
    fill(2),                           // twice the weight of fill(1)
  ],
  flex: 'space-between',
  spacing: { tag: 'space', val: 1 },
  margin: { horizontal: 1, vertical: 0 },
}, children);
```

The flex rule decides where leftover space goes when no segment takes it:

| Flex | Effect |
|---|---|
| `legacy` | The last segment takes the leftover space, as in ratatui's legacy rule. |
| `start` | Segments pack at the start. The leftover space goes at the end. |
| `end` | Segments pack at the end. |
| `center` | Segments are centred, with equal space on each side. |
| `space-between` | The leftover space is split into equal gaps between segments. The first and last segments sit at the edges. |
| `space-around` | Each segment has equal space on both sides. The edges get half a gap. |
| `space-evenly` | Every gap, including the edges, is the same size. |

The spacing rule is either `space(n)`, which puts `n` cells between segments, or `overlap(n)`, which lets neighbouring segments share `n` cells. `margin` takes a `horizontal` and a `vertical` count and removes that many cells on each side of the area before anything else happens. The solver is ratatui 0.29 `Layout::split`, with these rules pinned.

- The margin is applied first. `percentage` and `ratio` are relative to the area after the margin, before spacing.
- A `ratio` above 1 is clamped to 1.
- `fill(0)` gets 0 cells unless every constraint is `fill(0)`.
- The SDK builders default `flex` to `start`. The wire default, an absent field, is `legacy`.
- The segments plus gaps never exceed the area. With `legacy` flex they fill it exactly.

Three numeric examples serve as goldens. Each is solved on a row.

| Area | Constraints | Flex | Segment widths |
|---|---|---|---|
| 80 | `length(20)`, `fill(1)`, `percentage(25)` | `start` | 20, 40, 20 |
| 60 | `percentage(25)`, `ratio(1, 3)`, `fill(1)` | `start` | 15, 20, 25 |
| 60 | `length(8)`, `length(8)`, `length(8)` | `space-between` | 8, 8, 8 with gaps of 18 |

## How the solver divides one row

The solver works in this order. It removes the margin from the area. It sizes the segments with `length`, `min`, `max`, `percentage` and `ratio`. It divides the remaining cells among the `fill` segments by weight. Then it places any space left over according to `flex`. When the segments cannot all fit, ratatui's priority rules apply and trailing segments shrink; a segment that gets 0 cells draws nothing.

<figure>
<svg viewBox="0 0 410 150" role="img" aria-label="Constraint solving on one row">
  <title>Constraint solving on one row</title>
  <desc>Three 8-cell segments, Open, Save and Quit, are separated by two 18-cell gaps, drawn along a row of 60 cells.</desc>
  <defs>
    <marker id="layout-flow-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M 0 0 L 10 5 L 0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="40" width="48" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="34" y="59" font-size="11" text-anchor="middle" fill="currentColor">Open</text>
  <rect x="58" y="40" width="122" height="30" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="119" y="59" font-size="11" text-anchor="middle" fill="currentColor">gap 18</text>
  <rect x="180" y="40" width="48" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="204" y="59" font-size="11" text-anchor="middle" fill="currentColor">Save</text>
  <rect x="228" y="40" width="122" height="30" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="289" y="59" font-size="11" text-anchor="middle" fill="currentColor">gap 18</text>
  <rect x="350" y="40" width="48" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="374" y="59" font-size="11" text-anchor="middle" fill="currentColor">Quit</text>
  <line x1="10" y1="92" x2="398" y2="92" stroke="currentColor" marker-end="url(#layout-flow-arrow)"/>
  <text x="10" y="112" font-size="11" fill="currentColor">Segments are placed left to right. Dashed boxes are gaps.</text>
  <text x="10" y="132" font-size="11" fill="currentColor">Row of 60 cells: 8 + 18 + 8 + 18 + 8 = 60.</text>
</svg>
<figcaption>Flex space-between divides the 36 leftover cells evenly between the three segments. Dashed boxes are gaps.</figcaption>
</figure>

A plugin never sees a rectangle. When it needs a size, it reads the `area` argument of `render`, or the size of a basic pane (see "Reading the area" below).

## Six layouts

The renders below show six layouts at 60 columns. Each has a ruler above it, with a digit every 10 columns; in a real view the ruler is not drawn. Panels use the rounded border that the Ilium client draws for every panel.

### Fixed sidebar and fill

The first child has `length(20)` and the second has `fill(1)`. The sidebar is always 20 cells wide, and the body takes the rest.

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

### Minimum and maximum

The first child has `min(30)`, the second `fill(1)` and the third `max(10)`. The first child is never narrower than 30 cells, and the third never wider than 10.

<figure>
<pre class="term"><span class="t-dim">0         1         2         3         4         5
012345678901234567890123456789012345678901234567890123456789</span>
╭─ Left ─────────────────────╮╭─ Middle ─────────╮╭─ Right ╮
│min(30)                     ││fill(1)           ││max(10) │
│                            ││                  ││        │
│                            ││                  ││        │
╰────────────────────────────╯╰──────────────────╯╰────────╯
</pre>
<figcaption>Rendered at 60 columns. <code>min(30)</code>, <code>fill(1)</code> and <code>max(10)</code>. The right segment stops growing at 10 cells and the middle one takes the 20 that remain.</figcaption>
</figure>

### Percentage and ratio

The first child has `percentage(25)`, which is 25 per cent of 60 cells, or 15. The second has `ratio(1, 3)`, which is one third, or 20. The third has `fill(1)` and takes the remaining 25.

<figure>
<pre class="term"><span class="t-dim">0         1         2         3         4         5
012345678901234567890123456789012345678901234567890123456789</span>
╭─ Nav ───────╮╭─ Index ──────────╮╭─ Detail ──────────────╮
│25 %         ││ratio(1, 3)       ││fill(1)                │
│             ││                  ││                       │
│             ││                  ││                       │
╰─────────────╯╰──────────────────╯╰───────────────────────╯
</pre>
<figcaption>Rendered at 60 columns. <code>percentage(25)</code> gives 15 cells, <code>ratio(1, 3)</code> gives 20, and <code>fill(1)</code> takes the last 25.</figcaption>
</figure>

### Flex with space-between

Three children with `length(8)` each, under `flex: space-between`. The leftover 36 cells are split into two equal gaps of 18, one between each pair of children.

<figure>
<pre class="term"><span class="t-dim">0         1         2         3         4         5
012345678901234567890123456789012345678901234567890123456789</span>
╭──────╮                  ╭──────╮                  ╭──────╮
│  Open│                  │  Save│                  │  Quit│
╰──────╯                  ╰──────╯                  ╰──────╯
</pre>
<figcaption>Rendered at 60 columns. Three <code>length(8)</code> segments with <code>flex: space-between</code>. The 36 leftover cells become two gaps of 18 cells.</figcaption>
</figure>

### Margin and spacing

Two children with `fill(1)` each, a margin of 2 cells at the left and right and 1 row at the top and bottom, and a `space(2)` gap. After the margin the area is 56 cells wide, and the gap takes 2 of them, so each child gets 27 cells.

<figure>
<pre class="term"><span class="t-dim">0         1         2         3         4         5
012345678901234567890123456789012345678901234567890123456789</span>

  ╭─ Draft ─────────────────╮  ╭─ Preview ───────────────╮
  │fill(1)                  │  │fill(1)                  │
  │                         │  │                         │
  │                         │  │                         │
  ╰─────────────────────────╯  ╰─────────────────────────╯

</pre>
<figcaption>Rendered at 60 columns and 7 rows. Margin 2 by 1 and <code>space(2)</code>. The blank first and last rows and the two blank columns at each side are the margin.</figcaption>
</figure>

### Overlap

`overlap(1)` lets two neighbours share one cell. Nodes paint in pre-order, so a later sibling overdraws an earlier one cell by cell. Where bordered blocks share cells, the host uses the box-drawing merge rule. In a horizontal layout the shared vertical edge becomes `┬` at the top and `┴` at the bottom. In a vertical layout the shared horizontal edge becomes `├` at the left and `┤` at the right. Rounded corners that become joints use the plain joints. When the two border types differ, the later sibling's glyph wins. Popups, then tooltips, paint after all content.

<figure>
<pre class="term"><span class="t-dim">0         1         2         3         4         5
012345678901234567890123456789012345678901234567890123456789</span>
╭─ Left ──────────────────────┬─ Right ────────────────────╮
│fill(1)                      │fill(1)                     │
│                             │                            │
│                             │                            │
╰─────────────────────────────┴────────────────────────────╯
</pre>
<figcaption>Rendered at 60 columns. <code>overlap(1)</code>. The two panels share column 30, and the host draws the shared edge with the joints <code>┬</code> and <code>┴</code>.</figcaption>
</figure>

## Reading the area

`render` receives `area`, the `columns` and `rows` the view may use. The kernel calls `render` again when that size changes, so a plugin can pick a different layout for a narrow view. A basic pane reads its size with `basic.size`. The plugin gets sizes only, never positions.

| Source | Gives | Use it for |
|---|---|---|
| `area` in `render` | `columns` and `rows` of the view | Choosing a direction or a segment count. |
| `basic.size` | Columns and rows of a basic pane | Sizing an own character grid. |
| `ui.display-width(text)` | Cell width of a string, with icon widths | Measuring a label before choosing `length`. |
| `ui.theme-extras().density` | `compact`, `normal` or `comfortable` | Choosing spacing and margins. |

Do not guess a width from the character count of a string that holds icons or wide glyphs; use `display-width`. Do not tune a layout to one terminal size. Express the ratios with `fill`, `percentage` and `ratio`, and use `length` only for things with a fixed size such as a button or a gutter.

## Sizes elsewhere in the widget catalogue

Several other widgets take sizes with the same vocabulary. The solver rules above apply to all of them.

| Widget | Where sizes appear | Notes |
|---|---|---|
| `popup`, `dialog` | `width` and `height` as `constraint`; `centered((width, height))` anchors | A `percentage` is of the view, a `length` is in cells. The host clamps a popup to the screen. |
| `split` | `position` in per mille (50 to 950), `min-first` and `min-second` in cells | The user moves the divider; the position is host state `widget-state::split`. |
| `scroll-view` | `content-rows` | The single child is laid out at that height and clipped to the area. |
| `table` | `columns` with `width: constraint`, `min-width` and `priority` | Narrow areas drop columns with the lowest priority first, never below `min-width`. |
| `layout` slot of `length(0)` | A child that takes no space | Keeps a closed dialog or an empty notice in the tree without drawing it. |

See [Layout and containers](/pages/widgets-layout-and-containers/) for these widgets.

## Worked example in Rust

This view stacks its two parts when it is narrower than 60 columns. It reads `area.columns` to decide, and it redraws on resize because the kernel calls `render` again.

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::layout::{Area, Constraint, Direction, Margin};
use ilium_plugin_sdk::types::ui::{UiTree, ViewTarget};
use ilium_plugin_sdk::ui::build::Tree;

const NARROW_COLUMNS: u16 = 60;

#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
struct NotesPanel;

impl Lifecycle for NotesPanel {}

impl Handler for NotesPanel {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

impl Views for NotesPanel {
    fn render(&mut self, _view: ViewTarget, area: Area) -> UiTree {
        let mut tree = Tree::new();
        let sidebar = tree.paragraph(vec![vec![Span::plain("Notes")]]).key("sidebar");
        let body = tree.paragraph(vec![vec![Span::plain("Select a note to read it.")]]).key("body");
        let (direction, constraints) = if area.columns < NARROW_COLUMNS {
            (Direction::Vertical, [Constraint::Length(4), Constraint::Fill(1)])
        } else {
            (Direction::Horizontal, [Constraint::Length(20), Constraint::Fill(1)])
        };
        let root = tree
            .layout(direction, constraints)
            .margin(Margin { horizontal: 1, vertical: 0 })
            .child(sidebar)
            .child(body);
        tree.finish(root)
    }
}
```

## Worked example in TypeScript

The same view. It also puts three buttons in a footer row with `space-between`, under a body that takes all the remaining rows.

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission, Area, ViewTarget } from '@ilium/plugin-sdk/types';
import { button, fill, layout, length, paragraph, span, tree } from '@ilium/plugin-sdk/ui/build';

const NARROW_COLUMNS = 60;

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  onEvents(_batch: Event[]): Submission[] {
    return [];
  },
  render(_view: ViewTarget, area: Area) {
    const narrow = area.columns < NARROW_COLUMNS;
    const split = layout({
      direction: narrow ? 'vertical' : 'horizontal',
      constraints: narrow ? [length(4), fill(1)] : [length(20), fill(1)],
      margin: { horizontal: 1, vertical: 0 },
    }, [
      paragraph([[span('Notes')]], { key: 'sidebar' }),
      paragraph([[span('Select a note to read it.')]], { key: 'body' }),
    ]);
    const footer = layout({
      direction: 'horizontal',
      constraints: [length(8), length(8), length(8)],
      flex: 'space-between',
    }, [
      button('Open', { key: 'open' }),
      button('Save', { key: 'save' }),
      button('Quit', { key: 'quit' }),
    ]);
    return tree(layout({ direction: 'vertical', constraints: [fill(1), length(3)] }, [split, footer]));
  },
});
```

## Errors and edge cases

| Case | Result |
|---|---|
| Number of children differs from the number of constraints | The builder stops the render with a message that names the layout key. A hand-built invalid tree traps the instance. |
| `ratio` with a denominator of 0 | The tree is invalid and traps the instance with `ui.invalid-tree`. |
| Constraints that need more cells than the area has | The solver follows ratatui's priority rules; trailing segments shrink, and a segment with 0 cells draws nothing. |
| `percentage` above 100 | The segment is clamped to the available space. |
| `ratio` above 1 | Clamped to 1. |
| `fill(0)` | 0 cells, unless every constraint is `fill(0)`. |
| A margin larger than the area | The inner area is empty and nothing is drawn. |
| `spacing: overlap(n)` with bordered children | The shared edge is drawn once with joint glyphs. |
| A tree of more nodes than the effective `ui_tree_nodes` limit (4,096 by default, up to 16,384 for T1) | The tree is invalid and traps the instance with `ui.too-large`. |

A trap ends the plugin instance, and the host applies the [restart and disable rule](/pages/reference-errors-and-limits/#restart-and-disable-rule): restart after 1 s, then 5 s, and disable at the third failure in 10 minutes.

## What the user sees

The user sees panels that follow the size of the view and reflow when the terminal is resized or the user drags the sidebar width. They never see a constraint. Because the host owns the solver, two plugins with the same constraints align exactly, and a view looks the same on every platform.

Requirement ids for this page: [WID-004], [WID-005], [WID-006], [WID-007]. See the [requirements index](/pages/reference-requirements/).

## Narrow terminals

A pane can be as narrow as 40 columns, and the solver gives each segment only what is left. A plugin SHOULD test every view at 40 columns by 12 rows, which is the size `lint.widget-missing-key` renders at, and SHOULD give a `min` constraint to a segment that must stay readable. [AUT-505] A fixed width in cells is wrong for text whose length depends on the language. A plugin SHOULD prefer `fill` and `percentage` for text segments. [AUT-506]

## See also

- [Layout and containers](/pages/widgets-layout-and-containers/) for `layout`, `block`, `split`, `scroll-view` and the other containers
- [The UI model](/pages/ui-model/)
- [Widget catalogue](/pages/widgets-index/)
- [Accessibility and theming](/pages/accessibility-and-theming/) for density and reduced motion
- [Errors and limits](/pages/errors-and-limits/)

API version: `ilium:plugin@0.1.0`.
