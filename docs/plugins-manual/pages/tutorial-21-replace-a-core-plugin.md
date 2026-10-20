---
layout: page
title: "21. Replace a core plugin: Titles Lite"
permalink: /pages/tutorial-21-replace-a-core-plugin/
---

## 21. Replace a core plugin: Titles Lite

This step replaces a core plugin. The plugin is `example.titles-lite`. It takes the place of `ilium.core.titles`, the plugin that names panes from the prompts you send. The replacement is deliberately small. It names a pane once, from the first prompt, with four words or fewer. It keeps every item of the surface of the plugin it replaces, so the services, points and topics that other plugins use keep working. It drops the rest: the preview panel, the tree tidy and the settings tab.

Replacement is the heaviest of the five ways to work with a core plugin. The built-in plugin is turned off while yours is on, and you must keep up with it. Use it only when no extension point is enough. [Extending core plugins](/pages/extending-core-plugins/) compares all five, and [Reference: core extension points](/pages/reference-core-extension-points/) lists the surface of each plugin. [Core plugin: titles](/pages/core-plugin-titles/) describes the plugin you replace. This page does not repeat those tables.

You start a fresh plugin. You have finished tutorials 1 to 9 and read [Extension points](/pages/api-extension-points/), [tutorial 18](/pages/tutorial-18-add-an-extension-point/) and [tutorial 19](/pages/tutorial-19-stop-core-actions-with-hooks/). Tutorial plugin: `example.titles-lite`, version 0.1.0, API `ilium:plugin@0.1.0`. Rust and TypeScript behave the same in every step.

### You will need

- Rust: stable 1.90 or later with the `wasm32-wasip2` target, and the crates `ilium-plugin-sdk` and `serde_json`. TypeScript: Bun 1.2 or later and `@ilium/plugin-sdk`.
- Grants, with the reason for each in the table of step 5: `inference`, `agent.read-prompts`, `tree.read`, `tree.write`, `services.provide:ilium.core.titles/suggest`, `extensions.provide:ilium.core.titles/before-apply-title`, `extensions.provide:ilium.core.titles/title-styles` and `topics.subscribe:ilium.extensions/changed`.
- `"unstable": ["plugin-services"]`, because `suggest` is a plugin-defined service (dev mode, T2 and T3).
- An AI provider that the user has set up. The titles come from [Inference providers](/pages/core-plugin-inference/).
- A scratch folder for package inspection: `/media/arthur/tmp/titles-lite/` with a `.scratch-reaper` file in it.

### The surface this step uses

`ilium.core.titles` is replaceable and lists four surface items. A replacement must provide all four.

| Surface item | What the replacement provides | Declared as |
|---|---|---|
| `services:suggest` | A service that returns a title for some text | `contributes.services.provides` and `services.provide:ilium.core.titles/suggest` |
| `points:before-apply-title` | A transform hook, run before a title is applied | `contributes.extension_points` and `extensions.provide:ilium.core.titles/before-apply-title` |
| `points:title-styles` | A collect point of styles with an instruction text | `contributes.extension_points` and `extensions.provide:ilium.core.titles/title-styles` |
| `topics:title-applied` | A topic with `{ node, title }` | `contributes.topics.publishes` |

The replacement uses the names of the plugin it replaces, such as `ilium.core.titles/suggest`. This is what lets every extension and every caller written for the core plugin go on working. It is allowed only for a plugin that lists the id in `replaces`. Any other plugin that uses a name with another plugin's id is refused.

Other parts of the surface of this step: the top-level key `"replaces": ["ilium.core.titles"]` (at most 4 ids), the host call `inference::complete_request` for the model, `extensions::run_hook_request` and `take_reply` for the hook, and `topics::publish`.

### The flow

One prompt becomes one title in four moves. The model proposes, the hook may adjust, the plugin applies and then tells the rest of the product.

