---
layout: page
title: Errors and limits
permalink: /pages/errors-and-limits/
---

## Errors and limits

A host call can fail in two ways. It can return a typed error that the plugin handles, or it can trap, which ends the plugin instance at once. A returned error is an ordinary result: `Result<T, PluginError>` in Rust, or a thrown `PluginError` in TypeScript. A trap is a plugin bug. The kernel restarts the instance under a fixed back-off, and the third failure inside 10 minutes disables the plugin.

## You will need

- The grants for the capabilities you use. A declined or revoked grant returns `not-permitted`. Calling an import the manifest never requested is a trap.
- Rust with `ilium-plugin-sdk`, or TypeScript with `@ilium/plugin-sdk`.
- [Events and intents](/pages/events-and-intents/) and [Lifecycle](/pages/lifecycle/).

## Returned errors

Every function that can fail returns `result<T, plugin-error>`. The record has a `code`, a `message` and an optional `retry-after-ms`.

```wit
// Excerpt of ilium:types/errors in reference-wit
enum error-code {
  not-permitted, not-found, invalid-argument, conflict, quota-exceeded,
  rate-limited, busy, timed-out, cancelled, unsupported, pane-not-ready,
  revoked, io, network, internal,
}
record plugin-error { code: error-code, message: string, retry-after-ms: option<u32> }
```

In Rust the fields are `code`, `message` and `retry_after_ms`. In TypeScript they are `code`, `message` and `retryAfterMs`, and codes are strings such as `'rate-limited'`.

| Code | Meaning | Retry |
|---|---|---|
| `not-permitted` | The capability is missing, declined or revoked, the path leaves its scope, or a policy forbids it. | No. Hide the feature or ask for the grant. |
| `not-found` | The node, pane, buffer, file, command, plugin or provider does not exist. | No. |
| `invalid-argument` | The call breaks a rule the plugin can check first: syntax, range, size or corrupt input. | No. Fix the call. |
| `conflict` | A stale revision, a manual title, a lock, or an existing file for `create-new`. | Yes, after reading the state again. |
| `quota-exceeded` | A stored or held quota is full, or the CPU budget is spent. | After freeing resources, or after `retry-after-ms`. |
| `rate-limited` | A rate rule throttles the call. `retry-after-ms` is always set. | Yes, after `retry-after-ms`. |
| `busy` | Transient capacity is full: pending requests, the job queue, or a device in use. | Yes, later. |
| `timed-out` | A deadline passed: a lock wait, HTTP, a process, a job, or a reply. | Yes. |
| `cancelled` | The plugin, the user or a shutdown cancelled the request. | Maybe. |
| `unsupported` | The host or platform cannot do it, or the call is in the wrong instance kind. | No. |
| `pane-not-ready` | A restored terminal pane has not started its process yet. | Yes, after `pane-created` or `tree-changed`. |
| `revoked` | The authorization epoch changed during the call. | No. The plugin is re-activated. |
| `io` | An operating system I/O error other than a missing file. | Maybe. |
| `network` | DNS, connect, TLS or transport failure. | Yes. |
| `internal` | A host bug, or a trap inside a job instance. | No. |

Every function that returns a result may return `not-permitted`, `revoked` and `internal`. A call that returns a request id checks its grant, scope, syntax and per-call caps at once, and returns those errors synchronously. Anything that needs the disk, the network, another process or the user is reported later as `failed(code)` in the completion.

A value over a per-call cap is refused with `invalid-argument` before anything is written. The host never truncates a value silently, except that log lines are cut and marked.

### Disk full and read-only data

A full disk is not a quota. When a write fails because the physical disk has no room, the call returns `io` with the message `no-space`. The host never reports this as `quota-exceeded`, which stays for the plugin's own limits, such as a bucket or the `disk_mib` limit [LIF-119]. Check the message to tell the two apart.

