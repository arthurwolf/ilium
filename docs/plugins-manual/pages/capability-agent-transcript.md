---
layout: page
title: "Capability: agent.transcript"
permalink: /pages/capability-agent-transcript/
---

## agent.transcript

The `agent.transcript` capability lets a plugin read the conversation of an agent pane as typed turns: role, text, tool name, time and token count. The host parses the agent's own session file; the path stays resolved by the kernel and is never shown to the plugin. A transcript contains everything the user and the agent said, including pasted secrets, so the grant is high risk (**H**).

**You will need:**

- An `agent.transcript` grant (explicit, high risk).
- An agent pane id from [agent.read](/pages/capability-agent-read/).
- An agent signature with a known transcript format: otherwise the call returns `unsupported`.

<figure>
<svg viewBox="0 0 420 252" role="img" aria-label="A transcript query is parsed by the host from the agent session file and returned as typed turns">
  <defs><marker id="arr" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="27" text-anchor="middle" fill="currentColor" font-size="12">Plugin calls agent.transcript(pane, last-turns, max-bytes)</text>
  <text x="210" y="43" text-anchor="middle" fill="currentColor" font-size="10">at most 512 turns and 4 MiB</text>
  <line x1="210" y1="52" x2="210" y2="72" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="72" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="91" text-anchor="middle" fill="currentColor" font-size="12">Host parses the session file of the agent</text>
  <text x="210" y="107" text-anchor="middle" fill="currentColor" font-size="10">path stays in the kernel</text>
  <line x1="210" y1="116" x2="210" y2="136" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="136" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="155" text-anchor="middle" fill="currentColor" font-size="12">Plugin calls take-transcript</text>
  <text x="210" y="171" text-anchor="middle" fill="currentColor" font-size="10">returns the typed turns</text>
  <line x1="210" y1="180" x2="210" y2="200" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="110" y="224" text-anchor="middle" fill="currentColor" font-size="11">list of transcript-turn</text>
  <rect x="220" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="310" y="224" text-anchor="middle" fill="currentColor" font-size="11">unsupported, not-permitted</text>
</svg>
<figcaption>The plugin sees turns, never the path of the session file.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `agent.transcript` | yes | the only form |
| `agent.transcript:claude` | no | no argument; support depends on the signature |
| `fs.read:agent-sessions:*` | separate | file access to session folders is a different, high-risk grant |

High risk. A bundled T1 plugin still needs an explicit grant. Prefer [agent.read](/pages/capability-agent-read/) when the plugin only needs the activity or the model.

### What it unlocks

| Name | Kind | Meaning |
|---|---|---|
| `agent.transcript` | function | `transcript(transcript-query { pane, last-turns, max-bytes }) -> result<request-id, plugin-error>`. |
| `agent.take-transcript` | function | Returns `option<list<transcript-turn>>`. |
| `transcript-turn` | record | `{ role, text, tool?, at?, tokens? }`; `role` is `user`, `assistant`, `tool` or `system`. |

On the wire, `transcript` returns a `request-id`; the turns arrive after `request-completed` through `take-transcript` (the WIT in [Agent transcript](/pages/primitives-agent-transcript/) is authoritative). The `agent::transcript(query).block_on()` form and the TypeScript `await transcript(...)` in the examples below are SDK wrappers that submit the request, wait for `request-completed` and call `take-transcript` once, so they yield the turn list directly or the request's error.

See [Agent primitives](/pages/primitives-agent/) and [Requests and outcomes](/pages/requests-and-outcomes/).

### Manifest

The grant goes into `manifest.json` under `capabilities`, in the list of the side that uses it (server or client).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.recap",
  "capabilities": {
    "server": [
      "agent.read",
      "agent.transcript"
    ]
  }
}
```

### Worked example

This plugin counts the assistant turns among the last 20 turns of an agent pane when the command `com.example.recap/count` runs.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::intents::{Intent, Submission};
use ilium_plugin_sdk::primitives::agent::{self, TranscriptQuery, TurnRole};

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Recap;

impl Handler for Recap {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::CommandInvoked(invoked) = event else { continue };
            if invoked.command != "com.example.recap/count" { continue; }
            let Some(pane) = invoked.context.pane else { continue };
            let query = TranscriptQuery { pane, last_turns: 20, max_bytes: 262_144 };
            match agent::transcript(query).block_on() {
                Ok(turns) => {
                    let replies = turns.iter().filter(|turn| turn.role == TurnRole::Assistant).count();
                    log::log(Level::Info, &format!("{replies} assistant turns"));
                }
                Err(error) => log::log(Level::Warn, &format!("transcript failed: {error:?}")),
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
import { transcript } from '@ilium/plugin-sdk/primitives/agent';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  async onEvents(batch: Event[]): Promise<Submission[]> {
    for (const event of batch) {
      if (event.tag !== 'command-invoked' || event.val.command !== 'com.example.recap/count' || event.val.context.pane === undefined) continue;
      try {
        const turns = await transcript({ pane: event.val.context.pane, lastTurns: 20, maxBytes: 262144 });
        log('info', `${turns.filter((turn) => turn.role === 'assistant').length} assistant turns`);
      } catch (error) {
        log('warn', `transcript failed: ${String(error)}`);
      }
    }
    return [];
  },
});
```

Do not copy transcript text into the plugin's storage unless the user asked for it.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Grant missing or declined | `not-permitted` | Use `agent.read` data only. |
| Signature has no known transcript format | `unsupported` | Hide the feature for that agent. |
| `last-turns` above 512 or `max-bytes` above 4 MiB | `invalid-argument` | Ask for less. |
| Pane is not an agent pane | `not-found` | Check the pane kind. |
| Session file not written yet | empty list | Try again after the first turn. |
| Grant revoked while a call is in flight | `revoked` | Discard cached turns. |
| Call made after the grant was revoked | `not-permitted` | Discard cached turns and use `agent.read` data only. |

Turns come from the agent's file, so they can lag the screen by a moment.

### What the user sees

The review dialog lists the grant when a package is installed or first activated. The line is high risk and starts unchecked.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.recap 0.1.0 by Example Ltd                                     │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ <span class="t-warn">! [ ] Reads agent conversations</span> <span class="t-dim">…………………………………………………………………</span> <span class="t-warn">agent.transcript</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests agent.transcript, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar.</figcaption>
</figure>

Listing the grants of an installed plugin from the terminal:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.recap
<span class="t-dim">Grant                             Risk     State</span>
agent.transcript                  high     <span class="t-warn">declined</span>
</pre>

<figcaption>Output of the grant listing for this plugin. A high-risk grant that the user left unchecked shows as declined.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.recap > Capabilities. A revocation re-activates the plugin with the reduced grant set, and the stubs of the declined imports return `not-permitted`.

### Related pages

- [agent.read](/pages/capability-agent-read/).
- [agent.read-prompts](/pages/capability-agent-read-prompts/).
- [Agent primitives](/pages/primitives-agent/).
- [fs.read](/pages/capability-fs-read/).
