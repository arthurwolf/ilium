# Pane lifecycle and tree restructuring

## Pane lifecycle and tree restructuring

Intents can change what a pane is doing and where nodes sit in the tree. A plugin can freeze a pane's process and thaw it later, replace the command a pane runs, stop the process but keep the pane, resize a pane, create groups, folders, projects and splits, reorder many nodes in one undoable batch, mark a pane for attention, fix a title with its source recorded, and open a link in the desktop browser. All of them are intents: the plugin submits them and reads the outcome. None of them is a call that returns a result at once. See [requests and outcomes](/pages/requests-and-outcomes/).

The host keeps the hard parts to itself: starting and signalling processes, the durable resume binding that lets a pane come back after a restart, the layout solver, and the attention policy. The plugin states what it wants; the host decides whether it is possible.

**You will need:**

- `panes.lifecycle` (high risk) for `freeze-pane`, `unfreeze-pane`, `replace-pane-command`, `terminate-pane-process` and `resize-pane`.
- `tree.restructure` for `create-group`, `create-folder`, `create-project`, `create-split`, `restructure` and `revert-restructure`.
- `tree.write` for `set-bookmarked` and for `set-expanded` on real nodes, and `tree.decorate` for `set-inferred-icon`.
- `tree.attention` for `mark-attention` and `ack-attention`.
- `open-external` for the intent of the same name.
- `panes.read` to observe the result through `pane-lifecycle-changed`.
- Both sides of a plugin can submit these intents. The grants are listed in the [capability reference](/pages/reference-capabilities/); related grants are [panes.create](/pages/capability-panes-create/), [panes.signal](/pages/capability-panes-signal/) and [tree.write](/pages/capability-tree-write/).

### States of a pane process

<figure>
<svg viewBox="0 0 440 150" role="img" aria-label="Pane process states: running, frozen, restarting, exited, with the intents that move between them">
  <title>Pane process states</title>
  <desc>Running moves to frozen with freeze-pane and back with unfreeze-pane. Running moves to restarting with replace-pane-command and back to running. Running moves to exited with terminate-pane-process or when the process ends. Exited moves to restarting with replace-pane-command.</desc>
  <defs><marker id="pl-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M0 0 L10 5 L0 10 z" fill="currentColor"/></marker></defs>
  <rect x="10" y="50" width="90" height="36" rx="6" fill="none" stroke="currentColor"/><text x="55" y="73" font-size="12" text-anchor="middle" fill="currentColor">running</text>
  <rect x="170" y="8" width="90" height="36" rx="6" fill="none" stroke="currentColor"/><text x="215" y="31" font-size="12" text-anchor="middle" fill="currentColor">frozen</text>
  <rect x="170" y="104" width="90" height="36" rx="6" fill="none" stroke="currentColor"/><text x="215" y="127" font-size="12" text-anchor="middle" fill="currentColor">restarting</text>
  <rect x="340" y="50" width="90" height="36" rx="6" fill="none" stroke="currentColor"/><text x="385" y="73" font-size="12" text-anchor="middle" fill="currentColor">exited</text>
  <line x1="100" y1="58" x2="170" y2="30" stroke="currentColor" marker-end="url(#pl-arrow)"/><text x="88" y="30" font-size="10" fill="currentColor">freeze-pane</text>
  <line x1="170" y1="40" x2="100" y2="66" stroke="currentColor" marker-end="url(#pl-arrow)" stroke-dasharray="3 3"/>  <line x1="100" y1="78" x2="170" y2="116" stroke="currentColor" marker-end="url(#pl-arrow)"/><text x="40" y="118" font-size="10" fill="currentColor">replace-pane-command</text>
  <line x1="100" y1="68" x2="340" y2="68" stroke="currentColor" marker-end="url(#pl-arrow)"/><text x="190" y="62" font-size="10" fill="currentColor">terminate-pane-process</text>
  <line x1="385" y1="86" x2="260" y2="122" stroke="currentColor" marker-end="url(#pl-arrow)"/><text x="290" y="112" font-size="10" fill="currentColor">replace-pane-command</text>
  <text x="104" y="8" font-size="10" fill="currentColor">dashed: unfreeze-pane</text>
