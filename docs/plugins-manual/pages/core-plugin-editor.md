---
layout: page
title: Editor
permalink: /pages/core-plugin-editor/
---

## Editor

<div class="note">
<p>Status: the editor is native in v1. This page describes the plugin form that arrives in Phase 7 (see the <a href="/pages/roadmap/">roadmap</a>).</p>
</div>

The Editor plugin (`ilium.core.editor`, client part plus a small server part for settings) owns editor panes: buffers, syntax highlighting through a token provider, line numbers and the minimap, Markdown source and rendered views, the file explorer and picker, and creating an agent from an editor line. As the editor provider it owns buffer text while it runs.

### What it covers

- Editor panes (Editors and boards, Editor panes).
- Syntax highlighting, line numbers and the minimap (Editors and boards, Editor panes; Settings, Editor).
- Markdown source and rendered views (Editors and boards, Editor panes).
- The file explorer and file picker (Editors and boards, Editor panes).
- Creating an agent from an editor line (Automation, Create an agent from an editor line).

### See it

<figure><img src="/assets/demos/13-markdown-editor.gif" alt="A Markdown file edited in a pane next to its rendered view" loading="lazy"><figcaption>The Markdown editor. The source and the rendered view show the same buffer.</figcaption></figure>

<figure><img src="/assets/demos/09-agent-from-line.gif" alt="A TODO line in an editor pane starts a new agent with that line as its prompt" loading="lazy"><figcaption>Create an agent from a line. The line text becomes the first prompt of the new agent.</figcaption></figure>

### What the user sees

<figure>
<pre class="term">╭─ main.rs ────────────────────────────────────╮
│  1 use std::fs;                             ·│
│  2                                          -│
│  3 fn main() {                              =│
│  4     let n = 42;                          ≡│
│  5     // run it                            █│
│  6 }                                        ·│
╰──────────────────────────────────────────────╯</pre>
<figcaption>Rendered at 48 columns. Illustrative. An editor pane with line numbers, highlighted Rust and a minimap column at the right edge.</figcaption>
</figure>

