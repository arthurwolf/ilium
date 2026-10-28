# Process control

## Process control

The `process` import runs a helper program, such as `git`, and returns its exit status and output to your plugin. The helper never inherits the host environment. It runs at low priority, with a small environment, a hard timeout and a cap on its output. Your plugin receives only the exit status, the signal name if one ended the process, and the two output streams.

A second capability lives in the `signal-pane` intent. It sends an interrupt, terminate, kill or hangup signal to the process running inside a terminal pane. Because it can stop work that a user started, it is the high-risk half of this page.

### What you need

- **Grant `process.run:<program>`** for each program, for example `process.run:git`. The argument is a program name, resolved through `PATH`, or an absolute path, which runs exactly that file. Wildcards are refused with `manifest.grant-wildcard`. `process.run:sh` is valid, but only bundled plugins use it, such as the progress monitor.
- **Grant `fs.read`** for the scope that holds the `cwd` you pass. No separate grant exists per folder.
- **Grant `panes.signal`** for `signal-pane`. Also hold `panes.read` to receive `pane-focused` events.
- **Risk.** `process.run` and `panes.signal` are both high risk (H). The user checks each one in the review dialog. A bundled T1 plugin is asked once, at first activation.
- **Language.** Rust crate `ilium-plugin-sdk`, module `ilium_plugin_sdk::host::process`, or TypeScript `@ilium/plugin-sdk/host/process`. The intent uses `Intent::SignalPane` in Rust and `signal-pane` in TypeScript.
- **Side.** Server and client. Both sides use the same grants.

### The surface

| Name (Rust / TypeScript) | WIT form | Grant | Meaning |
|---|---|---|---|
| `process::run` / `run` | `run(spec) -> result<request-id, plugin-error>`, wrapped as async | `process.run:<program>` | Starts the program and resolves when it exits. Returns `ProcessExit`, or throws `PluginError`. A non-zero exit code is not an error. |
| `process::run_request` / `runRequest` | the raw `run` | `process.run:<program>` | Returns a `RequestId`. The exit arrives later as a `request-completed` event. |
| `process::take_exit` / `takeExit` | `take-exit(request) -> option<process-exit>` | `process.run:<program>` | Returns the exit once, for a raw request id. Async code does not need it. |
| `signal-pane` intent | `signal-pane(signal-pane-spec)` | `panes.signal` | Sends a signal to a pane's process. The outcome is `ok` or a refusal. |

The spec and result records:

| Record and field | Rust type | TypeScript type | Meaning |
|---|---|---|---|
| `process-spec.program` | `String` | `string` | Program name or absolute path. It must match a grant. |
| `process-spec.arguments` | `Vec<String>` | `string[]` | Arguments after the program. At most 1,024. |
| `process-spec.cwd` | `Option<FsPath>` | `FsPath \| undefined` | Working folder. It must lie in a scope the plugin can read. |
| `process-spec.stdin` | `Option<Vec<u8>>` | `Uint8Array \| undefined` | Standard input, at most 4 MiB. |
| `process-spec.environment` | `Vec<(String, String)>` | `[string, string][]` | Extra variables, at most 64 pairs. |
| `process-spec.timeout_millis` | `u32` | `number` | Required. From 1 to 600,000 ms. |
| `process-exit.status` | `Option<i32>` | `number \| undefined` | Exit code. `None` when a signal ended the process. |
| `process-exit.signal` | `Option<String>` | `string \| undefined` | Signal name, for example `SIGKILL`, when a signal ended the process. |
| `process-exit.stdout` | `Vec<u8>` | `Uint8Array` | Standard output. |
| `process-exit.stderr` | `Vec<u8>` | `Uint8Array` | Standard error, kept apart from stdout. |
| `signal-pane-spec.pane` | `NodeId` | `bigint` | The pane whose process receives the signal. |
| `signal-pane-spec.signal` | `Signal` | `'interrupt' \| 'terminate' \| 'kill' \| 'hangup'` | The signal to send. |

The `signal` values map to POSIX signals as follows:

| Value | Signal | Can the process catch it |
|---|---|---|
| `interrupt` | `SIGINT` | yes |
| `terminate` | `SIGTERM` | yes |
| `kill` | `SIGKILL` | no |
| `hangup` | `SIGHUP` | yes |

