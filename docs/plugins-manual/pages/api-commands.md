---
layout: page
title: commands
permalink: /pages/api-commands/
---

## commands

A command is a named action that a plugin contributes, such as "New note" or "Save note". The user runs a command from the command palette, a keybinding, a toolbar button, a context menu, a tree row action, a status bar segment, a voice phrase or the `ilium` CLI. The client side of the plugin receives every invocation of its own commands as a `command-invoked` event. Any plugin that holds the matching grant can invoke a command, including commands that other plugins contribute.

Commands connect the parts of a plugin that the user touches with the parts that do the work. A plugin declares a command in its manifest, handles the event in its client handler, and can grey the command out while its work runs. A command also carries a palette category and search keywords, and the menus that start commands can be static or filled by the plugin each time they open.

**You will need:**

- No grant to contribute a command, handle its `command-invoked` events, or change its state with `set-state`.
- `commands.invoke:<pattern>` to invoke a command of another plugin. Invoking your own command needs no grant. The pattern is an exact global id such as `ilium.git/status`, a plugin prefix such as `ilium.git/*`, or a bare `*` for every command. The bare `*` is high risk: the review dialog leaves it unchecked by default. See [Capability: commands.invoke](/pages/capability-commands-invoke/).
- `ui.chrome` for the contributions that put a button, a row action, a status segment or a frame slot in the kernel's chrome (`tree_toolbar`, `row_actions`, `status_segments`, `pane_frame_slots`). Menus and toolbar buttons of the agent primitive need no grant.
- The client side for `set-state` and for handling events. A server instance can call `invoke` but not `set-state`.
- The command's provider installed and enabled. An invocation of a command that no installed plugin contributes fails with `not-found`.

### Interface

```wit
// commands (C, plus S for invoke)
set-state: func(command: string, enabled: bool, checked: option<bool>) -> result<_, plugin-error>;   // own commands
invoke: func(command: string, arguments-json: string) -> result<request-id, plugin-error>;          // commands.invoke:<pattern>

// event command-invoked (own commands, never dropped)
//   command, arguments-json, origin, context { pane, view, buffer, line, virtual-row, menu-item }

// menu-provider (export of C, fragment with-menus)
menu-items: func(context: menu-context) -> list<menu-entry>;      // <= 64 entries, deadline 20 ms
```

### Command ids and manifest entries

A command is declared in the `contributes` section of the manifest. Its local id is written in the manifest, and its global id is the plugin id, a slash and the local id. So `example.notes` with the local id `new-note` becomes `example.notes/new-note`. Every function in this page takes the global id.

