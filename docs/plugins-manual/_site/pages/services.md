# Services

## Services

A service is a named interface that one plugin provides and other plugins call. The caller sends a method name and a JSON payload. The provider answers with a JSON reply, at once or later. The plugins never link to each other. The kernel checks both grants and the payload, and it routes the call to the provider the user has enabled. A consumer can also declare that it needs a provider, so the kernel starts that provider first.

Version 0.1 has two kinds of service. Kernel-known interfaces, such as `ilium:inference/provider`, have typed WIT signatures. Plugin-defined services use the generic `services.call` form, which is unstable in 0.1. The call-side reference is [services API](/pages/api-services/).

## What you need

- **Grants.** A caller needs `services.use:<name>`. A provider needs `services.provide:<name>`. The inference provider needs `inference.provide` instead.
- **Manifest.** The provider lists `service-provider` in `exports` and its services in `contributes.services.provides`. The caller lists the services it needs in `requires.services`. Both plugins set `"unstable": ["plugin-services"]`, which is accepted only in dev mode for T2 and T3 plugins (see [Manifest reference](/pages/reference-manifest/)).
- **SDK.** The [Rust SDK](/pages/rust-sdk/) provides `ilium_plugin_sdk::host::services`. The [TypeScript SDK](/pages/typescript-sdk/) provides `@ilium/plugin-sdk/host/services`.

## Names, versions and grants

| Item | Form | Example |
|---|---|---|
| Kernel-known service | `ilium:<package>/<interface>` | `ilium:inference/provider` |
| Plugin-defined service | `<plugin-id>/<name>`; `<name>` is 1 to 64 characters from `a-z`, `0-9`, `-` | `com.example.titles/suggest` |
| Caller grant | `services.use:<name>` | `services.use:com.example.titles/suggest` |
| Provider grant | `services.provide:<name>` | `services.provide:com.example.titles/suggest` |
| Provider manifest entry | object with `name`, `version`, `schema` | see below |
| Caller manifest entry | string with a version, in `requires.services` | `com.example.titles/suggest@0.1.0` |

A grant carries the bare name, with no version and no wildcard. The first colon in a grant separates the grant from its argument, so `services.use:ilium:inference/provider` names `ilium:inference/provider`. A version appears only in `requires` and `provides`, always written `@0.1.0`.

## The registry

The kernel keeps one service registry, keyed by name. A provider enters it when it is installed, enabled and granted `services.provide`. A name has one provider. If a second enabled plugin declares a registered name, the registry keeps the first and never calls the second.

<figure>
<svg viewBox="0 0 420 170" role="img" aria-label="Service registry routing calls to a provider">
  <title>Service registry routing</title>
  <desc>Two consumers call a service through the registry. The registry checks both grants and routes each call to the provider the user enabled.</desc>
  <defs>
    <marker id="services-arrow" viewBox="0 0 6 6" refX="5" refY="3" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0,0 L6,3 L0,6 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="8" y="22" width="110" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="63" y="46" font-size="11" text-anchor="middle" fill="currentColor">Consumer A</text>
  <rect x="8" y="108" width="110" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="63" y="132" font-size="11" text-anchor="middle" fill="currentColor">Consumer B</text>
  <line x1="118" y1="42" x2="168" y2="72" stroke="currentColor" marker-end="url(#services-arrow)"/>
  <line x1="118" y1="128" x2="168" y2="100" stroke="currentColor" marker-end="url(#services-arrow)"/>
  <rect x="168" y="50" width="130" height="72" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="233" y="78" font-size="11" text-anchor="middle" fill="currentColor">Service registry</text>
  <text x="233" y="96" font-size="10" text-anchor="middle" fill="currentColor">checks both grants</text>
  <text x="233" y="110" font-size="10" text-anchor="middle" fill="currentColor">routes each call</text>
  <line x1="298" y1="86" x2="320" y2="86" stroke="currentColor" marker-end="url(#services-arrow)"/>
  <rect x="320" y="50" width="92" height="72" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="366" y="78" font-size="11" text-anchor="middle" fill="currentColor">Provider</text>
  <text x="366" y="96" font-size="10" text-anchor="middle" fill="currentColor">chosen by user</text>
</svg>
<figcaption>Consumers never link to the provider. The registry checks both grants and routes each call.</figcaption>
</figure>

## Declaring a provider

The provider's schema file maps each method to a `params` schema and a `result` schema. The schemas use the keyword subset of JSON Schema draft 2020-12 that the [settings screens](/pages/settings-screens/) also use.

