# Smart Copy

## Smart Copy

The Smart Copy plugin (`ilium.core.smart-copy`, side both) owns selection and copying in terminal panes: mouse selection of regions, the terminal right-click menu, Smart Copy light (paths, commands and hashes recognised from cell styles) and Smart Copy with the inference model choosing semantic regions.

### What it covers

- Text selection and the terminal right-click menu (Smart Copy, Selecting regions and Terminal right-click menu).
- Smart Copy light, which recognises paths, commands and hashes without a model (Smart Copy, Smart Copy light).
- Smart Copy (AI), where the inference model labels the regions worth copying (Smart Copy, Smart Copy (AI)).

### See it

<figure><img src="/assets/demos/04-smart-copy.gif" alt="A region of terminal output is selected and copied with Smart Copy" loading="lazy"><figcaption>Smart Copy in a terminal pane. The right-click menu offers the recognised regions.</figcaption></figure>

### What the user sees

<figure>
<pre class="term">╭─ api ────────────────────────────────────────╮
│<span class="t-dim">$ git status</span>                                  │
│<span class="t-hi">  modified: src/lib.rs</span>                        │
│        ╭───────────────────╮                 │
│        │ Copy              │                 │
│        │ Copy path         │                 │
│        │<span class="t-hi"> Smart Copy       ▸</span>│                 │
│        │ Transfer screen   │                 │
│        ╰───────────────────╯                 │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The right-click menu over a terminal: Copy, Copy path, Smart Copy and Transfer screen.</figcaption>
</figure>

<figure>
<pre class="term">╭─ Smart Copy ─────────────────────────────────╮
│ <span class="t-hi">1</span> src/server/routes.rs                       │
│ <span class="t-hi">2</span> cargo test -p ilium-server                 │
│ <span class="t-hi">3</span> 9f3c2ab                                    │
│                                              │
│ <span class="t-dim">1-3 copy   Enter copy   Esc close</span>            │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The Smart Copy picker popup lists three recognised regions with key hints.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.smart-copy",
  "name": "Smart Copy",
  "version": "0.1.0",
  "publisher": "Ilium",
  "capabilities": {
    "server": ["terminal.read-cells", "terminal.read-screen", "inference"],
    "client": ["terminal.read-screen", "terminal.pointer", "clipboard.write", "inference"]
  },
  "contributes": {
    "context_menu": [
      { "id": "smart-copy", "title": "Smart Copy", "command": "smart-copy", "target": "terminal" }
    ]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.smart-copy",
  "name": "Smart copy",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "The Smart Copy plugin (ilium.core.smart-copy, side both) owns selection and copying in terminal panes: mouse selection of regions, the terminal right-click menu, Smart Copy light (paths, commands and hashes recognised from cell styles) and ",
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
      "terminal.read-cells",
      "terminal.read-screen",
      "inference"
    ],
    "client": [
      "terminal.read-screen",
      "terminal.pointer",
      "clipboard.write",
      "inference"
    ]
  },
  "activation": [
    "service:copy",
    "command:smart-copy"
  ],
  "contributes": {
    "context_menu": [
      {
        "id": "smart-copy",
        "title": "Smart Copy",
        "command": "smart-copy",
        "target": "terminal"
      }
    ]
  },
  "limits_tier": "first-party"
}
```

### Samples

#### Terminal right-click menu

A context menu is built when it opens, with entries that depend on the selection. The manifest declares a `context_menu` for target `terminal`, and the `menu-provider` export answers `menu-items`. Pages: [menus](/pages/api-menus/), [menu provider](/pages/api-menu-provider/), [collections](/pages/widgets-collections/) and [events](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::exports::MenuProvider;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::terminal;
use ilium_plugin_sdk::exports::menu_provider::MenuContext;
use ilium_plugin_sdk::types::ui::MenuEntry;

#[ilium_plugin(side = "client", exports = ["menu-provider"])]
#[derive(Default)]
struct SmartCopy;

impl Lifecycle for SmartCopy {}
impl Handler for SmartCopy {}

impl MenuProvider for SmartCopy {
    fn menu_items(&mut self, context: MenuContext) -> Vec<MenuEntry> {
        let Some(pane) = context.pane else { return Vec::new() };
        let selected = terminal::selection(pane).is_some_and(|s| !s.text.is_empty());
        vec![
            MenuEntry::new("copy", "Copy")
                .disabled_reason(if selected { None } else { Some("nothing selected") }),
            MenuEntry::new("copy-path", "Copy path"),
            MenuEntry::new("smart-copy", "Smart Copy").shortcut("ctrl+shift+c"),
            MenuEntry::new("transfer", "Transfer screen").separator_before(true),
        ]
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import * as terminal from '@ilium/plugin-sdk/primitives/terminal';
import type { MenuContext, MenuEntry } from '@ilium/plugin-sdk/types';

export const { lifecycle, handler, menuProvider } = definePlugin({
  side: 'client',
  onEvents() { return []; },
  menuItems(context: MenuContext): MenuEntry[] {
    if (context.pane === undefined) return [];
    const selected = (terminal.selection(context.pane)?.text ?? '') !== '';
    return [
      { id: 'copy', label: 'Copy', disabledReason: selected ? undefined : 'nothing selected' },
      { id: 'copy-path', label: 'Copy path' },
      { id: 'smart-copy', label: 'Smart Copy', shortcut: 'ctrl+shift+c' },
      { id: 'transfer', label: 'Transfer screen', separatorBefore: true },
    ];
  },
});
```

