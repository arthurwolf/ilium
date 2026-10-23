---
layout: page
title: Your first plugin in five minutes
permalink: /pages/getting-started-first-plugin/
---

## Your first plugin in five minutes

This page takes you from an empty folder to a badge on an agent row. You create a server-side plugin from the template, change one line of Rust, and run the plugin in dev mode. The page shows the tree before and after the change. The full build of a plugin with state, commands and tests is the subject of the [tutorial series](/pages/tutorial-01-agent-badge/).

### How the plugin works

The host sends a plugin a batch of events. When an agent changes activity, the host sends an `agent-activity-changed` event for that agent's row in the tree. The plugin reads the event and returns a list of submissions. A submission carries an intent. The intent `set-decoration` puts styled text into a named slot on the row, and `clear-decoration` removes it. The host draws the slot as a badge.

<figure>
<svg viewBox="0 0 400 250" role="img" aria-label="Event, handler, intent and badge in four steps">
  <title>Event to badge</title>
  <desc>The host sends an agent activity event to the plugin handler. The handler returns an intent, and the host draws a badge on the row.</desc>
  <defs>
    <marker id="arrow-first-plugin" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="110" y="8" width="180" height="40" rx="4" fill="none" stroke="currentColor"/>
  <text x="200" y="32" font-size="11" text-anchor="middle" fill="currentColor">Agent activity changes</text>
  <line x1="200" y1="48" x2="200" y2="70" stroke="currentColor" marker-end="url(#arrow-first-plugin)"/>
  <rect x="110" y="70" width="180" height="40" rx="4" fill="none" stroke="currentColor"/>
  <text x="200" y="94" font-size="11" text-anchor="middle" fill="currentColor">on_events(batch)</text>
  <line x1="200" y1="110" x2="200" y2="132" stroke="currentColor" marker-end="url(#arrow-first-plugin)"/>
  <rect x="40" y="132" width="320" height="40" rx="4" fill="none" stroke="currentColor"/>
  <text x="200" y="156" font-size="11" text-anchor="middle" fill="currentColor">set-decoration or clear-decoration intent</text>
  <line x1="200" y1="172" x2="200" y2="194" stroke="currentColor" marker-end="url(#arrow-first-plugin)"/>
  <rect x="90" y="194" width="220" height="40" rx="4" fill="none" stroke="currentColor"/>
  <text x="200" y="218" font-size="11" text-anchor="middle" fill="currentColor">Badge drawn on the tree row</text>
</svg>
<figcaption>The plugin never draws anything itself. It returns an intent, and the host draws the badge in the slot that the intent names.</figcaption>
</figure>

### What you need

- A Rust toolchain of version 1.90 or later with the `wasm32-wasip2` target. [Set up a toolchain](/pages/getting-started-toolchain/) shows the install steps.
- An Ilium build with the `ilium plugin` commands. Installing Ilium installs them.
- Two grants, both on the server side. The manifest lists them, and the install dialog shows each one as a row that you check to allow it.

| Grant | Dialog row text | What the plugin may do with it |
|---|---|---|
| `agent.read` | See agent status | Receive `agent-activity-changed` events for agent panes |
| `tree.decorate` | Add badges, icons and rows to the tree | Send `set-decoration` and `clear-decoration` intents |

A declined grant stays unchecked and dimmed in the dialog, and the plugin does not receive it in its activation context. Without `agent.read` the plugin gets no activity events, so it never draws a badge. Without `tree.decorate` the host refuses the whole batch that holds a decoration intent.

### Create the project

Run the command in a directory that is not inside a project you care about. The `--id`, `--lang` and `--side` flags are required. `--name` is optional and sets the display name.

**Rust**

```bash
ilium plugin new agent-tools --id example.agent-tools --lang rust --side server --name "Agent tools"
cd agent-tools
ilium plugin build
```

