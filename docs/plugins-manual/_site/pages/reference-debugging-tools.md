# Debugging tools

## Debugging tools

This page defines the tools that show what a plugin did: the log, the trace, the widget tree inspector, the profiler and the memory view. It also defines the log line format, where logs are stored and how they rotate. The task guide is [Debugging plugins](/pages/debugging/). The commands are in [Plugin command-line reference](/pages/reference-plugin-cli/).

Terms:

- **Log**: text lines that the plugin and the host write about a plugin.
- **Trace**: a record of what crossed the boundary between the host and one plugin instance: events in, submissions out, host calls, outcomes.
- **Inspector**: a view of the widget tree a plugin returned.
- **Profile**: the time each export spent over a period.

## You will need

- A running session for every tool on this page except the log files, which can be read after the session ends.
- A plugin installed with `ilium plugin dev` to see debug-level lines and unredacted traces.

## The log

### Line format

A log line is plain text, one line per entry.

`YYYY-MM-DD HH:mm:ss level(5) side(6) plugin-id message`

| Part | Width | Rule |
|---|---|---|
| Date and time | 19 | Local time of the machine, European order. |
| level | 5, left aligned | `trace`, `debug`, `info`, `warn`, `error`. Padded with spaces. |
| side | 6, left aligned | `server`, `client`, `scene` or `host` for lines the host wrote about the plugin. |
| plugin-id | variable | The plugin id. |
| message | rest | One line. A newline in a message is written as `\n`. |

One space separates the parts. Example:

```text
2030-01-01 09:14:02 info  server example.notes activate: ready
2030-01-01 09:14:09 warn  server example.notes storage near quota: 912 KiB of 1 MiB
2030-01-01 09:15:40 error host   example.notes restarting in 1 s (failure 1 of 3 in 10 minutes)
```

- **[DEV-650]** Every log line MUST follow the format above. A host line MUST use the side `host`.
- **[DEV-651]** A message longer than 16 KiB (16,384 bytes) MUST be cut at the last whole UTF-8 character that keeps the entry within the limit, and the entry MUST end with `…[truncated]`.
- **[DEV-652]** The thresholds in order are `trace`, `debug`, `info`, `warn`, `error`. A line is written when its level is at or above the threshold.
- **[DEV-653]** The default threshold MUST be `info`. A plugin installed with `dev` has the default `debug`. `debug` and `trace` lines at a threshold of `info` are dropped, not stored.
- **[DEV-654]** `ilium plugin log-level` and the environment variable `ILIUM_PLUGIN_LOG_LEVEL` MUST set the threshold. The command wins over the variable.
- **[DEV-655]** The restart message MUST be `restarting in <delay> s (failure <n> of 3 in 10 minutes)` at level `warn`, with the delay of that restart (1 for the first failure, 5 for the second) and its failure count, for example `restarting in 1 s (failure 1 of 3 in 10 minutes)` and `restarting in 5 s (failure 2 of 3 in 10 minutes)`. The last failure MUST log `disabled after 3 failures in 10 minutes`.
- **[DEV-656]** The log rate limit is 200 lines per second for an ordinary plugin and 1,000 for a trusted T1 plugin. Lines over the limit MUST be dropped and counted. Once per second, when lines were dropped, the host MUST write one `warn` line `<n> lines dropped in the last second`. That line does not use the rate budget.
- **[DEV-657]** When the host event queue of a plugin is full, the host MUST write one line with level `error` and the message `event queue full: dropped N events`.
- **[DEV-658]** A plugin MUST NOT be able to write a line with the side `host`.

### Storage and rotation

Logs live at `<ilium-state>/plugins/logs/<session-slug>/<plugin-id>.log`. `<ilium-state>` is the Ilium state folder, shown by `ilium plugin doctor`.

