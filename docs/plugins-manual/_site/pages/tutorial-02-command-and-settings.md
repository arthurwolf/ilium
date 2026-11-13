# 2. Command, keybinding and settings

## 2. Command, keybinding and settings

This step lets the user turn the agent badges off and on, and hide the badge of idle agents. It adds three things to the plugin: a command, a keybinding that runs the command, and a settings schema with two switches. The host builds the settings page from the schema, and each value saves as the user changes it. The plugin reads the settings at the start of every event batch, so a change takes effect at once and no restart is needed.

The plugin never reads the keyboard. A keybinding names a command, and the kernel sends the plugin a `command-invoked` event. The same command also runs from the palette, a menu or the `ilium` CLI, with no change to the plugin.

Tutorial plugin: `example.agent-tools`. Languages: Rust (target `wasm32-wasip2`) and TypeScript. Every listing is given in both.

### You will need

- [Tutorial step 1](/pages/tutorial-01-agent-badge/) finished. This step starts from its code.
- Grants, server side: none new. `settings` for the plugin's own values is always granted. Own commands need no grant to be invoked (the command grant `commands.invoke` is only for commands of other plugins).
- Crates: `serde` and `serde_json`, added to `Cargo.toml` to parse the settings JSON.
- Tab id: `agent-tools`. The settings entry names it in `id`, and the host gives the tab the global id `example.agent-tools/agent-tools`. The entry is a `kind` `tab` with one section, and that section holds two toggles, `show_badges` and `show_idle`.

### What changed from step 1

- `src/badge.rs` is unchanged. `Cargo.toml` adds `serde` and `serde_json`.
- `src/lib.rs` is rewritten to read the settings, handle the command and refresh the badges.
- `manifest.json` adds `commands`, `keybindings` and `settings` under `contributes`.

### The surface this step uses

| Area | Name | Type or form | Meaning |
|---|---|---|---|
| Manifest | `contributes.commands[]` | `id`, `title`, `palette`, `arguments_schema`, `icon`, `when`, `voice` | Local `id`; global id `<plugin-id>/<id>`. `palette` defaults to `true`. |
| Manifest | `contributes.keybindings[]` | `command`, `chord`, `mode` | Lowercase `chord`. `prefix` means the key after the prefix key. Default mode `global`. |
| Manifest | `contributes.settings[]` | `kind`, `id`, `title`, `sections[]` (`id`, `title`, `scope`, `rows`) | `kind` `tab` adds a rail page. `scope` is `server` or `client`, default `client`. |
| Host, Rust | `settings::get_json()` | `-> String` | Own values of the side; `{}` when none. |
| Host, Rust | `settings::set_json(patch)` | `&str -> Result<(), PluginError>` | Applies an RFC 7396 merge patch. |
| Event | `command-invoked` | `CommandInvokedEvent { command, arguments_json, origin, context }` | `origin`: `keybinding`, `palette`, `toolbar`, `menu`, `voice` or `cli`. |
| Event | `settings-changed` | `SettingsChange { keys, origin, scope }` | Sent for every change, including the plugin's own `set_json`. |

Schema keywords used here: `type`, `title`, `description` and `default`. Any keyword outside the allowed set fails the install with `manifest.schema-keyword`.

### The flow

A key press reaches the plugin as a command. The plugin saves the new value with `settings::set_json`. The kernel then sends `settings-changed` with the changed keys. The plugin refreshes every agent row, so the badges change without a restart.

