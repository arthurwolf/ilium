---
layout: page
title: What a plugin is
permalink: /pages/getting-started-overview/
---

## What a plugin is

Ilium splits its work into two parts. The kernel is the part that must not break. It owns the project tree, the pane processes, the terminal emulator, input routing and the saved state. A plugin adds behaviour on top of the kernel. It reads the tree, reacts to events, and asks the kernel to change things by submitting intents. A plugin never writes to the terminal, opens a pty or draws the screen on its own. Each thing a plugin may do is a capability, and the user grants it.

This page covers the parts, the call model, the trust tiers and a first plugin in Rust and TypeScript.

### You will need

- No grants. The first plugin on this page asks for none.
- The `ilium` binary. The `ilium plugin` commands are subcommands of `ilium` itself, so there is no separate tool to install.
- For Rust: Rust 1.90 or later with the `wasm32-wasip2` target. For TypeScript: Bun 1.2 or later. Both are set up in [Set up a toolchain](/pages/getting-started-toolchain/).
- The contract version. The baseline is `ilium:plugin@0.1.0`, and every event, intent, widget, grant, host interface and settings row belongs to that one contract. A plugin names `ilium:plugin@0.1.0` in its `api` key.

### Parts of a plugin

| Term | Meaning |
|---|---|
| Kernel | The Ilium code that owns the state: tree, panes, terminal, input and snapshots. |
| Plugin host | The kernel part that runs plugin instances. It delivers events, takes submissions, checks grants and enforces limits. |
| Package | A `.iliumpkg` file: `manifest.json`, one WebAssembly component per side, and optional assets, help topics, sounds and a signature. |
| Side | One WebAssembly component built for one world: server, client or scene. |
| Export | A function set that the host calls, such as `lifecycle` or `handler`. |
| Capability | A permission named by a grant string such as `tree.decorate`. The user grants it. |

### Three sides

A plugin has up to three sides. The manifest lists the sides under the keys `server`, `client` and `scene`. A plugin with only a badge needs only the server side. A plugin with a panel needs a client side as well.

- **Server side** runs with the server. It sees the tree, the panes, the agent state, jobs, the files it was granted and its own settings.
- **Client side** runs inside the terminal interface. It opens panels, provides pane kinds, handles keys and clicks, and reads the screen mirror.
- **Scene side** draws an ambient background, one frame at a time.

Each side has a fixed set of required and optional exports. The SDK checks them at compile time.

| Side | Manifest key | World | Required exports | Optional exports |
|---|---|---|---|---|
| Server | `server` | `ilium:server/base` | `lifecycle`, `handler` | `detector`, `job-runner`, `service-provider`, `inference-provider`, `catalog-provider`, `virtual-rows-provider` |
| Client | `client` | `ilium:client/base` | `lifecycle`, `handler` | `views`, `annotator`, `job-runner`, `service-provider`, `inference-provider`, `catalog-provider`, `menu-provider`, `virtual-rows-provider`, `token-provider` |
| Scene | `scene` | `ilium:anim/base` | `lifecycle`, `renderer` | `job-runner` |

In Rust, the `#[ilium_plugin]` attribute sets the side and the optional exports:

```rust
#[ilium_plugin(side = "server", exports = ["detector"])]
#[derive(Default)]
struct MyPlugin { /* state */ }
```

A listed export without a matching trait implementation is a compile error.

**TypeScript**

```ts
export const { lifecycle, handler, detector } = definePlugin({
  side: 'server',
  exports: ['detector'],
  // activate, onEvents, identify and classify go here
});
```

`definePlugin` returns one object per export, named in camelCase. The optional exports are `catalog-provider` (option lists for `choice` rows and widgets), `menu-provider` (dynamic context menu items), `virtual-rows-provider` (children of virtual tree rows) and `token-provider` (syntax tokens for the editor).

### How the host calls a plugin

A plugin does not run a loop and does not draw every frame. The host gives a plugin a batch of events through `on-events`, and the plugin returns a list of submissions. Each submission carries one intent, such as `set-decoration`, and a correlation number. In Rust, `Submission::new(intent)` assigns the number; `Submission::keyed(intent, key)` adds a dedup key.

A slow host call returns a request id at once and completes later as a `request-completed` event. In Rust, `fs::read_all(path).await` hides this: the SDK resolves the future when the completion arrives. In TypeScript, `await readAll(path)` does the same with a promise.

A view works the same way. The host calls `render` when a view first opens, after `ui.invalidate`, and on resize. The host keeps the widget state, such as the selected row, and paints the tree. The only per-frame callback is the `render` call of a scene.

