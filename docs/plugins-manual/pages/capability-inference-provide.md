---
layout: page
title: "Capability: inference.provide"
permalink: /pages/capability-inference-provide/
---

## inference.provide

The `inference.provide` grant lets a plugin act as the AI model provider that the user selects in Settings. Features in other plugins call `inference.complete` (see [inference](/pages/capability-inference/)). The kernel routes each call to the selected provider by calling its `complete` export. The provider does its work and answers later through the `inference.provider-reply` import, with the same call number. The kernel never waits inside `complete`, so a slow network request does not block anything.

A provider receives every prompt that the user's features send to it. Say where the prompts go in the plugin description and on its settings page.

### What you need

- **The grant `inference.provide`.** It takes no argument and has normal risk, so the install dialog checks it by default.
- **One grant `http:<provider-host>` per host** the provider calls, for example `http:api.provider.example`. Requests to those hosts also pass the AI admission gate.
- **The export `inference-provider`** in the manifest `exports` list, for the side that implements it (`server` or `client`). The component also exports `lifecycle` and `handler`. Those two are required and are never listed.
- **A toolchain.** Rust stable 1.90 or later with the `wasm32-wasip2` target, plus `ilium-plugin-sdk` and `serde_json` (the Rust example uses it). Or Bun 1.2 or later with `@ilium/plugin-sdk`.
- **A settings field for the API key**, declared with `"secret": true` in the settings schema. Settings masks the value, the host redacts it from logs, and only the owning plugin can read it (P3).

### Grant syntax

The grant takes no argument.

| Grant | Valid | Reason |
|---|---|---|
| `inference.provide` | yes | the only form |
| `inference.provide:example` | no | the grant takes no argument |
| `inference-provider` | no | that is the export name, not a grant |

### Surface

The provider implements two exports. Both are required when the manifest lists `inference-provider`.

| Name | Rust | TypeScript | Meaning |
|---|---|---|---|
| `inference-provider.complete` (export) | `fn complete(&mut self, call: ProviderCall) -> Result<(), PluginError>` | `complete(call)` | Accepts one request. `Ok` accepts it; an `Err` fails the caller's request with that error. The reply comes later. |
| `inference-provider.models` (export) | `fn models(&mut self) -> Vec<String>` | `models()` | The model names the provider offers. Consumers read them with `inference.models`. |
| `inference.provider-reply` (import) | `inference::provider_reply(call, outcome)` | `providerReply(call, outcome)` | Answers one call with `Ok(Completion)` or `Err(PluginError)`. It returns nothing and never blocks. |

The Rust names come from `ilium_plugin_sdk::exports` (the export traits and records) and `ilium_plugin_sdk::host::inference` (the import and the records). The TypeScript names come from `@ilium/plugin-sdk` and `@ilium/plugin-sdk/host/inference`.

| Record | Field | Rust | TypeScript | Meaning |
|---|---|---|---|---|
| `provider-call` | `call` | `u64` | `bigint` | The kernel's call number. Pass it back unchanged. |
| | `request` | `CompletionRequest` | `CompletionRequest` | The request, as below. |
| `completion-request` | `messages` | `Vec<ChatMessage>` | `ChatMessage[]` | The conversation in order. Never empty. |
| | `max_output_tokens` | `Option<u32>` | `number` or `undefined` | `None` asks for the model's maximum output. |
| | `json_schema` | `Option<String>` | `string` or `undefined` | A JSON Schema document as text. The reply is still plain text. |
| | `purpose` | `String` | `string` | The feature name, for example `note-tags`. The gate and the privacy log use it. |
| `chat-message` | `role` | `String` | `string` | `system`, `user` or `assistant`, as sent. |
| | `content` | `String` | `string` | The text of the message. |
| `completion` | `text` | `String` | `string` | The reply exactly as the provider wrote it. Up to 4 MiB. |
| | `model` | `String` | `string` | The model that answered. |
| | `input_tokens` | `u32` | `number` | Tokens in the request. |
| | `output_tokens` | `u32` | `number` | Tokens in the reply. |

TypeScript passes a WIT `result` as a tagged object: `{ tag: 'ok', val: completion }` or `{ tag: 'err', val: pluginError }`.

