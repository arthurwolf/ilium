# 8. The same plugin in TypeScript

## 8. The same plugin in TypeScript

This step ports the badge logic of step 1 and the table rows of step 3 to TypeScript. The manifest stays the same, and the plugin behaves the same way. The step teaches the build. `ilium plugin build` bundles the TypeScript with Bun, then `componentize-js` turns the bundle into a component named `server.wasm`. The host loads that component through the same Wasmtime path as the Rust build. The Rust code and the TypeScript code are two implementations of one WIT contract, `ilium:plugin@0.1.0`. Every listing in this step is shown twice, first as **Rust** and then as **TypeScript**, so you can follow the port in either direction. The step ends with a short section that uses three widgets of the extended widget set (`badge`, `spinner` and `progress-row`) from both languages.

### You will need

- [Tutorial step 7](/pages/tutorial-07-ambient-scene/) finished, or at least the Rust project from step 3. The TypeScript port is a second project next to it, called `agent-tools-ts`.
- Bun 1.2 or later. It is the only global tool. `bun install` installs `@ilium/plugin-sdk`, which pins `jco` and `componentize-js` exactly. Run TypeScript with Bun, never with `node`.
- The `ilium` binary, with its `plugin` subcommand.
- Server grants `agent.read` (activity events) and `tree.decorate` (set and clear badges). The manifest below declares both.

### What changed from the Rust version

- The badge logic lives in `src/badge.ts`. It imports only types from the SDK, so it has no host calls, and `bun test` runs it directly.
- `src/server.ts` is the entry point. Its `definePlugin` call exports the `lifecycle` and `handler` objects that a server plugin needs.
- The manifest is the step 1 manifest, unchanged. The id `example.agent-tools` matches step 1, so both builds describe the same plugin.
- The build output is a component, the same as the Rust build. The host does not know which language made it.
- Names follow the SDK rule for TypeScript: camelCase for functions and fields, PascalCase for types, and kebab-case strings for variant tags and enum values.

### The sequence

A TypeScript build has two stages: the bundle, then the component. Bun makes the bundle. `componentize-js` embeds it in the WIT world, which it reads from `node_modules/@ilium/plugin-sdk/wit/`. The project never holds a copy of the WIT files. Both stages run on your machine.

<figure>
<svg viewBox="0 0 420 190" role="img" aria-label="TypeScript build pipeline to server.wasm">
  <defs>
    <marker id="ts-build-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="10" width="120" height="36" rx="4" fill="none" stroke="currentColor"/>
  <text x="70" y="32" font-size="11" text-anchor="middle" fill="currentColor">src/server.ts</text>
  <rect x="10" y="80" width="120" height="36" rx="4" fill="none" stroke="currentColor"/>
  <text x="70" y="102" font-size="11" text-anchor="middle" fill="currentColor">bundle (one .js)</text>
  <rect x="170" y="10" width="130" height="36" rx="4" fill="none" stroke="currentColor" stroke-dasharray="5 3"/>
  <text x="235" y="32" font-size="11" text-anchor="middle" fill="currentColor">WIT world (SDK)</text>
  <rect x="170" y="80" width="130" height="36" rx="4" fill="none" stroke="currentColor"/>
  <text x="235" y="102" font-size="11" text-anchor="middle" fill="currentColor">componentize-js</text>
  <rect x="326" y="80" width="86" height="36" rx="4" fill="none" stroke="currentColor"/>
  <text x="369" y="102" font-size="11" text-anchor="middle" fill="currentColor">server.wasm</text>
  <line x1="70" y1="46" x2="70" y2="80" stroke="currentColor" marker-end="url(#ts-build-arrow)"/>
  <line x1="130" y1="98" x2="170" y2="98" stroke="currentColor" marker-end="url(#ts-build-arrow)"/>
  <line x1="235" y1="46" x2="235" y2="80" stroke="currentColor" stroke-dasharray="3 3" marker-end="url(#ts-build-arrow)"/>
  <line x1="300" y1="98" x2="326" y2="98" stroke="currentColor" marker-end="url(#ts-build-arrow)"/>
  <text x="10" y="150" font-size="11" fill="currentColor">Bun bundles the source. componentize-js adds the WIT world.</text>
  <text x="10" y="168" font-size="11" fill="currentColor">The host loads server.wasm like any other component.</text>
