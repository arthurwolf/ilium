---
layout: page
title: Fleet
permalink: /pages/core-plugin-fleet/
---

## Fleet

The Fleet plugin has the id `ilium.core.fleet` and runs on both sides. It owns work with many agents at once: messaging one agent or all of them, Ask for update, `ilium panes`, `ilium broadcast`, `ilium new-pane` and the local HTTP API route `POST /create_agent`. It reaches other sessions on the same machine, and it reads cost figures from the service that the [Cost](/pages/core-plugin-cost/) plugin provides. The client side only draws the Ask for update dialog.

Spawning a PTY and typing into it are kernel-native. Fleet drives them through the `create-pane` and `send-input` intents and never touches a terminal directly.

### What it covers

- Agent messaging and Ask for update (see Automation, "Ask for update").
- The local HTTP API `POST /create_agent` (see Automation, "Local HTTP API", and Settings, "API").
- `ilium new-pane` (see CLI reference, "ilium new-pane").
- `ilium panes` (see CLI reference, "ilium panes").
- `ilium broadcast` (see CLI reference, "ilium broadcast").

### See it

<figure><img src="/assets/demos/05-ask-for-update.gif" alt="Ilium asks two idle local agent fixtures for a status update while a working fixture is skipped" loading="lazy"><figcaption>Ask for status update sends the built-in prompt to two idle local fixtures. The working fixture receives no input.</figcaption></figure>

### What the user sees

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium panes
<span class="t-hi">SESSION  PANE  AGENT   STATE      COST</span>
default  3     claude  <span class="t-ok">working</span>    $1.42
default  5     codex   <span class="t-dim">idle</span>       $0.31
review   2     claude  <span class="t-warn">approval</span>   $0.87
review   4     shell   <span class="t-dim">-</span>          <span class="t-dim">-</span></pre>
<figcaption>Rendered at 48 columns. Illustrative. <code>ilium panes</code> across two sessions, with the state column and the cost column that comes from the Cost plugin. The command prints JSONL when it is not on a terminal.</figcaption>
</figure>

