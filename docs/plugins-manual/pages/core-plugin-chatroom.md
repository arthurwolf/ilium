---
layout: page
title: Chatroom
permalink: /pages/core-plugin-chatroom/
---

## Chatroom

The Chatroom is a shared message board for the agents of one project and for the user. Its messages live in a plain `CHATROOM.md` file at the project root, so every agent can read and append to it. The plugin id is `ilium.core.chatroom` and it runs on both sides: the server side owns the file and the tree row, and the client side draws the message view and the composer.

### What it covers

- The Chatroom row in the tree, with an unread count, and the shared `CHATROOM.md` file, described in the Chatroom section of the automation guide.
- `ilium chat`, described in the CLI reference. The command reads and appends to the same file the plugin tails.
- The message view and the composer.
- The context-injection output of `ilium chat context`, which the hooks that [Agent setup](/pages/core-plugin-agent-setup/) installs print into an agent prompt.

Agent setup owns the Setup tab, both instruction blocks and the hook entries. Chatroom asks Agent setup through the service `ilium.core.agent-setup/install` to install the chatroom block and hooks. At start, Agent setup repairs a missing or outdated chatroom hook when `CHATROOM.md` exists. `ilium chat init` installs the same hooks on request through the same service, so both paths write the same entries and neither one duplicates a hook.

### See it

<figure><img src="/assets/demos/14-chatroom.gif" alt="Two agents and the user exchange messages in the Chatroom view" loading="lazy"><figcaption>Agents append messages to the shared file and the Chatroom view shows them as timestamped rows.</figcaption></figure>

### What the user sees

<figure>
<pre class="term">╭─ Tree ─────────────────────────────────────────╮
│ ▶ 📂 Ilium                                     │
│ ▼ 💬 Chatroom <span class="t-hi">(3 new)</span>                          │
│ ›   📄 CHATROOM.md                             │
│ › 🦀 ⠋ Fix auth                                │
╰────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative: the icons depend on the icon set. The virtual Chatroom row expanded in the tree, with an unread count.</figcaption>
</figure>

<figure>
<pre class="term">╭─ Chatroom ─────────────────────────────────────╮
│<span class="t-hi"> Agents in this project</span>                         │
│<span class="t-dim"> fix-auth · review</span>                              │
│ 2026-10-12 09:14:02 fix-auth Claiming          │
│   src/auth.rs until 10:00.                     │
│ 2026-10-12 09:15:40 review Tests pass.         │
├────────────────────────────────────────────────┤
│<span class="t-dim"> Write message - Enter sends</span>                    │
╰────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Each message is a row <code>&lt;YYYY-MM-DD HH:mm:ss&gt; &lt;author&gt; &lt;content&gt;</code>, newest at the bottom, under the list "Agents in this project". The one-line composer posts as <code>user</code> and sends on Enter. With no messages the view reads "No messages yet. Type below to post as user."</figcaption>
</figure>

### Behaviour contract

