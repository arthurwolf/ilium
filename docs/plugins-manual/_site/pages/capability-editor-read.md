# editor.read

The `editor.read` grant lets a client-side plugin read the editor's buffers: the open files, their lines, the selections and whether they have unsaved changes. It also lets the plugin mark those buffers with decorations and gutter icons, and it delivers the editor lifecycle events. The plugin never changes the text itself. Changing text needs `editor.edit`.

<figure>
<svg viewBox="0 0 340 200" role="img" aria-label="A client plugin with editor.read calls read-lines, set-gutter and set-decorations on the native editor and receives editor-changed events back">
  <defs>
    <marker id="arr-read" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="8" y="50" width="110" height="110" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="63" y="80" text-anchor="middle" fill="currentColor" font-size="12">Client plugin</text>
  <text x="63" y="96" text-anchor="middle" fill="currentColor" font-size="10">editor.read</text>
  <rect x="222" y="50" width="110" height="110" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="277" y="80" text-anchor="middle" fill="currentColor" font-size="12">Native editor</text>
  <text x="277" y="96" text-anchor="middle" fill="currentColor" font-size="10">primitive</text>
  <line x1="118" y1="80" x2="218" y2="80" stroke="currentColor" marker-end="url(#arr-read)"/>
  <text x="168" y="74" text-anchor="middle" fill="currentColor" font-size="10">read-lines</text>
  <line x1="118" y1="110" x2="218" y2="110" stroke="currentColor" marker-end="url(#arr-read)"/>
  <text x="168" y="104" text-anchor="middle" fill="currentColor" font-size="10">set-gutter</text>
  <line x1="118" y1="140" x2="218" y2="140" stroke="currentColor" marker-end="url(#arr-read)"/>
  <text x="168" y="134" text-anchor="middle" fill="currentColor" font-size="10">set-decorations</text>
  <path d="M277,160 L277,178 L63,178 L63,160" fill="none" stroke="currentColor" marker-end="url(#arr-read)"/>
  <text x="170" y="172" text-anchor="middle" fill="currentColor" font-size="10">editor-changed</text>
  <text x="170" y="196" text-anchor="middle" fill="currentColor" font-size="10">Edits need editor.edit, not this grant.</text>
</svg>
<figcaption>A client plugin with editor.read reads lines and sets marks on the native editor. It receives editor events, but it cannot change the text.</figcaption>
</figure>

## You will need

- The grant `editor.read` in `capabilities.client`. It is a client-side grant only. A server-side plugin that asks for it fails with `manifest.grant-side`.
- Rust 1.90 or later with the `wasm32-wasip2` target, or Bun 1.2 or later for TypeScript. The SDK pins the rest of the toolchain.
- The SDK module `ilium_plugin_sdk::primitives::editor` in Rust, or `@ilium/plugin-sdk/primitives/editor` in TypeScript.
- [panes.read](/pages/capability-panes-read/) if the plugin needs to know which pane a buffer belongs to.
- `editor.edit` only when the plugin also changes text.

## Grant syntax

`editor.read` takes no argument. The valid form is:

- `editor.read`

The invalid form `editor.read:project` fails the build with this text:

`capability "editor.read" takes no argument (got "editor.read:project")`

Buffers are chosen by id or by pane, not by a grant argument. A file in the project is read through a file scope such as `fs.read:project`, which is a different grant.

## The surface

The `editor` primitive offers these functions on the client side. Every function is synchronous. The functions with a result return an error code. The others return a plain value.

| Rust | TypeScript | Returns | Meaning |
|---|---|---|---|
| `editor::buffers()` | `buffers()` | `Vec<BufferRef>` / `BufferRef[]` | Every open buffer. |
| `editor::buffer_for_pane(pane)` | `bufferForPane(pane)` | `Option<BufferRef>` / `BufferRef \| undefined` | The buffer shown in one pane. |
| `editor::read_lines(buffer, first_line, count)` | `readLines(buffer, firstLine, count)` | `Result<Vec<String>, PluginError>` / `string[]`, throws | Up to `count` lines from `first_line`. |
| `editor::selections(buffer)` | `selections(buffer)` | `Vec<TextRange>` / `TextRange[]` | The selected ranges of the buffer. |
| `editor::set_decorations(buffer, layer, items)` | `setDecorations(buffer, layer, items)` | `Result<(), PluginError>` / `void`, throws | Styled ranges with optional hover text. |
| `editor::set_gutter(buffer, layer, marks)` | `setGutter(buffer, layer, marks)` | `Result<(), PluginError>` / `void`, throws | Icons in the gutter, each with an optional tooltip and command. |

