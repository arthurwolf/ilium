---
layout: page
title: Diagnostics
permalink: /pages/core-plugin-diagnostics/
---

## Diagnostics

Diagnostics helps when something goes wrong. It builds a report of facts about the install, it watches the warnings and errors that plugins write, and it helps the user attach the right log file to an issue. The plugin id is `ilium.core.diagnostics` and it runs on both sides. The server side reads the log file and builds the report. The client side shows the report and the warning list.

The Debug and About tabs, and the file logging switch, belong to the Settings plugin. Diagnostics does not draw them again. It reads the Debug setting without writing it, and the Debug tab's Build report button runs this plugin's `build-report` command. Diagnostics holds no grant on the Settings plugin's extension points and contributes no settings rows.

### What it covers

- The log file: one file for each server lifetime, named `log-<local date and time>.txt`, under `/tmp/.ilium-<uid>/logs/<session-id>/` on Linux and macOS and `%LOCALAPPDATA%\ilium\logs` on Windows. `ILIUM_DEBUG_LOG_DIR` replaces the root. The manifest grant `fs.read:path:/tmp/.ilium-<uid>/logs` is fixed and does not follow the override: when the override puts the logs outside the granted path, reads of them return `not-permitted`, and the report shows the line `log directory not readable: <path>` and lists no log files. Diagnostics does not fail, does not retry in a loop and does not widen its own grant. The five newest logs of a session are kept and older ones are removed when the server starts.
- File logging is off by default. It records actions, errors and complete model text exchanges. Credential headers and URL parameters are redacted, but other text is not. Treat a log as sensitive.
- Log levels: trace, debug, info, warn, error. A plugin writes its own diary at these levels with the host log. The user reads it with `ilium plugin logs <id>`.
- The `ilium-logging` service of the server, which opens the file, applies the switch live to client and server, flushes on request and shuts down in order.
- The report: a text block of facts, one section for each contributor, at most 64 KiB (65,536 bytes) for each section.
- Attaching a log to an issue: choose a file, see what the report holds, and read a warning about private content before anything leaves the machine.

### See it

There is no recording of this feature yet. The renders below are illustrative.

### What the user sees

<figure>
<pre class="term">╭─ Settings · Debug ───────────────────────────╮
│ File logging ………………………………………………………………………… <span class="t-ok">on</span> │
│ Log file ………………… log-2026-10-12 09-14-03.txt │
│ Size ………………………………………………………………………………… 1.2 MiB │
│                                              │
│ <span class="t-dim">Logs hold prompts and replies. Remove</span>        │
│ <span class="t-dim">private text before you share one.</span>           │
│                                              │
│ [ <span class="t-hi">Build report</span> ]   [ Open folder ]           │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The Debug tab is drawn by the Settings plugin. Diagnostics supplies the `build-report` command that the button runs.</figcaption>
</figure>

<figure>
<pre class="term">╭─ Report preview ─────────────────────────────╮
│ <span class="t-hi">Ilium</span>                                        │
│ Version ……………………………………………………………………………… 0.9.4 │
│ Platform ………………………………………………………… linux x86_64 │
│ <span class="t-hi">Plugins</span>                                      │
│ Loaded ………………………………………………………………………… 31 of 31 │
│ Last warning ……………… 09:13:58 ilium.core.cost │
│ <span class="t-hi">gpu</span>  <span class="t-dim">(from example.gpu)</span>                      │
│ Renderer …………………………………………………………………… software │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The report shows one block for each section. A section from another plugin names its source.</figcaption>
</figure>

<figure>
<pre class="term"><span class="t-dim">$</span> ilium plugin logs ilium.core.diagnostics
2026-10-12 09:14:03 info  report built, 5 sections
2026-10-12 09:14:03 warn  section gpu: no reply in 2000 ms
2026-10-12 09:14:04 info  report written, 18 KiB
</pre>
<figcaption>Rendered at 80 columns or fewer. Illustrative. The plugin writes its own diary with the host log. This is not the Ilium log file.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

