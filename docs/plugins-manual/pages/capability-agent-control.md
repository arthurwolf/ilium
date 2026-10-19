---
layout: page
title: "agent.control"
permalink: /pages/capability-agent-control/
---

The `agent.control` grant lets a plugin press an agent's own action keys, such as the key sequence that asks Claude Code to compact its context. The plugin names an action id, such as `compact`, and never names keys. The kernel finds the keys in the pane's agent signature, checks the agent's current activity, and writes the keys to the pane. The same grant lets the plugin enable, disable or relabel its toolbar buttons for each agent pane.

The grant is high risk (H). An action can clear the agent's context, change its mode or answer a prompt, and the user does not see the keys go by.

<div class="warn">

An agent action can clear the agent's context, change its mode or answer a prompt on the user's behalf. Only the plugin's own toolbar and command code should use this grant, and each action should be something the user asked for in that moment.

</div>

## What you need

- The grant `agent.control`, requested on each side (server or client) whose code calls the functions. Grants are per side.
- The grant [agent.read](/pages/capability-agent-read/) when the plugin reads activity itself, for example to decide whether a button is enabled. The kernel checks the activity of every action on its own, so sending an action does not need `agent.read`.
- An action id declared in the `actions` map of a signature. Built-in signatures and the `signatures` contribution of any plugin both qualify. The intent carries no keys, so a plugin cannot send keys that no signature declares.

Grant strings take no argument. The manifest check refuses the scoped form with `manifest.grant-argument`:

```text
capability "agent.control" takes no argument (got "agent.control:compact")
```

The action is named in the intent, and the kernel checks it against the signature.

## The surface

| Surface | Type | Meaning |
|---|---|---|
| `agent-action` intent | record `agent-action-spec { pane: node-id, action-id: string }` | Presses the keys that the pane's signature declares for `action-id`. The submission needs a `dedup-key`. |
| `set-toolbar-state` | `func(button: string, pane: option<node-id>, enabled: bool, label: option<string>) -> result<_, plugin-error>` | Sets the enabled state and an optional label of one toolbar button the plugin contributed. |
| `actions` in a signature | map of action id to `{ keys: string, allowed_when?: list<agent-activity> }` | Declares the keys for each action. `keys` is at most 1 KiB. `allowed_when` lists the activities in which the action may run. |
| `requires_action` on a toolbar button | string | Shows the button only on panes whose signature declares that action. |
| `agent-activity` | enum | The activity the kernel reports for an agent pane (table below). |

The SDK names are `AgentActionSpec` in `ilium_plugin_sdk::types::intents` (Rust) and the intent `agent-action` with `{ pane, actionId }` in TypeScript. The function is `ilium_plugin_sdk::primitives::agent::set_toolbar_state` in Rust, and `setToolbarState` from `@ilium/plugin-sdk/primitives/agent` in TypeScript. Both return a `PluginError` on failure.

| Activity | Meaning |
|---|---|
| `none` | No agent is identified in the pane yet. |
| `working` | The agent is busy with a turn. |
| `idle` | The agent waits for the next prompt. |
| `awaiting-approval` | The agent shows a permission prompt. |
| `done` | The agent has finished its turn. |
| `menu` | The agent shows a menu or a picker. |

The signature's detector assigns the activity from the screen. The plugin does not set it.

## Declaring actions

A signature lists its actions with the keys they press. An action with `allowed_when` runs only in the listed activities. Without the key, the default list applies: `["idle", "done", "awaiting-approval", "menu"]`. A `compact` action therefore runs when the agent is idle, done, waiting for approval or showing a menu, and is refused while it is working.

```json
{
  "signatures": [{
    "id": "example-agent",
    "process_names": ["example-agent"],
    "actions": {
      "compact": { "keys": "/compact\r", "allowed_when": ["idle", "done"] }
    }
  }],
  "toolbar_buttons": [{
    "id": "compact",
    "icon": "compress",
    "label": "Compact",
    "command": "com.example.compact/run",
    "applies_to": "all",
    "requires_action": "compact"
  }],
  "capabilities": {
    "server": ["agent.read", "agent.control"]
  }
}
```

The `requires_action` key hides the Compact button on every pane whose signature has no `compact` action. The button then does not appear for agents that cannot compact.

## Submitting an action

