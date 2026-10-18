---
layout: page
title: Data display
permalink: /pages/widgets-data-display/
---

## Data display

Data widgets turn numbers into pictures. This family has ten widgets. `gauge` and `line-gauge` show one ratio. `sparkline` shows a short series in one row. `bar-chart` shows bars. `chart` plots datasets with axes, in a high resolution Braille render or as a text line. `canvas` draws shapes in a coordinate space. `calendar` shows a month. `progress-row` shows the state of one job. `meter` shows a segmented bar for costs and quotas. `minimap` shows the shape of a long text.

The plugin computes the numbers and the host draws them. Most data widgets are one way: they send nothing back unless the plugin sets the `focusable` flag or the widget has an action of its own, such as a click on a progress row.

## You need

- A grant to show the view: `ui.panels` for a panel ([UI panels](/pages/capability-ui-panels/)), or `panes.provide-kind` for a pane kind. `ui.invalidate` is always granted for views the plugin contributes.
- Rust with the `ilium-plugin-sdk` crate, or TypeScript with `@ilium/plugin-sdk`. Both target API `ilium:plugin@0.1.0`.
- The [widget catalogue](/pages/widgets-index/) for the common props, the widget keys, and the rule that a plugin renders only when it calls `ui.invalidate` or the view area changes.

<figure>
<svg viewBox="0 0 340 130" role="img" aria-label="A plugin sends numbers to the host, and the host draws them without sending actions back">
  <title>Data widgets are one way</title>
  <desc>The plugin computes numbers and sends a new tree after ui.invalidate. The host draws the widget and sends no action back.</desc>
  <defs>
    <marker id="dd-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="6" y="40" width="110" height="52" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="61" y="61" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
  <text x="61" y="78" font-size="10" text-anchor="middle" fill="currentColor">computes numbers</text>
  <rect x="224" y="40" width="110" height="52" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="279" y="61" font-size="12" text-anchor="middle" fill="currentColor">Host</text>
  <text x="279" y="78" font-size="10" text-anchor="middle" fill="currentColor">draws the widget</text>
  <line x1="118" y1="56" x2="220" y2="56" stroke="currentColor" marker-end="url(#dd-arrow)"/>
  <text x="169" y="50" font-size="10" text-anchor="middle" fill="currentColor">ui.invalidate</text>
  <line x1="118" y1="76" x2="220" y2="76" stroke="currentColor" marker-end="url(#dd-arrow)"/>
  <text x="169" y="90" font-size="10" text-anchor="middle" fill="currentColor">new ui-tree</text>
  <text x="170" y="118" font-size="10" text-anchor="middle" fill="currentColor">Most data widgets send no ui-action back.</text>
</svg>
<figcaption>Data widgets are mostly one way. The plugin computes the numbers and returns a new tree when they change.</figcaption>
</figure>

## How a data view updates

The host calls the plugin's `render` function when the view first opens, after `ui.invalidate` for that view, and when the area changes. The plugin answers with a full `ui-tree`. The host diffs the new tree against the last one by node `key`, and repaints the nodes that changed. Give a node a `key` that stays the same across renders, so that the diff can match it.

## The widgets

| Widget | Shows | Sends |
|---|---|---|
| `gauge` | One ratio as a bar with a centred label | nothing |
| `line-gauge` | One ratio as a single line | nothing |
| `sparkline` | A series in one row | nothing |
| `bar-chart` | Bars in groups | `selected` (a bar) when focusable |
| `chart` | Datasets on axes | `selected` (a point) when focusable |
| `canvas` | Shapes in a coordinate space | `activated` with a cell when focusable |
| `calendar` | One month (unstable) | `selected` (a day) when focusable |
| `progress-row` | The state of one job | `activated` |
| `meter` | A segmented bar | `selected` (a segment) when focusable |
| `minimap` | The shape of a long text | `activated`, `scrolled` |

Every widget has a builder. The required prop is the constructor argument, and the other props are setters. In Rust the setters are snake_case and chain on the node handle. In TypeScript they go in the props object in camelCase. A prop that is not set takes the default in the last column of its table.

## gauge

Mirrors `ratatui::widgets::Gauge`. It fills a bar with a ratio and draws a label over it.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `ratio` | `f64` (0 to 1) | required | The fraction of the bar that is filled. |
| `label` | `option<styled-text>` | none | Text centred over the bar. |
| `use-unicode` | `bool` | `true` | Whether partial cells use block characters. |
| `gauge-style` | `style` | empty | Colours of the filled and unfilled parts. |

Keys, mouse and actions: none. Roles: `accent` for the filled part and `muted` for the rest.