```json
{
  "exports": ["service-provider"],
  "contributes": {
    "services": {
      "provides": [
        {
          "name": "com.example.titles/suggest",
          "version": "0.1.0",
          "schema": "schemas/suggest.json"
        }
      ]
    }
  },
  "unstable": ["plugin-services"]
}
```

```json
{
  "suggest": {
    "params": {
      "type": "object",
      "properties": { "text": { "type": "string" } },
      "required": ["text"]
    },
    "result": {
      "type": "object",
      "properties": { "title": { "type": "string" } },
      "required": ["title"]
    }
  }
}
```

## Declaring a consumer

The consumer names the service with its version in `requires.services`. Its activation waits for a compatible provider. A provider is compatible when its major version equals the required major version and its minor version is at least the required minor version. For major version 0 the rule is stricter, because 0.x versions are unstable: the minor version must be equal, and the patch version is ignored. A consumer that requires `0.1.0` accepts a provider at `0.1.0` or `0.1.5`, and refuses `0.9.0` and any `0.2.x` release. A consumer that requires `1.2.0` accepts `1.2.0` and `1.4.0`, and refuses `1.1.0` and `2.0.0` [SVC-006]. A provider has at most 64 calls in flight at once; a 65th call returns `busy` at the call [SVC-014].

```json
{
  "requires": { "services": ["com.example.titles/suggest@0.1.0"] },
  "unstable": ["plugin-services"]
}
```

## The call surface

| Name | Side | Rust | TypeScript | Grant | Meaning |
|---|---|---|---|---|---|
| `call` | caller | `services::call(service, method, payload_json).await` | `await call(service, method, payloadJson)` | `services.use` | Sends the call, waits and resolves with the reply JSON. Fails with the completion code. |
| `call-request` | caller | `services::call_request(...)` | `callRequest(...)` | `services.use` | Sends the call and returns a request id at once. |
| `take-reply` | caller | `services::take_reply(request)` | `takeReply(request)` | `services.use` | Returns the reply JSON once. `None` or `undefined` after a failure, a cancel or a second take. |
| `handle` | provider | `ServiceProvider::handle(&mut self, call)` | `handle(call)` in `definePlugin` | `services.provide` | Receives each checked call and returns a `ServiceReply`. |
| `reply` | provider | `services::reply(call, outcome)` | `reply(call, outcome)` | `services.provide` | Answers a deferred call with `Ok(json)` or `Err(error)`. |

| Type | Field or case | Meaning |
|---|---|---|
| `ServiceCall` | `call` (`u64`; `bigint` in TypeScript) | The number to pass back to `reply`. |
| `ServiceCall` | `service`, `method`, `payload_json` | The name, the method, and the payload already checked against `params`. |
| `ServiceCall` | `caller` | The id of the calling plugin. |
| `ServiceReply` | `Done(String)` or `{ tag: 'done', val }` | Answers now with JSON that matches `result`. |
| `ServiceReply` | `Deferred` or `{ tag: 'deferred' }` | Promises a later `reply` with the same call number. |

The `ServiceCall`, `ServiceReply` and `ServiceProvider` names live in `ilium_plugin_sdk::exports` in Rust. In TypeScript they come from `@ilium/plugin-sdk`.

## How a call travels

<figure>
<svg viewBox="0 0 420 284" role="img" aria-label="Steps of one service call from caller to provider and back">
  <title>One service call</title>
  <desc>The caller sends a call, the kernel checks grants and the payload, the provider answers now or later, and the caller takes the reply.</desc>
  <defs>
    <marker id="flow-arrow" viewBox="0 0 6 6" refX="5" refY="3" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0,0 L6,3 L0,6 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="12" y="10" width="212" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="118" y="31" font-size="10" text-anchor="middle" fill="currentColor">1. Caller: services.call(...)</text>
  <text x="244" y="31" font-size="10" fill="currentColor">Needs services.use grant</text>
  <line x1="118" y1="44" x2="118" y2="68" stroke="currentColor" marker-end="url(#flow-arrow)"/>
  <rect x="12" y="68" width="212" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="118" y="89" font-size="10" text-anchor="middle" fill="currentColor">2. Kernel: check grants, schema</text>
  <text x="244" y="89" font-size="10" fill="currentColor">No provider: not-found</text>
  <line x1="118" y1="102" x2="118" y2="126" stroke="currentColor" marker-end="url(#flow-arrow)"/>
  <rect x="12" y="126" width="212" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="118" y="147" font-size="10" text-anchor="middle" fill="currentColor">3. Provider: handle(call)</text>
  <text x="244" y="147" font-size="10" fill="currentColor">Inactive: started first</text>
  <line x1="118" y1="160" x2="118" y2="184" stroke="currentColor" marker-end="url(#flow-arrow)"/>
  <rect x="12" y="184" width="212" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="118" y="205" font-size="10" text-anchor="middle" fill="currentColor">4. Provider: done or deferred</text>
  <text x="244" y="205" font-size="10" fill="currentColor">Deferred: reply() later</text>
  <line x1="118" y1="218" x2="118" y2="242" stroke="currentColor" marker-end="url(#flow-arrow)"/>
  <rect x="12" y="242" width="212" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="118" y="263" font-size="10" text-anchor="middle" fill="currentColor">5. Caller: take-reply(request)</text>
  <text x="244" y="263" font-size="10" fill="currentColor">Returns the JSON once</text>