The kernel checks a submission in this order. The first failing check decides the outcome.

1. The submission is well formed. Its `dedup-key` is present and uses 1 to 128 characters from `[A-Za-z0-9._:/-]`. A malformed submission refuses its whole batch with `invalid-argument`.
2. The plugin holds `agent.control`. Otherwise the batch is refused with `not-permitted`.
3. The key is not reserved. A reserved key completes as `duplicate(original)` with no effect.
4. The pane exists. Otherwise `refused(not-found)`.
5. The pane is restored and its process has started. Otherwise `refused(pane-not-ready)`.
6. The signature declares the action id. Otherwise `refused(unsupported)`.
7. The activity is in the action's `allowed_when` list. Otherwise `refused(conflict)`. The message names the current activity: `agent is working`, `agent is idle`, `agent is done`, `agent is awaiting approval` or `agent is in a menu`, named after the current activity.
8. The plugin has not exceeded the rate of 20 `send-input` or `agent-action` submissions per second for that pane. Otherwise `refused(rate-limited)`.
9. The key is reserved and the keys are written to the pane.

A submission refused at steps 1 to 8 does not reserve its key. A retry with the same key, after the cause is fixed, is valid. A submission accepted at step 9 keeps its key reserved even if the write later fails, and that failure completes as `failed(code)`. A retry of such an action needs a new key.

### Choosing the dedup key

The key identifies one user request. It is reserved for 10 minutes per plugin, so a retry after a lost reply reuses the key and does nothing a second time. The SDK helper `dedup::key_for` joins its parts with `:` and replaces disallowed characters with `-`, and `Submission::keyed` sets the key. `dedup::key_for(&["compact", "42", "7"])` gives `compact:42:7`. TypeScript has no helper, so the plugin builds the string itself.

The kernel journals `send-input` and `agent-action` keys in the snapshot before the write. A server restart then does not repeat the keys.

## Toolbar state

`set-toolbar-state` works on a button that the plugin contributed in `toolbar_buttons`. The `button` argument is that button's `id`. Any other id gives `not-found`.

- `pane: none` sets the state for every agent pane that shows the button.
- `pane: some(id)` sets the state for that pane only. It overrides the state set for all panes, and it is dropped when the pane closes.
- `label: none` keeps the label that is current. A label is at most 32 cells, and a longer one gives `invalid-argument`.

The state lasts until the plugin changes it. A restart of the plugin resets every button to its manifest state. When the grant is revoked, the toolbar state the plugin set is withdrawn, and the plugin is re-activated with the reduced grant set.

## Worked example in Rust

This server-side plugin enables the Compact button only while the agent is in an activity that allows compacting. A click on the button submits the action with a key that is unique per request.

**Rust**

```rust
use ilium_plugin_sdk::dedup;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::agent;
use ilium_plugin_sdk::types::events::{AgentActivityEvent, CommandInvokedEvent};
use ilium_plugin_sdk::types::intents::AgentActionSpec;
use ilium_plugin_sdk::types::tree::AgentActivity;

const COMPACT_BUTTON: &str = "compact";
const COMPACT_COMMAND: &str = "com.example.compact/run";

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct CompactButton {
    next_request: u64,
}

impl Handler for CompactButton {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::AgentActivityChanged(changed) => show_state(&changed),
                Event::CommandInvoked(invoked) => {
                    if let Some(submission) = self.compact(&invoked) {
                        submissions.push(submission);
                    }
                }
                _ => {}
            }
        }
        submissions
    }
}

fn show_state(changed: &AgentActivityEvent) {
    let enabled = matches!(
        changed.current,
        AgentActivity::Idle
            | AgentActivity::Done
            | AgentActivity::AwaitingApproval
            | AgentActivity::Menu
    );
    if let Err(error) = agent::set_toolbar_state(COMPACT_BUTTON, Some(changed.node), enabled, None)
    {
        log::log(Level::Warn, &format!("compact button state not set: {error:?}"));
    }
}

impl CompactButton {
    fn compact(&mut self, invoked: &CommandInvokedEvent) -> Option<Submission> {
        if invoked.command != COMPACT_COMMAND {
            return None;
        }
        let pane = invoked.context.pane?;
        self.next_request += 1;
        let pane_text = pane.to_string();
        let request_text = self.next_request.to_string();
        let key = dedup::key_for(&["compact", &pane_text, &request_text]);
        let spec = AgentActionSpec {
            pane,
            action_id: "compact".to_string(),
        };
        Some(Submission::keyed(Intent::AgentAction(spec), key))
    }
}
```

