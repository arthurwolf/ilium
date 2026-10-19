---
layout: page
title: "6. Notes pane with drag and drop"
permalink: /pages/tutorial-06-notes-pane/
---

## 6. Notes pane with drag and drop

This step adds a pane kind to the plugin. A notes pane is a basic pane: the kernel draws its frame,
title and footer line, and the plugin draws the body with `render`. Each note is one row. A row can be
dragged onto another row to change the order, and a text input at the bottom adds notes. The notes are
kept in node metadata, so they survive a restart. A command opens a new notes pane. While a note is dragged, the host works out which side of a row accepts the drop and draws the insertion line.

### You will need

- [Tutorial step 5](/pages/tutorial-05-todo-scanner/) finished.
- Rust for the core, client and server crates, or TypeScript for the client module.
- `serde` and `serde_json` in the core and client crates. The core crate has no host calls.

Grants added in this step. Grants are per side.

| Side | Grant | Used for |
|---|---|---|
| Client | `panes.provide-kind` | the `notes` pane kind and the `basic` imports |
| Client | `tree.metadata` | `tree::metadata` and `tree::set_metadata` |
| Server | `panes.create` | the `create-pane` intent sent by the command |

The server runs the command, so the client does not need `panes.create`.

<div class="note">
<p>Node metadata is one byte map per plugin per node. The server holds and replicates it, and both sides
read and write it with <code>tree.metadata</code>. This step writes it from the client only.</p>
</div>

### What changed from step 5

- `core/src/notes.rs` is new. It holds the note list and its rules.
- `client/src/notes.rs` is new. It renders the pane and applies drops and new notes.
- `client/src/lib.rs` routes `ui-action` events and the pane view to `notes.rs`.
- `server/src/lib.rs` gets one arm, which creates the pane.
- `manifest.json` adds the pane kind, the command, the keybinding and the grants above.

### How a drag reaches the plugin

A drag ends with one `ui-action` event of kind `dropped`. Its value is one `drop` value that carries
the source key, the target key and the insertion index. The plugin moves the note, saves the list and
invalidates the pane. The invalidate is needed because `set-metadata` does not change the node's
revision and sends no `tree-changed` event.

<figure>
<svg viewBox="0 0 400 214" role="img" aria-label="A drop becomes one action that the plugin saves">
  <title>Drag and drop reorders one note</title>
  <desc>The user drags a note row onto the drop target of another row. The host sends one dropped
  ui-action. The plugin reorders its list, calls set-metadata and invalidates the pane.</desc>
  <defs>
    <marker id="flow-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6"
            orient="auto">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="10" width="180" height="34" rx="4" fill="none" stroke="currentColor"/>
  <text x="100" y="31" font-size="11" text-anchor="middle" fill="currentColor">Row note-2 (draggable)</text>
  <rect x="210" y="10" width="180" height="34" rx="4" fill="none" stroke="currentColor"/>
  <text x="300" y="31" font-size="11" text-anchor="middle" fill="currentColor">Row note-1 (drop-target)</text>
  <line x1="190" y1="27" x2="210" y2="27" stroke="currentColor" marker-end="url(#flow-arrow)"/>
  <line x1="300" y1="44" x2="300" y2="70" stroke="currentColor" marker-end="url(#flow-arrow)"/>
  <text x="290" y="61" font-size="10" text-anchor="end" fill="currentColor">one drop action</text>
  <rect x="210" y="70" width="180" height="34" rx="4" fill="none" stroke="currentColor" stroke-dasharray="5 3"/>
  <text x="300" y="91" font-size="11" text-anchor="middle" fill="currentColor">ui-action: dropped</text>
  <line x1="300" y1="104" x2="300" y2="120" stroke="currentColor" marker-end="url(#flow-arrow)"/>
  <rect x="210" y="120" width="180" height="34" rx="4" fill="none" stroke="currentColor" stroke-dasharray="5 3"/>
  <text x="300" y="141" font-size="11" text-anchor="middle" fill="currentColor">reorder, set-metadata</text>
  <line x1="300" y1="154" x2="300" y2="170" stroke="currentColor" marker-end="url(#flow-arrow)"/>
  <rect x="210" y="170" width="180" height="34" rx="4" fill="none" stroke="currentColor" stroke-dasharray="5 3"/>
  <text x="300" y="191" font-size="11" text-anchor="middle" fill="currentColor">pane redraws (invalidate)</text>
  <text x="10" y="130" font-size="10" fill="currentColor">The host owns the drag.</text>
  <text x="10" y="146" font-size="10" fill="currentColor">The plugin owns the order.</text>
