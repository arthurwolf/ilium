---
layout: page
title: Debugging
permalink: /pages/debugging/
---

## Overview

A plugin problem shows up in one of four places: the plugin log, a trap, a deadline breach or the disabled notice in the tree footer. This page explains the `log` import and its levels, how to read `ilium plugin logs`, what the host writes for traps and restarts, how a debug build maps trap frames to source lines, and how `ilium plugin dev` reloads a plugin while you edit it.

The plugin log holds one entry per line: the time, the level, the side (`server`, `client`, `scene` or `host`), the plugin id and the message. The host writes entries for your own log calls, for standard output and error, for calls refused by a missing or declined grant, and for every trap, deadline breach and restart.

## What you need

- A plugin package with a `server`, `client` or `scene` side. The `log` import needs no grant.
- Rust: the crate `ilium-plugin-sdk`. TypeScript: `@ilium/plugin-sdk` with Bun 1.2 or later.
- The `ilium` binary. The `plugin` subcommands are part of it (U5), so nothing else is installed.
- Background reading: [Errors and limits](/pages/reference-errors-and-limits/) for the error codes and the restart rule, and [Lifecycle](/pages/lifecycle/) for `activate` and `deactivate`.

## The log import

The import is `ilium:host/log`. It has one function and one enum.

| Name | Kind | Meaning |
|---|---|---|
| `log(level, message)` | function, no result | Writes one entry. It never fails. |
| `level` | enum `trace`, `debug`, `info`, `warn`, `error` | Severity of the entry, from least to most serious. |
| Rust `log::log(Level, &str)` | SDK function | Rust path `ilium_plugin_sdk::host::log`, enum `Level`. |
| TypeScript `log(level, message)` | SDK function | Import from `@ilium/plugin-sdk/host/log`. Level is a string. |

Rules that apply to every entry:

- A message over 16 KiB is cut at the last whole UTF-8 character that keeps the entry at 16,384 bytes or less. The entry then ends with `…[truncated]`.
- A line break in the message is written as the two characters `\n`, so each entry stays on one line.
- An empty message is accepted and writes an entry with no text after the plugin id.
- The rate is 200 lines per second for an ordinary plugin and 1,000 for a trusted T1 plugin. Lines over the rate are dropped and counted. Once per second, with lines dropped, the host writes one `warn` entry, `<n> lines dropped in the last second`. That summary does not use the rate budget.
- WASI standard output is written at level `info` and standard error at level `warn`. Both share the rate budget. A captured WASI line longer than 16 KiB is cut and marked `[cut]` (HST-078); the `…[truncated]` marker above applies to `log::log` messages only.
- Log entries carry no source position. The Rust `log!` macros of the SDK add `file:line` to the message themselves.

Write each message as `operation: object`, for example `sync: 3 notes saved to data/notes.json`, so that the line can be read without the source code.

## Reading the plugin log

`ilium plugin logs <id>` prints the log of one plugin, oldest entry first. `--tail <n>` prints only the last `n` entries. Each line has this form:

```text
YYYY-MM-DD HH:mm:ss level(5) side(6) plugin-id message
```

The level is padded to five characters and the side to six. The side is `server`, `client`, `scene` or `host` for entries the host wrote. The default threshold is `info`, so `debug` and `trace` entries are dropped unless the plugin runs under `ilium plugin dev` or you raise it with `ilium plugin log-level`. The full format, the storage and the rotation rules are in [Debugging tools](/pages/reference-debugging-tools/). The host writes these kinds of entry:

