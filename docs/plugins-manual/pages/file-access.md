---
layout: page
title: File access
permalink: /pages/file-access/
---

## File access

A plugin reaches files only through named scopes. A scope is a directory the user has granted, such as the project folder, or a directory the plugin owns, such as its private data folder. The kernel resolves each grant to one canonical directory once. The plugin then reads and writes through paths relative to that scope, and it never learns the real path on the user's disk.

Use this page to choose a scope, read and write files in Rust or TypeScript, watch a folder for changes, take a lock on a shared file, and ask the user to choose a file with the picker. Every call listed here is in the `fs` interface of `ilium:host/fs`, version `0.1.0`.

## You will need

- A manifest with a `capabilities` object for the side that makes the call. Each file operation needs the grant for its scope. See [Capabilities and grants](/pages/capabilities/).
- The SDK for your language: `ilium_plugin_sdk` for Rust, or `@ilium/plugin-sdk` for TypeScript. See [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/).
- Familiarity with request ids and completions. Slow calls return a request id, and the result arrives later. See [Requests and outcomes](/pages/requests-and-outcomes/).

The manifest lists grants per side:

```json
"capabilities": {
  "server": ["fs.read:project", "fs.write:data", "fs.pick", "fs.read:picked"]
}
```

## Scopes

A grant names a scope, and the scope names a directory. The plugin sees each scope as a preopened directory at the path shown below. The kernel holds the real location.

| Scope | Preopen | Root | Default | Write | Watch |
|---|---|---|---|---|---|
| `package` | `/package` | Unpacked package `assets/` | Always, read only | No | No |
| `data` | `/data` | The plugin's private data directory | Always | Always | Always |
| `cache` | `/cache` | The plugin's private cache, evictable | Always | Always | Always |
| `project` | `/project` | The canonical project directory | Grant | Grant | Grant |
| `picked` | `/picked/<n>` | Each path the user chose in the picker | Grant per pick | Grant | Folders only |
| `agent-sessions:claude`, `:codex`, `:antigravity` | `/agent-sessions/<agent>` | The agent's transcript directory | Grant, high risk | Refused | Grant |
| `agent-config:claude`, `:codex` | `/agent-config/<agent>` | The agent's configuration directory | Grant, high risk | Grant, high risk | Grant |
| `minecraft-saves` | `/minecraft-saves` | The platform's saves directory | Grant | Grant | Grant |
| `worktrees` | `/worktrees` | The root set by `workspace.worktree_root` | Grant | High risk | Grant |
| `path:<template>` | `/paths/<n>` | A template, see below | Grant | Grant | Grant |

The `project` scope excludes the project's `.ilium` folder. Listing hides it, and every access returns `not-permitted`. Session snapshots are never exposed.

The `package` scope is never watchable, because the package does not change at run time.

`fs.read:<scope>` allows reads, `fs.write:<scope>` allows changes, and `fs.watch:<scope>` allows watching folders. Writes outside `data` and `cache` need an explicit user grant, even for bundled plugins.

### Path templates

A `path:<template>` grant starts with a placeholder (`{home}`, `{config}`, `{data}`, `{cache}`, `{project}` or `{worktrees}`) or with a literal absolute path. It ends with a file or folder name, which matches that item and its direct children, or with `/**`, which matches the whole subtree. A `*` anywhere else is refused at install.

- `fs.read:path:{home}/Pictures/**` reads the Pictures tree under the user's home directory, and nothing outside it.
- A literal absolute path such as `fs.read:path:/srv/logs/**` is always high risk.
- `fs.write:path:` with a literal absolute path is available to bundled plugins only.

The kernel names each `path:` grant `paths/<n>`, counting from 0 in manifest order, so the first grant appears as `/paths/0`.

## Calls

Synchronous calls return at once. Calls that need the disk, another process or the user return a request id, and the result is taken with the matching `take-*` function. The Rust and TypeScript SDKs wrap each slow call in one `async` function, so most code never touches a request id. Rust uses snake_case names, such as `read_all`, and TypeScript uses camelCase, such as `readAll`.

