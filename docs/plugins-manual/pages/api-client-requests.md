---
layout: page
title: client requests
permalink: /pages/api-client-requests/
---

## client requests

A plugin has up to two sides that run in different processes: the server side, which lives with the session, and the client side, which lives in the terminal window the user attached. Some facts exist only in the client: the size of its terminal, its theme, the contents of its clipboard. The server side cannot read them. Without this interface a plugin would have to build a question and answer from two `messages` and a hand-made correlation id. The `client-requests` interface gives the server side a typed question and the client side a typed answer.

Use it when the server side needs one answer from one client now. Use [messages and topics](/pages/api-messages-and-topics/) for news that nobody has to answer. Client routing stays in the kernel.

<figure>
<svg viewBox="0 0 440 280" role="img" aria-label="Sequence of a client request between the server side and the client side of one plugin">
  <defs>
    <marker id="api-creq-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <text x="70" y="16" font-size="12" text-anchor="middle" fill="currentColor">Server side</text>
  <text x="230" y="16" font-size="12" text-anchor="middle" fill="currentColor">Kernel</text>
  <text x="390" y="16" font-size="12" text-anchor="middle" fill="currentColor">Client side</text>
  <line x1="70" y1="24" x2="70" y2="270" stroke="currentColor" stroke-dasharray="3 3"/>
  <line x1="230" y1="24" x2="230" y2="270" stroke="currentColor" stroke-dasharray="3 3"/>
  <line x1="390" y1="24" x2="390" y2="270" stroke="currentColor" stroke-dasharray="3 3"/>
  <line x1="70" y1="44" x2="228" y2="44" stroke="currentColor" marker-end="url(#api-creq-arrow)"/>
  <text x="150" y="38" font-size="10" text-anchor="middle" fill="currentColor">request(client, kind, payload)</text>
  <line x1="228" y1="64" x2="72" y2="64" stroke="currentColor" marker-end="url(#api-creq-arrow)"/>
  <text x="150" y="82" font-size="10" text-anchor="middle" fill="currentColor">request-id</text>
  <line x1="232" y1="104" x2="388" y2="104" stroke="currentColor" marker-end="url(#api-creq-arrow)"/>
  <text x="310" y="98" font-size="10" text-anchor="middle" fill="currentColor">client-request(call, kind, payload)</text>
  <rect x="320" y="116" width="140" height="22" rx="4" fill="none" stroke="currentColor"/>
  <text x="390" y="131" font-size="10" text-anchor="middle" fill="currentColor">client reads its state</text>
  <line x1="388" y1="160" x2="232" y2="160" stroke="currentColor" marker-end="url(#api-creq-arrow)"/>
  <text x="310" y="154" font-size="10" text-anchor="middle" fill="currentColor">respond(call, ok(json))</text>
  <line x1="228" y1="190" x2="72" y2="190" stroke="currentColor" marker-end="url(#api-creq-arrow)"/>
  <text x="150" y="184" font-size="10" text-anchor="middle" fill="currentColor">request-completed(request, ok)</text>
  <line x1="70" y1="218" x2="228" y2="218" stroke="currentColor" marker-end="url(#api-creq-arrow)"/>
  <text x="150" y="212" font-size="10" text-anchor="middle" fill="currentColor">take-answer(request)</text>
  <line x1="228" y1="240" x2="72" y2="240" stroke="currentColor" marker-end="url(#api-creq-arrow)"/>
  <text x="150" y="258" font-size="10" text-anchor="middle" fill="currentColor">JSON text</text>
</svg>
<figcaption>The server side asks, the kernel delivers the question to the named client, the client answers once, and the server side takes the answer after the completion event.</figcaption>
</figure>

### What you need

- **Grant:** none. The interface is always granted between the two sides of one plugin (not for other plugins). Reading the clipboard inside an answer still needs `clipboard.read` on the client side.
- **Manifest:** every request kind is declared in `contributes.client_requests` (at most 16 per plugin). The client side exports `handler` and receives the `client-request` event.
- **Both sides:** the two manifests must agree. A kind or an export that is inconsistent between the sides fails the install with `manifest.client-request-unknown`.
- **Language:** Rust with `ilium-plugin-sdk` (`host::client_requests`), or TypeScript with `@ilium/plugin-sdk` (`@ilium/plugin-sdk/host/client-requests`).

### Manifest

