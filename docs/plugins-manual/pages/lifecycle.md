---
layout: page
title: Lifecycle
permalink: /pages/lifecycle/
---

## Lifecycle

A plugin moves through install, activation, running, stopping and, when something goes wrong, failure, revocation or disable. The host decides when each stage happens. Your code receives two calls, `activate` and `deactivate`. The bytes you return from `deactivate` come back as `restored` on the next activation, so a cursor or a scroll position can survive a restart.

## You will need

- A package with a `manifest.json`. Add the `activation` key for every trigger that must fire without a user action. See [Packages and manifests](/pages/packages/) and [Reference: manifest](/pages/reference-manifest/).
- Every component exports `lifecycle` and `handler`. The manifest lists only the optional exports, such as `views` or `detector`.
- For Rust: a stable Rust toolchain with the WASI Preview 2 component target, and the `ilium-plugin-sdk` crate. For TypeScript: Bun and `@ilium/plugin-sdk`. The SDK release pins the toolchain versions it needs, so nothing is installed globally.
- The grants your code uses. A plugin runs without any grant, and it sees only the grants listed in `granted`. See [Capabilities and grants](/pages/capabilities/).

## Install

Installing verifies the package and records the grants the user accepts.

- `ilium plugin install <file.iliumpkg> [--allow-unsigned]` installs a package. The check covers the layout, the file inventory, the `api` field and the `ilium:api-version` section of each component. A component built for a newer minor version than the host is refused, and the error names both versions.
- Tier T2 needs a signature from a publisher key the user trusts. `ilium plugin trust add <publisher.pub>` adds a key. An unknown key is offered for trust with its fingerprint, and declining refuses the install. T3 packages install only with `--allow-unsigned` or through `ilium plugin dev`.
- For a T2 or T3 package, the review dialog lists every capability with its risk. Normal capabilities are checked by default, high-risk ones are unchecked. An unchecked capability is not granted.
- A T1 bundled plugin receives its manifest ceiling automatically. Its high-risk capabilities are asked once, at first activation, with `Allow selected` and `Not now`. `Not now` asks again at the next server start, at most three times. The counter is stored with the grants record and is reset by an update ([LIF-275]).
- Installing a newer version is an update, and the dialog shows only the new capabilities. A lower version is refused unless you pass `--allow-downgrade`. The full pipeline, rollback and uninstall are specified in [Install, update and uninstall](/pages/reference-install-update-uninstall/).
- On Windows and macOS, T2 and T3 packages are refused with `Cannot install <name> <version>.` and the sandbox message. Bundled plugins keep working.

## Activation

A plugin is not instantiated at install. The host instantiates it when one of its triggers fires. Without an `activation` key, the plugin is instantiated on the first use of any of its contributions. That is lazy loading. A plugin that publishes decorations from the start must declare `"activation": ["startup"]`.

| Trigger | Fires when |
|---|---|
| `startup` | the server starts, or the client starts for client-side plugins |
| `event:<event-name>` | the named event is delivered to the plugin |
| `command:<id>` | a command of the plugin is invoked |
| `view:<id>` | the plugin's view is opened |
| `pane-kind:<id>` | a pane of the plugin's kind is created or restored |
| `scene:<id>` | the plugin's scene is shown |
| `cli:<name>` | the plugin's subcommand runs |
| `http:<path>` | a request arrives on the plugin's route |
| `service:<interface>` | a caller invokes the service, which waits for activation |
| `topic:<topic>` | the exact topic is published; a pattern subscriber uses `startup` |
| `settings:<tab-id>` | the plugin's own settings tab is opened, or a built-in tab that carries one of its sections is opened |

`activate` receives a context record. Its fields are the same in every language.

| WIT field | Rust | TypeScript | Meaning |
|---|---|---|---|
| `plugin` | `plugin` | `plugin` | the plugin id |
| `api` | `api` | `api` | the API version string |
| `instance` | `instance: u64` | `instance: bigint` | the instance number, unique per run |
| `side` | `side` | `side` | `server`, `client` or `scene` |
| `settings-json` | `settings_json` | `settingsJson` | the current settings as a JSON object |
| `granted` | `granted: Vec<String>` | `granted: string[]` | grants the user accepted; declined ones are absent |
| `restored` | `restored: Option<Vec<u8>>` | `restored: Uint8Array \| undefined` | bytes from the last `deactivate`, or none |

The WIT signatures are `activate: func(context: activation) -> result<_, plugin-error>` and `deactivate: func(reason: deactivation-reason) -> option<list<u8>>`.

