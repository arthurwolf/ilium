---
layout: page
title: Basic primitive
permalink: /pages/primitives-basic/
---

## Basic primitive

A basic pane is a pane whose body the plugin draws. The kernel draws everything around the body: the frame, the title in the top edge and a one-line status in the footer. The plugin declares a pane kind in its manifest, opens panes of that kind, and draws each pane's body from the `views` export. The plugin keeps its state in node metadata or in storage, so a restored pane shows the same content after a restart.

A basic pane has no process. The `pane-not-ready` error never applies to it, and the pane is usable as soon as the kernel creates it.

### What you need

**Grants.** The plugin needs `panes.provide-kind` to contribute a pane kind and to call the four `basic` functions for panes of that kind. The same grant lets the plugin draw the body through `ui`, so no separate `ui.panels` grant is needed. Add `tree.metadata` when the plugin keeps its state in node metadata.

**Language and side.** A basic pane runs in the client instance of the plugin. Its `handler` and its `views` export both run there. Rust and TypeScript both work; the examples below show each.

**Manifest.** Each pane kind is one entry in `pane_kinds`. The `persist` field has no default, so every entry must set it.

```json
{
  "pane_kinds": [{
    "id": "notes",
    "title": "Notes",
    "icon": "notebook",
    "primitive": "basic",
    "wants_keys": true,
    "persist": "metadata"
  }]
}
```

The global kind id is the plugin id, a dot and the local id. The plugin `example.scratchpad` with the local id `notes` gives the kind `example.scratchpad.notes`. The pane's content kind has the same string, so the plugin compares against it when a pane is created.

| Manifest field | Values | Meaning |
|---|---|---|
| `primitive` | `basic` | Selects this primitive. |
| `wants_keys` | `true` or `false` | With `true`, keys that the focused widget does not consume arrive as `key-pressed` for the pane. |
| `persist` | `metadata` or `none` | Decides what survives a restart (see below). |

With `persist: "metadata"`, the kernel saves the node's metadata, up to 64 KiB per node per plugin, and gives it back on restore. With `persist: "none"`, the pane starts empty after a restart.

### Interface

```wit
// Excerpt of ilium:primitives/basic in reference-wit
interface basic {
  set-title: func(pane: node-id, title: string) -> result<_, plugin-error>;
  set-status: func(pane: node-id, status: styled-text) -> result<_, plugin-error>;
  request-focus: func(pane: node-id) -> result<_, plugin-error>;
  size: func(pane: node-id) -> option<tuple<u16, u16>>;
}
```

| Function | Rust | TypeScript | Returns | Meaning |
|---|---|---|---|---|
| `set-title` | `basic::set_title(pane, &str)` | `basic.setTitle(pane, title)` | `Result<(), PluginError>` | Sets the title in the top edge of the frame. At most 256 characters. |
| `set-status` | `basic::set_status(pane, StyledText)` | `basic.setStatus(pane, status)` | `Result<(), PluginError>` | Sets the one-line footer. Styled text, at most 256 characters, no line break. |
| `request-focus` | `basic::request_focus(pane)` | `basic.requestFocus(pane)` | `Result<(), PluginError>` | Asks the kernel to focus the pane. |
| `size` | `basic::size(pane)` | `basic.size(pane)` | `Option<(u16, u16)>` | Columns and rows of the body. `None` for a node that is not a pane of the plugin's kinds. |

The TypeScript functions come from `@ilium/plugin-sdk/primitives/basic`, imported as
`import * as basic from '@ilium/plugin-sdk/primitives/basic';`. Rust uses
`use ilium_plugin_sdk::primitives::basic;`. A status is a list of spans. `Span::plain("Saved")` in
Rust and `span('Saved')` in TypeScript build a span with the default style.

`size` gives the same numbers as the `area` argument of `render` for that pane. Use either one to
lay out the body.

### Body and lifecycle

The body is the `views` export. Rust implements `fn render(&mut self, view: ViewTarget, area: Area)
-> UiTree`, and TypeScript exports `render(view, area)`. For a basic pane, `view` is
`ViewTarget::Pane(node)`, or `{ tag: 'pane', val: node }` in TypeScript.