| Rule | Value |
|---|---|
| File size before rotation | 4 MiB |
| Files kept | 5: `<plugin-id>.log` and `.log.1` to `.log.4` |
| Retention after uninstall | 7 days |
| `uninstall --purge` | Deletes the logs at once |
| Permissions | Owner only |

- **[DEV-660]** When the active file reaches 4 MiB, the host MUST rename `.log.3` to `.log.4`, `.log.2` to `.log.3`, `.log.1` to `.log.2`, the active file to `.log.1`, and start a new file. The oldest file is deleted.
- **[DEV-661]** The host MUST delete the logs of an uninstalled plugin 7 days after the uninstall, unless the plugin is installed again.
- **[DEV-662]** `ilium plugin logs` MUST read the files oldest first across rotation, so the output is in time order.
- **[DEV-663]** The log folder MUST be created with owner-only access.
- **[DEV-664]** Logs MUST NOT contain granted data such as file contents or storage values unless the plugin wrote them on purpose. The host lines never include payloads.

### Reading the log

`ilium plugin logs <id> [--level <level>] [--since <time>] [--side <side>] [--follow] [--tail <n>] [--json]`

- **[DEV-665]** `--level` MUST show lines at or above that level. `--since` MUST take `YYYY-MM-DD HH:mm:ss` or a duration such as `10m`.
- **[DEV-666]** `--tail <n>` MUST show the last n lines and default to 200.
- **[DEV-667]** With `--json` each line becomes `{"type":"log","time":"...","level":"...","side":"...","plugin":"...","message":"..."}` followed by one `summary` record.

## The trace

The trace is a ring buffer per plugin instance. It is on by default.

| Kind | Content |
|---|---|
| `event` | An event batch delivered to the plugin: count and event tags |
| `submission` | A submission the plugin returned: intent name, correlation, dedup key |
| `outcome` | An outcome: correlation and outcome tag |
| `host-call` | An import call: name, duration, result tag |
| `render` | A render: target, size, duration, node count |
| `trap` | A trap: code and export |
| `lifecycle` | Activate, deactivate, restart, disable |

- **[DEV-670]** The ring buffer MUST hold 1,024 records per instance. When full, the oldest record is dropped.
- **[DEV-671]** Tracing MUST be on for every instance. It MUST add no more than 5 percent to the time of an event batch.
- **[DEV-672]** Payloads MUST be redacted in a normal trace. A redacted payload prints as `<redacted N bytes>`. Tags, names, counts and durations are never redacted.
- **[DEV-673]** `--unredacted` MUST work only for a plugin installed with `dev`. It MUST be refused with `trace.unredacted-denied` for all others.
- **[DEV-674]** A trace record MUST carry a sequence number that increases by one per record per instance, and the fake or real time in milliseconds.
- **[DEV-675]** `ilium plugin trace --record <file>` MUST write the recorded inputs as a replay file in the format of [Test harness reference](/pages/reference-test-harness/).
- **[DEV-676]** The trace of an instance that restarted MUST keep the records of earlier instances, marked with the instance number.

A trace in text form:

<figure>
<pre class="term">
seq  time(ms)  kind        detail
 41  000120    event       batch 2: pane-created, node-renamed
 42  000121    submission  set-decoration corr=17
 43  000122    outcome     corr=17 ok
 44  000900    host-call   storage.put 0.2 ms ok
 45  001004    render      panel main 62x12 3.1 ms 8 nodes
 46  001510    trap        trap.deadline in on-events
 47  001511    lifecycle   restart 1 of 3, delay 1 s
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Output of ilium plugin trace example.notes --last 7.</figcaption>
</figure>

## The widget tree inspector

`ilium plugin inspect <id> --view <target>` prints the tree the plugin returned for a target, with keys and styles.

<figure>
<pre class="term">
╭ Widget tree: panel main ─────────────────────╮
│ stack  vertical                              │
│   text  "Open notes: 3"        style=title   │
│   list  rows=3 selected=1                    │
│     row  key=n1  "Inbox"                     │
│     row  key=n2  "Plans"       <- selected   │
│   button  key=add  "Add note"                │
│ nodes 8 of 2000   depth 3 of 16              │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The inspector view of panel main.</figcaption>
</figure>