| Field | Type | Default | Meaning |
|---|---|---|---|
| `id` | string | required | Local id, lower case, hyphens allowed |
| `title` | string | required | Text shown in the palette and menus |
| `palette` | boolean | `true` | Lists the command in the command palette |
| `icon` | string | none | Tabler icon name |
| `voice` | list of 1 to 8 strings | none | Phrases of 1 to 64 characters that invoke the command by voice |
| `arguments_schema` | JSON Schema subset | none | Validates `arguments-json` (see [errors](#errors)) |
| `when` | context expression | none | Shows the command only when the expression holds |
| `category` | string, at most 32 characters | none | The group the palette lists the command under |
| `keywords` | list of at most 8 strings of at most 24 characters | none | Extra search terms that the palette matches besides the title |

The command palette is a kernel surface: it ranks, groups and draws the commands, and a plugin only supplies the entries. A plugin cannot open the palette, add a mode to it, or read what the user typed. `category` and `keywords` are the whole contract. A `category` or `keywords` value above its limit is refused at install with `manifest.contribution-limit`.

Keybindings name a command and a chord. The `mode` key is `global`, `prefix` (the chord is the key after the prefix) or `pane:<kind>` (active only in one pane kind). Chords use the grammar of [keys](/pages/api-keys/). When two plugins bind the same chord, the user's remap wins, then built-in bindings, then the earlier installed plugin (the rule of [reference-manifest](/pages/reference-manifest/)). A plugin can read the user's current bindings with [keymap.bindings](/pages/api-kernel-settings-and-keymap/).

### Context expressions

The `when` value is a context expression. It is a key compared with a string, combined with `&&`, `||`, `!` and parentheses:

```
pane.primitive == "agent" && agent.activity != "working"
```

| Key | Values |
|---|---|
| `pane.primitive` | `basic`, `terminal`, `agent`, `editor`, `none` |
| `pane.kind` | the global id of a pane kind, such as `example.notes.note` (the content kind) |
| `agent.activity` | the `agent-activity` values: `idle`, `working`, `awaiting-approval`, `menu`, `done` |
| `node.kind` | `pane`, `group`, `folder`, `project` |
| `node.file_extension` | the extension of the node's file, without the dot |
| `agent.signature` | the id of the detected agent signature |
| `view.focused` | `true` or `false` |
| `display_mode` | the active tree display mode: `normal`, `attention` or a plugin mode id |

A key that refers to no pane is false when no pane is in scope. An expression with an unknown key, an unknown value or bad syntax is refused at install: bad syntax with `manifest.when-syntax`, an unknown key with `manifest.when-key`. The keys `node.kind`, `node.file_extension`, `agent.signature`, `view.focused` and `display_mode` can be used in the `when` of `commands`, `toolbar_buttons`, `context_menu`, `row_actions` and `tree_toolbar`. The expression controls only whether the command is shown in the palette, a menu or a keybinding. It never blocks `commands.invoke`.

### Functions

| Function | Grant | Sides | Rust | TypeScript | Result |
|---|---|---|---|---|---|
| `set-state` | none (own commands) | client | `commands::set_state(command, enabled, checked)` | `setState(command, enabled, checked)` | `Result<(), PluginError>`; throws in TypeScript |
| `invoke` | `commands.invoke:<pattern>` | server, client | `commands::invoke(command, arguments_json).await` | `await invoke(command, argumentsJson)` | `Result<RequestId, PluginError>`; `Promise<bigint>` |
| `invoke` request form | as `invoke` | server, client | `commands::invoke_request(...)` | `invokeRequest(...)` | `Result<RequestId, PluginError>`; `bigint` |

`set-state` takes `enabled: bool` and `checked: Option<bool>` (`undefined` in TypeScript). A `None` leaves the checked mark as it is. A `Some(true)` or `Some(false)` sets the check mark that the palette and menus draw for the command.

The async `invoke` resolves when the command has run. Use `invoke_request` only when you need the `request-id` yourself, for example to match the completion in `on-events`.

### Events

| Event | Field | Rust | TypeScript | Meaning |
|---|---|---|---|---|
| `command-invoked` | `command` | `String` | `string` | Global id of the command |
| | `arguments-json` | `String` | `string` | One JSON object, `{}` when none was given |
| | `origin` | `InvocationOrigin` | string union | How the user started it: `keybinding`, `palette`, `toolbar`, `menu`, `voice`, `cli`, `toast`, `row-action`, `status-segment`, `tree-toolbar`, `frame-slot`, `virtual-row-drop` or `text`. |
| | `context` | `CommandContext` | `{ pane, view, buffer, line, virtualRow, menuItem }` | Pane node, view, buffer and line, each optional. Also `virtual-row` (the row id) and `menu-item` (the id of the clicked menu entry). |

The event is never dropped. It is delivered to the client handler of the plugin that owns the command, and it needs no grant. The types are in `ilium:types/events`, so Rust code imports them from `ilium_plugin_sdk::types::events`.

<figure>
<svg viewBox="0 0 400 232" role="img" aria-label="Command routing from origins to the plugin and back">
<title>Command routing</title>
<desc>User origins reach the command registry, which delivers command-invoked to the owning plugin. The plugin sets state back with set-state, and invokes other commands whose results return as request-completed.</desc>
<defs>
<marker id="cmd-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="10" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
<text x="200" y="26" font-size="12" text-anchor="middle" fill="currentColor">User origins: keybinding, palette, toolbar, menu,</text>
<text x="200" y="42" font-size="12" text-anchor="middle" fill="currentColor">row action, voice or cli</text>
<line x1="200" y1="52" x2="200" y2="78" stroke="currentColor" marker-end="url(#cmd-arrow)"/>
<text x="210" y="70" font-size="11" fill="currentColor">invoke by origin</text>
<rect x="10" y="80" width="380" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="200" y="102" font-size="12" text-anchor="middle" fill="currentColor">Command registry: when, grants, enabled</text>
<line x1="200" y1="116" x2="200" y2="142" stroke="currentColor" marker-end="url(#cmd-arrow)"/>
<text x="210" y="134" font-size="11" fill="currentColor">command-invoked</text>
<rect x="10" y="144" width="380" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="200" y="166" font-size="12" text-anchor="middle" fill="currentColor">Plugin handler: example.notes (client)</text>
<line x1="60" y1="144" x2="60" y2="118" stroke="currentColor" marker-end="url(#cmd-arrow)"/>
<text x="68" y="134" font-size="11" fill="currentColor">set-state</text>
<line x1="340" y1="144" x2="340" y2="118" stroke="currentColor" marker-end="url(#cmd-arrow)"/>
<text x="348" y="134" font-size="11" fill="currentColor">invoke</text>
<text x="10" y="204" font-size="11" fill="currentColor">An invoke runs the target command and returns</text>
<text x="10" y="220" font-size="11" fill="currentColor">its outcome as request-completed.</text>
</svg>
<figcaption>The registry checks the when expression and the grant, then delivers the event to the owning plugin. The plugin answers with set-state or invoke.</figcaption>
</figure>

### Example 1: set-state and invoke

This plugin handles its own two commands. Saving disables the "Save note" command while the git status is refreshed, so the user cannot start a second save. It needs `commands.invoke:ilium.git/status` for the invoke. The manifest declares the commands, a palette category with keywords, and the grant:

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.notes",
  "contributes": {
    "commands": [
      { "id": "new-note", "title": "New note", "icon": "notes",
        "category": "Notes", "keywords": ["create", "write", "jot"],
        "when": "pane.primitive == \"basic\"" },
      { "id": "save-note", "title": "Save note", "icon": "device-floppy",
        "category": "Notes", "keywords": ["store", "write"] }
    ],
    "keybindings": [{ "command": "new-note", "chord": "ctrl+alt+n", "mode": "global" }]
  },
  "capabilities": { "client": ["commands.invoke:ilium.git/status"] }
}
```

**Rust**

```rust
use ilium_plugin_sdk::host::commands;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::task;
use ilium_plugin_sdk::types::events::CommandInvokedEvent;

const NEW_NOTE: &str = "example.notes/new-note";
const SAVE_NOTE: &str = "example.notes/save-note";

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct Notes;

impl Lifecycle for Notes {}

impl Handler for Notes {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if let Event::CommandInvoked(invoked) = event {
                handle_invocation(invoked);
            }
        }
        Vec::new()
    }
}