<figure>
<pre class="term">╭─ Disk use ───────────────────────────────────╮
│<span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-hi"> </span><span class="t-hi">6</span><span class="t-hi">7</span><span class="t-hi"> </span><span class="t-hi">%</span><span class="t-hi"> </span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-ok">█</span><span class="t-dim">░</span><span class="t-dim">░</span><span class="t-dim">░</span><span class="t-dim">░</span><span class="t-dim">░</span><span class="t-dim">░</span><span class="t-dim">░</span><span class="t-dim">░</span><span class="t-dim">░</span><span class="t-dim">░</span><span class="t-dim">░</span><span class="t-dim">░</span><span class="t-dim">░</span><span class="t-dim">░</span><span class="t-dim">░</span>│
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. A <code>gauge</code> at 67 per cent with its label centred over the bar. The filled part uses <code>accent</code> (shown as success green here) and the rest uses <code>muted</code>.</figcaption>
</figure>

**Rust**

```rust
let disk = tree
    .gauge(0.67)
    .label(vec![Span::plain("67 %")])
    .gauge_style(Style::role(ColorRole::Accent))
    .key("disk");
```

**TypeScript**

```ts
const disk = gauge(0.67, {
  key: 'disk',
  label: [span('67 %')],
  gaugeStyle: { fg: role('accent') },
});
```

## line-gauge

Mirrors `ratatui::widgets::LineGauge`. It draws the same ratio as a single line.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `ratio` | `f64` (0 to 1) | required | The fraction of the line that is filled. |
| `label` | `option<styled-text>` | none | Text before the line. |
| `filled-style`, `unfilled-style` | `style` | empty | Colours of the two parts. |
| `line-set` | `normal`, `thick` or `double` | `normal` | The characters that draw the line. |

The filled part is `━` and the rest is `─`. This is the line the client uses for the countdown of a toast. Actions: none.

<figure>
<pre class="term">╭─ Context window ─────────────────────────────╮
│ <span class="t-hi">67%</span> <span class="t-ok">━━━━━━━━━━━━━━━━━━━━━━━━━━</span><span class="t-dim">─────────────</span>  │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. A <code>line-gauge</code> with the label before the line. The filled part is <code>━</code> and the rest is <code>─</code>.</figcaption>
</figure>

**Rust**

```rust
let context = tree
    .line_gauge(0.67)
    .label(vec![Span::plain("67%")])
    .key("context");
```

**TypeScript**

```ts
const context = lineGauge(0.67, { key: 'context', label: [span('67%')] });
```

## sparkline

Mirrors `ratatui::widgets::Sparkline`. Each entry is one column. A `none` entry leaves its column empty, which shows a gap in the data.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `data` | `list<option<u64>>` | required | One value per column. |
| `max` | `option<u64>` | none | The value that fills the full height. Without it, the largest value fills it. |
| `direction` | `left-to-right` or `right-to-left` | `left-to-right` | Which end the newest value uses. |
| `style` | `style` | empty | The colour of the bars. |

Mouse: hovering a column shows its value as a tooltip. Roles: `accent`. Glyphs: `▁▂▃▄▅▆▇█`.

<figure>
<pre class="term">╭─ Sessions per hour ──────────────────────────╮
│ <span class="t-ok">▁▂▃▄▅▆▇█ ▇▆▅▄▃▂▁▂▄▆█</span>   <span class="t-dim">peak 8 at 07:00</span>       │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. A <code>sparkline</code> of 20 samples with one gap. Each sample is one cell wide and picks one of the eight block heights <code>▁▂▃▄▅▆▇█</code>.</figcaption>
</figure>

**Rust**

```rust
let hourly = tree
    .sparkline(vec![Some(1), Some(2), Some(8), None, Some(5)])
    .max(Some(8))
    .style(Style::role(ColorRole::Accent))
    .key("hourly");
```

**TypeScript**

```ts
const hourly = sparkline([1, 2, 8, null, 5], {
  key: 'hourly',
  max: 8,
  style: { fg: role('accent') },
});
```

## bar-chart

Mirrors `ratatui::widgets::BarChart`. Bars are grouped: each `bar-group` has a label and one or more bars. Each bar has a `value: u64`, a `label` drawn under the bar, a `text-value` drawn on the bar, and a `style`.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `groups` | `list<bar-group>` | required | The groups, each with a label and its bars. |
| `bar-width` | `u16` | `1` | Width of one bar in cells. |
| `bar-gap` | `u16` | `1` | Cells between bars in a group. |
| `group-gap` | `u16` | `0` | Cells between groups. |
| `direction` | `vertical` or `horizontal` | `vertical` | Which way the bars grow. |
| `max` | `option<u64>` | none | The value that fills the full length. Without it, the largest value fills it. |

When the node is focusable, Left and Right move between bars and a click on a bar sends `selected` with `index(u32)`. Roles: `accent` for bars and `muted` for labels. Glyphs: `█` and the eighth blocks.