| Source | Level | Message |
|---|---|---|
| Your log call, stdout, stderr | as given, `info`, `warn` | Your text. |
| Refused call | `warn` | Names the capability, the function and the reason, one word from the closed list in [Errors and limits](/pages/reference-errors-and-limits/#refusal-reasons). A call the manifest never requested is a trap, `host.capability-not-requested`, not a refusal. |
| Grant-sensitive action | `info` | File writes outside `data` and `cache`, and every process run, with the scope or program and the arguments. |
| Trap | `error` | `trap in <export> (<view>): <reason>`, followed by frame entries (see Traps). |
| Deadline breach | `error` | `<export> <elapsed> ms (limit <n> ms)`. |
| Restart | `warn` | `restarting in <1 or 5> s (failure <n> of 3 in 10 minutes)`. |
| Dropped lines | `warn` | `<n> lines dropped in the last second`. |

<figure>
<pre class="term">
<span class="t-prompt">$</span> ilium plugin logs notes --tail 5
<span class="t-dim">2026-10-11 14:02:04</span> info  server notes
  activate: 14 notes opened before
<span class="t-dim">2026-10-11 14:02:09</span> <span class="t-warn">warn</span>
                    notes 12 lines dropped in the last second
<span class="t-dim">2026-10-11 14:02:11</span> <span class="t-err">error</span>
  server notes trap in views.render (sidebar): no child 7
<span class="t-dim">2026-10-11 14:02:11</span> <span class="t-err">error</span>
  server notes   at src/lib.rs:42:7
<span class="t-dim">2026-10-11 14:02:11</span> <span class="t-warn">warn</span>
                    notes restarting in 1 s (failure 1 of 3 in 10 minutes)
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A trap in a view, the frame entry of a debug build and the first restart.</figcaption>
</figure>

## Traps

A trap ends the instance at once. The host drops the instance, withdraws its dynamic contributions, cancels its pending requests and starts a new instance after a delay. A trap is caused by one of these:

- A call to an import that the manifest never requested. A declined grant does not trap: the import returns `not-permitted`. A call without any request in the manifest does trap.
- An invalid `ui-tree`: a child index out of range, or two nodes with the same `key` in one tree. The SDK builder cannot produce a bad index, so this comes from hand-written `raw` code or a duplicate key.
- More than 256 submissions returned from one `on-events` call.
- A deadline breach on an export that traps (see the table below).
- Exceeding `memory_mib` or `table_elements`, or a Rust panic or a thrown TypeScript exception in an export that has no outcome.

The first entry names the export and, for `views.render`, the view. Each backtrace frame follows as its own `error` entry. The restart then follows the schedule in [Lifecycle](/pages/lifecycle/#failure-counting-and-restart): 1 s after the first failure and 5 s after the second. The third failure inside the window disables the plugin on that side.

| Deadline | Default | T1 ceiling | Effect when passed |
|---|---|---|---|
| `event_deadline_ms` | 20 ms | 50 ms | Trap |
| `command_deadline_ms` | 50 ms | 100 ms | Trap |
| `render_deadline_ms` (`views.render`) | 10 ms | 20 ms | Trap |
| `annotate_deadline_ms` | 10 ms | 20 ms | Late result dropped; the third breach in a row traps |
| `frame_deadline_ms` (scene `render`) | the frame budget | 50 ms | Frame skipped; 30 breaches in a row trap |
| `activate_deadline_ms` | 2,000 ms | 5,000 ms | Trap |
| `deactivate_deadline_ms` | 500 ms | 1,000 ms | Instance dropped; no restore bytes kept |

A job that runs past its deadline does not trap the plugin. The job request fails with `timed-out`, and the failure does not count toward the restart rule. Move slow work into a job, as described on the [background jobs page](/pages/background-jobs/).

### Stuck plugins

Each side has an event queue (1,024 entries by default; up to 4,096 for T1). When it is full, the oldest drop-oldest event is evicted and counted, and the host writes `event queue full: dropped N events` at level `error`. Only a never-dropped event that cannot fit makes the plugin stuck, and the host then restarts it. The new instance gets a `tree-changed` event for every node and reads its state again. The restart counts as a failure.

### Failures that do not count

A revoked re-activation, a disable, an upgrade during development and a shutdown never count toward the restart rule. A trap in a job instance does not count either. An `activate` that returns an error does count.

## Source maps and debug builds

`ilium plugin build --debug` keeps the debug information in the component:

- Rust: DWARF information. Each trap frame is written as `at src/lib.rs:42:7`.
- TypeScript: the bundle's source map, written to `dist/<side>.wasm.map`. A package carries the map under `src/` only when it was packed with `--include-src`. The host maps each trap position through it, so frames read `at src/index.ts:18:5`.

Release builds strip the debug information. Their frames show function names only, as `at handle_batch`. The host reads the source map only to write frame entries. The sources under `src/` are never loaded or run, and log entries are never mapped.

## Development reload

`ilium plugin dev` watches the package directory. A plugin installed this way has tier T3 and shows a `dev` badge. Each save follows these steps:

<figure>
<svg viewBox="0 0 400 290" role="img" aria-label="Development reload loop in five steps">
  <defs>
    <marker id="dev-reload-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <g fill="none" stroke="currentColor">
    <rect x="20" y="10" width="260" height="40" rx="6"/>
    <rect x="20" y="66" width="260" height="40" rx="6"/>
    <rect x="20" y="122" width="260" height="40" rx="6"/>
    <rect x="20" y="178" width="260" height="40" rx="6"/>
    <rect x="20" y="234" width="260" height="40" rx="6"/>
    <path d="M150 50 V66" marker-end="url(#dev-reload-arrow)"/>
    <path d="M150 106 V122" marker-end="url(#dev-reload-arrow)"/>
    <path d="M150 162 V178" marker-end="url(#dev-reload-arrow)"/>
    <path d="M150 218 V234" marker-end="url(#dev-reload-arrow)"/>
    <path d="M280 254 H320 V30 H284" marker-end="url(#dev-reload-arrow)"/>
  </g>
  <g fill="currentColor" text-anchor="middle">
    <text x="150" y="26" font-size="12">1. Save detected</text>
    <text x="150" y="41" font-size="10">watched: the package directory</text>
    <text x="150" y="82" font-size="12">2. Deactivate with reason upgrade</text>
    <text x="150" y="97" font-size="10">the old instance returns its restore bytes</text>
    <text x="150" y="138" font-size="12">3. Instantiate the new build</text>
    <text x="150" y="153" font-size="10">same plugin id, new component</text>
    <text x="150" y="194" font-size="12">4. Activate with restored bytes</text>
    <text x="150" y="209" font-size="10">the bytes from step 2 arrive as restored</text>
    <text x="150" y="250" font-size="12">5. Render the views again</text>
    <text x="150" y="265" font-size="10">tree and footer drawn by the new build</text>
    <text x="330" y="134" font-size="10">each</text>
    <text x="330" y="148" font-size="10">save</text>
  </g>
</svg>
<figcaption>A save triggers an upgrade. The new build receives the bytes that the old one returned on deactivate.</figcaption>
</figure>

A save starts a rebuild 300 ms after the last change in the directory. If the build fails, the running instance keeps running, the compiler error is printed in the terminal, and no deactivate happens. If the build succeeds, the old instance receives `deactivate(upgrade)` and returns its restore bytes, within the 500 ms deadline. The bytes are capped at 1 MiB. Bytes over the cap are dropped, and the new instance starts without a `restored` value.

Keep in the restore bytes only the state that the host does not keep. Settings, storage and node metadata already persist across the reload, so most plugins need a few bytes at most.

## Worked example: server-side plugin

**Rust**

The plugin logs each agent start, keeps a count across reloads and warns about a bad restore. The `log::log` calls use the SDK path `ilium_plugin_sdk::host::log`.

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::tree::AgentActivity;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Notes {
    started_work: u32,
}

impl Lifecycle for Notes {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        if let Some(bytes) = context.restored {
            match <[u8; 4]>::try_from(bytes.as_slice()) {
                Ok(array) => self.started_work = u32::from_le_bytes(array),
                Err(_) => log::log(Level::Warn, "restore: 4 bytes expected, count reset"),
            }
        }
        log::log(Level::Info, &format!("activate: {} agent starts so far", self.started_work));
        Ok(())
    }

    fn deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        log::log(Level::Debug, "deactivate: returning the start count");
        Some(self.started_work.to_le_bytes().to_vec())
    }
}