The plugin is replaceable. A replacement must provide the report service and both points. The log folder lives outside the plugin scopes, so reading it needs a literal path grant. A literal absolute path is high risk and needs an explicit user grant. The example uses user id 1000, and the real grant is written at install from the user id.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.diagnostics",
  "name": "Diagnostics",
  "version": "0.1.0",
  "publisher": "Ilium",
  "limits_tier": "first-party",
  "replaceable": true,
  "surface": [
    "services:report", "points:log-sinks", "points:report-sections", "commands:build-report"
  ],
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": ["service-provider"]
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": ["views"]
  },
  "capabilities": {
    "server": [
      "fs.read:path:/tmp/.ilium-1000/logs",
      "settings.read-kernel:debug", "panes.read", "journal.read",
      "services.use:ilium.core.settings/get",
      "services.provide:ilium.core.diagnostics/report",
      "extensions.provide:ilium.core.diagnostics/log-sinks",
      "extensions.provide:ilium.core.diagnostics/report-sections"
    ],
    "client": ["ui.panels", "fs.pick", "notify.toast", "help.read"]
  },
  "contributes": {
    "commands": [{ "id": "build-report", "title": "Build diagnostics report", "category": "Help" }],
    "panels": [{ "id": "warnings", "title": "Warnings", "icon": "alert-triangle" }],
    "help": [{ "id": "diagnostics", "file": "help/diagnostics.md", "title": "Logs and reports" }],
    "services": {
      "ilium.core.diagnostics/report": {
        "build": {
          "params": { "type": "object", "properties": { "include_log_tail": { "type": "boolean" } } },
          "result": { "type": "object", "required": ["text"],
            "properties": { "text": { "type": "string", "maxLength": 262144 } } }
        }
      }
    },
    "extension_points": [
      { "id": "log-sinks", "kind": "hook", "mode": "observe", "title": "Log sinks",
        "payload_schema": { "type": "object", "properties": {
          "level": { "type": "string", "enum": ["warn", "error"] }, "plugin": { "type": "string" },
          "target": { "type": "string" }, "message": { "type": "string" }, "time": { "type": "string" } } },
        "risk": "high", "deadline_ms": 20, "max_handlers": 8, "version": "0.1.0" },
      { "id": "report-sections", "kind": "collect", "title": "Report sections",
        "entry_schema": { "type": "object", "required": ["id", "title", "service"],
          "properties": { "id": { "type": "string" }, "title": { "type": "string" },
                          "service": { "type": "string" } } },
        "max_entries": 32, "dynamic": false, "version": "0.1.0" }
    ]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.diagnostics",
  "name": "Diagnostics",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "Diagnostics helps when something goes wrong.",
  "license": "MIT",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": [
      "service-provider"
    ]
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": [
      "views"
    ]
  },
  "capabilities": {
    "server": [
      "fs.read:path:/tmp/.ilium-1000/logs",
      "settings.read-kernel:debug",
      "panes.read",
      "journal.read",
      "services.use:ilium.core.settings/get",
      "services.provide:ilium.core.diagnostics/report",
      "extensions.provide:ilium.core.diagnostics/log-sinks",
      "extensions.provide:ilium.core.diagnostics/report-sections"
    ],
    "client": [
      "ui.panels",
      "fs.pick",
      "notify.toast",
      "help.read"
    ]
  },
  "activation": [
    "startup",
    "view:diagnostics.warnings"
  ],
  "replaceable": true,
  "surface": [
    "services:report",
    "points:log-sinks",
    "points:report-sections",
    "commands:build-report"
  ],
  "contributes": {
    "commands": [
      {
        "id": "build-report",
        "title": "Build diagnostics report",
        "category": "Help"
      }
    ],
    "panels": [
      {
        "id": "warnings",
        "title": "Warnings",
        "icon": "alert-triangle"
      }
    ],
    "help": [
      {
        "id": "diagnostics",
        "file": "help/diagnostics.md",
        "title": "Logs and reports"
      }
    ],
    "services": {
      "ilium.core.diagnostics/report": {
        "build": {
          "params": {
            "type": "object",
            "properties": {
              "include_log_tail": {
                "type": "boolean"
              }
            }
          },
          "result": {
            "type": "object",
            "required": [
              "text"
            ],
            "properties": {
              "text": {
                "type": "string",
                "maxLength": 262144
              }
            }
          }
        }
      }
    },
    "extension_points": [
      {
        "id": "log-sinks",
        "kind": "hook",
        "mode": "observe",
        "title": "Log sinks",
        "payload_schema": {
          "type": "object",
          "properties": {
            "level": {
              "type": "string",
              "enum": [
                "warn",
                "error"
              ]
            },
            "plugin": {
              "type": "string"
            },
            "target": {
              "type": "string"
            },
            "message": {
              "type": "string"
            },
            "time": {
              "type": "string"
            }
          }
        },
        "risk": "high",
        "deadline_ms": 20,
        "max_handlers": 8,
        "version": "0.1.0"
      },
      {
        "id": "report-sections",
        "kind": "collect",
        "title": "Report sections",
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
        "max_entries": 32,
        "dynamic": false,
        "version": "0.1.0"
      }
    ]
  },
  "limits_tier": "first-party"
}
```

### Samples

#### Read the Debug setting without writing it

The file logging switch is a kernel setting, and the Settings plugin owns the screen. Diagnostics only needs to know the state, so it reads the `debug` tab with the read-only grant and listens for `kernel-settings-changed`. No plugin writes kernel settings. See [kernel settings](/pages/api-kernel-settings-and-keymap/) and [settings.read-kernel](/pages/capability-settings-read-kernel/).

**Rust**

```rust
use ilium_plugin_sdk::host::kernel_settings;