<figure>
<pre class="term">╭─ Commands per day ───────────────────────────╮
│       <span class="t-ok">▂▂▂</span>         <span class="t-ok">███</span>                        │
│       <span class="t-ok">███</span>         <span class="t-ok">███</span>                        │
│       <span class="t-ok">███</span>   <span class="t-ok">▆▆▆</span>   <span class="t-ok">███</span>                        │
│ <span class="t-ok">███</span>   <span class="t-ok">███</span>   <span class="t-ok">███</span>   <span class="t-ok">███</span>   <span class="t-ok">▂▂▂</span>                  │
│ <span class="t-ok">███</span>   <span class="t-ok">███</span>   <span class="t-ok">███</span>   <span class="t-ok">███</span>   <span class="t-ok">███</span>                  │
│ <span class="t-ok">███</span>   <span class="t-ok">███</span>   <span class="t-ok">███</span>   <span class="t-ok">███</span>   <span class="t-ok">███</span>                  │
│ Mon   Tue   Wed   Thu   Fri                  │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. A vertical <code>bar-chart</code>, three cells per bar with a gap of three. The top of a bar uses an eighth block when the value falls between two rows.</figcaption>
</figure>

**Rust**

```rust
let commands = tree
    .bar_chart(vec![BarGroup::new("week", vec![
        BarItem::new(4).label("Mon"),
        BarItem::new(7).label("Tue"),
        BarItem::new(5).label("Wed"),
    ])])
    .bar_width(3)
    .bar_gap(3)
    .key("commands");
```

**TypeScript**

```ts
const commands = barChart([{
  label: 'week',
  bars: [
    { value: 4, label: 'Mon' },
    { value: 7, label: 'Tue' },
    { value: 5, label: 'Wed' },
  ],
}], { key: 'commands', barWidth: 3, barGap: 3 });
```

## chart

Mirrors `ratatui::widgets::Chart`. A chart has datasets, two axes and an optional legend. Each dataset draws its points as scatter markers, a line or bars, with its own marker and colour.

| Record or prop | Field | Type | Meaning |
|---|---|---|---|
| `dataset` | `name` | `string` | Legend text. |
| | `marker` | `dot`, `block`, `bar`, `braille`, `half-block`, `quadrant`, `sextant` or `octant` | Glyph set for the points. |
| | `graph-type` | `scatter`, `line` or `bar` | How the points are drawn. |
| | `points` | `list<tuple<f64, f64>>` | The data, in axis units. |
| | `style` | `style` | Colour of the dataset. |
| | `gaps` | `bool` | A `NaN` y value ends the line segment. |
| | `resample` | `none`, `mean`, `max` or `min-max` | The host fits the points to the width. |
| | `fill` | `bool` | Fill the area under the line. |
| `axis` | `title` | `option<styled-text>` | Axis title, shown beside the axis. |
| | `bounds` | `tuple<f64, f64>` | The lowest and highest value on the axis. |
| | `labels` | `list<styled-text>` | Tick labels drawn along the axis. |
| `legend-position` | | `option<legend-position>` | `top`, `top-right`, `top-left`, `left`, `right`, `bottom`, `bottom-right` or `bottom-left`. None hides the legend. |
| `render` | | `braille` or `text-line` | The render. Default `braille`. Where the terminal cannot draw Braille, the host falls back to `text-line`. |
| `y-label-width` | | `option<u16>` | The width of the y labels in cells. |

When the node is focusable, Left and Right move a cursor along the points and `selected` with `index(u32)` reports the point. Hovering a point shows it as a tooltip. Roles: the dataset styles, and `muted` for the axes.

### Braille render

The default render draws each dataset on a grid of Braille dots, two dots wide and four dots high in each cell. A line of 36 cells has 72 dots across.

<figure>
<pre class="term">╭─ Spend ────────────────────────────────────╮
│ 8 <span class="t-dim">│</span><span class="t-ok">⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣀⡤⠤⠖⠒⠉⠉⠉⠉</span>    │
│   <span class="t-dim">│</span><span class="t-ok">⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣀⣠⠤⠴⠒⠋⠁⠀⠀⠀⠀⠀⠀⠀⠀</span>    │
│ 6 <span class="t-dim">│</span><span class="t-ok">⠉⠁⠉⠉⠉⠈⠉⠉⠈⠉⠉⠁⠉⠉⠁⠉⠉⢁⣩⠟⠋⠉⠉⠉⠈⠉⠉⠁⠉⠉⠁⠉⠉⠉⠈⠉</span>    │
│   <span class="t-dim">│</span><span class="t-ok">⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⡤⠖⠚⠉⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀</span>    │
│ 4 <span class="t-dim">│</span><span class="t-ok">⠀⠀⠀⠀⠀⠀⠀⣀⣀⡤⠤⠞⠉⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀</span>    │
│   <span class="t-dim">│</span><span class="t-ok">⠀⠀⠀⣠⠴⠒⠋⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀</span>    │
│ 2 <span class="t-dim">│</span><span class="t-ok">⣠⠖⠋⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀</span>    │
│   <span class="t-dim">│</span><span class="t-ok">⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀</span>    │
│ 0 <span class="t-dim">┼────────────────────────────────────</span>    │
│      0             5                10     │
│ <span class="t-ok">⠒</span> cost per day   <span class="t-warn">⠒</span> budget                  │
╰────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 46 columns. A <code>chart</code> with the default <code>braille</code> render. Each cell holds a 2 by 4 grid of dots, so a line has eight times the cell resolution. The axes use <code>muted</code>. The legend sits at the bottom.</figcaption>
</figure>

