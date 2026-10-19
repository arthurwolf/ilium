# Plugin exports

## Plugin exports

A plugin imports host functions and also exports functions that the host calls. Each export belongs to one side of the plugin: `server`, `client` or `scene`. The host calls an export only when the feature it serves is present, so a plugin never receives calls for features it did not declare.

Every server and client plugin implements `lifecycle` and `handler`. A scene plugin implements `lifecycle` and `renderer`. Optional exports are named in the `exports` list of the side in the manifest. The component must export exactly the required set plus the listed optional exports. If it does not, install refuses the package with `manifest.exports-mismatch`.

### What you need

- **A manifest entry.** Add each optional export to `exports`, for example `"exports": ["detector"]`. Never list a required export.
- **The grant in the catalogue table.** Only the exports with a grant in that table need one. Your plugin's own host calls need their grants as usual.
- **A toolchain.** Rust with `ilium-plugin-sdk`, where the export traits live in `ilium_plugin_sdk::exports`. Or TypeScript with `@ilium/plugin-sdk` and `definePlugin`.

### The export catalogue

| Export | Side | Required | Manifest name | Grant | The host calls it to |
|---|---|---|---|---|---|
| `lifecycle` | all | yes | none | none | activate, and deactivate with a reason |
| `handler` | server, client | yes | none | none | deliver a batch of events, and take submissions back |
| `views` | client | no | `views` | none; `ui.invalidate` is always granted for own views | render a view tree when it opens, is invalidated or resizes |
| `detector` | server | no | `detector` | `agent.read` | identify an agent from a pane's processes, and classify its screen |
| `job-runner` | server, client, scene | no | `job-runner` | none; the job kind must be in `contributes.jobs` | run a declared job with input bytes, and return output bytes |
| `service-provider` | server, client | no | `service-provider` | `services.provide:<name>` (name without version) | answer a call to a service it provides |
| `inference-provider` | server, client | no | `inference-provider` | `inference.provide` | complete a model request, and list its models |
| `annotator` | client | no | `annotator` | `editor.read` | annotate the visible lines of an editor buffer |
| `settings-validate` | server, client | no | `settings-validate` | none | check a pending settings change before it is saved (100 ms) |
| `hook-handler` | server, client | no | `hook-handler` | `extensions.contribute` | run one hook extension of another plugin's extension point (WIT fragment `with-hook-handler`); the handler lives on the side that owns the point |
| `renderer` | scene | yes | none | none | draw frames of a background scene |
| `catalog-provider` | server, client | no | `catalog-provider` | none; ids must be in `contributes.catalogs` | serve a page of an option catalogue |
| `menu-provider` | client | no | `menu-provider` | none | build a dynamic context menu or toolbar submenu |
| `virtual-rows-provider` | server, client | no | `virtual-rows-provider` | `tree.decorate` for the rows themselves | return a page of children of a virtual tree row |
| `token-provider` | client | no | `token-provider` | `editor.read` | return syntax tokens for lines of a language the host does not know |
| `migrator` | server, client | no | `migrator` | none | convert stored data once before `activate` when `data_version` rose (WIT fragment `with-migrator`) |

The `handler` export receives events in batches of at most 256, and returns at most 256 submissions per call. The host applies the submissions in order. A malformed submission, or one without its grant, refuses the whole batch. See [Events](/pages/api-events/) for the event catalogue and [Intents](/pages/api-intents/) for the submission outcomes.

### Worlds and export fragments

A plugin's world is one base world plus the fragments it implements. The manifest `exports` list must match the component's exports.

| World | Required exports | Optional fragments |
|---|---|---|
| `ilium:server/base` | `lifecycle`, `handler` | `with-detector`, `with-jobs`, `with-service`, `with-inference-provider`, `with-settings-validate`, `with-catalog`, `with-virtual-rows`, `with-hook-handler`, `with-migrator` |
| `ilium:client/base` | `lifecycle`, `handler` | `with-views`, `with-annotator`, `with-jobs`, `with-service`, `with-inference-provider`, `with-settings-validate`, `with-catalog`, `with-menus`, `with-virtual-rows`, `with-tokens`, `with-hook-handler`, `with-migrator` |
| `ilium:anim/base` | `lifecycle`, `renderer` | `with-jobs` |