```json
{
  "contributes": {
    "client_requests": [
      { "id": "terminal-size", "timeout_ms": 2000 },
      { "id": "clipboard-text" }
    ]
  }
}
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `id` | string | required | The request kind. Used as `kind` in `request` and in the `client-request` event. |
| `timeout_ms` | integer, 1 to 30,000 | none | A ceiling for this kind. This key has no default; the host reply deadline is 5,000 ms when the caller passes no `timeout-millis`, and the caller can set 1 to 30,000 ms. |

### Surface

```wit
// Excerpt of ilium:host/client-requests in reference-wit
// ilium:host/client-requests
// server side
request:     func(client: client-id, kind: string, payload-json: string, timeout-millis: u32) -> result<request-id, plugin-error>;
take-answer: func(request: request-id) -> option<string>;
// client side
respond:     func(call: u64, outcome: result<string, plugin-error>) -> result<_, plugin-error>;
```

| WIT function | Side | Rust | TypeScript |
|---|---|---|---|
| `request` | server | `client_requests::request(client: ClientId, kind: &str, payload_json: &str, timeout_millis: u32) -> Result<RequestId, PluginError>` | `request(client: bigint, kind: string, payloadJson: string, timeoutMillis: number): bigint` |
| `take-answer` | server | `client_requests::take_answer(request: RequestId) -> Option<String>` | `takeAnswer(request: bigint): string \| undefined` |
| `respond` | client | `client_requests::respond(call: u64, outcome: Result<String, PluginError>)` | `respond(call: bigint, outcome: { tag: 'ok'; val: string } \| { tag: 'err'; val: PluginError }): void` |

`client` is a `client-id` from the `client-attached` event (or from the list of attached clients). `timeout-millis` is 1 to 30,000. The payload and the answer are JSON texts that the two sides agree on.

#### Event

| Event | Payload | Side | Rust | TypeScript | Delivery |
|---|---|---|---|---|---|
| `client-request` | `call` (u64), `kind`, `payload-json` | client, for its own declared kinds | `Event::ClientRequest(ClientRequestEvent { call, kind, payload_json })` | `{ tag: 'client-request', val: { call, kind, payloadJson } }` | never dropped |

The client side may start the answer in a later batch. The `call` number stays valid until the request times out or the client answers once.

The activation trigger `client-request:<kind>` activates the client side of a plugin when its server side asks (see [lifecycle](/pages/lifecycle/)).

### How the request completes

The `request-id` completes through the usual `request-completed` event (see [Requests and outcomes](/pages/requests-and-outcomes/)). On `ok`, `take-answer` returns the JSON text. Otherwise the completion is `failed` with a code:

| Completion | When |
|---|---|
| `ok` | The client answered with `ok`. `take-answer` returns the text. |
| `failed(not-found)` | The client detached before it could answer, or the kind is not declared. |
| `failed(timed-out)` | No answer within the timeout. |
| `failed(<code>)` | The client answered with `err(plugin-error)`. The code is the one the client chose. |

### Worked example: the terminal size

The server side needs the size of the client's terminal to lay out a report. It asks the attached client and takes the answer. The client side reads its terminal size from its own state.

**Rust, server side**

```rust
use ilium_plugin_sdk::host::client_requests;
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Reporter {
    client: Option<ClientId>,
    pending: Option<RequestId>,
}

impl Lifecycle for Reporter {}

impl Handler for Reporter {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::ClientAttached(client) => {
                    self.client = Some(client);
                    self.pending =
                        client_requests::request(client, "terminal-size", "{}", 2_000).ok();
                }
                Event::ClientDetached(client) if self.client == Some(client) => {
                    self.client = None;
                }
                Event::RequestCompleted(done) if Some(done.request) == self.pending => {
                    self.pending = None;
                    match client_requests::take_answer(done.request) {
                        Some(json) if done.outcome.is_ok() => use_size(&json),
                        _ => log_unavailable(&done.outcome),
                    }
                }
                _ => {}
            }
        }
        Vec::new()
    }
}
```

**Rust, client side**

```rust
use ilium_plugin_sdk::host::client_requests;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::ClientRequestEvent;

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct ReporterClient;

impl Lifecycle for ReporterClient {}

impl Handler for ReporterClient {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if let Event::ClientRequest(ClientRequestEvent { call, kind, .. }) = event {
                let outcome = match kind.as_str() {
                    "terminal-size" => Ok(terminal_size_json()),
                    other => Err(PluginError::not_found(format!("no handler for {other}"))),
                };
                client_requests::respond(call, outcome);
            }
        }
        Vec::new()
    }
}
```

**TypeScript, server side**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import * as clientRequests from '@ilium/plugin-sdk/host/client-requests';

let pending: bigint | undefined;

export const { handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'client-attached') {
        pending = clientRequests.request(event.val, 'terminal-size', '{}', 2_000);
      } else if (event.tag === 'request-completed' && event.val.request === pending) {
        pending = undefined;
        const json = event.val.outcome.tag === 'ok' ? clientRequests.takeAnswer(event.val.request) : undefined;
        if (json === undefined) logUnavailable(event.val.outcome);
        else useSize(json);
      }
    }
    return [];
  },
});
```

