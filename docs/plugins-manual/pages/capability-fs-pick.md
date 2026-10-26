---
layout: page
title: "Capability: fs.pick"
permalink: /pages/capability-fs-pick/
---

## fs.pick

The `fs.pick` capability lets a client-side plugin open the file picker. The picker is a modal panel that the kernel draws and owns. The plugin never sees the folders the user did not choose. It receives only the path the user confirmed, as a `picked/<n>` scope.

A plugin cannot choose a file by itself. It asks for a picker, the user decides, and the plugin gets one path back. Reading or writing that path needs a second grant, `fs.read:picked` or `fs.write:picked`.

### What you need

- **Grants.** `fs.pick` in `capabilities.client`, plus `fs.read:picked` for an `open-file` or `open-directory` pick, or `fs.write:picked` for a `save-file` pick. `fs.pick` without either of them fails the install with `manifest.grant-dependency`.
- **Language.** Rust with the `ilium_plugin_sdk` crate, or TypeScript with `@ilium/plugin-sdk`. Both wrappers are `async`.
- **Event handling.** A pick completes later as a `request-completed` event. The SDK wrapper hides this, but the plugin must run inside `definePlugin` or an async task so the completion can arrive. See [Requests and outcomes](/pages/requests-and-outcomes/).

### Grant syntax

The grant takes no argument.

| Grant | Valid | Reason |
|---|---|---|
| `fs.pick` | yes | the only form |
| `fs.pick:project` | no | the grant takes no argument |
| `fs.picker` | no | the name is `fs.pick` |

