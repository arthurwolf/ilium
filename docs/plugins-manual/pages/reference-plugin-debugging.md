---
layout: page
title: Debugging a running plugin
permalink: /pages/reference-plugin-debugging/
---

## Debugging a running plugin

This page defines the target debug workflow for a plugin: a debug build that keeps symbols, attaching a debugger to a running plugin, and turning a trap backtrace from a release component back into source lines. Release builds strip DWARF and source maps ([DEV-303]), so this workflow is the only way to see source lines. The tools that show what a plugin did are in [Debugging tools](/pages/reference-debugging-tools/). This page describes target behaviour; it does not claim the behaviour exists today.

Requirement ids of this page are in the range `DEV-876` to `DEV-878`. The complete list is in [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/).

### Rules

- **[DEV-876]** `ilium plugin build` without `--release` MUST produce a debug component that keeps DWARF information and, for TypeScript, an inline source map, and MUST write `dist/<name>.debug.json` that records the `input_hash` of the build. A debug component MUST NOT be accepted by `pack` or `publish`; they fail with `package.debug-build`.

- **[DEV-877]** `ilium plugin debug <dir>` MUST start the plugin in a session and open a debugger endpoint on a loopback address with a random port. It MUST print the endpoint in an `artifact` record of kind `debug-endpoint`. It MUST refuse to bind a non-loopback address and fail with `debug.not-loopback`. The endpoint MUST close when the session ends.

- **[DEV-878]** `ilium plugin symbolize <trap.json> --debug-info <file>` MUST map each frame of a trap backtrace to a source file, line and function name when the debug information matches the `input_hash` in the trap record. When the hash differs it MUST fail with `debug.hash-mismatch` and print no guessed lines. A frame with no information MUST be printed as its function name only.

### Access to the debugger endpoint

The rules below are not numbered. They complete [DEV-877]. A loopback address keeps other machines out, but it does not keep out other processes on the same machine, so the endpoint has its own gates.

- **Off by default.** The endpoint exists only while someone runs `ilium plugin debug <dir>` in a terminal. A setting, a grant or a plugin call never opens one, and a plugin cannot ask for a debugger.
- **Per-session secret.** When the session starts, `ilium plugin debug` draws a 256-bit secret from the operating system random source. It writes the secret to a token file in the owner-only runtime folder that also holds the HTTP API token: folder mode 0700, file mode 0600 ([Network egress](/pages/reference-network-egress/), [SEC-098]). The `debug-endpoint` artifact record names the endpoint and the token file path. It never contains the secret, so the secret stays out of logs and copies of the terminal output.
- **Authentication first.** The first message of every client is the secret. A connection that sends no secret, or a wrong one, gets one reply with the code `debug.unauthenticated` and is closed. Before a client authenticates, the endpoint sends it no session data. Each refusal adds one `debug.unauthenticated` diagnostic to the output of `ilium plugin debug` and one record to the [audit log](/pages/reference-audit-log/).
- **One client at a time.** The endpoint accepts one authenticated client. A second client, even one with the secret, gets `debug.busy`, and the first client stays attached. The output of `ilium plugin debug` prints a `warning` record with the same code.
- **Ends with the session.** The endpoint closes when the session ends ([DEV-877]). The host then deletes the token file, so a later session has a new secret and an old secret is refused.
- **Stated limit.** The token file is readable by any process that runs as the same user. Such a process can attach if it reads the file. The secret keeps out other users and processes that cannot read the file. A plugin's helper cannot reach the runtime folder ([SEC-098]), so a plugin cannot read the token.

A debugger client reads only the data of the paused plugin:

- **Frames and values.** The call stack with source positions from [DEV-878], and the locals and linear memory of that plugin instance. It never reads the memory of the server or of another plugin's helper.
- **Settings.** The plugin's own settings. A secret row is returned as `<redacted>`, the same replacement that `settings.read-kernel` uses for kernel secrets ([Secrets](/pages/reference-secrets/)).
- **Grants.** The name and state of each grant (granted, declined or revoked). The debugger does not list the values behind a grant, such as file contents or clipboard text. Those values are visible only inside linear memory, under the next rule.
- **Memory and secrets.** A byte range of linear memory that holds a plugin secret, in one of the forms that the redaction steps of [Secrets](/pages/reference-secrets/) match, is returned with those bytes set to zero and a `debug.redacted` note. A secret in a form that the redaction steps do not match is not found. The Secrets page states the same limit for logs.
- **Environment.** Only the environment that the helper gives the plugin, which is rebuilt for each helper ([SEC-021]). The server's environment and the API token are never returned.

Tests for these rules:

- A connection with no secret, a wrong secret and the secret of an earlier session each get `debug.unauthenticated`, receive no session data, and are closed. The session keeps running.
- The token file has mode 0600 in a 0700 folder, differs between two sessions, and is gone after the session ends.
- A plugin helper cannot open the token file ([SEC-098] test).
- A second authenticated client gets `debug.busy`, and the first client still reads frames.
- A secret in the settings reads as `<redacted>`. A plugin secret written into linear memory reads as zero bytes at its place. The server's environment variables are absent from the environment read.
- Without `ilium plugin debug`, no debugger port is open for the plugin.

### Debugger protocol and editor attach

The rules below are not numbered. They complete [DEV-877], [DEV-876] and [DEV-715]. They say which protocol the endpoint speaks and how an editor or a command-line debugger attaches.

**Wire protocol.** The endpoint speaks the Debug Adapter Protocol (DAP) over a plain TCP connection on the loopback port. Messages use the standard DAP framing: a `Content-Length` header, an empty line and a JSON body. The endpoint is a DAP server that the client connects to, so the client uses the `attach` request, never `launch`: the plugin is already running in the session. The host does not expose the Wasmtime debugging interface directly, and a client cannot send raw Wasmtime commands.

