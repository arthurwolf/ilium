---
layout: page
title: fs
permalink: /pages/api-fs/
---

## fs

The `fs` interface reads and writes files, but only inside file scopes. A scope is a named root that the user has granted, such as the project, a path the user picked, or the plugin's own data folder. Each call names a scope and a path relative to its root, with `/` as the separator. A slow call returns a request id at once, and the SDK turns its completion into an `async` function in Rust or a `Promise` in TypeScript.

For small values that the plugin owns, use [storage](/pages/api-storage/). To decode what you read, use [codec](/pages/api-codec/).

### What you need

Each operation needs a grant in the form `<grant>:<scope>`.

| Operation | Grant | Sides |
|---|---|---|
| Read, list, scan | `fs.read:<scope>` | server, client, scene |
| Write, create, remove, rename | `fs.write:<scope>` | server, client, scene |
| Lock | `fs.read:<scope>` and `fs.write:<scope>` | server, client, scene |
| Watch | `fs.watch:<scope>` | server, client |
| Picker | `fs.pick`, plus `fs.read:picked` or `fs.write:picked` | client |

Other rules that apply:

- `fs.roots` needs any `fs` grant. The `package`, `data` and `cache` scopes are always granted, so a manifest does not list them.
- `fs.pick` without `fs.read:picked` or `fs.write:picked` fails at install with `manifest.grant-dependency`.
- `fs.write:agent-sessions:<agent>` is refused at install.
- `fs.read:path:` with a literal absolute path, `fs.watch:path:` with one, and `fs.write:worktrees` are high-risk grants. The user checks each one by hand.
- The `project` scope hides the project's `.ilium` folder. Listing skips it, and access returns `not-permitted`.

Manifest example for a server side that reads and writes the project and watches its `docs` folder:

```json
"capabilities": { "server": ["fs.read:project", "fs.write:project", "fs.watch:project"] }
```

### Scopes

| Scope | Preopen | Root | Default |
|---|---|---|---|
| `package` | `/package` | unpacked package `assets/` | always, read only |
| `data` | `/data` | the plugin's private data folder | always, read and write |
| `cache` | `/cache` | the plugin's private cache folder, may be evicted | always, read and write |
| `project` | `/project` | the session's canonical project folder | grant |
| `worktrees` | `/worktrees` | the worktree folder set by `workspace.worktree_root` | grant, high risk for read and for write |
| `picked` | `/picked/<n>` | each path the user chose in the picker | grant per pick |
| `agent-sessions:claude`, `agent-sessions:codex`, `agent-sessions:antigravity`, `agent-sessions:gemini` | `/agent-sessions/<agent>` | the agent's transcript folder | grant, high risk |
| `agent-config:claude`, `agent-config:codex`, `agent-config:gemini`, `agent-config:antigravity` | `/agent-config/<agent>` | the agent's configuration folder | grant, high risk for write |
| `minecraft-saves` | `/minecraft-saves` | the platform's Minecraft saves folder | grant |
| `path:<template>` | `/paths/<n>` | a template with `{home}`, `{config}`, `{data}`, `{cache}`, `{project}`, `{worktrees}`, optionally ending in `/**` | grant |

Every scope except `package` and the `agent-config` scopes can be watched (a watch on `agent-config` is refused with `manifest.grant-scope`), and `picked` only for folders. A path that leaves the scope root through `..` or a symbolic link is refused with `not-permitted`. Synchronous `std::fs` code also works, because the component sees only the preopens.

### Surface

#### Functions

