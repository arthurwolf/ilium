# Recap

## Recap

A recap is a few factual sentences about what one pane is doing, written once in a short form (at most 40 words) and once in a long form (at most 250 words). `ilium recap` prints them for every selected agent and shell pane, as JSON lines or as Markdown. The plugin id is `ilium.core.recap` and it runs on both sides. The server side owns the command, reads the evidence, calls the model and writes the records. The client side holds the answer to one question the server cannot know: whether onboarding is finished.

The recap text comes from the AI provider the user configured under Inference and privacy. One model call covers both lengths of one pane. When automatic AI is off or onboarding is not finished, nothing is read and nothing is sent: each selected pane gets a `denied` record.

### What it covers

- The `ilium recap [selection]` command, with the same selection options as `ilium panes` and `ilium broadcast`, described in the CLI reference.
- Evidence for agent panes from the transcript of the pane's own verified session, never from the screen.
- Evidence for shell panes from the current screen, capped at 64 KiB.
- The six states of a record: `summarized`, `idle`, `denied`, `unavailable`, `stale` and `failed`.
- Goal lines read from the transcript, not written by the model.
- Redaction of secrets before any text leaves Ilium, and the rule that a pointer survives only when its path appears in the evidence, lies under the pane's project or folder, exists now and is not denied.

### See it

There is no recording of this feature yet. The renders below are illustrative.

### What the user sees

<figure>
<pre class="term"><span class="t-dim">$</span> ilium recap --kind agent --length short
{"type":"recap","pane_id":"12","state":"summarized",
 "goals":["Goal active: Finish the parser refactor"],
 "short":{"text":"The parser refactor is in progress. …"}, …}
{"type":"recap","pane_id":"14","state":"unavailable", …}
{"type":"warning","pane_id":"15","state":"failed",
 "message":"provider timed out"}
{"type":"summary","panes":3,"summarized":1,"unavailable":1,
 "failed":1,"cancelled":false}
</pre>
<figcaption>Rendered at 80 columns or fewer. Illustrative: fields are shortened with <code>…</code>. One <code>recap</code> record per pane, then a <code>summary</code>. The exit status is 0 because the run finished.</figcaption>
</figure>

<figure>
<pre class="term">╭─ Recap · Extensions ─────────────────────────╮
│<span class="t-hi">fact-sources</span> (collect)                        │
│  [x] Branch ticket                           │
│  [ ] Build status                            │
│                                              │
│<span class="t-hi">before-recap</span> (transform)                      │
│  [x] Redact customer names                   │
│  stopped after 3 timeouts       [Reset]      │
│  [x] Add ticket id                           │
│  <span class="t-dim">Run order: lower priority number first.</span>     │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The Extensions tab of the Recap plugin lists the contributors of each point in run order, each with a toggle. A contributor that breached its deadline three times in a row shows the one stopped label, `stopped after 3 timeouts [Reset]`, as defined in <a href="/pages/api-extension-points/">Extension points</a>.</figcaption>
</figure>

<figure>
<pre class="term">╭─ Recap ──────────────────────────────────────╮
│ <span class="t-warn">! AI is off for recaps</span>                       │
│ <span class="t-dim">  Every selected pane gets a denied record.</span>  │
│ <span class="t-dim">  Turn on automatic AI in Settings.</span>          │
│ Last run ……………………………………… 2026-10-12 09:14:03 │
│ Panes recapped ………………………………………………………… 3 of 3 │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A <code>notice</code> and two <code>leader-row</code> rows on the plugin panel when the consent gate is closed.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