</svg>
<figcaption>The four values of <code>pane-lifecycle-changed.state</code>. Restarting ends in running again once the new process has started.</figcaption>
</figure>

### Surface

All eleven lifecycle and restructure intents, with their payloads and outcomes. The catalogue lives in [intents](/pages/api-intents/); this page is where each is explained.

```wit
// Excerpt of ilium:types/intents in reference-wit
// types::intents
record freeze-pane-spec { pane: node-id }
record unfreeze-pane-spec { pane: node-id }
record replace-pane-command-spec { pane: node-id, command: list<string>, cwd: option<fs-path>, keep-scrollback: bool }
record terminate-pane-process-spec { pane: node-id, grace-ms: u32 }     // at most 10,000
record resize-pane-spec { pane: node-id, columns: option<u16>, rows: option<u16> }
record create-group-spec { parent: option<node-id>, title: string, placement: placement, children: list<node-id> }
record create-folder-spec { parent: option<node-id>, title: string, path: option<fs-path> }
record create-project-spec { title: string, root: fs-path }
record create-split-spec { panes: list<node-id>, direction: split-direction }   // 2 to 4 panes
record set-bookmarked-spec { node: node-id, bookmarked: bool }
record restructure-spec { operations: list<restructure-op>, label: string }    // at most 256
record mark-attention-spec { node: node-id, reason: string }                   // at most 80 characters
record open-external-spec { target: external-target }
variant external-target { url(string), file(fs-path), settings(string) }
```

| Intent | Grant | Outcome |
|---|---|---|
| `freeze-pane` | `panes.lifecycle` | `ok`; process stopped, output stops, pane shows the frozen mark. `refused(unsupported)` on Windows. |
| `unfreeze-pane` | `panes.lifecycle` | `ok`; `refused(conflict)` when the pane is not frozen. |
| `replace-pane-command` | `panes.lifecycle` | `ok` once the new process runs. |
| `terminate-pane-process` | `panes.lifecycle` | `ok` when the process has exited; the pane stays open with its exit status. |
| `resize-pane` | `panes.lifecycle` | `ok`; `refused(conflict)` when the layout cannot give the size. |
| `create-group`, `create-folder`, `create-project`, `create-split` | `tree.restructure` | `ok-node` with the new node. |
| `restructure` | `tree.restructure` | `ok-node` carrying an undo token (0 when there is nothing to undo). |
| `revert-restructure` | `tree.restructure` | `ok`; `refused(conflict)` when a later change touched the same nodes. |
| `set-bookmarked` | `tree.write` | `ok` |
| `set-inferred-icon` | `tree.decorate` | `ok`; `refused(conflict)` when a manual icon exists. |
| `mark-attention`, `ack-attention` | `tree.attention` | `ok` |
| `open-external` | `open-external` | `ok` after the desktop handler started. |

### Freeze and unfreeze

`freeze-pane` stops the pane's process with a stop signal on Linux and macOS. Output stops, and the pane draws a frozen mark: the snowflake `❄` in the status slot, one of the default icon slots. The terminal grid stays as it was. `unfreeze-pane` continues the process. A plugin that wants to hold an agent while a quota resets freezes it and sets a durable timer for the unfreeze.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{FreezePaneSpec, Intent, UnfreezePaneSpec};

fn pause_until_reset(pane: NodeId, reset_at: EpochMs) -> Result<Vec<Submission>, PluginError> {
    // A durable timer survives a restart of Ilium; its handler later submits the unfreeze.
    timers::at(reset_at, &format!("unfreeze-{pane}"))?;
    // The key makes a repeat within 10 minutes a harmless `duplicate`. The key table is in memory, so a restart clears it; a freeze does not survive a restart either, so check the pane state before submitting again.
    Ok(vec![Submission::keyed(Intent::FreezePane(FreezePaneSpec { pane }), format!("freeze-{pane}"))])
}

