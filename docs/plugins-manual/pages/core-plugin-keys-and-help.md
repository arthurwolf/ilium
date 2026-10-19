---
layout: page
title: Keys and help
permalink: /pages/core-plugin-keys-and-help/
---

## Keys and help

The Keys and help plugin (id `ilium.core.keys-and-help`, side: client) is the plugin form of Ilium's keyboard and help surfaces. It draws the key reference, shows the prefix and the active preset, offers a key-capture field for remapping, shows per-option help topics, adds commands to the command palette, and lists installed plugins with a switch for each.

The keymap itself is kernel-native: the kernel matches chords, applies the tmux or Screen preset and stores remaps. The plugin reads the keymap and the Keyboard tab through the contract and never changes how a chord is dispatched.

### What it covers

- **Prefix, keybindings, presets and remapping.** Steps 6, 7, 10 and 11 of Getting started describe the prefix, the presets and remapping a key.
- **Help and key reference.** Every keybinding is listed in step 7 of Getting started, and each Settings option has its own help topic (see Per-option help in the Settings page).
- **Command palette entries.** Commands appear in the palette under a category.
- **Plugin manager.** The list of installed plugins, with a switch to turn each on or off.

### See it

<figure><img src="/assets/demos/keys-and-help-hints-frame.png" alt="The bottom status row of the Ilium client showing key hints for the focused board" loading="lazy"><figcaption>A real frame from the settings tour. The bottom row lists the hints for the focused view. The keys reference, prefix presets and palette have no recording, so those parts use renders.</figcaption></figure>

### What the user sees

