---
layout: page
title: http
permalink: /pages/api-http/
---

## http

The `http` import sends an HTTP request to a host that the plugin has been granted, and it hands the response back to the plugin. A network call is slow, so the raw function returns a `request-id` at once. The response is then taken with `take-response` after the `request-completed` event arrives. The SDK wraps this in one `await` call per request, so most plugins never see the request id. Requests to a configured AI provider also pass the admission gate before they leave the host.

**You need:**

- A grant `http:<host-pattern>` for each host the plugin calls, on the side that makes the request. Examples: `http:api.example.com` and `http:*.openstreetmap.org`.
- A manifest entry for each grant, in the `capabilities` list of that side. Grants are per side: a server-side request needs a server-side grant.
- A Rust or TypeScript plugin built with the SDK. The module is `ilium_plugin_sdk::host::http` in Rust and `@ilium/plugin-sdk/host/http` in TypeScript.
- Approval at install. The review dialog lists each host as "Connect to `<host>`". Bundled plugins (T1) are granted the hosts their manifest declares, without a dialog.

```json
"capabilities": { "client": ["http:api.example.com"] }
```

The SDK names of the calls on this page, including the `_request` / `Request` starting forms and the `_json` / `Json` names, follow [Wrapper names in the SDKs](/pages/reference-wit/#wrapper-names-in-the-sdks).

### Surface

The WIT interface is `ilium:host/http`. Every function, type and event it covers is listed here.

| WIT function | Rust | TypeScript | Returns | Meaning |
|---|---|---|---|---|
| `fetch` | `http::fetch(request).await` | `fetch(request)` | `Result<HttpResponse, PluginError>` / `Promise<HttpResponse>` | Sends the request and resolves with the response. Use this in most code. |
| `fetch` (raw) | `http::fetch_request(request)` | `fetchRequest(request)` | `Result<RequestId, PluginError>` / `bigint` | Starts the request and returns a `request-id`. The completion arrives as an event. |
| `take-response` | `http::take_response(id)` | `takeResponse(id)` | `Option<HttpResponse>` / `HttpResponse \| undefined` | Returns the response once, for a completed request. |

| Type | Field | Type in Rust / TypeScript | Meaning |
|---|---|---|---|
| `HttpRequest` (`http-request`) | `method` | `String` / `string` | `GET`, `HEAD`, `POST`, `PUT`, `PATCH`, `DELETE` or `OPTIONS`, in upper case. |
| | `url` | `String` / `string` | An absolute `https://` URL. `http://` is allowed only for loopback. At most 8 KiB. |
| | `headers` | `Vec<Header>` / `Header[]` | Request headers. At most 64. |
| | `body` | `Option<Vec<u8>>` / `Uint8Array \| undefined` | Request body, at most 8 MiB. `None` or `undefined` sends no body. |
| | `timeout_millis` | `u32` / `number` | Required. From 1 to 300,000 ms. |
| `Header` (`header`) | `name`, `value` | `String` / `string` | One header. |
| `HttpResponse` (`http-response`) | `status` | `u16` / `number` | The HTTP status code, for example 200 or 404. |
| | `headers` | `Vec<Header>` / `Header[]` | Headers the server sent. |
| | `body` | `Vec<u8>` / `Uint8Array` | Response body, at most `http_response_mib`. |

| Event | Payload | Meaning |
|---|---|---|
| `request-completed` | `request-result` with `request` set to `call(request-id)` and `outcome` | Sent for each raw `fetch`. `ok` means the response is ready to take. `failed(plugin-error)` carries the code. The event is never dropped. |

A plugin that uses the raw path reads the completion in `on_events` and calls `take_response`. The SDK removes completions that an awaited future consumes, so the async wrapper never sees them.

### Grant forms

The argument of `http` is a host pattern.

| Form | Matches | Example |
|---|---|---|
| Exact host, optional port | That host only | `http:api.example.com`, `http:api.example.com:8443` |
| `*.` and a domain | Any subdomain at any depth, not the bare domain | `http:*.openstreetmap.org` |
| Loopback | `localhost`, `127.0.0.1`, `[::1]`; plain HTTP allowed | `http:localhost` |
| Setting | The host currently stored in the plugin's own setting `<key>` | `http:setting:feed_host` |
| Refused | A bare `*`, a wildcard anywhere else, a scheme or a path | `http:*`, `http:https://api.example.com/v1` |

Install refuses a bare `*`. A redirect follows the same rule for each hop.

`http:setting:<key>` names one of the plugin's own setting keys (otherwise the manifest fails with `manifest.grant-argument`). The setting is a `text` row with `item_format` `url` or `host`, or a `list` of them. The host is checked at `fetch` and `open-stream` time. When the stored value changes to a host the user has not confirmed, the user is asked once before the first request (SEC-074), and until then calls to it return `not-permitted`. Wildcards in the stored value are refused. The full flow, with the confirmation dialog, is on [http streams](/pages/api-http-streams/).

### Politeness

The host keeps requests polite on behalf of every plugin, so a plugin does not implement its own back-off tables.

| Mechanism | What the plugin sees |
|---|---|
| Per-host rate limiter | The request is not failed at once. Its Completion outcome is `rate-limited` with `retry-after-ms` set. |
| `Retry-After` | The failure carries `retry-after-ms`; wait that long before the next call to the host. |
| ETag cache | The host sends `If-None-Match` for a URL it has fetched before. A `304` is returned to the plugin as the cached `200`, with the full body. The plugin never handles a `304`. |

### Streaming

`fetch` waits for the whole body. For server-sent events, newline-delimited JSON or a long download, open a stream instead: `open-stream`, `close-stream` and the events `http-stream-data` and `http-stream-closed`. The grant is the same. See [http streams](/pages/api-http-streams/). An unstable WebSocket form exists there too.

### Limits

| Limit | Value | Behaviour over the limit |
|---|---|---|
| URL length | 8 KiB | `invalid-argument` |
| Headers per request | 64 | `invalid-argument` |
| Request body | 8 MiB | `invalid-argument` |
| `timeout_millis` | 1 to 300,000 | `invalid-argument` |
| Response body | `http_response_mib`: 8 by default, 64 at the tier 1 ceiling, 8 at the tier 2 and 3 ceiling | Completion `failed(quota-exceeded)`; no body is delivered |
| Pending requests per plugin | 64 by default (`pending_requests`; ceiling 256 at tier 1) | `busy` |
| Completed result held for `take-response` | 30 s, or until taken once | Later takes return `none`, which the wrapper turns into `not-found` |
| Connections for the whole session | 32 | Extra requests queue inside the host; no error |

### Worked example in Rust

This client-side plugin fetches a status page once, when it activates. The wrapper returns a response or an error, so the code reads like ordinary sequential code.

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::http::{self, Header, HttpRequest};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::exports::Activation;

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct StatusCheck;

impl Lifecycle for StatusCheck {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        task::spawn(check_status());
        Ok(())
    }
}

