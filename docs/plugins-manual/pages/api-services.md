---
layout: page
title: services
permalink: /pages/api-services/
---

## services

A service is a named interface that one plugin provides and other plugins call. A caller sends a method name and a JSON payload. The provider answers with a JSON string, either at once or later, and the kernel delivers the answer to the caller as a `request-completed` event. Services let a feature in one plugin use a feature in another without knowing its code. The kernel checks both grants, the payload schema and the deadlines on every call.

<div class="warn">
<p>Plugin-defined services are unstable (<code>@unstable</code>) and may change in a later release. Build on one only with a plan to update it. Both the provider and the caller list <code>"plugin-services"</code> in the manifest <code>unstable</code> array, including core plugins that provide a service. The kernel-known services <code>ilium:inference/provider</code> and the editor provider have typed interfaces instead, see the <a href="/pages/api-inference/">inference</a> page.</p>
</div>

### What you need

- **Grants.** The caller needs `services.use:<name>`. The provider needs `services.provide:<name>`. The same name string appears in both grants and in the manifest.
- **Manifest.** The provider lists the service in `contributes.services.provides` with a schema file and lists `service-provider` in the `exports` of its side. The caller lists the service in `requires.services`, so that activation waits until a provider runs.
- **Worlds.** The `services` import and the `service-provider` export exist in `ilium:server/base` and `ilium:client/base`. The scene world has neither.
- **Toolchain.** Rust: stable 1.90 or later with the `wasm32-wasip2` target and the `ilium-plugin-sdk` crate. TypeScript: Bun 1.2 or later and `@ilium/plugin-sdk`. Nothing is installed globally.

### Names, grants and manifest entries

| Item | Form | Example |
|---|---|---|
| Plugin-defined service name | `<plugin-id>/<name>`; `<name>` is 1 to 64 characters from `a-z`, `0-9` and `-` | `com.example.titles/suggest` |
| Kernel-known service name | `ilium:<package>/<interface>` | `ilium:inference/provider` |
| First-party plugin service name | the plugin-defined form, with the providing plugin's dotted id before the slash | `ilium.instructions/inputs`, `ilium.compact/trigger` |
| Caller grant | `services.use:<name>` | `services.use:com.example.titles/suggest` |
| Provider grant | `services.provide:<name>` | `services.provide:com.example.titles/suggest` |
| Provider manifest entry | object in `contributes.services.provides`; fields in [Manifest reference, services](/pages/reference-manifest/#services) | `{ "name": "com.example.titles/suggest", "version": "0.1.0", "schema": "schemas/suggest.json" }` |
| Caller manifest entry | `requires.services` with a minimum version | `"requires": { "services": ["com.example.titles/suggest@0.1.0"] }` |

A version appears in `requires` and `provides` only. A grant never carries a version. The prefix of a plugin-defined name is the id of the providing plugin.

The schema file maps each method to a `params` schema and a `result` schema. The subset of JSON Schema is the one the settings pages use:

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

### Interface

```wit
// ilium:host/services, imported by the caller and by the provider
call: func(service: string, method: string, payload-json: string)
  -> result<request-id, plugin-error>;
take-reply: func(request: request-id) -> option<string>;
reply: func(call: u64, outcome: result<string, plugin-error>)
  -> result<_, plugin-error>;

// service-provider export (with-service fragment)
variant service-reply { done(string), deferred }
record service-call {
  call: u64, service: string, method: string, payload-json: string, caller: plugin-id,
}
handle: func(call: service-call) -> service-reply;
```

The SDK names of the calls on this page, including the `_request` / `Request` starting forms and the `_json` / `Json` names, follow [Wrapper names in the SDKs](/pages/reference-wit/#wrapper-names-in-the-sdks).

### Functions

| Function | Grant | Rust | TypeScript | What it does |
|---|---|---|---|---|
| `call` | `services.use:<name>` | `services::call(service, method, payload_json).await` | `await call(service, method, payloadJson)` | Calls a method and resolves with the JSON reply, or fails with the completion code. The raw form returns a request id: `services::call_request` and `callRequest`. |
| `take-reply` | `services.use:<name>` | `services::take_reply(request)` | `takeReply(request)` | Returns the JSON reply once. Returns `None` or `undefined` when the call failed or the reply was already taken. |
| `reply` | `services.provide:<name>` | `services::reply(call, Ok(json))` | `reply(call, { tag: 'ok', val: json })` | Answers a call that `handle` deferred. Synchronous. |

The `ServiceCall` and `ServiceReply` types and the `ServiceProvider` trait are in `ilium_plugin_sdk::exports` in Rust. In TypeScript, the types come from `@ilium/plugin-sdk` and the provider is the `handle` method of `definePlugin`.

| Field or case | Type | Meaning |
|---|---|---|
| `ServiceCall.call` | `u64` (`bigint` in TypeScript) | The number the provider passes to `reply` for a deferred answer. |
| `ServiceCall.service`, `method` | `String` | The service name and the method named by the caller. |
| `ServiceCall.payload_json` | `String` | The payload. The host has already checked it against `params`. |
| `ServiceCall.caller` | plugin id | The plugin that made the call. |
| `ServiceReply::Done(String)` | JSON string | Answers at once. The value must match `result`. |
| `ServiceReply::Deferred` | none | The provider promises a later `reply` with the same call number. |

### Rules for payloads, replies and deadlines

- The host checks the payload against `params` before the provider runs. A failing payload returns `invalid-argument` at once and the provider is not called. A method that is not in the schema returns `unsupported`.
- A payload and a reply are each capped at 1 MiB.
- A `reply` value that fails the `result` schema is refused with `invalid-argument`. A `done` answer that fails it completes the caller's request as `failed(invalid-argument)`, and the provider logs the failure at level error.
- A caller that waits for a reply has 30 seconds from the call. After that, its request fails with `timed-out`. A `reply` that arrives later returns `not-found`.
- A provider that is installed but not running is started by the call. The call waits up to the provider's activation deadline (`activate_deadline_ms`, 2 seconds by default at T2 and T3) and then waits for the reply. If the activation deadline passes first, the request fails with `timed-out`.
- A provider that is not installed or is disabled returns `not-found` at once.

### Example: Rust provider and caller

The provider offers `com.example.titles/suggest`. It answers at once for short notes and defers longer ones to a task that calls `reply` later. The caller is a second plugin that submits a note when a command runs.

Provider, `src/lib.rs`:

```rust
use ilium_plugin_sdk::exports::{ServiceCall, ServiceReply};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::services;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::task;

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
        // The host has already checked the payload against the schema.
        if call.payload_json.len() < 200 {
            return ServiceReply::Done(r#"{"title":"Fix the parser"}"#.to_string());
        }
        start_long_title(call.call);
        ServiceReply::Deferred
    }
}

fn start_long_title(call: u64) {
    task::spawn(async move {
        // Stand-in for a job. A real provider calls reply when the job ends.
        let outcome = Ok(r#"{"title":"Long note"}"#.to_string());
        if let Err(error) = services::reply(call, outcome) {
            log::log(Level::Error, &error.message);
        }
    });
}
```

Caller, a second plugin with `services.use:com.example.titles/suggest`:

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::services;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::task;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct NoteCaller;

impl Lifecycle for NoteCaller {}

impl Handler for NoteCaller {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::CommandInvoked(invoked) = event else { continue };
            if invoked.command == "com.example.notes/suggest-title" {
                task::spawn(ask_for_title("Fix the parser crash on empty input".to_string()));
            }
        }
        Vec::new()
    }
}

