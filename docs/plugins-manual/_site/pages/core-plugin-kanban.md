# Kanban

## Kanban

<div class="note">
<p>Status: the board is native in v1. This page describes the plugin form that arrives in Phase 7 (see the <a href="/pages/roadmap/">roadmap</a>).</p>
</div>

The Kanban plugin has the id `ilium.core.kanban` and runs on the client side. It owns the board pane: a basic pane that draws cards in To do, Doing and Done columns. A board can be backed by a Markdown file. The plugin reloads the board when that file changes, and it adds the Kanban Board tab to the settings. Drawing a pane body and reading files are plain plugin work, so the board is a good first example of a pane kind that a plugin provides.

The pane itself, its place in the tree and its focus are kernel-native. The plugin declares a `board` pane kind and draws the body of that pane through the contract. It never touches the PTY or the layout solver.

### What it covers

- Kanban boards as a pane: columns, cards, a selected card (see Editors and boards, "Kanban boards").
- A board that is backed by a Markdown file, with automatic reload when the file changes (see Editors and boards, "Kanban boards").
- The Kanban Board settings tab: card preview and column width (see Settings, "Kanban Board").

### See it

<figure><img src="/assets/demos/08-kanban.gif" alt="A Kanban board with three columns while a card moves from To do to Doing" loading="lazy"><figcaption>A board pane with three columns. The selected card moves one column to the right.</figcaption></figure>

### What the user sees

<figure>
<pre class="term">╭ Board: plan.md ──────────────────────────────╮
│ To do (2)      Doing (1)      Done (1)       │
│                                              │
│ ╭────────────╮ ╭────────────╮ ╭────────────╮ │
│ │ Write      │ │ Port parser│ │ Fix typos  │ │
│ │ docs       │ ╰────────────╯ ╰────────────╯ │
│ ╰────────────╯                               │
│                ╭────────────╮                │
│ ╭────────────╮ │ [x] lex    │                │
│ │ Add        │ ╰────────────╯                │
│ │ tests      │                               │
│ ╰────────────╯ ╭────────────╮                │
│                │ [ ] emit   │                │
│                ╰────────────╯                │
│                                              │
╰──────────────────────────────────────────────╯</pre>
<figcaption>Rendered at 48 columns. Illustrative: the card borders depend on the icon set. The selected card is the one with a check list.</figcaption>
</figure>

