---
layout: page
title: Editor primitive
permalink: /pages/primitives-editor/
---

## Editor primitive

The editor primitive gives a client plugin access to the Ilium editor. A plugin can list the open buffers, read lines and selections, draw decorations and gutter marks, create and load buffers, save them, and change their text. It can also export an `annotator` that highlights text in the visible lines of a buffer. Only client-side plugins can use the primitive.

The native editor provides this interface. The editor is a built-in pane kind, and no grant lets a plugin replace it in 0.1.0. A later version may let an editor plugin provide it under the `editor.provide` grant. That provider is post-v1 and high risk, and a package that names `editor.provide` fails with `manifest.grant-unknown`. See [Editor provider](/pages/capability-editor-provide/).

### What you need

| Grant | What it allows |
|---|---|
| `editor.read` | List buffers, read lines and selections, set decorations and gutter marks, receive editor events, and export an `annotator`. |
| `editor.edit` | Create, load, save and close buffers, apply edits, and open files in the editor. It also reads buffers, so most edit plugins hold both grants. |
| `fs.read` | Load a file with `load-file`, or open a path with `editor-open`. The grant applies to the file's scope. |
| `fs.write` | Save a buffer with `save-buffer`. The grant applies to the file's scope. |

The grants take no argument. `editor.read` and `editor.edit` are valid, and `editor.read:project` is invalid. Files are reached through their scope, for example `project`. See [File access](/pages/file-access/).

The SDK for your language exposes the functions under `primitives/editor`. Rust uses `ilium-plugin-sdk` and TypeScript uses `@ilium/plugin-sdk`. Both need the manifest to list the grants above. The annotator also needs an entry in the manifest `exports` list. See [Plugin exports](/pages/api-exports/).

### Interface

```wit
record buffer-ref {
    buffer: buffer-id, pane: option<node-id>, path: option<fs-path>,
    language: option<string>, revision: u64, line-count: u32, dirty: bool,
}
record text-position { line: u32, column: u32 }
record text-range { start: text-position, end: text-position }
buffers: func() -> list<buffer-ref>;
buffer-for-pane: func(pane: node-id) -> option<buffer-ref>;
read-lines: func(buffer: buffer-id, first-line: u32, count: u32)
    -> result<list<string>, plugin-error>;
selections: func(buffer: buffer-id) -> list<text-range>;
record range-decoration { range: text-range, style: style, hover: option<string> }
set-decorations: func(buffer: buffer-id, layer: string, items: list<range-decoration>)
    -> result<_, plugin-error>;
record gutter-mark {
    line: u32, icon: string, tooltip: option<string>, command: option<string>,
}
set-gutter: func(buffer: buffer-id, layer: string, marks: list<gutter-mark>)
    -> result<_, plugin-error>;
create-buffer: func(initial: string, language: option<string>)
    -> result<buffer-id, plugin-error>;
load-file: func(path: fs-path) -> result<request-id, plugin-error>;
take-buffer: func(request: request-id) -> option<buffer-id>;
save-buffer: func(buffer: buffer-id, path: option<fs-path>)
    -> result<request-id, plugin-error>;
close-buffer: func(buffer: buffer-id);
record line-annotation {
    line: u32, first-column: u32, last-column: u32, style: style,
}
annotate: func(buffer: buffer-id, revision: u64, first-line: u32, lines: list<string>)
    -> list<line-annotation>;
```

Lines count from zero, and columns count Unicode scalar values, not bytes. A `text-range` is half open: its end is not part of the range. The `style` record is the shared one from the text model, and its colour roles are listed in [Accessibility and theming](/pages/accessibility-and-theming/).

### Functions

