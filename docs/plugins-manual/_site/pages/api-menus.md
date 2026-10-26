# menus

## menus

A menu is a list of entries the user picks from: a context menu opened by a right click, a submenu of a toolbar button, or the `menu` widget in a view. The menu model gives every entry an icon, a shortcut label, a checked state, a disabled reason, a tooltip and a danger flag, allows submenus two levels deep, and lets a plugin compute the entries when the menu opens through the `menu-provider` export.

### What you need

- **Grant.** `ui.panels` for menus in the plugin's own views; `ui.chrome` for menus attached to kernel surfaces (see [tree chrome](/pages/api-tree-chrome/)). The commands behind the entries need no extra grant.
- **Manifest.** Static entries in `contributes.context_menu` or `toolbar_buttons.submenu`; `dynamic: true` plus the export `menu-provider` for computed entries.

```json
"exports": ["menu-provider"],
"contributes": { "context_menu": [{ "id": "pane-actions", "applies_to": ["pane"], "dynamic": true, "command": "run-action" }] }
```

### Interface

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
// export menu-provider
menu-items: func(context: menu-context) -> list<menu-entry>;      // <= 64 entries
```

The host calls `menu-items` when a `dynamic` menu opens. The deadline is 20 ms; on a timeout the menu opens with its static entries only and the breach is counted. A click arrives as `command-invoked` with the menu's command and `arguments-json` `{"item": "<id>", ...context}`.

### Rules

- A menu has at most 64 entries; more are cut with `… N more`. Submenus nest at most 2 deep; static menus nested deeper fail the install with `manifest.menu-depth`.
- `disabled-reason` greys the entry, takes no click, and shows the reason as its tooltip (at most 40 characters).
- `icon` is a catalogue id or one glyph; normal icons are the default, a stable glyph only when the user opted in.
- `danger` draws the entry in the warning style.
- Placement, the submenu delay and the submenu stack belong to the host.

<figure>
<pre class="term">╭  ≡ ● · Pane ───────────────────╮
│ 🔎 Find in output     Ctrl+F   │
│ ✓ Follow output                │
│ 📋 Copy                      › │
│ ───────────────────────────────│
│ ❄ Freeze (no process)          │
│ 🚫 Close pane                  │
╰────────────────────────────────╯
</pre>
<figcaption>A context menu with an icon, shortcut label, check mark, submenu arrow (the client uses the single right angle quotation mark for it), separator and a dangerous entry. Rounded border and chrome title as in the client's panels; the exact row layout is the host's.</figcaption>
</figure>

### Examples

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;

impl MenuProvider for Actions {
    fn menu_items(&mut self, context: MenuContext) -> Vec<MenuEntry> {
        let frozen = context.pane.map_or(false, |pane| self.is_frozen(pane));
        vec![
            MenuEntry::new("freeze", if frozen { "Unfreeze" } else { "Freeze" }).icon("❄"),
            MenuEntry::new("copy", "Copy").submenu(),
            MenuEntry::new("copy-last", "Last answer").parent("copy"),
            MenuEntry::new("close", "Close pane").danger().separator_before()
                .disabled_reason_if(context.pane.is_none(), "No pane here"),
        ]
    }
}
```

**TypeScript**

```ts
import { definePlugin, type MenuContext, type MenuEntry } from '@ilium/plugin-sdk';

export const { menuProvider } = definePlugin({
  side: 'client',
  menuItems(context: MenuContext): MenuEntry[] {
    const frozen = context.pane !== undefined && isFrozen(context.pane);
    return [
      { id: 'freeze', label: frozen ? 'Unfreeze' : 'Freeze', icon: '❄', isSubmenu: false, separatorBefore: false, danger: false },
      { id: 'copy', label: 'Copy', isSubmenu: true, separatorBefore: false, danger: false },
      { id: 'copy-last', label: 'Last answer', parent: 'copy', isSubmenu: false, separatorBefore: false, danger: false },
      { id: 'close', label: 'Close pane', danger: true, separatorBefore: true, isSubmenu: false,
        disabledReason: context.pane === undefined ? 'No pane here' : undefined },
    ];
  },
});
```

### Menus in own views

A `menu` widget inside a popup anchored `at-pointer` is a context menu for a view; open it on the `context-requested` action (see [popovers](/pages/api-popovers/)). The widget takes the same `menu-entry` list.

### Errors and limits

| Case | Effect |
|---|---|
| Provider over 20 ms | static entries only |
| Over 64 entries | cut with `… N more` |
| Submenu depth over 2 | `manifest.menu-depth` (static) or truncated (dynamic) |
| Entry command unknown | install refused or entry disabled |

### See also

- [commands](/pages/api-commands/), [command palette](/pages/api-command-palette/), [popovers](/pages/api-popovers/), [ui](/pages/api-ui/), [Plugin exports](/pages/api-exports/).