impl Handler for Notes {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::AgentActivityChanged(AgentActivityEvent { current, .. }) = event
            else {
                continue;
            };
            if current != AgentActivity::Working {
                continue;
            }
            self.started_work += 1;
            log::log(Level::Info, &format!("agent started work, count {}", self.started_work));
        }
        Vec::new()
    }
}
```

`Handler` is required for a `server` plugin. `Lifecycle` has default methods, so you implement only the ones you need. Debug entries are written like any other entry, so use `Level::Debug` for detail that you need only while investigating.

**TypeScript**

The client plugin counts editor opens. It writes the count as restore bytes on `deactivate` and reads them on `activate`. It logs a warning, instead of failing, when the restored bytes are not valid JSON.

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';

interface NotesState {
  openedCount: number;
}

const encoder = new TextEncoder();
const decoder = new TextDecoder();
let state: NotesState = { openedCount: 0 };

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate(context) {
    if (context.restored !== undefined) {
      try {
        state = JSON.parse(decoder.decode(context.restored)) as NotesState;
      } catch (error) {
        log('warn', `restore: ${String(error)}, count reset`);
      }
    }
    log('info', `activate: ${state.openedCount} editors opened before this start`);
  },
  deactivate(reason) {
    log('debug', 'deactivate: returning the open count');
    return encoder.encode(JSON.stringify(state));
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'editor-opened') continue;
      state.openedCount += 1;
      log('debug', `editor opened, count ${state.openedCount}`);
    }
    return [];
  },
});
```

