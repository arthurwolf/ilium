---
layout: page
title: cli
permalink: /pages/api-cli/
---

## cli

The `cli` import lets a server-side plugin add a subcommand to the `ilium` command line. The plugin declares the subcommand in its manifest. When the user runs `ilium <name> [arguments]`, the kernel routes the call to the plugin as a `cli-invoked` event. The plugin writes output, optionally reads standard input, and ends the call with an exit code. The invocation stays open until the plugin calls `finish`, or until the caller disconnects.

The interface provides a JSON-lines output form, a plain text form, a separate standard error stream, bounded standard input, a terminal test, the caller's pane, a shared pane-selector argument group, and commands that run without a server. This page documents the whole contract. CLI dispatch, server auto-start and session control stay in the kernel.

<figure>
<svg viewBox="0 0 440 290" role="img" aria-label="Sequence of a CLI invocation between caller, kernel and plugin">
  <defs>
    <marker id="api-cli-seq-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <text x="60" y="16" font-size="12" text-anchor="middle" fill="currentColor">Caller</text>
  <text x="220" y="16" font-size="12" text-anchor="middle" fill="currentColor">Kernel</text>
  <text x="380" y="16" font-size="12" text-anchor="middle" fill="currentColor">Plugin (server)</text>
  <line x1="60" y1="24" x2="60" y2="280" stroke="currentColor" stroke-dasharray="3 3"/>
  <line x1="220" y1="24" x2="220" y2="280" stroke="currentColor" stroke-dasharray="3 3"/>
  <line x1="380" y1="24" x2="380" y2="280" stroke="currentColor" stroke-dasharray="3 3"/>
  <line x1="60" y1="46" x2="218" y2="46" stroke="currentColor" marker-end="url(#api-cli-seq-arrow)"/>
  <text x="140" y="40" font-size="10" text-anchor="middle" fill="currentColor">ilium tally</text>
  <line x1="220" y1="72" x2="378" y2="72" stroke="currentColor" marker-end="url(#api-cli-seq-arrow)"/>
  <text x="300" y="66" font-size="10" text-anchor="middle" fill="currentColor">cli-invoked</text>
  <line x1="378" y1="100" x2="222" y2="100" stroke="currentColor" marker-end="url(#api-cli-seq-arrow)"/>
  <text x="300" y="94" font-size="10" text-anchor="middle" fill="currentColor">read-stdin (optional)</text>
  <line x1="220" y1="128" x2="378" y2="128" stroke="currentColor" marker-end="url(#api-cli-seq-arrow)"/>
  <text x="300" y="122" font-size="10" text-anchor="middle" fill="currentColor">request-completed</text>
  <line x1="378" y1="156" x2="222" y2="156" stroke="currentColor" marker-end="url(#api-cli-seq-arrow)"/>
  <text x="300" y="150" font-size="10" text-anchor="middle" fill="currentColor">write-line / write-text</text>
  <line x1="218" y1="178" x2="62" y2="178" stroke="currentColor" marker-end="url(#api-cli-seq-arrow)"/>
  <text x="140" y="172" font-size="10" text-anchor="middle" fill="currentColor">stdout</text>
  <line x1="378" y1="204" x2="222" y2="204" stroke="currentColor" marker-end="url(#api-cli-seq-arrow)"/>
  <text x="300" y="198" font-size="10" text-anchor="middle" fill="currentColor">write-stderr</text>
  <line x1="218" y1="226" x2="62" y2="226" stroke="currentColor" marker-end="url(#api-cli-seq-arrow)"/>
  <text x="140" y="220" font-size="10" text-anchor="middle" fill="currentColor">stderr</text>
  <line x1="378" y1="252" x2="222" y2="252" stroke="currentColor" marker-end="url(#api-cli-seq-arrow)"/>
  <text x="300" y="246" font-size="10" text-anchor="middle" fill="currentColor">finish(0)</text>
  <line x1="218" y1="274" x2="62" y2="274" stroke="currentColor" marker-end="url(#api-cli-seq-arrow)"/>
  <text x="140" y="268" font-size="10" text-anchor="middle" fill="currentColor">exit status 0</text>