The signal goes to the child process that the pane started. Use `kill` only when `terminate` has failed to end the process.

### Limits

| Limit | Value | Breach |
|---|---|---|
| `timeout_millis` | 1 to 600,000 ms, required | `invalid-argument` |
| Standard input | 4 MiB | `invalid-argument` |
| Arguments and environment pairs | 1,024 and 64 | `invalid-argument` |
| Output, stdout and stderr together | 4 MiB by default, 32 MiB at the T1 ceiling (`process_output_mib`) | process killed, `failed(quota-exceeded)` |
| Concurrent children, per session (`session_processes`) | 16 per session, shared by `process.run` (`busy`) and `process.spawn` (`quota-exceeded`) | `busy` or `quota-exceeded` |
| Pending requests, per plugin | 64 by default (`pending_requests`) | `busy` |

### How the helper runs

1. **Grant check.** A `run` call that names no program of the plugin's grants is refused at once with `not-permitted` (a missing grant is always `not-permitted`). A `cwd` outside a readable scope is `not-permitted`.
2. **Program lookup.** A bare name is resolved through `PATH`. An absolute path runs exactly that file.
3. **Environment.** The helper starts with only `PATH`, `HOME` and `LANG`. Each entry in `environment` replaces the value of the same name, except that a key refused by [SEC-033] (`LD_*`, `DYLD_*`, `PATH`, `IFS`, `BASH_ENV` and the others listed there) makes the whole call fail with `invalid-argument` before the program starts.
4. **Priority and quota.** The helper runs at low priority, and its CPU time is charged to the plugin's quota.
5. **Confinement.** On T2 and T3 packages, the helper runs in a bubblewrap mount namespace. It sees only the plugin's granted scopes (read-only for read grants), plus read-only `/usr`, `/bin`, `/lib*` and `/etc`. It has no network and a seccomp filter. On T1 bundled plugins, the helper runs unconfined at low priority.
6. **Timeout.** The helper is killed when `timeout_millis` passes. The completion is `failed(timed-out)`.
7. **Logging.** Each run is logged at level info with the program and its full argument list.

<figure>
<svg viewBox="0 0 400 200" role="img" aria-label="Helper process confinement">
<defs>
<marker id="pc-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
<path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
</marker>
</defs>
<rect x="8" y="10" width="110" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
<text x="63" y="34" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
<rect x="150" y="10" width="120" height="40" rx="6" fill="none" stroke="currentColor"/>
<text x="210" y="34" font-size="12" text-anchor="middle" fill="currentColor">Grant check</text>
<rect x="290" y="10" width="102" height="40" rx="6" fill="none" stroke="currentColor"/>
<text x="341" y="34" font-size="12" text-anchor="middle" fill="currentColor">Exit result</text>
<rect x="8" y="110" width="384" height="60" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="20" y="134" font-size="11" fill="currentColor">Helper: low priority, small environment, timeout,</text>
<text x="20" y="154" font-size="11" fill="currentColor">output cap; no network and own scopes on T2 and T3</text>
<line x1="118" y1="30" x2="150" y2="30" stroke="currentColor" marker-end="url(#pc-arrow)"/>
<text x="122" y="24" font-size="10" fill="currentColor">run</text>
<line x1="210" y1="50" x2="210" y2="110" stroke="currentColor" marker-end="url(#pc-arrow)"/>
<text x="218" y="84" font-size="10" fill="currentColor">starts program</text>
<line x1="341" y1="110" x2="341" y2="50" stroke="currentColor" marker-end="url(#pc-arrow)"/>
<text x="349" y="84" font-size="10" fill="currentColor">exit</text>
<line x1="63" y1="110" x2="63" y2="50" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#pc-arrow)"/>
<text x="69" y="84" font-size="10" fill="currentColor">output</text>
</svg>
<figcaption>The grant check runs before the helper starts. The helper runs inside the confinement box, and the plugin receives only the exit and the output.</figcaption>
</figure>

### Example 1: git status and an interrupt in Rust