| Id | Rule | Exact value or text | Baseline source |
|---|---|---|---|
| chatroom-B01 | Record line | `- YYYY-MM-DD HH:mm:ss +HH:MM \| <author> \| <content>`, local time with its offset | `src/docs/automation.md#file-format-and-limits` |
| chatroom-B02 | Escaping | Newline and tab in content are escaped as `\n` and `\t`, backslash as `\\`; other control characters are removed | `ilium-client/src/chatroom.rs` |
| chatroom-B03 | Message limit | At most 4000 characters; author and message must not be empty | `src/docs/cli-reference.md#ilium-chat` |
| chatroom-B04 | Default author | `$ILIUM_CHATROOM_AUTHOR`, then `$AGENT_NAME`, then `agent` | `src/docs/cli-reference.md#ilium-chat` |
| chatroom-B05 | Initial file | The title `# ILIUM CHATROOM`, the marker `<!-- ilium-chatroom: v1 -->`, the guidance paragraph of the prompt template `agent/coordination-guidance` byte for byte, and the heading `## Messages`. An existing file is never overwritten; a symlinked or non-regular file is refused | `src/docs/automation.md#set-it-up` |
| chatroom-B06 | Ignore lines | `.gitignore` gets the lines `/CHATROOM.md` and `/CHATROOM.archive.md`, created if needed. The baseline writes both (`CHATROOM_ARCHIVE_IGNORE`); the user guide names only the first and is corrected | `ilium-client/src/chatroom.rs` |
| chatroom-B07 | View limits | The view reads at most the last 8 MiB and shows the newest 200 messages; retention while reading is 4 MiB; the unread scan covers at most 2000 records | `ilium-client/src/chatroom.rs` |
| chatroom-B08 | Composer | Placeholder "Write message - Enter sends"; Enter sends; the author is `user`. Empty view: "No messages yet. Type below to post as user." | `src/docs/automation.md#use-it-in-the-tui` |
| chatroom-B09 | View rows | `<YYYY-MM-DD HH:mm:ss> <author> <content>`, newest at the bottom, under the list "Agents in this project" | `src/docs/automation.md#use-it-in-the-tui` |
| chatroom-B10 | Command output | `init` prints `chatroom ready at <path>`; `send` prints `chatroom message sent`; `tail` prints `timestamp \| author \| content`, default 100 records | `src/docs/cli-reference.md#ilium-chat` |
| chatroom-B11 | Send errors | Printed as `ilium: <text>` on stderr with exit 1: `this project does not have a chatroom`; `chatroom author and message must not be empty`; `chatroom messages are limited to 4000 characters` | `ilium-client/src/chatroom.rs` |
| chatroom-B12 | Hook command | `ilium chat context --limit 40 --since-last-read --max-bytes 2048`, timeout 10 s, status message "Checking ilium chatroom" (installed by Agent setup) | `ilium-client/src/chatroom.rs` |
| chatroom-B13 | Context text | Rendered from the prompt templates `agent/chatroom-context`, `agent/chatroom-empty-context`, `agent/chatroom-omitted` and `agent/chatroom-context-row`, byte-identical to the baseline. A first read starts with "Ilium chatroom is enabled"; a since-last-read output starts with "New ilium chatroom messages" | `ilium-client/src/chatroom.rs` |
| chatroom-B14 | Output cap | When the output exceeds `--max-bytes`, 160 bytes are reserved for the omitted note and a cut row ends with the marker `…` followed by a newline. A session-start hook gets 4 times the budget | `ilium-client/src/chatroom.rs` |
| chatroom-B15 | Reader cursor | The file `.ilium/chat-readers/<first 32 hex chars of sha256(reader key)>` holds the 64-hex sha256 of `timestamp\0author\0content\0` of the last message read. The reader key is `--reader`, else `ILIUM_PANE_ID` | `ilium-client/src/chatroom.rs` |
| chatroom-B16 | Unread count | The tree row shows "(N new)", computed for the reader key `user`. New in the plugin build, not parity | none |
| chatroom-B17 | Locked append | Every append, from the CLI and from the composer, takes the exclusive advisory lock of the baseline (`ilium-platform` file lock on `CHATROOM.md`) and appends one line. A whole-file rewrite is allowed only for archive rotation, under the same lock | `src/docs/automation.md#file-format-and-limits` |

The test for chatroom-B17: 50 concurrent `ilium chat send` calls and 50 composer sends produce 100 records, none lost and none interleaved.

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

The contract version is 0.1. The grants follow the file scope rules: the room is in the `project` scope, and a topic grant names its topic. The plugin holds no `agent-config` grant: Agent setup owns the agent files.