| Situation | What the call returns | What is left |
|---|---|---|
| `put` or `write-batch` on storage | `io` with `no-space` | A refused `put` keeps the previous value. A failed batch rolls back as a whole. |
| A file write under `data` or `cache` | `io` with `no-space`. An async call arrives as `failed(io)` in its completion. | The old file stays. |
| A plugin `log` line | No error. The host drops the line and keeps a count. | Nothing is written for that line. |
| Install or update (a host action, not a plugin call) | `install.no-space`, refused before any write | The old version stays installed and enabled. |

A `no-space` error is not a plugin failure. The plugin is not trapped, is not restarted and does not count toward the three-failure rule [LIF-173]. Plugins keep running while free space is low. Log the failure yourself at `error` so the user sees it in `ilium plugin logs <id>`, as the [storage page](/pages/api-storage/) example does.

The host reacts to falling free space in steps. The thresholds below are proposed values in the [offline and low disk reference](/pages/reference-offline-and-low-disk/) and may still change. Below 512 MiB the host evicts the plugin cache and the compile cache. Below 128 MiB it stops plugin log writes except `error` lines. Below 64 MiB it blocks installs, updates and migrations. The footer shows one warning with the free space. No step stops the running plugins.

When the user frees space, the host resumes installs and log writes without a restart, and a storage write that returned `no-space` succeeds on retry. Retry only after a delay or after space has been freed, never in a loop.

If the data folder is not writable, Ilium runs in read-only mode. Installed plugins keep running. Install, update, uninstall, enable, disable and storage writes fail with `io` and the message `read-only`, which is not a failure either. The footer shows "Plugin data is read-only." The host re-checks the folder every 30 seconds and leaves read-only mode without a restart once it is writable again.

## Rate limits and retries

A `rate-limited` error is not a plugin bug. Wait for `retry-after-ms`, then try the same call again. A refused submission reports the same error in its outcome:

```json
{
  "type": "request-completed",
  "request": { "submission": 23 },
  "outcome": {
    "refused": {
      "code": "rate-limited",
      "message": "Too many send-input calls for this pane",
      "retry-after-ms": 2000
    }
  }
}
```

A refused submission does not reserve its `dedup-key`, so a retry with the same key is correct. A submission that was accepted and later failed keeps its key reserved. A retry of that action needs a new key.

A dedup key is 1 to 128 characters from `A-Z`, `a-z`, `0-9`, `.`, `_`, `:`, `/` and `-`. Keys are remembered for 10 minutes. A submission with a reserved key completes as `duplicate(original)` and has no effect. `send-input` and `agent-action` must carry a key.

## Worked examples

### Rust: handle the typed errors of a file read

This server plugin reads `NOTES.md`. A missing file is normal, an oversized file is skipped with a log line, and any other error is logged.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::{fs, log::{self, Level}};
use ilium_plugin_sdk::exports::Activation;

async fn read_notes() -> Result<Vec<u8>, PluginError> {
    let path = FsPath { scope: "project".into(), path: "NOTES.md".into() };
    match fs::read_all(&path).await {
        Ok(bytes) => Ok(bytes),
        Err(error) if matches!(error.code, ErrorCode::NotFound) => Ok(Vec::new()),
        Err(error) if matches!(error.code, ErrorCode::QuotaExceeded) => {
            log::log(Level::Warn, "NOTES.md is over the read-all limit; skipped");
            Ok(Vec::new())
        }
        Err(error) => Err(error),
    }
}

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct NotesReader;

impl Lifecycle for NotesReader {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        ilium_plugin_sdk::task::spawn(async {
            match read_notes().await {
                Ok(bytes) => log::log(Level::Info, &format!("read {} bytes", bytes.len())),
                Err(error) => log::log(Level::Error, &format!("read failed: {}", error.message)),
            }
        });
        Ok(())
    }
}