fn handle_invocation(invoked: CommandInvokedEvent) {
    match invoked.command.as_str() {
        NEW_NOTE => log::log(Level::Info, &format!("new note from {:?}", invoked.origin)),
        SAVE_NOTE => refresh_status(),
        _ => {}
    }
}

fn refresh_status() {
    if let Err(error) = commands::set_state(SAVE_NOTE, false, None) {
        log::log(Level::Warn, &format!("cannot disable save: {}", error.message));
    }
    task::spawn(async {
        match commands::invoke("ilium.git/status", "{}").await {
            Ok(()) => log::log(Level::Info, "git status refreshed"),
            Err(error) => {
                log::log(Level::Warn, &format!("git status failed: {}", error.message));
            }
        }
        if let Err(error) = commands::set_state(SAVE_NOTE, true, None) {
            log::log(Level::Warn, &format!("cannot enable save: {}", error.message));
        }
    });
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { invoke, setState } from '@ilium/plugin-sdk/host/commands';
import { log } from '@ilium/plugin-sdk/host/log';

const NEW_NOTE = 'example.notes/new-note';
const SAVE_NOTE = 'example.notes/save-note';

async function refreshStatus(): Promise<void> {
  setState(SAVE_NOTE, false, undefined);
  try {
    await invoke('ilium.git/status', '{}');
    log('info', 'git status refreshed');
  } catch (error) {
    log('warn', `git status failed: ${String(error)}`);
  } finally {
    setState(SAVE_NOTE, true, undefined);
  }
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {},
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'command-invoked') continue;
      const { command, origin } = event.val;
      if (command === NEW_NOTE) log('info', `new note from ${origin}`);
      if (command === SAVE_NOTE) void refreshStatus();
    }
    return [];
  },
});
```

### Menus: context menus, toolbar buttons and row actions

Four contribution points start a command from a click. They share the `when` grammar above, an icon, and the origin of the resulting `command-invoked`. Counts and grants are in the table.

| Key | Fields | Grant | Cap | Origin of the invocation |
|---|---|---|---|---|
| `context_menu` | `id`, `title`, `command`, `target` (`terminal`, `tree-node`, `editor`, `pane:<kind>`), and `icon`, `when`, `dynamic`, `items` | none | none | `menu` |
| `toolbar_buttons` | `id`, `icon`, `label`, `command`, `applies_to` (signature ids or `all`), and `submenu`, `right_click_command` | none | none | `toolbar` |
| `row_actions` | `id`, `icon`, `label`, `command`, `applies_to`, `when`, `order` (0 to 1000, default 500) | `ui.chrome` | 4 | `row-action`, arguments `{"node": <id>}` or `{"row": "<virtual row id>"}` |
| `tree_toolbar` | `id`, `icon`, `label` (at most 24 characters), `command`, `caption` (at most 12 cells), `popup_view`, `when` | `ui.chrome` | 4 | `tree-toolbar` |

A `context_menu` entry with `dynamic: true` makes the host ask the plugin for its entries each time the menu opens. A static menu lists `items` in the manifest. An item is a `menu-entry` with a `parent` reference; the depth is at most 2, and a deeper tree is refused at install with `manifest.menu-depth`. A `toolbar_buttons` entry with `submenu` opens the submenu on a left click instead of running its `command`; the value is a static `items` list or the string `"dynamic"`, served by the same export with the menu id `<button id>`. `right_click_command` runs on a right click. The client draws the menu: placement, the stack of submenus and the keyboard handling are the host's, and the plugin supplies entries only.

#### The `menu-provider` export

A dynamic menu needs the client side to list `menu-provider` in its `exports`.

```wit
// Excerpt of ilium:exports/menu-provider in reference-wit
record menu-context { menu: string, node: option<node-id>, pane: option<node-id>, view: option<view-target>,
                      key: option<string>, buffer: option<buffer-id>, line: option<u32>, selection: option<string> }