<figure>
<pre class="term"><span class="t-dim">╭ Ask for update ─────────────────────────────╮</span>
<span class="t-dim">│</span> Message…… What is your status in one line? <span class="t-dim"> │</span>
<span class="t-dim">│</span>                                             <span class="t-dim">│</span>
<span class="t-dim">│</span>                  [ Send ]   [ Cancel ]      <span class="t-dim">│</span>
<span class="t-dim">╰─────────────────────────────────────────────╯</span></pre>
<figcaption>Rendered at 48 columns. Illustrative. The Ask for update dialog with a message field and Send and Cancel buttons. The dialog has no target field.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.fleet",
  "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0" },
  "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0" },
  "contributes": {
    "cli": [
      { "name": "panes", "summary": "List panes in every session", "output": "jsonl",
        "requires_server": true },
      { "name": "broadcast", "summary": "Send a line to agents", "output": "jsonl",
        "requires_server": true, "argument_groups": ["pane-selector"] },
      { "name": "new-pane", "summary": "Create a pane", "output": "mixed",
        "requires_server": true }
    ],
    "http_routes": [ { "method": "POST", "path": "/create_agent" } ],
    "commands": [ { "id": "ask-for-update", "title": "Ask for update" } ]
  },
  "capabilities": {
    "server": [
      "cli.contribute", "sessions.cross", "http-api.route:/create_agent",
      "terminal.write", "panes.create", "panes.read", "agent.read",
      "messages.peer:ilium.core.notifications",
      "services.use:ilium.core.cost/usage"
    ],
    "client": ["ui.panels"]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.fleet",
  "name": "Fleet",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "The Fleet plugin has the id ilium.core.fleet and runs on both sides.",
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
      "cli.contribute",
      "sessions.cross",
      "http-api.route:/create_agent",
      "terminal.write",
      "panes.create",
      "panes.read",
      "agent.read",
      "messages.peer:ilium.core.notifications",
      "services.use:ilium.core.cost/usage",
      "settings.write-kernel:api",
      "extensions.provide:ilium.core.fleet/selectors",
      "extensions.provide:ilium.core.fleet/before-broadcast"
    ],
    "client": [
      "ui.panels"
    ]
  },
  "activation": [
    "cli:panes",
    "cli:broadcast",
    "http:/create_agent",
    "service:broadcast",
    "command:ask-for-update"
  ],
  "contributes": {
    "cli": [
      {
        "name": "panes",
        "summary": "List panes in every session",
        "output": "jsonl",
        "requires_server": true
      },
      {
        "name": "broadcast",
        "summary": "Send a line to agents",
        "output": "jsonl",
        "requires_server": true,
        "argument_groups": [
          "pane-selector"
        ]
      },
      {
        "name": "new-pane",
        "summary": "Create a pane",
        "output": "mixed",
        "requires_server": true
      }
    ],
    "http_routes": [
      {
        "method": "POST",
        "path": "/create_agent"
      }
    ],
    "commands": [
      {
        "id": "ask-for-update",
        "title": "Ask for update"
      }
    ],
    "extension_points": [
      {
        "id": "selectors",
        "kind": "collect",
        "title": "Selectors",
        "entry_schema": {
          "type": "object",
          "required": [
            "id",
            "title",
            "rule"
          ],
          "properties": {
            "id": {
              "type": "string"
            },
            "title": {
              "type": "string"
            },
            "rule": {
              "type": "object"
            }
          }
        },
        "risk": "standard",
        "max_entries": 64,
        "dynamic": false,
        "version": "0.1.0"
      },
      {
        "id": "before-broadcast",
        "kind": "hook",
        "mode": "transform",
        "title": "Before broadcast",
        "payload_schema": {
          "type": "object",
          "required": [
            "message",
            "targets",
            "source"
          ],
          "properties": {
            "message": {
              "type": "string"
            },
            "targets": {
              "type": "array",
              "items": {
                "type": "string"
              }
            },
            "source": {
              "type": "string"
            }
          }
        },
        "reply_schema": {
          "type": "object",
          "required": [
            "message"
          ],
          "properties": {
            "message": {
              "type": "string"
            }
          }
        },
        "risk": "high",
        "deadline_ms": 20,
        "max_handlers": 8,
        "version": "0.1.0"
      }
    ]
  },
  "limits_tier": "first-party"
}
```

### Kernel tabs this plugin owns

This plugin is the single owner of the following built-in Settings tabs. It holds `settings.write-kernel:<tab>` for each one (T1 only, one holder per tab, see [kernel settings](/pages/api-kernel-settings-and-keymap/#writing-a-kernel-tab)). The values stay in the same `config.toml` tables as before, so no user setting moves. [COR-030] [COR-033]

| Tab id | Label | Config table | Readers |
|---|---|---|---|
| `api` | API | `[api]` | none |

If the plugin is disabled, the tab stays in the rail with its stored values shown read-only and the notice "Fleet is disabled. Values are kept." The kernel keeps reading the keys it consumes itself (listed in the Consumer column of the key table in [config and data migration](/pages/reference-config-and-data-migration/)). Behaviour that only this plugin provides is off. The exact result for this plugin is its row in [kernel and boot](/pages/reference-kernel-and-boot/). [COR-022]

### Samples

#### ilium panes

A CLI subcommand lists panes across every running session. The plugin declares `panes` under [cli.contribute](/pages/capability-cli-contribute/) ([the cli API](/pages/api-cli/)) and reads sessions under [sessions.cross](/pages/capability-sessions-cross/) ([the sessions API](/pages/api-sessions/)). On `cli-invoked` ([events](/pages/api-events/)) it lists the sessions, asks each one for its panes and prints one JSONL line per pane.

**Rust**

```rust
use std::collections::HashMap;