<figure>
<svg viewBox="0 0 480 330" role="img" aria-label="Sequence of naming a pane">
  <title>Naming a pane with Titles Lite</title>
  <desc>The user sends a prompt. Titles Lite asks the inference provider for a title. It runs the before-apply-title hook. It renames the pane and publishes the title-applied topic.</desc>
  <defs>
    <marker id="t21-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="8" width="70" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="45" y="25" font-size="11" text-anchor="middle" fill="currentColor">Agent pane</text>
  <rect x="130" y="8" width="90" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="175" y="25" font-size="11" text-anchor="middle" fill="currentColor">Titles Lite</text>
  <rect x="270" y="8" width="90" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="315" y="25" font-size="11" text-anchor="middle" fill="currentColor">Inference</text>
  <rect x="400" y="8" width="70" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="435" y="25" font-size="11" text-anchor="middle" fill="currentColor">Registry</text>
  <line x1="45" y1="34" x2="45" y2="320" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="175" y1="34" x2="175" y2="320" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="315" y1="34" x2="315" y2="320" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="435" y1="34" x2="435" y2="320" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="45" y1="58" x2="175" y2="58" stroke="currentColor" marker-end="url(#t21-arrow)"/>
  <text x="110" y="52" font-size="10" text-anchor="middle" fill="currentColor">input-submitted</text>
  <line x1="175" y1="92" x2="315" y2="92" stroke="currentColor" marker-end="url(#t21-arrow)"/>
  <text x="245" y="86" font-size="10" text-anchor="middle" fill="currentColor">complete (4 words)</text>
  <line x1="315" y1="126" x2="175" y2="126" stroke="currentColor" marker-end="url(#t21-arrow)"/>
  <text x="245" y="120" font-size="10" text-anchor="middle" fill="currentColor">request-completed</text>
  <line x1="175" y1="168" x2="435" y2="168" stroke="currentColor" marker-end="url(#t21-arrow)"/>
  <text x="305" y="162" font-size="10" text-anchor="middle" fill="currentColor">run-hook(before-apply-title)</text>
  <line x1="435" y1="200" x2="175" y2="200" stroke="currentColor" marker-end="url(#t21-arrow)"/>
  <text x="305" y="194" font-size="10" text-anchor="middle" fill="currentColor">final title</text>
  <line x1="175" y1="244" x2="45" y2="244" stroke="currentColor" marker-end="url(#t21-arrow)"/>
  <text x="110" y="238" font-size="10" text-anchor="middle" fill="currentColor">rename-node</text>
  <line x1="175" y1="286" x2="435" y2="286" stroke="currentColor" stroke-dasharray="5 3" marker-end="url(#t21-arrow)"/>
  <text x="305" y="280" font-size="10" text-anchor="middle" fill="currentColor">publish title-applied</text>
</svg>
<figcaption>The pane's own prompt and the dashed publish are the two ends. The hook is the only place where another plugin can change the title.</figcaption>
</figure>

### Step 1: the manifest

