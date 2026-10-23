---
layout: page
title: "ui.panels"
permalink: /pages/capability-ui-panels/
---

The `ui.panels` grant lets a client-side plugin contribute views to the interface: full-screen or overlay panels, popups, and settings pages. The plugin describes each view as a UI tree. The host lays the tree out, paints it with the theme, keeps the state of each widget and reports the user's actions back to the plugin. The plugin never draws a pixel of its own, except through a `surface` widget that it fills with `ui.write-surface`.

A view is rendered by the `views` export. The host calls `render(view, area)` when the view first opens, after the plugin calls `invalidate`, and when the terminal is resized. The host never calls it per frame.

<figure>
<svg viewBox="0 0 400 140" role="img" aria-label="Host draws the view from the plugin tree and sends actions back">
  <defs><marker id="cap-ui-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <g fill="none" stroke="currentColor" stroke-width="1.5">
    <rect x="10" y="40" width="110" height="50" rx="4"/>
    <rect x="145" y="40" width="110" height="50" rx="4"/>
    <rect x="280" y="40" width="110" height="50" rx="4"/>
    <line x1="120" y1="65" x2="144" y2="65" marker-end="url(#cap-ui-arrow)"/>
    <line x1="255" y1="65" x2="279" y2="65" marker-end="url(#cap-ui-arrow)"/>
    <path d="M200,90 L200,115 L65,115 L65,92" marker-end="url(#cap-ui-arrow)"/>
  </g>
  <g fill="currentColor" font-family="sans-serif" text-anchor="middle">
    <text x="65" y="62" font-size="12">views export</text>
    <text x="65" y="78" font-size="11">returns UI tree</text>
    <text x="200" y="62" font-size="12">host layout</text>
    <text x="200" y="78" font-size="11">ratatui solver</text>
    <text x="335" y="62" font-size="12">terminal</text>
    <text x="335" y="78" font-size="11">theme colours</text>
    <text x="132" y="30" font-size="11">tree</text>
    <text x="267" y="30" font-size="11">cells</text>
    <text x="132" y="130" font-size="11">ui-action</text>
  </g>
</svg>
<figcaption>The plugin returns a tree and receives the user's actions. The host lays out and paints the tree.</figcaption>
</figure>

## You will need

- The grant `ui.panels` on the client side. The `ui` import is not available on the server side.
- A `panels` or `onboarding_steps` entry in the manifest for each view, or a `settings` tab with `"render": "view"`, and `"views"` in the manifest `exports` list.
- A `views` export that returns a UI tree for each view target.
- The grant [help.read](/pages/capability-help-read/) if the panel links to help topics.
- A Rust crate built with `ilium-plugin-sdk`, or a TypeScript entry module that imports `@ilium/plugin-sdk`. Both SDKs expose the same `ui` functions under the names in the tables below.

## Grant syntax

- Valid: `ui.panels`
- Invalid: `ui.panels:settings`. A custom settings page is a `settings` tab with `"render": "view"` in the manifest.

## Manifest contributions

| Contribution | Fields | View target | Meaning |
|---|---|---|---|
| `panels` | `id`, `title`, `icon?`, `default_size?`, `modal` | `panel("<id>")` | A full-screen or overlay view. |
| `settings` tab with `"render": "view"` | `id`, `title`, `icon?` | `settings-page("<id>")` | A custom page in Settings, listed under the plugin. |
| `onboarding_steps` | `id`, `title`, `order` | `onboarding-step("<id>")` | A guided setup step, shown in the given order. |

`settings` tabs with `"render": "view"` and `onboarding_steps` no longer carry a `view` key. The host finds the view through the view target. The `id` of a settings tab must differ from every other settings tab and section id of the same plugin, or the manifest is refused with `manifest.settings-id-clash`.

## The host imports