<figure>
<svg viewBox="0 0 400 180" role="img" aria-label="Events go in, submissions come out">
  <defs>
    <marker id="ov-flow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M 0 0 L 10 5 L 0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <title>Events in, submissions out</title>
  <desc>The kernel sends a batch of events to the plugin. The plugin returns submissions. The kernel checks the batch against the grants and applies the intents in order.</desc>
  <rect x="8" y="20" width="110" height="90" rx="4" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="63" y="60" text-anchor="middle" font-size="12" fill="currentColor">Kernel</text>
  <text x="63" y="78" text-anchor="middle" font-size="11" fill="currentColor">grants, order</text>
  <rect x="282" y="20" width="110" height="90" rx="4" fill="none" stroke="currentColor" stroke-dasharray="5 3"/>
  <text x="337" y="60" text-anchor="middle" font-size="12" fill="currentColor">Plugin</text>
  <text x="337" y="78" text-anchor="middle" font-size="11" fill="currentColor">your code</text>
  <line x1="118" y1="50" x2="282" y2="50" stroke="currentColor" marker-end="url(#ov-flow)"/>
  <text x="200" y="42" text-anchor="middle" font-size="11" fill="currentColor">on-events(batch)</text>
  <line x1="282" y1="92" x2="118" y2="92" stroke="currentColor" marker-end="url(#ov-flow)"/>
  <text x="200" y="84" text-anchor="middle" font-size="11" fill="currentColor">submissions</text>
  <text x="8" y="136" font-size="11" fill="currentColor">The kernel checks the batch against the grants.</text>
  <text x="8" y="152" font-size="11" fill="currentColor">It applies the submissions in order.</text>
  <text x="8" y="168" font-size="11" fill="currentColor">Each submission gets exactly one outcome.</text>
</svg>
<figcaption>One round trip: a batch of events in, a list of submissions out. The host returns one outcome per submission.</figcaption>
</figure>

A batch holds at most 256 events, and one `on-events` call may return at most 256 submissions; more is a trap. The host checks the batch against the grants, then applies the submissions in order.

### Capabilities and grants

A grant is a name, or a name with one argument after the first colon, such as `tree.read` or `http:api.example.com`. A plugin lists its grants per side in `capabilities`. Grants are recorded per side and per user configuration, not per project. Normal grants are checked by default in the install dialog. High-risk grants, marked H, are never checked by default. A bundled plugin gets its normal grants up to the manifest ceiling and is asked once for each H grant at first activation.

The contract includes 23 further grants for terminal cell reads, history and raw input, pane lifecycle, tree restructuring, attention marks, client chrome, external opening, cross-session control, long-lived children, kernel settings and keymap reads, agent transcripts, geocoding, embeddings, toasts and the pane journal. Eleven of them are high risk. The [capability reference](/pages/capabilities/) lists every one.

Two outcomes matter when you write code:

- A capability the user declined returns `not-permitted` from its functions. Your code gets an error value and keeps running.
- An import that the manifest never requested is a trap. That is a bug in the plugin, and the host restarts it.

Some imports need no grant at all, such as `log`, `clock`, `timers`, `storage` and `codec`.

A user can revoke a grant in Settings > Plugins > the plugin > Capabilities, or with `ilium plugin grants <id> --revoke <grant>`. The plugin then receives `deactivate(revoked)` and starts again with the smaller grant set.

### Trust tiers

Every installed plugin has one tier. The tier decides where its code runs and how far its limits go.

| Tier | Who it is for | Where it runs | How it gets installed |
|---|---|---|---|
| T0 | The kernel itself | Server process | Not a plugin |
| T1 | Bundled plugins shipped with Ilium | The process that hosts its side (server, client or CLI process), as precompiled `.cwasm` | Ships with Ilium |
| T2 | Signed packages from a publisher key the user trusts | Sandbox helper process | Install dialog with the review list |
| T3 | Unsigned local packages | Sandbox helper process, shown with a `dev` badge | `--allow-unsigned` or `ilium plugin dev` |

The limits differ by tier. The manifest asks for values in `limits`, and the host clamps them to the tier ceiling. A request above the ceiling is clamped and logged, never refused.

A T1 plugin may also set the top-level manifest key `limits_tier` to `"first-party"`. The host verifies the tier and then applies the higher ceilings of [Errors and limits](/pages/reference-errors-and-limits/). The table below shows the standard ceilings.