use ilium_plugin_sdk::host::sessions::{self, CrossRequest};
use ilium_plugin_sdk::host::cli;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::CliInvokedEvent;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Fleet {
    /// Request id to (invocation, session name).
    waiting: HashMap<RequestId, (u64, String)>,
    /// Invocation to the number of replies still expected.
    remaining: HashMap<u64, usize>,
}

impl Lifecycle for Fleet {}

impl Fleet {
    fn start_panes(&mut self, invocation: u64) {
        let live = sessions::list_live();
        let mut sent = 0;
        for session in &live {
            let request = CrossRequest {
                session: session.name.clone(),
                command: "list-panes".into(),
                arguments_json: "{}".into(),
            };
            if let Ok(id) = sessions::send(request) {
                self.waiting.insert(id, (invocation, session.name.clone()));
                sent += 1;
            }
        }
        self.remaining.insert(invocation, sent);
        if sent == 0 {
            cli::finish(invocation, 1);
        }
    }

    fn on_reply(&mut self, id: RequestId, is_ok: bool) {
        let Some((invocation, name)) = self.waiting.remove(&id) else { return };
        if let Some(text) = sessions::take_reply(id).filter(|_| is_ok) {
            let line = format!(r#"{{"type":"pane","session":"{name}","panes":{text}}}"#);
            let _ = cli::write_line(invocation, &line);
        }
        let Some(left) = self.remaining.get_mut(&invocation) else { return };
        *left -= 1;
        if *left == 0 {
            self.remaining.remove(&invocation);
            cli::finish(invocation, 0);
        }
    }
}

impl Handler for Fleet {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::CliInvoked(CliInvokedEvent { invocation, name, .. }) if name == "panes" => {
                    self.start_panes(invocation);
                }
                Event::RequestCompleted(done) => self.on_reply(done.request, done.outcome.is_ok()),
                _ => {}
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import * as cli from '@ilium/plugin-sdk/host/cli';
import * as sessions from '@ilium/plugin-sdk/host/sessions';

const waiting = new Map<bigint, { invocation: bigint; name: string }>();
const remaining = new Map<bigint, number>();

function startPanes(invocation: bigint): void {
  let sent = 0;
  for (const session of sessions.listLive()) {
    try {
      const id = sessions.send({ session: session.name, command: 'list-panes', argumentsJson: '{}' });
      waiting.set(id, { invocation, name: session.name });
      sent += 1;
    } catch {
      // A session that went away is skipped.
    }
  }
  remaining.set(invocation, sent);
  if (sent === 0) cli.finish(invocation, 1);
}

function onReply(id: bigint, isOk: boolean): void {
  const entry = waiting.get(id);
  if (entry === undefined) return;
  waiting.delete(id);
  const text = isOk ? sessions.takeReply(id) : undefined;
  if (text !== undefined) {
    cli.writeLine(entry.invocation, JSON.stringify({ type: 'pane', session: entry.name, panes: JSON.parse(text) }));
  }
  const left = (remaining.get(entry.invocation) ?? 1) - 1;
  remaining.set(entry.invocation, left);
  if (left === 0) {
    remaining.delete(entry.invocation);
    cli.finish(entry.invocation, 0);
  }
}

export const { handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'cli-invoked' && event.val.name === 'panes') startPanes(event.val.invocation);
      else if (event.tag === 'request-completed') onReply(event.val.request, event.val.outcome.tag === 'ok');
    }
    return [];
  },
});
```

#### ilium broadcast

A message goes to agents in every matching session. For other sessions the plugin calls `sessions.send` ([sessions API](/pages/api-sessions/), [sessions.cross](/pages/capability-sessions-cross/)). In the current session it submits `send-input` with `delivery: submitted` ([send-input](/pages/api-intent-send-input/)). The trigger is `cli-invoked` ([events](/pages/api-events/)).

**Rust**

```rust
use ilium_plugin_sdk::host::sessions::{self, CrossRequest};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::agent;
use ilium_plugin_sdk::types::intents::{Intent, SendInputSpec};
use ilium_plugin_sdk::types::intents::Delivery;