fn resume(pane: NodeId) -> Submission {
    Submission::keyed(Intent::UnfreezePane(UnfreezePaneSpec { pane }), format!("unfreeze-{pane}"))
}
```

**TypeScript**

```ts
import { at } from '@ilium/plugin-sdk/host/timers';
import type { Submission } from '@ilium/plugin-sdk';

export function pauseUntilReset(pane: bigint, resetAt: bigint): Submission[] {
  at(resetAt, `unfreeze-${pane}`); // durable: fires after a restart too
  return [{ intent: { tag: 'freeze-pane', val: { pane } }, correlation: 1n, dedupKey: `freeze-${pane}` }];
}

export function resume(pane: bigint): Submission {
  return { intent: { tag: 'unfreeze-pane', val: { pane } }, correlation: 2n, dedupKey: `unfreeze-${pane}` };
}
```

### Replace the command, terminate the process

`replace-pane-command` starts a new command in an existing pane. `keep-scrollback: true` keeps the old output above the new one. If `cwd` is none, the pane's directory stays. The pane keeps its node id, title, place in the tree and size; the durable resume binding is the host's. During the swap the pane is `restarting`.

`terminate-pane-process` asks the process to end, waits `grace-ms` (at most 10,000 ms), then kills it. The outcome is `ok` when the process has exited. The pane stays open and shows the exit status, so the user can read the last output.

**Rust**

```rust
fn restart_with_model(pane: NodeId, model: &str) -> Submission {
    Submission::new(Intent::ReplacePaneCommand(ReplacePaneCommandSpec {
        pane,
        command: vec!["claude".into(), "--model".into(), model.into()],
        cwd: None,
        keep_scrollback: true,
    }))
}

fn stop_politely(pane: NodeId) -> Submission {
    Submission::new(Intent::TerminatePaneProcess(TerminatePaneProcessSpec { pane, grace_ms: 3_000 }))
}
```

**TypeScript**

```ts
export function restartWithModel(pane: bigint, model: string): Submission {
  return {
    intent: { tag: 'replace-pane-command', val: { pane, command: ['claude', '--model', model], cwd: undefined, keepScrollback: true } },
    correlation: 1n,
    dedupKey: undefined,
  };
}

export function stopPolitely(pane: bigint): Submission {
  return { intent: { tag: 'terminate-pane-process', val: { pane, graceMs: 3000 } }, correlation: 2n, dedupKey: undefined };
}
```

### Transitions, restore and exit

Every state change and its trigger, in one table [PAN-001]:

| From | To | Trigger | Event |
|---|---|---|---|
| running | frozen | `freeze-pane` | `pane-lifecycle-changed` state `frozen` |
| frozen | running | `unfreeze-pane` | state `running` |
| running | restarting | `replace-pane-command` | state `restarting`, then `running` when the new process started |
| running | exited | `terminate-pane-process`, or the process ends | `pane-exited`, then state `exited` |
| exited | restarting | `replace-pane-command` | state `restarting`, then `running` |
| frozen | exited | `terminate-pane-process`, or the process ends | `pane-exited` |

A pane restored from a snapshot has no process. It is neither running nor frozen until its process starts; intents that need a process are refused with `pane-not-ready` [PAN-011]. The manual does not promise that a freeze survives a server restart; a plugin that needs a frozen pane checks the state from `pane-lifecycle-changed` after the pane goes live. When the user closes a pane, the order for its node is `pane-exited`, then `node-removed`. `terminate-pane-process` alone leaves the node in the tree.

`create-pane` with a `cwd` folder that does not exist is refused with `not-found` [PAN-030]. A program that cannot start completes as `failed(io)`. The first check happens at acceptance, and the second after the pane exists.

### Resize with refusal

`resize-pane` asks for a size in cells. Either field may be none to keep that dimension. When the layout cannot give the size, the outcome is `refused(conflict)` and the message names the neighbour that limits it, for example `pane 15 is at its minimum height`. A plugin should treat the refusal as information, not as a failure to retry.

**Rust**

```rust
fn widen(pane: NodeId, columns: u16) -> Submission {
    Submission::new(Intent::ResizePane(ResizePaneSpec { pane, columns: Some(columns), rows: None }))
}
```

**TypeScript**

```ts
export function widen(pane: bigint, columns: number): Submission {
  return { intent: { tag: 'resize-pane', val: { pane, columns, rows: undefined } }, correlation: 1n, dedupKey: undefined };
}
```

### Creating groups, folders, projects and splits

`create-group` makes a group under `parent` and moves `children` into it. `create-folder` makes a folder, optionally bound to a directory. `create-project` makes a top-level project with a root directory. `create-split` joins 2 to 4 panes into a split container: `direction` is vertical or horizontal. The outcome `ok-node` carries the new node's id, which can go straight into later intents.

**Rust**

```rust
fn group_agents(agents: Vec<NodeId>) -> Submission {
    Submission::new(Intent::CreateGroup(CreateGroupSpec {
        parent: None,
        title: "Agents".into(),
        placement: Placement::InsideLast,
        children: agents,
    }))
}

