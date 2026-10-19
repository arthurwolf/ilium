# agent.signatures

The `agent.signatures` grant lets a server-side plugin teach Ilium about an agent CLI that it does not recognise by default. A signature names the processes that run the agent, gives it aliases and an icon, says how to resume a session, and maps action ids to key sequences. A plugin can also export a `detector`, which names the agent from the process list and turns the screen into an activity. The kernel does the process walk and the screen capture. The plugin supplies the rules.

## You will need

- The grant `agent.signatures`, on the `server` side only. The grant takes no argument.
- The `detector` export, listed in `server.exports` of the manifest, only when the plugin identifies or classifies agents itself. The SDK adds the `with-detector` fragment to the world when that export is listed.
- The grant [agent.read](/pages/capability-agent-read/) if the plugin also reads the resulting activity.
- The Rust crate `ilium-plugin-sdk` or the npm package `@ilium/plugin-sdk`.

| Grant | Side | Risk | Install of a T2 or T3 plugin | T1 bundled plugin |
|---|---|---|---|---|
| `agent.signatures` | server | normal | listed and checked by default | granted up to the manifest ceiling |

A signature only identifies an agent and describes it. It does not allow the plugin to send input or read transcripts. Those need the grants [agent.control](/pages/capability-agent-control/) and `fs.read:agent-sessions:<agent>`.

## Three ways to add a signature

| Source | Where it is declared | Scope |
|---|---|---|
| Manifest contribution | `contributes.signatures` in `manifest.json` | Registered when the plugin installs. |
| Runtime call | `primitives::agent::register_signature` | Registered by the plugin while it runs. |
| Settings value | A plain JSON value in the plugin settings | Checked by the same rules as a manifest entry. The error names the field path. |

The id of a manifest or runtime signature is global: `<plugin-id>.<id>`. A signature with the id `example-agent` from the plugin `example-signatures` is `example-signatures.example-agent`. Built-in signatures keep bare ids (`claude`, `codex`, `antigravity`). If two signatures of different origins match the same process name, the user's settings win over plugins, and plugins win over built-in signatures. The kernel logs the overlap as a warning. It is not an error.

## Signature fields

| Field | Manifest type | Rust | TypeScript | Meaning and limits |
|---|---|---|---|---|
| `id` | string | `id: String` | `id` | 1 to 64 characters from `[a-z0-9-]`. |
| `process_names` | list of strings | `Vec<String>` | `string[]` | 1 to 32 names. Each is non-empty, at most 255 bytes, and has no `/` unless it is an absolute path. |
| `aliases` | list of strings | `Vec<String>` | `string[]` | At most 32 names that also identify the agent. |
| `icon` | string | `String` | `string` | A Tabler icon name. |
| `resume` | list of strings | `Vec<String>` | `string[]` | Argument list for resuming a session. `{session}` is replaced by the session id. Without it no resume is offered. |
| `actions` | map in the manifest | `Vec<SignatureAction>` | `SignatureAction[]` | At most 32 actions, sorted by id. Each has `keys` (at most 1 KiB) and optional `allowed_when`. |
| `session_scope` | string, optional | `Option<String>` | `string \| undefined` | Scope such as `agent-sessions:claude`. `agent.session` returns a value only when it is set. |
| `submit_keys` | string, optional | `Option<String>` | `string \| undefined` | Keys that submit a prompt. The default is `"\r"`. |

Each `SignatureAction` has `id`, `keys` and `allowed_when`. The `allowed_when` list holds `agent-activity` values. When it is absent, the action runs only in `idle`, `done`, `awaiting-approval` or `menu`. A compact action is therefore refused while the agent is `working`, with `refused(conflict)` and the message `agent is working`.

## The detector export

The `detector` export has two functions. The kernel calls them off the keystroke path.

| Function | Input | Output | When the kernel calls it |
|---|---|---|---|
| `identify` | `list<detector-process>`, each with `name` and `argv` | The local signature id, or none | For each pane that has no identified signature, on every detection tick. |
| `classify` | The signature id and a `detector-screen` (`columns`, `rows`, `cursor-row`, `title`, `revision`) | `classification`: `activity`, `objective`, `goal` | When the screen `revision` of an identified pane changes. |