<figure>
<svg viewBox="0 0 420 190" role="img" aria-label="Exports the host calls on each side">
<defs>
<marker id="exp-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<title>Host to plugin export calls</title>
<desc>The host calls required exports on every plugin and optional exports only when the matching feature is present.</desc>
<rect x="10" y="60" width="110" height="70" rx="6" fill="none" stroke="currentColor"/>
<text x="65" y="92" font-size="12" text-anchor="middle" fill="currentColor">Host</text>
<text x="65" y="110" font-size="10" text-anchor="middle" fill="currentColor">server or client</text>
<rect x="250" y="8" width="160" height="34" rx="6" fill="none" stroke="currentColor"/>
<text x="330" y="30" font-size="11" text-anchor="middle" fill="currentColor">lifecycle, handler</text>
<rect x="250" y="52" width="160" height="34" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
<text x="330" y="74" font-size="11" text-anchor="middle" fill="currentColor">views, annotator, tokens</text>
<rect x="250" y="96" width="160" height="34" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
<text x="330" y="118" font-size="11" text-anchor="middle" fill="currentColor">catalog, menus, virtual rows</text>
<rect x="250" y="140" width="160" height="34" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
<text x="330" y="162" font-size="11" text-anchor="middle" fill="currentColor">detector, jobs, providers</text>
<line x1="120" y1="85" x2="248" y2="28" stroke="currentColor" marker-end="url(#exp-arrow)"/>
<line x1="120" y1="95" x2="248" y2="72" stroke="currentColor" marker-end="url(#exp-arrow)"/>
<line x1="120" y1="105" x2="248" y2="114" stroke="currentColor" marker-end="url(#exp-arrow)"/>
<line x1="120" y1="115" x2="248" y2="156" stroke="currentColor" marker-end="url(#exp-arrow)"/>
</svg>
<figcaption>Solid boxes are required on every plugin. Dashed boxes are called only when the manifest lists them.</figcaption>
</figure>

### The four provider exports

Each has its own page with the full records, a Rust and a TypeScript example, and the render the user sees.

| Export | Called when | Deadline | Page |
|---|---|---|---|
| `catalog-provider` | a row or widget opens a plugin catalogue | 20 ms | [catalogs](/pages/api-catalogs/) |
| `menu-provider` | a dynamic menu or toolbar submenu opens | 20 ms | [menu provider](/pages/api-menu-provider/) |
| `virtual-rows-provider` | a provider virtual row is expanded | 20 ms | [virtual rows provider](/pages/api-virtual-rows-provider/) |
| `token-provider` | a buffer of a declared language is visible | 10 ms per call | [token provider](/pages/api-token-provider/) |

Further activation triggers: `catalog:<id>`, `menu:<id>`, `virtual-rows:<row-kind>`, `language:<id>`, `client-request:<kind>` and `durable-timer`. Until the instance answers, the host shows the static menu entries, a spinner row, or plain text.

Further stop rules: on deactivate, trap or revocation the host closes the plugin's HTTP streams, terminates its spawned children, releases its input locks, withdraws its status segments, frame slots, toasts and row actions, and fails its open client requests with `cancelled`. Durable timers and journal entries survive.

### Lifecycle

```wit
// Excerpt of ilium:types/lifecycle in reference-wit
enum deactivation-reason { shutdown, disabled, upgrade, crash-restart, revoked, granted }
```

```wit
// Excerpt of ilium:exports/lifecycle in reference-wit
activate: func(context: activation) -> result<_, plugin-error>;
deactivate: func(reason: deactivation-reason) -> option<list<u8>>;
```

