---
layout: page
title: Terminal
permalink: /pages/core-plugin-terminal/
---

## Terminal

The Terminal plugin (`ilium.core.terminal`, side both) owns terminal panes as plugins see them: pane processes and their signals, screen reads, scrollback history, typed and pasted input, clickable links, pane titles from the terminal and screen transfer between panes. Emulation stays in the kernel. The plugin uses the [terminal primitive](/pages/primitives-terminal/) to read and write, and embeds a live pane with the `terminal-view` widget.

### What it covers

- Pane processes: each terminal pane runs one program (Panes and layout, Terminal panes).
- Terminal emulation and screen state (How it works, Client and server).
- The terminal view, scrolling, history and the scrollback budget (Panes and layout, Scrolling and history; Settings, Terminal).
- Input and paste, and the literal prefix (Getting started, Keys while a pane is focused).
- Clickable links in terminal output (Smart Copy, What Smart Copy recognises).
- Screen transfer between panes (Smart Copy, Screen transfer between panes).
- Pane titles set by the terminal with OSC sequences (Titles and instructions, When a title is applied).

### See it

<figure><img src="/assets/demos/05-screen-transfer.gif" alt="The screen of one terminal pane is pasted into another pane" loading="lazy"><figcaption>Screen transfer. The visible screen and history of the source pane arrive in the target pane as one paste.</figcaption></figure>

### What the user sees

<figure>
<pre class="term">╭─ api ────────────────────────────────────────╮
│<span class="t-dim">$ cargo test</span>                                  │
│<span class="t-err">error[E0432]</span>: unresolved import               │
│  <span class="t-dim">--&gt;</span> <span class="t-hi">src/server/routes.rs:42</span>                 │
│                                              │
│<span class="t-dim">  open in editor</span>                              │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A terminal pane with a path link, shown highlighted, and the hover hint <code>open in editor</code>.</figcaption>
</figure>