The host calls `render` in five cases: when the pane's view first opens, after the plugin calls
`ui::invalidate` for that pane, on a resize, after a restore, and after a theme or font change. It never calls `render` for each
frame. The render deadline is 10 ms by default. A render that runs longer traps the plugin instance.

The pane's life follows this order:

1. The kernel creates the pane node and emits `pane-created` with a `pane-info` record. The record
   holds `node`, `content-kind`, `primitive`, `columns`, `rows`, `focused`, `title` and `is-restored`.
2. The plugin sets the title and the status for its own panes.
3. The kernel renders the body. Widget events arrive as `ui-action` events, and keys arrive as
   `key-pressed` when `wants_keys` is set.
4. After a change, the plugin writes its state and calls `ui::invalidate` so the body is drawn again.

### Events the plugin receives

| Event | Payload | Delivery |
|---|---|---|
| `pane-created` | `pane-info` | Never dropped. |
| `pane-resized` | `node`, `columns`, `rows` | Coalesced by node. |
| `pane-title-changed` | `node`, `title`, `source` | Coalesced by node. `source` is `osc`, `manual` or `inferred`. |
| `ui-action` | `view`, `key`, `action-id`, `kind`, `value` | Never dropped. `action-id` is a copy of `key` in 0.1.0. `kind` is `submitted` on Enter in a `text-input`. |
| `key-pressed` | `grab`, `pane`, `chord`, `repeat` | Only when `wants_keys` is `true`. Never dropped. |

The widget `key` is the action id that the plugin matches on, so give every interactive widget a
unique key.

### Example: a notes pane in Rust

The plugin `example.scratchpad` opens a pane kind for notes. The title shows the note count, the
footer shows the save state, and the body lists the notes above a text input. The notes are kept in
the node's metadata as UTF-8 bytes, one note per line, so the pane restores with its notes.

**Grants:** `panes.provide-kind`, `tree.metadata`

```rust
use ilium_plugin_sdk::host::{tree, ui};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::basic;
use ilium_plugin_sdk::types::layout::{Area, Constraint, Direction};
use ilium_plugin_sdk::types::text::ColorRole;
use ilium_plugin_sdk::types::ui::{ActionKind, ActionValue, UiTree, ViewTarget};
use ilium_plugin_sdk::ui::build::Tree;

const PANE_KIND: &str = "example.scratchpad.notes";
const NEW_NOTE_KEY: &str = "new-note";

#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
struct Scratchpad;

impl Lifecycle for Scratchpad {}

impl Handler for Scratchpad {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::PaneCreated(pane) if pane.content_kind == PANE_KIND => {
                    // Errors are not reported from here; see the edge cases below.
                    let _ = refresh_pane(pane.node, &load_notes(pane.node));
                }
                Event::UiAction(action) if action.key == NEW_NOTE_KEY => {
                    if let ViewTarget::Pane(node) = action.view {
                        if let (ActionKind::Submitted, ActionValue::Text(text)) =
                            (action.kind, action.value)
                        {
                            let _ = add_note(node, &text);
                        }
                    }
                }
                _ => {}
            }
        }
        Vec::new()
    }
}

impl Views for Scratchpad {
    fn render(&mut self, view: ViewTarget, _area: Area) -> UiTree {
        let notes = match view {
            ViewTarget::Pane(node) => load_notes(node),
            _ => Vec::new(),
        };
        let mut tree = Tree::new();
        let note_lines = tree.paragraph(notes).key("notes");
        let input = tree.text_input("New note").key(NEW_NOTE_KEY);
        let body = tree
            .layout(Direction::Vertical, [Constraint::Fill(1), Constraint::Length(3)])
            .child(note_lines)
            .child(input);
        tree.finish(body)
    }
}

fn load_notes(node: NodeId) -> Vec<String> {
    tree::metadata(node)
        .map(|bytes| String::from_utf8_lossy(&bytes).lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

fn save_notes(node: NodeId, notes: &[String]) -> Result<(), PluginError> {
    tree::set_metadata(node, notes.join("\n").into_bytes())
}

fn refresh_pane(node: NodeId, notes: &[String]) -> Result<(), PluginError> {
    basic::set_title(node, &format!("Notes ({})", notes.len()))?;
    basic::set_status(node, vec![Span::plain("Saved")])
}

fn add_note(node: NodeId, text: &str) -> Result<(), PluginError> {
    let note = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if note.is_empty() {
        return Ok(());
    }
    let mut notes = load_notes(node);
    notes.push(note);
    basic::set_status(node, vec![Span::plain("Saving")])?;
    if let Err(error) = save_notes(node, &notes) {
        basic::set_status(node, vec![Span::styled("Not saved", Style::role(ColorRole::Danger))])?;
        return Err(error);
    }
    refresh_pane(node, &notes)?;
    ui::invalidate(ViewTarget::Pane(node));
    Ok(())
}
```