| Function | Grant | Kind | Result and meaning |
|---|---|---|---|
| `roots` | any `fs` grant | sync | List of `(scope, writable)` pairs for the granted scopes. |
| `metadata(path)` | `fs.read` | sync | `fs-metadata` of one entry. `not-found` when it is absent. |
| `list_dir(path, max_entries)` | `fs.read` | sync | Children of a folder, at most 4,096. Each is a `dir-entry`. |
| `read(path, offset, max_bytes)` | `fs.read` | sync | Bytes from `offset`, at most 1 MiB per call. |
| `read_all(path)` | `fs.read` | async | Whole file as bytes, up to `fs_read_all_mib`. |
| `scan(path, glob, max_entries)` | `fs.read` | async | Relative paths and their entries that match `glob`. |
| `write(path, data, mode)` | `fs.write` | async | Writes bytes with `create-new`, `replace` or `append`. |
| `create_dir(path)` | `fs.write` | sync | Creates one folder. An existing folder counts as success. |
| `remove(path)` | `fs.write` | async | Removes a file, or a folder and everything in it. |
| `rename(source, target)` | `fs.write` | async | Renames inside one scope. A file target is replaced atomically. |
| `watch(path, recursive)` | `fs.watch` | sync | Starts a watch and returns a `watch-id`. |
| `unwatch(watch)` | `fs.watch` | sync | Stops a watch. |
| `lock(path, exclusive, timeout_millis)` | `fs.read` and `fs.write` | async | Takes a lock and returns a `lock-id`. |
| `unlock(lock)` | none | sync | Releases a lock. |
| `pick(spec)` | `fs.pick` | async | Opens the picker and returns a `request-id`. Retrieve the chosen path with `take_picked(request)` (WIT `take-picked`), which returns `option<fs-path>`: `none` means the user cancelled. The path arrives in a `picked/<n>` scope. |
| `read_with_revision(path)` | `fs.read` | async | Whole file plus its revision token. See [atomic write and tail](/pages/api-fs-atomic-and-tail/). |
| `write_atomic(path, data, expected)` | `fs.write` | async | Replaces the file only if it is still at `expected`, or does not exist when `expected` is none. Fails with `conflict` otherwise. |
| `tail(path, cursor, max_bytes)` | `fs.read` | sync | Bytes appended since `cursor`, at most 1 MiB, with the next cursor and a `truncated` flag. |

Async functions are `async` in Rust and return a `Promise` in TypeScript. The raw `*_request` and `take_*` functions stay available, but normal code does not need them.

#### Types

| Type | Fields | Meaning |
|---|---|---|
| `fs-path` | `scope: string`, `path: string` | A scope name and a path relative to its root. An empty path names the root. Rust: `FsPath`. |
| `entry-kind` | `file`, `directory`, `symlink`, `other` | Kind of one entry. Rust: `EntryKind::File`. |
| `fs-metadata` | `kind`, `length: u64`, `modified-unix-millis: option<u64>`, `read-only: bool` | Size and time of one entry. Both numbers are `bigint` in TypeScript. |
| `dir-entry` | `name: string`, `metadata: fs-metadata` | One child of a folder. |
| `write-mode` | `create-new`, `replace`, `append` | What `write` does when the file exists. Rust: `WriteMode::Append`. |
| `pick-kind` | `open-file`, `open-directory`, `save-file` | Mode of the picker. |
| `pick-spec` | `kind`, `title: string`, `extensions: list<string>` | Picker mode, title and extension filter. |
| `watch-id`, `lock-id` | `u64` | Handles returned by `watch` and `lock`. |
| `revision-token` | `string` | Opaque; equal tokens mean equal content. |
| `revisioned` | `bytes: list<u8>`, `revision: revision-token` | Result of `read_with_revision`. |
| `tail-chunk` | `bytes`, `cursor: u64`, `truncated: bool` | Result of `tail`. |
| `fs-changed-event` | `watch: watch-id`, `paths: list<string>` | Event for one watch. Paths are relative to the scope root. |

### Calls: sync and async

A sync call answers at once. An async call checks the grant, scope, syntax and size caps at once, so those errors return from the call itself. Errors that need the disk or the user arrive in the completion as `failed(code)`.

Use `read` for a slice and `read_all` for a whole file, up to 64 MiB by default. A larger file fails with `failed(quota-exceeded)`.

```rust
let bytes = fs::read_all(&FsPath { scope: "project".into(), path: "README.md".into() }).await?;
```

Async wrappers follow one rule. The value comes from the matching `take-*` function. Calls with no take function, such as `write`, `remove` and `rename`, return `()`. A completion of `failed(code)` returns `Err(code)`, and a cancelled pick returns `cancelled`.

### Writes

`write` takes one of three modes:

- `create-new` writes a new file. An existing file gives `failed(conflict)`.
- `replace` writes a temporary sibling file and renames it over the target. A reader never sees half a file.
- `append` adds bytes to the end of the file, creating it when it is absent.

One `write` takes at most `fs_write_mib` (16 MiB by default); write larger files in several `append` calls. Writes, removals, renames and folder creation outside `data` and `cache` are logged at info level.

`create_dir` is not recursive, so a missing parent gives `not-found`. `remove` on a scope root is `invalid-argument`.

### Locks

A lock lets writers take turns on one path. Locks are kept per session and keyed by the resolved path, so they coordinate plugins with each other. A lock is advisory: a program that does not take it is not blocked, and the file need not exist.

A shared lock (`exclusive: false`) coexists with other shared locks. An exclusive lock waits for every holder. The timeout is in milliseconds.

