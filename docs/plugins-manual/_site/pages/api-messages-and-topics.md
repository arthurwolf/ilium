# Messages and topics

## Messages and topics

Messages and topics let plugins talk without calling each other. A message goes to one receiver: the other side of the same plugin, or a named peer plugin. A topic is a named channel that a plugin publishes on. Every plugin that holds a matching subscribe grant receives each publish on that topic.

Use a message when one receiver must act on the data. Use a topic when any number of plugins may react and the publisher does not need to know who they are. Neither path returns a reply to the sender. A message intent completes only with whether the message was queued for its receiver.

### What you need

- **Grants.** Messages between the two sides of your own plugin need no grant. A message to another plugin needs `messages.peer:<plugin-id>` with the exact id of that plugin. Receiving a topic needs `topics.subscribe:<pattern>`. Publishing a topic needs no grant.
- **Manifest.** Each topic you publish must be declared under `contributes.topics.publishes`. Each topic you react to is listed under `contributes.topics.subscribes`. A subscription without a matching grant delivers nothing and raises no error.
- **Language.** Rust uses the crate `ilium-plugin-sdk`. TypeScript uses `@ilium/plugin-sdk`. Both need the toolchain from [getting started](/pages/getting-started-toolchain/).

### Surface

| Name | Kind | Types | Meaning |
|---|---|---|---|
| `publish(topic, payload-json)` | function, `ilium:host/topics` | two `string` arguments, returns `result<_, plugin-error>` | Publishes a JSON payload on a declared topic under the plugin's own id. Sync. |
| `message` | intent, case `message(message-spec)` | `message-spec` | Queues a message for a receiver. Completes `ok` when queued, or `refused(busy)` when the receiver queue has no free slot and no drop-oldest event to evict. |
| `message` | event | `message-event` | Delivers a message to the receiver. Never dropped. |
| `topic-published` | event | `topic-published-event` | Delivers a topic publish to a subscriber. Dropped and counted when the queue is full. |
| `events-dropped` | event | `list<dropped-count>` | Reports how many events of one source a subscriber lost. |
| `message-spec` | record | `to: message-target`, `channel: string`, `payload: list<u8>` | The intent payload. |
| `message-target` | record | `plugin: plugin-id`, `side: plugin-side`, `client: option<client-id>` | The receiver. `client: none` from the server reaches every attached client. |
| `plugin-side` | enum | `server`, `client` | The side of the receiver that gets the message. |
| `message-event` | record | `from`, `from-side`, `from-client`, `channel`, `payload` | The sender id, side, client (if any), channel and payload. |
| `topic-published-event` | record | `topic: string`, `from: plugin-id`, `payload-json: string` | The topic, the publishing plugin and its JSON payload. |

The WIT definitions behind the table:

```wit
// ilium:host/topics
publish: func(topic: string, payload-json: string) -> result<_, plugin-error>;

// ilium:types/intents, a case of the intent variant
message(message-spec)

record message-target { plugin: plugin-id, side: plugin-side, client: option<client-id> }
enum plugin-side { server, client }
record message-spec { to: message-target, channel: string, payload: list<u8> }

// ilium:types/events
record message-event {
    from: plugin-id, from-side: plugin-side, from-client: option<client-id>,
    channel: string, payload: list<u8>,
}
record topic-published-event { topic: string, from: plugin-id, payload-json: string }
```

Limits that apply to this surface:

| Limit | Value | Error when exceeded |
|---|---|---|
| Topic name | 128 characters | `invalid-argument` |
| Topic payload | 64 KiB, valid JSON | `invalid-argument` |
| Message payload | 1 MiB | `refused(invalid-argument)` |
| Channel | 1 to 64 characters from `[a-z0-9.-]` | `refused(invalid-argument)` |
| `topics.publish` rate | 100 calls per second per plugin | `rate-limited`, with `retry-after-ms` |

### Topic names and patterns

A topic id is the publisher's plugin id, a `/`, and a name, for example `example.notes/saved`. A plugin may publish only topics that start with its own plugin id followed by `/`. The kernel publishes its own topics under reserved kernel namespaces, which are ids that no installed plugin can take, for example `ilium.progress/finished`. The kernel namespace `ilium.progress` is not the id of the core plugin `ilium.core.progress`: the core plugin publishes only under its own id, as `ilium.core.progress/changed`, and no plugin may publish under `ilium.progress/`.

A subscription pattern takes one of three forms:

- An exact topic, such as `ilium.progress/finished`.
- A prefix followed by `/*`, such as `ilium.progress/*`. It matches every topic under that prefix at any depth.
- A bare `*`. It matches every topic and is a high-trust grant.