**Authentication in the protocol.** The first message of a client is the secret ([Access to the debugger endpoint](#access-to-the-debugger-endpoint)), sent as one line of 64 hexadecimal characters followed by a newline, before any DAP message. After the host accepts the secret, the connection carries DAP only. A client that cannot send a secret line cannot attach, so a generic DAP client needs the adapter command below.

**Client setup.** `ilium plugin debug <dir>` prints a `debug-endpoint` `artifact` record with the host, the port and the token file path. An author attaches in one of two ways:

- **VS Code.** Run `ilium plugin debug <dir> --adapter` in a terminal task. It starts a local stdio-to-endpoint bridge that reads the printed port and the token file, sends the secret, and then passes DAP messages in both directions. The `launch.json` entry has `"request": "attach"`, `"type": "ilium-wasm"` and a `debugServer` field that holds the port the `debug-endpoint` record printed. The bridge reads the token file itself, so the secret is never written into `launch.json`. If `launch.json` is committed, it holds only the port placeholder `${input:ilium-debug-port}`.
- **Command line.** Any DAP client that can run a command as its adapter, such as a terminal DAP client, uses `ilium plugin debug <dir> --adapter` as that command. A plain `lldb` or `gdb` cannot attach: they do not speak DAP.

**Sides that can be debugged.** The debugger attaches to one plugin side per session. `server`, `client` and `scene` components can each be debugged when the build is a debug build ([DEV-876]). Without `--side`, `ilium plugin debug` attaches to the only declared side. When the manifest declares several sides, the command fails with `usage.side-required` and names the sides. Only the Wasm component is debugged. A helper process of the plugin, the host and the other side of the plugin are not reachable; their frames never appear in a stack.

**Source maps for TypeScript.** The debug build keeps an inline source map ([DEV-876]). The host maps each Wasm frame to a TypeScript file and line with that map and sends the TypeScript position to the client as the DAP stack frame. Breakpoints set in a `.ts` file are translated the other way. A breakpoint on a line that the map does not cover is reported as unverified with the reason `no source map for this line` and never fires. The separate `dist/<side>.wasm.map` file of a release build ([DEV-715]) is not used by the debugger. A source file whose content differs from the one recorded in the map is shown, and a `warning` record names the file.

**Protocol version mismatch.** The first DAP `initialize` request carries the client's DAP version in `adapterID` and `clientID` fields. The host supports the DAP version of `ilium:plugin@0.1.0`. When the client requires a version the host does not support, the host answers the `initialize` request with `success: false`, the code `debug.protocol-unsupported`, and the host's supported versions in the message, then closes the connection. The session keeps running and the endpoint stays open for a client with a matching version. The output of `ilium plugin debug` adds one `diagnostic` record with the same code, and the exit code follows [Plugin command-line reference](/pages/reference-plugin-cli/#exit-codes) only when the session ends.

Tests for these rules:

- A DAP client connected through `--adapter` attaches, reads a stack and continues, and a client that sends `launch` is refused with an error response.
- A TypeScript debug build resolves a frame to the `.ts` line, and a breakpoint on an unmapped line is reported unverified.
- Two sides declared and no `--side` fail with `usage.side-required`.
- A client that requires an unsupported DAP version gets `debug.protocol-unsupported`, the connection closes, and a second client with a supported version attaches.

### Pauses and deadlines

A debug session can pause a call at a breakpoint or while stepping. These rules say how the pause interacts with the limits of the call. The limits are the ones of [Errors and limits](/pages/reference-errors-and-limits/), such as `event_deadline_ms` and `activate_deadline_ms`.

- **Pause time is not deadline time.** The deadline clock of a call stops when the debugger pauses the call and starts again when the call resumes. A developer who steps through a call does not get `trap.deadline` from the time spent in the pause.
- **Own running time still counts.** The part of the call that runs between pauses counts toward its deadline as usual. A call that runs 2,100 ms without a pause still traps at a 2,000 ms `activate_deadline_ms`, with `trap.deadline` and the normal limit.
- **Only the paused call.** A call of another plugin that waits for the paused plugin keeps its own deadline. If that deadline passes while the other plugin is paused, the other call traps as it would if a plugin were slow.
- **Memory ceiling unchanged.** `memory_mib` is not a time limit, so a pause does not change it. A paused instance keeps its memory, and growth after the resume is checked as in a normal run (`trap.memory`, [Errors and limits](/pages/reference-errors-and-limits/)). The debugger never allocates guest memory to answer a read.
- **Difference from tests.** `ilium plugin test --component` runs without a debugger, so it has no pauses. A debug run can take longer in wall time than a test run, but pause time never causes a trap. The trap outcome of a call therefore depends only on its running time, which is the same in both runs.

Tests for these rules:

- An `activate` paused for 3,000 ms at a breakpoint and then resumed returns `ok` when its own running time is under 2,000 ms.
- The same component, with 2,100 ms of running time and no pause, traps with `trap.deadline`.
- A call with a 1,000 ms pause and 1,900 ms of running time returns `ok`, and the same call with 2,100 ms of running time traps with `trap.deadline`, whatever the pause.
- A pause followed by growth past `memory_mib` traps with `trap.memory`.

### Failure behaviour

Each rule names its failure code. A failure prints a `diagnostic` record in JSONL or one line on standard error in text, and the process exit code follows [Plugin command-line reference](/pages/reference-plugin-cli/#exit-codes).

## See also

- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/)

API checked against `ilium:plugin@0.1.0`.
