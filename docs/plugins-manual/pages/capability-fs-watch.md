---
layout: page
title: "Capability: fs.watch"
permalink: /pages/capability-fs-watch/
---

## fs.watch:&lt;scope&gt;

The `fs.watch:<scope>` grant lets a plugin ask the host to watch a folder inside one file scope and to tell it when files in that folder change. The host sends the changes as `fs-changed` events, one per watch and batch, so the plugin never has to poll the disk.

A watch names a scope and a path relative to that scope. It never names a path on the disk. A watch reads no file contents: to read a changed file, the plugin also needs `fs.read` for the same scope.

**You will need:**

- A manifest grant `fs.watch:<scope>` on the side that calls `watch`, unless the scope is `data` or `cache`, which are always granted. Grants are recorded per side.
- The `fs.read:<scope>` grant, when the plugin reads the changed files. See [fs.read](/pages/capability-fs-read/).
- The SDK for your language: `ilium-plugin-sdk` for Rust, or `@ilium/plugin-sdk` for TypeScript. Both sides can watch.
- An active plugin instance. A watch belongs to the instance that started it.

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `fs.watch:project` | yes | watches the session's project folder |
| `fs.watch:data` | yes | watches the plugin's own data folder |
| `fs.watch:path:{project}/**` | yes | a path template; `path:` must come first |
| `fs.watch:path:/srv/notes` | yes, high risk | a literal absolute path after `path:` |
| `fs.watch` | no | the scope argument is missing (`manifest.grant-argument-missing`) |
| `fs.watch:/srv/notes` | no | a bare absolute path is not a scope; write `path:/srv/notes` |
| `fs.watch:package` | no | `package` is not watchable, because it never changes |
| `fs.read:project` | no | a read grant does not allow a watch |

A grant that breaks these rules stops the install. Each error is one JSONL `error` line of `ilium plugin install` and `ilium plugin build`, with the code shown in the table above.

### Surface

| Name | Rust | TypeScript | Meaning |
|---|---|---|---|
| `watch(path, recursive)` | `fs::watch(&FsPath, bool) -> Result<u64, PluginError>` | `watch(path: FsPath, recursive: boolean): bigint` | Starts a watch on a folder and returns its id. Synchronous. |
| `unwatch(watch)` | `fs::unwatch(u64)` | `unwatch(watch: bigint): void` | Stops a watch. Synchronous. An unknown id is ignored. |
| `fs-changed` event | `Event::FsChanged(FsChangedEvent { watch, paths })` | `{ tag: 'fs-changed', val: { watch, paths } }` | The changed paths of one watch. |

The WIT declarations, from which the SDK bindings are generated, are:

```wit
type watch-id = u64;
watch: func(path: fs-path, recursive: bool) -> result<watch-id, plugin-error>;  // fs.watch
unwatch: func(watch: watch-id);
record fs-changed-event { watch: watch-id, paths: list<string> }               // fs-changed
```

Every `u64` is a `bigint` in TypeScript. The `path` argument is an `fs-path` record with a `scope` and a `path` field in both languages.

### Watchable scopes

| Scope | Grant example | Watchable |
|---|---|---|
| Project folder | `fs.watch:project` | yes; the `.ilium` folder never appears |
| Worktrees | `fs.watch:worktrees` | yes |
| Plugin data and cache | `fs.watch:data`, `fs.watch:cache` | yes |
| Picked folder | `fs.watch:picked` with `picked/<n>` | yes, folders only |
| Agent transcripts | `fs.watch:agent-sessions:claude` | yes |
| Agent configuration | `fs.watch:agent-config:<agent>` | no; refused with `manifest.grant-scope` |
| Minecraft saves | `fs.watch:minecraft-saves` | yes |
| Literal or template path | `fs.watch:path:{home}/notes/**` | yes |
| Plugin package | `fs.watch:package` | no |

### How a watch behaves

- **Window.** After the first change, the host waits 200 ms and sends one `fs-changed` event with every path changed in that time. A watch sends at most five events per second.
- **Paths.** Paths are relative to the scope root and use `/` separators, so `FsPath { scope, path }` can be built from them directly. Each path appears once per event.
- **Reach.** A non-recursive watch reports only the direct children of the folder. A recursive watch reports every changed path below it. A rename lists the old and the new path.
- **Symbolic links.** A link is reported by its own name. The host never follows it, so a change inside its target is not reported.
- **Large bursts.** An event lists at most 1,024 paths. A larger burst becomes one event whose `paths` holds only the watched path. The plugin must then read that folder again.
- **Removed folder.** When the watched folder is removed or renamed, one event lists the watched path, and the watch ends.
- **Quota.** `watches` defaults to 64 per plugin. A recursive watch counts once, however many folders it covers. The manifest may request another value under `limits`, clamped to 256 for a T1 bundled plugin and 64 for T2 and T3. A `watch` beyond the quota returns `quota-exceeded`.
- **Duplicates.** A second `watch` on the same folder returns a new id and counts separately. Each id sends its own event.
- **Lifetime.** A watch ends on `unwatch`, when its instance deactivates, restarts or is disabled, or when the plugin loses the grant. Ids do not survive a restart, so the plugin starts its watches again in `activate`.
- **Delivery.** `fs-changed` reaches only the instance that called `watch`. Events of one watch keep their order, and `fs-changed` is coalesced by watch: events of the same watch that are still pending merge, so a slow plugin sees fewer events with the newest paths and none is reported in `events-dropped` (the coalesced delivery policy defined in [events](/pages/api-events/)).

