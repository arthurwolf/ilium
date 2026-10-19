---
layout: page
title: "Capability: process.run"
permalink: /pages/capability-process-run/
---

## process.run:&lt;program&gt;

The `process.run:<program>` capability lets a plugin start one named external program, wait for it to exit, and read its exit status and output. The grant names the program. Arguments, working folder, environment and timeout are chosen on each call, inside the limits on this page.

This is a high-risk capability (H). A plugin that can run programs can do nearly anything the user can do at a command line, so the user decides on every grant, even for a bundled plugin.

<div class="warn">
<p>Granting <code>process.run</code> lets the plugin run the named program with the user's permissions. Grant it only to a program whose behaviour you can review. Grant a shell (<code>sh</code>) only when the plugin's purpose needs one, because a shell can start any other program.</p>
</div>

### What you need

| Item | Requirement |
|---|---|
| Grant | `process.run:<program>` for each program, on the side that starts it (`server` or `client`). Each side holds its own grants. |
| Working folder | `fs.read` for the scope that holds the `cwd` you pass. No grant exists per folder. |
| Job code | A job instance may call `process.run` with the same grants as its plugin side. |
| Rust | `ilium_plugin_sdk::host::process`, with `process::run`, `ProcessSpec` and `ProcessExit`. |
| TypeScript | `@ilium/plugin-sdk/host/process`, with `run`, `ProcessSpec` and `ProcessExit`. |
| Packages | T1 bundled plugins ask for the grant once, at first activation. T2 and T3 packages run the helper in a sandbox, and they are refused on Windows and macOS until a native sandbox exists. |

