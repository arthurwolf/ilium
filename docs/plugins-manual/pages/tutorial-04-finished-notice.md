---
layout: page
title: "4. Finished-agent notice and toolbar button"
permalink: /pages/tutorial-04-finished-notice/
---

## 4. Finished-agent notice and toolbar button

When an agent finishes, the plugin plays a sound and shows a desktop notice. Both submissions carry a dedup key, so one finish raises one notice even if the event arrives twice. The plugin also adds a Compact button to the agent toolbar, which sends the agent's own `compact` action. The step reads the outcome of every submission, so the log shows duplicates and refusals.

### You will need

- [Tutorial step 3](/pages/tutorial-03-panel/) finished.
- Server grants: `agent.read` (already granted), `agent.control` (new, risk H: the user must grant it explicitly), `tree.read` (new), `notify.desktop` (new) and `sound.play` (new).
- Client grants: unchanged.
- Language: Rust (`server/src/finished.rs`, `server/src/lib.rs`) or TypeScript (`server/src/finished.ts`, `server/src/lib.ts`). Each listing below has a **Rust** block and a **TypeScript** block.
- Settings: the section `badges` (server scope, `show_badges` and `show_idle`) from step 2 and the section `panel` (client scope, `panel_show_idle`) from step 3 stay in the manifest. This step does not change them. Settings are stored per side, so the panel uses its own key. The `toast` intent at the end of the step needs the new grant `notify.toast`.

### What changes from step 3

- `server/src/lib.rs` handles two more events and one more command. Its `on_events` function gets three new arms.
- `server/src/finished.rs` is new. It builds the notice, the sound and the compact action.
- `manifest.json` adds a toolbar button, a sound, a command and four server grants.

### The surface this step uses

| Name | Kind | Fields and types | Meaning |
|---|---|---|---|
| `agent-activity-changed` | event | `node: node-id`, `previous`, `current: agent-activity`, `revision: u64` | Fires on every activity change. This step acts when `current` is `done`. |
| `command-invoked` | event | `command: string`, `context.pane: option<node-id>` | Delivers a press of the Compact button. The command is `compact-agent`. |
| `request-completed` | event | `request: request-ref`, `outcome: outcome` | Completes each submission exactly once. |
| `notify` | intent | `title` (1 to 80 characters), `body` (0 to 2,048), `urgency` (`low`, `normal`, `high`), `sound: option<string>` | Shows a desktop notice. Needs `notify.desktop`. Limit: 6 per minute. |
| `play-sound` | intent | sound id: `string` | Plays a sound from the catalogue. Needs `sound.play`. Limit: 4 per second. |
| `agent-action` | intent | `pane: node-id`, `action-id: string`, and a mandatory `dedup-key` | Presses the keys that the pane's signature declares for the action. Needs `agent.control`. |
| `outcome` | variant | `ok`, `ok-node`, `duplicate(request-ref)`, `refused(plugin-error)`, `failed(plugin-error)`, `cancelled`, `ok-epoch(u64)` | The result of one submission. `ok-epoch` is returned only by `send-input` on an agent pane; this step never sees it. A submission whose grant was revoked before it applied completes as `refused(revoked)`. |
| `dedup-key` | submission field | 1 to 128 characters from `[A-Za-z0-9._:/-]` | A repeat of a reserved key within 10 minutes completes as `duplicate` and has no effect. |
| `requires_action` | toolbar button key | an action id, such as `"compact"` | Shows the button only on panes whose signature declares that action. |

### The sequence

The plugin submits each notice with a dedup key. The kernel completes it with `request-completed`. A repeat with the same key completes as `duplicate` and does nothing.