The `next_request` counter restarts at zero when the plugin restarts. Within 10 minutes of a restart, a new request can then reuse a key that is still reserved, and the kernel answers `duplicate(original)`. A plugin that needs unique keys across restarts should add a value that does not repeat, such as a timestamp.

## Worked example in TypeScript

This server-side plugin does the same job. Its handler never throws. An exception thrown from `onEvents` is a trap, and a trap counts against the restart rule.

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';
import { setToolbarState } from '@ilium/plugin-sdk/primitives/agent';
import type { Event, Submission } from '@ilium/plugin-sdk/types';

const COMPACT_BUTTON = 'compact';
const COMPACT_COMMAND = 'com.example.compact/run';
const COMPACTABLE_ACTIVITIES = ['idle', 'done', 'awaiting-approval', 'menu'];

let nextRequest = 0;

function showState(pane: bigint, current: string): void {
  const enabled = COMPACTABLE_ACTIVITIES.includes(current);
  try {
    setToolbarState(COMPACT_BUTTON, pane, enabled, undefined);
  } catch (error) {
    log('warn', `compact button state not set: ${String(error)}`);
  }
}

function compact(command: string, pane: bigint | undefined): Submission | undefined {
  if (command !== COMPACT_COMMAND || pane === undefined) {
    return undefined;
  }
  nextRequest += 1;
  return {
    intent: { tag: 'agent-action', val: { pane, actionId: 'compact' } },
    correlation: BigInt(nextRequest),
    dedupKey: `compact:${pane}:${nextRequest}`,
  };
}

