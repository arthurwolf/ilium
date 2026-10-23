---
layout: page
title: "Capability: fs.write"
permalink: /pages/capability-fs-write/
---

## fs.write:&lt;scope&gt;

The `fs.write:<scope>` capability lets a plugin create, replace and append files, create folders, remove entries and rename files inside one file scope. A scope is a named root that the host hands to the plugin, such as the project folder, the plugin's private `data` folder or a path the user picked. Every write path is resolved inside that root. A path that tries to leave the root through `..` or a symbolic link is refused.

Writes are asynchronous. A call checks the grant, the scope, the path syntax and the size cap at once, then returns a `request-id`. The disk result arrives later as a `request-completed` event.

<div class="note">
<p>Grants are per side. A plugin that writes from its server side needs the grant in <code>capabilities.server</code>. A client-side write needs it in <code>capabilities.client</code>. A scene can write its <code>data</code> and <code>cache</code> scopes without a grant; any other scope is declared in <code>capabilities.scene</code>.</p>
</div>

**You will need:** a manifest grant `fs.write:<scope>`, or no grant for the always-granted `data` and `cache` scopes. Read [fs.read](/pages/capability-fs-read/) first, because every write path is resolved the same way as a read path. The calls themselves are documented on [fs](/pages/api-fs/).

SDK paths: Rust `ilium_plugin_sdk::host::fs`, TypeScript `@ilium/plugin-sdk/host/fs`. Paths use the `FsPath` type: a struct in Rust, and a `{ scope, path }` object in TypeScript.

### Grant syntax

The grant is `fs.write:<scope>`. The argument is one scope name from the table in the next section, or `path:<template>`. The manifest checks the grant when the package is installed.

| Grant | Valid | Reason |
|---|---|---|
| `fs.write:project` | yes | the session's project folder, high risk |
| `fs.write:path:{project}/**` | yes | a template with the `{project}` placeholder and a trailing `/**`, high risk |
| `fs.write:worktrees` | yes | the worktree folder, high risk; `{worktrees}` names the same root |
| `fs.write:data` | yes | always granted, so no manifest entry is needed; not logged |
| `fs.write` | no | `manifest.grant-argument-missing`: needs an argument, for example `fs.write:project` |
| `fs.write:/home/me/notes` | no | `manifest.grant-scope`; an absolute path needs `path:<template>` |
| `fs.write:package` | no | `manifest.grant-scope`; the `package` scope is read only |
| `fs.write:agent-sessions:claude` | no | `manifest.grant-scope`; agent transcripts are read only |
| `fs.write:agent-config:claude` | no | `manifest.grant-scope`; agent configuration is changed only through a proposal, see [agent-config.propose](/pages/capability-agent-config-propose/) |
| `fs.read:project` | no | a read grant does not allow writes |

A literal absolute path inside `path:` (for example `fs.write:path:/srv/logs/**`) is high risk and, as in the sandbox scope table, is available to all tiers.

### Scopes

| Scope | Root on disk | Grant to write | Risk |
|---|---|---|---|
| `project` | the session's project folder, without the `.ilium` folder | `fs.write:project` | H |
| `worktrees` | the worktree folder from `workspace.worktree_root` | `fs.write:worktrees` | H |
| `path:<template>` | the folders a template names | `fs.write:path:<template>` | H |
| `agent-config:<agent>` (`claude`, `codex`, `gemini`, `antigravity`) | each agent's configuration folder | none: `fs.write:agent-config:<agent>` is refused at install with `manifest.grant-scope`. A plugin proposes changes with [`agent-config.propose:<agent>`](/pages/capability-agent-config-propose/), and the host writes only what the user accepts | H for the proposal grant |
| `picked/<n>` | the file or folder the user chose | `fs.write:picked` for a save pick | H, granted per pick |
| `data` | the plugin's private data folder | always granted | none |
| `cache` | the plugin's private cache folder, which the host may evict | always granted | none |
| `package`, `agent-sessions:*` | read only | not writable | refused at install |