</svg>
<figcaption>Two stages, one artifact. The WIT world fixes the names, and the bundle holds the logic.</figcaption>
</figure>

### Create the project

```bash
ilium plugin new agent-tools-ts --id example.agent-tools \
  --lang typescript --side server --name "Agent tools"
cd agent-tools-ts
bun install
```

`ilium plugin new` writes the template: `manifest.json`, `package.json` and `src/server.ts`. Replace `src/server.ts` with the code below, and add the other files of the layout. `package.json` needs no build script. `ilium plugin build` detects a TypeScript project from the file, and bundles `src/server.ts` for the server side.

```text
agent-tools-ts/
  manifest.json
  package.json
  tsconfig.json        strict mode on
  src/server.ts        host glue, the server entry point
  src/badge.ts         pure badge logic
  src/badge.test.ts    bun test cases
  src/table.ts         row text of step 3 (client side)
```

The manifest is the step 1 manifest:

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

### The surface this step uses

| Name | Import path | Kind and meaning |
|---|---|---|
| `definePlugin` | `@ilium/plugin-sdk` | Function. Declares the side and the export objects, and returns them. |
| `lifecycle.activate(context)` | export | Runs once when the instance starts. `context.plugin` holds the plugin id. |
| `handler.onEvents(batch)` | export | Receives a batch of events and returns submissions. Required on the server. |
| `Event`, `Submission` | `@ilium/plugin-sdk` | Types. `Submission` has `intent`, `correlation` and `dedupKey`. |
| `AgentActivity` | `@ilium/plugin-sdk/types` | String union: `'none'`, `'working'`, `'idle'`, `'awaiting-approval'`, `'done'`, `'menu'`. |
| `AgentActivityEvent` | `@ilium/plugin-sdk/types` | Payload with `node: bigint`, `previous`, `current` and `revision: bigint`. |
| `SetDecorationSpec`, `DecorationKey` | `@ilium/plugin-sdk/types` | Payloads of the `set-decoration` and `clear-decoration` intents. |
| `span(text, style?)`, `role(name)` | `@ilium/plugin-sdk/types` | Build styled text. `role` takes a colour role name such as `'accent'`. |
| `log(level, message)` | `@ilium/plugin-sdk/host/log` | Writes one line to the plugin log. |

### The pure logic

`badge.ts` is the port of `badge.rs`. It contains no host calls. The type import is erased at build time. The Rust original is listed after the TypeScript file, in the section "The Rust version of the same function".

**TypeScript**

```typescript
// src/badge.ts. Pure logic: type imports only, no host calls.
import type { AgentActivity } from "@ilium/plugin-sdk/types";

export type BadgeRole = "accent" | "muted" | "warning";

export interface Badge {
  label: string;
  role: BadgeRole;
}

/** Returns the badge for an activity, or undefined when the row needs no badge. */
export function badgeFor(activity: AgentActivity): Badge | undefined {
  switch (activity) {
    case "working":
      return { label: "working", role: "accent" };
    case "idle":
      return { label: "idle", role: "muted" };
    case "awaiting-approval":
      return { label: "approve?", role: "warning" };
    case "none":
    case "done":
    case "menu":
      return undefined;
    default:
      return assertNever(activity);
  }
}

/** A new activity in the generated bindings is a type error here, not a silent fallback. */
function assertNever(value: never): never {
  throw new Error(`unhandled activity: ${String(value)}`);
}
```

