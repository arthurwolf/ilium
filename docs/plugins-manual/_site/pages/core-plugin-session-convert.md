# Session conversion

## Session conversion

Session conversion lets a conversation continue under the other agent. A Claude Code conversation becomes a Codex conversation, or a Codex conversation becomes a Claude Code one, without starting over. The plugin id is `ilium.core.session-convert` and it runs on both sides. The client side adds the context menu entries and draws the progress dialog. The server side checks the pane, freezes it, runs the conversion as a job, and then replaces the frozen pane with a new pane that resumes the converted session.

The original transcript is read and never changed. Antigravity sessions cannot be converted. The conversion engine itself is the Rust crate `ilium-session-convert`, which reports steps, log lines and progress. The plugin turns those reports into the dialog.

### What it covers

- The entries **Convert to Codex** (for a Claude Code pane) and **Convert to Claude Code** (for a Codex pane). The entry for the other agent is always listed in the menu of an agent pane. It is enabled only when the pane has a verified session, no other conversion is running, and the pane is not a worktree pane; otherwise it stays visible but disabled and shows the reason.
- The progress dialog with one row per step, a progress bar and a log.
- Claude Code to Codex: locate the transcript, inspect it, start a private `codex app-server`, import the session with Codex's own importer, verify the Codex rollout, stop the app-server.
- Codex to Claude Code: locate the rollout, parse it, convert it to Claude Code lines, write the Claude Code transcript, verify it, register it with Claude Code. User prompts, assistant text, and shell or tool calls with results are carried over. Reasoning and token events are dropped.
- Cancel with Esc, and the recovery choice after a failure: Enter resumes the original session, Esc closes the dialog.

### See it

<figure><img src="/assets/demos/26-convert-session.gif" alt="A Claude Code session converts to Codex, which still remembers the secret word, then converts back to Claude" loading="lazy"><figcaption>A session is converted from Claude to Codex and back while keeping its context.</figcaption></figure>

### What the user sees

<figure>
<pre class="term">╭─ Pane ───────────────────────────────────────╮
│ Rename                                       │
│ Copy screen to pane…                         │
│ <span class="t-hi">Convert to Codex</span>                             │
│ <span class="t-dim">Convert to Claude Code  (Codex pane only)</span>    │
│ Close                                        │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The right-click menu of a Claude Code pane. Only the entry that fits the agent is enabled, and a disabled entry shows its reason.</figcaption>
</figure>

<figure>
<pre class="term">╭─ Convert Claude Code to Codex ───────────────╮
│ <span class="t-ok">✓</span> Locate the Claude Code transcript          │
│ <span class="t-ok">✓</span> Inspect the transcript                     │
│ <span class="t-ok">✓</span> Start a private codex app-server           │
│ <span class="t-hi">▶ Import the session</span>                         │
│ <span class="t-dim">○ Verify the Codex rollout</span>                   │
│ <span class="t-dim">○ Stop the app-server</span>                        │
│                                              │
│ <span class="t-hi">████████████</span><span class="t-dim">░░░░░░░░░░░░</span> 51%                 │
│ <span class="t-dim">imported 186 of 364 items</span>                    │
│ <span class="t-dim">Esc cancels</span>                                  │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The progress dialog: a step list, a <code>progress-row</code> and a log line inside a <code>dialog</code>.</figcaption>
</figure>

<figure>
<pre class="term">╭─ Convert Codex to Claude Code ───────────────╮
│ <span class="t-err">✕ Conversion failed</span>                          │
│ <span class="t-dim">  The Claude Code transcript did not verify.</span> │
│ <span class="t-dim">  The original session is untouched.</span>         │
│                                              │
│ Carried over ……………………………………………………… 212 items │
│ Left out …………………………………………………………………… 57 items │
│                                              │
│ [ <span class="t-hi">Resume original</span> ]   [ Close ]              │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. After a failure that came after the agent was stopped, Enter resumes the original session and Esc closes the dialog.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