**Rust**

```rust
// types::ui::{Dataset, GraphType, Marker, Axis}
let spend = tree
    .chart(
        vec![
            Dataset::new("cost per day", self.cost_points.clone())
                .marker(Marker::Braille)
                .graph_type(GraphType::Line),
            Dataset::new("budget", vec![(0.0, 6.0), (10.0, 6.0)])
                .marker(Marker::Braille)
                .graph_type(GraphType::Line),
        ],
        Axis { title: None, bounds: (0.0, 10.0), labels: vec![] },
        Axis { title: None, bounds: (0.0, 8.0), labels: vec![] },
    )
    .legend_position(Some(LegendPosition::Bottom))
    .key("spend");
```

**TypeScript**

```ts
const spend = chart(
  [
    { name: 'cost per day', points: costPoints, marker: 'braille', graphType: 'line' },
    { name: 'budget', points: [[0, 6], [10, 6]], marker: 'braille', graphType: 'line' },
  ],
  { bounds: [0, 10], labels: [] },
  { bounds: [0, 8], labels: [] },
  { key: 'spend', legendPosition: 'bottom' },
);
```

### Text-line render

With `render: text-line` the host draws the datasets as a line of box-drawing glyphs, with the y labels right-aligned in `y-label-width` cells. It works over plain terminals and in places where Braille is hard to read. Use `gaps` for a series with holes, and `resample` when there are more points than cells: `mean` and `max` keep one value per cell, and `min-max` keeps the extremes of each cell.

<figure>
<pre class="term">╭─ Tokens per minute ────────────────────────╮
│   12.0┤           ╭╮                       │
│   10.5┤          ╭╯╰╮                      │
│    9.0┤    ╭╮    │  ╰╮                     │
│    7.5┤   ╭╯╰╮  ╭╯   ╰                     │
│    6.0┤ ╭╮│  ╰╮ │                          │
│    4.5┤╭╯╰╯   ╰─╯                          │
│    3.0┼╯                                   │
╰────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 46 columns. A <code>chart</code> with <code>render: text-line</code>. The host draws the curve with the box-drawing glyphs <code>╭ ╮ ╰ ╯ ─ │</code>, the axis with <code>┤</code> and <code>┼</code>, and the y labels right-aligned in <code>y-label-width</code> cells.</figcaption>
</figure>

**Rust**

```rust
// types::ui::{Dataset, ChartRender, Resample}
let tokens = tree
    .chart(
        vec![
            Dataset::new("input", self.input_series()).gaps(true).resample(Resample::MinMax),
            Dataset::new("output", self.output_series()).gaps(true).resample(Resample::MinMax),
        ],
        Axis { title: None, bounds: (0.0, 60.0), labels: vec![] },
        Axis { title: None, bounds: (0.0, 12.0), labels: vec![] },
    )
    .render(ChartRender::TextLine)
    .y_label_width(6)
    .key("tokens");
```

**TypeScript**

```ts
const tokens = chart(
  [
    { name: 'input', points: inputSeries(), gaps: true, resample: 'min-max' },
    { name: 'output', points: outputSeries(), gaps: true, resample: 'min-max' },
  ],
  { bounds: [0, 60], labels: [] },
  { bounds: [0, 12], labels: [] },
  { key: 'tokens', render: 'text-line', yLabelWidth: 6 },
);
```

## canvas

Mirrors `ratatui::widgets::canvas::Canvas`. The plugin gives the bounds of a coordinate space and a list of shapes. The `marker` chooses the resolution: `dot` is one cell per point, and `braille` gives a sub-cell grid of dots. The default is `braille`. Canvas is also the glyph plane that a plugin can draw a map on.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `x-bounds`, `y-bounds` | `tuple<f64, f64>` | required | The coordinate space. |
| `marker` | the marker enum of `chart` | `braille` | The resolution of drawing. |
| `shapes` | `list<shape>` | empty | The shapes, drawn in list order. |