**Grants:** `process.run:git` and `panes.signal`

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::process::{self, ProcessSpec};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::tree::Signal;
use ilium_plugin_sdk::types::intents::SignalPaneSpec;

/// Returns the short git status of a project, or None when git fails.
async fn project_status(project: FsPath) -> Option<String> {
    let spec = ProcessSpec {
        program: "git".to_string(),
        arguments: vec!["status".to_string(), "--short".to_string()],
        cwd: Some(project),
        stdin: None,
        environment: Vec::new(),
        timeout_millis: 10_000,
    };
    let exit = match process::run(spec).await {
        Ok(exit) => exit,
        Err(error) => {
            log::log(Level::Warn, &format!("git could not run: {:?}", error.code));
            return None;
        }
    };
    if exit.status != Some(0) {
        log::log(Level::Warn, &format!("git status exited with {:?}", exit.status));
        return None;
    }
    String::from_utf8(exit.stdout).ok()
}

/// Called from a command handler. The task runs on the SDK executor.
fn on_refresh_requested(project: FsPath) {
    task::spawn(async move {
        if let Some(status) = project_status(project).await {
            log::log(Level::Info, &format!("git status:\n{status}"));
        }
    });
}

/// Builds the submission that sends SIGINT to a pane's process.
fn interrupt_pane(pane: NodeId) -> Submission {
    Submission::new(Intent::SignalPane(SignalPaneSpec { pane, signal: Signal::Interrupt }))
}
```

The SDK resolves `process::run(spec).await` when the completion arrives. The plugin does not call `take_exit`. `Submission::new` assigns the next correlation number, so the plugin needs no counter of its own.

### Example 2: git status and an interrupt in TypeScript

**Grants:** `process.run:git`, `panes.read` and `panes.signal`

```ts
import { definePlugin, PluginError, type Event, type Submission } from '@ilium/plugin-sdk';
import { run, type ProcessExit } from '@ilium/plugin-sdk/host/process';
import { log } from '@ilium/plugin-sdk/host/log';
import type { FsPath, NodeId } from '@ilium/plugin-sdk/types';

const INTERRUPT_COMMAND = 'com.example.runner/interrupt';
let focusedPane: NodeId | undefined;
let nextCorrelation = 1n;

export async function projectStatus(project: FsPath): Promise<string | undefined> {
  let exit: ProcessExit;
  try {
    exit = await run({
      program: 'git',
      arguments: ['status', '--short'],
      cwd: project,
      stdin: undefined,
      environment: [],
      timeoutMillis: 10_000,
    });
  } catch (error) {
    log('warn', `git could not run: ${(error as PluginError).code}`);
    return undefined;
  }
  if (exit.status !== 0) {
    log('warn', `git status exited with ${exit.status}`);
    return undefined;
  }
  return new TextDecoder().decode(exit.stdout);
}