```json
{
  "id": "ilium.core.chatroom",
  "capabilities": {
    "server": [
      "tree.decorate", "tree.write",
      "fs.read:project", "fs.write:project", "fs.watch:project",
      "topics.subscribe:chatroom",
      "services.use:ilium.core.agent-setup"
    ],
    "client": ["topics.subscribe:chatroom", "ui.panels"]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.chatroom",
  "name": "Chatroom",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "The Chatroom is a shared message board for the agents of one project and for the user.",
  "license": "MIT",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": [
      "virtual-rows-provider"
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
      "tree.decorate",
      "tree.write",
      "fs.read:project",
      "fs.write:project",
      "fs.watch:project",
      "topics.subscribe:chatroom",
      "services.use:ilium.core.agent-setup",
      "cli.contribute"
    ],
    "client": [
      "topics.subscribe:chatroom",
      "ui.panels"
    ]
  },
  "activation": [
    "startup",
    "topic:chatroom"
  ],
  "limits_tier": "first-party",
  "contributes": {
    "cli": [{
      "name": "chat",
      "summary": "Project chatroom: init, send, context, tail",
      "output": "text",
      "stdin": "none",
      "requires_server": false
    }]
  }
}
```

### Samples

#### The Chatroom row

The row is a virtual row with its own children. The plugin exports `list-virtual-children`, which must answer within 20 ms, sets the top-level rows with the unread count, and expands the row on open. See [virtual rows](/pages/api-virtual-rows-provider/), [exports](/pages/api-exports/) and [tree structure intents](/pages/api-intent-tree-structure/).

**Rust**

```rust
use ilium_plugin_sdk::exports::VirtualRowsProvider;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{ExpandTarget, SetExpandedSpec, SetVirtualRowsSpec, VirtualRow};
use ilium_plugin_sdk::exports::virtual_rows_provider::VirtualRowChild;

impl VirtualRowsProvider for Chatroom {
    fn list_virtual_children(
        &mut self, row_kind: &str, _parent: &str, _cursor: Option<String>,
    ) -> Result<(Vec<VirtualRowChild>, Option<String>), PluginError> {
        if row_kind != "chatroom" {
            return Err(PluginError::not_found("unknown row kind"));
        }
        let file = VirtualRowChild::new("file", "CHATROOM.md").icon("file");
        Ok((vec![file], None))
    }
}

fn unread_row(unread: u32) -> Submission {
    let title = if unread == 0 { "Chatroom".to_string() } else { format!("Chatroom ({unread} new)") };
    Submission::new(Intent::SetVirtualRows(SetVirtualRowsSpec {
        row_kind: "chatroom".into(),
        rows: vec![VirtualRow::new("chatroom", &title).expandable(true)],
    }))
}

fn open_row() -> Submission {
    Submission::new(Intent::SetExpanded(SetExpandedSpec {
        target: ExpandTarget::VirtualRow("chatroom".into()),
        expanded: true,
    }))
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Submission, VirtualRowChild } from '@ilium/plugin-sdk/types';

export const { virtualRows } = definePlugin({
  side: 'server',
  listVirtualChildren(rowKind: string): [VirtualRowChild[], string | undefined] {
    if (rowKind !== 'chatroom') throw new Error('unknown row kind');
    return [[{ id: 'file', label: 'CHATROOM.md', icon: 'file', expandable: false,
      command: undefined, acceptsDrop: false }], undefined];
  },
});

export const unreadRow = (unread: number): Submission => ({
  intent: { tag: 'set-virtual-rows', val: { rowKind: 'chatroom', rows: [{
    id: 'chatroom', label: unread === 0 ? 'Chatroom' : `Chatroom (${unread} new)`,
    expandable: true } ] } },
  correlation: 1n, dedupKey: undefined,
});

export const openRow = (): Submission => ({
  intent: { tag: 'set-expanded', val: { target: { tag: 'virtual-row', val: 'chatroom' }, expanded: true } },
  correlation: 2n, dedupKey: undefined,
});
```

#### Tail CHATROOM.md and publish