The manifest has the key `replaces`, the four surface items and the grants. The plugin has a server side only. The core plugin has a client side for its preview panel, and the replacement does not need one.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.titles-lite",
  "name": "Titles Lite",
  "version": "0.1.0",
  "publisher": "Example Developer",
  "description": "Names a pane once, from its first prompt, in four words or fewer.",
  "icon": "tag",
  "replaces": ["ilium.core.titles"],
  "unstable": ["plugin-services"],
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": ["service-provider"]
  },
  "capabilities": {
    "server": [
      "inference",
      "agent.read-prompts",
      "tree.read",
      "tree.write",
      "services.provide:ilium.core.titles/suggest",
      "extensions.provide:ilium.core.titles/before-apply-title",
      "extensions.provide:ilium.core.titles/title-styles",
      "topics.subscribe:ilium.extensions/changed"
    ]
  },
  "activation": ["startup"],
  "contributes": {
    "topics": {
      "publishes": ["ilium.core.titles/title-applied"],
      "subscribes": ["ilium.extensions/changed"]
    },
    "services": {
      "provides": [
        { "name": "ilium.core.titles/suggest", "version": "0.1.0", "schema": "schemas/suggest.json" }
      ]
    },
    "extension_points": [
      {
        "id": "before-apply-title",
        "kind": "hook",
        "mode": "transform",
        "title": "Before a title is applied",
        "version": "0.1.0",
        "deadline_ms": 20,
        "payload_schema": {
          "type": "object",
          "required": ["node", "proposed", "source"],
          "properties": {
            "node": { "type": "integer" },
            "proposed": { "type": "string", "maxLength": 120 },
            "source": { "type": "string", "enum": ["transcript", "process", "manual-trigger"] },
            "previous": { "type": "string", "maxLength": 120 }
          }
        },
        "reply_schema": {
          "type": "object",
          "required": ["proposed"],
          "properties": { "proposed": { "type": "string", "maxLength": 120 } }
        }
      },
      {
        "id": "title-styles",
        "kind": "collect",
        "title": "Title styles",
        "version": "0.1.0",
        "max_entries": 16,
        "entry_schema": {
          "type": "object",
          "required": ["id", "title", "instruction"],
          "properties": {
            "id": { "type": "string", "maxLength": 64 },
            "title": { "type": "string", "maxLength": 60 },
            "instruction": { "type": "string", "maxLength": 400 }
          }
        }
      }
    ]
  }
}
```

`schemas/suggest.json` describes the `title` method: its params are `{ "text": string }` with a `maxLength` of 4000, and its result is `{ "title": string }`.

Points to notice:

- `replaces` takes ids. Install checks every one. An unknown id fails with `manifest.replaces-unknown`. A core plugin with `replaceable: false` fails with `manifest.replaces-not-replaceable`. Two packages that replace each other fail with `manifest.replaces-cycle`. No grant is needed for `replaces`. The consent is the user's, in the review dialog.
- The service, the points and the topic use the id `ilium.core.titles`, not `example.titles-lite`. A name with a foreign id is refused in any plugin that does not replace that id.
- The point schemas must be compatible with the core plugin's: the same field names and types, so a handler written for the core plugin sees what it expects. Copy the shape from [Reference: core extension points](/pages/reference-core-extension-points/) and the page [Core plugin: titles](/pages/core-plugin-titles/), and keep `version` at the same number.
- There is no `tree.restructure`. That high-risk grant belongs to the tree tidy, which the replacement drops. The review dialog is shorter for it.
- The plugin subscribes to `ilium.extensions/changed` because it owns `title-styles` and reloads the list when a contributor changes it. That topic goes to the owner only.

### Step 2: the logic

The plugin has four jobs. It names a pane from the first prompt. It answers the `suggest` service. It runs the hook before it applies a title. It publishes the topic. All four share one small state machine, because each step ends in a `request-completed` event.

The state is a map from request id to what the request was for.

| Pending kind | Started by | Ends with |
|---|---|---|
| `Inference` for a pane | `input-submitted` | Run the hook with the proposed title |
| `Inference` for a service call | A `suggest` call | Reply to the call with the title |
| `Hook` for a pane | End of the inference | Rename the pane, publish the topic |

Titles Lite keeps a set of panes it has already titled. It names a pane once. A title typed by hand is never replaced, because the host answers `refused(conflict)` to an inferred rename against a manual title, and the plugin only reads the outcome.

**Rust**

```rust
// src/lib.rs
use std::collections::{HashMap, HashSet};

use ilium_plugin_sdk::dedup;
use ilium_plugin_sdk::host::extensions;
use ilium_plugin_sdk::host::inference::{self, ChatMessage, CompletionRequest};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::services;
use ilium_plugin_sdk::host::topics;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::RenameNodeSpec;
use ilium_plugin_sdk::types::tree::TitleSource;
use serde_json::{json, Value};

const BEFORE_APPLY: &str = "ilium.core.titles/before-apply-title";
const STYLES: &str = "ilium.core.titles/title-styles";
const APPLIED: &str = "ilium.core.titles/title-applied";
const DEFAULT_INSTRUCTION: &str = "Reply with a title of at most four words and nothing else.";