</svg>
<figcaption>A drag produces one dropped action. The plugin decides what the drop means.</figcaption>
</figure>

### Manifest

Merge these entries into the step 5 file. The pane kind id is `notes`, and its global id,
`example.agent-tools.notes`, is the content kind that `pane-info` reports. `wants_keys` stays false,
so the pane gets no raw key events. The text input sends its own `ui-action` events.

```json
{
  "capabilities": {
    "server": ["agent.read", "agent.control", "tree.read", "tree.decorate", "notify.desktop",
               "sound.play", "fs.read:project", "jobs.run", "panes.create"],
    "client": ["agent.read", "tree.read", "tree.metadata", "ui.panels", "editor.edit",
               "topics.subscribe:example.agent-tools/*", "panes.provide-kind"]
  },
  "contributes": {
    "commands": [{ "id": "new-notes", "title": "New notes pane" }],
    "keybindings": [
      { "command": "example.agent-tools/new-notes", "chord": "n", "mode": "prefix" }
    ],
    "pane_kinds": [
      { "id": "notes", "title": "Notes", "icon": "notebook", "primitive": "basic",
        "wants_keys": false, "persist": "metadata" }
    ]
  }
}
```

`persist: "metadata"` keeps the pane node across a restart, together with its metadata.

### The surface this step uses

| Name | Rust | TypeScript | Meaning |
|---|---|---|---|
| `metadata` | `tree::metadata(node)` | `treeHost.metadata(node)` | The bytes, or none or `undefined` when absent or the grant is declined |
| `set-metadata` | `tree::set_metadata(node, bytes)` | `treeHost.setMetadata(node, bytes)` | Writes the map. `quota-exceeded` above 64 KiB |
| `invalidate` | `ui::invalidate(ViewTarget::Pane(node))` | `invalidate({ tag: 'pane', val: node })` | Asks for a new `render` |
| `set-status` | `basic::set_status(node, spans)` | `basic.setStatus(node, spans)` | Writes the one-line footer |
| `ui-action` | `Event::UiAction(UiAction)` | `{ tag: 'ui-action', val }` | Fields `view`, `key`, `kind`, `value` |
| `dropped` | `ActionKind::Dropped`, `ActionValue::Drop(source, target, index)` | `'dropped'`, `{ tag: 'drop', val: [source, target, index] }` | One drop |
| `submitted` | `ActionKind::Submitted`, `ActionValue::Text(text)` | `'submitted'`, `{ tag: 'text', val }` | Enter in the text input |
| `create-pane` | `Intent::CreatePane(CreatePaneSpec)` | intent `create-pane` | `parent: None` places the pane like a user pane |
| Row flags | `NodeFlags::DRAGGABLE`, `NodeFlags::DROP_TARGET` | `{ draggable: true, dropTarget: true }` | A drag source and a drop target |

### The pure note list

A note is a key and a text. The key is unique within the list and names the note in a drop.
`move_note` takes the note out and inserts it at `index`, counted after the removal. An index past the
end appends. An unknown key changes nothing and returns `false`, so nothing is saved. `add_note` joins
the words with single spaces, ignores blank text and gives the note the key `note-<n>`, where `n` is one
more than the count. No step removes notes, so that key is never in use.

**Rust**