export const { handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'agent-activity-changed') {
        showState(event.val.node, event.val.current);
      } else if (event.tag === 'command-invoked') {
        const submission = compact(event.val.command, event.val.context.pane);
        if (submission !== undefined) {
          submissions.push(submission);
        }
      }
    }
    return submissions;
  },
});
```

The activity values are kebab-case strings in TypeScript. Node ids and correlation numbers are `bigint`, and the `dedupKey` field is the submission's `dedup-key`.

## What the user sees

The Compact button is a toolbar item at the bottom of the agent pane. While the agent works, the plugin disables it, so the user cannot press it and no action is submitted.

<figure>
<pre class="term"><span class="t-hi">example-agent  working</span>
&gt; refactor the scanner module
<span class="t-dim">esc to interrupt</span>
<span class="t-dim">[ Compact ]</span> <span class="t-warn">disabled</span>
</pre>
<figcaption>Illustrative, not generated. The agent pane while the agent works, with the Compact button disabled. The wording is a placeholder.</figcaption>
</figure>

<figure>
<svg viewBox="0 0 420 230" role="img" aria-label="An agent action goes from the plugin to the signature lookup, the activity check and then to the pane">
<title>How an agent action reaches the agent</title>
<desc>The plugin names an action id. The kernel looks the id up in the pane's signature. If the signature has it, the kernel checks the activity, then sends the declared keys to the pane. If the signature has no such action, the submission is refused as unsupported.</desc>
<defs>
<marker id="arr-agent-control" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
<path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="8" y="8" width="240" height="34" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="128" y="30" text-anchor="middle" font-size="11" fill="currentColor">Plugin: agent-action compact</text>
<rect x="8" y="66" width="240" height="34" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="128" y="88" text-anchor="middle" font-size="11" fill="currentColor">Kernel: find action in signature</text>
<rect x="8" y="124" width="240" height="34" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="128" y="146" text-anchor="middle" font-size="11" fill="currentColor">Kernel: check allowed_when activity</text>
<rect x="8" y="182" width="240" height="34" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="128" y="204" text-anchor="middle" font-size="11" fill="currentColor">ok: keys written to the pane</text>
<line x1="128" y1="42" x2="128" y2="66" stroke="currentColor" marker-end="url(#arr-agent-control)"/>
<line x1="128" y1="100" x2="128" y2="124" stroke="currentColor" marker-end="url(#arr-agent-control)"/>
<line x1="128" y1="158" x2="128" y2="182" stroke="currentColor" marker-end="url(#arr-agent-control)"/>
<line x1="248" y1="83" x2="284" y2="83" stroke="currentColor" stroke-dasharray="3 3" marker-end="url(#arr-agent-control)"/>
<rect x="288" y="66" width="124" height="34" rx="6" fill="none" stroke="currentColor"/>
<text x="350" y="81" text-anchor="middle" font-size="10" fill="currentColor">no such action:</text>
<text x="350" y="95" text-anchor="middle" font-size="10" fill="currentColor">refused(unsupported)</text>
</svg>
<figcaption>The plugin never sends keys itself. The keys come from the pane's signature, and an unknown action is refused before anything reaches the pane.</figcaption>
</figure>

When the grant is declined, `agent-action` completes with `refused(not-permitted)`. `set-toolbar-state` returns `not-permitted`, and the button the plugin contributed stays hidden or disabled. The user can still run the agent's own compact command in the agent's interface.

## Risk and tier rules

- The grant is high risk (H). A T1 bundled plugin does not receive it automatically. The user is asked once, at first activation, with one checkbox for each H grant.
- A T2 or T3 plugin lists the grant in the install review, unchecked by default. The line carries the H mark.
- The plugin can trigger only actions that a signature declares. Sending arbitrary text needs [terminal.write](/pages/capability-terminal-write/), which is a separate high-risk grant.

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.agent 0.1.0 by Example Ltd                                     │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ <span class="t-warn">! [ ] Starts agent actions such as compact</span> <span class="t-dim">……………………………………………</span> <span class="t-warn">agent.control</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests agent.control, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

## Errors and edge cases

| Call | Code | When |
|---|---|---|
| `agent-action` | `not-permitted` | The grant is not held or was declined. The batch is refused as a whole. |
| `agent-action` | `invalid-argument` | The dedup key is missing or malformed. The batch is refused as a whole. |
| `agent-action` | `refused(not-found)` | The pane does not exist. |
| `agent-action` | `refused(pane-not-ready)` | The pane was restored and its process has not started. Retry after `pane-created` or `tree-changed` for that pane. |
| `agent-action` | `refused(unsupported)` | The signature declares no action with that id. |
| `agent-action` | `refused(conflict)` | The activity is not in `allowed_when`. The message names the current activity: `agent is working`, `agent is idle`, `agent is done`, `agent is awaiting approval` or `agent is in a menu`, named after the current activity. The key is not reserved, so the same key can be used later. |
| `agent-action` | `refused(rate-limited)` | The pane received more than 20 submissions per second. `retry-after-ms` is set. |
| `agent-action` | `duplicate(original)` | The key was reserved within the last 10 minutes. Nothing happens. |
| `agent-action` | `failed(code)` | The key was accepted and the write failed. The key stays reserved, so a retry needs a new key. |
| `set-toolbar-state` | `not-found` | The button is not the plugin's own, or the pane does not exist. |
| `set-toolbar-state` | `invalid-argument` | The label is over 32 cells. |
| both | `not-permitted`, `revoked`, `internal` | The codes that every host function can return. |

Edge cases to design for:

- A batch refused for a malformed submission or a missing grant cancels its other submissions, each with `refused(cancelled)`. Other checks refuse only the submission that fails them.
- Activity is checked when the batch is accepted. The agent can become busy after that check, and the keys are then written while it works.
- A revoked grant ends in-flight calls with `revoked`. The plugin is re-activated without the grant, and its toolbar state is withdrawn.
- A plugin that uses the same key for two different requests gets `duplicate(original)` for the second one and no action. Use one key per request.

## First-party features that use it

- Feature 35, agent toolbar.
- Feature 80, remote compaction.
- Feature 81, compact button routing.

## See also

- [agent.read](/pages/capability-agent-read/) for the activity that the toolbar state depends on.
- [agent.signatures](/pages/capability-agent-signatures/) for the signature that declares the actions.
- [primitives-agent](/pages/primitives-agent/) for the full agent interface.
- [api-intents](/pages/api-intents/) for submissions and dedup keys.
- [terminal.write](/pages/capability-terminal-write/) for free text input.
- [tutorial-04-finished-notice](/pages/tutorial-04-finished-notice/) for a plugin that adds a Compact button.

Checked against the API version `ilium:plugin@0.1.0`.