<figure>
<pre class="term">╭─ api ─────────────╮╭─ Transfer ────────────╮╭─ notes ───────────╮
│<span class="t-dim">$ cargo test</span>       ││ <span class="t-hi">Transfer 42 lines</span>     ││<span class="t-dim">$ cat &gt; log.txt</span>    │
│<span class="t-ok">test result: ok</span>    ││ <span class="t-dim">to: notes</span>            ││<span class="t-ok">running 9 tests</span>     │
│<span class="t-dim">$</span>                 ││ <span class="t-ok">[Enter]</span> <span class="t-dim">send  Esc</span>    ││<span class="t-ok">test result: ok</span>      │
╰───────────────────╯╰───────────────────────╯╰───────────────────╯
</pre>
<figcaption>Rendered at 71 columns. Illustrative. Screen transfer: the source pane on the left, the dialog <code>Transfer 42 lines</code> in the middle and the target pane receiving the paste on the right.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.terminal",
  "name": "Terminal",
  "version": "0.1.0",
  "publisher": "Ilium",
  "capabilities": {
    "server": [
      "terminal.read-screen", "terminal.read-cells", "terminal.history", "terminal.write",
      "panes.read", "panes.signal", "tree.write",
      "extensions.provide:ilium.core.terminal/before-send-input"
    ],
    "client": [
      "terminal.read-screen", "editor.edit",
      "commands.invoke:ilium.core.editor/*", "ui.panels", "settings.write-kernel:terminal",
      "extensions.provide:ilium.core.terminal/open-link",
      "extensions.provide:ilium.core.terminal/link-actions"
    ]
  },
  "contributes": {
    "link_detectors": [
      { "id": "path-line", "regex": "[./A-Za-z0-9_-]+\\.[a-z]+:[0-9]+", "command": "link-open" }
    ],
    "extension_points": [
      {
        "id": "before-send-input",
        "kind": "hook",
        "mode": "veto",
        "title": "Before input is sent to a terminal pane",
        "payload_schema": {
          "type": "object",
          "properties": {
            "pane": {
              "type": "string"
            },
            "length": {
              "type": "integer"
            },
            "source": {
              "type": "string"
            },
            "preview": {
              "type": "string"
            }
          },
          "required": [
            "pane",
            "length",
            "source",
            "preview"
          ]
        },
        "risk": "high",
        "deadline_ms": 20,
        "max_handlers": 8,
        "version": "0.1.0"
      },
      {
        "id": "open-link",
        "kind": "hook",
        "mode": "first",
        "title": "How a clicked link opens",
        "payload_schema": {
          "type": "object",
          "properties": {
            "pane": {
              "type": "string"
            },
            "link_kind": {
              "type": "string"
            },
            "text": {
              "type": "string"
            }
          },
          "required": [
            "pane",
            "link_kind",
            "text"
          ]
        },
        "reply_schema": {
          "type": "object",
          "properties": {
            "action": {
              "type": "string",
              "enum": [
                "open-external",
                "run-command",
                "ignore"
              ]
            },
            "command": {
              "type": "string"
            }
          },
          "required": [
            "action"
          ]
        },
        "risk": "high",
        "deadline_ms": 20,
        "max_handlers": 8,
        "version": "0.1.0"
      },
      {
        "id": "link-actions",
        "kind": "collect",
        "title": "Actions for a detected link in the right-click menu",
        "entry_schema": {
          "type": "object",
          "properties": {
            "id": {
              "type": "string"
            },
            "link_kind": {
              "type": "string"
            },
            "title": {
              "type": "string"
            },
            "command": {
              "type": "string"
            }
          },
          "required": [
            "id",
            "link_kind",
            "title",
            "command"
          ]
        },
        "risk": "standard",
        "max_entries": 64,
        "dynamic": false,
        "version": "0.1.0"
      }
    ]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.terminal",
  "name": "Terminal",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "The Terminal plugin (ilium.core.terminal, side both) owns terminal panes as plugins see them: pane processes and signals, screen reads, scrollback history, input, clickable links, pane titles and screen transfer.",
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
      "terminal.read-screen",
      "terminal.read-cells",
      "terminal.history",
      "terminal.write",
      "panes.read",
      "panes.signal",
      "tree.write",
      "extensions.provide:ilium.core.terminal/before-send-input"
    ],
    "client": [
      "terminal.read-screen",
      "editor.edit",
      "commands.invoke:ilium.core.editor/*",
      "ui.panels",
      "settings.write-kernel:terminal",
      "extensions.provide:ilium.core.terminal/open-link",
      "extensions.provide:ilium.core.terminal/link-actions"
    ]
  },
  "activation": [
    "event:screen-changed",
    "startup"
  ],
  "contributes": {
    "link_detectors": [
      {
        "id": "path-line",
        "regex": "[./A-Za-z0-9_-]+\\.[a-z]+:[0-9]+",
        "command": "link-open"
      }
    ],
    "extension_points": [
      {
        "id": "before-send-input",
        "kind": "hook",
        "mode": "veto",
        "title": "Before input is sent to a terminal pane",
        "payload_schema": {
          "type": "object",
          "properties": {
            "pane": {
              "type": "string"
            },
            "length": {
              "type": "integer"
            },
            "source": {
              "type": "string"
            },
            "preview": {
              "type": "string"
            }
          },
          "required": [
            "pane",
            "length",
            "source",
            "preview"
          ]
        },
        "risk": "high",
        "deadline_ms": 20,
        "max_handlers": 8,
        "version": "0.1.0"
      },
      {
        "id": "open-link",
        "kind": "hook",
        "mode": "first",
        "title": "How a clicked link opens",
        "payload_schema": {
          "type": "object",
          "properties": {
            "pane": {
              "type": "string"
            },
            "link_kind": {
              "type": "string"
            },
            "text": {
              "type": "string"
            }
          },
          "required": [
            "pane",
            "link_kind",
            "text"
          ]
        },
        "reply_schema": {
          "type": "object",
          "properties": {
            "action": {
              "type": "string",
              "enum": [
                "open-external",
                "run-command",
                "ignore"
              ]
            },
            "command": {
              "type": "string"
            }
          },
          "required": [
            "action"
          ]
        },
        "risk": "high",
        "deadline_ms": 20,
        "max_handlers": 8,
        "version": "0.1.0"
      },
      {
        "id": "link-actions",
        "kind": "collect",
        "title": "Actions for a detected link in the right-click menu",
        "entry_schema": {
          "type": "object",
          "properties": {
            "id": {
              "type": "string"
            },
            "link_kind": {
              "type": "string"
            },
            "title": {
              "type": "string"
            },
            "command": {
              "type": "string"
            }
          },
          "required": [
            "id",
            "link_kind",
            "title",
            "command"
          ]
        },
        "risk": "standard",
        "max_entries": 64,
        "dynamic": false,
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
| `terminal` | Terminal | `[terminal]` | Smart copy |

If the plugin is disabled, the tab stays in the rail with its stored values shown read-only and the notice "Terminal is disabled. Values are kept." The kernel keeps reading the keys it consumes itself (listed in the Consumer column of the key table in [config and data migration](/pages/reference-config-and-data-migration/)). Behaviour that only this plugin provides is off. The exact result for this plugin is its row in [kernel and boot](/pages/reference-kernel-and-boot/). [COR-022]

### Samples

#### Clickable links

Paths and URLs in terminal output become links that open the editor. The manifest declares a `link_detectors` entry for paths with a `:line` suffix. On `link-activated` the client submits `editor-open` with the path, the line and `placement: split`. Pages: [terminal primitive](/pages/primitives-terminal/), [commands.invoke](/pages/capability-commands-invoke/), [events](/pages/api-events/) and [intents](/pages/api-intents/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{EditorOpenSpec, EditorPlacement};
use ilium_plugin_sdk::types::paths::FsPath;

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct Links;

impl Lifecycle for Links {}

impl Handler for Links {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::LinkActivated(link) = event else { continue };
            if link.detector_id != "path-line" { continue; }
            let Some((path, line)) = link.text.rsplit_once(':') else { continue };
            let Ok(line) = line.parse::<u32>() else { continue };
            submissions.push(Submission::new(Intent::EditorOpen(EditorOpenSpec {
                path: FsPath { scope: "project".into(), path: path.into() },
                line: Some(line.saturating_sub(1)),
                placement: EditorPlacement::Split,
            })));
        }
        submissions
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';

let next = 1n;

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'link-activated' || event.val.detectorId !== 'path-line') continue;
      const cut = event.val.text.lastIndexOf(':');
      const line = Number(event.val.text.slice(cut + 1));
      if (cut < 0 || !Number.isInteger(line)) continue;
      submissions.push({
        intent: { tag: 'editor-open', val: {
          path: { scope: 'project', path: event.val.text.slice(0, cut) },
          line: Math.max(line - 1, 0),
          placement: 'split',
        } },
        correlation: next++,
        dedupKey: undefined,
      });
    }
    return submissions;
  },
});
```

#### Pane title from the terminal

OSC titles rename the pane unless the user named it. On `pane-title-changed` with `source: osc`, the server submits `rename-node` with `source: inferred`, so a manual name always wins. Pages: [terminal primitive](/pages/primitives-terminal/), [panes.read](/pages/capability-panes-read/), [events](/pages/api-events/) and [intents](/pages/api-intents/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::tree::{TitleOrigin, TitleSource};
use ilium_plugin_sdk::types::intents::RenameNodeSpec;

fn on_title(event: Event) -> Option<Submission> {
    let Event::PaneTitleChanged(change) = event else { return None };
    if change.source != TitleOrigin::Osc { return None; }
    Some(Submission::keyed(
        Intent::RenameNode(RenameNodeSpec {
            node: change.node,
            title: change.title,
            source: TitleSource::Inferred,
            expected_revision: None,
        }),
        format!("osc-title-{}", change.node),
    ))
}
```

