# editor.edit

## Overview

The `editor.edit` grant lets a client-side plugin change text in the Ilium editor. With it, a plugin can create an in-memory buffer, load a file into a buffer, save a buffer to a file, close a buffer, apply text edits to an open buffer and open a file in an editor pane.

Every edit names the revision of the buffer that the plugin read. Each buffer keeps a revision number that rises with every change, whether the user types or a plugin's edit lands. When the buffer has moved on, the kernel refuses the whole edit with `conflict` and applies none of it. The plugin then reads the buffer again, works out the edit again and submits it against the new revision. An edit made against old text therefore never lands in the wrong place.

## What you need

- The grant `editor.edit`, on the client side only. Server-side plugins cannot hold it.
- The grant [`editor.read`](/pages/capability-editor-read/). The plugin reads the buffer list, the revision and the lines before it edits. The revision comes only from a `buffer-ref`, which needs `editor.read`.
- `fs.read` on the file's scope for `load-file`, and for the path of `editor-open`. `fs.write` on the file's scope for `save-buffer`. Scopes are named in the path, for example `project`. See [File access](/pages/file-access/).
- The SDK for your language: `ilium-plugin-sdk` for Rust, `@ilium/plugin-sdk` for TypeScript. Both expose the editor functions under `primitives/editor`.

The grant takes no argument. `editor.edit` is valid. `editor.edit:project` is invalid, because files are reached through their file scope, not through this grant.

## The surface

The primitive functions belong to the editor interface, `ilium:primitives/editor`. The table lists every function this grant covers, with its SDK form.

| Function | Rust (`primitives::editor`) | TypeScript (`primitives/editor`) | Result |
|---|---|---|---|
| `create-buffer(initial, language)` | `create_buffer(initial: &str, language: Option<&str>)` | `createBuffer(initial: string, language?: string)` | `Result<BufferId, PluginError>`; `bigint`, throws |
| `load-file(path)` | `load_file(path).await`; raw `load_file_request(path)` | `loadFile(path)` | `Result<BufferId, PluginError>`; `Promise<bigint>` |
| `take-buffer(request)` | `take_buffer(request)` (raw, re-exported) | `takeBuffer(request)` | `Option<BufferId>`; `bigint \| undefined` |
| `save-buffer(buffer, path)` | `save_buffer(buffer, path: Option<FsPath>).await` | `saveBuffer(buffer, path?)` | `Result<(), PluginError>`; `Promise<void>` |
| `close-buffer(buffer)` | `close_buffer(buffer)` | `closeBuffer(buffer)` | nothing |

`load-file` and `save-buffer` are slow calls. Their Rust `async` wrappers wait for the completion and return its result. `load-file` takes the buffer from the completion for you, so the plugin does not call `take-buffer` itself. The raw `_request` form returns a `RequestId` and leaves the rest to the plugin.

The edit intents are submitted, not called. They are listed in the next table with their outcomes.

| Intent | Payload | Outcomes |
|---|---|---|
| `editor-apply-edits` | `buffer: buffer-id`, `base-revision: u64`, `edits: list<text-edit>` | `ok`; `refused(not-found)`, `refused(conflict)`, `refused(invalid-argument)` |
| `editor-open` | `path: fs-path`, `line: option<u32>`, `placement: editor-placement` | `ok-node(node-id)`, the pane that opened; `refused(not-permitted)`, `failed(not-found)`, `failed(io)` |

The payload records are defined in `ilium:types/editing` and in the `editor-placement` enum:

```wit
record text-position { line: u32, column: u32 }
record text-range { start: text-position, end: text-position }
record text-edit { range: text-range, replacement: string }
record editor-apply-edits-spec {
  buffer: buffer-id, base-revision: u64, edits: list<text-edit>,
}
enum editor-placement { new-pane, split, replace }
record editor-open-spec {
  path: fs-path, line: option<u32>, placement: editor-placement,
}
```

`line` and `column` are 0-based. `column` counts Unicode scalar values, not bytes or UTF-16 units. The user interface shows line numbers from 1, so a plugin converts with `line - 1` when it reads a number from the user. A range with `start` equal to `end` is an insertion. In Rust the placement is `EditorPlacement::{NewPane, Split, Replace}`. In TypeScript it is the string `'new-pane'`, `'split'` or `'replace'`.

### Events that need `editor.read`