On `fs-changed` the server reads what was added since its cursor and publishes each message on the topic `chatroom`. For each message it then publishes the contributed topic `ilium.core.chatroom/posted` with `{ author, kind, length }`, where `length` is the number of characters of the message text; `message_header` in the example reads `author` and `kind` from the message header. The client updates when it receives `topic-published`. See [messages and topics](/pages/api-messages-and-topics/), [topics.subscribe](/pages/capability-topics-subscribe/), [atomic writes and tail](/pages/api-fs-atomic-and-tail/), [fs.watch](/pages/capability-fs-watch/) and [events](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::host::{fs, topics};
use ilium_plugin_sdk::types::events::FsChangedEvent;
use ilium_plugin_sdk::types::paths::FsPath;

fn on_fs_changed(&mut self, changed: FsChangedEvent) {
    if self.watch != Some(changed.watch) {
        return;
    }
    let path = FsPath { scope: "project".into(), path: "CHATROOM.md".into() };
    let Ok(chunk) = fs::tail(&path, self.cursor, 65_536) else { return };
    self.cursor = chunk.cursor;
    for block in split_messages(&chunk.bytes) {
        let payload = serde_json::json!({ "text": block }).to_string();
        let _ = topics::publish("chatroom", &payload);
        let (author, kind) = message_header(&block);
        let posted = serde_json::json!({ "author": author, "kind": kind, "length": block.chars().count() }).to_string();
        let _ = topics::publish("ilium.core.chatroom/posted", &posted);
    }
}
```

**TypeScript**

```ts
import { tail } from '@ilium/plugin-sdk/host/fs';
import { publish } from '@ilium/plugin-sdk/host/topics';

let cursor = 0n;

export function onFsChanged(watch: bigint, ownWatch: bigint): void {
  if (watch !== ownWatch) return;
  const chunk = tail({ scope: 'project', path: 'CHATROOM.md' }, cursor, 65_536);
  cursor = chunk.cursor;
  for (const block of splitMessages(chunk.bytes)) {
    publish('chatroom', JSON.stringify({ text: block }));
    const [author, kind] = messageHeader(block);
    publish('ilium.core.chatroom/posted', JSON.stringify({ author, kind, length: [...block].length }));
  }
}

// Client side: case 'topic-published': if (event.val.topic === 'chatroom') invalidate the view.
```

#### Talk between sides and peers

The composer lives on the client side, which sends a `message` with the draft. The server appends one line under the same exclusive lock that the `ilium chat send` command takes. Another plugin can post by sending a message to `ilium.core.chatroom`; its own manifest then needs `messages.peer:ilium.core.chatroom`. See [peer messages](/pages/capability-messages-peer/), [worlds and sides](/pages/worlds-and-sides/), [events](/pages/api-events/) and [messages and topics](/pages/api-messages-and-topics/).

**Rust**

```rust
use ilium_plugin_sdk::host::fs::{self, WriteMode};
use ilium_plugin_sdk::types::events::MessageEvent;
use ilium_plugin_sdk::types::intents::{MessageSpec, MessageTarget, PluginSide};

// Client side: send the draft to the server side of this plugin.
fn send_draft(draft: &str) -> Submission {
    Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget { plugin: "ilium.core.chatroom".into(), side: PluginSide::Server, client: None },
        channel: "post".into(),
        payload: draft.as_bytes().to_vec(),
    }))
}

// Server side: append one record under the exclusive advisory lock.
async fn on_message(&mut self, message: MessageEvent) -> Result<(), PluginError> {
    let path = FsPath { scope: "project".into(), path: "CHATROOM.md".into() };
    let lock = fs::lock(&path, true, 2_000).await?; // the lock `ilium chat send` takes
    let outcome = fs::write(&path, format_record(&message.from, &message.payload), WriteMode::Append).await;
    fs::unlock(lock);
    outcome
}
```

**TypeScript**

```ts
import { lock, unlock, write } from '@ilium/plugin-sdk/host/fs';
import type { Submission } from '@ilium/plugin-sdk/types';

// Client side: send the draft to the server side of this plugin.
export const sendDraft = (draft: string): Submission => ({
  intent: { tag: 'message', val: {
    to: { plugin: 'ilium.core.chatroom', side: 'server', client: undefined },
    channel: 'post', payload: new TextEncoder().encode(draft) } },
  correlation: 1n, dedupKey: undefined,
});

