---
layout: page
title: Agent setup
permalink: /pages/core-plugin-agent-setup/
---

## Agent setup

Agents work better in Ilium when their instruction files describe what Ilium offers. Agent setup installs two blocks of instructions into the files an agent reads at start. The Chatroom block tells agents how to talk to each other. The Progress block tells them how to report long jobs. The plugin id is `ilium.core.agent-setup`. The client side draws the Setup tab and asks before it changes a project file. The server side reads the files and writes the project files, because only the server holds the file grants. It never writes a global file in an agent's configuration folder: it proposes the change with `agent-config.propose:<agent>`, the kernel shows the user the diff, and the host writes only what the user accepts ([agent-config.propose](/pages/capability-agent-config-propose/)).

Each block sits between two marker lines, so Ilium can find it, upgrade it and leave everything else alone.

### What it covers

- The two blocks, Chatroom and Progress, and the files they go into: the global Claude file `~/.claude/CLAUDE.md` (the path can be changed), the global Codex file `~/.codex/AGENTS.md`, and for each open project a `CLAUDE.md` and an `AGENTS.md`.
- Markers: `<!-- ilium-agent-feature: chatroom -->` before the block and `<!-- /ilium-agent-feature: chatroom -->` after it. The Progress marker adds a version, as in `<!-- ilium-agent-feature: progress version=13 -->`, so an older block is upgraded and a newer block is left alone.
- Safety rules: text outside the markers is kept, a file over 512 KiB is not changed, a symbolic link stays a symbolic link, and look-alike text without markers is reported as "Instruction detected" and not touched.
- Timing: when a client attaches and when a project opens, the plugin checks the global files. A missing or older global block becomes one proposal per agent, which the user accepts or rejects in the kernel's proposal dialog. A rejected proposal is not repeated for the same block version; the user installs it later from the Setup tab. Project copies are never added automatically. They come only from the Setup tab.
- The five statuses: Set up by Ilium, Ilium update required, Newer Ilium instructions detected, Instruction detected, Not set up.
- Chatroom setup also creates `CHATROOM.md` and the hooks that deliver messages.

### See it

There is no recording of this feature yet. The renders below are illustrative.

### What the user sees

<figure>
<pre class="term">╭─ Settings · Setup ───────────────────────────╮
│ Global file ……………………………… ~/.claude/CLAUDE.md │
│                                              │
│ <span class="t-hi">Chatroom</span>                                     │
│ Global ……………………………………………………… <span class="t-ok">Set up by Ilium</span> │
│ Project ilium ………………………………………………… <span class="t-dim">Not set up</span> │
│                                              │
│ <span class="t-hi">Progress</span>                                     │
│ Global ……………………………………… <span class="t-warn">Ilium update required</span> │
│ Project ilium ……………………… <span class="t-warn">Instruction detected</span> │
│ <span class="t-dim">Enter refreshes a row. R restores path.</span>      │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The Setup tab: one row per target, with the status of its block.</figcaption>
</figure>

### Setup tab: statuses, rows and files

The Setup tab belongs to this plugin, and the Chatroom and Progress pages link here instead of describing it again. [COR-011]

| Status badge | Meaning |
|---|---|
| Set up by Ilium | The current managed block is present. |
| Ilium update required | An older managed block is present. The plugin proposes the refresh, and it is applied when the user accepts it. |
| Newer Ilium instructions detected | The file was written by a newer Ilium. It is left untouched. |
| Instruction detected | Matching text that Ilium did not write. |
| Not set up | No block yet. |

There is no sixth badge for a file over 512 KiB. Such a file keeps the badge its content gives it, the install and refresh actions on its row fail with `invalid-argument` and the message `file is over 512 KiB`, the notice shows that message, and the file is not read further or written.

Each feature has a **Global file** row (`Enter` changes the path, `R` restores `~/.claude/CLAUDE.md`), a **Global** row per global file and a **Project** row for every open project. `Enter` on a target refreshes it: a project target after the plugin's own confirm dialog, a global target through a proposal that the user accepts in the kernel's dialog. Setup is not an on/off switch, because removing a mandatory block would leave new agents uninformed. The paths are stored in `[agent_setup]` as `chatroom_global_file` and `progress_global_file`.