Use `log('debug', ...)` for lines that repeat on every event. Keep `info` for lifecycle events, which the user reads while debugging.

## Edge cases and errors

| Situation | What the host does | Entry in the plugin log |
|---|---|---|
| More lines than the rate allows | Drops the extra lines | `warn` summary, once per second |
| A message over 16 KiB | Cuts it, keeps the marker within the cap | Entry ends with `…[truncated]` |
| A call with a declined or revoked grant | Returns `not-permitted`, or `none` for calls with no error channel | `warn` refusal entry |
| A call to an import the manifest never requested | Traps the instance | `error` trap entry |
| A `deactivate` past 500 ms | Drops the instance without restore bytes | `warn`: `deactivate deadline passed; state not kept` |
| Restore bytes over 1 MiB | Drops them, starts without `restored` | `warn`: `restore bytes over 1 MiB dropped` |
| Three failures in 10 minutes | Disables the plugin on that side | Disable notice in the tree footer |

The disabled notice is the last step of the restart schedule. A disabled plugin keeps its `data` and storage. Enabling it again resets the failure count to zero. Check the plugin log for the first trap, because later restarts usually repeat it.

## Debugging the extended surfaces

The contract includes surfaces that fail in ways the earlier sections do not cover. The table lists the first thing to check for each.

| Symptom | Likely cause | What to check |
|---|---|---|
| The render traps, the previous view stays and the plugin log has `ui.invalid-tree` or another `ui.*` code | A tree or widget cap: 4,096 nodes, 1,024 KiB, 256 `check-list` items, 64 `list-editor` items | The log entry names the cap. Page the data with a virtual-row or catalogue provider. |
| `rate-limited` with `retry-after-ms` | More than 10 widget updates per second, or 3 toasts in 10 s | Update when a value changes by a visible step. The entry shows the retry delay. |
| A menu or a token list shows static content | A `menu-items` or `tokens` provider missed its 10 or 20 ms deadline | Time the provider with `clock.monotonic-millis` and cache the result. |
| `quota-exceeded` on `http.stream`, `process.spawn` or `vector` | A requestable limit (`limits` in the manifest) is at its ceiling | Raise the key in the manifest within the tier ceiling, or close what you opened. |
| The manifest is refused with a `manifest.` code | A new manifest code, for example `manifest.asset-size` or `manifest.scene-prerender` | The code and the path are in the install error. See [Manifest reference](/pages/reference-manifest/). |
| A client request resolves with a timeout | `client-requests.request` timeout between 1 and 30,000 ms | Increase the timeout, and check that the target pane still exists. |
| A handler runs after a screen change but sees old rows | `screen-changed` is coalesced to 10 per second per pane | Read the rows in `first-row` to `last-row` of the newest event. |

Use `journal.log-event` for events that a user should see in the session journal, and `log` for the plugin log that only you read. Rows of the plugin log are 16 KiB at most, so log a count or an id, not a whole payload.

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::clock;

