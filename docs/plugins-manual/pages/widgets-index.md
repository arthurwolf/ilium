---
layout: page
title: Widget catalogue
permalink: /pages/widgets-index/
---

## Widget catalogue

The Ilium plugin UI has 51 widgets in 7 families. A plugin never draws cells. It describes a view as a `ui-tree`, a flat list of nodes, and the host lays it out, paints it and sends user actions back as events. This page lists every widget, links to the page of its family, and states the rules that all widgets share.

## You need

- A grant to show a view: `ui.panels` for a panel, a settings page or an onboarding step, or `panes.provide-kind` for a pane kind. `ui.invalidate` is always granted for views the plugin contributes.
- The Rust crate `ilium-plugin-sdk` or the npm package `@ilium/plugin-sdk`. Both target API `ilium:plugin@0.1.0`. Every example in this manual is shown in both languages.
- For the model behind the tree, read [The UI model](/pages/ui-model/) and [Layout](/pages/layout/).

## The seven families

| Family | Widgets | Page |
|---|---|---|
| Layout and containers | `layout`, `block`, `tabs`, `popup`, `clear`, `scrollbar`, `split`, `separator`, `dialog`, `scroll-view` | [Layout and containers](/pages/widgets-layout-and-containers/) |
| Text | `paragraph`, `markdown`, `leader-row`, `key-hints`, `diff-view` | [Text](/pages/widgets-text/) |
| Collections | `%list`, `table`, `tree-list`, `menu`, `check-list`, `list-editor` | [Collections](/pages/widgets-collections/) |
| Data display | `gauge`, `line-gauge`, `sparkline`, `bar-chart`, `chart`, `canvas`, `calendar`, `progress-row`, `meter`, `minimap` | [Data display](/pages/widgets-data-display/) |
| Input | `button`, `toggle`, `choice`, `number`, `text-input`, `text-area`, `schema-form`, `key-capture`, `color-picker`, `path-field`, `location-picker` | [Input](/pages/widgets-input/) and [Pickers](/pages/widgets-pickers/) |
| Status and feedback | `badge`, `spinner`, `notice`, `icon` | [Status and feedback](/pages/widgets-status-and-feedback/) |
| Media and embedded | `image`, `surface`, `code-view`, `terminal-view`, `video` | [Media and embedded views](/pages/widgets-media-and-embedded/) |

## All 51 widgets

