# commands.invoke

## Overview

The `commands.invoke` grant lets a plugin run commands that belong to other plugins, or to the kernel, by their global id. A plugin can start a git status refresh, a compaction or a voice command without knowing how those features work inside. You name the command by its id, and the kernel runs it.

Every call passes one check. The kernel compares the global id of the command with the pattern in your grant. A match runs the command. Anything else returns `not-permitted`, and the command does not run. The grant reaches only the ids that its pattern matches, so a narrow pattern keeps the reach small. The pattern `*` reaches every command of every plugin. It is a high-risk grant.

Your own commands need no grant. You invoke them without the check, and their invocations arrive as `command-invoked` events.

<figure>
<svg viewBox="0 0 400 230" role="img" aria-label="A caller plugin sends invoke to the kernel grant check, which runs a matching command or returns not-permitted">
  <defs>
    <marker id="arrow-invoke" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="20" y="10" width="360" height="50" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="200" y="30" text-anchor="middle" fill="currentColor" font-size="12">Caller plugin</text>
  <text x="200" y="48" text-anchor="middle" fill="currentColor" font-size="10">holds commands.invoke:ilium.git/*</text>
  <line x1="200" y1="60" x2="200" y2="84" stroke="currentColor" marker-end="url(#arrow-invoke)"/>
  <text x="210" y="76" fill="currentColor" font-size="10">invoke(command, arguments-json)</text>
  <rect x="20" y="84" width="360" height="50" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="200" y="104" text-anchor="middle" fill="currentColor" font-size="12">Kernel grant check</text>
  <text x="200" y="122" text-anchor="middle" fill="currentColor" font-size="10">does the pattern match the global id?</text>
  <line x1="110" y1="134" x2="110" y2="166" stroke="currentColor" marker-end="url(#arrow-invoke)"/>
  <text x="118" y="154" fill="currentColor" font-size="10">match</text>
  <line x1="290" y1="134" x2="290" y2="166" stroke="currentColor" marker-end="url(#arrow-invoke)"/>
  <text x="282" y="154" text-anchor="end" fill="currentColor" font-size="10">no match</text>
  <rect x="20" y="166" width="170" height="52" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="105" y="186" text-anchor="middle" fill="currentColor" font-size="10">ilium.git/status runs</text>
  <text x="105" y="204" text-anchor="middle" fill="currentColor" font-size="10">and returns its outcome</text>
  <rect x="210" y="166" width="170" height="52" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="295" y="186" text-anchor="middle" fill="currentColor" font-size="11">not-permitted</text>
  <text x="295" y="204" text-anchor="middle" fill="currentColor" font-size="10">the command does not run</text>
</svg>
<figcaption>A call that matches the granted pattern runs the command. Any other call returns not-permitted, and the command does not run.</figcaption>
</figure>

## You will need

- A grant `commands.invoke:<pattern>` on every side that makes the call. The `invoke` call exists on the server side and on the client side. A client handler needs the grant in `capabilities.client`, and a server handler needs it in `capabilities.server`.
- The global id of each command you call. A global id is the plugin id, a slash and the local id, for example `ilium.git/status`.
- Nothing else for your own commands. Only commands of other plugins, and kernel commands, need the grant.

## Grant syntax

The first colon in the grant separates the name from the pattern, and the pattern is required. A `*` is a whole-segment wildcard. It never crosses the slash between the plugin id and the local id, so `ilium.git/*` covers the local ids of one plugin and nothing else.

| Pattern | Matches | Result at install |
|---|---|---|
| `ilium.git/status` | exactly one command | valid |
| `ilium.git/*` | every command of the git plugin | valid |
| `*` | every command of every plugin | valid, high risk (H) |
| `ilium.git*` | a wildcard inside a word | rejected |
| `ilium.git/prefix*` | a wildcard inside the local id | rejected |
| `*/status` | a wildcard in the plugin id | rejected |
| `commands.invoke` | no pattern at all | rejected |

## Surface

The grant unlocks five call forms and one intent. The Rust and TypeScript wrappers wait for the outcome, so most plugins use them. The raw forms return the request id at once, and the outcome arrives later as an event.

| Form | Side | Signature | Returns |
|---|---|---|---|
| WIT `invoke` | server, client | `invoke(command: string, arguments-json: string) -> result<request-id, plugin-error>` | A request id. The outcome arrives later as `request-completed`. |
| Rust wrapper | server, client | `commands::invoke(command, arguments_json).await` | `Result<(), PluginError>`, once the command has finished |
| Rust raw form | server, client | `commands::invoke_request(command, arguments_json)` | `Result<RequestId, PluginError>` |
| TypeScript wrapper | server, client | `invoke(command, argumentsJson)`, from `@ilium/plugin-sdk/host/commands` | `Promise<void>`, which rejects with the plugin error |
| TypeScript raw form | server, client | `invokeRequest(command, argumentsJson)` | `Promise<bigint>`, the request id |
| Intent `invoke-command` | server, client | Fields `command` and `arguments-json` in a submission | `ok` once the command has run |

Every form takes the same two arguments.

| Argument | Type | Rule |
|---|---|---|
| `command` | string | An exact global id. Patterns are not accepted here. |
| `arguments-json` | string | One JSON object, at most 64 KiB. The target's `arguments_schema` checks it when the target declares one. Without a schema, any object is accepted. |

The outcome of a raw call arrives as `request-completed`. Its `request-ref` is `call(<request-id>)`, and its outcome is one of these:

| Outcome | Meaning |
|---|---|
| `ok` | The command ran and succeeded. |
| `failed(code)` | The command reported its own failure, or the call timed out. |
| `refused(code)` | The kernel refused the call before the command ran. |
| `cancelled` | The call was cancelled before it ran. |

A command returns an outcome only, `ok` or `failed`. It cannot return data. If you need a value from another plugin, use a service (see [api-services](/pages/api-services/)).

Your own commands arrive as `command-invoked` events, with an `origin` of `keybinding`, `palette`, `toolbar`, `menu`, `voice` or `cli`. See [api-events](/pages/api-events/) for the event record.

## Worked example in Rust

The client handler below refreshes git status when the user runs one of its own commands. It needs the grant `commands.invoke:ilium.git/status` in `capabilities.client`.

**Rust**

```rust
use ilium_plugin_sdk::host::commands;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::task;

