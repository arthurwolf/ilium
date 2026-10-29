# agent.read

## Overview

The `agent.read` grant lets a plugin see the agent CLIs that run in terminal panes. For each pane it can read which agent runs there (for example Claude Code or Codex), what that agent is doing, its objective and its goal. It also receives events when an agent is identified, when its activity changes and when its goal changes.

The grant is read only. Sending keys or text to an agent needs [terminal.write](/pages/capability-terminal-write/) or [agent.control](/pages/capability-agent-control/). Most agent status features depend on this grant: status badges, sound alerts, desktop notifications, session statistics and automatic pane titles.

## You will need

- The grant `agent.read` on the server side, the client side, or both. The agent primitive runs on both sides. The grant is requested per side, so a server plugin that only reads activity requests it on the server side only.
- The SDK for your language: the Rust crate `ilium-plugin-sdk`, or the npm package `@ilium/plugin-sdk` for TypeScript.
- An agent pane whose agent the kernel recognises. The built-in signature ids are `claude`, `codex` and `antigravity`.
- Optional grants, each requested only when the plugin uses the matching feature:
  - [tree.read](/pages/capability-tree-read/) or [panes.read](/pages/capability-panes-read/) to show agents next to tree rows or panes.
  - [agent.signatures](/pages/capability-agent-signatures/) to add the plugin's own agent signatures.
  - [agent.read-prompts](/pages/capability-agent-read-prompts/) for the prompts the user submits. This grant is high risk.
  - `fs.read` on `agent-sessions:<agent>` to read a transcript. This grant is high risk. See [fs.read](/pages/capability-fs-read/).

## Grant syntax

- Valid: `agent.read`
- Invalid: `agent.read:codex`. The agent name is a field of each record, not a grant argument.

## Functions

The functions come from the agent primitive, [primitives-agent](/pages/primitives-agent/). The SDK import paths are listed in that page.

| Function | Rust | TypeScript | Returns | Meaning |
|---|---|---|---|---|
| `agents()` | `agent::agents()` | `agents()` | `Vec<AgentPane>` / `AgentPane[]` | One record for each pane where an agent is identified. |
| `view(pane)` | `agent::view(pane)` | `view(pane)` | `Option<AgentView>` / `AgentView \| undefined` | The view of one pane, or none. |
| `session(pane)` | `agent::session(pane)` | `session(pane)` | `Option<SessionRef>` / `SessionRef \| undefined` | A reference to the agent's transcript file, or none. |