- **[DEV-680]** The inspector MUST print the last tree the plugin returned for the target, not a new render. If the plugin never rendered the target, it MUST fail with `inspect.no-render`.
- **[DEV-681]** Each node MUST show its kind, its key when it has one, its main text and its style name. Selected rows MUST be marked.
- **[DEV-682]** The footer MUST show the node count against the node limit and the depth against the depth limit of [Errors and limits](/pages/reference-errors-and-limits/).
- **[DEV-683]** `--json` MUST print the tree as one `tree` record with nested `children`.
- **[DEV-684]** The inspector MUST read the tree without changing focus or selection.

## The profiler

`ilium plugin profile <id> --duration <seconds>` measures the time each export spent over the period.

| Field | Meaning |
|---|---|
| `export` | `on-events`, `render`, `activate`, `deactivate`, `handle-hook`, `frame` |
| `calls` | Number of calls in the period |
| `total_ms` | Sum of call time |
| `p50_ms`, `p95_ms`, `max_ms` | Percentiles of call time |
| `over_budget` | Calls over the deadline of that export |

- **[DEV-690]** `profile` MUST sample every call. It MUST NOT sample by chance.
- **[DEV-691]** `over_budget` MUST count calls over 20 ms for `on-events` and 10 ms for `render`, the limits in [Errors and limits](/pages/reference-errors-and-limits/).
- **[DEV-692]** A period with no calls MUST print a `timing` record with `calls` 0 for the export and no percentiles.
- **[DEV-693]** Profiling MUST NOT change the time of calls by more than 5 percent.

## The memory view

`ilium plugin inspect <id> --memory` prints the memory a plugin uses.

<figure>
<pre class="term">
╭ Memory: example.notes (server) ──────────────╮
│ linear memory   6.2 MiB of 32 MiB            │
│ peak            9.8 MiB                      │
│ storage         14 KiB of 1 MiB              │
│ instances       1   restarts 0               │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The memory view of one plugin side.</figcaption>
</figure>

- **[DEV-700]** The memory view MUST show linear memory now, the peak since the instance started, the limit, storage used against the 1 MiB quota, the instance count and the restart count.
- **[DEV-701]** A peak of more than 80 percent of the limit MUST add the line `warning: peak above 80 percent of memory_mib`.
- **[DEV-702]** Values MUST use binary units (KiB, MiB).

## Dev loop contract

`ilium plugin dev` installs the build as a T3 plugin and keeps it fresh.

- **[DEV-710]** A dev install MUST show the dev badge in Settings > Plugins and in the plugin list, so nobody mistakes it for a signed package.
- **[DEV-711]** A dev install MUST use its own data folder, separate from an installed release of the same id. Two installs of one id cannot exist: `dev` MUST refuse when the release is installed, with `dev.id-installed`, unless `--replace-installed` is given.
- **[DEV-712]** After a rebuild, `dev` MUST reload the plugin: deactivate with reason `upgrade`, load the new component, activate. Storage is kept.
- **[DEV-713]** A failed rebuild MUST leave the running version in place and print the build errors.

## Source maps

- **[DEV-715]** A TypeScript build MUST write the source map at `dist/<side>.wasm.map`. A package contains the map under `src/` only when `pack --include-src` is used. A trap trace shows file and line when the map is present.
- **[DEV-716]** A Rust build MUST keep the `name` section of the component in debug builds and strip it in release builds. `inspect --verify` MUST report which it found.

## Record catalogue for --json

Each tool prints JSONL with `--json`. `type` is the first key. A `summary` record and then a `result` record end every run.

