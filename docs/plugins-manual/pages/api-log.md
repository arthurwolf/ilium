---
layout: page
title: log
permalink: /pages/api-log/
---

## log

The `log` import writes one line to the plugin log at a level you choose. Each plugin has its own log. The user reads it with `ilium plugin logs <id>`. The host writes its own records into the same log, such as refused calls, trapped calls and failed activations, so one file explains what happened to the plugin.

Use `log` for diagnostics that a user may need later: a save that failed, a setting that the plugin ignored, or a milestone such as activation. Do not log every frame of a render or every key press. Lines beyond the rate limit are dropped (see [Rate limit](#rate-limit)).

<div class="note">
<p>No import reads the log back. A plugin cannot see what it wrote, or what the host wrote, from its own code.</p>
</div>

<div class="warn">
<p>The log is plain text that users can read and copy into bug reports. Never write tokens, passwords or the content of private notes into it.</p>
</div>

### What you need

- **Grant:** none. The import is always granted on the server, client and scene sides, and in job instances. It never appears in the `capabilities` list of the manifest.
- **Language:** Rust, with the crate `ilium-plugin-sdk`, or TypeScript, with the package `@ilium/plugin-sdk`. The import paths are `ilium_plugin_sdk::host::log` in Rust and `@ilium/plugin-sdk/host/log` in TypeScript.
- **Prerequisites:** a plugin built with `ilium plugin build`, as described in [Getting started with a first plugin](/pages/getting-started-first-plugin/). Nothing else is needed.

### Interface

```wit
// Excerpt of ilium:host/log in reference-wit
enum level { trace, debug, info, warn, error }
log: func(level: level, message: string);
```

The WIT names are kebab-case. The Rust and TypeScript names follow these rules (see the [Rust SDK](/pages/rust-sdk/) and the [TypeScript SDK](/pages/typescript-sdk/)): snake_case functions and UpperCamelCase variants in Rust, camelCase functions and kebab-case string tags in TypeScript.

| Function | Rust | TypeScript | Grant | Sides | Sync or async | Returns |
|---|---|---|---|---|---|---|
| `log` | `log::log(level: Level, message: &str)` | `log(level: Level, message: string): void` | always | S C A | sync | nothing |

The levels are:

| Level | Rust | TypeScript | Use it for |
|---|---|---|---|
| `trace` | `Level::Trace` | `'trace'` | Every step of a sequence, when you trace a bug |
| `debug` | `Level::Debug` | `'debug'` | Values that help you find a fault |
| `info` | `Level::Info` | `'info'` | Milestones: activation, a finished save |
| `warn` | `Level::Warn` | `'warn'` | Something was declined or ignored, and the plugin went on |
| `error` | `Level::Error` | `'error'` | An operation failed, and the user may notice it |

The message is a Rust `&str` or a TypeScript `string`. An empty message writes an empty line with the level and plugin id.

### Limits

| Limit | Default | Manifest key | Ceiling | Over the limit |
|---|---|---|---|---|
| Message size | 16 KiB (16,384 bytes of UTF-8) | none, fixed | none | The line is cut and marked |
| Lines per second, per plugin side instance | 200 | `limits.log_lines_per_second` | 1,000 for tier 1 bundled plugins, 200 for tier 2 and 3 | Dropped and counted |

A manifest value above the ceiling is clamped to it, and the host logs the clamp. The user can set the same key in `config.toml` under `[plugins.limits]`, for every plugin or for the whole session.

### Rate limit

Each side instance of a plugin has its own budget of lines per second. Lines within the budget are appended to the log in order. Lines over the budget are dropped and counted. Once per second in which lines were dropped, the host writes one summary line at level `warn` that gives the number of dropped lines. The summary line does not use the budget, so it always appears.

<figure>
<svg viewBox="0 0 420 180" role="img" aria-label="Log line path through the rate limiter to the plugin log">
<title>Log line path</title>
<desc>The plugin calls log with a level and a message. The host rate limiter either appends the line to the plugin log or counts it as dropped.</desc>
<defs>
<marker id="log-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="10" y="8" width="220" height="32" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
<text x="120" y="29" font-size="12" text-anchor="middle" fill="currentColor">Plugin: log(level, message)</text>
<line x1="120" y1="40" x2="120" y2="70" stroke="currentColor" marker-end="url(#log-arrow)"/>
<text x="128" y="58" font-size="11" fill="currentColor">message</text>
<rect x="10" y="72" width="220" height="32" rx="6" fill="none" stroke="currentColor"/>
<text x="120" y="93" font-size="12" text-anchor="middle" fill="currentColor">Rate limiter (host)</text>
<line x1="230" y1="88" x2="262" y2="88" stroke="currentColor" marker-end="url(#log-arrow)"/>
<rect x="262" y="72" width="148" height="32" rx="6" fill="none" stroke="currentColor"/>
<text x="336" y="93" font-size="12" text-anchor="middle" fill="currentColor">over the rate: counted</text>
<line x1="120" y1="104" x2="120" y2="134" stroke="currentColor" marker-end="url(#log-arrow)"/>
<text x="128" y="122" font-size="11" fill="currentColor">accepted</text>
<rect x="10" y="134" width="220" height="32" rx="6" fill="none" stroke="currentColor"/>
<text x="120" y="155" font-size="12" text-anchor="middle" fill="currentColor">Plugin log (ilium plugin logs)</text>
</svg>
<figcaption>A line within the budget reaches the log. A line over the budget is only counted, and the count appears once per second.</figcaption>
</figure>

### Message size

A message that makes its line longer than 16 KiB is cut at the last whole character that fits. The marker `…[truncated]` is appended, and the line, marker included, is at most 16,384 bytes. The host never cuts a line silently. The marker is always there, so the user can see that text is missing. If you need a longer record, write it in pieces with several calls.

### Newlines

A message can contain line breaks, but the log stores one entry per line. The host writes each line break in the message as the two characters `\n`. A message of three lines therefore becomes one log entry with `\n` in it.

### Standard output and standard error

A WASI component writes standard output and standard error to the plugin log through the `wasi:cli` imports. Standard output is written at level `info`, and standard error at level `error`. These writes count against the same rate budget and the same size limit as `log`. Use `log` when you want a level of your choice or a message that stands out in the log.

### Examples

#### Example 1: one line at each level

Grants: none. The import is always granted.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::log::{self, Level};

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Notes;

impl Lifecycle for Notes {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        log::log(Level::Info, &format!("activated as {}", context.side));
        if !context.granted.iter().any(|grant| grant == "tree.read") {
            log::log(Level::Warn, "tree.read declined; tree badges hidden");
        }
        Ok(())
    }
}

impl Handler for Notes {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        log::log(Level::Trace, &format!("{} events received", batch.len()));
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate(context) {
    log('info', `activated as ${context.side}`);
    if (!context.granted.includes('tree.read')) {
      log('warn', 'tree.read declined; tree badges hidden');
    }
  },
  onEvents(batch: Event[]): Submission[] {
    log('trace', `${batch.length} events received`);
    return [];
  },
});
```

`context.granted` lists the capabilities that were actually granted, so the plugin can detect a declined grant at activation.

#### Example 2: report a failed operation

Grants: none. The function turns a failed call into one `error` line that includes the error code.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::log::{self, Level};

fn report_save_failure(error: &PluginError) {
    log::log(
        Level::Error,
        &format!("save failed [{:?}]: {}", error.code, error.message),
    );
}

fn after_save(path: &str, result: Result<(), PluginError>) {
    match result {
        Ok(()) => log::log(Level::Info, &format!("saved {path}")),
        Err(error) => report_save_failure(&error),
    }
}
```