enum Pending {
    PaneTitle(NodeId),
    ServiceTitle(u64),
    Hook { node: NodeId, proposed: String },
}

#[ilium_plugin(side = "server", exports = ["service-provider"])]
#[derive(Default)]
struct TitlesLite {
    pending: HashMap<RequestId, Pending>,
    titled: HashSet<NodeId>,
    instruction: String,
}

/// Keeps a model reply to at most four plain words.
pub fn clean_title(raw: &str) -> String {
    let words: Vec<&str> = raw
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'' || c == '`' || c == '.')
        .split_whitespace()
        .take(4)
        .collect();
    words.join(" ")
}

/// The instruction of the first style in run order, or the default.
pub fn instruction_from(entries_json: &[String]) -> String {
    entries_json
        .iter()
        .filter_map(|entry| serde_json::from_str::<Value>(entry).ok())
        .find_map(|entry| entry["instruction"].as_str().map(str::to_string))
        .unwrap_or_else(|| DEFAULT_INSTRUCTION.to_string())
}

fn ask(instruction: &str, text: &str) -> Result<RequestId, PluginError> {
    inference::complete_request(CompletionRequest {
        messages: vec![
            ChatMessage { role: "system".into(), content: instruction.into() },
            ChatMessage { role: "user".into(), content: text.chars().take(4000).collect() },
        ],
        max_output_tokens: Some(24),
        json_schema: None,
        purpose: "pane-title".into(),
    })
}

impl TitlesLite {
    fn reload_styles(&mut self) {
        let entries = extensions::list(STYLES).unwrap_or_default();
        let json_entries: Vec<String> = entries.into_iter().map(|entry| entry.entry_json).collect();
        self.instruction = instruction_from(&json_entries);
    }
}

impl Lifecycle for TitlesLite {
    fn activate(&mut self) {
        self.reload_styles();
    }
}

impl Handler for TitlesLite {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::TopicPublished(published) if published.topic == "ilium.extensions/changed" => {
                    self.reload_styles();
                }
                Event::InputSubmitted(input) if self.titled.insert(input.node) => {
                    match ask(&self.instruction, &input.text) {
                        Ok(id) => {
                            self.pending.insert(id, Pending::PaneTitle(input.node));
                        }
                        Err(error) => {
                            self.titled.remove(&input.node);
                            log::log(Level::Warn, &format!("no title: {}", error.code));
                        }
                    }
                }
                Event::RequestCompleted(done) => {
                    let Some(pending) = self.pending.remove(&done.request) else { continue };
                    self.finish(pending, done.request, &mut submissions);
                }
                _ => {}
            }
        }
        submissions
    }
}

impl TitlesLite {
    fn finish(&mut self, pending: Pending, request: RequestId, submissions: &mut Vec<Submission>) {
        match pending {
            Pending::PaneTitle(node) => {
                let proposed = inference::take_completion(request).map(|c| clean_title(&c.text)).unwrap_or_default();
                if proposed.is_empty() {
                    self.titled.remove(&node);
                    return;
                }
                let payload = json!({ "node": node, "proposed": proposed, "source": "transcript" }).to_string();
                match extensions::run_hook_request(BEFORE_APPLY, &payload) {
                    Ok(id) => {
                        self.pending.insert(id, Pending::Hook { node, proposed });
                    }
                    Err(_) => apply(node, &proposed, submissions),
                }
            }
            Pending::ServiceTitle(call) => {
                let title = inference::take_completion(request).map(|c| clean_title(&c.text)).unwrap_or_default();
                let _ = services::reply(call, Ok(json!({ "title": title }).to_string()));
            }
            Pending::Hook { node, proposed } => {
                // No handler, or a refused chain, leaves the proposed title as it is.
                let adjusted = extensions::take_reply(request)
                    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                    .and_then(|value| value["proposed"].as_str().map(clean_title))
                    .filter(|title| !title.is_empty());
                apply(node, &adjusted.unwrap_or(proposed), submissions);
            }
        }
    }
}