The `activate` call may yield. Its deadline is 2 s by default and 5 s for T1 plugins (`activate_deadline_ms`). A breach is a trap. The `deactivate` call has 500 ms by default and 1 s for T1 plugins (`deactivate_deadline_ms`). When it misses that deadline, the instance is dropped and no restore bytes are kept. The defaults are the same for every tier, and the figures for T1 are ceilings. Every deadline, its breach effect and whether it counts as a failure are in the call-class table of [Host scheduling](/pages/reference-host-scheduling/#call-classes).

The reasons passed to `deactivate` are:

| Reason | Sent when | Counts as a failure | Returned bytes |
|---|---|---|---|
| `shutdown` | the server stops, after its services stop | no | stored as restore state |
| `disabled` | the user switches the plugin off | no | kept for the next activation |
| `upgrade` | an installed update replaces the running version, or `ilium plugin dev` rebuilds it | no | passed to the new version as `restored` when its `data_version` is unchanged |
| `revoked` | a granted capability is revoked | no | passed back as `restored` |
| `granted` | a capability is added | no | passed back as `restored` |
| `crash-restart` | reserved, never sent [ABI-028]; a trap drops the instance without a call | not applicable | none |

The enum `deactivation-reason` has six cases in a fixed order: `shutdown`, `disabled`, `upgrade`, `crash-restart`, `revoked`, `granted`. Five are delivered. A client that disconnects has no case of its own: the host deactivates that instance with `shutdown` and the returned bytes are stored as restore state, see [persistence and restart](/pages/reference-persistence-and-restart/).

- [LIF-043] The enum `deactivation-reason` MUST have the six values above, and the host MUST send `crash-restart` never. Test: enumerate the reasons seen across a full lifecycle test.
- [LIF-044] A production update MUST send `upgrade` to every live instance of the old version before the swap. Test: update an active plugin and read the reason.

Revocations and grant changes re-activate the plugin with the new `granted` list, so read `granted` on every activation.

## Durable timers and further triggers

Ordinary timers end with the instance. A durable timer (`timers.at`, always granted) survives a server restart. It fires as `durable-timer-fired` with `timer`, `name` and `late-by-millis`. If Ilium was not running at the due time, it fires once after the next activation, with the lateness. The same `name` replaces an earlier timer. Durable timers and journal entries also survive a deactivation.

Further activation triggers are `catalog:<id>`, `menu:<id>`, `virtual-rows:<row-kind>`, `language:<id>`, `client-request:<kind>` and `durable-timer`. On stop, the host also closes the plugin's HTTP streams, ends its spawned children, releases its input locks, withdraws its status segments, frame slots, toasts and row actions, and fails its open client requests with `cancelled`. After a trap the host does the same for the dead instance, and also cancels the jobs and ends the helper processes it started ([LIF-271]). A `cli` entry with `requires_server: false` activates the server component in the CLI process and deactivates it with the reason `shutdown` at the end of the command.

**Rust**

```rust
// Re-arm after every activation. The same name replaces an earlier timer.
timers::at(clock::now_unix_millis() + 86_400_000, "nightly-scan")?;
```

**TypeScript**

```ts
// Re-arm after every activation. The same name replaces an earlier timer.
timers.at(clock.nowUnixMillis() + 86_400_000n, 'nightly-scan');
```

## Activation order

The host decides which plugins start and in which order. This section is normative.

### Start order

On the server, the kernel starts in this order:

1. Boot and configuration load.
2. The snapshot is restored.
3. The plugin host verifies each installed manifest, applies policy and safe mode, and registers static contributions. Nothing is instantiated yet.
4. Plugins with the `startup` trigger are activated, in the order below.
5. Each activated plugin receives one `tree-changed` batch for all restored nodes.

The `startup` trigger fires when the server starts for server-side plugins, and when the client starts for client-side plugins.

- [LIF-045] The host MUST compute the start order of `startup` plugins with Kahn's algorithm over the edges of the non-optional `requires.plugins` entries and of the `requires.services` entries to their providers. A provider's `activate` MUST return before any consumer that requires it begins `activate`. Test: three plugins, A requires B requires C, record the order of `activate.begin`.
- [LIF-046] At each step of that algorithm the host MUST take the ready node with the smallest key: the tier number (T1 first), then the plugin id in ascending byte order. Test: install T1 `b`, T2 `a` and T1 `a` with no dependencies and assert the order T1 `a`, T1 `b`, T2 `a`.
- [LIF-047] Independent plugins MAY activate in parallel. The host MUST run at most 4 concurrent activations per side, and MUST start them in the order of LIF-046. Test: 12 independent plugins, each blocking in `activate`, and assert the peak is 4 and that the begin order is the key order.
- [LIF-048] A cycle in the non-optional `requires.services` entries MUST be refused at install, at stage 10, with `manifest.service-cycle`. A cycle in `requires.plugins` is refused with `dependency.cycle`. The rules for both are in [Versions and dependencies](/pages/reference-dependency-resolution/). Test: install two plugins that require each other's services and read the code and the cycle order.
- [LIF-049] Dependencies are `requires.plugins` and `requires.services`. The host version is checked only through `requires.host`, which is refused with `dependency.host-version` at stage 10. A key named `engines` is refused with `manifest.unknown-key`. Test: install a manifest with `requires.host` above the running release, and one with an `engines` key.

### Stop order

- [LIF-050] On server stop the host MUST call `deactivate(shutdown)` in the exact reverse of the start order, one at a time, so that a consumer stops before the providers it requires. The order of completion of the starts does not matter. Test: record the order of `deactivate` calls after a start in which activations finished in a different order.

### When a provider is not available

A consumer is a plugin that requires a service interface. A consumer is never deactivated because its provider changed state.

| Provider state | What a call to the service returns |
|---|---|
| `activating` or `inactive` | The call waits for activation, up to the call deadline (`service:` trigger). |
| `restart-scheduled` | `busy` with `retry-after-ms` set to the time left on the timer. |
| `stopped-by-failure`, `installed-disabled-by-user`, `disabled-untrusted`, `disabled-blocked`, refused or uninstalled | `not-found`. |

- [LIF-051] A consumer MUST stay active when its provider fails, is disabled or is uninstalled. Test: disable the provider and assert the consumer state.
- [LIF-052] The host MUST return the code in the table for a service call. Test: one case per row.
- [LIF-053] The host MUST send the event `service-availability-changed` (`interface`, `available`, `provider`) to every active consumer when the availability of a required interface changes. Test: disable and enable the provider and read the events.
- [LIF-054] When the user disables a plugin that is a provider for other plugins, the disable dialog MUST list the dependants, and the CLI MUST print a `warning` line for each dependant. The disable is not blocked. Test: disable a provider and read the dialog text.

### Extension owners and contributors

- [LIF-055] The host MUST activate the owner of an extension point before its contributors. After the owner's `activate` returns, the host MUST activate in the background the lazy contributors of its points, so that a contribution needs no trigger of its own. Test: a lazy contributor to a hook point becomes `active` after the owner activates.
- [LIF-056] A hook call MUST NOT wait for a contributor that is not yet `active`. The host skips it, treats it as `pass` and logs `extension.contributor-not-ready`. Test: call a hook while the contributor is `activating`.

### Client handshake and version skew

The client handshake carries the plugin id, version and API version of every installed plugin. A mismatch in the API version fails loudly, as before. A difference in plugin version is allowed during an update.

- [LIF-057] If the server and a client run different versions of one plugin, each side MUST keep running the version it has until it receives the change notice for the update. The host MUST NOT block messages between the sides. Test: update a plugin while a client is attached and assert both sides finish on the new version.
- [LIF-058] A client whose API version differs from the server's MUST be refused at the handshake with the message `api-version mismatch` naming both versions. Test: connect a client with a different API version.

### Startup budget

- [LIF-059] The client SHOULD render its first frame when the essential client plugins are active, or after 1,000 ms, whichever comes first. Plugins that are not active by then continue to activate in the background, and the first frame shows a `starting plugins` mark. Test: delay a non-essential plugin by 3 s and assert the first frame appears within 1,100 ms.

## Core plugins at boot

The `startup` trigger applies per side: a server side with `startup` starts when the server host starts, and a client side with `startup` starts when the client host starts. The core plugins that start at boot and those that start on first use are listed in [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). Safe start (`--safe-plugins`) loads only the essential set, and `--no-plugins` loads none. [COR-020] [COR-021]

## Running

The host calls the exports in response to events, views, commands and scene frames. Each call has its own deadline, for example 20 ms for `handler.on-events`. A breach counts as a failure. See [Errors and limits](/pages/reference-errors-and-limits/).

## Stop and the final save

On server shutdown, the services stop first. Then the plugin host refuses new plugin submissions, which complete as `cancelled`. Each active plugin receives `deactivate(shutdown)` in reverse start order (consumers before the providers they require), and its returned bytes become its restore state. Pending requests are cancelled and instances are dropped. The final snapshot is written last, so no plugin intent can reach it after the writer stops.

<figure>
<svg viewBox="0 0 400 260" role="img" aria-label="Server start and shutdown sequence">
  <title>Server start and shutdown sequence</title>
  <desc>Start runs from boot through snapshot restore, plugin host start, startup activation and the first tree-changed batch. Shutdown runs from services stopping through deactivate calls, request cancellation and the final snapshot.</desc>
  <text x="8" y="20" font-size="11" fill="currentColor">Server start</text>
  <line x1="24" y1="26" x2="24" y2="128" stroke="currentColor"/>
  <circle cx="24" cy="40" r="4" fill="currentColor"/>
  <text x="40" y="44" font-size="10" fill="currentColor">Kernel boot and configuration load</text>
  <circle cx="24" cy="62" r="4" fill="currentColor"/>
  <text x="40" y="66" font-size="10" fill="currentColor">Snapshot restored</text>
  <circle cx="24" cy="84" r="4" fill="currentColor"/>
  <text x="40" y="88" font-size="10" fill="currentColor">Plugin host verifies manifests and registers contributions</text>
  <circle cx="24" cy="106" r="4" fill="currentColor"/>
  <text x="40" y="110" font-size="10" fill="currentColor">Startup plugins activated, providers before consumers</text>
  <circle cx="24" cy="128" r="4" fill="currentColor"/>
  <text x="40" y="132" font-size="10" fill="currentColor">One tree-changed batch for all restored nodes</text>
  <text x="8" y="162" font-size="11" fill="currentColor">Server shutdown</text>
  <line x1="24" y1="168" x2="24" y2="248" stroke="currentColor"/>
  <circle cx="24" cy="182" r="4" fill="currentColor"/>
  <text x="40" y="186" font-size="10" fill="currentColor">Services stop, plugin submissions are refused</text>
  <circle cx="24" cy="204" r="4" fill="currentColor"/>
  <text x="40" y="208" font-size="10" fill="currentColor">deactivate(shutdown) in reverse activation order</text>
  <circle cx="24" cy="226" r="4" fill="currentColor"/>
  <text x="40" y="230" font-size="10" fill="currentColor">Pending requests cancelled, instances dropped</text>
  <circle cx="24" cy="248" r="4" fill="currentColor"/>
  <text x="40" y="252" font-size="10" fill="currentColor">Final snapshot written</text>
</svg>
<figcaption>Start runs top to bottom. On shutdown the final snapshot is written last, so no plugin intent reaches it.</figcaption>
</figure>

## Worked example: keep a cursor across restarts

The Rust example is a client plugin. It keeps a line number in memory, saves it on shutdown and reads it back on the next activation. It moves the cursor on a command, and it moves it only when the user granted `fs.read:project`. Both exports are required, so the plugin implements `Handler` as well as `Lifecycle`.

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::exports::{Activation, DeactivationReason};
use ilium_plugin_sdk::host::log::{self, Level};

const NEXT_COMMAND: &str = "com.example.cursor/next";
const FILE_READ_GRANT: &str = "fs.read:project";

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct CursorKeeper {
    cursor_line: u32,
    can_read_files: bool,
}

impl Lifecycle for CursorKeeper {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        self.can_read_files = context.granted.iter().any(|grant| grant.as_str() == FILE_READ_GRANT);
        if let Some(saved) = context.restored.as_deref() {
            if let Ok(four_bytes) = <[u8; 4]>::try_from(saved) {
                self.cursor_line = u32::from_le_bytes(four_bytes);
            }
        }
        log::log(Level::Info, &format!("cursor at line {}", self.cursor_line));
        Ok(())
    }

    fn deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>> {
        match reason {
            DeactivationReason::Shutdown => Some(self.cursor_line.to_le_bytes().to_vec()),
            _ => None,
        }
    }
}