```rust
// core/src/notes.rs. Note list and its rules. No host calls.
use serde::{Deserialize, Serialize};

/// The global pane kind id, shared by the manifest and the server's create-pane intent.
pub const PANE_KIND: &str = "example.agent-tools.notes";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub key: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteList {
    pub notes: Vec<Note>,
}

/// Moves the note with `source_key` to `index`, counted after the removal.
/// Returns false when the key is unknown, so the caller saves nothing.
pub fn move_note(list: &mut NoteList, source_key: &str, index: usize) -> bool {
    let Some(position) = list.notes.iter().position(|note| note.key == source_key) else {
        return false;
    };
    let note = list.notes.remove(position);
    let target = index.min(list.notes.len());
    list.notes.insert(target, note);
    true
}

/// Appends a note. Blank text adds nothing and returns false.
pub fn add_note(list: &mut NoteList, text: &str) -> bool {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return false;
    }
    // Notes are never removed in this step, so count + 1 is never a key in use.
    let key = format!("note-{}", list.notes.len() + 1);
    list.notes.push(Note { key, text });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> NoteList {
        let mut list = NoteList::default();
        for text in ["note a", "note b", "note c"] {
            add_note(&mut list, text);
        }
        list
    }

    #[test]
    fn moves_a_note_to_the_front() {
        let mut list = sample();
        assert!(move_note(&mut list, "note-3", 0));
        let keys: Vec<&str> = list.notes.iter().map(|note| note.key.as_str()).collect();
        assert_eq!(keys, ["note-3", "note-1", "note-2"]);
    }

    #[test]
    fn unknown_key_and_blank_text_change_nothing() {
        let mut list = sample();
        assert!(!move_note(&mut list, "zz", 0));
        assert!(!add_note(&mut list, "   "));
        assert_eq!(list, sample());
    }
}
```

### Client: the pane view, drops and new notes (Rust)

The client loads the list from metadata on every render. A drop or a submitted text loads the list,
changes it, saves it and invalidates the pane. An event handler returns submissions, not errors, so a
failed save shows `Not saved` in the footer instead.

**Rust**

```rust
// client/src/notes.rs. Notes pane: render, drop and add.
use agent_tools_core::notes::{add_note, move_note, NoteList};
use ilium_plugin_sdk::host::{tree, ui};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::basic;
use ilium_plugin_sdk::types::layout::{Constraint, Direction};
use ilium_plugin_sdk::types::text::ColorRole;
use ilium_plugin_sdk::types::ui::{ActionKind, ActionValue, UiAction, UiTree, ViewTarget};
use ilium_plugin_sdk::ui::build::{NodeFlags, Tree};

pub const NEW_NOTE_KEY: &str = "new-note";

/// A missing or damaged value is an empty list, so the pane still opens.
pub fn load(node: NodeId) -> NoteList {
    tree::metadata(node)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save(node: NodeId, list: &NoteList) -> Result<(), PluginError> {
    let bytes = serde_json::to_vec(list).map_err(|error| PluginError {
        code: ErrorCode::InvalidArgument,
        message: error.to_string(),
        retry_after_ms: None,
    })?;
    tree::set_metadata(node, bytes)
}

/// Routes one ui-action. Other views and kinds this pane does not use are ignored.
pub fn on_action(action: &UiAction) {
    let ViewTarget::Pane(node) = &action.view else {
        return;
    };
    let node = *node;
    let result = match (&action.kind, &action.value) {
        (ActionKind::Dropped, ActionValue::Drop(source, _target, index)) => {
            on_dropped(node, source, *index)
        }
        (ActionKind::Submitted, ActionValue::Text(text)) if action.key == NEW_NOTE_KEY => {
            on_added(node, text)
        }
        _ => return,
    };
    match result {
        Ok(()) => ui::invalidate(ViewTarget::Pane(node)),
        Err(_) => {
            let status = Span::styled("Not saved", Style::role(ColorRole::Danger));
            let _ = basic::set_status(node, vec![status]);
        }
    }
}

fn on_dropped(node: NodeId, source_key: &str, index: u32) -> Result<(), PluginError> {
    let mut list = load(node);
    if move_note(&mut list, source_key, index as usize) {
        save(node, &list)?;
    }
    Ok(())
}

fn on_added(node: NodeId, text: &str) -> Result<(), PluginError> {
    let mut list = load(node);
    if add_note(&mut list, text) {
        save(node, &list)?;
    }
    Ok(())
}

/// One draggable row per note, then the input for new notes.
pub fn render_pane(node: NodeId) -> UiTree {
    let list = load(node);
    let mut builder = Tree::new();
    let rows: Vec<_> = list
        .notes
        .iter()
        .map(|note| {
            builder
                .paragraph([note.text.as_str()])
                .key(note.key.as_str())
                .flags(NodeFlags::DRAGGABLE | NodeFlags::DROP_TARGET)
        })
        .collect();
    let input = builder.text_input("New note").key(NEW_NOTE_KEY);
    let mut constraints: Vec<Constraint> = rows.iter().map(|_| Constraint::Min(1)).collect();
    constraints.push(Constraint::Length(3));
    let mut root = builder.layout(Direction::Vertical).constraints(constraints);
    for row in rows {
        root = root.child(row);
    }
    builder.finish(root.child(input))
}
```

