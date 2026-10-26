# editor.provide

<div class="warn">
<p>Status: reserved, not available in `ilium:plugin@0.1.0`. A package that names `editor.provide` is refused with `manifest.grant-unknown`, and the interface `editor-provider` is not in the [WIT reference](/pages/reference-wit/). The editor is a built-in pane kind. A plugin that needs its own text pane uses the `basic` primitive with `panes.provide-kind`. The rest of this page describes the design planned for a later version.</p>
</div>

The `editor.provide` grant lets a client-side plugin replace the native editor. The plugin exports the `editor-provider` interface, which has every function of `ilium:primitives/editor` plus two handover calls, `adopt` and `release`. When the provider starts, the native editor hands its open buffers to the plugin. While the provider runs, every other plugin's `editor` calls reach the provider instead of the native editor. When the provider stops, the buffers return to the native editor.

<div class="warn">

High risk. The grant is high risk (H) and for T1 bundled plugins only. By default the native editor serves `ilium:primitives/editor`, and no other feature depends on this grant.

</div>

<figure>
<svg viewBox="0 0 420 180" role="img" aria-label="In v1 the native editor serves the editor interface to the editor panes. A T1 provider plugin would serve it instead, with the same interface.">
  <defs>
    <marker id="arr-provide" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <text x="8" y="16" fill="currentColor" font-size="11">In v1: the native editor serves the interface</text>
  <rect x="8" y="26" width="120" height="50" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="68" y="48" text-anchor="middle" fill="currentColor" font-size="12">Native editor</text>
  <text x="68" y="64" text-anchor="middle" fill="currentColor" font-size="10">v1 kernel</text>
  <line x1="130" y1="51" x2="298" y2="51" stroke="currentColor" marker-end="url(#arr-provide)"/>
  <text x="214" y="44" text-anchor="middle" fill="currentColor" font-size="10">ilium:primitives/editor</text>
  <rect x="300" y="26" width="112" height="50" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="356" y="55" text-anchor="middle" fill="currentColor" font-size="12">Editor panes</text>
  <text x="8" y="110" fill="currentColor" font-size="11">With editor.provide: a provider plugin serves it</text>
  <rect x="8" y="120" width="120" height="50" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="68" y="142" text-anchor="middle" fill="currentColor" font-size="12">Provider plugin</text>
  <text x="68" y="158" text-anchor="middle" fill="currentColor" font-size="10">T1 only</text>
  <line x1="130" y1="145" x2="298" y2="145" stroke="currentColor" marker-end="url(#arr-provide)"/>
  <text x="214" y="138" text-anchor="middle" fill="currentColor" font-size="10">ilium:primitives/editor</text>
  <rect x="300" y="120" width="112" height="50" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="356" y="149" text-anchor="middle" fill="currentColor" font-size="12">Editor panes</text>
</svg>
<figcaption>In v1 the native editor serves the interface. With editor.provide, a T1 plugin serves the same interface to the editor panes.</figcaption>
</figure>

## Overview

A provider is a client-side plugin that owns the text of every open buffer while it runs. The native editor keeps no copy of the text during that time. The provider answers every editor call, so decorations, gutter marks, edits and file loads all go through the provider. The kernel still checks grants on each call. The buffers stay the user's work: a provider that stops or traps must not silently lose them (see the lifecycle below).

Only one editor provider runs at a time.

## You will need

- A T1 bundled plugin. A T2 or T3 package gets `manifest.grant-tier` at install.
- The API string `ilium:plugin@0.1.0` in the manifest `api` key.
- A client-side build. A grant or export on the server side gives `manifest.grant-side`.
- The calls that the provider implements are described on [editor.read](/pages/capability-editor-read/) and [editor.edit](/pages/capability-editor-edit/). The provider needs no extra grant for them, because it is the implementation.
- `fs.read` and `fs.write` only if the provider reads or writes files itself, for `load-file` and `save-buffer`.

## Grant and export syntax

- Valid grant: `editor.provide`.
- Invalid: `editor.provide:code-view`. The grant takes no argument, so the install error is `manifest.grant-argument`: `capability "editor.provide" takes no argument (got "editor.provide:code-view")`.
- Manifest export: `"editor-provider"` in the client `exports` list. Listing it without the grant gives `manifest.export-grant`.

```json
{
  "api": "ilium:plugin@0.1.0",
  "capabilities": { "client": ["editor.provide"] },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": ["editor-provider"]
  }
}
```

## Full surface

