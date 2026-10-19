---
layout: page
title: "fs.read:<scope>"
permalink: /pages/capability-fs-read/
---

## Overview

The `fs.read:<scope>` grant lets a plugin read files and list directories inside one named scope. A scope is a directory the host has chosen for the plugin: the session's project, a folder or file the user picked, the Minecraft saves folder, or the transcript or configuration folder of an agent.

The plugin never names a path on the disk. It names a scope and a path relative to that scope, and the host refuses any path that leaves the scope. The grant is checked on every call, so a revoked grant stops reads at once.

Each scope is mounted for the plugin as a preopen, a directory the sandbox can see. Standard synchronous file code in a Rust or TypeScript plugin also works against these preopens, but the SDK functions below are the supported route.

## What you need

- A grant `fs.read:<scope>` in the manifest for every side that reads the scope. Use `server`, `client` or `scene`, as the scope applies:

  ```json
  {
    "capabilities": {
      "server": ["fs.read:project"]
    }
  }
  ```

- Rust with the `wasm32-wasip2` target, or TypeScript with Bun 1.2 or later and `@ilium/plugin-sdk`.
- Nothing extra for the always-granted scopes. The set in [always-granted](/pages/capability-always-granted/) already covers reads of `package`, `data` and `cache`.
- A separate `fs.write:<scope>` grant for any write to the same scope. See [fs.write](/pages/capability-fs-write/).

## Scopes and grant syntax

The scope is the text after the colon. Valid grants:

| Grant | Preopen | Root and meaning | Risk |
|---|---|---|---|
| `fs.read:project` | `/project` | The session's canonical project directory. Excludes its `.ilium` folder. | normal |
| `fs.read:worktrees` | `/worktrees` | The project's worktree folder from `workspace.worktree_root`. | normal |
| `fs.read:picked` | `/picked/<n>` | Each path the user chose in a picker. Granted per pick. | normal |
| `fs.read:minecraft-saves` | `/minecraft-saves` | The platform's Minecraft saves directory. | normal |
| `fs.read:agent-config:claude` | `/agent-config/claude` | The Claude configuration directory. Also `codex`. | high |
| `fs.read:agent-sessions:claude` | `/agent-sessions/claude` | Claude transcript directory. Also `codex` and `antigravity`. | high |
| `fs.read:path:<template>` | `/paths/<n>` | A path template, see below. | normal, high if literal |

A path template starts with a placeholder or a literal absolute path, and ends with a name or with `/**`:

- Placeholders: `{home}`, `{config}`, `{data}`, `{cache}`, `{project}`, `{worktrees}`.
- `fs.read:path:{config}/ilium/themes/**` grants the whole theme subtree.
- `fs.read:path:{home}/.gitconfig` grants that one file.
- `fs.read:path:/srv/logs/**` uses a literal absolute path. Such a grant is always high risk.

Invalid grants are refused at install with `manifest.grant-scope`:

- `fs.read` with no scope.
- `fs.read:agent-sessions` with no agent name. The agent is part of the scope.
- `fs.read:*`, or a wildcard anywhere except the trailing `/**` of a template.
- `fs.write:agent-sessions:claude`. Transcript scopes are read only.

## Risk and review

- Most scopes are normal risk. The three `agent-sessions` scopes are high risk, because they hold the user's private conversations.
- A T1 bundled plugin receives normal scopes automatically, up to its manifest ceiling. High-risk scopes are asked for once, at first activation.
- A T2 or T3 plugin needs an explicit grant in the review dialog for each scope. The grant is remembered until it is revoked.

## Surface

The `fs` interface offers these read functions. Rust names are in `snake_case` and TypeScript names in `camelCase`.