| Widget | Family | What it is | Page |
|---|---|---|---|
| `layout` | Layout | Splits an area into rows or columns with constraints | [Layout and containers](/pages/widgets-layout-and-containers/) |
| `block` | Layout | A rounded border with titles and padding | [Layout and containers](/pages/widgets-layout-and-containers/) |
| `tabs` | Layout | A tab strip with a selected tab | [Layout and containers](/pages/widgets-layout-and-containers/) |
| `popup` | Layout | A modal or non-modal overlay | [Layout and containers](/pages/widgets-layout-and-containers/) |
| `clear` | Layout | Clears the cells under an overlay | [Layout and containers](/pages/widgets-layout-and-containers/) |
| `scrollbar` | Layout | A track and thumb for a scrollable area | [Layout and containers](/pages/widgets-layout-and-containers/) |
| `split` | Layout | Two panes with a draggable divider | [Layout and containers](/pages/widgets-layout-and-containers/) |
| `separator` | Layout | A horizontal or vertical rule | [Layout and containers](/pages/widgets-layout-and-containers/) |
| `dialog` | Layout | A titled dialog with action buttons | [Layout and containers](/pages/widgets-layout-and-containers/) |
| `scroll-view` | Layout | A scrolling container for a child | [Layout and containers](/pages/widgets-layout-and-containers/) |
| `paragraph` | Text | Styled, wrapped text with clickable spans | [Text](/pages/widgets-text/) |
| `markdown` | Text | A rendered Markdown document | [Text](/pages/widgets-text/) |
| `leader-row` | Text | A label and a value joined by `…` | [Text](/pages/widgets-text/) |
| `key-hints` | Text | A row of key and action hints | [Text](/pages/widgets-text/) |
| `diff-view` | Text | A unified or split diff | [Text](/pages/widgets-text/) |
| `%list` | Collections | A selectable list | [Collections](/pages/widgets-collections/) |
| `table` | Collections | Rows and columns with a header | [Collections](/pages/widgets-collections/) |
| `tree-list` | Collections | An expandable tree | [Collections](/pages/widgets-collections/) |
| `menu` | Collections | A list of commands, also the context menu | [Collections](/pages/widgets-collections/) |
| `check-list` | Collections | A list with a check state per item | [Collections](/pages/widgets-collections/) |
| `list-editor` | Collections | An editable list of typed items | [Collections](/pages/widgets-collections/) |
| `gauge` | Data | One ratio as a bar | [Data display](/pages/widgets-data-display/) |
| `line-gauge` | Data | One ratio as a line | [Data display](/pages/widgets-data-display/) |
| `sparkline` | Data | A series in one row | [Data display](/pages/widgets-data-display/) |
| `bar-chart` | Data | Grouped bars | [Data display](/pages/widgets-data-display/) |
| `chart` | Data | Datasets on axes, Braille or text line | [Data display](/pages/widgets-data-display/) |
| `canvas` | Data | Shapes in a coordinate space | [Data display](/pages/widgets-data-display/) |
| `calendar` | Data | One month (unstable) | [Data display](/pages/widgets-data-display/) |
| `progress-row` | Data | The state of one job | [Data display](/pages/widgets-data-display/) |
| `meter` | Data | A segmented bar | [Data display](/pages/widgets-data-display/) |
| `minimap` | Data | The shape of a long text | [Data display](/pages/widgets-data-display/) |
| `button` | Input | An action button | [Input](/pages/widgets-input/) |
| `toggle` | Input | An on or off switch, a checkbox or a tri-state | [Input](/pages/widgets-input/) |
| `choice` | Input | One option of many, as a stepper or a dialog | [Input](/pages/widgets-input/) |
| `number` | Input | A stepper or a slider | [Input](/pages/widgets-input/) |
| `text-input` | Input | A single-line field | [Input](/pages/widgets-input/) |
| `text-area` | Input | A multi-line field | [Input](/pages/widgets-input/) |
| `schema-form` | Input | A form generated from a schema | [Input](/pages/widgets-input/) |
| `key-capture` | Input | Records a key chord | [Input](/pages/widgets-input/) |
| `color-picker` | Input | A colour field with a dialog | [Pickers](/pages/widgets-pickers/) |
| `path-field` | Input | A file or folder path with a dialog | [Pickers](/pages/widgets-pickers/) |
| `location-picker` | Input | A world map location chooser | [Pickers](/pages/widgets-pickers/) |
| `badge` | Status | A short coloured label | [Status and feedback](/pages/widgets-status-and-feedback/) |
| `spinner` | Status | A stepping busy indicator | [Status and feedback](/pages/widgets-status-and-feedback/) |
| `notice` | Status | An inline message with a tone | [Status and feedback](/pages/widgets-status-and-feedback/) |
| `icon` | Status | A glyph from the icon catalogue | [Status and feedback](/pages/widgets-status-and-feedback/) |
| `image` | Media | A picture | [Media and embedded views](/pages/widgets-media-and-embedded/) |
| `surface` | Media | Pixels the plugin draws | [Media and embedded views](/pages/widgets-media-and-embedded/) |
| `code-view` | Media | A text buffer with the native editor | [Media and embedded views](/pages/widgets-media-and-embedded/) |
| `terminal-view` | Media | A live view of a pane | [Media and embedded views](/pages/widgets-media-and-embedded/) |
| `video` | Media | A clip | [Media and embedded views](/pages/widgets-media-and-embedded/) |

## Gallery

These renders show the real glyphs that the host draws. Borders are rounded only. A form row joins its label and control with grey `…` leaders, one blank cell on each side.