</svg>
<figcaption>One invocation. The plugin answers the <code>cli-invoked</code> event, optionally reads standard input through the request model, writes output, and ends the call with <code>finish</code>. If the caller disconnects first, the kernel sends <code>cli-cancelled</code> instead.</figcaption>
</figure>

### What you need

- **Grant:** `cli.contribute`, on the server side only. The grant takes no argument (see [capability-cli-contribute](/pages/capability-cli-contribute/)). Every function and event on this page needs it.
- **Manifest:** a `cli` entry under `contributes`, and a `server` component that exports `handler`.
- **Language:** Rust with the `ilium-plugin-sdk` crate, or TypeScript with the `@ilium/plugin-sdk` package. Both are set up by `ilium plugin new` (see [getting started](/pages/getting-started-toolchain/)).
- **Imports:** `ilium_plugin_sdk::host::cli` in Rust, `@ilium/plugin-sdk/host/cli` in TypeScript.

### Manifest

The `cli` key under `contributes` declares the subcommands. It holds one object, or a list of at most 16 objects, and the names MUST be unique inside the plugin. A name in the reserved kernel list (see [CLI compatibility](/pages/reference-cli-compatibility/)) is refused with `manifest.cli-name-reserved`. Each entry has these fields; six are optional.

```json
{
  "capabilities": { "server": ["cli.contribute"] },
  "contributes": {
    "cli": [{
      "name": "tally",
      "summary": "Count lines of text read from standard input",
      "arguments_schema": { "type": "object", "properties": { "label": { "type": "string" } } },
      "output": "text",
      "stdin": "optional",
      "requires_server": false,
      "argument_groups": ["pane-selector"]
    }]
  }
}
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `name` | string | required | The subcommand. It must match `[a-z][a-z0-9-]{0,31}` and runs as `ilium <name>`. |
| `summary` | string | required | One line shown in `ilium --help`. |
| `arguments_schema` | JSON Schema object | none | Documents the arguments for help. The host passes `argv` unchanged and does not validate it. |
| `output` | `"jsonl"`, `"text"` or `"mixed"` | `"jsonl"` | The output form. It decides which write function is allowed (see "Output forms"). |
| `aliases` | list of strings | empty | Further names for the same command. Each follows the `name` rule. |
| `stdin` | `"none"`, `"optional"` or `"required"` | `"none"` | Whether the command may read standard input. |
| `requires_server` | bool | `true` | With `false`, the command also runs when no server is running (see "Commands without a server"). |
| `argument_groups` | list of strings | empty | `["pane-selector"]` adds `--pane <id\|title\|.>` and `--all-panes`, resolved by the host. |

A name that a kernel command uses is refused at install with `manifest.cli-name-reserved`. A name that another installed plugin owns is refused with `manifest.cli-name-taken`. An invalid `output`, `stdin` or `argument_groups` value is refused with `manifest.cli-field`.

### Surface

#### WIT

```wit
// Excerpt of ilium:host/cli in reference-wit
// ilium:host/cli  (cli.contribute, server side)
write-line:   func(invocation: u64, jsonl: string) -> result<_, plugin-error>;       // output "jsonl"
write-error:  func(invocation: u64, text: string)  -> result<_, plugin-error>;       // plain text to stderr
finish:       func(invocation: u64, exit-code: u8);