| Scope | Files edited |
|---|---|
| Global (Claude) | `~/.claude/CLAUDE.md`, or the custom path of that feature |
| Global (Codex) | `~/.codex/AGENTS.md` |
| Each open project | `CLAUDE.md` and `AGENTS.md` in the project root, only from the Setup tab |

Blocks sit between the markers `<!-- ilium-agent-feature: chatroom -->` and `<!-- /ilium-agent-feature: chatroom -->`. Only text between the markers is replaced or removed. The `ilium chat init` command and the start-time repair (when `CHATROOM.md` exists) install the chatroom hooks through this plugin; the Setup tab installs the instruction blocks. The two are separate actions with this plugin as the single owner of the files. [COR-011]

<figure>
<pre class="term">╭─ Install Progress block ─────────────────────╮
│ File: ~/.codex/AGENTS.md                     │
│ Block version 12 will become version 13.     │
│                                              │
│ <span class="t-dim">Text outside the markers stays as it is.</span>     │
│ <span class="t-dim">The file is 4 KiB. The limit is 512 KiB.</span>     │
│                                              │
│ [ <span class="t-hi">Update</span> ]   [ Cancel ]                      │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The confirm dialog for an upgrade of an older block.</figcaption>
</figure>

<figure>
<pre class="term"><span class="t-dim">$</span> ilium agent-setup status
{"type":"target","block":"chatroom","scope":"global",
 "path":"~/.claude/CLAUDE.md","status":"set-up"}
{"type":"target","block":"progress","scope":"global",
 "path":"~/.codex/AGENTS.md","status":"update-required",
 "installed":12,"current":13}
{"type":"summary","targets":2,"set_up":1,"update_required":1}
</pre>
<figcaption>Rendered at 80 columns or fewer. Illustrative. The command <code>ilium agent-setup</code> is an example of what a plugin may add. It is not part of the current CLI.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