The handler arm and the views export route to the module:

**Rust**

```rust
// client/src/lib.rs. New arm of the handler body, and a new arm of the views export.
Event::UiAction(action) => notes::on_action(&action),

// views export: a pane of this kind is drawn by the notes module.
ViewTarget::Pane(node) => notes::render_pane(node),
```

### Client: the same module (TypeScript)

The TypeScript client stores the same JSON, so a list written by one client reads in the other. The pure note list of `core/src/notes.rs` is mirrored in the same file (`moveNote`, `addNote`), because a TypeScript plugin has no Rust core crate.

**TypeScript**

```ts
// client/src/notes.ts. Notes pane for the TypeScript client.
import { definePlugin } from '@ilium/plugin-sdk';
import * as basic from '@ilium/plugin-sdk/primitives/basic';
import * as treeHost from '@ilium/plugin-sdk/host/tree';
import { invalidate } from '@ilium/plugin-sdk/host/ui';
import { layout, paragraph, span, textInput, tree } from '@ilium/plugin-sdk/ui/build';
import { role, type Event, type ViewTarget } from '@ilium/plugin-sdk/types';

const NEW_NOTE_KEY = 'new-note';

interface Note { key: string; text: string }
interface NoteList { notes: Note[] }

function isNote(value: unknown): value is Note {
  return typeof value === 'object' && value !== null
    && typeof (value as Note).key === 'string' && typeof (value as Note).text === 'string';
}

function loadNotes(node: bigint): NoteList {
  const bytes = treeHost.metadata(node);
  if (bytes === undefined) return { notes: [] };
  try {
    const notes: unknown = (JSON.parse(new TextDecoder().decode(bytes)) as NoteList).notes;
    return Array.isArray(notes) && notes.every(isNote) ? { notes } : { notes: [] };
  } catch {
    return { notes: [] };
  }
}

function moveNote(list: NoteList, sourceKey: string, index: number): boolean {
  const position = list.notes.findIndex((note) => note.key === sourceKey);
  if (position === -1) return false;
  const [note] = list.notes.splice(position, 1);
  list.notes.splice(Math.min(index, list.notes.length), 0, note);
  return true;
}

function addNote(list: NoteList, rawText: string): boolean {
  const text = rawText.split(/\s+/).filter((word) => word !== '').join(' ');
  if (text === '') return false;
  list.notes.push({ key: `note-${list.notes.length + 1}`, text });
  return true;
}

/** Loads, changes, saves and redraws. A refused save (quota-exceeded) shows in the footer. */
function applyEdit(node: bigint, change: (list: NoteList) => boolean): void {
  const list = loadNotes(node);
  if (!change(list)) return;
  try {
    treeHost.setMetadata(node, new TextEncoder().encode(JSON.stringify(list)));
  } catch {
    basic.setStatus(node, [span('Not saved', { fg: role('danger') })]);
    return;
  }
  invalidate({ tag: 'pane', val: node });
}

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]) {
    for (const event of batch) {
      if (event.tag !== 'ui-action' || event.val.view.tag !== 'pane') continue;
      const node = event.val.view.val;
      const { key, kind, value } = event.val;
      if (kind === 'dropped' && value.tag === 'drop') {
        const [sourceKey, , index] = value.val;
        applyEdit(node, (list) => moveNote(list, sourceKey, index));
      } else if (kind === 'submitted' && key === NEW_NOTE_KEY && value.tag === 'text') {
        applyEdit(node, (list) => addNote(list, value.val));
      }
    }
    return [];
  },
  render(view: ViewTarget) {
    if (view.tag !== 'pane') return tree(paragraph(['This view is not a notes pane.']));
    const rows = loadNotes(view.val).notes.map((note) => paragraph([note.text], {
      key: note.key,
      flags: { draggable: true, dropTarget: true },
    }));
    return tree(layout({
      direction: 'vertical',
      constraints: [...rows.map(() => ({ tag: 'min', val: 1 })), { tag: 'length', val: 3 }],
    }, [...rows, textInput('New note', { key: NEW_NOTE_KEY })]));
  },
});
```