The manifest lists each grant under the side that uses it:

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.notes",
  "capabilities": { "server": ["process.run:git", "fs.read:project"] }
}
```

### Grant syntax

The argument is either a program name, resolved through `PATH`, or an absolute path, which pins one exact file. Install checks each grant with the rules of the grant grammar and these program rules:

| Grant | Result at install | Reason |
|---|---|---|
| `process.run:git` | accepted | a program name, resolved through `PATH` |
| `process.run:/usr/bin/git` | accepted | an absolute path; the grant names one file |
| `process.run:sh` | accepted for T1 only, H | a shell can start any program, so the grant is very wide |
| `process.run` | `manifest.grant-argument-missing` | a program is required |
| `process.run:*` | `manifest.grant-wildcard` | wildcards are refused for this grant |
| `process.run:git status` | `manifest.grant-program` | arguments belong in `arguments`; a program name has no spaces |

A T2 or T3 package that asks for `process.run:sh` fails with `manifest.grant-tier`.

### The surface

| Name (Rust / TypeScript) | WIT form | Meaning |
|---|---|---|
| `process::run` / `run` | `run(spec) -> result<request-id, plugin-error>`, wrapped as async | Starts the program and resolves with `ProcessExit` when it exits. A non-zero exit code is not an error. A failed run rejects with `PluginError`. |
| `process::run_request` / `runRequest` | the raw `run` | Returns a `RequestId`. The exit arrives later as a `request-completed` event. |
| `process::take_exit` / `takeExit` | `take-exit(request) -> option<process-exit>` | Returns the exit of a raw request once. The result is held for 30 seconds or until taken. Async code does not need it. |

The records:

| Field | Rust | TypeScript | Meaning and limit |
|---|---|---|---|
| `process-spec.program` | `String` | `string` | Program name or absolute path. It must match a grant. |
| `process-spec.arguments` | `Vec<String>` | `string[]` | Arguments after the program. At most 1,024. |
| `process-spec.cwd` | `Option<FsPath>` | `FsPath \| undefined` | Working folder. It must lie in a scope the plugin can read. |
| `process-spec.stdin` | `Option<Vec<u8>>` | `Uint8Array \| undefined` | Standard input. At most 4 MiB. |
| `process-spec.environment` | `Vec<(String, String)>` | `[string, string][]` | Each pair replaces the inherited variable of the same name, except `PATH`, `LD_PRELOAD` and `LD_LIBRARY_PATH`, which cannot be set (`invalid-argument`, [SEC-033]). At most 64 pairs. |
| `process-spec.timeout_millis` | `u32` | `number` | Required. From 1 to 600,000 ms. |
| `process-exit.status` | `Option<i32>` | `number \| undefined` | Exit code. `None` when a signal ended the process. |
| `process-exit.signal` | `Option<String>` | `string \| undefined` | POSIX name such as `SIGKILL`. Set only when a signal ended the process. |
| `process-exit.stdout` | `Vec<u8>` | `Uint8Array` | Standard output. |
| `process-exit.stderr` | `Vec<u8>` | `Uint8Array` | Standard error, kept apart from stdout. |

### How the helper runs

<figure>

<svg viewBox="0 0 420 260" role="img" aria-label="Path of a process.run call from plugin to helper and back">
  <defs>
    <marker id="pr-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="110" y="10" width="200" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="34" text-anchor="middle" font-size="12" fill="currentColor">Plugin instance (server or job)</text>
  <line x1="210" y1="50" x2="210" y2="78" stroke="currentColor" marker-end="url(#pr-arrow)"/>
  <text x="220" y="68" font-size="10" fill="currentColor">process.run(spec)</text>
  <rect x="110" y="78" width="200" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="102" text-anchor="middle" font-size="12" fill="currentColor">Grant check and limits</text>
  <line x1="210" y1="118" x2="210" y2="146" stroke="currentColor" marker-end="url(#pr-arrow)"/>
  <text x="220" y="136" font-size="10" fill="currentColor">starts helper</text>
  <rect x="40" y="146" width="340" height="60" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="72" y="166" font-size="11" fill="currentColor">Helper: low priority, CPU quota, timeout</text>
  <text x="72" y="182" font-size="11" fill="currentColor">Environment: PATH, HOME, LANG and given pairs</text>
  <text x="72" y="198" font-size="11" fill="currentColor">Output cap; T2 and T3 add confinement</text>
  <line x1="210" y1="206" x2="210" y2="226" stroke="currentColor" marker-end="url(#pr-arrow)"/>
  <text x="220" y="219" font-size="10" fill="currentColor">exit</text>
  <rect x="40" y="226" width="340" height="26" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="243" text-anchor="middle" font-size="11" fill="currentColor">Completion: status, signal, stdout, stderr</text>
</svg>
<figcaption>The grant check runs before the helper starts, and it refuses a call at once. The helper runs inside its limits. The plugin receives only the exit and the two output streams.</figcaption>
</figure>

When the plugin calls `run`:

1. **Grant check.** A program that matches no grant, including a grant the user declined, returns `not-permitted` at once. A `cwd` outside a readable scope returns `not-permitted`. A value over a fixed cap returns `invalid-argument`. Nothing starts.
2. **Program lookup.** A bare name is resolved through `PATH`. An absolute path runs exactly that file.
3. **Environment.** The helper starts with `PATH` (fixed to `/usr/bin:/bin`, [SEC-027]), `HOME` and `LANG` only. Each `environment` pair then replaces or adds one variable, except `PATH`, `LD_PRELOAD` and `LD_LIBRARY_PATH`, which cannot be set (`invalid-argument`, [SEC-033]).
4. **Priority and quota.** The helper runs at low priority, and its CPU time is charged to the plugin's CPU quota.
5. **Confinement.** On T2 and T3 packages the helper runs in a bubblewrap mount namespace with a seccomp filter. It sees only the plugin's granted scopes (read-only for read grants), plus read-only `/usr`, `/bin`, `/lib*` and `/etc`, and it has no network. On T1 bundled plugins the helper runs unconfined, at low priority.
6. **Timeout.** The helper is killed when `timeout_millis` passes, and the completion is `failed(timed-out)`.
7. **Output cap.** When stdout and stderr together pass the cap, the helper is killed and the completion is `failed(quota-exceeded)`. No partial output is delivered.
8. **Logging.** Each run is written to the plugin log at level info, with the program and its full argument list.

### Limits

| Limit | Default | T1 ceiling | Breach |
|---|---|---|---|
| Stdout plus stderr (`process_output_mib`) | 4 MiB | 32 MiB | `failed(quota-exceeded)`; process killed |
| Standard input | 4 MiB | 4 MiB | `invalid-argument` |
| Arguments / environment pairs | 1,024 / 64 | 1,024 / 64 | `invalid-argument` |
| `timeout_millis` | required, 1 to 600,000 | same | `invalid-argument` |
| Concurrent children per session (`session_processes`, 16 per session, shared by `process.run` and `process.spawn`) | 16 | 16 | `busy` for `process.run`; `process.spawn` returns `quota-exceeded` |
| Pending requests per plugin (`pending_requests`) | 64 | 256 | `busy` |

### Risk and the review dialog

The review dialog lists the program name from each grant. It does not show arguments, because they are not known at install. Every run and its argument list appear in the plugin log, so the user can see what ran after the fact.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.process 0.1.0 by Example Ltd                                   │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ <span class="t-warn">! [ ] Runs the program git</span> <span class="t-dim">…………………………………………………………………………………</span> <span class="t-warn">process.run:git</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests process.run:git, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs demo-git --tail 2
<span class="t-ok">info</span>  run git status --short (cwd /home/arthur/dev/demo)
<span class="t-warn">warn</span>  git exited with Some(128): not a git repository
</pre>

<figcaption>Illustrative, not generated. The log shows the run line that the host writes and a warning that the plugin wrote after a failed exit.</figcaption>
</figure>

### Errors and edge cases

| Where | Code | When it happens |
|---|---|---|
| `run`, sync | `not-permitted` | The program matches no grant, or a declined grant leaves none. |
| `run`, sync | `invalid-argument` | `timeout_millis` is 0 or over 600,000. Stdin is over 4 MiB. There are more than 1,024 arguments or 64 pairs. |
| `run`, sync | `not-permitted` | `cwd` lies outside a scope the plugin can read. |
| `run`, sync | `busy` | The session runs 16 children, or the plugin's pending requests are full. |
| Completion | `failed(not-found)` | The program is not on `PATH`, or `cwd` does not exist. |
| Completion | `failed(io)` | The program was found but could not start. |
| Completion | `failed(timed-out)` | The timeout passed. The helper was killed first. |
| Completion | `failed(quota-exceeded)` | Output passed the cap. The helper was killed, and no output is delivered. |
| Completion | `cancelled` | The plugin was disabled, stopped or shut down while the program ran. The helper is killed. |

Edge cases to handle in code:

- **A non-zero exit is not an error for `run`.** Check `status` yourself. `grep` exits with 1 when nothing matches, which is a normal answer.
- **A signal ends the process.** Then `status` is `None` and `signal` holds a name such as `SIGKILL`. Treat that as a failure.
- **Only one completion.** The exit and the output arrive together, once the helper has exited.
- **A missing program is not a grant error.** The grant matched, so the failure is `failed(not-found)`, not `invalid-argument`.
- **Timeouts are required.** Choose a value that fits the work, and keep it under 600,000 ms.

### Worked example 1: Rust

This example runs `git status --short` in a project folder and reports the outcome to the log. It needs `process.run:git` and `fs.read` for the project scope.

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::process::{self, ProcessExit, ProcessSpec};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::task;

/// Returns the short status of a project folder, or the reason it could not be read.
pub async fn short_status(project: FsPath) -> Result<String, String> {
    let spec = ProcessSpec {
        program: "git".to_string(),
        arguments: vec!["status".to_string(), "--short".to_string()],
        cwd: Some(project),
        stdin: None,
        environment: Vec::new(),
        timeout_millis: 10_000,
    };
    let exit: ProcessExit = process::run(spec)
        .await
        .map_err(|error| format!("git did not run: {:?}", error.code))?;
    if let Some(signal) = exit.signal {
        return Err(format!("git was stopped by {signal}"));
    }
    if exit.status != Some(0) {
        return Err(format!("git exited with {:?}", exit.status));
    }
    Ok(String::from_utf8_lossy(&exit.stdout).into_owned())
}

/// Called from a command handler. The SDK runs the task on its executor.
pub fn on_refresh_requested(project: FsPath) {
    task::spawn(async move {
        match short_status(project).await {
            Ok(text) => log::log(Level::Info, &format!("git status:\n{text}")),
            Err(reason) => log::log(Level::Warn, &reason),
        }
    });
}
```