Manifest excerpt for a plugin that opens a picker and then reads the chosen file:

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.notes",
  "capabilities": { "client": ["fs.pick", "fs.read:picked"] }
}
```

### Surface

| Name | Kind | Type | Meaning |
|---|---|---|---|
| `pick(spec)` | function | `pick-spec` to `result<request-id, plugin-error>` | Opens the picker. Needs `fs.pick`. Take the result with `take-picked`. |
| `take-picked(request)` | function | `request-id` to `option<fs-path>` | The chosen path, once the completion arrives. |
| `pick-spec` | record | `kind`, `title: string`, `extensions: list<string>` | Picker mode, title line and extension filter. |
| `pick-kind` | enum | `open-file`, `open-directory`, `save-file` | The mode of the picker and the button it shows. |
| `fs-path` | record | `scope: string`, `path: string` | The result. `scope` is `picked/<n>`, and `path` is empty. |
| `picked/<n>` | scope | grant per pick | Reading and writing needs `fs.read:picked` or `fs.write:picked`. |

Rust names: `fs::pick(spec).await`, `PickSpec`, `PickKind::OpenFile`, `PickKind::OpenDirectory`
and `PickKind::SaveFile`, all from `ilium_plugin_sdk::host::fs`. TypeScript names: `pick(spec)`
from `@ilium/plugin-sdk/host/fs`, with `kind` set to the strings `'open-file'`, `'open-directory'`
or `'save-file'`.

The `extensions` filter takes lower-case names without a leading dot, such as `png`. It matches
case-insensitively. An empty list allows every file. The filter applies only to file modes, not to
`open-directory`.

### How the pick flows

<figure>
<svg viewBox="0 0 400 230" role="img" aria-label="Pick flow from the plugin request through the kernel dialog to the picked path">
  <defs>
    <marker id="fsp-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="10" width="380" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="32" text-anchor="middle" font-size="11" fill="currentColor">1. Plugin calls pick(spec), gets a request-id</text>
  <line x1="200" y1="46" x2="200" y2="66" stroke="currentColor" marker-end="url(#fsp-arrow)"/>
  <rect x="10" y="66" width="380" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="200" y="88" text-anchor="middle" font-size="11" fill="currentColor">2. Kernel draws the picker panel (not the plugin)</text>
  <line x1="200" y1="102" x2="200" y2="122" stroke="currentColor" marker-end="url(#fsp-arrow)"/>
  <rect x="10" y="122" width="380" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="144" text-anchor="middle" font-size="11" fill="currentColor">3. User confirms; picked/1 scope is created</text>
  <line x1="200" y1="158" x2="200" y2="178" stroke="currentColor" marker-end="url(#fsp-arrow)"/>
  <rect x="10" y="178" width="380" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="200" text-anchor="middle" font-size="11" fill="currentColor">4. take-picked returns fs-path { picked/1, "" }</text>
</svg>
<figcaption>The dialog belongs to the kernel. The dashed box marks the step the plugin does not control.</figcaption>
</figure>

### What the user sees

The review dialog lists the grant in plain words when the package is installed. Tier T1 bundled plugins
receive `fs.pick` without a prompt. Tier T2 and T3 packages show it in the review dialog.

<figure>
<pre class="term"><span class="t-dim">Review permissions: com.example.notes 0.1.0</span>
     Opens the file picker: fs.pick
     Lets you choose a file or folder. The plugin sees only your choice.
<span class="t-dim">[allow]  [decline]</span>
</pre>

<figcaption>Illustrative, not generated. The review line for <code>fs.pick</code>.</figcaption>
</figure>

The picker itself has a title line, a path input, the entry list, the extension filter and two buttons.
The button label depends on the mode. Keys: Enter opens a folder or confirms, Backspace goes to the
parent folder, Tab moves between the input, the list and the buttons, Ctrl+H shows hidden files, and
Escape cancels.

<figure>
<pre class="term"><span class="t-hi">Choose a background image</span>
<span class="t-dim">╭  ≡ ● · Choose a background image─────────────────────────╮</span>
<span class="t-dim">│</span> Path …… /home/arthur/Pictures                            <span class="t-dim">│</span>
<span class="t-dim">│</span> <span class="t-dim">────────────────────────────────────────────────</span>         <span class="t-dim">│</span>
<span class="t-dim">│</span> <span class="t-ok">› sunrise.png</span>                                            <span class="t-dim">│</span>
<span class="t-dim">│</span>   dune.jpg                                               <span class="t-dim">│</span>
<span class="t-dim">│</span>   notes.md                  <span class="t-dim">(hidden by filter)</span>           <span class="t-dim">│</span>
<span class="t-dim">│</span> <span class="t-dim">────────────────────────────────────────────────</span>         <span class="t-dim">│</span>
<span class="t-dim">│</span> Filter …… png, jpg                                       <span class="t-dim">│</span>
<span class="t-dim">│</span>                                                          <span class="t-dim">│</span>
<span class="t-dim">│</span>  Esc Cancel      Enter Open                              <span class="t-dim">│</span>
<span class="t-dim">│</span> <span class="t-dim">Esc cancel · Enter open</span>                                  <span class="t-dim">│</span>
<span class="t-dim">╰──────────────────────────────────────────────────────────╯</span>
</pre>

<figcaption>Illustrative, not generated. The picker opened by <code>pick</code> with
<code>kind</code> set to <code>open-file</code> and <code>extensions</code> set to png and jpg.</figcaption>
</figure>

The button reads `Open` for `open-file`, `Save` for `save-file` and `Choose` for `open-directory`.
`Cancel` is always present.

### Worked example: Rust

This example asks for an image and reads it. It treats a cancelled pick as a normal outcome.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::fs::{self, PickKind, PickSpec};

/// Asks the user for a background image. Needs fs.pick, fs.read:picked.
async fn choose_background() -> Result<FsPath, PluginError> {
    let spec = PickSpec {
        kind: PickKind::OpenFile,
        title: "Choose a background image".to_string(),
        extensions: vec!["png".to_string(), "jpg".to_string()],
    };
    fs::pick(spec).await
}

/// Returns the image bytes, or None when the user cancelled the picker.
async fn load_background() -> Result<Option<Vec<u8>>, PluginError> {
    let picked_path = match choose_background().await {
        Ok(picked_path) => picked_path,
        Err(error) if error.code == ErrorCode::Cancelled => return Ok(None),
        Err(error) => return Err(error),
    };
    let bytes = fs::read_all(&picked_path).await?;
    Ok(Some(bytes))
}
```

