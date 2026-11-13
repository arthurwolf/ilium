---
layout: page
title: "panes.create"
permalink: /pages/capability-panes-create/
---

## Overview

The `panes.create` grant lets a plugin open a new pane in the tree. A pane is either a terminal that runs a command, or a pane of a kind that the plugin or the kernel provides. The plugin asks for the pane with the `create-pane` intent. It never starts a process itself: the kernel creates the node, starts any command inside it, and reports the result.

The request is asynchronous. A plugin returns the submission from `on-events`, or calls `intents.submit` from a timer or request callback. The kernel answers later with a `request-completed` event that carries `ok-node(<id>)`. A `pane-created` event for the same node follows. A plugin that needs the new id must wait for the completion and must not assume the pane exists when the submission is returned.

<figure>
<svg viewBox="0 0 340 140" role="img" aria-label="A plugin sends create-pane to the kernel pane tree, which adds the node and returns ok-node with its id">
  <defs>
    <marker id="arr-pane" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="8" y="8" width="324" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="170" y="32" text-anchor="middle" fill="currentColor" font-size="12">Plugin (server or client side)</text>
  <line x1="130" y1="48" x2="130" y2="76" stroke="currentColor" marker-end="url(#arr-pane)"/>
  <text x="138" y="66" fill="currentColor" font-size="10">create-pane</text>
  <rect x="8" y="76" width="324" height="50" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="170" y="98" text-anchor="middle" fill="currentColor" font-size="12">Pane tree (kernel)</text>
  <text x="170" y="116" text-anchor="middle" fill="currentColor" font-size="10">adds the node and starts the command</text>
  <line x1="250" y1="76" x2="250" y2="48" stroke="currentColor" marker-end="url(#arr-pane)"/>
  <text x="258" y="66" fill="currentColor" font-size="10">ok-node(id)</text>
</svg>
<figcaption>The plugin asks the kernel for a pane with create-pane. The kernel makes the node, starts any command, and returns ok-node with the new id.</figcaption>
</figure>

## You will need

- The grant `panes.create` in the manifest, under the side that submits the intent (`server` or `client`).
- `fs.read:<scope>` for every file scope that `cwd` uses, for example `fs.read:project`. See [fs.read](/pages/capability-fs-read/).
- [terminal.write](/pages/capability-terminal-write/) when the plugin sets `initial-input`, because that text is typed into the new pane.
- [tree.read](/pages/capability-tree-read/) or [panes.read](/pages/capability-panes-read/) when the plugin must find or watch the new pane afterwards.
- Rust with the `ilium-plugin-sdk` crate, or TypeScript with `@ilium/plugin-sdk`. Both forms are shown below. Submissions are described in [intents](/pages/api-intents/).

## Grant syntax

- Valid: `panes.create`
- Invalid: `panes.create:terminal`. The grant takes no argument (`manifest.grant-argument`). The kind of pane is an intent field, not part of the grant.

## Surface

The grant opens one intent, `create-pane`. Its payload is a record named `create-pane-spec`.

| Field (WIT) | Rust (`CreatePaneSpec`) | TypeScript | Meaning |
|---|---|---|---|
| `parent: option<node-id>` | `parent: Option<NodeId>` | `parent: bigint \| undefined` | The node that holds the new pane. `none` places it after the focused pane in its group, or at the end of the session's root group when nothing has focus. |
| `title: string` | `title: String` | `title: string` | The title shown in the tree. |
| `content-kind: string` | `content_kind: String` | `contentKind: string` | `ilium.terminal` for a terminal, or a plugin pane kind (see the rules below). |
| `cwd: option<fs-path>` | `cwd: Option<FsPath>` | `cwd: FsPath \| undefined` | The start folder, as a scope and a relative path. |
| `command: option<list<string>>` | `command: Option<Vec<String>>` | `command: string[] \| undefined` | The program and its arguments. The first item is the program. |
| `initial-input: option<string>` | `initial_input: Option<String>` | `initialInput: string \| undefined` | Text typed into the pane after it starts. Needs `terminal.write`. |

`FsPath` is a record with `scope` and `path`. The path is relative to the scope root and uses `/` as separator. A path is never absolute.

The outcome arrives in a `request-completed` event. The `request` field is `submission(<correlation>)` for the submission that asked for the pane:

| Outcome | Meaning |
|---|---|
| `ok-node(node-id)` | The pane exists. Rust `Outcome::OkNode(node)`; TypeScript `{ tag: 'ok-node', val: 58n }`. |
| `duplicate(request-ref)` | The dedup key was reserved by an earlier accepted submission. Nothing was created. |
| `refused(plugin-error)` | A check failed before anything was created. The submission can be retried after the cause is fixed. |
| `failed(plugin-error)` | The kernel accepted the request but could not finish it, for example the program did not start. |
| `cancelled` | The batch was refused, or the request was cancelled. |