<figure>
<pre class="term">╭ Open board from file ───────────────────────╮
│ Markdown file ……………… docs/plan.md [ Browse ]│
│ Column width ………………………………………………………………………… 24│
│                                             │
│                 [ Open ]   [ Cancel ]       │
╰─────────────────────────────────────────────╯</pre>
<figcaption>Rendered at 48 columns. Illustrative. The Open board from file picker with a path field and the column width setting.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.kanban",
  "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0" },
  "contributes": {
    "pane_kinds": [
      { "id": "board", "title": "Board", "icon": "layout-kanban",
        "primitive": "basic", "wants_keys": true, "persist": "metadata" }
    ],
    "settings": [
      { "kind": "tab", "id": "kanban", "title": "Kanban Board", "scope": "client",
        "rows": [
          { "type": "toggle", "key": "card_preview", "title": "Card preview",
            "default": true },
          { "type": "number", "key": "column_width", "title": "Column width",
            "minimum": 12, "maximum": 60, "step": 1, "unit": "cells", "default": 24 }
        ] }
    ]
  },
  "capabilities": {
    "client": [
      "panes.provide-kind", "fs.read:project", "fs.write:project",
      "fs.watch", "fs.pick", "fs.read:picked", "editor.read"
    ]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.kanban",
  "name": "Kanban",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "The Kanban plugin has the id ilium.core.kanban and runs on the client side.",
  "license": "MIT",
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0"
  },
  "capabilities": {
    "client": [
      "panes.provide-kind",
      "fs.read:project",
      "fs.write:project",
      "fs.watch",
      "fs.pick",
      "fs.read:picked",
      "editor.read",
      "settings.write-kernel:kanban-board"
    ]
  },
  "activation": [
    "pane-kind:kanban"
  ],
  "contributes": {
    "pane_kinds": [
      {
        "id": "board",
        "title": "Board",
        "icon": "layout-kanban",
        "primitive": "basic",
        "wants_keys": true,
        "persist": "metadata"
      }
    ],
    "settings": [
      {
        "kind": "tab",
        "id": "kanban",
        "title": "Kanban Board",
        "scope": "client",
        "rows": [
          {
            "type": "toggle",
            "key": "card_preview",
            "title": "Card preview",
            "default": true
          },
          {
            "type": "number",
            "key": "column_width",
            "title": "Column width",
            "minimum": 12,
            "maximum": 60,
            "step": 1,
            "unit": "cells",
            "default": 24
          }
        ]
      }
    ]
  },
  "limits_tier": "first-party"
}
```

### Kernel tabs this plugin owns

This plugin is the single owner of the following built-in Settings tabs. It holds `settings.write-kernel:<tab>` for each one (T1 only, one holder per tab, see [kernel settings](/pages/api-kernel-settings-and-keymap/#writing-a-kernel-tab)). The values stay in the same `config.toml` tables as before, so no user setting moves. [COR-030] [COR-033]

| Tab id | Label | Config table | Readers |
|---|---|---|---|
| `kanban-board` | Kanban Board | `[kanban_board]` | none |

If the plugin is disabled, the tab stays in the rail with its stored values shown read-only and the notice "Kanban is disabled. Values are kept." The kernel keeps reading the keys it consumes itself (listed in the Consumer column of the key table in [config and data migration](/pages/reference-config-and-data-migration/)). Behaviour that only this plugin provides is off. The exact result for this plugin is its row in [kernel and boot](/pages/reference-kernel-and-boot/). [COR-022]

### Samples

#### A board pane kind

The plugin declares a `board` pane kind with the [basic primitive](/pages/primitives-basic/) and the grant in [panes.provide-kind](/pages/capability-panes-provide-kind/). It draws each column as a `block` inside a horizontal `layout` (see [layout and containers](/pages/widgets-layout-and-containers/) and the [UI model](/pages/ui-model/)). The selected card stays in view state, so a redraw keeps it.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::layout::{Area, Constraint, Direction};
use ilium_plugin_sdk::types::ui::{UiTree, ViewTarget};
use ilium_plugin_sdk::ui::build::Tree;

const COLUMNS: [&str; 3] = ["To do", "Doing", "Done"];

#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
struct Kanban {
    cards: Vec<Vec<String>>, // one list of card titles per column
    selected: (usize, usize), // column, card
}

impl Lifecycle for Kanban {}

impl Handler for Kanban {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

impl Views for Kanban {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let mut blocks = Vec::new();
        for (index, name) in COLUMNS.iter().enumerate() {
            let titles = self.cards.get(index).cloned().unwrap_or_default();
            let lines: Vec<Vec<Span>> = titles
                .iter()
                .enumerate()
                .map(|(row, title)| {
                    let marker = if self.selected == (index, row) { "> " } else { "  " };
                    vec![Span::plain(&format!("{marker}{title}"))]
                })
                .collect();
            let body = tree.paragraph(lines);
            blocks.push(tree.block().title(*name).child(body));
        }
        let mut row = tree.layout(Direction::Horizontal, vec![Constraint::Fill(1); 3]);
        for block in blocks {
            row = row.child(block);
        }
        tree.finish(row)
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { block, fill, layout, paragraph, span, tree } from '@ilium/plugin-sdk/ui/build';

const COLUMNS = ['To do', 'Doing', 'Done'];
const state = { cards: [[], [], []] as string[][], selected: [0, 0] as [number, number] };

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  onEvents() {
    return [];
  },
  render() {
    const blocks = COLUMNS.map((name, column) => {
      const lines = state.cards[column].map((title, row) => {
        const marker = state.selected[0] === column && state.selected[1] === row ? '> ' : '  ';
        return [span(`${marker}${title}`)];
      });
      return block({ title: name }, [paragraph(lines)]);
    });
    return tree(layout({ direction: 'horizontal', constraints: COLUMNS.map(() => fill(1)) }, blocks));
  },
});
```