fn apply(node: NodeId, title: &str, submissions: &mut Vec<Submission>) {
    let key = dedup::key_for(&["title", &node.to_string(), title]);
    submissions.push(Submission::keyed(
        Intent::RenameNode(RenameNodeSpec { node, title: title.to_string(), source: TitleSource::Inferred, expected_revision: None }),
        key,
    ));
    let payload = json!({ "node": node, "title": title }).to_string();
    if let Err(error) = topics::publish(APPLIED, &payload) {
        log::log(Level::Warn, &format!("publish failed: {error:?}"));
    }
}

impl ServiceProvider for TitlesLite {
    fn handle(&mut self, call: ServiceCall) -> ServiceReply {
        let params: Value = serde_json::from_str(&call.payload_json).unwrap_or(Value::Null);
        let Some(text) = params["text"].as_str() else {
            return ServiceReply::Done(json!({ "title": "" }).to_string());
        };
        match ask(&self.instruction, text) {
            Ok(id) => {
                self.pending.insert(id, Pending::ServiceTitle(call.call));
                ServiceReply::Deferred
            }
            Err(_) => ServiceReply::Done(json!({ "title": "" }).to_string()),
        }
    }
}
```

**TypeScript**

```ts
// src/server.ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import type { ServiceCall, ServiceReply } from '@ilium/plugin-sdk';
import { list, runHookRequest, takeReply } from '@ilium/plugin-sdk/host/extensions';
import { completeRequest, takeCompletion } from '@ilium/plugin-sdk/host/inference';
import { log } from '@ilium/plugin-sdk/host/log';
import { reply } from '@ilium/plugin-sdk/host/services';
import { publish } from '@ilium/plugin-sdk/host/topics';

const BEFORE_APPLY = 'ilium.core.titles/before-apply-title';
const STYLES = 'ilium.core.titles/title-styles';
const APPLIED = 'ilium.core.titles/title-applied';
const DEFAULT_INSTRUCTION = 'Reply with a title of at most four words and nothing else.';

type Pending =
  | { kind: 'pane'; node: bigint }
  | { kind: 'service'; call: bigint }
  | { kind: 'hook'; node: bigint; proposed: string };

const pending = new Map<bigint, Pending>();
const titled = new Set<bigint>();
let instruction = DEFAULT_INSTRUCTION;
let correlation = 1n;

