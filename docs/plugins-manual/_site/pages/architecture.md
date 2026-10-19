# Architecture

## Architecture

Ilium keeps the code that runs per byte, per keystroke or per frame in the kernel. Everything else that a feature needs can be a plugin. Plugins run inside a plugin host, behind a fixed contract, `ilium:plugin@0.1.0`, which lists every function a plugin may call. This page explains where the boundary sits, what sits on each side of it, and what happens when a plugin breaks the rules.

In short, a plugin never touches the terminal, the tree or the disk directly. It calls an SDK function. The host checks the plugin's grants, the kernel does the work, and the result returns to the plugin as an event or as an outcome.

## You will need

Nothing beyond the overview. The later concept pages describe each component in detail.

To write a plugin you need the toolchain from [Getting started toolchain](/pages/getting-started-toolchain/). Rust plugins need Rust stable 1.90 or later with the `wasm32-wasip2` target. TypeScript plugins need Bun 1.2 or later. Both SDKs pin their binding generators, so nothing is installed globally.

## The kernel

The kernel is the native part of Ilium. It runs in the processes that host plugins (the session server, each client and the CLI process; see [Host architecture](/pages/reference-host-architecture/)) and owns the components below. A plugin never reaches into these parts directly. It asks for an operation through a host import, and the kernel checks the plugin's grants before it acts.

| Component | What it owns | How a plugin reaches it |
|---|---|---|
| Process and session lifecycle | Sessions, the CLI dispatcher, server and client processes | `cli` and `http-api` imports, `cli-invoked` events |
| PTY and screen state | PTY spawn, input delivery, vt100 parsing, screen state | `terminal` and `agent` primitives, `send-input` intent |
| Generic tree | Groups and panes, plus plugin metadata the kernel stores but never reads | `tree` and `panes` imports, `tree-changed` events, intents |
| Snapshot writer | The snapshot file and plugin-owned sections | Bytes returned from `deactivate` are stored as restore state |
| Layout and compositor | Split geometry, PTY sizing, the shared animation look, dithering | `ui` widget trees, `anim` scenes |
| Input routing | Keymap, mouse hit-testing, focus, the modal stack | `keys` import, `keys.grab` |
| Commands and settings | The command registry and the settings store | `commands` and `settings` imports |
| Event bus and quotas | Per-plugin queues, timers, the execution quota group | `on-events` export, `timers` import |
| Host services | HTTP, file access by preopen, process runner, sound, notifications, clipboard, audio, video, inference transport | Imports that a grant unlocks |

The terminal and agent pane primitives stay native, because they handle the per-byte path. The editor primitive is native. The contract keeps the same split: its events, intents, grants and widgets let plugins reach much of the kernel without moving any per-byte work into a plugin.

The map below shows the three layers. Solid boxes are kernel components. The dashed box is the plugin host. The double line is the sandbox boundary, which applies to third-party plugins (tiers T2 and T3).

<figure>
<svg viewBox="0 0 340 260" role="img" aria-label="Kernel components, plugin host and sandbox boundary">
  <title>Kernel, plugin host and sandbox</title>
  <desc>Solid kernel component boxes sit above a dashed plugin host. A double-lined sandbox holds third-party plugin components.</desc>
  <rect x="6" y="6" width="328" height="112" rx="6" fill="none" stroke="currentColor"/>
  <text x="16" y="22" font-size="11" fill="currentColor">Kernel (native)</text>
  <rect x="16" y="30" width="144" height="34" rx="4" fill="none" stroke="currentColor"/>
  <text x="88" y="51" font-size="10" text-anchor="middle" fill="currentColor">Tree and snapshot</text>
  <rect x="180" y="30" width="144" height="34" rx="4" fill="none" stroke="currentColor"/>
  <text x="252" y="51" font-size="10" text-anchor="middle" fill="currentColor">PTY and screen state</text>
  <rect x="16" y="74" width="144" height="34" rx="4" fill="none" stroke="currentColor"/>
  <text x="88" y="95" font-size="10" text-anchor="middle" fill="currentColor">Event bus and quotas</text>
  <rect x="180" y="74" width="144" height="34" rx="4" fill="none" stroke="currentColor"/>
  <text x="252" y="95" font-size="10" text-anchor="middle" fill="currentColor">Compositor and input</text>
  <line x1="170" y1="118" x2="170" y2="134" stroke="currentColor"/>
  <rect x="6" y="134" width="328" height="46" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="16" y="150" font-size="11" fill="currentColor">Plugin host (dashed)</text>
  <text x="16" y="166" font-size="10" fill="currentColor">Package loader, capability broker, Wasmtime</text>
  <rect x="2" y="190" width="336" height="66" rx="4" fill="none" stroke="currentColor"/>
  <rect x="6" y="194" width="328" height="58" rx="3" fill="none" stroke="currentColor"/>
  <text x="14" y="208" font-size="10" fill="currentColor">Sandbox helper (T2 and T3 only)</text>
  <rect x="14" y="216" width="150" height="26" rx="4" fill="none" stroke="currentColor" stroke-dasharray="5 3"/>
  <text x="89" y="233" font-size="10" text-anchor="middle" fill="currentColor">Plugin component</text>
  <rect x="176" y="216" width="150" height="26" rx="4" fill="none" stroke="currentColor" stroke-dasharray="5 3"/>
  <text x="251" y="233" font-size="10" text-anchor="middle" fill="currentColor">Plugin component</text>