<figure><pre class="term">╭─ Key reference ───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│                                                                                                                                                                                                           │
│ <span class="t-hi">Panes</span>                                                                                                                                                                                                     │
│ Split vertically ……………………………………………… Prefix  %                                                                                                                                                             │
│ Split horizontally ………………………………………… Prefix  "                                                                                                                                                             │
│ <span class="t-hi">Tree</span>                                                                                                                                                                                                      │
│ Move up …………………………………………………………………… Prefix  Up                                                                                                                                                             │
│ Rename ………………………………………………………………………… Prefix  r                                                                                                                                                             │
│                                                                                                                                                                                                           │
│ <span class="t-dim">Prefix is Ctrl+B. Preset: tmux.</span>                                                                                                                                                                           │
│                                                                                                                                                                                                           │
╰───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯</pre><figcaption>Rendered at 48 columns. Illustrative. A <code>key-hints</code> panel groups the bindings by area. Labels come from the keymap, so a remap shows here after <code>keymap-changed</code>.</figcaption></figure>
<figure><pre class="term">╭─ Keyboard ───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│                                                                                                                                                                                                                  │
│ Prefix ………………………………………………………………………………… Ctrl+B                                                                                                                                                                    │
│ Preset …………………………………………………………………………… ‹ tmux ›                                                                                                                                                                    │
│ Split vertically ……………………………………………… Prefix  %                                                                                                                                                                    │
│                                                                                                                                                                                                                  │
│ Press the new key chord.                                                                                                                                                                                         │
│                                                                                                                                                                                                                  │
│ <span class="t-hi">Ctrl+Shift+K</span>                                                                                                                                                                                                     │
│ <span class="t-warn">used by Paste in pane</span>                                                                                                                                                                                            │
│                                                                                                                                                                                                                  │
│ Esc cancel · Enter apply                                                                                                                                                                                         │
╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯</pre><figcaption>Rendered at 48 columns. Illustrative. The Keyboard tab with a key-capture field recording a chord. The warning line appears when the kernel already binds the chord.</figcaption></figure>
### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.keys-and-help",
  "name": "Keys and help",
  "version": "0.1.0",
  "publisher": "Ilium",
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": ["views"]
  },
  "capabilities": {
    "client": ["keymap.read", "settings.read-kernel:keyboard", "help.read",
               "plugins.manage", "ui.panels"]
  },
  "contributes": {
    "commands": [
      { "id": "show-keys", "title": "Show the key reference",
        "category": "Help", "keywords": ["shortcut", "binding"] },
      { "id": "show-plugins", "title": "Manage plugins", "category": "Help" }
    ],
    "keybindings": [
      { "command": "ilium.core.keys-and-help/show-keys", "chord": "F1", "mode": "prefix" }
    ],
    "panels": [
      { "id": "keys", "title": "Keys", "modal": false },
      { "id": "plugins", "title": "Plugins", "modal": false }
    ],
    "help": [{ "id": "keys-overview", "file": "help/keys-overview.md", "title": "Keys" }],
    "settings": [
      { "kind": "section", "id": "chords", "tab": "keyboard", "placement": "end",
        "title": "Plugin keys", "scope": "client",
        "rows": [
          { "type": "keybinding", "key": "show_keys", "title": "Key reference",
            "command": "show-keys", "default": "?" }
        ] }
    ]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.keys-and-help",
  "name": "Keys and help",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "The Keys and help plugin (id ilium.core.keys-and-help, side: client) is the plugin form of Ilium's keyboard and help surfaces.",
  "license": "MIT",
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": [
      "views"
    ]
  },
  "capabilities": {
    "client": [
      "keymap.read",
      "settings.read-kernel:keyboard",
      "help.read",
      "plugins.manage",
      "ui.panels",
      "settings.write-kernel:keyboard"
    ]
  },
  "activation": [
    "command:ilium.core.keys-and-help/show-keys",
    "command:ilium.core.keys-and-help/show-plugins",
    "settings:keyboard"
  ],
  "contributes": {
    "commands": [
      {
        "id": "show-keys",
        "title": "Show the key reference",
        "category": "Help",
        "keywords": [
          "shortcut",
          "binding"
        ]
      },
      {
        "id": "show-plugins",
        "title": "Manage plugins",
        "category": "Help"
      }
    ],
    "keybindings": [
      {
        "command": "ilium.core.keys-and-help/show-keys",
        "chord": "F1",
        "mode": "prefix"
      }
    ],
    "panels": [
      {
        "id": "keys",
        "title": "Keys",
        "modal": false
      },
      {
        "id": "plugins",
        "title": "Plugins",
        "modal": false
      }
    ],
    "help": [
      {
        "id": "keys-overview",
        "file": "help/keys-overview.md",
        "title": "Keys"
      }
    ],
    "settings": [
      {
        "kind": "section",
        "id": "chords",
        "tab": "keyboard",
        "placement": "end",
        "title": "Plugin keys",
        "scope": "client",
        "rows": [
          {
            "type": "keybinding",
            "key": "show_keys",
            "title": "Key reference",
            "command": "show-keys",
            "default": "?"
          }
        ]
      }
    ]
  },
  "limits_tier": "first-party"
}
```

### Action names, command ids and presets

The kernel owns the keymap table. The plugin owns the Keyboard tab and the key reference screen, so it shows exactly these 35 actions. Each action has a stable name (the key in `[keybindings]`), a global command id `kernel.<name>` that a plugin can read from `keymap.bindings`, and a default key after the prefix. [COR-011]

<figure>
<pre class="term"><span class="t-dim">╭──────────────────────────────────────────────╮</span>
<span class="t-dim">│</span> Keyboard                                     <span class="t-dim">│</span>
<span class="t-dim">│</span> Preset………………… tmux      Prefix…… Ctrl+B      <span class="t-dim">│</span>
<span class="t-dim">│</span> Navigation prefix………………………… Ctrl+B           <span class="t-dim">│</span>
<span class="t-dim">│</span>                                              <span class="t-dim">│</span>
<span class="t-dim">│</span>  new_terminal ………………………………………… c             <span class="t-dim">│</span>
<span class="t-dim">│</span>  close_pane ……………………………………………… x             <span class="t-dim">│</span>
<span class="t-dim">│</span>  rename ………………………………………………… ,                <span class="t-dim">│</span>
<span class="t-dim">│</span>  focus_pane_left ……………………………… Left           <span class="t-dim">│</span>
<span class="t-dim">│</span>  quit ……………………………………………… &                   <span class="t-dim">│</span>
<span class="t-dim">╰──────────────────────────────────────────────╯</span></pre>
<figcaption>Rendered at 48 columns. Illustrative. The Keyboard tab: the preset, the two prefixes and the first rows of the action list. The plugin draws the rows from the 35 action names; the kernel stores the choice.</figcaption>
</figure>

| Action name | Command id | Default | GNU Screen preset | tmux preset |
|---|---|---|---|---|
| `new_terminal` | `kernel.new_terminal` | `c` | `c` | `c` |
| `new_agent_worktree` | `kernel.new_agent_worktree` | `W` | `W` | `W` |
| `new_editor` | `kernel.new_editor` | `e` | `e` | `e` |
| `new_board` | `kernel.new_board` | `B` | `B` | `B` |
| `close_pane` | `kernel.close_pane` | `x` | `k` | `x` |
| `new_group` | `kernel.new_group` | `g` | `g` | `g` |
| `new_split_view` | `kernel.new_split_view` | `"` | `S` | `"` |
| `new_folder` | `kernel.new_folder` | `F` | `F` | `F` |
| `rename` | `kernel.rename` | unbound | `A` | `,` |
| `toggle_move` | `kernel.toggle_move` | `m` | `m` | `m` |
| `focus_tree` | `kernel.focus_tree` | `w` | `t` | `w` |
| `focus_pane` | `kernel.focus_pane` | `P` | `P` | `P` |
| `focus_next_pane` | `kernel.focus_next_pane` | `o` | `n` | `o` |
| `focus_previous_pane` | `kernel.focus_previous_pane` | `;` | `p` | `;` |
| `focus_pane_left` | `kernel.focus_pane_left` | `Left` | `H` | `Left` |
| `focus_pane_right` | `kernel.focus_pane_right` | `Right` | `L` | `Right` |
| `focus_pane_up` | `kernel.focus_pane_up` | `Up` | `K` | `Up` |
| `focus_pane_down` | `kernel.focus_pane_down` | `Down` | `J` | `Down` |
| `cycle_next_in_group` | `kernel.cycle_next_in_group` | `n` | `Down` | `n` |
| `cycle_previous_in_group` | `kernel.cycle_previous_in_group` | `p` | `Up` | `p` |
| `jump_next_group` | `kernel.jump_next_group` | `)` | `PgDn` | `)` |
| `jump_previous_group` | `kernel.jump_previous_group` | `(` | `PgUp` | `(` |
| `scrollback_up` | `kernel.scrollback_up` | `[` | `[` | `[` |
| `scrollback_down` | `kernel.scrollback_down` | `]` | `]` | `]` |
| `save` | `kernel.save` | `S` | `s` | `S` |
| `run_command` | `kernel.run_command` | `!` | `:` | `!` |
| `search` | `kernel.search` | `f` | `/` | `f` |
| `Help` | `kernel.Help` | `?` | `?` | `?` |
| `detach` | `kernel.detach` | `d` | `d` | `d` |
| `quit` | `kernel.quit` | `&` | `\` | `&` |
| `toggle_editor_view_mode` | `kernel.toggle_editor_view_mode` | `v` | `v` | `v` |
| `toggle_line_numbers` | `kernel.toggle_line_numbers` | `N` | `N` | `N` |
| `toggle_minimap` | `kernel.toggle_minimap` | `b` | `b` | `b` |
| `toggle_autosave` | `kernel.toggle_autosave` | `a` | `a` | `a` |
| `settings` | `kernel.settings` | `:` | `o` | `:` |

Rules that hold for all rows:

- The general prefix is `shortcut_base`, a `Ctrl` letter from `Ctrl+A` to `Ctrl+Z`, default `Ctrl+B`. The four tree actions `cycle_next_in_group`, `cycle_previous_in_group`, `jump_next_group` and `jump_previous_group` use the separate `navigation_shortcut_base`, so moving the general prefix to `Ctrl+A` keeps `Ctrl+B n` for tree traversal.
- The two presets are complete. The GNU Screen preset sets the prefix to `Ctrl+A`, and the tmux preset sets it to `Ctrl+B`. Choosing a preset replaces every key in the table. Actions that a preset does not name keep their default key.
- A key is bindable only when it is in the fixed list of printable keys, arrow keys and Page keys, and no other action uses it. The collision check runs for the picker and for preset load.
- Pressing the prefix twice passes it through to the program in the pane, which is how tmux inside Ilium keeps working.
- `rename` has no default key. It is in the table so a user can bind it.
- The plugin holds `settings.write-kernel:keyboard`. It is the only plugin that may write the `[keyboard]` and `[keybindings]` tables. Other plugins read the result with `keymap.read`. See [kernel settings](/pages/api-kernel-settings-and-keymap/#writing-a-kernel-tab).

### Kernel tabs this plugin owns

This plugin is the single owner of the following built-in Settings tabs. It holds `settings.write-kernel:<tab>` for each one (T1 only, one holder per tab, see [kernel settings](/pages/api-kernel-settings-and-keymap/#writing-a-kernel-tab)). The values stay in the same `config.toml` tables as before, so no user setting moves. [COR-030] [COR-033]

| Tab id | Label | Config table | Readers |
|---|---|---|---|
| `keyboard` | Keyboard | `[keyboard]`, `[keybindings]` | none |

If the plugin is disabled, the tab stays in the rail with its stored values shown read-only and the notice "Keys and help is disabled. Values are kept." The kernel keeps reading the keys it consumes itself (listed in the Consumer column of the key table in [config and data migration](/pages/reference-config-and-data-migration/)). Behaviour that only this plugin provides is off. The exact result for this plugin is its row in [kernel and boot](/pages/reference-kernel-and-boot/). [COR-022]

### Samples

#### 1. Key reference

A panel reads every binding with `keymap.bindings`, turns each chord into its display form with `chord_label`, and draws the pairs as `key-hints`. It rebuilds when `keymap-changed` arrives, so a remap appears at once. See the [kernel settings and keymap API](/pages/api-kernel-settings-and-keymap/), the [keymap.read grant](/pages/capability-keymap-read/), the [event catalogue](/pages/api-events/) and [text widgets](/pages/widgets-text/).

**Rust**

```rust
use ilium_plugin_sdk::host::{keymap, ui};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ui::KeyHint;
use ilium_plugin_sdk::ui::build::Tree;

impl Handler for KeysPanel {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if matches!(event, Event::KeymapChanged) {
                ui::invalidate(ViewTarget::Panel("keys".into()));
            }
        }
        Vec::new()
    }
}