| Flag or argument | Value in this page | Meaning |
|---|---|---|
| `<dir>` | `agent-tools` | Folder to create. |
| `--id` | `example.agent-tools` | Plugin id. Global command ids start with it, for example `example.agent-tools/greet`. |
| `--lang` | `rust` | `rust` or `typescript`. |
| `--side` | `server` | `server`, `client`, `both` or `scene`. |
| `--name` | `Agent tools` | Display name shown in the install dialog and in settings. |

The command prints JSON lines. Each line has a `type` field, so a script can read the result.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin new agent-tools --id example.agent-tools \
    --lang rust --side server
{"type":"progress","step":"template","message":"Writing the Rust template"}
{"type":"result","ok":true}
{"type":"summary","duration_ms":412,"warnings":0,"errors":0}
</pre>
<figcaption>Illustrative, not generated. The new command writes three files and prints one result line.</figcaption>
</figure>

The template writes three files:

| File | Contents |
|---|---|
| `manifest.json` | Plugin id, version, grants and contributions. The next section shows the file. |
| `Cargo.toml` | Crate settings. The crate builds as a component for `wasm32-wasip2`. |
| `src/lib.rs` | The server side. It holds the badge text function and the handler. |

The template handler already submits one intent for each activity event. While `badge_text` returns an empty string for every activity, each event clears the slot, so the template draws no badge. The first render in this page shows that state.

### The manifest

The manifest below is the one the template writes. It declares both grants and the `badge` slot that the plugin writes to. The `files` inventory is written by `ilium plugin pack`, so a source manifest may omit it.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.agent-tools",
  "name": "Agent tools",
  "version": "0.1.0",
  "publisher": "Example Developer",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": []
  },
  "capabilities": {
    "server": ["agent.read", "tree.decorate"]
  },
  "contributes": {
    "decoration_slots": [
      { "id": "badge", "placement": "badge", "priority": 10 }
    ]
  }
}
```

| Key | Type | Meaning |
|---|---|---|
| `api` | string | `ilium:plugin@0.1.0`. |
| `id` | string | Plugin id. It must match the `--id` given to `new`. |
| `name` | string | Display name in the install dialog and in settings. |
| `version` | string | Plugin version. A higher version of an installed id is an update. |
| `publisher` | string | Publisher name shown in the install dialog. |
| `server.entry` | string | Component file. `server.wasm` for the server side. |
| `server.world` | string | WIT world. `ilium:server/base@0.1.0` for the server side. |
| `server.exports` | list | Optional exports: `views`, `detector`, `job-runner`, `service-provider`, `inference-provider` and `annotator`. The handler needs no entry. |
| `capabilities.server` | list | Grants of the server side, as in the grants table above. |
| `contributes.decoration_slots` | list | Each entry has `id`, `placement` and `priority`. `placement` is one of `badge`, `icon`, `subtitle` or `footer`. A lower `priority` is drawn first within the same placement. |

### Worked example: the complete server side in both languages

This is the complete server side after the change in the next section, in Rust (`src/lib.rs`) and in TypeScript (`src/server.ts`). Both plugins build as they stand.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{DecorationKey, SetDecorationSpec};
use ilium_plugin_sdk::types::tree::AgentActivity;

const BADGE_SLOT: &str = "badge";

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct AgentTools;

/// Text for the badge slot. An empty string clears the slot.
fn badge_text(activity: AgentActivity) -> &'static str {
    match activity {
        AgentActivity::Working => "working",
        _ => "",
    }
}

impl Handler for AgentTools {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::AgentActivityChanged(change) = event else { continue };
            let text = badge_text(change.current);
            let intent = if text.is_empty() {
                Intent::ClearDecoration(DecorationKey {
                    node: change.node,
                    slot: BADGE_SLOT.into(),
                })
            } else {
                Intent::SetDecoration(SetDecorationSpec {
                    node: change.node,
                    slot: BADGE_SLOT.into(),
                    content: vec![Span::plain(text)],
                    icon: None,
                    sort_key: None,
                    persist: false,
                })
            };
            submissions.push(Submission::new(intent));
        }
        submissions
    }
}
```