<figure>
<svg viewBox="0 0 400 300" role="img" aria-label="Command dispatch from key to plugin and back">
  <title>Command dispatch from key to plugin and back</title>
  <desc>The user presses the prefix key and b. The kernel resolves the keybinding to the command id. The plugin receives command-invoked and saves the setting. The kernel then sends settings-changed, and the plugin refreshes the badges.</desc>
  <defs>
    <marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="50" y="10" width="300" height="36" rx="4" fill="none" stroke="currentColor"/>
  <text x="200" y="32" font-size="11" text-anchor="middle" fill="currentColor">Keybinding: prefix, then b</text>
  <line x1="200" y1="46" x2="200" y2="60" stroke="currentColor" marker-end="url(#arrow)"/>
  <rect x="50" y="60" width="300" height="36" rx="4" fill="none" stroke="currentColor"/>
  <text x="200" y="82" font-size="11" text-anchor="middle" fill="currentColor">Command: example.agent-tools/toggle-badges</text>
  <line x1="200" y1="96" x2="200" y2="110" stroke="currentColor" marker-end="url(#arrow)"/>
  <rect x="50" y="110" width="300" height="36" rx="4" fill="none" stroke="currentColor" stroke-dasharray="5 3"/>
  <text x="200" y="132" font-size="11" text-anchor="middle" fill="currentColor">Event: command-invoked (origin keybinding)</text>
  <line x1="200" y1="146" x2="200" y2="160" stroke="currentColor" marker-end="url(#arrow)"/>
  <rect x="50" y="160" width="300" height="36" rx="4" fill="none" stroke="currentColor" stroke-dasharray="5 3"/>
  <text x="200" y="182" font-size="11" text-anchor="middle" fill="currentColor">Plugin: settings::set_json(show_badges)</text>
  <line x1="200" y1="196" x2="200" y2="210" stroke="currentColor" marker-end="url(#arrow)"/>
  <rect x="50" y="210" width="300" height="36" rx="4" fill="none" stroke="currentColor" stroke-dasharray="5 3"/>
  <text x="200" y="232" font-size="11" text-anchor="middle" fill="currentColor">Event: settings-changed (show_badges)</text>
  <line x1="200" y1="246" x2="200" y2="260" stroke="currentColor" marker-end="url(#arrow)"/>
  <rect x="50" y="260" width="300" height="36" rx="4" fill="none" stroke="currentColor"/>
  <text x="200" y="282" font-size="11" text-anchor="middle" fill="currentColor">Plugin: refresh every agent row</text>
</svg>
<figcaption>Key to command to settings to refresh. The plugin never reads the key itself.</figcaption>
</figure>

### Manifest

The manifest gains three contributions. The command id is local (`toggle-badges`), so its global id is `example.agent-tools/toggle-badges`. The keybinding names that global id. The settings entry adds the tab `agent-tools`, with one section that holds two toggle rows. They are saved as `show_badges` and `show_idle`, and both default to on.

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
    ],
    "commands": [
      { "id": "toggle-badges", "title": "Toggle agent badges" }
    ],
    "keybindings": [
      { "command": "example.agent-tools/toggle-badges", "chord": "b", "mode": "prefix" }
    ],
    "settings": [
      {
        "kind": "tab",
        "id": "agent-tools",
        "title": "Agent tools",
        "sections": [
          {
            "id": "badges",
            "title": "Badges",
            "scope": "server",
            "rows": [
              {
                "type": "toggle",
                "key": "show_badges",
                "title": "Show agent badges",
                "description": "Turn every agent badge off with one switch.",
                "default": true
              },
              {
                "type": "toggle",
                "key": "show_idle",
                "title": "Show idle agents",
                "description": "Also show the badge of agents that are idle.",
                "default": true
              }
            ]
          }
        ]
      }
    ]
  }
}
```

Manifest rules that apply here:

- Row keys must be unique across all settings tabs and sections of the plugin. A duplicate fails the install with `manifest.settings-key-clash`. The tab and section ids must be unique in the plugin too, and a tab title must not equal a built-in tab name.
- Keybinding clashes resolve in this order: the user's remap, kernel built-ins, bundled plugins, then other plugins. The losing binding is inactive.

### Host glue, Rust

`lib.rs` is rewritten. The badge logic in `badge.rs` is reused, and `visible_badge` applies the two settings on top of it. A command or a settings change never draws anything itself. It sets a value or submits intents, and the kernel does the rest.

**Rust**

```rust
// src/lib.rs. Server side of agent-tools, step 2.
mod badge;

use badge::{badge_for, ActivityKind, Badge, BadgeRole};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::settings;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::agent;
use ilium_plugin_sdk::types::events::{AgentActivityEvent, CommandInvokedEvent};
use ilium_plugin_sdk::types::intents::{DecorationKey, SetDecorationSpec};
use ilium_plugin_sdk::types::text::ColorRole;
use ilium_plugin_sdk::types::tree::AgentActivity;
use serde::Deserialize;