// Server side: append one record under the exclusive advisory lock.
export async function onMessage(from: string, payload: Uint8Array): Promise<void> {
  const path = { scope: 'project', path: 'CHATROOM.md' };
  const lockId = await lock(path, true, 2000); // the lock `ilium chat send` takes
  try {
    await write(path, formatRecord(from, payload), 'append');
  } finally {
    unlock(lockId);
  }
}
```

#### Message view and composer

Messages render as rows of `text` in a `scroll-view` that follows the tail, and a single-line `text-input` is the composer. `text-input` submits on Enter when `submit-on-enter` is true, so Enter sends and there is no send command. See [text widgets](/pages/widgets-text/), [layout and containers](/pages/widgets-layout-and-containers/) and [input widgets](/pages/widgets-input/).

**Rust**

```rust
impl Views for ChatroomView {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        // Each row is "<YYYY-MM-DD HH:mm:ss> <author> <content>".
        let rows = tree.text(self.transcript_rows_text()).key("messages");
        let scroll = tree.scroll_view(self.transcript_rows).key("scroll").follow_tail(true).child(rows);
        let composer = tree.text_input("Write message - Enter sends", self.draft.clone())
            .key("composer").submit_on_enter(true);
        // ("composer", ActionKind::Submitted, ActionValue::Text(t)) => post t as `user`
        let body = tree
            .layout(Direction::Vertical, [Constraint::Fill(1), Constraint::Length(1)])
            .child(scroll).child(composer);
        let root = tree.block().title("Chatroom").child(body);
        tree.finish(root)
    }
}
```

**TypeScript**

```ts
import { block, fill, layout, length, scrollView, text, textInput, tree } from '@ilium/plugin-sdk/ui/build';

export function render() {
  // Each row is "<YYYY-MM-DD HH:mm:ss> <author> <content>".
  const rows = text(state.transcriptText, { key: 'messages' });
  const scroll = scrollView({ contentRows: state.transcriptRows, key: 'scroll', followTail: true }, [rows]);
  // Submitted fires on Enter: post the text as `user`.
  const composer = textInput('Write message - Enter sends', state.draft, { key: 'composer', submitOnEnter: true });
  const body = layout({ direction: 'vertical', constraints: [fill(1), length(1)] }, [scroll, composer]);
  return tree(block({ titles: [{ text: 'Chatroom', position: 'top', alignment: 'left' }] }, [body]));
}
```

#### Ask Agent setup to install the block and hooks

The Chatroom plugin writes no agent file. When the user adds a chatroom to a project, it calls the service of the Agent setup plugin, which owns both instruction blocks and the hook entries. See [services](/pages/api-services/) and [Agent setup](/pages/core-plugin-agent-setup/).

**Rust**

```rust
use ilium_plugin_sdk::host::services;

// manifest: "capabilities": { "server": ["services.use:ilium.core.agent-setup"] }
async fn install_agent_integration(&mut self, project: &str) -> Result<(), PluginError> {
    let request = serde_json::json!({ "feature": "chatroom", "project": project }).to_string();
    services::call("ilium.core.agent-setup", "install", &request).await?;
    Ok(())
}
```

**TypeScript**

```ts
import { call } from '@ilium/plugin-sdk/host/services';

// manifest: "capabilities": { "server": ["services.use:ilium.core.agent-setup"] }
export async function installAgentIntegration(project: string): Promise<void> {
  const request = JSON.stringify({ feature: 'chatroom', project });
  await call('ilium.core.agent-setup', 'install', request);
}
```

#### 6. Keep the scroll position

The message list sends `scrolled` with the offset in rows. The plugin keeps following new messages only while the view is at the bottom, so reading older messages is not interrupted.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ui::{ActionKind, ActionValue};

impl Handler for FollowTail {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::UiAction(action) = event else { continue };
            if let (ActionKind::Scrolled, ActionValue::Number(offset)) = (action.kind, action.value) {
                self.at_bottom = offset >= self.last_row_count as f64;
                self.offset = offset;
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
export function onAction(event: Extract<Event, { tag: 'ui-action' }>): Submission[] {
  if (event.val.kind !== 'scrolled' || event.val.value.tag !== 'number') return [];
  scroll.offset = event.val.value.val;
  scroll.atBottom = scroll.offset >= lastRowCount;
  return [];
}
```