</svg>
<figcaption>Kernel components are solid. The plugin host is dashed. Third-party plugin components run inside the sandbox helper, which is drawn with a double line.</figcaption>
</figure>

## The plugin host

The plugin host runs plugin components inside Wasmtime. It verifies each package and records the grants. Every plugin side (server, client or scene) gets its own instance and limits. See [Worlds and sides](/pages/worlds-and-sides/).

The host sorts plugins into four tiers. The tier decides where a plugin runs and what it may ask for.

| Tier | Who signs it | Where it runs | How it installs |
|---|---|---|---|
| T0 | The kernel itself | Native code in the server and the client | Not a plugin |
| T1 | Ilium, embedded in the release as precompiled `.cwasm` | The process that hosts its side (server, client or CLI process), on every platform | Granted up to the manifest ceiling; high-risk grants asked once |
| T2 | A publisher key that the user trusts | The sandbox helper process | Review dialog; grants are remembered |
| T3 | Nobody (unsigned local package) | The sandbox helper process | Only with `--allow-unsigned` or `ilium plugin dev`; shown with a `dev` badge |

A T1 plugin that asks for a high-risk (H) capability runs without it until the user allows it in a single prompt at first activation.

On Windows and macOS, T2 and T3 packages are refused until a native sandbox exists there. The refusal text is:

```
Cannot install acme-agent 1.0.0.
Third-party plugins need the plugin sandbox, which is not available on Windows yet.
Bundled plugins keep working.
```

On macOS the same text names macOS instead of Windows.

<pre class="term"><span class="t-dim">2026-10-11 09:12:03</span> <span class="t-ok">load</span>    demo-notes 0.1.0, tier T1, server and client
<span class="t-dim">2026-10-11 09:12:03</span> <span class="t-ok">load</span>    acme-agent 1.0.0, tier T2, server, sandbox helper
<span class="t-dim">2026-10-11 09:12:04</span> <span class="t-ok">restore</span> snapshot restored, one tree-changed batch
<span class="t-dim">2026-10-11 09:12:04</span> <span class="t-warn">refuse</span>  vendor-ext 0.1.0, tier T3, no sandbox on macOS
<span class="t-dim">2026-10-11 09:14:40</span> <span class="t-dim">stop</span>    plugins stopped before the final snapshot write
</pre>

Illustrative, not generated. A host log that shows which plugins load, which one is refused on a platform without a sandbox, and when the host stops. The names are placeholders.

### The sandbox helper

T2 and T3 components run in a separate helper process. The helper confines itself with a seccomp filter and a bubblewrap mount namespace. The namespace holds only the scopes that the plugin was granted, mounted read-only for read grants, plus read-only `/usr`, `/bin`, `/lib*` and `/etc`. A component that imports `wasi:sockets` or `wasi:http` is refused at load, so network access goes through the `http` import, which the kernel performs for the plugin.

Child processes that a plugin starts with `process.run` inherit the same confinement. T1 children run without confinement, at low priority.