async fn ask_for_title(note: String) {
    // Escape the note with a JSON library before you format it in a real plugin.
    let payload = format!(r#"{{"text":"{note}"}}"#);
    match services::call("com.example.titles/suggest", "suggest", &payload).await {
        Ok(reply) => log::log(Level::Info, &format!("reply: {reply}")),
        Err(error) => log::log(Level::Warn, &format!("title failed: {}", error.message)),
    }
}
```

The caller's manifest has `"requires": { "services": ["com.example.titles/suggest@0.1.0"] }` and the capability `services.use:com.example.titles/suggest` in its `server` capabilities.

### Example: TypeScript provider and caller

The same service in TypeScript. The provider uses the `handle` method of `definePlugin`, and the caller awaits `call`, which resolves with the reply string.

Provider, `src/server.ts`:

```ts
import { definePlugin, type ServiceCall, type ServiceReply } from '@ilium/plugin-sdk';
import { reply } from '@ilium/plugin-sdk/host/services';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler, serviceProvider } = definePlugin({
  side: 'server',
  onEvents() {
    return [];
  },
  handle(call: ServiceCall): ServiceReply {
    // The host has already checked the payload against the schema.
    if (call.payloadJson.length < 200) {
      return { tag: 'done', val: JSON.stringify({ title: 'Fix the parser' }) };
    }
    startLongTitle(call.call);
    return { tag: 'deferred' };
  },
});