| Function | Rust (`primitives::editor`) | TypeScript (`primitives/editor`) | Grant | Sync or async | What it does |
|---|---|---|---|---|---|
| `buffers` | `buffers()` | `buffers()` | `editor.read` | sync | Lists the open buffers with their revisions and line counts. |
| `buffer-for-pane` | `buffer_for_pane(pane)` | `bufferForPane(pane)` | `editor.read` | sync | Returns the buffer shown in one pane, or none. |
| `read-lines` | `read_lines(buffer, first_line, count)` | `readLines(buffer, firstLine, count)` | `editor.read` | sync | Returns up to `count` lines from `first_line`. |
| `selections` | `selections(buffer)` | `selections(buffer)` | `editor.read` | sync | Returns the selections as text ranges. |
| `set-decorations` | `set_decorations(buffer, layer, &[..])` | `setDecorations(buffer, layer, [..])` | `editor.read` | sync | Replaces every decoration of one layer on a buffer. |
| `set-gutter` | `set_gutter(buffer, layer, &[..])` | `setGutter(buffer, layer, [..])` | `editor.read` | sync | Replaces every gutter mark of one layer on a buffer. |
| `create-buffer` | `create_buffer(initial, language)` | `createBuffer(initial, language?)` | `editor.edit` | sync | Creates a buffer with initial text and returns its id. |
| `load-file` | `load_file(path).await` | `loadFile(path)` | `editor.edit`, `fs.read` | async | Loads a file. The completion carries the buffer id. |
| `take-buffer` | `take_buffer(request)` | `takeBuffer(request)` | none | sync | Reads the buffer id of a finished `load-file`. |
| `save-buffer` | `save_buffer(buffer, path).await` | `saveBuffer(buffer, path?)` | `editor.edit`, `fs.write` | async | Writes a buffer to its path, or to a new path. |
| `close-buffer` | `close_buffer(buffer)` | `closeBuffer(buffer)` | `editor.edit` | sync | Closes a buffer and frees its slot in the quota. |

Fallible sync functions return `Result<T, PluginError>` in Rust. In TypeScript they return the value or throw a `PluginError`. The infallible editor reads `buffers()`, `buffer-for-pane()` and `selections()` have no result: they return the list or option directly, and with `editor.read` declined or revoked they return an empty list or none (see the edge cases). The async functions return a request in the raw form, and the wrappers take the buffer at once.

Decorations and gutter marks belong to the plugin. Each layer is named by the plugin, and a call on one layer never touches another layer or another plugin's marks. The host clears every layer of a plugin when the plugin stops.

### Events and intents

| Item | Payload | Grant | Delivery |
|---|---|---|---|
| `editor-opened`, `editor-closed` | `buffer-ref` | `editor.read` | Never dropped. |
| `editor-changed` | `buffer`, `revision`, `first-line`, `last-line` | `editor.read` | Coalesced per buffer, debounced 100 ms. |
| `editor-saved` | `buffer`, `path` | `editor.read` | Never dropped. |
| `editor-apply-edits` intent | `buffer`, `base-revision`, `edits` (`range`, `replacement`) | `editor.edit` | Outcome `ok`, or `refused(conflict)` when the revision is stale. |
| `editor-open` intent | `path`, `line`, `placement` (`new-pane`, `split`, `replace`) | `editor.edit`, `fs.read` | Outcome `ok-node`, the pane that opened. |

An edit names the revision that the plugin read. The kernel applies all edits of one submission as one change, and the revision rises by one. Edits in one submission must not overlap. Read the buffer again after a `conflict`, decide whether the change is still needed, and submit again. See [Editor edit](/pages/capability-editor-edit/) for the full rules.

### Annotator export

The annotator is an optional export of the client side. It has one method, `annotate`, which the kernel calls for the visible lines of a buffer. It runs at most once every 100 ms per buffer, and a late result for an old revision is dropped. It needs `editor.read`. A call that breaches its time budget is dropped, and three breaches in a row trap the plugin. See [Plugin exports](/pages/api-exports/).

### Worked example in Rust