The contract version is 0.1. The plugin declares two extension points, provides one service and one command line command, and is replaceable. A replacement must provide every item of `surface`.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.recap",
  "name": "Recap",
  "version": "0.1.0",
  "publisher": "Ilium",
  "limits_tier": "first-party",
  "replaceable": true,
  "surface": [
    "services:recap", "points:fact-sources", "points:before-recap", "cli:recap"
  ],
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": ["job-runner", "service-provider"]
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": []
  },
  "capabilities": {
    "server": [
      "cli.contribute", "panes.read", "agent.read", "agent.transcript",
      "terminal.read-screen", "inference", "jobs.run",
      "settings.read-kernel:inference",
      "services.provide:ilium.core.recap/recap",
      "extensions.provide:ilium.core.recap/fact-sources",
      "extensions.provide:ilium.core.recap/before-recap"
    ],
    "client": ["help.read"]
  },
  "contributes": {
    "cli": {
      "name": "recap",
      "summary": "Print short and long recaps of the selected panes",
      "output": "jsonl",
      "stdin": "none",
      "requires_server": true,
      "argument_groups": ["pane-selector"]
    },
    "jobs": [{ "kind": "recap-pane", "deadline_ms": 5000, "memory_mib": 64 }],
    "client_requests": [{ "kind": "onboarding-state" }],
    "help": [{ "id": "recap-privacy", "file": "help/recap-privacy.md", "title": "What a recap sends" }],
    "services": {
      "ilium.core.recap/recap": {
        "get": {
          "params": { "type": "object", "required": ["panes"],
            "properties": { "panes": { "type": "array", "items": { "type": "integer" } },
                            "length": { "type": "string", "enum": ["short", "long", "both"] } } },
          "result": { "type": "object", "properties": { "recaps": { "type": "array" } },
            "required": ["recaps"] }
        }
      }
    },
    "extension_points": [
      { "id": "fact-sources", "kind": "collect", "title": "Fact sources",
        "entry_schema": { "type": "object", "required": ["id", "title", "service"],
          "properties": { "id": { "type": "string" }, "title": { "type": "string" },
                          "service": { "type": "string" } } },
        "max_entries": 16, "dynamic": false, "version": "0.1.0" },
      { "id": "before-recap", "kind": "hook", "mode": "transform", "title": "Before recap",
        "payload_schema": { "type": "object", "properties": {
          "pane": { "type": "integer" }, "kind": { "type": "string", "enum": ["short", "long"] },
          "text": { "type": "string", "maxLength": 65536 } } },
        "reply_schema": { "type": "object", "properties": { "text": { "type": "string", "maxLength": 65536 } } },
        "risk": "standard", "deadline_ms": 20, "max_handlers": 8, "version": "0.1.0" }
    ]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.recap",
  "name": "Recap",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "A recap is a few factual sentences about what one pane is doing, written once in a short form (at most 40 words) and once in a long form (at most 250 words).",
  "license": "MIT",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": [
      "job-runner",
      "service-provider"
    ]
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": []
  },
  "capabilities": {
    "server": [
      "cli.contribute",
      "panes.read",
      "agent.read",
      "agent.transcript",
      "terminal.read-screen",
      "inference",
      "jobs.run",
      "settings.read-kernel:inference",
      "services.provide:ilium.core.recap/recap",
      "extensions.provide:ilium.core.recap/fact-sources",
      "extensions.provide:ilium.core.recap/before-recap"
    ],
    "client": [
      "help.read"
    ]
  },
  "activation": [
    "cli:recap",
    "command:recap"
  ],
  "replaceable": true,
  "surface": [
    "services:recap",
    "points:fact-sources",
    "points:before-recap",
    "cli:recap"
  ],
  "contributes": {
    "cli": {
      "name": "recap",
      "summary": "Print short and long recaps of the selected panes",
      "output": "jsonl",
      "stdin": "none",
      "requires_server": true,
      "argument_groups": [
        "pane-selector"
      ]
    },
    "jobs": [
      {
        "kind": "recap-pane",
        "deadline_ms": 5000,
        "memory_mib": 64
      }
    ],
    "client_requests": [
      {
        "kind": "onboarding-state"
      }
    ],
    "help": [
      {
        "id": "recap-privacy",
        "file": "help/recap-privacy.md",
        "title": "What a recap sends"
      }
    ],
    "services": {
      "ilium.core.recap/recap": {
        "get": {
          "params": {
            "type": "object",
            "required": [
              "panes"
            ],
            "properties": {
              "panes": {
                "type": "array",
                "items": {
                  "type": "integer"
                }
              },
              "length": {
                "type": "string",
                "enum": [
                  "short",
                  "long",
                  "both"
                ]
              }
            }
          },
          "result": {
            "type": "object",
            "properties": {
              "recaps": {
                "type": "array"
              }
            },
            "required": [
              "recaps"
            ]
          }
        }
      }
    },
    "extension_points": [
      {
        "id": "fact-sources",
        "kind": "collect",
        "title": "Fact sources",
        "entry_schema": {
          "type": "object",
          "required": [
            "id",
            "title",
            "service"
          ],
          "properties": {
            "id": {
              "type": "string"
            },
            "title": {
              "type": "string"
            },
            "service": {
              "type": "string"
            }
          }
        },
        "max_entries": 16,
        "dynamic": false,
        "version": "0.1.0"
      },
      {
        "id": "before-recap",
        "kind": "hook",
        "mode": "transform",
        "title": "Before recap",
        "payload_schema": {
          "type": "object",
          "properties": {
            "pane": {
              "type": "integer"
            },
            "kind": {
              "type": "string",
              "enum": [
                "short",
                "long"
              ]
            },
            "text": {
              "type": "string",
              "maxLength": 65536
            }
          }
        },
        "reply_schema": {
          "type": "object",
          "properties": {
            "text": {
              "type": "string",
              "maxLength": 65536
            }
          }
        },
        "risk": "standard",
        "deadline_ms": 20,
        "max_handlers": 8,
        "version": "0.1.0"
      }
    ]
  },
  "limits_tier": "first-party"
}
```

### Samples

#### The `recap` command with JSON lines

The command is declared with `output: "jsonl"`, `requires_server: true` and the `pane-selector` argument group, so the host resolves `--pane` and `--all-panes`. The richer selection options (`--match`, `--project`, `--state` and the rest) arrive in `argv` and the plugin resolves them against the pane list. One `recap` record is written per pane, then one `summary`. The exit code is 0 when the run finished, even if some panes failed. See [cli](/pages/api-cli/) and [capability-cli-contribute](/pages/capability-cli-contribute/).

**Rust**

```rust
use ilium_plugin_sdk::host::cli;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::CliInvokedEvent;