/** Keeps a model reply to at most four plain words. */
export function cleanTitle(raw: string): string {
  return raw.trim().replace(/^["'`.]+|["'`.]+$/g, '').split(/\s+/).filter(Boolean).slice(0, 4).join(' ');
}

/** The instruction of the first style in run order, or the default. */
export function instructionFrom(entriesJson: string[]): string {
  for (const text of entriesJson) {
    try {
      const entry = JSON.parse(text) as { instruction?: unknown };
      if (typeof entry.instruction === 'string') return entry.instruction;
    } catch {
      // A broken entry is skipped.
    }
  }
  return DEFAULT_INSTRUCTION;
}

function ask(text: string): bigint {
  return completeRequest({
    messages: [
      { role: 'system', content: instruction },
      { role: 'user', content: text.slice(0, 4000) },
    ],
    maxOutputTokens: 24,
    jsonSchema: undefined,
    purpose: 'pane-title',
  });
}

function reloadStyles(): void {
  try {
    instruction = instructionFrom(list(STYLES).map((entry) => entry.entryJson));
  } catch {
    instruction = DEFAULT_INSTRUCTION;
  }
}

function apply(node: bigint, title: string, submissions: Submission[]): void {
  submissions.push({
    intent: { tag: 'rename-node', val: { node, title, source: 'inferred', expectedRevision: undefined } },
    correlation: correlation++,
    dedupKey: `title:${node}:${title}`.slice(0, 128).replace(/[^A-Za-z0-9._:/-]/g, '-'),
  });
  try {
    publish(APPLIED, JSON.stringify({ node: Number(node), title }));
  } catch (error) {
    log('warn', `publish failed: ${String(error)}`);
  }
}

function finish(request: bigint, kind: Pending, submissions: Submission[]): void {
  if (kind.kind === 'pane') {
    const completion = takeCompletion(request);
    const proposed = completion ? cleanTitle(completion.text) : '';
    if (proposed === '') {
      titled.delete(kind.node);
      return;
    }
    try {
      const id = runHookRequest(BEFORE_APPLY, JSON.stringify({ node: Number(kind.node), proposed, source: 'transcript' }));
      pending.set(id, { kind: 'hook', node: kind.node, proposed });
    } catch {
      apply(kind.node, proposed, submissions);
    }
  } else if (kind.kind === 'service') {
    const completion = takeCompletion(request);
    const title = completion ? cleanTitle(completion.text) : '';
    reply(kind.call, { tag: 'ok', val: JSON.stringify({ title }) });
  } else {
    // No handler, or a refused chain, leaves the proposed title as it is.
    let adjusted = '';
    try {
      const text = takeReply(request);
      adjusted = text === undefined ? '' : cleanTitle((JSON.parse(text) as { proposed?: string }).proposed ?? '');
    } catch {
      adjusted = '';
    }
    apply(kind.node, adjusted || kind.proposed, submissions);
  }
}

export const { lifecycle, handler, serviceProvider } = definePlugin({
  side: 'server',
  activate() {
    reloadStyles();
  },
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'topic-published' && event.val.topic === 'ilium.extensions/changed') {
        reloadStyles();
      } else if (event.tag === 'input-submitted' && !titled.has(event.val.node)) {
        titled.add(event.val.node);
        try {
          pending.set(ask(event.val.text), { kind: 'pane', node: event.val.node });
        } catch (error) {
          titled.delete(event.val.node);
          log('warn', `no title: ${String((error as { code?: string }).code)}`);
        }
      } else if (event.tag === 'request-completed') {
        const entry = pending.get(event.val.request);
        if (entry === undefined) continue;
        pending.delete(event.val.request);
        finish(event.val.request, entry, submissions);
      }
    }
    return submissions;
  },
  handle(call: ServiceCall): ServiceReply {
    const params = JSON.parse(call.payloadJson) as { text?: string };
    if (typeof params.text !== 'string') return { tag: 'done', val: JSON.stringify({ title: '' }) };
    try {
      pending.set(ask(params.text), { kind: 'service', call: call.call });
      return { tag: 'deferred' };
    } catch {
      return { tag: 'done', val: JSON.stringify({ title: '' }) };
    }
  },
});
```

Points to notice:

- `clean_title` is pure. The model may answer with quotes, a full stop or a sentence. The function keeps at most four words, so the pane title never grows.
- The hook is run with `run_hook_request`, not `run_hook`, because the work happens inside `on_events`, which cannot wait. The completion arrives as a `request-completed` event, and `take_reply` returns the final payload. A chain with no handler gives no reply, and the plugin keeps its own proposal.
- The service reply is deferred. `suggest` returns at once with `Deferred`, and `reply` answers when the model has answered. A late `reply` after the caller's 30 second timeout gives `not-found`, and the plugin ignores the result. [Tutorial 22](/pages/tutorial-22-versioned-service-with-fallbacks/) covers deferred replies in detail.
- The dedup key joins the node and the title. A repeat of the same title for the same pane is a `duplicate`, not an error.
- `source: inferred` marks the rename. The host never lets an inferred title replace a title the user typed. The plugin does not need its own check.

### Step 3: what happens to the old plugin

When the user turns Titles Lite on, the built-in plugin is turned off. Only one of the two is enabled at a time. The change applies in this order.

1. The review dialog at install says what the replacement does. The user confirms.
2. The kernel turns `ilium.core.titles` off and turns `example.titles-lite` on.
3. The registry keeps every contribution that was made to `ilium.core.titles/title-styles` and `before-apply-title`. They apply to the replacement, because it provides the points of the same names.
4. The settings tab, the commands and the pane kinds of the built-in plugin disappear. The Titles settings tab is not in the rail while the replacement is on. Every command that `ilium.core.titles` contributes, for example its tree-tidying action, and the preview panel are gone too, unless the replacement contributes a command with the same id.
5. Stored data stays on disk. The replacement does not read it. It cannot see the titles history, the style choice or the prompt text of the built-in plugin.

Turning Titles Lite off, or uninstalling it, turns the built-in plugin back on. It finds its data as it left it. Titles that the replacement applied to panes stay on the panes. A title is a property of the pane, not of the plugin.

Some parts of the product cannot be replaced.

| Plugin | Replaceable | Reason |
|---|---|---|
| `ilium.core.titles` and the other feature plugins | yes | A feature may be swapped. |
| `ilium.core.sessions` | no | It owns saved sessions. A broken replacement could lose them. |
| `ilium.core.settings` | no | It hosts the Plugins screen. Without it the user could not turn a replacement off. |

A package that lists either of these in `replaces` fails with `manifest.replaces-not-replaceable`.

### Step 4: the install errors

The surface check is the main safety rule of replacement. Remove the service from the manifest, or leave out the topic, and run `ilium plugin build`:

<figure>
<pre class="term">
<span class="t-prompt">$</span> ilium plugin build
<span class="t-err">error manifest.replaces-surface-incomplete</span>
  replaces ilium.core.titles
  missing topics:title-applied
<span class="t-dim">1 error, 0 warnings</span>
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The error lists every missing item, so one run shows them all.</figcaption>
</figure>

### Step 5: grants, risk and consent

| Grant | Risk | Why |
|---|---|---|
| `inference` | normal | The titles come from the user's model. The user's AI admission settings apply to the purpose `pane-title`. |
| `agent.read-prompts` | normal | The plugin reads the prompt to name the pane. It does not store it. |
| `tree.read` | normal | The plugin reads the node it renames. |
| `tree.write` | normal | The plugin renames panes. |
| `services.provide:ilium.core.titles/suggest` | normal | The plugin answers one service. |
| `extensions.provide:` two points | normal | The plugin owns two points. |
| `topics.subscribe:ilium.extensions/changed` | normal | The plugin reloads its styles. |

- A replacement of a core plugin is a trust decision. The built-in plugin is reviewed with Ilium. Yours is reviewed by the user, once, in the dialog. A signed package from a publisher the user trusts is the right shape.

### Step 6: test it

The fake host described in [Testing](/pages/testing/) does not run the model. `FakeSession` runs the registry and a stub of the core plugin, so test the pure parts, the manifest and the service calls through the session. See [Test harness reference](/pages/reference-test-harness/).

**Rust**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_or_quoted_reply_is_shortened() {
        assert_eq!(clean_title("\"Fix the login redirect loop today\"."), "Fix the login redirect");
        assert_eq!(clean_title("  Refactor parser  "), "Refactor parser");
        assert_eq!(clean_title(""), "");
    }

    #[test]
    fn first_style_wins_and_broken_entries_are_skipped() {
        let entries = vec!["not json".to_string(), r#"{"id":"a","title":"A","instruction":"Use verbs."}"#.to_string()];
        assert_eq!(instruction_from(&entries), "Use verbs.");
        assert_eq!(instruction_from(&[]), DEFAULT_INSTRUCTION);
    }
}
```

