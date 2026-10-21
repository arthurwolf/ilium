---
layout: page
title: Reference, plugin lifecycle states
permalink: /pages/reference-plugin-lifecycle-states/
---

## Reference, plugin lifecycle states

This page is the closed state model of a plugin. It lists every state a plugin package and a plugin instance can be in, every transition between states, the guard that allows it, what the host does in order, and what the user and other plugins see. A host that implements these tables has no state that this page does not name. A test author can derive one test per row.

Contract version: `ilium:plugin@0.1.0`. Requirement ids have the form `[LIF-nnn]`. They are collected in [Requirements, lifecycle and operations](/pages/reference-requirements-lifecycle-ops/). Failure counting and restart delays are defined once, in [Lifecycle](/pages/lifecycle/). This page refers to them and does not repeat the numbers.

### Two state machines

A plugin has two layers of state.

- The package state is stored on disk. It says whether the plugin is installed, whether it may run, and why not. It survives a server restart and a reboot.
- The instance state is held in memory, once per side and per world. It says whether code is running now. A side is `server`, `client` or `scene`. The server side has one instance per session server. The client and scene sides have one instance per attached client.

A package state decides which instance states are allowed. An instance can be `active` only while its side is allowed to run.

### Package states

| State | Stored where | Meaning |
|---|---|---|
| `not-installed` | no record | The id has no entry in the installed registry. |
| `installed-enabled` | `installed.json`, `state` | The package is installed and the user allows it to run. |
| `installed-disabled-by-user` | `installed.json`, `state` | The user, the CLI or `plugins.set-enabled` turned it off. |
| `disabled-untrusted` | `installed.json`, `state` | The signing key was removed from the trust store with `trust remove --force`. The files, grants and settings stay. It leaves this state when the key is trusted again, or with `enable <id> --accept-risk`. |
| `disabled-blocked` | `installed.json`, `state` | The signing key was revoked as `compromised`, or a registry takedown named this version. It leaves this state only with `enable <id> --accept-risk`, or with an update signed by another trusted key of the same publisher. |
| `disabled-by-policy` | policy file, or the start flags | An administrator rule or safe mode forbids it for this run. Not written to `installed.json`. |
| `refused-at-load` | recomputed at every load | The host checked the package and refused to run it. The reason is a code, for example `package.api-unsupported`, `trust.key-revoked`, `policy.tier-denied`, `package.corrupt` or `policy.blocked`. |
| `update-staged` | `plugins/store/<id>/<version>/` | A newer version is unpacked and verified, and the swap has not happened. Used only when the new version asks for grants beyond those the user accepted; otherwise the update goes straight to the swap. |
| `uninstall-pending` | `installed.json`, `state` | The user asked for removal and an instance is still stopping. |

Rules for package states:

- [LIF-001] The host MUST give every installed plugin exactly one package state at any time. Test: list a plugin through every transition below and assert one state in `ilium plugin list --json`.
- [LIF-002] A stop by failure belongs to one side, not to the package. It is the instance state `stopped-by-failure` of that side, and the package state stays `installed-enabled`. A stop of the client side MUST NOT stop the server side. Test: trap the client side three times and assert the server side stays `active`.
- [LIF-003] A package with state `refused-at-load` MUST NOT be instantiated, and the refusal code MUST be shown in `disabled-reason`. Test: install a package whose api window the host does not support and read the list output.
- [LIF-004] `disabled-by-policy` and safe mode MUST NOT be written to `installed.json`. Test: start in safe mode, stop, start normally and assert the plugin is `installed-enabled` again.

### Instance states