impl Handler for NotesReader {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}
```

The read runs in `task::spawn`, so activation returns without waiting for the file.

### TypeScript: retry a rate-limited notice with the same key

When the command runs, this server sends a notice. If the host refuses it with `rate-limited`, the plugin waits `retryAfterMs` with a one-shot timer, then sends a notice again with the same dedup key. The plugin submits only notices, so every refused submission is one of them.

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import * as timers from '@ilium/plugin-sdk/host/timers';

const SAVED_NOTICE_KEY = 'example.notes:saved:notes-md';
let nextCorrelation = 1n;
// Timer id to the submission it will send. Timers are not saved across a restart.
const pendingByTimer = new Map<bigint, Submission>();

function savedNotice(): Submission {
  nextCorrelation += 1n;
  return {
    intent: {
      tag: 'notify',
      val: { title: 'Notes', body: 'Notes saved', urgency: 'low', sound: undefined },
    },
    correlation: nextCorrelation,
    dedupKey: SAVED_NOTICE_KEY,
  };
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const out: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === 'example.notes/save') {
        out.push(savedNotice());
      } else if (event.tag === 'request-completed' && event.val.outcome.tag === 'refused') {
        const error = event.val.outcome.val;
        if (error.code === 'rate-limited' && error.retryAfterMs !== undefined) {
          pendingByTimer.set(timers.set(error.retryAfterMs, undefined), savedNotice());
        }
      } else if (event.tag === 'timer-fired') {
        const retry = pendingByTimer.get(event.val);
        pendingByTimer.delete(event.val);
        if (retry !== undefined) {
          out.push(retry);
        }
      }
    }
    return out;
  },
});
```

The retry gets a new correlation number and keeps the dedup key. The refused notice did not reserve its key, so the retry is accepted.

## Traps

A trap ends the instance at once, and the kernel restarts it under the rules below. These cases trap:

- The plugin calls an import that its manifest never requested.
- One `on-events` call returns more than 256 submissions.
- A `ui-tree` has a child index out of range, a graph that is not a tree, or a duplicate `key`.
- A call passes its deadline, or the instance exceeds its memory or table limit.
- The guest panics, or a TypeScript export with no result (`onEvents`, `render`, `deactivate`) throws.

A declined capability does not trap. The call returns `not-permitted`. A trap inside a job fails the job with `internal`, and the plugin keeps running.

<div class="warn">
<p>Do not use a trap as normal control flow. Each trap is a failure, and the third one inside 10 minutes disables the plugin until the user turns it on again.</p>
</div>

## Logged refusals and traps

Every `not-permitted` result and every trap is written to the plugin log, with the plugin id, the interface and the function. Each entry is one line: the time as `YYYY-MM-DD HH:mm:ss`, the level padded to five characters, the plugin id and the message. Refusals are logged at `warn`, traps at `error` and restarts at `info`. Run `ilium plugin logs <id>`, or add `--tail <n>` for the last `n` lines.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs example.notes --tail 4
<span class="t-dim">2026-10-11 10:14:02</span> <span class="t-warn">warn </span> example.notes refused fs.read: not-permitted
<span class="t-dim">2026-10-11 10:14:03</span> <span class="t-err">error</span> example.notes trap in ui.set-widget-state: index 7
<span class="t-dim">2026-10-11 10:14:03</span> <span class="t-ok">info </span> example.notes restarting in 1 s (failure 1 of 3)
<span class="t-dim">2026-10-11 10:14:09</span> <span class="t-err">error</span> example.notes trap in ui.set-widget-state: index 7
</pre>
<figcaption>Illustrative, not generated. The plugin log: one refusal, two traps and the restart between them.</figcaption>
</figure>

## Restart and disable

A trap or a deadline breach drops the instance, cancels its pending requests and restarts it. The count is kept per plugin on each side. A failing view counts for the plugin, not for the view.

