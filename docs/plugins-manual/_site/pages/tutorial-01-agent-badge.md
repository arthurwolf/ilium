# 1. Agent badge

## 1. Agent badge

This step turns the template from the first plugin page into a real server plugin. It puts a badge on every agent row of the project tree: `working`, `idle` or `approve?`. The kernel sends the plugin batches of events. The plugin answers with submissions, and the kernel applies them. That event-in, intent-out loop is the core of every server plugin.

Tutorial plugin: `example.agent-tools`. Languages: Rust (target `wasm32-wasip2`) and TypeScript. Every code listing in this step is given twice, a **Rust** block and a **TypeScript** block, and both build the same component behaviour. Each later step is in the same project and builds on the code of this step.

### You need

- [Your first plugin in five minutes](/pages/getting-started-first-plugin/), which creates the project, `manifest.json` and `src/lib.rs`.
- The toolchain from [Toolchain](/pages/getting-started-toolchain/): Rust stable 1.90 or later with the `wasm32-wasip2` target. For the TypeScript version, Bun 1.2 or later (see also [TypeScript plugins](/pages/tutorial-08-typescript/)).
- Server grants: `agent.read` to receive `agent-activity-changed` events, and `tree.decorate` to set and clear decorations. Both are listed in the manifest in [The manifest](#the-manifest).
- At least one agent CLI, Claude Code or Codex, running in a pane of the project. Without an agent pane the plugin receives no activity events and draws nothing.

### What the step builds

The plugin listens for `agent-activity-changed` events. For each event it submits one intent:

- `set-decoration` when the new activity has a badge.
- `clear-decoration` when the new activity has no badge.

The decoration goes into the `badge` slot that the manifest declares. The kernel draws the slot on the tree row. The plugin never draws anything itself.

<figure>
<svg viewBox="0 0 400 176" role="img" aria-label="Events in, intents out loop for the agent badge">
  <title>Events in, intents out</title>
  <desc>The kernel emits agent-activity-changed. The handler maps the activity to a badge. The handler submits set-decoration or clear-decoration. The kernel applies it and redraws the tree row.</desc>
  <defs>
    <marker id="agent-badge-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="8" y="16" width="180" height="40" rx="4" fill="none" stroke="currentColor"/>
  <text x="98" y="40" font-size="11" text-anchor="middle" fill="currentColor">agent-activity-changed</text>
  <rect x="212" y="16" width="180" height="40" rx="4" fill="none" stroke="currentColor" stroke-dasharray="5 3"/>
  <text x="302" y="40" font-size="11" text-anchor="middle" fill="currentColor">handler: badge_for</text>
  <line x1="188" y1="36" x2="212" y2="36" stroke="currentColor" marker-end="url(#agent-badge-arrow)"/>
  <rect x="212" y="106" width="180" height="40" rx="4" fill="none" stroke="currentColor"/>
  <text x="302" y="130" font-size="11" text-anchor="middle" fill="currentColor">set or clear decoration</text>
  <line x1="302" y1="56" x2="302" y2="106" stroke="currentColor" marker-end="url(#agent-badge-arrow)"/>
  <rect x="8" y="106" width="180" height="40" rx="4" fill="none" stroke="currentColor"/>
  <text x="98" y="130" font-size="11" text-anchor="middle" fill="currentColor">Tree row, badge slot</text>
  <line x1="212" y1="126" x2="188" y2="126" stroke="currentColor" marker-end="url(#agent-badge-arrow)"/>
  <text x="8" y="168" font-size="11" fill="currentColor">Submissions return in the same order as the events.</text>
</svg>
<figcaption>Events in, intents out. The plugin asks the kernel to set the slot. The kernel draws the row.</figcaption>
</figure>

### The surface this step uses

| Name | Kind | Fields or values | Rust path (TypeScript: same module under `@ilium/plugin-sdk/types`) | Grant |
|---|---|---|---|---|
| `agent-activity-changed` | event | `node: node-id`, `previous`, `current`: `agent-activity`, `revision: u64` | `types::events::AgentActivityEvent` | `agent.read` |
| `agent-activity` | enum | `none`, `working`, `idle`, `awaiting-approval`, `done`, `menu` | `types::tree::AgentActivity` | none |
| `set-decoration` | intent | `node`, `slot: string`, `content: styled-text`, `icon: option<string>`, `sort-key: option<s64>`, `persist: bool` | `types::intents::SetDecorationSpec` | `tree.decorate` |
| `clear-decoration` | intent | `node`, `slot` | `types::intents::DecorationKey` | `tree.decorate` |
| `submission` | record | `intent`, `correlation: u64`, `dedup-key: option<string>` | `types::intents::Submission` | none |
| `span` | record | `text: string`, `style: style` | `types::text::Span` (in the prelude) | none |
| `style` | record | `fg`, `bg`: option color, `add`, `remove`: modifiers | `types::text::Style` | none |
| `color-role` | enum | `foreground`, `muted`, `accent`, `success`, `warning`, `danger`, `info`, `selection`, `border` | `types::text::ColorRole` | none |
| `decoration_slots` | manifest | `id`, `placement` (`badge`, `icon`, `subtitle`, `footer`), `priority` | not code | none |
| `activate` | lifecycle export | receives `Activation` with `plugin`, `granted`, `restored` | `Lifecycle::activate` | none |
| `on_events` | handler export | `Vec<Event>` in, `Vec<Submission>` out | `Handler::on_events` | none |

`ilium_plugin_sdk::prelude::*` (TypeScript: the named imports of `@ilium/plugin-sdk`) brings in `Event`, `Intent`, `Submission`, `Span`, `PluginError` and the export traits. The other names come from the paths in the table; [the API pages](/pages/api-events/) list each module. The `icon` field takes a Tabler icon name. This step passes `None`.

### Files

The project has two source files. `badge.rs` (or `badge.ts`) holds the pure logic, with no host calls, so `cargo test` (or `bun test`) can run it. `lib.rs` (or `server.ts`) holds the host glue. The manifest is the one from the first plugin page, extended with the `badge` slot.

**Rust**

```text
agent-tools/
  manifest.json
  Cargo.toml
  src/badge.rs
  src/lib.rs
```

**TypeScript**

```text
agent-tools/
  manifest.json
  package.json
  src/badge.ts
  src/badge.test.ts
  src/server.ts
```

### The pure logic

`badge.rs` and `badge.ts` map an activity to a label and a colour role. They do not use the host, so the same function runs on a development machine.

**Rust**

```rust
// src/badge.rs. Pure logic: no host calls, no bindings.

/// The activity values this plugin cares about. The glue maps the SDK's
/// `AgentActivity` onto this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
    None,
    Working,
    Idle,
    AwaitingApproval,
    Done,
    Menu,
}

/// Which theme colour role the badge text uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeRole {
    Accent,
    Muted,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Badge {
    pub label: &'static str,
    pub role: BadgeRole,
}

/// Returns the badge for an activity, or `None` when the row needs no badge.
pub fn badge_for(activity: ActivityKind) -> Option<Badge> {
    match activity {
        ActivityKind::Working => Some(Badge { label: "working", role: BadgeRole::Accent }),
        ActivityKind::Idle => Some(Badge { label: "idle", role: BadgeRole::Muted }),
        ActivityKind::AwaitingApproval => {
            Some(Badge { label: "approve?", role: BadgeRole::Warning })
        }
        ActivityKind::None | ActivityKind::Done | ActivityKind::Menu => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_agent_gets_accent_badge() {
        let badge = badge_for(ActivityKind::Working).expect("working has a badge");
        assert_eq!(badge.label, "working");
        assert_eq!(badge.role, BadgeRole::Accent);
    }

    #[test]
    fn awaiting_approval_gets_warning_badge() {
        let badge = badge_for(ActivityKind::AwaitingApproval).expect("badge");
        assert_eq!(badge.role, BadgeRole::Warning);
    }

    #[test]
    fn none_and_done_have_no_badge() {
        assert!(badge_for(ActivityKind::None).is_none());
        assert!(badge_for(ActivityKind::Done).is_none());
    }
}
```

**TypeScript**

```ts
// src/badge.ts. Pure logic: no host calls, no bindings.
export type ActivityKind =
  | 'none'
  | 'working'
  | 'idle'
  | 'awaiting-approval'
  | 'done'
  | 'menu';

export type BadgeRole = 'accent' | 'muted' | 'warning';

export interface Badge {
  label: string;
  role: BadgeRole;
}

/** Returns the badge for an activity, or undefined when the row needs no badge. */
export function badgeFor(activity: ActivityKind): Badge | undefined {
  switch (activity) {
    case 'working':
      return { label: 'working', role: 'accent' };
    case 'idle':
      return { label: 'idle', role: 'muted' };
    case 'awaiting-approval':
      return { label: 'approve?', role: 'warning' };
    default:
      return undefined;
  }
}
```

```ts
// src/badge.test.ts
import { expect, test } from 'bun:test';
import { badgeFor } from './badge';

test('working agent gets an accent badge', () => {
  expect(badgeFor('working')).toEqual({ label: 'working', role: 'accent' });
});

test('awaiting approval gets a warning badge', () => {
  expect(badgeFor('awaiting-approval')?.role).toBe('warning');
});

test('none and done have no badge', () => {
  expect(badgeFor('none')).toBeUndefined();
  expect(badgeFor('done')).toBeUndefined();
});
```

### The host glue

`lib.rs` and `server.ts` declare the plugin struct and implements the two traits the SDK exports: `Lifecycle` and `Handler`. The `#[ilium_plugin]` attribute (Rust) and `definePlugin` (TypeScript) generate the export glue.

**Rust**

```rust
// src/lib.rs. Server side of agent-tools, step 1.
mod badge;

use badge::{badge_for, ActivityKind, Badge, BadgeRole};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::intents::{DecorationKey, SetDecorationSpec};
use ilium_plugin_sdk::types::text::{ColorRole, Style};
use ilium_plugin_sdk::types::tree::AgentActivity;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct AgentTools;

impl Lifecycle for AgentTools {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        log::log(Level::Info, "agent-tools active");
        Ok(())
    }
}

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

/// Decorates one agent row. An activity without a badge clears the slot.
fn decorate(change: AgentActivityEvent) -> Submission {
    let intent = match badge_for(to_kind(change.current)) {
        Some(badge) => Intent::SetDecoration(SetDecorationSpec {
            node: change.node,
            slot: "badge".into(),
            content: badge_spans(&badge),
            icon: None,
            sort_key: None,
            persist: false,
        }),
        None => Intent::ClearDecoration(DecorationKey {
            node: change.node,
            slot: "badge".into(),
        }),
    };
    Submission {
        intent,
        correlation: change.revision,
        dedup_key: None,
    }
}

fn badge_spans(badge: &Badge) -> Vec<Span> {
    vec![Span::styled(badge.label, Style::role(color_role(badge.role)))]
}

fn color_role(role: BadgeRole) -> ColorRole {
    match role {
        BadgeRole::Accent => ColorRole::Accent,
        BadgeRole::Muted => ColorRole::Muted,
        BadgeRole::Warning => ColorRole::Warning,
    }
}

fn to_kind(activity: AgentActivity) -> ActivityKind {
    match activity {
        AgentActivity::None => ActivityKind::None,
        AgentActivity::Working => ActivityKind::Working,
        AgentActivity::Idle => ActivityKind::Idle,
        AgentActivity::AwaitingApproval => ActivityKind::AwaitingApproval,
        AgentActivity::Done => ActivityKind::Done,
        AgentActivity::Menu => ActivityKind::Menu,
    }
}
```

**TypeScript**

```ts
// src/server.ts. Server side of agent-tools, step 1.
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';
import { role, span } from '@ilium/plugin-sdk/types';
import { badgeFor, type ActivityKind } from './badge';

type ActivityChange = { node: bigint; current: ActivityKind; revision: bigint };

function decorate(change: ActivityChange): Submission {
  const badge = badgeFor(change.current);
  const intent = badge
    ? {
        tag: 'set-decoration' as const,
        val: {
          node: change.node,
          slot: 'badge',
          content: [span(badge.label, { fg: role(badge.role) })],
          icon: undefined,
          sortKey: undefined,
          persist: false,
        },
      }
    : { tag: 'clear-decoration' as const, val: { node: change.node, slot: 'badge' } };
  return { intent, correlation: change.revision, dedupKey: undefined };
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate() {
    log('info', 'agent-tools active');
  },
  onEvents(batch: Event[]): Submission[] {
    const out: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'agent-activity-changed') out.push(decorate(event.val));
    }
    return out;
  },
});
```

Points to notice:

- `activate` logs one line. The kernel does not replay old events. A restarted plugin receives one `tree-changed` batch for all nodes and reads what it needs from that.
- `decorate` returns exactly one submission per event. The kernel checks a batch as a whole, so one malformed submission refuses every other submission in the batch. Keep the builder simple.
- `correlation` is set to the event's `revision`, so a later reader can match the outcome to the change. Numbers below 2^63 belong to the plugin. If you leave the field to `Submission::new(intent)`, the SDK picks the next number instead.
- `persist: false` keeps the badge out of the saved snapshot. Badges are derived from live activity, so this is the right choice here.
- The TypeScript `role(...)` helper and the Rust `Style::role(...)` build the same style record: a colour role from the theme, not a fixed colour, so the badge follows the user's colour scheme.

### The manifest

The manifest requests `agent.read` and `tree.decorate` on the server side. It declares the `badge` slot under `contributes`, so the kernel knows where a decoration may go.

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

**Rust**: `Cargo.toml` has the SDK as a normal dependency. The `testing` feature goes in `[dev-dependencies]`, so the wasm build never includes it, and only `cargo test` enables it:

```toml
[package]
name = "agent-tools"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
ilium-plugin-sdk = "0.1"

[dev-dependencies]
ilium-plugin-sdk = { version = "0.1", features = ["testing"] }
```

**TypeScript**: `package.json` pins the SDK. The build dependencies come from `bun install`, and `bun test` needs no extra package:

```json
{
  "name": "agent-tools",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "dependencies": { "@ilium/plugin-sdk": "0.1.0" }
}
```

### Extended decoration

`set-decoration` has optional fields: `spans` (rich text with a tone per span), `tooltip`, `frames` and `frame-ms` for a host-stepped animation, `trailing` to draw the decoration at the end of the row, `visibility` (`always` or `on-hover`) and `tone`. The host steps the frames itself, so a spinner costs no plugin call per frame, and reduced motion shows the first frame. This change shows a hover tooltip on the working badge and a trailing spinner:

**Rust**

```rust
Intent::SetDecoration(SetDecorationSpec {
    node: change.node,
    slot: "badge".into(),
    content: badge_spans(&badge),
    icon: None,
    sort_key: None,
    persist: false,
    // extended fields
    tooltip: Some(Tooltip {
        lines: vec![vec![Span::plain("The agent is running tools")]],
        delay_ms: None,
    }),
    frames: ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
        .iter()
        .map(|glyph| vec![Span::plain(*glyph)])
        .collect(),
    frame_ms: 120,
    trailing: true,
    ..Default::default()
})
```

**TypeScript**

```ts
{
  tag: 'set-decoration',
  val: {
    node: change.node,
    slot: 'badge',
    content: [span(badge.label, { fg: role(badge.role) })],
    icon: undefined,
    sortKey: undefined,
    persist: false,
    // extended fields
    tooltip: { lines: [[span('The agent is running tools')]], delayMs: undefined },
    frames: ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'].map((glyph) => [span(glyph)]),
    frameMs: 120,
    trailing: true,
  },
}
```

The fields are the WIT record fields in both languages (`frames` is a list of styled text, `frame-ms` is at least 90). The tutorial keeps the plain badge in the following steps. `spans`, `visibility` and `tone` are the other new fields.

### Edge cases and errors

The kernel answers every submission with one outcome. These are the outcomes this step can meet:

| Outcome | When it happens | What the plugin sees |
|---|---|---|
| `refused(not-permitted)` | The `tree.decorate` grant is missing. The batch is refused as a whole. | The offending submission gets this code. The others get `refused(cancelled)`. |
| `refused(invalid-argument)` | The `slot` is not declared in `decoration_slots`, or a submission is malformed. | Same batch rule as above. |
| `refused(not-found)` | The node was removed before the submission was accepted. | Only that submission is refused. |
| `failed(not-found)` | The node was removed by an earlier submission of the same batch, or before the kernel applied it. | The submission was accepted but could not be applied. |
| `refused(quota-exceeded)` | The plugin has 4,096 decorations on the server side. | Only that submission is refused. |
| trap | The handler returns more than 256 submissions, or runs longer than 20 ms. | The instance ends. The kernel restarts it after 1 s, then 5 s; the third failure in 10 minutes disables it. |

Other points that affect what the user sees:

- Clearing a slot that has no decoration is accepted and changes nothing.
- A badge longer than 12 cells is cut, and the cut text ends with an ellipsis. The labels in this step are shorter.
- Decorations from `persist: false` are dropped when the server restarts. This step does not read agent state on start, so a row gets its badge back when its activity next changes.
- When only the plugin restarts after a crash (the server keeps running), the plugin keeps its decorations, marked stale. A server restart is the case of the previous item.
- If `tree.decorate` is revoked, the plugin receives `deactivate(revoked)`, and its decorations are withdrawn.

### Test it

Run the unit tests of the pure logic. Then run the plugin in development mode and start an agent in a pane of the project, so the events reach the handler.

**Rust**

```bash
cargo test badge
ilium plugin dev
```

**TypeScript**

```bash
bun test
ilium plugin dev
```

`ilium plugin dev` watches the project folder. After each change it rebuilds the component and reloads it. A failed build keeps the running instance and prints the compiler error. The dev package carries a `dev` badge in the plugin list, because it is an unsigned local package.

The Rust SDK builds for the host target with the `testing` feature. `cargo test` therefore links `lib.rs` without a wasm target, and the `testing` feature replaces the host calls with a fake host. For tests that deliver events to the handler, see [Testing](/pages/testing/).

### What you see

The render shows the tree after four agents change state. Each row shows its badge. The `Release notes` row has no badge, because its activity is `done`, and `done` clears the slot. The `done` state gets its own handling in step 4.

<figure>
<pre class="term">╭  ≡ ● · Sessions ───────────────────────────────╮
│ ▼ 📂 shop                                      │
│ › ▼ 📂 api                                     │
│ ››   🦀 <span class="t-ok">⠋</span> Fix login redirect <span class="t-ok">working</span>           │
│ ››   🐢 <span class="t-dim">●</span> Review schema <span class="t-dim">idle</span>                   │
│ ››   🖥️    Shell                               │
│ › ▼ 📂 web                                     │
│ ››   🦀 <span class="t-warn">✋</span> Build pipeline <span class="t-warn">approve?</span>             │
│ ››   🐢 <span class="t-ok">🔔</span> Release notes                       │
│                                                │
╰────────────────────────────────────────────────╯
 <span class="t-dim">accent: working · muted: idle · warning: approve?</span>
</pre>
<figcaption>Illustrative, not generated. The kernel draws the identity icon, the Now column (spinner, hand, bell, dot) and the title. The plugin only adds the badge text after the title. The release notes row has no badge because its activity is done.</figcaption>
</figure>

### See also

- [Tutorial step 2: command, keybinding and settings](/pages/tutorial-02-command-and-settings/) adds a command that turns the badges off.
- [Agent read capability](/pages/capability-agent-read/) lists the agent events and their delivery rules.
- [Tree decoration capability](/pages/capability-tree-decorate/) describes the slots, widths and limits of decorations.
- [Primitives: agent](/pages/primitives-agent/) describes the agent activity values.
- [Events](/pages/api-events/) and [Lifecycle](/pages/lifecycle/) explain the event loop and the activate and deactivate exports.
- [Debugging](/pages/debugging/) explains how to read plugin logs and trap backtraces.
- [What a plugin is](/pages/getting-started-overview/) explains the event and intent loop.

API checked against `ilium:plugin@0.1.0`.