| State | Meaning |
|---|---|
| `inactive` | The side is allowed to run and has no instance. It waits for a trigger, or for host capacity. |
| `activating` | The instance exists and `activate` is running. The activate deadline applies. |
| `waiting-dependency` | The side is allowed to run, but a required plugin or the provider of a required service is not `active`. It leaves to `activating` when every requirement is `active`, or to `inactive` with the refusal `dependency.unresolved` shown in the row after `wait_ms` ([PKG-326](/pages/reference-dependency-resolution/)). A wait is not a failure. |
| `active` | `activate` returned `ok`. The host calls the other exports. |
| `deactivating` | `deactivate(reason)` is running. The deactivate deadline applies. |
| `failed` | A counted failure just dropped the instance. The state lasts only until the host picks `restart-scheduled` or `stopped`. It is visible in events and logs, not in lists. |
| `restart-scheduled` | The instance was dropped after a failure and a timer will activate it again. The record carries the due time `restart-at`. |
| `stopped-by-failure` | The third counted failure in a window stopped this side. No restart is scheduled. The server side is stored in `installed.json` as `failure_stopped: ["server"]`. A client or scene side is held only in memory and is dropped when its client detaches. See [Lifecycle](/pages/lifecycle/#failure-counting-and-restart). |
| `stopped` | The side may not run now. The package state says why. |

<figure>
<svg viewBox="0 0 440 250" role="img" aria-label="Instance states and their transitions">
  <title>Instance states</title>
  <desc>An inactive instance becomes activating on a trigger, then active. A failure moves activating or active to failed, then to restart-scheduled or stopped. A restart timer returns restart-scheduled to activating. Active moves to deactivating, then to inactive or stopped.</desc>
  <defs>
    <marker id="pls-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="8" y="100" width="80" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="48" y="121" font-size="11" text-anchor="middle" fill="currentColor">inactive</text>
  <rect x="130" y="100" width="84" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="172" y="121" font-size="11" text-anchor="middle" fill="currentColor">activating</text>
  <rect x="256" y="100" width="70" height="34" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="291" y="121" font-size="11" text-anchor="middle" fill="currentColor">active</text>
  <rect x="350" y="100" width="82" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="391" y="121" font-size="11" text-anchor="middle" fill="currentColor">deactivating</text>
  <rect x="190" y="12" width="70" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="225" y="31" font-size="11" text-anchor="middle" fill="currentColor">failed</text>
  <rect x="60" y="12" width="100" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="110" y="31" font-size="11" text-anchor="middle" fill="currentColor">restart-scheduled</text>
  <rect x="300" y="12" width="70" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="335" y="31" font-size="11" text-anchor="middle" fill="currentColor">stopped</text>
  <line x1="88" y1="117" x2="130" y2="117" stroke="currentColor" marker-end="url(#pls-arrow)"/>
  <line x1="214" y1="117" x2="256" y2="117" stroke="currentColor" marker-end="url(#pls-arrow)"/>
  <line x1="326" y1="117" x2="350" y2="117" stroke="currentColor" marker-end="url(#pls-arrow)"/>
  <line x1="180" y1="100" x2="215" y2="42" stroke="currentColor" marker-end="url(#pls-arrow)"/>
  <line x1="280" y1="100" x2="245" y2="42" stroke="currentColor" marker-end="url(#pls-arrow)"/>
  <line x1="190" y1="27" x2="160" y2="27" stroke="currentColor" marker-end="url(#pls-arrow)"/>
  <line x1="260" y1="27" x2="300" y2="27" stroke="currentColor" marker-end="url(#pls-arrow)"/>
  <line x1="100" y1="42" x2="150" y2="100" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#pls-arrow)"/>
  <path d="M391 134 V190 H48 V134" fill="none" stroke="currentColor" marker-end="url(#pls-arrow)"/>
  <text x="220" y="204" font-size="10" text-anchor="middle" fill="currentColor">disable, update, revoke, grant, detach: back to inactive or stopped</text>
  <text x="300" y="70" font-size="10" fill="currentColor">3rd failure</text>
  <text x="40" y="86" font-size="10" fill="currentColor">timer</text>
  <text x="110" y="232" font-size="10" fill="currentColor">Dashed arrow: automatic. The package state decides stopped.</text>
</svg>
<figcaption>Instance states of one side. A failure always passes through failed, which lasts only until the host chooses a restart or a stop.</figcaption>
</figure>

- [LIF-005] An instance MUST be in exactly one instance state, and only the transitions in the tables below MAY occur. Test: a state-machine fuzz test drives random triggers and asserts every observed transition is in the table.
- [LIF-006] The host MUST NOT call `handler`, `views`, `renderer` or provider exports on an instance that is not `active`. Test: send an event to an instance in `activating` and assert it is queued, and an event to `stopped` is dropped.

### Package-state transitions

The trigger column names who or what causes the change. The effect column is what a user sees. Event names are defined in the section after the tables.

| Row | From | Trigger | Guard | Host actions, in order | Events and log | User-visible effect |
|---|---|---|---|---|---|---|
| P1 | `not-installed` | `ilium plugin install` | all install checks pass | Run the install pipeline. Write the registry entry. Set state. | `plugin-state-changed`, log `install.ok` | Plugin appears in Settings > Plugins |
| P2 | `installed-enabled` | disable | none | Record the desired state. For every side: run I9. | `plugin-state-changed` reason `user` | Switch off, contributions withdrawn |
| P3 | `installed-disabled-by-user` | enable | not refused, not blocked by policy | Record the desired state. Clear the failure windows. Activate per trigger rules. | `plugin-state-changed` reason `user` | Switch on |
| P4 | `installed-enabled` | third counted failure | side-specific | Drop the instance. Do not schedule a restart, except for an essential core plugin (LIF-042), which restarts after 5 s and is never stopped by failure. Set the side `stopped-by-failure`. The package state does not change. | `plugin-state-changed` reason `failure`, log `failure.disabled` | Footer notice, red mark in the list |
| P5 | `installed-enabled` | enable, reset-failures or update | a side is `stopped-by-failure` | Clear the failure window. Set the side `inactive`. | `plugin-state-changed` reason `user` or `update` | Notice cleared |
| P6 | `installed-enabled` | policy file lists the id or version | policy loaded | Stop all sides. Set the overlay. | `plugin-state-changed` reason `policy`, log `policy.blocked` | Row shows `Blocked by policy` |
| P7 | any installed state | package check fails at load | API window, tier, signature, hash, trust, policy | Refuse. Keep the files. | log `load.refused` with the code | Row shows the code and the fix |
| P8 | `installed-enabled` | update downloaded and verified | install pipeline commit step (SEC-100 step 11) committed | Keep the candidate in `store/<id>/<version>/` beside the active version, with `update.state: staged` | `plugin-state-changed` reason `update` | none until the swap |
| P9 | `installed-enabled` with `update.state: staged` (or `update-staged` after a grant-expansion review) | swap | the first `activate` of the candidate returns `ok` on the server side | Run data migration. Activate. Set `active_version` to the candidate. | log `update.ok` | New version shown |
| P10 | `installed-enabled` with `update.state: staged` (or `update-staged` after a grant-expansion review) | swap | the first `activate` fails, traps or misses its deadline | Roll back at once to the previous version and its pre-update data snapshot. Mark the candidate `skip`. Do not count the failure. | log `update.rolled-back` | Toast: `Update failed, rolled back` |
| P11 | any installed state | uninstall | no uninstall already pending | Set `uninstall-pending`. Stop all sides. Remove files. | log `uninstall.ok` | Plugin disappears |
| P12 | `uninstall-pending` | server start after a crash | files still present | Finish the uninstall | log `uninstall.recovered` | none |
| P13 | `installed-enabled` | `trust remove <key_id> --force` | the key signed this plugin | Stop all sides. Record `state`. | `plugin-state-changed` reason `policy`, log `trust.removed` | Row shows `Publisher no longer trusted`, with a badge |
| P14 | `disabled-untrusted` | the key is trusted again, or `enable --accept-risk` | none | Record `installed-enabled`. Clear the failure windows. Activate per trigger rules. | `plugin-state-changed` reason `user` | Switch on |
| P15 | any installed state | key revoked as `compromised`, or registry takedown of this version | none | Stop all sides. Record `disabled-blocked` and the reason. | `plugin-state-changed` reason `policy`, log `trust.key-revoked` or `registry.takedown` | Row shows `Blocked`, with the reason |
| P16 | `disabled-blocked` | `enable --accept-risk`, or an update signed by another trusted key of the same publisher | none | Record `installed-enabled`. Clear the failure windows. | `plugin-state-changed` reason `user` or `update` | Switch on |

- [LIF-007] Enabling a plugin that is `refused-at-load` or `disabled-by-policy` MUST return `not-permitted` with the refusal code in the message, and MUST NOT change any state. Test: call `set-enabled(id, true)` on a policy-blocked plugin.
- [LIF-008] A failure that moves the server side to `stopped-by-failure` MUST be written to `failure_stopped` in `installed.json`, so it survives a server restart. A client or scene side is not stored. Test: cause three failures, restart the server and read the list output.
- [LIF-009] Enabling, resetting failures or installing an update MUST clear the failure window of the sides it affects. Test: two failures, then enable, then one failure restarts after the first-failure delay.
- [LIF-010] A candidate whose first `activate` fails MUST roll back to the previous version, as P10 says, and MUST NOT leave the plugin in `update-staged`. A failure after a successful first activation MUST NOT roll back. Test: stage a version whose `activate` always errors and assert the old version is active.
- [LIF-283] The package states `disabled-untrusted` and `disabled-blocked` MUST follow rows P13 to P16, MUST be stored in `installed.json` and MUST survive a restart. A plugin in either state MUST NOT run, and MUST keep its grants, settings and data. Test: remove the signing key with `--force`, restart the server and assert `disabled-untrusted`; revoke a key as `compromised` and assert `disabled-blocked`; leave each state by the stated way.

### Instance-state transitions

| Row | From | Trigger | Guard | Host actions, in order | Deactivate reason | Events and log | User-visible effect |
|---|---|---|---|---|---|---|---|
| I1 | `inactive` | activation trigger fires | pool memory available, concurrency cap not reached | Create the instance. Call `activate`. | none | log `activate.begin` | none |
| I2 | `inactive` | activation trigger fires | pool memory full | Stay `inactive` with reason `host-capacity`. Retry on release, and after 1 s, 5 s and 30 s, then every 60 s without end while the plugin is enabled ([LIF-034](/pages/lifecycle/)). | none | log `activate.deferred` | Row shows `waiting for memory` |
| I3 | `activating` | `activate` returns `ok` | within deadline | Mark `active`. Flush queued events. | none | `plugin-state-changed`, log `activate.ok` | none |
| I4 | `activating` | `activate` errors, traps or misses its deadline | none | Drop the instance. Count one failure. Run I6 or I7. | none | log `failure.counted` | none, or notice on the third |
| I5 | `active` | trap, deadline breach, queue overflow or memory limit | none | Drop the instance. Withdraw dynamic contributions. Keep decorations, marked stale. Cancel pending requests. Count one failure. | none | log `failure.counted` | Decorations shown stale |
| I6 | `failed` | counted failure number 1 or 2 | not stopping | Schedule the restart timer. | none | log `restart.scheduled` with the delay | Row shows `restarting` |
| I7 | `failed` | counted failure number 3 in the window | none | Set the side `stopped-by-failure`. Run P4. | none | log `failure.disabled` | Footer notice |
| I8 | `restart-scheduled` | timer fires | side still allowed, server not stopping | Run I1. | none | log `restart.begin` | none |
| I9 | `active` | disable | none | Stop accepting calls. Call `deactivate(disabled)`. Withdraw contributions. Release locks. Cancel requests. | `disabled` | log `deactivate.ok` | Row dim |
| I10 | `active` | server stop begins | none | Call `deactivate(shutdown)` in reverse start order. Store the bytes. | `shutdown` | log `deactivate.ok` | none |
| I11 | `active` | client detaches | side is `client` or `scene` | Call `deactivate(shutdown)`. Send the bytes to the server if the link is up. | `shutdown` | log `deactivate.ok` | none |
| I12 | `active` | update swap or dev reload | none | Call `deactivate(upgrade)`. Then run I1 for the new build. | `upgrade` | log `deactivate.ok` | none |
| I13 | `active` | grant revoked or granted | none | Bump the authorization epoch. Call `deactivate(revoked)` or `deactivate(granted)`. Run I1 with the new `granted` list. | `revoked` or `granted` | `plugin-state-changed` | none |
| I14 | `restart-scheduled` | disable, update or uninstall | none | Cancel the timer. No `deactivate` call, there is no instance. | none | log `restart.cancelled` | none |
| I15 | any | `deactivate` misses its deadline | none | Drop the instance. Discard the bytes. | as sent | log `deactivate.deadline` | none |
| I16 | `inactive` | activation trigger fires | a required plugin, or the provider of a required service, is not `active` | Set `waiting-dependency`. Start the wait of `wait_ms` per provider. Activate the missing requirement if it is lazy. | none | log `activate.waiting` | Row shows `waiting for <name>` |
| I17 | `waiting-dependency` | every requirement is `active` | none | Run I1. | none | log `activate.begin` | none |
| I18 | `waiting-dependency` | `wait_ms` runs out | none | Set `inactive` with the reason `dependency.unresolved`. Retry when a requirement becomes `active`, and after 1 s, 5 s and 30 s, then every 60 s without end while the plugin is enabled. | none | log `activate.wait-expired` | Row keeps `waiting for <name>` |
| I19 | `inactive` | disable | none | Record the desired state. Cancel pending retries. No `deactivate` call, there is no instance. | none | `plugin-state-changed` reason `user` | Switch off, contributions withdrawn |
| I20 | `waiting-dependency` | disable | none | Record the desired state. Cancel the wait. No `deactivate` call, there is no instance. | none | `plugin-state-changed` reason `user` | Switch off, `waiting for <name>` cleared |

- [LIF-011] The host MUST apply row I2 when the session memory pool cannot admit the instance, and MUST NOT count that refusal as a failure. Test: fill the pool with other plugins, trigger activation and assert the failure window of the refused plugin is empty.
- [LIF-012] A trap, an error from `activate` or a deadline breach in `activating` or `active` MUST follow rows I4 or I5 and MUST drop the instance without calling `deactivate`. Test: trap in `on-events` and assert no `deactivate` call is logged.
- [LIF-013] The host MUST apply row I14 when a plugin is disabled, updated or uninstalled while a restart is scheduled. Test: schedule a restart, disable the plugin and assert the timer never fires.
- [LIF-014] Rows I9 to I13 MUST send exactly one `deactivate` call per live instance, with the reason in the table. Test: for each row, count `deactivate` calls and read the reason.
- [LIF-015] A client or scene instance MUST receive `deactivate(shutdown)` when its client disconnects cleanly, and MUST receive no other reason. After a crash or a kill of the client process no call is possible: the server uses the last restore bytes it stored. Test: kill the client process and read the server log for the stored restore bytes.
- [LIF-284] A side whose required plugin or service provider is not `active` MUST be in the instance state `waiting-dependency`, MUST NOT count a failure, and MUST follow rows I16 to I18. Test: activate a consumer while its provider is disabled, assert `waiting-dependency`, enable the provider and assert `activating`; repeat with a provider that stays off for longer than `wait_ms` and assert `inactive` with `dependency.unresolved`.

### Events and counters

Two events report state changes. The first is delivered only to plugins that hold `plugins.manage` ([capability](/pages/capability-plugins-manage/)); the second, `ilium.extensions/changed`, is delivered only to the owner of the extension point. The kernel also publishes the first one on the topic `ilium.plugins/state` for the settings screens.

| Event | Fields | When |
|---|---|---|
| `plugin-state-changed` | `plugin`, `side`, `scope`, `from`, `to`, `reason`, `at-unix-ms` | Every row of both tables that changes a package or instance state |
| `ilium.extensions/changed` | `point`, `owner` | A contributor to an extension point appears, changes or disappears. See [extension points](/pages/api-extension-points/) |

The `reason` is one of `user`, `failure`, `policy`, `update`, `uninstall`, `trigger`, `capacity`, `server-start`, `server-stop`, `detach`, `grant` and `dev`.

- [LIF-016] The host MUST emit `plugin-state-changed` once per state change, in the order the changes happened, to every subscriber with `plugins.manage`. Test: record events through a full disable, enable and failure sequence.

### The `plugin-info` record

The record of [help and plugins](/pages/api-help-and-plugins/) has 19 fields, numbered in the table below. `enabled` and `failed` stay as derived fields, so a plugin that reads only them keeps working.

```wit
enum package-state { not-installed, installed-enabled, installed-disabled-by-user,
                     disabled-untrusted, disabled-blocked, disabled-by-policy,
                     refused-at-load, update-staged, uninstall-pending }
enum instance-state { inactive, activating, waiting-dependency, active, deactivating,
                      restart-scheduled, stopped-by-failure, stopped }
enum health-state { healthy, degraded, restarting, failed, disabled, dormant, refused }

record side-state {
  side: string,                       // "server", "client" or "scene"
  scope: string,                      // "server", or the attached client id
  state: instance-state,
  failures-in-window: u32,
  last-error-code: option<string>,    // for example "trap.deadline"
  restart-at-unix-ms: option<u64>,
}

record plugin-info {
  id: plugin-id, name: string, version: string, tier: string,
  enabled: bool, failed: bool,
  state: package-state,
  side-states: list<side-state>,
  scope: string,                      // "global" in 0.1.0
  disabled-reason: option<string>,    // a code such as "user", "untrusted", "blocked", "policy.blocked"
  health: health-state,
  publisher: string,                  // display name from the manifest
  publisher-key: option<string>,      // key id; none for T3
  source: string,                     // "file", "url" or "registry"
  channel: string,                    // "stable" or "beta"
  pinned: bool,
  update-available: option<string>,   // newest allowed version, from cached registry data
  deprecated: bool,
  yanked: bool,
}
```

| No. | Field | Type | Meaning |
|---|---|---|---|
| 1 | `id` | `plugin-id` | The plugin id. |
| 2 | `name` | string | Display name from the manifest. |
| 3 | `version` | string | Version of the active package. |
| 4 | `tier` | string | `T1`, `T2` or `T3`. |
| 5 | `enabled` | bool | Derived, see the rules below. |
| 6 | `failed` | bool | Derived, see the rules below. |
| 7 | `state` | `package-state` | The package state. |
| 8 | `side-states` | list of `side-state` | One record per side and scope. |
| 9 | `scope` | string | `global` in 0.1.0. |
| 10 | `disabled-reason` | option of string | Why the plugin is off. |
| 11 | `health` | `health-state` | The health value. |
| 12 | `publisher` | string | Display name from the manifest. |
| 13 | `publisher-key` | option of string | Key id; none for T3. |
| 14 | `source` | string | `file`, `url` or `registry`. |
| 15 | `channel` | string | `stable` or `beta`. |
| 16 | `pinned` | bool | The user pinned the version. |
| 17 | `update-available` | option of string | Newest allowed version, from cached registry data. |
| 18 | `deprecated` | bool | The registry marks the plugin deprecated. |
| 19 | `yanked` | bool | The registry marks the active version yanked. |

| Derived field | Rule |
|---|---|
| `enabled` | `true` when `state` is `installed-enabled` or `update-staged` |
| `failed` | `true` when any side has the instance state `stopped-by-failure` |
| `disabled-reason` | `none` when enabled, otherwise `user`, `untrusted`, `blocked`, `policy.blocked`, `safe-mode` or a refusal code |

- [LIF-017] `plugins.list` MUST return one record per installed plugin with every field of the `plugin-info` table, in table order. Test: install two plugins, disable one and compare the records with the table.
- [LIF-018] `enabled` MUST equal the rule above in every state. Test: parametrised over the nine package states.

The CLI shows the same data. `ilium plugin list --json` prints one JSON line per plugin, with the type `plugin`:

```text
{"type":"plugin","id":"com.example.notes","version":"0.3.1","tier":"T2","state":"installed-enabled","scope":"global","health":"healthy","enabled":true,"failed":false,"disabled_reason":null,"side_states":[{"side":"server","scope":"server","state":"active","failures_in_window":0,"last_error_code":null,"restart_at_unix_ms":null}]}
```

The JSON uses snake_case keys, as all JSONL output of the CLI does. A final `summary` line gives `plugins` and `problems`.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin list
<span class="t-dim">ID                    VER    STATE      HEALTH</span>
ilium.core.tree       0.1.0  <span class="t-ok">enabled</span>    healthy
com.example.notes     0.3.1  <span class="t-ok">enabled</span>    healthy
com.example.cursor    0.2.0  <span class="t-err">failed</span>     failed
com.example.old       1.0.0  <span class="t-dim">off(user)</span>  disabled
vendor.ext            0.1.0  <span class="t-warn">refused</span>    refused
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The default list prints one row per plugin. Add <code>--json</code> for the full record.</figcaption>
</figure>

### Idempotency and concurrency

- [LIF-019] `set-enabled(id, true)` on an enabled plugin and `set-enabled(id, false)` on a disabled plugin MUST return `ok` and change nothing, and no event MUST be emitted. Test: call twice and count events.
- [LIF-020] The host MUST serialise all transitions of one plugin id. Transitions of different plugin ids MAY run in parallel. Test: issue disable and uninstall at the same time and assert one runs after the other.

How a request behaves while another transition of the same plugin is in flight:

| In flight | A new `set-enabled` | A new install, update or uninstall |
|---|---|---|
| `activating` | recorded at once, returns `ok`, applied when activation ends | queued behind the transition |
| `deactivating` | recorded at once, returns `ok`, applied when the deactivate ends | queued behind the transition |
| update swap | refused with `conflict` | refused with `conflict` and the message `update in progress` |
| `uninstall-pending` | refused with `conflict` | refused with `conflict` |

The last recorded desired state wins. A queued request never waits longer than the sum of the deadlines of the transition in flight. The call returns when the change is recorded, never when the transition ends.

### Edge cases

| Situation | What happens |
|---|---|
| Disable during `activating` | The desired state is recorded. When `activate` returns, the host calls `deactivate(disabled)`. If `activate` never returns, the activate deadline drops the instance. |
| Enable a plugin with a scheduled restart | The plugin is already enabled, so the call returns `ok`. The timer is not changed. |
| Two clients attached, one fails three times | Only that client's side becomes `stopped-by-failure`. The other client keeps the plugin. |
| Server stops while a restart is scheduled | The timer is cancelled. See [Lifecycle](/pages/lifecycle/#failure-counting-and-restart). |
| The registry file cannot be written | The transition is refused with `io` and the state stays as it was. See [offline and low disk](/pages/reference-offline-and-low-disk/). |

### See also

- [Lifecycle](/pages/lifecycle/) for activation order, failure counting and restart delays
- [Install, update and uninstall](/pages/reference-install-update-uninstall/)
- [Health and watchdog](/pages/reference-health-and-watchdog/)
- [Plugin scope and sessions](/pages/reference-plugin-scope-and-sessions/)
- [Persistence and restart](/pages/reference-persistence-and-restart/)
- [Safe mode and troubleshooting](/pages/user-safe-mode-and-troubleshooting/)
- [Requirements, lifecycle and operations](/pages/reference-requirements-lifecycle-ops/)

API version checked: `ilium:plugin@0.1.0`.