`process::run(spec).await` resolves when the helper exits. The `?` operator passes a refused or failed run out as a message. The plugin never calls `take_exit`.

### Worked example 2: TypeScript

This example counts `TODO` lines with `grep`. It needs `process.run:grep`. It treats exit code 1 as zero matches, and it reports a signal or any other failure as an error.

**TypeScript**

```ts
import { PluginError } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';
import { run, type ProcessExit } from '@ilium/plugin-sdk/host/process';
import type { FsPath } from '@ilium/plugin-sdk/types';

/** Counts TODO lines under a project folder. Returns undefined when grep fails. */
export async function countTodos(project: FsPath): Promise<number | undefined> {
  let exit: ProcessExit;
  try {
    exit = await run({
      program: 'grep',
      arguments: ['-r', '--count', 'TODO', '.'],
      cwd: project,
      stdin: undefined,
      environment: [],
      timeoutMillis: 5_000,
    });
  } catch (error) {
    const code = error instanceof PluginError ? error.code : 'internal';
    log('warn', `grep could not run: ${code}`);
    return undefined;
  }
  if (exit.signal !== undefined) {
    log('warn', `grep was stopped by ${exit.signal}`);
    return undefined;
  }
  if (exit.status === 1) return 0;
  if (exit.status !== 0) {
    log('warn', `grep exited with ${exit.status}`);
    return undefined;
  }
  const lines = new TextDecoder().decode(exit.stdout).split('\n');
  return lines
    .filter((line) => line.length > 0)
    .reduce((total, line) => total + Number(line.split(':').pop() ?? 0), 0);
}
```