- Back-off: the first failure in the window restarts the plugin after 1 s and the second after 5 s. The third failure in the window disables it; no 30 s step exists. A failure older than 10 minutes no longer counts, so the next failure restarts after 1 s again [API-040].
- Each failure counts for 10 minutes after it happens. The window slides, and a successful run does not reset it.
- Disable: the third failure inside the window disables the plugin on that side. A manual re-enable resets the count to zero.

A revoked re-activation, a disable, an upgrade or a shutdown never counts.

<figure>
<svg viewBox="0 0 400 240" role="img" aria-label="Failure paths and restart back-off">
  <title>Failure paths and restart back-off</title>
  <defs>
    <marker id="errors-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="130" y="10" width="140" height="30" rx="4" fill="none" stroke="currentColor"/>
  <text x="200" y="29" font-size="12" text-anchor="middle" fill="currentColor">Host call</text>
  <rect x="10" y="80" width="160" height="30" rx="4" fill="none" stroke="currentColor"/>
  <text x="90" y="99" font-size="12" text-anchor="middle" fill="currentColor">Returned error</text>
  <rect x="230" y="80" width="160" height="30" rx="4" fill="none" stroke="currentColor"/>
  <text x="310" y="99" font-size="12" text-anchor="middle" fill="currentColor">Trap or deadline breach</text>
  <rect x="10" y="140" width="160" height="30" rx="4" fill="none" stroke="currentColor"/>
  <text x="90" y="159" font-size="12" text-anchor="middle" fill="currentColor">Plugin handles code</text>
  <rect x="230" y="140" width="160" height="30" rx="4" fill="none" stroke="currentColor"/>
  <text x="310" y="159" font-size="12" text-anchor="middle" fill="currentColor">Restart: 1 s, 5 s, disable</text>
  <rect x="190" y="200" width="200" height="30" rx="4" fill="none" stroke="currentColor"/>
  <text x="290" y="219" font-size="12" text-anchor="middle" fill="currentColor">Third failure: disabled</text>
  <line x1="180" y1="40" x2="90" y2="80" stroke="currentColor" marker-end="url(#errors-arrow)"/>
  <line x1="220" y1="40" x2="310" y2="80" stroke="currentColor" marker-end="url(#errors-arrow)"/>
  <line x1="90" y1="110" x2="90" y2="140" stroke="currentColor" marker-end="url(#errors-arrow)"/>
  <line x1="310" y1="110" x2="310" y2="140" stroke="currentColor" marker-end="url(#errors-arrow)"/>
  <line x1="310" y1="170" x2="310" y2="200" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#errors-arrow)"/>
</svg>
<figcaption>A call either returns an error the plugin handles, or it traps. Failures one and two restart the plugin. A third failure within 10 minutes disables it.</figcaption>
</figure>

When a plugin is disabled, the tree footer shows a notice that names the export and the cause. The view is named only when the failing export was `views.render`.

<figure>
<pre class="term"><span class="t-err">Plugin example.notes stopped after 3 failures in 10 minutes: handler trap.</span>
<span class="t-dim">Turn it on again in Settings &gt; Plugins.</span>
</pre>
<figcaption>Illustrative, not generated. The tree footer after the third failure.</figcaption>
</figure>

Timers and watches end at a restart, so the plugin registers them again in `activate`.

## Resource limits

Each limit is a manifest key with a default and two ceilings. The T1 ceiling applies to bundled first-party plugins, and the T2/T3 ceiling to all others. A manifest may ask for any value up to the ceiling. A value above the ceiling is clamped and logged, never refused. The user sets each limit in `config.toml` under `[plugins.limits]`, or for one plugin under `[plugins."<id>".limits]`, with the same key. The effective value is the smallest of the tier ceiling and the first of the per-plugin user value, the all-plugins user value, the manifest value and the default; 0 or a missing value means not set. The user can raise a value up to the ceiling and can lower it. The default is the same for every tier; a T1 plugin may request up to the T1 ceiling in `limits`.

