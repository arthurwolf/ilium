---
layout: page
title: Notifications
permalink: /pages/core-plugin-notifications/
---

## Notifications

Notifications is the plugin form of Ilium's alerts. It plays sounds for events, shows desktop notices, draws in-app toasts, merges bursts of redundant task alerts, and marks tree nodes that need you for the Attention display mode. Its id is `ilium.core.notifications` and it runs on both sides: the server side reacts to agent events and submits sounds, notices and marks, and the client side draws the toasts inside the Ilium window.

The kernel keeps the policy. The look of an attention mark, the done prefix and the sound rate belong to the kernel, so the plugin only decides when to ask.

### What it covers

- **Sound alerts.** One sound per event kind, chosen in Settings (Sounds in the notifications guide).
- **Desktop notifications.** A notice from the operating system for events you may miss (Desktop notifications).
- **Coalescing of redundant alerts.** Task alerts that repeat inside a window are merged or skipped (How task alerts are merged or skipped).
- **Attention marks on nodes.** Nodes that need you carry a mark until you acknowledge it (Normal mode and Attention mode in the agent monitoring guide).

### See it

<figure><img src="/assets/demos/05-ask-for-update.gif" alt="Ilium shows two idle local agent fixtures and a working local fixture during a status update request" loading="lazy"><figcaption>Local fixtures show idle and working activity during Ask for status update. This capture does not demonstrate an approval notification or sound.</figcaption></figure>

The recording shows the approval prompt. No recording shows the desktop toast or the mute toggle, so those parts use renders.

### What the user sees

The first render is a toast in the corner with a countdown line and two actions. The second is the sound tab of Settings.

<figure>
<pre class="term">╭─ Build finished ─────────────────────────────╮
│ Pane claude-api reached done. The report is  │
│ ready to open.                               │
│ Open pane · Dismiss                          │
│ ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━───────────────│
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A toast of tone success with two buttons, about 70 percent through its countdown gauge.</figcaption>
</figure>

<figure>
<pre class="term">╭─ Sounds ─────────────────────────────────────╮
│ Approval needed …………………………………………………… ‹ ask › │
│ Agent finished ………………………………………… ‹ finished › │
│ Task done ………………………………………………………………… ‹ none › │
│ Desktop notifications ……………………………………… ‹ On › │
│ Merge task alerts within …………………………………… 30 s │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Per-event sound choices, the desktop notification toggle and the merge window.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.notifications",
  "capabilities": {
    "server": ["sound.play", "notify.desktop", "agent.read", "tree.read", "tree.attention"],
    "client": ["notify.toast"]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.notifications",
  "name": "Notifications",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "Notifications is the plugin form of Ilium's alerts.",
  "license": "MIT",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0"
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0"
  },
  "capabilities": {
    "server": [
      "sound.play",
      "notify.desktop",
      "agent.read",
      "tree.read",
      "tree.attention"
    ],
    "client": [
      "notify.toast",
      "settings.write-kernel:sound"
    ]
  },
  "activation": [
    "startup"
  ],
  "limits_tier": "first-party"
}
```

### Kernel tabs this plugin owns

This plugin is the single owner of the following built-in Settings tabs. It holds `settings.write-kernel:<tab>` for each one (T1 only, one holder per tab, see [kernel settings](/pages/api-kernel-settings-and-keymap/#writing-a-kernel-tab)). The values stay in the same `config.toml` tables as before, so no user setting moves. [COR-030] [COR-033]

| Tab id | Label | Config table | Readers |
|---|---|---|---|
| `sound` | Sound | `[sound]`, `[notifications]` | none |

If the plugin is disabled, the tab stays in the rail with its stored values shown read-only and the notice "Notifications is disabled. Values are kept." The kernel keeps reading the keys it consumes itself (listed in the Consumer column of the key table in [config and data migration](/pages/reference-config-and-data-migration/)). Behaviour that only this plugin provides is off. The exact result for this plugin is its row in [kernel and boot](/pages/reference-kernel-and-boot/). [COR-022]

### Samples

#### 1. Sound on approval prompts

On `agent-activity-changed` to awaiting approval, the plugin submits `play-sound` with the sound chosen in Settings. This needs the [sound.play grant](/pages/capability-sound-play/); see [notifications and sounds](/pages/api-notifications/) and [events](/pages/api-events/). The dedup key stops a repeat of the same prompt from sounding twice.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::tree::AgentActivity;

// manifest: "capabilities": { "server": ["sound.play", "agent.read"] },
//   "contributes": { "sounds": [{ "id": "ask", "file": "sounds/ask.wav" }] }
impl Handler for Notifier {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::AgentActivityChanged(change) = event else { continue };
            if matches!(change.current, AgentActivity::AwaitingApproval) {
                // One sound per approval prompt; a new revision is a new prompt.
                submissions.push(Submission::keyed(
                    Intent::PlaySound(self.approval_sound.clone()),
                    format!("ask:{}:{}", change.node, change.revision),
                ));
            }
        }
        submissions
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';

// manifest: "capabilities": { "server": ["sound.play", "agent.read"] },
//   "contributes": { "sounds": [{ "id": "ask", "file": "sounds/ask.wav" }] }
let approvalSound = 'ask';
let correlation = 1n;

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'agent-activity-changed') continue;
      const change = event.val;
      if (change.current === 'awaiting-approval') {
        // One sound per approval prompt; a new revision is a new prompt.
        submissions.push({
          intent: { tag: 'play-sound', val: approvalSound },
          correlation: correlation++,
          dedupKey: `ask:${change.node}:${change.revision}`,
        });
      }
    }
    return submissions;
  },
});
```