**TypeScript, client side**

```ts
import { definePlugin, PluginError, type Event, type Submission } from '@ilium/plugin-sdk';
import * as clientRequests from '@ilium/plugin-sdk/host/client-requests';

export const { handler } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'client-request') continue;
      const { call, kind } = event.val;
      if (kind === 'terminal-size') {
        clientRequests.respond(call, { tag: 'ok', val: terminalSizeJson() });
      } else {
        clientRequests.respond(call, { tag: 'err', val: new PluginError('not-found', `no handler for ${kind}`) });
      }
    }
    return [];
  },
});
```

`use_size`, `log_unavailable`, `terminal_size_json` and their TypeScript equivalents are the plugin's own helpers. The client reads its size from the same state it draws with.

### Worked example: asking for the clipboard

The kind `clipboard-text` lets a server-side command use the clipboard of the client that ran it. The client side holds `clipboard.read` and answers with the text. If the user declined the grant, `read-text` returns nothing and the client answers with an error instead of an empty text.

**Rust, client side**

```rust
use ilium_plugin_sdk::host::{client_requests, clipboard};
use ilium_plugin_sdk::prelude::*;

fn answer_clipboard(call: u64) {
    let outcome = match clipboard::read_text() {
        Some(text) => Ok(format!(r#"{{"text":{}}}"#, json_string(&text))),
        None => Err(PluginError::not_found("clipboard has no text".into())),
    };
    client_requests::respond(call, outcome);
}
```

**TypeScript, client side**

```ts
import * as clientRequests from '@ilium/plugin-sdk/host/client-requests';
import * as clipboard from '@ilium/plugin-sdk/host/clipboard';
import { PluginError } from '@ilium/plugin-sdk';

export function answerClipboard(call: bigint): void {
  const text = clipboard.readText();
  if (text === undefined) {
    clientRequests.respond(call, { tag: 'err', val: new PluginError('not-found', 'clipboard has no text') });
    return;
  }
  clientRequests.respond(call, { tag: 'ok', val: JSON.stringify({ text }) });
}
```

### What the user sees

The user sees only the effect. A command such as `ilium voice say` is a server-side CLI command (see [cli](/pages/api-cli/)) that asks the client side to act, and the plugin log shows the exchange.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs example.reporter --tail 3
<span class="t-dim">2026-10-12 10:21:07</span> <span class="t-ok">info </span> client-requests.request client 2 terminal-size
<span class="t-dim">2026-10-12 10:21:07</span> <span class="t-ok">info </span> client-request answered terminal-size in 4 ms
<span class="t-dim">2026-10-12 10:21:09</span> <span class="t-warn">warn </span> client 3 detached: request failed not-found
<span class="t-prompt">$</span> <span class="t-dim">3 lines shown</span>
</pre>
<figcaption>One request is answered, and one fails because the client detached. Illustrative, not generated.</figcaption>
</figure>

### Error codes

| Function | Code | When |
|---|---|---|
| `request` | `not-found` | The client id is unknown, or the kind is not declared in `client_requests`. |
| completion | `not-found` | The client detached before it answered. |
| completion | `timed-out` | No answer within the timeout. |
| `take-answer` | none | Returns `none` when the answer was already taken or the request failed. |
| `respond` | `not-found` | The call id is unknown, already answered, or the request timed out or was cancelled. The answer is dropped and `respond` returns the error to the client plugin. |
| `respond` | `invalid-argument` | The outcome text is not valid JSON. |
| manifest | `manifest.client-request-unknown` | The sides disagree about an export or a `client_requests` entry. |
| manifest | `manifest.contribution-limit` | More than 16 `client_requests` entries. |

### Limits

| Item | Value |
|---|---|
| Request kinds per plugin | 16 |
| Timeout | 1 to 30,000 ms; the manifest default is 5,000 ms |
| Answer hold time | 30 s, or until taken once |

### Edge cases

| Situation | Result |
|---|---|
| Several clients are attached | Each `request` names one client. Ask each client with its own request. |
| The client side is not active | The kernel activates it for the kind (`client-request:<kind>`). |

### See also

- [messages and topics](/pages/api-messages-and-topics/) for one-way news between the two sides.
- [Requests and outcomes](/pages/requests-and-outcomes/) for the completion model.
- [clipboard](/pages/api-clipboard/) for the call used in the clipboard example.
- [cli](/pages/api-cli/) for the server-side commands that ask a client.
- [Lifecycle](/pages/lifecycle/) for activation triggers.
- [Errors and limits](/pages/reference-errors-and-limits/) for the common error codes.

Interface version: `ilium:host/client-requests`, version 0.1.