fn side_by_side(left: NodeId, right: NodeId) -> Submission {
    Submission::new(Intent::CreateSplit(CreateSplitSpec { panes: vec![left, right], direction: SplitDirection::Vertical }))
}
```

**TypeScript**

```ts
export function groupAgents(agents: bigint[]): Submission {
  return {
    intent: { tag: 'create-group', val: { parent: undefined, title: 'Agents', placement: 'inside-last', children: agents } },
    correlation: 1n,
    dedupKey: undefined,
  };
}

export function sideBySide(left: bigint, right: bigint): Submission {
  return { intent: { tag: 'create-split', val: { panes: [left, right], direction: 'vertical' } }, correlation: 2n, dedupKey: undefined };
}
```

<figure>
<pre class="term"><span class="t-dim">before</span>                          <span class="t-dim">after group_agents</span>
▼ 📂 Ilium                      ▼ 📂 Ilium
›   🦀 ⠋ Fix auth               › ▼ 📂 Agents
›   🐢 ● Docs pass              › ›   🦀 ⠋ Fix auth
›   🖥️    Shell                 › ›   🐢 ● Docs pass
                                ›   🖥️    Shell
</pre>
<figcaption>The tree before and after <code>group_agents</code>. The group takes the folder-style icon of a group; its children move one level in.</figcaption>
</figure>

### Restructure batches with undo

`restructure` applies up to 256 operations as one step. The operations are `move`, `create-group`, `ungroup` and `rename`. The batch is all or nothing: if one operation fails, nothing changes, and the outcome is `refused(conflict)` with the index of the failing operation in the message. On success the outcome is `ok-node`, and the node id is an undo token. `revert-restructure { token }` undoes the batch, unless a later change touched the same nodes, which is `refused(conflict)`. The `label` is the name that the user sees in the undo entry.

<figure>
<svg viewBox="0 0 420 110" role="img" aria-label="A restructure batch is applied whole or refused whole, and can be reverted by token">
  <title>Restructure batch</title>
  <desc>A list of operations enters the host. If all succeed the tree changes once and a token returns. If any fails nothing changes. A later revert with the token restores the previous tree.</desc>
  <defs><marker id="rs-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M0 0 L10 5 L0 10 z" fill="currentColor"/></marker></defs>
  <rect x="8" y="35" width="100" height="40" rx="6" fill="none" stroke="currentColor"/><text x="58" y="59" font-size="11" text-anchor="middle" fill="currentColor">restructure (n ops)</text>
  <rect x="170" y="8" width="110" height="34" rx="6" fill="none" stroke="currentColor"/><text x="225" y="29" font-size="11" text-anchor="middle" fill="currentColor">all ok: ok-node(token)</text>
  <rect x="170" y="68" width="110" height="34" rx="6" fill="none" stroke="currentColor"/><text x="225" y="89" font-size="11" text-anchor="middle" fill="currentColor">one fails: refused</text>
  <rect x="320" y="8" width="92" height="34" rx="6" fill="none" stroke="currentColor"/><text x="366" y="29" font-size="11" text-anchor="middle" fill="currentColor">revert(token)</text>
  <line x1="108" y1="50" x2="170" y2="26" stroke="currentColor" marker-end="url(#rs-arrow)"/>
  <line x1="108" y1="60" x2="170" y2="84" stroke="currentColor" marker-end="url(#rs-arrow)"/>
  <line x1="280" y1="25" x2="320" y2="25" stroke="currentColor" marker-end="url(#rs-arrow)"/>
</svg>
<figcaption>One batch, one undo step. A refused batch leaves the tree exactly as it was.</figcaption>
</figure>

**Rust**

```rust
use ilium_plugin_sdk::types::intents::{RestructureOp, RestructureSpec};