- A held lock with timeout 0 gives `conflict` at once.
- A lock still held when its timeout ends gives `timed-out`. `busy` is never used for locks.
- Locks are released with `unlock`, and also when the plugin is deactivated, disabled or loses the grant.

### Watches

`watch(path, recursive)` reports changes to a scope path. The event is `fs-changed`, and its payload is an `fs-changed-event`. A watch yields at most one event per batch, listing each changed path once. Watches end when the plugin deactivates, is disabled or loses the grant.

### Example: append a line under a lock

The plugin appends one line to a shared project log. The lock keeps two writers from interleaving lines.

**Grants:** `fs.read:project`, `fs.write:project`

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::fs::{self, WriteMode};
use ilium_plugin_sdk::host::log::{self, Level};

const LOG_PATH: &str = "docs/chat.log";

async fn append_line(line: String) -> Result<(), PluginError> {
    let target = FsPath { scope: "project".into(), path: LOG_PATH.into() };
    let lock = fs::lock(&target, true, 2_000).await?;
    let outcome = fs::write(&target, format!("{line}\n").into_bytes(), WriteMode::Append).await;
    fs::unlock(lock);
    outcome
}

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct ChatLog;

impl Lifecycle for ChatLog {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        let line = format!("plugin {} started", context.plugin);
        task::spawn(async move {
            if let Err(error) = append_line(line).await {
                log::log(Level::Error, &format!("append failed: {}", error.message));
            }
        });
        Ok(())
    }
}

impl Handler for ChatLog {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin, type PluginError } from '@ilium/plugin-sdk';
import { lock, unlock, write } from '@ilium/plugin-sdk/host/fs';

const LOG_PATH = 'docs/chat.log';

async function appendLine(line: string): Promise<void> {
  const target = { scope: 'project', path: LOG_PATH };
  const lockId = await lock(target, true, 2000);
  try {
    await write(target, new TextEncoder().encode(`${line}\n`), 'append');
  } finally {
    unlock(lockId);
  }
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate(context) {
    appendLine(`plugin ${context.plugin} started`).catch((error: PluginError) => {
      console.error(`append failed: ${error.message}`);
    });
  },
  onEvents: () => [],
});
```

Outcome: the append runs under the lock, and `unlock` runs even when the write fails. A second writer that waits longer than 2 s gets `timed-out`.

### Example: read files as they change

The plugin watches the project's `docs` folder and logs the size of each changed file. Event paths are relative to the scope root, so they work directly in an `fs-path`.

**Grants:** `fs.read:project`, `fs.watch:project`

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::fs;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::types::events::FsChangedEvent;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct DocsWatcher {
    watch: Option<u64>,
}

impl Lifecycle for DocsWatcher {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        let docs = FsPath { scope: "project".into(), path: "docs".into() };
        self.watch = Some(fs::watch(&docs, true)?);
        Ok(())
    }
}

impl Handler for DocsWatcher {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::FsChanged(FsChangedEvent { watch, paths }) = event else { continue };
            if self.watch != Some(watch) {
                continue;
            }
            for path in paths {
                let target = FsPath { scope: "project".into(), path: path.clone() };
                task::spawn(async move {
                    match fs::read_all(&target).await {
                        Ok(bytes) => {
                            log::log(Level::Info, &format!("{path}: {} bytes", bytes.len()));
                        }
                        Err(error) => {
                            log::log(Level::Error, &format!("{path}: {}", error.message));
                        }
                    }
                });
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type PluginError, type Submission } from '@ilium/plugin-sdk';
import { readAll, watch } from '@ilium/plugin-sdk/host/fs';
import { log } from '@ilium/plugin-sdk/host/log';

let docsWatch: bigint | undefined;

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate() {
    docsWatch = watch({ scope: 'project', path: 'docs' }, true);
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'fs-changed' || event.val.watch !== docsWatch) {
        continue;
      }
      for (const path of event.val.paths) {
        readAll({ scope: 'project', path })
          .then((bytes) => log('info', `${path}: ${bytes.length} bytes`))
          .catch((error: PluginError) => log('error', `${path}: ${error.message}`));
      }
    }
    return [];
  },
});
```

### What the user sees

When a plugin asks for a grant, the review dialog lists it in plain words:

| Grant | Dialog text |
|---|---|
| `fs.read:project` | Read files in this project |
| `fs.write:project` | Change files in this project |
| `fs.watch:project` | Watch files in this project for changes |
| `fs.pick` | Ask you to choose files |