| Call | Kind | Result | Meaning |
|---|---|---|---|
| `roots()` | Sync | `list<tuple<string, bool>>` | Granted scopes and whether each is writable |
| `metadata(path)` | Sync | `fs-metadata` | Kind, length, modified time and read-only flag |
| `list-dir(path, max-entries)` | Sync | `list<dir-entry>` | Direct children, at most 4,096 |
| `read(path, offset, max-bytes)` | Sync | `list<u8>` | Up to 1 MiB from an offset |
| `read-all(path)` | Async | `take-bytes` | Whole file, up to 64 MiB by default |
| `scan(path, glob, max-entries)` | Async | `take-entries` | Matching entries, at most 65,536 |
| `write(path, data, mode)` | Async | `()` | Write one call of up to 16 MiB |
| `create-dir(path)` | Sync | `()` | Create a folder; an existing folder is success |
| `remove(path)` | Async | `()` | Remove a file, or a folder recursively |
| `rename(source, target)` | Async | `()` | Rename within one scope; a target file is replaced |
| `watch(path, recursive)` | Sync | `watch-id` | Start a watch on an existing folder |
| `unwatch(watch)` | Sync | none | Stop a watch; an unknown id is ignored |
| `lock(path, exclusive, timeout-millis)` | Async | `take-lock` | Take an advisory lock |
| `unlock(lock)` | Sync | none | Release a lock |
| `pick(spec)` | Async | `take-picked` | Ask the user to choose a file or folder |
| `read-with-revision(path)` | Async | `take-revisioned` | Whole file plus a revision token |
| `write-atomic(path, data, expected)` | Async | `take-written` | Write to a temporary file, then rename over the target; `expected` makes it a compare-and-swap |
| `tail(path, cursor, max-bytes)` | Sync | `tail-chunk` | New bytes after a cursor, at most 1 MiB |

The key record and enum types are:

| Type | Fields or cases |
|---|---|
| `fs-path` | `scope: string`, `path: string`, with `/` as separator and relative to the scope root |
| `fs-metadata` | `kind` (`file`, `directory`, `symlink`, `other`), `length: u64`, `modified-unix-millis`, `read-only` |
| `dir-entry` | `name: string`, `metadata: fs-metadata` |
| `write-mode` | `create-new`, `replace`, `append` |
| `pick-kind` | `open-file`, `open-directory`, `save-file` |

In Rust these types are `FsPath`, `FsMetadata`, `DirEntry`, `WriteMode` and `PickKind`, with cases
such as `WriteMode::Replace` and `PickKind::OpenFile`. In TypeScript the cases are kebab-case
strings, such as `'replace'` and `'open-file'`.

Every call that returns a request id also returns `busy` when the plugin has too many pending requests, and `quota-exceeded` when its CPU budget is spent.

## Synchronous reads and std::fs

Synchronous code works against the preopens, so a Rust or JavaScript plugin can use the standard library directly for small reads. No Ilium call is needed. The path is the preopen, not the path on disk.

```rust
// Needs the grant fs.read:project in the manifest.
// The path is the preopen, not the path on the user's disk.
let readme_bytes = std::fs::read("/project/README.md")?;
```

Paths that leave the scope through `..` or through a symbolic link are refused with `not-permitted`. A symbolic link is reported as kind `symlink` by `metadata` and `list-dir`, and it is not followed. Other operations follow a link only when its target stays inside the same scope root.

## Examples

### Read a project file and write a summary

This example reads a file from the project and writes a summary to the plugin's data folder. It needs `fs.read:project` and `fs.write:data`.

**Rust**