| Limit | Default | Ceiling T1 | Ceiling T2/T3 | Effect when reached |
|---|---|---|---|---|
| Linear memory per instance | 32 MiB | 256 MiB | 128 MiB | trap |
| Table elements | 10,000 | 50,000 | 20,000 | trap |
| `on-events` deadline | 20 ms | 50 ms | 20 ms | trap |
| Command handling inside `on-events` | 50 ms | 100 ms | 50 ms | trap |
| `views.render` deadline | 10 ms | 20 ms | 10 ms | trap |
| `annotate` deadline | 10 ms | 20 ms | 10 ms | late result dropped; third miss in a row traps |
| Scene `render` deadline | the frame budget | 50 ms | 50 ms | frame skipped; 30 misses in a row traps |
| `activate` deadline (may yield) | 2,000 ms | 5,000 ms | 2,000 ms | trap |
| `deactivate` deadline | 500 ms | 1,000 ms | 500 ms | instance dropped without restore bytes |
| Job CPU deadline | 2,000 ms | 60,000 ms | 10,000 ms | `failed(timed-out)` |
| Job wall-clock deadline | 20,000 ms | 600,000 ms | 100,000 ms | `failed(timed-out)` |
| Job memory | 64 MiB | 512 MiB | 256 MiB | `failed(quota-exceeded)` |
| Job input or output, each | 16 MiB | 64 MiB | 16 MiB | `invalid-argument` |
| Concurrent jobs per plugin | 2 | 8 | 2 | queued |
| Job queue | 16 | 64 | 16 | `busy` |
| Event queue per plugin | 1,024 | 4,096 | 1,024 | stuck plugin: restart |
| Pending requests | 64 | 256 | 64 | `busy` |
| Timers | 64 | 256 | 64 | `quota-exceeded` |
| Output filters | 32 | 128 | 32 | `quota-exceeded` |
| Watches (a recursive watch counts once) | 64 | 256 | 64 | `quota-exceeded` |
| Key grabs | 8 | 32 | 8 | `quota-exceeded` |
| `ui-tree` nodes | 4,096 | 16,384 | 4,096 | trap |
| `ui-tree` size | 1 MiB | 4 MiB | 1 MiB | trap |
| Decorations | 4,096 | 16,384 | 4,096 | `refused(quota-exceeded)` |
| Node metadata per node | 64 KiB | 256 KiB | 64 KiB | `quota-exceeded` |
| Storage per bucket | 16 MiB | 256 MiB | 16 MiB | `quota-exceeded` |
| Disk for `data` plus `cache` | 256 MiB | 4 GiB | 256 MiB | `failed(quota-exceeded)` |
| `fs.read-all` per request | 64 MiB | 512 MiB | 64 MiB | `failed(quota-exceeded)` |
| `fs.write` per call | 16 MiB | 64 MiB | 16 MiB | `invalid-argument` |
| HTTP response body | 8 MiB | 64 MiB | 8 MiB | `failed(quota-exceeded)` |
| Process output (stdout plus stderr) | 4 MiB | 32 MiB | 4 MiB | `failed(quota-exceeded)` |
| Log lines | 200 per second | 1,000 per second | 200 per second | dropped and counted |
| Open videos | 2 | 8 | 2 | `quota-exceeded` |
| Audio captures | 2 | 4 | 2 | `quota-exceeded` |

CPU time and bytes also draw on the plugin's execution quota group. When it is spent, submissions get `quota-exceeded` with `retry-after-ms`.

Per-call caps are fixed:

| Cap | Value | Error |
|---|---|---|
| Submissions per `on-events` return | 256 | trap |
| Events per `on-events` batch | 256 | the excess is split into later batches |
| Synchronous `fs.read` | 1 MiB | `invalid-argument` |
| Codec input | 16 MiB | `invalid-argument` |
| Storage key and value | 256 bytes and 1 MiB | `invalid-argument` |
| `send-input` text or raw bytes per intent | 64 KiB | `refused(invalid-argument)` |
| `fs.scan` entries | 65,536 | results truncated, `truncated: true` |
| Journal payload | 4 KiB | `invalid-argument` |
| Message payload | 1 MiB | `refused(invalid-argument)` |
| Topic payload | 64 KiB | `invalid-argument` |
| Command `arguments-json` | 64 KiB | `invalid-argument` |