The contract version is 0.1. The plugin is replaceable. A replacement must provide the command, the service and both points.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.session-convert",
  "name": "Session conversion",
  "version": "0.1.0",
  "publisher": "Ilium",
  "limits_tier": "first-party",
  "replaceable": true,
  "surface": [
    "services:convert", "points:formats", "points:after-convert", "commands:convert"
  ],
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": ["job-runner", "service-provider"]
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": ["views", "menu-provider"]
  },
  "unstable": ["plugin-services"],
  "capabilities": {
    "server": [
      "panes.read", "panes.lifecycle", "panes.create", "tree.write", "agent.read",
      "fs.read:agent-sessions:claude", "fs.read:agent-sessions:codex",
      "fs.write:agent-config:claude",
      "process.run:codex", "process.long-lived", "jobs.run", "notify.toast",
      "services.provide:ilium.core.session-convert/convert",
      "extensions.provide:ilium.core.session-convert/formats",
      "extensions.provide:ilium.core.session-convert/after-convert"
    ],
    "client": ["agent.read", "ui.panels"]
  },
  "contributes": {
    "commands": [{ "id": "convert", "title": "Convert session", "category": "Agents" }],
    "context_menu": [{ "id": "pane-convert", "applies_to": ["pane"], "dynamic": true, "command": "convert" }],
    "panels": [{ "id": "progress", "title": "Convert session", "modal": true }],
    "jobs": [{ "kind": "codex-to-claude", "deadline_ms": 60000, "memory_mib": 256 }],
    "services": {
      "ilium.core.session-convert/convert": {
        "start": {
          "params": { "type": "object", "required": ["pane", "target"],
            "properties": { "pane": { "type": "integer" },
                            "target": { "type": "string", "enum": ["codex", "claude"] } } },
          "result": { "type": "object", "properties": { "conversion": { "type": "string" } },
            "required": ["conversion"] }
        }
      }
    },
    "extension_points": [
      { "id": "formats", "kind": "collect", "title": "Transcript formats",
        "entry_schema": { "type": "object", "required": ["id", "title", "direction", "service"],
          "properties": { "id": { "type": "string" }, "title": { "type": "string" },
                          "direction": { "type": "string", "enum": ["read", "write", "both"] },
                          "service": { "type": "string" } } },
        "max_entries": 8, "dynamic": false, "version": "0.1.0" },
      { "id": "after-convert", "kind": "hook", "mode": "observe", "title": "After conversion",
        "payload_schema": { "type": "object", "properties": {
          "from": { "type": "string" }, "to": { "type": "string" },
          "messages": { "type": "integer" }, "path": { "type": "string" } } },
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
  "id": "ilium.core.session-convert",
  "name": "Session convert",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "Session conversion lets a conversation continue under the other agent.",
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
    "exports": [
      "views",
      "menu-provider"
    ]
  },
  "unstable": ["plugin-services"],
  "capabilities": {
    "server": [
      "panes.read",
      "panes.lifecycle",
      "panes.create",
      "tree.write",
      "agent.read",
      "fs.read:agent-sessions:claude",
      "fs.read:agent-sessions:codex",
      "fs.write:agent-config:claude",
      "process.run:codex",
      "process.long-lived",
      "jobs.run",
      "notify.toast",
      "services.provide:ilium.core.session-convert/convert",
      "extensions.provide:ilium.core.session-convert/formats",
      "extensions.provide:ilium.core.session-convert/after-convert"
    ],
    "client": [
      "agent.read",
      "ui.panels"
    ]
  },
  "activation": [
    "command:convert",
    "service:convert",
    "view:session-convert.progress"
  ],
  "replaceable": true,
  "surface": [
    "services:convert",
    "points:formats",
    "points:after-convert",
    "commands:convert"
  ],
  "contributes": {
    "commands": [
      {
        "id": "convert",
        "title": "Convert session",
        "category": "Agents"
      }
    ],
    "context_menu": [
      {
        "id": "pane-convert",
        "applies_to": [
          "pane"
        ],
        "dynamic": true,
        "command": "convert"
      }
    ],
    "panels": [
      {
        "id": "progress",
        "title": "Convert session",
        "modal": true
      }
    ],
    "jobs": [
      {
        "kind": "codex-to-claude",
        "deadline_ms": 60000,
        "memory_mib": 256
      }
    ],
    "services": {
      "ilium.core.session-convert/convert": {
        "start": {
          "params": {
            "type": "object",
            "required": [
              "pane",
              "target"
            ],
            "properties": {
              "pane": {
                "type": "integer"
              },
              "target": {
                "type": "string",
                "enum": [
                  "codex",
                  "claude"
                ]
              }
            }
          },
          "result": {
            "type": "object",
            "properties": {
              "conversion": {
                "type": "string"
              }
            },
            "required": [
              "conversion"
            ]
          }
        }
      }
    },
    "extension_points": [
      {
        "id": "formats",
        "kind": "collect",
        "title": "Transcript formats",
        "entry_schema": {
          "type": "object",
          "required": [
            "id",
            "title",
            "direction",
            "service"
          ],
          "properties": {
            "id": {
              "type": "string"
            },
            "title": {
              "type": "string"
            },
            "direction": {
              "type": "string",
              "enum": [
                "read",
                "write",
                "both"
              ]
            },
            "service": {
              "type": "string"
            }
          }
        },
        "max_entries": 8,
        "dynamic": false,
        "version": "0.1.0"
      },
      {
        "id": "after-convert",
        "kind": "hook",
        "mode": "observe",
        "title": "After conversion",
        "payload_schema": {
          "type": "object",
          "properties": {
            "from": {
              "type": "string"
            },
            "to": {
              "type": "string"
            },
            "messages": {
              "type": "integer"
            },
            "path": {
              "type": "string"
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

#### Menu entries with disabled reasons

The client side answers the dynamic menu with `menu-items`. It reads the agent session of the clicked pane and offers one entry. An entry that cannot run stays visible with a short reason. The click comes back as `command-invoked` with `{"item":"to-codex","node":42}` in `arguments_json`. See [menus](/pages/api-menus/), [the menu provider](/pages/api-menu-provider/) and [agent](/pages/primitives-agent/).

**Rust**

```rust
use ilium_plugin_sdk::exports::MenuProvider;
use ilium_plugin_sdk::primitives::agent;
use ilium_plugin_sdk::exports::menu_provider::MenuContext;
use ilium_plugin_sdk::types::ui::MenuEntry;

impl MenuProvider for Convert {
    fn menu_items(&mut self, context: MenuContext) -> Vec<MenuEntry> {
        let Some(pane) = context.pane else { return Vec::new() };
        let Some(signature) = self.signatures.get(&pane) else { return Vec::new() };
        let (id, label) = match signature.as_str() {
            "claude" => ("to-codex", "Convert to Codex"),
            "codex" => ("to-claude", "Convert to Claude Code"),
            _ => return Vec::new(), // Antigravity and custom agents cannot be converted
        };
        let reason = if agent::session(pane).is_none() {
            Some("no verified session")
        } else if self.running.is_some() {
            Some("another conversion runs")
        } else if self.worktree_panes.contains(&pane) {
            Some("worktree pane")
        } else {
            None
        };
        vec![MenuEntry::new(id, label).icon("arrows-exchange").disabled_reason(reason)]
    }
}
```

**TypeScript**

```ts
import { session } from '@ilium/plugin-sdk/primitives/agent';
import type { MenuContext, MenuEntry } from '@ilium/plugin-sdk/types';

export function menuItems(context: MenuContext): MenuEntry[] {
  if (context.pane === undefined) return [];
  const signature = signatures.get(context.pane);
  const choice = signature === 'claude' ? { id: 'to-codex', label: 'Convert to Codex' }
    : signature === 'codex' ? { id: 'to-claude', label: 'Convert to Claude Code' }
    : undefined;
  if (choice === undefined) return []; // Antigravity and custom agents cannot be converted
  const disabledReason = session(context.pane) === undefined ? 'no verified session'
    : running !== undefined ? 'another conversion runs'
    : worktreePanes.has(context.pane) ? 'worktree pane'
    : undefined;
  return [{ ...choice, icon: 'arrows-exchange', disabledReason }];
}
```

#### Freeze the pane and keep a journal

On the chosen entry the client side sends a message to the server side. The server checks that the source transcript exists, freezes the pane, writes the state to `storage` and adds a journal entry, so the plugin knows what it was doing after a crash. A durable timer is the safety net: if nothing finishes in ten minutes, the pane is unfrozen. See [storage](/pages/api-storage/), [durable timers and the journal](/pages/api-durable-timers/) and [pane lifecycle intents](/pages/api-intent-pane-lifecycle/).

**Rust**

```rust
use ilium_plugin_sdk::host::{clock, journal, storage, timers};
use ilium_plugin_sdk::host::storage::Bucket;
use ilium_plugin_sdk::types::intents::FreezePaneSpec;

const WATCHDOG_MILLIS: u64 = 10 * 60 * 1000;

fn begin(&mut self, pane: NodeId, target: &str) -> Result<Vec<Submission>, PluginError> {
    let state = format!(r#"{{"pane":{},"target":"{target}","phase":"frozen"}}"#, pane.0);
    storage::put(Bucket::Session, &format!("convert/{}", pane.0), state.as_bytes())?;
    journal::log_event(Some(pane), "convert-started", &format!(r#"{{"target":"{target}"}}"#))?;
    timers::at(clock::now_unix_millis() + WATCHDOG_MILLIS, &format!("convert-watchdog/{}", pane.0))?;
    Ok(vec![Submission::keyed(Intent::FreezePane(FreezePaneSpec { pane }), format!("freeze-{}", pane.0))])
}

// Event::DurableTimerFired(fired) if fired.name.starts_with("convert-watchdog/"):
//   read storage "convert/<pane>", and when it is still there, submit UnfreezePane and remove the key.
```

**TypeScript**

```ts
import { nowUnixMillis } from '@ilium/plugin-sdk/host/clock';
import * as journal from '@ilium/plugin-sdk/host/journal';
import * as storage from '@ilium/plugin-sdk/host/storage';
import * as timers from '@ilium/plugin-sdk/host/timers';
import type { Submission } from '@ilium/plugin-sdk/types';

const WATCHDOG_MILLIS = 10n * 60n * 1000n;

export function begin(pane: bigint, target: string): Submission[] {
  const state = JSON.stringify({ pane: Number(pane), target, phase: 'frozen' });
  storage.put('session', `convert/${pane}`, new TextEncoder().encode(state));
  journal.logEvent(pane, 'convert-started', JSON.stringify({ target }));
  timers.at(nowUnixMillis() + WATCHDOG_MILLIS, `convert-watchdog/${pane}`);
  return [{ intent: { tag: 'freeze-pane', val: { pane } }, correlation: 1n, dedupKey: `freeze-${pane}` }];
}

// case 'durable-timer-fired': when event.val.name starts with 'convert-watchdog/' and the storage key is
// still set, submit an unfreeze-pane intent and remove the key.
```

#### Codex to Claude Code as a job

Parsing a rollout and writing Claude Code lines can take longer than the event deadline, so it runs as a job. The job reads the rollout from the read-only Codex sessions scope, keeps user prompts, assistant text and tool calls with results, and drops reasoning and token events. It reports progress as it goes and stops when cancelled. See [jobs](/pages/api-jobs/), [fs](/pages/api-fs/) and [file access](/pages/file-access/).

**Rust**

```rust
use ilium_plugin_sdk::host::{fs, jobs};
use ilium_plugin_sdk::types::paths::FsPath;

fn codex_to_claude(input: &ConvertInput) -> Result<ConvertOutput, PluginError> {
    let rollout = FsPath { scope: "agent-sessions:codex".into(), path: input.rollout.clone() };
    // One read returns at most 1 MiB, so a larger rollout is read in a loop over offsets.
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        let chunk = fs::read(&rollout, bytes.len() as u64, 1_048_576)?;
        if chunk.is_empty() { break; }
        bytes.extend_from_slice(&chunk);
    }
    let text = String::from_utf8_lossy(&bytes).to_string();
    let lines: Vec<&str> = text.lines().collect();
    let (mut out, mut kept, mut dropped) = (Vec::new(), 0u32, 0u32);
    for (index, line) in lines.iter().enumerate() {
        if jobs::is_cancelled() { return Err(PluginError::cancelled()); }
        match claude_line(line) {
            Some(converted) => { out.push(converted); kept += 1; }
            None => dropped += 1, // reasoning and token events
        }
        if index % 50 == 0 { jobs::report_progress(100.0 * index as f32 / lines.len() as f32); }
    }
    Ok(ConvertOutput { claude_jsonl: out.join("\n"), kept, dropped })
}
```

**TypeScript**

```ts
import { read } from '@ilium/plugin-sdk/host/fs';
import { isCancelled, reportProgress } from '@ilium/plugin-sdk/host/jobs';

export function codexToClaude(input: { rollout: string }) {
  // One read returns at most 1 MiB, so a larger rollout is read in a loop over offsets.
  const chunks: Uint8Array[] = [];
  let offset = 0n;
  for (;;) {
    const chunk = read({ scope: 'agent-sessions:codex', path: input.rollout }, offset, 1048576);
    if (chunk.length === 0) break;
    chunks.push(chunk);
    offset += BigInt(chunk.length);
  }
  const bytes = new Uint8Array(chunks.reduce((total, part) => total + part.length, 0));
  let at = 0;
  for (const part of chunks) { bytes.set(part, at); at += part.length; }
  const lines = new TextDecoder().decode(bytes).split('\n');
  const out: string[] = [];
  let dropped = 0;
  lines.forEach((line, index) => {
    if (isCancelled()) throw { code: 'cancelled', message: 'conversion cancelled' };
    const converted = claudeLine(line);
    if (converted === undefined) dropped += 1; // reasoning and token events
    else out.push(converted);
    if (index % 50 === 0) reportProgress((100 * index) / lines.length);
  });
  return { claudeJsonl: out.join('\n'), kept: out.length, dropped };
}
```

#### Write the Claude Code transcript and verify it

The new transcript is written with an atomic write whose expected revision is `none`, so an existing file is never replaced: a clash completes as `failed(conflict)`. After the write, the plugin reads the file back with its revision and checks the line count before it registers the session. See [atomic write and revisions](/pages/api-fs-atomic-and-tail/) and [fs.write](/pages/capability-fs-write/).

**Rust**

```rust
use ilium_plugin_sdk::host::fs;

async fn write_and_verify(path: FsPath, jsonl: String, expected_lines: usize) -> Result<String, PluginError> {
    let revision = fs::write_atomic(&path, jsonl.into_bytes(), None).await?; // None: the file must not exist
    let back = fs::read_with_revision(&path).await?;
    if back.revision != revision {
        return Err(PluginError::conflict("transcript changed after the write"));
    }
    let lines = back.bytes.iter().filter(|byte| **byte == b'\n').count() + 1;
    if lines != expected_lines {
        return Err(PluginError::internal("line count differs after the write"));
    }
    Ok(revision)
}
```

**TypeScript**

```ts
import { readWithRevision, writeAtomic } from '@ilium/plugin-sdk/host/fs';
import type { FsPath } from '@ilium/plugin-sdk/types';

export async function writeAndVerify(path: FsPath, jsonl: string, expectedLines: number): Promise<string> {
  const revision = await writeAtomic(path, new TextEncoder().encode(jsonl), undefined); // undefined: must not exist
  const back = await readWithRevision(path);
  if (back.revision !== revision) throw { code: 'conflict', message: 'transcript changed after the write' };
  const lines = back.bytes.filter((byte) => byte === 0x0a).length + 1;
  if (lines !== expectedLines) throw { code: 'internal', message: 'line count differs after the write' };
  return revision;
}
```

#### Claude Code to Codex with a private app-server

Claude Code to Codex uses Codex's own importer, so the plugin starts a private `codex app-server` as a long-lived child and talks to it by lines. The child ends when the conversion ends, whatever the result. See [process control](/pages/api-process-control/), [process.run](/pages/capability-process-run/) and [process.long-lived](/pages/capability-process-long-lived/).

**Rust**

```rust
use ilium_plugin_sdk::host::process::{self, ProcessSpec, SpawnSpec};

fn start_app_server(&mut self, conversion: u64) -> Result<(), PluginError> {
    let child = process::spawn(&SpawnSpec {
        process: ProcessSpec {
            program: "codex".into(), arguments: vec!["app-server".into()],
            cwd: None, stdin: None, environment: Vec::new(), timeout_millis: 1_000,
        },
        stdin_open: true,
        line_mode: true,
    })?;
    self.app_servers.insert(conversion, child);
    Ok(())
}

fn stop_app_server(&mut self, conversion: u64) {
    if let Some(child) = self.app_servers.remove(&conversion) {
        let _ = child.signal("terminate"); // dropping the handle also ends it
    }
}
// Event::ChildOutput(output): one JSON line per message; match it to the import request of the conversion.
```

**TypeScript**

```ts
import { spawn, type Child } from '@ilium/plugin-sdk/host/process';

const appServers = new Map<bigint, Child>();

export function startAppServer(conversion: bigint): void {
  const child = spawn({
    process: { program: 'codex', arguments: ['app-server'], cwd: undefined, stdin: undefined,
               environment: [], timeoutMillis: 1_000 },
    stdinOpen: true,
    lineMode: true,
  });
  appServers.set(conversion, child);
}

export function stopAppServer(conversion: bigint): void {
  appServers.get(conversion)?.signal('terminate'); // dropping the handle also ends it
  appServers.delete(conversion);
}
// case 'child-output': one JSON line per message; match it to the import request of the conversion.
```

#### The progress dialog

The server sends steps, log lines and progress to the client side as messages. The client side keeps them and draws a `dialog` with a step list and a `progress-row`. Esc is the cancel button. After a failure that came after the agent was stopped, the primary button resumes the original session. See [layout and containers](/pages/widgets-layout-and-containers/), [data display](/pages/widgets-data-display/) and [ui](/pages/api-ui/).

**Rust**

```rust
use ilium_plugin_sdk::types::ui::{DialogKind, ProgressState};

fn render_dialog(&self, tree: &mut Tree) -> NodeHandle {
    let steps: Vec<NodeHandle> = self.steps.iter().map(|step| {
        tree.paragraph(vec![vec![Span::plain(format!("{} {}", step.mark(), step.title))]])
    }).collect();
    let progress = tree
        .progress_row(&self.title)
        .state(if self.failed { ProgressState::Error } else { ProgressState::Running })
        .percent(self.percent)
        .message(&self.last_log)
        .key("progress");
    let body = tree.layout(Direction::Vertical)
        .constraints(vec![Constraint::Fill(1), Constraint::Length(1)])
        .child(tree.layout(Direction::Vertical).constraints(vec![Constraint::Length(1); steps.len()]).children(steps))
        .child(progress);
    tree.dialog("convert-progress", &self.title)
        .kind(DialogKind::Plain)
        .width(Constraint::Length(48))
        .height(Constraint::Length(14))
        .child(body)
}
```

**TypeScript**

```ts
import { dialog, layout, length, fill, paragraph, progressRow, span } from '@ilium/plugin-sdk/ui/build';

export function renderDialog(state: ConvertState) {
  const steps = state.steps.map((step) => paragraph([[span(`${step.mark} ${step.title}`)]]));
  const progress = progressRow(state.title, {
    key: 'progress',
    state: state.failed ? 'error' : 'running',
    percent: state.percent,
    message: state.lastLog,
  });
  const body = layout({ direction: 'vertical', constraints: [fill(1), length(1)] }, [
    layout({ direction: 'vertical', constraints: steps.map(() => length(1)) }, steps),
    progress,
  ]);
  return dialog('convert-progress', {
    title: state.title, kind: 'plain', width: length(48), height: length(14),
    buttons: state.failed
      ? [{ id: 'resume', label: 'Resume original', role: 'primary', isCancel: false },
         { id: 'close', label: 'Close', role: 'secondary', isCancel: true }]
      : [{ id: 'cancel', label: 'Cancel', role: 'secondary', isCancel: true }],
  }, [body]);
}
```

#### Replace the frozen pane

On success the server creates a new pane that resumes the converted session in the other agent and closes the frozen one. On a failure after the agent was stopped, the user's choice decides: **Resume original** replaces the pane command with the original resume command and keeps the scrollback. See [pane lifecycle intents](/pages/api-intent-pane-lifecycle/), [intents](/pages/api-intents/) and [panes.create](/pages/capability-panes-create/).

**Rust**

```rust
use ilium_plugin_sdk::types::intents::{CreatePaneSpec, ReplacePaneCommandSpec};

fn finish_ok(pane: NodeId, target: &str, new_session: &str, cwd: FsPath) -> Vec<Submission> {
    let command = match target {
        "codex" => vec!["codex".to_string(), "resume".into(), new_session.into()],
        _ => vec!["claude".to_string(), "--resume".into(), new_session.into()],
    };
    vec![
        Submission::new(Intent::CreatePane(CreatePaneSpec {
            parent: None, title: format!("{target} (converted)"), content_kind: "ilium.terminal".into(),
            cwd: Some(cwd), command: Some(command), initial_input: None,
        })),
        Submission::new(Intent::CloseNode(pane)),
    ]
}

fn resume_original(pane: NodeId, original: Vec<String>) -> Submission {
    Submission::new(Intent::ReplacePaneCommand(ReplacePaneCommandSpec {
        pane, command: original, cwd: None, keep_scrollback: true,
    }))
}
```

**TypeScript**

```ts
import type { FsPath, Submission } from '@ilium/plugin-sdk/types';

export function finishOk(pane: bigint, target: string, newSession: string, cwd: FsPath): Submission[] {
  const command = target === 'codex' ? ['codex', 'resume', newSession] : ['claude', '--resume', newSession];
  return [
    { intent: { tag: 'create-pane', val: { parent: undefined, title: `${target} (converted)`,
        contentKind: 'ilium.terminal', cwd, command, initialInput: undefined } },
      correlation: 1n, dedupKey: undefined },
    { intent: { tag: 'close-node', val: pane }, correlation: 2n, dedupKey: undefined },
  ];
}

export const resumeOriginal = (pane: bigint, original: string[]): Submission => ({
  intent: { tag: 'replace-pane-command', val: { pane, command: original, cwd: undefined, keepScrollback: true } },
  correlation: 3n, dedupKey: undefined,
});
```

#### Owner side: formats and `after-convert`

`formats` is a collect point. A contributor adds a transcript format with a direction and a service that reads or writes it. The plugin lists the entries to build its source and target choices, and lists again when the kernel publishes `ilium.extensions/changed`. `after-convert` is an observe hook: the plugin runs it once when a conversion finishes and ignores the replies. See [events](/pages/api-events/), [requests and outcomes](/pages/requests-and-outcomes/) and [messages and topics](/pages/api-messages-and-topics/).

**Rust**

```rust
use ilium_plugin_sdk::host::extensions;

fn formats(&self) -> Vec<Format> {
    extensions::list("ilium.core.session-convert/formats")
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| serde_json::from_str::<Format>(&entry.entry_json).ok())
        .collect() // each Format has id, title, direction and service
}

fn announce(&mut self, from: &str, to: &str, messages: u32, path: &str) {
    let payload = serde_json::json!({ "from": from, "to": to, "messages": messages, "path": path });
    if let Ok(request) = extensions::run_hook_request("ilium.core.session-convert/after-convert", &payload.to_string()) {
        self.observers.insert(request);
    }
}

// Event::RequestCompleted(done) if self.observers.remove(&done.request): log the outcome, nothing else.
// Event::TopicPublished(t) if t.topic == "ilium.extensions/changed": call self.formats() again.
```

**TypeScript**

```ts
import * as extensions from '@ilium/plugin-sdk/host/extensions';

export const formats = () =>
  extensions.list('ilium.core.session-convert/formats')
    .map((entry) => JSON.parse(entry.entryJson) as { id: string; title: string; direction: string; service: string });

export function announce(from: string, to: string, messages: number, path: string): void {
  const request = extensions.runHook('ilium.core.session-convert/after-convert',
    JSON.stringify({ from, to, messages, path }));
  observers.add(request);
}

// case 'request-completed': if (observers.delete(event.val.request)) log the outcome, nothing else.
// case 'topic-published': if (event.val.topic === 'ilium.extensions/changed') formats() again.
```


### Extension points

Other plugins change what `ilium.core.session-convert` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.session-convert/<point>`). Contributors need `extensions.contribute:ilium.core.session-convert/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `formats` | collect | Add a transcript format that conversion can read or write. | entry: id, title, direction (read, write, both), service | standard |
| `after-convert` | hook (observe) | Learn that a conversion finished. | from, to, messages, path | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.session-convert/convert`: start: convert a transcript between two formats

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.session-convert"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Adds a third transcript format

A package adds this entry in its manifest. It needs no code.

```json
{
  "api": "ilium:plugin@0.1.0",
  "capabilities": { "server": ["extensions.contribute:ilium.core.session-convert/formats"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.session-convert/formats",
        "id": "gemini",
        "priority": 50,
        "entry": {
          "title": "Gemini CLI",
          "direction": "both",
          "service": "example.gemini/convert"
        }
      }
    ]
  }
}
```

### ABI used

- [Menus](/pages/api-menus/) and [the menu provider](/pages/api-menu-provider/)
- [Agent primitive](/pages/primitives-agent/)
- [Pane lifecycle intents](/pages/api-intent-pane-lifecycle/), [panes.lifecycle](/pages/capability-panes-lifecycle/) and [panes.create](/pages/capability-panes-create/)
- [Storage](/pages/api-storage/), [durable timers and the journal](/pages/api-durable-timers/)
- [Jobs](/pages/api-jobs/) and [jobs.run](/pages/capability-jobs-run/)
- [fs](/pages/api-fs/), [atomic write and revisions](/pages/api-fs-atomic-and-tail/) and [file access](/pages/file-access/)
- [Process control](/pages/api-process-control/) and [process.long-lived](/pages/capability-process-long-lived/)
- [Layout and containers](/pages/widgets-layout-and-containers/) and [data display](/pages/widgets-data-display/)
- [Intents](/pages/api-intents/), [events](/pages/api-events/) and [requests and outcomes](/pages/requests-and-outcomes/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Sessions](/pages/core-plugin-sessions/)
- [Compaction](/pages/core-plugin-compaction/)
