---
layout: page
title: menu provider
permalink: /pages/api-menu-provider/
---

## menu provider

The `menu-provider` export lets a client plugin build a context menu or a toolbar submenu at the moment it opens. A static menu is a list in the manifest. A dynamic menu is built from what the user clicked: the node, the pane, the editor line, the selected text. The host calls the export, draws the returned entries with its own menu painter, and delivers the click as a `command-invoked` event. The plugin never draws the menu and never decides where it appears.

### You will need

| Need | Value |
|---|---|
| Side | Client only. The export fragment is `with-menus` in the world `ilium:client/base`. |
| Manifest | `"exports": ["menu-provider"]` on the client side, plus a `context_menu` entry with `"dynamic": true` or a `toolbar_buttons` entry with `"submenu": "dynamic"`. |
| Grant | None for the export. The menu target decides what the click handler may do: its own host calls need their own grants. |
| Rust | Crate `ilium-plugin-sdk`, trait `ilium_plugin_sdk::exports::MenuProvider`. |
| TypeScript | Package `@ilium/plugin-sdk`, optional member `menuItems` of `definePlugin`. |
| Activation | The trigger `menu:<id>` starts a stopped client instance when a dynamic menu opens. Until the instance answers, the host shows the static entries. |

### Interface

```wit
// Excerpt of ilium:exports/menu-provider in reference-wit
record menu-context {
  menu: string,                      // the menu id: a context_menu id, or a toolbar button id
  node: option<node-id>,             // tree node under the click
  pane: option<node-id>,             // pane that owns the click
  view: option<view-target>,         // plugin view, when the click was inside one
  key: option<string>,               // widget key inside that view
  buffer: option<buffer-id>,         // editor buffer, for target "editor"
  line: option<u32>,                 // editor line, 0 based
  selection: option<string>,         // selected text, at most 64 KiB
}

record menu-entry {
  id: string,                        // stable within the menu
  label: string,
  icon: option<string>,              // catalogue id or glyph
  shortcut: option<string>,          // display only, for example "ctrl+shift+c"
  parent: option<string>,            // id of a submenu entry; depth at most 2
  is-submenu: bool,
  separator-before: bool,
  checked: option<tri-state>,        // off, on or mixed
  disabled-reason: option<string>,   // at most 40 characters; a disabled entry shows it
  tooltip: option<tooltip>,
  danger: bool,
}

menu-items: func(context: menu-context) -> list<menu-entry>;   // at most 64 entries
```

`menu-entry` is also the item record of the `menu` widget, so the same value describes a menu inside a plugin view.

### Functions

| WIT function | Rust | TypeScript | Result | What it does |
|---|---|---|---|---|
| `menu-items` | `MenuProvider::menu_items(&mut self, context: MenuContext) -> Vec<MenuEntry>` | `menuItems(context: MenuContext): MenuEntry[]` | `list<menu-entry>` | Returns the entries for the menu that is opening. |

### How the call behaves

- **When.** The host calls `menu-items` each time a dynamic menu opens, with the click context. A menu opened twice calls the export twice. The plugin may cache nothing across calls except what it keeps itself.
- **Deadline.** 20 ms, soft. On a timeout the menu opens with the static `items` only, and the breach counts toward the streak rule of the plugin (the third in a row traps; see [Host scheduling](/pages/reference-host-scheduling/)).
- **Open latency.** The client calls all providers of the menu at once, one call per plugin side. It opens the menu when every provider has answered or after 20 ms, whichever comes first. The static entries are always in the first frame; entries of a provider that answered in time are in it too, and entries of a late provider are dropped for that opening. With three providers that answer in under 1 ms the menu is on screen within 5 ms of the click (benchmark B11).
- **What the user sees while waiting.** Nothing is drawn until the menu opens, so the user never sees a menu that fills in after it appeared. The wait is at most 20 ms.
- **T2 and T3.** A provider in a helper pays one round trip of about 0.5 ms at the 99th percentile inside the 20 ms. Keep the work small. A provider that is not running is activated by the trigger `menu:<id>`, and the first opening shows the static entries only because activation takes longer than the 20 ms wait. The menu does not wait for an activation.
- **[HST-476]** The client MUST call all menu providers of one opening at once, MUST open the menu when all have answered or after 20 ms, and MUST drop the entries of a late provider for that opening.
- **[HST-477]** The static entries MUST be in the first frame of the menu, and a menu MUST NOT change after its first frame is drawn.
- **[HST-478]** An opening MUST NOT wait for the activation of a provider that is not running.
- **Size.** At most 64 entries. Extra entries are cut and the host appends a last row `… N more`.
- **Depth.** A submenu entry has `is-submenu: true`. Its children name it in `parent`. Depth is at most 2. A static list nested deeper is refused at install with `manifest.menu-depth`. 
- **Click.** Choosing an entry arrives as `command-invoked` with `command` set to the command of the menu, `origin` set to `menu`, and the `context` field carrying `menu-item` with the chosen id, and `arguments-json` equal to `{"item": "<id>"}` merged with the click context fields that are present, for example `{"item":"open-in-editor","node":42,"line":17}`. A disabled entry cannot be chosen.
- **Placement.** The host places the menu, clamps it to the screen, and opens a submenu to the right of its parent entry. A submenu opens on hover after a short delay or on Right. The plugin cannot influence either.