fn broadcast(text: &str) -> Vec<Submission> {
    // Other sessions: ask each one to run its own broadcast.
    for session in sessions::list_live().into_iter().filter(|session| !session.is_self) {
        let request = CrossRequest {
            session: session.name,
            command: "broadcast".into(),
            arguments_json: serde_json::json!({ "text": text }).to_string(),
        };
        let _ = sessions::send(request);
    }
    // This session: one send-input per agent pane.
    agent::agents()
        .into_iter()
        .map(|pane| {
            let spec = SendInputSpec {
                pane: pane.pane,
                text: text.to_string(),
                delivery: Delivery::Submitted,
                prompt_epoch: None,
                stages: Vec::new(),
                raw: None,
                mouse: None,
                if_current: None,
                lock_user_input: false,
                journal: true,
            };
            Submission::keyed(Intent::SendInput(spec), format!("broadcast-{}-{}", pane.pane, text.len()))
        })
        .collect()
}
```

**TypeScript**

```ts
import * as sessions from '@ilium/plugin-sdk/host/sessions';
import { agents } from '@ilium/plugin-sdk/primitives/agent';
import type { Submission } from '@ilium/plugin-sdk/types';

export function broadcast(text: string): Submission[] {
  for (const session of sessions.listLive().filter((entry) => !entry.isSelf)) {
    try {
      sessions.send({ session: session.name, command: 'broadcast', argumentsJson: JSON.stringify({ text }) });
    } catch {
      // The session went away; nothing to send.
    }
  }
  return agents().map((agent) => ({
    intent: {
      tag: 'send-input',
      val: {
        pane: agent.pane, text, delivery: 'submitted', promptEpoch: undefined, stages: [],
        raw: undefined, mouse: undefined, ifCurrent: undefined, lockUserInput: false, journal: true,
      },
    },
    correlation: 1n,
    dedupKey: `broadcast-${agent.pane}-${text.length}`,
  }));
}
```

#### Ask for update

A dialog sends a short request to one agent or to all of them. The dialog has no target field: the server component that receives the `ask-for-update` message sends the text to the agent pane that was focused when the dialog opened, or to every agent when no agent pane was focused. The client side shows a `dialog` ([layout and containers](/pages/widgets-layout-and-containers/)) with a `text-input` and buttons ([input widgets](/pages/widgets-input/)) under [ui.panels](/pages/capability-ui-panels/). On Send it submits a `message` to the server side ([messages and topics](/pages/api-messages-and-topics/)), which submits `send-input` to each target ([send-input](/pages/api-intent-send-input/)).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ui::{ActionKind, ActionValue};
use ilium_plugin_sdk::types::intents::{Intent, MessageSpec, MessageTarget, PluginSide};

fn render_dialog(tree: &mut Tree, text: &str) -> NodeRef {
    let message = tree.text_input("Message", text.to_string()).key("update-text");
    let send = tree.button("Send").key("update-send").role(ButtonRole::Primary);
    let cancel = tree.button("Cancel").key("update-cancel");
    tree.dialog("ask-for-update", "Ask for update")
        .child(message).child(send).child(cancel)
        .into_node()
}

fn on_action(key: &str, kind: ActionKind, value: ActionValue, text: &mut String) -> Vec<Submission> {
    match (key, kind, value) {
        ("update-text", ActionKind::Changed, ActionValue::Text(next)) => *text = next,
        ("update-send", ActionKind::Activated, _) if !text.trim().is_empty() => {
            return vec![Submission::new(Intent::Message(MessageSpec {
                to: MessageTarget { plugin: "ilium.core.fleet".into(), side: PluginSide::Server, client: None },
                channel: "ask-for-update".into(),
                payload: text.clone().into_bytes(),
            }))];
        }
        _ => {}
    }
    Vec::new()
}
```

**TypeScript**