The `run` call rejects with `PluginError` only when the run fails. A grep that finds nothing resolves with `status` 1, so the function returns 0 rather than an error.

### Who requests it

First-party plugins from [feature traceability](/pages/reference-feature-traceability/):

- 59 Progress monitor: `process.run:sh`, to run arbitrary probe commands. Only a T1 bundled plugin can hold this grant.
- 67, 68, 69 and 70 Git worktree features: `process.run:git`.
- 71 Worktree include copy and post-create command: `process.run:sh`.

Requirement ids for this page: [CAP-009]. See the [requirements index](/pages/reference-requirements/).

### See also

- [Process control](/pages/api-process-control/) for the full `process` import and the `signal-pane` intent.
- [Capability: signal panes](/pages/capability-panes-signal/) for the `panes.signal` grant that stops a pane's process.
- [Capability: fs.read](/pages/capability-fs-read/) for the scope that holds a working folder.
- [Capability: workspace.manage](/pages/capability-workspace-manage/), which creates worktrees through the kernel and uses this grant for Git commands.
- [Capability: jobs.run](/pages/capability-jobs-run/) for work that does not need an external program.
- [Errors and limits](/pages/reference-errors-and-limits/) for the full error table.
- [Security](/pages/security/) for the risk tiers behind high-risk grants.

Targets `ilium:plugin@0.1.0`.

### Security

Children get a rebuilt environment: `LD_PRELOAD`, `LD_LIBRARY_PATH` and `PATH` cannot be set (`invalid-argument`, [SEC-033]), `PATH` is fixed to `/usr/bin:/bin` ([SEC-027]), the data folder is mounted `noexec`, the child has no network on T2 and T3 (T1 bundled plugins run unconfined), and secrets are redacted from captured output ([SEC-243]). A refused program returns `not-permitted`. See [Sandbox reference](/pages/reference-sandbox/).