fn file_logging_on() -> bool {
    kernel_settings::read("debug")
        .ok()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .and_then(|value| value["file_logging_enabled"].as_bool())
        .unwrap_or(false)
}

// Event::KernelSettingsChanged(change) if change.tab == "debug" && change.keys.iter().any(|k| k == "file_logging_enabled"):
//   self.logging_on = file_logging_on();
//   self.tail_cursor = None; // a new server lifetime has a new file
```

**TypeScript**

```ts
import { read } from '@ilium/plugin-sdk/host/kernel-settings';

export function fileLoggingOn(): boolean {
  try {
    return JSON.parse(read('debug')).file_logging_enabled === true;
  } catch {
    return false;
  }
}

// case 'kernel-settings-changed': when event.val.tab === 'debug' and keys include 'file_logging_enabled':
//   state.loggingOn = fileLoggingOn(); state.tailCursor = undefined;
```

#### Tail the log file

The log can grow large, so the plugin never reads it whole. It keeps a cursor and asks for a chunk. A chunk holds the bytes, a new cursor and a flag that says the start was cut. The `name` argument is relative to the granted root, so it includes the session folder: `<session-id>/log-<local date and time>.txt`. The newest warning lines go to the warnings panel, and the size goes to the Debug tab. See [tail](/pages/api-fs-atomic-and-tail/) and [fs.read](/pages/capability-fs-read/).

**Rust**

```rust
use ilium_plugin_sdk::host::fs;
use ilium_plugin_sdk::types::paths::FsPath;

async fn poll_log(&mut self, name: &str) -> Result<(), PluginError> {
    let path = FsPath { scope: "path:/tmp/.ilium-1000/logs".into(), path: format!("{}/{}", self.session_id, name) };
    let chunk = fs::tail(&path, self.tail_cursor, 64 * 1024).await?;
    self.tail_cursor = Some(chunk.cursor);
    for line in String::from_utf8_lossy(&chunk.bytes).lines() {
        if line.contains(" WARN ") || line.contains(" ERROR ") {
            self.warnings.push_back(line.to_string());
            if self.warnings.len() > 200 { self.warnings.pop_front(); }
        }
    }
    if chunk.truncated { self.warnings.push_back("(earlier lines were skipped)".into()); }
    Ok(())
}
```

**TypeScript**

```ts
import { tail } from '@ilium/plugin-sdk/host/fs';