### How a request flows

<figure>
<svg viewBox="0 0 420 190" role="img" aria-label="Provider call flow: the kernel calls complete, the provider makes an HTTP request, the provider calls provider-reply, and the caller receives the completion">
<defs>
<marker id="ipv-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
<path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
</marker>
</defs>
<rect x="20" y="10" width="380" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="210" y="29" text-anchor="middle" font-size="11" fill="currentColor">1. Kernel calls complete(call) on the provider</text>
<line x1="210" y1="40" x2="210" y2="56" stroke="currentColor" marker-end="url(#ipv-arrow)"/>
<rect x="20" y="56" width="380" height="30" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="210" y="75" text-anchor="middle" font-size="11" fill="currentColor">2. Provider calls its own service over HTTP</text>
<line x1="210" y1="86" x2="210" y2="102" stroke="currentColor" marker-end="url(#ipv-arrow)"/>
<rect x="20" y="102" width="380" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="210" y="121" text-anchor="middle" font-size="11" fill="currentColor">3. Provider calls provider-reply(call, outcome)</text>
<line x1="210" y1="132" x2="210" y2="148" stroke="currentColor" marker-end="url(#ipv-arrow)"/>
<rect x="20" y="148" width="380" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="210" y="167" text-anchor="middle" font-size="11" fill="currentColor">4. The caller receives the completion or the error</text>
</svg>
<figcaption>The provider answers asynchronously. The call number ties the reply to the request that the kernel sent.</figcaption>
</figure>

The `complete` export must return at once. Work that needs the network runs in a task (`task::spawn` in Rust, a promise in TypeScript), and the task answers through `provider_reply`. A reply whose call number is unknown, already answered or expired makes `provider-reply` return `not-found` to the provider; the kernel changes nothing else.

### Worked example 1: a Rust provider

The manifest lists only the optional export. `lifecycle` and `handler` are required, so the component implements them and the manifest does not name them.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.provider",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": ["inference-provider"]
  },
  "capabilities": {
    "server": ["inference.provide", "http:api.provider.example"]
  }
}
```

The provider reads its API key from its settings, sends the conversation to the provider's HTTP API, and answers the call:

**Rust**

```rust
use ilium_plugin_sdk::exports::{InferenceProvider, ProviderCall};
use ilium_plugin_sdk::host::http::{self, Header, HttpRequest, HttpResponse};
use ilium_plugin_sdk::host::inference::{self, Completion};
use ilium_plugin_sdk::host::settings;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::task;

const PROVIDER_URL: &str = "https://api.provider.example/v1/chat";

#[ilium_plugin(side = "server", exports = ["inference-provider"])]
#[derive(Default)]
struct ExampleProvider;

impl Lifecycle for ExampleProvider {}

impl Handler for ExampleProvider {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

impl InferenceProvider for ExampleProvider {
    fn complete(&mut self, call: ProviderCall) -> Result<(), PluginError> {
        let settings: serde_json::Value =
            serde_json::from_str(&settings::get_json()).unwrap_or_default();
        let api_key = settings["apiKey"].as_str().unwrap_or_default().to_owned();
        let messages: Vec<serde_json::Value> = call
            .request
            .messages
            .iter()
            .map(|message| {
                serde_json::json!({ "role": message.role, "content": message.content })
            })
            .collect();
        let body = serde_json::json!({
            "messages": messages,
            "max_tokens": call.request.max_output_tokens,
        });
        let request = HttpRequest {
            method: "POST".into(),
            url: PROVIDER_URL.into(),
            headers: vec![
                Header { name: "authorization".into(), value: format!("Bearer {api_key}") },
                Header { name: "content-type".into(), value: "application/json".into() },
            ],
            body: Some(body.to_string().into_bytes()),
            timeout_millis: 60_000,
        };
        let call_number = call.call;
        task::spawn(async move {
            let outcome = match http::fetch(request).await {
                Ok(response) => parse_reply(response),
                Err(error) => Err(error),
            };
            inference::provider_reply(call_number, outcome);
        });
        Ok(())
    }