```rust
use ilium_plugin_sdk::host::fs::{self, WriteMode};
use ilium_plugin_sdk::prelude::*;

// Manifest grants: fs.read:project and fs.write:data on the server side.
pub async fn save_readme_size() -> Result<(), PluginError> {
    let readme = FsPath { scope: "project".into(), path: "README.md".into() };
    let bytes = fs::read_all(readme).await?;

    // create_dir is synchronous and succeeds when the folder already exists.
    fs::create_dir(FsPath { scope: "data".into(), path: "reports".into() })?;

    let summary = format!("README.md is {} bytes\n", bytes.len());
    let target = FsPath { scope: "data".into(), path: "reports/readme-size.txt".into() };
    fs::write(target, summary.into_bytes(), WriteMode::Replace).await?;
    Ok(())
}
```

**TypeScript**

```ts
import { createDir, readAll, write } from '@ilium/plugin-sdk/host/fs';

// Manifest grants: fs.read:project and fs.write:data on the server side.
export async function saveReadmeSize(): Promise<void> {
  const bytes = await readAll({ scope: 'project', path: 'README.md' });
  createDir({ scope: 'data', path: 'reports' });
  const summary = new TextEncoder().encode(`README.md is ${bytes.length} bytes\n`);
  await write({ scope: 'data', path: 'reports/readme-size.txt' }, summary, 'replace');
}
```

### Append to a shared file under a lock

This example appends one line to a file that another process also writes. It takes an exclusive
lock and waits at most 2 seconds. It releases the lock on both the success and the error path.

**Rust**

```rust
use ilium_plugin_sdk::host::fs::{self, WriteMode};
use ilium_plugin_sdk::prelude::*;

pub async fn append_journal_line(line: &str) -> Result<(), PluginError> {
    let journal = FsPath { scope: "data".into(), path: "journal.log".into() };
    let lock = fs::lock(journal.clone(), true, 2_000).await?;
    let result = fs::write(journal, format!("{line}\n").into_bytes(), WriteMode::Append).await;
    fs::unlock(lock);
    result
}
```

**TypeScript**

```ts
import { lock, unlock, write } from '@ilium/plugin-sdk/host/fs';

export async function appendJournalLine(line: string): Promise<void> {
  const journal = { scope: 'data', path: 'journal.log' };
  const held = await lock(journal, true, 2_000);
  try {
    await write(journal, new TextEncoder().encode(`${line}\n`), 'append');
  } finally {
    unlock(held);
  }
}
```

### Pick a note and count its headings

This example asks the user to choose a Markdown file, reads it, and writes the count to the data
folder. It needs `fs.pick`, `fs.read:picked` and `fs.write:data`.

**TypeScript**

```ts
import { pick, readAll, write } from '@ilium/plugin-sdk/host/fs';

// Manifest grants: fs.pick, fs.read:picked and fs.write:data on the server side.
export async function countHeadings(): Promise<number> {
  const chosen = await pick({
    kind: 'open-file',
    title: 'Choose a Markdown note',
    extensions: ['md'],
  });
  const text = new TextDecoder().decode(await readAll(chosen));
  const headings = text.split('\n').filter((line) => line.startsWith('#')).length;
  const report = new TextEncoder().encode(`headings: ${headings}\n`);
  await write({ scope: 'data', path: 'heading-count.txt' }, report, 'replace');
  return headings;
}
```

**Rust**

```rust
use ilium_plugin_sdk::host::fs::{self, PickKind, PickSpec, WriteMode};
use ilium_plugin_sdk::prelude::*;

// Manifest grants: fs.pick, fs.read:picked and fs.write:data on the server side.
pub async fn count_headings() -> Result<usize, PluginError> {
    let chosen = fs::pick(PickSpec {
        kind: PickKind::OpenFile,
        title: "Choose a Markdown note".into(),
        extensions: vec!["md".into()],
    })
    .await?;
    let text = String::from_utf8_lossy(&fs::read_all(chosen).await?).into_owned();
    let headings = text.lines().filter(|line| line.starts_with('#')).count();
    let report = FsPath { scope: "data".into(), path: "heading-count.txt".into() };
    fs::write(report, format!("headings: {headings}\n").into_bytes(), WriteMode::Replace).await?;
    Ok(headings)
}
```