A `*` in any other position is not valid.

### Kernel topic for extension points

The kernel publishes `ilium.extensions/changed` with the payload `{ "point": "<id>" }` when the contributions to a point change, for example when a plugin is installed, enabled, disabled or calls `register`. The kernel delivers it only to the owner of that point. It needs no `topics.subscribe` grant beyond `extensions.provide` for the point. After the topic arrives, the owner calls `list` again. See [extension points](/pages/api-extension-points/).

### Routing

<figure>
<svg viewBox="0 0 420 200" role="img" aria-label="Message and topic routing between sides and plugins">
<title>Message and topic routing</title>
<desc>A message goes from the server side to the client side. A peer plugin receives a message with a grant. A topic reaches every subscriber whose pattern matches.</desc>
<defs>
<marker id="arrow-messages" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="10" y="10" width="110" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="65" y="32" font-size="12" text-anchor="middle" fill="currentColor">Server side</text>
<rect x="300" y="10" width="110" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="355" y="32" font-size="12" text-anchor="middle" fill="currentColor">Client side</text>
<line x1="120" y1="28" x2="300" y2="28" stroke="currentColor" marker-end="url(#arrow-messages)"/>
<text x="210" y="22" font-size="10" text-anchor="middle" fill="currentColor">message, one receiver</text>
<rect x="10" y="100" width="110" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
<text x="65" y="122" font-size="12" text-anchor="middle" fill="currentColor">Peer plugin</text>
<line x1="65" y1="46" x2="65" y2="100" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#arrow-messages)"/>
<text x="72" y="76" font-size="10" fill="currentColor">messages.peer grant</text>
<rect x="146" y="100" width="128" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="210" y="122" font-size="10" text-anchor="middle" fill="currentColor">example.notes/saved</text>
<rect x="300" y="100" width="110" height="36" rx="6" fill="none" stroke="currentColor"/>
<text x="355" y="122" font-size="12" text-anchor="middle" fill="currentColor">Subscriber</text>
<line x1="274" y1="118" x2="300" y2="118" stroke="currentColor" marker-end="url(#arrow-messages)"/>
<text x="10" y="165" font-size="10" fill="currentColor">A topic reaches every subscriber whose pattern matches</text>
<text x="10" y="181" font-size="10" fill="currentColor">and who holds the topics.subscribe grant.</text>
</svg>
<figcaption>Messages go to one receiver. Topics go to every matching subscriber.</figcaption>
</figure>

### Manifest declarations

The example plugin `example.notes` publishes one topic and subscribes to the progress topic. The
examples below also react to the command `example.notes/save`, which is declared as described on
the [commands](/pages/api-commands/) page:

```json
{
  "id": "example.notes",
  "contributes": {
    "topics": {
      "publishes": ["example.notes/saved"],
      "subscribes": ["ilium.progress/finished"]
    }
  },
  "capabilities": {
    "server": [],
    "client": ["topics.subscribe:ilium.progress/finished"]
  }
}
```

The client side needs no grant to receive its own server side's messages. A message to another
plugin would add `messages.peer:<that-plugin-id>` to the sending side's list.

### Example: the server side publishes and messages the client side (Rust)

The server side runs when the user invokes `example.notes/save`. It publishes the topic for other
plugins, then sends the same data to its own client side.

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::topics;
use ilium_plugin_sdk::types::events::CommandInvokedEvent;
use ilium_plugin_sdk::types::intents::{MessageSpec, MessageTarget, PluginSide};

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct NotesServer;

impl Lifecycle for NotesServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }

    fn deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        None
    }
}

impl Handler for NotesServer {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::CommandInvoked(CommandInvokedEvent { command, .. }) = event else {
                continue;
            };
            if command == "example.notes/save" {
                submissions.extend(announce_saved("notes/plan.md"));
            }
        }
        submissions
    }
}

