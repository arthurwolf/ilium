# Capability: workspace.manage

## workspace.manage

The `workspace.manage` capability lets a server-side plugin record Git worktrees in the kernel and release them again. A worktree is a second checkout of a repository in its own folder, so one project can have several branches checked out at the same time. An agent pane often works in a worktree of its own.

The kernel keeps a custody record for each registered worktree. The record holds the id, the folder, the branch, the owning pane or tree node, and whether a process is running in the worktree. The snapshot, the close policy and the list all read these records, so they agree on who owns what.

The kernel never runs Git. The plugin creates the branch and the folder with `git worktree add`, through [process control](/pages/api-process-control/) and the `process.run:git` grant, and then registers the result. Release removes the custody record only. The folder and the branch stay on disk until the plugin removes them itself. The host functions are listed on the [workspace](/pages/api-workspace/) page. This page covers the grant, its rules, its errors and what the user sees.

<div class="warn">
<p>Register writes a custody record that the snapshot and the close policy depend on. Release the record when the worktree is no longer needed. Grant this capability only to a plugin whose purpose is managing worktrees.</p>
</div>

### What you need

- **Grant `workspace.manage`** (risk H, no argument). It covers `register`, `release` and `list`.
- **Grant `process.run:git`** (risk H). The plugin needs it to run `git worktree add` and `git worktree remove`. `workspace.manage` does not imply it, and the kernel checks the two separately.
- **Grant `fs.read:project`** for the folder passed as `repository`.
- **Grant `fs.write:worktrees`** (risk H) for the folder passed as `path`. The `worktrees` scope is rooted at the setting `workspace.worktree_root`. Its default is `<parent of the project>/<project name>.worktrees/`.
- **Side and language.** Server side only, in Rust or TypeScript. The client world has no `workspace` import.
- **Tier.** The grant is H for every tier. A T1 bundled plugin is asked once, at its first activation. A T2 or T3 plugin shows it in the install review dialog, unchecked.

### Grant syntax

The grant takes no argument. The install check rejects the other forms with these errors:

| Grant | Result | Install error |
|---|---|---|
| `workspace.manage` | valid | none; the only form |
| `workspace.manage:git` | rejected | `manifest.grant-argument`: `capability "workspace.manage" takes no argument (got "workspace.manage:git")` |
| `workspace` | rejected | `manifest.grant-unknown`: `capability "workspace" is not known to plugin API ilium:plugin@0.1.0` |

### Surface

The interface as the kernel declares it (WIT, `ilium:host/workspace@0.1.0`). The name `%list` escapes a keyword in WIT. In code the function is `list`.

```wit
// Excerpt of ilium:host/workspace in reference-wit
record workspace-spec {
  repository: fs-path, path: fs-path, branch: string,
  base: option<string>, owner: option<node-id>,
}
record workspace-info {
  id: u64, path: fs-path, branch: string, owner: option<node-id>, in-use: bool,
}
register: func(spec: workspace-spec) -> result<u64, plugin-error>;
release: func(workspace: u64) -> result<_, plugin-error>;
%list: func() -> list<workspace-info>;
```

All three functions are synchronous. They return at once, have no request id and never return `busy`.

| Function | Rust | TypeScript | Returns | Meaning |
|---|---|---|---|---|
| `register` | `workspace::register(spec)` | `register(spec)` | `Result<u64, PluginError>`, `bigint` | Records a worktree and its owner. Returns the new record id. |
| `release` | `workspace::release(id)` | `release(id)` | `Result<(), PluginError>`, `void` | Removes the custody record. The folder and the branch stay. |
| `%list` | `workspace::list()` | `list()` | `Vec<WorkspaceInfo>`, `WorkspaceInfo[]` | Returns every record in ascending id order. |

The fields of the two records:

| Record | Field | Rust | TypeScript | Meaning |
|---|---|---|---|---|
| `WorkspaceSpec` | `repository` | `FsPath` | `FsPath` | The Git repository folder. The plugin needs `fs.read` for its scope. |
| `WorkspaceSpec` | `path` | `FsPath` | `FsPath` | The worktree folder. Normally in scope `worktrees`. |
| `WorkspaceSpec` | `branch` | `String` | `string` | The branch that the worktree checks out. |
| `WorkspaceSpec` | `base` | `Option<String>` | `base?: string` | The commit or branch the branch started from. Stored and shown by `list`. The kernel does not apply it. |
| `WorkspaceSpec` | `owner` | `Option<NodeId>` | `owner?: bigint` | The pane or tree node that owns the worktree. |
| `WorkspaceInfo` | `id` | `u64` | `bigint` | The record id that `register` returned. |
| `WorkspaceInfo` | `in-use` | `in_use: bool` | `inUse: boolean` | Whether the worktree is in use now (see below). |

Node ids are `u64` in WIT, so they are `u64` in Rust and `bigint` in TypeScript. Write them with a trailing `n` in TypeScript literals, for example `41n`.

