# ui

## Overview

The `ui` import controls the views that a client-side plugin shows: panels, popups, the state of the
widgets inside them, the theme and the width of text. A plugin never draws cells. It describes each
view as a tree of widgets, returned from its `views.render` export. The host lays the tree out with
the constraint solver described in [layout](/pages/widgets-layout-and-containers/), paints it with the
user's theme and keeps the state of every widget (selection, scroll offset, text) between renders.

A view renders in four cases only: when it first opens, after the plugin calls `invalidate`, when its
area changes (a resize), and after a theme change. Views never render per frame. When the user acts on a widget, the host sends a `ui-action`
event back to the plugin's `handler`, and the plugin decides whether to change the view.

## What you need

- **Grants.** `ui.panels` gives panels, settings pages, onboarding steps, popups and the state calls.
  `panes.provide-kind` gives the same calls for the view of a basic pane. `ui.invalidate` needs no
  grant for views the plugin contributes.
- **Side.** Client only. The plugin declares the `client` side and exports `views` (`exports = ["views"]`
  in Rust, a `render` function in TypeScript). The `ui` imports do not exist on the server.
- **Manifest.** Panels go in `contributes.panels`, custom settings pages as `settings` tabs with `"render": "view"`, and
  guided steps in `onboarding_steps`. Each entry names a local `id`, which becomes the view target.
- **Language.** Rust with `ilium-plugin-sdk`, or TypeScript with `@ilium/plugin-sdk`. Both use the
  same names, spelled per the naming rule of the SDK.

```json
{
  "contributes": {
    "panels": [{ "id": "notes", "title": "Notes", "modal": false }]
  },
  "capabilities": { "client": ["ui.panels"] }
}
```

## Functions

Rust imports these from `ilium_plugin_sdk::host::ui`. TypeScript imports them from
`@ilium/plugin-sdk/host/ui`. Every call that returns `Result` throws `PluginError` in TypeScript.

| Rust and TypeScript | Returns | Meaning |
|---|---|---|
| `invalidate(view)` | nothing | Queues a new render of the view. |
| `open_panel(panel)` | `Result` | Shows a panel declared in the manifest. |
| `close_panel(panel)` | nothing | Hides a panel. |
| `open_popup(popup, anchor)` | `Result` | Shows the `popup` or `dialog` node whose key is `popup`. It searches the last committed tree of each open view. No match returns `not-found`, two matches `invalid-argument`, and a fifth open popup in one view `quota-exceeded`. Popup ids are unique across all open views of the plugin. |
| `close_popup(popup)` | nothing | Hides that popup. An id that is not open is a no-op that returns `ok`. A popup whose node disappears from the next tree closes and sends `closed`. |
| `widget_state(view, key)` | `Option` | Reads the host-owned state of a widget: selection, offset, tab, text, expansion or check. |
| `set_widget_state(view, key, state)` | `Result` | Changes that state, for example to select row 0 of a list. |
| `display_width(text)` | `u16` | Width in cells of a text, using the host's rules for icons and wide glyphs. |
| `theme()` | `ThemeTokens` | The theme colours, whether the theme is dark, and whether stable glyphs are on. |
| `write_surface(view, key, first_row, format, data)` | `Result` | Writes pixels into a `surface` widget, starting at `first_row`. |

`display_width`, `theme` and `invalidate` never return an error code.

## Types

| Type | Cases | Meaning |
|---|---|---|
| `ViewTarget` | `panel(string)`, `pane(node-id)`, `popup(string)`, `settings-page(string)`, `onboarding-step(string)` | The view that a call or event refers to. |
| `PopupAnchor` | `centered(tuple<constraint, constraint>)`, `at-cell(tuple<u16, u16>)`, `below-key(string)` | Where `open_popup` places the popup. |
| `WidgetState` | `none`, `selected(option<u32>)`, `offset(u32)`, `tab(u32)`, `text(string)`, `expanded(list<string>)`, `checked(bool)` | Host-owned state of one widget. |
| `Constraint` | `length(u16)`, `min(u16)`, `max(u16)`, `percentage(u16)`, `ratio(tuple<u32, u32>)`, `fill(u16)` | A size rule for layouts, popups and panels. |
| `PixelFormat` | `mask8`, `mono1`, `gray8`, `rgb8`, `rgba8` | The pixel format of a `surface` widget. |
| `ThemeTokens` | `foreground`, `background`, `accent`, `muted`, `success`, `warning`, `danger` (each `u32` as `0x00RRGGBB`), `is-dark`, `stable-glyphs` | The theme returned by `theme()`. |

