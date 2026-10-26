# sessions

## sessions

The `sessions` import lets a server-side plugin see the other Ilium sessions that run on the same machine and run one command of its own in another session. Ilium keeps one server per project session, so a plugin in one session knows nothing about the others by default. This import closes that gap for commands such as a message to every agent in every session.

The import is deliberately narrow. A plugin can talk only to the same plugin in another session, and only to a command that plugin declared. It can never reach a kernel command of another session. The grant is high risk.

<figure>
<svg viewBox="0 0 440 250" role="img" aria-label="A plugin sends a command to the same plugin in another session">
  <defs>
    <marker id="api-sessions-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="10" width="200" height="140" rx="8" fill="none" stroke="currentColor"/>
  <text x="110" y="28" font-size="12" text-anchor="middle" fill="currentColor">Session work (this one)</text>
  <rect x="30" y="44" width="160" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="110" y="65" font-size="11" text-anchor="middle" fill="currentColor">Plugin sync, caller</text>
  <rect x="30" y="98" width="160" height="34" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="110" y="119" font-size="11" text-anchor="middle" fill="currentColor">Kernel session router</text>
  <line x1="110" y1="78" x2="110" y2="96" stroke="currentColor" marker-end="url(#api-sessions-arrow)"/>
  <rect x="230" y="10" width="200" height="140" rx="8" fill="none" stroke="currentColor"/>
  <text x="330" y="28" font-size="12" text-anchor="middle" fill="currentColor">Session notes (target)</text>
  <rect x="250" y="44" width="160" height="34" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="330" y="65" font-size="11" text-anchor="middle" fill="currentColor">Kernel session router</text>
  <rect x="250" y="98" width="160" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="330" y="119" font-size="11" text-anchor="middle" fill="currentColor">Plugin sync, command</text>
  <line x1="190" y1="115" x2="248" y2="61" stroke="currentColor" marker-end="url(#api-sessions-arrow)"/>
  <text x="228" y="100" font-size="10" text-anchor="middle" fill="currentColor">send</text>
  <line x1="330" y1="78" x2="330" y2="96" stroke="currentColor" marker-end="url(#api-sessions-arrow)"/>
  <text x="10" y="176" font-size="11" fill="currentColor">1. list-live shows the running sessions of this user.</text>
  <text x="10" y="194" font-size="11" fill="currentColor">2. send names a session and a command of the same plugin.</text>
  <text x="10" y="212" font-size="11" fill="currentColor">3. The target runs it. The caller takes the reply text later.</text>
  <text x="10" y="236" font-size="11" fill="currentColor">Kernel commands of the target are never reachable.</text>
</svg>
<figcaption>The caller and the target run the same plugin. The kernel routes the request between the two session servers and delivers the reply as a completion.</figcaption>
</figure>

### What you need

- **Grant:** `sessions.cross`, server side only, no argument. High risk (see [capabilities](/pages/capabilities/)). The grant dialog shows it in the warning style, even for a bundled tier 1 plugin.
- **Both sessions:** the same plugin must be installed and enabled in the target session, with `sessions.cross` granted there too.
- **Tier:** the grant is limited to tier 1 and signed tier 2 plugins. An unsigned tier 3 plugin cannot hold it.
- **Language:** Rust with `ilium-plugin-sdk`, or TypeScript with `@ilium/plugin-sdk`. Imports: `ilium_plugin_sdk::host::sessions` and `@ilium/plugin-sdk/host/sessions`.

### Surface

```wit
// Excerpt of ilium:host/sessions in reference-wit
// ilium:host/sessions  (sessions.cross, server side)
record live-session {
  name: string,            // the session name, "default" for a bare ilium
  project: string,         // the canonical project directory
  socket-id: string,       // the kernel's identity of the session socket
  panes: u32,
  attached-clients: u32,
  is-self: bool,           // true for the session that runs this plugin instance
}
list-live: func() -> list<live-session>;

record cross-request { session: string, command: string, arguments-json: string }
send: func(request: cross-request) -> result<request-id, plugin-error>;
take-reply: func(request: request-id) -> option<string>;
```

| WIT function | Rust | TypeScript | Meaning |
|---|---|---|---|
| `list-live` | `sessions::list_live() -> Vec<LiveSession>` | `listLive(): LiveSession[]` | The user's running Ilium sessions on this machine. Other users' sockets are never visible. Synchronous. |
| `send` | `sessions::send(request: CrossRequest) -> Result<RequestId, PluginError>` | `send(request: CrossRequest): bigint` | Runs one command of the same plugin in another session. Asynchronous. The reply arrives as `request-completed`. |
| `take-reply` | `sessions::take_reply(request: RequestId) -> Option<String>` | `takeReply(request: bigint): string \| undefined` | Takes the reply text, once. The reply is whatever the target command returned. |

#### Records