`ui::invalidate` returns nothing, so it is not wrapped in `?`. The handler discards errors with
`let _ =` only where the state is already shown in the footer.

### Example: the same pane in TypeScript

The TypeScript entry module uses `definePlugin`. It exports the same three objects as the Rust
version: `lifecycle`, `handler` and `views`.

**Grants:** `panes.provide-kind`, `tree.metadata`

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { role, type Area, type Event, type Submission, type ViewTarget }
  from '@ilium/plugin-sdk/types';
import * as basic from '@ilium/plugin-sdk/primitives/basic';
import * as treeHost from '@ilium/plugin-sdk/host/tree';
import { invalidate } from '@ilium/plugin-sdk/host/ui';
import { layout, paragraph, span, textInput, tree } from '@ilium/plugin-sdk/ui/build';

const PANE_KIND = 'example.scratchpad.notes';
const NEW_NOTE_KEY = 'new-note';

function loadNotes(node: bigint): string[] {
  const bytes = treeHost.metadata(node);
  if (bytes === undefined) return [];
  return new TextDecoder().decode(bytes).split('\n').filter((line) => line.length > 0);
}

function saveNotes(node: bigint, notes: string[]): void {
  treeHost.setMetadata(node, new TextEncoder().encode(notes.join('\n')));
}

function refreshPane(node: bigint, notes: string[]): void {
  basic.setTitle(node, `Notes (${notes.length})`);
  basic.setStatus(node, [span('Saved')]);
}