<figure>
<svg viewBox="0 0 320 200" role="img" aria-label="Request and outcome sequence for a finished notice">
  <title>Finished notice sequence</title>
  <desc>Time runs down. The kernel sends agent-activity-changed to the plugin. The plugin submits notify and play-sound with dedup keys. The kernel replies with request-completed. A repeated notify with the same key returns a duplicate outcome.</desc>
  <defs>
    <marker id="fn-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <text x="40" y="14" font-size="11" text-anchor="middle" fill="currentColor">Kernel</text>
  <text x="280" y="14" font-size="11" text-anchor="middle" fill="currentColor">Plugin</text>
  <line x1="40" y1="20" x2="40" y2="190" stroke="currentColor" stroke-dasharray="3 3"/>
  <line x1="280" y1="20" x2="280" y2="190" stroke="currentColor" stroke-dasharray="3 3"/>
  <line x1="40" y1="44" x2="280" y2="44" stroke="currentColor" marker-end="url(#fn-arrow)"/>
  <text x="160" y="40" font-size="11" text-anchor="middle" fill="currentColor">agent-activity-changed: done</text>
  <line x1="280" y1="70" x2="40" y2="70" stroke="currentColor" marker-end="url(#fn-arrow)"/>
  <text x="160" y="66" font-size="11" text-anchor="middle" fill="currentColor">notify, key finished:n:r</text>
  <line x1="280" y1="96" x2="40" y2="96" stroke="currentColor" marker-end="url(#fn-arrow)"/>
  <text x="160" y="92" font-size="11" text-anchor="middle" fill="currentColor">play-sound, key finished:n:r:sound</text>
  <line x1="40" y1="122" x2="280" y2="122" stroke="currentColor" marker-end="url(#fn-arrow)"/>
  <text x="160" y="118" font-size="11" text-anchor="middle" fill="currentColor">request-completed: ok</text>
  <line x1="280" y1="146" x2="40" y2="146" stroke="currentColor" marker-end="url(#fn-arrow)"/>
  <text x="160" y="142" font-size="11" text-anchor="middle" fill="currentColor">notify again, same key</text>
  <line x1="40" y1="170" x2="280" y2="170" stroke="currentColor" marker-end="url(#fn-arrow)"/>
  <text x="160" y="166" font-size="11" text-anchor="middle" fill="currentColor">request-completed: duplicate</text>
  <text x="8" y="196" font-size="10" fill="currentColor">Time runs down.</text>
</svg>
<figcaption>Each submission completes exactly once. The repeated notice reuses its key, so it does nothing and reports duplicate.</figcaption>
</figure>

For node `42` at revision `7`, `dedup::key_for` gives `finished:42:7`. A new finish has a new revision and so a new key. The sound submission appends `:sound`, so the two submissions reserve separate keys.

### Manifest

The manifest adds the toolbar button, its command, and the sound file. The four new server grants go in `capabilities.server`.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.agent-tools",
  "name": "Agent tools",
  "version": "0.1.0",
  "publisher": "Example Developer",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": []
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": ["views"]
  },
  "capabilities": {
    "server": [
      "agent.read",
      "agent.control",
      "tree.read",
      "tree.decorate",
      "notify.desktop",
      "notify.toast",
      "sound.play"
    ],
    "client": ["agent.read", "tree.read", "ui.panels"]
  },
  "contributes": {
    "decoration_slots": [
      { "id": "badge", "placement": "badge", "priority": 10 }
    ],
    "commands": [
      { "id": "toggle-badges", "title": "Toggle agent badges" },
      { "id": "show-agents", "title": "Show agents panel" },
      { "id": "compact-agent", "title": "Compact the agent context" }
    ],
    "keybindings": [
      { "command": "example.agent-tools/toggle-badges", "chord": "b", "mode": "prefix" },
      { "command": "example.agent-tools/show-agents", "chord": "a", "mode": "prefix" }
    ],
    "toolbar_buttons": [
      {
        "id": "compact",
        "icon": "arrows-minimize",
        "label": "Compact",
        "command": "example.agent-tools/compact-agent",
        "applies_to": "all",
        "requires_action": "compact"
      }
    ],
    "panels": [
      { "id": "agents", "title": "Agents", "modal": false }
    ],
    "sounds": [
      { "id": "finished", "file": "sounds/finished.wav" }
    ],
    "settings": [
      {
        "kind": "tab",
        "id": "agent-tools",
        "title": "Agent tools",
        "sections": [
          {
            "id": "badges",
            "title": "Badges",
            "scope": "server",
            "rows": [
              {
                "type": "toggle",
                "key": "show_badges",
                "title": "Show agent badges",
                "description": "Turn every agent badge off with one switch.",
                "default": true
              },
              {
                "type": "toggle",
                "key": "show_idle",
                "title": "Show idle agents",
                "description": "Also show the badge of agents that are idle.",
                "default": true
              }
            ]
          },
          {
            "id": "panel",
            "title": "Agents panel",
            "scope": "client",
            "rows": [
              {
                "type": "toggle",
                "key": "panel_show_idle",
                "title": "Show idle agents in the panel",
                "description": "List agents that are idle in the Agents panel.",
                "default": true
              }
            ]
          }
        ]
      }
    ]
  }
}
```

The manifest rules that matter here:

- `requires_action: "compact"` hides the button on panes whose signature has no `compact` action.
- The sound file must be WAV (PCM) or OGG Vorbis under `sounds/`. A missing file or another format fails the package check, and the plugin is not installed.
- `agent.control` is risk H, so the user must tick it in the install dialog.

### The notice and action module

The module keeps the notice and the action logic apart from the handler. Each function returns submissions, and the handler sends them. In Rust, `Submission::keyed` sets the key and takes the next correlation number. In TypeScript, a module counter gives each submission a distinct correlation number.

**Rust**

```rust
// server/src/finished.rs. Finished notices and the compact action.
use ilium_plugin_sdk::dedup;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{AgentActionSpec, NotifySpec, Urgency};