const TOGGLE_COMMAND: &str = "example.agent-tools/toggle-badges";
const BADGE_SLOT: &str = "badge";

/// The plugin's own settings. A missing key takes its default, so a partial
/// JSON object from the host is still valid.
#[derive(Deserialize, Clone, Copy)]
#[serde(default)]
struct BadgeSettings {
    show_badges: bool,
    show_idle: bool,
}

impl Default for BadgeSettings {
    fn default() -> Self {
        Self { show_badges: true, show_idle: true }
    }
}

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
        let badge_settings = load_settings();
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::AgentActivityChanged(AgentActivityEvent { node, current, .. }) => {
                    submissions.push(decorate(node, to_kind(current), badge_settings));
                }
                Event::CommandInvoked(CommandInvokedEvent { command, .. })
                    if command == TOGGLE_COMMAND =>
                {
                    toggle_badges();
                }
                Event::SettingsChanged(change)
                    if change.keys.iter().any(|key| key.starts_with("show_")) =>
                {
                    submissions.extend(refresh_all(badge_settings));
                }
                _ => {}
            }
        }
        submissions
    }
}

/// Reads the saved values. A value that does not parse falls back to the defaults.
fn load_settings() -> BadgeSettings {
    serde_json::from_str(&settings::get_json()).unwrap_or_default()
}

/// Flips `show_badges` from the value saved at this moment. Each toggle reads
/// the store again, so two toggles in one batch each flip the value. The save
/// comes back as `settings-changed`, and that event refreshes the rows.
fn toggle_badges() {
    let current = load_settings();
    let patch = serde_json::json!({ "show_badges": !current.show_badges }).to_string();
    if let Err(error) = settings::set_json(&patch) {
        log::log(Level::Warn, &format!("could not save setting: {}", error.message));
    }
}

/// Re-decorates every agent pane with the current settings.
fn refresh_all(badge_settings: BadgeSettings) -> Vec<Submission> {
    agent::agents()
        .into_iter()
        .map(|entry| decorate(entry.pane, to_kind(entry.view.activity), badge_settings))
        .collect()
}

/// Returns the badge to show, or `None` when the row must stay clear.
fn visible_badge(activity: ActivityKind, badge_settings: BadgeSettings) -> Option<Badge> {
    if !badge_settings.show_badges {
        return None;
    }
    if activity == ActivityKind::Idle && !badge_settings.show_idle {
        return None;
    }
    badge_for(activity)
}

fn decorate(node: NodeId, activity: ActivityKind, badge_settings: BadgeSettings) -> Submission {
    let slot = BADGE_SLOT.to_string();
    let intent = match visible_badge(activity, badge_settings) {
        Some(badge) => Intent::SetDecoration(SetDecorationSpec {
            node,
            slot,
            content: vec![badge_span(&badge)],
            icon: None,
            sort_key: None,
            persist: false,
        }),
        None => Intent::ClearDecoration(DecorationKey { node, slot }),
    };
    Submission::new(intent)
}

fn badge_span(badge: &Badge) -> Span {
    Span::styled(badge.label, Style::role(color_role(badge.role)))
}