### Server: the command that creates the pane

The command arrives on the server as `command-invoked`. The intent names the content kind, which is
the pane kind id from the core crate. `parent: None` places the pane like one the user created.
`Submission::new` sends no dedup key, so a repeated command opens a second pane.

**Rust**

```rust
// server/src/lib.rs. New arm of the handler body.
const NEW_NOTES_COMMAND: &str = "example.agent-tools/new-notes";

Event::CommandInvoked(invocation) if invocation.command == NEW_NOTES_COMMAND => {
    submissions.push(Submission::new(Intent::CreatePane(CreatePaneSpec {
        parent: None,
        title: "Notes".to_string(),
        content_kind: agent_tools_core::notes::PANE_KIND.to_string(),
        cwd: None,
        command: None,
        initial_input: None,
    })));
}
```

**TypeScript**

```ts
// server/src/server.ts. New branch of onEvents.
const NEW_NOTES_COMMAND = 'example.agent-tools/new-notes';
const NOTES_PANE_KIND = 'example.agent-tools.notes';

for (const event of batch) {
  if (event.tag === 'command-invoked' && event.val.command === NEW_NOTES_COMMAND) {
    submissions.push({
      intent: {
        tag: 'create-pane',
        val: {
          parent: undefined,
          title: 'Notes',
          contentKind: NOTES_PANE_KIND,
          cwd: undefined,
          command: undefined,
          initialInput: undefined,
        },
      },
      correlation: 1n, // chosen by the plugin, unique per plugin, below 2^63
      dedupKey: undefined,
    });
  }
}
```

### Drag feedback

The host owns the drag and decides the drop position: it computes `accept` from the pointer row ([the UI model](/pages/ui-model/)), and the plugin has no prop that overrides it. While a drag hovers a row with `DROP_TARGET`, the host sends one `ui-action` of kind `drag-over` per change of target. Its value is a `drag-over-value` with `source-key`, `target-key`, `index` and `accept`. The plugin reads `accept` (`before`, `after`, `inside` or `none`) to preview the move, for example in a status line. The host draws the insertion line (`before`, `after`) or the inside highlight, and scrolls the list near its edges. The event is coalesced per target.

| Name | Rust | TypeScript | Meaning |
|---|---|---|---|
| `drag-over` | `ActionKind::DragOver`, `ActionValue::DragOver(DragOverValue)` | `'drag-over'`, `{ tag: 'drag-over', val }` | A dragged row is held over a target |
| `drop-accept` | `DropAccept::{None, Before, After, Inside}` | `'none'`, `'before'`, `'after'`, `'inside'` | The host's computed drop position for that target |
| `pointer-events` | `NodeFlags::POINTER_EVENTS` | `{ pointerEvents: true }` | Opt-in for `pointer` actions with sub-cell positions. This step does not need it. |

The rule for the answer is pure, so it goes in the core crate and is tested like the rest. A row dragged downward lands after the target, a row dragged upward lands before it, and the row itself accepts nothing.

**Rust**

```rust
// core/src/notes.rs. Which side of a target accepts the drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropSide { None, Before, After }

pub fn drop_side(list: &NoteList, source_key: &str, target_key: &str) -> DropSide {
    let position = |key: &str| list.notes.iter().position(|note| note.key == key);
    match (position(source_key), position(target_key)) {
        (Some(source), Some(target)) if source < target => DropSide::After,
        (Some(source), Some(target)) if source > target => DropSide::Before,
        _ => DropSide::None,
    }
}
```

**TypeScript**