function startLongTitle(callNumber: bigint): void {
  // Stand-in for a job. A real provider calls reply when the job ends.
  void Promise.resolve()
    .then(() => {
      reply(callNumber, { tag: 'ok', val: JSON.stringify({ title: 'Long note' }) });
    })
    .catch((error: unknown) => log('error', `reply failed: ${String(error)}`));
}
```

Caller, `src/server.ts` of a second plugin:

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { call } from '@ilium/plugin-sdk/host/services';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      const command = event.tag === 'command-invoked' ? event.val.command : undefined;
      if (command === 'com.example.notes/suggest-title') {
        void askForTitle('Fix the parser crash on empty input');
      }
    }
    return [];
  },
});

async function askForTitle(note: string): Promise<void> {
  const payloadJson = JSON.stringify({ text: note });
  try {
    const reply = await call('com.example.titles/suggest', 'suggest', payloadJson);
    log('info', `reply: ${reply}`);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    log('warn', `title failed: ${message}`);
  }
}
```

A caller with `services.use` and `requires.services` gets a `request-completed` for every call, whether the provider answers at once or later. The `await` form hides that event, so the plugin never sees it.

### What the user sees

The review dialog of a T2 or T3 install lists both grants: "Use the service `<name>`" for the caller and "Provide the service `<name>`" for the provider. Nothing else is shown to the user for a service call. The caller's log lines appear in `ilium plugin logs`:

<pre class="term">
<span class="t-prompt">$</span> ilium plugin logs com.example.notes --tail 3
<span class="t-dim">12:04:11</span> info  reply: {"title":"Fix the parser"}
<span class="t-dim">12:04:19</span> info  reply: {"title":"Long note"}
<span class="t-dim">12:04:20</span> warn  title failed: payload fails schema
</pre>

Illustrative, not generated. Caller log lines for a short note, a long note that was deferred, and a payload that the provider's schema refused.

### Diagram: service call sequence

<figure>
<svg viewBox="0 0 340 200" role="img" aria-label="Service call sequence between caller, kernel and provider">
<title>Service call sequence</title>
<desc>Time runs downwards across three lanes: the caller, the kernel and the provider. The provider answers at once, or returns deferred and calls reply later.</desc>
<defs>
<marker id="api-services-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<line x1="50" y1="24" x2="50" y2="196" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="170" y1="24" x2="170" y2="196" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="290" y1="24" x2="290" y2="196" stroke="currentColor" stroke-dasharray="3 3"/>
<text x="50" y="16" font-size="11" text-anchor="middle" fill="currentColor">Caller</text>
<text x="170" y="16" font-size="11" text-anchor="middle" fill="currentColor">Kernel</text>
<text x="290" y="16" font-size="11" text-anchor="middle" fill="currentColor">Provider</text>
<line x1="50" y1="40" x2="170" y2="40" stroke="currentColor" marker-end="url(#api-services-arrow)"/>
<text x="110" y="36" font-size="10" text-anchor="middle" fill="currentColor">services.call</text>
<line x1="170" y1="64" x2="290" y2="64" stroke="currentColor" marker-end="url(#api-services-arrow)"/>
<text x="230" y="60" font-size="10" text-anchor="middle" fill="currentColor">handle(call)</text>
<line x1="290" y1="88" x2="170" y2="88" stroke="currentColor" marker-end="url(#api-services-arrow)"/>
<text x="230" y="84" font-size="10" text-anchor="middle" fill="currentColor">done(json)</text>
<line x1="170" y1="112" x2="50" y2="112" stroke="currentColor" marker-end="url(#api-services-arrow)"/>
<text x="110" y="108" font-size="10" text-anchor="middle" fill="currentColor">request-completed</text>
<line x1="290" y1="136" x2="170" y2="136" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#api-services-arrow)"/>
<text x="230" y="132" font-size="10" text-anchor="middle" fill="currentColor">deferred</text>
<line x1="290" y1="160" x2="170" y2="160" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#api-services-arrow)"/>
<text x="230" y="156" font-size="10" text-anchor="middle" fill="currentColor">reply(call, json)</text>
<line x1="170" y1="184" x2="50" y2="184" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#api-services-arrow)"/>
<text x="110" y="180" font-size="10" text-anchor="middle" fill="currentColor">request-completed</text>
</svg>
<figcaption>The caller gets one request-completed whether the provider answers at once or later. The dashed steps apply only to deferred answers.</figcaption>
</figure>

### Edge cases and error codes

Errors that come back at once from `call` or `reply` are returned as a `plugin-error`. Errors found later arrive in the caller's completion as `failed(code)`.

| Code | When | Retry |
|---|---|---|
| `not-permitted` | The caller lacks `services.use:<name>`, or the provider lacks `services.provide:<name>`. | No |
| `not-found` | No installed and enabled provider (at call). The call id is unknown, answered or expired (at `reply`). | No |
| `invalid-argument` | Payload over 1 MiB or failing `params`. Reply over 1 MiB or failing `result`. | No, fix the call |
| `unsupported` | The method is not in the provider's schema. | No |
| `busy` | The plugin already has 64 pending requests (`pending_requests`). | Yes, later |
| `quota-exceeded` | The CPU budget of the execution quota group is spent. `retry-after-ms` is set. | Yes, after the delay |
| `failed(timed-out)` | No reply 30 seconds after the call, or the provider did not start in time. | Yes |
| `failed(cancelled)` | The provider stopped, restarted or was disabled before it answered. | Maybe |
| `failed(<code>)` | The provider returned that code from `done` or from a failed job. | As that code says |