The pane argument is a `NodeId`. In TypeScript every node id is a `bigint`, so write `view(42n)`. None of these functions returns a `result`, so none of them has an error code. See [behaviour when the grant is declined](#behaviour-when-the-grant-is-declined).

## Types

| Type | Fields | Meaning |
|---|---|---|
| `AgentPane` | `pane: NodeId`, `view: AgentView` | A pane and its agent view. |
| `AgentView` | `signature: string`, `activity: AgentActivity`, `objective: option<string>`, `goal: option<string>` | The agent in one pane. `signature` is the id of the matched signature, for example `codex`. |
| `SessionRef` | `scope: string`, `path: string`, `session-id: string` | A path inside the `agent-sessions:<agent>` scope. It names the transcript; it is not the transcript. |
| `AgentActivity` | enum, see below | The state the detector reads from the screen. |

Plugin signatures have a global id of the form `<plugin-id>.<id>`, for example `example.notes.review-bot`. The `signature` field carries that id.

### Activity values

| Activity | Rust | TypeScript | Meaning |
|---|---|---|---|
| `none` | `AgentActivity::None` | `'none'` | No agent is identified yet, or the screen has not been classified. |
| `working` | `AgentActivity::Working` | `'working'` | The agent is processing a turn. |
| `idle` | `AgentActivity::Idle` | `'idle'` | The agent waits for the next prompt. |
| `awaiting-approval` | `AgentActivity::AwaitingApproval` | `'awaiting-approval'` | The agent asks the user to approve an action. |
| `done` | `AgentActivity::Done` | `'done'` | The agent has finished its task. |
| `menu` | `AgentActivity::Menu` | `'menu'` | The agent shows a selection menu. |

## Events

The grant opens three events. Two of them are never dropped. `agent-goal-changed` is coalesced by node.

| Event | Payload | Delivery |
|---|---|---|
| `agent-identified` | `node: NodeId`, `signature: string` | Never dropped. |
| `agent-activity-changed` | `node: NodeId`, `previous: AgentActivity`, `current: AgentActivity`, `revision: u64` | Never dropped. |
| `agent-goal-changed` | `node: NodeId`, `goal: option<string>` | Coalesced by node: one event per pane in a batch, at the position of its newest change. |

Two ordering rules apply. `agent-identified` for a pane always arrives before any `agent-activity-changed` for that pane. Before identification, a pane's activity is `none`, and no activity event is sent for it. In Rust the payloads are `AgentIdentifiedEvent`, `AgentActivityEvent` and `AgentGoalEvent` in `ilium_plugin_sdk::types::events`. In TypeScript they arrive as `{ tag: 'agent-activity-changed', val: { ... } }`.

<figure>
<svg viewBox="0 0 400 230" role="img" aria-label="Agent state flows from the pane screen through the detector and the agent view to the agent.read check and then to the plugin">
<defs>
<marker id="agent-read-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="10" y="10" width="120" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="70" y="29" font-size="12" text-anchor="middle" fill="currentColor">Pane screen</text>
<line x1="130" y1="25" x2="170" y2="25" stroke="currentColor" marker-end="url(#agent-read-arrow)"/>
<rect x="170" y="10" width="130" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="235" y="29" font-size="12" text-anchor="middle" fill="currentColor">Detector</text>
<line x1="235" y1="40" x2="235" y2="70" stroke="currentColor" marker-end="url(#agent-read-arrow)"/>
<text x="242" y="58" font-size="10" fill="currentColor">classify</text>
<rect x="170" y="70" width="130" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="235" y="89" font-size="12" text-anchor="middle" fill="currentColor">agent-view</text>
<line x1="235" y1="100" x2="235" y2="130" stroke="currentColor" marker-end="url(#agent-read-arrow)"/>
<text x="242" y="118" font-size="10" fill="currentColor">view, events</text>
<rect x="120" y="130" width="230" height="30" rx="4" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="235" y="149" font-size="11" text-anchor="middle" fill="currentColor">agent.read check</text>
<line x1="120" y1="145" x2="98" y2="145" stroke="currentColor" marker-end="url(#agent-read-arrow)"/>
<text x="53" y="122" font-size="10" text-anchor="middle" fill="currentColor">declined</text>
<rect x="8" y="130" width="90" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="53" y="149" font-size="10" text-anchor="middle" fill="currentColor">none, empty</text>
<line x1="235" y1="160" x2="235" y2="190" stroke="currentColor" marker-end="url(#agent-read-arrow)"/>
<text x="242" y="178" font-size="10" fill="currentColor">granted</text>
<rect x="170" y="190" width="130" height="30" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="235" y="209" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
</svg>
<figcaption>The kernel classifies each pane screen into an agent view. The grant check decides whether the plugin receives the view and the events. Without the grant the reads return none or an empty list.</figcaption>
</figure>

## Manifest

The manifest requests the grant on the side where the code runs. This excerpt requests it on the server side:

```json
{
  "capabilities": {
    "server": ["agent.read"]
  }
}
```

## Worked example 1: approval watch in Rust

This server plugin logs each pane where an agent starts waiting for approval. It reads the signature with `view`, because the event carries only the node id.

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::agent;
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::tree::AgentActivity;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct ApprovalWatch;

impl Handler for ApprovalWatch {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::AgentActivityChanged(AgentActivityEvent { node, current, .. }) =
                event
            else {
                continue;
            };
            if current != AgentActivity::AwaitingApproval {
                continue;
            }
            // view() is synchronous and returns None when the grant is declined.
            let Some(view) = agent::view(node) else {
                continue;
            };
            log::log(Level::Info, &format!("{} is waiting for approval", view.signature));
        }
        Vec::new()
    }
}
```

The handler returns no submissions, so it changes nothing in the terminal. It only writes to the plugin log.

## Worked example 2: list agents in TypeScript

This server plugin counts the agent panes when it starts, then logs each identification. It imports `agents` from the agent primitive and `log` from the logger.

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { agents } from '@ilium/plugin-sdk/primitives/agent';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate(context) {
    const found = agents();
    log('info', `${context.plugin}: ${found.length} agent pane(s) at start`);
  },
  onEvents(batch) {
    for (const event of batch) {
      if (event.tag === 'agent-identified') {
        log('info', `agent ${event.val.signature} identified on a pane`);
      }
    }
    return [];
  },
});
```