// Manifest, client side: "commands.invoke:ilium.git/status"
const GIT_STATUS: &str = "ilium.git/status";

// Starts the git status command. The call returns at once. The task waits for the
// outcome, so the event handler never blocks.
fn refresh_git_status() {
    task::spawn(async {
        match commands::invoke(GIT_STATUS, "{}").await {
            Ok(()) => log::log(Level::Info, "git status refreshed"),
            Err(error) => {
                log::log(Level::Warn, &format!("git status failed: {}", error.message));
            }
        }
    });
}
```

## Worked example in TypeScript

The same action in TypeScript. Any rejection counts as a failed refresh, and the handler logs the reason. The arguments are an empty JSON object because the status command takes none.

**TypeScript**

```ts
import { invoke } from '@ilium/plugin-sdk/host/commands';
import { log } from '@ilium/plugin-sdk/host/log';

const GIT_STATUS = 'ilium.git/status';

// Returns true when the git plugin ran the status command.
async function refreshGitStatus(): Promise<boolean> {
  try {
    await invoke(GIT_STATUS, '{}');
    log('info', 'git status refreshed');
    return true;
  } catch (error) {
    log('warn', `git status failed: ${String(error)}`);
    return false;
  }
}
```

A command that takes arguments needs a JSON object that matches its schema, for example `JSON.stringify({ target: 'line' })`. Build the string with `JSON.stringify` rather than by hand, so the quoting stays valid.

## Edge cases and errors

Each call returns one of these codes, either at once or as the outcome of the request.

| Code | When it happens | What to do |
|---|---|---|
| `not-permitted` | The pattern does not cover the id, the grant was declined or revoked, or the plugin lacks the grant on this side. | Do not retry. Hide the action. A grant change deactivates the plugin with reason `revoked`. Read the `granted` list on the next activation before you show the action again. |
| `not-found` | No command has that id. | Check the id against the owning plugin's manifest. |
| `conflict` | The command is disabled by its owner. The `invoke-command` intent gives `refused(conflict)`. | Retry after the owner enables it. |
| `invalid-argument` | The arguments are not one JSON object, are over 64 KiB, or fail the target's schema. | Fix the call. Do not retry it unchanged. |
| `timed-out` | The command did not reply within 30 s. The outcome is `failed(timed-out)`. | Retry later. The command may still finish, so make the action idempotent. |
| `busy` | The plugin's pending request limit is full. | Retry after a short delay. See [errors-and-limits](/pages/reference-errors-and-limits/) for the limits. |
| `quota-exceeded` | The plugin's CPU budget is spent. | Retry after `retry-after-ms`. |

Three more rules apply:

- The `when` expression of a command only shows or hides its palette, menu and keybinding entries. It never blocks `commands.invoke`.
- A refused call writes a refusal line to the plugin log. The line has the form that [reference-capabilities](/pages/reference-capabilities/) describes.
- Invoking a disabled command is not an error in the plugin. The call returns `conflict`, and the plugin decides what to show.

<div class="warn">
<p>Do not use the pattern <code>*</code> to save work. It grants every command of every plugin, including commands that change git state or run agents. Use <code>ilium.git/*</code> or a single id.</p>
</div>

## What the user sees

At install, the review dialog lists each grant with its pattern as written, so the user can judge its width before approving it. A T2 or T3 plugin's `commands.invoke` grant is listed and checked by default, and the user can uncheck it. A T1 bundled plugin receives a narrow pattern up to its manifest ceiling. The bare `*` pattern is unchecked by default, and a T1 plugin asks for it once, at first activation.

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.commands 0.1.0 by Example Ltd                                  │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Runs commands of ilium.git <span class="t-dim">………………………………………</span> commands.invoke:ilium.git/* │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests commands.invoke:ilium.git/*, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

When a call is refused, the palette shows nothing new. The refusal goes to the plugin log, which the user reads with `ilium plugin logs`.

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs example-notes
<span class="t-warn">warn</span> 2026-10-11 09:14:02 invoke refused, command outside grant
{"type":"refusal","grant":"commands.invoke:ilium.compaction/compact",
 "function":"commands.invoke","reason":"not-granted"}
</pre>

Illustrative, not generated. The refusal line has the same shape as the other refusal lines in the plugin log.

## See also

- [api-commands](/pages/api-commands/) for declaring and handling your own commands, and for `set-state`.
- [api-intents](/pages/api-intents/) for submitting the `invoke-command` intent in a batch.
- [api-events](/pages/api-events/) for `request-completed` and `command-invoked`.
- [capability-agent-control](/pages/capability-agent-control/) for sending an agent its own action keys, which is a different route to the same goal.
- [capability-ui-panels](/pages/capability-ui-panels/) for the commands that open views.
- [reference-capabilities](/pages/reference-capabilities/) for every grant and its risk tier.

Checked against the API version `ilium:plugin@0.1.0`.