<figure>
<svg viewBox="0 0 400 180" role="img" aria-label="Request path from plugin code through the kernel">
  <title>A request from plugin code to the kernel</title>
  <desc>Plugin code calls an SDK wrapper. The host checks the grant, then the kernel acts. The result returns to the plugin.</desc>
  <defs>
    <marker id="arch-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="6" y="20" width="112" height="40" rx="4" fill="none" stroke="currentColor"/>
  <text x="62" y="44" font-size="11" text-anchor="middle" fill="currentColor">Plugin code</text>
  <rect x="144" y="20" width="112" height="40" rx="4" fill="none" stroke="currentColor"/>
  <text x="200" y="44" font-size="11" text-anchor="middle" fill="currentColor">SDK wrapper</text>
  <rect x="282" y="20" width="112" height="40" rx="4" fill="none" stroke="currentColor"/>
  <text x="338" y="44" font-size="11" text-anchor="middle" fill="currentColor">Grant check</text>
  <line x1="118" y1="40" x2="144" y2="40" stroke="currentColor" marker-end="url(#arch-arrow)"/>
  <line x1="256" y1="40" x2="282" y2="40" stroke="currentColor" marker-end="url(#arch-arrow)"/>
  <line x1="338" y1="60" x2="338" y2="104" stroke="currentColor" marker-end="url(#arch-arrow)"/>
  <text x="344" y="86" font-size="10" fill="currentColor">granted</text>
  <rect x="282" y="104" width="112" height="40" rx="4" fill="none" stroke="currentColor"/>
  <text x="338" y="128" font-size="11" text-anchor="middle" fill="currentColor">Kernel</text>
  <line x1="282" y1="124" x2="118" y2="124" stroke="currentColor" marker-end="url(#arch-arrow)"/>
  <text x="200" y="118" font-size="10" text-anchor="middle" fill="currentColor">outcome or event</text>
  <rect x="6" y="104" width="112" height="40" rx="4" fill="none" stroke="currentColor"/>
  <text x="62" y="128" font-size="10" text-anchor="middle" fill="currentColor">Result to plugin</text>
  <text x="6" y="168" font-size="10" fill="currentColor">A declined grant returns not-permitted before the kernel runs.</text>
</svg>
<figcaption>A call leaves the plugin through an SDK wrapper. The grant check runs first, the kernel acts only when the check passes, and the outcome returns to the plugin.</figcaption>
</figure>

## Kernel and core plugin split

The kernel keeps PTY and vt100, the tree model, IPC, the snapshot writer, the config store, the keymap, layout and the compositor, the look painter, the scene host, the plugin host, the recovery page, the HTTP listener and the parsing of kernel CLI commands. Everything else is a core plugin. The full list, the boot order of both sides, the minimum kernel that runs with `--no-plugins` and the safe start are on [kernel and boot](/pages/reference-kernel-and-boot/). The owner of every Ilium feature is on [feature traceability](/pages/reference-feature-traceability/). [COR-010] [COR-020]

## Start and stop order

On the server, the host starts in this order:

1. The kernel boots and loads the configuration.
2. The snapshot is restored.
3. The plugin host verifies every installed manifest and registers its static contributions. Nothing is instantiated yet.
4. `startup` plugins are instantiated and activated. Service providers activate before the consumers that require them. Cycles are refused at install.
5. Each activated plugin receives one `tree-changed` batch for all restored nodes. It never replays the restore.

On shutdown, the order is reversed in the places where it matters:

1. Services stop in reverse activation order.
2. The host refuses plugin submissions. They complete as `cancelled`.
3. Each active plugin receives `deactivate(shutdown)` in reverse activation order, within `deactivate_deadline_ms`. The bytes it returns become its restore state.
4. Pending requests are cancelled, instances are dropped and quotas are released.
5. The final snapshot is written. No plugin intent can reach the snapshot after the writer stops.

A client connects with a handshake that carries the plugin protocol number, the contract version and its plugin set. A protocol or contract mismatch is refused with `ipc.protocol-mismatch` and the client exits with code 76. A plugin that differs between client and server loses only its client half. See [Plugin traffic over IPC](/pages/reference-host-ipc/).

## Boundary rules

- Plugins depend only on the SDK packages: the crate `ilium-plugin-sdk` for Rust and `@ilium/plugin-sdk` for TypeScript. They never depend on Ilium crates.
- Every call is checked against the grant set in force at that moment. A declined grant links its imports to stubs that return `not-permitted`. An interface that no requested capability unlocks is refused at install with `manifest.import-not-requested`. A function of a never-requested capability inside a linked interface traps the instance with `host.capability-not-requested`. See [Host runtime](/pages/reference-host-runtime/).
- Each plugin has one bounded event queue, sized by `event_queue` (1,024 by default). Plugins hold no event-journal cursors of their own. Events with the drop policy are counted in an `events-dropped` event.
- The dispatcher charges each plugin's bytes and CPU time to the execution quota group. When that budget is spent, calls return `quota-exceeded` with `retry-after-ms`.
- Plugins never print raw terminal escape sequences. They describe widgets as data, and the kernel applies the theme, icon widths and the stable-glyph preference. The host removes escape sequences from text fields and logs one warning per view.