The placeholders for `path:<template>` are `{home}`, `{config}`, `{data}`, `{cache}`, `{project}` and `{worktrees}`. A template ends with a name, which covers that file or folder, or with `/**`, which covers the whole subtree. A bare `*` is refused in every position; a trailing `/**` is the only wildcard form, so a manifest that needs both agent configuration folders lists both grants.

### Functions

| Function | Rust (`fs::`) | TypeScript | Kind | Result |
|---|---|---|---|---|
| `write(path, data, mode)` | `write(&FsPath, Vec<u8>, WriteMode)` | `write(path, Uint8Array, mode)` | async | `ok`, or `failed(code)` |
| `create-dir(path)` | `create_dir(&FsPath)` | `createDir(path)` | sync | `Result<(), PluginError>` |
| `remove(path)` | `remove(&FsPath)` | `remove(path)` | async | `ok`, or `failed(code)` |
| `rename(source, target)` | `rename(&FsPath, &FsPath)` | `rename(source, target)` | async | `ok`, or `failed(code)` |

The Rust and TypeScript async wrappers return `()` or `void` on `ok`, and an error on `failed(code)`. The raw calls that return a request id stay available as `write_request`, `remove_request` and `rename_request`.

`create-dir` is synchronous and not recursive. It creates one folder. An existing folder counts as success, so a call can be repeated safely.

### Write modes

The `write-mode` enum has three cases. In Rust they are `WriteMode::CreateNew`, `WriteMode::Replace` and `WriteMode::Append`. In TypeScript they are the strings `'create-new'`, `'replace'` and `'append'`.

- `create-new` creates the file. When the file already exists the completion is `failed(conflict)`, and the file is not changed.
- `replace` writes a temporary sibling file and renames it over the target. A reader sees either the old content or the new content, never a partial file. It also creates the file when it is absent.
- `append` adds bytes to the end of the file and creates the file when it is absent.

One `write` call takes at most `fs_write_mib` (16 MiB by default). Write a larger file with several `append` calls, or produce it in a job.

### Completion

A finished write arrives in a batch as a `request-completed` event. Its `request-result` names the call and gives the outcome:

| Outcome | Meaning |
|---|---|
| `ok` | The file, folder or entry was changed as requested. |
| `failed(code)` | The disk or the current state refused the call. The code is in the errors table. |
| `cancelled` | The plugin stopped, the user cancelled, or the host shut down. |

```
request-result { request: call(41), outcome: ok }
```

The SDK turns this event into the result of the async wrapper, so most plugins never read it directly.

<figure>
<svg viewBox="0 0 400 280" role="img" aria-label="Write call flow through the grant check, scope root, operation and completion event">
  <defs>
    <marker id="fsw-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="20" y="8" width="360" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="30" text-anchor="middle" font-size="11" fill="currentColor">Plugin calls write(path, data, mode)</text>
  <line x1="200" y1="44" x2="200" y2="64" stroke="currentColor" marker-end="url(#fsw-arrow)"/>
  <rect x="20" y="64" width="360" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="86" text-anchor="middle" font-size="11" fill="currentColor">Host checks the grant, the size cap and the path syntax</text>
  <line x1="200" y1="100" x2="200" y2="120" stroke="currentColor" marker-end="url(#fsw-arrow)"/>
  <rect x="20" y="120" width="360" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="142" text-anchor="middle" font-size="11" fill="currentColor">Path is resolved inside the scope root</text>
  <line x1="200" y1="156" x2="200" y2="176" stroke="currentColor" marker-end="url(#fsw-arrow)"/>
  <rect x="20" y="176" width="360" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="198" text-anchor="middle" font-size="11" fill="currentColor">Operation runs: create-new, replace or append</text>
  <line x1="200" y1="212" x2="200" y2="232" stroke="currentColor" marker-end="url(#fsw-arrow)"/>
  <rect x="20" y="232" width="360" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="254" text-anchor="middle" font-size="11" fill="currentColor">request-completed reports ok or failed(code)</text>
</svg>
<figcaption>A write call passes the grant check, the size cap and the scope check before the disk is touched. Synchronous errors return from the call. Disk errors arrive in the completion.</figcaption>
</figure>

### Errors

Every write function may also return `not-permitted`, `revoked` and `internal`. Synchronous errors come back from the call. Errors that need the disk or the user arrive in the completion.

