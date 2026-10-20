---
layout: page
title: workspace
permalink: /pages/api-workspace/
---

## workspace

The `workspace` import records which pane owns which Git worktree. A worktree is a second checkout of a repository in its own folder, so one project can have several branches checked out at the same time. An agent pane often works in a worktree of its own.

The kernel keeps one custody record for each registered worktree. The record holds the id, the folder, the branch, the owning pane and whether a process is running in it. The snapshot, the close policy and the worktree list all read these records, so they agree on who owns what.

The kernel never runs Git. A plugin creates the worktree with `git worktree add` through [process control](/pages/api-process-control/), then registers it here. Release removes the record only. The folder stays on disk until the plugin runs `git worktree remove`.

<div class="note">
<p>Register and release change custody records only. Creating and removing the folder are Git commands that the plugin runs itself.</p>
</div>

### What you need

- **Grant `workspace.manage`** (high risk). It lets the plugin register worktrees and release them. A registered worktree can be owned by a pane.
- **Grant `process.run:git`** (high risk). It lets the plugin run `git worktree add` and `git worktree remove`.
- **Grant `fs.read:project`** for the repository folder that `register` names.
- **Grant `fs.write:worktrees`** (high risk) for the worktree folder. The `worktrees` scope is rooted at the setting `workspace.worktree_root`. Its default is `<project name>.worktrees/` in the parent folder of the project.
- **Side and language.** Server side only, in Rust or TypeScript. The client world has no `workspace` import.

### Surface

The interface as the kernel declares it (WIT, `ilium:host/workspace@0.1.0`). The name `%list` escapes a keyword in WIT. In code the function is `list`.

```wit
// Excerpt of ilium:host/workspace in reference-wit
record workspace-spec {
  repository: fs-path, path: fs-path, branch: string, base: option<string>, owner: option<node-id>,
}
record workspace-info {
  id: u64, path: fs-path, branch: string, owner: option<node-id>, in-use: bool,
}
register: func(spec: workspace-spec) -> result<u64, plugin-error>;
release: func(workspace: u64) -> result<_, plugin-error>;
%list: func() -> list<workspace-info>;
```

All three functions are synchronous. There is no request id and nothing to await.

| Function | Rust | TypeScript | Grant | Meaning |
|---|---|---|---|---|
| `register` | `workspace::register(spec)` | `register(spec)` | `workspace.manage` | Records a worktree and its owner. Returns the new id. |
| `release` | `workspace::release(id)` | `release(id)` | `workspace.manage` | Removes the custody record. The folder and branch stay. |
| `%list` | `workspace::list()` | `list()` | `workspace.manage` | Returns every record, ordered by id. |

The fields of the two records:

| Field | Rust type | TypeScript type | Meaning |
|---|---|---|---|
| `WorkspaceSpec.repository` | `FsPath` | `FsPath` | Git repository folder. The plugin must hold `fs.read` for its scope. |
| `WorkspaceSpec.path` | `FsPath` | `FsPath` | Worktree folder, normally in the `worktrees` scope. |
| `WorkspaceSpec.branch` | `String` | `string` | Branch that the worktree checks out. |
| `WorkspaceSpec.base` | `Option<String>` | optional `string` | Commit or branch the branch starts from. Recorded for display only. |
| `WorkspaceSpec.owner` | `Option<NodeId>` | optional `bigint` | Pane or tree node that owns the worktree. |
| `WorkspaceInfo.id` | `u64` | `bigint` | Id returned by `register`. |
| `WorkspaceInfo.owner` | `Option<NodeId>` | optional `bigint` | Owner. It becomes `None` when the owner pane closes. |
| `WorkspaceInfo.in_use` | `bool` | `inUse: boolean` | True while a process runs in the worktree (see below). |

`in_use` is true while the owner pane is open with a running process. It is also true while any pane with a running process has a working directory inside the worktree path. A worktree with an idle shell is not in use.