#[derive(Default)]
struct Run { invocation: u64, length: String, waiting: usize, summarized: u32, failed: u32 }

fn start(&mut self, event: CliInvokedEvent) -> Result<(), PluginError> {
    let length = flag_value(&event.argv, "--length").unwrap_or_else(|| "both".into());
    if !["short", "long", "both"].contains(&length.as_str()) {
        cli::write_stderr(event.invocation, "recap: --length must be short, long or both")?;
        cli::finish(event.invocation, 2);
        return Ok(());
    }
    let panes = self.select_panes(&event)?; // agent and shell panes, self skipped
    for pane in panes.iter().take(MAX_PANES) {
        self.start_job(event.invocation, *pane, &length)?;
    }
    if panes.len() > MAX_PANES {
        let warning = r#"{"type":"warning","message":"extra panes skipped"}"#;
        cli::write_line(event.invocation, warning)?;
    }
    self.runs.insert(event.invocation, Run { invocation: event.invocation, length, waiting: panes.len().min(MAX_PANES), ..Run::default() });
    Ok(())
}
```

**TypeScript**

```ts
import * as cli from '@ilium/plugin-sdk/host/cli';
import type { CliInvokedEvent } from '@ilium/plugin-sdk/types';

export function start(event: CliInvokedEvent): void {
  const length = flagValue(event.argv, '--length') ?? 'both';
  if (!['short', 'long', 'both'].includes(length)) {
    cli.writeStderr(event.invocation, 'recap: --length must be short, long or both');
    cli.finish(event.invocation, 2);
    return;
  }
  const panes = selectPanes(event); // agent and shell panes, self skipped
  for (const pane of panes.slice(0, MAX_PANES)) startJob(event.invocation, pane, length);
  if (panes.length > MAX_PANES) {
    cli.writeLine(event.invocation, JSON.stringify({ type: 'warning', message: 'extra panes skipped' }));
  }
  runs.set(event.invocation, { length, waiting: Math.min(panes.length, MAX_PANES), summarized: 0, failed: 0 });
}
```

#### The consent gate

Before any pane is read, the plugin asks two questions. Is automatic AI on? That is a read-only look at a kernel settings tab. Is onboarding finished? That state lives in the client, so the server side sends a client request and the client side answers. A closed gate turns every pane into a `denied` record. See [kernel settings](/pages/api-kernel-settings-and-keymap/), [capability settings.read-kernel](/pages/capability-settings-read-kernel/) and [client requests](/pages/api-client-requests/).

**Rust**

```rust
use ilium_plugin_sdk::host::{client_requests, kernel_settings};