function addNote(node: bigint, rawText: string): void {
  const note = rawText.split(/\s+/).filter((word) => word !== '').join(' ');
  if (note === '') return;
  const notes = [...loadNotes(node), note];
  basic.setStatus(node, [span('Saving')]);
  try {
    saveNotes(node, notes);
  } catch (error) {
    basic.setStatus(node, [span('Not saved', { fg: role('danger') })]);
    return;
  }
  refreshPane(node, notes);
  invalidate({ tag: 'pane', val: node });
}

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'pane-created' && event.val.contentKind === PANE_KIND) {
        refreshPane(event.val.node, loadNotes(event.val.node));
      }
      if (event.tag === 'ui-action' && event.val.key === NEW_NOTE_KEY
          && event.val.kind === 'submitted' && event.val.view.tag === 'pane'
          && event.val.value.tag === 'text') {
        addNote(event.val.view.val, event.val.value.val);
      }
    }
    return [];
  },
  render(view: ViewTarget, area: Area) {
    const notes = view.tag === 'pane' ? loadNotes(view.val) : [];
    return tree(layout({
      direction: 'vertical',
      constraints: [{ tag: 'fill', val: 1 }, { tag: 'length', val: 3 }],
    }, paragraph(notes, { key: 'notes' }), textInput('New note', { key: NEW_NOTE_KEY })));
  },
});
```

The notes are passed through `tree.metadata` as bytes in both versions. A note with a line break
cannot be stored, so both versions join the words of the input with single spaces first.

### Simulated render: a notes pane

<figure>
<pre class="term">
╭──≡ ● · Notes (3)───────────────────────────────────────────────╮
│ Project plan                                                   │
│ Meeting notes                                                  │
│ Reading list                                                   │
│                                                                │
│ ╭──≡ ● · New note────────────────────────────────────────────╮ │
│ │ <span class="t-dim">Type a note and press Enter</span>                              │   │
│ ╰────────────────────────────────────────────────────────────╯ │
│ <span class="t-ok">Saved</span>                                                          │
╰────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>A basic notes pane in the dark theme, 66 columns by 10 rows. Every border is rounded and carries the chrome title. The pane title shows the note count and the footer row shows the save state.</figcaption>
</figure>

The frame is drawn by the kernel. The title `Notes (3)` comes from `basic.set_title`, and the
footer `Saved` comes from `basic.set_status`. The body is the plugin's tree.

### What the user sees

- When the pane opens, the title shows the number of stored notes and the footer shows `Saved`.
- The user types into the input and presses Enter. The footer changes to `Saving`, then back to
  `Saved`, and the title count goes up by one.
- If the save is refused (for example `quota-exceeded`), the footer shows `Not saved` in the danger
  colour, and the note is not kept.
- After a restart the pane restores its notes, because `persist` is `metadata`.

### Diagram: frame and body

<figure>
<svg viewBox="0 0 340 150" role="img" aria-label="Kernel frame around a plugin body">
<title>Kernel frame and plugin body</title>
<desc>The kernel draws the outer frame with the title and the status line. The plugin draws the body inside the frame from its views.render tree.</desc>
<rect x="8" y="8" width="324" height="130" rx="6" fill="none" stroke="currentColor"/>
<text x="20" y="28" font-size="12" fill="currentColor">Title: set by set-title (kernel frame)</text>
<rect x="20" y="40" width="300" height="70" rx="4" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
<text x="170" y="78" font-size="12" text-anchor="middle" fill="currentColor">Plugin body: views.render</text>
<text x="20" y="128" font-size="12" fill="currentColor">Status: set by set-status</text>
</svg>
<figcaption>The kernel owns the frame. The plugin owns the body and the text it sets for the title and the footer.</figcaption>
</figure>

### Diagram: one submitted note

<figure>
<svg viewBox="0 0 400 250" role="img" aria-label="Flow from Enter in the input to a redrawn body">
<defs>
<marker id="basic-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M 0 0 L 10 5 L 0 10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="10" y="10" width="380" height="34" rx="4" fill="none" stroke="currentColor"/>
<text x="200" y="31" font-size="11" text-anchor="middle" fill="currentColor">1. The user presses Enter in the new-note input</text>
<line x1="200" y1="44" x2="200" y2="60" stroke="currentColor" marker-end="url(#basic-arrow)"/>
<rect x="10" y="60" width="380" height="34" rx="4" fill="none" stroke="currentColor"/>
<text x="200" y="81" font-size="11" text-anchor="middle" fill="currentColor">2. The kernel sends ui-action with kind submitted</text>
<line x1="200" y1="94" x2="200" y2="110" stroke="currentColor" marker-end="url(#basic-arrow)"/>
<rect x="10" y="110" width="380" height="34" rx="4" fill="none" stroke="currentColor"/>
<text x="200" y="131" font-size="11" text-anchor="middle" fill="currentColor">3. The plugin writes the notes with tree.set-metadata</text>
<line x1="200" y1="144" x2="200" y2="160" stroke="currentColor" marker-end="url(#basic-arrow)"/>
<rect x="10" y="160" width="380" height="34" rx="4" fill="none" stroke="currentColor"/>
<text x="200" y="181" font-size="11" text-anchor="middle" fill="currentColor">4. The plugin sets the title and the footer to Saved</text>
<line x1="200" y1="194" x2="200" y2="210" stroke="currentColor" marker-end="url(#basic-arrow)"/>
<rect x="10" y="210" width="380" height="34" rx="4" fill="none" stroke="currentColor"/>
<text x="200" y="231" font-size="11" text-anchor="middle" fill="currentColor">5. ui.invalidate redraws the body through views.render</text>
</svg>
<figcaption>One submitted note. Each step is one call or event; the body is redrawn only in step 5.</figcaption>
</figure>

### Errors and edge cases

| Code | When it happens on a basic pane | What the plugin does |
|---|---|---|
| `not-permitted` | The node is not a pane of one of the plugin's kinds, or `panes.provide-kind` or `tree.metadata` is missing. | Stop calling for that node. The host logs `refused <grant> by basic.set-title (not-permitted)`, naming the missing grant at level warn. |
| `invalid-argument` | A title over 256 characters, or a status over 256 characters or with a line break. | Fix the argument. Nothing is changed. |
| `conflict` | The user has set a manual title on the pane. The title is not changed. | Show the user's title. The plugin title appears again after the user clears the manual title. |
| `quota-exceeded` | The metadata would be over 64 KiB. `set-metadata` writes nothing. | Keep the older notes, show `Not saved`, and ask the user to delete notes. |
| `not-found` | The node was removed during `set-metadata`. | Drop the node from the plugin's state. |
| `revoked`, `internal` | The authorization epoch changed, or the kernel failed. | Retry later or wait for the next event. |

Other rules:

- `pane-not-ready` never applies to basic panes.
- `size` returns `none`, with no error code, for a node that is not one of the plugin's panes.
- `metadata` returns `none` when `tree.metadata` is declined or revoked, so an empty list is shown
  instead of an error.
- A render that returns a layout whose child count differs from its constraint count traps the
  instance. A zero-size area draws nothing and is not an error.
- With `persist: "none"`, notes kept only in metadata are lost on restart. Pick the
  persist mode that matches where the state lives.

### Pane activity and dirty state

```wit
// Excerpt of ilium:primitives/basic in reference-wit
report-activity: func(pane: node-id, activity: agent-activity) -> result<_, plugin-error>;
set-dirty: func(pane: node-id, dirty: bool) -> result<_, plugin-error>;
```

| Rust (`primitives::basic`) | TypeScript | Effect |
|---|---|---|
| `report_activity(pane, activity)` | `reportActivity` | Drives the activity icon of the pane's tree row, as an agent pane does. |
| `set_dirty(pane, dirty)` | `setDirty` | Marks the pane as holding unsaved work; closing it asks before discarding. |

Both need `panes.provide-kind` and return `not-permitted` for a node that is not the plugin's kind. A pane that reports `working` shows the spinner in the now slot of its tree row.

**Rust**

```rust
basic::report_activity(pane, AgentActivity::Working)?;
basic::set_dirty(pane, true)?;
```

**TypeScript**

```ts
reportActivity(pane, 'working');
setDirty(pane, true);
```

<pre class="term">▼ 📂 Ilium
›   📝 <span class="t-hi">⠋</span> Notes (3)
</pre>

### Used by

- The file explorer and picker is planned as a basic pane kind with a `tree-list` body, `fs.list-dir`,
  `fs.watch` and the `editor-open` intent.
- Kanban boards are native in version 1. In phase 7 they move to a basic pane kind.
- Boards created from Markdown files are native in version 1 and become part of the board plugin in
  phase 7.

Requirement ids for this page: [PRI-030], [PRI-031]. See the [requirements index](/pages/reference-requirements/).

### See also

- [Plugin exports](/pages/api-exports/) for the `views` and `handler` exports.
- [Panels and the ui tree](/pages/api-ui/) for the widgets used in the body.
- [Layout](/pages/layout/) for how the `area` argument divides the body.
- [Text input and other input widgets](/pages/widgets-input/) for `changed` and `submitted`.
- [Panes](/pages/api-panes/) for `pane-created`, `pane-info` and `is-restored`.
- [Tree](/pages/api-tree/) for `metadata` and `set-metadata`.
- [Notes pane tutorial](/pages/tutorial-06-notes-pane/) for a step-by-step version of this page.
- [Terminal primitive](/pages/primitives-terminal/) and [Agent primitive](/pages/primitives-agent/) for the other two primitives.
- [Errors and limits](/pages/reference-errors-and-limits/) for the error codes and limits used here.

This page follows `ilium:plugin@0.1.0`.