record menu-entry {
  id: string, label: string, icon: option<string>, shortcut: option<string>,
  parent: option<string>,                 // id of a submenu entry; depth <= 2
  is-submenu: bool, separator-before: bool,
  checked: option<tri-state>, disabled-reason: option<string>,   // <= 40 characters
  tooltip: option<tooltip>, danger: bool,
}
menu-items: func(context: menu-context) -> list<menu-entry>;      // <= 64 entries
```

| `menu-entry` field | Rust | TypeScript | Meaning |
|---|---|---|---|
| `id` | `String` | `string` | Id of the entry, returned in `menu-item` of the click context |
| `label` | `String` | `string` | Text of the row |
| `icon` | `Option<String>` | `string \| undefined` | Catalogue id or glyph |
| `shortcut` | `Option<String>` | `string \| undefined` | Text shown at the right edge, informational only |
| `parent` | `Option<String>` | `string \| undefined` | Id of the submenu entry this entry belongs to |
| `is_submenu` | `bool` | `isSubmenu` | The entry opens the entries that name it as `parent` |
| `separator_before` | `bool` | `separatorBefore` | A rule is drawn above the entry |
| `checked` | `Option<TriState>` | `'off' \| 'on' \| 'mixed' \| undefined` | A check mark; `off` is an empty mark, `undefined` has no mark |
| `disabled_reason` | `Option<String>` | `string \| undefined` | When set, the entry is dimmed and cannot be chosen; at most 40 characters |
| `tooltip` | `Option<Tooltip>` | `Tooltip \| undefined` | Hover help, at most 200 characters |
| `danger` | `bool` | `boolean` | Draws the entry in the warning style |

The host calls `menu-items` with the deadline of 20 ms. On a timeout the menu opens with the static entries only, and the breach counts as a deadline breach in the plugin's health record (the breach count that can turn the plugin off, see [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/)). The export is activated by the trigger `menu:<id>`; until the instance answers, the host shows the static entries. A click arrives as `command-invoked` with `command` set to the menu's command and `arguments-json` `{"item": "<id>", ...context}`, and `context.menu-item` carries the entry id. At most 64 entries are shown; further ones are cut with a `… N more` row.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::exports::menu_provider::MenuContext;
use ilium_plugin_sdk::types::ui::MenuEntry;
use ilium_plugin_sdk::types::text::TriState;

fn entry(id: &str, label: &str) -> MenuEntry {
    MenuEntry {
        id: id.into(), label: label.into(), icon: None, shortcut: None, parent: None,
        is_submenu: false, separator_before: false, checked: None,
        disabled_reason: None, tooltip: None, danger: false,
    }
}

impl MenuProvider for NoteMenus {
    fn menu_items(&mut self, context: MenuContext) -> Vec<MenuEntry> {
        let mut order = MenuEntry { is_submenu: true, ..entry("order", "Order notes by") };
        order.separator_before = true;
        let mut by_title = entry("order-title", "Title");
        by_title.parent = Some("order".into());
        by_title.checked = Some(if self.order == "title" { TriState::On } else { TriState::Off });
        let mut by_date = entry("order-date", "Date");
        by_date.parent = Some("order".into());
        by_date.checked = Some(if self.order == "date" { TriState::On } else { TriState::Off });
        let mut delete = entry("delete", "Delete note");
        delete.danger = true;
        if context.node.is_none() {
            delete.disabled_reason = Some("select a note first".into());
        }
        vec![entry("open", "Open note"), order, by_title, by_date, delete]
    }
}
```