The grant `editor.edit` does not deliver events. A plugin that wants to follow buffer changes also needs `editor.read`.

| Event | Payload | Delivery |
|---|---|---|
| `editor-opened`, `editor-closed` | `buffer-ref` | never dropped |
| `editor-changed` | `buffer`, `revision`, `first-line`, `last-line` | coalesced per buffer, debounced 100 ms |
| `editor-saved` | `buffer`, `path` | never dropped |

A `buffer-ref` carries `buffer`, `pane`, `path`, `language`, `revision`, `line-count` and `dirty`. The same record comes from `buffers()`.

## The stale revision rule

<figure>
<svg viewBox="0 0 360 260" role="img" aria-label="A stale edit is refused, and the plugin retries with the new revision">
<title>An edit against an old revision</title>
<desc>The plugin reads revision 17. The user types, so the buffer moves to revision 18. The plugin submits its edit with base revision 17. The kernel refuses it with a conflict. The plugin reads again and retries with revision 18.</desc>
<defs>
<marker id="arr-editor-edit" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
<path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="10" y="8" width="120" height="28" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="70" y="26" text-anchor="middle" font-size="11" fill="currentColor">Plugin</text>
<rect x="230" y="8" width="120" height="28" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="290" y="26" text-anchor="middle" font-size="11" fill="currentColor">Kernel buffer</text>
<line x1="70" y1="36" x2="70" y2="250" stroke="currentColor" stroke-dasharray="2 4"/>
<line x1="290" y1="36" x2="290" y2="250" stroke="currentColor" stroke-dasharray="2 4"/>
<line x1="70" y1="62" x2="290" y2="62" stroke="currentColor" marker-end="url(#arr-editor-edit)"/>
<text x="180" y="56" text-anchor="middle" font-size="10" fill="currentColor">read buffer: revision 17</text>
<text x="180" y="108" text-anchor="middle" font-size="10" fill="currentColor">the user types: revision 18</text>
<line x1="70" y1="142" x2="290" y2="142" stroke="currentColor" marker-end="url(#arr-editor-edit)"/>
<text x="180" y="136" text-anchor="middle" font-size="10" fill="currentColor">edit with base-revision 17</text>
<line x1="290" y1="182" x2="70" y2="182" stroke="currentColor" stroke-dasharray="3 3" marker-end="url(#arr-editor-edit)"/>
<text x="180" y="176" text-anchor="middle" font-size="10" fill="currentColor">refused: conflict</text>
<line x1="70" y1="220" x2="290" y2="220" stroke="currentColor" marker-end="url(#arr-editor-edit)"/>
<text x="180" y="214" text-anchor="middle" font-size="10" fill="currentColor">read again, edit with base-revision 18</text>
</svg>
<figcaption>A stale edit is refused as a whole. The plugin reads the buffer again and submits the edit against the current revision.</figcaption>
</figure>

The rule has three parts:

- All edits in one `editor-apply-edits` refer to the text at `base-revision`. They must not overlap. Overlapping edits give `refused(invalid-argument)`.
- The kernel applies the edits of one submission as one change, so the revision rises by one, not once per edit.
- The only code for a stale revision is `conflict`. The plugin decides whether to retry. The safe retry reads the buffer again, checks whether the change is still needed, and submits again.

## Worked example in Rust

This client-side plugin adds a heading to the top of `NOTES.md` when the user runs its command. It reads the buffer list, skips the edit when the heading is already there, and retries when a submission comes back as `conflict`. The manifest grants `editor.read` and `editor.edit` on the client side.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::editor;
use ilium_plugin_sdk::types::editing::{BufferRef, TextEdit, TextPosition, TextRange};
use ilium_plugin_sdk::types::intents::EditorApplyEditsSpec;

const STAMP_COMMAND: &str = "com.example.notes/stamp-heading";
const HEADING: &str = "# Notes";

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct NotesStamp;

impl Lifecycle for NotesStamp {}

impl Handler for NotesStamp {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::CommandInvoked(invoked) if invoked.command == STAMP_COMMAND => {
                    task::spawn(stamp_heading());
                }
                Event::RequestCompleted(result) => {
                    // Only a stale revision is retried; the retry reads the new revision.
                    if let Outcome::Refused(error) = result.outcome {
                        if error.code == ErrorCode::Conflict {
                            task::spawn(stamp_heading());
                        }
                    }
                }
                _ => {}
            }
        }
        Vec::new()
    }
}