The picker is a modal panel. Enter opens a folder or confirms, Backspace goes up one folder, Tab moves focus, Ctrl+H shows hidden files, and Escape cancels.

<figure>
<pre class="term"><span class="t-dim">╭  ≡ ● · Open file───────────────────────────────────────────────────╮</span>
<span class="t-dim">│</span> Location ………… /home/user/projects/notes                            <span class="t-dim">│</span>
<span class="t-dim">│</span> Filter ……………… *.md                        <span class="t-dim">Tab moves focus</span>          <span class="t-dim">│</span>
<span class="t-dim">│</span> <span class="t-dim">Name                              Size     Modified</span>                <span class="t-dim">│</span>
<span class="t-dim">│</span>   ▶ docs/                          -        2026-10-10             <span class="t-dim">│</span>
<span class="t-dim">│</span> <span class="t-ok">› plan.md                          4.2 KB   2026-10-11</span>             <span class="t-dim">│</span>
<span class="t-dim">│</span>   notes.md                         1.1 KB   2026-10-09             <span class="t-dim">│</span>
<span class="t-dim">│</span>                                                                    <span class="t-dim">│</span>
<span class="t-dim">│</span>  Esc Cancel      Enter Open                                        <span class="t-dim">│</span>
<span class="t-dim">│</span> <span class="t-dim">Esc cancel · Enter open</span>                                            <span class="t-dim">│</span>
<span class="t-dim">╰────────────────────────────────────────────────────────────────────╯</span>
</pre>
<figcaption>Illustrative, not generated. The picker that <code>fs.pick</code> opens, 68 by 10 characters, dark theme. The selected row and the extension filter show what the user chooses.</figcaption>
</figure>

Writes outside `data` and `cache` appear in the plugin log. `ilium plugin logs <id>` shows them:

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs com.example.chatlog
<span class="t-dim">2026-10-11 09:14:02</span> <span class="t-ok">info</span>  fs.write project docs/chat.log append 42 bytes
<span class="t-dim">2026-10-11 09:14:05</span> <span class="t-ok">info</span>  fs.write project docs/chat.log append 37 bytes
<span class="t-dim">2026-10-11 09:14:09</span> <span class="t-err">error</span> append failed: timed-out
</pre>
<figcaption>Illustrative, not generated. A plugin log after two appends and one lock timeout.</figcaption>
</figure>

The diagram shows how a path is checked. The grant is tested first, and the escape check runs only for a granted scope.

<figure>
<svg viewBox="0 0 420 240" role="img" aria-label="Scope resolution for a file path">
<title>Scope resolution</title>
<desc>A path is checked against the grant. A granted path is checked for escapes from its root. A path that has no grant or escapes the root is refused with not-permitted.</desc>
<defs>
<marker id="api-fs-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="120" y="8" width="160" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="200" y="30" font-size="12" text-anchor="middle" fill="currentColor">1. Scope and path</text>
<line x1="200" y1="44" x2="200" y2="68" stroke="currentColor" marker-end="url(#api-fs-arrow)"/>
<rect x="120" y="68" width="160" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="200" y="90" font-size="12" text-anchor="middle" fill="currentColor">2. Grant check</text>
<line x1="200" y1="104" x2="200" y2="128" stroke="currentColor" marker-end="url(#api-fs-arrow)"/>
<text x="208" y="120" font-size="10" fill="currentColor">granted</text>
<rect x="120" y="128" width="160" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="200" y="150" font-size="12" text-anchor="middle" fill="currentColor">3. Escape check</text>
<line x1="200" y1="164" x2="200" y2="188" stroke="currentColor" marker-end="url(#api-fs-arrow)"/>
<text x="208" y="180" font-size="10" fill="currentColor">inside</text>
<rect x="120" y="188" width="160" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="200" y="210" font-size="12" text-anchor="middle" fill="currentColor">4. Read or write</text>
<path d="M280 86 H354 V188" fill="none" stroke="currentColor" marker-end="url(#api-fs-arrow)"/>
<text x="290" y="80" font-size="10" fill="currentColor">no grant</text>
<path d="M280 146 H354" fill="none" stroke="currentColor"/>
<text x="290" y="140" font-size="10" fill="currentColor">escape</text>
<rect x="296" y="188" width="116" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
<text x="354" y="210" font-size="12" text-anchor="middle" fill="currentColor">not-permitted</text>
</svg>
<figcaption>A path is checked against its grant, then for escapes from the scope root. Failures end in not-permitted.</figcaption>
</figure>

### Edge cases and errors