<figure>
<svg viewBox="0 0 420 250" role="img" aria-label="Sequence of a dynamic menu from right click to command">
<defs>
<marker id="menu-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<title>Dynamic menu sequence</title>
<desc>The user right clicks. The host calls menu-items on the client plugin with the click context, draws the entries, and later delivers the chosen entry as command-invoked.</desc>
<text x="50" y="16" font-size="12" text-anchor="middle" fill="currentColor">User</text>
<text x="190" y="16" font-size="12" text-anchor="middle" fill="currentColor">Host (client)</text>
<text x="360" y="16" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
<line x1="50" y1="24" x2="50" y2="240" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="190" y1="24" x2="190" y2="240" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="360" y1="24" x2="360" y2="240" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="50" y1="48" x2="188" y2="48" stroke="currentColor" marker-end="url(#menu-arrow)"/>
<text x="56" y="42" font-size="10" fill="currentColor">right click</text>
<line x1="190" y1="84" x2="358" y2="84" stroke="currentColor" marker-end="url(#menu-arrow)"/>
<text x="196" y="78" font-size="10" fill="currentColor">menu-items(menu-context)</text>
<line x1="358" y1="116" x2="192" y2="116" stroke="currentColor" marker-end="url(#menu-arrow)"/>
<text x="206" y="110" font-size="10" fill="currentColor">list of menu-entry, within 20 ms</text>
<line x1="188" y1="148" x2="52" y2="148" stroke="currentColor" marker-end="url(#menu-arrow)"/>
<text x="60" y="142" font-size="10" fill="currentColor">menu drawn by the host</text>
<line x1="50" y1="180" x2="188" y2="180" stroke="currentColor" marker-end="url(#menu-arrow)"/>
<text x="56" y="174" font-size="10" fill="currentColor">Enter on an entry</text>
<line x1="190" y1="214" x2="358" y2="214" stroke="currentColor" marker-end="url(#menu-arrow)"/>
<text x="196" y="208" font-size="10" fill="currentColor">command-invoked {item}</text>
</svg>
<figcaption>A dynamic menu. The plugin returns data; the host draws the menu and routes the click.</figcaption>
</figure>

### Example: a line menu for an editor

A plugin offers three entries for an editor line: open the path on the line, copy it, and a submenu with two refactor actions. The entries depend on whether the line holds a path, which is why the menu is dynamic. The manifest entry is `{ "id": "line-actions", "target": "editor", "dynamic": true, "command": "my.plugin/line-action" }` under `contributes.context_menu`.

**Grants:** `editor.read` (to read the line text for the path test).

**Rust**