| Command | Record `type` | Fields |
|---|---|---|
| `logs` | `log` | `time`, `level`, `side`, `plugin`, `message` |
| `trace` | `trace` | `seq`, `time_ms`, `kind`, `instance`, `detail` (object, redacted unless allowed) |
| `profile` | `timing` | `export`, `calls`, `total_ms`, `p50_ms`, `p95_ms`, `max_ms`, `over_budget` |
| `inspect --view` | `tree` | `target`, `nodes`, `depth`, `root` (nested `children`) |
| `inspect --memory` | `memory` | `side`, `linear_bytes`, `peak_bytes`, `limit_bytes`, `storage_bytes`, `instances`, `restarts` |

- **[DEV-720]** Field names in `--json` records MUST use `snake_case` and MUST match the table. A new field MAY be added. A field MUST NOT be removed or renamed within contract `ilium:plugin@0.1.0`.
- **[DEV-721]** Durations MUST be numbers in milliseconds with up to three decimals. Sizes MUST be integers in bytes.
- **[DEV-722]** Records of one run MUST appear in time order, except `summary` and `result`, which come last.

A `trace` record in JSON:

```json
{"type":"trace","seq":46,"time_ms":1510,"kind":"trap","instance":2,"detail":{"code":"trap.deadline","export":"on-events"}}
```

## Worked example: find why a plugin restarts

A plugin `example.notes` restarts every few minutes. The steps use only the tools on this page.

1. Read the log: `ilium plugin logs example.notes --level warn`. It shows `restarting in 1 s (failure 1 of 3 in 10 minutes)` and an `error` line `trap in on-events: deadline 20 ms`.
2. Read the trace: `ilium plugin trace example.notes --last 20 --kinds event,trap`. The trap follows a batch that holds 256 `node-renamed` events.
3. Profile: `ilium plugin profile example.notes --duration 30`. `on-events` shows `p95_ms` 14.8 and `max_ms` 31.2 with `over_budget` 2.
4. Inspect memory: `ilium plugin inspect example.notes --memory`. The peak is 9.8 MiB of 32 MiB, so memory is not the cause.
5. Reproduce: `ilium plugin trace example.notes --record rename-burst.jsonl`, then `ilium plugin test --replay rename-burst.jsonl`. The test traps the same way.
6. Fix the handler to coalesce renames, and run the replay again. It now exits 0.

- **[DEV-725]** A recorded trace replayed with `ilium plugin test --replay` MUST reproduce a deadline trap in component mode when the original call exceeded the deadline, as the recorded host call times are replayed with the fake clock.

## Problems and fixes

| Symptom | Likely cause | Check | Fix |
|---|---|---|---|
| No `debug` lines in the log | Threshold is `info` | `ilium plugin log-level <id>` | Run `log-level <id> debug` |
| `trace.unredacted-denied` | Not a dev install | `ilium plugin list` shows no `[dev]` | Install with `ilium plugin dev` |
| Trace shows fewer than 1,024 records | The instance is new | `seq` starts low | Wait for activity |
| Log folder missing | Plugin never logged | Look in `ilium plugin doctor` state path | Log one line on activation |
| `inspect.no-render` | View never rendered | Open the panel once | Open the target, then inspect |

- **[DEV-730]** `inspect --view` for a plugin that is disabled MUST fail with `inspect.no-render` and exit 1.
- **[DEV-731]** `profile` of a plugin that is disabled MUST fail with `plugin.not-running` and exit 1.

## Edge cases

- A plugin with several sides writes all its lines to one file. The `side` column tells them apart.
- Time moves back when the machine clock changes. The host MUST NOT reorder lines.
- Logs for a disabled plugin stay readable.
- A plugin that logs 200 lines per second at 80 bytes per line fills five files of 4 MiB in about 20 minutes. Old lines are then lost. Use the trace for short windows.

## See also

- [Debugging plugins](/pages/debugging/)
- [Test harness reference](/pages/reference-test-harness/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Plugin command-line reference](/pages/reference-plugin-cli/)

API checked against `ilium:plugin@0.1.0`.