| Shape | Fields |
|---|---|
| `canvas-line` | `x1`, `y1`, `x2`, `y2`, `color` |
| `canvas-rectangle` | `x`, `y`, `width`, `height`, `color` |
| `canvas-circle` | `x`, `y`, `radius`, `color` |
| `canvas-points` | `coords: list<tuple<f64, f64>>`, `color` |
| `canvas-label` | `x`, `y`, `text: string` |
| `canvas-map` | `resolution` (`low` or `high`), `color`, and `land-color`, `sea-color`, `projection` (`equirectangular`), `bounds` |

When the node is focusable, a click sends `activated` with `cell(cell-position)`. The `color` field takes a colour in the same form as `style.fg`. See [Accessibility and theming](/pages/accessibility-and-theming/). For a surface that the plugin fills cell by cell, use `surface` ([Media and embedded](/pages/widgets-media-and-embedded/)).

<figure>
<pre class="term">╭─ Topology ─────────────────────────────────╮
│ <span class="t-ok">⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀</span>       │
│ <span class="t-ok">⠀⠀⡏⠉node A⠉⠉⠉⠉⠉⡇⠀⠀⠀⠀⣤⠀⠀⢀⡠⠤⠤⠤⣀⠀⠛⠀⠀⠀⠀⠀</span>       │
│ <span class="t-ok">⠀⠀⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡇⠀⠀⠀⠀⠀⢀⡔⠉⠀⠀⠀⠀⠈⠑⣄⠀⠀⠀⠀⠀</span>       │
│ <span class="t-ok">⠀⠀⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡇⠀⠀⠀⠀⠀⡎⠀⠀⠀⠀⠀⠀⠀⠀⠈⡆⠀⠀⠀⠀</span>       │
│ <span class="t-ok">⠀⠀⠧⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠇⠀⠀⠀⠀⠀⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⡇⠀⠀⠀⠀</span>       │
│ <span class="t-ok">⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠘⢆⠀⠀⠀⠀⠀⠀⢀⠞⠀⠀⠀⠀⠀</span>       │
│ <span class="t-ok">⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠙⠢⠤⠤⠤⠚⠁⠀⠀⠀⣤⠀⠀</span>       │
│ <span class="t-ok">⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀</span>       │
╰────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 46 columns. A <code>canvas</code> with the default <code>braille</code> marker: a rectangle, a circle, three points and a <code>canvas-label</code> (the label cells replace the dots under them).</figcaption>
</figure>

**Rust**

```rust
// types::ui::Shape
let topology = tree
    .canvas((0.0, 72.0), (0.0, 32.0))
    .shapes(vec![
        Shape::rectangle(4.0, 4.0, 26.0, 14.0, Color::role(ColorRole::Accent)),
        Shape::circle(52.0, 16.0, 10.0, Color::role(ColorRole::Accent)),
        Shape::points(vec![(40.0, 6.0), (60.0, 4.0)], Color::role(ColorRole::Success)),
        Shape::label(6.0, 28.0, "node A"),
    ])
    .key("topology");
```

**TypeScript**

```ts
const topology = canvas([0, 72], [0, 32], {
  key: 'topology',
  shapes: [
    { tag: 'canvas-rectangle', val: { x: 4, y: 4, width: 26, height: 14, color: role('accent') } },
    { tag: 'canvas-circle', val: { x: 52, y: 16, radius: 10, color: role('accent') } },
    { tag: 'canvas-points', val: { coords: [[40, 6], [60, 4]], color: role('success') } },
    { tag: 'canvas-label', val: { x: 6, y: 28, text: 'node A' } },
  ],
});
```

## calendar

Mirrors `ratatui::widgets::calendar::Monthly`. It shows one month with Monday first. Marked days are drawn with a style of their own. The widget is unstable.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `year` | `u32` | required | The year to show. |
| `month` | `u32` (1 to 12) | required | The month to show. |
| `marked` | `list<marked-day>` | empty | Days with a style of their own. |
| `show-month-header` | `bool` | `true` | Whether the month and year are shown above the grid. |
| `show-weekdays` | `bool` | `true` | Whether the weekday header is shown. |

A `marked-day` record has two fields only: `day: u8` and `style: style`. Actions: `selected` with `index(u32)`, the day of the month, when the user clicks a day or moves with the arrow keys. Roles: `accent` for today and `selection`.

<figure>
<pre class="term">╭─ October 2026 ─────────────╮
│ Mo Tu We Th Fr Sa Su       │
│             1  2  3  4     │
│  5  6  7  8  9 10 <span class="t-hi">11</span>       │
│ 12 13 14 15 16 17 18       │
│ 19 20 21 22 23 24 25       │
│ 26 27 28 29 30 31          │
╰────────────────────────────╯
</pre>
<figcaption>Rendered at 30 columns. A <code>calendar</code> (unstable) with the 11th marked. Weeks start on Monday.</figcaption>
</figure>