**TypeScript**

```ts
import type { Event, Submission } from '@ilium/plugin-sdk/types';

function onTitle(event: Event): Submission | undefined {
  if (event.tag !== 'pane-title-changed' || event.val.source !== 'osc') return undefined;
  return {
    intent: { tag: 'rename-node', val: {
      node: event.val.node, title: event.val.title, source: 'inferred', expectedRevision: undefined,
    } },
    correlation: 1n,
    dedupKey: `osc-title-${event.val.node}`,
  };
}
```

#### Screen transfer between panes

The visible screen and the history of one pane are pasted into another. The server reads rows with `terminal.read-screen` and asks for the history, trims trailing blank rows, then submits `send-input` with `delivery: pasted` to the target. `history-text` answers later through `take-history`. Pages: [terminal reads](/pages/primitives-terminal-reads/), [read-screen](/pages/capability-terminal-read-screen/), [history](/pages/capability-terminal-history/), [terminal input](/pages/primitives-terminal-input/), [terminal.write](/pages/capability-terminal-write/), [events](/pages/api-events/) and [send-input](/pages/api-intent-send-input/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::terminal::{self, ScreenPart};
use ilium_plugin_sdk::types::intents::{Delivery, SendInputSpec};

fn transfer(source: NodeId, target: NodeId) -> Option<Submission> {
    let screen = terminal::read_screen(source, ScreenPart::Visible, 500)?;
    let text = screen.text.trim_end().to_string(); // drop trailing blank rows
    if text.is_empty() { return None; }
    Some(Submission::keyed(
        Intent::SendInput(SendInputSpec {
            pane: target,
            text,
            delivery: Delivery::Pasted,
            prompt_epoch: None,
            stages: vec![],
            raw: None,
            mouse: None,
            if_current: None,
            lock_user_input: false,
            journal: false,
        }),
        format!("transfer-{source}-{target}-{}", screen.revision),
    ))
}
```

**TypeScript**

```ts
import { readScreen } from '@ilium/plugin-sdk/primitives/terminal';
import type { Submission } from '@ilium/plugin-sdk/types';

function transfer(source: bigint, target: bigint): Submission | undefined {
  const screen = readScreen(source, 'visible', 500);
  if (screen === undefined) return undefined;
  const text = screen.text.trimEnd(); // drop trailing blank rows
  if (text === '') return undefined;
  return {
    intent: { tag: 'send-input', val: {
      pane: target, text, delivery: 'pasted', promptEpoch: undefined,
      stages: [], raw: undefined, mouse: undefined, ifCurrent: undefined,
      lockUserInput: false, journal: false,
    } },
    correlation: 1n,
    dedupKey: `transfer-${source}-${target}-${screen.revision}`,
  };
}
```

#### Live terminal in a panel

A client panel embeds a pane with its own scrollback. It renders a `terminal-view` of the selected pane inside a `block`, and the host handles scrolling and input. Pages: [media and embedded widgets](/pages/widgets-media-and-embedded/), [ui.panels](/pages/capability-ui-panels/) and [layout and containers](/pages/widgets-layout-and-containers/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::layout::Area;
use ilium_plugin_sdk::types::ui::{UiTree, ViewTarget};
use ilium_plugin_sdk::ui::build::Tree;

impl Views for Links {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let live = tree.terminal_view(self.selected_pane).rows(12).key("live");
        let root = tree.block().title("Build output").child(live);
        tree.finish(root)
    }
}
```

**TypeScript**

```ts
import { block, terminalView, tree } from '@ilium/plugin-sdk/ui/build';

function render(selectedPane: bigint) {
  const live = terminalView(selectedPane, { key: 'live', rows: 12 });
  return tree(block({ title: 'Build output' }, [live]));
}
```

#### Interrupt and watch exits

Ctrl+C style interrupts and exit codes of pane processes. The server submits `signal-pane` with `interrupt`, then `terminate` after a timer, and logs the status and signal from `pane-exited`. Pages: [panes.signal](/pages/capability-panes-signal/), [panes](/pages/api-panes/), [intents](/pages/api-intents/) and [events](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::tree::Signal;
use ilium_plugin_sdk::types::intents::SignalPaneSpec;
use ilium_plugin_sdk::host::log::{self, Level};

fn interrupt(pane: NodeId) -> Submission {
    Submission::new(Intent::SignalPane(SignalPaneSpec { pane, signal: Signal::Interrupt }))
}

// Run from a timer 3 s later if the pane has not exited.
fn terminate(pane: NodeId) -> Submission {
    Submission::new(Intent::SignalPane(SignalPaneSpec { pane, signal: Signal::Terminate }))
}

fn on_exit(event: Event) {
    if let Event::PaneExited(exit) = event {
        log::log(Level::Info, &format!("pane {} exited status={:?} signal={:?}", exit.node, exit.status, exit.signal));
    }
}
```

**TypeScript**

```ts
import { log } from '@ilium/plugin-sdk/host/log';
import type { Event, Submission } from '@ilium/plugin-sdk/types';

const interrupt = (pane: bigint): Submission => ({
  intent: { tag: 'signal-pane', val: { pane, signal: 'interrupt' } },
  correlation: 1n,
  dedupKey: undefined,
});

// Run from a timer 3 s later if the pane has not exited.
const terminate = (pane: bigint): Submission => ({
  intent: { tag: 'signal-pane', val: { pane, signal: 'terminate' } },
  correlation: 2n,
  dedupKey: undefined,
});

function onExit(event: Event): void {
  if (event.tag === 'pane-exited') {
    log('info', `pane ${event.val.node} exited status=${event.val.status} signal=${event.val.signal}`);
  }
}
```

#### Visible rows that changed

`screen-changed` names the first and last changed row of a pane, at most ten times a second. The plugin reads only those rows when it looks for a prompt, so it does not re-read the whole screen.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::terminal::{self, ScreenPart};

impl Handler for PromptWatch {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if let Event::ScreenChanged(change) = event {
                let Some(screen) = terminal::read_screen(change.pane, ScreenPart::Visible, 200) else { continue };
                let changed = &screen.rows[change.first_row as usize..=change.last_row as usize];
                if changed.iter().any(|row| row.ends_with("$ ")) {
                    self.prompt_seen.insert(change.pane);
                }
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
export function onEvents(events: Event[]): Submission[] {
  for (const event of events) {
    if (event.tag !== 'screen-changed') continue;
    const { pane, firstRow, lastRow } = event.val;
    const screen = terminal.readScreen(pane, 'visible', 200);
    const changed = screen.rows.slice(firstRow, lastRow + 1);
    if (changed.some((row) => row.endsWith('$ '))) promptSeen.add(pane);
  }
  return [];
}
```

### Extension points

Other plugins change what `ilium.core.terminal` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.terminal/<point>`). Contributors need `extensions.contribute:ilium.core.terminal/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `before-send-input` | hook (veto) | Refuse or end input that the user or a plugin sends to a terminal pane. | pane, length, source (user, paste, plugin:<id>), preview (first 120 characters) | high |
| `open-link` | hook (first) | Decide how a clicked link opens. The first handler that answers wins. | pane, link_kind, text; reply: action (open-external, run-command, ignore) and optional command | high |
| `link-actions` | collect | Offer actions for a detected link kind in the terminal right-click menu. | entry: id, link_kind, title, command | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.terminal/transfer`: send-screen: copy the visible screen of one pane to another pane

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.terminal/screen-changed`: { pane, first-row, last-row, revision } when visible rows change, at most 10 per second

Link detectors from `contributes.link_detectors` feed the `link-actions` point.

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.terminal"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Adds `Open ticket` to the right-click menu on a link of kind `ticket`

A package adds this entry in its manifest. It needs no code.

```json
{
  "api": "ilium:plugin@0.1.0",
  "capabilities": { "server": ["extensions.contribute:ilium.core.terminal/link-actions"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.terminal/link-actions",
        "id": "open-ticket",
        "priority": 50,
        "entry": {
          "link_kind": "ticket",
          "title": "Open ticket",
          "command": "open-ticket"
        }
      }
    ]
  }
}
```

### Extension points this plugin owns

The table gives the values that [EXT-052] requires for each point this plugin owns. The page does not state them, so the values come from the defaults in [Extension points](/pages/api-extension-points/) and are marked (default). The main manifest grants `extensions.provide:ilium.core.terminal/<point>` on the side shown in the table (`before-send-input` on the server, `open-link` and `link-actions` on the client) and declares each point in `contributes.extension_points`.

| Point | Side | Version | Deadline | Max | Dynamic |
|---|---|---|---|---|---|
| `ilium.core.terminal/before-send-input` | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) |
| `ilium.core.terminal/open-link` | client | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) |
| `ilium.core.terminal/link-actions` | client | 0.1.0 (default) | none (default) | 64 entries (default) | no (default) |