The plugin is replaceable. A replacement must provide the service and the three points: the `surface` list names exactly those (a surface entry has the form `services:`, `points:`, `commands:`, `pane-kinds:`, `topics:` or `cli:`, so a settings tab cannot be listed) and install checks only that list. Without its own Setup tab the replacement leaves the Setup tab empty, and agents lose the per-project install rows. The grants for the files are `fs.read:agent-config:<agent>` (high risk) with `agent-config.propose:<agent>` (high risk) for the global files, and `fs.read:project` with `fs.write:project` for the project files. A direct `fs.write:agent-config:<agent>` grant is refused at install with `manifest.grant-scope`.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.agent-setup",
  "name": "Agent setup",
  "version": "0.1.0",
  "publisher": "Ilium",
  "limits_tier": "first-party",
  "replaceable": true,
  "surface": [
    "services:install", "points:instruction-blocks", "points:targets",
    "points:before-install", "commands:refresh"
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
      "fs.read:agent-config:claude", "agent-config.propose:claude",
      "fs.read:agent-config:codex", "agent-config.propose:codex",
      "fs.read:project", "fs.write:project",
      "tree.read", "settings.read-kernel:setup",
      "services.provide:ilium.core.agent-setup/install",
      "extensions.provide:ilium.core.agent-setup/instruction-blocks",
      "extensions.provide:ilium.core.agent-setup/targets",
      "extensions.provide:ilium.core.agent-setup/before-install",
      "settings.write-kernel:setup"
    ],
    "client": ["ui.panels", "help.read"]
  },
  "contributes": {
    "commands": [{ "id": "refresh", "title": "Refresh agent instructions", "category": "Agents" }],
    "help": [{ "id": "agent-setup", "file": "help/agent-setup.md", "title": "Agent setup" }],
    "services": {
      "ilium.core.agent-setup/install": {
        "install": { "params": { "type": "object", "required": ["block", "target"],
          "properties": { "block": { "type": "string" }, "target": { "type": "string" } } },
          "result": { "type": "object", "properties": { "status": { "type": "string" } } } },
        "remove": { "params": { "type": "object", "required": ["block", "target"],
          "properties": { "block": { "type": "string" }, "target": { "type": "string" } } },
          "result": { "type": "object", "properties": { "status": { "type": "string" } } } },
        "status": { "params": { "type": "object" },
          "result": { "type": "object", "properties": { "targets": { "type": "array" } } } }
      }
    },
    "extension_points": [
      { "id": "instruction-blocks", "kind": "collect", "title": "Instruction blocks",
        "entry_schema": { "type": "object", "required": ["id", "title", "marker", "file"],
          "properties": { "id": { "type": "string" }, "title": { "type": "string" },
                          "marker": { "type": "string" }, "file": { "type": "string" } } },
        "max_entries": 8, "dynamic": false, "version": "0.1.0" },
      { "id": "targets", "kind": "collect", "title": "Target files",
        "entry_schema": { "type": "object", "required": ["id", "title", "path_template", "scope"],
          "properties": { "id": { "type": "string" }, "title": { "type": "string" },
                          "path_template": { "type": "string" },
                          "scope": { "type": "string", "enum": ["global", "project"] } } },
        "max_entries": 16, "dynamic": false, "version": "0.1.0" },
      { "id": "before-install", "kind": "hook", "mode": "veto", "title": "Before install",
        "payload_schema": { "type": "object", "properties": {
          "target": { "type": "string" }, "block": { "type": "string" }, "path": { "type": "string" } } },
        "risk": "high", "deadline_ms": 20, "max_handlers": 4, "version": "0.1.0" }
    ]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.agent-setup",
  "name": "Agent setup",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "Agents work better in Ilium when their instruction files describe what Ilium offers.",
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
      "fs.read:agent-config:claude",
      "agent-config.propose:claude",
      "fs.read:agent-config:codex",
      "agent-config.propose:codex",
      "fs.read:project",
      "fs.write:project",
      "tree.read",
      "settings.read-kernel:setup",
      "services.provide:ilium.core.agent-setup/install",
      "extensions.provide:ilium.core.agent-setup/instruction-blocks",
      "extensions.provide:ilium.core.agent-setup/targets",
      "extensions.provide:ilium.core.agent-setup/before-install",
      "settings.write-kernel:setup"
    ],
    "client": [
      "ui.panels",
      "help.read"
    ]
  },
  "activation": [
    "settings:setup",
    "command:refresh",
    "service:install"
  ],
  "replaceable": true,
  "surface": [
    "services:install",
    "points:instruction-blocks",
    "points:targets",
    "points:before-install",
    "commands:refresh"
  ],
  "contributes": {
    "commands": [
      {
        "id": "refresh",
        "title": "Refresh agent instructions",
        "category": "Agents"
      }
    ],
    "help": [
      {
        "id": "agent-setup",
        "file": "help/agent-setup.md",
        "title": "Agent setup"
      }
    ],
    "services": {
      "ilium.core.agent-setup/install": {
        "install": {
          "params": {
            "type": "object",
            "required": [
              "block",
              "target"
            ],
            "properties": {
              "block": {
                "type": "string"
              },
              "target": {
                "type": "string"
              }
            }
          },
          "result": {
            "type": "object",
            "properties": {
              "status": {
                "type": "string"
              }
            }
          }
        },
        "remove": {
          "params": {
            "type": "object",
            "required": [
              "block",
              "target"
            ],
            "properties": {
              "block": {
                "type": "string"
              },
              "target": {
                "type": "string"
              }
            }
          },
          "result": {
            "type": "object",
            "properties": {
              "status": {
                "type": "string"
              }
            }
          }
        },
        "status": {
          "params": {
            "type": "object"
          },
          "result": {
            "type": "object",
            "properties": {
              "targets": {
                "type": "array"
              }
            }
          }
        }
      }
    },
    "extension_points": [
      {
        "id": "instruction-blocks",
        "kind": "collect",
        "title": "Instruction blocks",
        "entry_schema": {
          "type": "object",
          "required": [
            "id",
            "title",
            "marker",
            "file"
          ],
          "properties": {
            "id": {
              "type": "string"
            },
            "title": {
              "type": "string"
            },
            "marker": {
              "type": "string"
            },
            "file": {
              "type": "string"
            }
          }
        },
        "max_entries": 8,
        "dynamic": false,
        "version": "0.1.0"
      },
      {
        "id": "targets",
        "kind": "collect",
        "title": "Target files",
        "entry_schema": {
          "type": "object",
          "required": [
            "id",
            "title",
            "path_template",
            "scope"
          ],
          "properties": {
            "id": {
              "type": "string"
            },
            "title": {
              "type": "string"
            },
            "path_template": {
              "type": "string"
            },
            "scope": {
              "type": "string",
              "enum": [
                "global",
                "project"
              ]
            }
          }
        },
        "max_entries": 16,
        "dynamic": false,
        "version": "0.1.0"
      },
      {
        "id": "before-install",
        "kind": "hook",
        "mode": "veto",
        "title": "Before install",
        "payload_schema": {
          "type": "object",
          "properties": {
            "target": {
              "type": "string"
            },
            "block": {
              "type": "string"
            },
            "path": {
              "type": "string"
            }
          }
        },
        "risk": "high",
        "deadline_ms": 20,
        "max_handlers": 4,
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
| `setup` | Setup | `[agent_setup]` | none |

If the plugin is disabled, the tab stays in the rail with its stored values shown read-only and the notice "Agent setup is disabled. Values are kept." The kernel keeps reading the keys it consumes itself (listed in the Consumer column of the key table in [config and data migration](/pages/reference-config-and-data-migration/)). Behaviour that only this plugin provides is off. The exact result for this plugin is its row in [kernel and boot](/pages/reference-kernel-and-boot/). [COR-022]

### Samples

#### Replace one marked block in place

The core of the plugin is one pure function and one safe change. The function finds the markers, replaces what lies between them, and returns the new text. It replaces only when both markers are present and the opening marker comes first. In every other case (no marker, only one of the two, or the closing marker before the opening one) the sample appends the block at the end and leaves the stray marker text where it is. The real plugin first reports the lone or reversed marker as "Instruction detected" (see the safety rules above) and does not write, so this append-only fallback is a sample simplification. A project file is written with `write-atomic`. A global file in an agent's configuration folder is proposed with `propose-agent-config`, so the kernel shows the user the diff and the host writes it only when the user accepts. Both carry the revision that came with the read, so a change made by someone else in between ends as `failed(conflict)` and nothing is lost. See [atomic write and revisions](/pages/api-fs-atomic-and-tail/), [fs.write](/pages/capability-fs-write/) and [agent-config.propose](/pages/capability-agent-config-propose/).

**Rust**

```rust
use ilium_plugin_sdk::host::fs::{self, ConfigChange};
use ilium_plugin_sdk::types::paths::FsPath;

const MAX_BYTES: usize = 512 * 1024;

fn with_block(text: &str, marker: &str, block: &str) -> String {
    let open = format!("<!-- ilium-agent-feature: {marker}");
    let close = format!("<!-- /ilium-agent-feature: {marker} -->");
    match (text.find(&open), text.find(&close)) {
        (Some(start), Some(end)) if start < end => {
            let end = end + close.len();
            format!("{}{}{}", &text[..start], block, &text[end..])
        }
        _ => format!("{}{}\n{}\n", text, if text.ends_with('\n') || text.is_empty() { "" } else { "\n" }, block),
    }
}

async fn install(path: FsPath, marker: &str, block: &str) -> Result<(), PluginError> {
    let current = fs::read_with_revision(&path).await?;
    if current.bytes.len() > MAX_BYTES {
        return Err(PluginError::invalid_argument("file is over 512 KiB"));
    }
    let text = String::from_utf8_lossy(&current.bytes);
    let next = with_block(&text, marker, block).into_bytes();
    match path.scope.strip_prefix("agent-config:") {
        // A global file: the user sees the diff and decides. A rejection ends as Err(not-permitted).
        Some(agent) => {
            let change = ConfigChange {
                path: path.path.clone(),
                content: Some(next),
                revision: Some(current.revision),
                summary: format!("install the {marker} block"),
            };
            fs::propose_agent_config(agent, "Ilium instructions", vec![change]).await?;
        }
        // A project file, after the plugin's own confirm dialog.
        None => {
            fs::write_atomic(&path, next, Some(current.revision)).await?;
        }
    }
    Ok(())
}
```

**TypeScript**

```ts
import { proposeAgentConfig, readWithRevision, writeAtomic } from '@ilium/plugin-sdk/host/fs';
import type { FsPath } from '@ilium/plugin-sdk/types';

const MAX_BYTES = 512 * 1024;

export function withBlock(text: string, marker: string, block: string): string {
  const open = `<!-- ilium-agent-feature: ${marker}`;
  const close = `<!-- /ilium-agent-feature: ${marker} -->`;
  const start = text.indexOf(open);
  const end = text.indexOf(close);
  if (start !== -1 && end > start) return text.slice(0, start) + block + text.slice(end + close.length);
  const gap = text === '' || text.endsWith('\n') ? '' : '\n';
  return `${text}${gap}\n${block}\n`;
}

export async function install(path: FsPath, marker: string, block: string): Promise<void> {
  const current = await readWithRevision(path);
  if (current.bytes.length > MAX_BYTES) throw { code: 'invalid-argument', message: 'file is over 512 KiB' };
  const next = new TextEncoder().encode(withBlock(new TextDecoder().decode(current.bytes), marker, block));
  if (path.scope.startsWith('agent-config:')) {
    // A global file: the user sees the diff and decides. A rejection throws not-permitted.
    const agent = path.scope.slice('agent-config:'.length);
    await proposeAgentConfig(agent, 'Ilium instructions', [
      { path: path.path, content: next, revision: current.revision, summary: `install the ${marker} block` },
    ]);
    return;
  }
  await writeAtomic(path, next, current.revision); // a project file, after the plugin's own confirm dialog
}
```

#### Read the status of a target

A target has one of five statuses. The plugin reads the file and compares what it finds with the block it would write. A block with markers and the same version is "Set up by Ilium". An older version is "Ilium update required". A newer version is "Newer Ilium instructions detected". Text that looks like the block but has no markers is "Instruction detected". "Looks like the block" means the text contains the feature's look-alike phrase, `LOOK_ALIKE_PHRASE` in the samples: the first sentence of the block body, a fixed string the plugin ships with each feature and compares case-sensitively. Nothing at all is "Not set up". See [fs](/pages/api-fs/) and [file access](/pages/file-access/).

**Rust**

```rust
/// First sentence of the block body for this feature; the text a hand-written copy would share.
const LOOK_ALIKE_PHRASE: &str = "Use the Ilium agent tools";

#[derive(Debug, PartialEq)]
enum Status { SetUp, UpdateRequired, Newer, Detected, NotSetUp }

fn status_of(text: &str, marker: &str, current_version: u32) -> Status {
    let open = format!("<!-- ilium-agent-feature: {marker}");
    if let Some(start) = text.find(&open) {
        let line = text[start..].lines().next().unwrap_or("");
        let version = line.split("version=").nth(1)
            .and_then(|rest| rest.trim_end_matches("-->").trim().parse::<u32>().ok())
            .unwrap_or(0);
        return match version.cmp(&current_version) {
            std::cmp::Ordering::Less => Status::UpdateRequired,
            std::cmp::Ordering::Greater => Status::Newer,
            std::cmp::Ordering::Equal => Status::SetUp,
        };
    }
    if text.contains(LOOK_ALIKE_PHRASE) { Status::Detected } else { Status::NotSetUp }
}
```

**TypeScript**

```ts
// First sentence of the block body for this feature.
const LOOK_ALIKE_PHRASE = 'Use the Ilium agent tools';

export type Status = 'set-up' | 'update-required' | 'newer' | 'detected' | 'not-set-up';

export function statusOf(text: string, marker: string, currentVersion: number): Status {
  const open = `<!-- ilium-agent-feature: ${marker}`;
  const start = text.indexOf(open);
  if (start !== -1) {
    const line = text.slice(start).split('\n', 1)[0];
    const version = Number(/version=(\d+)/.exec(line)?.[1] ?? 0);
    if (version < currentVersion) return 'update-required';
    return version > currentVersion ? 'newer' : 'set-up';
  }
  return text.includes(LOOK_ALIKE_PHRASE) ? 'detected' : 'not-set-up';
}
```

#### The Setup tab as a check list

The tab is a `check-list` with one row per target. The status is the row's trailing text. The row key carries the block and the target, so the action can say what to refresh. Enter on a row sends a refresh request. See [collections](/pages/widgets-collections/) and [ui](/pages/api-ui/).

**Rust**

```rust
use ilium_plugin_sdk::types::ui::CheckItem;
use ilium_plugin_sdk::types::text::Tone;

fn render_setup(&self, tree: &mut Tree) -> NodeHandle {
    let items: Vec<CheckItem> = self.targets.iter().map(|target| {
        CheckItem::new(format!("{}|{}", target.block, target.id), format!("{}  {}", target.block, target.title))
            .checked(target.status == Status::SetUp)
            .trailing(target.status.label())
            .tone(match target.status {
                Status::SetUp => Tone::Success,
                Status::NotSetUp => Tone::Neutral,
                _ => Tone::Warning,
            })
    }).collect();
    tree.check_list(items).key("targets").all_toggle(false)
}

// Event::UiAction(action) if action.key == "targets" && action.kind == ActionKind::Activated:
//   split the row id at '|' and call the install service for that block and target.
```

**TypeScript**

```ts
import { checkList } from '@ilium/plugin-sdk/ui/build';

export function renderSetup(targets: Target[]) {
  return checkList(
    targets.map((target) => ({
      id: `${target.block}|${target.id}`,
      label: `${target.block}  ${target.title}`,
      checked: target.status === 'set-up',
      trailing: statusLabel(target.status),
      tone: target.status === 'set-up' ? 'success' : target.status === 'not-set-up' ? 'muted' : 'warning',
    })),
    { key: 'targets', allToggle: false },
  );
}

// case 'ui-action': when event.val.key === 'targets' and kind is 'activated',
// split the row id at '|' and call the install service for that block and target.
```

#### Choose the global file

Row "Global file" lets the user change the path. A `path-field` of kind `save-file` shows it. The value is saved after 600 ms, the usual rule for text inputs. A picked path outside the config scopes needs the pick grant, so the field uses the picker. R restores the default. See [pickers](/pages/widgets-pickers/) and [fs.pick](/pages/capability-fs-pick/).

**Rust**

```rust
use ilium_plugin_sdk::host::storage::{self, Bucket};
use ilium_plugin_sdk::types::ui::PathKind;

const DEFAULT_GLOBAL: &str = "~/.claude/CLAUDE.md";

fn render_global_file(&self, tree: &mut Tree) -> NodeHandle {
    tree.path_field(PathKind::SaveFile).key("global-file").value(&self.global_file)
}

fn on_path_changed(&mut self, value: String) -> Result<(), PluginError> {
    self.global_file = value.clone();
    storage::put(Bucket::Global, "global-file", value.as_bytes())?;
    Ok(())
}

fn restore_default(&mut self) -> Result<(), PluginError> {
    storage::remove(Bucket::Global, "global-file")?;
    self.global_file = DEFAULT_GLOBAL.to_string();
    Ok(())
}
```

**TypeScript**

```ts
import * as storage from '@ilium/plugin-sdk/host/storage';
import { pathField } from '@ilium/plugin-sdk/ui/build';

const DEFAULT_GLOBAL = '~/.claude/CLAUDE.md';

export const renderGlobalFile = (value: string) => pathField('save-file', { key: 'global-file', value });

export function onPathChanged(value: string): void {
  state.globalFile = value;
  storage.put('global', 'global-file', new TextEncoder().encode(value));
}

export function restoreDefault(): void {
  storage.remove('global', 'global-file');
  state.globalFile = DEFAULT_GLOBAL;
}
```

#### Refresh on attach with a durable timer

Global blocks are refreshed when a client attaches. A client may attach many times in a minute, so the plugin keeps one durable timer named `refresh`. A new attach moves the timer to two seconds from now. The timer survives a server restart. When it fires, the plugin runs one refresh pass, and a late timer still runs. See [durable timers](/pages/api-durable-timers/) and [clock and timers](/pages/api-clock-and-timers/).

**Rust**

```rust
use ilium_plugin_sdk::host::{clock, timers};

fn on_attach(&mut self) -> Result<(), PluginError> {
    timers::cancel_durable("refresh")?;
    timers::at(clock::now_unix_millis() + 2_000, "refresh")?;
    Ok(())
}

// Event::DurableTimerFired(fired) if fired.name == "refresh":
//   self.refresh_global_blocks();  // fired.late_by_millis may be large after a restart
```

**TypeScript**

```ts
import { nowUnixMillis } from '@ilium/plugin-sdk/host/clock';
import * as timers from '@ilium/plugin-sdk/host/timers';

export function onAttach(): void {
  timers.cancelDurable('refresh');
  timers.at(nowUnixMillis() + 2_000n, 'refresh');
}

// case 'durable-timer-fired': when event.val.name === 'refresh',
// run refreshGlobalBlocks(). event.val.lateByMillis may be large after a restart.
```

#### Owner side: the veto hook before an install

`before-install` is a veto hook with high risk. The plugin runs it before every write or proposal with the target, the block and the path. A handler may refuse, for example a policy plugin that forbids edits to a shared team file. A refusal completes the request as `refused(conflict)` whose message starts with the prefix `extension.vetoed`. The plugin then shows the reason and writes nothing. See [events](/pages/api-events/) and [requests and outcomes](/pages/requests-and-outcomes/).

**Rust**

```rust
use ilium_plugin_sdk::host::extensions;
use ilium_plugin_sdk::types::requests::{Outcome, RequestResult};

fn ask_before_install(&mut self, target: &str, block: &str, path: &str) -> Result<(), PluginError> {
    let payload = serde_json::json!({ "target": target, "block": block, "path": path }).to_string();
    let request = extensions::run_hook_request("ilium.core.agent-setup/before-install", &payload)?;
    self.pending.insert(request, (target.to_string(), block.to_string()));
    Ok(())
}

fn on_hook_done(&mut self, done: RequestResult) -> Vec<Submission> {
    let Some((target, block)) = self.pending.remove(&done.request) else { return Vec::new() };
    match done.outcome {
        Outcome::Ok => self.write_block(&target, &block),
        Outcome::Refused(refusal) if refusal.code == "conflict" && refusal.message.starts_with("extension.vetoed") => {
            self.show_notice(format!("Install stopped: {}", refusal.message)); // no write happens
            Vec::new()
        }
        _ => {
            self.show_notice("Install stopped: the check did not finish".into());
            Vec::new()
        }
    }
}
```

**TypeScript**

```ts
import * as extensions from '@ilium/plugin-sdk/host/extensions';

export function askBeforeInstall(target: string, block: string, path: string): void {
  const request = extensions.runHookRequest('ilium.core.agent-setup/before-install',
    JSON.stringify({ target, block, path }));
  pending.set(request, { target, block });
}

export function onHookDone(request: bigint, outcome: { tag: string; val?: { code: string; message: string } }) {
  const item = pending.get(request);
  if (item === undefined) return [];
  pending.delete(request);
  if (outcome.tag === 'ok') return writeBlock(item.target, item.block);
  if (outcome.tag === 'refused' && outcome.val?.code === 'conflict' && outcome.val?.message?.startsWith('extension.vetoed')) {
    showNotice(`Install stopped: ${outcome.val.message}`); // no write happens
    return [];
  }
  showNotice('Install stopped: the check did not finish');
  return [];
}
```

#### Owner side: blocks and targets from other plugins

`instruction-blocks` and `targets` are collect points. A block entry names a marker and a file under `help/` that holds the text, so another plugin can add a third block, for example `review-rules`. A target entry names a path template, such as `{project}/GEMINI.md`, and a scope. The plugin lists both, builds its rows from them, and lists again when the kernel publishes `ilium.extensions/changed`. See [events](/pages/api-events/) and [messages and topics](/pages/api-messages-and-topics/).

**Rust**

```rust
use ilium_plugin_sdk::host::extensions;

#[derive(serde::Deserialize)]
struct BlockEntry { id: String, title: String, marker: String, file: String }
#[derive(serde::Deserialize)]
struct TargetEntry { id: String, title: String, path_template: String, scope: String }

fn reload(&mut self) {
    self.blocks = collect::<BlockEntry>("ilium.core.agent-setup/instruction-blocks");
    self.targets = collect::<TargetEntry>("ilium.core.agent-setup/targets");
}

fn collect<T: serde::de::DeserializeOwned>(point: &str) -> Vec<T> {
    extensions::list(point)
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| serde_json::from_str(&entry.entry_json).ok())
        .collect()
}

// Event::TopicPublished(t) if t.topic == "ilium.extensions/changed": self.reload()
```

**TypeScript**

```ts
import * as extensions from '@ilium/plugin-sdk/host/extensions';

function collect<T>(point: string): T[] {
  try {
    return extensions.list(point).map((entry) => JSON.parse(entry.entryJson) as T);
  } catch {
    return [];
  }
}

export function reload(): void {
  blocks = collect<{ id: string; title: string; marker: string; file: string }>(
    'ilium.core.agent-setup/instruction-blocks');
  targets = collect<{ id: string; title: string; pathTemplate: string; scope: string }>(
    'ilium.core.agent-setup/targets');
}

// case 'topic-published': when event.val.topic === 'ilium.extensions/changed', call reload().
```


### Extension points

Other plugins change what `ilium.core.agent-setup` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.agent-setup/<point>`). Contributors need `extensions.contribute:ilium.core.agent-setup/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `instruction-blocks` | collect | Add a marker-delimited instruction block that Setup installs. | entry: id, title, marker, file (text under the plugin's `help/` folder) | high |
| `targets` | collect | Add a file that Setup can edit, such as a new agent config file. A global target is changed through a proposal, so it must lie in an `agent-config:<agent>` scope this plugin can propose for. | entry: id, title, path_template, scope (global, project) | high |
| `before-install` | hook (veto) | Refuse writing a block to a file. | target, block, path | high |

**Services** (grant `services.use:<name>`):

- `ilium.core.agent-setup/install`: install, remove, status: manage blocks

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.agent-setup"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Installs a block of review rules next to the Chatroom and Progress blocks

A package adds this entry in its manifest. It needs no code.

```json
{
  "api": "ilium:plugin@0.1.0",
  "capabilities": { "server": ["extensions.contribute:ilium.core.agent-setup/instruction-blocks"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.agent-setup/instruction-blocks",
        "id": "review-rules",
        "priority": 50,
        "entry": {
          "title": "Review rules",
          "marker": "review-rules",
          "file": "help/review-rules.md"
        }
      }
    ]
  }
}
```

### ABI used

- [fs](/pages/api-fs/), [atomic write and revisions](/pages/api-fs-atomic-and-tail/) and [file access](/pages/file-access/)
- [fs.read](/pages/capability-fs-read/), [fs.write](/pages/capability-fs-write/) and [fs.pick](/pages/capability-fs-pick/)
- [Storage](/pages/api-storage/)
- [Durable timers](/pages/api-durable-timers/) and [clock and timers](/pages/api-clock-and-timers/)
- [Collections](/pages/widgets-collections/) and [pickers](/pages/widgets-pickers/)
- [Help](/pages/api-help-and-plugins/) and [help.read](/pages/capability-help-read/)
- [Events](/pages/api-events/) and [requests and outcomes](/pages/requests-and-outcomes/)
- [Messages and topics](/pages/api-messages-and-topics/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Settings](/pages/core-plugin-settings/)
- [Reference: capabilities](/pages/reference-capabilities/)

### Security

Risk marks of this plugin's extension points follow the corrected table in [Extension security](/pages/reference-extension-security/) ([SEC-293]).