</svg>
<figcaption>One call. A refused call ends at step 2. Otherwise the reply reaches the caller at step 5.</figcaption>
</figure>

The payload is checked against `params` before the provider runs, and a failing payload never reaches it. A `Done` answer or a `reply` value must match `result`. The caller has 30 seconds from the call to receive its reply.

## Activation order

At startup the kernel starts service providers before the consumers that require them. A provider that is installed but not running is started by the first call to its service, through the trigger `service:<name>`. The call waits up to the provider's activation deadline, then for the reply. At shutdown, a provider stops after its consumers, in reverse activation order. A cycle, where two plugins each require the other, is refused at install.

<pre class="term">
<span class="t-prompt">$</span> ilium plugin logs com.example.notes-ui
<span class="t-dim">info</span>  services.call com.example.titles/suggest
      <span class="t-dim">provider inactive, activation started (2000 ms)</span>
<span class="t-err">error</span> services.call com.example.titles/suggest
      <span class="t-err">failed(timed-out)</span>: provider did not start
</pre>

Illustrative, not generated. The provider was installed but not running, and it did not start before its activation deadline.

## Errors and limits

| Code | Returned by | Meaning |
|---|---|---|
| `not-found` | `call` | No installed and enabled provider. Returned at once. |
| `not-permitted` | `call`, `reply` | The plugin lacks the grant. |
| `invalid-argument` | `call` | The payload is over 1 MiB or fails `params`. The provider is not called. |
| `unsupported` | `call` | The method is not in the schema. |
| `busy`, `quota-exceeded` | `call` | Too many pending requests, or the CPU budget is spent. |
| `invalid-argument` | `reply` | The reply is over 1 MiB or fails `result`. |
| `not-found` | `reply`, `take-reply` | The call is unknown, answered or expired. |
| `timed-out` | caller | No reply within 30 seconds, or the provider did not start in time. |
| `cancelled` | caller | The provider stopped, restarted or was disabled before answering. |
| provider's code | caller | A failed `reply` passes its `plugin-error` on to the caller. |

A `Done` answer that fails `result` completes the caller's request as `failed(invalid-argument)`, and the provider logs the failure at level error.

| Limit | Value |
|---|---|
| Payload of one call, or one reply | 1 MiB |
| Wait for the reply | 30 seconds from the call |
| Provider activation | `activate_deadline_ms`: 2 seconds by default, 5 seconds at most |

<div class="warn">
<p>Plugin-defined services are unstable in 0.1 and may change within 0.x. Update both plugins when the API changes.</p>
</div>

## Worked example: a provider and a caller

The provider answers short notes at once and defers long ones to a task. The caller submits a note when a command runs. Each plugin is its own `src/lib.rs`.

**Rust**

Provider:

```rust
use ilium_plugin_sdk::exports::{
    Handler, Lifecycle, ServiceCall, ServiceProvider, ServiceReply,
};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::services;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::task;

/// Notes shorter than this many bytes are answered at once.
const SHORT_NOTE_BYTES: usize = 200;

#[ilium_plugin(side = "server", exports = ["service-provider"])]
#[derive(Default)]
struct TitleProvider;

impl Lifecycle for TitleProvider {}

impl Handler for TitleProvider {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

impl ServiceProvider for TitleProvider {
    fn handle(&mut self, call: ServiceCall) -> ServiceReply {
        // The host has already checked the payload against the params schema.
        if call.payload_json.len() < SHORT_NOTE_BYTES {
            return ServiceReply::Done(r#"{"title":"Fix the parser"}"#.to_string());
        }
        let call_number = call.call;
        task::spawn(async move {
            // Stand-in for real work. A real provider computes the title first.
            let outcome = Ok(r#"{"title":"Long note"}"#.to_string());
            if let Err(error) = services::reply(call_number, outcome) {
                log::log(Level::Error, &error.message);
            }
        });
        ServiceReply::Deferred
    }
}
```