Side basis: `before-send-input` is server because input reaches a pane through the `send-input` intent that the server submits (the paste sample) and the server holds `terminal.write`. `open-link` is client because the click is handled on the client, which submits `editor-open` on `link-activated` (the Clickable links sample). `link-actions` is client because the point feeds the terminal right-click menu, which is client UI.

### ABI used

- [Terminal primitive](/pages/primitives-terminal/), [Terminal reads](/pages/primitives-terminal-reads/), [Terminal input](/pages/primitives-terminal-input/)
- [terminal.read-screen](/pages/capability-terminal-read-screen/), [terminal.history](/pages/capability-terminal-history/), [terminal.write](/pages/capability-terminal-write/), [panes.read](/pages/capability-panes-read/), [panes.signal](/pages/capability-panes-signal/), [commands.invoke](/pages/capability-commands-invoke/)
- [send-input](/pages/api-intent-send-input/), [Intents](/pages/api-intents/), [Events](/pages/api-events/), [Panes](/pages/api-panes/)
- [Media and embedded widgets](/pages/widgets-media-and-embedded/), [Layout and containers](/pages/widgets-layout-and-containers/), [ui.panels](/pages/capability-ui-panels/)
- [Terminal read cells](/pages/capability-terminal-read-cells/), [Terminal read screen](/pages/capability-terminal-read-screen/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Smart Copy plugin](/pages/core-plugin-smart-copy/)
- [Editor plugin](/pages/core-plugin-editor/)

### Security

Risk marks of this plugin's extension points follow the corrected table in [Extension security](/pages/reference-extension-security/) ([SEC-293]).