**Rust**

```rust
let october = tree
    .calendar(2026, 10)
    .marked(vec![MarkedDay { day: 11, style: Style::role(ColorRole::Accent) }])
    .key("october");
```

**TypeScript**

```ts
const october = calendar(2026, 10, {
  key: 'october',
  marked: [{ day: 11, style: { fg: role('accent') } }],
});
```

## progress-row

`progress-row` is one monitor row of the progress footer. It shows the state of one job as an icon, a state name, a percentage and a bar. It is the row that the Ilium progress monitors use.

| Prop | Type | Meaning |
|---|---|---|
| `state` | `waiting`, `running`, `done` or `error` | The state of the job. |
| `title` | `styled-text` | The name of the job. |
| `percent` | `option<f32>` | The percentage from 0 to 100. None draws an empty bar. |
| `message` | `option<string>` | A short line after the bar. |
| `details` | `option<styled-lines>` | Longer text, shown as a tooltip on hover. |

Actions: `activated` with `none`, on Enter or on a click. Roles: `accent` for running, `success` for done, `danger` for error, and `muted` for waiting. The state set is closed (`progress-state` in the WIT has no degraded state). Glyphs: `▶` running, `✓` done, `✕` error, `○` waiting, and a bar of `█` over `░`.

<figure>
<pre class="term">╭─ Progress ─────────────────────────────────────────────╮
│ <span class="t-hi">▶ RUNNING  42%</span>  <span class="t-hi">████████</span><span class="t-dim">░░░░░░░░░░░░</span>  152 of 412       │
│ <span class="t-ok">✓ DONE  100%</span>  <span class="t-ok">████████████████████</span><span class="t-dim"></span>                     │
│ <span class="t-err">✕ ERROR  17%</span>  <span class="t-err">███</span><span class="t-dim">░░░░░░░░░░░░░░░░░</span>  3 failed           │
│ <span class="t-err">✕ MONITOR FAILED  0%</span>  <span class="t-dim">░░░░░░░░░░░░░░░░░░░░</span>             │
│ <span class="t-dim">○ NOT STARTED</span>                                          │
╰────────────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 58 columns. Five <code>progress-row</code> states. The status prefix is bold in the state colour: <code>accent</code> for running, <code>success</code> for done, <code>danger</code> for error and <code>muted</code> for waiting. The bar is <code>█</code> filled and <code>░</code> unfilled.</figcaption>
</figure>

**Rust**

```rust
// types::ui::ProgressState
let job = tree
    .progress_row("Replaying 412 sessions")
    .state(ProgressState::Running)
    .percent(37.0)
    .message("session 152 of 412")
    .key("job");
```

**TypeScript**

```ts
const job = progressRow('Replaying 412 sessions', {
  key: 'job',
  state: 'running',
  percent: 37,
  message: 'session 152 of 412',
});
```

## meter

`meter` is a segmented bar. It shows how a total splits into parts, such as the cost of a month against a budget. A segment has a `label`, a `value` and a `tone`. The tone is one of `info`, `success`, `warning`, `danger` and `muted`, and it picks the colour. The same record is used by the settings call `set-meter`.

| Prop | Type | Meaning |
|---|---|---|
| `segments` | `list<meter-segment>` | At most 8 segments. |
| `max` | `f64` | The value that fills the whole bar. |
| `label` | `option<styled-text>` | Text before the bar. |
| `show-values` | `bool` | Show the value of each segment. |
| `orientation` | `horizontal` or `vertical` | The direction of the bar. |

When the node is focusable, Left and Right move between segments and `selected` with `index(u32)` reports the segment. Hovering a segment shows its label and value. Glyphs: `█` for segments and `░` for the remainder.

<figure>
<pre class="term">╭─ Spend this month ───────────────────────────────╮
│ <span class="t-hi">█████████████████████</span><span class="t-err">████</span><span class="t-dim">░░░░░░░░░░░░░░░</span>  $5.00  │
│ <span class="t-hi">█</span> today $4.20   <span class="t-err">█</span> over budget $0.80              │
╰──────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 52 columns. A <code>meter</code> with two segments and <code>max</code> 5.0, with <code>show-values</code> on. Each segment uses the colour of its <code>tone</code>. The remainder up to <code>max</code> is <code>░</code>.</figcaption>
</figure>

**Rust**

```rust
// types::ui::MeterSegment; types::text::Tone
let spend = tree
    .meter(vec![
        MeterSegment { label: "today".into(), value: 4.20, tone: Tone::Info },
        MeterSegment { label: "over budget".into(), value: 0.80, tone: Tone::Danger },
    ])
    .max(5.0)
    .show_values(true)
    .key("spend");
```