// further output and input functions
write-text:   func(invocation: u64, text: string) -> result<_, plugin-error>;        // output "text"
write-stderr: func(invocation: u64, text: string) -> result<_, plugin-error>;        // plain text to the caller's stderr
read-stdin:   func(invocation: u64, max-bytes: u32) -> result<request-id, plugin-error>;   // at most 4 MiB in total
take-stdin:   func(request: request-id) -> option<list<u8>>;                         // empty list: end of input
is-tty:       func(invocation: u64) -> tuple<bool, bool>;                            // stdout, stderr are terminals
caller-pane:  func(invocation: u64) -> option<node-id>;                              // from ILIUM_PANE_ID
record pane-selection { panes: list<node-id>, all: bool }
selected-panes: func(invocation: u64) -> option<pane-selection>;                     // with argument_groups ["pane-selector"]
```

#### Functions

| WIT function | Rust | TypeScript | Meaning |
|---|---|---|---|
| `write-line` | `cli::write_line(invocation: u64, jsonl: &str) -> Result<(), PluginError>` | `writeLine(invocation: bigint, jsonl: string): void` | One JSON object and a newline to standard output. Only for `output: "jsonl"`. |
| `write-error` | `cli::write_error(invocation: u64, text: &str) -> Result<(), PluginError>` | `writeError(invocation: bigint, text: string): void` | Plain text and a newline to standard error. |
| `write-text` | `cli::write_text(invocation: u64, text: &str) -> Result<(), PluginError>` | `writeText(invocation: bigint, text: string): void` | Plain text to standard output, written as given. Only for `output: "text"`. |
| `write-stderr` | `cli::write_stderr(invocation: u64, text: &str) -> Result<(), PluginError>` | `writeStderr(invocation: bigint, text: string): void` | Plain text to standard error, for either output form. |
| `read-stdin` | `cli::read_stdin(invocation: u64, max_bytes: u32) -> Result<RequestId, PluginError>` | `readStdin(invocation: bigint, maxBytes: number): bigint` | Asks for up to `max_bytes` of standard input. The chunk arrives as `request-completed`. |
| `take-stdin` | `cli::take_stdin(request: RequestId) -> Option<Vec<u8>>` | `takeStdin(request: bigint): Uint8Array \| undefined` | Takes the chunk. An empty list means end of input. |
| `is-tty` | `cli::is_tty(invocation: u64) -> (bool, bool)` | `isTty(invocation: bigint): [boolean, boolean]` | Whether standard output and standard error are terminals. |
| `caller-pane` | `cli::caller_pane(invocation: u64) -> Option<NodeId>` | `callerPane(invocation: bigint): bigint \| undefined` | The pane the command ran in, taken from `ILIUM_PANE_ID`. |
| `selected-panes` | `cli::selected_panes(invocation: u64) -> Option<PaneSelection>` | `selectedPanes(invocation: bigint): PaneSelection \| undefined` | The panes chosen by `--pane` or `--all-panes`. |
| `finish` | `cli::finish(invocation: u64, exit_code: u8)` | `finish(invocation: bigint, exitCode: number): void` | Ends the invocation. `exit_code` becomes the process exit status. |

In TypeScript, a failed call throws `PluginError`. `PaneSelection` is `{ panes: bigint[]; all: boolean }` in TypeScript and `PaneSelection { panes: Vec<NodeId>, all: bool }` in Rust.

#### Events

| Event | Payload | Rust | TypeScript |
|---|---|---|---|
| `cli-invoked` | `invocation` (u64), `name`, `argv` (list of strings), `cwd` | `Event::CliInvoked(CliInvokedEvent { invocation, name, argv, cwd })` | `{ tag: 'cli-invoked', val: { invocation, name, argv, cwd } }` |
| `cli-cancelled` | `invocation` (u64) | `Event::CliCancelled(u64)` | `{ tag: 'cli-cancelled', val: bigint }` |
| `request-completed` | `request`, `outcome` (for `read-stdin`) | `Event::RequestCompleted(..)` | `{ tag: 'request-completed', val: { request, outcome } }` |

Both CLI events are never dropped. `argv` holds the words after the name as the shell passed them. `cwd` is the absolute path of the caller's working directory.

### Output forms

The manifest `output` key picks the form. `jsonl` and `text` never mix in one command. `mixed` allows both `write-line` and `write-text`, for a command such as `new-pane` that prints a plain line for one argument shape and JSONL for another.

| Form | Allowed output calls | Rule | Typical use |
|---|---|---|---|
| `jsonl` (default) | `write-line`, `write-stderr`, `write-error` | Every line is exactly one JSON object with a string `type` field. Agents read this output. | Commands that agents and scripts call. |
| `text` | `write-text`, `write-stderr`, `write-error` | Written as given, no parsing. | Commands a person reads, or that feed a pipe. |

`write-text` on a JSONL command, and `write-line` on a text command, both return `invalid-argument`. The refused write is not sent.

### Standard input

Set `stdin` to `optional` or `required`. The plugin then reads input with the request model: `read-stdin` returns a `request-id`, the host completes it with `request-completed`, and `take-stdin` returns the bytes. An empty list means the caller closed standard input. The total over one invocation is at most 4 MiB. A command with `stdin: "none"` calls `read-stdin` and gets `unsupported`. See [Requests and outcomes](/pages/requests-and-outcomes/).

### Commands without a server

With `requires_server: false`, `ilium <name>` works when no Ilium server is running. The `ilium` process then runs the plugin's server component in a transient host inside the CLI process, with the same grants. The transient host is smaller than a server:

- `tree.*` returns empty results and no panes exist.
- Storage and settings are read from disk. The host neither reads nor writes the `restored` bytes.
- No other plugin and no client side runs there.
- `caller-pane` returns `none`.
- The host ends at `finish`, and the component is deactivated with the reason `shutdown`.

When a server is running, the command runs in the normal instance instead.

### Pane selector

`argument_groups: ["pane-selector"]` gives the command two options without any parsing in the plugin: `--pane <id|title|.>` (a pane id, a title, or `.` for the caller's pane) and `--all-panes`. The host resolves them to node ids. The plugin calls `selected-panes`. It receives the resolved list, or `none` when the caller gave neither option. `caller-pane` is separate: it reports the pane the command ran in, from the `ILIUM_PANE_ID` environment variable, and is `none` outside Ilium.

### Worked example: a tally command

`ilium tally` counts the lines on standard input and prints the result. It declares `output: "text"` and `stdin: "optional"`.

**Rust**

```rust
use ilium_plugin_sdk::host::cli;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::CliInvokedEvent;