impl Handler for CursorKeeper {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if let Event::CommandInvoked(invoked) = event {
                if invoked.command == NEXT_COMMAND && self.can_read_files {
                    self.cursor_line += 1;
                }
            }
        }
        Vec::new()
    }
}
```

The TypeScript version uses the same manifest and the same behaviour. `definePlugin` infers the types of `context` and `reason`, so no import is needed for them.

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';

const NEXT_COMMAND = 'com.example.cursor/next';
const FILE_READ_GRANT = 'fs.read:project';

let cursorLine = 0;
let canReadFiles = false;

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate(context) {
    canReadFiles = context.granted.includes(FILE_READ_GRANT);
    if (context.restored !== undefined && context.restored.byteLength === 4) {
      const view = new DataView(context.restored.buffer, context.restored.byteOffset, 4);
      cursorLine = view.getUint32(0, true);
    }
    const reads = canReadFiles ? 'allowed' : 'hidden';
    log('info', `cursor at line ${cursorLine}, file reads ${reads}`);
  },
  deactivate(reason) {
    if (reason !== 'shutdown') return undefined;
    const bytes = new Uint8Array(4);
    new DataView(bytes.buffer).setUint32(0, cursorLine, true);
    return bytes;
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === NEXT_COMMAND && canReadFiles) {
        cursorLine += 1;
      }
    }
    return [];
  },
});
```