#### Cards with check lists

Each card shows its sub-tasks as a `check-list` from [collections](/pages/widgets-collections/), placed in a `block` from [layout and containers](/pages/widgets-layout-and-containers/). A click on an item arrives as a `ui-action` event ([events](/pages/api-events/)) with the kind `changed` and an `item-state` value. The plugin toggles the item and marks the board dirty so it is saved.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ui::{ActionKind, ActionValue};
use ilium_plugin_sdk::types::ui::CheckItem;

fn card_body(tree: &mut Tree, card: &Card) -> NodeRef {
    let items: Vec<CheckItem> = card
        .tasks
        .iter()
        .map(|task| CheckItem::new(&task.id, &task.label).state(task.state))
        .collect();
    let list = tree.check_list(items).key(&format!("card-{}", card.id));
    tree.block().title(&card.title).child(list).into_node()
}

impl Handler for Kanban {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::UiAction(action) = event else { continue };
            let Some(card_id) = action.key.strip_prefix("card-") else { continue };
            if let (ActionKind::Changed, ActionValue::ItemState(task_id, state)) =
                (action.kind, action.value)
            {
                self.set_task_state(card_id, &task_id, state);
                self.is_dirty = true;
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { block, checkList } from '@ilium/plugin-sdk/ui/build';
import type { Event, Submission } from '@ilium/plugin-sdk/types';

function cardBody(card: Card) {
  const items = card.tasks.map((task) => ({ id: task.id, label: task.label, state: task.state }));
  return block({ title: card.title }, [checkList(items, { key: `card-${card.id}` })]);
}

function onEvents(batch: Event[]): Submission[] {
  for (const event of batch) {
    if (event.tag !== 'ui-action' || !event.val.key.startsWith('card-')) continue;
    const { kind, value, key } = event.val;
    if (kind === 'changed' && value.tag === 'item-state') {
      setTaskState(key.slice('card-'.length), value.val[0], value.val[1]);
      state.isDirty = true;
    }
  }
  return [];
}
```

#### Read and write the Markdown file safely

The board file is written only if nobody changed it meanwhile. The plugin reads with a revision token and writes with the conditional write from [atomic writes and tail](/pages/api-fs-atomic-and-tail/). The grant is [fs.write](/pages/capability-fs-write/), and the scopes are explained on [file access](/pages/file-access/). On a conflict, the plugin reloads and applies the move again.

**Rust**

```rust
use ilium_plugin_sdk::host::fs;
use ilium_plugin_sdk::prelude::*;

const MAX_ATTEMPTS: u32 = 3;

async fn move_card_in_file(path: &FsPath, card: &str, column: usize) -> Result<(), PluginError> {
    for _attempt in 0..MAX_ATTEMPTS {
        let current = fs::read_with_revision(path).await?;
        let updated = apply_move(&current.bytes, card, column)?;
        match fs::write_atomic(path, updated, Some(current.revision)).await {
            Ok(_) => return Ok(()),
            Err(error) if error.code == ErrorCode::Conflict => continue,
            Err(error) => return Err(error),
        }
    }
    Err(PluginError {
        code: ErrorCode::Conflict,
        message: "the board changed three times; try again".to_string(),
        retry_after_ms: None,
    })
}
```

**TypeScript**

```ts
import { PluginError } from '@ilium/plugin-sdk';
import { readWithRevision, writeAtomic } from '@ilium/plugin-sdk/host/fs';

export async function moveCardInFile(
  path: { scope: string; path: string },
  card: string,
  column: number,
): Promise<void> {
  for (let attempt = 0; attempt < 3; attempt++) {
    const current = await readWithRevision(path);
    const updated = applyMove(current.bytes, card, column);
    try {
      await writeAtomic(path, updated, current.revision);
      return;
    } catch (error) {
      if (error instanceof PluginError && error.code === 'conflict') continue;
      throw error;
    }
  }
  throw new PluginError('conflict', 'the board changed three times; try again');
}
```

#### Reload when the file changes

Edits made in the editor or by an agent show up on the board. The plugin watches the file with [fs.watch](/pages/capability-fs-watch/) (see also [file access](/pages/file-access/)). On `fs-changed`, or on `editor-saved` for the same path, it parses the file again and asks the host to redraw. Both events are listed in [events](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::host::{fs, ui};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::{EditorSavedEvent, FsChangedEvent};
use ilium_plugin_sdk::types::ui::ViewTarget;

impl Lifecycle for Kanban {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        let path = FsPath { scope: "project".into(), path: "docs/plan.md".into() };
        self.watch = Some(fs::watch(&path, false)?);
        Ok(())
    }
}

impl Handler for Kanban {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let is_board_changed = match event {
                Event::FsChanged(FsChangedEvent { watch, .. }) => self.watch == Some(watch),
                Event::EditorSaved(EditorSavedEvent { path, .. }) => path.ends_with("docs/plan.md"),
                _ => false,
            };
            if is_board_changed {
                self.reload_from_disk();
                let _ = ui::invalidate(ViewTarget::Pane(self.pane));
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { watch } from '@ilium/plugin-sdk/host/fs';
import { invalidate } from '@ilium/plugin-sdk/host/ui';

const BOARD = { scope: 'project', path: 'docs/plan.md' };
let boardWatch: bigint | undefined;

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  activate() {
    boardWatch = watch(BOARD, false);
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      const isChanged =
        (event.tag === 'fs-changed' && event.val.watch === boardWatch) ||
        (event.tag === 'editor-saved' && event.val.path.endsWith('docs/plan.md'));
      if (isChanged) {
        reloadFromDisk();
        invalidate({ tag: 'pane', val: state.pane });
      }
    }
    return [];
  },
  render,
});
```

#### Open a board from a file

The user picks a Markdown file with the picker from [fs.pick](/pages/capability-fs-pick/). A `path-field` from [pickers](/pages/widgets-pickers/) starts it. The column width comes from the `column_width` `number` row, declared as described on [settings extension](/pages/settings-extension/) and [settings row types](/pages/reference-settings-rows/).

**Rust**

```rust
use ilium_plugin_sdk::host::{fs, settings};
use ilium_plugin_sdk::host::fs::{PickKind, PickSpec};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ui::PathKind;

fn open_field(tree: &mut Tree, shown: &str) -> NodeRef {
    tree.path_field(PathKind::OpenFile)
        .key("board-file")
        .display(shown)
        .extensions(vec!["md"])
        .into_node()
}

async fn pick_board() -> Result<Option<FsPath>, PluginError> {
    let spec = PickSpec {
        kind: PickKind::OpenFile,
        title: "Open board from file".to_string(),
        extensions: vec!["md".to_string()],
    };
    match fs::pick(spec).await {
        Ok(path) => Ok(Some(path)),
        Err(error) if error.code == ErrorCode::Cancelled => Ok(None),
        Err(error) => Err(error),
    }
}

fn column_width() -> u16 {
    let values: serde_json::Value =
        serde_json::from_str(&settings::get_json()).unwrap_or(serde_json::Value::Null);
    values["column_width"].as_u64().unwrap_or(24) as u16
}
```

**TypeScript**

```ts
import { PluginError } from '@ilium/plugin-sdk';
import { pick } from '@ilium/plugin-sdk/host/fs';
import { getJson } from '@ilium/plugin-sdk/host/settings';
import { pathField } from '@ilium/plugin-sdk/ui/build';

export const openField = (shown: string) =>
  pathField('open-file', { key: 'board-file', display: shown, extensions: ['md'] });

export async function pickBoard() {
  try {
    return await pick({ kind: 'open-file', title: 'Open board from file', extensions: ['md'] });
  } catch (error) {
    if (error instanceof PluginError && error.code === 'cancelled') return undefined;
    throw error;
  }
}

export function columnWidth(): number {
  const width = (JSON.parse(getJson()) as Record<string, unknown>)['column_width'];
  return typeof width === 'number' ? width : 24;
}
```

#### Drop marker during a drag

While a card is dragged over a column, the board sends `drag-over` with the target and the acceptance rule. The plugin shows a marker before or after the card that the drop will use, or nothing when the drop is refused.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ui::{ActionKind, ActionValue, DropAccept};

impl Handler for DropMarker {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::UiAction(action) = event else { continue };
            if let (ActionKind::DragOver, ActionValue::DragOver(over)) = (action.kind, action.value) {
                self.marker = match over.accept {
                    DropAccept::None => None,
                    accept => Some((over.target_key, over.index, accept)),
                };
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
export function onAction(event: Extract<Event, { tag: 'ui-action' }>): Submission[] {
  const { kind, value } = event.val;
  if (kind !== 'drag-over' || value.tag !== 'drag-over') return [];
  const { targetKey, index, accept } = value.val;
  marker = accept === 'none' ? undefined : { targetKey, index, accept };
  return [];
}
```

### Extension points

Other plugins change what `ilium.core.kanban` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.kanban/<point>`). Contributors need `extensions.contribute:ilium.core.kanban/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `card-actions` | collect | Add an action to the card context menu. | entry: id, title, command | standard |
| `card-moved` | hook (observe) | Learn that a card changed column. | board, card, from_column, to_column | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.kanban/boards`: list, get, move-card, add-card: read and edit boards

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.kanban/card-moved`: { board, card, to_column }

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.kanban"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replace)).

#### Example: Posts a toast when a card reaches the `Done` column

A package adds a `observe` handler. Every handler runs; replies are ignored.

```json
{
  "api": "ilium:plugin@0.1.0",
  "exports": [
    "hook-handler"
  ],
  "capabilities": { "client": ["extensions.contribute:ilium.core.kanban/card-moved"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.kanban/card-moved",
        "id": "notify-done",
        "priority": 50,
        "handler": "notify_done"
      }
    ]
  }
}
```

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;

impl HookHandler for Plugin {
    fn handle_hook(&mut self, call: HookCall) -> HookReply {
        match call.extension.as_str() {
            "notify_done" => {
                // call.payload_json holds: board, card, from_column, to_column
                let _payload: serde_json::Value = match serde_json::from_str(&call.payload_json) {
                    Ok(value) => value,
                    Err(_) => return HookReply::Pass,
                };
                HookReply::Pass
            }
            _ => HookReply::Pass,
        }
    }
}
```

**TypeScript**

```ts
definePlugin({
  handleHook(call) {
    if (call.extension !== 'notify_done') return { tag: 'pass' };
    // call.payloadJson holds: board, card, from_column, to_column
    const payload = JSON.parse(call.payloadJson);
    void payload;
    return { tag: 'pass' };
  },
});
```

A handler that is late, traps or returns a reply the mode does not allow counts as `pass`. See [Extension points](/pages/api-extension-points/#hooks).

### ABI used

- [Basic primitive](/pages/primitives-basic/) and [panes.provide-kind](/pages/capability-panes-provide-kind/)
- [UI model](/pages/ui-model/), [layout and containers](/pages/widgets-layout-and-containers/), [collections](/pages/widgets-collections/) and [pickers](/pages/widgets-pickers/)
- [Events](/pages/api-events/)
- [Atomic writes and tail](/pages/api-fs-atomic-and-tail/), [fs.write](/pages/capability-fs-write/), [fs.watch](/pages/capability-fs-watch/), [fs.pick](/pages/capability-fs-pick/) and [file access](/pages/file-access/)
- [Settings extension](/pages/settings-extension/) and [settings row types](/pages/reference-settings-rows/)

### See also

- [Editor](/pages/core-plugin-editor/), which edits the board file as text.
- [Agent detection](/pages/core-plugin-agent-detect/), for the agents that tick the tasks off.