This client plugin highlights every `TODO` word in the visible lines through its annotator. It also puts a gutter mark on each line that contains `TODO`. The mark runs `example.todo/open` when clicked. The plugin needs the grant `editor.read` and the manifest entries `exports = ["annotator"]` and a command with the id `example.todo/open`.

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::editor::{self, GutterMark, LineAnnotation};
use ilium_plugin_sdk::types::ids::BufferId;
use ilium_plugin_sdk::types::text::ColorRole;

const TODO_LAYER: &str = "todo-marks";
const OPEN_COMMAND: &str = "example.todo/open";
const MAX_LINES: u32 = 10_000;

#[ilium_plugin(side = "client", exports = ["annotator"])]
#[derive(Default)]
struct TodoMarks;

impl Lifecycle for TodoMarks {}

impl Handler for TodoMarks {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::EditorChanged(change) = event else { continue };
            let Some(buffer) = editor::buffers()
                .into_iter()
                .find(|candidate| candidate.buffer == change.buffer)
            else {
                continue;
            };
            // set-gutter replaces the whole layer, so every mark of the buffer is sent.
            let count = buffer.line_count.min(MAX_LINES);
            let Ok(lines) = editor::read_lines(change.buffer, 0, count) else { continue };
            let marks = todo_marks(&lines);
            let _ = editor::set_gutter(change.buffer, TODO_LAYER, &marks);
        }
        Vec::new()
    }
}

fn todo_marks(lines: &[String]) -> Vec<GutterMark> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, text)| text.contains("TODO"))
        .map(|(index, _)| GutterMark {
            line: index as u32,
            icon: "list-check".into(),
            tooltip: Some("Open the TODO issue".into()),
            command: Some(OPEN_COMMAND.into()),
        })
        .collect()
}

impl Annotator for TodoMarks {
    fn annotate(
        &mut self,
        _buffer: BufferId,
        _revision: u64,
        first_line: u32,
        lines: Vec<String>,
    ) -> Vec<LineAnnotation> {
        lines
            .iter()
            .enumerate()
            .filter_map(|(index, text)| {
                let byte_index = text.find("TODO")?;
                let column = text[..byte_index].chars().count() as u32;
                Some(LineAnnotation {
                    line: first_line + index as u32,
                    first_column: column,
                    last_column: column + "TODO".chars().count() as u32,
                    style: Style::role(ColorRole::Accent),
                })
            })
            .collect()
    }
}
```

### Worked example in TypeScript

This client plugin handles a command that summarises the selections of the buffer where it ran. It writes the summary into a new buffer and highlights the selected ranges in the source buffer. The plugin needs `editor.read` and `editor.edit`, and the command `example.selections/summarise` in its manifest.

```ts
import { definePlugin, PluginError } from '@ilium/plugin-sdk';
import {
  createBuffer, readLines, selections, setDecorations, type RangeDecoration,
} from '@ilium/plugin-sdk/primitives/editor';
import { role, type Event, type Submission } from '@ilium/plugin-sdk/types';

const SUMMARY_COMMAND = 'example.selections/summarise';
const SELECTION_LAYER = 'selection-summary';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {},
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'command-invoked' || event.val.command !== SUMMARY_COMMAND) continue;
      const buffer = event.val.context.buffer;
      if (buffer !== undefined) summarise(buffer);
    }
    return [];
  },
});