The picked path has an empty `path`, so `read_all` with `picked_path` reads the chosen file itself.

### Worked example: TypeScript

The same flow in TypeScript. A cancelled pick rejects with a `PluginError`, which the function turns
into `undefined`.

**TypeScript**

```ts
import { PluginError } from '@ilium/plugin-sdk';
import { pick, readAll } from '@ilium/plugin-sdk/host/fs';

/** Asks the user for a background image and returns its bytes, or undefined on cancel. */
export async function loadBackground(): Promise<Uint8Array | undefined> {
  try {
    const pickedPath = await pick({
      kind: 'open-file',
      title: 'Choose a background image',
      extensions: ['png', 'jpg'],
    });
    return await readAll(pickedPath);
  } catch (error) {
    if (error instanceof PluginError && error.code === 'cancelled') {
      return undefined;
    }
    throw error;
  }
}
```

### Save-file example

A `save-file` pick returns a scope whose path is empty and names the file the user typed. The file may
not exist yet. Write it with `write` and a `create-new` mode to avoid overwriting an existing file:

**Rust**

```rust
let target = fs::pick(PickSpec {
    kind: PickKind::SaveFile,
    title: "Save the export".to_string(),
    extensions: vec!["json".to_string()],
}).await?;
fs::write(&target, export_bytes, WriteMode::CreateNew).await?;
```

This needs `fs.write:picked` in the manifest. Without it, `write` returns `not-permitted`.

### Limits and rules

| Rule | Value |
|---|---|
| Pickers open at once per plugin | 1. A second `pick` returns `busy`. |
| Picked paths kept per plugin | 64. The oldest is forgotten when a 65th is added. |
| `<n>` in `picked/<n>` | A per-plugin counter that never repeats. |
| Lifetime of a picked path | Survives restarts until its grant is revoked. |
| Watching a picked scope | Allowed for folders only. |

Paths inside a picked folder are relative to it, as for every scope. A path that leaves the folder
through `..` or a symbolic link returns `not-permitted`.

### Errors

Every error code of `plugin-error` can occur. These are the ones that matter for a picker:

| Code | When | Where it appears |
|---|---|---|
| `not-permitted` | `fs.pick` declined or revoked. No dialog opens. | Returned by `pick`. |
| `busy` | This plugin already shows a picker. | Returned by `pick`, before any dialog opens. |
| `cancelled` | The user pressed Cancel or Escape. | Completion; the SDK wrapper fails with it. |
| `not-found` | The completion succeeded but has no picked path. | SDK wrapper. |
| `revoked` | The grant changed during the call. The plugin is reactivated with the reduced grant set. | Returned or completion. |
| `not-permitted` on read or write | The plugin lacks `fs.read:picked` or `fs.write:picked`. | `read_all`, `write`, and so on. |

When the grant is declined, the plugin should tell the user that the feature needs the picker
permission. It should not retry in a loop.

### Who requests it

No first-party plugin in the feature mapping lists `fs.pick` in its capability column. Feature 22
(File explorer and picker) keeps the picker dialog in the kernel. Feature 101 (Media scenes) reads
paths that the user picked, under the `picked` scope. A scene gets its path from a settings field with
the `picker` keyword, so it declares `fs.read:picked` without `fs.pick`.

### See also

- [fs.read](/pages/capability-fs-read/) for the read functions that work on a picked path.
- [fs.write](/pages/capability-fs-write/) for writing to a path the user saved.
- [fs.watch](/pages/capability-fs-watch/) for watching a picked folder.
- [File access](/pages/file-access/) for scopes, grants and preopened folders.
- [Filesystem API](/pages/api-fs/) for every `fs` function, type and limit.
- [Requests and outcomes](/pages/requests-and-outcomes/) for how completions reach the plugin.
- [Errors and limits](/pages/reference-errors-and-limits/) for the full error code list.

Checked against `ilium:plugin@0.1.0` (API draft of 2026-10-11).