| Code | Where | When |
|---|---|---|
| `not-permitted` | call | No grant, the grant was declined, or the path escapes the scope root or a symbolic link leaves it. |
| `invalid-argument` | call | A bad path, data over `fs_write_mib`, an empty `path` outside a `picked/<n>` scope, a rename across two scopes, or `remove` on a scope root. |
| `busy` | call | The plugin already has `pending_requests` (64 by default) open. |
| `quota-exceeded` | call | The CPU budget is spent; `retry-after-ms` is set. |
| `revoked` | call | The grant was revoked while the call was running. |
| `conflict` | completion | `create-new` found a file, a folder has the target name, another holder has a lock on the path, or a rename targets a non-empty folder. |
| `not-found` | completion | The parent folder is missing, or the source of a rename or remove does not exist. |
| `quota-exceeded` | completion | The plugin's own `data` plus `cache` disk limit (`disk_mib`, 256 MiB by default) is full. A physically full disk instead completes with `io` and the message `no-space`. |
| `io` | completion | An operating system error other than a missing file. |
| `revoked` | completion | A write still pending when its grant is revoked completes as `failed(revoked)`. |

`create-dir` is the exception. It is synchronous, so `conflict` (a file has that name), `not-found` (the parent is missing) and `io` come back from the call.

### Limits

| Key | Default | T1 ceiling | T2 and T3 ceiling | Effect |
|---|---|---|---|---|
| `fs_write_mib` (one call) | 16 | 64 | 16 | `invalid-argument` above the limit |
| `disk_mib` (`data` plus `cache`) | 256 | 4,096 | 256 | `failed(quota-exceeded)` |
| `pending_requests` | 64 | 256 | 64 | `busy` above the limit |

A plugin asks for a higher value in its manifest. The host clamps the request to the ceiling and writes a log line, so a T2 plugin cannot write more than 16 MiB in one call:

```json
"limits": { "fs_write_mib": 32 }
```

### Risk, review and logging

- Writes outside the `data` and `cache` scopes are high risk (H). A bundled T1 plugin with an H grant is first activated without it. The user then sees one prompt per plugin, `Plugin <name> asks for: <list>`, with the buttons `Allow selected` and `Not now`.
- A T2 or T3 package shows every requested grant in the review dialog. H rows start unchecked, and the user checks each one by hand. The dialog is 70 columns wide and each row shows its wording and its grant string.
- A grant is a standing permission. There is no prompt per call.
- Revoking the grant in Settings > Plugins, or with `ilium plugin grants <id> --revoke <grant>`, fails calls in flight with `revoked`. The plugin then restarts with the reduced grant set.
- Every `write`, `remove`, `rename` and `create-dir` outside `data` and `cache` is written to the plugin log at level info, with the scope, the path, the operation and the byte count. `ilium plugin logs <id>` shows them.

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.fs 0.1.0 by Example Ltd                                        │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ <span class="t-warn">! [ ] Writes files in this project</span> <span class="t-dim">…………………………………………………………</span> <span class="t-warn">fs.write:project</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests fs.write:project, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

#### Diagnosing a failed write

Every failure is one of two things: a code returned by the call, or a code in the `request-completed` outcome that arrives later. The plugin log records the grant refusals, the host lines and the plugin's own lines. Use this table to match a symptom to the code and the log line to search.