The tests run with `bun test` in TypeScript and with `cargo test` in Rust. They cover the same three cases.

**TypeScript**

```typescript
// src/badge.test.ts
import { expect, test } from "bun:test";
import { badgeFor } from "./badge";

test("working agent gets accent badge", () => {
  expect(badgeFor("working")).toEqual({ label: "working", role: "accent" });
});

test("awaiting approval gets warning badge", () => {
  expect(badgeFor("awaiting-approval")?.role).toBe("warning");
});

test("none, done and menu have no badge", () => {
  expect(badgeFor("none")).toBeUndefined();
  expect(badgeFor("done")).toBeUndefined();
  expect(badgeFor("menu")).toBeUndefined();
});
```

**Rust**

```rust
// src/badge.rs, test module. The same three cases as src/badge.test.ts.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_agent_gets_accent_badge() {
        let badge = badge_for(AgentActivity::Working).expect("working has a badge");
        assert_eq!(badge.label, "working");
        assert_eq!(badge.role, BadgeRole::Accent);
    }

    #[test]
    fn awaiting_approval_gets_warning_badge() {
        assert_eq!(badge_for(AgentActivity::AwaitingApproval).expect("badge").role, BadgeRole::Warning);
    }

    #[test]
    fn none_done_and_menu_have_no_badge() {
        assert!(badge_for(AgentActivity::None).is_none());
        assert!(badge_for(AgentActivity::Done).is_none());
        assert!(badge_for(AgentActivity::Menu).is_none());
    }
}
```

### The host glue

`server.ts` imports the SDK and turns each activity change into an intent. The names of the generated tags keep the WIT spelling, so the tag is `'agent-activity-changed'`. The Rust glue (step 1) matches `Event::AgentActivityChanged` instead, and both builds submit the same intent.

**TypeScript**

```typescript
// src/server.ts. Server side of agent-tools, the TypeScript port of step 1.
import { definePlugin, type Event, type Submission } from "@ilium/plugin-sdk";
import { log } from "@ilium/plugin-sdk/host/log";
import {
  role,
  span,
  type AgentActivityEvent,
  type DecorationKey,
  type SetDecorationSpec,
} from "@ilium/plugin-sdk/types";
import { badgeFor, type Badge } from "./badge";

const BADGE_SLOT = "badge";

export const { lifecycle, handler } = definePlugin({
  side: "server",
  activate(context) {
    log("info", `agent-tools active in ${context.plugin}`);
  },
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag === "agent-activity-changed") {
        submissions.push(decorate(event.val));
      }
    }
    return submissions;
  },
});

/** Decorates one agent row. An activity without a badge clears the slot. */
function decorate(change: AgentActivityEvent): Submission {
  const badge = badgeFor(change.current);
  const intent = badge
    ? { tag: "set-decoration" as const, val: setDecorationFor(change.node, badge) }
    : { tag: "clear-decoration" as const, val: clearFor(change.node) };
  // The revision is a u64, so it is a bigint. Plugin correlation numbers stay below 2^63.
  return { intent, correlation: change.revision, dedupKey: undefined };
}

function setDecorationFor(node: bigint, badge: Badge): SetDecorationSpec {
  return {
    node,
    slot: BADGE_SLOT,
    content: [span(badge.label, { fg: role(badge.role) })],
    icon: undefined,
    sortKey: undefined,
    persist: false,
  };
}

function clearFor(node: bigint): DecorationKey {
  return { node, slot: BADGE_SLOT };
}
```

**Rust**