#### Select regions with the mouse

Drag selects a region and double click selects a word. The client claims the pointer, tracks `down`, `drag` and `up` in `pointer` events and keeps the selected rows and columns. Pages: [terminal pointer](/pages/primitives-terminal-pointer/), [terminal.pointer](/pages/capability-terminal-pointer/) and [events](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::terminal::{self, PointerKind};

#[derive(Default)]
struct Region { start: Option<(u32, u32)>, end: (u32, u32) }

impl Handler for SmartCopy {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::Pointer(pointer) = event else { continue };
            match pointer.kind {
                PointerKind::Down => {
                    self.region.start = Some((pointer.row, pointer.column));
                    self.region.end = (pointer.row, pointer.column);
                }
                PointerKind::Drag => self.region.end = (pointer.row, pointer.column),
                PointerKind::Up => { /* region is final: keep rows and columns */ }
                _ => {}
            }
        }
        Vec::new()
    }
}

fn start(pane: NodeId) -> Result<(), PluginError> {
    terminal::claim_pointer(pane)
}
```

**TypeScript**

```ts
import { claimPointer } from '@ilium/plugin-sdk/primitives/terminal';
import type { Event } from '@ilium/plugin-sdk/types';

let start: [number, number] | undefined;
let end: [number, number] = [0, 0];

export const startSelecting = (pane: bigint): void => claimPointer(pane);

export function onPointer(event: Event): void {
  if (event.tag !== 'pointer') return;
  const here: [number, number] = [event.val.row, event.val.column];
  if (event.val.kind === 'down') { start = here; end = here; }
  else if (event.val.kind === 'drag') end = here;
  // 'up': the region is final; keep start and end.
}
```

#### Smart Copy light

The server recognises paths, commands and hashes from text and cell styles and sends the candidates to the client side. It reads styled cells with `terminal.read-cells`, matches the patterns and sends a `message`. Pages: [read-cells](/pages/capability-terminal-read-cells/), [terminal reads](/pages/primitives-terminal-reads/), [worlds and sides](/pages/worlds-and-sides/), [events](/pages/api-events/) and [messages and topics](/pages/api-messages-and-topics/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::terminal::{self, ScreenPart};
use ilium_plugin_sdk::types::intents::{MessageSpec, MessageTarget, PluginSide};

fn candidates(pane: NodeId, first_row: u32, count: u32) -> Option<Submission> {
    let rows = terminal::read_cells(pane, ScreenPart::Visible, first_row, count)?;
    let mut found: Vec<String> = Vec::new();
    for row in rows.rows.iter().map(|cells| cells.iter().map(|cell| cell.text.as_str()).collect::<String>()) {
        for word in row.split_whitespace() {
            let is_path = word.contains('/') && word.contains('.');
            let is_hash = word.len() >= 7 && word.chars().all(|c| c.is_ascii_hexdigit());
            if is_path || is_hash { found.push(word.to_string()); }
        }
    }
    Some(Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget { plugin: "ilium.core.smart-copy".into(), side: PluginSide::Client, client: None },
        channel: "candidates".into(),
        payload: found.join("\n").into_bytes(),
    })))
}
```