| Symptom | Code and where it arrives | Search the plugin log for | Next step |
|---|---|---|---|
| Writes always fail with `not-permitted` | Call, reason `declined` | A refusal line that names the grant, for example `fs.write:project`, and ends with `declined` | `ilium plugin info <id>` lists the grants. The user grants the row in Settings > Plugins. |
| Write to a path outside the scope, or through a symbolic link | Call, `not-permitted`, reason `scope` | A refusal line that ends with `scope` | Fix the path in the plugin. The grant does not change. |
| Writes stop after the user revokes a grant | Call `revoked`, or completion `failed(revoked)` for a write still pending | The re-activation that follows the revocation. `ilium plugin info <id>` shows the grants that remain. | Expected behaviour. The plugin must run without the grant. |
| `busy` or `quota-exceeded` at the call | Call only | None defined by the host. A plugin that needs a record must log the code itself. | For `busy`, wait for pending requests to finish (`pending_requests`, 64 by default). For `quota-exceeded`, wait `retry-after-ms`. Then retry. |
| `failed(quota-exceeded)` on a completion | Completion | The plugin's own line for the failed write | `ilium plugin stats <id>` shows use against `disk_mib`. Free space in `data` or `cache`. The limit comes from the plugin's manifest and the tier ceiling, so only the plugin author can raise it. |
| `failed(io)` with the message `no-space` | Completion | The plugin's own line for the failed write | The physical disk is full. Free disk space, then retry. |
| `failed(conflict)` or `failed(not-found)` | Completion | The info line `write <scope> <path> <mode>`, which is written when the call is made, plus the plugin's own warning | `create-new` found a file, or the parent folder is missing. Check the path. |
| `cancelled` on a write | Completion | `ilium plugin status <id>` shows a restart or disable at that time | The plugin stopped, the user cancelled, or the host shut down. It is not a failure, and it does not count toward the restart rule. Start the work again. |
| Plugin disabled after repeated failures | A notice, and the plugin side is disabled | The first trap in `ilium plugin logs <id> --tail 200`, because later restarts repeat it | Disable happens on the third failure within 10 minutes on that side. Fix the first trap. |

The info line is written when the call is made, before the completion arrives. A line in the log therefore does not show that the write succeeded. Check the completion outcome before deciding what happened.

