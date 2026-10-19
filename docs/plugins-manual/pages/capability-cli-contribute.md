---
layout: page
title: "Capability: cli.contribute"
permalink: /pages/capability-cli-contribute/
---

## cli.contribute

The `cli.contribute` grant lets a server-side plugin add one subcommand to the `ilium` command
line (a plugin that owns several commands declares each one; see [Manifest](#manifest)). The plugin declares the subcommands in its manifest. When the user runs
`ilium <name> [arguments]`, the server sends a `cli-invoked` event to the plugin, and the plugin
answers with JSON lines on standard output and ends the call with an exit code. In `jsonl` output form every
output line must be one JSON object with a `type` field, because agents read this output. A
subcommand declared with output form `text` or `mixed` may also write plain text with `write-text`.

The grant decides whether the subcommand exists for the user at all. If the user declines it, the
subcommand is hidden from help and cannot be run. The grant is server-side only, and it takes no
argument: the subcommand name belongs in the `cli` contribution.

**You will need:**

- the `cli.contribute` grant on the `server` side of the manifest, with no argument;
- a `cli` object under `contributes` that names the subcommand;
- a `server` component that exports `handler`;
- Rust with `ilium_plugin_sdk` 0.1, or TypeScript with `@ilium/plugin-sdk`. Create a project with
  `ilium plugin new notes --id com.example.notes --lang rust --side server`.

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `cli.contribute` | yes | the only form |
| `cli.contribute:notes` | no | refused at install with `manifest.grant-argument` |
| `cli:contribute` | no | the separator is a dot; refused with `manifest.grant-unknown` |

The install error for the first row reads
`capability "cli.contribute" takes no argument (got "cli.contribute:notes")`.

### Manifest

A plugin declares its subcommands in the `cli` key. The key holds one object, or a list of at most 8 objects when the plugin owns several commands (the fleet plugin owns `panes`, `broadcast` and `new-pane`). Names MUST be unique inside the plugin:

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.notes",
  "capabilities": { "server": ["cli.contribute"] },
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": []
  },
  "contributes": {
    "cli": [{
      "name": "notes",
      "summary": "List notes",
      "arguments_schema": { "type": "object", "properties": { "query": { "type": "string" } } },
      "output": "jsonl"
    }]
  }
}
```

| Key | Type | Meaning |
|---|---|---|
| `name` | string | The subcommand. It must match `[a-z][a-z0-9-]{0,31}`. It runs as `ilium <name>`. |
| `summary` | string | One line shown in `ilium --help`. |
| `arguments_schema` | JSON Schema object | Documents the arguments for help. The host passes `argv` unchanged and does not validate it. |
| `output` | `"jsonl"`, `"text"` or `"mixed"` | The output form. `mixed` allows both `write-line` and `write-text` (used by `new-pane`). |
| `aliases` | list of strings | Further names for the same command, for example `wait` for `progress wait`. Same name rule. |

A name that a kernel command uses is refused at install with `manifest.cli-name-reserved`; a name
that another installed plugin owns is refused with `manifest.cli-name-taken`. A plugin starts for the subcommand on its first use when its activation
rule is `cli:<name>`, or on first use of any contribution when no activation rule is set.

### Surface

The import interface is `ilium:host/cli@0.1.0`. In Rust it is `ilium_plugin_sdk::host::cli`, and in
TypeScript it is `@ilium/plugin-sdk/host/cli`.

| Function | Rust | TypeScript | Meaning | Errors beyond the universal set |
|---|---|---|---|---|
| `write-line` | `cli::write_line(invocation: u64, jsonl: &str) -> Result<(), PluginError>` | `writeLine(invocation: bigint, jsonl: string): void` | Writes one JSON object and a newline to the caller's standard output. | `not-found` (finished or cancelled), `invalid-argument` (not one object with a string `type`, a raw newline, or over 64 KiB) |
| `write-text` | `cli::write_text(invocation: u64, text: &str) -> Result<(), PluginError>` | `writeText(invocation: bigint, text: string): void` | Writes plain text to the caller's standard output. Allowed only when the subcommand's `output` is `"text"` or `"mixed"`. | `not-found`, `not-permitted` (output form is `"jsonl"`), `invalid-argument` (over 64 KiB) |
| `write-error` | `cli::write_error(invocation: u64, text: &str) -> Result<(), PluginError>` | `writeError(invocation: bigint, text: string): void` | Writes plain text and a newline to standard error. | `not-found`, `invalid-argument` (over 64 KiB) |
| `finish` | `cli::finish(invocation: u64, exit_code: u8)` | `finish(invocation: bigint, exitCode: number): void` | Ends the invocation. `exit_code` becomes the process exit status. | none; an unknown invocation is ignored |

In TypeScript a failed `writeLine` or `writeError` throws a `PluginError`. The universal codes
`not-permitted`, `revoked` and `internal` can also appear (see
[errors and limits](/pages/reference-errors-and-limits/)).

| Event | Payload | Rust | TypeScript |
|---|---|---|---|
| `cli-invoked` | `invocation` (u64), `name` (string), `argv` (list of strings), `cwd` (string) | `Event::CliInvoked(CliInvokedEvent { invocation, name, argv, cwd })` | `{ tag: 'cli-invoked', val: { invocation, name, argv, cwd } }` |
| `cli-cancelled` | `invocation` (u64) | `Event::CliCancelled(u64)` | `{ tag: 'cli-cancelled', val: bigint }` |

`name` is the subcommand name. `argv` holds the words after the name, exactly as the shell passed
them. `cwd` is the absolute path of the caller's working directory. Invocation ids are assigned by
the host and are unique for the life of the server. Both events are never dropped.

The WIT form of the three imports is:

```wit
// Excerpt of ilium:host/cli in reference-wit
// cli (cli.contribute)
write-line: func(invocation: u64, jsonl: string) -> result<_, plugin-error>;
write-error: func(invocation: u64, text: string) -> result<_, plugin-error>;
finish: func(invocation: u64, exit-code: u8);
```

### How an invocation runs

<figure>
<svg viewBox="0 0 400 218" role="img" aria-label="Grant outcome: a granted subcommand runs, a declined subcommand is hidden">
  <defs>
    <marker id="cli-cap-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="60" y="8" width="280" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="29" font-size="12" text-anchor="middle" fill="currentColor">Manifest declares cli and cli.contribute</text>
  <line x1="200" y1="42" x2="200" y2="60" stroke="currentColor" marker-end="url(#cli-cap-arrow)"/>
  <rect x="60" y="60" width="280" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="81" font-size="12" text-anchor="middle" fill="currentColor">User checks or declines the grant</text>
  <line x1="170" y1="94" x2="100" y2="116" stroke="currentColor" marker-end="url(#cli-cap-arrow)"/>
  <line x1="230" y1="94" x2="300" y2="116" stroke="currentColor" marker-end="url(#cli-cap-arrow)"/>
  <rect x="10" y="116" width="180" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="100" y="137" font-size="12" text-anchor="middle" fill="currentColor">Granted: listed in help</text>
  <rect x="210" y="116" width="180" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="300" y="137" font-size="12" text-anchor="middle" fill="currentColor">Declined: hidden from help</text>
  <line x1="100" y1="150" x2="100" y2="172" stroke="currentColor" marker-end="url(#cli-cap-arrow)"/>
  <line x1="300" y1="150" x2="300" y2="172" stroke="currentColor" marker-end="url(#cli-cap-arrow)"/>
  <rect x="10" y="172" width="180" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="100" y="193" font-size="12" text-anchor="middle" fill="currentColor">cli-invoked reaches plugin</text>
  <rect x="210" y="172" width="180" height="34" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="300" y="193" font-size="12" text-anchor="middle" fill="currentColor">Fails: unknown command</text>
</svg>
<figcaption>The grant decides at install whether the subcommand exists. A declined subcommand never
reaches the plugin.</figcaption>
</figure>

Once the command runs, the plugin writes its output and calls `finish`:

1. The host sends `cli-invoked` with the invocation id, the name, the arguments and the working
   directory.
2. The plugin calls `write-line` once per JSON object. Each line is written as it arrives.
3. The plugin calls `finish` with the exit code. The invocation closes.

The invocation has no deadline of its own. It stays open until `finish`, or until the caller
disconnects, which sends `cli-cancelled`. Several invocations can run at once, and each has its own
id. Keep the handler short, because it runs inside the plugin's event deadline. Start slow work in
a task and write from it.

### Risk, review and revocation

- **Risk:** normal. The capability table does not mark the grant as high risk.
- **Tier T1 (bundled):** granted automatically up to the manifest ceiling.
- **Tier T2 and T3:** the review dialog lists the grant as a checked row, and the user can uncheck
  it before installing. The row text is `Adds commands to the ilium program`.
- **Other grants:** the subcommand runs inside the plugin, so the plugin's other grants still
  apply to anything the subcommand reads or writes.
- **Revoke:** in Settings > Plugins > (plugin) > Capabilities, or with
  `ilium plugin grants com.example.notes --revoke cli.contribute`. The subcommand is withdrawn at
  once. Calls already in flight return `revoked`.

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.cli 0.1.0 by Example Ltd                                       │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Adds commands to the ilium program <span class="t-dim">……………………………………………………</span> cli.contribute │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests cli.contribute, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

### Worked example in Rust

This plugin answers `ilium notes` with one JSON line per note. It uses two sample titles. A real
plugin reads its notes with `storage` or with `fs.read`, which need their own grants.

**Rust**

```rust
use ilium_plugin_sdk::host::cli;
use ilium_plugin_sdk::prelude::*;