<figure>
<svg viewBox="0 0 400 190" role="img" aria-label="The plugin runs git, which creates a worktree folder, then registers the worktree with the kernel custody records linked to its owner pane">
<title>Worktree custody flow</title>
<desc>The plugin runs git, which creates the worktree folder on disk. The plugin then calls register, and the kernel records the worktree and its owner pane. Release removes the record only.</desc>
<defs>
<marker id="arrow-workspace" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="10" y="20" width="110" height="32" rx="6" fill="none" stroke="currentColor"/>
<text x="65" y="40" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
<rect x="270" y="20" width="120" height="32" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="330" y="40" font-size="12" text-anchor="middle" fill="currentColor">Custody records</text>
<line x1="120" y1="36" x2="266" y2="36" stroke="currentColor" marker-end="url(#arrow-workspace)"/>
<text x="193" y="30" font-size="10" text-anchor="middle" fill="currentColor">register</text>
<line x1="65" y1="52" x2="65" y2="96" stroke="currentColor" marker-end="url(#arrow-workspace)"/>
<text x="72" y="78" font-size="10" fill="currentColor">process.run git</text>
<rect x="10" y="100" width="120" height="32" rx="6" fill="none" stroke="currentColor"/>
<text x="70" y="120" font-size="12" text-anchor="middle" fill="currentColor">Worktree folder</text>
<line x1="330" y1="52" x2="330" y2="96" stroke="currentColor" marker-end="url(#arrow-workspace)"/>
<text x="338" y="78" font-size="10" fill="currentColor">owner</text>
<rect x="270" y="100" width="120" height="32" rx="6" fill="none" stroke="currentColor"/>
<text x="330" y="120" font-size="12" text-anchor="middle" fill="currentColor">Owner pane</text>
<text x="10" y="162" font-size="11" fill="currentColor">Release removes the custody record only.</text>
<text x="10" y="178" font-size="11" fill="currentColor">The worktree folder stays on disk.</text>
</svg>
<figcaption>Git creates the folder. The kernel custody record ties the folder to its owner pane.</figcaption>
</figure>

### Example 1: create a worktree and register it (Rust)

The plugin runs `git worktree add` for a new branch, then records the worktree as owned by an agent pane. The caller passes the absolute host path of the new folder as a string, because git needs a host path and the SDK does not give one for a scope.

**Grants:** `workspace.manage`, `process.run:git`, `fs.read:project`, `fs.write:worktrees`

```rust
use ilium_plugin_sdk::host::process::{self, ProcessSpec};
use ilium_plugin_sdk::host::workspace::{self, WorkspaceSpec};
use ilium_plugin_sdk::prelude::*;

/// Creates a worktree for `branch` and records it as owned by `pane`.
async fn create_agent_worktree(
    repository: FsPath,
    worktree: FsPath,
    worktree_host_path: String,
    branch: String,
    pane: NodeId,
) -> Result<u64, PluginError> {
    let git = ProcessSpec {
        program: "git".to_string(),
        arguments: vec![
            "worktree".to_string(),
            "add".to_string(),
            "-b".to_string(),
            branch.clone(),
            worktree_host_path,
            "HEAD".to_string(),
        ],
        cwd: Some(repository.clone()),
        stdin: None,
        environment: Vec::new(),
        timeout_millis: 60_000,
    };
    let exit = process::run(git).await?;
    if exit.status != Some(0) {
        return Err(PluginError {
            code: ErrorCode::Io,
            message: "git worktree add exited with a non-zero status".into(),
            retry_after_ms: None,
        });
    }
    workspace::register(WorkspaceSpec {
        repository,
        path: worktree,
        branch,
        base: Some("HEAD".to_string()),
        owner: Some(pane),
    })
}
```

### Example 1: create a worktree and register it (TypeScript)

**Grants:** `workspace.manage`, `process.run:git`, `fs.read:project`, `fs.write:worktrees`

```ts
import { run } from '@ilium/plugin-sdk/host/process';
import { register } from '@ilium/plugin-sdk/host/workspace';
import type { FsPath, NodeId } from '@ilium/plugin-sdk/types';

/** Creates a worktree for `branch` and records it as owned by `pane`. */
export async function createAgentWorktree(
  repository: FsPath,
  worktree: FsPath,
  worktreeHostPath: string,
  branch: string,
  pane: NodeId,
): Promise<bigint> {
  const exit = await run({
    program: 'git',
    arguments: ['worktree', 'add', '-b', branch, worktreeHostPath, 'HEAD'],
    cwd: repository,
    stdin: undefined,
    environment: [],
    timeoutMillis: 60_000,
  });
  if (exit.status !== 0) {
    throw new Error(`git worktree add exited with status ${exit.status}`);
  }
  // register is synchronous: it returns the id or throws a PluginError.
  return register({ repository, path: worktree, branch, base: 'HEAD', owner: pane });
}
```

### Example 2: release a worktree after its pane closed (Rust and TypeScript)

When the owner pane closes, the kernel sets `owner` to `None`, and the record stays. The plugin keeps the id it received from `register` in its own [storage](/pages/api-storage/). It releases the record once no process uses the folder. A release that hits a running process returns `conflict`, so the plugin tries again later.

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::workspace;
use ilium_plugin_sdk::prelude::*;

