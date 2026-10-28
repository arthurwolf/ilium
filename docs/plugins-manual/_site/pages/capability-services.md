# Capability: services

## services.use:&lt;name&gt; and services.provide:&lt;name&gt;

The `services.use:<name>` and `services.provide:<name>` grants let plugins call each other through named services. A provider exports `service-provider` and answers calls. A consumer calls the service by name, then takes the reply later. The payload is JSON. The kernel checks every payload against the schema that the provider declares in its manifest.

Plugin-defined services are `@unstable`. A plugin that uses them must list `plugin-services` in its manifest `unstable` key. Kernel-known services, such as `ilium:inference/provider`, have typed interfaces instead, and they are covered on their own pages.

### What you need

- **A grant on each side.** A consumer needs `services.use:<name>`. A provider needs `services.provide:<name>` and the `service-provider` export.
- **The `unstable` opt-in.** Add `"unstable": ["plugin-services"]` to the manifest of both plugins.
- **A language SDK.** Rust: crate `ilium-plugin-sdk`, module `ilium_plugin_sdk::host::services`. TypeScript: `@ilium/plugin-sdk/host/services`.
- **A side.** Services exist on the server and the client side. Each side has its own grants.

### Names and grants

A grant takes the service name as its argument. The first colon separates the grant name from the argument, so `services.use:ilium:inference/provider` is valid. A name has one of two forms:

- A plugin-defined service is `<plugin-id>/<name>`. The `<name>` part is 1 to 64 characters from `[a-z0-9-]`.
- A kernel-known typed interface is `ilium:<package>/<interface>`.

A version is never part of a grant. It appears only in `requires` and `provides`.

| Grant | Valid | Reason |
|---|---|---|
| `services.use:com.example.notes/search` | yes | consumer grant for one service |
| `services.provide:com.example.notes/search` | yes | provider grant for the same service |
| `services.use:ilium:inference/provider` | yes | kernel-known typed interface |
| `services.use` | no | `manifest.grant-argument-missing`: the argument is required |
| `services.use:com.example.notes/*` | no | `manifest.grant-wildcard`: wildcards are refused |
| `services.use:com.example.notes/search@0.1.0` | no | a version is never part of a grant |
| `services.call` | no | `manifest.grant-unknown`: `call` is a function, not a grant |

### The full surface

Functions and records of the service interface. The Rust names are the SDK names; the WIT names are the same in kebab-case.

| Item | Side and grant | Rust | TypeScript | Meaning |
|---|---|---|---|---|
| `call(service, method, payload-json)` | consumer, `services.use` | `services::call_request` returns `Result<RequestId, PluginError>`; `services::call(...).await` returns `Result<String, PluginError>` | `callRequest` returns `bigint`; `call` returns `Promise<string>` | Starts a call. Checks the grant, the schema and the size caps at once. |
| `take-reply(request)` | consumer | `services::take_reply` returns `Option<String>` | `takeReply` returns `string \| undefined` | Returns the JSON of an `ok` completion. Nothing for a failed or cancelled call, or on a second take. |
| `handle(call: service-call)` | provider export | `ServiceProvider::handle` returns `ServiceReply` | `handle` in `definePlugin` | Answers the call at once, or defers it. |
| `reply(call, outcome)` | provider, `services.provide` | `services::reply` returns `Result<(), PluginError>` | `reply(call, outcome)` | Answers a deferred call. `outcome` is `ok` with a JSON string, or `err` with a `PluginError`. |

The records:

| Type | Fields |
|---|---|
| `ServiceCall` (WIT `service-call`) | `call: u64`, `service: string`, `method: string`, `payload_json: String`, `caller: plugin-id` |
| `ServiceReply` (WIT `service-reply`) | `Done(String)` or `Deferred`. In TypeScript `{ tag: 'done', val }` or `{ tag: 'deferred' }` |
| `PluginError` (WIT `plugin-error`) | `code`, `message`, `retry_after_ms: Option<u32>` |

Call numbers (`call`) and request ids are `u64`, so TypeScript uses `bigint` for them.

<figure>
<svg viewBox="0 0 320 290" role="img" aria-label="Service call flow from consumer to kernel, provider and reply">
  <defs>
    <marker id="svc-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="8" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="32" text-anchor="middle" font-size="11" fill="currentColor">Consumer: call(service, method, json)</text>
  <line x1="160" y1="48" x2="160" y2="64" stroke="currentColor" marker-end="url(#svc-arrow)"/>
  <rect x="10" y="64" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="88" text-anchor="middle" font-size="11" fill="currentColor">Kernel: check grant, schema and size caps</text>
  <line x1="160" y1="104" x2="160" y2="120" stroke="currentColor" marker-end="url(#svc-arrow)"/>
  <rect x="10" y="120" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="144" text-anchor="middle" font-size="11" fill="currentColor">Provider: handle(call) returns done or deferred</text>
  <line x1="160" y1="160" x2="160" y2="176" stroke="currentColor" marker-end="url(#svc-arrow)"/>
  <rect x="10" y="176" width="300" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="160" y="200" text-anchor="middle" font-size="11" fill="currentColor">Deferred: provider calls reply later</text>
  <line x1="160" y1="216" x2="160" y2="232" stroke="currentColor" marker-end="url(#svc-arrow)"/>
  <rect x="10" y="232" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="256" text-anchor="middle" font-size="11" fill="currentColor">Consumer: take-reply, or the completion event</text>