fn announce_saved(path: &str) -> Vec<Submission> {
    let payload = format!(r#"{{"path":"{path}"}}"#);
    if let Err(error) = topics::publish("example.notes/saved", &payload) {
        log::log(Level::Warn, &format!("publish refused: {:?}", error.code));
    }
    vec![Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget {
            plugin: "example.notes".to_string(),
            side: PluginSide::Client,
            client: None,
        },
        channel: "saved".to_string(),
        payload: payload.into_bytes(),
    }))]
}
```

### Example: the client side receives the topic and the message (Rust)

The client side receives the progress topic through its grant and the saved message from its own
server side.

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::types::events::{MessageEvent, TopicPublishedEvent};

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct NotesClient;

impl Lifecycle for NotesClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }

    fn deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        None
    }
}

impl Handler for NotesClient {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::TopicPublished(TopicPublishedEvent { topic, from, payload_json }) => {
                    log::log(Level::Info, &format!("{topic}: {payload_json}"));
                }
                Event::Message(MessageEvent { from, channel, payload, .. })
                    if channel == "saved" =>
                {
                    log::log(Level::Info, &format!("saved by {from}: {} bytes", payload.len()));
                }
                _ => {}
            }
        }
        Vec::new()
    }
}
```

### Example: the server side publishes and messages the client side (TypeScript)

```ts
import { definePlugin, PluginError, type Event } from '@ilium/plugin-sdk';
import type { Intent, Submission } from '@ilium/plugin-sdk/types';
import { publish } from '@ilium/plugin-sdk/host/topics';
import { log } from '@ilium/plugin-sdk/host/log';

function announceSaved(path: string): Submission[] {
  const payload = JSON.stringify({ path });
  try {
    publish('example.notes/saved', payload);
  } catch (error) {
    log('warn', `publish refused: ${(error as PluginError).code}`);
  }
  const intent: Intent = {
    tag: 'message',
    val: {
      to: { plugin: 'example.notes', side: 'client', client: undefined },
      channel: 'saved',
      payload: new TextEncoder().encode(payload),
    },
  };
  return [{ intent, correlation: 1n, dedupKey: undefined }];
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate() {},
  onEvents(batch: Event[]): Submission[] {
    const out: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === 'example.notes/save') {
        out.push(...announceSaved('notes/plan.md'));
      }
    }
    return out;
  },
});
```

### Example: the client side receives the topic and the message (TypeScript)

```ts
import { definePlugin, type Event } from '@ilium/plugin-sdk';
import type { Submission } from '@ilium/plugin-sdk/types';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {},
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'topic-published') {
        const { topic, from, payloadJson } = event.val;
        log('info', `${topic}: ${payloadJson}`);
      } else if (event.tag === 'message' && event.val.channel === 'saved') {
        log('info', `saved by ${event.val.from}: ${event.val.payload.length} bytes`);
      }
    }
    return [];
  },
});
```

The intent completes `ok` once the message is queued for the client side. A subscriber to
`example.notes/*` receives one `topic-published` event with `from` set to `example.notes`.

### What the user sees

The user sees messages and topics only through what plugins do with them and through their logs.
The log of the subscriber after one save looks like this:

<pre class="term">$ ilium plugin logs example.notes-watch --tail 2
2026-10-11 10:20:31 info  example.notes/saved: {"path":"notes/plan.md"}
2026-10-11 10:20:31 info  saved by example.notes: 24 bytes
$ 2 lines shown, 0 dropped
</pre>

Illustrative, not generated. The log of a subscriber that reacts to the saved topic and the message.

When a publish is refused, the publisher's own code decides what the user sees. The example above
logs this warning:

<pre class="term">$ ilium plugin logs example.notes --tail 1
2026-10-11 10:21:04 warn  publish refused: InvalidArgument
$ 1 line shown, 0 dropped
</pre>

Illustrative, not generated. The warning the publisher logs when it publishes an undeclared topic.

### Payload of the progress topic

The progress monitor publishes `ilium.progress/finished` when a monitored job ends. Subscribe with
`topics.subscribe:ilium.progress/finished` or `topics.subscribe:ilium.progress/*`. The payload is
a JSON object:

| Field | Type | Meaning |
|---|---|---|
| `pane` | number | Node id of the pane that ran the job. |
| `job_id` | string | Id of the monitored job. |
| `status` | `"done"` or `"error"` | Final state of the job. |
| `percent` | number, 0 to 100 | Progress when the job ended. |
| `message` | string | One line that describes the outcome. |
| `details` | string or `null` | Longer text, or `null` when there is none. |

### Edge cases and errors

Synchronous codes are returned by `publish` at the call. Completion codes arrive as the outcome
of the message intent.