```ts
import { button, dialog, textInput } from '@ilium/plugin-sdk/ui/build';
import type { Event, Submission } from '@ilium/plugin-sdk/types';

let text = '';

export const renderDialog = () =>
  dialog('ask-for-update', {
    title: 'Ask for update',
    children: [
      textInput('Message', text, { key: 'update-text' }),
      button('Send', { key: 'update-send', role: 'primary' }),
      button('Cancel', { key: 'update-cancel' }),
    ],
  });

export function onAction(event: Extract<Event, { tag: 'ui-action' }>): Submission[] {
  const { key, kind, value } = event.val;
  if (key === 'update-text' && kind === 'changed' && value.tag === 'text') text = value.val;
  if (key === 'update-send' && kind === 'activated' && text.trim() !== '') {
    return [{
      intent: {
        tag: 'message',
        val: {
          to: { plugin: 'ilium.core.fleet', side: 'server', client: undefined },
          channel: 'ask-for-update',
          payload: new TextEncoder().encode(text),
        },
      },
      correlation: 1n,
      dedupKey: undefined,
    }];
  }
  return [];
}
```

#### Hand a request to the notifications plugin

When an agent asks for a user decision, the fleet plugin sends a one-way message to the notifications plugin so it raises a desktop notification. The fleet plugin needs `messages.peer:ilium.core.notifications` for that. The notifications plugin needs no grant to receive it.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{Intent, MessageSpec, MessageTarget, PluginSide};

fn ask_user(pane_label: &str, question: &str) -> Submission {
    let payload = format!("{pane_label}: {question}").into_bytes();
    Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget {
            plugin: "ilium.core.notifications".into(),
            side: PluginSide::Server,
            client: None,
        },
        channel: "agent-question".into(),
        payload,
    }))
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk/types';

export function askUser(paneLabel: string, question: string): Submission {
  return {
    intent: {
      tag: 'message',
      val: {
        to: { plugin: 'ilium.core.notifications', side: 'server', client: undefined },
        channel: 'agent-question',
        payload: new TextEncoder().encode(`${paneLabel}: ${question}`),
      },
    },
    correlation: 2n,
    dedupKey: undefined,
  };
}
```

---

#### POST /create_agent

The kernel owns the HTTP listener, its port and its on/off setting (Settings, API tab). The fleet plugin owns only the route: it registers `POST /create_agent` with `http-api.route`, and the kernel returns 404 for the route while the plugin is disabled. [COR-010]

The local HTTP API creates an agent pane. The plugin routes `POST /create_agent` ([HTTP API](/pages/api-http-api/), [http-api.route](/pages/capability-http-api-route/)). On `http-called` ([events](/pages/api-events/)) it validates the JSON body and submits `create-pane` ([intents](/pages/api-intents/)). When the `request-completed` event carries the new node, it replies with the pane id.

On success the route answers 201 with `{"pane_id":N}`. When the host refuses the `create-pane` intent or it fails, the plugin answers 502 with `{"error":"create-pane failed"}`; an unparsable body answers 400 and another path or method answers 404.

**Rust**

```rust
use ilium_plugin_sdk::host::http_api;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::HttpCalledEvent;
use ilium_plugin_sdk::types::intents::{CreatePaneSpec, Intent};

#[derive(serde::Deserialize)]
struct CreateAgent {
    title: String,
    command: Vec<String>,
}