Keep the `node-id` from `ok-node`. Later reads and intents refer to the pane by that id.

## Field rules

| Rule | Limit | Result when broken |
|---|---|---|
| `title` | 256 characters | `refused(invalid-argument)` |
| `command` | 1,024 arguments | `refused(invalid-argument)` |
| `command` program | resolved through `PATH`, or an absolute path | `failed(io)` when the program cannot start |
| `initial-input` | 64 KiB | `refused(invalid-argument)` |
| `cwd` | scope must be granted with `fs.read`; the folder must exist | `refused(not-permitted)` or `refused(not-found)` |
| `parent` | the node must exist | `refused(not-found)` |
| `content-kind` | `ilium.terminal`, or a kind the plugin declares | `refused(invalid-argument)` |
| `dedup-key` | optional for this intent, 1 to 128 characters | retries without a key open a second pane |

With `command: none` and `content-kind: "ilium.terminal"`, the pane runs the user's login shell from the `SHELL` variable, or `/bin/sh` when `SHELL` is unset. With `cwd: none`, the pane starts in the root of the `project` scope.

A plugin's own pane kind has the content kind `<plugin-id>/<kind>`, where `<kind>` is declared in the `pane_kinds` entry of its manifest. See [panes.provide-kind](/pages/capability-panes-provide-kind/).

## Worked example: Rust, server side

This server plugin opens a review pane each time an agent finishes. It keeps the correlation numbers of its own submissions and logs the node id when the pane exists. The manifest requests `agent.read` for the activity event, `panes.create` for the new pane and `panes.create-command:git` for the `git` program it runs in that pane.

```json
{
  "capabilities": { "server": ["panes.create", "panes.create-command:git", "agent.read"] }
}
```

**Rust**

```rust
use ilium_plugin_sdk::dedup;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::intents::CreatePaneSpec;
use ilium_plugin_sdk::types::requests::RequestResult;
use ilium_plugin_sdk::types::tree::AgentActivity;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct ReviewPane {
    /// Correlation numbers of create-pane submissions still waiting for a result.
    pending: Vec<u64>,
}

impl Handler for ReviewPane {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::AgentActivityChanged(AgentActivityEvent { node, current, .. })
                    if current == AgentActivity::Done =>
                {
                    let spec = CreatePaneSpec {
                        parent: None,
                        title: "Review of agent work".into(),
                        content_kind: "ilium.terminal".into(),
                        cwd: None,
                        command: Some(vec!["git".into(), "diff".into(), "--stat".into()]),
                        initial_input: None,
                    };
                    let key = dedup::key_for(&["review", &node.to_string()]);
                    let submission = Submission::keyed(Intent::CreatePane(spec), key);
                    self.pending.push(submission.correlation);
                    submissions.push(submission);
                }
                Event::RequestCompleted(RequestResult {
                    request: RequestRef::Submission(correlation),
                    outcome: Outcome::OkNode(new_node),
                }) => {
                    if let Some(index) = self.pending.iter().position(|&c| c == correlation) {
                        self.pending.swap_remove(index);
                        log::log(Level::Info, &format!("review pane is node {new_node}"));
                    }
                }
                _ => {}
            }
        }
        submissions
    }
}
```

`Submission::keyed` sets the dedup key, so an agent that reports `done` twice in the same window does not open two panes. `Submission::new` would take the next correlation number without a key. Use `intents::submit` from a timer or request callback, where no batch is being returned.

## Worked example: TypeScript, client side

This client plugin opens a pane that tails a build log when a command is invoked. It reads the completion and logs the node id. The command must be declared in the manifest and be one of the plugin's own commands.

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { log } from '@ilium/plugin-sdk/host/log';