```rust
// src/lib.rs. Server side of agent-tools, the Rust original (step 1, shortened).
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::intents::{DecorationKey, SetDecorationSpec};
use ilium_plugin_sdk::types::text::{ColorRole, Style};

mod badge;
use badge::{badge_for, BadgeRole};

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct AgentTools;

impl Lifecycle for AgentTools {}

impl Handler for AgentTools {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        batch
            .into_iter()
            .filter_map(|event| match event {
                Event::AgentActivityChanged(change) => Some(decorate(change)),
                _ => None,
            })
            .collect()
    }
}

fn color_role(role: BadgeRole) -> ColorRole {
    match role {
        BadgeRole::Accent => ColorRole::Accent,
        BadgeRole::Muted => ColorRole::Muted,
        BadgeRole::Warning => ColorRole::Warning,
    }
}

fn decorate(change: AgentActivityEvent) -> Submission {
    let intent = match badge_for(change.current) {
        Some(badge) => Intent::SetDecoration(SetDecorationSpec {
            node: change.node,
            slot: "badge".into(),
            content: vec![Span::styled(badge.label, Style::role(color_role(badge.role)))],
            icon: None,
            sort_key: None,
            persist: false,
        }),
        None => Intent::ClearDecoration(DecorationKey { node: change.node, slot: "badge".into() }),
    };
    Submission { intent, correlation: change.revision, dedup_key: None }
}
```

Points to notice:

- In TypeScript `node` is a `bigint`; in Rust it is a `u64`. WIT `u64` maps to `bigint` in the TypeScript binding. `Number(node)` loses precision above 2^53, so the code never converts it.
- `dedupKey` is `undefined`. Decorations need no de-duplication, as in step 1.
- `persist: false` keeps the badge out of the snapshot. A restart clears the badges until the next activity change.
- `clear-decoration` on an empty slot returns `ok`, so the code does not check the slot first.

### The Rust version of the same function

**Rust**

The Rust `badge_for` uses the SDK's `AgentActivity` enum and a `match`. The compiler proves that every case is handled, as the `assertNever` line does in TypeScript.

```rust
// src/badge.rs. Same logic as src/badge.ts.
use ilium_plugin_sdk::types::tree::AgentActivity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeRole {
    Accent,
    Muted,
    Warning,
}

pub struct Badge {
    pub label: &'static str,
    pub role: BadgeRole,
}

pub fn badge_for(activity: AgentActivity) -> Option<Badge> {
    match activity {
        AgentActivity::Working => Some(Badge { label: "working", role: BadgeRole::Accent }),
        AgentActivity::Idle => Some(Badge { label: "idle", role: BadgeRole::Muted }),
        AgentActivity::AwaitingApproval => {
            Some(Badge { label: "approve?", role: BadgeRole::Warning })
        }
        AgentActivity::None | AgentActivity::Done | AgentActivity::Menu => None,
    }
}
```

The two versions differ in three places. Rust returns `Option<Badge>` and TypeScript returns `Badge | undefined`. Both use the same three badge roles (`BadgeRole`); the Rust version maps them to a `ColorRole` for `Style::role(...)`, and TypeScript passes the role name to `role()`. TypeScript reports a new activity only in a type check, not at run time, because the bindings are generated from the same WIT as the Rust enum.

### The table rows of step 3

The client panel of step 3 builds table rows. `tableRow` is pure text formatting, so it ports directly. The panel layout stays in the Rust client. To use the port, add the client side to the project and import `tableRow` from `src/client.ts`. The Rust original is listed below the TypeScript file.

**TypeScript**

```typescript
// src/table.ts. Row text for the agents panel, the port of step 3.
export interface AgentRow {
  name: string;
  activity: string;
  ageSeconds: number;
}

/** Formats one agent row. The age column reads "42 s" or "7 min". */
export function tableRow(row: AgentRow): string[] {
  const age =
    row.ageSeconds < 60 ? `${row.ageSeconds} s` : `${Math.floor(row.ageSeconds / 60)} min`;
  return [row.name, row.activity, age];
}
```

**Rust**