function summarise(buffer: bigint): void {
  const ranges = selections(buffer);
  if (ranges.length === 0) return;
  try {
    const summaryLines = ranges.map((range) => {
      const [text = ''] = readLines(buffer, range.start.line, 1);
      return `line ${range.start.line + 1}: ${text.trim()}`;
    });
    createBuffer(`${summaryLines.join('\n')}\n`);
    const decorations: RangeDecoration[] = ranges.map((range) => ({
      range,
      style: { fg: role('accent') },
      hover: 'Included in the selection summary',
    }));
    setDecorations(buffer, SELECTION_LAYER, decorations);
  } catch (error) {
    // not-found means the buffer closed during the call. Other errors are not handled here.
    if (!(error instanceof PluginError && error.code === 'not-found')) throw error;
  }
}
```

### What the user sees

The annotator colours the matched words in the editor, and the gutter marks sit beside the matched lines. Both are drawn in the editor pane, not in the plugin's own view.

<figure>
<pre class="term">
<span class="t-dim"> 41 </span>  fn load(path: &amp;Path) -&gt; Notes {
<span class="t-hi"> 42 </span>● <span class="t-warn">// TODO</span> handle
<span class="t-dim"> 43 </span>      let text = fs::read(path)?;
<span class="t-dim"> 44 </span>      parse(&amp;text)
<span class="t-dim"> 45 </span>  }
</pre>
<figcaption>Illustrative, not generated. An editor pane with a TODO highlight and a gutter mark
on line 42, in the dark theme. The mark on line 42 opens the issue when clicked.</figcaption>
</figure>

The summary command in the TypeScript example leaves a new buffer that the user can save or close:

<figure>
<pre class="term">
<span class="t-hi">line 42:</span> <span class="t-warn">// TODO</span> handle
<span class="t-hi">line 44:</span> parse(&amp;text)
</pre>
<figcaption>Illustrative, not generated. The buffer created by the summary command. The
selected ranges in the source buffer carry the accent colour and a hover text.</figcaption>
</figure>

### Diagrams

<figure>
<svg viewBox="0 0 400 140" role="img" aria-label="Annotator and gutter paths for one buffer">
  <defs>
    <marker id="arrow-annotate" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7"
            markerHeight="7" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="10" width="110" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="65" y="32" font-size="12" text-anchor="middle" fill="currentColor">Buffer lines</text>
  <line x1="120" y1="27" x2="158" y2="27" stroke="currentColor" marker-end="url(#arrow-annotate)"/>
  <rect x="160" y="10" width="110" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="215" y="32" font-size="12" text-anchor="middle" fill="currentColor">annotate()</text>
  <line x1="270" y1="27" x2="288" y2="27" stroke="currentColor" marker-end="url(#arrow-annotate)"/>
  <rect x="290" y="10" width="110" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="345" y="32" font-size="12" text-anchor="middle" fill="currentColor">Highlight drawn</text>
  <rect x="10" y="70" width="120" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="70" y="92" font-size="12" text-anchor="middle" fill="currentColor">editor-changed</text>
  <line x1="130" y1="87" x2="158" y2="87" stroke="currentColor" marker-end="url(#arrow-annotate)"/>
  <rect x="160" y="70" width="110" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="215" y="92" font-size="12" text-anchor="middle" fill="currentColor">set-gutter()</text>
  <line x1="270" y1="87" x2="288" y2="87" stroke="currentColor" marker-end="url(#arrow-annotate)"/>
  <rect x="290" y="70" width="110" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="345" y="92" font-size="12" text-anchor="middle" fill="currentColor">Gutter icon</text>
  <text x="10" y="130" font-size="11" fill="currentColor">Top: annotator. Bottom: the handler marks the gutter.</text>
</svg>
<figcaption>The annotator runs on the visible lines, and the handler sets the gutter after each
change to the buffer.</figcaption>
</figure>

<figure>
<svg viewBox="0 0 340 130" role="img" aria-label="Edit path with base revision check">
  <defs>
    <marker id="arrow-edit" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7"
            markerHeight="7" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="8" y="8" width="130" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="73" y="30" font-size="12" text-anchor="middle" fill="currentColor">Plugin submits</text>
  <line x1="138" y1="26" x2="180" y2="26" stroke="currentColor" marker-end="url(#arrow-edit)"/>
  <rect x="182" y="8" width="150" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="257" y="30" font-size="12" text-anchor="middle" fill="currentColor">Compare revision</text>
  <line x1="257" y1="44" x2="257" y2="70" stroke="currentColor" marker-end="url(#arrow-edit)"/>
  <text x="263" y="62" font-size="10" fill="currentColor">stale</text>
  <rect x="182" y="72" width="150" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="257" y="94" font-size="12" text-anchor="middle" fill="currentColor">Refused: conflict</text>
  <text x="8" y="124" font-size="11" fill="currentColor">A matching revision applies the edits and returns ok.</text>
</svg>
<figcaption>A plugin never edits a buffer it has not just read.</figcaption>
</figure>

### Edge cases and errors

| Situation | Result |
|---|---|
| A sync call without its grant | `not-permitted` at once. |
| `buffers()`, `buffer-for-pane()` or `selections()` with `editor.read` declined or revoked | An empty list or none, and one refusal line in the plugin log. These calls have no result. |
| `read-lines`, `set-decorations` or `set-gutter` with the grant declined or revoked | `not-permitted`. |
| `read-lines` with a first line past the end of the buffer | `invalid-argument`. |
| `read-lines` with a count past the last line | The lines that exist, with no error. |
| `read-lines` with a count over 10,000 | `invalid-argument`. |
| `set-decorations` or `set-gutter` with over 10,000 items in one layer | `invalid-argument`. |
| A buffer id that does not exist or has closed | `not-found`. |
| `create-buffer` with more than 16 MiB of text | `invalid-argument`. |
| The 65th buffer held by one plugin | `quota-exceeded`. Closing a buffer frees its slot. |
| `load-file` with a binary file or a file over 16 MiB | `failed(invalid-argument)` in the completion. |
| `load-file` with a missing file | `failed(not-found)` in the completion. |
| `save-buffer` with `path: none` on a buffer without a path | `invalid-argument` at once. |
| `save-buffer` when the file changed on disk after loading | `failed(conflict)` in the completion. |
| `editor-apply-edits` with a stale base revision | `refused(conflict)`. No edit is applied. |
| `editor-apply-edits` with overlapping or out-of-range edits | `refused(invalid-argument)`. |

A `load-file` completes once the buffer is open. If the plugin does not call `take-buffer` within 30 seconds, the buffer stays open and the plugin finds it again with `buffers()`. Buffers stay open when a plugin stops, is disabled or is restarted. The editor keeps their text.

### Limits

| Limit | Value | Error |
|---|---|---|
| Lines per `read-lines` call | 10,000 | `invalid-argument` |
| Decorations or gutter marks per layer | 10,000 | `invalid-argument` |
| Buffers created or held per plugin | 64 | `quota-exceeded` |
| Text in `create-buffer` | 16 MiB | `invalid-argument` |
| Edits in one `editor-apply-edits` | 10,000 | `refused(invalid-argument)` |
| Annotator calls per buffer | once per 100 ms | Extra calls are not made. |

### Viewport, pointer and tokens

The viewport, the `editor-pointer` and `editor-viewport-changed` events, minimap marks and the `token-provider` export are described in [Editor viewport and tokens](/pages/primitives-editor-viewport/). They need only `editor.read`.

<pre class="term">╭──≡ ● · main.rs───────────────────────────────────────╮
│fn main() {                                       <span class="t-dim">·</span>   │
│    <span class="t-err">oops();</span>                                     <span class="t-err">≡</span>     │
│}                                                 <span class="t-dim">·</span>   │
╰──────────────────────────────────────────────────────╯
</pre>

Requirement ids for this page: [PRI-020], [PRI-021], [PRI-022], [PRI-023], [PRI-024]. See the [requirements index](/pages/reference-requirements/).

### See also

- [Editor viewport and tokens](/pages/primitives-editor-viewport/) for the viewport, pointer events, minimap marks and token provider.

- [Plugin exports](/pages/api-exports/) for the `annotator` export and its call budget.
- [Editor read](/pages/capability-editor-read/) for the grant, events and limits in full.
- [Editor edit](/pages/capability-editor-edit/) for buffers, saving and the edit intent.
- [Commands](/pages/api-commands/) for the command that a gutter mark runs.
- [Errors and limits](/pages/reference-errors-and-limits/) for the error codes used here.
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/) for the import paths.

Checked against `ilium:plugin@0.1.0`.
