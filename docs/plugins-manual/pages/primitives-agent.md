---
layout: page
title: Agent primitive
permalink: /pages/primitives-agent/
---

## Agent primitive

An agent pane runs an agent CLI such as Claude Code or Codex. The agent primitive lets a plugin find
the agent in a pane, read its activity and session, read the last prompt the user submitted, add
toolbar buttons, send actions that the agent understands, and register signatures for agents the host
does not know.

The kernel owns the process walk, the screen snapshot and every key write. The plugin owns the
signature (how to recognise the agent) and the classifier (what the agent is doing now).

### What you need

| Grant | Needed for |
|---|---|
| `agent.read` | `agents`, `view`, `session`; the events `agent-identified`, `agent-activity-changed` and `agent-goal-changed` |
| `agent.signatures` | The `signatures` manifest entry, the `detector` export and `register-signature` |
| `agent.control` | `set-toolbar-state` and the intent `agent-action` |
| `agent.read-prompts` | The event `input-submitted` and `last-prompt`. High risk: it exposes what the user typed. |
| `fs.read:agent-sessions:<agent>` | Reading the transcript file that `session` points to |

`agent-action` needs only `agent.control`. The kernel checks the activity itself, so the plugin does
not need `agent.read` to send an action.

The detector and runtime signatures run on the server side. Toolbar state and agent actions also work
from the client side. Plugins are written in Rust (stable 1.90 or later with the `wasm32-wasip2`
target) or TypeScript (Bun 1.2 or later). See [toolchain setup](/pages/getting-started-toolchain/).

### Interface

```wit
// ilium:types/tree
enum agent-activity { none, working, idle, awaiting-approval, done, menu }
record agent-view {
    signature: string, activity: agent-activity, objective: option<string>, goal: option<string>,
}

// ilium:primitives/agent
record agent-pane { pane: node-id, view: agent-view }
record session-ref { scope: string, path: string, session-id: string }
record signature-action { id: string, keys: string, allowed-when: option<list<agent-activity>> }
record signature-spec {
    id: string, process-names: list<string>, aliases: list<string>, icon: option<string>,
    resume: option<list<string>>, actions: list<signature-action>,
    session-scope: option<string>, submit-keys: option<string>,
}
agents: func() -> list<agent-pane>;
view: func(pane: node-id) -> option<agent-view>;
session: func(pane: node-id) -> option<session-ref>;
last-prompt: func(pane: node-id) -> option<string>;
register-signature: func(spec: signature-spec) -> result<_, plugin-error>;
set-toolbar-state: func(button: string, pane: option<node-id>, enabled: bool,
    label: option<string>) -> result<_, plugin-error>;
```

The `detector` export is a separate contract, so a plugin can implement it without registering
signatures at run time:

```wit
// Excerpt of ilium:exports/detector in reference-wit
// ilium:server/detector
record detector-process { name: string, argv: list<string> }
record detector-screen {
    columns: u16, rows: list<string>, cursor-row: u16, title: option<string>, revision: u64,
}
record classification { activity: agent-activity, objective: option<string>, goal: option<string> }
identify: func(processes: list<detector-process>) -> option<string>;
classify: func(signature: string, screen: detector-screen) -> classification;
```

### Functions

| Function (Rust / TypeScript) | Grant | Returns | Meaning |
|---|---|---|---|
| `agents()` / `agents()` | `agent.read` | `list<agent-pane>` | One entry per identified agent pane. Empty when the grant is declined. |
| `view(pane)` / `view(pane)` | `agent.read` | `option<agent-view>` | Signature and activity of one pane. None when the pane is not identified or the grant is declined. |
| `session(pane)` / `session(pane)` | `agent.read` | `option<session-ref>` | Transcript reference. Only a signature with `session_scope` yields one. |
| `last_prompt(pane)` / `lastPrompt(pane)` | `agent.read-prompts` | `option<string>` | Last prompt submitted to the pane. None when the grant is declined. |
| `register_signature(spec)` / `registerSignature(spec)` | `agent.signatures` | `result<_, plugin-error>` | Adds a signature, or replaces one the same plugin added at run time. |
| `set_toolbar_state(...)` / `setToolbarState(...)` | `agent.control` | `result<_, plugin-error>` | Enables or relabels a toolbar button. |