fn answer(request: &HttpCalledEvent, pending: &mut std::collections::HashMap<u64, u64>) -> Vec<Submission> {
    let route_path = request.path.split('?').next().unwrap_or("");
    if request.method != "POST" || route_path != "/create_agent" {
        let _ = http_api::respond(request.call, 404, &[], &[]);
        return Vec::new();
    }
    let Ok(body) = serde_json::from_slice::<CreateAgent>(&request.body) else {
        let _ = http_api::respond(request.call, 400, &[], br#"{"error":"invalid json"}"#);
        return Vec::new();
    };
    let spec = CreatePaneSpec {
        parent: None,
        title: body.title,
        content_kind: "ilium.terminal".into(),
        cwd: None,
        command: Some(body.command),
        initial_input: None,
    };
    let submission = Submission::keyed(Intent::CreatePane(spec), format!("http-{}", request.call));
    pending.insert(submission.correlation, request.call);
    vec![submission]
}

fn on_refused(call: u64) {
    // The create-pane intent was refused or failed: answer 502 so the HTTP caller never hangs.
    let _ = http_api::respond(call, 502, &[], br#"{"error":"create-pane failed"}"#);
}

fn on_created(call: u64, node: NodeId) {
    let headers = vec![("content-type".to_string(), "application/json".to_string())];
    let body = format!(r#"{{"pane_id":{node}}}"#);
    let _ = http_api::respond(call, 201, &headers, body.as_bytes());
}
```

**TypeScript**

```ts
import { respond } from '@ilium/plugin-sdk/host/http-api';
import type { HttpCalledEvent, Submission } from '@ilium/plugin-sdk/types';

const pending = new Map<bigint, bigint>(); // correlation to http call
let nextCorrelation = 1n;

export function answer(request: HttpCalledEvent): Submission[] {
  const routePath = request.path.split('?')[0];
  if (request.method !== 'POST' || routePath !== '/create_agent') {
    respond(request.call, 404, [], new Uint8Array());
    return [];
  }
  let body: { title: string; command: string[] };
  try {
    body = JSON.parse(new TextDecoder().decode(request.body));
  } catch {
    respond(request.call, 400, [], new TextEncoder().encode('{"error":"invalid json"}'));
    return [];
  }
  const correlation = nextCorrelation++;
  pending.set(correlation, request.call);
  return [{
    intent: {
      tag: 'create-pane',
      val: { parent: undefined, title: body.title, contentKind: 'ilium.terminal',
             cwd: undefined, command: body.command, initialInput: undefined },
    },
    correlation,
    dedupKey: `http-${request.call}`,
  }];
}

export function onRefused(correlation: bigint): void {
  // The create-pane intent was refused or failed: answer 502 so the HTTP caller never hangs.
  const call = pending.get(correlation);
  if (call === undefined) return;
  pending.delete(correlation);
  respond(call, 502, [], new TextEncoder().encode('{"error":"create-pane failed"}'));
}

export function onCreated(correlation: bigint, node: bigint): void {
  const call = pending.get(correlation);
  if (call === undefined) return;
  pending.delete(correlation);
  respond(call, 201, [['content-type', 'application/json']],
    new TextEncoder().encode(JSON.stringify({ pane_id: Number(node) })));
}
```

#### Cost column from a service

`ilium panes` asks the Cost plugin for the spend per pane. The plugin holds `services.use:ilium.core.cost/usage` ([services](/pages/capability-services/)) and calls it with `services::call` ([services API](/pages/api-services/)). When the reply is ready (the `request-completed` event), the SDK future resolves with the reply text. The plugin merges the figures into the JSONL lines.

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::services;
use ilium_plugin_sdk::prelude::*;

async fn cost_by_pane() -> std::collections::HashMap<u64, f64> {
    let payload = r#"{"window":"session"}"#;
    match services::call("ilium.core.cost/usage", "usage", payload).await {
        Ok(reply) => {
            let parsed: serde_json::Value = serde_json::from_str(&reply).unwrap_or_default();
            parsed["panes"]
                .as_object()
                .map(|panes| {
                    panes.iter()
                        .filter_map(|(pane, cost)| Some((pane.parse().ok()?, cost.as_f64()?)))
                        .collect()
                })
                .unwrap_or_default()
        }
        Err(error) => {
            log::log(Level::Warn, &format!("cost service failed: {}", error.message));
            Default::default()
        }
    }
}
```

**TypeScript**

```ts
import { call } from '@ilium/plugin-sdk/host/services';
import { log } from '@ilium/plugin-sdk/host/log';

export async function costByPane(): Promise<Map<number, number>> {
  const costs = new Map<number, number>();
  try {
    const reply = await call('ilium.core.cost/usage', 'usage', JSON.stringify({ window: 'session' }));
    const panes = (JSON.parse(reply) as { panes?: Record<string, number> }).panes ?? {};
    for (const [pane, cost] of Object.entries(panes)) costs.set(Number(pane), cost);
  } catch (error) {
    log('warn', `cost service failed: ${String(error)}`);
  }
  return costs;
}
```

### Extension points

Other plugins change what `ilium.core.fleet` does through the points below. The plugin owns each point: the main manifest holds the grant `extensions.provide:ilium.core.fleet/<point>` on the server side and declares the point in `contributes.extension_points`. Contributors need `extensions.contribute:ilium.core.fleet/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `selectors` | collect | Add a named selector to `ilium panes`, `ilium broadcast` and the dialogs. | entry: id, title, rule (JSON rule over signature, state, session and tags) | standard |
| `before-broadcast` | hook (transform) | Change or end a message before it goes to many agents. | message, targets, source; reply: message | high |

**Services** (grant `services.use:<name>`):

- `ilium.core.fleet/broadcast`: send: send a message to a selection
- `ilium.core.fleet/panes`: list: list panes of every session

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.fleet/broadcast-sent`: { targets, length }

Calls `ilium.core.cost/usage` for the cost column.

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.fleet"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Lets `ilium broadcast --select reviewers` reach every pane tagged `review`

A package adds this entry in its manifest. It needs no code.

```json
{
  "api": "ilium:plugin@0.1.0",
  "capabilities": { "server": ["extensions.contribute:ilium.core.fleet/selectors"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.fleet/selectors",
        "id": "reviewers",
        "priority": 50,
        "entry": {
          "title": "Reviewers",
          "rule": {
            "tag": "review"
          }
        }
      }
    ]
  }
}
```

### Extension points this plugin owns

The table gives the values for the two points this plugin owns. A value marked (default) is not stated on this page; it comes from the defaults in [Extension points](/pages/api-extension-points/). Both points run on the server side: the main manifest grants `extensions.provide` for each on the server side and declares each in `contributes.extension_points`.

| Point | Side | Version | Deadline | Max | Dynamic |
|---|---|---|---|---|---|
| `ilium.core.fleet/selectors` | server | 0.1.0 | none (default) | 64 entries | no |
| `ilium.core.fleet/before-broadcast` | server | 0.1.0 | 20 ms | 8 handlers | no (default) |

Side basis: `ilium.core.fleet/selectors` is server because its rules are applied by `ilium panes` and `ilium broadcast`, which the page documents as server commands (`requires_server: true`, with the panes and broadcast samples written as server code).
Side basis: `ilium.core.fleet/before-broadcast` is server because it changes a message before `ilium broadcast` sends it, a server-side command in the broadcast sample; the client side only draws the Ask for update dialog.

### ABI used

- [CLI](/pages/api-cli/) and [cli.contribute](/pages/capability-cli-contribute/)
- [Sessions](/pages/api-sessions/) and [sessions.cross](/pages/capability-sessions-cross/)
- [send-input](/pages/api-intent-send-input/), [intents](/pages/api-intents/) and [events](/pages/api-events/)
- [Layout and containers](/pages/widgets-layout-and-containers/), [input widgets](/pages/widgets-input/) and [ui.panels](/pages/capability-ui-panels/)
- [Messages and topics](/pages/api-messages-and-topics/)
- [messages.peer](/pages/capability-messages-peer/)
- [HTTP API](/pages/api-http-api/) and [http-api.route](/pages/capability-http-api-route/)
- [Services API](/pages/api-services/) and [services](/pages/capability-services/)

### See also

- [Core replacement matrix](/pages/reference-core-replacement-matrix/) for the surface, stored data, CLI names and install codes that a replacement of this plugin must keep.
- [Cost](/pages/core-plugin-cost/), which serves the usage figures used above.
- [Agent detection](/pages/core-plugin-agent-detect/), which decides what counts as an agent pane.
- [Agent toolbar](/pages/core-plugin-agent-toolbar/), which sends actions to a single agent.