**TypeScript**

```ts
import type { MenuContext, MenuEntry } from '@ilium/plugin-sdk/types/menus';

function entry(id: string, label: string): MenuEntry {
  return {
    id, label, icon: undefined, shortcut: undefined, parent: undefined,
    isSubmenu: false, separatorBefore: false, checked: undefined,
    disabledReason: undefined, tooltip: undefined, danger: false,
  };
}

// the `menuItems` member of definePlugin
export function menuItems(context: MenuContext): MenuEntry[] {
  const order = { ...entry('order', 'Order notes by'), isSubmenu: true, separatorBefore: true };
  const byTitle = { ...entry('order-title', 'Title'), parent: 'order',
    checked: currentOrder === 'title' ? 'on' as const : 'off' as const };
  const byDate = { ...entry('order-date', 'Date'), parent: 'order',
    checked: currentOrder === 'date' ? 'on' as const : 'off' as const };
  const remove = { ...entry('delete', 'Delete note'), danger: true,
    disabledReason: context.node === undefined ? 'select a note first' : undefined };
  return [entry('open', 'Open note'), order, byTitle, byDate, remove];
}
```

The manifest entry that uses it:

```json
{
  "client": { "exports": ["menu-provider"] },
  "contributes": {
    "context_menu": [
      { "id": "note-actions", "title": "Note actions", "command": "example.notes/menu-click",
        "target": "tree-node", "icon": "notes", "dynamic": true,
        "when": "node.kind == \"pane\" && pane.kind == \"example.notes.note\"" }
    ]
  }
}
```