### The custody record and in-use

`register` creates a custody record. The owner pane may close while the record lives. `release` ends it.

- **Owner.** The `owner` field names a pane or tree node. When the owner pane closes, the kernel sets `owner` to `none`. The record stays, so the plugin still sees the worktree in `list` and can release it.
- **In use.** A worktree is in use while its owner pane is open with a running process, or while any pane with a running process has a working directory inside the worktree path. An open owner pane that is idle does not make the worktree in use. The kernel evaluates this when a call runs. It is not stored.
- **Close policy.** The kernel never deletes a worktree. Closing a pane changes the record, not the folder. The plugin removes the folder with `git worktree remove` after `release`.

<figure>

<svg viewBox="0 0 360 280" role="img" aria-label="Worktree custody: the plugin calls the kernel custody record, the snapshot reads it, and Git runs through process.run:git">
  <defs>
    <marker id="wm-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="10" width="300" height="50" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="30" text-anchor="middle" font-size="12" fill="currentColor">Plugin</text>
  <text x="160" y="48" text-anchor="middle" font-size="11" fill="currentColor">calls register, release and list</text>
  <line x1="160" y1="60" x2="160" y2="84" stroke="currentColor" marker-end="url(#wm-arrow)"/>
  <rect x="10" y="84" width="300" height="60" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="106" text-anchor="middle" font-size="12" fill="currentColor">Kernel custody record</text>
  <text x="160" y="126" text-anchor="middle" font-size="11" fill="currentColor">id, path, branch, owner pane or node, in-use</text>
  <line x1="160" y1="144" x2="160" y2="172" stroke="currentColor" marker-end="url(#wm-arrow)"/>
  <rect x="10" y="172" width="300" height="48" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="192" text-anchor="middle" font-size="12" fill="currentColor">Snapshot and close policy</text>
  <text x="160" y="210" text-anchor="middle" font-size="11" fill="currentColor">read the records, never write them</text>
  <rect x="10" y="236" width="300" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="160" y="258" text-anchor="middle" font-size="11" fill="currentColor">Git commands, granted by process.run:git</text>
  <path d="M310,35 H336 V254 H311" fill="none" stroke="currentColor" marker-end="url(#wm-arrow)"/>
</svg>
<figcaption>The plugin asks the kernel to change custody records and runs Git itself. The kernel stores the records and reads them for the snapshot and the close policy. It never runs Git.</figcaption>
</figure>

### Checks and errors

`register` checks the call before it writes anything. The checks run in this order:

1. The grant is held, and `path` sits inside a write scope the plugin holds (`fs.write:worktrees` normally). Otherwise `not-permitted`.
2. The branch name has valid Git syntax. Otherwise `invalid-argument`.
3. `repository` and `path` are existing folders with a `.git` entry. The `.git` entry is the marker that `git worktree add` writes. Otherwise `not-found`.
4. `owner`, when set, names a node that exists. Otherwise `not-found`.
5. `path` is not registered already. Otherwise `conflict`.

The kernel does not check that the branch exists, because the plugin creates it before it registers. It does not apply `base` either.

| Function | Code | Cause | What the plugin does |
|---|---|---|---|
| `register` | `not-permitted` | Grant missing or declined, or `path` outside a write scope the plugin holds | Offer the user a manual route. Do not retry. |
| `register` | `invalid-argument` | Branch name breaks Git's syntax rules | Fix the name and call again. |
| `register` | `not-found` | `repository` or `path` is not a Git folder or does not exist, or `owner` is gone | Create the worktree, or fix the call. |
| `register` | `conflict` | `path` is already registered | Call `list` and reuse the existing id. |
| `release` | `not-found` | Unknown id, or the record was already released | Nothing to do. |
| `release` | `conflict` | The worktree is in use (see above) | Wait for the process to end, read `list` again, then release. |

Every function may also return the universal codes `not-permitted`, `revoked` and `internal`. `revoked` means the grant changed during the call. The plugin is then reactivated with its reduced grant set.

<div class="note">
<p>Read `list` before `release` when the plugin is unsure whether a worktree is still in use. Its `in-use` field is the same value that `release` checks.</p>
</div>

### Review dialog and what the user sees

For a T2 or T3 package, the install review dialog lists the grant in the server block. The dialog marks it as high risk and leaves it unchecked by default. The user checks it, or the plugin installs without it. The `worktrees` scope is described as "this project's worktrees".

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.workspace 0.1.0 by Example Ltd                                 │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ <span class="t-warn">! [ ] Creates and removes worktrees</span> <span class="t-dim">………………………………………………………</span> <span class="t-warn">workspace.manage</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests workspace.manage, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>

</figure>

A T1 bundled plugin is activated first without its high-risk grants. The first activation shows one prompt, "Plugin <name> asks for:", with a checkbox for `workspace.manage` and the buttons `Allow selected` and `Not now`. `Not now` asks again at the next server start, at most three times. After that the user grants it from Settings > Plugins.