fn ai_is_on() -> bool {
    kernel_settings::read("inference")
        .ok()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .and_then(|value| value["automatic_ai"].as_bool())
        .unwrap_or(false)
}

fn ask_onboarding(&mut self, client: ClientId) -> Result<(), PluginError> {
    let request = client_requests::request(client, "onboarding-state", "{}", 2_000)?;
    self.gate_requests.insert(request);
    Ok(())
}

// Client side, on Event::ClientRequest(call) with kind "onboarding-state":
//   client_requests::respond(call.call, Ok(format!(r#"{{"finished":{}}}"#, self.onboarding_done)));
```

**TypeScript**

```ts
import * as clientRequests from '@ilium/plugin-sdk/host/client-requests';
import { read } from '@ilium/plugin-sdk/host/kernel-settings';

export function aiIsOn(): boolean {
  try {
    return JSON.parse(read('inference')).automatic_ai === true;
  } catch {
    return false; // unreadable means closed
  }
}

export function askOnboarding(client: bigint): void {
  gateRequests.add(clientRequests.request(client, 'onboarding-state', '{}', 2_000));
}

// Client side, on event.tag === 'client-request' with kind 'onboarding-state':
//   clientRequests.respond(event.val.call, { tag: 'ok', val: JSON.stringify({ finished: onboardingDone }) });
```

#### Evidence from an agent transcript

For an agent pane the plugin reads only the transcript of the pane's own session. If the host has no verified session for the pane, the record is `unavailable` and the plugin never falls back to the screen. The read is bounded, and secrets are replaced with `[redacted]` before the text goes to a job. See [agent transcript](/pages/primitives-agent-transcript/), [agent.transcript](/pages/capability-agent-transcript/) and [agent](/pages/primitives-agent/).

**Rust**

```rust
use ilium_plugin_sdk::primitives::agent::{self, TranscriptQuery};

fn agent_evidence(&mut self, pane: NodeId) -> Evidence {
    if agent::session(pane).is_none() {
        return Evidence::Unavailable; // no verified session id for this pane
    }
    match agent::transcript(TranscriptQuery { pane, last_turns: 40, max_bytes: 512 * 1024 }) {
        Ok(request) => {
            self.transcript_requests.insert(request, pane);
            Evidence::Pending
        }
        Err(_) => Evidence::Unavailable,
    }
}

fn redact(text: &str) -> String {
    SECRET_KEY.replace_all(text, "$1[redacted]").into_owned()
}
```

**TypeScript**

```ts
import { session, transcript } from '@ilium/plugin-sdk/primitives/agent';

export function agentEvidence(pane: bigint): 'unavailable' | 'pending' {
  if (session(pane) === undefined) return 'unavailable'; // no verified session id
  try {
    transcriptRequests.set(transcript({ pane, lastTurns: 40, maxBytes: 512 * 1024 }), pane);
    return 'pending';
  } catch {
    return 'unavailable';
  }
}

export const redact = (text: string): string => text.replace(SECRET_KEY, '$1[redacted]');
```

#### Evidence from a shell screen

A shell pane has no transcript. The plugin reads the visible screen only, at most 40 rows, and stops at 64 KiB. An empty screen is `idle`, not `failed`. See [terminal.read-screen](/pages/capability-terminal-read-screen/) and [terminal reads](/pages/primitives-terminal-reads/).

**Rust**

```rust
use ilium_plugin_sdk::primitives::terminal::{self, ScreenPart};

const SCREEN_BYTES: usize = 64 * 1024;