**TypeScript**

```ts
const spend = meter([
  { label: 'today', value: 4.2, tone: 'info' },
  { label: 'over budget', value: 0.8, tone: 'danger' },
], { key: 'spend', max: 5, showValues: true });
```

## minimap

`minimap` draws the shape of a long text in a narrow column. The plugin gives one density value for each line bucket and the host draws it with the density ramp `· - = ≡ ≣ █`, from light to heavy. The host also draws the marks and the viewport box.

| Prop | Type | Meaning |
|---|---|---|
| `density` | `list<u8>` | One value from 0 to 255 for each line bucket. |
| `marks` | `list<minimap-mark>` | Coloured marks: `line`, `color` and `lane` (0 to 2). |
| `viewport` | `tuple<u32, u32>` | The first and last visible line. |
| `total-lines` | `u32` | The number of lines in the text. |

Actions: `activated` with `index(u32)`, the line, when the user clicks, and `scrolled` with `index(u32)` when the user drags the viewport. Keys: Up and Down move the viewport box. The plugin moves its own view on `scrolled`. Roles: `muted` for the density and the colours of the marks.

<figure>
<pre class="term">╭─ notes.md ─────────────────────╮
│ # Release notes           <span class="t-dim">≣</span>    │
│                           <span class="t-dim">·</span>    │
│ Fixes the detection loop. <span class="t-hi">│</span><span class="t-dim">=</span><span class="t-hi">│</span>  │
│                          <span class="t-hi">│</span><span class="t-dim">·</span><span class="t-hi">│</span>   │
│ - faster startup         <span class="t-hi">│</span><span class="t-dim">-</span><span class="t-hi">│</span>   │
│ - fewer redraws          <span class="t-hi">│</span><span class="t-dim">-</span><span class="t-hi">│</span>   │
│                           <span class="t-dim">·</span>    │
│ ```                       <span class="t-dim">≡</span>    │
│ run()                     <span class="t-dim">≡</span>    │
│ ```                       <span class="t-dim">≡</span>    │
│                           <span class="t-dim">·</span>    │
╰────────────────────────────────╯
</pre>
<figcaption>Rendered at 34 columns. A <code>minimap</code> beside a text view. Each row is one line bucket drawn with the density ramp <code>· - = ≡ ≣ █</code>. The two <code>│</code> columns mark the viewport.</figcaption>
</figure>

**Rust**

```rust
// types::ui::MinimapMark
let overview = tree
    .minimap(self.density.clone())
    .marks(vec![MinimapMark { line: 120, color: Color::role(ColorRole::Danger), lane: 1 }])
    .viewport((self.first_line, self.last_line))
    .total_lines(self.total_lines)
    .key("overview");
```

**TypeScript**

```ts
const overview = minimap(density, {
  key: 'overview',
  marks: [{ line: 120, color: role('danger'), lane: 1 }],
  viewport: [firstLine, lastLine],
  totalLines: totalLines,
});
```

## Where the tooltip comes from

A data widget with a hover tooltip (a sparkline column, a chart point, a meter segment, the details of a progress row) draws it with the same rounded block as every other tooltip, after the 500 ms delay. [The UI model](/pages/ui-model/) shows it.

## Worked example in Rust

This plugin panel shows the context window as a line gauge, a token series as a text line chart, a job as a progress row and the month's spend as a meter. It keeps its data in fields, and the host calls `render` each time the plugin calls `ui::invalidate`.

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::layout::{Area, Constraint, Direction};
use ilium_plugin_sdk::types::ui::{Axis, Dataset, MeterSegment, ProgressState, UiTree, ViewTarget};
use ilium_plugin_sdk::types::text::Tone;
use ilium_plugin_sdk::types::ui::{ChartRender, Resample};
use ilium_plugin_sdk::ui::build::Tree;

#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct UsagePanel {
    pub context_ratio: f64,
    pub tokens: Vec<(f64, f64)>,
    pub percent: f32,
    pub spent: f64,
}

