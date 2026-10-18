---
layout: page
title: "Capability: messages.peer"
permalink: /pages/capability-messages-peer/
---

## messages.peer:&lt;plugin-id&gt;

The `messages.peer:<plugin-id>` capability lets a plugin send a one-way message to one other plugin. The sender submits the intent `message` with a channel name and a payload of bytes. The receiving plugin gets a `message` event that names the sender, the side it came from, the channel and the payload. The receiver cannot answer through this capability. A request that expects a reply uses [services.use](/pages/capability-services/). A message that many listeners receive without naming a receiver uses [topics.subscribe](/pages/capability-topics-subscribe/).

A plugin reaches its own other side without any grant. A server half sends to its client half, and a client half sends to its server half, by naming its own plugin id in the intent.

### What you need

- A grant `messages.peer:<plugin-id>` for each other plugin that the sending side messages. Grants are per side, so the grant goes in `capabilities.server` or `capabilities.client`, whichever side sends.
- The id of the receiving plugin, exactly as its manifest `id` writes it.
- The Rust crate `ilium-plugin-sdk` or the npm package `@ilium/plugin-sdk`, both for `ilium:plugin@0.1.0`.
- On the receiver, no grant, but its manifest must list the sending plugin in `accepts_messages_from` ([PKG-579]). A message from an id not on that list, or to a plugin that declares none, is refused at send with `refused(not-permitted)` and is never queued.

The receiver must be installed and enabled. If it is installed but not active, the host activates it first and delivers the message after `activate` returns.

### Grant syntax

The argument is the exact id of the receiving plugin. The host checks the grant string at install.

| Grant | Result at install | Reason |
|---|---|---|
| `messages.peer:com.example.other` | valid | one exact receiver |
| `messages.peer` | refused, `manifest.grant-argument-missing` | the argument is required |
| `messages.peer:*` | refused, `manifest.grant-wildcard` | wildcards are not accepted |
| `messages.peer:com.example.*` | refused, `manifest.grant-wildcard` | a wildcard is not accepted in any position |

### Surface

The intent `message` sends. The event `message` receives.

**Intent `message` (sender):**

| Field | Type | Meaning |
|---|---|---|
| `to` | `message-target` | the receiver, described by the three fields below |
| `to.plugin` | `plugin-id` | the receiving plugin id |
| `to.side` | `plugin-side` | `server` or `client` |
| `to.client` | `option<client-id>` | one attached client; `none` from the server reaches every attached client |
| `channel` | `string` | 1 to 64 characters from `a-z`, `0-9`, `.` and `-` |
| `payload` | `list<u8>` | the message bytes, at most 1 MiB |

The submission also carries `correlation` and an optional `dedup-key`. A `message` does not need a dedup key. A key that the same plugin already used within 10 minutes completes as `duplicate(original)` with no effect.

**Event `message` (receiver):**

| Field | Type | Meaning |
|---|---|---|
| `from` | `plugin-id` | the sending plugin |
| `from-side` | `plugin-side` | the side that sent the message |
| `from-client` | `option<client-id>` | the client that sent it, when the sending side had one |
| `channel` | `string` | the channel name, as sent |
| `payload` | `list<u8>` | the bytes, unchanged |

Message events that reach the receiver are never dropped by the host. A message from a sender not on the receiver's `accepts_messages_from` list is refused at send with `not-permitted` and is never queued.

**Binding names:**

| WIT | Rust | TypeScript |
|---|---|---|
| intent `message(message-spec)` | `Intent::Message(MessageSpec { to, channel, payload })` | `{ tag: 'message', val: { to, channel, payload } }` |
| `message-target` | `MessageTarget { plugin, side, client }` in `types::intents` | `{ plugin, side, client }` from `@ilium/plugin-sdk/types` |
| `plugin-side` | `PluginSide::Server`, `PluginSide::Client` | `'server'`, `'client'` |
| event `message(message-event)` | `Event::Message(MessageEvent { from, from_side, from_client, channel, payload })` in `types::events` | `{ tag: 'message', val: { from, fromSide, fromClient, channel, payload } }` |
| `list<u8>` | `Vec<u8>` | `Uint8Array` |

### Outcomes

The sender gets a `request-completed` event for each `message` submission.

| Outcome | When |
|---|---|
| `ok` | the message is queued for the receiver. The receiver may not have handled it yet. |
| `refused(not-permitted)` | no grant for that receiver, or the grant was declined or revoked before the call |
| `refused(not-found)` | the receiver is not installed or is disabled |
| `refused(invalid-argument)` | channel outside the allowed form, payload over 1 MiB, or a malformed submission |
| `duplicate(original)` | the same `dedup-key` was used within 10 minutes |

A missing grant refuses the whole batch. The offending submission completes as `refused(not-permitted)`, and every other submission of that batch completes as `refused(cancelled)` with the message `batch refused: submission <index>`.

<figure>
<svg viewBox="0 0 320 280" role="img" aria-label="Message flow from sender intent through grant check to the receiver's message event">
  <defs>
    <marker id="mp-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="8" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="32" text-anchor="middle" font-size="11" fill="currentColor">Sender submits intent message</text>
  <line x1="160" y1="48" x2="160" y2="70" stroke="currentColor" marker-end="url(#mp-arrow)"/>
  <rect x="10" y="70" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="94" text-anchor="middle" font-size="11" fill="currentColor">Kernel checks messages.peer for the receiver</text>
  <line x1="160" y1="110" x2="160" y2="132" stroke="currentColor" marker-end="url(#mp-arrow)"/>
  <rect x="10" y="132" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="156" text-anchor="middle" font-size="11" fill="currentColor">Message is queued for the receiver</text>
  <line x1="160" y1="172" x2="160" y2="194" stroke="currentColor" marker-end="url(#mp-arrow)"/>
  <rect x="10" y="194" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="218" text-anchor="middle" font-size="11" fill="currentColor">Receiver gets a message event</text>
  <text x="160" y="262" text-anchor="middle" font-size="11" fill="currentColor">Own-side messages skip the grant check.</text>