const SAMPLE_NOTES: [&str; 2] = ["Weekly plan", "Release checklist"];

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct NotesCommand;

impl Lifecycle for NotesCommand {}

impl Handler for NotesCommand {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::CliInvoked(invoked) = event else {
                continue;
            };
            if invoked.name == "notes" {
                list_notes(invoked.invocation);
            }
        }
        Vec::new()
    }
}

fn list_notes(invocation: u64) {
    for title in SAMPLE_NOTES {
        let line = format!(r#"{{"type":"note","title":"{title}"}}"#);
        if let Err(error) = cli::write_line(invocation, &line) {
            let _ = cli::write_error(invocation, &error.message);
            cli::finish(invocation, 1);
            return;
        }
    }
    cli::finish(invocation, 0);
}
```

### Worked example in TypeScript

The same behaviour in TypeScript, with the manifest shown above. `JSON.stringify` does the
escaping, and `finish` is called on both the success and the error path.

**TypeScript**

```ts
import { definePlugin, PluginError, type Event, type Submission } from '@ilium/plugin-sdk';
import { finish, writeError, writeLine } from '@ilium/plugin-sdk/host/cli';

const SAMPLE_NOTES = ['Weekly plan', 'Release checklist'];

function listNotes(invocation: bigint): void {
  try {
    for (const title of SAMPLE_NOTES) {
      writeLine(invocation, JSON.stringify({ type: 'note', title }));
    }
    finish(invocation, 0);
  } catch (error) {
    const message = error instanceof PluginError ? error.message : String(error);
    writeError(invocation, message);
    finish(invocation, 1);
  }
}

export const { handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'cli-invoked' && event.val.name === 'notes') {
        listNotes(event.val.invocation);
      }
    }
    return [];
  },
});
```

### What the user sees

With the grant granted, the subcommand appears in help and prints one JSON object per line:

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium --help
Commands:
  <span class="t-hi">notes</span>    List notes
<span class="t-prompt">$</span> ilium notes
<span class="t-dim">{"type":"note","title":"Weekly plan"}</span>
<span class="t-dim">{"type":"note","title":"Release checklist"}</span>
<span class="t-ok">exit 0</span>
</pre>
<figcaption>Illustrative, not generated. A granted subcommand, from help to a successful
run.</figcaption>
</figure>

With the grant declined, the subcommand is hidden and cannot run. The grant list still shows it:

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium notes
<span class="t-err">error</span>: unknown command
<span class="t-prompt">$</span> ilium plugin grants com.example.notes
  server  cli.contribute  <span class="t-warn">declined</span>
</pre>
<figcaption>Illustrative, not generated. A declined subcommand and its grant state.</figcaption>
</figure>

### Edge cases and errors

| Situation | Result |
|---|---|
| Grant declined at install | The subcommand is hidden from `ilium --help`. `ilium <name>` fails with `unknown command`. No `cli-invoked` arrives. |
| Grant revoked while running | In-flight calls return `revoked`. The plugin is re-activated with the reduced grant set. An open invocation ends with exit code 1 and `grant revoked` on standard error. |
| Grant with an argument | Install refused with `manifest.grant-argument`. |
| Unknown grant spelling | Install refused with `manifest.grant-unknown`. |
| Name outside `[a-z][a-z0-9-]{0,31}` | Install refused. |
| Name used by a kernel command | Install refused with `manifest.cli-name-reserved`. |
| Name owned by another installed plugin | Install refused with `manifest.cli-name-taken`. |
| `write-line` text is not one JSON object with a string `type` | `invalid-argument`. The line is not written. |
| `write-line` text contains a raw newline | `invalid-argument`. The host adds the newline itself. |
| `write-line` or `write-error` text over 64 KiB | `invalid-argument`. |
| `write-line` or `write-error` after `finish` or after cancel | `not-found`. |
| `finish` for an unknown or finished invocation | Ignored. |
| Caller disconnects early | `cli-cancelled` arrives. Later writes return `not-found`. |
| Plugin restarts, is disabled or is uninstalled while open | The invocation ends with exit code 1 and `plugin stopped` on standard error. |

<div class="warn">
<p>A plugin that never calls <code>finish</code> leaves the caller waiting, with no deadline.
Call <code>finish</code> on every path, including the error path.</p>
</div>

<div class="note">
<p>The host does not check <code>argv</code> against <code>arguments_schema</code>. The schema
only documents the arguments for help, so the plugin validates them itself.</p>
</div>

### Used by

First-party plugins from [feature traceability](/pages/reference-feature-traceability/) that hold this grant:

- 52 `ilium voice say`, which sends a `message` to the plugin's own client side.
- 60 `ilium progress`, which writes its status with `write-line` and `finish`.
- 62 Chatroom, which adds its own CLI commands.
- 64 Context-injection hook, which reads its read position from `storage` when `cli-invoked`
  arrives.
- 109 `ilium panes`.
- 110 `ilium broadcast`.

### See also

- [api-cli](/pages/api-cli/) for the full import reference and the Rust and TypeScript SDK paths.
- [capability-http-api-route](/pages/capability-http-api-route/) for the same kind of answer over
  the loopback HTTP API.
- [capability-fs-read](/pages/capability-fs-read/) for commands that read project files.
- [capabilities](/pages/capabilities/) for the grant model, risk tiers and revocation.
- [getting started with a first plugin](/pages/getting-started-first-plugin/) for a complete
  project.

Interface version: `ilium:host/cli@0.1.0`.