Both versions store the cursor as four little-endian bytes and check the length before decoding.

## Failure counting and restart

This section is the single normative source for what counts as a failure and for restart delays. Other pages link here and do not restate the numbers.

A failure is an event that makes the host drop a running instance because of the plugin. When the host drops an instance after a failure, it withdraws the instance's dynamic contributions, keeps its decorations marked stale, cancels its pending requests, records the failure and then either schedules a restart or stops the side.

### The failure window

- [LIF-025] The host MUST keep one failure window for each combination of plugin id, side and scope. The scope is the session server for the `server` side, and one attached client connection for the `client` and `scene` sides. Test: attach two clients, trap the plugin twice on client A and once on client B, and assert that `failures-in-window` is 2 and 1.
- [LIF-026] Two clients MUST NOT share a window. Disabling the plugin by failure on client A MUST NOT stop it on client B or on the server. Test: as above, with a third trap on client A.
- [LIF-027] A window MUST be a sliding window of 10 minutes on a monotonic clock. A failure counts while its age is less than 600 s. Test: with the FakeHost `advance_clock`, trap at 0 s, advance 599 s and assert the failure counts, advance 2 s more and assert it does not.
- [LIF-285] The failure window is per plugin, per side, per scope and per version. It MUST be emptied when the active version changes, on `ilium plugin dev` reload and on a manual enable. The first activation failure of an update candidate is not counted (LIF-075), and a manual `ilium plugin restart <id>` neither counts nor empties the window. Test: trap twice, change the active version and trap again and assert 1 failure; repeat for dev reload and enable; fail a candidate's first activation and assert the window is empty; restart manually and assert the count is unchanged.
- [LIF-028] The first counted failure in a window MUST schedule a restart after 1 s. Test: trap once and read `restart-at`.
- [LIF-029] The second counted failure in a window MUST schedule a restart after 5 s. Test: trap twice within 10 minutes.
- [LIF-030] The third counted failure in a window MUST stop that side with the instance state `stopped-by-failure` and MUST NOT schedule a restart. The package state stays `installed-enabled`. Test: trap three times within 10 minutes and assert no activation follows.
- [LIF-031] The host MUST NOT use any other restart delay. There is no step after the second, because the third failure stops the side. Test: after a stop, advance the clock one hour and assert the side is still stopped.
- [LIF-032] The host MUST NOT add random jitter. The delay timer starts when the instance is dropped. The next activation MUST NOT begin earlier than the delay, and SHOULD begin within 250 ms after it. Test: assert the begin time is in the range delay to delay plus 250 ms.