Further edge cases:

- Two calls to the same service return two request ids. Each completion carries its own request, so take each reply by its id.
- `take-reply` gives the JSON once. A second call for the same request returns `None` or `undefined`.
- A deferred reply that arrives after 30 seconds returns `not-found`. The provider should log it and drop the work.
- A provider that restarts after a failure (see the restart and disable rule) loses its deferred calls. Their callers receive `failed(cancelled)`. If the restart was caused by the provider breaching its call deadline (`command_deadline_ms`: 50 ms by default, 100 ms at the T1 ceiling), the caller of the breaching call receives `failed(internal)` (the deadline breach is a trap, not a missing reply, so it is never `timed-out`) and every other deferred caller receives `failed(cancelled)`.

### When to use extension points instead

A service has one provider. If several plugins should add data or handlers to one owner, declare an extension point. See [extension points](/pages/api-extension-points/).

### Used by

- Custom instruction inputs (feature 45) use `services.use:ilium.instructions/inputs`.
- The agent auto-compact trigger reader (feature 83) uses `services.use:ilium.compact/trigger`.

### Services of the host and catalogue services

Two more things use the service machinery in this contract.

- **Catalogues of another plugin.** `catalogs.query` on the catalogue of plugin `P` needs `services.use:catalog/P`. The provider is the `catalog-provider` export, not a `service-provider` method, so the plugin declares it in `exports`. See [catalogs](/pages/api-catalogs/).
- **Typed host services.** Some services have their own typed interfaces rather than JSON payloads: `inference`, the editor provider, `geocode`, `vector` and `catalogs`. They are listed in [geocode](/pages/api-geocode/) and [inference](/pages/api-inference/); a plugin that serves one of them uses the matching export.

| Need | Interface | Grant |
|---|---|---|
| Call another plugin's JSON service | `services.call` | `services.use:<name>` |
| Read another plugin's catalogue | `catalogs.query` | `services.use:catalog/<plugin-id>` |
| Ask a client a question | `client-requests.request` | none |
| Run a command in another session | `sessions.send` | `sessions.cross` |

Deadlines for the provider exports are fixed: `query-catalog` 20 ms, `menu-items` 20 ms, `list-virtual-children` 20 ms and `tokens` 10 ms per call. A breach counts like any deadline breach; `menu-items` and `tokens` fall back to static content instead of failing the action.

**Rust**

```rust
use ilium_plugin_sdk::host::catalogs::{self, CatalogQuery};

// manifest: "client": ["services.use:catalog/example.layouts"]
fn other_layouts() -> Vec<String> {
    let query = CatalogQuery { catalog: "example.layouts/layouts".into(), filter: String::new(), cursor: None, limit: 50 };
    catalogs::query_async(&query).map(|id| vec![format!("request {id:?} started")]).unwrap_or_default()
}
```

**TypeScript**

```ts
import { queryAsync } from '@ilium/plugin-sdk/host/catalogs';

// manifest: "client": ["services.use:catalog/example.layouts"]
export const otherLayouts = () =>
  queryAsync({ catalog: 'example.layouts/layouts', filter: '', cursor: undefined, limit: 50 });

Requirement ids for this page: [SVC-001], [SVC-005], [SVC-007], [SVC-008], [SVC-010], [SVC-011]. See the [requirements index](/pages/reference-requirements/).

### See also

- [inference](/pages/api-inference/) for the typed provider interface of `ilium:inference/provider`.
- [exports](/pages/api-exports/) for `service-provider` and its `with-service` fragment.
- [capability: services](/pages/capability-services/) for the grant rules and the review dialog.
- [extension points](/pages/api-extension-points/) for the many-contributors alternative.
- [requests and outcomes](/pages/requests-and-outcomes/) for the completion event and `request-result`.
- [errors and limits](/pages/reference-errors-and-limits/) and [reference: errors and limits](/pages/reference-errors-and-limits/) for the full code table.
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/) for the module layout of each language.
- [packages](/pages/packages/) and [reference: manifest](/pages/reference-manifest/) for the manifest keys used above.

Checked against `ilium:plugin@0.1.0`.

## Production additions: T3-abi-api

- **[API-209]** A service call chain deeper than 4 hops, or a cycle where a plugin is already on the chain, MUST be refused with `reentrant` before any callee runs.