/// Moves every idle pane under one group in a single undoable step.
fn tidy(idle: &[NodeId], archive: NodeId) -> Submission {
    let operations = idle.iter().map(|&node| RestructureOp::Move {
        node,
        target: archive,
        placement: Placement::InsideLast,
    }).collect();
    Submission::new(Intent::Restructure(RestructureSpec { operations, label: "Archive idle panes".into() }))
}

fn undo(token: u64) -> Submission {
    Submission::new(Intent::RevertRestructure(token))
}
```

**TypeScript**

```ts
export function tidy(idle: bigint[], archive: bigint): Submission {
  return {
    intent: {
      tag: 'restructure',
      val: {
        operations: idle.map((node) => ({ tag: 'move' as const, val: { node, target: archive, placement: 'inside-last' as const } })),
        label: 'Archive idle panes',
      },
    },
    correlation: 1n,
    dedupKey: undefined,
  };
}

export function undo(token: bigint): Submission {
  return { intent: { tag: 'revert-restructure', val: { token } }, correlation: 2n, dedupKey: undefined };
}
```

The token comes from the outcome `ok-node` in `request-completed`. A plugin that offers an Undo button keeps it with the label.

### Titles, icons, bookmarks and attention

- `rename-node` gains two fields: `source` (`plugin` or `inferred`, required) and `expected-revision` (optional). A stale revision is `refused(conflict)`. A title with source `inferred` never replaces a title the user typed.
- `set-inferred-icon { node, icon, source }` sets an icon from a catalogue id or a glyph. A manual icon wins and the outcome is `refused(conflict)`.
- `set-bookmarked { node, bookmarked }` sets the bookmark star `★` on a node.
- `mark-attention { node, reason }` marks a node as needing attention, with a reason of at most 80 characters. The host owns what attention looks like (the done prefix, the sound). `ack-attention { node }` clears the plugin's mark. Changes arrive as `attention-changed { node, marked, reason, by }`.

**Rust**

```rust
use ilium_plugin_sdk::types::tree::TitleSource;

fn name_from_content(node: NodeId, revision: u64, inferred: &str) -> Submission {
    Submission::new(Intent::RenameNode(RenameNodeSpec {
        node,
        title: inferred.into(),
        source: TitleSource::Inferred,
        expected_revision: Some(revision), // from NodeView.revision
    }))
}

fn needs_you(node: NodeId, reason: &str) -> Submission {
    Submission::new(Intent::MarkAttention(MarkAttentionSpec { node, reason: reason.into() }))
}
```

**TypeScript**

```ts
export function nameFromContent(node: bigint, revision: bigint, inferred: string): Submission {
  return {
    intent: { tag: 'rename-node', val: { node, title: inferred, source: 'inferred', expectedRevision: revision } },
    correlation: 1n,
    dedupKey: undefined,
  };
}