export async function pollLog(name: string): Promise<void> {
  const chunk = await tail({ scope: 'path:/tmp/.ilium-1000/logs', path: `${state.sessionId}/${name}` }, state.tailCursor, 64 * 1024);
  state.tailCursor = chunk.cursor;
  for (const line of new TextDecoder().decode(chunk.bytes).split('\n')) {
    if (line.includes(' WARN ') || line.includes(' ERROR ')) {
      state.warnings.push(line);
      if (state.warnings.length > 200) state.warnings.shift();
    }
  }
  if (chunk.truncated) state.warnings.push('(earlier lines were skipped)');
}
```

#### Write the plugin's own diary

Every plugin has a diary that the host keeps for it. It is separate from the Ilium log file. The plugin writes lines at five levels with the host log, at most 1,000 lines for each second, because it is a trusted T1 plugin (an ordinary plugin gets 200). The user reads them with `ilium plugin logs ilium.core.diagnostics`. No import reads the diary back, so the plugin keeps what it needs itself. See [log](/pages/api-log/).

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};

fn note_report(&self, sections: usize, bytes: usize, slow: &[String]) {
    log::log(Level::Info, &format!("report built, {sections} sections"));
    for name in slow {
        log::log(Level::Warn, &format!("section {name}: no reply in 2000 ms"));
    }
    if bytes > 200 * 1024 {
        log::log(Level::Debug, &format!("report is {} KiB", bytes / 1024));
    }
}
```

**TypeScript**

```ts
import { log } from '@ilium/plugin-sdk/host/log';

export function noteReport(sections: number, bytes: number, slow: string[]): void {
  log('info', `report built, ${sections} sections`);
  for (const name of slow) log('warn', `section ${name}: no reply in 2000 ms`);
  if (bytes > 200 * 1024) log('debug', `report is ${Math.floor(bytes / 1024)} KiB`);
}
```

#### Keep a journal of warnings

Warnings that matter to a pane go to the journal, so the story of that pane has them. The journal is durable and each entry has a kind and a JSON payload. The report reads the newest entries back. Reading entries that other plugins wrote needs the `journal.read` grant. See [durable timers and the journal](/pages/api-durable-timers/) and [journal.read](/pages/capability-journal-read/).

**Rust**

```rust
use ilium_plugin_sdk::host::journal;

fn remember(&self, pane: Option<NodeId>, line: &str) -> Result<(), PluginError> {
    let payload = serde_json::json!({ "line": line }).to_string();
    journal::log_event(pane, "diagnostics.warning", &payload)
}

fn recent_for_report(&self, pane: NodeId) -> Vec<String> {
    journal::read(Some(pane), 0, 20)
        .into_iter()
        .filter(|entry| entry.kind == "diagnostics.warning")
        .map(|entry| entry.payload_json)
        .collect()
}
```

**TypeScript**

```ts
import * as journal from '@ilium/plugin-sdk/host/journal';

export const remember = (pane: bigint | undefined, line: string): void =>
  journal.logEvent(pane, 'diagnostics.warning', JSON.stringify({ line }));

export const recentForReport = (pane: bigint | undefined): string[] =>
  journal.read(pane, 0n, 20)
    .filter((entry) => entry.kind === 'diagnostics.warning')
    .map((entry) => entry.payloadJson);
```

#### Build the report in a service

The service `report/build` returns the text. It starts with its own facts, then asks each contributed section for text. A section that is slow or fails is named and skipped, so one bad contributor cannot stop the report. The result is cut at 256 KiB. The call is deferred, so the plugin answers when the sections are in. Each section service gets a reply deadline of 2000 ms, and the whole report has an overall deadline of 10,000 ms; sections not in by then are named as slow and skipped. These two deadlines are the plugin's own constants. They are not fields of the `report-sections` point or of its entry schema. The calls to the section services need no `services.use` grant: the kernel admits them because the contributed entry is active (see [entry-bound services](/pages/api-extension-points/#entry-bound-services)). Only `services.use:ilium.core.settings/get` is granted for the Settings read. See [services](/pages/api-services/) and [client requests](/pages/api-client-requests/).

**Rust**