Rust names are `ViewTarget::Panel`, `PopupAnchor::BelowKey` and `WidgetState::Selected`. In TypeScript
a variant is `{ tag, val }` with a kebab-case tag, so `below-key` becomes `{ tag: 'below-key', val: 'help' }`.
A `u64` node id is a `bigint` in TypeScript.

## Events and actions

The host sends these events to `handler.on-events` for the plugin's own views.

| Event | Payload | Delivery |
|---|---|---|
| `ui-action` | `view`, `key`, `action-id`, `kind`, `value` | Never dropped; `scrolled` is coalesced. |
| `view-visibility` | `view`, `visible` | Coalesced. |
| `theme-changed` | none | Sent when the user changes the theme, after the host has repainted the last tree. One `views.render` per open view follows. |

The `key` is the widget's key. In 0.1.0 `action-id` is always a copy of `key`. The `kind` is one of `activated`, `changed`, `submitted`, `selected`,
`toggled`, `scrolled`, `dropped`, `context-requested`, `closed`, `focus-gained` or `focus-lost`. The
`value` is an option: `none` for a button `activated`, a menu or key-capture `closed`, and the hover and focus kinds; otherwise `text(string)`, `index(u32)`, `number(f64)`, `flag(bool)` or `drop(source, target, index)`.
Which kind each widget sends is listed on the widget pages, starting with [widgets index](/pages/widgets-index/).

## Rules for trees

- **Keys.** Every interactive widget needs a `key` that stays the same from one render to the next,
  and keys must be unique across the whole view, not only among siblings. A duplicate key anywhere traps with `ui.duplicate-key`. The host keeps widget state by key, so a widget without
  a key loses its state on every render.
- **Size.** A tree is a flat list of nodes with child indices. It holds at most 4,096 nodes and 1 MiB
  by default. A manifest `limits` entry can raise both to 16,384 nodes and 4 MiB.
- **Render time.** `views.render` has a deadline of 10 ms by default (`render_deadline_ms`, ceiling 20 ms).
- **Pure call.** `views.render` is a pure call, like `annotate` and `settings-validate`. An effectful import called inside it (an intent submission, a request, a storage write or a timer) returns `err(not-permitted)` with the message `effectful import in a pure call` and the call continues [ABI-048]. Read-only imports stay allowed.
- **Popups.** A `popup` widget in a tree is drawn only while an `open_popup` call has opened it. A popup or dialog node takes no space in its parent's layout. Its size and placement follow [Popup size and placement](/pages/widgets-layout-and-containers/#popup-size-and-placement).
- **Theme.** A tree stores colour roles such as `accent` and `muted`, not colours. The host resolves
  roles when it paints. On a theme change the host repaints the last tree of every open view at once, delivers `theme-changed`, and triggers exactly one `views.render` per open view, coalesced with any other trigger in the same 16 ms window. A plugin need not call `invalidate`. Plugins cannot ship themes.

<div class="warn">
<p>An invalid tree traps the plugin with a <code>ui.*</code> code: <code>ui.invalid-tree</code>, <code>ui.duplicate-key</code>, <code>ui.too-deep</code>, <code>ui.too-large</code>, <code>ui.widget-not-allowed</code> or <code>ui.render-timeout</code>. The host keeps the previous tree, marked stale, and logs one error. <code>views.render</code> returns a tree, not a result, so it never returns <code>invalid-argument</code>.
A trap counts as a failure for the restart rule: the third failure in ten minutes disables the plugin on
that side.</p>
</div>