fn shell_evidence(pane: NodeId) -> Evidence {
    let Some(screen) = terminal::read_screen(pane, ScreenPart::Visible, 40) else {
        return Evidence::Unavailable; // not a terminal, or the pane was replaced
    };
    let mut text = screen.rows.join("\n");
    text.truncate(text.floor_char_boundary(SCREEN_BYTES));
    if text.trim().is_empty() { Evidence::Idle } else { Evidence::Text(redact(&text)) }
}
```

**TypeScript**

```ts
import { readScreen } from '@ilium/plugin-sdk/primitives/terminal';

const SCREEN_BYTES = 64 * 1024;

export function shellEvidence(pane: bigint) {
  const screen = readScreen(pane, 'visible', 40);
  if (screen === undefined) return { tag: 'unavailable' } as const;
  const text = new TextDecoder().decode(new TextEncoder().encode(screen.rows.join('\n')).slice(0, SCREEN_BYTES));
  return text.trim() === '' ? ({ tag: 'idle' } as const) : ({ tag: 'text', text: redact(text) } as const);
}
```

#### One job per pane

The model call can take seconds, far over the 20 ms event deadline, so each pane is one job of kind `recap-pane`. The job asks for both lengths in one call, reports its progress and stops when the run is cancelled (Ctrl-C or the timeout). The run limit `--max-concurrency` maps to the plugin's `concurrent_jobs`; extra jobs wait in the queue. See [jobs](/pages/api-jobs/), [jobs.run](/pages/capability-jobs-run/) and [inference](/pages/api-inference/).

**Rust**

```rust
use ilium_plugin_sdk::host::{inference, jobs};
use ilium_plugin_sdk::exports::JobRunner;
use ilium_plugin_sdk::host::inference::{ChatMessage, CompletionRequest};

impl JobRunner for Recap {
    fn run(&mut self, kind: String, input: Vec<u8>) -> Result<Vec<u8>, PluginError> {
        let job: RecapJob = serde_json::from_slice(&input).map_err(PluginError::invalid_argument)?;
        jobs::report_progress(10.0);
        let request = inference::complete_request(CompletionRequest {
            messages: vec![
                ChatMessage { role: "system".into(), content: RECAP_PROMPT.into() },
                ChatMessage { role: "user".into(), content: job.evidence },
            ],
            max_output_tokens: None,
            json_schema: Some(RECAP_SCHEMA.into()),
            purpose: "recap".into(),
        })?;
        jobs::wait_request(request, 55_000)?;
        if jobs::is_cancelled() { return Err(PluginError::cancelled()); }
        let completion = inference::take_completion(request).ok_or_else(PluginError::internal)?;
        jobs::report_progress(100.0);
        Ok(completion.text.into_bytes())
    }
}
```

**TypeScript**

```ts
import { completeRequest, takeCompletion } from '@ilium/plugin-sdk/host/inference';
import { isCancelled, reportProgress, waitRequest } from '@ilium/plugin-sdk/host/jobs';

export function run(kind: string, input: Uint8Array): Uint8Array {
  const job = JSON.parse(new TextDecoder().decode(input)) as RecapJob;
  reportProgress(10);
  const request = completeRequest({
    messages: [
      { role: 'system', content: RECAP_PROMPT },
      { role: 'user', content: job.evidence },
    ],
    maxOutputTokens: undefined,
    jsonSchema: RECAP_SCHEMA,
    purpose: 'recap',
  });
  waitRequest(request, 55_000);
  if (isCancelled()) throw { code: 'cancelled', message: 'run cancelled' };
  const completion = takeCompletion(request);
  if (completion === undefined) throw { code: 'internal', message: 'no completion' };
  reportProgress(100);
  return new TextEncoder().encode(completion.text);
}
```

#### Owner side: collect the fact sources

`fact-sources` is a collect point. Before it builds the evidence, the plugin lists the entries that other plugins contributed, in run order, and asks each entry's service for facts about the pane. Each fact is added to the evidence block. When contributions change, the kernel publishes the topic `ilium.extensions/changed` to the owner only, and the plugin lists again. See [events](/pages/api-events/), [services](/pages/api-services/) and [messages and topics](/pages/api-messages-and-topics/).

**Rust**

```rust
use ilium_plugin_sdk::host::{extensions, services};