The records used by these functions are:

| Record | Fields (Rust, TypeScript) | Meaning |
|---|---|---|
| `BufferRef` | `buffer` (u64, bigint), `pane` (option of node id), `path` (option of path), `language` (option of string), `revision` (u64), `line_count` / `lineCount` (u32), `dirty` (bool) | The description of one buffer. `revision` rises with every change. |
| `TextPosition` | `line`, `column` (u32) | A position. Columns count Unicode scalar values. |
| `TextRange` | `start`, `end` (`TextPosition`) | A range in one buffer. |
| `RangeDecoration` | `range` (`TextRange`), `style` (`Style`), `hover` (option of string) | A styled range. |
| `GutterMark` | `line` (u32), `icon` (Tabler icon name), `tooltip` (option of string), `command` (option of command id) | One icon in the gutter of a line. |

### Events

The grant opens four events, delivered to the `on-events` export of the `handler` trait:

| Event | Rust variant | TypeScript tag and payload | Delivery |
|---|---|---|---|
| `editor-opened` | `Event::EditorOpened(BufferRef)` | `'editor-opened'`, `BufferRef` | Never dropped. |
| `editor-closed` | `Event::EditorClosed(BufferRef)` | `'editor-closed'`, `BufferRef` | Never dropped. |
| `editor-changed` | `Event::EditorChanged(EditorChangedEvent)` | `'editor-changed'`, `buffer`, `revision`, `firstLine`, `lastLine` | Coalesced by buffer, debounced by 100 ms. |
| `editor-saved` | `Event::EditorSaved(EditorSavedEvent)` | `'editor-saved'`, `buffer`, `path` | Never dropped. |

`first_line` and `last_line` of `editor-changed` are both inside the changed range.

A plugin that offers the `annotator` export also needs this grant. The kernel calls the annotator for the visible lines of a buffer at most once every 100 ms. Without the grant the manifest fails with `manifest.export-grant`.

## Limits

- `read_lines` takes at most 10,000 lines per call. A larger `count` is `invalid-argument`.
- `set_decorations` and `set_gutter` take at most 10,000 items per layer. A larger list is `invalid-argument`.
- Each plugin owns its layers. A layer name is any string the plugin chooses. The decorations and marks of a layer are cleared when the plugin stops.

## Worked example: marking error lines in Rust

This client plugin reads the lines of every changed range and puts a gutter icon on each line that
contains ` ERROR `. The manifest requests the grant and exports nothing optional.

```json
{
  "capabilities": {
    "client": ["editor.read"]
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": []
  }
}
```

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::editor::{self, GutterMark};
use ilium_plugin_sdk::types::events::EditorChangedEvent;

const LOG_LAYER: &str = "log-levels";
const MAX_LINES_PER_READ: u32 = 10_000;

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct LogLevels;

impl Lifecycle for LogLevels {}

impl Handler for LogLevels {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::EditorChanged(EditorChangedEvent { buffer, first_line, last_line, .. })
                = event
            else {
                continue;
            };
            let count = (last_line - first_line + 1).min(MAX_LINES_PER_READ);
            let Ok(lines) = editor::read_lines(buffer, first_line, count) else {
                continue;
            };
            let marks: Vec<GutterMark> = lines
                .iter()
                .enumerate()
                .filter(|(_, text)| text.contains(" ERROR "))
                .map(|(offset, _)| GutterMark {
                    line: first_line + offset as u32,
                    icon: "alert-triangle".into(),
                    tooltip: Some("Error line".into()),
                    command: None,
                })
                .collect();
            let _ = editor::set_gutter(buffer, LOG_LAYER, marks);
        }
        Vec::new()
    }
}
```

## Worked example: the same plugin in TypeScript

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { readLines, setGutter, type GutterMark } from '@ilium/plugin-sdk/primitives/editor';

const LOG_LAYER = 'log-levels';
const MAX_LINES_PER_READ = 10_000;

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {},
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'editor-changed') continue;
      const { buffer, firstLine, lastLine } = event.val;
      const count = Math.min(lastLine - firstLine + 1, MAX_LINES_PER_READ);
      let lines: string[];
      try {
        lines = readLines(buffer, firstLine, count);
      } catch {
        continue; // The buffer closed before the call, or the grant was revoked.
      }
      const marks: GutterMark[] = [];
      lines.forEach((text, offset) => {
        if (!text.includes(' ERROR ')) return;
        marks.push({
          line: firstLine + offset,
          icon: 'alert-triangle',
          tooltip: 'Error line',
          command: undefined,
        });
      });
      try {
        setGutter(buffer, LOG_LAYER, marks);
      } catch {
        // The buffer closed between the two calls, so there is nothing left to mark.
      }
    }
    return [];
  },
});
```