impl Views for KeysPanel {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let hints: Vec<KeyHint> = keymap::bindings()
            .into_iter()
            .filter_map(|binding| {
                let chord = binding.chords.first()?;
                Some(KeyHint::new(&keymap::chord_label(chord), &binding.command))
            })
            .collect();
        let mut tree = Tree::new();
        let panel = tree.key_hints(hints).key("hints");
        tree.finish(panel)
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { bindings, chordLabel } from '@ilium/plugin-sdk/host/keymap';
import { invalidate } from '@ilium/plugin-sdk/host/ui';
import { keyHints, tree } from '@ilium/plugin-sdk/ui/build';

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  activate() {},
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'keymap-changed') invalidate({ tag: 'panel', val: 'keys' });
    }
    return [];
  },
  render() {
    const hints = bindings()
      .filter((binding) => binding.chords.length > 0)
      .map((binding) => ({ keys: chordLabel(binding.chords[0]), label: binding.command }));
    return tree(keyHints(hints, { key: 'hints' }));
  },
});
```

#### 2. Prefix and presets

The panel shows the prefix and the active preset by reading the Keyboard tab with `kernel-settings`, and refreshes on `kernel-settings-changed`. The plugin owns this tab and writes it with `settings.write-kernel:keyboard`; secrets would arrive as `<redacted>`. See the [settings.read-kernel grant](/pages/capability-settings-read-kernel/), the [kernel settings API](/pages/api-kernel-settings-and-keymap/) and the [event catalogue](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::host::kernel_settings;
use ilium_plugin_sdk::host::log::{self, Level};

// Needs "settings.read-kernel:keyboard". Values are listed generically, so the
// panel follows whatever the Keyboard tab holds.
fn keyboard_lines() -> Vec<String> {
    let Ok(text) = kernel_settings::read("keyboard") else { return Vec::new() };
    let values: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    values.as_object().map_or_else(Vec::new, |object| {
        object.iter().map(|(key, value)| format!("{key} = {value}")).collect()
    })
}

fn on_kernel_settings_changed(tab: &str, keys: &[String]) {
    if tab == "keyboard" {
        log::log(Level::Info, &format!("keyboard tab changed: {keys:?}"));
        // then invalidate the panel, as in sample 1
    }
}
```