/// Returns true when the record is gone, false when it must be tried again later.
fn release_after_close(workspace_id: u64) -> Result<bool, PluginError> {
    let Some(info) = workspace::list().into_iter().find(|record| record.id == workspace_id) else {
        return Ok(true);
    };
    if info.in_use {
        log::log(Level::Info, &format!("worktree {workspace_id} is in use, trying later"));
        return Ok(false);
    }
    match workspace::release(workspace_id) {
        Ok(()) => Ok(true),
        Err(error) if error.code == ErrorCode::Conflict => Ok(false),
        Err(error) => Err(error),
    }
}
```

```ts
import { log } from '@ilium/plugin-sdk/host/log';
import { list, release } from '@ilium/plugin-sdk/host/workspace';
import { PluginError } from '@ilium/plugin-sdk';

/** Returns true when the record is gone, false when it must be tried again later. */
export function releaseAfterClose(workspaceId: bigint): boolean {
  const info = list().find((record) => record.id === workspaceId);
  if (info === undefined) {
    return true;
  }
  if (info.inUse) {
    log('info', `worktree ${workspaceId} is in use, trying later`);
    return false;
  }
  try {
    release(workspaceId);
    return true;
  } catch (error) {
    if (error instanceof PluginError && error.code === 'conflict') {
      return false;
    }
    throw error;
  }
}
```

After `release` succeeds, the folder is still on disk. To remove it, the plugin runs `git worktree remove <path>` through `process.run:git`, as a separate step. The release record and the folder are two separate things.

### Live sessions across projects

One Ilium server runs per project session. A workspace plugin often needs to see the others: which sessions are running, and how busy they are. The `sessions` interface (server side, grant `sessions.cross`, high risk) lists the user's live sessions and runs one command of the same plugin in another session.

```wit
// Excerpt of ilium:host/sessions in reference-wit
// ilium:host/sessions
record live-session { name: string, project: string, socket-id: string, panes: u32, attached-clients: u32, is-self: bool }
list-live: func() -> list<live-session>;
record cross-request { session: string, command: string, arguments-json: string }
send: func(request: cross-request) -> result<request-id, plugin-error>;
take-reply: func(request: request-id) -> option<string>;
```

| Function | Rust (`host::sessions`) | TypeScript (`host/sessions`) | Meaning |
|---|---|---|---|
| `list-live` | `list_live()` | `listLive()` | The user's running Ilium sessions on this machine. Another user's sockets are never visible. |
| `send` | `send(request)` | `send(request)` | Runs one command of this same plugin in the named session. Returns a request id. |
| `take-reply` | `take_reply(id)` | `takeReply(id)` | The reply text once the request completed, else none. |

Rules. The target session must have the same plugin installed and enabled, with `sessions.cross` granted there. The command must be one that the plugin declares. Kernel commands of another session cannot be reached. The request carries the caller's plugin id and session name, so the receiving command can tell who asked. A target that is not found is `not-found`; a target without the plugin or grant is `not-permitted`. `take-reply` has no error channel: a completion that fails with `timed-out` or `network` is reported by the outcome of the `request-completed` event, and `take-reply` then returns none.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::sessions::{self, CrossRequest};

/// Asks every other live session to report its busy agents.
fn ask_all() -> Vec<(String, Result<RequestId, PluginError>)> {
    sessions::list_live().into_iter()
        .filter(|session| !session.is_self)
        .map(|session| {
            let request = CrossRequest {
                session: session.name.clone(),
                command: "report-busy".into(),
                arguments_json: "{}".into(),
            };
            (session.name, sessions::send(request))
        })
        .collect()
}

fn reply_for(request: RequestId) -> Option<String> {
    sessions::take_reply(request)
}
```

**TypeScript**

```ts
import * as sessions from '@ilium/plugin-sdk/host/sessions';

/** Asks every other live session to report its busy agents. */
export function askAll(): Array<{ session: string; request: bigint | Error }> {
  return sessions.listLive()
    .filter((session) => !session.isSelf)
    .map((session) => {
      try {
        const request = sessions.send({ session: session.name, command: 'report-busy', argumentsJson: '{}' });
        return { session: session.name, request };
      } catch (error) {
        return { session: session.name, request: error as Error };
      }
    });
}

export function replyFor(request: bigint): string | undefined {
  return sessions.takeReply(request);
}
```

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium workspace-overview report
<span class="t-hi">session</span>      <span class="t-hi">project</span>                 <span class="t-hi">panes</span>  <span class="t-hi">clients</span>
default      ~/dev/ai/ilium            9        1
docs         ~/dev/ai/ilium/docs       3        0   <span class="t-dim">no client attached</span>
website      ~/dev/web/site            5        1
</pre>
<figcaption>Illustrative output of a command that joins <code>list-live</code> with the replies to <code>send</code>. The columns are the fields of <code>live-session</code>.</figcaption>
</figure>