All of them are synchronous and take no request id. The Rust module is
`ilium_plugin_sdk::primitives::agent`; the TypeScript import is `@ilium/plugin-sdk/primitives/agent`.
Types follow the SDK naming rules: `AgentPane`, `SessionRef`,
`SignatureSpec` and `SignatureAction` in Rust, with the same fields in camelCase in TypeScript.
`AgentActivity` and `AgentView` come from `ilium_plugin_sdk::types::tree`.

`set_toolbar_state` with `pane: None` sets the state for every agent pane that shows the button. With a
pane id it sets that pane only, overrides the all-pane state, and is dropped when the pane closes. A
`label` of `None` keeps the current label. A label over 32 cells is `invalid-argument`.

Activity values are `none` (not identified yet), `working`, `idle`, `awaiting-approval`, `done` and
`menu`. In Rust they are `AgentActivity::AwaitingApproval` and so on; in TypeScript they are the
strings `'awaiting-approval'` and so on.

### Signatures

A signature describes one agent program. Plugin signatures from the manifest and from
`register-signature` share one set of rules:

| Manifest key | Field | Default | Rule |
|---|---|---|---|
| `id` | `id` | required | 1 to 64 characters from `[a-z0-9-]`. The global id is `<plugin-id>.<id>`. |
| `process_names` | `process-names` | required | 1 to 32 names, each non-empty, at most 255 bytes, with no `/` unless absolute. |
| `aliases` | `aliases` | none | At most 32 entries. |
| `icon` | `icon` | none | A Tabler icon name. |
| `resume` | `resume` | none | An argv template. `{session}` is replaced by the session id. |
| `actions` | `actions` | none | At most 32 actions. Each `keys` value is at most 1 KiB. |
| `session_scope` | `session-scope` | none | A scope such as `agent-sessions:claude`. |
| `submit_keys` | `submit-keys` | `"\r"` | Keys that submit a prompt in this agent. |

Each action may set `allowed_when`, a list of activities in which the action is allowed. The default is
`["idle", "done", "awaiting-approval", "menu"]`, so a compact action is refused while the agent works.
Built-in signatures keep bare ids (`claude`, `codex`, `antigravity`). A signature from `register-signature`
replaces an earlier one with the same id from the same plugin. A signature id that the manifest already
contributes returns `conflict`.

Settings can also supply signatures as plain JSON. They go through the same validation, and an error
names the field path, for example `process_names[2]: empty name`.

### Contributions

```json
{
  "id": "example.agent-tools",
  "signatures": [{
    "id": "example-agent",
    "process_names": ["example-agent"],
    "aliases": ["ea"],
    "icon": "robot",
    "actions": [{ "id": "compact", "keys": "/compact\r", "allowed_when": ["idle", "done"] }],
    "resume": ["example-agent", "--resume", "{session}"],
    "session_scope": "agent-sessions:claude"
  }],
  "commands": [{ "id": "compact", "title": "Compact agent context" }],
  "toolbar_buttons": [{
    "id": "compact", "icon": "compress", "label": "Compact",
    "command": "example.agent-tools/compact",
    "applies_to": ["example-agent"], "requires_action": "compact"
  }],
  "capabilities": { "server": ["agent.read", "agent.control", "agent.signatures"] }
}
```

A toolbar button has `applies_to`, either `"all"` or a list of signature ids. Use the local id for the
plugin's own signatures and the bare id for built-in ones. `requires_action` shows the button only on
panes whose signature declares that action. The button's `command` is a global command id.