**TypeScript**

```ts
import { read } from '@ilium/plugin-sdk/host/kernel-settings';
import { log } from '@ilium/plugin-sdk/host/log';

// Needs "settings.read-kernel:keyboard". Values are listed generically, so the
// panel follows whatever the Keyboard tab holds.
export function keyboardLines(): string[] {
  try {
    const values = JSON.parse(read('keyboard')) as Record<string, unknown>;
    return Object.entries(values).map(([key, value]) => `${key} = ${JSON.stringify(value)}`);
  } catch {
    return [];
  }
}

export function onKernelSettingsChanged(tab: string, keys: string[]): void {
  if (tab !== 'keyboard') return;
  log('info', `keyboard tab changed: ${keys.join(',')}`);
  // then invalidate the panel, as in sample 1
}
```

#### 3. Remap a key

A `keybinding` row remaps one of the plugin's own commands. For a panel, a `key-capture` field records the next chord and, with the reserved-key check, warns when the kernel already binds it. See [settings rows](/pages/reference-settings-rows/) and [input widgets](/pages/widgets-input/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ui::{ActionKind, ActionValue};
use ilium_plugin_sdk::ui::build::Tree;

// Manifest row: { "type": "keybinding", "key": "show_keys", "title": "Key reference",
//                 "command": "show-keys", "default": "?" }
impl Views for KeysPanel {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let chord = tree
            .key_capture()
            .chord(self.chord.clone())
            .reserved_check(true)
            .key("chord");
        tree.finish(chord)
    }
}