## Writes

A write takes one of three modes.

- `create-new` creates the file. If the file exists, the call completes as `failed(conflict)`.
- `replace` writes the whole file atomically. The kernel writes a temporary sibling file and then renames it over the target.
- `append` adds the data to the end of the file. It creates the file if it is missing.

<figure>
<svg viewBox="0 0 400 110" role="img" aria-label="Atomic replace through a temporary sibling">
  <defs>
    <marker id="fa-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="6" y="36" width="100" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="56" y="52" font-size="11" text-anchor="middle" fill="currentColor">Plugin write</text>
  <text x="56" y="66" font-size="10" text-anchor="middle" fill="currentColor">mode: replace</text>
  <line x1="106" y1="56" x2="130" y2="56" stroke="currentColor" marker-end="url(#fa-arrow)"/>
  <rect x="130" y="36" width="122" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="191" y="52" font-size="11" text-anchor="middle" fill="currentColor">Temp sibling</text>
  <text x="191" y="66" font-size="10" text-anchor="middle" fill="currentColor">notes.md.tmp</text>
  <line x1="252" y1="56" x2="294" y2="56" stroke="currentColor" marker-end="url(#fa-arrow)"/>
  <text x="273" y="49" font-size="10" text-anchor="middle" fill="currentColor">rename</text>
  <rect x="294" y="36" width="100" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="344" y="52" font-size="11" text-anchor="middle" fill="currentColor">Target</text>
  <text x="344" y="66" font-size="10" text-anchor="middle" fill="currentColor">notes.md</text>
  <text x="6" y="100" font-size="10" fill="currentColor">A reader sees the old file or the new file, never half of one.</text>
</svg>
<figcaption>A replace write creates a temporary sibling, then renames it over the target in one step.</figcaption>
</figure>

A `write` with `replace` on a path that names an existing folder completes as `failed(conflict)`. A `write` whose parent folder is missing completes as `not-found`. Create the folder with `create-dir` first.

A write outside `data` and `cache` is logged at level info, with the scope, path, operation and byte count. To write more than 16 MiB, split the data into `append` calls, or let a [background job](/pages/background-jobs/) produce the file.

## Reading large files and scans

`read-all` returns the whole file when it is 64 MiB or smaller. A larger file completes as `failed(quota-exceeded)`. Read a large file in pieces with the synchronous `read` call, which returns at most 1 MiB per call. A `read` at or past the end of the file returns an empty list, not an error.

`scan` matches a glob against paths relative to the scope root. `*` matches within one segment, a whole `**` segment matches any depth, and `?` matches one character. Braces and character classes are not supported. Matches are sorted by path bytes, and dotfiles are included. More matches than `max-entries` complete as `failed(quota-exceeded)`, never as a silent truncation.

## Revisions, atomic writes and tailing

Two plugins, or a plugin and an agent CLI, often share one file. Use `read-with-revision` to read the bytes and a revision token together. The token is a hash of the content, the size and the modification time. Pass it as `expected` to `write-atomic`: the host writes a temporary file in the same directory and renames it over the target, so a reader never sees a half-written file. If the file changed since the read, the call fails with `conflict` and the plugin reads again. With `expected` set to none, the call creates the file and fails with `conflict` when it exists.

`tail` reads what a growing file gained. The plugin keeps the `cursor` from the last chunk. When the chunk has `truncated: true`, the file shrank or was replaced, and the next read restarts at 0. One call returns at most 1 MiB.

Further scopes are `agent-sessions:gemini`, `agent-config:gemini` and `agent-config:antigravity`, with the same risk as the Claude and Codex ones.