For a problem that happens only at a low log level, raise the threshold before reproducing it. The default threshold is `info`, which drops `debug` lines. Run `ilium plugin log-level <id> debug`, then `ilium plugin log-level <id> --reset` afterwards ([log-level](/pages/reference-plugin-cli/#logs-and-log-level)).

#### What to collect from a user

The manual defines no single support-bundle file. A support report is the set of command outputs listed in [Supporting your users](/pages/guide-publishing-and-support/#supporting-your-users), with the [bug-report template](/pages/guide-publishing-and-support/#a-template-for-bug-reports) filled in. For a file-write problem, ask for:

- `ilium plugin logs <id> --tail 200`, which is the main evidence.
- `ilium plugin status <id>` and `ilium plugin stats <id>`, for the failure window and the use against each limit.
- `ilium plugin info <id>`, for the version and the granted rows.
- `ilium plugin doctor`, when the sandbox or the host may be the cause.

The log lines name file paths and byte counts, not file contents. Ask the user to check the paths for private names before sharing them. Do not ask for unredacted trace output or for file contents. A byte count is enough.

### When the grant is declined

A declined grant links the write imports to stubs that return `not-permitted` at once. The plugin keeps running and can show the error to the user. A write call whose capability the manifest never requested traps the instance instead, because that is a plugin bug.

### Examples

Both examples assume this manifest, with the grant on the server side:

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.notes",
  "capabilities": { "server": ["fs.write:project"] }
}
```

**Example 1: replace a note atomically (Rust).** The `replace` mode means that a reader never sees a half-written note. A failed write comes back in `Err`, and the plugin logs its `code`.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::fs::{self, WriteMode};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::exports::Activation;

const NOTE_PATH: &str = "notes/today.md";

// Replaces the note in one step. A reader sees either the old or the new file.
async fn save_note(body: String) -> Result<(), PluginError> {
    let target = FsPath { scope: "project".into(), path: NOTE_PATH.into() };
    fs::write(&target, body.into_bytes(), WriteMode::Replace).await
}

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct NoteSaver;

impl Lifecycle for NoteSaver {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        let body = format!("# Notes\n\nPlugin {} started.\n", context.plugin);
        task::spawn(async move {
            if let Err(error) = save_note(body).await {
                log::log(Level::Error, &format!("save failed: {:?}", error.code));
            }
        });
        Ok(())
    }
}

impl Handler for NoteSaver {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}
```

**Example 2: create an export without overwriting it (TypeScript).** `create-new` fails with `conflict` when the file exists. The plugin treats that as a normal result and leaves the existing file alone.

**TypeScript**

```ts
import { definePlugin, PluginError } from '@ilium/plugin-sdk';
import { createDir, write } from '@ilium/plugin-sdk/host/fs';

const encoder = new TextEncoder();

// Creates exports/<name> and never overwrites an existing file.
async function createExport(name: string, body: string): Promise<'created' | 'exists'> {
  createDir({ scope: 'project', path: 'exports' });
  try {
    const target = { scope: 'project', path: `exports/${name}` };
    await write(target, encoder.encode(body), 'create-new');
    return 'created';
  } catch (error) {
    if (error instanceof PluginError && error.code === 'conflict') {
      return 'exists';
    }
    throw error;
  }
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate(context) {
    const body = `# Summary\n\nFrom ${context.plugin}.\n`;
    createExport('summary.md', body).catch((error: unknown) => {
      console.error('export failed', error);
    });
  },
  onEvents: () => [],
});
```

### What the user sees

Writes outside `data` and `cache` appear in the plugin log. The user reads them with the plugin's logs:

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs com.example.notes
<span class="t-dim">2026-10-11 09:14:02</span> <span class="t-ok">info</span>  write project notes/today.md replace 1842 bytes
<span class="t-dim">2026-10-11 09:14:09</span> <span class="t-ok">info</span>  write project exports/summary.md create-new 96 bytes
<span class="t-dim">2026-10-11 09:14:09</span> <span class="t-warn">warn</span>  create-new conflict on exports/summary.md, file kept
</pre>
<figcaption>Illustrative, not generated. A plugin log after two writes. The warning is the plugin's own line, not a host line.</figcaption>
</figure>

### Who requests it

First-party plugins from [feature traceability](/pages/reference-feature-traceability/):

- 32 Agent debug history appends to the `data` scope, which needs no grant.
- 37 Configure agent: no `fs.write` grant on agent configuration (`fs.write:agent-config:<agent>` is refused at install with `manifest.grant-scope`). It proposes each edit with `agent-config.propose:claude` and `agent-config.propose:codex`, and the user approves the diff ([agent-config.propose](/pages/capability-agent-config-propose/)).
- 62 Chatroom: `fs.write:project`, used for appends.
- 63 Agent setup: `fs.write:project` for instruction files in the project folder. Files in an agent's configuration folder are changed only through `agent-config.propose:claude` and `agent-config.propose:codex`, with the user approving each diff.
- 67 Agent in a new Git worktree: `fs.write:worktrees`.
- 71 Worktree include copy: `fs.write:worktrees`.
- 77 Convert a session: no write grant on `agent-sessions` or `agent-config`. Transcripts are read only, and a manifest that asks for a write is refused at install. The converted Claude Code transcript is proposed as a new file with `agent-config.propose:claude`, and the host writes it when the user accepts.
- 82 Compaction optimiser: no `fs.write` grant on agent configuration. **Apply** and **Revert** propose the threshold change with `agent-config.propose:claude` and `agent-config.propose:codex`; the host keeps the previous content in the plugin's `data` scope as the history entry that **Revert** proposes back.

Requirement ids for this page: [CAP-008]. See the [requirements index](/pages/reference-requirements/).

### See also

- [fs.read](/pages/capability-fs-read/) for the read side of the same scopes and the path rules.
- [fs.watch](/pages/capability-fs-watch/) to react to changes made by other writers.
- [fs.pick](/pages/capability-fs-pick/) for saving to a path the user chose.
- [agent-config.propose](/pages/capability-agent-config-propose/) for changing agent configuration through a user-approved proposal.
- [fs](/pages/api-fs/) for the full function surface, including locks.
- [Workspace manage](/pages/capability-workspace-manage/) for worktrees, which need `fs.write:worktrees`.
- [Requests and outcomes](/pages/requests-and-outcomes/) for how completions are delivered.
- [Errors and limits](/pages/reference-errors-and-limits/) for the common error codes.

### Security

The authoritative file scope table is in [Sandbox reference](/pages/reference-sandbox/) ([SEC-058] to [SEC-062]). Writes outside `data` and `cache` are high risk. Agent configuration is never written directly; see [agent-config.propose](/pages/capability-agent-config-propose/).