<figure>
<svg viewBox="0 0 420 110" role="img" aria-label="A plugin in one session sends a command to the same plugin in another session">
  <title>Cross-session request</title>
  <desc>Session default sends a request through the host to session docs, where the same plugin runs the declared command and returns a reply.</desc>
  <defs><marker id="ws-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M0 0 L10 5 L0 10 z" fill="currentColor"/></marker></defs>
  <rect x="8" y="30" width="130" height="50" rx="6" fill="none" stroke="currentColor"/><text x="73" y="52" font-size="11" text-anchor="middle" fill="currentColor">session default</text><text x="73" y="68" font-size="10" text-anchor="middle" fill="currentColor">plugin: send()</text>
  <rect x="282" y="30" width="130" height="50" rx="6" fill="none" stroke="currentColor"/><text x="347" y="52" font-size="11" text-anchor="middle" fill="currentColor">session docs</text><text x="347" y="68" font-size="10" text-anchor="middle" fill="currentColor">same plugin, command</text>
  <line x1="138" y1="45" x2="282" y2="45" stroke="currentColor" marker-end="url(#ws-arrow)"/><text x="210" y="39" font-size="10" text-anchor="middle" fill="currentColor">request</text>
  <line x1="282" y1="65" x2="138" y2="65" stroke="currentColor" marker-end="url(#ws-arrow)"/><text x="210" y="82" font-size="10" text-anchor="middle" fill="currentColor">reply (take-reply)</text>
</svg>
<figcaption>The request is limited to commands that the plugin declares in both sessions.</figcaption>
</figure>

### What the user sees

Pane 14 owns worktree 7. The plugin logs each step in `ilium plugin logs`.

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs demo-worktrees
2026-10-11 12:04:11 <span class="t-ok">ok</span>   registered worktree 7 for pane 14
2026-10-11 12:09:40 <span class="t-warn">warn</span> worktree 7 is in use, trying later
2026-10-11 12:09:52 <span class="t-ok">ok</span>   released worktree 7, folder kept
2026-10-11 12:12:03 <span class="t-err">error</span> release 9: not-found, no such id
</pre>

Illustrative, not generated. Each line is a plugin log entry. The retry in the second line is the plugin's own decision, based on `in_use`.

### Errors and edge cases

| Call | Code | When it happens |
|---|---|---|
| any | `not-permitted` | The `workspace.manage` grant is missing, or the folder is outside the granted scope. |
| `register` | `not-permitted` | The `path` lies outside `fs.write:worktrees`, or `repository` lies outside a read grant. |
| `register` | `not-found` | The repository is not a Git repository, or the `owner` node does not exist. |
| `register` | `conflict` | The `path` is already registered. |
| `register` | `invalid-argument` | The branch name is empty or not a valid Git branch name. |
| `send` | `not-found` | The target session is not running. |
| `send` | `not-permitted` | The target has not got the plugin installed and enabled with `sessions.cross` granted, the command is a kernel command, or the command is not one that the plugin declares. |
| `send` | `invalid-argument` | `arguments-json` is not valid JSON. |
| `release` | `not-found` | The id is not registered. A second release of the same id gives this too. |
| `release` | `conflict` | The worktree is in use (see `in_use`). Try again after the process ends. |

Edge cases:

- If `register` fails after git has created the folder, the folder stays on disk with no record. Remove it with `git worktree remove`, then try again.
- If the owner pane closes while a process still runs, `owner` becomes `None` and `release` returns `conflict` until the process ends.
- `base` is written to the record and shown in the list. The kernel does not use it. The plugin passes the same `base` it gave `git worktree add`.
- The records are the kernel's view. A plugin that has forgotten a worktree id can find the record again with `list`, matching on `path`.
- Every refusal is logged with the plugin id and function, and shows in `ilium plugin logs`.

### Used by

- Agent in a new Git worktree: calls `register` after `process.run` creates the worktree.
- Agent in an existing worktree: lists worktrees with `list`.
- Worktree cleanup and prune: calls `release` after `process.run` removes the worktree, behind a confirmation `popup`.

### See also

- Live sessions use the grant `sessions.cross`; see the [capability reference](/pages/reference-capabilities/).
- [Process control](/pages/api-process-control/) for the git commands and their grants.
- [Capability: workspace.manage](/pages/capability-workspace-manage/) for the grant and its risk.
- [File access](/pages/api-fs/) for the `worktrees` and `project` scopes.
- [Settings](/pages/api-settings/) for `workspace.worktree_root`.
- [Plugin exports](/pages/api-exports/) for the server side world that holds this import.

Matches `ilium:host/workspace@0.1.0`.