### Limits

The manifest may request a limit. The host clamps the request to the ceiling for the tier and logs the clamp; it never refuses the package. The user sets the same keys under `[plugins.limits]` in `config.toml`, for every plugin or for the whole session. The main keys are below. The full list is in [Errors and limits](/pages/errors-and-limits/).

| Key | Default | T1 ceiling | T2 and T3 ceiling | Effect on breach |
|---|---|---|---|---|
| `memory_mib` | 32 | 256 | 128 | Trap |
| `event_deadline_ms` | 20 | 50 | 20 | Trap |
| `render_deadline_ms` | 10 | 20 | 10 | Trap |
| `job_deadline_ms` (CPU) | 2,000 | 60,000 | 10,000 | `failed(timed-out)` |
| `event_queue` | 1,024 | 4,096 | 1,024 | Stuck plugin restart |
| `ui_tree_nodes` | 4,096 | 16,384 | 4,096 | Trap |
| `storage_bucket_mib` | 16 | 256 | 16 | `quota-exceeded` |
| `http_response_mib` | 8 | 64 | 8 | `failed(quota-exceeded)` |

The session keys cap the sum over all plugins in one process. For example, `session_memory_mib` is 1,024 MiB. A new instance that would exceed it is refused and logged. It does not count as a failure. The session keys and the exact failure list are in [Errors and limits](/pages/reference-errors-and-limits/#host-rules-for-limits-and-restarts).

## Hook chains

An owner can run a hook chain through the `extensions` host interface. The host looks up the extensions of that point, sorts them by priority (lower first, ties by plugin id) and calls each handler through the contributor's `hook-handler` export, one after the other. The reply of each handler is checked against the point's mode and `reply_schema` before the next handler runs. Hooks fail open. A handler that times out, traps or errors counts as a pass, three breaches in a row stop that extension until its plugin restarts, and the whole chain is capped at 100 ms. Handlers after the cap are skipped and logged. `run-hook` is asynchronous: it returns a request id at once and the chain runs without blocking the owner's call. Handlers run strictly one after another, the 100 ms cap is measured from the start of the first handler, and a hook never crosses a process: a client-owned point runs only the handlers in that client. T1 handlers run in the process that hosts the owner's side. T2 and T3 handlers cost one helper round trip, which counts inside the handler deadline. See [extension points](/pages/api-extension-points/) and [Host scheduling](/pages/reference-host-scheduling/).

## Failures and restarts

A trap or a deadline breach drops the instance, cancels its pending requests and keeps its decorations, marked stale. The host then restarts the plugin on a back-off:

- The first failure restarts after 1 second.
- The second failure restarts after 5 seconds.
- The third failure within the 10-minute window stops that side; there is no 30-second step.
- A third failure inside a sliding 10-minute window disables the plugin on that side. On the client, each client counts separately.

A revocation, a disable, an upgrade or a shutdown never counts as a failure. When the host disables a plugin, the tree footer shows a notice in this form:

`Plugin Acme Agent stopped after 3 failures in 10 minutes: views.render trap (sidebar). Turn it on again in Settings > Plugins.`

The part in parentheses names the view only when the failing export was `views.render`.

## Worked example 1: a Rust server plugin

This plugin runs on the server. It places a badge on every pane whose agent starts working. The manifest asks for two grants: `agent.read` to receive activity events and `tree.decorate` to set the badge.

```json
{
  "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": [] },
  "capabilities": { "server": ["agent.read", "tree.decorate"] },
  "contributes": {
    "decoration_slots": [{ "id": "hello", "placement": "badge", "priority": 50 }]
  }
}
```

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::intents::SetDecorationSpec;
use ilium_plugin_sdk::types::tree::AgentActivity;

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
        let mut submissions = Vec::new();
        for event in batch {
            let Event::AgentActivityChanged(change) = event else { continue };
            if change.current != AgentActivity::Working { continue; }
            submissions.push(Submission::new(Intent::SetDecoration(SetDecorationSpec {
                node: change.node,
                slot: "hello".into(),
                content: vec![Span::plain("hi")],
                icon: None,
                sort_key: None,
                persist: false,
            })));
        }
        submissions
    }
}
```

The path through the boundary is as follows:

1. The dispatcher delivers `agent-activity-changed` into the plugin's queue. Delivery requires `agent.read`.
2. `log::log` is an always-granted import. Its line goes to the plugin log.
3. The returned `Submission` is checked when the batch is accepted. The kernel verifies `tree.decorate`, the node and the slot `hello`, which the manifest declares.
4. If the user declined `tree.decorate`, the kernel answers `refused(not-permitted)` for that submission. The batch is refused as a whole only for a malformed submission or a missing grant.

## Worked example 2: a TypeScript client plugin

This plugin runs in the client. It answers its own command with a desktop notice. It needs the `notify.desktop` grant, and the command `com.example.hello/greet` must be declared under `contributes.commands`.

```json
{
  "client": { "entry": "client.js", "world": "ilium:client/base@0.1.0", "exports": [] },
  "capabilities": { "client": ["notify.desktop"] }
}
```

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate(context) {
    log('info', `hello from ${context.plugin}`);
  },
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'command-invoked') continue;
      if (event.val.command !== 'com.example.hello/greet') continue;
      submissions.push({
        intent: {
          tag: 'notify',
          val: { title: 'Hello', body: 'from a plugin', urgency: 'normal', sound: undefined },
        },
        correlation: 1n,
        dedupKey: undefined,
      });
    }
    return submissions;
  },
});
```