Failures older than 10 minutes drop out of the count on their own, so a plugin that runs for 10 minutes without a failure starts again from zero. A stable run needs no separate reset rule. For example, failures at 0 s, 300 s and 660 s: the failure at 660 s sees only the one at 300 s in the window, so it is the second counted failure and restarts after 5 s.

<figure>
<svg viewBox="0 0 440 200" role="img" aria-label="Two timelines of failures and restarts">
  <title>Worked failure timelines</title>
  <desc>Timeline A has three traps within ten minutes: restart after 1 s, restart after 5 s, then the side is stopped. Timeline B has traps at 0 s, 300 s and 660 s: the third trap sees only one earlier failure, so it restarts after 5 s.</desc>
  <text x="8" y="16" font-size="11" fill="currentColor">A: three failures inside 10 minutes</text>
  <line x1="20" y1="60" x2="420" y2="60" stroke="currentColor"/>
  <circle cx="40" cy="60" r="4" fill="currentColor"/>
  <text x="40" y="46" font-size="10" text-anchor="middle" fill="currentColor">0 s trap</text>
  <text x="40" y="78" font-size="10" text-anchor="middle" fill="currentColor">restart +1 s</text>
  <circle cx="150" cy="60" r="4" fill="currentColor"/>
  <text x="150" y="46" font-size="10" text-anchor="middle" fill="currentColor">61 s trap</text>
  <text x="150" y="78" font-size="10" text-anchor="middle" fill="currentColor">restart +5 s</text>
  <circle cx="300" cy="60" r="4" fill="currentColor"/>
  <text x="300" y="46" font-size="10" text-anchor="middle" fill="currentColor">180 s trap</text>
  <text x="300" y="78" font-size="10" text-anchor="middle" fill="currentColor">stopped</text>
  <text x="8" y="116" font-size="11" fill="currentColor">B: the first failure expires</text>
  <line x1="20" y1="156" x2="420" y2="156" stroke="currentColor"/>
  <circle cx="40" cy="156" r="4" fill="currentColor"/>
  <text x="40" y="142" font-size="10" text-anchor="middle" fill="currentColor">0 s trap</text>
  <text x="40" y="174" font-size="10" text-anchor="middle" fill="currentColor">restart +1 s</text>
  <circle cx="190" cy="156" r="4" fill="currentColor"/>
  <text x="190" y="142" font-size="10" text-anchor="middle" fill="currentColor">300 s trap</text>
  <text x="190" y="174" font-size="10" text-anchor="middle" fill="currentColor">restart +5 s</text>
  <circle cx="390" cy="156" r="4" fill="currentColor"/>
  <text x="390" y="142" font-size="10" text-anchor="middle" fill="currentColor">660 s trap</text>
  <text x="390" y="174" font-size="10" text-anchor="middle" fill="currentColor">restart +5 s</text>
</svg>
<figcaption>Timeline A stops the side on the third trap. In timeline B the trap at 0 s is older than 600 s when the third trap arrives, so the window holds two failures.</figcaption>
</figure>

### What counts

| Kind of failure | Counts | Notes |
|---|---|---|
| Trap in any export | yes | Includes a Rust panic and a thrown TypeScript exception that escapes an export. |
| `activate` returns an error | yes | Also when a `service:` or `cli:` call triggered the activation. |
| `activate` misses its deadline | yes | |
| `handler.on-events`, `views.render` or another export misses its deadline | yes | |
| `deactivate` misses its deadline, any reason | no | The miss is logged and dropped: the instance is dropped, its restore bytes are discarded and a warn line is logged [HST-191]. |
| Event queue overflow (more than 1,024 events by default) | yes | |
| Memory limit of the instance reached | yes | A trap. |
| Instantiation refused because the session memory pool is full | no | Host capacity, see below. |
| Service provider fails to start when a call needs it | yes | It is an `activate` failure. The caller gets `busy` or `not-found`. |
| Failed job (trap, deadline, memory) | never | The job request completes as failed. The plugin keeps running. |
| Extension handler timeout, trap or error | not for restart | It counts only toward the 3-breach stop of that extension. See [extension points](/pages/api-extension-points/). |
| Sandbox helper process crash or hang (T2, T3) | one failure for the plugin whose export was running, none for the others | The other plugins in the helper restart without a count. See [health and watchdog](/pages/reference-health-and-watchdog/). |
| Revocation, grant, disable, update, shutdown, detach | never | These are deliberate stops. |

- [LIF-033] The host MUST count each kind in the table as the table says. Test: a parametrised test, one case per row, reading `failures-in-window`.
- [LIF-034] A refusal caused by host capacity MUST NOT count against the plugin. The instance stays `inactive` with the reason `host-capacity`. The host MUST retry the activation when pool memory is released, and in any case 1 s, 5 s and 30 s after the refusal, then every 60 s without end while the plugin is enabled. Test: fill the pool, trigger activation, assert `failures-in-window` is 0, free memory and assert the activation runs.
- [LIF-035] A failed job MUST NOT count. Test: trap inside a job and assert `failures-in-window` is unchanged.

### Reset and persistence