The record `activation` is defined once in the [WIT reference](/pages/reference-wit/#413-lifecycle). It has the fields `plugin`, `api`, `instance`, `side`, `settings-json`, `granted` and `restored`.

`granted` lists the capabilities actually granted, so a plugin can hide features it cannot use. The bytes that `deactivate` returns come back as `restored` on the next activation. Use them to keep state across a restart.

| Reason | When the host sends it |
|---|---|
| `shutdown` | The server or client stops. |
| `disabled` | The user turns the plugin off, or calls `plugins.set-enabled(id, false)`. |
| `upgrade` | An update, a rollback or a dev reload replaces this version. |
| `crash-restart` | Reserved. In the enum but never sent [ABI-028]: a trap drops the instance without a call (see the restart rule below). |
| `revoked` | The user removes a capability. The plugin is activated again with the smaller `granted` list. |
| `granted` | The user adds a capability. The plugin is activated again with the larger `granted` list. |

A client that detaches has no reason of its own. A client instance that loses its connection is deactivated with `shutdown`, and the returned bytes are stored for the next attach.

The time limits are fixed per call. `activate` has 2,000 ms, or 5,000 ms for a bundled plugin (tier T1). `deactivate` has 500 ms, or 1,000 ms for a bundled plugin. If `deactivate` runs past its limit, the host drops the instance and keeps no restore bytes.

### Example 1: lifecycle and handler on the server

This plugin counts the events it receives. It keeps the count across restarts by returning it from `deactivate`. It makes no host calls except `log`, which every plugin may use.

**Grants:** none.

**Rust**

```rust
use ilium_plugin_sdk::exports::{Activation, DeactivationReason, Handler, Lifecycle};
use ilium_plugin_sdk::host::log::{log, Level};
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Plugin {
    events_seen: u64,
}

impl Lifecycle for Plugin {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        if let Some(bytes) = &context.restored {
            if let Ok(array) = <[u8; 8]>::try_from(bytes.as_slice()) {
                self.events_seen = u64::from_le_bytes(array);
            }
        }
        let message = format!("activated {} with {} grants", context.plugin, context.granted.len());
        log(Level::Info, &message);
        Ok(())
    }

    fn deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>> {
        log(Level::Info, &format!("deactivating: {reason:?}"));
        Some(self.events_seen.to_le_bytes().to_vec())
    }
}

impl Handler for Plugin {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        self.events_seen += batch.len() as u64;
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';
import type { Event, Submission } from '@ilium/plugin-sdk/types';

let eventsSeen = 0;

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate(context) {
    if (context.restored) {
      eventsSeen = Number(new TextDecoder().decode(context.restored));
    }
    log('info', `activated ${context.plugin} with ${context.granted.length} grants`);
  },
  deactivate(reason) {
    log('info', `deactivating: ${reason}`);
    return new TextEncoder().encode(String(eventsSeen));
  },
  onEvents(batch: Event[]): Submission[] {
    eventsSeen += batch.length;
    return [];
  },
});
```

The host calls `activate` first, then `on-events` with the first batch. A `deactivate` with `shutdown` returns the count, and the next `activate` receives those bytes as `restored`.

<pre class="term">
<span class="t-prompt">$</span> ilium plugin logs example.skeleton --tail 3
<span class="t-dim">2026-10-11 08:31:02</span> <span class="t-ok">info </span> activated example.skeleton with 2 grants
<span class="t-dim">2026-10-11 08:31:02</span> <span class="t-dim">debug</span> on-events batch: 3 events, 1 submission
<span class="t-dim">2026-10-11 09:45:19</span> <span class="t-ok">info </span> deactivating: shutdown, 0 bytes returned
<span class="t-prompt">$</span> <span class="t-dim">3 lines shown</span>
</pre>

Illustrative, not generated. The log lines a skeleton plugin writes during one session.

### Example 2: a detector on the server

A detector identifies an agent in a pane's process list, then classifies the pane's screen. This example recognises Codex by its process name. It reports the agent as working while the screen shows the interrupt hint, and as idle otherwise.

**Grants:** `agent.read`, the grant for agent state.

**Rust**

```rust
use ilium_plugin_sdk::exports::{
    Activation, Classification, Detector, DetectorProcess, DetectorScreen,
    DeactivationReason, Handler, Lifecycle,
};
use ilium_plugin_sdk::primitives::agent::AgentActivity;
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "server", exports = ["detector"])]
#[derive(Default)]
struct Plugin;

impl Lifecycle for Plugin {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }

    fn deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        None
    }
}

impl Handler for Plugin {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

impl Detector for Plugin {
    fn identify(&mut self, processes: Vec<DetectorProcess>) -> Option<String> {
        processes
            .iter()
            .find(|process| process.name == "codex")
            .map(|_| "codex".to_string())
    }

    fn classify(&mut self, _signature: String, screen: DetectorScreen) -> Classification {
        let working = screen.rows.iter().any(|row| row.contains("esc to interrupt"));
        let activity = if working { AgentActivity::Working } else { AgentActivity::Idle };
        Classification { activity, objective: None, goal: None }
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Classification, DetectorProcess, DetectorScreen } from '@ilium/plugin-sdk';

export const { lifecycle, handler, detector } = definePlugin({
  side: 'server',
  activate(context) {},
  deactivate(reason) { return undefined; },
  onEvents(batch) { return []; },
  identify(processes: DetectorProcess[]): string | undefined {
    return processes.some((process) => process.name === 'codex') ? 'codex' : undefined;
  },
  classify(signature: string, screen: DetectorScreen): Classification {
    const working = screen.rows.some((row) => row.includes('esc to interrupt'));
    return { activity: working ? 'working' : 'idle', objective: undefined, goal: undefined };
  },
});
```

A detector has to export `lifecycle` and `handler` as well, because they are required in every server plugin. The `activity` values are the `agent-activity` cases: `none`, `working`, `idle`, `awaiting-approval`, `done` and `menu`. Rust writes them as `AgentActivity::Working`. TypeScript writes the kebab-case string.

<figure>
<svg viewBox="0 0 400 200" role="img" aria-label="Host triggers mapped to the export each one calls">
<title>Host triggers and the exports they call</title>
<desc>Four host triggers, each with an arrow to the export it calls. A call happens only for an export the plugin declared.</desc>
<defs>
<marker id="export-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="8" y="10" width="130" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="73" y="29" font-size="11" text-anchor="middle" fill="currentColor">Plugin activated</text>
<line x1="140" y1="25" x2="196" y2="25" stroke="currentColor" marker-end="url(#export-arrow)"/>
<rect x="200" y="10" width="192" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="296" y="29" font-size="11" text-anchor="middle" fill="currentColor">lifecycle.activate</text>
<rect x="8" y="60" width="130" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="73" y="79" font-size="11" text-anchor="middle" fill="currentColor">Events arrive</text>
<line x1="140" y1="75" x2="196" y2="75" stroke="currentColor" marker-end="url(#export-arrow)"/>
<rect x="200" y="60" width="192" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="296" y="79" font-size="11" text-anchor="middle" fill="currentColor">handler.on-events</text>
<rect x="8" y="110" width="130" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="73" y="129" font-size="11" text-anchor="middle" fill="currentColor">View opens</text>
<line x1="140" y1="125" x2="196" y2="125" stroke="currentColor" marker-end="url(#export-arrow)"/>
<rect x="200" y="110" width="192" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="296" y="129" font-size="11" text-anchor="middle" fill="currentColor">views.render</text>
<rect x="8" y="160" width="130" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="73" y="179" font-size="11" text-anchor="middle" fill="currentColor">Agent process seen</text>
<line x1="140" y1="175" x2="196" y2="175" stroke="currentColor" marker-end="url(#export-arrow)"/>
<rect x="200" y="160" width="192" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="296" y="179" font-size="11" text-anchor="middle" fill="currentColor">detector.identify</text>
</svg>
<figcaption>Each host trigger calls one export. A trigger for an export the plugin did not declare calls nothing.</figcaption>
</figure>

### Deadlines per export

Each export runs under a deadline. The default is the same for every tier. The value in brackets is the **T1 ceiling**: the most that a bundled plugin may ask for in its manifest `limits`. T2 and T3 ceilings are in [Host scheduling](/pages/reference-host-scheduling/), which holds the single call-class table with the breach effect and the failure counting of each row. A batch that contains a `command-invoked` event runs under `command_deadline_ms` (50 ms, T1 ceiling 100 ms) instead of the event deadline.

| Export | Deadline | Breach result |
|---|---|---|
| `lifecycle.activate` | 2,000 ms (5,000 ms) | trap |
| `lifecycle.deactivate` | 500 ms (1,000 ms) | instance dropped, no restore bytes |
| `handler.on-events` | 20 ms (50 ms) | trap |
| `views.render` | 10 ms (20 ms) | trap |
| `annotator.annotate` | 10 ms (20 ms) | late result dropped; the third breach in a row traps |
| `renderer` frame | the frame budget (1,000 ms divided by the effective frame rate, rounded down; 33 ms at 30 fps; `frame_deadline_ms` may raise it to at most 50 ms) | frame skipped; 30 breaches in a row trap |
| `detector.identify` and `detector.classify` | 20 ms (50 ms), key `detector_deadline_ms` | previous classification kept; the third breach in a row traps |
| `lifecycle.migrate` | 30,000 ms (60,000 ms), key `migrate_deadline_ms` | the migration fails, the backup is restored; not a failure |
| `hook-handler.handle-hook` | 20 ms by default, 5 to 50 ms by the manifest `deadline_ms` | counts as `pass`; the third breach in a row disables that extension |
| `service-provider` call | 50 ms (100 ms), key `command_deadline_ms` | trap; the caller's request fails with `internal` |
| `inference-provider.complete` and `models` | 50 ms (100 ms), key `command_deadline_ms` | trap; the request fails with `internal` |
| client-request answer | 5,000 ms request timeout (30,000 ms) | the request completes `failed(timed-out)` |
| `settings-validate` | 100 ms | the save is refused and the previous value kept |
| `catalog-provider.query-catalog` | 20 ms | cached page used, breach counted |
| `menu-provider.menu-items` | 20 ms | static entries only, breach counted |
| `virtual-rows-provider.list-virtual-children` | 20 ms | spinner row stays, breach counted |
| `token-provider.tokens` | 10 ms per call | lines stay plain, host retries |
| `job-runner.run` | 2,000 ms CPU (60,000 ms), 20,000 ms wall (600,000 ms) | `failed(timed-out)` |

<div class="warn">
<p>A Rust panic or a thrown TypeScript exception in on-events or views.render is a trap, and a trap restarts the plugin. In deactivate it drops the instance without counting as a failure. Handle every expected error inside the export and return a value.</p>
</div>

### Errors and restarts

An export can return `plugin-error` only when a caller is waiting for its result. The host passes the error on in these cases:

- `job-runner.run` fails the job request with that error.
- `inference-provider.complete` fails the inference request.
- A deferred `service-provider` reply fails the service call.

Exports with no outcome, such as `handler.on-events`, `views.render` and `deactivate`, cannot return an error. The table shows each failure and whether it counts toward the restart rule.

| Situation | Result | Counts as a failure |
|---|---|---|
| `activate` returns an error | logged at level error with the code and message | yes |
| Trap or deadline breach in `on-events` or `views.render` | the plugin is restarted | yes |
| Trap or deadline breach in `deactivate` | the instance is dropped and no restore bytes are kept | no |
| Restart of a stuck event queue, or death of the sandbox helper during a call | the plugin is restarted | yes |
| Trap in a job instance | `failed(internal)` with the trap text | no |
| Job CPU or wall deadline | `failed(timed-out)` | no |
| Job memory limit | `failed(quota-exceeded)` | no |
| Re-activation for `revoked` or `granted` | the plugin runs with the new grants | no |
| `disabled`, `upgrade` or `shutdown` | the plugin stops | no |

Restart back-off is 1 s after the first failure and 5 s after the second. The third failure inside a sliding window of 10 minutes disables the plugin on that side. The rules are defined once in [Lifecycle](/pages/lifecycle/#failure-counting-and-restart). On the client side each client counts separately, and a failing view counts for the plugin, not for the view.

Two more results apply to a batch. A malformed submission completes as `refused(invalid-argument)`, or `refused(not-permitted)` when its grant is missing. Every other submission in that batch completes as `refused(cancelled)` with the message `batch refused: submission <index>`. A return of more than 256 submissions is a trap.

### What the user sees

<pre class="term">
<span class="t-warn">warn</span> Plugin example.detector stopped after 3 failures in 10 minutes:
      detector.identify trap. Turn it on again in Settings &gt; Plugins.
</pre>

Illustrative, not generated. The notice shown in the tree footer and in Settings > Plugins after the third failure. The text names the view only when the failing export was `views.render`.

- The plugin's log lines appear in `ilium plugin logs`.
- A disabled plugin stays disabled until the user turns it on in Settings > Plugins. Its contributions are withdrawn and its pending requests are cancelled.
- Turning it on again resets the failure count to zero.

Requirement ids for this page: [RND-001]. See the [requirements index](/pages/reference-requirements/).

### See also

- [Lifecycle](/pages/lifecycle/) for what a disable does to contributions.
- [Events](/pages/api-events/) for the batches that `handler` receives.
- [Intents](/pages/api-intents/) for the submission outcomes.
- [Primitives: agent](/pages/primitives-agent/) for the agent state that `detector` reports.
- [Services](/pages/api-services/) for `service-provider`.
- [UI](/pages/api-ui/) for `views`.
- [Scenes](/pages/api-scenes/) for `renderer`.
- [Jobs](/pages/api-jobs/) for `job-runner`.
- [Catalogs](/pages/api-catalogs/), [menu provider](/pages/api-menu-provider/), [virtual rows provider](/pages/api-virtual-rows-provider/) and [token provider](/pages/api-token-provider/) for the provider exports.
- [Inference](/pages/api-inference/) for `inference-provider`.