<figure>
<pre class="term">╭─ Settings ─────────────────────────────────────────────╮
│ <span class="t-hi">General</span>  Display  Keys                                 │
│                                                        │
│ Restore previous sessions on boot <span class="t-dim">…………………………………</span> [ On ] │
│ Font size <span class="t-dim">…………………………………………………………………………………………………</span> - 12 + │
│ Scene <span class="t-dim">……………………………………………………………………………………………………</span> ← Pipes → │
│ Opacity <span class="t-dim">………………………………………………………………………………</span> ─────●────  60% │
│ Notify on done <span class="t-dim">……………………………………………………………………………………………</span> [x] │
│                                                        │
│ <span class="t-dim">Esc cancel · Enter apply</span>                               │
╰────────────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 58 columns. Form widgets in a settings view: a tab strip, then rows with <code>…</code> leaders (a toggle, a number stepper, a choice stepper, a slider and a checkbox) and the key hints row. The leaders are inert grey and are never part of a hit target.</figcaption>
</figure>

<figure>
<pre class="term">╭─ Dashboard ────────────────────────────────────────────╮
│ <span class="t-hi">▶ RUNNING  42%</span>  <span class="t-ok">██████████████</span><span class="t-dim">░░░░░░░░</span>  152 of 412     │
│ <span class="t-ok">✓ DONE  100%</span>  <span class="t-ok">██████████████████████</span>                   │
│ <span class="t-dim">Sessions</span>  <span class="t-ok">▁▂▃▄▅▆▇█▇▆▅▄▃▂▁▂▄▆█</span>                          │
│ <span class="t-dim">Disk</span>      <span class="t-ok">━━━━━━━━━━━━━━━━━━━━</span><span class="t-dim">────────────</span> 62%         │
│                                                        │
│ <span class="t-hi">▼</span> src                                                  │
│   <span class="t-hi">▶</span> tests                                              │
│   <span class="t-dim">›</span> main.rs                                            │
╰────────────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 58 columns. Data and collection widgets in one panel: two progress rows, a sparkline, a line gauge and a tree list with expanded and collapsed nodes.</figcaption>
</figure>

## Overlays

Overlays are not widgets. A plugin builds them from the widgets above. [The UI model](/pages/ui-model/) shows each one in detail.

| Overlay | How a plugin gets it |
|---|---|
| Tooltip | The `tooltip` prop on any node, on decorations, on menu items and on choice options. Up to 200 characters. |
| Popover | A `popup` with `modal: false` and an anchor (`at-key`, `at-tree-node`, `at-pointer`), `pinnable` and `dismiss-on-leave`. |
| Context menu | `contributes.context_menu` for kernel surfaces. A `menu` in a popup anchored `at-pointer` for own views, opened on `context-requested`. |
| Toast | The `notify.toast` intent. |
| Command palette | A kernel surface over the commands that a plugin contributes with `palette: true`. |
| Filterable choice dialog | A `choice` with `style: "dialog"` and `filter: true`. |

## How a view becomes pixels

The host calls the `views` export when a view first opens, after the plugin calls `ui.invalidate`, and when the area changes. The export is not called for every frame.

<figure>
<svg viewBox="0 0 360 300" role="img" aria-label="A plugin returns a ui-tree, the host lays it out and paints it, and ui-action events return to the plugin">
  <title>From ui-tree to screen</title>
  <desc>The plugin returns a ui-tree. The host diffs it, runs the layout solver and paints it. User actions come back as ui-action events.</desc>
  <defs>
    <marker id="index-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="10" width="240" height="50" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="130" y="31" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
  <text x="130" y="48" font-size="10" text-anchor="middle" fill="currentColor">views.render returns a ui-tree</text>
  <line x1="130" y1="60" x2="130" y2="84" stroke="currentColor" marker-end="url(#index-arrow)"/>
  <rect x="10" y="84" width="240" height="50" rx="6" fill="none" stroke="currentColor"/>
  <text x="130" y="105" font-size="12" text-anchor="middle" fill="currentColor">ui-tree</text>
  <text x="130" y="122" font-size="10" text-anchor="middle" fill="currentColor">flat node list with keys</text>
  <line x1="130" y1="134" x2="130" y2="158" stroke="currentColor" marker-end="url(#index-arrow)"/>
  <rect x="10" y="158" width="240" height="50" rx="6" fill="none" stroke="currentColor"/>
  <text x="130" y="179" font-size="12" text-anchor="middle" fill="currentColor">Host</text>
  <text x="130" y="196" font-size="10" text-anchor="middle" fill="currentColor">diff by key, layout solver</text>
  <line x1="130" y1="208" x2="130" y2="232" stroke="currentColor" marker-end="url(#index-arrow)"/>
  <rect x="10" y="232" width="240" height="50" rx="6" fill="none" stroke="currentColor"/>
  <text x="130" y="253" font-size="12" text-anchor="middle" fill="currentColor">Screen</text>
  <text x="130" y="270" font-size="10" text-anchor="middle" fill="currentColor">theme roles, leader rule, icon widths</text>
  <path d="M250,257 H290 V35 H250" fill="none" stroke="currentColor" marker-end="url(#index-arrow)"/>
  <text transform="translate(306,150) rotate(90)" font-size="10" text-anchor="middle" fill="currentColor">ui-action event</text>