const CHUNK_BYTES: u32 = 64 * 1024;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Tally {
    /// Open invocation, the read request in flight, and the lines counted so far.
    open: Option<(u64, RequestId, u64)>,
}

impl Lifecycle for Tally {}

impl Handler for Tally {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::CliInvoked(CliInvokedEvent { invocation, name, .. }) if name == "tally" => {
                    self.start(invocation);
                }
                Event::RequestCompleted(done) => self.on_chunk(done.request, done.outcome.is_ok()),
                Event::CliCancelled(invocation) => {
                    if matches!(self.open, Some((open, _, _)) if open == invocation) {
                        self.open = None;
                    }
                }
                _ => {}
            }
        }
        Vec::new()
    }
}

impl Tally {
    fn start(&mut self, invocation: u64) {
        let (stdout_is_tty, _) = cli::is_tty(invocation);
        if stdout_is_tty {
            let _ = cli::write_stderr(invocation, "tally: pipe some text into this command");
            cli::finish(invocation, 2);
            return;
        }
        match cli::read_stdin(invocation, CHUNK_BYTES) {
            Ok(request) => self.open = Some((invocation, request, 0)),
            Err(error) => {
                let _ = cli::write_stderr(invocation, &error.message);
                cli::finish(invocation, 1);
            }
        }
    }