**TypeScript**

```ts
// src/index.test.ts
import { expect, test } from 'bun:test';
import { cleanTitle, instructionFrom } from './index';

test('long or quoted reply is shortened', () => {
  expect(cleanTitle('"Fix the login redirect loop today".')).toBe('Fix the login redirect');
  expect(cleanTitle('  Refactor parser  ')).toBe('Refactor parser');
  expect(cleanTitle('')).toBe('');
});

test('first style wins and broken entries are skipped', () => {
  const entries = ['not json', '{"id":"a","title":"A","instruction":"Use verbs."}'];
  expect(instructionFrom(entries)).toBe('Use verbs.');
  expect(instructionFrom([])).toContain('four words');
});
```

Then check it in the app, with the steps from [Debugging](/pages/debugging/):

1. Pack and install as in [tutorial 9](/pages/tutorial-09-package-and-share/). Read the dialog and confirm.
2. Open Settings. The Titles tab must be gone from the rail.
3. Send a prompt in an agent pane. The pane gets a title of four words or fewer. A second prompt does not rename it.
4. Run `ilium plugin logs example.titles-lite --tail 5`. A failed model call appears as `no title: <code>`.
5. Turn Titles Lite off in Settings > Plugins. The Titles tab returns and the older settings are still there.

### What you see