const FACT_SOURCES: &str = "ilium.core.recap/fact-sources";

fn reload_sources(&mut self) {
    self.sources = extensions::list(FACT_SOURCES).unwrap_or_default();
}

async fn gather_facts(&self, pane: NodeId) -> Vec<String> {
    let mut facts = Vec::new();
    for source in &self.sources {
        let entry: FactSource = match serde_json::from_str(&source.entry_json) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let payload = format!(r#"{{"pane":{}}}"#, pane.0);
        if let Ok(reply) = services::call(&entry.service, "facts", &payload).await {
            facts.push(format!("{}: {}", entry.title, redact(&reply)));
        }
    }
    facts
}

// In on_events: Event::TopicPublished(t) if t.topic == "ilium.extensions/changed" => self.reload_sources(),
```

**TypeScript**

```ts
import * as extensions from '@ilium/plugin-sdk/host/extensions';
import { call } from '@ilium/plugin-sdk/host/services';

const FACT_SOURCES = 'ilium.core.recap/fact-sources';
let sources: extensions.ExtensionEntry[] = [];

export const reloadSources = (): void => {
  try { sources = extensions.list(FACT_SOURCES); } catch { sources = []; }
};

export async function gatherFacts(pane: bigint): Promise<string[]> {
  const facts: string[] = [];
  for (const source of sources) {
    const entry = JSON.parse(source.entryJson) as { title: string; service: string };
    try {
      const reply = await call(entry.service, 'facts', JSON.stringify({ pane: Number(pane) }));
      facts.push(`${entry.title}: ${redact(reply)}`);
    } catch { /* a failing source never stops the recap */ }
  }
  return facts;
}

// In onEvents: if (event.tag === 'topic-published' && event.val.topic === 'ilium.extensions/changed') reloadSources();
```

#### Owner side: run the `before-recap` hook

`before-recap` is a transform hook. The plugin runs it once per length, with the pane, the kind and the text. `run-hook` returns a request id. When the matching `request-completed` arrives with outcome `ok`, `take-reply` gives the final text, with every handler's change applied in priority order. No reply means nobody changed the text. A handler that fails counts as `pass`, so a broken contributor can never block a recap. See [events](/pages/api-events/) and [requests and outcomes](/pages/requests-and-outcomes/).

**Rust**

```rust
use ilium_plugin_sdk::host::extensions;
use ilium_plugin_sdk::types::requests::{Outcome, RequestResult};

const BEFORE_RECAP: &str = "ilium.core.recap/before-recap";

fn start_hook(&mut self, pane: NodeId, kind: &str, text: &str) {
    let payload = serde_json::json!({ "pane": pane.0, "kind": kind, "text": text }).to_string();
    match extensions::run_hook(BEFORE_RECAP, &payload) {
        Ok(request) => { self.hooks.insert(request, (pane, kind.to_string(), text.to_string())); }
        Err(_) => self.finish_text(pane, kind, text.to_string()),
    }
}

fn on_hook_done(&mut self, done: RequestResult) {
    let Some((pane, kind, original)) = self.hooks.remove(&done.request) else { return };
    let text = match done.outcome {
        Outcome::Ok => extensions::take_reply(done.request)
            .and_then(|json| serde_json::from_str::<HookText>(&json).ok())
            .map_or(original, |reply| reply.text),
        _ => original, // the point is a transform: any other outcome keeps the text
    };
    self.finish_text(pane, &kind, text);
}
```

**TypeScript**

```ts
import * as extensions from '@ilium/plugin-sdk/host/extensions';

const BEFORE_RECAP = 'ilium.core.recap/before-recap';

export function startHook(pane: bigint, kind: string, text: string): void {
  try {
    const request = extensions.runHook(BEFORE_RECAP, JSON.stringify({ pane: Number(pane), kind, text }));
    hooks.set(request, { pane, kind, original: text });
  } catch {
    finishText(pane, kind, text);
  }
}

export function onHookDone(request: bigint, outcome: { tag: string }): void {
  const pending = hooks.get(request);
  if (pending === undefined) return;
  hooks.delete(request);
  let text = pending.original;
  if (outcome.tag === 'ok') {
    const reply = extensions.takeReply(request);
    if (reply !== undefined) text = (JSON.parse(reply) as { text: string }).text;
  }
  finishText(pending.pane, pending.kind, text);
}
```

#### Keep only real pointers

A pointer is a path the recap names. The plugin keeps it only when the path appears in the evidence, lies under the pane's project, exists now and is not in a denied area. It counts the rest in `rejected_pointers`. The existence check uses the project file scope. See [fs](/pages/api-fs/), [fs.read](/pages/capability-fs-read/) and [file access](/pages/file-access/).

**Rust**

```rust
use ilium_plugin_sdk::host::fs;
use ilium_plugin_sdk::types::paths::FsPath;

async fn keep_pointers(evidence: &str, project_relative: Vec<(String, String)>) -> (Vec<(String, String)>, u32) {
    let mut kept = Vec::new();
    let mut rejected = 0;
    for (path, note) in project_relative {
        let named = evidence.contains(&path);
        let allowed = !DENIED_PREFIXES.iter().any(|prefix| path.starts_with(prefix));
        let exists = fs::read(&FsPath { scope: "project".into(), path: path.clone() }).await.is_ok();
        if named && allowed && exists { kept.push((path, note)); } else { rejected += 1; }
    }
    (kept, rejected)
}
```

**TypeScript**

```ts
import { read } from '@ilium/plugin-sdk/host/fs';

export async function keepPointers(evidence: string, pointers: { path: string; note: string }[]) {
  const kept: { path: string; note: string }[] = [];
  let rejected = 0;
  for (const pointer of pointers) {
    const named = evidence.includes(pointer.path);
    const allowed = !DENIED_PREFIXES.some((prefix) => pointer.path.startsWith(prefix));
    const exists = await read({ scope: 'project', path: pointer.path }).then(() => true, () => false);
    if (named && allowed && exists) kept.push(pointer); else rejected += 1;
  }
  return { kept, rejected };
}
```


### Extension points

Other plugins change what `ilium.core.recap` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.recap/<point>`). Contributors need `extensions.contribute:ilium.core.recap/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `fact-sources` | collect | Add facts to a recap, such as the ticket of a branch. | entry: id, title, service (returns facts for a pane) | standard |
| `before-recap` | hook (transform) | Change the recap text before it is shown or returned. | pane, kind (short, long), text; reply: text | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.recap/recap`: get: short or long recap of panes

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.recap"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replace)).