    fn models(&mut self) -> Vec<String> {
        vec!["example-model-1".to_owned()]
    }
}

fn parse_reply(response: HttpResponse) -> Result<Completion, PluginError> {
    let failure = |message: String| PluginError {
        code: ErrorCode::Network,
        message,
        retry_after_ms: None,
    };
    if response.status != 200 {
        return Err(failure(format!("provider answered HTTP {}", response.status)));
    }
    let reply: serde_json::Value =
        serde_json::from_slice(&response.body).map_err(|error| failure(error.to_string()))?;
    Ok(Completion {
        text: reply["text"].as_str().unwrap_or_default().to_owned(),
        model: reply["model"].as_str().unwrap_or_default().to_owned(),
        input_tokens: u32::try_from(reply["input_tokens"].as_u64().unwrap_or(0)).unwrap_or(0),
        output_tokens: u32::try_from(reply["output_tokens"].as_u64().unwrap_or(0)).unwrap_or(0),
    })
}
```

The settings schema declares the key as `"apiKey": { "type": "string", "secret": true }`.

### Worked example 2: a TypeScript provider

The same manifest applies. The TypeScript entry module exports the same three names: `lifecycle`, `handler` and `inferenceProvider`.

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { fetch as httpFetch } from '@ilium/plugin-sdk/host/http';
import { providerReply, type Completion } from '@ilium/plugin-sdk/host/inference';
import { getJson } from '@ilium/plugin-sdk/host/settings';

const PROVIDER_URL = 'https://api.provider.example/v1/chat';

function networkFailure(message: string) {
  return {
    tag: 'err' as const,
    val: { code: 'network' as const, message, retryAfterMs: undefined },
  };
}

// The SDK's fetch is renamed on import so it does not shadow the global fetch.
async function answer(callNumber: bigint, apiKey: string, body: string): Promise<void> {
  let outcome;
  try {
    const response = await httpFetch({
      method: 'POST',
      url: PROVIDER_URL,
      headers: [
        { name: 'authorization', value: `Bearer ${apiKey}` },
        { name: 'content-type', value: 'application/json' },
      ],
      body: new TextEncoder().encode(body),
      timeoutMillis: 60_000,
    });
    if (response.status !== 200) {
      outcome = networkFailure(`provider answered HTTP ${response.status}`);
    } else {
      const reply = JSON.parse(new TextDecoder().decode(response.body));
      const completion: Completion = {
        text: reply.text,
        model: reply.model,
        inputTokens: reply.input_tokens,
        outputTokens: reply.output_tokens,
      };
      outcome = { tag: 'ok' as const, val: completion };
    }
  } catch (error) {
    outcome = networkFailure(String(error));
  }
  providerReply(callNumber, outcome);
}

export const { lifecycle, handler, inferenceProvider } = definePlugin({
  side: 'server',
  onEvents(): Submission[] {
    return [];
  },
  complete(call) {
    const settings = JSON.parse(getJson()) as { apiKey?: string };
    const messages = call.request.messages.map((message) => ({
      role: message.role,
      content: message.content,
    }));
    const body = JSON.stringify({ messages, max_tokens: call.request.maxOutputTokens ?? null });
    // answer() catches every failure itself, so the promise never rejects.
    void answer(call.call, settings.apiKey ?? '', body);
  },
  models: () => ['example-model-1'],
});
```

### Edge cases and errors

| Situation | What happens |
|---|---|
| No provider selected | The caller's `inference.complete` returns `not-found` at once. It does not wait. |
| Selected provider installed but not running | The call starts it and waits up to its activation deadline (2,000 ms by default). If it does not start in time, the caller's request fails with `timed-out`. |
| Grant declined at install | The plugin installs, is never selected, and Settings shows it as unavailable. |
| Export listed without the grant | Install refuses the package with `manifest.export-grant`. |
| Grant revoked while installed | The plugin is deactivated and re-activated with the reduced grant set. It is no longer selected. |
| `complete` returns `Err` | The caller's request fails with that error. |
| Reply after the caller's 120 s deadline | The caller already has `failed(timed-out)`. The late `provider-reply` returns `not-found` to the provider. |
| Second reply for one call | The second call returns `not-found`. |
| Reply text over 4 MiB | `provider-reply` returns `invalid-argument`. |
| Reply sent without the grant | The reply is dropped and the host writes a refusal line to the plugin log. The caller times out. |
| Gate refuses the purpose or provider | The caller gets `not-permitted`. The same gate applies to the provider's own HTTP calls to its host. |
| Gate throttles the call | The caller gets `rate-limited` with `retry-after-ms`, the wait the gate asks for before a retry. |
| HTTP to a host without a grant | `http.fetch` returns `not-permitted` and the plugin log records the refusal. |
| Trap or deadline breach in `complete` | A failure under the restart rule. It follows the [restart and disable rule](/pages/reference-errors-and-limits/#restart-and-disable-rule): restart after 1 s, then 5 s, and disable at the third failure inside 10 minutes. |
| Trap or deadline breach in `models` | The same failure under the restart rule as for `complete`. The caller's `inference.models` lists no models for this provider on that call (an empty list, not an error); other providers still list theirs. |
| Empty `messages` or input over 4 MiB | The caller's `inference.complete` returns `invalid-argument` before the provider sees it. |

Codes a provider can return from `complete` are the plugin's own: use `invalid-argument` when the request breaks a rule the provider checks (for example, no messages), and `unsupported` when the provider does not offer the requested model.

### Limits

| Limit | Value | Source |
|---|---|---|
| Request input, one call | 4 MiB | [Error codes and limits](/pages/reference-errors-and-limits/) |
| `json_schema` | 64 KiB | [Error codes and limits](/pages/reference-errors-and-limits/) |
| Reply text | 4 MiB | [Error codes and limits](/pages/reference-errors-and-limits/) |
| Caller's deadline | 120 s | [Error codes and limits](/pages/reference-errors-and-limits/) |
| `complete` and `models` deadline | `command_deadline_ms`, 50 ms by default | [Resource limits](/pages/reference-errors-and-limits/#resource-limits-in-the-manifest) |
| Activation deadline of a stopped provider | 2,000 ms by default | [Resource limits](/pages/reference-errors-and-limits/#resource-limits-in-the-manifest) |
| Memory of the provider instance | 32 MiB by default | [Resource limits](/pages/reference-errors-and-limits/#resource-limits-in-the-manifest) |

### What the user sees

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.inference 0.1.0 by Example Ltd                                 │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Offers a language model to others <span class="t-dim">………………………………………………</span> inference.provide │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests inference.provide, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

- **Install review.** The dialog shows one row per grant, and the rows use the wording of the [capability table](/pages/reference-capabilities/). Both grants are checked by default because they have normal risk.
- **Settings.** Settings > Plugins lists the provider with its capability list. Settings > Plugins > (provider) shows it as unavailable if the grant was declined. Requests go to the provider that the user selected in Settings, and the plugin cannot choose another one.
- **Secret field.** The API key appears masked. The host never stores it in the snapshot (P3).
- **Privacy log.** Each request that the gate admits adds one line to the privacy log: the consumer's plugin id, the `purpose`, the provider, the model and the token counts. Settings > Privacy shows that log (api-inference).
- **Revocation.** `ilium plugin grants com.example.provider --revoke inference.provide` takes the grant away. The plugin is re-activated without it and is no longer selected.

### See also

- [inference](/pages/capability-inference/) for the consumer side and the grant that calls it.
- [api-inference](/pages/api-inference/) for the calls, the request and completion records, and the errors that callers see.
- [capability-http](/pages/capability-http/) for the provider's own network calls and the grant that allows them.
- [api-http](/pages/api-http/) for the admission gate and the `http` function.
- [capability-services](/pages/capability-services/) for how a provider is activated on demand.
- [api-settings](/pages/api-settings/) for the settings schema and the `secret` field.

Checked against `ilium:plugin@0.1.0` (API draft of 2026-10-11).

### Security

`inference.provide` has normal risk in the capability table, but the provider sees every prompt other plugins send, so the install dialog states that in the grant's review wording. See [Threat model](/pages/reference-threat-model/) ([SEC-007]) and [User security and privacy](/pages/user-plugin-security-and-privacy/).