- [LIF-036] Enabling the plugin, the command `ilium plugin reset-failures <id>`, the Enable action of the recovery page and installing an update MUST clear the failure window of the affected sides. Test: two traps, then each action, then one trap restarts after 1 s.
- [LIF-037] The instance state `stopped-by-failure` of the server side MUST be written to `failure_stopped` in `installed.json` and MUST survive a server restart. The failure window itself MUST NOT; it starts empty. The state of a client or scene side MUST be dropped when its client detaches. Test: stop the server after a stop-by-failure and assert the state after the restart; detach and reattach a client and assert the client side runs again.

### Restart timer and other events

- [LIF-038] When a server stop begins, the host MUST cancel all pending restart timers and MUST NOT schedule a new one. A failure during shutdown is logged and not counted. Test: schedule a restart, begin a stop and assert no activation.
- [LIF-039] When the user disables the plugin while a restart is pending, the host MUST cancel the timer and move the side to `installed-disabled-by-user`. Test: schedule, disable, advance the clock past the delay and assert no activation.
- [LIF-040] When an update arrives while a restart is pending, the host MUST cancel the timer, clear the window and activate the new version by its normal triggers. Test: schedule, update, assert the new version activates.
- [LIF-041] A revocation or grant while a restart is pending MUST keep the timer. The new instance MUST receive the current `granted` list. Test: schedule, revoke a grant, assert the restarted instance sees the shorter list.
- [LIF-042] An essential core plugin (see [Core plugins](/pages/core-plugins/)) MUST never be stopped by failure. Its third and later failures in a window MUST restart after 5 s and MUST raise the essential-failure banner. Test: trap `ilium.core.tree` three times and assert it restarts and the banner shows.

When the host stops a side, the tree footer shows one notice, called N14 in this manual, and Settings > Plugins lists the same text. This is the only place where the notice is defined. Other pages link here.

- [USR-130] The notice N14 MUST read `Plugin <name> stopped after 3 failures in 10 minutes: <export> <code or trap>. Turn it on again in Settings > Plugins.` The `<export>` is the failing export. It names the view, as in `views.render trap (sidebar)`, only when the failing export was `views.render`. The tree footer and the row in Settings > Plugins MUST show the same text. Test: trap a plugin three times and compare the footer text and the row text with the template.

<figure>
<pre class="term">$ ilium plugin logs com.example.cursor
<span class="t-err">error</span> trap in activate: index out of bounds
<span class="t-warn">warn</span>  restart in 1 s (failure 1 of 3, 10 min)
<span class="t-err">error</span> trap in activate: index out of bounds
<span class="t-warn">warn</span>  restart in 5 s (failure 2 of 3, 10 min)
<span class="t-err">error</span> trap in activate: index out of bounds
<span class="t-err">error</span> stopped: failure 3 of 3, 10 min
</pre>
<figcaption>Illustrative, not generated. Log lines of a plugin that traps three times while it activates.</figcaption>
</figure>

<figure>
<pre class="term">
<span class="t-err">!</span> Plugin Cursor keeper stopped after 3 failures
  in 10 minutes: activate trap. Turn it on
  again in Settings &gt; Plugins.
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Notice N14, the tree footer notice after the third failure. The notice uses the danger colour.</figcaption>
</figure>

A user re-enables a stopped plugin in Settings > Plugins. That clears the failure window, as LIF-036 says. A call to `plugins.set-enabled(id, true)` on a failed plugin clears its `failed` state the same way.

## Dormant extensions

A plugin may contribute to a point whose owner is not installed or not enabled. Install does not fail. The contribution stays in the package and counts for nothing, and Settings > Plugins shows `waiting for <owner>`. When the owner appears, the host activates the contribution and sends the owner `ilium.extensions/changed`. When the owner is disabled, its points drop their contributions in the same way, and they return when it is enabled again. A contributor that was stopped after three breaches in a row stays stopped until its plugin instance restarts or the user presses Reset. The stop is not saved across a server restart ([LIF-270]). See [extension points](/pages/api-extension-points/).

## Disable and enable

`plugins.set-enabled(id, false)` and the Settings switch take effect at once. The call returns `ok` when the change is recorded. Then the plugin receives `deactivate(disabled)` within its deadline, its contributions are withdrawn, its locks are released and its requests are cancelled. Enabling records the change and activates the plugin by its normal rules: a `startup` plugin at once, a lazy plugin at its next trigger.

Disabling or uninstalling removes all decorations of the plugin. A disable keeps the plugin's `data` directory and its storage. An uninstall keeps or deletes them by the rules in [Install, update and uninstall](/pages/reference-install-update-uninstall/). A session bucket is deleted with its session.

### Uninstall cascade