Caller:

```rust
use ilium_plugin_sdk::exports::{Handler, Lifecycle};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::services;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::task;

const TITLE_SERVICE: &str = "com.example.titles/suggest";

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct NoteCaller;

impl Lifecycle for NoteCaller {}

impl Handler for NoteCaller {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::CommandInvoked(invoked) = event else {
                continue;
            };
            if invoked.command == "com.example.notes/suggest-title" {
                task::spawn(ask_for_title("Fix the parser crash on empty input".to_string()));
            }
        }
        Vec::new()
    }
}

async fn ask_for_title(note: String) {
    // A real plugin builds the payload with a JSON library, so quotes in the note are escaped.
    let payload_json = format!(r#"{{"text":"{note}"}}"#);
    match services::call(TITLE_SERVICE, "suggest", &payload_json).await {
        Ok(reply_json) => log::log(Level::Info, &format!("title: {reply_json}")),
        Err(error) => log::log(Level::Warn, &format!("title failed: {}", error.message)),
    }
}
```

**TypeScript**

The same pair in TypeScript. The provider returns the `ServiceReply` union in the jco form. The caller awaits `call`, which resolves with the reply text or throws the completion code.

Provider, `src/index.ts`:

```ts
import { definePlugin, type Event, type ServiceCall, type ServiceReply } from '@ilium/plugin-sdk';
import { reply } from '@ilium/plugin-sdk/host/services';
import { log } from '@ilium/plugin-sdk/host/log';

// Notes shorter than this many characters are answered at once.
const SHORT_NOTE_CHARACTERS = 200;

export const { lifecycle, handler, serviceProvider } = definePlugin({
  side: 'server',
  onEvents(_batch: Event[]) {
    return [];
  },
  handle(call: ServiceCall): ServiceReply {
    if (call.payloadJson.length < SHORT_NOTE_CHARACTERS) {
      return { tag: 'done', val: JSON.stringify({ title: 'Fix the parser' }) };
    }
    answerLater(call.call);
    return { tag: 'deferred' };
  },
});

function answerLater(callNumber: bigint): void {
  // Stand-in for real work. A real provider computes the title first.
  queueMicrotask(() => {
    const outcome = reply(callNumber, { tag: 'ok', val: JSON.stringify({ title: 'Long note' }) });
    if (outcome.tag === 'err') {
      log('error', `reply failed: ${outcome.val.message}`);
    }
  });
}
```

Caller, `src/index.ts` of the second plugin:

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { call } from '@ilium/plugin-sdk/host/services';
import { log } from '@ilium/plugin-sdk/host/log';

const TITLE_SERVICE = 'com.example.titles/suggest';

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      const isTitleCommand =
        event.tag === 'command-invoked' && event.val.command === 'com.example.notes/suggest-title';
      if (isTitleCommand) {
        void askForTitle('Fix the parser crash on empty input');
      }
    }
    return [];
  },
});

async function askForTitle(note: string): Promise<void> {
  const payloadJson = JSON.stringify({ text: note });
  try {
    const replyJson = await call(TITLE_SERVICE, 'suggest', payloadJson);
    log('info', `title: ${replyJson}`);
  } catch (error: unknown) {
    log('warn', `title failed: ${String(error)}`);
  }
}
```

## When to use extension points instead

A service has one provider and many callers, and the caller pulls an answer. If many plugins should add to one plugin's behaviour, the owner declares an extension point and the others fill it. Use a collect point for data the owner reads, and a hook when the others' code must run inside the owner's flow. See [Extension points](/pages/api-extension-points/) and [Extending core plugins](/pages/extending-core-plugins/).

## Other ways two parts of a plugin talk

A plugin service is for two different plugins. Two other channels cover the cases that look similar.

| Need | Use | Grant |
|---|---|---|
| The server side asks its own client side a question only a client can answer (terminal size, clipboard, theme) | `client-requests` | None for own sides |
| One plugin runs a command in another running Ilium session | `sessions.list-live`, `sessions.send` | `sessions.cross`, high risk |
| Choose from a list that a plugin provides | A plugin catalogue, see [Catalogs](/pages/api-catalogs/) | `catalogs.provide` |

`client-requests` needs the request `kind` declared in `contributes.client_requests`, with an optional `timeout_ms` from 1 to 30,000 and 5,000 by default. The client side receives a `client-request` event and answers with `respond`. A client that is detached completes the request with `not-found`. No answer in time completes it with `timed-out`.

<figure>
<pre class="term">╭ client-requests ───────────────────────────────────────────────╮
│ server  <span class="t-hi">request</span>(client, "terminal-size", 2000 ms)              │
│ client  <span class="t-hi">client-request</span> {call: 7, kind: terminal-size}          │
│ client  <span class="t-hi">respond</span>(7, ok)                                         │
│ server  <span class="t-ok">✓ request-completed: ok</span>                                │
│ <span class="t-dim">no client attached</span>  <span class="t-err">✗ failed(not-found)</span>                        │
│ <span class="t-dim">no answer in time</span>   <span class="t-err">✗ failed(timed-out)</span>                        │
╰────────────────────────────────────────────────────────────────╯</pre>
<figcaption>Illustrative, not generated. The order of events of one client request and its two failure outcomes.</figcaption>
</figure>

**Rust**

```rust
use ilium_plugin_sdk::host::client_requests;
use ilium_plugin_sdk::prelude::*;