export const { handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'pane-focused') {
        focusedPane = event.val;
        continue;
      }
      if (event.tag !== 'command-invoked' || event.val.command !== INTERRUPT_COMMAND) continue;
      if (focusedPane === undefined) continue;
      submissions.push({
        intent: { tag: 'signal-pane', val: { pane: focusedPane, signal: 'interrupt' } },
        correlation: nextCorrelation,
        dedupKey: undefined,
      });
      nextCorrelation += 1n;
    }
    return submissions;
  },
});
```

`run` in TypeScript returns a `Promise` that resolves with `ProcessExit`. A refused or failed run rejects with `PluginError`, whose `code` field holds the error code. The `signal-pane` submission carries no dedup key, because only `send-input` and `agent-action` require one.

### Long-lived child processes

`process.run` starts one program and returns when it ends. Some jobs need a process that stays: a language server, a file watcher, a log follower, a transcoder that reports progress. `process.spawn` starts such a child and streams its output as events. It is on both sides, and it needs two grants: `process.long-lived` and `process.run:<program>` for the program itself.

```wit
// Excerpt of ilium:host/process in reference-wit
// ilium:host/process
record spawn-spec { process: process-spec, stdin-open: bool, line-mode: bool }
resource child {
  pid-hint: func() -> u32;                                   // for display only
  write-stdin: func(bytes: list<u8>) -> result<_, plugin-error>;
  close-stdin: func();
  signal: func(signal: string) -> result<_, plugin-error>;   // interrupt, terminate, kill, hangup
}
spawn: func(spec: spawn-spec) -> result<child, plugin-error>;
```

| Item | Rust (`host::process`) | TypeScript (`host/process`) | Meaning |
|---|---|---|---|
| `spawn` | `spawn(spec)` | `spawn(spec)` | Starts the child. Returns a handle or an error. |
| `child.pid_hint` | `child.pid_hint()` | `child.pidHint()` | An id to show in the UI. Never use it to signal. |
| `child.write_stdin` | `child.write_stdin(bytes)` | `child.writeStdin(bytes)` | Needs `stdin-open: true`. |
| `child.close_stdin` | `child.close_stdin()` | `child.closeStdin()` | Sends end of input. |
| `child.signal` | `child.signal("interrupt")` | `child.signal('interrupt')` | `interrupt`, `terminate`, `kill` or `hangup`. |

Output and the end arrive as events, delivered to `on-events`:

| Event | Payload | Delivery |
|---|---|---|
| `child-output` | `child`, `stream` (`stdout` or `stderr`), `bytes` | Never dropped. The host stops reading the pipe while the plugin's queue is full, so the child slows down instead of losing output. |
| `child-exited` | `child`, `status`, `signal` | Never dropped. |

With `line-mode: true`, each `child-output` is one whole line. `timeout-millis` of the `process-spec` must still be 1 to 600,000 (else `invalid-argument`) but is ignored for a spawned child. The child runs at low priority under the plugin's CPU quota, with the same environment rules as `run`. When the plugin drops the `child` handle or stops, the host sends SIGTERM and, after 2 seconds, SIGKILL.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::process::{self, ProcessSpec, SpawnSpec, Child};
use ilium_plugin_sdk::types::events::{Event, OutputStream};
use ilium_plugin_sdk::host::log::{self, Level};

struct LogFollower { child: Option<Child> }

impl LogFollower {
    fn start(&mut self, file: &str) -> Result<(), PluginError> {
        let child = process::spawn(&SpawnSpec {
            process: ProcessSpec {
                program: "tail".into(),
                arguments: vec!["-F".into(), file.into()],
                cwd: None,
                stdin: None,
                environment: Vec::new(),
                timeout_millis: 1, // must still be valid (1..600,000); ignored for spawned children
            },
            stdin_open: false,
            line_mode: true,
        })?;
        self.child = Some(child);
        Ok(())
    }

    fn on_event(&mut self, event: &Event) {
        match event {
            Event::ChildOutput(output) if output.stream == OutputStream::Stdout => {
                log::log(Level::Info, &format!("line: {}", String::from_utf8_lossy(&output.bytes)));
            }
            Event::ChildExited(exit) => {
                log::log(Level::Warn, &format!("child ended: status {:?}, signal {:?}", exit.status, exit.signal));
                self.child = None;
            }
            _ => {}
        }
    }

    fn stop(&mut self) {
        if let Some(child) = self.child.take() {
            let _ = child.signal("terminate");
        }
    }
}
```

**TypeScript**

```ts
import * as process from '@ilium/plugin-sdk/host/process';
import type { Child } from '@ilium/plugin-sdk/host/process';
import type { PluginEvent } from '@ilium/plugin-sdk';

let child: Child | undefined;

export function start(file: string): void {
  child = process.spawn({
    process: { program: 'tail', arguments: ['-F', file], cwd: undefined, stdin: undefined, environment: [], timeoutMillis: 1_000 },
    stdinOpen: false,
    lineMode: true,
  });
}

export function onEvent(event: PluginEvent): void {
  if (event.tag === 'child-output' && event.val.stream === 'stdout') {
    console.info('line:', new TextDecoder().decode(event.val.bytes));
  } else if (event.tag === 'child-exited') {
    console.warn('child ended', event.val.status, event.val.signal);
    child = undefined;
  }
}

export function stop(): void {
  child?.signal('terminate');
  child = undefined;
}
```