```rust
use ilium_plugin_sdk::host::{extensions, services};

async fn build_report(&mut self) -> String {
    let mut text = self.core_facts();
    for entry in extensions::list("ilium.core.diagnostics/report-sections").unwrap_or_default() {
        let Ok(section) = serde_json::from_str::<Section>(&entry.entry_json) else { continue };
        match services::call(&section.service, "text", "{}").await {
            Ok(reply) => {
                let mut end = reply.len().min(64 * 1024);
                while !reply.is_char_boundary(end) {
                    end -= 1;
                }
                let body: String = reply[..end].to_string();
                text.push_str(&format!("\n## {} (from {})\n{}\n", section.title, entry.contributor, body));
            }
            Err(error) => {
                log::log(Level::Warn, &format!("section {}: {}", section.id, error.code));
                text.push_str(&format!("\n## {}\n(not available)\n", section.title));
            }
        }
    }
    text.chars().take(256 * 1024).collect()
}
```

**TypeScript**

```ts
import * as extensions from '@ilium/plugin-sdk/host/extensions';
import { call } from '@ilium/plugin-sdk/host/services';
import { log } from '@ilium/plugin-sdk/host/log';

/** Cuts at a byte limit without splitting a character. */
function cap_bytes(text: string, limit: number): string {
  const bytes = new TextEncoder().encode(text).slice(0, limit);
  return new TextDecoder('utf-8').decode(bytes).replace(/\uFFFD+$/, '');
}

export async function buildReport(): Promise<string> {
  let text = coreFacts();
  for (const entry of extensions.list('ilium.core.diagnostics/report-sections')) {
    const section = JSON.parse(entry.entryJson) as { id: string; title: string; service: string };
    try {
      const reply = await call(section.service, 'text', '{}');
      text += `\n## ${section.title} (from ${entry.contributor})\n${cap_bytes(reply, 64 * 1024)}\n`;
    } catch (error) {
      log('warn', `section ${section.id}: ${(error as { code: string }).code}`);
      text += `\n## ${section.title}\n(not available)\n`;
    }
  }
  return text.slice(0, 256 * 1024);
}
```

#### Owner side: run the `log-sinks` hook

`log-sinks` is an observe hook. Warnings and errors that the plugin finds in the log are sent to it in batches of up to 64 records, one chain per 250 ms ([EXT-054]). Handlers may forward them to a chat tool or a metrics tool. Replies are ignored, and a handler that fails does not matter. The plugin lists the current contributors only to show the count in the panel, and it lists again on `ilium.extensions/changed`. See [events](/pages/api-events/) and [requests and outcomes](/pages/requests-and-outcomes/).

**Rust**

```rust
use ilium_plugin_sdk::host::extensions;
use ilium_plugin_sdk::types::requests::{Outcome, RequestResult};

fn publish_line(&mut self, level: &str, plugin: &str, message: &str, time: &str) {
    if self.sink_count == 0 { return; } // nobody listens
    let payload = serde_json::json!({
        "level": level, "plugin": plugin, "target": "plugin", "message": message, "time": time,
    });
    if let Ok(request) = extensions::run_hook_request("ilium.core.diagnostics/log-sinks", &payload.to_string()) {
        self.observed.insert(request);
    }
}

fn on_completed(&mut self, done: RequestResult) {
    self.observed.remove(&done.request); // an observe hook has no reply to take
}

// Event::TopicPublished(t) if t.topic == "ilium.extensions/changed":
//   self.sink_count = extensions::list("ilium.core.diagnostics/log-sinks").map_or(0, |list| list.len());
```

**TypeScript**

```ts
import * as extensions from '@ilium/plugin-sdk/host/extensions';

export function publishLine(level: string, plugin: string, message: string, time: string): void {
  if (state.sinkCount === 0) return; // nobody listens
  const request = extensions.runHookRequest('ilium.core.diagnostics/log-sinks',
    JSON.stringify({ level, plugin, target: 'plugin', message, time }));
  state.observed.add(request);
}

export const onCompleted = (request: bigint): void => {
  state.observed.delete(request); // an observe hook has no reply to take
};