<figure>
<pre class="term">╭─ notes.md ───────────╮╭─ notes.md (view) ────╮
│<span class="t-hi"># Plan</span>              ││<span class="t-hi">Plan</span>                    │
│<span class="t-dim">- [ ] ship it</span>       ││<span class="t-dim">[ ] ship it</span>             │
│<span class="t-warn">TODO write docs</span>     ││<span class="t-warn">TODO write docs</span>         │
│<span class="t-dim">                    </span> ││                       │
│<span class="t-ok">Create agent from line</span> ││                     │
╰──────────────────────╯╰──────────────────────╯
</pre>
<figcaption>Rendered at 46 columns. Illustrative. The Markdown rendered view next to its source, and the <code>Create agent from line</code> gutter hint over the TODO line.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.editor",
  "name": "Editor",
  "version": "0.1.0",
  "publisher": "Ilium",
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": ["views", "token-provider", "service-provider"]
  },
  "capabilities": {
    "client": [
      "editor.read", "editor.edit", "fs.read:project",
      "panes.create", "terminal.write", "ui.panels"
    ]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.editor",
  "name": "Editor",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "The Editor plugin (ilium.core.editor, client plus a small server part) owns editor panes: buffers, syntax highlighting through a token provider, line numbers and the minimap, Markdown source and rendered views, the file explorer and picker, and creating an age",
  "license": "MIT",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": [
      "service-provider"
    ]
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": [
      "views",
      "token-provider",
      "service-provider"
    ]
  },
  "capabilities": {
    "client": [
      "editor.read",
      "editor.edit",
      "fs.read:project",
      "panes.create",
      "terminal.write",
      "ui.panels",
      "services.use:ilium.core.editor/set-setting"
    ],
    "server": [
      "services.provide:ilium.core.editor/set-setting",
      "settings.write-kernel:editor"
    ]
  },
  "activation": [
    "pane-kind:editor"
  ],
  "unstable": ["plugin-services"],
  "contributes": {
    "services": {
      "provides": [
        { "name": "ilium.core.editor/set-setting", "version": "0.1.0", "schema": "schemas/set-setting.json" }
      ]
    }
  },
  "limits_tier": "first-party"
}
```

### Server part

This plugin has a server part, added only for settings. Its one job is to write the `editor` Settings tab it owns: `settings.write-kernel:editor` is a server-side grant and the client part never holds it (assumption recorded here: the grant is server side only, see [settings.write-kernel](/pages/capability-settings-write-kernel/)). The client part draws the controls. When the user changes a value, the client part sends it to the server part through the plugin service `ilium.core.editor/set-setting` (grants `services.use:ilium.core.editor/set-setting` on the client side and `services.provide:ilium.core.editor/set-setting` on the server side, see [services](/pages/api-services/)). The name follows the rule `<plugin-id>/<name>` with a name of 1 to 64 characters from `a-z`, `0-9` and `-`. The server part validates the value and calls the kernel-settings write.

The schema file for `set-setting` (`schemas/set-setting.json`, shown here because the manual ships no package files):

```json
{
  "set": {
    "params": {
      "type": "object",
      "properties": {
        "tab": { "type": "string" },
        "key": { "type": "string" },
        "value": { "type": "string" }
      },
      "required": ["tab", "key", "value"]
    },
    "result": {
      "type": "object",
      "properties": {},
      "required": []
    }
  }
}
```

### Kernel tabs this plugin owns

This plugin is the single owner of the following built-in Settings tabs. It holds `settings.write-kernel:<tab>` for each one (T1 only, one holder per tab, see [kernel settings](/pages/api-kernel-settings-and-keymap/#writing-a-kernel-tab)). The values stay in the same `config.toml` tables as before, so no user setting moves. [COR-030] [COR-033]

| Tab id | Label | Config table | Readers |
|---|---|---|---|
| `editor` | Editor | `[editor]` | Terminal |

If the plugin is disabled, the tab stays in the rail with its stored values shown read-only and the notice "Editor is disabled. Values are kept." The kernel keeps reading the keys it consumes itself (listed in the Consumer column of the key table in [config and data migration](/pages/reference-config-and-data-migration/)). Behaviour that only this plugin provides is off. The exact result for this plugin is its row in [kernel and boot](/pages/reference-kernel-and-boot/). [COR-022]

### Samples

#### Be the editor provider

The plugin owns buffer text and answers every editor call. It exports the editor provider, loads text on `editor-opened`, drops it on `editor-closed` and answers reads from its own rope. See [editor.provide](/pages/capability-editor-provide/), the [editor primitive](/pages/primitives-editor/), [editor.read](/pages/capability-editor-read/) and the [events](/pages/api-events/). The `editor-provider` service is described on the editor.provide page.

**Rust**

```rust
use std::collections::HashMap;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::fs;
use ilium_plugin_sdk::primitives::editor;

#[ilium_plugin(side = "client", exports = ["views", "service-provider", "token-provider"])]
#[derive(Default)]
struct Editor {
    ropes: HashMap<BufferId, ropey::Rope>,
}

impl Lifecycle for Editor {}

impl Handler for Editor {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::EditorOpened(opened) => {
                    let text = opened
                        .path
                        .as_ref()
                        .and_then(|path| fs::read("project", path).ok())
                        .unwrap_or_default();
                    self.ropes.insert(opened.buffer, ropey::Rope::from_str(&text));
                }
                Event::EditorClosed(buffer) => {
                    self.ropes.remove(&buffer);
                }
                _ => {}
            }
        }
        Vec::new()
    }
}

impl Editor {
    // Answers a read call of the editor-provider service.
    fn line_text(&self, buffer: BufferId, line: u32) -> Option<String> {
        let rope = self.ropes.get(&buffer)?;
        (line as usize) < rope.len_lines()
            .then(|| rope.line(line as usize).to_string())
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import * as fs from '@ilium/plugin-sdk/host/fs';
import * as editor from '@ilium/plugin-sdk/primitives/editor';
import type { Event, Submission } from '@ilium/plugin-sdk/types';

const buffers = new Map<bigint, string[]>();

export const { lifecycle, handler, views, serviceProvider } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'editor-opened') {
        const opened = event.val;
        const text = opened.path === undefined ? '' : fs.readToString('project', opened.path);
        buffers.set(opened.buffer, text.split('\n'));
      } else if (event.tag === 'editor-closed') {
        buffers.delete(event.val);
      }
    }
    return [];
  },
  render() { /* panel views */ return tree(paragraph([[span('Editor')]])); },
  handle(call) { /* answers editor-provider reads from `buffers` */ return { tag: 'ok', val: new Uint8Array() }; },
});