The handler reads two fields from each event. `change.node` names the row, and `change.current` holds the new activity. It writes one intent for each event. `persist: false` means the badge is dropped when the plugin shuts down.

### The one changed line

In the template, the `AgentActivity::Working` arm returns an empty string, so the badge never has text. Change that arm to return the word `working`:

**Rust**

```rust
        AgentActivity::Working => "working",   // was: ""
```

**TypeScript**

```ts
    case 'working':
      return 'working';   // was: ''
```

Nothing else changes. Build the plugin again with `ilium plugin build`.

### Build and run in dev mode

Run dev mode from the project folder. Dev mode installs the plugin as an unsigned development package, which the tree shows with a `dev` badge. The plugin reloads each time a file in the project changes.

```bash
ilium plugin build
ilium plugin dev
```

The dev command prints one `reload` line each time it loads a new build.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin dev
<span class="t-ok">watching</span> agent-tools/ for changes
<span class="t-hi">reloaded</span> example.agent-tools 0.1.0
</pre>
<figcaption>Illustrative, not generated. The same reload appears as a JSON line with type <code>reload</code>, shown below.</figcaption>
</figure>

```json
{"type":"reload","plugin":"example.agent-tools","version":"0.1.0","restored_bytes":0}
```

### Before and after

The first render shows the tree before the change. The agent in `api/server` is working, but the template draws no badge. The second render shows the same tree after the change. The `working` badge appears on the row. The other rows do not change.

<figure>
<pre class="term">╭  ≡ ● · Sessions ─────────────────────╮
│ ▼ 📂 shop                            │
│ › ▼ 📂 api                           │
│ ››   🦀 <span class="t-ok">⠋</span> Fix login redirect         │
│ ››   🐢 <span class="t-dim">●</span> Review schema              │
│ ››   🖥️    Shell                     │
│ › ▶ 📂 web                           │
│                                      │
╰──────────────────────────────────────╯
 <span class="t-dim">↑/↓ move · Enter open · F1 help</span>
</pre>
<figcaption>Illustrative, not generated. The agent in the first pane is working, shown by the spinner in the Now column, and the template draws no badge.</figcaption>
</figure>

<figure>
<pre class="term">╭  ≡ ● · Sessions ─────────────────────╮
│ ▼ 📂 shop                            │
│ › ▼ 📂 api                           │
│ ››   🦀 <span class="t-ok">⠋</span> Fix login redirect <span class="t-ok">working</span> │
│ ››   🐢 <span class="t-dim">●</span> Review schema              │
│ ››   🖥️    Shell                     │
│ › ▶ 📂 web                           │
│                                      │
╰──────────────────────────────────────╯
 <span class="t-hi">After: the row carries the badge.</span>
</pre>
<figcaption>Illustrative, not generated. The <code>working</code> badge is drawn after the title of the row. The other rows do not change.</figcaption>
</figure>

When the agent leaves the working state, the plugin receives a new `agent-activity-changed` event. The handler then returns `clear-decoration`, and the badge disappears.

### Create the TypeScript project

The TypeScript form is created with `--lang typescript`. Its manifest is the same, except for the id.

**TypeScript**

```bash
ilium plugin new agent-tools-ts --id example.agent-tools-ts \
  --lang typescript --side server --name "Agent tools"
cd agent-tools-ts
bun install
ilium plugin build
ilium plugin dev
```

Bun 1.2 or later is the only tool you install globally. `bun install` fetches the pinned `@ilium/plugin-sdk`, which provides the types and the component build. The entry file is `src/server.ts`.

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { span } from '@ilium/plugin-sdk/types';
import type { AgentActivity } from '@ilium/plugin-sdk/primitives/agent';

const BADGE_SLOT = 'badge';

/** Text for the badge slot. An empty string clears the slot. */
function badgeText(activity: AgentActivity): string {
  switch (activity) {
    case 'working':
      return 'working';
    default:
      return '';
  }
}