</svg>
<figcaption>The plugin describes the view. The host lays it out, paints it and sends user actions back as events.</figcaption>
</figure>

## Shared rules

- **Keys are action ids.** A widget's `key` is a string that is stable across renders and unique across the whole view. The `ui-action` event carries the key twice, as `key` and as `action-id` (always a copy of `key` in 0.1.0), so the plugin matches on either. A widget without a key sends nothing and loses its host-owned state on every render. A duplicate key traps the plugin with `ui.duplicate-key`.
- **Host-owned state.** Selection, scroll offsets, the tab index, text contents and cursors belong to the host. The plugin reads them with `widget-state` and changes them with `set-widget-state`.
- **Text and width.** Labels are `styled-text`, a list of spans. Colours come from theme roles, so one plugin works in dark and light themes. Use `display-width` when a label holds icons or wide glyphs.
- **Common node props.** Any node can carry `tooltip`, `tab-index` and `disabled-reason`, and the flags `focusable`, `draggable`, `drop-target`, `hidden`, `hover-events` and `pointer-events`.
- **Disabled widgets** use the `muted` role, and `disabled-reason` shows as a tooltip.
- **Limits.** The default is 4,096 nodes and 1 MiB for every tier; a T1 plugin may request up to 16,384 nodes and 4 MiB in `limits`. A render has a default deadline of 10 ms and a ceiling of 20 ms.
- **Settings views** accept a widened subset of widgets. `popup`, `dialog`, `menu`, `video`, `terminal-view`, an editable `code-view` and most input widgets are refused there. See [The UI model](/pages/ui-model/).

### Host-owned state values

| `widget-state` case | Used by | Meaning |
|---|---|---|
| `none` | Widgets without state | No state. |
| `selected(option<u32>)` | `%list`, `table` | The selected index, or none. |
| `offset(u32)` | Scrollable widgets | The top row offset. |
| `tab(u32)` | `tabs` | The selected tab index. |
| `text(string)` | `text-input`, `text-area` | The current contents. |
| `expanded(list<string>)` | `tree-list` | The ids of the expanded items. |
| `checked(bool)` | `toggle` | The toggle state. |
| `tri` | `toggle` in tri-state style | On, off or mixed. |
| `geo` | `location-picker` | The chosen place. |
| `number(f64)` | `number` | The current value. |
| `split` | `split` | The divider position. |
| `ids(list<string>)` | `check-list`, `tree-list` | A set of item ids. |

Writing `selected(some(7))` on a list of three items clamps the selection to the last item. Writing `selected(none)` clears it.

### Functions in the `ui` import

| Function | Rust | TypeScript | Errors |
|---|---|---|---|
| `invalidate(view)` | `ui::invalidate` | `invalidate` | none |
| `open-panel(panel)` | `ui::open_panel` | `openPanel` | `not-found` when the panel is not declared |
| `close-panel(panel)` | `ui::close_panel` | `closePanel` | none |
| `open-popup(popup, anchor)` | `ui::open_popup` | `openPopup` | `not-found` when no `popup` with that id is in the last tree; `quota-exceeded` over 4 popups |
| `close-popup(popup)` | `ui::close_popup` | `closePopup` | none |
| `widget-state(view, key)` | `ui::widget_state` | `widgetState` | none |
| `set-widget-state(view, key, state)` | `ui::set_widget_state` | `setWidgetState` | `not-found`; `invalid-argument` when the state kind does not fit |
| `display-width(text)` | `ui::display_width` | `displayWidth` | none |
| `theme()` | `ui::theme` | `theme` | none |
| `write-surface(view, key, first-row, format, data)` | `ui::write_surface` | `writeSurface` | `not-found`; `invalid-argument` |