## Example 1: a help popup below a button

The panel shows a `Keys` button. When the user activates it, the plugin opens a popup below the
button. The popup is part of the view's tree, so `views.render` declares it. The popup needs
`ui.panels`.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::{log::{self, Level}, ui};
use ilium_plugin_sdk::types::layout::{Area, Constraint, Direction};
use ilium_plugin_sdk::types::ui::{ActionKind, PopupAnchor, UiTree, ViewTarget};
use ilium_plugin_sdk::ui::build::Tree;

#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
struct RenameHelp;

impl Lifecycle for RenameHelp {}

impl Handler for RenameHelp {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::UiAction(action) = event else { continue };
            if action.key != "help" || action.kind != ActionKind::Activated { continue; }
            let anchor = PopupAnchor::BelowKey("help".to_string());
            if let Err(error) = ui::open_popup("keys-help", anchor) {
                log::log(Level::Warn, &error.message);
            }
        }
        Vec::new()
    }
}

impl Views for RenameHelp {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let help = tree.button("Keys").key("help");
        let text = tree.paragraph(["Enter saves the name.", "Esc cancels."]);
        let popup = tree.popup("keys-help", text).title("Keys")
            .width(Constraint::Length(40)).height(Constraint::Length(4));
        let root = tree.layout(Direction::Vertical,
            vec![Constraint::Length(1), Constraint::Length(0)], vec![help, popup]);
        tree.finish(root)
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { log } from '@ilium/plugin-sdk/host/log';
import { openPopup } from '@ilium/plugin-sdk/host/ui';
import { button, layout, paragraph, popup, tree } from '@ilium/plugin-sdk/ui/build';

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'ui-action') continue;
      if (event.val.key !== 'help' || event.val.kind !== 'activated') continue;
      try {
        openPopup('keys-help', { tag: 'below-key', val: 'help' });
      } catch (error) {
        log('warn', String(error));
      }
    }
    return [];
  },
  render() {
    return tree(layout({ direction: 'vertical',
      constraints: [{ tag: 'length', val: 1 }, { tag: 'length', val: 0 }] }, [
      button('Keys', { key: 'help' }),
      popup('keys-help', { title: 'Keys', modal: false,
        width: { tag: 'length', val: 40 }, height: { tag: 'length', val: 4 } }, [
        paragraph(['Enter saves the name.', 'Esc cancels.']),
      ]),
    ]));
  },
});
```

The popup node sits in a `length(0)` slot, because a popup takes no space in the layout. The call returns `ok`, and the popup appears directly below the `help` button, with its left edge aligned to the button. If no popup with id
`keys-help` is in the last committed tree of any open view, the call returns `not-found`. Escape closes the popup. The plugin
then receives a `ui-action` with kind `closed`.

## Example 2: a notes panel with a list and a delete button

This panel lists note titles in a `%list` widget keyed `notes-list`, with a `Delete` button keyed
`delete`. The plugin tracks the selected row from `selected` actions. After a delete, it moves the
selection to row 0 and invalidates the view. The host keeps the list's offset; the plugin owns the
titles.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::{log::{self, Level}, ui};
use ilium_plugin_sdk::types::layout::{Area, Constraint, Direction};
use ilium_plugin_sdk::types::ui::{ActionKind, ActionValue, UiTree, ViewTarget, WidgetState};
use ilium_plugin_sdk::ui::build::Tree;

#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
struct Notes { titles: Vec<String>, selected: usize }

fn notes_view() -> ViewTarget { ViewTarget::Panel("notes".to_string()) }

impl Lifecycle for Notes {}

impl Handler for Notes {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::UiAction(action) = event else { continue };
            if action.view != notes_view() { continue; }
            match (action.key.as_str(), action.kind, action.value) {
                ("notes-list", ActionKind::Selected, ActionValue::Index(index)) => {
                    self.selected = index as usize;
                }
                ("delete", ActionKind::Activated, _) if self.selected < self.titles.len() => {
                    self.titles.remove(self.selected);
                    self.selected = 0;
                    let state = WidgetState::Selected(Some(0));
                    if let Err(error) = ui::set_widget_state(notes_view(), "notes-list", state) {
                        log::log(Level::Warn, &error.message);
                    }
                    ui::invalidate(notes_view());
                }
                _ => {}
            }
        }
        Vec::new()
    }
}

impl Views for Notes {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let list = tree.list(self.titles.clone()).key("notes-list");
        let delete = tree.button("Delete").key("delete");
        let root = tree.layout(Direction::Vertical,
            vec![Constraint::Fill(1), Constraint::Length(3)], vec![list, delete]);
        tree.finish(root)
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import type { ViewTarget } from '@ilium/plugin-sdk/types';
import { invalidate, setWidgetState } from '@ilium/plugin-sdk/host/ui';
import { log } from '@ilium/plugin-sdk/host/log';
import { button, layout, list, tree } from '@ilium/plugin-sdk/ui/build';

const NOTES_VIEW: ViewTarget = { tag: 'panel', val: 'notes' };
let titles: string[] = [];
let selected = 0;

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'ui-action') continue;
      const { view, key, kind, value } = event.val;
      if (view.tag !== 'panel' || view.val !== 'notes') continue;
      if (key === 'notes-list' && kind === 'selected' && value.tag === 'index') {
        selected = value.val;
      }
      if (key === 'delete' && kind === 'activated' && selected < titles.length) {
        titles.splice(selected, 1);
        selected = 0;
        try {
          setWidgetState(NOTES_VIEW, 'notes-list', { tag: 'selected', val: 0 });
        } catch (error) {
          log('warn', String(error));
        }
        invalidate(NOTES_VIEW);
      }
    }
    return [];
  },
  render() {
    return tree(layout({ direction: 'vertical',
      constraints: [{ tag: 'fill', val: 1 }, { tag: 'length', val: 3 }] }, [
      list(titles, { key: 'notes-list' }),
      button('Delete', { key: 'delete' }),
    ]));
  },
});
```