| Name | Kind | Signature (WIT) | Meaning |
|---|---|---|---|
| `adopt` | export | `adopt(buffers: list<adopted-buffer>)` | Receives every open buffer at start. Returns nothing. |
| `release` | export | `release() -> list<adopted-buffer>` | Returns every buffer at stop, with its current text. |
| `apply-edits` | export | `(buffer, base-revision: u64, edits: list<text-edit>) -> result<u64, plugin-error>` | Answers the intent `editor-apply-edits`. Returns the new revision. |
| `buffers` | export | `() -> list<buffer-ref>` | As `ilium:primitives/editor`. |
| `buffer-for-pane` | export | `(pane: node-id) -> option<buffer-ref>` | As `ilium:primitives/editor`. |
| `read-lines` | export | `(buffer, first-line: u32, count: u32) -> result<list<string>, plugin-error>` | As the editor primitive. At most 10,000 lines per call. |
| `selections` | export | `(buffer) -> list<text-range>` | As the editor primitive. |
| `set-decorations` | export | `(buffer, layer: string, items: list<range-decoration>) -> result<_, plugin-error>` | As the editor primitive. |
| `set-gutter` | export | `(buffer, layer: string, marks: list<gutter-mark>) -> result<_, plugin-error>` | As the editor primitive. |
| `create-buffer` | export | `(initial: string, language: option<string>) -> result<buffer-id, plugin-error>` | As the editor primitive. |
| `load-file` | export | `(path: fs-path) -> result<request-id, plugin-error>` | As the editor primitive. Also answers the intent `editor-open`. |
| `take-buffer` | export | `(request: request-id) -> option<buffer-id>` | As the editor primitive. |
| `save-buffer` | export | `(buffer, path: option<fs-path>) -> result<request-id, plugin-error>` | As the editor primitive. |
| `close-buffer` | export | `(buffer)` | As the editor primitive. |

The record `adopted-buffer` has the fields `buffer: buffer-id`, `path: option<fs-path>`, `text: string`, `language: option<string>`, `cursor: text-position` and `dirty: bool`. The `buffer` id is the one the native editor issued, so a plugin that stored it keeps working after the handover. A buffer that the provider creates gets a new id from the provider's own counter, and that counter never reuses an adopted id.

The record `text-edit` has the fields `range: text-range` and `replacement: string`.

In Rust, the trait is `ilium_plugin_sdk::exports::EditorProvider`, with the same twelve methods in snake_case (`read_lines`, `apply_edits` and so on). In TypeScript, the methods sit flat in the `definePlugin` object beside `lifecycle` and `handler`, in camelCase.

## Lifecycle of a provider