```rust
use ilium_plugin_sdk::exports::MenuProvider;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::editor;
use ilium_plugin_sdk::exports::menu_provider::MenuContext;
use ilium_plugin_sdk::types::ui::MenuEntry;
use ilium_plugin_sdk::types::text::TriState;

impl MenuProvider for Plugin {
    fn menu_items(&mut self, context: MenuContext) -> Vec<MenuEntry> {
        let (Some(buffer), Some(line)) = (context.buffer, context.line) else {
            return Vec::new();
        };
        let text = editor::read_lines(buffer, line).unwrap_or_default();
        let has_path = looks_like_path(&text);
        vec![
            MenuEntry::new("open", "Open referenced path")
                .icon("file")
                .disabled_reason(if has_path { None } else { Some("no path on this line") }),
            MenuEntry::new("copy", "Copy line").shortcut("ctrl+shift+c"),
            MenuEntry::new("refactor", "Refactor").submenu().separator_before(true),
            MenuEntry::new("rename", "Rename symbol").parent("refactor"),
            MenuEntry::new("extract", "Extract function").parent("refactor")
                .checked(TriState::Off),
        ]
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import * as editor from '@ilium/plugin-sdk/primitives/editor';
import type { MenuContext, MenuEntry } from '@ilium/plugin-sdk/types';

export const { menuProvider } = definePlugin({
  side: 'client',
  menuItems(context: MenuContext): MenuEntry[] {
    if (context.buffer === undefined || context.line === undefined) return [];
    const text = editor.lineText(context.buffer, context.line) ?? '';
    const hasPath = looksLikePath(text);
    return [
      { id: 'open', label: 'Open referenced path', icon: 'file',
        disabledReason: hasPath ? undefined : 'no path on this line' },
      { id: 'copy', label: 'Copy line', shortcut: 'ctrl+shift+c' },
      { id: 'refactor', label: 'Refactor', isSubmenu: true, separatorBefore: true },
      { id: 'rename', label: 'Rename symbol', parent: 'refactor' },
      { id: 'extract', label: 'Extract function', parent: 'refactor', checked: 'off' },
    ];
  },
});
```

Fields not set take the defaults: `icon`, `shortcut`, `parent`, `checked`, `disabled-reason` and `tooltip` are absent, and `is-submenu`, `separator-before` and `danger` are false.

Handling the click:

**Rust**

```rust
fn on_command(&mut self, event: CommandInvokedEvent) {
    let args: serde_json::Value = serde_json::from_str(&event.arguments_json).unwrap_or_default();
    match args["item"].as_str() {
        Some("open") => self.open_path(args["line"].as_u64()),
        Some("copy") => self.copy_line(args["line"].as_u64()),
        _ => {}
    }
}
```

**TypeScript**

```ts
function onCommand(event: CommandInvokedEvent): void {
  const args = JSON.parse(event.argumentsJson) as { item?: string; line?: number };
  if (args.item === 'open') openPath(args.line);
  else if (args.item === 'copy') copyLine(args.line);
}
```

### What the user sees

The host draws the menu with its rounded block, a two-space inset on every row, the check marker, the icon, and `▸` at the right of a submenu entry. The selected row is reversed. A disabled entry is dimmed and its reason is shown as a tooltip.

<pre class="term">
╭──────────────────────────────╮
│   Open referenced path       │
│   Copy line                  │
│                              │
│   Refactor                 ▸ │
╰──────────────────────────────╯
</pre>

The submenu opens beside the parent with its parent label as title:

<pre class="term">
╭──────────────────────────────╮╭──Refactor─────────────────╮
│   Open referenced path       ││   Rename symbol           │
│   Copy line                  ││   Extract function        │
│                              │╰───────────────────────────╯
│   Refactor                 ▸                              │
╰───────────────────────────────────────────────────────────╯
</pre>

A checked entry draws `✓ ` before its icon; an unchecked tri-state entry keeps two blank cells so labels stay aligned. This is the layout of the host function `popup_action_line` of the client, and the glyph table in the manual's glyph reference.

### Errors and limits

| Situation | Result |
|---|---|
| Deadline of 20 ms exceeded | Static `items` shown, breach counted. |
| More than 64 entries | Cut to 64, last row `… N more`. |
| Depth above 2 in a static list | Install refused with `manifest.menu-depth`. |
| `disabled-reason` longer than 40 characters | Truncated with `…`. |
| Trap inside `menu-items` | The menu opens with static items only and the plugin restarts under the usual rule. |

### See also

- [Plugin exports](/pages/api-exports/) for the export list and the deadline table.
- [Commands](/pages/api-commands/) for `command-invoked`, `context_menu` and `toolbar_buttons`.
- [Events](/pages/api-events/) for the click event.
- [Widgets: collections](/pages/widgets-collections/) for the `menu` widget that uses the same record.
- [Errors and limits](/pages/reference-errors-and-limits/) for `manifest.menu-depth`.

Return unique entry ids and valid parents; an entry whose `parent` is unknown is dropped, and of two entries with the same `id` only the first is kept. Each dropped entry gets a `warn` line in the plugin log, and the rest of the menu is still shown. Contract: `ilium:plugin@0.1.0`, export fragment `with-menus`.