| Record | Rust | TypeScript |
|---|---|---|
| `live-session` | `LiveSession { name: String, project: String, socket_id: String, panes: u32, attached_clients: u32, is_self: bool }` | `{ name: string; project: string; socketId: string; panes: number; attachedClients: number; isSelf: boolean }` |
| `cross-request` | `CrossRequest { session: String, command: String, arguments_json: String }` | `{ session: string; command: string; argumentsJson: string }` |

`session` in a `cross-request` is the `name` of a `live-session`. `command` is the id of a command the plugin declared in `contributes.commands`. `arguments-json` is a JSON text passed to that command as its arguments. The request carries the caller's plugin id and the caller's session name, so the target command can tell where a call came from.

### How a call runs

1. The caller calls `list-live` and picks a session whose `is-self` is `false`.
2. The caller calls `send`. The kernel checks the grant, finds the session, and checks the target.
3. The target kernel runs the command in its copy of the same plugin. The request carries the caller's plugin id and session name.
4. The target returns a reply text. The caller's kernel completes the request, and the caller takes the reply with `take-reply`.

The target must have the plugin installed and enabled with `sessions.cross` granted, and the command must be declared by that plugin. If any of these fail, `send` returns `not-permitted`. Kernel commands are never reachable (D16).

### Worked example: ask every other session for its pane count

A plugin declares a command `pane-count` that returns the number of panes in its session as JSON. A second command, `count-everywhere`, asks all other sessions and sums the replies. This is the pattern of a message to all agents in all sessions, reduced to a count.

Manifest:

```json
{
  "capabilities": { "server": ["sessions.cross", "cli.contribute"] },
  "contributes": {
    "commands": [{ "id": "pane-count", "title": "Count panes" }],
    "cli": { "name": "count-everywhere", "summary": "Count panes in every session" }
  }
}
```

**Rust**

```rust
use std::collections::HashMap;

use ilium_plugin_sdk::host::{cli, sessions};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::sessions::CrossRequest;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct CountEverywhere {
    /// Request id to (invocation, session name).
    waiting: HashMap<RequestId, (u64, String)>,
    /// Invocation to the number of replies still expected and the sum so far.
    totals: HashMap<u64, (usize, u64)>,
}

impl Lifecycle for CountEverywhere {}

impl CountEverywhere {
    fn start(&mut self, invocation: u64) {
        let others: Vec<_> = sessions::list_live().into_iter().filter(|s| !s.is_self).collect();
        if others.is_empty() {
            let _ = cli::write_line(invocation, r#"{"type":"summary","sessions":0,"panes":0}"#);
            cli::finish(invocation, 0);
            return;
        }
        let mut sent = 0;
        for session in &others {
            let request = CrossRequest {
                session: session.name.clone(),
                command: "pane-count".into(),
                arguments_json: "{}".into(),
            };
            match sessions::send(request) {
                Ok(id) => {
                    self.waiting.insert(id, (invocation, session.name.clone()));
                    sent += 1;
                }
                Err(error) => {
                    let line = format!(
                        r#"{{"type":"warning","session":"{}","code":"{:?}"}}"#,
                        session.name, error.code
                    );
                    let _ = cli::write_line(invocation, &line);
                }
            }
        }
        self.totals.insert(invocation, (sent, 0));
        if sent == 0 {
            cli::finish(invocation, 1);
        }
    }

    fn on_reply(&mut self, id: RequestId, is_ok: bool) {
        let Some((invocation, name)) = self.waiting.remove(&id) else { return };
        let panes = sessions::take_reply(id)
            .filter(|_| is_ok)
            .and_then(|text| text.trim().parse::<u64>().ok());
        let Some((remaining, sum)) = self.totals.get_mut(&invocation) else { return };
        *remaining -= 1;
        match panes {
            Some(count) => {
                *sum += count;
                let line = format!(r#"{{"type":"session","name":"{name}","panes":{count}}}"#);
                let _ = cli::write_line(invocation, &line);
            }
            None => {
                let line = format!(r#"{{"type":"warning","session":"{name}","code":"no-reply"}}"#);
                let _ = cli::write_line(invocation, &line);
            }
        }
        if *remaining == 0 {
            let line = format!(r#"{{"type":"summary","panes":{}}}"#, *sum);
            let _ = cli::write_line(invocation, &line);
            cli::finish(invocation, 0);
            self.totals.remove(&invocation);
        }
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import * as cli from '@ilium/plugin-sdk/host/cli';
import * as sessions from '@ilium/plugin-sdk/host/sessions';

const waiting = new Map<bigint, { invocation: bigint; name: string }>();
const totals = new Map<bigint, { remaining: number; sum: number }>();

function start(invocation: bigint): void {
  const others = sessions.listLive().filter((session) => !session.isSelf);
  if (others.length === 0) {
    cli.writeLine(invocation, JSON.stringify({ type: 'summary', sessions: 0, panes: 0 }));
    cli.finish(invocation, 0);
    return;
  }
  let sent = 0;
  for (const session of others) {
    try {
      const id = sessions.send({ session: session.name, command: 'pane-count', argumentsJson: '{}' });
      waiting.set(id, { invocation, name: session.name });
      sent += 1;
    } catch (error) {
      const code = error instanceof Error ? error.name : 'error';
      cli.writeLine(invocation, JSON.stringify({ type: 'warning', session: session.name, code }));
    }
  }
  totals.set(invocation, { remaining: sent, sum: 0 });
  if (sent === 0) cli.finish(invocation, 1);
}

function onReply(id: bigint, isOk: boolean): void {
  const entry = waiting.get(id);
  if (entry === undefined) return;
  waiting.delete(id);
  const total = totals.get(entry.invocation);
  if (total === undefined) return;
  total.remaining -= 1;
  const text = isOk ? sessions.takeReply(id) : undefined;
  const panes = text === undefined ? Number.NaN : Number.parseInt(text.trim(), 10);
  if (Number.isNaN(panes)) {
    cli.writeLine(entry.invocation, JSON.stringify({ type: 'warning', session: entry.name, code: 'no-reply' }));
  } else {
    total.sum += panes;
    cli.writeLine(entry.invocation, JSON.stringify({ type: 'session', name: entry.name, panes }));
  }
  if (total.remaining === 0) {
    cli.writeLine(entry.invocation, JSON.stringify({ type: 'summary', panes: total.sum }));
    cli.finish(entry.invocation, 0);
    totals.delete(entry.invocation);
  }
}

export const { handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'cli-invoked' && event.val.name === 'count-everywhere') start(event.val.invocation);
      else if (event.tag === 'request-completed') onReply(event.val.request, event.val.outcome.tag === 'ok');
    }
    return [];
  },
});
```