pub const COMPACT_COMMAND: &str = "example.agent-tools/compact-agent";
pub const FINISHED_SOUND: &str = "finished";
const COMPACT_ACTION: &str = "compact";

/// The notice and the sound for one finish. The notice has no sound of its
/// own, so the sound plays once per finish.
pub fn on_finished(node: u64, title: &str, revision: u64) -> Vec<Submission> {
    let key = dedup::key_for(&["finished", &node.to_string(), &revision.to_string()]);
    let notice = NotifySpec {
        title: "Agent finished".to_string(),
        body: title.to_string(),
        urgency: Urgency::Normal,
        sound: None,
    };
    vec![
        Submission::keyed(Intent::Notify(notice), key.clone()),
        Submission::keyed(
            Intent::PlaySound(FINISHED_SOUND.to_string()),
            format!("{key}:sound"),
        ),
    ]
}

/// Presses the `compact` action of the pane's signature. Each press gets its
/// own key. The action's `allowed_when` check refuses a press while the agent
/// is working, so a second press during a compact does nothing.
pub fn compact_agent(pane: u64, now_unix_millis: u64) -> Submission {
    let key = dedup::key_for(&["compact", &pane.to_string(), &now_unix_millis.to_string()]);
    let spec = AgentActionSpec { pane, action_id: COMPACT_ACTION.to_string() };
    Submission::keyed(Intent::AgentAction(spec), key)
}
```

### The handler arms

**Rust**

The three new arms go into the `on_events` of `lib.rs`. The helpers from steps 1 to 3 (`to_kind`, `refresh_all`, `load_settings`) stay as they are, with two signature changes: `decorate` takes the activity revision as a third argument (`decorate(node, kind, revision, settings)`), and `toggle_badges` takes the loaded settings (`toggle_badges(settings)`). Update their definitions and the step 2 call sites to match. Add these imports to `lib.rs`: `use ilium_plugin_sdk::host::{clock, log::{self, Level}, tree};`.

```rust
// server/src/lib.rs, the handler body. Replaces the step 2 version.
impl handler::Guest for AgentTools {
    fn on_events(batch: Vec<Event>) -> Vec<Submission> {
        let settings = load_settings();
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::AgentActivityChanged(change) => {
                    let kind = to_kind(change.current);
                    submissions.push(decorate(change.node, kind, change.revision, settings));
                    if matches!(change.current, AgentActivity::Done) {
                        let title = tree::get(change.node)
                            .map(|node| node.title)
                            .unwrap_or_default();
                        let notices = finished::on_finished(change.node, &title, change.revision);
                        submissions.extend(notices);
                    }
                }
                Event::CommandInvoked(invocation) if invocation.command == TOGGLE_COMMAND => {
                    toggle_badges(settings);
                }
                Event::CommandInvoked(invocation)
                    if invocation.command == finished::COMPACT_COMMAND =>
                {
                    match invocation.context.pane {
                        Some(pane) => {
                            let now = clock::now_unix_millis();
                            submissions.push(finished::compact_agent(pane, now));
                        }
                        None => log::log(Level::Warn, "compact needs an agent pane"),
                    }
                }
                Event::SettingsChanged(change)
                    if change.keys.iter().any(|key| key.starts_with("show_")) =>
                {
                    submissions.extend(refresh_all(settings));
                }
                Event::RequestCompleted(result) => log_outcome(&result),
                _ => {}
            }
        }
        submissions
    }
}