fn color_role(role: BadgeRole) -> ColorRole {
    match role {
        BadgeRole::Accent => ColorRole::Accent,
        BadgeRole::Muted => ColorRole::Muted,
        BadgeRole::Warning => ColorRole::Warning,
        BadgeRole::Success => ColorRole::Success,
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

Points to notice:

- The handler reads the settings once per batch. A toggle reads the store again, so it always flips the current value.
- `toggle_badges` does not refresh the rows. The save returns as `settings-changed`, and that one path refreshes the rows.
- `visible_badge` applies both switches. `show_badges` off clears every badge, and `show_idle` off clears only the badge of an idle agent. `ActivityKind` derives `PartialEq` in `badge.rs`.
- `starts_with("show_")` matches both keys, so a change to either switch refreshes every row once.
- `settings::set_json` validates the patch before storing it. A bad value returns `invalid-argument`, and the code logs it instead of failing the batch.

### Host glue, TypeScript

The TypeScript version has the same manifest and the same behaviour. It lives in `src/index.ts`. The badge table is the same as `badge.rs`, written as a small `switch`. The module names are the subpaths of `@ilium/plugin-sdk`.

**TypeScript**

```ts
// src/index.ts. Server side of agent-tools, step 2, in TypeScript.
import { definePlugin, PluginError, type Event, type Submission } from '@ilium/plugin-sdk';
import type { AgentActivity } from '@ilium/plugin-sdk/types';
import { log } from '@ilium/plugin-sdk/host/log';
import { getJson, setJson } from '@ilium/plugin-sdk/host/settings';
import { agents } from '@ilium/plugin-sdk/primitives/agent';

const TOGGLE_COMMAND = 'example.agent-tools/toggle-badges';
const BADGE_SLOT = 'badge';

type BadgeSettings = { showBadges: boolean; showIdle: boolean };

/** Maps the saved snake_case keys onto camelCase fields. Missing keys take the defaults. */
function loadSettings(): BadgeSettings {
  const stored = JSON.parse(getJson()) as { show_badges?: boolean; show_idle?: boolean };
  return { showBadges: stored.show_badges ?? true, showIdle: stored.show_idle ?? true };
}

function badgeFor(activity: AgentActivity, settings: BadgeSettings) {
  if (!settings.showBadges) return undefined;
  if (activity === 'idle' && !settings.showIdle) return undefined;
  switch (activity) {
    case 'working': return { label: 'working', role: 'accent' as const };
    case 'idle': return { label: 'idle', role: 'muted' as const };
    case 'awaiting-approval': return { label: 'approve?', role: 'warning' as const };
    default: return undefined;
  }
}

function decorate(node: bigint, activity: AgentActivity, settings: BadgeSettings): Submission {
  const badge = badgeFor(activity, settings);
  const intent = badge
    ? {
        tag: 'set-decoration' as const,
        val: {
          node,
          slot: BADGE_SLOT,
          content: [{
            text: badge.label,
            style: {
              fg: { tag: 'role' as const, val: badge.role },
              bg: undefined,
              add: {},
              remove: {},
            },
          }],
          icon: undefined,
          sortKey: undefined,
          persist: false,
        },
      }
    : { tag: 'clear-decoration' as const, val: { node, slot: BADGE_SLOT } };
  return { intent, correlation: 0n, dedupKey: undefined };
}

/** Flips show_badges from the value saved at this moment. */
function toggleBadges(): void {
  const current = loadSettings();
  try {
    setJson(JSON.stringify({ show_badges: !current.showBadges }));
  } catch (error) {
    if (error instanceof PluginError) log('warn', `could not save setting: ${error.message}`);
    else throw error;
  }
}

function refreshAll(settings: BadgeSettings): Submission[] {
  return agents().map((entry) => decorate(entry.pane, entry.view.activity, settings));
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate(context) {
    log('info', `agent-tools active in ${context.plugin}`);
  },
  onEvents(batch: Event[]): Submission[] {
    const settings = loadSettings();
    const out: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'agent-activity-changed') {
        out.push(decorate(event.val.node, event.val.current, settings));
      } else if (event.tag === 'command-invoked' && event.val.command === TOGGLE_COMMAND) {
        toggleBadges();
      } else if (event.tag === 'settings-changed') {
        if (event.val.keys.some((key) => key.startsWith('show_'))) out.push(...refreshAll(settings));
      }
    }
    return out;
  },
});
```

The TypeScript side uses `bigint` for every `u64`, so `node` and `correlation` are `bigint` values (`0n`). Variants use the `{ tag, val }` form, and enum values are strings such as `'awaiting-approval'`.

### The settings page

The host draws the tab from the manifest. Each toggle row has the label at the left, the value at the right, and grey leader dots between them. A toggle saves at once.

<figure>
<pre class="term">╭  ≡ ● · Agent tools · plugin───────────────────────────╮
│                                                       │
│<span class="t-dim">  ⧉ Added by example.agent-tools 0.1.0                 </span>│
│                                                       │
│<span class="t-hi">  BADGES                                               </span>│
│<span class="t-hi">  Show agent badges …………………………………………………………………… ‹ On ›  </span>│
│<span class="t-dim">    Turn every agent badge off with one switch.        </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│  Show idle agents ……………………………………………………………………… ‹ On ›  │
│<span class="t-dim">    Also show the badge of agents that are idle.       </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>The two rows of the tab, in the content frame (59 columns). The focused row is drawn in the accent colours. Leader dots are grey <code>…</code> with one blank cell on each side.</figcaption>
</figure>

<figure>
<pre class="term">╭  ≡ ● · Agent tools · plugin───────────────────────────╮
│                                                       │
│<span class="t-dim">  ⧉ Added by example.agent-tools 0.1.0                 </span>│
│                                                       │
│<span class="t-hi">  BADGES                                               </span>│
│  Show agent badges …………………………………………………………………… ‹ On ›  │
│<span class="t-dim">    Turn every agent badge off with one switch.        </span>│
│<span class="t-dim">  ──────────────────────                               </span>│
│<span class="t-hi">  Show idle agents • ……………………………………………………………… ‹ Off ›  </span>│
│<span class="t-dim">    Also show the badge of agents that are idle.       </span>│
╰───────────────────────────────────────────────────────╯</pre>
<figcaption>After the user turns <em>Show idle agents</em> off. The dim <code>•</code> after the label says that the value differs from the default. The save returns as <code>settings-changed</code> and the plugin clears the idle badges.</figcaption>
</figure>

The tab appears in the Settings rail with the title `Agent tools`, marked as a plugin tab. Its id is `agent-tools`, from the manifest.

### The command in the palette

The command is listed because `palette` defaults to `true`. Running it from the palette sends the same event as the keybinding, with `origin` set to `palette`.

<figure>
<pre class="term">╭  ≡ ● · Command palette───────────────────────────────────╮
│ badges                                                   │
│ Toggle agent badges                       prefix b       │
╰──────────────────────────────────────────────────────────╯</pre>
<figcaption>The palette lists the command under its title, with its keybinding at the right. The kernel draws the palette, not the plugin.</figcaption>
</figure>

### Test it

Run the unit tests, then the plugin live:

```bash
cargo test badge
ilium plugin dev
```

Press the prefix key and `b`. The badges disappear. Press the keys again and they return. Check the stored value in the settings page, and check failed saves with `ilium plugin logs example.agent-tools`. To replay events without the UI, use `ilium plugin test` with a fixture that holds a `settings` object and the events to deliver. The [Testing](/pages/testing/) page covers the fixture format and `FakeHost`.

### Edge cases and errors

| Case | What happens | Code or result |
|---|---|---|
| Agent changes state while badges are off | `visible_badge` returns `None`; the row is cleared | `clear-decoration`, `ok` |
| Agent goes idle while `show_idle` is off | `visible_badge` returns `None` for the idle activity; the row is cleared | `clear-decoration`, `ok` |
| Setting changes while no agent runs | `refresh_all` returns no submissions | No error |
| Patch holds a key not in the schema | Nothing is stored; the error names the key | `invalid-argument` |
| Patch breaks the schema (string for a boolean) | Nothing is stored; the error gives the JSON path | `invalid-argument` |
| Patch sets a key of the other side | Refused; `scope` decides the owning side | `invalid-argument` |
| Patch changes no stored value | Succeeds; no event is sent | `ok` |
| The command runs twice in one batch | Each run reads the store again and flips it | Two saves |
| Changes arrive together | One `settings-changed` may carry the union of keys | Coalesced delivery |
| Prefix key `b` is already bound | The losing binding is inactive | Listed under Settings > Keys |

A patch is limited to 256 KiB, which this step never reaches. Another plugin can change these values only with `settings.write-all`, a high-risk grant that only bundled plugins may hold.

### See also

- [Tutorial step 1](/pages/tutorial-01-agent-badge/) introduces the badge logic.
- [Tutorial step 3: a panel with a table](/pages/tutorial-03-panel/) adds a client side.
- [Settings API](/pages/api-settings/) and [Commands API](/pages/api-commands/).
- [Settings screens](/pages/settings-screens/) and [Manifest reference](/pages/reference-manifest/).
- [Plugin settings extension](/pages/settings-extension/) for tabs, sections and the built-in tab ids.
- [Tutorial 10: a new settings tab](/pages/tutorial-10-settings-new-tab/) builds a tab with every common row type.
- [Tutorial 11: a section in a built-in tab](/pages/tutorial-11-settings-extend-tab/) adds rows to Terminal.

API checked against `ilium:plugin@0.1.0`.