Larger codec input belongs in a background job. See [Background jobs](/pages/background-jobs/).

Rate rules return `rate-limited` with `retry-after-ms`, except where noted. `topics.publish` allows 100 per second per plugin. `notify` allows 6 per minute, and `play-sound` 4 per second, both refused with `refused(rate-limited)`. `send-input` allows 20 per second per pane. Log lines and output-filter matches over their rate are dropped and counted.

Events that drop under load (`output-matched`, `topic-published`, `audio-frames`) are counted. The next batch starts with one `events-dropped` event that lists each source and count.

There are five session keys in `[plugins.limits]`, which apply to all plugins in one process: `session_memory_mib`, `session_helpers`, `session_processes`, `session_http_streams` and `session_decorations`. The user may only lower them. Each instance reserves its configured linear memory limit against `session_memory_mib` at activation (a sandbox helper reserves its `memory_mib` plus 32 MiB). When the pool is full, a new instance is refused and the refusal is logged. It does not count as a failure. At most 16 children per session (`session_processes`) are shared by `process.run` (`busy`) and `process.spawn` (`quota-exceeded`). Fixed session caps are 4,096 pending requests (`busy`) and 32 key grabs (`quota-exceeded`).

Replies to slow calls wait 30 s for commands and services, 120 s for inference, and 30 s for HTTP API replies. These and the other fixed reply deadlines are in the [reply deadlines table](/pages/reference-errors-and-limits/#reply-deadlines).

## Settings limits

Settings rows have their own limits. A value that is stored but no longer valid is ignored, and the default applies. The limits fall into two groups, by the moment they are checked.

**Checked at install (`manifest.settings-limit` refusal; the message names the item and the limit).**

| Item | Limit |
|---|---|
| Tabs per plugin | 2 |
| Sections per plugin tab | 8 |
| Sections per plugin, per built-in tab | 2 |
| Plugin sections in one built-in tab, all plugins | 6; the extra ones are not drawn |
| Plugin tabs in the rail, all plugins | 16 |
| Rows per section, children per group | 24 (a `group` counts as one row plus its children), and 12 children in one group |
| Value keys per plugin | 128 |
| Choice options | 64 |
| List items | 64, each object at most 8 fields |

**Checked on save or at run time (`invalid-argument`, the value is not stored; a `settings-validate` timeout refuses the save).**

| Item | Limit |
|---|---|
| Text value | 4,096 characters; text area 65,536 |
| All values of one plugin, serialized | 256 KiB |
| `settings-validate` answer | 100 ms; a timeout refuses the save |

The full list of codes and warnings is in [Reference: errors and limits](/pages/reference-errors-and-limits/). A plugin's settings are explained in [Plugin settings extension](/pages/settings-extension/).

## Additional codes, limits and deadlines

The extended surface adds no runtime error code. The 15 codes stay, and the newer functions reuse them. The complete lists are in [Errors and limits reference](/pages/reference-errors-and-limits/) and the [install error catalogue](/pages/reference-manifest-schema/#error-catalogue).

Four ideas explain most of the new rules.

- **Rate limits are per slot or per plugin.** A status segment accepts 10 updates per second, a toast 3 per 10 seconds, a geocode request 1 per second. An update over the rate is refused with `rate-limited`, and the previous content stays.
- **Quotas are counts of live things.** Streams, children, durable timers and vector entries count what is open or stored. Close or delete one, then retry.
- **Deadlines fall back.** A slow `menu-items` or `tokens` call does not fail the user action. The menu opens with its static entries, and the lines stay plain.
- **The first-party tier is not a way around limits.** `limits_tier: "first-party"` raises the settings caps for bundled plugins. A third-party package that sets it is refused at install.

**Rust**

```rust
// A toast is refused over 3 per 10 s. Coalesce instead of retrying in a loop.
fn show_progress(&mut self, text: &str) -> Vec<Submission> {
    self.latest = text.to_string();
    if self.toast_in_flight { return vec![]; }   // the next event shows self.latest
    self.toast_in_flight = true;
    vec![Submission::new(Intent::Toast(ToastSpec {
        id: Some("progress".into()), tone: Tone::Info, title: text.into(),
        body: None, actions: vec![], duration_ms: None,
    }))]
}
```

**TypeScript**

```ts
// A toast is refused over 3 per 10 s. Coalesce instead of retrying in a loop.
function showProgress(text: string) {
  latest = text;
  if (toastInFlight) return []; // the next event shows `latest`
  toastInFlight = true;
  return [{ intent: { tag: 'toast', val: { id: 'progress', tone: 'info', title: text,
    body: undefined, actions: [], durationMs: undefined } },
    correlation: nextCorrelation++, dedupKey: undefined }];
}
```

<pre class="term">╭  ≡ ● · Settings─────────────────────────────────╮
│ Palette ………………………… ← Sunset … → +               │
│ ! Stars must be 10 to 2000. Value not saved.    │
│ ✕ Too many streams: close one first.            │
╰──────────────────────────────────────────────── ╯
</pre>

A refused value and a refused call as the settings page shows them. Illustrative, not generated.

## Extension point codes and limits

Extension points add install codes and run codes, and no new `error-code` case. Each run code is carried by one of the 15 codes plus a message prefix; the mapping is in [Extension points](/pages/api-extension-points/#edge-cases-and-error-codes). The tables are in [Error codes and limits](/pages/reference-errors-and-limits/#extension-point-codes). In short, a point owner may declare 16 points and a contributor may add 64 extensions. A hook handler has 5 to 50 ms (default 20 ms), and a whole chain has 100 ms. A payload or reply is at most 64 KiB, and a collect entry is at most 16 KiB. Hooks fail open: a timeout, trap or error counts as a pass, and `extension.vetoed` appears only when a veto handler refused on purpose.

## Install codes and budgets for authors

Errors in this page are returned to a running plugin. A package that fails to install gets a `manifest.` or `package.` code instead, and no plugin runs. The complete list of those codes is the install code index in [Error codes and limits](/pages/reference-errors-and-limits/). The limits an author should keep a plugin well inside are collected in the budget table of [Performance](/pages/performance/). An author MUST treat a limit as a trap and not as a warning. [AUT-111]

## Edge cases

- Audio rates from 8,000 to 192,000 Hz are accepted. More than two channels is `unsupported`.
- A malformed submission, or a batch whose grant is missing, refuses the batch. The offending submission completes as `refused`, and every other submission of that batch completes as `refused(cancelled)` with the message `batch refused: submission <index>`. A batch of more than 256 submissions is a trap, not a refusal.

## Rules

- Handle `rate-limited`, `busy` and `pane-not-ready` by waiting and then retrying. Never loop without a delay.
- Fix the cause of every trap. The third trap inside 10 minutes disables the plugin.

## See also

- [Debugging](/pages/debugging/)
- [Lifecycle](/pages/lifecycle/)
- [Plugin settings extension](/pages/settings-extension/)
- [Requests and outcomes](/pages/requests-and-outcomes/)
- [Background jobs](/pages/background-jobs/)
- [Reference: errors and limits](/pages/reference-errors-and-limits/)

API version checked: `ilium:plugin@0.1.0`.

### Security

Security codes and limits are listed in [Reference: errors and limits](/pages/reference-errors-and-limits/) and defined in [Security and policy requirements](/pages/reference-requirements-security-policy/).