/// Logs the outcome of one submission. `duplicate` means a submission with the
/// same key was already applied.
fn log_outcome(result: &RequestResult) {
    match &result.outcome {
        Outcome::Ok | Outcome::OkNode(_) => log::log(Level::Debug, "submission applied"),
        Outcome::Duplicate(_) => log::log(Level::Debug, "submission skipped: duplicate key"),
        Outcome::Refused(error) | Outcome::Failed(error) => {
            log::log(Level::Warn, &format!("not applied: {}", error.message));
        }
        Outcome::Cancelled => log::log(Level::Info, "submission cancelled"),
    }
}
```

**TypeScript**

The TypeScript module builds the same submissions. Node ids, revisions and the clock are `bigint`, and `dedupKey` is a plain string.

```ts
// server/src/finished.ts. Finished notices and the compact action.
import { log } from '@ilium/plugin-sdk/host/log';
import type { Outcome, Submission } from '@ilium/plugin-sdk/types';

export const COMPACT_COMMAND = 'example.agent-tools/compact-agent';
export const FINISHED_SOUND = 'finished';

let nextCorrelation = 1n;

/** The notice and the sound for one finish. */
export function onFinished(node: bigint, title: string, revision: bigint): Submission[] {
  const key = `finished:${node}:${revision}`;
  return [
    {
      intent: {
        tag: 'notify',
        val: { title: 'Agent finished', body: title, urgency: 'normal', sound: undefined },
      },
      correlation: nextCorrelation++,
      dedupKey: key,
    },
    {
      intent: { tag: 'play-sound', val: FINISHED_SOUND },
      correlation: nextCorrelation++,
      dedupKey: `${key}:sound`,
    },
  ];
}

/** Presses the `compact` action. Each press gets its own key. */
export function compactAgent(pane: bigint, nowUnixMillis: bigint): Submission {
  return {
    intent: { tag: 'agent-action', val: { pane, actionId: 'compact' } },
    correlation: nextCorrelation++,
    dedupKey: `compact:${pane}:${nowUnixMillis}`,
  };
}

/** Writes the outcome of one submission to the plugin log. */
export function logOutcome(outcome: Outcome): void {
  switch (outcome.tag) {
    case 'ok':
    case 'ok-node':
      log('debug', 'submission applied');
      return;
    case 'duplicate':
      log('debug', 'submission skipped: duplicate key');
      return;
    case 'refused':
    case 'failed':
      log('warn', `not applied: ${outcome.val.message}`);
      return;
    case 'cancelled':
      log('info', 'submission cancelled');
      return;
  }
}
```

The handler arm module follows. This `onEvents` replaces the step 2 version. It shows only the arms of this step. The badge and settings handling from steps 1 and 2 stays in the same function, unchanged:

```ts
// server/src/lib.ts, the onEvents body. Replaces the step 2 version.
import type { Event } from '@ilium/plugin-sdk';
import type { Submission } from '@ilium/plugin-sdk/types';
import { log } from '@ilium/plugin-sdk/host/log';
import { nowUnixMillis } from '@ilium/plugin-sdk/host/clock';
import { get } from '@ilium/plugin-sdk/host/tree';
import { COMPACT_COMMAND, compactAgent, logOutcome, onFinished } from './finished';