The client's own context menus show how such a menu looks: a rounded block with the chrome title, a blank row at top and bottom, rows with two cells of padding, `✓` marking the checked entry in a submenu, `▸` at the right edge of an entry that opens a submenu, and a `─` rule above entries with `separator_before`. The two frames below follow that row layout, with the example plugin's entries in place of the client's own and the icons switched off.

<figure>
<pre class="term">╭  ≡ ● · Note actions──────────────╮
│                                  │
│  Open note                       │
│  ──────────────────────────────  │
│  Order notes by                ▸ │
│  <span class="t-dim">Delete note</span>                     │
│  <span class="t-dim">  select a note first</span>           │
│                                  │
╰──────────────────────────────────╯
╭  ≡ ● · Order notes by────────────╮
│                                  │
│    Title                         │
│  ✓ Date                          │
│                                  │
╰──────────────────────────────────╯
</pre>
<figcaption>The menu of the example, composed by hand from the client's popup layout (ui.rs <code>popup_action_line</code>, <code>popup_rows</code>): the row prefix, the <code>▸</code> and <code>✓</code> markers and the separator are the client's. Where the menu shows the disabled reason, the client's own submenu model appends it to the label; its exact placement is the host's choice, so the second dimmed line shows one possible placement.</figcaption>
</figure>

### Palette, keywords and categories

The palette lists a command under its `category` and finds it by title, id, `category` and `keywords`. The host does the matching, case-insensitively and ranked by prefix first, the same way as for catalogues. A command with `palette: false` is not listed, but it still runs from keybindings, menus and `invoke`.

<figure>
<pre class="term">╭  ≡ ● · Commands───────────────────────────────────╮
│ <span class="t-prompt">&gt;</span> note                                            │
│                                                   │
│   Notes                                           │
│ <span class="t-hi">  New note                         ctrl+alt+n    </span> │
│   Open notes panel                                │
│ <span class="t-dim">  Save note                         disabled      </span>│
│                                                   │
│ <span class="t-dim">Enter run · Esc close · 3 commands match</span>          │
╰───────────────────────────────────────────────────╯
</pre>
<figcaption>The command palette as the specification describes it: a kernel surface, grouped by category, with disabled commands dimmed. The client build in this tree has no command palette yet, so the frame is composed by hand from the client's chrome and hint-bar conventions and is not a capture.</figcaption>
</figure>

A command disabled by `set-state` stays listed, dimmed. Pressing its keybinding does nothing. When the command runs, the user sees its effect: a new pane, a panel or a notice. The palette shows no status for an invocation. A refused invocation is written to the plugin log as a refusal line, the same way as a refused HTTP call.

### Errors

Synchronous errors come back from the call itself. Errors in the completion come back later as `request-completed`, with the outcome `failed` or `cancelled`.

| Call | Code | Cause |
|---|---|---|
| `set-state` | `not-found` | The command is not one of the plugin's own commands |
| `set-state` | `unsupported` | Called from a server instance |
| `invoke` | `not-found` | No installed plugin contributes that global id |
| `invoke` | `not-permitted` | The id is outside the granted pattern |
| `invoke` | `invalid-argument` | `arguments-json` is not one JSON object, is over 64 KiB, or fails `arguments_schema` |
| `invoke` | `conflict` | The command is disabled with `set-state` |
| `invoke` | `busy`, `quota-exceeded` | Pending requests are full, or the CPU budget is spent |
| completion | `ok` | The command ran |
| completion | `failed(code)` | The command returned its own error code |
| completion | `failed(timed-out)` | The command did not reply within 30 seconds |
| completion | `cancelled` | The plugin cancelled the request, the application shut down, or the owning plugin was disabled while the command ran |