| Function | Rust (`ilium_plugin_sdk::host::fs`) | TypeScript (`@ilium/plugin-sdk/host/fs`) | Kind | Result |
|---|---|---|---|---|
| `roots` | `roots() -> Vec<(String, bool)>` | `roots(): [string, boolean][]` | sync | Each granted scope and whether it is writable |
| `metadata` | `metadata(path) -> Result<FsMetadata, PluginError>` | `metadata(path): FsMetadata` | sync | Kind, length, modified time, read-only flag |
| `list_dir` | `list_dir(path, max_entries)` returns `Result<Vec<DirEntry>, PluginError>` | `listDir(path, maxEntries): DirEntry[]` | sync | Entries of one folder, at most 4,096 |
| `read` | `read(path, offset, max_bytes)` returns `Result<Vec<u8>, PluginError>` | `read(path, offset, maxBytes): Uint8Array` | sync | At most 1 MiB from `offset` |
| `read_all` | `read_all(path).await` returns `Result<Vec<u8>, PluginError>` | `readAll(path): Promise<Uint8Array>` | async | Whole file, up to the read-all limit |
| `scan` | `scan(path, glob, max_entries).await` | `scan(path, glob, maxEntries): Promise<[string, DirEntry][]>` | async | Matching entries, sorted by path bytes |

The WIT functions `take-bytes` and `take-entries` are used only through the wrappers above. The raw request forms, such as `read_all_request`, are for code that needs them.

Records, from `ilium:host/fs`:

```wit
record fs-path { scope: string, path: string }
enum entry-kind { file, directory, symlink, other }
record fs-metadata {
  kind: entry-kind, length: u64, modified-unix-millis: option<u64>, read-only: bool,
}
record dir-entry { name: string, metadata: fs-metadata }
```

The `path` is relative to the scope root and uses `/` as the separator. An empty `path` names the root itself. For a `picked/<n>` scope, an empty `path` names the picked file or folder.

Limits:

| Limit | Value | Over the limit |
|---|---|---|
| `read` `max-bytes` | 1 MiB per call | `invalid-argument` |
| `list-dir` `max-entries` | 1 to 4,096 | `invalid-argument` |
| `scan` `max-entries` | 1 to 65,536 | `invalid-argument`; more matches complete as `failed(quota-exceeded)` |
| `read-all` file size | 64 MiB by default (`fs_read_all_mib`) | `failed(quota-exceeded)` |

See [errors and limits](/pages/reference-errors-and-limits/) for the full table.

Glob grammar for `scan`: `*` matches any run of characters inside one segment. A whole `**` segment matches zero or more folders. `?` matches one character inside a segment. Braces, character classes and escapes are not supported. A leading `/`, a `..` segment or a backslash is `invalid-argument`.

## Worked example: Rust

This server-side plugin reads the first kilobyte of one file, and lists the Rust sources of the project from a task.

**Rust**

```rust
use ilium_plugin_sdk::host::fs;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;

/// Returns the first `max_bytes` bytes of a file in the project scope.
fn read_head(relative_path: &str, max_bytes: u32) -> Result<Vec<u8>, PluginError> {
    let path = FsPath {
        scope: "project".into(),
        path: relative_path.into(),
    };
    fs::read(path, 0, max_bytes)
}

/// Lists the Rust sources under src/, sorted by path.
async fn rust_sources() -> Result<Vec<String>, PluginError> {
    let root = FsPath { scope: "project".into(), path: String::new() };
    let entries = fs::scan(root, "src/**/*.rs", 500).await?;
    Ok(entries.into_iter().map(|(relative_path, _entry)| relative_path).collect())
}

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Scanner;

impl Handler for Scanner {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        match read_head("src/lib.rs", 1024) {
            Ok(bytes) => log::log(Level::Info, &format!("read {} bytes", bytes.len())),
            Err(error) if error.code == ErrorCode::NotPermitted => {
                // The grant is missing or declined: the scope is simply absent.
            }
            Err(error) => log::log(Level::Warn, &format!("read failed: {}", error.message)),
        }
        task::spawn(async {
            // A real plugin turns the list into an intent or a tree update.
            let _sources = rust_sources().await;
        });
        Vec::new()
    }
}
```