export function onEvents(batch: Event[]): Submission[] {
  const submissions: Submission[] = [];
  for (const event of batch) {
    if (event.tag === 'agent-activity-changed') {
      const change = event.val;
      if (change.current === 'done') {
        const title = get(change.node)?.title ?? '';
        submissions.push(...onFinished(change.node, title, change.revision));
      }
    } else if (event.tag === 'command-invoked' && event.val.command === COMPACT_COMMAND) {
      const pane = event.val.context.pane;
      if (pane === undefined) {
        log('warn', 'compact needs an agent pane');
      } else {
        submissions.push(compactAgent(pane, nowUnixMillis()));
      }
    } else if (event.tag === 'request-completed') {
      logOutcome(event.val.outcome);
    }
  }
  return submissions;
}
```

### In-app toast

A desktop notice leaves the terminal. A `toast` (grant `notify.toast`) stays inside the Ilium client and draws in its footer. Use it for a short message that needs no operating-system notice. The step 4 plugin defines this toast but does not send it: a compact press that is refused because the agent is working sends nothing, as the edge-case table below shows. Call `compact_busy_toast` from the refusal path if you want the message.

**Rust**

```rust
use ilium_plugin_sdk::types::intents::ToastSpec;
use ilium_plugin_sdk::types::text::Tone;