At install the review dialog has a block for the replacement above the grants.

<figure>
<pre class="term">
╭──────────────────────────────────────────────╮
│ Review: example.titles-lite 0.1.0            │
│                                              │
│ Replaces Titles and organisation.            │
│ The built-in plugin is turned off while      │
│ this one is on.                              │
│                                              │
│ Gives up                                     │
│   Settings tab Titles                        │
│   Command Tidy the tree                      │
│   Preview panel                              │
│                                              │
│ Grants                                       │
│   [x] inference                              │
│   [x] agent.read-prompts                     │
│   [x] tree.write                             │
│                                              │
│   [replace]  [decline]                       │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The words in the first block are fixed by the product. The "Gives up" list names what disappears.</figcaption>
</figure>

On Settings > Plugins, the plugin shows its Surface, the four items that make it a valid replacement.

<figure>
<pre class="term">
╭──────────────────────────────────────────────╮
│ Titles Lite                                  │
│ Replaces ilium.core.titles                   │
│                                              │
│ Surface                                      │
│   [x] services   suggest                     │
│   [x] points     before-apply-title          │
│   [x] points     title-styles                │
│   [x] topics     title-applied               │
│                                              │
│   [x] Enabled      Built-in plugin is off    │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Turning the toggle off turns the built-in plugin back on.</figcaption>
</figure>

### Edge cases and errors: what can go wrong

| Code or symptom | Cause | Fix |
|---|---|---|
| `manifest.replaces-surface-incomplete` | A surface item is missing. The error lists them. | Provide each missing item. |
| `manifest.replaces-unknown` | The id is not an installed core plugin. | Check the id. |
| `manifest.replaces-not-replaceable` | The plugin is `ilium.core.sessions` or `ilium.core.settings`. | Extend it instead. |
| `manifest.replaces-cycle` | Two packages replace each other. | Replace one plugin with one package. |
| `timed-out` on `suggest` | The model took over 30 s. | The caller shows a plain fallback. |
| `not-found` on `reply` | The call already ended. | Ignore. The plugin drops the result. |
| `refused(conflict)` on rename | The user typed the title by hand. | None. This is the rule. |
| `extension.reply-not-allowed` | A handler used `refuse` on this transform point. | The handler must use `replace` or `pass`. |
| Title never appears | No model is set up, or the pane is already in `titled`. | Set up a provider. Check the log. |

### See also

- [Extending core plugins](/pages/extending-core-plugins/) for the five ways and the rules of replacement.
- [Reference: core extension points](/pages/reference-core-extension-points/) for the surface of each core plugin.
- [Core plugin: titles](/pages/core-plugin-titles/) and [Inference providers](/pages/core-plugin-inference/).
- [Extension points](/pages/api-extension-points/), [services](/pages/services/) and [inference](/pages/api-inference/).
- [Tutorial 18](/pages/tutorial-18-add-an-extension-point/), [tutorial 22](/pages/tutorial-22-versioned-service-with-fallbacks/), [Testing](/pages/testing/) and [Debugging](/pages/debugging/).

API checked against `ilium:plugin@0.1.0`.

### Security

A replacement inherits no grants and T3 cannot replace ([SEC-300], [SEC-301]). See [Extension security](/pages/reference-extension-security/).