export const lineText = (buffer: bigint, line: number): string | undefined =>
  buffers.get(buffer)?.[line];
```

#### Syntax highlighting

A token provider teaches the editor a language. This one implements `tokens(lines)` for TOML within the 10 ms deadline, and invalidates only the changed range from `editor-changed`. Pages: [token provider](/pages/api-token-provider/), the [editor primitive](/pages/primitives-editor/) and [events](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::exports::TokenProvider;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::exports::token_provider::{Token, TokenKind};

impl TokenProvider for Editor {
    fn tokens(
        &mut self,
        _buffer: BufferId,
        first_line: u32,
        lines: Vec<String>,
        _state_in: Option<Vec<u8>>,
    ) -> (Vec<Token>, Option<Vec<u8>>) {
        let mut out = Vec::new();
        for (offset, text) in lines.iter().enumerate() {
            let line = first_line + offset as u32;
            let trimmed = text.trim_start();
            if trimmed.starts_with('#') {
                out.push(Token { line, start: 0, length: text.chars().count() as u32, kind: TokenKind::Comment });
            } else if trimmed.starts_with('[') {
                out.push(Token { line, start: 0, length: text.chars().count() as u32, kind: TokenKind::Type });
            } else if let Some(at) = text.find('=') {
                out.push(Token { line, start: 0, length: at as u32, kind: TokenKind::Variable });
                if text[at..].contains('"') {
                    out.push(Token { line, start: at as u32 + 1, length: (text.len() - at - 1) as u32, kind: TokenKind::String });
                }
            }
        }
        (out, None) // TOML needs no state between blocks
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Token } from '@ilium/plugin-sdk/types';

export const { lifecycle, handler, tokenProvider } = definePlugin({
  side: 'client',
  onEvents() { return []; },
  tokens(_buffer, firstLine, lines): [Token[], Uint8Array | undefined] {
    const out: Token[] = [];
    lines.forEach((text, offset) => {
      const line = firstLine + offset;
      const trimmed = text.trimStart();
      if (trimmed.startsWith('#')) {
        out.push({ line, start: 0, length: text.length, kind: 'comment' });
      } else if (trimmed.startsWith('[')) {
        out.push({ line, start: 0, length: text.length, kind: 'type-name' });
      } else {
        const at = text.indexOf('=');
        if (at < 0) return;
        out.push({ line, start: 0, length: at, kind: 'variable' });
        if (text.slice(at).includes('"')) {
          out.push({ line, start: at + 1, length: text.length - at - 1, kind: 'string' });
        }
      }
    });
    return [out, undefined]; // TOML needs no state between blocks
  },
});
```

#### Edits, minimap and viewport