fn timed_provider<T>(name: &str, work: impl FnOnce() -> T) -> T {
    let started = clock::monotonic_millis();
    let result = work();
    let elapsed = clock::monotonic_millis() - started;
    if elapsed > 10 {
        log::log(Level::Warn, &format!("{name} took {elapsed} ms; the deadline is 10 to 20 ms"));
    }
    result
}
```

**TypeScript**

```ts
import { log } from '@ilium/plugin-sdk/host/log';
import { monotonicMillis } from '@ilium/plugin-sdk/host/clock';

function timedProvider<T>(name: string, work: () => T): T {
  const started = monotonicMillis();
  const result = work();
  const elapsed = Number(monotonicMillis() - started);
  if (elapsed > 10) log('warn', `${name} took ${elapsed} ms; the deadline is 10 to 20 ms`);
  return result;
}
```

## Beyond the plugin log

The `log` import is one of several tools. The trace, the widget tree inspector, the profiler and the memory view are specified in [Debugging tools](/pages/reference-debugging-tools/). Symptoms from users, listed by phase with a fix for each, are in [Troubleshooting](/pages/guide-troubleshooting/). A plugin MUST NOT depend on `--unredacted` traces to diagnose a user's problem, because that flag works only for a plugin installed with `dev`. [AUT-804]

### Bug report procedure

Ask for the same evidence each time. Only `ilium plugin support-bundle <id> --out <file>` writes a support bundle, and no other command may offer a second format ([USR-104]). The command needs a running session. The file name must not exist yet: an existing file is never overwritten, and the command fails with `support-bundle.exists` and exit 2 ([DEV-832]).

Collect these items:

1. **Support bundle.** The zip holds exactly `summary.txt`, `log.txt` (the newest 200 lines of the plugin log), `stats.txt` and `crashes.txt` ([USR-221]). `summary.txt` lists the Ilium version, plugin id, version, tier, state, platform, host API range, failure count, redaction counts, dropped log lines and a SHA-256 of the other three entries ([USR-335]). The bundle is capped at 8 MiB. When the cap cuts it, the oldest log lines go first and the summary says so ([USR-224], [DEV-833]). Add `--include-host` only when the host may be the cause. It adds the last 500 host log lines, redacted, and the summary names the addition ([USR-334]).
2. **Versions.** `ilium --version`, and `ilium plugin --version`, which prints the CLI version, the contract version and the supported WIT versions without a session ([DEV-906]).
3. **Installed package.** The line of `ilium plugin list --json --inventory` for the plugin. It holds the id, version, tier, content hash, signing key id, state and granted permissions, with no paths under the user's home ([USR-341]). `ilium plugin inspect <id> --verify` recomputes the package hashes and prints a `diff` record for each mismatch; a mismatch exits 1 with `inspect.hash-mismatch` ([DEV-801]).
4. **Environment.** `ilium plugin doctor` as text. Exit 4 means at least one item failed; a warning does not change the exit code ([DEV-811]). The output names state folders, so the user replaces the home folder with `~` before sending it. `doctor` writes text only, never a bundle.
5. **Steps taken.** The user's own account of the command, the exit status and what happened. The template in [Publishing and support](/pages/guide-publishing-and-support/#a-template-for-bug-reports) lists the fields.

Do not ask for these. Unredacted traces: `--unredacted` works only for a `dev` install ([AUT-314], [AUT-804]). Restore bytes or storage: the bundle holds neither, so ask for the plugin's own restore warning line from `log.txt` instead. Standard input content: ask for its byte or line count only, because the content may be private text.

### Redaction check before sharing

The host redacts the bundle as follows ([USR-222]):

| Item | In the bundle | Rule |
|---|---|---|
| Credential headers, such as `Authorization:` | Redacted | [USR-222] |
| URL parameters | Redacted | [USR-222] |
| Home-directory paths | Shown as `~` | [USR-222] |
| Plugin payloads | Byte count only | [USR-222] |
| Values equal to a plugin secret, and their base64 and percent-encoded forms | Replaced with `<redacted>` | [SEC-223], [SEC-243] |

Redaction is a safety net, not permission to log secrets: a plugin MUST NOT write them ([AUT-315]). The rules above do not name process arguments in the grant-sensitive lines, or free text in your own `log` calls, so read those lines by hand. Check the bundle in this order:

1. Run `ilium plugin support-bundle <id> --dry-run`. It prints the file names, sizes and the count of redactions per rule, and writes no zip ([USR-332]). A count of zero for a rule that your test data should trigger is a finding.
2. Read the redacted `summary.txt` and `log.txt` in the viewer the host opens after a bundle is built ([USR-332]).
3. Search each file for every known form of each plugin secret: plain, base64 and percent-encoded. Keep the forms in a file outside the bundle folder and pass it with `grep -F -f`, so they stay out of the shell history. Also search for the home folder. Every count must be 0.
4. If any count is not 0, do not attach the bundle. Fix the plugin so it stops writing that value, then rotate the secret with `ilium plugin secret rotate <id> <name>` ([SEC-451]), because the value has reached a log file.

The bundle is the user's file. Ilium never uploads it, and the author never receives it unless the user sends it ([USR-106], [USR-332]).

### Reproducing a trap from a report

1. In `log.txt`, find the first `trap in <export>` entry and its frame lines. Later restarts usually repeat it ([Lifecycle](/pages/lifecycle/#failure-counting-and-restart)).
2. Ask for a replay file, not the trace text: `ilium plugin trace <id> --last 100 --record trace.replay`. Payloads in a normal trace are redacted ([DEV-672]). The manual does not say whether `--record` keeps payload bytes, so read the file before the user sends it.
3. Run `ilium plugin test --replay trace.replay`. It exits 0 on a match and 3 on divergence ([DEV-544]). A deadline trap reproduces, because the recorded host call times are replayed on the fake clock ([DEV-725]). In component mode a trap is a test failure unless the test calls `expect_trap` ([DEV-403]).
4. When the trap depends on restored state, write a fixture with a `restart` step that carries base64 restore bytes ([Test harness reference](/pages/reference-test-harness/)). Build the bytes with the layout your code writes, whose first byte names the format ([AUT-206]). Restore bytes reach an instance only when its `data_version` matches the writer's ([LIF-103]), so set the same `data_version` in the fixture. The bundle has no restore bytes, so generate them from your own code and never ask the user for theirs.
5. Fix the cause, run the replay again until it exits 0, and keep the fixture in the test suite so the failure does not return ([AUT-316]). Testing commands are in [Testing plugins](/pages/testing/).

## What the user sees

When a plugin is disabled, the tree footer shows notice N14 with the plugin name and the failing export, and Settings > Plugins shows the same text. N14 is defined once, with its render, in [Lifecycle](/pages/lifecycle/#failure-counting-and-restart).

<div class="warn">
<p>Disabling a plugin does not delete its data. Fix the first trap, re-enable the plugin, and check the plugin log for the first trap, because later restarts usually repeat it.</p>
</div>

### Reload and health while you develop

`ilium plugin dev` reloads each rebuilt side with the reason `upgrade` and keeps restore bytes up to 1 MiB ([Lifecycle](/pages/lifecycle/#development-reload)). A trap during development follows the normal restart rule with one exception: the third and later failures restart after 5 s and never disable the side ([Lifecycle](/pages/lifecycle/#development-reload)). Run `ilium plugin status <id>` to see health and near-misses ([Health and watchdog](/pages/reference-health-and-watchdog/)), and `ilium plugin stats <id>` for resource use ([Resource accounting](/pages/reference-resource-accounting/)). A development install can be reset with `--purge-on-exit`.


## See also

- [Testing plugins](/pages/testing/)
- [Lifecycle](/pages/lifecycle/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Background jobs](/pages/background-jobs/)

API version checked: `ilium:plugin@0.1.0`.

### Security

Logs are redacted of secrets ([SEC-243]). If a plugin breaks the client, start with `ilium --safe-mode` ([Security response](/pages/reference-security-response/)).

Requirements for this page: [Developer tooling requirements](/pages/reference-requirements-dev-tooling/).