| Situation | Result |
|---|---|
| Topic id does not start with the plugin id and `/` | `not-permitted` (sync) |
| Topic not declared in `contributes.topics.publishes` | `invalid-argument` (sync) |
| Topic name over 128 characters, or payload not JSON or over 64 KiB | `invalid-argument` (sync) |
| More than 100 publishes per second | `rate-limited`, with `retry-after-ms` (sync) |
| Message to the plugin's own other side | `ok` when queued |
| Message to a peer without `messages.peer:<plugin-id>` | `refused(not-permitted)` on that submission |
| Message to a plugin that is not installed or is disabled | `refused(not-found)` |
| Channel outside `[a-z0-9.-]` or longer than 64 characters | `refused(invalid-argument)` |
| Message payload over 1 MiB | `refused(invalid-argument)` |
| Message to an installed plugin that is not active | The plugin is activated, and the message is delivered after `activate` |
| Subscriber without the subscribe grant | No events and no error |
| Subscriber queue full for `topic-published` | Oldest events are dropped and counted. The next batch starts with `events-dropped`. |
| Receiver queue full for `message` | Back-pressure at the sender: the `message` intent completes `refused(busy)` and nothing is queued. The receiver is not restarted. A `message` that was accepted is never dropped [SVC-030]. |

A refused submission makes the other submissions of its batch complete as `refused(cancelled)`,
with the message `batch refused: submission <index>`. A publish that is refused for a topic does not
reach any subscriber.

### Used by

- The progress monitor (feature 59) publishes `ilium.progress/finished` for other plugins to react to.
- Semantic animation (feature 103) subscribes to `ilium.organise/*` from the tree organisation plugin.
- AI triggers (feature 44) subscribe to topics as their events.
- Extension point owners receive `ilium.extensions/changed` for their own points.
- `ilium voice say` (feature 52) sends a message from the server side to the plugin's own client side.

### Questions between the two sides

A message is one-way news. When the server side needs an answer that only a client can give, such as its terminal size or theme, use `client-requests` instead: a typed request, a typed answer, a timeout and a failure when the client is gone. See [client requests](/pages/api-client-requests/).

| Need | Use |
|---|---|
| Tell the other side something | `message` intent |
| Tell any plugin that subscribed | `publish` on a topic |
| Ask one client a question and wait | `client-requests.request` |
| Run a command of the same plugin in another session | `sessions.send` (below) |

### Across sessions

The `sessions` interface (server side, grant `sessions.cross`, high risk) lists the user's other running Ilium sessions and runs one declared command of the same plugin in one of them.

```wit
// Excerpt of ilium:host/sessions in reference-wit
record live-session { name: string, project: string, socket-id: string, panes: u32, attached-clients: u32, is-self: bool }
list-live: func() -> list<live-session>;
record cross-request { session: string, command: string, arguments-json: string }
send: func(request: cross-request) -> result<request-id, plugin-error>;
take-reply: func(request: request-id) -> option<string>;
```

The target session must have the same plugin installed and enabled with `sessions.cross` granted, and `command` must be declared by that plugin. Kernel commands of another session are never reachable. Other users' sockets are never listed. Errors: `not-found` (session), `not-permitted` (target lacks the plugin or the grant); completion `timed-out` or `network`.

**Rust**

```rust
use ilium_plugin_sdk::host::sessions::{self, CrossRequest};

fn ping_other_sessions() {
    for live in sessions::list_live().into_iter().filter(|s| !s.is_self) {
        let _ = sessions::send(&CrossRequest {
            session: live.name, command: "refresh-index".into(), arguments_json: "{}".into(),
        });
    }
}
```

**TypeScript**

```ts
import { listLive, send } from '@ilium/plugin-sdk/host/sessions';

export function pingOtherSessions(): void {
  for (const live of listLive().filter((s) => !s.isSelf)) {
    send({ session: live.name, command: 'refresh-index', argumentsJson: '{}' });
  }
}
```

Kernel changes also reach plugins as events: `kernel-settings-changed { tab, keys }`, `keymap-changed` and `display-mode-changed { mode }` (see [events](/pages/api-events/)). They are delivered coalesced, never as topics.

Requirement ids for this page: [SVC-020], [SVC-021], [SVC-022], [SVC-023], [SVC-024], [SVC-025], [SVC-026], [SVC-027], [SVC-028], [SVC-029], [SVC-030]. See the [requirements index](/pages/reference-requirements/).

### See also

- [Intents](/pages/api-intents/) for the `message` intent and its outcomes.
- [Events](/pages/api-events/) for the `message` and `topic-published` delivery policies.
- [Events and intents](/pages/events-and-intents/) for the submission and batch model.
- [Exports](/pages/api-exports/) for the `Lifecycle` and `Handler` exports used in the examples.
- [Capabilities](/pages/capabilities/) for the grant strings.
- [Log](/pages/api-log/) for the `log` import used in the examples.

Checked against `ilium:plugin@0.1.0`.