## Simulated render

<figure>
<pre class="term"><span class="t-dim">╭  ≡ ● · Notes────────────────────────────────────────────────╮</span>
<span class="t-dim">│</span> <span class="t-hi">[ Keys ]</span>  [ Delete ]                                       <span class="t-dim"> │</span>
<span class="t-dim">│</span> <span class="t-dim">╭  ≡ ● · Keys─────────────────────────────╮</span>               <span class="t-dim">  │</span>
<span class="t-dim">│</span> <span class="t-dim">│</span> Enter saves the name.                    <span class="t-dim">│</span>               <span class="t-dim"> │</span>
<span class="t-dim">│</span> <span class="t-dim">│</span> Esc cancels.                             <span class="t-dim">│</span>               <span class="t-dim"> │</span>
<span class="t-dim">│</span> <span class="t-dim">╰──────────────────────────────────────────╯</span>               <span class="t-dim"> │</span>
<span class="t-dim">│</span>                                                            <span class="t-dim"> │</span>
<span class="t-dim">│</span> Project notes …………………………………………………… <span class="t-dim">? help</span>                  <span class="t-dim"> │</span>
<span class="t-dim">╰─────────────────────────────────────────────────────────────╯</span>
</pre>
<figcaption>Illustrative, not generated. A non-modal notes panel, 62 by 9 characters, dark theme, with
the help popup open below the Keys button. The popup is drawn over the panel content.</figcaption>
</figure>

## Diagram: the render cycle