#### Example: Adds the ticket of the pane branch to every long recap

A package adds this entry in its manifest. It needs no code.

```json
{
  "api": "ilium:plugin@0.1.0",
  "capabilities": { "server": ["extensions.contribute:ilium.core.recap/fact-sources"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.recap/fact-sources",
        "id": "branch-ticket",
        "priority": 50,
        "entry": {
          "title": "Branch ticket",
          "service": "example.tickets/for-pane"
        }
      }
    ]
  }
}
```

### ABI used

- [cli](/pages/api-cli/) and [capability-cli-contribute](/pages/capability-cli-contribute/)
- [jobs](/pages/api-jobs/) and [jobs.run](/pages/capability-jobs-run/)
- [inference](/pages/api-inference/)
- [Agent transcript](/pages/primitives-agent-transcript/) and [agent.transcript](/pages/capability-agent-transcript/)
- [terminal.read-screen](/pages/capability-terminal-read-screen/)
- [Kernel settings](/pages/api-kernel-settings-and-keymap/) and [settings.read-kernel](/pages/capability-settings-read-kernel/)
- [Client requests](/pages/api-client-requests/)
- [Services](/pages/api-services/) and [messages and topics](/pages/api-messages-and-topics/)
- [fs](/pages/api-fs/) and [file access](/pages/file-access/)
- [Requests and outcomes](/pages/requests-and-outcomes/) and [events](/pages/api-events/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Inference](/pages/core-plugin-inference/)
- [Compaction](/pages/core-plugin-compaction/)