**TypeScript**

```ts
import { PluginError } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';

export function reportSaveFailure(error: unknown): void {
  if (error instanceof PluginError) {
    log('error', `save failed [${error.code}]: ${error.message}`);
    return;
  }
  log('error', `save failed: ${String(error)}`);
}
```

Write the code in the error log, because the user sees only the log. The code tells the user whether the problem is a full quota, a missing file or a declined grant.

### What the user sees

Each line of the log has five columns: the local timestamp as `YYYY-MM-DD HH:mm:ss`, the level padded to five characters, the side padded to six characters, the plugin id and the message. The command `ilium plugin logs <id>` prints the whole log. `--tail <n>` prints only the last `n` lines.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs example.notes --tail 5
<span class="t-dim">2026-10-11 09:14:02</span> <span class="t-ok">info </span> example.notes activated as server
<span class="t-dim">2026-10-11 09:14:02</span> <span class="t-warn">warn </span> example.notes tree.read declined; tree badges hidden
<span class="t-dim">2026-10-11 09:14:09</span> <span class="t-err">error</span> example.notes save failed [quota-exceeded]: disk full
<span class="t-dim">2026-10-11 09:14:10</span> <span class="t-warn">warn </span> example.notes 312 lines dropped in the last second
<span class="t-dim">2026-10-11 09:14:11</span> <span class="t-dim">trace</span> example.notes 3 events received
<span class="t-prompt">$</span> <span class="t-dim">5 lines shown</span>
</pre>
<figcaption>Illustrative, not generated. The log of one plugin in an 80-column terminal, dark theme. The summary line from the rate limit has level warn.</figcaption>
</figure>