<figure>
<svg viewBox="0 0 400 190" role="img" aria-label="View render cycle">
<title>View render cycle</title>
<defs><marker id="ui-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker></defs>
<rect x="10" y="24" width="150" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="85" y="46" font-size="12" text-anchor="middle" fill="currentColor">views.render</text>
<line x1="160" y1="42" x2="220" y2="42" stroke="currentColor" marker-end="url(#ui-arrow)"/>
<rect x="220" y="24" width="170" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="305" y="46" font-size="12" text-anchor="middle" fill="currentColor">Host lays out and paints</text>
<line x1="305" y1="60" x2="305" y2="102" stroke="currentColor" marker-end="url(#ui-arrow)"/>
<text x="313" y="84" font-size="10" fill="currentColor">user input</text>
<rect x="220" y="102" width="170" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="305" y="124" font-size="12" text-anchor="middle" fill="currentColor">User acts on the widget</text>
<line x1="220" y1="120" x2="160" y2="120" stroke="currentColor" marker-end="url(#ui-arrow)"/>
<rect x="10" y="102" width="150" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="85" y="124" font-size="12" text-anchor="middle" fill="currentColor">ui-action event</text>
<line x1="85" y1="102" x2="85" y2="60" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#ui-arrow)"/>
<text x="93" y="84" font-size="10" fill="currentColor">plugin reacts</text>
<text x="10" y="172" font-size="11" fill="currentColor">Views render on open, invalidate and resize only.</text>
</svg>
<figcaption>The plugin renders on request. The host keeps the state of each widget between renders, and
user actions return to the plugin as events.</figcaption>
</figure>

## Errors and limits

| Call or case | Code | Cause |
|---|---|---|
| Any `ui` call | `not-permitted` | The grant is missing or the user declined it. |
| `open_panel`, `close_panel` | `not-found` | The panel id is not declared in the manifest. |
| `open_popup` | `not-found` | No popup or dialog node with that key is in the last committed tree of any open view. |
| `open_popup` | `invalid-argument` | Two open nodes have that key. |
| `open_popup` | `quota-exceeded` | A fifth popup would be open in one view. |
| `set_widget_state` | `not-found` | The key is unknown in that view. |
| `set_widget_state` | `invalid-argument` | The state kind does not fit the widget, for example `offset` on a button. |
| `write_surface` | `not-found` | No `surface` widget has that key. |
| `write_surface` | `invalid-argument` | The format differs from the widget, the data is not a whole number of rows, or it is over 16 MiB. |
| Any call | `revoked` | The authorization changed during the call; the plugin is re-activated. |

A `secret` text input sends its value in `changed` and `submitted`, because the plugin needs it. The
host redacts that value in logs and never stores it in the snapshot. Text that contains escape
sequences has them removed, and the host logs one warning per view.

## What the user sees