## Worked example: TypeScript

This server-side plugin checks which scopes it holds, reads a file, handles a refused escape, and counts the newlines of a README through the async call.

**TypeScript**

```ts
import { definePlugin, PluginError, type Event, type Submission } from '@ilium/plugin-sdk';
import { listDir, read, readAll, roots } from '@ilium/plugin-sdk/host/fs';
import { log } from '@ilium/plugin-sdk/host/log';

async function countReadmeLines(): Promise<number> {
  const bytes = await readAll({ scope: 'project', path: 'README.md' });
  return bytes.filter((byte) => byte === 10).length;
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate() {},
  onEvents(_batch: Event[]): Submission[] {
    const hasProject = roots().some(([scope]) => scope === 'project');
    if (!hasProject) return [];

    const head = read({ scope: 'project', path: 'src/lib.rs' }, 0n, 1024);
    log('info', `read ${head.length} bytes`);

    const sources = listDir({ scope: 'project', path: 'src' }, 200);
    log('info', `src holds ${sources.length} entries`);

    try {
      read({ scope: 'project', path: '../.ssh/id_ed25519' }, 0n, 1024);
    } catch (error) {
      if (error instanceof PluginError && error.code === 'not-permitted') {
        log('warn', 'escape refused, as expected');
      }
    }

    countReadmeLines()
      .then((lines) => log('info', `README has ${lines} lines`))
      .catch((error) => log('warn', `README read failed: ${String(error)}`));
    return [];
  },
});
```

`offset` is a `u64`, so it is a `bigint` (`0n`) in TypeScript. `max-bytes` is a `u32` and stays a plain number.

## Edge cases and errors

| Code | When it happens for a read |
|---|---|
| `not-permitted` | The grant is missing or declined, the path leaves the scope through `..`, or a symbolic link points outside the scope root. |
| `not-found` | No entry exists at the path, or the picked file was removed. |
| `invalid-argument` | A bad path, `max-entries` of 0 or above the cap, `max-bytes` above 1 MiB, or the wrong kind, such as `list-dir` on a file. |
| `revoked` | The grant was revoked while the call was in flight. The plugin is reactivated. |
| `io` | An operating system error other than a missing file. A retry may succeed. |
| `busy` | Too many requests are pending. Only the async calls return this. |
| `quota-exceeded` | `read-all` finished with a file over the limit, or `scan` found more matches than `max-entries`. |

Behaviour the plugin can rely on:

- **Symbolic links.** `metadata` and `list-dir` report a link as kind `symlink` and do not follow it. `read` and `read-all` follow it only when the target stays inside the same scope root.
- **Hidden folder.** The project's `.ilium` folder is missing from listings, and access to it returns `not-permitted`.
- **Offset past the end.** A `read` at or beyond the end of a file returns an empty list. A read that reaches the end returns fewer bytes than `max-bytes`.
- **Dotfiles.** `scan` and `list-dir` return dotfiles too.
- **Declined scope.** The scope is absent from `roots()`, and calls on it return `not-permitted`. Other scopes keep working. A workspace search that loses its project scope should say that it needs project access, not show zero results.
- **Revocation.** Revoking the grant bumps the authorization epoch, so reads in flight fail with `revoked`.

## What the user sees

The review dialog names every scope, so the user can tell a project request from a picked file or a transcript folder. Illustrative, not generated.

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.fs 0.1.0 by Example Ltd                                        │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Reads files in this project <span class="t-dim">……………………………………………………………………</span> fs.read:project │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

A T1 bundled plugin that asks for a high-risk scope gets one prompt at first activation. Illustrative, not generated.

<pre class="term">
Plugin Session stats 0.1.0 asks for:
  [ ] fs.read:agent-sessions:claude   Claude session transcripts (high risk)
  [ ] fs.read:agent-sessions:codex    Codex session transcripts (high risk)

  <span class="t-hi">[Allow selected]</span>   [Not now]
</pre>

The diagram shows how a path becomes an allowed read:

<figure>
<svg viewBox="0 0 400 260" role="img" aria-label="A scope fixes the root, and a path is allowed only if it stays inside the root">
<title>How a scope resolves a path</title>
<desc>The grant mounts a preopen with a root. The relative path is resolved under the root. If the canonical path stays under the root the read is allowed, otherwise it is refused with not-permitted.</desc>
<defs>
<marker id="arr-fs-read" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
<path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="10" y="10" width="190" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="105" y="29" text-anchor="middle" font-size="11" fill="currentColor">Grant fs.read:project</text>
<rect x="10" y="60" width="190" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="105" y="79" text-anchor="middle" font-size="11" fill="currentColor">Preopen /project = root</text>
<rect x="10" y="110" width="190" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="105" y="129" text-anchor="middle" font-size="11" fill="currentColor">Path src/lib.rs, relative</text>
<rect x="10" y="160" width="190" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="105" y="179" text-anchor="middle" font-size="11" fill="currentColor">Canonical path under root?</text>
<rect x="10" y="220" width="100" height="30" rx="15" fill="none" stroke="currentColor"/>
<text x="60" y="239" text-anchor="middle" font-size="11" fill="currentColor">read allowed</text>
<rect x="130" y="220" width="130" height="30" rx="15" fill="none" stroke="currentColor"/>
<text x="195" y="239" text-anchor="middle" font-size="11" fill="currentColor">not-permitted</text>
<line x1="105" y1="40" x2="105" y2="60" stroke="currentColor" marker-end="url(#arr-fs-read)"/>
<line x1="105" y1="90" x2="105" y2="110" stroke="currentColor" marker-end="url(#arr-fs-read)"/>
<line x1="105" y1="140" x2="105" y2="160" stroke="currentColor" marker-end="url(#arr-fs-read)"/>
<line x1="80" y1="190" x2="60" y2="220" stroke="currentColor" marker-end="url(#arr-fs-read)"/>
<line x1="130" y1="190" x2="195" y2="220" stroke="currentColor" stroke-dasharray="3 3" marker-end="url(#arr-fs-read)"/>
<text x="225" y="120" font-size="10" fill="currentColor">A path such as ../secrets</text>
<text x="225" y="134" font-size="10" fill="currentColor">or a symlink that points</text>
<text x="225" y="148" font-size="10" fill="currentColor">out of the root fails here</text>
</svg>
<figcaption>The scope fixes the root. A relative path is allowed only while its canonical form stays inside that root.</figcaption>
</figure>

## Declined and revoked grants

When the user declines a scope, or the grant is revoked, the scope is removed from `roots()` and every call on it returns `not-permitted`. Each refused call writes one refusal line to the plugin log, which `ilium plugin logs` shows. The line names the scope and the function, and gives the reason: `declined` or `revoked`, `not-granted` when the argument is outside the grant (the manifest asked for `fs.read` but never for that scope), or `scope` when a path lies outside the granted scope. A call of `fs.read` itself without any `fs.read` request is not a refusal: it traps with `host.capability-not-requested`. See the [refusal reasons](/pages/reference-errors-and-limits/#refusal-reasons).

## See also

- [fs.write](/pages/capability-fs-write/) for the matching write grant and its rules per scope.
- [fs.watch](/pages/capability-fs-watch/) for change events on the same scopes.
- [fs.pick](/pages/capability-fs-pick/) for how `picked/<n>` scopes are created.
- [always-granted set](/pages/capability-always-granted/) for the reads that need no grant.
- [errors and limits](/pages/reference-errors-and-limits/) for every size cap.
- [panes.provide-kind](/pages/capability-panes-provide-kind/) for a file explorer, which reads the project through this grant.

Checked against the API version `ilium:plugin@0.1.0`.

### Security

The authoritative file scope table is in [Sandbox reference](/pages/reference-sandbox/) ([SEC-058] to [SEC-062]). Reads that reach a sensitive path are high risk.