The `view` argument is a `view-target` variant: `panel(string)`, `pane(node-id)`, `popup(string)`, `settings-page(string)` or `onboarding-step(string)`. In Rust it is `ViewTarget::Panel("cleanup".into())`. In TypeScript it is `{ tag: 'panel', val: 'cleanup' }`.

## Action events

The host sends one `ui-action` event for each user action on a keyed widget. The event has the fields `view`, `key`, `kind` and `value`. The Rust event is `Event::UiAction(UiAction)`, and the TypeScript event is `{ tag: 'ui-action', val: {...} }`. There are 17 kinds.

| Kind | Value | Sent by |
|---|---|---|
| `activated` | `none`, `index`, `text` or `cell` | `button`, lists, `table`, `tree-list`, `menu`, `markdown`, `surface`, `progress-row` |
| `changed` | `text`, `number`, `flag`, `index`, `geo` or `ids` | `text-input`, `text-area`, `number`, `toggle`, `choice`, pickers, `check-list` |
| `submitted` | `text` | `text-input` with `submit-on-enter` |
| `selected` | `index` or `text` | `tabs`, lists, `table`, `tree-list`, charts and meters |
| `toggled` | `text` | `tree-list` expand or collapse, `markdown` task click |
| `scrolled` | `index` or `number` | Scrollable widgets (coalesced), `surface`, `minimap` |
| `dropped` | `drop` | A `draggable` node dropped on a `drop-target` node |
| `context-requested` | `pointer` | A right click on a node with `pointer-events` |
| `closed` | `none` | `popup`, `dialog`, `menu` |
| `focus-gained`, `focus-lost` | `none` | Any focusable node |
| `hover-enter`, `hover-leave` | `none` | A node with `hover-events` |
| `pointer` | `pointer` | A node with `pointer-events` |
| `drag-over` | `drag-over` | A `drop-target` during a drag |
| `resized` | `number` | `split` divider |
| `filter-changed` | `text` | A filterable list or choice dialog |

The full value list is `none`, `text`, `index`, `number`, `flag`, `cell`, `drop`, `pointer`, `drag-over`, `item-state`, `geo` and `ids`. Text fields send `changed` after a 600 ms debounce, and a `text-area` of more than 10,000 bytes debounces for 1 s unless `debounce-ms` is set. The per-widget state and action values are in [Widget input map](/pages/reference-widget-input-map/). Toggles and choices send at once. `scrolled` events are coalesced and never dropped.

## Worked example in Rust

This plugin shows a counter and one button in a panel. The button sends `activated`, the handler increments the counter and invalidates the view, and the host calls `render` again.

```rust
use ilium_plugin_sdk::host::ui;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::layout::{Area, Constraint, Direction};
use ilium_plugin_sdk::types::ui::{ActionKind, UiTree, ViewTarget};
use ilium_plugin_sdk::ui::build::Tree;

const PANEL: &str = "cleanup";

#[ilium_plugin(side = "client", exports = ["handler", "views"])]
#[derive(Default)]
struct Cleanup {
    runs: u32,
}

impl Handler for Cleanup {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::UiAction(action) = event else { continue };
            if action.key == "run" && action.kind == ActionKind::Activated {
                self.runs += 1;
                ui::invalidate(ViewTarget::Panel(PANEL.to_string()));
            }
        }
        Vec::new()
    }
}

impl Views for Cleanup {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let status = tree
            .paragraph(vec![vec![Span::plain(format!("Runs so far: {}", self.runs))]])
            .key("status");
        let run = tree.button("Run cleanup").key("run");
        let hint = tree
            .key_hints(vec![KeyHint::new("Enter", "run")])
            .key("hint");
        let column = tree
            .layout(
                Direction::Vertical,
                [Constraint::Length(1), Constraint::Length(3), Constraint::Fill(1)],
            )
            .child(status)
            .child(run)
            .child(hint);
        let frame = tree.block().title("Cleanup").child(column);
        tree.finish(frame)
    }
}
```