/// A short in-app message, keyed so a repeated refusal shows one toast.
pub fn compact_busy_toast(pane: u64) -> Submission {
    let spec = ToastSpec {
        text: "Agent is working; try Compact when it is idle".to_string(),
        tone: Tone::Warning,
        ..Default::default()
    };
    Submission::keyed(Intent::Toast(spec), format!("toast:compact-busy:{pane}"))
}
```

**TypeScript**

```ts
/** A short in-app message, keyed so a repeated refusal shows one toast. */
export function compactBusyToast(pane: bigint): Submission {
  return {
    intent: {
      tag: 'toast',
      val: { text: 'Agent is working; try Compact when it is idle', tone: 'warning' },
    },
    correlation: nextCorrelation++,
    dedupKey: `toast:compact-busy:${pane}`,
  };
}
```

The manifest of this tutorial already lists `notify.toast` in `capabilities.server`. The toast shows in the footer as a notice line.

### Points to notice

- Each Compact press gets a key built from the current time, so every press is a new request. The guard against a double compact is the activity check: while the agent is `working`, the second press is refused with `refused(conflict)`.
- Without `tree.read`, `get` returns `none`, so the title, and the notice body, is empty.

### Edge cases and errors

| Case | Outcome | What the plugin sees |
|---|---|---|
| `notify` or `play-sound` without its grant | `refused(not-permitted)`. The rest of the batch completes as `refused(cancelled)`. | A warn line. The finish raises no notice. |
| Sound id not in `contributes.sounds` | `refused(not-found)` for that submission only. | A warn line. Other submissions in the batch apply. |
| Seventh `notify` within one minute | `refused(rate-limited)` with `retry-after-ms`. | A warn line. The notice is dropped. |
| Same finish delivered twice within 10 minutes | `duplicate(request-ref)`. | A debug line. No second notice. |
| Compact while the agent is `working` | `refused(conflict)`, message `agent is working`. | A warn line. Nothing is sent. |
| Pane not restored yet | `refused(pane-not-ready)`. | A warn line. Retry on a later event. |
| Keys written to the agent fail after acceptance | `failed(code)`. The key stays reserved. | A warn line. The next press uses a new key. |
| Notice body over 2,048 characters | `refused(invalid-argument)`. | A warn line. |

Limits: `notify` 6 per minute, `play-sound` 4 per second, `agent-action` 20 per second per pane. A refused submission does not reserve its key.

#### No notification service

A desktop notice needs the desktop's notification service. Some sessions have none: an SSH login with no D-Bus session bus, a headless machine, or a minimal desktop with no notification daemon. In those sessions the `notify` submission still completes as `ok`, because `ok` means the notice was queued, not shown. No outcome, refusal code or request-completed field reports the missing service, so the plugin cannot detect it from the outcome.

What the user sees and what the plugin gets in that case:

- **The user sees no desktop pop-up.** Nothing on the screen says the notice was dropped. If an attached client reports terminal focus, the finish message still appears in the client footer, because that path does not use the desktop service.
- **The plugin receives `ok` for `notify`.** Do not retry it, and do not treat the missing pop-up as a failure. The retry rule for `ok` in [notifications](/pages/api-notifications/) applies.
- **The sound is a separate submission.** `play-sound` depends on the audio device, not on the notification service. It completes as `failed(io)` when the client has no audio output (see [sound.play](/pages/capability-sound-play/)). Whether a sound id inside `notify` plays when no notice can be shown is not defined by the manual, so this step submits `play-sound` on its own.
- **The plugin log shows nothing for the dropped notice.** `ilium plugin logs` records the plugin's own lines and the refusal lines of the host. A notice that was queued and not shown leaves no line there, so the step's log cannot prove the user was not notified.

Handle this case in the plugin, not in the host. Keep the in-app record of each finish: a toast with `notify.toast` (see [in-app toast](#in-app-toast)) or the status line of the agents panel from [step 3](/pages/tutorial-03-panel/). These show in the Ilium client and do not need a desktop service. Keep the notice's text self-explanatory too, because a screen reader gets nothing from `notify` (see [notifications](/pages/api-notifications/)).

To test the case, start the session with `DBUS_SESSION_BUS_ADDRESS` set to a socket path that does not exist, finish an agent, and check two things: the `notify` submission still reports `ok` in `ilium plugin test` output, and no pop-up appears on the desktop. The second check is manual. The expected result is the one above; the step does not add a retry or a fallback that depends on the desktop.

### Test it

Run the plugin, start an agent, and let it finish. The notice and the sound appear, and `ilium plugin logs` shows the outcome. When an attached client reports terminal focus on the pane, the notice goes to the client footer instead of the desktop.

To test the duplicate case, write a JSON fixture that lists the same finish twice. `ilium plugin test` prints each submission and its outcome as JSONL, and the second notice shows `duplicate`.

```bash
ilium plugin dev
ilium plugin test . --fixture fixtures/finish-twice.json
ilium plugin logs example.agent-tools
```

### What you see

The toolbar of an agent pane shows the new button only when the signature has a `compact` action. The desktop notice is drawn by the operating system and is not shown.

<figure>
<pre class="term">╭  ≡ ● · api-refactor · claude ────────────────────────────╮
│ (agent terminal output)                                  │
│                                                          │
╰──────────────────────────────────────────────────────────╯
 <span class="t-hi"> Compact </span>  Interrupt  Menu
</pre>
<figcaption>Illustrative. A Claude pane with the new Compact toolbar button, drawn here as the selected entry. The pane content is a stand-in.</figcaption>
</figure>

<figure>
<pre class="term">╭  ≡ ● · review-codex · codex ─────────────────────────────╮
│ (agent terminal output)                                  │
│                                                          │
╰──────────────────────────────────────────────────────────╯
  Interrupt  Menu
</pre>
<figcaption>Illustrative. The same toolbar on a pane whose signature has no compact action. The Compact button is hidden.</figcaption>
</figure>

<figure>
<pre class="term">╭  ≡ ● · Agent finished ───────────────────────────────────╮
│ ℹ api-refactor is done. Compact is available.            │
╰──────────────────────────────────────────────────────────╯
</pre>
<figcaption>Illustrative. How the finish message reads when the client shows it inside Ilium (focus case or a toast). The desktop notice itself is drawn by the operating system.</figcaption>
</figure>

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs example.agent-tools
<span class="t-dim">2026-10-11 12:04:02</span> debug example.agent-tools submission applied
<span class="t-dim">2026-10-11 12:04:02</span> debug example.agent-tools submission applied
<span class="t-dim">2026-10-11 12:04:03</span> debug example.agent-tools submission skipped: duplicate key
<span class="t-warn">2026-10-11 12:09:44</span> warn  example.agent-tools not applied: agent is working
</pre>
<figcaption>Illustrative, not generated. The four lines are the outcomes that a finish and a compact press can produce. The message text comes from the kernel.</figcaption>
</figure>

### See also

- [Tutorial step 3](/pages/tutorial-03-panel/) adds the client side that shows agents.
- [Tutorial step 5: TODO scanner](/pages/tutorial-05-todo-scanner/) starts a background job.
- [Primitive: agent](/pages/primitives-agent/) describes activity values, signatures and the toolbar.
- [Capability: agent.control](/pages/capability-agent-control/) describes `agent-action`, dedup keys and `requires_action`.
- [Capability: notify.desktop](/pages/capability-notify-desktop/) and [Capability: sound.play](/pages/capability-sound-play/) cover the two output intents.
- [Intents and outcomes](/pages/api-intents/) and [Events and intents](/pages/events-and-intents/) describe submissions, batches and dedup keys.

API checked against `ilium:plugin@0.1.0`. The `toast` intent needs the `notify.toast` grant.