```ts
// client/src/notes.ts. Mirrors drop_side in core/src/notes.rs.
type DropSide = 'none' | 'before' | 'after';

function dropSide(list: NoteList, sourceKey: string, targetKey: string): DropSide {
  const source = list.notes.findIndex((note) => note.key === sourceKey);
  const target = list.notes.findIndex((note) => note.key === targetKey);
  if (source === -1 || target === -1 || source === target) return 'none';
  return source < target ? 'after' : 'before';
}
```

The client calls `drop_side` from the `drag-over` arm of its action handler, keeps the answer with the target key, and invalidates the pane. `render` shows the answer as preview text; it sets no row prop, because the host owns the insertion line. The final move happens on the `dropped` action.

### Errors and edge cases

| Case | Result |
|---|---|
| Save over the 64 KiB metadata limit | `quota-exceeded` from `set-metadata`. Nothing is written. The footer shows `Not saved`. |
| Drop whose source key no longer exists | `move_note` returns `false`. Nothing is saved, and no error is shown. |
| Drop onto the row the note came from | The note returns to its own place, so the order does not change. |
| Index past the end of the list | Clamped. The note moves to the last place. |
| Metadata missing or damaged | Loads as an empty list. The next save replaces the damaged bytes. |
| Blank or spaces-only text | Nothing is added and nothing is saved. |
| `tree.metadata` grant declined or revoked | `metadata` returns none, so the pane shows no notes. `set-metadata` returns `not-permitted`. |
| Command runs twice | Two panes open. The intent has no dedup key in this step. |
| Drag over the source row itself | `drop_side` returns none, so no line is drawn. |
| Pane restarted after a save | The node and its metadata come back from the snapshot, so the list loads again. |

### Test it

Run the core tests, then run the plugin in development mode.

```bash
cargo test -p agent-tools-core notes
ilium plugin dev
```

For the TypeScript client, run the mirrored list functions under Bun, then the same dev loop:

```bash
bun test client/src/notes.test.ts
ilium plugin dev
```

Press the prefix key and `n`, type two notes into the input and press Enter, then drag one note onto
the other. Also check a drop onto the same row, and a pane that is reopened after a restart.

### What you see

At normal width, each note is one row with the input below. At a narrow width, long notes wrap. During a drag, the host draws an insertion line where the drop would land, and the note moves when the drop ends. The frame is the kernel's: rounded border, the chrome icons in the title, and a cyan border while the pane has focus.

<figure>
<pre class="term"><span class="t-hi">╭──≡ ● · Notes─────────────────────────────────────────────────╮</span>
<span class="t-hi">│</span>                                                              <span class="t-hi">│</span>
<span class="t-hi">│</span> Check the token lifetime with the auth team                  <span class="t-hi">│</span>
<span class="t-hi">│</span> Rotate the signing key before the release                    <span class="t-hi">│</span>
<span class="t-hi">│</span> Ask about the retry budget for 503 responses                 <span class="t-hi">│</span>
<span class="t-hi">│</span>                                                              <span class="t-hi">│</span>
<span class="t-hi">│</span> ╭ New note ────────────────────────────────────────────────╮ <span class="t-hi">│</span>
<span class="t-hi">│</span> │Draft the release notes                                   │ <span class="t-hi">│</span>
<span class="t-hi">│</span> ╰──────────────────────────────────────────────────────────╯ <span class="t-hi">│</span>
<span class="t-hi">╰──────────────────────────────────────────────────────────────╯</span>
</pre>
<figcaption>Illustrative, not generated. 64 columns, dark theme, pane focused (cyan rounded border, title with the chrome icons). Three notes at normal width, with the text input below. Enter in the input adds a note.</figcaption>
</figure>