- A non-modal panel takes the whole content area. A modal panel is centred at 80 percent by 80 percent by default.
- Escape follows the order in [Escape](/pages/ui-model/#escape): a capturing key-capture, a text selection, the topmost popup, then a modal panel.
- Popups and panels are painted with the user's theme. The user alone chooses the theme and the
  `ui.stable_glyphs` setting, which the plugin reads from `theme()`.

## Used by

- Workspace search and the help reference open panels with `text-input` and list widgets.
- Chatroom and session stats show a panel or a popup.
- Voice confirmations open a popup with buttons.
- The settings screens render custom pages with `tabs`, `schema-form` and `leader-row`.

### Further fields and flags

The UI tree has optional fields and flags.

| Addition | Where | Page |
|---|---|---|
| `tooltip`, `tab-index`, `disabled-reason` on `ui-node` | node fields | [popovers](/pages/api-popovers/) |
| `hover-events`, `pointer-events` node flags | `node-flags` | [pointer](/pages/api-pointer/) |
| `open-popup-with` and the new `popup-anchor` cases (`at-key`, `at-tree-node`, `at-pointer`) | `ui` | [popovers](/pages/api-popovers/) |
| `menu-entry` and the `menu-provider` export | menus | [menus](/pages/api-menus/) |
| `theme-extras`: reduced motion, density, icon mode | `ui` | below |
| `widget-state` cases `geo`, `number`, `split`, `ids`, `tri` | state | below |
| `ui-action` kinds `hover-enter`, `hover-leave`, `pointer`, `drag-over`, `resized`, `filter-changed` | actions | [pointer](/pages/api-pointer/) |
| Settings views may embed display widgets | settings pages | below |

**Theme extras.** `theme-extras()` returns `{ reduced-motion, density, info, selection, border, icon-mode }`. Honour `reduced-motion` by drawing one static frame; `icon-mode` is `stable` only when the user opted in, and normal icons stay the default. `theme().stable-glyphs` is a derived convenience field: it always equals `icon-mode == stable`, and widgets follow `icon-mode`.

**Rust**

```rust
let extras = ui::theme_extras();
let frames = if extras.reduced_motion { 1 } else { 8 };
```

**TypeScript**

```ts
import { themeExtras } from '@ilium/plugin-sdk/host/ui';
const frames = themeExtras().reducedMotion ? 1 : 8;
```

**Focus and popups.** Tab and Shift+Tab follow the [focus order](/pages/ui-model/#focus-order). At most 4 popups per view are open (`quota-exceeded`). A node with `disabled-reason` is muted and shows the reason as its tooltip.

**Settings views.** A `settings-page` tree may now use layout, `split`, `scroll-view`, `markdown`, `diff-view`, `check-list`, the data-display widgets, `progress-row`, `meter`, `minimap`, `badge`, `spinner`, `notice`, `icon`, `image`, `surface`, `location-picker` and a read-only `code-view`. Still refused there: `popup`, `dialog`, `menu`, `video`, `terminal-view` and the input widgets other than `button`, `schema-form` and `location-picker`; they fail with `invalid-argument` on render and are logged.

### WIT

The contract below is an excerpt. The owner is the [WIT reference](/pages/reference-wit/), and that page wins if the two differ.

```wit
// Excerpt of ilium:host/ui-host in reference-wit
/// Marks the view as stale. The host calls views.render before the next frame. Always granted for own views.
invalidate: func(view: view-target);
/// Errors: not-found when the panel is not declared.
open-panel: func(panel: string) -> result<_, plugin-error>;
close-panel: func(panel: string);
/// Errors: not-found when no popup or dialog node with that key is in the last committed tree of any
/// open view, invalid-argument when two match, quota-exceeded for a fifth open popup in one view.
open-popup: func(popup: string, anchor: popup-anchor) -> result<_, plugin-error>;
open-popup-with: func(popup: string, anchor: popup-anchor, options: popup-options) -> result<_, plugin-error>;
close-popup: func(popup: string);
widget-state: func(view: view-target, key: string) -> option<widget-state>;
/// Errors: not-found for an unknown key, invalid-argument when the case does not fit the widget.
set-widget-state: func(view: view-target, key: string, state: widget-state) -> result<_, plugin-error>;
/// Width in cells by the cell width rule of the UI model page.
display-width: func(text: string) -> u16;
theme: func() -> theme-tokens;
theme-extras: func() -> theme-extras;
/// Rows are top to bottom. first-row plus the rows written must be within the surface height.
/// Errors: not-found for a key with no surface, invalid-argument for a bad length.
write-surface: func(view: view-target, key: string, first-row: u32, format: pixel-format, data: list<u8>) -> result<_, plugin-error>;
```

## See also

- [Widgets index](/pages/widgets-index/) for every widget, its props and its actions.
- [Layout](/pages/widgets-layout-and-containers/) for constraints, popups and containers.
- [Commands](/pages/api-commands/) for commands that open panels.
- [Keys](/pages/api-keys/) for keys that a focused view receives.
- [Panes](/pages/api-panes/) for the view of a basic pane.
- [Settings screens](/pages/settings-screens/) for settings pages and `schema-form`.
- [Capability: ui panels](/pages/capability-ui-panels/) for the grant and its dialog.
- [Basic primitive](/pages/primitives-basic/) for plugin pane bodies.
- [Errors and limits](/pages/errors-and-limits/) for the error codes and the restart rule.

Interface: `ilium:host/ui` and `ilium:types/ui` in `ilium:plugin@0.1.0`.