    fn on_chunk(&mut self, request: RequestId, is_ok: bool) {
        let Some((invocation, open_request, lines)) = self.open else { return };
        if open_request != request {
            return;
        }
        self.open = None;
        let Some(bytes) = cli::take_stdin(request).filter(|_| is_ok) else {
            let _ = cli::write_stderr(invocation, "tally: input failed");
            cli::finish(invocation, 1);
            return;
        };
        if bytes.is_empty() {
            let _ = cli::write_text(invocation, &format!("{lines} lines\n"));
            cli::finish(invocation, 0);
            return;
        }
        let total = lines + bytes.iter().filter(|byte| **byte == b'\n').count() as u64;
        match cli::read_stdin(invocation, CHUNK_BYTES) {
            Ok(next) => self.open = Some((invocation, next, total)),
            Err(_) => cli::finish(invocation, 1),
        }
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import * as cli from '@ilium/plugin-sdk/host/cli';

const CHUNK_BYTES = 64 * 1024;

interface OpenCall { invocation: bigint; request: bigint; lines: number }
let open: OpenCall | undefined;

function start(invocation: bigint): void {
  const [stdoutIsTty] = cli.isTty(invocation);
  if (stdoutIsTty) {
    cli.writeStderr(invocation, 'tally: pipe some text into this command');
    cli.finish(invocation, 2);
    return;
  }
  open = { invocation, request: cli.readStdin(invocation, CHUNK_BYTES), lines: 0 };
}

function onChunk(request: bigint, isOk: boolean): void {
  if (open === undefined || open.request !== request) return;
  const call = open;
  open = undefined;
  const bytes = isOk ? cli.takeStdin(request) : undefined;
  if (bytes === undefined) {
    cli.writeStderr(call.invocation, 'tally: input failed');
    cli.finish(call.invocation, 1);
    return;
  }
  if (bytes.length === 0) {
    cli.writeText(call.invocation, `${call.lines} lines\n`);
    cli.finish(call.invocation, 0);
    return;
  }
  call.lines += bytes.filter((byte) => byte === 0x0a).length;
  call.request = cli.readStdin(call.invocation, CHUNK_BYTES);
  open = call;
}

export const { handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'cli-invoked' && event.val.name === 'tally') start(event.val.invocation);
      else if (event.tag === 'request-completed') onChunk(event.val.request, event.val.outcome.tag === 'ok');
      else if (event.tag === 'cli-cancelled' && open?.invocation === event.val) open = undefined;
    }
    return [];
  },
});
```

Both examples keep one open call and one read request in flight, and call `finish` on every path. A real command keeps a map keyed by invocation, because several invocations can run at once.

### Worked example: JSON lines and the pane selector

A command declared with `output: "jsonl"` and `argument_groups: ["pane-selector"]` reports the selected panes. It writes one object per pane, then a summary.

**Rust**

```rust
use ilium_plugin_sdk::host::cli;
use ilium_plugin_sdk::prelude::*;