The user can also inspect and revoke the grant from the command line:

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.worktrees
com.example.worktrees 0.1.0, server side
  <span class="t-err">HIGH</span>  workspace.manage    Create and track Git worktrees  <span class="t-ok">granted</span>
  <span class="t-err">HIGH</span>  process.run:git     Run git                         <span class="t-warn">declined</span>
<span class="t-prompt">$</span> ilium plugin grants com.example.worktrees --revoke workspace.manage
<span class="t-ok">ok</span>   workspace.manage revoked; the plugin is reactivated
</pre>

Revoking the grant takes effect at once. The plugin then runs with the reduced grant set. The render above is illustrative, not generated.

### Example: register and release in Rust

The plugin first runs `git worktree add -b` through process control, which creates the branch and the folder. The functions below run from a `Lifecycle` or `Handler` implementation of a server plugin.

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::workspace::{self, WorkspaceSpec};
use ilium_plugin_sdk::prelude::*;

/// Records a worktree that the plugin already created with `git worktree add -b`.
pub fn register_worktree(folder: &str, branch: &str, owner: NodeId) -> Result<u64, PluginError> {
    let spec = WorkspaceSpec {
        repository: FsPath { scope: "project".into(), path: String::new() },
        path: FsPath { scope: "worktrees".into(), path: folder.into() },
        branch: branch.into(),
        base: Some("master".into()),
        owner: Some(owner),
    };
    workspace::register(spec)
}

/// Removes the custody record only. The folder and the branch stay on disk.
pub fn release_worktree(workspace_id: u64) -> Result<(), PluginError> {
    workspace::release(workspace_id).map_err(|error| {
        let message = format!("release of worktree {workspace_id} failed: {:?}", error.code);
        log::log(Level::Warn, &message);
        error
    })
}
```

The manifest declares the server grants `workspace.manage`, `process.run:git`, `fs.read:project` and `fs.write:worktrees`.

### Example: register, list and release in TypeScript

The same flow in TypeScript. `owner` is a `bigint`, and a failed call throws `PluginError`, whose `code` field holds the error code.

**TypeScript**

```ts
import { definePlugin, PluginError } from '@ilium/plugin-sdk';
import { list, register, release } from '@ilium/plugin-sdk/host/workspace';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle } = definePlugin({
  side: 'server',
  activate() {
    for (const record of list()) {
      log('info', `worktree ${record.id} on ${record.branch}, in use: ${record.inUse}`);
    }
  },
});

/** Records a worktree that the plugin already created with `git worktree add -b`. */
export function registerWorktree(folder: string, branch: string, owner: bigint): bigint {
  return register({
    repository: { scope: 'project', path: '' },
    path: { scope: 'worktrees', path: folder },
    branch,
    base: 'master',
    owner,
  });
}

/** Removes the custody record only. The folder and the branch stay on disk. */
export function releaseWorktree(workspaceId: bigint): void {
  try {
    release(workspaceId);
  } catch (error) {
    if (error instanceof PluginError) {
      log('warn', `release of worktree ${workspaceId} failed: ${error.code}`);
    }
    throw error;
  }
}
```

### Edge cases

- **Owner pane closes.** The record survives with `owner` set to `none`. Release it when the plugin finishes with the worktree.
- **Process running in the worktree after the owner closed.** `release` returns `conflict`, because another pane with a running process has the worktree path as its working directory.
- **Grant revoked.** Calls in flight return `revoked`. Later calls return `not-permitted`. The plugin is reactivated without the grant.
- **Declined grant.** `register`, `release` and `list` return `not-permitted`. The plugin should tell the user what it cannot do and offer a manual way to create the worktree.
- **Deleting the folder by hand.** The kernel does not notice. The record stays until the plugin releases it.

### Who requests it

First-party plugins from `feature-mapping.md` use this capability:

- 67 Agent in a new Git worktree: `workspace.manage`, `process.run:git`, `fs.write:worktrees`.
- 68 Agent in an existing worktree: `workspace.manage`, `process.run:git`.
- 70 Worktree cleanup and prune: `workspace.manage`, `process.run:git`.

### See also

- [workspace](/pages/api-workspace/) for the host functions and their records.
- [process.run](/pages/capability-process-run/) for the Git commands the plugin runs.
- [process control](/pages/api-process-control/) for starting the Git commands and reading their exit status.
- [fs.write](/pages/capability-fs-write/) for the `worktrees` scope the worktree is written to.
- [fs.read](/pages/capability-fs-read/) for the `project` scope that holds the repository.
- [settings](/pages/api-settings/) for reading `workspace.worktree_root`.
- [Capabilities](/pages/capabilities/) for the grant model and the risk tiers.

Checked against `ilium:plugin@0.1.0` (API draft of 2026-10-11).

### Security

Workspace changes are audited where the audit log lists them; see [Audit log](/pages/reference-audit-log/).