`agents()` returns an empty array when the grant is declined, so the count is 0 and the plugin keeps running.

## Behaviour when the grant is declined

The agent functions have no error code. A declined grant gives an empty result instead:

| Call or event | Result without the grant |
|---|---|
| `agents()` | Empty list. |
| `view(pane)` | None. |
| `session(pane)` | None. |
| `agent-identified`, `agent-activity-changed`, `agent-goal-changed` | Not delivered. |
| `node-view.agent` in [tree.read](/pages/capability-tree-read/) | None. |

The grant is checked on every call. A grant revoked while the plugin runs takes effect on the next call.

<div class="warn">

A plugin that treats an empty result as "no agent is running" shows a false idle state when the grant is declined. Hide the status display instead of showing an idle state the plugin cannot confirm.

</div>

## What the user sees

When a plugin asks for `agent.read`, the review dialog lists the grant. The user can leave it unchecked. Nothing is shown in the terminal until the plugin acts on the data.

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.agent 0.1.0 by Example Ltd                                     │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Reads agent status and model <span class="t-dim">………………………………………………………………………………</span> agent.read │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests agent.read, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

With the approval watch running, `ilium plugin logs` prints lines like these:

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs example.approval-watch --tail 3
<span class="t-dim">2026-10-11 09:14:02</span> <span class="t-ok">info </span> example.approval-watch activated as server
<span class="t-dim">2026-10-11 09:14:31</span> <span class="t-ok">info </span> example.approval-watch codex is waiting for approval
<span class="t-dim">2026-10-11 09:16:08</span> <span class="t-ok">info </span> example.approval-watch claude is waiting for approval
</pre>

## Edge cases

- A pane whose process matches no signature has no agent view. `view` returns none, and no activity events are sent for it.
- Plugin signatures are reported with their global id, `<plugin-id>.<id>`. Built-in ids keep their bare names. When a user-configured signature and a plugin signature match the same process name, the user's signature wins, and the overlap is logged as a warning.
- The `session` result is a path. Reading the file it names needs `fs.read` on the matching `agent-sessions:<agent>` scope. Without that grant the file read fails with `not-permitted`, not the `session` call.
- A batch can hold many `agent-goal-changed` events for one pane. Only the newest goal is delivered, so a plugin never acts on a stale goal from that batch.
- Activity events for a pane always come after its `agent-identified` event, even when both are in one batch.

## First-party features that request it

Feature 9 (workspace search), feature 28 (pane status signals), feature 29 (goal state tracking), feature 30 (display modes and custom signatures), feature 31 (session stats), feature 32 (agent debug history), feature 34 (agent icons and identifiers), feature 35 (agent toolbar), feature 36 (agent messaging), feature 38 (spend and quota indicators), feature 41 (automatic pane titles), feature 47 (sound alerts), feature 48 (desktop notifications) and feature 49 (coalescing of redundant alerts) request this grant. Feature 33, the last-prompt banner, needs `agent.read` together with `agent.read-prompts`.

## See also

- [primitives-agent](/pages/primitives-agent/) for the full agent primitive, including `last-prompt` and `register-signature`.
- [agent.read-prompts](/pages/capability-agent-read-prompts/) for the prompts that the user submits.
- [agent.signatures](/pages/capability-agent-signatures/) for telling the kernel how to recognise an agent.
- [agent.control](/pages/capability-agent-control/) for sending an agent its own action keys.
- [api-events](/pages/api-events/) for the event envelope and the `events-dropped` event.
- [tutorial-01-agent-badge](/pages/tutorial-01-agent-badge/) for a complete plugin that shows agent activity on tree rows.