#### 2. Desktop notification, coalesced

One notice for a burst of finished tasks. The plugin submits `notify` with a coalesce key and a 30,000 ms window, so notices inside the window replace each other. It needs the [notify.desktop grant](/pages/capability-notify-desktop/); the fields are in [notifications and sounds](/pages/api-notifications/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{NotifySpec, Urgency};

// manifest: "capabilities": { "server": ["notify.desktop"] }
/// A burst of finished tasks becomes one notice: notices with the same
/// coalesce key inside the window (at most 60,000 ms) replace each other.
fn task_done_notice(count: u32) -> Submission {
    Submission::new(Intent::Notify(NotifySpec {
        title: "Tasks finished".into(),
        body: format!("{count} tasks are done."),
        urgency: Urgency::Normal,
        sound: None,
        coalesce_key: Some("task-done".into()),
        coalesce_window_ms: 30_000,
    }))
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk';

// manifest: "capabilities": { "server": ["notify.desktop"] }
// Notices with the same coalesce key inside the window (at most 60,000 ms)
// replace each other, so a burst becomes one notice.
export function taskDoneNotice(count: number, correlation: bigint): Submission {
  return {
    intent: {
      tag: 'notify',
      val: {
        title: 'Tasks finished',
        body: `${count} tasks are done.`,
        urgency: 'normal',
        sound: undefined,
        coalesceKey: 'task-done',
        coalesceWindowMs: 30_000,
      },
    },
    correlation,
    dedupKey: undefined,
  };
}
```

#### 3. In-app toast with actions

A toast offers Open and Dismiss. The client submits `toast` with tone, title and two actions, using [toast](/pages/api-toast/), the [notify.toast grant](/pages/capability-notify-toast/) and the [chrome intents](/pages/api-intent-chrome/). Each button names a command that the plugin declares, and a press arrives as a `command-invoked` [event](/pages/api-events/); the dismiss button then submits `dismiss-toast`.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{ToastAction, ToastSpec};
use ilium_plugin_sdk::types::text::Tone;

// manifest: "capabilities": { "client": ["notify.toast"] },
//   "contributes": { "commands": [
//     { "id": "open-pane", "title": "Open the pane" },
//     { "id": "dismiss-toast", "title": "Dismiss" }] }
fn finished_toast(node: NodeId) -> Submission {
    Submission::new(Intent::Toast(ToastSpec {
        id: Some(format!("finished-{node}")),
        tone: Tone::Success,
        title: "Agent finished".into(),
        body: None,
        actions: vec![
            ToastAction { label: "Open".into(), command: "open-pane".into() },
            ToastAction { label: "Dismiss".into(), command: "dismiss-toast".into() },
        ],
        duration_ms: Some(10_000),
    }))
}

/// A press of a toast button arrives as command-invoked.
fn on_toast_command(command: &str, node: NodeId) -> Vec<Submission> {
    if command.ends_with("/dismiss-toast") {
        return vec![Submission::new(Intent::DismissToast(format!("finished-{node}")))];
    }
    Vec::new() // open-pane focuses the pane with the tree intents
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk';

// manifest: "capabilities": { "client": ["notify.toast"] },
//   "contributes": { "commands": [
//     { "id": "open-pane", "title": "Open the pane" },
//     { "id": "dismiss-toast", "title": "Dismiss" }] }
export function finishedToast(node: bigint): Submission {
  return {
    intent: {
      tag: 'toast',
      val: {
        id: `finished-${node}`,
        tone: 'success',
        title: 'Agent finished',
        body: undefined,
        actions: [
          { label: 'Open', command: 'open-pane' },
          { label: 'Dismiss', command: 'dismiss-toast' },
        ],
        durationMs: 10_000,
      },
    },
    correlation: 1n,
    dedupKey: undefined,
  };
}

// A press of a toast button arrives as command-invoked.
export function onToastCommand(command: string, node: bigint): Submission[] {
  if (command.endsWith('/dismiss-toast')) {
    return [{ intent: { tag: 'dismiss-toast', val: `finished-${node}` }, correlation: 2n, dedupKey: undefined }];
  }
  return []; // open-pane focuses the pane with the tree intents
}
```

#### 4. Attention marks

Nodes that need the user get a mark until acknowledged. The plugin submits `mark-attention` with a reason, and on `attention-changed` it submits `ack-attention` for the same node. This needs the [tree.attention grant](/pages/capability-tree-attention/); the event is in [events](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::MarkAttentionSpec;

// manifest: "capabilities": { "server": ["tree.attention", "tree.read"] }
impl Handler for Notifier {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::AgentActivityChanged(change) if is_waiting(change.current) => {
                    submissions.push(Submission::new(Intent::MarkAttention(MarkAttentionSpec {
                        node: change.node,
                        reason: "waiting for you".into(), // at most 80 characters
                    })));
                }
                // The user looked at the node: clear the mark for the same node.
                Event::AttentionChanged(changed) if changed.marked && self.seen(changed.node) => {
                    submissions.push(Submission::new(Intent::AckAttention(changed.node)));
                }
                _ => {}
            }
        }
        submissions
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';

// manifest: "capabilities": { "server": ["tree.attention", "tree.read"] }
let correlation = 1n;

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'agent-activity-changed' && isWaiting(event.val.current)) {
        submissions.push({
          intent: { tag: 'mark-attention', val: { node: event.val.node, reason: 'waiting for you' } },
          correlation: correlation++,
          dedupKey: undefined,
        });
      } else if (event.tag === 'attention-changed' && event.val.marked && seen(event.val.node)) {
        // The user looked at the node: clear the mark for the same node.
        submissions.push({
          intent: { tag: 'ack-attention', val: event.val.node },
          correlation: correlation++,
          dedupKey: undefined,
        });
      }
    }
    return submissions;
  },
});
```

#### 5. Mute sound with a toggle

The sound toggle in the toast sends `toggled` with a flag. The plugin stores the flag and skips `play-sound` while it is set. The flag is kept across restarts through the plugin's storage.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ui::{ActionKind, ActionValue};

impl Handler for MuteToggle {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::UiAction(action) = event else { continue };
            if let (ActionKind::Toggled, ActionValue::Flag(muted), "sound-mute") =
                (action.kind, action.value, action.key.as_str())
            {
                self.muted = muted;
                storage::put("muted", if muted { b"1" } else { b"0" });
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
export function onAction(event: Extract<Event, { tag: 'ui-action' }>): Submission[] {
  const { kind, value, key } = event.val;
  if (kind === 'toggled' && key === 'sound-mute' && value.tag === 'flag') {
    soundMuted = value.val;
    storage.put('muted', soundMuted ? '1' : '0');
  }
  return [];
}
```