</svg>
<figcaption>Sending to another plugin needs the grant. Sending to a plugin's own other side does not.</figcaption>
</figure>

### Review and risk

- The [grant risk table](/pages/reference-capabilities/) does not mark `messages.peer` as high risk. A plugin with this grant is installed with it checked by default in the review dialog.
- The grant names the receiver, so a sender cannot reach a plugin that was not granted. The review dialog shows each receiver id before the user allows it.
- A T2 or T3 package shows every receiver id in the review dialog. T1 bundled plugins take the grant from their manifest ceiling.

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.messages 0.1.0 by Example Ltd                                  │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Sends messages to com.example.peer <span class="t-dim">…………</span> messages.peer:com.example.peer │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests messages.peer:com.example.peer, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

A declined grant appears unchecked and dimmed. The plugin then sees no `messages.peer` entry in its granted list, and every message to that receiver completes as `refused(not-permitted)`.

### What the user sees

- During install, the review row above, with the receiver id in the grant string.
- Each refused message is logged at level `warn`, with the sending plugin id, the import `ilium:host/intents` and the function `submit`. The line appears in `ilium plugin logs <id>`:

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs com.example.notes --tail 1
2026-10-11 09:14:02 warn  com.example.notes not-permitted: messages.peer
</pre>
<figcaption>Illustrative, not generated. The log line for a refused message. The text after the plugin id is an example.</figcaption>
</figure>

### Worked example in Rust

The server half of `com.example.notes` sends `export-done` to `com.example.other` when the user runs its `export` command. The manifest grants `messages.peer:com.example.other` under `capabilities.server`.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::exports::Activation;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::types::events::CommandInvokedEvent;
use ilium_plugin_sdk::types::intents::{MessageSpec, MessageTarget, PluginSide};

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Notes;

impl Lifecycle for Notes {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        log::log(Level::Info, &format!("started as {}", context.plugin));
        Ok(())
    }
}

impl Handler for Notes {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::CommandInvoked(CommandInvokedEvent { command, .. }) = event else {
                continue;
            };
            if command != "com.example.notes/export" {
                continue;
            }
            // Submission::new uses the next correlation number and no dedup key.
            submissions.push(Submission::new(Intent::Message(MessageSpec {
                to: MessageTarget {
                    plugin: "com.example.other".into(),
                    side: PluginSide::Server,
                    client: None,
                },
                channel: "status".into(),
                payload: b"export-done".to_vec(),
            })));
        }
        submissions
    }
}
```

### Worked example in TypeScript

The receiver is `com.example.other`, a server half that logs each status message from `com.example.notes`. The handler needs no grant of its own.

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate(context) {
    log('info', `started as ${context.plugin}`);
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'message') continue;
      const { from, channel, payload } = event.val;
      if (from !== 'com.example.notes' || channel !== 'status') continue;
      // The payload is ASCII in this example, so a char-code decode is enough.
      const text = String.fromCharCode(...payload);
      log('info', `status from ${from}: ${text}`);
    }
    return [];
  },
});
```

The matching send from a TypeScript sender is one literal submission. The intent `to` takes `client: undefined` to reach the receiver's server half:

**TypeScript**

```ts
{
  intent: {
    tag: 'message',
    val: {
      to: { plugin: 'com.example.other', side: 'server', client: undefined },
      channel: 'status',
      payload: new TextEncoder().encode('export-done'),
    },
  },
  correlation: 3n,
  dedupKey: undefined,
}
```

### Edge cases and errors

| Case | Result |
|---|---|
| Grant missing, declined or revoked during a call | `refused(not-permitted)` for that submission, batch refused (see Outcomes) |
| Receiver not installed or disabled | `refused(not-found)` |
| Receiver installed but not active | the host activates it; the message is delivered after `activate` |
| Receiver's `activate` returns an error | the message is discarded, the error is logged at level `error` under the receiver, and the sender is not told |
| Channel empty, longer than 64 characters, or with other characters | `refused(invalid-argument)` |
| Payload over 1 MiB | `refused(invalid-argument)` |
| Batch with more than 256 submissions | the plugin traps and none of the batch is applied |
| Same `dedup-key` within 10 minutes | `duplicate(original)`, no second delivery |
| Message to the plugin's own other side | allowed with no grant; `from` is the plugin's own id |

A `message` is not a request. An `ok` outcome means the host queued the message, so the receiver's handler must decide what to do with it.

### Who requests it

No first-party plugin in [feature traceability](/pages/reference-feature-traceability/) lists `messages.peer` in its capability column. Feature 52 (`ilium voice say`) sends an intent `message` to the plugin's own client side, which needs no grant.

### See also

- [topics.subscribe](/pages/capability-topics-subscribe/) for one-to-many messages that do not name a receiver.
- [services.use](/pages/capability-services/) for a request that expects a reply.
- [API: messages and topics](/pages/api-messages-and-topics/) for the full messages and topics API.
- [Events and intents](/pages/events-and-intents/) for how submissions and outcomes work.

### Security

A receiver MUST declare `accepts_messages_from`, a list of plugin ids ([SEC-007], [SEC-050]); a peer message from an id not on the list, or to a plugin that declared none, is refused at send: the sender gets `refused(not-permitted)` and nothing is queued. Cross-plugin data flows are discussed in [Threat model](/pages/reference-threat-model/).