| Limit | Default | T1 ceiling | T2 and T3 ceiling |
|---|---|---|---|
| `memory_mib` | 32 | 256 | 128 |
| `table_elements` | 10,000 | 50,000 | 20,000 |
| `event_deadline_ms` | 20 | 50 | 20 |

A package signed by a key the user does not trust is offered for trust at install. Declining refuses the install. On Windows and macOS, the kernel refuses T2 and T3 packages until a sandbox exists there. The refusal reads:

```text
Cannot install <name> <version>.
Third-party plugins need the plugin sandbox, which is not available on Windows yet.
Bundled plugins keep working.
```

macOS shows the same text with `macOS` in place of `Windows`.

### Lifecycle in brief

The host starts plugins in a fixed order. On the server, it restores the saved state, verifies every installed manifest and registers its static contributions without running any code. Then it activates the plugins with the `startup` trigger, and gives each one a single `tree-changed` batch that covers the restored nodes. A plugin without an `activation` key starts on first use of one of its contributions.

When a plugin stops, the host calls `deactivate` with a reason such as `shutdown`, `disabled` or `revoked`. A trap or a missed deadline restarts the instance after 1 s, then 5 s. A third failure within 10 minutes disables the plugin until the user enables it again. The full sequence is on the [Lifecycle](/pages/lifecycle/) page.

### The manifest

The manifest is JSON with snake_case keys. The host refuses unknown keys.

| Key | Required | Meaning |
|---|---|---|
| `api` | yes | Contract version, `ilium:plugin@0.1.0`. |
| `id` | yes | Reverse-DNS-like id, for example `example.hello`. |
| `name`, `version`, `publisher` | yes | Display name, semver version and publisher name. |
| `server`, `client`, `scene` | one of them | Object with `entry`, `world` and `exports`. |
| `capabilities` | no | Grant strings for each side. |
| `activation` | no | Triggers that start the plugin, such as `startup`. |
| `contributes` | no | Static contributions such as commands, settings, decoration slots, panels, `tree_toolbar`, `row_actions`, `tree_orderings`, `status_segments`, `pane_frame_slots`, `display_modes`, `catalogs`, `client_requests` and `languages`. |
| `limits`, `limits_tier` | no | Requested limits, clamped by tier, and the optional `first-party` tier for T1. |
| `unstable` | no | Opt-in list for unstable features such as `gpu-kernels`. |
| `files` | packed only | Length and SHA-256 of each package entry. `ilium plugin pack` writes it. |

### Your first plugin in Rust

This server-side plugin logs once when it starts and logs the size of each event batch. It needs no grants. The `Lifecycle` trait has a default `deactivate`, so the plugin implements only `activate`.

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::log::{self, Level};

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Hello;

impl Lifecycle for Hello {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        log::log(Level::Info, &format!("hello from {}", context.plugin));
        Ok(())
    }
}

impl Handler for Hello {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        log::log(Level::Debug, &format!("{} events", batch.len()));
        Vec::new()
    }
}
```

The prelude brings in the export traits and the common types. The log function takes a `Level` value, not a string.

### Your first plugin in TypeScript

The TypeScript version has the same contract. `definePlugin` returns the `lifecycle` and `handler` objects, and each one has the method names from the Rust trait in camelCase.

```typescript
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate(context) {
    log('info', `hello from ${context.plugin}`);
  },
  deactivate(reason) {
    return undefined;
  },
  onEvents(batch: Event[]): Submission[] {
    log('debug', `${batch.length} events`);
    return [];
  },
});
```

Log levels are kebab-case strings in TypeScript: `'trace'`, `'debug'`, `'info'`, `'warn'` and `'error'`.

The manifest is the same for both versions. It names the entry file, the world the component was built for, and the exports it provides. The `exports` list is empty because `lifecycle` and `handler` are required. This source manifest omits `files`, which `ilium plugin pack` adds later.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.hello",
  "name": "Hello",
  "version": "0.1.0",
  "publisher": "Example Developer",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": []
  }
}
```

### SDK import paths

Each WIT interface has one Rust module and one TypeScript subpath with the same name. Most plugins use a handful of them.

| Rust module | TypeScript import | Use |
|---|---|---|
| `ilium_plugin_sdk::host::log` | `@ilium/plugin-sdk/host/log` | Write log lines. |
| `ilium_plugin_sdk::host::tree` | `@ilium/plugin-sdk/host/tree` | Read nodes. Needs `tree.read`. |
| `ilium_plugin_sdk::host::ui` | `@ilium/plugin-sdk/host/ui` | Client side: theme and invalidation. |
| `ilium_plugin_sdk::anim::renderer` | `@ilium/plugin-sdk/anim/renderer` | Scene renderer. |