const OPEN_LOG_COMMAND = 'com.example.build/open-log';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate(context) {
    log('info', `ready in ${context.plugin}`);
  },
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === OPEN_LOG_COMMAND) {
        submissions.push({
          intent: {
            tag: 'create-pane',
            val: {
              parent: undefined,
              title: 'Build log',
              contentKind: 'ilium.terminal',
              cwd: { scope: 'project', path: 'logs' },
              command: ['tail', '-n', '200', 'build.log'],
              initialInput: undefined,
            },
          },
          correlation: 1n, // chosen by the plugin, unique per plugin, below 2^63
          dedupKey: 'open-log-build',
        });
      }
      if (event.tag === 'request-completed' && event.val.request.tag === 'submission') {
        if (event.val.outcome.tag === 'ok-node') {
          log('info', `build log pane is node ${event.val.outcome.val}`);
        }
      }
    }
    return submissions;
  },
});
```

The manifest for this plugin requests `panes.create` and `panes.create-command:tail` (the program in `command`) on the client side, and `fs.read:project` for the `cwd` scope:

```json
{
  "capabilities": { "client": ["panes.create", "panes.create-command:tail", "fs.read:project"] }
}
```

In TypeScript, every `u64` is a `bigint`, so `correlation` is written `1n`. The `dedupKey` is written as a plain string.

## Risk tier

- The grant is `normal` risk, not marked high.
- A T1 bundled plugin receives it automatically up to its manifest ceiling.
- A T2 or T3 plugin lists it in the review dialog, checked by default. The user can uncheck it.
- `initial-input` needs `terminal.write`, which is high risk. The user must turn that row on separately.

The grant is recorded per plugin and per side. A revoked grant withdraws any pane creation still pending and completes it as `failed(revoked)`, and the plugin is re-activated with the reduced set.

## Review dialog

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.panes 0.1.0 by Example Ltd                                     │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Creates panes <span class="t-dim">…………………………………………………………………………………………………………………</span> panes.create │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests panes.create, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

## Edge cases and errors

| Situation | Outcome |
|---|---|
| The grant is not granted or the user declined it | `refused(not-permitted)`. No pane is created and no process starts. |
| The grant is revoked while the request is pending | `failed(revoked)` for the request. The plugin is re-activated. |
| `cwd` names a scope without `fs.read` | `refused(not-permitted)` |
| `cwd` names a folder that does not exist | `refused(not-found)` |
| `cwd` names a file | `refused(not-found)` |
| `parent` names a node that was removed | `refused(not-found)` |
| `content-kind` is not declared | `refused(invalid-argument)` |
| `title` longer than 256 characters | `refused(invalid-argument)` |
| `initial-input` without `terminal.write` | The whole batch is refused as `refused(not-permitted)`. |
| The program in `command` cannot start | `failed(io)`. |
| A key was accepted within the last 10 minutes | `duplicate(<original>)`. Nothing is created. |
| The batch holds more than 256 submissions | The plugin traps. |

A refused submission does not reserve its dedup key, so a retry with the same key after fixing the cause is valid. A key that was accepted stays reserved even if the pane later fails. Retry a failed creation with a new key.

## When the grant is declined

`create-pane` completes with `refused(not-permitted)`. No pane is created, and no process starts. The plugin can still run its other features. A plugin that offers "start an agent in a new pane" should hide that action when the grant is missing. Check the grant list at activation, so the menu item is never shown for a pane that cannot open.

## What the user sees

After the kernel accepts the request, the new pane appears in the tree under its parent, with its title and status line. The command runs at once. With `parent: none`, the pane is placed after the focused pane in its group, the same place a user-created pane gets.

<figure>
<pre class="term"><span class="t-dim">session default</span>
├─ Group
│  ├─ <span class="t-dim">codex-1</span>      working
│  ├─ <span class="t-hi">Review of agent work</span>  running
│  ╰─ shell              idle
</pre>
<figcaption>Illustrative, not generated. The new pane sits in the same group as the agent that finished. Its command output is shown in the pane itself.</figcaption>
</figure>

## Minimal example

The intent payload as the kernel sees it, written as a value (not WIT), and the outcome:

```text
create-pane { parent: some(7), title: "Test run", content-kind: "ilium.terminal",
              cwd: none, command: some(["cargo", "test"]), initial-input: none }
```

```text
request-completed: request submission(1) -> ok-node(58)
```

## First-party features that request it

- Feature 23, create an agent from an editor line.
- Feature 67, agent in a new Git worktree.
- Feature 68, agent in an existing worktree.
- Feature 80, remote compaction, which creates a pane with the resume template.

Requirement ids for this page: [PAN-030], [PAN-031]. See the [requirements index](/pages/reference-requirements/).

## See also

- [panes.read](/pages/capability-panes-read/) for watching the new pane.
- [panes.signal](/pages/capability-panes-signal/) for stopping a pane the plugin created.
- [panes.provide-kind](/pages/capability-panes-provide-kind/) for plugin pane kinds.
- [terminal.write](/pages/capability-terminal-write/) for `initial-input` and its dedup key.
- [fs.read](/pages/capability-fs-read/) for the file scopes that `cwd` uses.
- [intents](/pages/api-intents/) for batches, outcomes and dedup keys.

Checked against the API version `ilium:plugin@0.1.0`.

### Security

**[SEC-040]** A plugin MAY create a pane that runs a program only with `panes.create-command:<program>` (high risk). The grant is defined in the [Capability table](/pages/reference-capabilities/) (it requires `panes.create` in the same side, else the install fails with `manifest.grant-dependency`). A `command` whose program matches no such grant completes `refused(not-permitted)` with reason `not-granted`, or `declined` when the grant was requested but unchecked. The program is not confined ([Sandbox reference](/pages/reference-sandbox/)).