```rust
// src/table.rs. Row text for the agents panel (step 3).
pub struct AgentRow {
    pub name: String,
    pub activity: String,
    pub age_seconds: u64,
}

/// Formats one agent row. The age column reads "42 s" or "7 min".
pub fn table_row(row: &AgentRow) -> Vec<String> {
    let age = if row.age_seconds < 60 {
        format!("{} s", row.age_seconds)
    } else {
        format!("{} min", row.age_seconds / 60)
    };
    vec![row.name.clone(), row.activity.clone(), age]
}
```

### Build and test

Install the packages, run the tests, then build the component.

```bash
bun install
bun test
ilium plugin build agent-tools-ts
```

`ilium plugin build` prints JSONL, one object per line. Its `artifact` line gives the path and the SHA-256 of `server.wasm`, which is written next to `manifest.json`. Each author runs this build on their own machine. Then `ilium plugin install` verifies the inventory before it installs the plugin.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin build agent-tools-ts
{"type":"progress","step":"bundle","message":"Bundling src/server.ts with Bun"}
{"type":"progress","step":"component","message":"Componentizing the bundle"}
{"type":"artifact","path":"server.wasm","sha256":"64 hex digits","bytes":48213}
{"type":"result","ok":true}
{"type":"summary","duration_ms":1840,"warnings":0,"errors":0}
</pre>
<figcaption>Illustrative, not generated. The build of the TypeScript project, as JSONL lines.</figcaption>
</figure>

To replay events against the built component without the running app, use `ilium plugin test agent-tools-ts --fixture <file.json>`. The fixture holds `tree`, `panes`, `agents`, `settings` and `events`, with events written as `{"tag": "...", "val": ...}`. For trap positions in `src/`, build with `ilium plugin build --debug`, which keeps the bundle's source map.

### Edge cases and errors

| Situation | Result |
|---|---|
| Activity is `none`, `done` or `menu` | `clear-decoration` for the badge slot, which returns `ok`. |
| The node was removed before the submission is accepted | `refused(not-found)`. |
| The slot `badge` is not declared in the manifest | `refused(invalid-argument)`. Declare it, as above. |
| More than 4,096 decorations for one server instance (default limit) | `refused(quota-exceeded)`. |
| `onEvents` runs longer than 20 ms (default `event_deadline_ms`) | Trap. The host restarts the plugin under the restart rule. |
| An exception is thrown inside `onEvents` | Trap, handled the same way. Catch errors that the handler can expect. |
| A new activity appears in a later SDK version | A type error at `assertNever`, found at build time. |

The limits and their codes are listed in [errors and limits](/pages/reference-errors-and-limits/).

### What you see

Each badge appears on its row as soon as the activity changes. The text uses the accent, muted or warning colour of the active theme. The kernel draws the badge, and the plugin never draws anything itself. A row whose activity is `done` has no badge. The tree below is the same tree as step 1, drawn by the TypeScript build. The Rust build draws the identical tree.

<figure>
<pre class="term"><span class="t-hi">╭  ≡ ● · Projects──────────────────────────────────────────╮</span>
│▼ 📂 shop                                                 │
│› ▼ 📂 api                                                │
│› ›   🦀 <span class="t-ok">⠋</span> server  <span class="t-hi">working</span>                                │
│› ›   🖥️    tests  <span class="t-dim">idle</span>                                   │
│› ▼ 📂 web                                                │
│› ›   🦀 <span class="t-warn">✋</span> build  <span class="t-warn">approve?</span>                               │
│› ›   🦀 <span class="t-ok">🔔</span> review                                        │
╰──────────────────────────────────────────────────────────╯
</pre>
<figcaption>Illustrative, not generated. The tree panel with the real tree glyphs: <code>▼</code> expanded group, <code>›</code> once per depth, two spaces for a leaf. The now slot holds the kernel signal and the badge text follows the title in the colour of its role. The review row has no badge because its activity is done.</figcaption>
</figure>

### Using three more widgets from the port

Further widgets are available that a client view can draw without a plugin call per frame. This part adds a small view to the client side of the port. It shows the number of open agents as a `badge`, a `spinner` while an index job runs, and a `progress-row` for the job. Nothing here changes the server build above.