impl Views for UsagePanel {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let context = tree.line_gauge(self.context_ratio).key("context");
        let tokens = tree
            .chart(
                vec![Dataset::new("tokens", self.tokens.clone()).resample(Resample::MinMax)],
                Axis { title: None, bounds: (0.0, 60.0), labels: vec![] },
                Axis { title: None, bounds: (0.0, 12.0), labels: vec![] },
            )
            .render(ChartRender::TextLine)
            .y_label_width(6)
            .key("tokens");
        let job = tree
            .progress_row("Replaying sessions")
            .state(ProgressState::Running)
            .percent(self.percent)
            .key("job");
        let spend = tree
            .meter(vec![MeterSegment { label: "today".into(), value: self.spent, tone: Tone::Info }])
            .max(5.0)
            .show_values(true)
            .key("spend");
        let column = tree
            .layout(
                Direction::Vertical,
                [Constraint::Length(1), Constraint::Fill(1), Constraint::Length(1), Constraint::Length(2)],
            )
            .child(context)
            .child(tokens)
            .child(job)
            .child(spend);
        let frame = tree.block().title("Usage").child(column);
        tree.finish(frame)
    }
}
```

`tree.finish` fills the version and the node indices and returns the `UiTree` that `render` must return. Call `ui::invalidate(view)` in the event handler after a field changes.

## Worked example in TypeScript

The same panel in TypeScript. The builders take the required prop first, then the props object, then the child handles.

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import {
  block, chart, fill, layout, length, lineGauge, meter, progressRow, tree,
} from '@ilium/plugin-sdk/ui/build';

let contextRatio = 0.67;
let tokens: [number, number][] = [];
let percent = 37;
let spent = 4.2;

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  activate(_context) {},
  onEvents(_batch) { return []; },
  render(_view, _area) {
    const context = lineGauge(contextRatio, { key: 'context' });
    const tokenChart = chart(
      [{ name: 'tokens', points: tokens, resample: 'min-max' }],
      { bounds: [0, 60], labels: [] },
      { bounds: [0, 12], labels: [] },
      { key: 'tokens', render: 'text-line', yLabelWidth: 6 },
    );
    const job = progressRow('Replaying sessions', { key: 'job', state: 'running', percent });
    const spend = meter([{ label: 'today', value: spent, tone: 'info' }], {
      key: 'spend', max: 5, showValues: true,
    });
    const column = layout({
      direction: 'vertical',
      constraints: [length(1), fill(1), length(1), length(2)],
    }, [context, tokenChart, job, spend]);
    return tree(block({ titles: [{ text: 'Usage', position: 'top', alignment: 'left' }] }, [column]));
  },
});
```

Style fields that are not given take their empty value, so `{ fg: role('success') }` leaves `bg`, `add` and `remove` empty.

## Edge cases and errors

Data widgets return no error codes. Their mistakes show up as a trap or as a quiet empty draw.

| Case | Result |
|---|---|
| `ratio` below 0 or above 1 | The host clamps it to the range. `NaN` draws as 0. |
| `sparkline` or `bar-chart` with no entries, or a sparkline with only `none` | The frame draws with no bars or columns. |
| A `value` above `max` | The bar fills the full length. |
| A dataset with no points | The dataset draws nothing. |
| A `NaN` y value without `gaps` | The point is skipped. With `gaps` it ends the line segment. |
| `resample` with fewer points than cells | No change. |
| Points or shapes outside the bounds | They are clipped at the edge of the plot. |
| `marked` day outside the month | The entry is ignored. |
| `progress-row` with `percent` above 100 | The host clamps it to 100. |
| `meter` with more than 8 segments | The tree is invalid. |
| `meter` segments that sum to more than `max` | The bar is full, and the last segments are cut. |
| `minimap` with empty `density` | The column draws empty. |
| `month` outside 1 to 12, or axis or canvas bounds where the low value is not below the high value | The builder stops the render with a message that names the widget, and the invalid tree traps the instance. |
| A tree of more nodes than the effective `ui_tree_nodes` limit (4,096 by default) | The tree is invalid and traps the instance. |
| `ui.invalidate` for a view the plugin does not declare | The call is ignored, and the host writes one `warn` log line that names the view. |

A trap ends the plugin instance, and the host applies the [restart and disable rule](/pages/reference-errors-and-limits/#restart-and-disable-rule): restart after 1 s, then 5 s, and disable at the third failure in 10 minutes.

## What the user sees

The view repaints when the plugin returns a new tree. Most data widgets take no key input of their own and send nothing back, so a key press on them does nothing unless the plugin sets the `focusable` flag or the widget has an action of its own. Colours come from the theme and the shared look, and the user can switch to stable glyphs. Under reduced motion this is guidance, not a tested rule: a plugin should avoid redrawing a chart more often than its data changes.

Requirement ids for this page: [WID-022], [WID-023], [WID-024]. See the [requirements index](/pages/reference-requirements/).

## See also

- [Widget catalogue](/pages/widgets-index/)
- [Layout and containers](/pages/widgets-layout-and-containers/)
- [Media and embedded](/pages/widgets-media-and-embedded/) for `surface`, `image` and `video`
- [The UI model](/pages/ui-model/)
- [Accessibility and theming](/pages/accessibility-and-theming/)
- [Errors and limits](/pages/reference-errors-and-limits/)

API version: `ilium:plugin@0.1.0`.