<figure>
<svg viewBox="0 0 430 120" role="img" aria-label="A spawned child sends output events to the plugin and the plugin signals or writes to it">
  <title>Spawned child</title>
  <desc>The plugin calls spawn and receives a child handle. The child process sends child-output events while it runs and a child-exited event at the end. The plugin can write stdin and signal the child. Dropping the handle sends terminate, then kill after two seconds.</desc>
  <defs><marker id="pc-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M0 0 L10 5 L0 10 z" fill="currentColor"/></marker></defs>
  <rect x="8" y="30" width="120" height="60" rx="6" fill="none" stroke="currentColor"/><text x="68" y="55" font-size="11" text-anchor="middle" fill="currentColor">plugin</text><text x="68" y="72" font-size="10" text-anchor="middle" fill="currentColor">holds child handle</text>
  <rect x="300" y="30" width="120" height="60" rx="6" fill="none" stroke="currentColor"/><text x="360" y="55" font-size="11" text-anchor="middle" fill="currentColor">child process</text><text x="360" y="72" font-size="10" text-anchor="middle" fill="currentColor">low priority</text>
  <line x1="128" y1="42" x2="300" y2="42" stroke="currentColor" marker-end="url(#pc-arrow)"/><text x="214" y="36" font-size="10" text-anchor="middle" fill="currentColor">write-stdin, signal</text>
  <line x1="300" y1="62" x2="128" y2="62" stroke="currentColor" marker-end="url(#pc-arrow)"/><text x="214" y="58" font-size="10" text-anchor="middle" fill="currentColor">child-output (events)</text>
  <line x1="300" y1="80" x2="128" y2="80" stroke="currentColor" marker-end="url(#pc-arrow)" stroke-dasharray="3 3"/><text x="214" y="96" font-size="10" text-anchor="middle" fill="currentColor">child-exited</text>
  <text x="8" y="112" font-size="10" fill="currentColor">drop handle or stop: SIGTERM, then SIGKILL after 2 s</text>
</svg>
<figcaption>Output flows to the plugin as events with back-pressure. Control flows the other way through the handle.</figcaption>
</figure>

<figure>
<pre class="term"><span class="t-dim">plugin log, ilium plugin logs log-follower</span>
2026-10-12 10:14:02 <span class="t-ok">info</span>  line: build started
2026-10-12 10:14:09 <span class="t-ok">info</span>  line: 3 tests failed
2026-10-12 10:14:09 <span class="t-warn">warn</span>  child ended: status Some(0), signal None
</pre>
<figcaption>Illustrative plugin log of the follower above. The format shows the timestamp, level and message.</figcaption>
</figure>

Errors and limits of `spawn`:

| Situation | Result |
|---|---|
| Missing `process.long-lived` or `process.run:<program>` | `not-permitted` |
| More children than `children` (default 2, T1 ceiling 8, T2/T3 ceiling 2) | `quota-exceeded` |
| More than 16 children per session (`session_processes`), shared by `process.run` (`busy`) and `process.spawn` (`quota-exceeded`) | `quota-exceeded` for `process.spawn` |
| Program not found | `not-found` |
| `write-stdin` when `stdin-open` was false | `invalid-argument` |
| Chunk of output larger than 1 MiB | Split into several events |
| Child ends | `child-exited` with `status` or `signal` |

### What the user sees

Installing the plugin opens the review dialog. It lists the program named in each grant. Arguments are not known at install time, so the dialog does not show them.

<pre class="term"><span class="t-dim">╭  ≡ ● · Plugin demo-git asks for high-risk grants───────────────╮</span>
<span class="t-dim">│</span>                                                                <span class="t-dim">│</span>
<span class="t-dim">│</span> [x] process.run:git     run the program git                    <span class="t-dim">│</span>
<span class="t-dim">│</span> [x] panes.signal        interrupt or stop programs             <span class="t-dim">│</span>
<span class="t-dim">│</span>                                                                <span class="t-dim">│</span>
<span class="t-dim">│</span> Arguments are not known at install and are not shown.          <span class="t-dim">│</span>
<span class="t-dim">│</span> Each run and each signal is written to the plugin log.         <span class="t-dim">│</span>
<span class="t-dim">│</span>                                                                <span class="t-dim">│</span>
<span class="t-dim">│</span>                         Esc Not now   Enter Allow selected     <span class="t-dim">│</span>
<span class="t-dim">│</span> <span class="t-dim">Space toggle · Enter allow · Esc cancel</span>                        <span class="t-dim">│</span>
<span class="t-dim">╰────────────────────────────────────────────────────────────────╯</span>
</pre>