Each import has its own API page, such as [Log](/pages/api-log/) and [Tree](/pages/api-tree/).

### What you see

The `activate` line goes to the plugin log once, when the plugin starts. The `debug` line appears once for each event batch.

<figure>
<pre class="term">
<span class="t-prompt">$</span> ilium plugin logs example.hello
2026-10-11 12:04:08 info  example.hello hello from example.hello
2026-10-11 12:04:08 debug example.hello 3 events
2026-10-11 12:04:09 debug example.hello 1 events
</pre>
<figcaption>Illustrative, not generated. The log format is the one `ilium plugin logs` prints: timestamp, level padded to five characters, plugin id and message. The batch sizes depend on the project's activity.</figcaption>
</figure>

The tree does not change, because this plugin decorates nothing. A plugin that adds a badge to each pane needs the `tree.decorate` grant in its manifest's `capabilities.server` list.

### What the user sees at install

Before a T2 or T3 plugin installs, Ilium shows the review dialog. Each requested grant is a checkbox row with a dotted leader to its meaning. High-risk grants start unchecked and use the warning style. The user can decline a grant and still install. Declined features return `not-permitted`.

<figure>
<pre class="term">╭  ≡ ● · Install Hello 0.1.0? ───────────────────────────────────────╮
│ Publisher: Example Developer (trusted)                             │
│ This plugin asks for:                                              │
│                                                                    │
│ [x] tree.read ………………………… read the project tree                     │
│ [x] tree.decorate ………………… add badges to tree rows                  │
│ [ ] <span class="t-warn">terminal.raw-input</span> ………… <span class="t-warn">send raw bytes to a pane</span>               │
│                                                                    │
│      Esc Cancel          Enter Install                             │
│ <span class="t-dim">Space toggle · Enter install · Esc cancel</span>                          │
╰────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>Illustrative, 70 columns. A modal dialog with a rounded border, checkbox rows with <code>…</code> leaders, a Neutral and a Primary button, and the footer hint line with <code>·</code> separators.</figcaption>
</figure>

### Edge cases and errors

Every fallible call returns a `plugin-error` with one of these codes. The retry column says whether the same call can succeed later.

| Code | Meaning | Retry |
|---|---|---|
| `not-permitted` | No grant, the user declined it, or a path or policy blocks the call. | No |
| `not-found` | The node, pane, file, command or plugin does not exist any more. | No |
| `invalid-argument` | The argument breaks a rule the plugin can check, such as a size cap. | No, fix the call |
| `conflict` | State changed or is held, such as a stale revision or a locked file. | Yes, after re-reading |
| `quota-exceeded` | A stored or held quota is full. | After freeing resources |
| `rate-limited` | A rate rule throttles the call. `retry-after-ms` is set. | Yes, after the delay |
| `busy` | Transient capacity is full, such as the job queue. | Yes, later |
| `timed-out` | A deadline passed. | Yes |
| `cancelled` | The plugin, the user or a shutdown cancelled the request. | Maybe |
| `unsupported` | The host or platform cannot do it. | No |
| `pane-not-ready` | A restored terminal pane has not started its process yet. | Yes |
| `revoked` | The grant set changed while this call was in flight. The plugin is re-activated. Calls made after that return `not-permitted`. | No |
| `io` | An operating system I/O error other than a missing file. | Maybe |
| `network` | DNS, connect, TLS or transport failure. | Yes |
| `internal` | A host bug or a trap inside a job instance. | No |

Rules that apply to every plugin:

- A trap, such as a panic in `on-events`, drops the instance. The server keeps running, and the plugin restarts after its back-off.
- Log lines over 16 KiB are cut and end with `…[truncated]`. Other oversize values give `invalid-argument` before anything is written.
- Events marked "drop oldest, counted" are dropped when the plugin falls behind. The next batch starts with an `events-dropped` event that gives the count.

### See also

- [Set up a toolchain](/pages/getting-started-toolchain/) installs the tools that build this code.
- [Your first plugin in five minutes](/pages/getting-started-first-plugin/) changes one line of a generated template.
- [Tutorial step 1](/pages/tutorial-01-agent-badge/) adds the first decoration.
- [Lifecycle](/pages/lifecycle/) describes activation, deactivation and restarts in full.
- [Capabilities](/pages/capabilities/) lists every grant and what it unlocks.
- [Errors and limits](/pages/reference-errors-and-limits/) gives the full error and limit tables.
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/) describe the two SDKs.