The host writes its own lines into the same log. A refused call names the capability and the function, for example `http:example.org` and `http.fetch`, and a plugin whose activation fails gets an `error` line with the code and message.

### Errors

`log` returns no result and cannot fail, so it has no error code. A plugin that logs too fast loses lines, never the call. A message over 16 KiB is cut and marked. The `wasi:cli` writes are cut at 16 KiB too, but they are marked `[cut]` (see [Host runtime](/pages/reference-host-runtime/)).

Two host-side conditions write to the log instead of returning an error to the plugin:

- An `activate` call that returns an error is logged at level `error`, with the error code and message, and counts as a failure for the restart rule.
- A trap, such as a call to an import that the manifest never requested, is logged with the plugin id, the interface and the function.

### Used by

The host and the first-party capabilities write these lines through the same path:

- [clipboard](/pages/capability-clipboard/): each successful read, at level `info`, with the length only.
- [fs write](/pages/capability-fs-write/): each write, remove, rename and create-dir outside `data` and `cache`, at level `info`.
- [http](/pages/capability-http/): each refused request to a host without a grant.
- [process run](/pages/capability-process-run/): each run, at level `info`, with the program and its arguments.

### Log, journal and toast: which one

Two more records sit beside the log. They answer different questions, so a plugin often uses more than one.

| Record | Written by | Read by | Survives restart | Use it for |
|---|---|---|---|---|
| Plugin log (`log`) | the plugin and the host | the user, with `ilium plugin logs` | yes, as a file | diagnostics for a human |
| Pane journal (`journal.log-event`) | the plugin and the host | plugins, with `journal.read` | yes, in the snapshot | facts a plugin must read back, such as an input it sent |
| Toast (`toast` intent) | the plugin | the user, for seconds | no | short news while the user is in the window |

See [durable timers and journal](/pages/api-durable-timers/) and [toast](/pages/api-toast/). The first-party tier (`limits_tier: "first-party"`, bundled `ilium.*` plugins only) does not raise `log_lines_per_second`; the ceiling of 1,000 lines per second already belongs to tier 1.

**Rust**

```rust
use ilium_plugin_sdk::host::{journal, log::{self, Level}};

fn record_send(pane: NodeId, key: &str) {
    log::log(Level::Info, &format!("sent nightly prompt to pane {pane}"));
    let _ = journal::log_event(Some(pane), "note", &format!("{{\"key\":\"{key}\"}}"));
}
```

**TypeScript**

```ts
import { log } from '@ilium/plugin-sdk/host/log';
import { logEvent } from '@ilium/plugin-sdk/host/journal';

export function recordSend(pane: bigint, key: string): void {
  log('info', `sent nightly prompt to pane ${pane}`);
  logEvent(pane, 'note', JSON.stringify({ key }));
}
```

### See also

- [clock and timers](/pages/api-clock-and-timers/) for the time sources that log lines often carry.
- [Plugin exports](/pages/api-exports/) for the lifecycle calls that usually log.
- [Events](/pages/api-events/) for the events that a handler logs when it receives them.
- [Debugging](/pages/debugging/) for trap backtraces, which show source positions in builds made with `ilium plugin build --debug`.
- [Errors and limits](/pages/reference-errors-and-limits/) for the full table of limits.
- [Testing](/pages/testing/) for reading log lines in a test with the fake host.

### Security

Log lines are redacted of secrets ([SEC-243]). See [Secrets](/pages/reference-secrets/).