<figure>
<pre class="term">╭ Write ─────────────────────────────────────────────────────╮
│ <span class="t-hi">read-with-revision</span>   counter.txt  <span class="t-dim">rev 9f2c</span>                 │
│ <span class="t-dim">another writer saves the file: rev 71ab</span>                    │
│ <span class="t-hi">write-atomic</span>         expected 9f2c                         │
│ <span class="t-err">✗ failed: conflict</span>  <span class="t-dim">read again and retry</span>                   │
╰────────────────────────────────────────────────────────────╯</pre>
<figcaption>Illustrative, not generated. A compare-and-swap write that loses to another writer and retries.</figcaption>
</figure>

**Rust**

```rust
use ilium_plugin_sdk::host::fs;
use ilium_plugin_sdk::prelude::*;

pub async fn bump_counter() -> Result<(), PluginError> {
    let path = FsPath { scope: "data".into(), path: "counter.txt".into() };
    for _attempt in 0..3 {
        let current = fs::read_with_revision(path.clone()).await?;
        let value: u64 = String::from_utf8_lossy(&current.bytes).trim().parse().unwrap_or(0);
        let next = format!("{}\n", value + 1).into_bytes();
        match fs::write_atomic(path.clone(), next, Some(current.revision)).await {
            Ok(_) => return Ok(()),
            Err(error) if error.code == ErrorCode::Conflict => continue,
            Err(error) => return Err(error),
        }
    }
    Err(PluginError::new(ErrorCode::Conflict, "the counter kept changing"))
}

pub fn read_new_log_bytes(path: FsPath, cursor: u64) -> Result<(Vec<u8>, u64), PluginError> {
    let chunk = fs::tail(path, cursor, 65_536)?;
    let next = if chunk.truncated { 0 } else { chunk.cursor };
    Ok((chunk.bytes, next))
}
```

**TypeScript**

```ts
import { readWithRevision, tail, writeAtomic } from '@ilium/plugin-sdk/host/fs';

export async function bumpCounter(): Promise<void> {
  const path = { scope: 'data', path: 'counter.txt' };
  for (let attempt = 0; attempt < 3; attempt += 1) {
    const current = await readWithRevision(path);
    const value = Number.parseInt(new TextDecoder().decode(current.bytes).trim(), 10) || 0;
    try {
      await writeAtomic(path, new TextEncoder().encode(`${value + 1}\n`), current.revision);
      return;
    } catch (error) {
      if ((error as { code?: string }).code !== 'conflict') throw error;
    }
  }
  throw new Error('the counter kept changing');
}

export function readNewLogBytes(path: { scope: string; path: string }, cursor: bigint) {
  const chunk = tail(path, cursor, 65_536);
  return { bytes: chunk.bytes, next: chunk.truncated ? 0n : chunk.cursor };
}
```

## Watching folders

`watch(path, recursive)` takes an existing folder. A file gives `invalid-argument`, a missing path gives `not-found`, and a symbolic link gives `not-permitted`. Changes arrive as `fs-changed` events, and each event is a record with the `watch` id and a list of `paths`.

- The host waits 200 ms after the first change and sends one event for all changes in that time. A watch sends at most five events per second.
- An event lists at most 1,024 paths. A larger batch becomes one event that lists only the watched path, and the plugin must read that folder again.
- A non-recursive watch reports only the direct children. A recursive watch reports every changed path below the folder. A rename lists the old and the new path.
- A plugin may hold 64 watches. A recursive watch counts once.
- Watch ids do not survive a restart. Watch again in `activate`.

The event goes only to the instance that made the watch call. A watch ends when its instance deactivates or restarts, when the plugin is disabled, or when it loses the grant. When the watched folder is removed or renamed, one event lists the watched path, and the watch then ends.

## Locks

A lock coordinates plugins with each other, and with any program that takes the same lock. The lock is advisory. A program that does not take the lock is not blocked. The file does not need to exist.

- `exclusive: true` waits behind any other holder. `exclusive: false` shares the lock with other shared holders.
- A held lock that is still held when `timeout-millis` ends completes as `timed-out`.
- A request with timeout `0` on a held lock returns `conflict` at once.
- Locks are keyed by the resolved path. They are released when the plugin is deactivated, disabled, or loses the grant.