### Extension points

Other plugins change what `ilium.core.chatroom` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.chatroom/<point>`). Contributors need `extensions.contribute:ilium.core.chatroom/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `before-post` | hook (transform) | Change a message before it is written to the room. | author, text, kind; reply: text | high |
| `message-renderers` | collect | Draw a message kind in a special way, such as a task claim. | entry: id, kind, view (a plugin view that draws the message) | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.chatroom/post`: send: post a message
- `ilium.core.chatroom/tail`: get: read recent messages

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.chatroom/posted`: { author, kind, length }

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.chatroom"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Removes lines that look like keys from a message

A package adds a `transform` handler. Each handler gets the previous output and may replace it.

```json
{
  "api": "ilium:plugin@0.1.0",
  "exports": [
    "hook-handler"
  ],
  "capabilities": { "server": ["extensions.contribute:ilium.core.chatroom/before-post"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.chatroom/before-post",
        "id": "strip-secrets",
        "priority": 50,
        "handler": "strip_secrets"
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
            "strip_secrets" => {
                // call.payload_json holds: author, text, kind
                let mut payload: serde_json::Value = match serde_json::from_str(&call.payload_json) {
                    Ok(value) => value,
                    Err(_) => return HookReply::Pass,
                };
                // Change the fields the hook owns, for example payload["text"].
                let new_payload_json = payload.to_string();
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
    if (call.extension !== 'strip_secrets') return { tag: 'pass' };
    // call.payloadJson holds: author, text, kind
    const payload = JSON.parse(call.payloadJson);
    // Change the fields the hook owns, for example payload.text.
    const newPayloadJson = JSON.stringify(payload);
    return { tag: 'replace', val: newPayloadJson };
  },
});
```

A handler that is late, traps or returns a reply the mode does not allow counts as `pass`. See [Extension points](/pages/api-extension-points/#hooks).

### Extension points this plugin owns

The table gives the values that [EXT-052] requires for each point this plugin owns. The page does not state the version, deadline, max or dynamic values for these points, so those come from the defaults in [Extension points](/pages/api-extension-points/) and are marked (default). The side comes from the page: the `before-post` example declares its handler under the server side, and the message view that draws `message-renderers` entries runs on the client side.

| Point | Side | Version | Deadline | Max | Dynamic |
|---|---|---|---|---|---|
| `ilium.core.chatroom/before-post` | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) |
| `ilium.core.chatroom/message-renderers` | client | 0.1.0 (default) | none (default) | 64 entries (default) | no (default) |

### ABI used

- [Virtual rows](/pages/api-virtual-rows-provider/), [exports](/pages/api-exports/) and [tree structure intents](/pages/api-intent-tree-structure/)
- [Messages and topics](/pages/api-messages-and-topics/), [topics.subscribe](/pages/capability-topics-subscribe/) and [peer messages](/pages/capability-messages-peer/)
- [Atomic writes and tail](/pages/api-fs-atomic-and-tail/), [fs.watch](/pages/capability-fs-watch/), [fs.write](/pages/capability-fs-write/) and [file access](/pages/file-access/)
- [Events](/pages/api-events/) and [worlds and sides](/pages/worlds-and-sides/)
- [Text widgets](/pages/widgets-text/), [layout and containers](/pages/widgets-layout-and-containers/) and [input widgets](/pages/widgets-input/)
- [Settings row types](/pages/reference-settings-rows/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Progress](/pages/core-plugin-progress/)

### Security

Risk marks of this plugin's extension points follow the corrected table in [Extension security](/pages/reference-extension-security/) ([SEC-293]).