Needs: `ui.panels` on the client side (the manifest of step 3 already has it). The widget names, props and builder argument order are those of [the widgets index](/pages/widgets-index/): the required prop is the first argument, and every optional prop is a chained setter in Rust and a field of the props object in TypeScript.

**Rust**

```rust
// src/views.rs. Client side. ilium_plugin_sdk::ui::build::Tree; types::layout::{Constraint, Direction}; types::ui::{ProgressState, SpinnerFrames}; types::text::Tone
impl Views for AgentTools {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let open = tree.badge().count(self.open_agents).tone(Tone::Info).key("open");
        let busy = tree.spinner(SpinnerFrames::Braille).interval_ms(100).label("Indexing 412 sessions");
        let job = tree.progress_row("Indexing 412 sessions")
            .state(ProgressState::Running).percent(37.0).message("session 152 of 412").key("job");
        let root = tree.layout(Direction::Vertical, [Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)])
            .child(open).child(busy).child(job);
        tree.finish(root)
    }
}
```

**TypeScript**

```typescript
// src/client.ts. Client side of the port.
import { definePlugin } from "@ilium/plugin-sdk";
import { tree, layout, badge, spinner, progressRow } from "@ilium/plugin-sdk/ui/build";

export const { lifecycle, views } = definePlugin({
  side: "client",
  render(_view, _area) {
    return tree(
      layout({ direction: "vertical", constraints: [{ tag: "length", val: 1 }, { tag: "length", val: 1 }, { tag: "length", val: 1 }] }, [
        badge({ key: "open", count: openAgents, tone: "info" }),
        spinner("braille", { intervalMs: 100, label: "Indexing 412 sessions" }),
        progressRow("Indexing 412 sessions", { key: "job", state: "running", percent: 37, message: "session 152 of 412" }),
      ]),
    );
  },
});
```

The host steps the spinner frames itself, so the plugin is called only when `openAgents` or the job percent changes. With reduced motion the spinner shows one static frame. The view draws like this in the panel:

<figure>
<pre class="term"><span class="t-hi">╭  ≡ ● · Agents────────────────────────────────────────────╮</span>
│Open agents …………………………………………………………… <span class="t-hi">3</span>                     │
│                                                          │
│<span class="t-ok">⠋</span> Indexing 412 sessions                                   │
│                                                          │
│<span class="t-ok">▶ RUNNING  37%</span>  <span class="t-ok">███████████</span><span class="t-dim">░░░░░░░░░░░░░░░░░░░░</span>           │
╰──────────────────────────────────────────────────────────╯
</pre>
<figcaption>Illustrative, not generated. The count badge, the braille spinner and the progress row, using the progress-bar glyphs <code>▶</code>, <code>█</code> and <code>░</code> of the real client.</figcaption>
</figure>

<div class="note">

<p>The contract fixes the widget builders (<code>badge</code>, <code>spinner</code>, <code>progressRow</code>, <code>layout</code>) but show only their render bodies. The option name of the client entry point in TypeScript (<code>render</code> above, mirroring the server <code>onEvents</code>) follows the same rule as the server call.</p>

</div>

### See also

- [Tutorial step 1](/pages/tutorial-01-agent-badge/) is the Rust original.
- [Tutorial step 9: package and share](/pages/tutorial-09-package-and-share/) packs either build.
- [TypeScript SDK](/pages/typescript-sdk/) lists every subpath of `@ilium/plugin-sdk`.
- [Tree decoration capability](/pages/capability-tree-decorate/) covers slots, widths and limits.
- [Widgets index](/pages/widgets-index/) lists every widget family, including the status and feedback family.
- [Testing](/pages/testing/) describes the fake host of `@ilium/plugin-sdk/testing`.
- [Debugging](/pages/debugging/) explains how traps map back to source.