All functions are in the `ui` interface. Each one needs `ui.panels` or the `panes.provide-kind` grant, except `invalidate`, which the plugin may call for its own views at any time. The functions with no error code (`close-panel`, `close-popup`, `widget-state`, `display-width` and `theme`, as in [UI](/pages/api-ui/)) cannot refuse. When the grant is declined they return their plain result: `display-width` and `theme` still answer, `widget-state` returns nothing because no view is registered, and `close-panel` and `close-popup` do nothing.

| WIT function | Rust | TypeScript | Returns | Meaning |
|---|---|---|---|---|
| `open-panel(panel)` | `ui::open_panel` | `openPanel` | `result` | Shows a declared panel. |
| `close-panel(panel)` | `ui::close_panel` | `closePanel` | none | Hides a panel. |
| `open-popup(popup, anchor)` | `ui::open_popup` | `openPopup` | `result` | Shows a `popup` widget of the last rendered tree. |
| `close-popup(popup)` | `ui::close_popup` | `closePopup` | none | Hides a popup. |
| `invalidate(view)` | `ui::invalidate` | `invalidate` | none | Asks the host to call `render` again. |
| `widget-state(view, key)` | `ui::widget_state` | `widgetState` | `option` | Reads the host-owned state of a keyed widget. |
| `set-widget-state(view, key, state)` | `ui::set_widget_state` | `setWidgetState` | `result` | Changes that state, for example the selected row. |
| `write-surface(view, key, first-row, format, data)` | `ui::write_surface` | `writeSurface` | `result` | Writes whole rows of pixels into a `surface`. |
| `display-width(text)` | `ui::display_width` | `displayWidth` | `u16` | Width of a string in cells, with the host's rules for icons and wide glyphs. |
| `theme()` | `ui::theme` | `theme` | `theme-tokens` | The current colour roles, dark or light flag and glyph preference. |

`open-view` and `close-view` intents do the same as `open-panel` and `close-panel` when a command or the server submits them. The `open-view` intent takes a view target and a `focus` flag.

## Types

| Type | Cases or fields |
|---|---|
| `view-target` | `panel(string)`, `pane(node-id)`, `popup(string)`, `settings-page(string)`, `onboarding-step(string)` |
| `popup-anchor` | `centered((constraint, constraint))`, `at-cell((u16, u16))`, `below-key(string)` |
| `widget-state` | `none`, `selected(option<u32>)`, `offset(u32)`, `tab(u32)`, `text(string)`, `expanded(list<string>)`, `checked(bool)` |
| `area` | `columns: u16`, `rows: u16` |
| `pixel-format` | `mask8`, `mono1`, `gray8`, `rgb8`, `rgba8` |
| `theme-tokens` | colour roles `foreground`, `background`, `accent`, `muted`, `success`, `warning`, `danger` as `u32`, plus `is-dark: bool` and `stable-glyphs: bool` |

## Events

| Event | Payload | When it arrives |
|---|---|---|
| `ui-action` | `view`, `key`, `kind`, `value` | The user acts on a widget of the plugin's own view. Never dropped. |
| `view-visibility` | `view`, `visible` | A view opens or closes. Coalesced. |
| `theme-changed` | none | The user changes the theme. The host repaints the colour roles itself, so no new render is needed. |

The `kind` of a `ui-action` is one of `activated`, `changed`, `submitted`, `selected`, `toggled`, `scrolled`, `dropped`, `context-requested`, `closed`, `focus-gained`, `focus-lost`, `hover-enter`, `hover-leave`, `pointer`, `drag-over`, `resized` or `filter-changed`. The `value` is one of `none`, `text(string)`, `index(u32)`, `number(f64)`, `flag(bool)`, `drop(source-key, target-key, index)` or `cell(cell-position)`. The widget `key` is the action id: the plugin matches on it.

The host sends `changed` for a text widget 600 ms after the last keystroke, 1 s for a large editor, and immediately for a toggle or a choice. A `tabs` widget sends `selected` with the new index. A `schema-form` sends no `ui-action`: the host saves each change and the owning plugin receives `settings-changed`.

## The UI tree