// Server side: ask the attached client for its terminal size.
pub async fn ask_client_size(client: ClientId) -> Result<String, PluginError> {
    client_requests::request(client, "terminal-size", "{}", 2_000).await
}

// Client side: answer the request. `call` is the number in the client-request event.
pub fn answer_size(call: u64, columns: u16, rows: u16) {
    let payload = format!("{{\"columns\":{columns},\"rows\":{rows}}}");
    client_requests::respond(call, Ok(payload));
}
```

**TypeScript**

```ts
import { request, respond } from '@ilium/plugin-sdk/host/client-requests';

// Server side: ask the attached client for its terminal size.
export async function askClientSize(client: bigint): Promise<string> {
  return request(client, 'terminal-size', '{}', 2_000);
}

// Client side: answer the request. `call` is the number in the client-request event.
export function answerSize(call: bigint, columns: number, rows: number): void {
  respond(call, { tag: 'ok', val: JSON.stringify({ columns, rows }) });
}
```

## Kernel-known services: the inference provider

The inference provider is the typed service that most consumers use. A plugin that exports `inference-provider` must hold `inference.provide`. Its WIT shape is:

```wit
// Provider exports
record provider-call { call: u64, request: completion-request }
complete: func(call: provider-call) -> result<_, plugin-error>;
models: func() -> list<string>;

// Provider import, needs inference.provide
provider-reply: func(call: u64, outcome: result<completion, plugin-error>);
```

`complete` accepts the call and returns at once. Network work runs in a task that answers through `provider-reply` with the same call number. A consumer that must wait for a provider declares `requires.services` with `ilium:inference/provider@0.1.0`. The full contract is in [Inference provider](/pages/capability-inference-provide/).

## What the user sees

- The install review lists each grant, such as `services.use:com.example.titles/suggest`.
- If the user declines `services.provide`, the provider stays installed but is never registered. Every caller gets `not-found`.
- Failed calls appear in the caller's plugin log.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs com.example.notes-ui
<span class="t-warn">warn</span>  services.call com.example.titles/suggest
      <span class="t-err">not-found</span>: no installed and enabled provider
</pre>
<figcaption>Illustrative, not generated. The caller's log line for a service with no enabled provider.</figcaption>
</figure>

## Common mistakes

- Expecting `call` to wait for a provider that is not installed or is disabled. It returns `not-found` at once.
- Putting a version in a grant. Grants carry the bare name.
- Replying to a deferred call after 30 seconds. The late `reply` returns `not-found`.
- Sending large payloads. Each call and each reply is capped at 1 MiB, so pass identifiers and short summaries.
- Assuming that the consumer can read the provider's data. The consumer sees only the reply it asked for.

### Provider lifecycle

A provider stops after its consumers, in reverse activation order. When a provider fails, its consumers move to `waiting` and requests in flight complete as `cancelled`. They are not counted as consumer failures. When the provider restarts, waiting consumers resume. See [Lifecycle](/pages/lifecycle/) and [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/). [LIF-280]

- [LIF-280] A provider failure MUST NOT count as a failure of its consumers. Test: trap a provider and read the consumer window.


Requirement ids for this page: [SVC-002], [SVC-003], [SVC-004], [SVC-006], [SVC-009], [SVC-012], [SVC-013], [SVC-014]. See the [requirements index](/pages/reference-requirements/).

## See also

- [Services API](/pages/api-services/)
- [Capability: services](/pages/capability-services/)
- [Inference provider](/pages/capability-inference-provide/)
- [Lifecycle](/pages/lifecycle/)
- [Background jobs](/pages/background-jobs/)
- [Errors and limits](/pages/reference-errors-and-limits/)

API version checked: `ilium:plugin@0.1.0`.

Requirements for this page: [Services and topics requirements](/pages/reference-requirements-services-topics/).