export const { handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    batch.forEach((event, index) => {
      if (event.tag !== 'agent-activity-changed') return;
      const { node, current } = event.val;
      const text = badgeText(current);
      const intent = text === ''
        ? { tag: 'clear-decoration' as const, val: { node, slot: BADGE_SLOT } }
        : {
            tag: 'set-decoration' as const,
            val: {
              node,
              slot: BADGE_SLOT,
              content: [span(text)],
              icon: undefined,
              sortKey: undefined,
              persist: false,
            },
          };
      submissions.push({ intent, correlation: BigInt(index), dedupKey: undefined });
    });
    return submissions;
  },
});
```

The TypeScript template differs in one line: its `'working'` case returns an empty string until you change it, as in the Rust template.

The contract lets the same intent carry more than text. The extended `set-decoration` adds styled spans, a tooltip, a frame animation and trailing segments, so the badge could be a spinner or a meter instead of the word. Tutorial step 1 uses these fields. Node ids are `bigint` values. The event index serves as the `correlation` number, which is unique within one batch.

### Surface used on this page

| Item | Rust | TypeScript | Meaning |
|---|---|---|---|
| Server side | `#[ilium_plugin(side = "server")]` | `definePlugin({ side: 'server' })` | Declares the side. |
| Handler | `impl Handler`, `fn on_events` | `onEvents(batch)` | Receives a batch and returns submissions. |
| Activity event | `Event::AgentActivityChanged(change)` | `event.tag === 'agent-activity-changed'`, `event.val` | Fields `node`, `previous`, `current` and `revision`. |
| Activity values | `AgentActivity::Working` | `'working'` | `none`, `working`, `idle`, `awaiting-approval`, `done` and `menu`. |
| Set badge | `Intent::SetDecoration(SetDecorationSpec)` | tag `'set-decoration'` | Writes styled text into a slot. |
| Clear badge | `Intent::ClearDecoration(DecorationKey)` | tag `'clear-decoration'` | Empties a slot. Clearing an empty slot returns `ok`. |
| Text | `Span::plain(text)` | `span(text)` | One unstyled run of text. |
| Submission | `Submission::new(intent)` | object with `intent`, `correlation` and `dedupKey` | One intent in the returned list. |

`SetDecorationSpec` has these fields:

| Field | Type | Meaning |
|---|---|---|
| `node` | node id | Tree row that receives the decoration. |
| `slot` | string | Slot id. It must match an id in `contributes.decoration_slots`. |
| `content` | list of spans | Text. A badge is at most 12 cells wide. Wider text is cut and ends with `…`. |
| `icon` | optional string | Tabler icon name. `None` draws no icon. |
| `sort_key` | optional integer | Orders decorations that share a row. Lower sorts first. |
| `persist` | boolean | `true` keeps the decoration across a server restart, shown as stale. `false` drops it at shutdown. |

### Edge cases and errors

| Situation | Result | What happens |
|---|---|---|
| The manifest asks for a grant that does not exist | Build error `manifest.grant-unknown` | The text is `capability "<grant>" is not known to plugin API <api>`. Fix the spelling in `capabilities.server`. |
| The slot id is not declared in the manifest | `refused(invalid-argument)` | Only that submission is refused. The other submissions of the batch run. |
| The node was removed before the intent is applied | `refused(not-found)` | The row no longer exists. Nothing is drawn. |
| The plugin already holds the maximum number of decorations | `refused(quota-exceeded)` | The default limit is 4,096 decorations for each plugin. |
| A batch holds a decoration intent without `tree.decorate` | `refused(not-permitted)` for the whole batch | A missing grant refuses the batch. |
| The user declines `agent.read` | No activity events | No badge is drawn. The row in the install dialog stays unchecked. |
| The user revokes `tree.decorate` later | Decorations are withdrawn | The plugin is re-activated with the reduced grant set. The restart does not count as a failure. |
| `on_events` panics or traps | Plugin restart | The plugin restarts after 1 s, then 5 s. A third failure within 10 minutes disables the plugin, and the tree footer shows a notice. |
| The plugin restarts after a crash | Decorations stay, marked stale | Stale badges are drawn in the muted colour until the plugin sets or clears them again. |