Edits apply safely and the minimap shows the shape of the file. The plugin submits `editor-apply-edits` with a `base-revision`, so a stale revision is refused with `refused(conflict)`. It draws a `minimap` that follows `editor-viewport-changed`. Pages: [editor viewport](/pages/primitives-editor-viewport/), [editor.edit](/pages/capability-editor-edit/), [data display](/pages/widgets-data-display/), [intents](/pages/api-intents/) and [events](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::EditorApplyEditsSpec;
use ilium_plugin_sdk::types::editing::TextEdit;
use ilium_plugin_sdk::types::editing::{TextPosition, TextRange};
use ilium_plugin_sdk::ui::build::Tree;

fn replace_line(buffer: BufferId, revision: u64, line: u32, replacement: &str) -> Submission {
    Submission::new(Intent::EditorApplyEdits(EditorApplyEditsSpec {
        buffer,
        base_revision: revision,
        edits: vec![TextEdit {
            range: TextRange {
                start: TextPosition { line, column: 0 },
                end: TextPosition { line: line + 1, column: 0 },
            },
            replacement: format!("{replacement}\n"),
        }],
    }))
}

// On Event::EditorViewportChanged(viewport): keep the visible range.
fn minimap(tree: &mut Tree, density: Vec<u8>, first: u32, last: u32, total: u32) -> NodeHandle {
    tree.minimap(density).viewport((first, last)).total_lines(total).key("overview")
}
```

**TypeScript**

```ts
import { minimap } from '@ilium/plugin-sdk/ui/build';
import type { Submission } from '@ilium/plugin-sdk/types';

const replaceLine = (buffer: bigint, revision: bigint, line: number, replacement: string): Submission => ({
  intent: { tag: 'editor-apply-edits', val: {
    buffer,
    baseRevision: revision,
    edits: [{
      range: { start: { line, column: 0 }, end: { line: line + 1, column: 0 } },
      replacement: `${replacement}\n`,
    }],
  } },
  correlation: 1n,
  dedupKey: undefined,
});

// On the 'editor-viewport-changed' event: keep the visible range.
const overview = (density: number[], first: number, last: number, total: number) =>
  minimap(density, { key: 'overview', viewport: [first, last], totalLines: total });
```

#### Markdown rendered view

A panel renders the buffer as Markdown with the source beside it. It shows a `code-view` of the source and a `markdown` widget of the same buffer in a `split`. Pages: [text widgets](/pages/widgets-text/), [media and embedded widgets](/pages/widgets-media-and-embedded/) and [layout and containers](/pages/widgets-layout-and-containers/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::layout::{Area, Direction};
use ilium_plugin_sdk::types::ui::{UiTree, ViewTarget};
use ilium_plugin_sdk::ui::build::Tree;

impl Views for Editor {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let source = tree.code_view(self.current).key("source");
        let rendered = tree.markdown(self.source_text()).key("rendered");
        let root = tree
            .split(Direction::Vertical)
            .key("markdown-split")
            .position(500)
            .child(source)
            .child(rendered);
        tree.finish(root)
    }
}
```

**TypeScript**

```ts
import { codeView, markdown, split, tree } from '@ilium/plugin-sdk/ui/build';

function render(current: bigint, sourceText: string) {
  const source = codeView(current, { key: 'source' });
  const rendered = markdown(sourceText, { key: 'rendered' });
  return tree(split({ direction: 'vertical', key: 'markdown-split', position: 500 }, [source, rendered]));
}
```

#### Create an agent from a line

Clicking a TODO line starts an agent with that line as its prompt. On `editor-pointer` over a line that starts with `TODO`, the plugin submits `create-pane` with `initial-input` set to the line text. Pages: the [editor primitive](/pages/primitives-editor/), [panes.create](/pages/capability-panes-create/), [events](/pages/api-events/) and [intents](/pages/api-intents/). `initial-input` also needs the `terminal.write` grant from the manifest.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::editor;
use ilium_plugin_sdk::types::intents::CreatePaneSpec;

fn on_pointer(event: Event) -> Option<Submission> {
    let Event::EditorPointer(pointer) = event else { return None };
    let text = editor::read_lines(pointer.buffer, pointer.position.line, 1)?;
    if !text.trim_start().starts_with("TODO") { return None; }
    Some(Submission::keyed(
        Intent::CreatePane(CreatePaneSpec {
            parent: None,
            title: "TODO agent".into(),
            content_kind: "ilium.terminal".into(),
            cwd: None,
            command: Some(vec!["claude".into()]),
            initial_input: Some(text.trim().to_string()),
        }),
        format!("todo-{}-{}", pointer.buffer, pointer.position.line),
    ))
}
```

**TypeScript**

```ts
import * as editor from '@ilium/plugin-sdk/primitives/editor';
import type { Event, Submission } from '@ilium/plugin-sdk/types';

function onPointer(event: Event): Submission | undefined {
  if (event.tag !== 'editor-pointer') return undefined;
  const text = editor.lineText(event.val.buffer, event.val.position.line);
  if (text === undefined || !text.trimStart().startsWith('TODO')) return undefined;
  return {
    intent: { tag: 'create-pane', val: {
      parent: undefined, title: 'TODO agent', contentKind: 'ilium.terminal',
      cwd: undefined, command: ['claude'], initialInput: text.trim(),
    } },
    correlation: 1n,
    dedupKey: `todo-${event.val.buffer}-${event.val.position.line}`,
  };
}
```

#### Changed lines from the editor

`editor-changed` gives the buffer, its new revision and the first and last changed line. The plugin re-lints only that range and leaves the rest of the decorations as they are.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;

impl Handler for LintRange {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if let Event::EditorChanged(change) = event {
                self.dirty.entry(change.buffer).or_default().push(change.first_line..=change.last_line);
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
    if (event.tag !== 'editor-changed') continue;
    const { buffer, firstLine, lastLine } = event.val;
    markDirty(buffer, firstLine, lastLine);
  }
  return [];
}
```

### Extension points

Other plugins change what `ilium.core.editor` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.editor/<point>`). Contributors need `extensions.contribute:ilium.core.editor/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `before-save` | hook (transform) | Change buffer text just before it is written, for example to format it. | buffer, path, language, text; reply: text | high |
| `file-openers` | collect | Choose which pane kind opens files with certain extensions. | entry: id, extensions, pane_kind, priority | standard |
| `line-badges` | collect | Add a marker in the editor gutter for lines that match. | entry: id, regex, glyph, tooltip | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.editor/buffers`: list, read, replace-range: read and edit open buffers

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.editor/saved`: { buffer, path } after a save

`languages` entries and the `token-provider` export colour the buffers this plugin shows.

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.editor"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Removes trailing spaces on every line when a Markdown buffer is saved

A package adds a `transform` handler. Each handler gets the previous output and may replace it.

```json
{
  "api": "ilium:plugin@0.1.0",
  "exports": [
    "hook-handler"
  ],
  "capabilities": { "client": ["extensions.contribute:ilium.core.editor/before-save"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.editor/before-save",
        "id": "trim-trailing",
        "priority": 50,
        "handler": "trim_trailing"
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
            "trim_trailing" => {
                // call.payload_json holds: buffer, path, language, text
                let mut payload: serde_json::Value = match serde_json::from_str(&call.payload_json) {
                    Ok(value) => value,
                    Err(_) => return HookReply::Pass,
                };
                // Change the fields the hook owns, for example payload["text"].
                let new_payload_json = payload.to_string();
                HookReply::Replace(new_payload_json)
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
    if (call.extension !== 'trim_trailing') return { tag: 'pass' };
    // call.payloadJson holds: buffer, path, language, text
    const payload = JSON.parse(call.payloadJson);
    // Change the fields the hook owns, for example payload.text.
    const newPayloadJson = JSON.stringify(payload);
    return { tag: 'replace', val: newPayloadJson };
  },
});
```

A handler that is late, traps or returns a reply the mode does not allow counts as `pass`. See [Extension points](/pages/api-extension-points/#hooks).

### Extension points this plugin owns

The table gives each point's side, version, deadline, maximum and dynamic value. A value marked (default) is not stated on this page; it is the default from [Extension points](/pages/api-extension-points/): version 0.1.0, a 20 ms deadline for hook points, no deadline for collect points, 8 handlers for hook points, 64 entries for collect points, and not dynamic.

| Point | Side | Version | Deadline | Max | Dynamic |
|---|---|---|---|---|---|
| `ilium.core.editor/before-save` | client | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) |
| `ilium.core.editor/file-openers` | client | 0.1.0 (default) | none (default) | 64 entries (default) | no (default) |
| `ilium.core.editor/line-badges` | client | 0.1.0 (default) | none (default) | 64 entries (default) | no (default) |

Side basis: `ilium.core.editor/file-openers` is client because the manifest gives the server part only `services.provide` and `settings.write-kernel:editor` (no `extensions.provide` for it), and the page says the server part's one job is settings, while the client part owns the editor panes that pick which pane kind opens a file. Side basis: `ilium.core.editor/line-badges` is client because the manifest gives the server part no `extensions.provide` for it, and the gutter markers it adds belong to the editor panes the client part draws.

The side of `before-save` is client because the example above contributes that point from the client capabilities. This page does not state a side for `file-openers` or `line-badges`, so their side is unknown.

### ABI used

- [editor.provide](/pages/capability-editor-provide/), [editor.read](/pages/capability-editor-read/), [editor.edit](/pages/capability-editor-edit/), [Editor primitive](/pages/primitives-editor/), [Editor viewport](/pages/primitives-editor-viewport/)
- [Token provider](/pages/api-token-provider/), [panes.create](/pages/capability-panes-create/)
- [Data display](/pages/widgets-data-display/), [Text widgets](/pages/widgets-text/), [Media and embedded widgets](/pages/widgets-media-and-embedded/), [Layout and containers](/pages/widgets-layout-and-containers/)
- [Intents](/pages/api-intents/), [Events](/pages/api-events/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Terminal plugin](/pages/core-plugin-terminal/)
- [Kanban plugin](/pages/core-plugin-kanban/)

### Security

Risk marks of this plugin's extension points follow the corrected table in [Extension security](/pages/reference-extension-security/) ([SEC-293]).