| Code | When it happens with `fs` |
|---|---|
| `not-permitted` | The grant is missing, declined or revoked, the path escapes the root, or the call writes to a read-only scope. |
| `not-found` | The entry, or the parent folder of a new entry, does not exist. |
| `invalid-argument` | A bad path or glob, `max_entries` over 4,096 for `list_dir` or over 65,536 for `scan`, `max_bytes` over 1 MiB, a write over `fs_write_mib`, or `rename` across two scopes. |
| `conflict` | `create-new` finds a file, a held lock blocks a zero-timeout request, a rename target is a non-empty folder, or a `write_atomic` revision is stale. |
| `timed-out` | A lock is still held when its timeout ends. |
| `quota-exceeded` | A file is larger than the `read_all` limit, a `scan` finds more matches than `max_entries`, or the `data` and `cache` disk limit is full. |
| `revoked` | The grant changed during the call. The plugin is re-activated with the reduced grant set. |
| `cancelled` | The user cancelled the picker. |
| `busy` | The plugin already shows a picker. |

Other rules to know:

- A picked path survives restarts until its grant is revoked, up to 64 per plugin.
- These limits can be requested in the manifest `limits` object. The host clamps a request above the tier ceiling and logs it.

| Limit | Default | T1 ceiling | Error when reached |
|---|---|---|---|
| `fs_read_all_mib` | 64 | 512 | `failed(quota-exceeded)` |
| `fs_write_mib` (one `write`) | 16 | 64 | `invalid-argument` |
| `disk_mib` (`data` plus `cache`) | 256 | 4,096 | `failed(quota-exceeded)` |
| `watches` | 64 | 256 | `quota-exceeded` |
| `pending_requests` | 64 | 256 | `busy` |

### Sequence of an async call

<figure>
<svg viewBox="0 0 440 190" role="img" aria-label="Sequence of an asynchronous fs call">
<title>Asynchronous fs call</title>
<desc>The plugin calls write and gets a request id. The host writes the file in the background and sends request-completed with ok or failed. The SDK resolves the awaited future from that event.</desc>
<defs>
<marker id="fs-seq-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker>
</defs>
<text x="80" y="16" font-size="12" text-anchor="middle" fill="currentColor">Plugin and SDK</text>
<text x="330" y="16" font-size="12" text-anchor="middle" fill="currentColor">Host</text>
<line x1="80" y1="24" x2="80" y2="184" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="330" y1="24" x2="330" y2="184" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="80" y1="44" x2="328" y2="44" stroke="currentColor" marker-end="url(#fs-seq-arrow)"/>
<text x="86" y="38" font-size="10" fill="currentColor">write(path, data, mode)</text>
<line x1="328" y1="68" x2="82" y2="68" stroke="currentColor" marker-end="url(#fs-seq-arrow)"/>
<text x="324" y="62" font-size="10" text-anchor="end" fill="currentColor">request id (grant, scope, size checked)</text>
<text x="334" y="104" font-size="10" fill="currentColor">writes in the background</text>
<line x1="328" y1="130" x2="82" y2="130" stroke="currentColor" marker-end="url(#fs-seq-arrow)"/>
<text x="324" y="124" font-size="10" text-anchor="end" fill="currentColor">request-completed (ok or failed(code))</text>
<text x="86" y="164" font-size="10" fill="currentColor">The awaited future resolves; the event is not delivered to the plugin.</text>
</svg>
<figcaption>Checks that need only the call arguments fail at once. Disk and user errors arrive in the completion.</figcaption>
</figure>

### WIT

The contract below is an excerpt. The owner is the [WIT reference](/pages/reference-wit/), and that page wins if the two differ.

```wit
// Excerpt of ilium:host/fs in reference-wit
read-all: func(path: fs-path) -> result<request-id, plugin-error>;
take-bytes: func(request: request-id) -> option<list<u8>>;
```

The full interface, with every record and function, is in the [WIT reference](/pages/reference-wit/) section for `fs`. This excerpt shows the first slow call and its take function.

### See also

- [fs: atomic write, revisions, tail, agent homes](/pages/api-fs-atomic-and-tail/) for revisions, atomic writes, tail and the Gemini and Antigravity scopes.
- [storage](/pages/api-storage/) for small values that do not need files.
- [codec](/pages/api-codec/) for decoding files after they are read.
- [Events](/pages/api-events/) for `fs-changed` and `request-completed`.
- [File access](/pages/file-access/) for the rules common to all scopes.

### Security

Scopes and the deny set are defined in [Sandbox reference](/pages/reference-sandbox/) ([SEC-058] to [SEC-062]).