async fn stamp_heading() {
    let buffers = editor::buffers();
    let Some(target) = buffers.into_iter().find(is_notes_buffer) else { return };
    let Ok(first_lines) = editor::read_lines(target.buffer, 0, 1) else { return };
    if first_lines.first().map(String::as_str) == Some(HEADING) {
        return;
    }
    let spec = EditorApplyEditsSpec {
        buffer: target.buffer,
        base_revision: target.revision,
        edits: vec![TextEdit {
            range: TextRange {
                start: TextPosition { line: 0, column: 0 },
                end: TextPosition { line: 0, column: 0 },
            },
            replacement: format!("{HEADING}\n"),
        }],
    };
    task::submit(Submission::new(Intent::EditorApplyEdits(spec)));
}

fn is_notes_buffer(buffer: &BufferRef) -> bool {
    buffer
        .path
        .as_ref()
        .is_some_and(|path| path.scope == "project" && path.path == "NOTES.md")
}
```

`task::submit` appends the submission to the batch that the handler returns. The outcome comes back as a `request-completed` event. The handler checks the outcome, and only a `conflict` starts another attempt.

To create a buffer that a code view can show, call the SDK function directly:

**Rust**

```rust
use ilium_plugin_sdk::types::ids::BufferId;

let buffer: BufferId = editor::create_buffer("fn main() {}\n", Some("rust"))?;
```

## Worked example in TypeScript

This client-side plugin loads `NOTES.md` when it activates, keeps the buffer id, and submits the same heading edit when its command runs. The SDK drains the promise queue before each export returns, so the result of `loadFile` is in place before the next `onEvents` call.

**TypeScript**

```ts
import { definePlugin, PluginError, type Event, type Submission } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';
import { buffers, loadFile } from '@ilium/plugin-sdk/primitives/editor';
import type { FsPath } from '@ilium/plugin-sdk/types';

const STAMP_COMMAND = 'com.example.notes/stamp-heading';
const NOTES_PATH: FsPath = { scope: 'project', path: 'NOTES.md' };
const HEADING = '# Notes\n';

let notesBuffer: bigint | undefined;
let nextCorrelation = 1n;

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {
    loadFile(NOTES_PATH)
      .then((buffer) => {
        notesBuffer = buffer;
      })
      .catch((error: unknown) => {
        if (error instanceof PluginError) {
          log('warn', `load NOTES.md failed: ${error.code}: ${error.message}`);
        }
      });
  },
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'command-invoked' || event.val.command !== STAMP_COMMAND) continue;
      const submission = stampHeading();
      if (submission) submissions.push(submission);
    }
    return submissions;
  },
});