The buffer id and the revision are `bigint` values in TypeScript. Line numbers are plain `number`
values. A failing call throws a `PluginError` that carries its code, so the `try` blocks are the
place where a closed buffer or a revoked grant is handled.

## Edge cases and errors

| Situation | Result |
|---|---|
| `read_lines`, `set_decorations` or `set_gutter` with the grant declined or revoked | `not-permitted` |
| `buffers()`, `buffer_for_pane()` or `selections()` with the grant declined or revoked | An empty list, none, an empty list. One refusal line is written to the plugin log. |
| The buffer no longer exists | `not-found` |
| `first_line` at or past the last line of the buffer | `invalid-argument` |
| `count` over 10,000 | `invalid-argument` |
| `count` runs past the last line | The lines that exist are returned, without an error. |
| More than 10,000 items for one layer | `invalid-argument` |
| The grant is revoked during a call | `revoked`. The plugin is re-activated with the reduced set. |
| Any call fails inside the host | `internal` |

The grant is revoked from Settings > Plugins > the plugin > Capabilities, or with
`ilium plugin grants <id> --revoke editor.read`. The kernel then withdraws the plugin's decorations
and gutter marks, sends `deactivate(revoked)`, and re-activates the plugin without the grant. A
revocation never counts as a failure. A disabled plugin loses its marks in the same way.

## Risk and tier rules

- This grant has the normal risk tier. It is not marked high risk.
- A T1 bundled plugin receives it automatically up to its manifest ceiling.
- A T2 or T3 plugin lists it in the review dialog, checked by default. The user can uncheck it
  before the install completes. The grant is remembered for that plugin on the client side.
- The native editor serves every call. The text is the user's work, so a plugin should read only
  the lines it needs.

## What the user sees

The review dialog lists the grant for a T2 plugin on the client side:

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.editor 0.1.0 by Example Ltd                                    │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ [x] Reads open files and buffers <span class="t-dim">……………………………………………………………………………</span> editor.read │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests editor.read, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

When the plugin marks a line, the gutter shows the icon and its tooltip appears on hover. A click
runs the command named in the mark, if the mark has one. Decorations appear as styled ranges with
their hover text. A plugin whose grant is declined still loads, but its marks and extra lines are
missing. The editor itself works without any plugin.

If the grant is requested on the wrong side, the build stops and names the side:

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin build
<span class="t-err">error</span> manifest.grant-side
      capability "editor.read" is not available on the server side
</pre>
<figcaption>Illustrative, not generated. The build output for a manifest that lists editor.read under the server side.</figcaption>
</figure>

## First-party features that request it

- Feature 20, syntax highlighting and source windows, through the annotator in Phase 7.
- Feature 21, Markdown source and rendered views, through the annotator in Phase 7.
- Feature 23, create an agent from an editor line, which reads the line under the cursor.

## See also

- [editor.edit](/pages/capability-editor-edit/) for changing the text.
- [editor.provide](/pages/capability-editor-provide/) for the post-v1 provider role.
- [panes.read](/pages/capability-panes-read/) for the pane of a buffer.
- [primitives: editor](/pages/primitives-editor/) for the primitive as a whole.
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/) for the bindings.
- [Errors and limits](/pages/errors-and-limits/) for the error codes.
- [Events and intents](/pages/events-and-intents/) for the event delivery rules.

Checked against the API version `ilium:plugin@0.1.0`.