## The file picker

A plugin cannot choose a file itself. It calls `pick` with the `fs.pick` grant, and the host draws a modal picker. The user chooses the path, and the plugin receives an `fs-path` in the `picked/<n>` scope. Reading the result needs `fs.read:picked`, and saving to it needs `fs.write:picked`. Without the read or write grant, `fs.pick` fails the install with `manifest.grant-dependency`.

| Kind | Result | Notes |
|---|---|---|
| `open-file` | `picked/<n>` with the file path | Read it with `fs.read:picked` |
| `open-directory` | `picked/<n>` with path `""` | The folder itself; paths inside it are relative to it |
| `save-file` | `picked/<n>` with path `""` | The file may not exist yet; `write` needs `fs.write:picked` |

The `extensions` list is written in lower case without a dot. It matches case-insensitively, and an empty list allows every file.

The picker accepts Enter to open a folder or confirm, Backspace to go to the parent folder, Tab to move between the input, the list and the buttons, Ctrl+H to show hidden files, and Escape to cancel. A cancelled pick fails with `cancelled`. A second pick while a picker is open fails at once with `busy`, and no second dialog opens.

Picked paths are recorded with the plugin's grants and survive restarts. The plugin keeps up to 64 picked paths. When it picks a 65th, the oldest is forgotten. The `<n>` counter never repeats, so a picked scope keeps its name until its grant is revoked.

<figure>
<pre class="term">╭  ≡ ● · Open file ────────────────────────────────────────╮
│ <span class="t-dim">Notes wants a file</span>                                       │
│ Folder ……………………………………………………… /home/arthur/dev/app/docs   │
│   ..                                                     │
│   <span class="t-hi">meeting-2026-10-09.md</span>                                  │
│   release-checklist.md                                   │
│   plan.md                                                │
│ Filter ………………………………………………………………………………………………………………… .md   │
│ <span class="t-dim">Enter open · Backspace parent · Tab move · Esc cancel</span>    │
╰──────────────────────────────────────────────────────────╯</pre>
<figcaption>Illustrative, not generated. The host file picker. The folder is an example. The chosen file is shared with the plugin until the user revokes it.</figcaption>
</figure>

## Scope resolution

A grant becomes a preopen once. The kernel does this work through the platform layer, and the plugin never sees a path it was not granted.

<figure>
<svg viewBox="0 0 400 150" role="img" aria-label="Scope resolution from grant to preopen">
  <title>Scope resolution</title>
  <desc>A grant such as fs.read:project is resolved by the kernel to a canonical directory. The directory is opened as a preopen for synchronous calls, and slow calls go through the file host with request ids.</desc>
  <defs>
    <marker id="fa-arrow2" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="6" y="50" width="110" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="61" y="68" font-size="11" text-anchor="middle" fill="currentColor">Grant</text>
  <text x="61" y="84" font-size="10" text-anchor="middle" fill="currentColor">fs.read:project</text>
  <line x1="116" y1="72" x2="138" y2="72" stroke="currentColor" marker-end="url(#fa-arrow2)"/>
  <rect x="138" y="50" width="136" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="206" y="68" font-size="11" text-anchor="middle" fill="currentColor">Kernel resolves</text>
  <text x="206" y="84" font-size="10" text-anchor="middle" fill="currentColor">canonical directory</text>
  <line x1="274" y1="64" x2="300" y2="36" stroke="currentColor" marker-end="url(#fa-arrow2)"/>
  <line x1="274" y1="80" x2="300" y2="108" stroke="currentColor" marker-end="url(#fa-arrow2)"/>
  <rect x="300" y="14" width="94" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="347" y="32" font-size="11" text-anchor="middle" fill="currentColor">Preopen</text>
  <text x="347" y="48" font-size="10" text-anchor="middle" fill="currentColor">sync calls</text>
  <rect x="300" y="88" width="94" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="347" y="106" font-size="11" text-anchor="middle" fill="currentColor">File host</text>
  <text x="347" y="122" font-size="10" text-anchor="middle" fill="currentColor">request ids</text>