1. **Grant.** A T1 plugin activates first without its H capabilities, and then the prompt appears (see below). If the user declines, the provider role does not start and the native editor keeps serving the interface.
2. **Activate.** `activate` runs under `activate_deadline_ms`: 2,000 ms by default, 5,000 ms ceiling for T1.
3. **Adopt.** The kernel calls `adopt` with every open buffer. A buffer with unsaved changes carries `dirty: true`. `adopt` runs under the same activation deadline. A trap or a missed deadline stops the provider, and the native editor keeps its buffers.
4. **Run.** Other plugins' `editor` calls reach the provider. Decorations and gutter layers stay per plugin and clear when that plugin stops.
5. **Release.** On a clean stop, `deactivate` runs, then `release` returns the buffers under `deactivate_deadline_ms`: 500 ms by default, 1,000 ms ceiling. The native editor takes them back before the final snapshot is written.
6. **Trap or restart.** A trap hands the buffers back to the native editor at the text the provider last adopted. Edits made after `adopt` and not saved are lost, and the tree footer shows a warn line that names the plugin. The provider follows the [restart and disable rule](/pages/reference-errors-and-limits/#restart-and-disable-rule): restart after 1 s, then 5 s, disabled at the third failure in 10 minutes. On restart the kernel calls `adopt` again with the buffers the native editor holds.
7. **Revocation.** Revoking `editor.provide` makes calls in flight fail with `revoked`. The plugin receives `deactivate(revoked)`, and its buffers return to the native editor (see [Editor primitives](/pages/primitives-editor/)).

## Worked example: Rust

This provider keeps the adopted buffers in memory. It implements `adopt`, `release` and `read_lines`. The other methods listed above follow the same pattern. The trait does not compile until all twelve exist.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::exports::EditorProvider;
use ilium_plugin_sdk::primitives::editor_provider::AdoptedBuffer;
use ilium_plugin_sdk::types::ids::BufferId;

#[ilium_plugin(side = "client", exports = ["editor-provider"])]
#[derive(Default)]
struct BufferKeeper {
    buffers: Vec<AdoptedBuffer>,
}

impl BufferKeeper {
    fn find(&mut self, buffer: BufferId) -> Result<&mut AdoptedBuffer, PluginError> {
        self.buffers
            .iter_mut()
            .find(|entry| entry.buffer == buffer)
            .ok_or_else(|| error(ErrorCode::NotFound, "no such buffer"))
    }
}

fn error(code: ErrorCode, message: &str) -> PluginError {
    PluginError { code, message: message.to_owned(), retry_after_ms: None }
}

impl Lifecycle for BufferKeeper {}

impl Handler for BufferKeeper {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

impl EditorProvider for BufferKeeper {
    fn adopt(&mut self, buffers: Vec<AdoptedBuffer>) {
        self.buffers = buffers;
    }

    fn release(&mut self) -> Vec<AdoptedBuffer> {
        std::mem::take(&mut self.buffers)
    }

    fn read_lines(
        &mut self,
        buffer: BufferId,
        first_line: u32,
        count: u32,
    ) -> Result<Vec<String>, PluginError> {
        if count > 10_000 {
            return Err(error(ErrorCode::InvalidArgument, "at most 10,000 lines per call"));
        }
        let entry = self.find(buffer)?;
        let line_total = entry.text.lines().count();
        if first_line as usize > line_total {
            return Err(error(ErrorCode::InvalidArgument, "line past the end of the buffer"));
        }
        let lines = entry.text.lines().skip(first_line as usize).take(count as usize);
        Ok(lines.map(str::to_owned).collect())
    }

    // buffers, buffer_for_pane, selections, set_decorations, set_gutter,
    // create_buffer, load_file, take_buffer, save_buffer, close_buffer and apply_edits
    // are implemented the same way, as listed in the table above.
}
```

## Worked example: TypeScript

The same provider in TypeScript. `definePlugin` returns the `editorProvider` export. A thrown `PluginError` becomes the `plugin-error` record that the host sees.

**TypeScript**

```ts
import { definePlugin, PluginError } from '@ilium/plugin-sdk';
import type { AdoptedBuffer } from '@ilium/plugin-sdk/primitives/editor-provider';

const buffers = new Map<bigint, AdoptedBuffer>();

function requireBuffer(buffer: bigint): AdoptedBuffer {
  const found = buffers.get(buffer);
  if (found === undefined) {
    throw new PluginError('not-found', 'no such buffer');
  }
  return found;
}

export const { lifecycle, handler, editorProvider } = definePlugin({
  side: 'client',
  activate() {},
  deactivate() { return undefined; },
  onEvents() { return []; },
  adopt(adopted: AdoptedBuffer[]) {
    buffers.clear();
    for (const entry of adopted) buffers.set(entry.buffer, entry);
  },
  release() {
    const returned = [...buffers.values()];
    buffers.clear();
    return returned;
  },
  readLines(buffer: bigint, firstLine: number, count: number) {
    if (count > 10_000) {
      throw new PluginError('invalid-argument', 'at most 10,000 lines per call');
    }
    const lines = requireBuffer(buffer).text.split('\n');
    if (firstLine > lines.length) {
      throw new PluginError('invalid-argument', 'line past the end of the buffer');
    }
    return lines.slice(firstLine, firstLine + count);
  },
  // The other editor functions follow the same pattern, in camelCase.
});
```

## Edge cases and errors

| Code | Where it comes from | Meaning |
|---|---|---|
| `not-permitted` | Any call | The grant was declined or revoked during the call. |
| `revoked` | Any call | The authorization epoch changed. The plugin is re-activated. |
| `not-found` | Buffer calls | The buffer id is unknown. |
| `invalid-argument` | `read-lines`, `apply-edits` | A line past the end of the buffer, more than 10,000 lines, or overlapping edits. |
| `conflict` | `apply-edits` | The `base-revision` is stale. Read the buffer again and retry. |
| trap | `adopt`, `release`, any call | A deadline breach, a wrong record shape or a guest panic. The instance is dropped (see [Editor primitives](/pages/primitives-editor/)). |

Install errors:

| Code | Cause |
|---|---|
| `manifest.grant-tier` | A T2 or T3 package asks for the grant. |
| `manifest.grant-side` | The grant or export is on the server side. |
| `manifest.grant-unknown` | The `api` key names an unknown API version. |
| `manifest.export-grant` | `editor-provider` is listed without the grant. |

A second provider does not start. It logs a warn line, and the user frees the role by disabling the other plugin in settings.

## What the user sees

When a T1 plugin with the grant starts for the first time, the user gets one prompt. The H capability is unticked until the user ticks it.

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.editor 0.1.0 by Example Ltd                                    │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ <span class="t-warn">! [ ] Replaces the built-in editor</span> <span class="t-dim">………………………………………………………………</span> <span class="t-warn">editor.provide</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests editor.provide, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

A T2 package that asks for the grant fails at install instead of showing a prompt:

<pre class="term"><span class="t-prompt">$</span> ilium plugin install ./editor-demo.iliumpkg
<span class="t-err">error</span> manifest.grant-tier capability "editor.provide" is available to
      bundled plugins only
</pre>

The user can check the grants of an installed plugin with `ilium plugin grants <id>`. Grants are per side and per session (see [Capabilities reference](/pages/reference-capabilities/)).

## See also

- [editor.read](/pages/capability-editor-read/) for the read calls the provider implements.
- [editor.edit](/pages/capability-editor-edit/) for the edit calls and intents.
- [Editor primitive](/pages/primitives-editor/) for the full `ilium:primitives/editor` interface.
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/) for the build setup.
- [Capabilities](/pages/capabilities/) for the risk tiers and the grant table.
- [Manifest reference](/pages/reference-manifest/) for the `exports` key.

### Security

Editor providers see file content; see [Extension security](/pages/reference-extension-security/).