// In on_events:
// ("chord", ActionKind::Changed, ActionValue::Text(c)) => self.chord = Some(c).filter(|c| !c.is_empty()),
```

**TypeScript**

```ts
import { keyCapture, tree } from '@ilium/plugin-sdk/ui/build';

// Manifest row: { "type": "keybinding", "key": "show_keys", "title": "Key reference",
//                 "command": "show-keys", "default": "?" }
let chord: string | undefined;

export function render() {
  return tree(keyCapture({ key: 'chord', chord, reservedCheck: true }));
}

// In onEvents:
// if (key === 'chord' && value.tag === 'text') chord = value.val || undefined;
```

#### 4. Help topics

The panel lists topic ids in a `tree-list` and renders the chosen topic as `markdown`. `help.topic` returns the Markdown of any installed topic by its global id, `<plugin-id>/<local-id>`; there is no call that lists topics, so the plugin keeps its own list of ids. See the [help and plugins API](/pages/api-help-and-plugins/), the [help.read grant](/pages/capability-help-read/), [text widgets](/pages/widgets-text/) and [collection widgets](/pages/widgets-collections/). The topic ids shown are examples.

**Rust**

```rust
use ilium_plugin_sdk::host::help;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ui::TreeItem;
use ilium_plugin_sdk::ui::build::Tree;

const TOPICS: [(&str, &str); 2] = [
    ("ilium.core.keys-and-help/keys-overview", "Keys"),
    ("ilium/getting-started", "Getting started"),
];

impl Views for HelpPanel {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let items: Vec<TreeItem> = TOPICS
            .iter()
            .map(|(id, title)| TreeItem::new(id, title).depth(0))
            .collect();
        let source = help::topic(&self.chosen).unwrap_or_else(|| "No such topic.".into());
        let mut tree = Tree::new();
        let list = tree.tree_list(items).key("topics");
        let page = tree.markdown(source).key("page");
        let both = tree
            .layout(Direction::Horizontal, [Constraint::Length(24), Constraint::Fill(1)])
            .child(list)
            .child(page);
        tree.finish(both)
    }
}
// In on_events: ("topics", ActionKind::Selected, ActionValue::Text(id)) => self.chosen = id,
```

**TypeScript**

```ts
import { topic } from '@ilium/plugin-sdk/host/help';
import { fill, layout, length, markdown, tree, treeList } from '@ilium/plugin-sdk/ui/build';

const TOPICS = [
  { id: 'ilium.core.keys-and-help/keys-overview', label: 'Keys', depth: 0 },
  { id: 'ilium/getting-started', label: 'Getting started', depth: 0 },
];
let chosen = TOPICS[0].id;

