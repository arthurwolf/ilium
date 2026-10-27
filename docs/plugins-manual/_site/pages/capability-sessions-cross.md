# Capability: sessions.cross

## sessions.cross

The `sessions.cross` capability lets a server-side plugin list the user's other running Ilium sessions on the same machine and run one declared command of the same plugin in another session. The target session must also have the plugin installed and enabled with `sessions.cross` granted. It is high risk (**H**) because it lets a plugin act across project boundaries.

**You will need:**

- A `sessions.cross` grant on the server side (explicit, high risk).
- The same plugin installed and enabled in the target session, with the grant.
- A command declared by the plugin in `contributes.commands`: kernel commands of another session are never reachable (D16).

<figure>
<svg viewBox="0 0 420 252" role="img" aria-label="A plugin lists live sessions and sends one declared command to another session">
  <defs><marker id="arr" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="27" text-anchor="middle" fill="currentColor" font-size="12">Plugin calls sessions.list-live()</text>
  <text x="210" y="43" text-anchor="middle" fill="currentColor" font-size="10">only the user's own sockets are visible</text>
  <line x1="210" y1="52" x2="210" y2="72" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="72" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="91" text-anchor="middle" fill="currentColor" font-size="12">Plugin calls sessions.send(session, command, arguments)</text>
  <text x="210" y="107" text-anchor="middle" fill="currentColor" font-size="10">same plugin, declared command</text>
  <line x1="210" y1="116" x2="210" y2="136" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="136" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="155" text-anchor="middle" fill="currentColor" font-size="12">Target session runs the command and replies</text>
  <text x="210" y="171" text-anchor="middle" fill="currentColor" font-size="10">take-reply returns the reply string</text>
  <line x1="210" y1="180" x2="210" y2="200" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="110" y="224" text-anchor="middle" fill="currentColor" font-size="11">reply string</text>
  <rect x="220" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="310" y="224" text-anchor="middle" fill="currentColor" font-size="11">not-permitted, not-found</text>
</svg>
<figcaption>Both sessions must run the same plugin with the grant. The request carries the plugin id and the caller session name.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `sessions.cross` | yes | the only form |
| `sessions.cross:*` | no | no argument; the target is chosen per call |
| `sessions.all` | no | unknown grant name |

High risk and server-side only. A bundled plugin still needs the explicit grant, in both sessions.

### What it unlocks

| Name | Kind | Meaning |
|---|---|---|
| `sessions.list-live` | function | Returns `list<live-session>`: `{ name, project, socket-id, panes, attached-clients, is-self }`. |
| `sessions.send` | function | `send(cross-request { session, command, arguments-json }) -> result<request-id, plugin-error>`. |
| `sessions.take-reply` | function | Returns `option<string>` once the target replied. |

See the host interface list in [Commands](/pages/api-commands/) and [Messages and topics](/pages/api-messages-and-topics/) for same-session messaging.

### Manifest

The grant goes into `manifest.json` under `capabilities`, in the list of the side that uses it (server only).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.sessionbridge",
  "capabilities": {
    "server": [
      "sessions.cross"
    ]
  }
}
```

### Worked example

This plugin sends the command `com.example.sessionbridge/ping` to every other live session and logs the replies.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::intents::{Intent, Submission};
use ilium_plugin_sdk::host::sessions::{self, CrossRequest, RequestId};

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct SessionBridge {
    waiting: std::collections::HashMap<RequestId, String>,
}

impl Handler for SessionBridge {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::CommandInvoked(invoked) if invoked.command == "com.example.sessionbridge/ping" => {
                    for session in sessions::list_live().into_iter().filter(|s| !s.is_self) {
                        let request = CrossRequest {
                            session: session.name.clone(),
                            command: "com.example.sessionbridge/pong".into(),
                            arguments_json: "{}".into(),
                        };
                        // `send` returns the request id, not the reply.
                        match sessions::send(request) {
                            Ok(id) => { self.waiting.insert(id, session.name); }
                            Err(error) => log::log(Level::Warn, &format!("{}: {error:?}", session.name)),
                        }
                    }
                }
                Event::RequestCompleted(done) => {
                    let Some(name) = self.waiting.remove(&done.request) else { continue };
                    match sessions::take_reply(done.request).filter(|_| done.outcome.is_ok()) {
                        Some(reply) => log::log(Level::Info, &format!("{name}: {reply}")),
                        None => log::log(Level::Warn, &format!("{name}: no reply")),
                    }
                }
                _ => {}
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { listLive, send, takeReply } from '@ilium/plugin-sdk/host/sessions';
import { log } from '@ilium/plugin-sdk/host/log';

const waiting = new Map<bigint, string>();

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === 'com.example.sessionbridge/ping') {
        for (const session of listLive().filter((s) => !s.isSelf)) {
          try {
            // `send` returns the request id, not the reply.
            waiting.set(send({ session: session.name, command: 'com.example.sessionbridge/pong', argumentsJson: '{}' }), session.name);
          } catch (error) {
            log('warn', `${session.name}: ${String(error)}`);
          }
        }
      } else if (event.tag === 'request-completed') {
        const name = waiting.get(event.val.request);
        if (name === undefined) continue;
        waiting.delete(event.val.request);
        const reply = event.val.outcome.tag === 'ok' ? takeReply(event.val.request) : undefined;
        log(reply === undefined ? 'warn' : 'info', `${name}: ${reply ?? 'no reply'}`);
      }
    }
    return [];
  },
});
```

A reply is a string chosen by the target command. Treat it as untrusted input: it comes from another session, possibly of another project.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Grant missing in this session | `not-permitted` | Hide the feature. |
| Target session lacks the plugin or the grant | `not-permitted` | Tell the user to enable it there. |
| Command not declared by the plugin | `not-permitted` | Declare it in `contributes.commands`. |
| Session closed meanwhile | `not-found` | Call `list-live` again. |
| No reply within 30,000 ms ([reply deadlines](/pages/reference-errors-and-limits/#reply-deadlines)) | `timed-out` | Retry once; do not loop. |
| Other user's socket | never listed | Nothing. |

The target session sees the caller's plugin id and session name, so the command can refuse unknown callers.

### What the user sees

The review dialog lists the grant when a package is installed or first activated. The line is high risk and starts unchecked.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.sessionbridge 0.1.0 by Example Ltd                             │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ <span class="t-warn">! [ ] Runs plugin commands in other sessions</span> <span class="t-dim">……………………………………</span> <span class="t-warn">sessions.cross</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests sessions.cross, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar.</figcaption>
</figure>

Listing the grants of an installed plugin from the terminal:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.sessionbridge
<span class="t-dim">Grant                             Risk     State</span>
sessions.cross                    high     <span class="t-warn">declined</span>
</pre>

<figcaption>Output of the grant listing for this plugin. A high-risk grant that the user left unchecked shows as declined.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.sessionbridge > Capabilities. A revocation re-activates the plugin with the reduced grant set, and the stubs of the declined imports return `not-permitted`.

### Related pages

- [messages.peer](/pages/capability-messages-peer/).
- [workspace.manage](/pages/capability-workspace-manage/).
- [Commands](/pages/api-commands/).
- [Security](/pages/security/).

### Security

Not available to T3 (`manifest.grant-tier`); see [Security](/pages/security/).