export function needsYou(node: bigint, reason: string): Submission {
  return { intent: { tag: 'mark-attention', val: { node, reason } }, correlation: 2n, dedupKey: undefined };
}
```

### Open a link in the desktop browser

`open-external` takes a target: `url(string)`, `file(fs-path)` or `settings(string)` for an `ilium://settings/...` address. Only `http`, `https` and `mailto` schemes are allowed for URLs. The host checks the scheme list and asks the user once per target origin and plugin; there is no other allowlist. The outcome is `ok` after the desktop handler started, or `refused(not-permitted)` when the user declines. A `file` target must be an absolute path of an existing file or folder, else `refused(invalid-argument)`. A `settings` target must start with `ilium://settings/` and name an existing settings page, else `refused(invalid-argument)`.

**Rust**

```rust
fn open_status_page() -> Submission {
    Submission::new(Intent::OpenExternal(OpenExternalSpec {
        target: ExternalTarget::Url("https://status.example.com".into()),
    }))
}
```

**TypeScript**

```ts
export function openStatusPage(): Submission {
  return {
    intent: { tag: 'open-external', val: { target: { tag: 'url', val: 'https://status.example.com' } } },
    correlation: 1n,
    dedupKey: undefined,
  };
}
```

### Events

| Event | Payload | Grant | Delivery |
|---|---|---|---|
| `pane-lifecycle-changed` | `pane`, `state` (`running`, `frozen`, `restarting`, `exited`), `by` | `panes.read` | Never dropped. |
| `attention-changed` | `node`, `marked`, `reason`, `by` | `tree.attention` or `tree.read` | Coalesced by node. |
| `pane-exited` | `node`, `status`, `signal` | `panes.read` | Never dropped. |

### Errors and limits

| Situation | Result |
|---|---|
| `freeze-pane` on Windows | `refused(unsupported)` |
| `unfreeze-pane` on a pane that is not frozen | `refused(conflict)` |
| `freeze-pane` on a pane that is exited, restarting or already frozen | `refused(conflict)` |
| `create-split` with fewer than 2 or more than 4 panes | `refused(invalid-argument)` |
| `open-external` with a `file` target that is not an absolute existing path, or a `settings` target outside `ilium://settings/` | `refused(invalid-argument)` |
| `terminate-pane-process` with `grace-ms` above 10,000 | `refused(invalid-argument)` |
| `resize-pane` the layout cannot give | `refused(conflict)`, message names the neighbour |
| Intent to a restored pane whose process has not started | `refused(pane-not-ready)` |
| `restructure` with more than 256 operations | `refused(invalid-argument)` |
| `restructure` with a failing operation | `refused(conflict)`, message holds the operation index; nothing changed |
| `revert-restructure` after a later change touched the nodes | `refused(conflict)` |
| `rename-node` with a stale `expected-revision` | `refused(conflict)` |
| `set-inferred-icon` over a manual icon | `refused(conflict)` |
| `open-external` with a scheme other than http, https or mailto | `refused(not-permitted)` |
| Any of these without its grant | `refused(not-permitted)` |

<div class="warn">
<p><code>panes.lifecycle</code> is a high-risk grant. A plugin with it can stop and replace the processes that the user runs. The user sees it in the install review, and every use is written to the plugin log.</p>
</div>

Requirement ids for this page: [EVT-005], [PAN-001], [PAN-002], [PAN-003], [PAN-004], [PAN-005], [PAN-006], [PAN-007], [PAN-020], [PAN-021], [PAN-022], [PAN-023], [PAN-024], [PAN-025], [PAN-026], [PAN-032]. See the [requirements index](/pages/reference-requirements/).

### See also

- [tree](/pages/api-tree/) for reading nodes, and [tree and window chrome](/pages/api-tree-chrome/) for decorations and the places that show attention.
- [panes](/pages/api-panes/) for pane facts and the basic pane intents.
- [Intents](/pages/api-intents/) and [requests and outcomes](/pages/requests-and-outcomes/) for submissions, keys and outcomes.
- [Clock and timers](/pages/api-clock-and-timers/) for durable timers.