function stampHeading(): Submission | undefined {
  if (notesBuffer === undefined) return undefined;
  const target = buffers().find((candidate) => candidate.buffer === notesBuffer);
  if (!target) return undefined;
  const correlation = nextCorrelation;
  nextCorrelation += 1n;
  return {
    intent: {
      tag: 'editor-apply-edits',
      val: {
        buffer: target.buffer,
        baseRevision: target.revision,
        edits: [
          {
            range: { start: { line: 0, column: 0 }, end: { line: 0, column: 0 } },
            replacement: HEADING,
          },
        ],
      },
    },
    correlation,
    dedupKey: undefined,
  };
}
```

`buffers()` returns `buffer-ref` records, so `revision` is a `bigint` and matches `baseRevision` without conversion. Correlation numbers below 2^63 belong to the plugin, which is why the counter starts at 1.

## Edge cases and errors

Synchronous checks fail at the call. Checks that need the disk, the user or the editor's apply step come back in the completion as `failed(code)`.

| Situation | Result |
|---|---|
| Grant declined, or revoked during the call | `not-permitted` at once; `revoked` for a call in flight, then re-activation |
| `create-buffer` with more than 16 MiB of text | `invalid-argument` |
| 65th buffer held by one plugin (64 allowed) | `quota-exceeded` |
| `load-file` with a bad path | `invalid-argument` at once |
| `load-file`, file missing | `failed(not-found)` in the completion |
| `load-file`, binary file or over 16 MiB | `failed(invalid-argument)` in the completion |
| `save-buffer` with `path: none` on a buffer without a path | `invalid-argument` at once |
| `save-buffer`, file changed on disk since it was loaded | `failed(conflict)` in the completion |
| `save-buffer`, no `fs.write` for the scope | `not-permitted` at once |
| `editor-apply-edits`, stale `base-revision` | `refused(conflict)`; no edit applied |
| `editor-apply-edits`, overlapping or out-of-range edits | `refused(invalid-argument)`; no edit applied |
| `editor-apply-edits`, more than 10,000 edits | `refused(invalid-argument)` |
| `editor-apply-edits`, unknown or closed buffer | `refused(not-found)` |
| `editor-apply-edits`, buffer closed after acceptance | `failed(not-found)` |
| `editor-apply-edits`, empty `edits` list | `ok`; the revision does not change |
| `editor-open`, path without `fs.read` | `refused(not-permitted)` |
| `editor-open`, file missing or unreadable | `failed(not-found)` or `failed(io)` |

A plugin that saves right after an edit waits for the edit's `ok` outcome before it calls `save-buffer`. The save then writes the edited text, and the `editor-saved` event follows it.

A language name that the editor does not know opens the buffer as plain text. That is not an error. Use lowercase names, as the editor reports them in `buffer-ref.language`.

## Buffer lifetime

Buffers stay open when the plugin stops, is disabled or is re-activated. The editor keeps their text, because the text belongs to the user. A re-activated plugin finds its buffers again with `buffers()`. Closing a buffer with `close-buffer` frees its slot in the 64-buffer quota.

A completed `load-file` is held for 30 seconds for `take-buffer`. If the plugin takes the buffer later than that, the buffer is still open, and `buffers()` finds it.

## Risk and tier rules

- This grant is normal risk, not high risk.
- A T1 bundled plugin receives it automatically up to its manifest ceiling.
- A T2 or T3 plugin shows the grant in the review dialog, listed and checked by default. The user's choice is remembered.
- An edit changes the user's text. A plugin should edit only for an action the user asked for, and should show the change in the editor, not hide it.

## What the user sees

The review dialog lists the grant when the plugin is installed. If the user declines it, the editor works as before, and every call of this grant returns `not-permitted`. A code view or file explorer that needs to open files should then show that opening is unavailable.

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.editor 0.1.0 by Example Ltd                                    │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ [x] Edits open files and buffers <span class="t-dim">……………………………………………………………………………</span> editor.edit │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests editor.edit, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

After the stamp command runs, the heading appears on line 1 of the buffer, and the revision has risen by one. The user sees the change in the editor like any other edit:

<figure>
<pre class="term"><span class="t-hi">╭─ NOTES.md (project) ───────────────────────────────── revision 18 ─╮</span>
│ 1 <span class="t-ok"># Notes</span>                                                          │
│ 2 Ideas for the next release.                                      │
│ 3 <span class="t-dim">Open questions for the design review.</span>                            │
<span class="t-hi">╰────────────────────────────────────────────────────────────────────╯</span>
</pre>
<figcaption>Illustrative, not generated. The heading line added by the edit, with the buffer revision after it.</figcaption>
</figure>

A code view that shows a buffer the plugin created with `create-buffer` needs `editor.edit` only. Showing any other buffer also needs `editor.read`. Without the grant, the widget draws an empty frame with the text `not permitted`.

## Manifest

The manifest requests the grant on the client side, together with the read grant that gives the revision:

```json
{
  "capabilities": {
    "client": ["editor.read", "editor.edit"]
  }
}
```

## See also

- [editor.read](/pages/capability-editor-read/): reading buffers, lines and the revision.
- [Editor primitive](/pages/primitives-editor/): the full editor interface.
- [Intents](/pages/api-intents/): how submissions and their outcomes work.
- [Editor provider](/pages/capability-editor-provide/): replacing the built-in editor after v1.
- [File access](/pages/file-access/) and [fs.read](/pages/capability-fs-read/): the file scopes that load and open calls need.
- [fs.write](/pages/capability-fs-write/): the write grant that `save-buffer` needs.
- [Code view](/pages/widgets-media-and-embedded/): showing a buffer in a pane.
- [Errors and limits](/pages/errors-and-limits/): the error codes and caps used on this page.
- [Tutorial 05: TODO scanner](/pages/tutorial-05-todo-scanner/): opening files with `editor-open`.