// case 'topic-published': when event.val.topic === 'ilium.extensions/changed',
// state.sinkCount = extensions.list('ilium.core.diagnostics/log-sinks').length.
```

#### Attach a log to an issue

The user picks a file to attach. A dialog warns that logs hold private text and shows the size. Only after the user confirms does the plugin copy the log to the place the user chose. The default button is Cancel. The copy is a plain read and write, so it keeps the original. See [dialogs](/pages/widgets-layout-and-containers/), [pickers](/pages/widgets-pickers/) and [fs.pick](/pages/capability-fs-pick/).

**Rust**

```rust
use ilium_plugin_sdk::types::ui::DialogKind;

fn render_attach(&self, tree: &mut Tree) -> NodeHandle {
    let message = format!(
        "{} ({} KiB) can hold prompts, replies and file text. Remove private text before you share it.",
        self.picked_name, self.picked_kib
    );
    tree.dialog("attach-log", "Attach a log to an issue")
        .kind(DialogKind::DangerConfirm)
        .message(&message)
        .width(Constraint::Length(56))
        .height(Constraint::Length(10))
        .child(tree.path_field(PathKind::SaveFile).key("destination").value(&self.destination))
}

// Event::UiAction(action) if action.key == "attach-log" && action.kind == ActionKind::Activated
//   and the button id is "confirm": copy the file, then submit a Toast intent that names the destination.
```

**TypeScript**

```ts
import { dialog, length, pathField } from '@ilium/plugin-sdk/ui/build';

export const renderAttach = (state: AttachState) =>
  dialog('attach-log', {
    title: 'Attach a log to an issue',
    kind: 'danger-confirm',
    message: `${state.pickedName} (${state.pickedKib} KiB) can hold prompts, replies and file text. ` +
      'Remove private text before you share it.',
    width: length(56),
    height: length(10),
    buttons: [
      { id: 'confirm', label: 'Copy log', role: 'primary', isCancel: false },
      { id: 'cancel', label: 'Cancel', role: 'secondary', isCancel: true },
    ],
  }, [pathField('save-file', { key: 'destination', value: state.destination })]);

// case 'ui-action': when key is 'attach-log', kind is 'activated' and the button is 'confirm':
// copy the file, then submit a toast intent that names the destination.
```


### Extension points

Other plugins change what `ilium.core.diagnostics` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.diagnostics/<point>`). Contributors need `extensions.contribute:ilium.core.diagnostics/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `log-sinks` | hook (observe) | Receive log records, for example to forward them. | level, plugin, target, message, time (records at warn and above only) | high |
| `report-sections` | collect | Add a section to the diagnostics report. | entry: id, title, service (returns text, at most 64 KiB) | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.diagnostics/report`: build: build the diagnostics report

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.diagnostics"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Adds GPU facts to the report

A package adds this entry in its manifest. It needs no code.

```json
{
  "api": "ilium:plugin@0.1.0",
  "capabilities": { "server": ["extensions.contribute:ilium.core.diagnostics/report-sections"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.diagnostics/report-sections",
        "id": "gpu",
        "priority": 50,
        "entry": {
          "title": "GPU",
          "service": "example.gpu/report"
        }
      }
    ]
  }
}
```

### ABI used

- [Kernel settings](/pages/api-kernel-settings-and-keymap/) and [settings.read-kernel](/pages/capability-settings-read-kernel/)
- [fs](/pages/api-fs/), [tail](/pages/api-fs-atomic-and-tail/), [fs.read](/pages/capability-fs-read/) and [fs.pick](/pages/capability-fs-pick/)
- [Log](/pages/api-log/)
- [Durable timers and the journal](/pages/api-durable-timers/) and [journal.read](/pages/capability-journal-read/)
- [Services](/pages/api-services/) and [client requests](/pages/api-client-requests/)
- [Layout and containers](/pages/widgets-layout-and-containers/) and [pickers](/pages/widgets-pickers/)
- [Events](/pages/api-events/) and [requests and outcomes](/pages/requests-and-outcomes/)
- [Messages and topics](/pages/api-messages-and-topics/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Settings](/pages/core-plugin-settings/)
- [Settings screens](/pages/settings-screens/)

### Security

Log sinks receive redacted records ([SEC-249]); see [Secrets](/pages/reference-secrets/).