**TypeScript**

```ts
import { readCells } from '@ilium/plugin-sdk/primitives/terminal';
import type { Submission } from '@ilium/plugin-sdk/types';

function candidates(pane: bigint, firstRow: number, count: number): Submission | undefined {
  const rows = readCells(pane, 'visible', firstRow, count);
  if (rows === undefined) return undefined;
  const found: string[] = [];
  for (const row of rows.rows.map((cells) => cells.map((cell) => cell.text).join(''))) {
    for (const word of row.split(/\s+/)) {
      const isPath = word.includes('/') && word.includes('.');
      const isHash = word.length >= 7 && /^[0-9a-f]+$/i.test(word);
      if (isPath || isHash) found.push(word);
    }
  }
  return {
    intent: { tag: 'message', val: {
      to: { plugin: 'ilium.core.smart-copy', side: 'client', client: undefined },
      channel: 'candidates',
      payload: new TextEncoder().encode(found.join('\n')),
    } },
    correlation: 1n,
    dedupKey: undefined,
  };
}
```

#### Copy to the clipboard

The chosen region lands on the system clipboard. On the candidate `message` the client calls `clipboard.write-text` with the chosen region and reports a short status line. Pages: [clipboard](/pages/api-clipboard/), [clipboard.write](/pages/capability-clipboard/) and [events](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::clipboard;

fn on_candidates(&mut self, payload: &[u8], chosen: usize) {
    let text = String::from_utf8_lossy(payload);
    let Some(region) = text.lines().nth(chosen) else { return };
    match clipboard::write_text(region) {
        Ok(()) => self.status = format!("Copied {} characters", region.chars().count()),
        Err(error) => self.status = format!("Copy failed: {:?}", error.code),
    }
}
```

**TypeScript**

```ts
import * as clipboard from '@ilium/plugin-sdk/host/clipboard';

let status = '';

function onCandidates(payload: Uint8Array, chosen: number): void {
  const region = new TextDecoder().decode(payload).split('\n')[chosen];
  if (region === undefined) return;
  try {
    clipboard.writeText(region);
    status = `Copied ${[...region].length} characters`;
  } catch (error) {
    status = `Copy failed: ${String(error)}`;
  }
}
```

#### Smart Copy picker

The model ranks regions and a popup lets the user pick one. The client asks `inference.complete` to label the regions, then shows them in a `popup` over a `clear`, with a `%list` and `key-hints`. Pages: [inference](/pages/api-inference/), [inference grant](/pages/capability-inference/), [layout and containers](/pages/widgets-layout-and-containers/), [collections](/pages/widgets-collections/) and [text](/pages/widgets-text/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::inference::{self, ChatMessage, CompletionRequest};
use ilium_plugin_sdk::types::ui::{ListItem, UiTree};
use ilium_plugin_sdk::ui::build::Tree;

async fn rank(screen: String) -> Vec<String> {
    let request = CompletionRequest {
        messages: vec![
            ChatMessage { role: "system".into(), content: "Return the three most useful strings to copy, one per line.".into() },
            ChatMessage { role: "user".into(), content: screen },
        ],
        max_output_tokens: None,
        json_schema: None,
        purpose: "smart-copy".into(),
    };
    match inference::complete(request).await {
        Ok(completion) => completion.text.lines().take(3).map(str::to_string).collect(),
        Err(_) => Vec::new(), // fall back to the light candidates
    }
}

fn picker(regions: &[String]) -> UiTree {
    let mut tree = Tree::new();
    let items = regions.iter().enumerate()
        .map(|(index, region)| ListItem::plain(&format!("{} {}", index + 1, region)))
        .collect::<Vec<_>>();
    let list = tree.list(items).key("regions");
    let hints = tree.key_hints(vec![("1-3", "copy"), ("Esc", "close")]).key("hints");
    let body = tree.clear().child(list).child(hints);
    let root = tree.popup("smart-copy").title("Smart Copy").modal(true).child(body);
    tree.finish(root)
}
```

**TypeScript**

```ts
import { complete } from '@ilium/plugin-sdk/host/inference';
import { clear, keyHints, list, popup, tree } from '@ilium/plugin-sdk/ui/build';

async function rank(screen: string): Promise<string[]> {
  try {
    const completion = await complete({
      messages: [
        { role: 'system', content: 'Return the three most useful strings to copy, one per line.' },
        { role: 'user', content: screen },
      ],
      maxOutputTokens: undefined,
      jsonSchema: undefined,
      purpose: 'smart-copy',
    });
    return completion.text.split('\n').slice(0, 3);
  } catch {
    return []; // fall back to the light candidates
  }
}

function picker(regions: string[]) {
  const items = regions.map((region, index) => ({ label: `${index + 1} ${region}` }));
  const regionList = list(items, { key: 'regions' });
  const hints = keyHints([{ keys: '1-3', label: 'copy' }, { keys: 'Esc', label: 'close' }], { key: 'hints' });
  return tree(popup('smart-copy', { title: 'Smart Copy', modal: true }, [clear([regionList, hints])]));
}
```

### Extension points

Other plugins change what `ilium.core.smart-copy` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.smart-copy/<point>`). Contributors need `extensions.contribute:ilium.core.smart-copy/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `recognisers` | collect | Teach Smart Copy a new kind of structure to select. | entry: id, kind, regex, multiline, label | standard |
| `after-copy` | hook (transform) | Change the copied text before it reaches the clipboard, for example to remove secrets. | text, kind, pane; reply: text | standard |

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.smart-copy/copied`: { kind, length } after text is copied

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.smart-copy"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replace)).

#### Example: Replaces anything that looks like an API token with `[removed]` before the copy

A package adds a `transform` handler. Each handler gets the previous output and may replace it.

```json
{
  "api": "ilium:plugin@0.1.0",
  "exports": [
    "hook-handler"
  ],
  "capabilities": { "server": ["extensions.contribute:ilium.core.smart-copy/after-copy"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.smart-copy/after-copy",
        "id": "strip-tokens",
        "priority": 50,
        "handler": "strip_tokens"
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
            "strip_tokens" => {
                // call.payload_json holds: text, kind, pane
                let _payload: serde_json::Value = match serde_json::from_str(&call.payload_json) {
                    Ok(value) => value,
                    Err(_) => return HookReply::Pass,
                };
                HookReply::Replace(new_payload_json)
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
    if (call.extension !== 'strip_tokens') return { tag: 'pass' };
    // call.payloadJson holds: text, kind, pane
    const payload = JSON.parse(call.payloadJson);
    void payload;
    return { tag: 'replace', val: newPayloadJson };
  },
});
```

A handler that is late, traps or returns a reply the mode does not allow counts as `pass`. See [Extension points](/pages/api-extension-points/#hooks).

### ABI used

- [Menus](/pages/api-menus/), [Menu provider](/pages/api-menu-provider/), [Collections](/pages/widgets-collections/), [Text](/pages/widgets-text/), [Layout and containers](/pages/widgets-layout-and-containers/)
- [Terminal pointer](/pages/primitives-terminal-pointer/), [terminal.pointer](/pages/capability-terminal-pointer/), [Terminal reads](/pages/primitives-terminal-reads/), [terminal.read-cells](/pages/capability-terminal-read-cells/)
- [Clipboard](/pages/api-clipboard/), [clipboard.write](/pages/capability-clipboard/)
- [Inference](/pages/api-inference/), [inference grant](/pages/capability-inference/)
- [Events](/pages/api-events/), [Messages and topics](/pages/api-messages-and-topics/), [Worlds and sides](/pages/worlds-and-sides/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Terminal plugin](/pages/core-plugin-terminal/)
- [Inference providers plugin](/pages/core-plugin-inference/)

### Security

Risk marks of this plugin's extension points follow the corrected table in [Extension security](/pages/reference-extension-security/) ([SEC-293]).