<div class="note">
<p>An empty string from <code>badge_text</code> is a valid state. The handler uses it to clear the slot when an agent is idle, done or waiting for approval. Clearing an empty slot returns <code>ok</code>, so the plugin does not need to check whether a badge exists.</p>
</div>

<div class="warn">
<p>The slot id in the code and in <code>contributes.decoration_slots</code> must match exactly. A mismatch does not stop the build. Each submission then fails with <code>refused(invalid-argument)</code>, and no badge appears.</p>
</div>

### Support bundle for a bug report

A single log line often does not show why a plugin failed. When a bug report needs the log, the state, the limits and the crash history together, ask the user for one support bundle. One command writes it:

```bash
ilium plugin support-bundle example.agent-tools --out agent-tools-support.zip
```

The `<file>` must not exist yet. The archive holds exactly these four text entries and no others:

| Entry | Contents |
|---|---|
| `summary.txt` | Plugin id, version and digest, Ilium version, operating system and grants. It also holds the output of `ilium plugin status` and `ilium plugin doctor`, and the count of log lines dropped to fit the size limit. |
| `log.txt` | The newest 200 log lines of the plugin. |
| `stats.txt` | The output of `ilium plugin stats`, the resource use against each limit. |
| `crashes.txt` | One record for each helper crash, kill or out-of-memory kill in the last 30 days, with the time in UTC, the version, the side, the reason, the exit status and the export that was running. |

The steps for the user are these:

1. Run the same command with `--dry-run` added. It prints the file names, the sizes and the count of redactions for each rule, and writes nothing.
2. Run the command without `--dry-run`. The bundle is at most 8 MiB. When the log is too large, the oldest lines go first.
3. Open the zip and read `summary.txt` and `log.txt` before you send it. Nothing leaves the machine by itself. Ilium does not upload the bundle.

The bundle works for a plugin in state `failed`, `blocked` or `incompatible`, and for a plugin whose helper is missing. Writing it does not change the plugin's state, grants or health counters. A failed write leaves no partial file. An id that is not installed fails with `bundle.plugin-unknown`.

What the bundle leaves out is part of the contract. It redacts credential headers, URL parameters and home-directory paths, which print as `~`. It shows each plugin payload as a byte count only. It never contains the plugin's data folder, so your notes, your stored state and the content of any file the plugin read stay out. It has no trace recordings and no grant secrets. A secret value in a settings row is not in the output of `ilium plugin ... --json` either.

The one part that comes from your plugin is `log.txt`. Keep it clean by following the log rule in [Publishing and support](/pages/guide-publishing-and-support/#supporting-your-users): write counts and ids, never secrets, file contents or whole payloads. The Diagnostics screen has the same build as a "Build support bundle" button. It shows the file list and size first, and its default button is Cancel. The full rules are in [Diagnostics](/pages/core-plugin-diagnostics/#support-bundle).

For your own bug report template, ask for the bundle together with the answers in [A template for bug reports](/pages/guide-publishing-and-support/#a-template-for-bug-reports). Do not ask a user for `--unredacted` output. That flag works only for a plugin installed with `ilium plugin dev`. [AUT-314]

### See also

- [Tutorial step 1: agent badge](/pages/tutorial-01-agent-badge/) builds on this plugin and handles every activity state. [Tutorial step 2](/pages/tutorial-02-command-and-settings/) adds a command and a setting.
- [What a plugin is](/pages/getting-started-overview/) explains the sides, the tiers and the events used here.
- [Set up a toolchain](/pages/getting-started-toolchain/) covers the Rust target and the `ilium plugin` commands.
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/) list the module paths.
- [Tree capability: decorate](/pages/capability-tree-decorate/) documents the full decoration contract.
- [Agent primitive](/pages/primitives-agent/) lists the activity values.
- [Manifest reference](/pages/reference-manifest/) lists every manifest key.
- [Testing](/pages/testing/) shows how to test `on_events` without a running Ilium.