export function render() {
  const source = topic(chosen) ?? 'No such topic.';
  return tree(layout(
    { direction: 'horizontal', constraints: [length(24), fill(1)] },
    [treeList(TOPICS, { key: 'topics' }), markdown(source, { key: 'page' })],
  ));
}
// In onEvents: if (key === 'topics' && kind === 'selected' && value.tag === 'text') chosen = value.val;
```

#### 5. Palette and plugin manager

Commands declared in the manifest appear in the palette under their `category`. When the user runs the manager command, the plugin reads every installed plugin with `plugins.list` and turns one on or off with `plugins.set_enabled`. See the [command palette](/pages/api-command-palette/), the [plugins.manage grant](/pages/capability-plugins-manage/) and the [event catalogue](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::host::plugins;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::CommandInvokedEvent;

// Manifest: commands with "category": "Help", as in the manifest above.
impl Handler for PluginManager {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::CommandInvoked(CommandInvokedEvent { command, .. }) = event else {
                continue;
            };
            if command == "ilium.core.keys-and-help/show-plugins" {
                self.rows = plugins::list()
                    .into_iter()
                    .map(|info| (info.id, info.name, info.enabled))
                    .collect();
            }
        }
        Vec::new()
    }
}

fn toggle(plugin_id: &str, enabled: bool) -> Result<(), PluginError> {
    plugins::set_enabled(plugin_id, enabled)
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { list, setEnabled } from '@ilium/plugin-sdk/host/plugins';
import { read as readKernel } from '@ilium/plugin-sdk/host/kernel-settings';

// Manifest: commands with "category": "Help", as in the manifest above.
let rows: Array<{ id: string; name: string; enabled: boolean }> = [];

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {},
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'command-invoked'
          && event.val.command === 'ilium.core.keys-and-help/show-plugins') {
        rows = list().map(({ id, name, enabled }) => ({ id, name, enabled }));
      }
    }
    return [];
  },
});

export function toggle(pluginId: string, enabled: boolean): void {
  setEnabled(pluginId, enabled);
}
```

#### 6. Reload hints after a kernel change

The Keyboard kernel settings tab (`keyboard`) sends `kernel-settings-changed` with the keys that changed. The plugin reloads only the hint strings for those keys, not the whole reference.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;

impl Handler for HintCache {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::KernelSettingsChanged(change) = event else { continue };
            if change.tab != "keyboard" { continue; }
            // `read` takes the tab only and returns the whole tab as JSON.
            let Ok(text) = kernel_settings::read("keyboard") else { continue };
            let Ok(values) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
            for key in &change.keys {
                if let Some(value) = values.get(key) {
                    self.hints.insert(key.clone(), value.to_string());
                }
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
export function onEvents(events: Event[]): Submission[] {
  for (const event of events) {
    if (event.tag !== 'kernel-settings-changed' || event.val.tab !== 'keyboard') continue;
    // `read` takes the tab only and returns the whole tab as JSON.
    const values = JSON.parse(readKernel('keyboard')) as Record<string, unknown>;
    for (const key of event.val.keys) {
      if (key in values) hints.set(key, JSON.stringify(values[key]));
    }
  }
  return [];
}
```

### Extension points

Other plugins change what `ilium.core.keys-and-help` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.keys-and-help/<point>`). Contributors need `extensions.contribute:ilium.core.keys-and-help/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `key-hints` | collect | Add a hint to the status bar in a context. | entry: id, context, chord, label | standard |
| `help-sources` | collect | Add a help source, such as a remote documentation index. | entry: id, title, service | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.keys-and-help/bindings`: list: current key bindings with labels

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.keys-and-help/changed`: { reason } after a remap

`contributes.help` topics and `contributes.keybindings` appear in the key reference.

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.keys-and-help"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Shows a hint while a board pane has focus

A package adds this entry in its manifest. It needs no code.

```json
{
  "api": "ilium:plugin@0.1.0",
  "capabilities": { "client": ["extensions.contribute:ilium.core.keys-and-help/key-hints"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.keys-and-help/key-hints",
        "id": "board-hints",
        "priority": 50,
        "entry": {
          "context": "pane:board",
          "chord": "n",
          "label": "New card"
        }
      }
    ]
  }
}
```

### ABI used

- [Kernel settings and keymap](/pages/api-kernel-settings-and-keymap/)
- [keymap.read](/pages/capability-keymap-read/) and [settings.read-kernel](/pages/capability-settings-read-kernel/)
- [Help and plugins](/pages/api-help-and-plugins/), [help.read](/pages/capability-help-read/) and [plugins.manage](/pages/capability-plugins-manage/)
- [Command palette](/pages/api-command-palette/)
- [Events](/pages/api-events/)
- [Text widgets](/pages/widgets-text/), [input widgets](/pages/widgets-input/) and [collection widgets](/pages/widgets-collections/)
- [Settings rows](/pages/reference-settings-rows/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Settings plugin](/pages/core-plugin-settings/)