</svg>
<figcaption>The dashed step applies only to deferred answers. Done answers skip it.</figcaption>
</figure>

### Manifest entries

A consumer lists the services it needs in `requires`. A provider lists each service it answers in `contributes.services.provides`, with a version and a schema file. The grant itself goes in `capabilities`.

Consumer manifest, with a kernel-known service:

```json
{
  "id": "com.example.search-ui",
  "unstable": ["plugin-services"],
  "requires": { "services": ["ilium:inference/provider@0.1.0"] },
  "capabilities": { "server": ["services.use:com.example.notes/search"] }
}
```

Provider manifest:

```json
{
  "id": "com.example.notes",
  "unstable": ["plugin-services"],
  "capabilities": { "server": ["services.provide:com.example.notes/search"] },
  "contributes": {
    "services": {
      "provides": [
        { "name": "com.example.notes/search", "version": "0.1.0",
          "schema": "schemas/search.json" }
      ]
    }
  }
}
```

The schema file maps each method to its `params` and `result` schemas. It uses the settings subset of the JSON schema, draft 2020-12. Allowed keywords are `type`, `properties`, `required`, `enum`, `title`, `description`, `default`, `minimum`, `maximum`, `pattern`, `maxLength` and `items` for arrays. Any other keyword makes install fail with `manifest.schema-keyword`.

```json
{
  "query": {
    "params": {
      "type": "object", "required": ["text"],
      "properties": {
        "text": { "type": "string", "maxLength": 200 },
        "limit": { "type": "integer", "minimum": 1, "maximum": 50 }
      }
    },
    "result": {
      "type": "object", "required": ["hits"],
      "properties": { "hits": { "type": "array", "items": { "type": "string" } } }
    }
  }
}
```

A component that exports `service-provider` without a `services.provide` grant is refused at install with `manifest.export-grant`.

### Worked example: a provider in Rust

This complete server-side provider answers `query` calls with a fixed hit list. A real provider parses `payload_json` and searches its notes. The kernel has already checked the payload against the schema, so the provider reads only the fields the schema declares.

**Rust**

```rust
use ilium_plugin_sdk::exports::{ServiceCall, ServiceReply};
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "server", exports = ["service-provider"])]
#[derive(Default)]
struct NotesSearch;

impl Lifecycle for NotesSearch {}

impl Handler for NotesSearch {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

impl ServiceProvider for NotesSearch {
    fn handle(&mut self, call: ServiceCall) -> ServiceReply {
        // The SDK has already validated call.payload_json against search.json.
        let _query_text = &call.payload_json;
        ServiceReply::Done(r#"{"hits":["2026-10-01 release plan"]}"#.to_string())
    }
}
```

The `Lifecycle` trait has default methods, so the empty `impl` is enough. The `Handler` export is required on every server component, even when the plugin handles no events.

### Worked example: a consumer in TypeScript

This server-side consumer calls the provider once on activation. It logs the reply, or the error code when the call fails.

**TypeScript**

```ts
import { definePlugin, PluginError, type Event } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';
import { call } from '@ilium/plugin-sdk/host/services';

async function searchNotes(text: string): Promise<void> {
  try {
    const reply = await call(
      'com.example.notes/search',
      'query',
      JSON.stringify({ text, limit: 5 }),
    );
    log('info', `search reply: ${reply}`);
  } catch (error) {
    if (error instanceof PluginError) {
      log('warn', `search failed: ${error.code}: ${error.message}`);
    }
  }
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate() {
    void searchNotes('release plan');
  },
  onEvents(batch: Event[]) {
    return [];
  },
});
```

`call` resolves with the reply JSON. The SDK resolves the promise when the completion arrives, and the `definePlugin` wrapper drains pending promises before it returns from each export.

### Deferred replies

A provider that cannot answer at once returns `Deferred` and keeps the call number. It answers later with `reply`. The call number is `u64`.

**Rust**

