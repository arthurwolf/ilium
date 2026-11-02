# Capability: topics.subscribe

## topics.subscribe:&lt;topic-pattern&gt;

The `topics.subscribe:<topic-pattern>` capability lets a plugin receive the topics that other plugins publish. A topic is a named channel such as `ilium.progress/finished`. Each publish on a matching topic reaches the subscriber as one `topic-published` event with the topic name, the publishing plugin id and the JSON payload.

Topics let plugins share state without knowing each other. A message (see [messages.peer](/pages/capability-messages-peer/)) goes to one named receiver. A topic goes to every subscriber whose pattern matches, and the publisher never learns who they are.

Subscribing is declarative. There is no subscribe call and no unsubscribe call. The grant and the manifest decide what a plugin receives, and the kernel checks both on every publish.

### What you need

- **A grant per pattern.** Each `topics.subscribe:<topic-pattern>` string is one grant. Grants are per side, so a server half and a client half that both need topics each list their own grant.
- **A manifest entry.** List every topic you react to under `contributes.topics.subscribes`. The entries use the same pattern syntax as the grant. An invalid entry fails the install check.
- **An activation trigger.** A plugin that is not active receives no events. Use `"startup"` in `activation`, or `"topic:<exact-topic>"` for each exact topic. A pattern cannot be a trigger, so a plugin with a pattern subscription uses `"startup"`.
- **A language SDK.** Rust uses the crate `ilium-plugin-sdk` (imported as `ilium_plugin_sdk`). TypeScript uses `@ilium/plugin-sdk`. Both need the toolchain from [getting started](/pages/getting-started-toolchain/).
- **No grant to publish.** A plugin that publishes a topic needs its own id prefix and a `contributes.topics.publishes` entry, not this grant. See [messages and topics](/pages/api-messages-and-topics/).

### Patterns

| Pattern | Matches | Risk | Accepted |
|---|---|---|---|
| `ilium.progress/finished` | that one topic | normal | yes |
| `ilium.progress/*` | every topic under `ilium.progress/`, any depth | normal | yes |
| `*` | every topic | high (H) | yes |
| `ilium.*`, `*/finished`, `ilium.progress*` | none | not applicable | no, `manifest.grant-wildcard` |
| `topics.subscribe` | none | not applicable | no, `manifest.grant-argument-missing` |

A prefix pattern matches a topic only when the topic continues past the slash, so `ilium.progress/*` does not match the bare name `ilium.progress`. A topic name is at most 128 characters.

### The full surface

| Name | Kind | Types | Meaning |
|---|---|---|---|
| `topics.subscribe:<topic-pattern>` | grant, sides server and client | pattern string | Lets the side receive matching topics. |
| `contributes.topics.subscribes` | manifest list | topic patterns | Declares the topics the plugin reacts to. |
| `topic:<exact-topic>` | activation trigger | exact topic name | Activates the plugin when that topic is published. |
| `topic-published` | event | `topic: string`, `from: plugin-id`, `payload-json: string` | One publish on a matching topic. Drop oldest, counted. |
| `events-dropped` | event | `list<dropped-count>` | Reports lost events. `dropped-count` holds `event`, `source` (the topic name) and `count`. Never dropped. |
| `granted` | field of the activation context | `list<string>` | The grant strings actually granted. A declined grant is absent. |

Bindings for the event:

| Language | Event | Fields |
|---|---|---|
| Rust | `Event::TopicPublished(TopicPublishedEvent { topic, from, payload_json })` | snake_case fields |
| TypeScript | `{ tag: 'topic-published', val: { topic, from, payloadJson } }` | camelCase fields |

The payload is the JSON text the publisher sent, unchanged. The kernel does not parse it.

### Worked example: the progress notice in Rust (server side)

The plugin keeps a server-side record of finished jobs. It checks at activation that the grant is present, and it logs each notice it receives.

Manifest excerpt for both examples in this page:

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.jobwatch",
  "activation": ["startup"],
  "contributes": {
    "topics": { "publishes": [], "subscribes": ["ilium.progress/finished"] }
  },
  "capabilities": {
    "server": ["topics.subscribe:ilium.progress/finished"],
    "client": ["topics.subscribe:ilium.progress/finished"]
  }
}
```

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::types::events::TopicPublishedEvent;
use ilium_plugin_sdk::exports::{Activation, DeactivationReason};

const JOB_FINISHED_TOPIC: &str = "ilium.progress/finished";
const JOB_FINISHED_GRANT: &str = "topics.subscribe:ilium.progress/finished";

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct JobWatch;

impl Lifecycle for JobWatch {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        if !context.granted.iter().any(|grant| grant == JOB_FINISHED_GRANT) {
            log::log(Level::Warn, "job notices declined; no topic events will arrive");
        }
        Ok(())
    }

    fn deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        None
    }
}

impl Handler for JobWatch {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::TopicPublished(TopicPublishedEvent { topic, from, payload_json })
                    if topic == JOB_FINISHED_TOPIC =>
                {
                    log::log(Level::Info, &format!("{from} published {topic}: {payload_json}"));
                }
                Event::EventsDropped(_) => {
                    log::log(Level::Warn, "topic events were dropped; re-read the job state");
                }
                _ => {}
            }
        }
        Vec::new()
    }
}
```

The handler only logs and returns no submissions. It must finish inside the `on-events` deadline (see edge cases).

### Worked example: the progress notice in TypeScript (client side)

The client half parses the payload. The keys inside the JSON keep the catalogue spelling (`job_id`), because the publisher wrote them that way.

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { log } from '@ilium/plugin-sdk/host/log';

const JOB_FINISHED_TOPIC = 'ilium.progress/finished';