fn report_panes(invocation: u64) -> Result<(), PluginError> {
    let Some(selection) = cli::selected_panes(invocation) else {
        cli::write_stderr(invocation, "pass --pane or --all-panes")?;
        cli::finish(invocation, 2);
        return Ok(());
    };
    for pane in &selection.panes {
        cli::write_line(invocation, &format!(r#"{{"type":"pane","id":{}}}"#, pane.0))?;
    }
    cli::write_line(
        invocation,
        &format!(r#"{{"type":"summary","panes":{},"all":{}}}"#, selection.panes.len(), selection.all),
    )?;
    cli::finish(invocation, 0);
    Ok(())
}
```

**TypeScript**

```ts
import * as cli from '@ilium/plugin-sdk/host/cli';

export function reportPanes(invocation: bigint): void {
  const selection = cli.selectedPanes(invocation);
  if (selection === undefined) {
    cli.writeStderr(invocation, 'pass --pane or --all-panes');
    cli.finish(invocation, 2);
    return;
  }
  for (const pane of selection.panes) {
    cli.writeLine(invocation, JSON.stringify({ type: 'pane', id: Number(pane) }));
  }
  cli.writeLine(
    invocation,
    JSON.stringify({ type: 'summary', panes: selection.panes.length, all: selection.all }),
  );
  cli.finish(invocation, 0);
}
```

For a slow command, start the work with `task::spawn` (Rust) or an async function (TypeScript) and write from it. Keep `on-events` short, because its deadline is 20 ms.

### What the user sees

Help lists the subcommand with its summary. A text command prints what the plugin wrote. A JSONL command prints one object per line. Standard error is a separate stream, so a pipe sees only standard output.

<figure>
<pre class="term"><span class="t-prompt">$</span> printf 'a\nb\nc\n' | ilium tally
3 lines
<span class="t-prompt">$</span> echo $?
0
<span class="t-prompt">$</span> ilium tally
<span class="t-err">tally: pipe some text into this command</span>
<span class="t-prompt">$</span> echo $?
2
</pre>
<figcaption>A text command. The first run reads a pipe. The second run has a terminal on standard output, so the plugin refuses with a message on standard error and exit status 2. Illustrative, not generated.</figcaption>
</figure>

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium panes-report --all-panes
<span class="t-dim">{"type":"pane","id":3}</span>
<span class="t-dim">{"type":"pane","id":7}</span>
<span class="t-dim">{"type":"summary","panes":2,"all":true}</span>
<span class="t-prompt">$</span> ilium panes-report
<span class="t-err">pass --pane or --all-panes</span>
</pre>
<figcaption>A JSON lines command with the pane selector. Every stdout line carries a type field. Illustrative, not generated.</figcaption>
</figure>

When the grant is declined, the subcommand is hidden from help and the command fails:

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium tally
<span class="t-err">error</span>: unknown command
</pre>
<figcaption>The plugin never runs, because the grant was declined. Illustrative, not generated.</figcaption>
</figure>

### Error codes

| Function | Code | When |
|---|---|---|
| `write-line` | `invalid-argument` | Not exactly one JSON object with a string `type`, a raw newline in the line, over 64 KiB, or the command declares `output: "text"`. |
| `write-text` | `invalid-argument` | The command declares `output: "jsonl"`, or the text is over 64 KiB. |
| `write-error`, `write-stderr` | `invalid-argument` | Text over 64 KiB. |
| every write | `not-found` | The invocation is finished or cancelled. |
| every write | `not-permitted` | The user declined `cli.contribute`. |
| `read-stdin` | `unsupported` | The command declares `stdin: "none"`. |
| `read-stdin` | `quota-exceeded` | The 4 MiB total for this invocation is used up. |
| `read-stdin` completion | `cancelled` | The caller left before the chunk arrived. |
| `finish` | none | An unknown or finished invocation is ignored. |
| manifest | `manifest.cli-field` | A bad `output`, `stdin` or `argument_groups` value. |
| manifest | `manifest.cli-name-taken` | The name belongs to another plugin. |
| manifest | `manifest.cli-name-reserved` | The name is a kernel command. |
| kernel | `cli.owner-disabled` | The owner plugin is disabled. The kernel answers with exit code 1 before any plugin runs. |

### Limits

| Item | Value | Error |
|---|---|---|
| Text of one write | 64 KiB | `invalid-argument` |
| Standard input per invocation | 4 MiB in total | `quota-exceeded` |
| Invocation deadline | none; open until `finish` or the caller disconnects | none |
| `on-events` deadline | 20 ms (50 ms at tier 1) | trap |

### Diagnostics and support

The host adds no debug switch to a contributed command. It passes `argv` unchanged and writes nothing about verbosity, so a subcommand is as verbose as its plugin makes it. Two settings control what the plugin log keeps, and neither is per invocation:

| Setting | How to set it | Scope |
|---|---|---|
| Threshold of one plugin | `ilium plugin log-level <id> debug`, and `ilium plugin log-level <id> --reset` to go back | All sides of the plugin, until the session stops ([log-level](/pages/reference-plugin-cli/#logs-and-log-level)) |
| Default threshold | Environment variable `ILIUM_PLUGIN_LOG_LEVEL` | Read by a session server when it starts. Setting it on one `ilium <name>` call does not change a server that is already running. |

The default threshold is `info`, so `debug` and `trace` lines are dropped, not stored ([DEV-653](/pages/reference-debugging-tools/)). Raise the threshold before the failure happens. `ilium plugin logs --level` only filters lines that were already stored.

For plugin authors:

- Write the detail a user may need later with the `log` import ([log](/pages/api-log/)): `debug` for the steps taken, `warn` or `error` for the failure. Write to standard error only the text the user must see at the terminal.
- A plugin that wants a verbose run defines its own flag, for example `--debug`, lists it in `arguments_schema` and parses it from `argv`. The host defines no such flag, so name it in the plugin README.
- Do not log `argv` values or standard input content as they are. They can hold private text. Log a count or an id instead. A log line is 16 KiB at most ([AUT-315](/pages/guide-publishing-and-support/)).

When a user reports a failed subcommand, ask for this evidence, after checking it for secrets:

1. The exact command line, and the exit status printed by `echo $?` right after it. The kernel uses exit code 2 for `unknown command` and exit code 1 when the owner is disabled or the plugin stopped during the call ([CLI compatibility](/pages/reference-cli-compatibility/)).
2. Standard output and standard error in separate files, for example `ilium <name> <arguments> >out.txt 2>err.txt`. A pipe carries only standard output.
3. The Ilium version (`ilium --version`), and whether a server was running.
4. `ilium plugin info <id>`, which shows the version and grants. A declined `cli.contribute` hides the subcommand, so `unknown command` points to that grant.
5. `ilium plugin status <id>` and `ilium plugin logs <id> --tail 200`. Host lines show refused calls, traps, restarts and failed activations. The page does not define a log line for each invocation, so a missing line does not prove the call never arrived.
6. `ilium plugin doctor`, when the failure may come from the host or the sandbox rather than the plugin.

Do not ask for unredacted trace output or for the content of standard input. Ask for its byte or line count. Use the report template in [Publishing and support](/pages/guide-publishing-and-support/#a-template-for-bug-reports) to collect the rest.

### Edge cases

| Situation | Result |
|---|---|
| No `cli.contribute` grant | The subcommand is hidden from help. `ilium <name>` fails with `unknown command`. Calls return `not-permitted`. |
| Caller disconnects early | `cli-cancelled` arrives. Later writes return `not-found`. |
| Plugin restarts, is disabled or is uninstalled while open | The invocation ends with exit code 1 and `plugin stopped` on standard error. |
| Handler traps | The plugin restarts under the rule in [errors and limits](/pages/reference-errors-and-limits/), and the open invocation ends as above. |
| Output form mismatch | `invalid-argument`; the write is dropped. |

<div class="warn">
<p>A plugin that never calls <code>finish</code> leaves the caller waiting with no deadline. Call <code>finish</code> on every path, including the error path and the cancel path.</p>
</div>

<div class="note">
<p>Several invocations can run at once. Each has its own id, and each <code>cli-invoked</code> event arrives in a batch with other events.</p>
</div>

### Used by

- `ilium voice say` is a `cli` subcommand that asks the plugin's own client side to act (see [client requests](/pages/api-client-requests/)).
- `ilium progress` writes its status through `cli.write-line` and `cli.finish`.
- The chatroom commands hold `cli.contribute`.
- `ilium broadcast` (all agent panes, across sessions) is the cross-session case (see [sessions](/pages/api-sessions/)).
- The context-injection hook reads its read position from `storage` when `cli-invoked` arrives.

### See also

- [http-api](/pages/api-http-api/) for the other server entry point.
- [Events](/pages/api-events/) for `cli-invoked` and `cli-cancelled`.
- [Exports](/pages/api-exports/) for the `handler` export that receives them.
- [sessions](/pages/api-sessions/) for running a command of the same plugin in another Ilium session.
- [Capability: cli.contribute](/pages/capability-cli-contribute/) for the grant and its declined behaviour.
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/) for the build setup.

Interface version: `ilium:host/cli`, version 0.1.