A view returns a flat `ui-tree`: a list of nodes with child indices, and a root index. Each node has an optional `key`, a `widget`, its `children` and flags (`focusable`, `draggable`, `drop-target`, `hidden`).

- Keys are unique across the whole view and stable across renders. The host keeps widget state by key, so a widget without a key loses its selection, scroll offset and typed text on every render.
- The host owns list and table selection, tab index, scroll offsets, text contents and cursor, tree expansion, the popup stack and focus.
- A tree holds at most 4,096 nodes and 1,024 KiB by default. Each limit is a trap when exceeded.
- Layout uses the ratatui solver. The constraints are `length(u16)`, `min(u16)`, `max(u16)`, `percentage(u16)`, `ratio((u32, u32))` and `fill(u16)`. The plugin never sees a `Rect`. It reads `area` in `render` when it needs geometry.

Every widget is listed in [widgets-index](/pages/widgets-index/). A settings page built from `schema-form` and `tabs` uses those entries unchanged.

## Panel size

A panel's `default_size` is `{ "width": <constraint>, "height": <constraint> }`. A constraint is a string: `"80%"` for a share of the content area, `"60"` for a number of cells, or `"fill"` for the remaining space.

- Without `default_size`, a modal panel is centred at 80% by 80%.
- Without `default_size`, a non-modal panel takes the whole content area.

## Popups and Escape

A `popup` widget is drawn only while an `open-popup` call has it open. The popup has a `modal` flag and a title. `close-popup` or Escape hides it, and the plugin then receives `closed` with `none`. A modal popup cannot refuse the close.

Escape is reserved by the host. It closes the topmost popup first, then the topmost panel. A focused text widget consumes the first Escape, so the second Escape closes the panel. The plugin can also submit `close-view` for its own panel.

## Risk and tier rules

- This grant is not marked high risk.
- A T1 bundled plugin receives it automatically up to its manifest ceiling.
- A T2 or T3 plugin needs an explicit grant in the review dialog, and the grant is remembered.
- A view shows only what the plugin renders. Any data a view reads needs its own grant, such as `fs.read:project`.

## What the review dialog says

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.ui 0.1.0 by Example Ltd                                        │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ [x] Shows panels and settings pages <span class="t-dim">…………………………………………………………………………</span> ui.panels │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests ui.panels, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

## When the grant is declined

- Every `ui` import that has an error code returns `not-permitted`. `invalidate`, `close-panel`, `close-popup`, `widget-state`, `display-width` and `theme` have no error code, as in [UI](/pages/api-ui/): none of them returns `not-permitted`. `invalidate`, `close-panel` and `close-popup` are no-ops, `widget-state` returns nothing, and `display-width` and `theme` return their normal values.
- The `panels`, `settings` tabs with `"render": "view"` and `onboarding_steps` contributions are not registered.
- The `open-view` intent is refused with `not-permitted`.
- A settings page that is not registered does not appear in the Settings screen.
- The rest of the plugin, such as its commands and its server logic, is unaffected.

## Worked example in Rust: a run log panel

The manifest declares the panel, a command that opens it, and the `views` export.

```json
{
  "id": "example-plugin",
  "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": ["views"] },
  "capabilities": { "client": ["ui.panels"] },
  "contributes": {
    "commands": [{ "id": "open-run-log", "title": "Open run log" }],
    "panels": [
      { "id": "run-log", "title": "Run log", "icon": "list",
        "default_size": { "width": "80%", "height": "60%" }, "modal": false }
    ]
  }
}
```

The command id is `example-plugin/open-run-log`, because the local id gets the plugin id as prefix.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::{log, ui};
use ilium_plugin_sdk::host::log::Level;
use ilium_plugin_sdk::types::layout::{Area, Constraint, Direction};
use ilium_plugin_sdk::types::ui::{ActionKind, UiTree, ViewTarget};

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct RunLog {
    lines: Vec<String>,
}

impl Lifecycle for RunLog {}