interface JobFinishedPayload {
  pane: number;
  job_id: string;
  status: 'done' | 'error';
  percent: number;
  message: string;
  details: string | null;
}

function describeJobNotice(payloadJson: string): string {
  try {
    const payload = JSON.parse(payloadJson) as JobFinishedPayload;
    return `job ${payload.job_id} ${payload.status}: ${payload.message}`;
  } catch {
    return 'job notice with an unreadable payload';
  }
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {
    log('info', 'job notice plugin ready');
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'topic-published' && event.val.topic === JOB_FINISHED_TOPIC) {
        log('info', describeJobNotice(event.val.payloadJson));
      }
      if (event.tag === 'events-dropped') {
        log('warn', 'topic events were dropped; re-read the job state');
      }
    }
    return [];
  },
});
```

### The progress payload

The kernel publishes `ilium.progress/finished` when a background job ends. The payload is:

```json
{"pane": 42, "job_id": "release-build", "status": "done", "percent": 100,
 "message": "Release build finished", "details": null}
```

`pane` is the node id as a JSON number. `status` is `"done"` or `"error"`. `percent` runs from 0 to 100. `message` is one line. `details` is text or `null`.

### Surface diagram

<figure>
<svg viewBox="0 0 400 230" role="img" aria-label="Topic delivery from the publisher through the kernel to the subscriber handler">
  <defs>
    <marker id="topics-sub-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="20" y="8" width="360" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="30" font-size="11" text-anchor="middle" fill="currentColor">ilium.progress publishes ilium.progress/finished</text>
  <line x1="200" y1="44" x2="200" y2="66" stroke="currentColor" marker-end="url(#topics-sub-arrow)"/>
  <rect x="20" y="66" width="360" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="88" font-size="11" text-anchor="middle" fill="currentColor">Kernel matches each subscriber pattern and grant</text>
  <line x1="200" y1="102" x2="200" y2="124" stroke="currentColor" marker-end="url(#topics-sub-arrow)"/>
  <rect x="20" y="124" width="360" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="146" font-size="11" text-anchor="middle" fill="currentColor">Subscriber queue: 1,024 events, drop oldest, counted</text>
  <line x1="200" y1="160" x2="200" y2="182" stroke="currentColor" marker-end="url(#topics-sub-arrow)"/>
  <rect x="20" y="182" width="360" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="204" font-size="11" text-anchor="middle" fill="currentColor">on-events receives topic-published in a batch</text>
</svg>
<figcaption>Only subscribers holding a matching grant receive the event, and only while active.</figcaption>
</figure>

### What the user sees

The review dialog shows one row per grant. A normal grant is checked by default. A bare `*` is a high-risk row: it is unchecked on a T2 or T3 install, and a T1 bundled plugin asks once at first activation.

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.topics 0.1.0 by Example Ltd                                    │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Listens to topics build/* <span class="t-dim">…………………………………………………</span> topics.subscribe:build/* │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests topics.subscribe:build/*, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

For a T1 bundled plugin with the high-risk `*` grant, the first activation shows one prompt, "Plugin <name> asks for:", with `Allow selected` and `Not now`. `Not now` asks again at the next server start, up to three times. Settings > Plugins lets the user grant it at any time.

### Edge cases and errors

| Situation | What happens | Signal to the plugin |
|---|---|---|
| Grant declined at install or in the prompt | No events for the pattern. | No error. The `granted` list omits the string. |
| Grant revoked while running | The plugin is deactivated with reason `revoked` and reactivated with the new set. | Lifecycle only. |
| Grant added while running | The plugin is deactivated with reason `granted` and reactivated with the new set. | Lifecycle only. |
| Queue full (1,024 events by default) | The oldest event is dropped. The next batch starts with `events-dropped`. | `events-dropped` with `event: "topic-published"`, `source` set to the topic and `count`. |
| Handler too slow | The `on-events` deadline is 20 ms by default and 50 ms at the T1 ceiling. A breach counts as a plugin failure. | Counted as a failure. The plugin keeps its other events. |
| Plugin not active | Nothing is delivered. | None. Use a `startup` or `topic:` trigger. |
| Plugin publishes a topic it also subscribes to | It receives its own publish when it holds the matching grant. | Ordinary `topic-published`. |
| Invalid pattern in the grant | The install check refuses the package. | `manifest.grant-wildcard` or `manifest.grant-argument-missing`. |
| Invalid entry in `subscribes` | The install check refuses the package. | The same codes. |

The publisher enforces two limits, and the subscriber inherits them. A topic payload is valid JSON of at most 64 KiB, and a publisher may call `publish` at most 100 times per second, with `rate-limited` and `retry-after-ms` above that.

### Who requests it

First-party plugins from the feature mapping use this capability:

- AI triggers (feature 44) hold `topics.subscribe:*`, as a T1 plugin with the user's grant.
- Semantic animation (feature 103) holds `topics.subscribe:ilium.organise/*`, to receive topics from the tree organisation plugin.

The progress monitor (feature 59) publishes `ilium.progress/finished`. It needs no subscription grant for that.

### See also

- [Messages and topics](/pages/api-messages-and-topics/) for the publish function, its errors and the full event list.
- [messages.peer](/pages/capability-messages-peer/) for messages sent to one named plugin.
- [services.use](/pages/capability-services/) for a request that expects a reply.
- [Events](/pages/api-events/) for every event and its delivery policy.
- [Errors and limits](/pages/reference-errors-and-limits/) for the error codes and rate rules.
- [Lifecycle](/pages/lifecycle/) for activation triggers and deactivation reasons.

Surface: `ilium:plugin@0.1.0`.

### Security

Topic payloads are cross-plugin data flow; see the toxic combination rule in [Threat model](/pages/reference-threat-model/) ([SEC-004]).