### Extension points

Other plugins change what `ilium.core.notifications` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.notifications/<point>`). Contributors need `extensions.contribute:ilium.core.notifications/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `before-notify` | hook (veto) | Mute or refuse a notification, for example at night. | event, pane, title, urgency, channels | standard |
| `channels` | collect | Add a delivery channel such as a chat webhook. | entry: id, title, service (a service the plugin provides that delivers one notice) | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.notifications/notify`: send: raise a notice with a title, a body and an urgency

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.notifications/raised`: { event, pane, urgency }

The `agent-question` channel of `ilium.core.fleet` arrives here as a peer message.

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.notifications"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Refuses every notice with urgency `low` between 22:00 and 07:00

A package adds a `veto` handler. Handlers run until one refuses.

```json
{
  "api": "ilium:plugin@0.1.0",
  "exports": [
    "hook-handler"
  ],
  "capabilities": { "server": ["extensions.contribute:ilium.core.notifications/before-notify"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.notifications/before-notify",
        "id": "quiet-hours",
        "priority": 50,
        "handler": "quiet_hours"
      }
    ]
  }
}
```

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;

impl HookHandler for Plugin {
    fn handle_hook(&mut self, call: HookCall) -> HookReply {
        match call.extension.as_str() {
            "quiet_hours" => {
                // call.payload_json holds: event, pane, title, urgency, channels
                let _payload: serde_json::Value = match serde_json::from_str(&call.payload_json) {
                    Ok(value) => value,
                    Err(_) => return HookReply::Pass,
                };
                HookReply::Refuse("reason".into())
            }
            _ => HookReply::Pass,
        }
    }
}
```

**TypeScript**

```ts
definePlugin({
  handleHook(call) {
    if (call.extension !== 'quiet_hours') return { tag: 'pass' };
    // call.payloadJson holds: event, pane, title, urgency, channels
    const payload = JSON.parse(call.payloadJson);
    void payload;
    return { tag: 'refuse', val: 'reason' };
  },
});
```

A handler that is late, traps or returns a reply the mode does not allow counts as `pass`. See [Extension points](/pages/api-extension-points/#hooks).

### ABI used

- [notifications and sounds](/pages/api-notifications/), [sound.play](/pages/capability-sound-play/) and [notify.desktop](/pages/capability-notify-desktop/)
- [toast](/pages/api-toast/), [notify.toast](/pages/capability-notify-toast/) and [chrome intents](/pages/api-intent-chrome/)
- [tree.attention](/pages/capability-tree-attention/)
- [events](/pages/api-events/)
- [Storage](/pages/api-storage/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Agent detection](/pages/core-plugin-agent-detect/), the source of the activity events
- [Scheduler](/pages/core-plugin-scheduler/)

### Security

Risk marks of this plugin's extension points follow the corrected table in [Extension security](/pages/reference-extension-security/) ([SEC-293]).