### Detector rules

- `identify` runs on every detection tick for each pane that has no signature. It returns the local
  signature id, and the kernel adds the plugin prefix. An id the plugin did not register is ignored, the
  pane stays unidentified, and the kernel logs a warning.
- `classify` runs for an identified pane whenever its screen revision changes.
- Each call has a 20 ms deadline. A missed deadline keeps the previous classification, emits nothing and
  logs a warning.
- A trap counts as a failure and follows the [restart and disable rule](/pages/reference-errors-and-limits/#restart-and-disable-rule): restart after 1 s, then 5 s, and disable
  on the third failure within 10 minutes.
- Without the `detector` export a pane is identified by `process_names` and `aliases` alone. Its
  activity stays `none`, but `resume`, `actions` and `submit_keys` still apply.

<figure>
<svg viewBox="0 0 400 190" role="img" aria-label="Agent detection loop between kernel and plugin">
  <defs>
    <marker id="agent-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6"
      markerHeight="6" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="20" width="150" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="85" y="42" font-size="11" text-anchor="middle" fill="currentColor">Process walk and screen</text>
  <line x1="160" y1="38" x2="240" y2="38" stroke="currentColor" marker-end="url(#agent-arrow)"/>
  <rect x="240" y="20" width="150" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="315" y="42" font-size="11" text-anchor="middle" fill="currentColor">identify, classify</text>
  <line x1="315" y1="56" x2="315" y2="92" stroke="currentColor" marker-end="url(#agent-arrow)"/>
  <rect x="240" y="92" width="150" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="315" y="114" font-size="11" text-anchor="middle" fill="currentColor">Store, emit change</text>
  <line x1="240" y1="110" x2="160" y2="110" stroke="currentColor" marker-end="url(#agent-arrow)"/>
  <rect x="10" y="92" width="150" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="85" y="114" font-size="11" text-anchor="middle" fill="currentColor">Next detection tick</text>
  <line x1="85" y1="92" x2="85" y2="56" stroke="currentColor" marker-end="url(#agent-arrow)"/>
  <text x="10" y="160" font-size="11" fill="currentColor">The plugin never polls. The kernel calls it on each tick.</text>
  <text x="10" y="176" font-size="11" fill="currentColor">classify runs only when the screen revision changes.</text>
</svg>
<figcaption>The kernel owns the timing and the stored state. The plugin answers identify and classify
calls only.</figcaption>
</figure>

### Example 1: classifier and compact button (Rust)

This server plugin identifies `example-agent`, classifies its screen, and enables the Compact button
only while the agent is idle. When the user presses the button, the plugin submits the `compact` action,
which the kernel sends as `/compact` followed by Enter. The manifest above is the matching manifest.

```rust
use ilium_plugin_sdk::exports::{Classification, DetectorProcess, DetectorScreen};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::agent;
use ilium_plugin_sdk::types::events::{AgentActivityEvent, CommandInvokedEvent};
use ilium_plugin_sdk::types::intents::AgentActionSpec;
use ilium_plugin_sdk::types::tree::AgentActivity;

const SIGNATURE: &str = "example-agent";
const COMPACT_COMMAND: &str = "example.agent-tools/compact";

#[ilium_plugin(side = "server", exports = ["detector"])]
#[derive(Default)]
struct AgentTools;

impl Detector for AgentTools {
    fn identify(&mut self, processes: Vec<DetectorProcess>) -> Option<String> {
        let found = processes.iter().any(|process| process.name == "example-agent");
        found.then(|| SIGNATURE.to_string())
    }

    fn classify(&mut self, _signature: String, screen: DetectorScreen) -> Classification {
        let shows = |text: &str| screen.rows.iter().any(|row| row.contains(text));
        let activity = if shows("Do you want to proceed?") {
            AgentActivity::AwaitingApproval
        } else if shows("esc to interrupt") {
            AgentActivity::Working
        } else {
            AgentActivity::Idle
        };
        Classification { activity, objective: None, goal: None }
    }
}

impl Handler for AgentTools {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::AgentActivityChanged(AgentActivityEvent { node, current, .. }) => {
                    let enabled = current == AgentActivity::Idle;
                    // A pane that closed meanwhile returns not-found; nothing else depends on it.
                    let _ = agent::set_toolbar_state("compact", Some(node), enabled, None);
                }
                Event::CommandInvoked(CommandInvokedEvent { command, context, .. })
                    if command == COMPACT_COMMAND =>
                {
                    let Some(pane) = context.pane else { continue };
                    let spec = AgentActionSpec { pane, action_id: "compact".into() };
                    let key = format!("compact:{pane}");
                    submissions.push(Submission::keyed(Intent::AgentAction(spec), key));
                }
                _ => {}
            }
        }
        submissions
    }
}
```

Grants: `agent.read` for the activity event, `agent.control` for the toolbar state and the action, and
`agent.signatures` for the manifest signature and the detector export.

Outcome: on a permission prompt the pane shows `awaiting-approval` and Compact is disabled. A press
sends `compact` once; a second press with the same key within 10 minutes completes as `duplicate`.

### Example 2: runtime signature and session (TypeScript)

This server plugin registers a second agent, `example-agent-beta`, when it activates. A command reads the
session reference and the last prompt of the focused pane and writes them to the plugin log.

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';
import { lastPrompt, registerSignature, session } from '@ilium/plugin-sdk/primitives/agent';

const SHOW_SESSION_COMMAND = 'example.beta-agent/show-session';

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate() {
    // Throws PluginError (invalid-argument) when validation fails; activation then fails.
    registerSignature({
      id: 'example-agent-beta',
      processNames: ['example-agent-beta'],
      aliases: [],
      icon: 'robot',
      resume: ['example-agent-beta', '--resume', '{session}'],
      actions: [],
      sessionScope: 'agent-sessions:codex',
      submitKeys: undefined,
    });
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'command-invoked') continue;
      if (event.val.command !== SHOW_SESSION_COMMAND) continue;
      const pane = event.val.context.pane;
      if (pane === undefined) continue;
      const reference = session(pane);
      const prompt = lastPrompt(pane);
      log('info', `session ${reference?.path ?? 'none'}; last prompt ${prompt ?? 'none'}`);
    }
    return [];
  },
});
```

Grants: `agent.signatures`, `agent.read` and `agent.read-prompts`. The manifest also needs a `commands` entry with id `show-session`. Reading the transcript behind
`reference.path` would also need `fs.read:agent-sessions:codex`, which this example does not use.

### Intent and events

`agent-action` is an intent, `Intent::AgentAction(AgentActionSpec { pane, action_id })` in Rust and
`{ tag: 'agent-action', val: { pane, actionId } }` in TypeScript. Its submission must carry a
`dedup-key`. The kernel checks the steps below in order. A refused submission does not reserve its
key, so a retry with the same key after fixing the cause is valid.

| Step | Outcome when it fails |
|---|---|
| Well formed, key present | `refused(invalid-argument)`; the whole batch is refused |
| Grant `agent.control` | `refused(not-permitted)` |
| Key reserved already | `duplicate(request-ref)` |
| Pane exists and is an agent pane | `refused(not-found)` |
| Pane restored and process started | `refused(pane-not-ready)` |
| Action id in the signature | `refused(unsupported)` |
| Activity allowed by `allowed_when` | `refused(conflict)` with a message named after the current activity, for example `agent is working` |
| Rate: 20 per second per pane per plugin | `refused(rate-limited)` with `retry-after-ms` |
| Write | `failed(code)`; the key stays reserved, so a retry needs a new key |

Events:

| Event | Payload | Grant | Delivery |
|---|---|---|---|
| `agent-identified` | `node`, `signature` | `agent.read` | Never dropped. Always before the first activity event of the pane. |
| `agent-activity-changed` | `node`, `previous`, `current`, `revision` | `agent.read` | Never dropped. |
| `agent-goal-changed` | `node`, `goal` | `agent.read` | Coalesced per node. |
| `input-submitted` | `node`, `text`, `source`, `epoch` | `agent.read-prompts` | Never dropped. Without the grant, nothing derived from the text is sent. |

`source` is one of `user`, `plugin`, `scheduled` or `voice`. `epoch` is the prompt counter of the pane, and it increases with each submitted prompt.

### Errors and edge cases

- `register-signature` returns `invalid-argument` for a rule that breaks the signature limits (id, process names, aliases, icon,
  action count or key size). The message names the field path. It returns `conflict` for an id the
  manifest already contributes.
- Two signatures from different origins that match one process name do not fail. The user's signature
  wins over a plugin signature, which wins over a built-in one, and the overlap is logged as a warning.
- `set-toolbar-state` returns `not-found` for a button that is not the plugin's own or a pane that does
  not exist, and `not-permitted` when the grant is missing. Its state lasts until the plugin changes it.
  A plugin restart resets each button to its manifest state, and a revoked grant withdraws the state.
- `agents()` returns an empty list when `agent.read` is declined. `view`, `session` and `last_prompt`
  return none, because these functions have no error channel.
- A grant revoked at run time takes effect on the next call.
- A pane has no activity before its `agent-identified` event. Restored panes return none for
  `last_prompt` until a new prompt is submitted, because the text is not in the snapshot.

### What the user sees

The toolbar of an agent pane shows each button that matches `applies_to` and `requires_action`. A
disabled button is greyed out and does not respond. The tree shows each identified pane's activity
through `node-view.agent`.

<figure>
<pre class="term">╭──≡ ● · Example agent──────────────────────────────────╮
│<span class="t-warn">Do you want to proceed?</span>                                │
│<span class="t-dim">  1. Yes   2. No</span>                                       │
╰───────────────────────────────────────────────────────╯
<span class="t-dim">🗜️ Compact  disabled while the agent awaits approval</span>
</pre>
<figcaption>An agent pane with a permission prompt on screen. The tree row shows the waiting-approval icon, and the plugin has disabled the Compact toolbar button.</figcaption>
</figure>

<pre class="term">▼ 📂 Ilium
›   🦀 ✋ Fix auth
</pre>

### Transcript, model and signals

`model`, `prompt-epoch`, the typed transcript and the events `signal-changed` and `agent-model-changed` are described in [Agent transcript and signals](/pages/primitives-agent-transcript/). They need `agent.read`, and `agent.transcript` (high risk) for the transcript. `user-line-submitted` is described in [Terminal reads](/pages/primitives-terminal-reads/).

Requirement ids for this page: [PRI-010], [PRI-011], [PRI-012], [PRI-013], [PRI-015]. See the [requirements index](/pages/reference-requirements/).

### See also

- [Agent transcript and signals](/pages/primitives-agent-transcript/) for these facts.

- [Plugin exports](/pages/api-exports/) for the `detector` export and the trait signatures.
- [Commands](/pages/api-commands/) for the commands that toolbar buttons run.
- [Events](/pages/api-events/) and [Intents](/pages/api-intents/) for the full catalogues.
- [Tree](/pages/api-tree/) for `node-view.agent`.
- [Agent read](/pages/capability-agent-read/), [agent read prompts](/pages/capability-agent-read-prompts/),
  [agent control](/pages/capability-agent-control/) and [agent signatures](/pages/capability-agent-signatures/)
  for the grants.
- [Errors and limits](/pages/reference-errors-and-limits/) for the codes and the restart rule.

### Security

**[SEC-042]** Signature registration needs the normal-risk `agent.signatures` grant. See [Sandbox reference](/pages/reference-sandbox/).