</svg>
<figcaption>One grant gives one canonical directory. Synchronous calls use the preopen, and slow calls go through the file host.</figcaption>
</figure>

## Errors

Every call returns a plugin error with one of these codes. The table lists the codes beyond the universal `not-permitted`, `revoked` and `internal`.

| Call | Synchronous errors | Completion errors |
|---|---|---|
| `metadata`, `list-dir`, `read` | `not-found`, `invalid-argument` (bad path, limits exceeded, wrong kind), `io` | None |
| `read-all` | `invalid-argument` (bad path) | `not-found`, `io`, `quota-exceeded` (over 64 MiB) |
| `scan` | `invalid-argument` (bad path or glob, `max-entries` over 65,536) | `not-found`, `io`, `quota-exceeded` |
| `write` | `invalid-argument` (bad path, over 16 MiB) | `conflict` (`create-new` found a file, or a lock holds it), `not-found`, `quota-exceeded`, `io` |
| `create-dir` | `conflict` (a file has that name), `not-found`, `io` | None |
| `remove` | `invalid-argument` (scope root) | `not-found`, `conflict` (lock), `io` |
| `rename` | `invalid-argument` (two different scopes) | `not-found`, `conflict` (target is a non-empty folder or locked), `io` |
| `watch` | `not-found`, `invalid-argument` (not watchable), `quota-exceeded` (64 watches) | None |
| `unwatch` | None (an unknown id is ignored) | None |
| `read-with-revision` | `invalid-argument` (bad path) | `not-found`, `io`, `quota-exceeded` (over 64 MiB) |
| `write-atomic` | `invalid-argument` (bad path, over 16 MiB) | `conflict` (the file changed since the read: stale `expected` revision, or `expected` is none and the file exists, or a lock holds it), `not-found`, `quota-exceeded`, `io` |
| `tail` | `not-found`, `invalid-argument` (bad path, `max-bytes` over 1 MiB), `io` | None |
| `lock` | `invalid-argument` (bad path) | `conflict` (held and timeout 0), `timed-out`, `not-found` |
| `pick` | `busy` (a picker is already open) | `cancelled` when the user cancels |

## What the user sees

- High-risk scopes appear in the review dialog unchecked, and the user must check each one. See [Capabilities and grants](/pages/capabilities/).
- A call refused for a missing or declined grant writes one refusal line to the plugin log, with the capability, the call and the reason:

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs notes
<span class="t-err">refusal</span> capability=fs.read:project function=fs.read reason=declined
<span class="t-ok">info</span>    write scope=project path=docs/notes.md op=replace bytes=2048
</pre>
<figcaption>Illustrative, not generated. The fields are the ones the plugin log records, in an example layout.</figcaption>
</figure>

## Edge cases

- An empty `path` names the scope root. `write` and `remove` refuse it at once, with `invalid-argument`. On a `picked/<n>` scope, an empty path names the picked item itself.
- A declined grant makes every call on that scope return `not-permitted`. A revoked grant ends the plugin's locks and watches.

## Rules

- Ask for the narrowest scope. Prefer a path template to the whole project when only one folder is needed.
- Write your own state to `data`, and use the picker when the user has to choose a file.
- Use `replace` for any file that another reader must never see half-written.
- Take a lock on any file another process writes, and release it on every path.

## See also

- [Capabilities and grants](/pages/capabilities/)
- [Requests and outcomes](/pages/requests-and-outcomes/)
- [Background jobs](/pages/background-jobs/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Security](/pages/security/)
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/)

API version checked: `ilium:plugin@0.1.0`.

### Security

Scopes and the deny set are defined in [Sandbox reference](/pages/reference-sandbox/) ([SEC-058] to [SEC-062]).