The `activity` values are `none`, `working`, `idle`, `awaiting-approval`, `done` and `menu`. Before a pane is identified its activity is `none`. The `classify` function must be pure. It reads the snapshot it receives and returns a value, with no file or network access. Each export call has a 20 ms deadline. A missed deadline keeps the previous classification, emits no event and logs a warning. See [Detection loop](#detection-loop).

## Example 1: manifest signature with a Rust detector

The manifest declares the signature and the export. The server requests the grant and lists `detector`. Other manifest keys are omitted.

```json
{
  "capabilities": { "server": ["agent.signatures"] },
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": ["detector"]
  },
  "contributes": {
    "signatures": [
      {
        "id": "example-agent",
        "process_names": ["example-agent"],
        "aliases": ["example"],
        "icon": "robot",
        "resume": ["example-agent", "--resume", "{session}"],
        "session_scope": "agent-sessions:claude",
        "actions": { "compact": { "keys": "/compact\r", "allowed_when": ["idle"] } }
      }
    ]
  }
}
```

The Rust plugin implements the detector and the two required exports, `Lifecycle` and `Handler`. The detector records (`DetectorProcess`, `DetectorScreen` and `Classification`) are the generated types of the detector export: the `detector-process`, `detector-screen` and `classification` records of the [WIT reference](/pages/reference-wit/). `AgentActivity` is the `agent-activity` enum in `ilium_plugin_sdk::types::tree`, the path the agent-events example uses.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::exports::{Detector, Handler, Lifecycle};
use ilium_plugin_sdk::types::tree::AgentActivity;

#[ilium_plugin(side = "server", exports = ["detector"])]
#[derive(Default)]
struct ExampleSignatures;

impl Lifecycle for ExampleSignatures {}

impl Handler for ExampleSignatures {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

impl Detector for ExampleSignatures {
    fn identify(&mut self, processes: Vec<DetectorProcess>) -> Option<String> {
        let found = processes.iter().any(|process| process.name == "example-agent");
        found.then(|| "example-agent".to_string())
    }

    fn classify(&mut self, signature: String, screen: DetectorScreen) -> Classification {
        if signature != "example-agent" {
            return Classification { activity: AgentActivity::None, objective: None, goal: None };
        }
        let mut activity = AgentActivity::Idle;
        let mut objective = None;
        for row in &screen.rows {
            if row.contains("Allow this command?") {
                activity = AgentActivity::AwaitingApproval;
            } else if let Some(text) = row.strip_prefix("* Working on ") {
                activity = AgentActivity::Working;
                objective = Some(text.trim().to_string());
            }
        }
        Classification { activity, objective, goal: None }
    }
}
```

The `identify` function returns the local id. The kernel reports the pane's signature as `example-signatures.example-agent`.

## Example 2: runtime signature in TypeScript

This plugin registers its signature when it activates, so it needs no `signatures` entry in the manifest. Registering again with the same id replaces the earlier signature of the same plugin. A manifest entry with that id would return `conflict` instead.

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { registerSignature } from '@ilium/plugin-sdk/primitives/agent';

export const { lifecycle, handler, detector } = definePlugin({
  side: 'server',
  activate() {
    registerSignature({
      id: 'other-agent',
      processNames: ['other-agent', 'other-agent-cli'],
      aliases: ['other'],
      icon: 'brain',
      resume: ['other-agent', '--session', '{session}'],
      sessionScope: undefined,
      submitKeys: '\r',
      actions: [{ id: 'clear', keys: '/clear\r', allowedWhen: ['idle', 'done'] }],
    });
  },
  onEvents() {
    return [];
  },
  identify(processes) {
    const found = processes.some((process) => process.name === 'other-agent');
    return found ? 'other-agent' : undefined;
  },
  classify(signature, screen) {
    if (signature !== 'other-agent') {
      return { activity: 'none', objective: undefined, goal: undefined };
    }
    const waiting = screen.rows.some((row) => row.includes('Approve? [y/N]'));
    return {
      activity: waiting ? 'awaiting-approval' : 'idle',
      objective: undefined,
      goal: undefined,
    };
  },
});
```

The TypeScript record fields are camelCase (`processNames`, `allowedWhen`). The `undefined` value stands for an absent `option`. The `revision` field of `detector-screen` is a `bigint`, which this example does not read.

## Errors and edge cases

| Situation | Result |
|---|---|
| The grant is declined in the review dialog | The manifest signature is not registered, `detector` is not called for the plugin, and `register_signature` returns `not-permitted`. Agents only this plugin recognises stay unidentified. |
| A field breaks a P5 limit | `invalid-argument`, with the field path, for example `process_names[2]: empty name`. |
| A runtime id equals a manifest id of the same plugin | `conflict`. |
| The grant is revoked while the plugin runs | The plugin is deactivated with `revoked` and re-activated without the grant. Its signatures and detector stop. |
| `identify` returns an id the plugin did not register | The id is ignored, the pane stays unidentified and a warning is logged. |
| `classify` traps or misses its deadline | A trap counts as a failure under the restart rule. A missed deadline keeps the last activity. |
| An action has no entry in the signature | `agent-action` returns `refused(unsupported)`. |
| An action is not allowed in the current activity | `agent-action` returns `refused(conflict)`, for example `agent is working`. |
| `session_scope` is absent | `agent.session` returns nothing for the pane. |
| The plugin has no `detector` export | The pane is identified by `process_names` and `aliases` only. Its activity stays `none`. |

A runtime signature returns after every restart, because the plugin registers it again in `activate`.

## What the user sees

A review dialog lists `agent.signatures` as one checkbox. A user who unchecks it declines the grant. The wording below is from the manifest:

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.agent 0.1.0 by Example Ltd                                     │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Teaches Ilium to recognise agents <span class="t-dim">…………………………………………………</span> agent.signatures │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests agent.signatures, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

When the owner revokes the grant later, the terminal reports the withdrawal:

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants example-signatures --revoke agent.signatures
<span class="t-ok">ok</span>   agent.signatures revoked for example-signatures (server)
<span class="t-warn">warn</span> signature example-signatures.example-agent withdrawn from 1 pane
</pre>

The figure below shows the three sources and the registry. The second figure shows the order of one detection pass.

<figure>
<svg viewBox="0 0 410 250" role="img" aria-label="Three signature sources feed the signature registry, which the process walk and screen capture use">
  <defs>
    <marker id="arrow-sources" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="8" y="8" width="150" height="30" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="83" y="27" font-size="11" text-anchor="middle" fill="currentColor">Manifest contributes</text>
  <rect x="8" y="95" width="150" height="30" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="83" y="114" font-size="11" text-anchor="middle" fill="currentColor">Runtime register call</text>
  <rect x="8" y="170" width="150" height="30" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="83" y="189" font-size="11" text-anchor="middle" fill="currentColor">Detector export</text>
  <line x1="158" y1="23" x2="200" y2="75" stroke="currentColor" marker-end="url(#arrow-sources)"/>
  <line x1="158" y1="110" x2="200" y2="90" stroke="currentColor" marker-end="url(#arrow-sources)"/>
  <line x1="158" y1="185" x2="200" y2="105" stroke="currentColor" marker-end="url(#arrow-sources)"/>
  <rect x="200" y="60" width="200" height="60" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="300" y="94" font-size="11" text-anchor="middle" fill="currentColor">Signature registry</text>
  <line x1="300" y1="120" x2="300" y2="170" stroke="currentColor" marker-end="url(#arrow-sources)"/>
  <text x="306" y="148" font-size="10" fill="currentColor">matches</text>
  <rect x="200" y="170" width="200" height="50" rx="6" fill="none" stroke="currentColor"/>
  <text x="300" y="192" font-size="11" text-anchor="middle" fill="currentColor">Process walk and screen capture</text>
  <text x="8" y="240" font-size="10" fill="currentColor">The classify function is pure: it reads a snapshot.</text>
</svg>
<figcaption>A plugin contributes signatures in three places. The kernel keeps the registry and runs the process walk and the screen capture.</figcaption>
</figure>

<figure>
<svg viewBox="0 0 340 346" role="img" aria-label="Detection pipeline from process walk through identify, signature id, screen capture and classify to the activity event">
  <defs>
    <marker id="arrow-pipeline" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="20" y="10" width="300" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="170" y="32" font-size="11" text-anchor="middle" fill="currentColor">Process walk (kernel)</text>
  <line x1="170" y1="46" x2="170" y2="70" stroke="currentColor" marker-end="url(#arrow-pipeline)"/>
  <rect x="20" y="70" width="300" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="170" y="92" font-size="11" text-anchor="middle" fill="currentColor">identify (plugin detector export)</text>
  <line x1="170" y1="106" x2="170" y2="130" stroke="currentColor" marker-end="url(#arrow-pipeline)"/>
  <rect x="20" y="130" width="300" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="170" y="152" font-size="11" text-anchor="middle" fill="currentColor">Signature id (plugin prefix added)</text>
  <line x1="170" y1="166" x2="170" y2="190" stroke="currentColor" marker-end="url(#arrow-pipeline)"/>
  <rect x="20" y="190" width="300" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="170" y="212" font-size="11" text-anchor="middle" fill="currentColor">Screen snapshot (kernel)</text>
  <line x1="170" y1="226" x2="170" y2="250" stroke="currentColor" marker-end="url(#arrow-pipeline)"/>
  <rect x="20" y="250" width="300" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="170" y="272" font-size="11" text-anchor="middle" fill="currentColor">classify (plugin detector export)</text>
  <line x1="170" y1="286" x2="170" y2="308" stroke="currentColor" marker-end="url(#arrow-pipeline)"/>
  <rect x="20" y="308" width="300" height="30" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="170" y="327" font-size="11" text-anchor="middle" fill="currentColor">agent-activity-changed event</text>
</svg>
<figcaption>The kernel runs the process walk and the capture. The plugin names the agent, then classifies each new screen revision.</figcaption>
</figure>

## Detection loop

The poll loop that walks processes and captures screens stays native in the server. It decides when to poll (the adaptive interval) and which panes are due. It does not run plugin code. This section says where detector calls run and what a slow detector costs.

- **Where.** The loop posts detector calls to the plugin executor of the server, never to the loop thread. A call is asynchronous: the loop does not wait, and it applies the result at its next tick.
- **One call per pane.** `identify` takes one pane's process list and `classify` takes one pane's screen, so the loop makes one call per pane. It never batches panes into one call.
- **Parallel across plugins, serial inside one.** Calls to one plugin run one after the other, as for every export. Calls to different plugins run in parallel on different executor threads.
- **Order between detectors.** For an unidentified pane the loop asks the detectors in this order: bundled (T1) plugins first, then the others by plugin id. The first detector that returns a signature id wins, and the later ones are not asked for that pane. A pane that is identified is classified only by the detector whose signature identified it.
- **Per-poll budget.** One poll cycle may spend at most **50 ms** of detector call time in total. The loop starts the cycle at a rotating pane, so every pane gets its turn. When the budget is spent, the remaining panes keep their last activity and are first in line in the next cycle.
- **Slow detector.** The deadline of a call is soft (20 ms by default, key `detector_deadline_ms`, ceiling 50 ms for T1). A call that misses it keeps the previous classification, emits no event and is counted as a breach. The third breach in a row traps the plugin side, which counts as a failure. A slow detector can use the poll budget of one cycle, but it cannot delay another plugin's detector or the loop.
- **Expected cost.** With 30 panes and a detector that takes 0.5 ms per pane (benchmark B8), a full cycle costs 15 ms of executor time. A cycle in which only screens that changed are classified costs less.

| Case | Result |
|---|---|
| 30 panes, detector at 0.5 ms | 15 ms per full cycle, inside the budget |
| 30 panes, detector at 20 ms (the deadline) | 2 panes served per cycle; the others keep their activity and are served in the next cycles in rotation |
| Detector traps | Side restarted under the restart rule; panes keep their last activity during the restart |
| Two plugins both identify a pane | The first in the order above wins; the other is not asked |
| No detector plugin is running | The kernel's built-in table identifies agents as before |

- **[HST-450]** Detector calls MUST run on the plugin executor and MUST NOT run on the detection loop thread.
- **[HST-451]** The loop MUST make one detector call per pane and MUST NOT wait for a call inside its tick.
- **[HST-452]** For an unidentified pane the loop MUST ask T1 detectors first, then others by plugin id, and MUST stop at the first signature id.
- **[HST-453]** One poll cycle MUST spend at most 50 ms of detector call time, MUST start at a rotating pane, and MUST leave unserved panes with their last activity.
- **[HST-454]** A missed `detector_deadline_ms` MUST keep the previous classification, emit no event and count a breach, and the third breach in a row MUST trap the side.
- **[HST-455]** A slow detector MUST NOT delay another plugin's detector or the detection loop.
- **[HST-456]** `detector_deadline_ms` MUST default to 20 ms with a ceiling of 50 ms for T1 and 20 ms for T2 and T3.
- **[HST-457]** `classify` for 30 panes through a plugin detector MUST cost under 0.5 ms per pane at the 99th percentile (benchmark B8).

## Limits

- A signature has at most 32 actions, and each action's `keys` is at most 1 KiB.
- Each `identify` and `classify` call has a 20 ms soft deadline (`detector_deadline_ms`, T1 ceiling 50 ms). The rules for a slow detector and the per-poll budget are in the detection loop section above.
- The `id`, `process_names` and `aliases` limits are those of the table in the signature fields section.

## See also

- [agent.read](/pages/capability-agent-read/) for the activity values and the `agent-identified` event.
- [agent.control](/pages/capability-agent-control/) for the actions that a signature declares.
- [primitives: agent](/pages/primitives-agent/) for the `agent-view` record and the register call.
- [Exports](/pages/api-exports/) for the export traits and the `definePlugin` shape.
- [Manifest reference](/pages/reference-manifest/) for the `contributes.signatures` keys.

Checked against the API version `ilium:plugin@0.1.0`.

### Security

**[SEC-042]** `agent.signatures` changes how agents are detected, so a wrong signature can hide an agent's state. The grant is high risk and signatures are validated against the regex limits of [SEC-046]. See [Sandbox reference](/pages/reference-sandbox/).