<figure>
<svg viewBox="0 0 400 280" role="img" aria-label="Watch flow from the watch call to one fs-changed event">
  <defs>
    <marker id="fsw-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="20" y="8" width="360" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="30" text-anchor="middle" font-size="12" fill="currentColor">Plugin calls watch(path, recursive)</text>
  <line x1="200" y1="44" x2="200" y2="64" stroke="currentColor" marker-end="url(#fsw-arrow)"/>
  <rect x="20" y="64" width="360" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="86" text-anchor="middle" font-size="12" fill="currentColor">Host checks the fs.watch grant and the scope</text>
  <line x1="200" y1="100" x2="200" y2="120" stroke="currentColor" marker-end="url(#fsw-arrow)"/>
  <rect x="20" y="120" width="360" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="142" text-anchor="middle" font-size="12" fill="currentColor">Host watches the folder and waits 200 ms</text>
  <line x1="200" y1="156" x2="200" y2="176" stroke="currentColor" marker-end="url(#fsw-arrow)"/>
  <rect x="20" y="176" width="360" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="198" text-anchor="middle" font-size="12" fill="currentColor">Changes in the window form one fs-changed event</text>
  <line x1="200" y1="212" x2="200" y2="232" stroke="currentColor" marker-end="url(#fsw-arrow)"/>
  <rect x="20" y="232" width="360" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="254" text-anchor="middle" font-size="12" fill="currentColor">Plugin reads the listed paths, or the folder</text>
</svg>
<figcaption>A watch stays active until the plugin calls unwatch or its instance ends.</figcaption>
</figure>

### Risk and tier rules

- `fs.watch:<scope>` is a normal-risk grant, except `fs.watch:agent-sessions:<agent>`, which is high risk like the `agent-sessions` scopes of [fs.read](/pages/capability-fs-read/): the user checks it by hand and a T1 plugin is asked at first activation.
- `fs.watch:path:` with a literal absolute path is high risk. The user checks it by hand.
- A T1 bundled plugin receives a normal grant up to its manifest ceiling. A T2 or T3 package lists the grant in the install dialog, checked by default.
- Revoking the grant withdraws every watch of the plugin. Calls in flight return `revoked`. The plugin receives `deactivate(revoked)` and is activated again with the reduced grant set. A later `watch` returns `not-permitted`.

### What the user sees

The install and permission dialog shows one row for the grant. The row text is "Watches files in this project", followed by the grant string. The row is grey-dotted, as every row of that dialog is.

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.fs 0.1.0 by Example Ltd                                        │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Watches files in this project <span class="t-dim">……………………………………………………………</span> fs.watch:project │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests fs.watch:project, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

The client never shows a prompt for a watch or for an event. A grant is a standing permission until it is revoked. To revoke it, open Settings > Plugins > <plugin> > Capabilities, or run `ilium plugin grants <id> --revoke fs.watch:project`.

### Errors

| Call | Code | When |
|---|---|---|
| `watch` | `not-permitted` | the grant is missing or declined, the path leaves the scope, or it is a symbolic link |
| `watch` | `not-found` | the folder does not exist |
| `watch` | `invalid-argument` | the scope is not watchable, or the path names a file |
| `watch` | `quota-exceeded` | the plugin already holds its `watches` limit |
| `watch` | `revoked` | the authorization changed during the call |
| `watch` | `internal` | a host bug |
| `unwatch` | none | an unknown id is ignored |

A `watch` call that fails inside `activate` makes the activation fail. The host counts it as a failure under the restart rule, and repeated failures disable the plugin.

When the grant is declined, `watch` returns `not-permitted`. The plugin keeps running without change notifications and must read the folder on demand.

### Example: a docs watcher in Rust

The manifest requests the grants and a lower watch limit:

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.docs-watch",
  "capabilities": { "server": ["fs.read:project", "fs.watch:project"] },
  "limits": { "watches": 8 }
}
```

The plugin starts the watch in `activate`, reads each changed file, and logs its size.

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

### Example: the same watch in TypeScript

The TypeScript version runs on the server side. It keeps the watch id as a `bigint`, because every `u64` is a `bigint` in TypeScript.

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { readAll, watch } from '@ilium/plugin-sdk/host/fs';
import { log } from '@ilium/plugin-sdk/host/log';

const DOCS_FOLDER = { scope: 'project', path: 'docs' };
let docsWatch: bigint | undefined;

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate(context) {
    docsWatch = watch(DOCS_FOLDER, true);
    log('info', `${context.plugin} watches the docs folder`);
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'fs-changed' || event.val.watch !== docsWatch) continue;
      for (const path of event.val.paths) {
        readAll({ scope: 'project', path })
          .then((bytes: Uint8Array) => log('info', `${path}: ${bytes.length} bytes`))
          .catch(() => log('warn', `${path} could not be read`));
      }
    }
    return [];
  },
});
```

### Who requests it

First-party plugins from [feature traceability](/pages/reference-feature-traceability/) that request a watch on the project:

- 22 File explorer and picker: `fs.watch:project`, to refresh the tree.
- 62 Chatroom: `fs.watch:project`, to notice new coordination messages.
- 69 Worktree status and branch line: `fs.watch:project`, to refresh the branch line.

### See also

- [fs.read](/pages/capability-fs-read/) to read the files that changed.
- [fs.write](/pages/capability-fs-write/) for the writes that usually trigger these events.
- [api-fs](/pages/api-fs/) for the full file API, including `fs-path`.
- [api-events](/pages/api-events/) for how events are batched and ordered.
- [Errors and limits](/pages/reference-errors-and-limits/) for every error code and limit key.
- [Manifest reference](/pages/reference-manifest/) for the `capabilities` and `limits` keys.

Checked against `ilium:plugin@0.1.0` (API draft of 2026-10-11).

### Security

Watch scopes follow the table in [Sandbox reference](/pages/reference-sandbox/) ([SEC-058]).