Illustrative, not generated. The review dialog for a T1 bundled plugin. The user checks each grant before it is allowed.

At run time, every helper run and every refusal appears in the plugin log:

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs demo-git --tail 4
<span class="t-ok">run</span>  git status --short (cwd /home/arthur/dev/demo)
<span class="t-ok">exit</span> request 5, status 0, stdout 0 bytes
<span class="t-ok">exit</span> request 6, status 0, stdout 12 bytes
<span class="t-dim">       " M notes.md"</span>
<span class="t-err">refused</span> process.run sh: not-permitted, no grant
</pre>

Illustrative, not generated. A plugin log for two git runs and one refused run. The refusal names `sh`, because no grant of this plugin names it.

### Errors and edge cases

| Where | Code | When it happens |
|---|---|---|
| Sync, `run` | `not-permitted` | Program matches no `process.run:<program>` grant. |
| Sync, `run` | `invalid-argument` | Timeout is 0 or over 600,000 ms, stdin over 4 MiB, too many arguments or variables. |
| Sync, `run` | `not-permitted` | `cwd` lies outside a scope the plugin can read. |
| Sync, `run` | `busy` | The session already runs 16 children, or the plugin has too many pending requests. |
| Completion | `failed(not-found)` | The program is missing from `PATH`, or `cwd` does not exist. |
| Completion | `failed(io)` | The program exists but could not be started. |
| Completion | `failed(timed-out)` | The timeout passed. The helper was killed first. |
| Completion | `failed(quota-exceeded)` | Output passed the cap. The helper was killed and no partial output is delivered. |
| Completion | `cancelled` | The plugin was disabled or stopped while the helper ran. |
| Outcome, `signal-pane` | `ok` | The signal was sent. The process may still be running. |
| Outcome, `signal-pane` | `refused(not-found)` | No such pane, or the process has exited. The message is `no running process`. |
| Outcome, `signal-pane` | `refused(pane-not-ready)` | The pane was restored and its process has not started yet. Retry after the process starts. |
| Outcome, `signal-pane` | `refused(not-permitted)` | The batch lacks the `panes.signal` grant. |

Edge cases to handle in code:

- **Non-zero exit is success at the API level.** Check `status` yourself. Exit code 1 from `grep`, for example, means no match, not a failure.
- **Signal-ended processes.** `status` is `None` and `signal` holds a name such as `SIGKILL`.
- **Output and exit ordering.** The completion arrives once, after the helper exits. A partial stream is never delivered.
- **Disabling the plugin.** Pending runs are cancelled and their helpers are killed.

### Used by

- The progress monitor (feature 59) runs its probe commands with `process.run:sh`. This is a high-risk grant that only a bundled plugin holds.
- Worktree status and branch line (feature 69) and worktree cleanup (feature 70) run `git` with `process.run:git`.
- Agent in a new Git worktree (feature 67) runs `git` to create the worktree.

### See also

- Long-lived children use the grant `process.long-lived`; see the [capability reference](/pages/reference-capabilities/). The pane-process intents `terminate-pane-process` and `replace-pane-command` are on [pane lifecycle](/pages/api-pane-lifecycle/).
- [Intents](/pages/api-intents/) for the submission and outcome shapes, and the full `signal-pane` catalogue entry.
- [Capability: process run](/pages/capability-process-run/) for grant rules, confinement and the review dialog.
- [Capability: signal panes](/pages/capability-panes-signal/) for the `panes.signal` grant.
- [Workspace](/pages/api-workspace/) for worktree custody records, which go with `git` work.
- [Errors and limits](/pages/reference-errors-and-limits/) for the full error table.
- [Security](/pages/security/) for the risk tiers behind high-risk grants.

Targets `ilium:plugin@0.1.0`.

### Security

Children run with a fixed `PATH`, no network, and a data mount that is `noexec` ([SEC-033]). Output is redacted ([SEC-243]). See [Sandbox reference](/pages/reference-sandbox/).