impl Handler for RunLog {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::CommandInvoked(command)
                    if command.command == "example-plugin/open-run-log" =>
                {
                    if ui::open_panel("run-log").is_err() {
                        log::log(Level::Warn, "the run log panel could not be opened");
                    }
                }
                Event::UiAction(action) if action.key == "refresh" => {
                    if action.kind == ActionKind::Activated {
                        self.lines.push(format!("refresh {}", self.lines.len() + 1));
                        ui::invalidate(ViewTarget::Panel("run-log".into()));
                    }
                }
                _ => {}
            }
        }
        Vec::new()
    }
}

impl Views for RunLog {
    fn render(&mut self, _view: ViewTarget, area: Area) -> UiTree {
        let mut tree = Tree::new();
        let visible: Vec<String> =
            self.lines.iter().rev().take(area.rows as usize).cloned().collect();
        let log_view = tree.paragraph(visible).key("log").scrollable(true);
        let refresh = tree.button("Refresh").key("refresh");
        let body = tree
            .layout(Direction::Vertical)
            .constraints([Constraint::Fill(1), Constraint::Length(3)])
            .child(log_view)
            .child(refresh);
        let root = tree.block().title("Run log").child(body);
        tree.finish(root)
    }
}
```

The `Tree` type comes from `ilium_plugin_sdk::ui::build`. The example omits that import for brevity.

## Worked example in TypeScript: a settings page with tabs

The manifest declares the settings page. The view target is `settings-page("display")`.

```json
{
  "capabilities": { "client": ["ui.panels"] },
  "exports": ["views"],
  "contributes": {
    "settings": [{ "kind": "tab", "id": "display", "title": "Display", "icon": "sliders", "render": "view" }]
  }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { invalidate } from '@ilium/plugin-sdk/host/ui';
import { block, paragraph, span, tabs, toggle, tree } from '@ilium/plugin-sdk/ui/build';

const preferences = { showHints: true };
const DISPLAY_PAGE = { tag: 'settings-page', val: 'display' } as const;

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  activate() {},
  deactivate() { return undefined; },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'ui-action') continue;
      const action = event.val;
      if (action.key !== 'show-hints' || action.kind !== 'toggled') continue;
      preferences.showHints = action.value.tag === 'flag' && action.value.val;
      invalidate(DISPLAY_PAGE);
    }
    return [];
  },
  render(_view, _area) {
    return tree(
      block({ title: 'Display' }, [
        tabs({ titles: [span('General'), span('Advanced')] }, [
          toggle({ key: 'show-hints', label: 'Show hints', value: preferences.showHints }),
          paragraph(['Advanced options are listed here.']),
        ]),
      ]),
    );
  },
});
```

The `tabs` widget renders only its selected child, and the host keeps the tab index. A real plugin stores its preferences with `settings`, not in a module variable.

## Limits, errors and edge cases

| Situation | Result |
|---|---|
| `open-panel` for a panel that is not declared | `not-found` |
| `open-popup` when no `popup` widget with that id is in the last rendered trees | `not-found` |
| `set-widget-state` with a key unknown in that view | `not-found` |
| `set-widget-state` with a state kind that does not fit the widget | `invalid-argument` |
| `write-surface` with no `surface` of that key | `not-found` |
| `write-surface` with a format that differs from the widget, data that is not whole rows, or data over 16 MiB | `invalid-argument` |
| `invalidate`, `close-panel`, `close-popup`, `widget-state`, `display-width`, `theme` | no error code |
| Two nodes with the same `key` in one tree | trap; the plugin is restarted |
| A child index out of range, or a tree that is not a tree | trap |
| More than 4,096 nodes, or more than 1,024 KiB of tree | trap |
| `views.render` runs past its deadline (10 ms by default, up to 20 ms for a T1 plugin) | trap |
| Three failures in 10 minutes | the plugin stops, and Settings > Plugins offers to turn it on again |

## What the user sees

The run log panel from the Rust example, after two presses of Refresh, looks like this in an 80-column terminal:

<figure>
<pre class="term">
╭─ Run log ────────────────────────────────────────────────╮
│ refresh 1                                                │
│ refresh 2                                                │
│ refresh 3                                                │
│                                                          │
│                                                          │
│                       [ Refresh ]                        │
│                                                          │
╰──────────────────────────────────────────────────────────╯
  Enter: press the button   Esc: close the panel
</pre>
<figcaption>Illustrative, not generated. The host paints the run log view from the tree in the Rust example.</figcaption>
</figure>

- A panel opens at its `default_size` and closes with Escape.
- A settings page is listed under the plugin in Settings.
- An onboarding step appears in the setup flow, in the order the manifest gives.
- Widgets that have a key keep their selection, scroll offset and typed text across renders.
- Colours follow the theme. The host applies the leader-row and icon rules to every view without extra code.

## Translated text and locale

The host does not tell a plugin the user's locale, time zone or language. A plugin that shows translated text picks its own language, usually from a `choice` settings row, and passes finished strings to the `ui` functions. The full rules are in [Internationalisation](/pages/guide-internationalisation/). This section lists what they mean for a view.

- **Where plugin text appears.** A view's labels, paragraphs and titles, the text of a `toast` or `notice`, a `badge` or status segment, a help topic, and the one-line reason a plugin gives for a grant in its manifest. The host shows each of these as the plugin wrote it, apart from removing terminal escape sequences and cutting to the width.
- **Manifest titles have one value.** The `title` of a `panels` entry, a `settings` tab and an `onboarding_steps` entry is read from the manifest, not from a view, so it cannot change with the user's language. Keep those titles in one language and put translated text in the tree that `render` returns. Identifiers (`id`, command ids, setting keys) are never translated.
- **Width.** Measure every string with `display-width` and cut only at a grapheme boundary. A cut ends with `…`, which takes one cell. A translated label is often 30 percent longer than the English text, so leave room and do not assume a fixed width. In a `leader-row`, the host shrinks the value first, then the label.
- **Limits by surface.** A string that exceeds its box is cut with `…` and never overflows. A badge is cut at 12 cells, and a toast title and body have the length limits of [Events and intents](/pages/reference-events-and-intents/). The grant reason in the review dialog is cut at the dialog width, which is 70 columns, and a plugin should keep it within 60 cells after translation. See [Grant review and consent](/pages/reference-grant-review-and-consent/).
- **Right-to-left text is not supported.** The host draws each line from left to right, does not reorder bidirectional text and does not mirror a layout. Put right-to-left text in its own row, apart from left-to-right labels, and say in the plugin README that such text is not laid out correctly.
- **Fallback.** A plugin that ships several languages reads them from `i18n/<tag>.json`. A missing message falls back to one default language, normally English, and never shows the message id. A language tag that the plugin does not ship uses the default language. Host text such as the review dialog's fixed words, buttons and Settings labels follows the host's own language setting. It is not translated by the plugin, and the plugin must not rely on it matching its own language.
- **Testing.** Run the view with the `en-XA` pseudo-locale, which expands every string by 40 percent. `ilium plugin lint` fails a view that clips or overflows at that locale with `i18n.layout-overflow`.

## See also

- [widgets-index](/pages/widgets-index/) for every widget and its props.
- [widgets-layout-and-containers](/pages/widgets-layout-and-containers/) for `block`, `tabs`, `popup` and `layout`.
- [widgets-input](/pages/widgets-input/) for `button`, `toggle`, `text-input` and `schema-form`.
- [widgets-media-and-embedded](/pages/widgets-media-and-embedded/) for `surface`, which `write-surface` fills.
- [settings-screens](/pages/settings-screens/) for how settings pages are listed.
- [capability-panes-provide-kind](/pages/capability-panes-provide-kind/) for panes whose body is a view.
- [errors-and-limits](/pages/reference-errors-and-limits/) for the trap and restart rules.
- [tutorial-03-panel](/pages/tutorial-03-panel/) for a step-by-step panel.

Checked against the API version `ilium:plugin@0.1.0`.