The returned submissions are checked by the kernel. A `notify` intent is limited to 6 per minute per plugin; a seventh returns `refused(rate-limited)`.

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs com.example.hello --tail 3
<span class="t-dim">2026-10-11 09:14:02</span> <span class="t-ok">info </span> com.example.hello hello from com.example.hello
<span class="t-dim">2026-10-11 09:14:31</span> <span class="t-warn">warn </span> com.example.hello 2 lines dropped in the last second
<span class="t-dim">2026-10-11 09:15:07</span> <span class="t-err">error</span> com.example.hello restart after 1 s, handler trap
</pre>

Illustrative, not generated. The log lines of one client plugin, as `ilium plugin logs` prints them. The plugin id, the level and the message follow the timestamp.

## Errors and edge cases

| Situation | Result |
|---|---|
| Function of a capability the manifest never requested, in a linked interface | The instance traps with `host.capability-not-requested` and counts as a failure. An interface that is not unlocked at all is refused at install |
| Grant declined by the user | The call returns `not-permitted`; the plugin keeps running |
| Component exports differ from required plus listed | Install refused with `manifest.exports-mismatch` |
| Component imports `wasi:sockets` or `wasi:http` | Refused at load |
| Windows or macOS, T2 or T3 package | Install refused with the text above |
| Session memory limit reached | New instance refused and logged; not counted as a failure (`host-capacity`) |
| Third failure inside 10 minutes | Plugin disabled with the notice above |
| Grant revoked while a call is in flight | Call returns `revoked`; the plugin is re-activated with the reduced grants |

## What the user sees

The tree footer shows disable notices. Settings > Plugins lists each plugin with its tier, grants and an on/off switch; a T3 plugin carries a `dev` badge. Decorations appear in the pane rows they name, such as the `hi` badge in example 1. Logs appear through `ilium plugin logs <id>`.

<figure>
<pre class="term">╭  ≡ ● · Plugins ────────────────────────────────────────────────╮
│ <span class="t-hi">demo-notes 0.1.0</span>   T1   server, client   <span class="t-ok">Active</span>                │
│ vendor-ext 0.1.0  T3 <span class="t-dim">[dev]</span>  server   <span class="t-warn">Refused</span>                   │
│                                                                │
╰────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>Illustrative. Settings > Plugins lists each plugin with its tier, sides and state. A T3 plugin carries the <code>[dev]</code> badge.</figcaption>
</figure>

## See also

- [Worlds and sides](/pages/worlds-and-sides/)
- [Packages and manifests](/pages/packages/)
- [Lifecycle](/pages/lifecycle/)
- [Capabilities and grants](/pages/capabilities/)
- [Primitives](/pages/primitives/)
- [Services](/pages/services/)
- [Security](/pages/security/)
- [Errors and limits](/pages/errors-and-limits/)
- [Rust SDK](/pages/rust-sdk/)
- [TypeScript SDK](/pages/typescript-sdk/)

API version checked: `ilium:plugin@0.1.0`.

### Security

Security architecture is summarised in [Security](/pages/security/) and detailed in [Threat model](/pages/reference-threat-model/).