```rust
use ilium_plugin_sdk::exports::{ServiceCall, ServiceProvider, ServiceReply};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::services;

#[derive(Default)]
struct SlowSearch { waiting: Vec<u64> }

impl ServiceProvider for SlowSearch {
    fn handle(&mut self, call: ServiceCall) -> ServiceReply {
        self.waiting.push(call.call);
        ServiceReply::Deferred
    }
}

// Later, from a task or another export call. reply returns
// Result<(), PluginError>; it fails with not-found after the 30 s deadline.
fn answer_later(call_number: u64) {
    if let Err(error) = services::reply(call_number, Ok(r#"{"hits":[]}"#.to_string())) {
        log::log(Level::Warn, &format!("reply refused: {:?}", error.code));
    }
}
```

A deferred call that gets no reply within 30 seconds fails the caller's request with `timed-out`. A `reply` that arrives after that returns `not-found`.

### Errors and completions

A call returns a `request-id` or a synchronous error. Everything that needs the provider is reported in the completion of the request as a `request-completed` event, or to the Rust future.

| Code | When | Where it appears |
|---|---|---|
| `not-permitted` | consumer grant missing, declined or revoked | synchronous |
| `revoked` | the grant changed during the call | completion |
| `not-found` | no installed and enabled provider; `reply` for an unknown or expired call | synchronous |
| `invalid-argument` | payload over 1 MiB or fails the schema; `reply` over 1 MiB or fails the result schema | synchronous |
| `unsupported` | the method is not in the provider schema | synchronous |
| `busy` | the pending request queue is full | synchronous |
| `quota-exceeded` | CPU budget of the execution quota group spent, with `retry-after-ms` | synchronous |
| `timed-out` | the provider did not activate within `activate_deadline_ms` (2,000), or no reply within 30 seconds | completion |
| `cancelled` | the provider stopped or restarted before it answered | completion |
| `invalid-argument` | a `done` answer fails the provider's result schema | completion; the provider logs it at level error |
| the provider's own code | the provider answered with an error | completion |

Revocation, disable and upgrade behave as the other capabilities do. A grant revoked during a call makes that call return `revoked`. A disabled provider is treated as not installed, so new calls return `not-found`.

### Limits and timing

| Limit | Value |
|---|---|
| Payload size, call and reply | 1 MiB |
| Provider activation wait | 2,000 ms (`activate_deadline_ms`) |
| Reply deadline for a call | 30 seconds from the call |
| Service name after the slash | 1 to 64 characters from `[a-z0-9-]` |

A call that finds the provider installed but not running starts it with the trigger `service:<name>`. The call then waits for the provider's activation, then for its reply.

### Declined grants and edge cases

- **Consumer declined.** Every `call` returns `not-permitted`. Each refusal is written to the plugin log with the grant name.
- **Provider declined.** The provider is installed, but its service is never registered and its `handle` export is never called. Callers get `not-found`.
- **Provider not installed or disabled.** `call` returns `not-found` at once.
- **Provider cycle.** Two plugins that require each other's services are refused at install with `manifest.service-cycle` ([errors and limits](/pages/reference-errors-and-limits/)).
- **Failing provider.** A provider that stops or restarts before it answers fails its pending callers with `cancelled`. Three failures within 10 minutes disable it.

### Review dialog and what the user sees

The install dialog lists the consumer grant as a row of its own. The row is unchecked for high-risk grants. `services.use` and `services.provide` are normal risk, so they are checked by default. A provider sees the payload of every call made to it, so review providers before you grant them to other plugins.

<figure>
<pre class="term"><span class="t-dim">Review permissions: com.example.search-ui 0.1.0</span>
  Use the service com.example.notes/search
  services.use:com.example.notes/search
<span class="t-dim">[allow]  [decline]</span>
</pre>
<figcaption>Illustrative, not generated. The review row for a consumer grant. The service name is an example.</figcaption>
</figure>

When a declined grant blocks a call, the plugin log shows the refusal:

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs com.example.search-ui
<span class="t-warn">warn</span>  services.call com.example.notes/search
      <span class="t-err">not-permitted</span> (declined)
</pre>
<figcaption>Illustrative, not generated. The refusal line of a declined consumer grant.</figcaption>
</figure>

### Who uses it in the first-party plugins

Two first-party features use services. Feature 45, custom instruction inputs, is a consumer of `services.use:ilium.instructions/inputs`, and the `ilium.instructions` plugin provides it. Feature 83, the agent auto-compact trigger, is a consumer of `services.use:ilium.compact/trigger`, and the `ilium.compact` plugin provides it. Both providers declare `services.provide` for those names.

### See also

- [api-services](/pages/api-services/) for the complete function reference.
- [inference](/pages/capability-inference/) for calls to the kernel-known inference provider.
- [inference.provide](/pages/capability-inference-provide/) for the typed provider that the kernel already knows.
- [topics](/pages/capability-topics-subscribe/) for one-way messages instead of calls.
- [jobs.run](/pages/capability-jobs-run/) for background work that does not need a named caller.

Checked against `ilium:plugin@0.1.0` (API draft of 2026-10-11).

### Security

Service calls cross plugin boundaries; callers see the same error for a missing and an ungranted service ([SEC-373]).