The `pane-count` command in the target session answers from its own tree and returns the number as text. Its handler is an ordinary command handler and needs no special code to be reachable from another session, because the request carries the caller's plugin id and session name.

### What the user sees

The user runs the plugin's own CLI command. The plugin writes one JSON line per session and a summary. This is the same line shape that `ilium broadcast` prints in the repository (types `progress`, `result`, `warning` and `summary`).

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium count-everywhere
<span class="t-dim">{"type":"session","name":"default","panes":5}</span>
<span class="t-dim">{"type":"session","name":"docs","panes":3}</span>
<span class="t-warn">{"type":"warning","session":"build","code":"NotPermitted"}</span>
<span class="t-dim">{"type":"summary","panes":8}</span>
<span class="t-prompt">$</span> echo $?
0
</pre>
<figcaption>The plugin asks three other sessions. The session named build has no matching plugin enabled, so the send is refused with not-permitted (the Rust example prints the code with its debug name). Illustrative, not generated.</figcaption>
</figure>

### Error codes

| Function | Code | When |
|---|---|---|
| `list-live` | none | Returns an empty list when only this session runs. |
| `send` | `not-permitted` | The grant is missing here, the target lacks the plugin or the grant, the target plugin is disabled, or the command is not declared by the plugin. |
| `send` | `not-found` | No live session has the given name. |
| completion | `timed-out` | The target did not answer in time. |
| completion | `network` | The target session's socket closed before the reply. |
| `take-reply` | none | Returns `none` when the reply was already taken, or when the request failed. |

### Limits

| Item | Value |
|---|---|
| Reply hold time | 30 s, or until taken once, as for every `take-*` function |
| Sessions visible | Only the sessions of the current user on this machine |
| Reach | The same plugin id, commands declared in `contributes.commands` only |
| Tier | Tier 1 and signed tier 2 |

### Edge cases

| Situation | Result |
|---|---|
| The target session stops while the request runs | The request completes as `failed(network)`. |
| The target does not declare the command | `send` returns `not-permitted`. |
| A plugin lists a session it cannot reach | The session is listed. Only `send` reveals whether the plugin is enabled there. |
| Many `send` calls in one batch | Each gets its own request id. Replies can arrive in any order. |

<div class="warn">
<p>A plugin that holds <code>sessions.cross</code> can run its own commands in every session of the user. Declare only commands that are safe to run from another session, and validate <code>arguments-json</code> in the target as for any untrusted input.</p>
</div>

### See also

- [cli](/pages/api-cli/) for the commands that usually start a cross-session call.
- [Requests and outcomes](/pages/requests-and-outcomes/) for `request-completed` and the `take-*` pattern.
- [commands](/pages/api-commands/) for declaring the command that the target runs.
- [Capabilities](/pages/capabilities/) for risk levels and the grant dialog.
- [Errors and limits](/pages/errors-and-limits/) for the common error codes.

Interface version: `ilium:host/sessions`, version 0.1.