impl Handler for StatusCheck {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

async fn check_status() {
    let request = HttpRequest {
        method: "GET".to_string(),
        url: "https://api.example.com/v1/status".to_string(),
        headers: vec![Header {
            name: "accept".to_string(),
            value: "application/json".to_string(),
        }],
        body: None,
        timeout_millis: 5_000,
    };
    match http::fetch(request).await {
        Ok(response) if response.status == 200 => {
            log::log(Level::Info, &format!("status: {} bytes", response.body.len()));
        }
        Ok(response) => {
            log::log(Level::Warn, &format!("status request returned {}", response.status));
        }
        Err(error) if error.code == ErrorCode::TimedOut => {
            log::log(Level::Warn, "status request timed out");
        }
        Err(error) => {
            log::log(Level::Error, &format!("status request failed: {}", error.message));
        }
    }
}
```

### Worked example in TypeScript

The same plugin in TypeScript. `definePlugin` drains the promise queue after each export call, so the request finishes before the host reads the plugin's output. The SDK function is imported as `httpFetch` so that it does not shadow the global `fetch`.

```ts
import { definePlugin, type PluginError } from '@ilium/plugin-sdk';
import { fetch as httpFetch, type HttpRequest } from '@ilium/plugin-sdk/host/http';
import { log } from '@ilium/plugin-sdk/host/log';

async function checkStatus(): Promise<void> {
  const request: HttpRequest = {
    method: 'GET',
    url: 'https://api.example.com/v1/status',
    headers: [{ name: 'accept', value: 'application/json' }],
    body: undefined,
    timeoutMillis: 5000,
  };
  try {
    const response = await httpFetch(request);
    if (response.status !== 200) {
      log('warn', `status request returned ${response.status}`);
      return;
    }
    log('info', `status: ${response.body.byteLength} bytes`);
  } catch (error) {
    const pluginError = error as PluginError;
    log('warn', `status request failed: ${pluginError.code}`);
  }
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {
    void checkStatus();
  },
  onEvents() {
    return [];
  },
});
```

### Raw path in Rust

Use the raw path when a plugin starts several requests and handles them in one place. It starts a request and returns the id. The completion arrives as an event, and the response is taken with its id.

```rust
let id = http::fetch_request(request)?;   // Err when the call is refused at once
// later, in on_events, for Event::RequestCompleted(result) with result.request == id:
let response = http::take_response(id);   // Option<HttpResponse>; None after 30 s
```

### What the user sees

- **Install.** The review dialog shows one line per host, for example "Connect to api.example.com". Normal-risk grants are checked by default.
- **Refusal.** A call to a host without a grant returns `not-permitted`. The refusal is written to the plugin log as one JSON line, which `ilium plugin logs` shows. The line is shown here formatted for reading:

```json
{
  "type": "refusal",
  "grant": "http:evil.example",
  "function": "http.fetch",
  "reason": "not-granted"
}
```

`reason` is `not-granted` when the manifest asked for other hosts but not this one, and `declined` when the user refused the grant for it. A manifest with no `http` grant at all traps with `host.capability-not-requested` instead.

- **Log lines.** Results appear in the plugin log like this:

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs example.status-check --tail 3
<span class="t-dim">2026-10-11 09:20:01</span> <span class="t-ok">info</span>  status: 214 bytes
<span class="t-dim">2026-10-11 09:21:01</span> <span class="t-warn">warn</span>  status request timed out
<span class="t-dim">2026-10-11 09:22:01</span> <span class="t-err">error</span> status request failed: connection refused
<span class="t-prompt">$</span> <span class="t-dim">3 lines shown, 0 dropped</span>
</pre>

Illustrative, not generated. The three lines come from the Rust example above.

### Path through the admission gate

<figure>
<svg viewBox="0 0 420 210" role="img" aria-label="Request path through the grant check and the admission gate">
<title>Request path through the grant check and the admission gate</title>
<desc>A fetch call first checks the host grant. A host without a grant is refused as not-permitted. A granted host goes to the admission gate. A refused provider request completes with an error. Otherwise the request is sent and its completion is delivered.</desc>
<defs>
<marker id="http-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="10" y="8" width="200" height="32" rx="6" fill="none" stroke="currentColor"/>
<text x="110" y="28" font-size="12" text-anchor="middle" fill="currentColor">fetch(request)</text>
<line x1="110" y1="40" x2="110" y2="62" stroke="currentColor" marker-end="url(#http-arrow)"/>
<rect x="10" y="62" width="200" height="32" rx="6" fill="none" stroke="currentColor"/>
<text x="110" y="82" font-size="12" text-anchor="middle" fill="currentColor">Host grant check</text>
<line x1="210" y1="78" x2="246" y2="78" stroke="currentColor" marker-end="url(#http-arrow)"/>
<rect x="248" y="62" width="160" height="32" rx="6" fill="none" stroke="currentColor"/>
<text x="328" y="82" font-size="12" text-anchor="middle" fill="currentColor">not-permitted</text>
<line x1="110" y1="94" x2="110" y2="116" stroke="currentColor" marker-end="url(#http-arrow)"/>
<rect x="10" y="116" width="200" height="32" rx="6" fill="none" stroke="currentColor"/>
<text x="110" y="136" font-size="12" text-anchor="middle" fill="currentColor">Admission gate (AI providers)</text>
<line x1="210" y1="132" x2="246" y2="132" stroke="currentColor" marker-end="url(#http-arrow)"/>
<rect x="248" y="116" width="160" height="32" rx="6" fill="none" stroke="currentColor"/>
<text x="328" y="136" font-size="12" text-anchor="middle" fill="currentColor">refusal in completion</text>
<line x1="110" y1="148" x2="110" y2="170" stroke="currentColor" marker-end="url(#http-arrow)"/>
<rect x="10" y="170" width="200" height="32" rx="6" fill="none" stroke="currentColor"/>
<text x="110" y="190" font-size="12" text-anchor="middle" fill="currentColor">Send request</text>
<line x1="210" y1="186" x2="246" y2="186" stroke="currentColor" marker-end="url(#http-arrow)"/>
<text x="250" y="190" font-size="10" fill="currentColor">request-completed event</text>
</svg>
<figcaption>The grant check runs first. Only requests to a configured inference provider host pass the admission gate. Its refusal completes the request with an error.</figcaption>
</figure>

The gate is the AI admission policy the user sets for each provider. It can refuse a request by policy, which completes it as `not-permitted`, or throttle it, which completes it as `rate-limited` with `retry-after-ms`. A request to any other host skips the gate.

### Errors

| Code | Returned by | When | Retry |
|---|---|---|---|
| `invalid-argument` | `fetch`, at once | URL not HTTPS outside loopback, unparsable, over 8 KiB; body over 8 MiB; over 64 headers; timeout 0 or over 300,000 ms | No, fix the call |
| `not-permitted` | `fetch`, at once | Host not granted | No |
| `busy` | `fetch`, at once | The plugin's pending requests are full | Yes, later |
| `quota-exceeded` | `fetch`, at once | CPU budget spent; `retry-after-ms` is set | Yes, after the delay |
| `not-permitted` | Completion | Admission policy refused a provider request, or a redirect led to a host without a grant | No |
| `rate-limited` | Completion | The admission gate or the per-host limiter throttled the request, or the server sent `Retry-After`; `retry-after-ms` is set | Yes, after the delay |
| `network` | Completion | DNS, connect or TLS failure; more than 5 redirects | Yes |
| `timed-out` | Completion | No complete response, body included, before `timeout_millis` | Yes |
| `quota-exceeded` | Completion | Response body over `http_response_mib` | No, unless the limit is raised |
| `revoked` | Completion | The grant was withdrawn while the request was pending | No; the plugin is re-activated |

Edge cases:

- **HTTP error status.** A 404, 429 or 500 response is a normal response with `ok` status. Only transport failures are errors. Check `status` in the plugin.
- **Late take.** A response is held for 30 s or until it is taken, once. The wrapper returns `not-found` for a late or repeated take.
- **Restart.** A plugin restart cancels its pending requests. Their completions are not delivered to the new instance.
- **Redirects.** Each hop needs its own grant. A redirect to an ungranted host completes as `not-permitted`.
- **Admission before send.** A gate refusal never sends bytes to the provider.

### Used by

- Live-data scenes (feature 99) fetch one host per data source.
- Maps and Wikipedia (feature 100) fetch tiles and articles from OpenStreetMap and Wikipedia.
- Reset planning (feature 88) fetches a feed once an hour.
- AI providers (feature 84) and the voice session (feature 50) call their providers over HTTP.

### WIT

The contract below is an excerpt. The owner is the [WIT reference](/pages/reference-wit/), and that page wins if the two differ.

```wit
// Excerpt of ilium:host/http in reference-wit
record http-request { method: string, url: string, headers: list<header>, body: option<list<u8>>, timeout-millis: u32 }
record http-response { status: u16, headers: list<header>, body: list<u8> }
enum stream-framing { raw, lines, server-sent-events, ndjson }
/// max-chunk-bytes at most 64 KiB.
record stream-request { request: http-request, framing: stream-framing, max-chunk-bytes: u32 }
/// Async, take-response. Body at most 8 MiB by default. Errors: network, timed-out.
fetch: func(request: http-request) -> result<request-id, plugin-error>;
take-response: func(request: request-id) -> option<http-response>;
/// Delivers http-stream-data and http-stream-closed events. At most 4 streams per plugin by default, 64 per session.
open-stream: func(request: stream-request) -> result<stream-id, plugin-error>;
close-stream: func(stream: stream-id);
// @unstable(feature = websocket)
open-websocket: func(url: string, protocols: list<string>) -> result<stream-id, plugin-error>;
// @unstable(feature = websocket)
send-websocket: func(stream: stream-id, text: string) -> result<_, plugin-error>;
```

### See also

- [http streams](/pages/api-http-streams/) for `open-stream`, framing and hosts taken from a setting.
- [inference](/pages/api-inference/) for model calls, which use the admission gate without a host grant of their own.
- [codec](/pages/api-codec/) for decoding fetched images.
- [Events](/pages/api-events/) for `request-completed` and the other event variants.
- [jobs](/pages/api-jobs/) for fetching inside a background job with `wait-request`.
- [log](/pages/api-log/) for the log levels used in the examples.

### Security

Address checks, redirects, TLS and header rules are defined in [Network egress](/pages/reference-network-egress/) ([SEC-072] to [SEC-084]).