## Worked example in TypeScript

The same panel in TypeScript.

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { invalidate } from '@ilium/plugin-sdk/host/ui';
import {
  block, button, fill, keyHints, layout, length, paragraph, tree,
} from '@ilium/plugin-sdk/ui/build';
import { span, type Event, type Submission, type ViewTarget } from '@ilium/plugin-sdk/types';

let runs = 0;

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  activate() {},
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'ui-action') continue;
      if (event.val.key === 'run' && event.val.kind === 'activated') {
        runs += 1;
        invalidate({ tag: 'panel', val: 'cleanup' });
      }
    }
    return [];
  },
  render(_view: ViewTarget) {
    const status = paragraph([[span(`Runs so far: ${runs}`)]], { key: 'status' });
    const run = button('Run cleanup', { key: 'run' });
    const hint = keyHints([{ key: 'Enter', action: 'run' }], { key: 'hint' });
    const column = layout(
      { direction: 'vertical', constraints: [length(1), length(3), fill(1)] },
      [status, run, hint],
    );
    return tree(block({ titles: [{ text: 'Cleanup', position: 'top', alignment: 'left' }] }, [column]));
  },
});
```

## What the user sees

The panel after one press of the button. The hint row is drawn dim, and the focused button has the cyan bold border.

<figure>
<pre class="term">╭─ <span class="t-hi">Cleanup</span> ────────────────────────────────────────╮
│ Runs so far: 1                                   │
│ ╭──────────────────╮                             │
│ │   Run cleanup    │                             │
│ ╰──────────────────╯                             │
│ <span class="t-dim">Enter run</span>                                        │
╰──────────────────────────────────────────────────╯
</pre>
<figcaption>The panel after one activation. The button border is rounded like every other border.</figcaption>
</figure>

## Edge cases and errors

- **Invalid tree.** A child index out of range, a graph that is not a tree, a duplicate key or more than 4,096 nodes traps the instance. The host restarts it under the restart rule, and three failures within 10 minutes disable the plugin with a notice.
- **Size.** The manifest can raise the limit to 16,384 nodes and 4 MiB only for first-party plugins.
- **Unknown view.** An `invalidate` call for a view the plugin does not declare is ignored, and the host writes one warning that names the view.
- **Popups.** A `popup` is drawn only while `open-popup` has it open. At most 4 are open per view, and more give `quota-exceeded`. Escape closes the topmost popup and the plugin receives `closed`. A modal popup cannot refuse the close.
- **Missing grant.** A `code-view` or `terminal-view` without the grant draws an empty frame with the text `not permitted`.
- **Secret fields.** A `text-input` with `secret: true` sends its value in `changed` and `submitted`. The host redacts it from logs and never stores it in the snapshot.
- **Widgets that send nothing.** `layout`, `block`, `clear`, `scrollbar`, `leader-row`, the gauges, `image` and `schema-form` send no `ui-action`. `schema-form` changes reach the plugin as `settings-changed`, and `code-view` changes arrive as `editor-changed`.

Requirement ids for this page: [WID-001]. See the [requirements index](/pages/reference-requirements/).

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

- [The UI model](/pages/ui-model/) and [Layout](/pages/layout/)
- [Layout and containers](/pages/widgets-layout-and-containers/)
- [Text](/pages/widgets-text/)
- [Collections](/pages/widgets-collections/)
- [Data display](/pages/widgets-data-display/)
- [Input](/pages/widgets-input/) and [Pickers](/pages/widgets-pickers/)
- [Status and feedback](/pages/widgets-status-and-feedback/)
- [Media and embedded views](/pages/widgets-media-and-embedded/)
- [Capability: ui panels](/pages/capability-ui-panels/)
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/)
- [Accessibility and theming](/pages/accessibility-and-theming/)
- [Errors and limits](/pages/reference-errors-and-limits/)

API version: `ilium:plugin@0.1.0`.

### Security

Kernel security dialogs are not widgets plugins can draw ([SEC-377]).

Requirements for this page: [Widgets requirements](/pages/reference-requirements-widgets/).