An uninstall changes what other plugins see. Nothing is deleted from them. The table is the one rule set for every page. The user-facing wording is in [Install and manage plugins](/pages/user-install-and-manage/#remove-a-plugin).

| What depended on the removed plugin | What happens |
|---|---|
| A contribution to a point it owned | The contribution goes dormant and Settings shows `waiting for <owner>`. It wakes when the owner returns. |
| A consumer of one of its services | The consumer stays active. Calls return `not-found` ([LIF-051]). |
| A contribution it made to another plugin's point | It disappears from that point at once. |
| A core plugin it replaced | The core plugin turns back on. Its data was never touched. |
| A pane of a kind it provided | The pane stays in the tree as a placeholder, described in [Panes](/pages/api-panes/#a-pane-whose-plugin-is-gone). |
| Keys, menu items, status segments, toolbar buttons | They disappear. |
| The user's order for its points | The entries are deleted. |

- [USR-172] An uninstall MUST apply the effects of the table above in the same host step that removes the plugin from the registry. Test: remove a provider with a consumer, a contributor and a pane, and check each row.

### Settings rows by state

Settings rows follow the plugin's state. A value is never deleted by disable, failure, upgrade or uninstall.

| State | Plugin tab | Section in a built-in tab | Values |
|---|---|---|---|
| Enabled, not yet started | normal; starts when the tab opens (`settings:<tab-id>`) | normal | editable |
| Running | normal | normal | editable |
| Restarting after a failure | a notice names the attempt and the delay | the divider shows `plugin · restarting` | editable, unless the plugin exports `settings-validate`, then read-only |
| Disabled by the user | dim in the rail, with a button to the recovery page | the divider shows `plugin · off`; rows dim and read-only | kept, read-only |
| Disabled after repeated failures | the same as disabled, with the reason | the divider shows `plugin · disabled` | kept, read-only |
| Refused at load (API or schema) | dim, with the required API version | as disabled | kept, untouched |
| Uninstalled | gone from the rail | gone | kept until the user purges them with `ilium plugin uninstall --purge` or "Forget settings" on the recovery page |

A reinstalled plugin reads its kept values again. The recovery page is the only settings screen that stays reachable when the plugin is disabled.

## Revocation and grant changes

Revoking a grant in Settings > Plugins > Capabilities, or with `ilium plugin grants <id> --revoke <grant>`, bumps the authorization epoch. Calls in flight return `revoked`, and pending requests that need the capability complete as `failed(revoked)`. The plugin receives `deactivate(revoked)` and is activated again with the reduced grant set and its restore bytes. Decorations, toolbar state, grabs, claims, highlights, filters, watches and routes that need the capability are withdrawn.

A capability added later by the user, or by an accepted upgrade, produces `deactivate(granted)` and a re-activation with the new set. Neither revocation nor a grant change counts as a failure.

## Development reload

`ilium plugin dev`, run in the package directory, watches the sources and reloads the plugin in a running session. This section is normative. The walkthrough is in [Debugging](/pages/debugging/).

- [LIF-220] `ilium plugin dev` MUST attach to the session of `--cwd` (the current directory by default), or to the session named by `--session <name>`. When no server runs for it, the command MUST fail with `session.unreachable` and exit code 6, and MUST NOT start a session itself [DEV-180]. Test: run it with no server.
- [LIF-221] A development package MUST shadow an installed package with the same id for that session only. When `ilium plugin dev` ends, the installed version MUST become active again. Test: install version 1, run dev with version 2, end dev and read the version.
- [LIF-222] A rebuild MUST start 300 ms after the last change in the directory. Test: touch a file twice 100 ms apart and assert one build.
- [LIF-223] The reload steps MUST run in this order: build, validate the manifest, compare grants, deactivate the server side with `upgrade`, deactivate each attached client side with `upgrade` in attach order, swap, activate the server side, then each client side, each with `restored`. Test: record the log order with two clients.
- [LIF-224] New grants MUST produce a review prompt in the dev terminal, and grants that are no longer in the manifest MUST be revoked without a prompt. With `--grant <name>` the named grants are accepted without a prompt. A new high-risk grant that is not accepted MUST NOT be granted, and the reload MUST continue with the smaller set. Test: add a grant to the manifest and assert the prompt.
- [LIF-225] A change to contributions or activation triggers MUST be re-registered at the swap and MUST be followed by `ilium.extensions/changed` for the affected points. Test: add an extension contribution and read the event.
- [LIF-226] In dev mode, failures MUST be counted and shown but MUST NOT stop the plugin. The third and later failures restart after 5 s. A successful reload MUST clear the window. Test: trap three times and assert the plugin restarts.
- [LIF-227] A failed build or a manifest error MUST keep the running instance and print one `build-error` line. No `deactivate` MUST be sent. Test: introduce a compile error.
- [LIF-228] Restore bytes MUST be passed to the new build only when its `data_version` equals the one of the build that produced them. Otherwise they are dropped and the host logs `restore.dropped-data-version`. Test: change `data_version` between two builds.
- [LIF-229] The dev instance MUST be held by a lease. The `ilium plugin dev` process renews it every 2 s over its connection. The server MUST remove the dev package and restore the installed version when the lease is not renewed for 10 s, or when the connection closes. Test: kill the dev process with SIGKILL and assert the installed version returns within 10 s.

The command prints JSON lines. Every line has a `type`.

| `type` | Fields | When |
|---|---|---|
| `dev-start` | `plugin`, `session`, `version` | the command attached |
| `build` | `plugin`, `ok`, `millis` | a build ended |
| `build-error` | `plugin`, `message`, `file`, `line` | a build or manifest check failed |
| `grant-review` | `plugin`, `added`, `removed` | the grants changed |
| `reload` | `plugin`, `version`, `restored_bytes`, `sides` | a reload finished |
| `plugin-failure` | `plugin`, `side`, `code`, `failures_in_window` | a counted failure in dev mode |
| `dev-stop` | `plugin`, `restored_version` | the command ended |

```text
{"type":"reload","plugin":"<id>","version":"<version>","restored_bytes":<n>,"sides":["server","client"]}
```

A development package carries a `dev` badge in Settings, and it uses the T2 sandbox and ceilings. Plugin logs appear in `ilium plugin logs <id>`.

## What `activate` does not tell you

The `activate` call receives the activation context and the restore bytes, and nothing else about the past. There is no `previous-version` value and no flag that says "this is the first run after an update" in `ilium:plugin@0.1.0`. A plugin that must act once after an update uses one of these two ways, in this order of preference:

1. Put the work in a `migrator` export and raise `data_version`. The host calls it once per side before `activate`, with the old and new data versions ([Data migration](/pages/guide-data-migration/)).
2. Store the version that last ran under a storage key such as `last_version`, compare it in `activate` and write the new value after the work succeeds.

A plugin MUST NOT rely on a deactivate reason or a restore value to detect that an update happened, because `upgrade` is also the reason used by `ilium plugin dev`. [AUT-215] The author rules for updates are in [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/).

## Edge cases and errors

| Situation | What happens | Code the caller sees |
|---|---|---|
| `activate` returns an error | the error is logged at level error with its code and message; counts as a failure | none; the instance is not active |
| `activate` passes its deadline | trap; counts as a failure | none |
| call in flight during revocation | fails | `revoked` |
| pending request when the plugin stops or is disabled | cancelled | `cancelled` |
| HTTP route call open when the plugin stops | answered with a 503 response | `503 Service Unavailable` |
| message to a plugin whose activation fails | discarded; the sender's `ok` stands | none |
| watch or video id used after a restart | the id no longer exists; watch again in `activate` | `not-found` |
| input to a restored pane before its process starts | refused | `pane-not-ready` |
| restore bytes over 1 MiB | dropped; the next activation gets no `restored` value | none; one warn log line |
| contribution to a point whose owner is missing or disabled | dormant until the owner is enabled | none; Settings shows `waiting for <owner>` |
| `deactivate` passes its deadline | instance dropped without restore bytes | none; one warn log line |
| activation refused because the session memory pool is full | the side stays `inactive` with reason `host-capacity`; not a failure | none; the list shows `waiting for memory` |
| trap while a restart is already scheduled | impossible: there is no instance; a second failure needs a new instance | none |
| server stops with a restart scheduled | the timer is cancelled | none |

## What the user sees

The tree footer shows notice N14 when the host stopped a plugin. Settings > Plugins shows the plugin state and its capabilities, with the state words of the row state table. The install review dialog lists the capabilities before installation. `ilium plugin logs <id>` shows every trap, restart, deadline breach and dropped restore value.

<div class="warn">
<p>Restore bytes are not a migration. If a new build changes their layout, it must check the length and fall back to defaults when the bytes do not fit.</p>
</div>

<figure>
<pre class="term">╭  ≡ ● · Plugins ────────────────────────────────────────────────╮
│ <span class="t-hi">com.example.cursor 0.1.0</span>    T2  <span class="t-ok">On</span>                             │
│ com.example.notes 0.1.0     T2  <span class="t-dim">Off</span>                            │
│ vendor.ext 0.1.0            T3  <span class="t-warn">Refused: no sandbox</span>            │
│                                                                │
╰────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>Illustrative. The plugin list shows each plugin with its tier and state. The state words are defined in the row state table of <a href="/pages/core-plugin-settings/#plugin-row-states">Settings, plugin rows</a>. An off plugin keeps its durable timers until it is removed.</figcaption>
</figure>

### Rules defined elsewhere on this page

- [LIF-271] On a trap or stop the host MUST release every effect listed under the stop-effects sentence of [Durable timers and further triggers](#durable-timers-and-further-triggers), for the dead instance as well. Test: trap a plugin holding an input lock, a stream and a child process.
- [USR-173] An update MUST keep the grants the user already answered and MUST ask only for new ones. A failed update MUST roll back to the previous version. The user-facing rules are USR-123 and USR-124 in [Install and manage plugins](/pages/user-install-and-manage/#update-a-plugin). Test: update with one new grant and count the rows in the dialog.
- [LIF-275] An update MUST reset the "Not now" counter of a bundled plugin. Test: answer Not now twice, update, and count prompts.

## See also

- [Plugin settings extension](/pages/settings-extension/) for settings rows in each lifecycle state
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Capabilities and grants](/pages/capabilities/)
- [Packages and manifests](/pages/packages/)
- [Exports](/pages/api-exports/)
- [Storage](/pages/api-storage/)
- [Debugging](/pages/debugging/)
- [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/)
- [Install, update and uninstall](/pages/reference-install-update-uninstall/)
- [Persistence and restart](/pages/reference-persistence-and-restart/)
- [Safe mode and troubleshooting](/pages/user-safe-mode-and-troubleshooting/)
- [Health and watchdog](/pages/reference-health-and-watchdog/)
- [Requirements, lifecycle and operations](/pages/reference-requirements-lifecycle-ops/)
- [Your first plugin](/pages/getting-started-first-plugin/)
- [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/)

API version checked: `ilium:plugin@0.1.0`.

### Security

`ilium --safe-mode` (the same as `--safe-mode=third-party`) starts with all third-party plugins off ([SEC-328]). See [Security response](/pages/reference-security-response/).