<figure>
<pre class="term"><span class="t-hi">╭──≡ ● · Notes─────────────────────────────────────────────────╮</span>
<span class="t-hi">│</span>                                                              <span class="t-hi">│</span>
<span class="t-hi">│</span> Check the token lifetime with the auth team                  <span class="t-hi">│</span>
<span class="t-hi">│</span><span class="t-hi"> ──────────────────────────────────────────────────────────── </span><span class="t-hi">│</span>
<span class="t-hi">│</span> Rotate the signing key before the release                    <span class="t-hi">│</span>
<span class="t-hi">│</span><span class="t-dim"> Ask about the retry budget for 503 responses</span>                 <span class="t-hi">│</span>
<span class="t-hi">│</span>                                                              <span class="t-hi">│</span>
<span class="t-hi">│</span> ╭ New note ────────────────────────────────────────────────╮ <span class="t-hi">│</span>
<span class="t-hi">│</span> │                                                          │ <span class="t-hi">│</span>
<span class="t-hi">│</span> ╰──────────────────────────────────────────────────────────╯ <span class="t-hi">│</span>
<span class="t-hi">╰──────────────────────────────────────────────────────────────╯</span>
</pre>
<figcaption>Illustrative, not generated. A drag is in progress: the third note (dimmed) is over the first note, and the host draws the insertion line after it because the host sent <code>drag-over</code> with <code>accept</code> set to <code>after</code>. The drop will send <code>dropped</code> with index 1.</figcaption>
</figure>


<figure>
<pre class="term"><span class="t-hi">╭──≡ ● · Notes───────────────────╮</span>
<span class="t-hi">│</span>                                <span class="t-hi">│</span>
<span class="t-hi">│</span> Check the token lifetime with  <span class="t-hi">│</span>
<span class="t-hi">│</span> the auth team                  <span class="t-hi">│</span>
<span class="t-hi">│</span> Rotate the signing key before  <span class="t-hi">│</span>
<span class="t-hi">│</span> the release                    <span class="t-hi">│</span>
<span class="t-hi">│</span> Ask about the retry budget for <span class="t-hi">│</span>
<span class="t-hi">│</span> 503 responses                  <span class="t-hi">│</span>
<span class="t-hi">│</span>                                <span class="t-hi">│</span>
<span class="t-hi">│</span> ╭ New note ──────────────────╮ <span class="t-hi">│</span>
<span class="t-hi">│</span> │                            │ <span class="t-hi">│</span>
<span class="t-hi">│</span> ╰────────────────────────────╯ <span class="t-hi">│</span>
<span class="t-hi">╰────────────────────────────────╯</span>
</pre>
<figcaption>Illustrative, not generated. 34 columns, dark theme. The narrow pane wraps each note.</figcaption>
</figure>


<figure>
<pre class="term"><span class="t-hi">╭──≡ ● · Notes─────────────────────────────────────────────────╮</span>
<span class="t-hi">│</span>                                                              <span class="t-hi">│</span>
<span class="t-hi">│</span> Check the token lifetime with the auth team                  <span class="t-hi">│</span>
<span class="t-hi">│</span> Rotate the signing key before the release                    <span class="t-hi">│</span>
<span class="t-hi">│</span> Ask about the retry budget for 503 responses                 <span class="t-hi">│</span>
<span class="t-hi">│</span>                                                              <span class="t-hi">│</span>
<span class="t-hi">│</span> ╭ New note ────────────────────────────────────────────────╮ <span class="t-hi">│</span>
<span class="t-hi">│</span> │                                                          │ <span class="t-hi">│</span>
<span class="t-hi">│</span> ╰──────────────────────────────────────────────────────────╯ <span class="t-hi">│</span>
<span class="t-hi">│</span><span class="t-err"> Not saved</span>                                                    <span class="t-hi">│</span>
<span class="t-hi">╰──────────────────────────────────────────────────────────────╯</span>
</pre>
<figcaption>Illustrative, not generated. A save was refused (<code>quota-exceeded</code>), so the plugin wrote <code>Not saved</code> to the footer with the danger role.</figcaption>
</figure>

### See also

- [Basic primitive](/pages/primitives-basic/) covers the pane kind contract, the footer and
  `basic.set-status`.
- [Node metadata](/pages/capability-tree-metadata/) covers the 64 KiB limit and the grant rules.
- [Pane kinds](/pages/capability-panes-provide-kind/) covers `persist` and the pane kind manifest.
- [Tutorial step 7: an ambient scene](/pages/tutorial-07-ambient-scene/) adds a scene component.

API checked against `ilium:plugin@0.1.0` (drag feedback, AX11).