A command that is disabled and then invoked by the user through a keybinding or the palette gives the intent `refused(conflict)`, with the same meaning.

Manifest errors (the install is refused with the key path):

| Code | Cause |
|---|---|
| `manifest.when-syntax` | A `when` value with bad syntax or an unknown value |
| `manifest.when-key` | An unknown `when` key |
| `manifest.menu-depth` | Static `context_menu` or `toolbar_buttons.submenu` items nested deeper than 2 |
| `manifest.contribution-limit` | More `row_actions`, `tree_toolbar` entries and so on than the cap of 4, or a `category` or `keywords` value above its limit |
| `manifest.contribution-target` | `row_actions.applies_to` names an unknown target |
| `manifest.grant-wildcard` | A `commands.invoke` grant uses a wildcard where the pattern grammar does not allow one, such as `*/status` or `ilium.git/prefix*` |

### Limits

| Limit | Value |
|---|---|
| `arguments-json` | 64 KiB, one JSON object |
| Command completion | 30 s, then `failed(timed-out)` |
| `category` | 32 characters |
| `keywords` | 8 entries of at most 24 characters |
| `voice` phrases | 1 to 8, each 1 to 64 characters |
| Menu entries per menu | 64, then `… N more` |
| Submenu depth | 2 |
| `disabled-reason` | 40 characters |
| Tooltip | 200 characters |
| `menu-items` deadline | 20 ms, static entries on a breach |
| `tree_toolbar`, `row_actions` | 4 each, per plugin |
| `tree_toolbar` `label`, `caption` | 24 characters, 12 cells |

### Edge cases

- Own commands need no grant, both for `set-state` and for `invoke`. A plugin can invoke its own command without holding any `commands.invoke` grant.
- `when` is not checked by `invoke`. A plugin can run a command that is hidden from the palette.
- A command returns no data. The completion carries an outcome only. To send data back, use [services](/pages/api-services/) or a [topic](/pages/api-messages-and-topics/).
- The `*` pattern never crosses a slash. `*/status` and `ilium.git/prefix*` are refused at install with `manifest.grant-wildcard`.
- `set-state` changes the state until the next call or until the plugin restarts. A restart resets every command to its manifest state.
- A dynamic menu whose provider is slow shows the static entries for that opening only; the next opening asks again.

### See also

- [kernel settings and keymap reads](/pages/api-kernel-settings-and-keymap/) for the user's bindings.
- [ui](/pages/api-ui/) for panels that a command opens, and for the `menu` widget that uses `menu-entry`.
- [keys](/pages/api-keys/) for grabs, which receive keys without going through commands.
- [events](/pages/api-events/) for the full event catalogue.
- [intents](/pages/api-intents/) for the `invoke-command` intent, which submits an invocation through `on-events`.
- [Plugin exports](/pages/api-exports/) for `menu-provider`.
- [capability-commands-invoke](/pages/capability-commands-invoke/) for the grant and its review text.
- [widgets-text](/pages/widgets-text/) for markdown links and key hints that run a command.
- [rust-sdk](/pages/rust-sdk/) and [typescript-sdk](/pages/typescript-sdk/) for the SDK layout.

Checked against `ilium:plugin@0.1.0`.

## Production additions: T3-abi-api

- [API-214] Registering a command id that another plugin already owns MUST return `conflict` and leave both registrations unchanged, and unregistering a command during its own invocation MUST let that invocation finish. Test: register the same id from two plugins and assert `conflict` with both registrations unchanged; unregister a command inside its own handler and assert that the handler completes and its result is delivered.
