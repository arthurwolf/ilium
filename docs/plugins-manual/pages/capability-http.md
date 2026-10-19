---
layout: page
title: "Capability: http"
permalink: /pages/capability-http/
---

## http:&lt;host-pattern&gt;

The `http:<host-pattern>` grant lets a plugin send HTTP requests to the hosts that match the pattern. Each host needs its own grant. The host checks the grant before a request leaves the machine, so a request to any other host returns `not-permitted` at once and nothing is sent.

Use this grant for remote APIs, data feeds, map tiles and article sources. For large transfers, run the download in a job with [`jobs.run`](/pages/capability-jobs-run/). A job can call `http.fetch` with the same grants as its side, and the job's memory and wall-clock limits apply.

### What you need

| Item | Requirement |
|---|---|
| Grant | One `http:<host-pattern>` per host, on the side that sends the request: `client`, `server` or `scene` in the manifest. A job needs the grant on the side that runs it. |
| Rust | `ilium_plugin_sdk::host::http` |
| TypeScript | `@ilium/plugin-sdk/host/http` |
| Protocol | HTTPS only. Plain HTTP works only for loopback hosts (`localhost`, `127.0.0.1`, `[::1]`) when the grant names them. |
| Tier | T1 bundled plugins receive the grant up to the manifest ceiling. T2 and T3 packages must be approved in the review dialog. |

Declare grants in the manifest under the side that makes the request:

```json
"capabilities": { "server": ["http:api.example.com", "http:*.openstreetmap.org"] }
```

### Grant syntax

| Grant | Valid | Meaning |
|---|---|---|
| `http:api.example.com` | yes | one exact host |
| `http:api.example.com:8443` | yes | one exact host and port |
| `http:*.openstreetmap.org` | yes | any subdomain at any depth; not the bare domain |
| `http:localhost` | yes | loopback; plain HTTP allowed |
| `http:127.0.0.1` | yes | loopback IPv4 literal; plain HTTP allowed |
| `http:[::1]` | yes | loopback IPv6 literal; plain HTTP allowed |
| `http:*` | no | a bare `*` is refused |
| `http:tile.*.org` | no | a wildcard in any position other than the leftmost label |
| `http` | no | the host pattern is missing |
| `http:https://api.example.com/v1` | no | a scheme or path; keep only the host |

A grant without a port matches the default port of the scheme only (443 for HTTPS, 80 for plain HTTP on loopback). A grant with `:<port>` matches that port alone.

`http:*.openstreetmap.org` covers `a.openstreetmap.org` and `a.b.openstreetmap.org`, but not `openstreetmap.org`. Declare the bare domain as its own grant when you need it.

### Surface

The `http` module has three calls: one that waits for the response, one that starts a request and returns its id, and one that reads a stored response.

| Function | Rust | TypeScript | Returns | Meaning |
|---|---|---|---|---|
| Send and wait | `http::fetch(request).await` | `await http.fetch(request)` | `Result<HttpResponse, PluginError>` / `Promise<HttpResponse>` | Sends the request and resolves with the full response. |
| Start | `http::fetch_request(request)` | `http.fetchRequest(request)` | `Result<RequestId, PluginError>` / `bigint` | Starts the request and returns its id. Use it when you need the id first. |
| Read result | `http::take_response(id)` | `http.takeResponse(id)` | `Option<HttpResponse>` / `HttpResponse \| undefined` | Returns the response once it is available. Nothing is returned before the request completes. |

Failed requests do not appear as a response. A transport failure, timeout or limit breach completes the request with an error code. The async wrapper turns that completion into a `PluginError`. The raw form delivers it in a `request-completed` event.

The types are the same in both languages:

| Type | Field | Type | Meaning |
|---|---|---|---|
| `HttpRequest` | `method` | string | `GET`, `HEAD`, `POST`, `PUT`, `PATCH`, `DELETE` or `OPTIONS`, in upper case. Any other value is `invalid-argument`. |
| `HttpRequest` | `url` | string | Absolute `https://` URL, at most 8 KiB. |
| `HttpRequest` | `headers` | `Header` list | At most 64 entries. |
| `HttpRequest` | `body` | bytes, optional | At most 8 MiB. `None` or `undefined` sends no body. |
| `HttpRequest` | `timeout_millis` | `u32` | 1 to 300,000. Covers connect, redirects and reading the body. |
| `Header` | `name`, `value` | string | One header. |
| `HttpResponse` | `status` | `u16` | The HTTP status code. A 404 or a 503 is a normal response. |
| `HttpResponse` | `headers` | `Header` list | The response headers. |
| `HttpResponse` | `body` | bytes | The response body. Default cap 8 MiB for every tier; a T1 plugin may request up to 64 MiB in `limits`. |

Each plugin may hold 64 pending requests by default (`pending_requests`); a T1 plugin may request up to 256. A call over that limit returns `busy`.

### Worked example: Rust, server side

This plugin fetches a notes feed when it activates. The request runs on the server side, so the manifest grants `http:api.example.com` under `server`.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.notes",
  "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": [] },
  "capabilities": { "server": ["http:api.example.com"] }
}
```

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::http::{self, Header, HttpRequest};
use ilium_plugin_sdk::host::log::{self, Level};

const NOTES_URL: &str = "https://api.example.com/v1/notes?since=2026-10-01";

async fn sync_notes() -> Result<(), PluginError> {
    let request = HttpRequest {
        method: "GET".into(),
        url: NOTES_URL.into(),
        headers: vec![Header { name: "accept".into(), value: "application/json".into() }],
        body: None,
        timeout_millis: 5_000,
    };
    let response = http::fetch(request).await?;
    let text = String::from_utf8_lossy(&response.body);
    log::log(
        Level::Info,
        &format!("notes sync: status {}, {} bytes", response.status, text.len()),
    );
    Ok(())
}

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct NotesSync;

impl Lifecycle for NotesSync {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        task::spawn(async {
            if let Err(error) = sync_notes().await {
                log::log(Level::Warn, &format!("notes sync failed: {:?}", error.code));
            }
        });
        Ok(())
    }
}

impl Handler for NotesSync {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}
```

The `?` operator works because `http::fetch` returns `Result<_, PluginError>`. The `task::spawn` block keeps the request running after `activate` returns, and its error is logged rather than dropped.

### Worked example: TypeScript, client side

The same request from a client plugin. The manifest grants the host under `client`.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.notes",
  "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": [] },
  "capabilities": { "client": ["http:api.example.com"] }
}
```

**TypeScript**

```ts
import { definePlugin, type PluginError } from '@ilium/plugin-sdk';
import * as http from '@ilium/plugin-sdk/host/http';
import { log } from '@ilium/plugin-sdk/host/log';

const NOTES_URL = 'https://api.example.com/v1/notes?since=2026-10-01';

async function syncNotes(): Promise<void> {
  const response = await http.fetch({
    method: 'GET',
    url: NOTES_URL,
    headers: [{ name: 'accept', value: 'application/json' }],
    body: undefined,
    timeoutMillis: 5000,
  });
  const text = new TextDecoder().decode(response.body);
  log('info', `notes sync: status ${response.status}, ${text.length} characters`);
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {
    syncNotes().catch((error: PluginError) => {
      log('warn', `notes sync failed: ${error.code}`);
    });
  },
  onEvents() {
    return [];
  },
});
```

`import * as http` keeps the SDK call apart from the global `fetch`. The promise is handled in `activate`, so a failure is logged and never becomes an unhandled rejection.

<figure>
<svg viewBox="0 0 320 300" role="img" aria-label="HTTP request flow through host grant, admission gate, request and response">
  <defs>
    <marker id="htt-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="8" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="32" text-anchor="middle" font-size="11" fill="currentColor">Plugin calls fetch(http-request)</text>
  <line x1="160" y1="48" x2="160" y2="66" stroke="currentColor" marker-end="url(#htt-arrow)"/>
  <rect x="10" y="66" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="90" text-anchor="middle" font-size="11" fill="currentColor">Host matches an http grant</text>
  <line x1="160" y1="106" x2="160" y2="124" stroke="currentColor" marker-end="url(#htt-arrow)"/>
  <rect x="10" y="124" width="300" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="160" y="148" text-anchor="middle" font-size="11" fill="currentColor">Admission gate, for inference provider hosts</text>
  <line x1="160" y1="164" x2="160" y2="182" stroke="currentColor" marker-end="url(#htt-arrow)"/>
  <rect x="10" y="182" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="206" text-anchor="middle" font-size="11" fill="currentColor">Request runs; redirects checked again</text>
  <line x1="160" y1="222" x2="160" y2="240" stroke="currentColor" marker-end="url(#htt-arrow)"/>
  <rect x="10" y="240" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="264" text-anchor="middle" font-size="11" fill="currentColor">Response returned to the plugin</text>
</svg>
<figcaption>Each request passes the host grant check first. The dashed admission gate is kernel policy; the plugin cannot skip it.</figcaption>
</figure>

### Rules for every request

- Only HTTPS is accepted, except loopback when the grant names `localhost`, `127.0.0.1` or `[::1]`.
- A request follows at most 5 redirects, and each hop must match a grant. A redirect to an ungranted host completes as `failed(not-permitted)`. A sixth redirect completes as `failed(network)`.
- A request to the host of a configured inference provider passes the AI admission gate first. The gate can refuse it by the user's policy (`not-permitted`) or throttle it (`rate-limited`, with `retry-after-ms`). Each admitted request adds one line to the privacy log. A refused request writes none.
- A non-2xx status is a normal response. Check `status` yourself; only transport failures are errors.
- The response body is capped at 8 MiB by default for every tier; a T1 plugin may request up to 64 MiB in `limits`. A larger body completes as `failed(quota-exceeded)`.
- `timeout_millis` covers the whole exchange. Passing it completes as `failed(timed-out)`.
- The kernel never forwards a request from one side to another. The side that calls `fetch` needs the grant.

### Review dialog and tiers

The install review dialog shows one row per capability. An `http` row reads `Connect to <host>` and shows the grant string at the right edge. Rows that are not `HIGH` start checked. `http` is not a `HIGH` capability, so it does not need a separate opt-in.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.http 0.1.0 by Example Ltd                                      │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ [x] Connects to api.example.com <span class="t-dim">………………………………………………………</span> http:api.example.com │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests http:api.example.com, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

A declined grant shows as unchecked and dimmed. The plugin's `granted` list in the activation context leaves it out, so a plugin can check its own status at start and tell the user what is missing.

### When a grant is missing or declined

A request to a host without a grant returns `not-permitted` at once. The host also writes one refusal line to the plugin log. The `reason` field is `declined` when the user unticked the row, and `not-granted` when the manifest asked for other hosts but not this one:

```json
{"type":"refusal","grant":"http:api.example.com","function":"http.fetch","reason":"declined"}
```

A request that matches a granted host still works. A granted host that is down or unreachable returns `network`, not `not-permitted`.

### Errors

| Code | When it happens | Where it appears | Retry |
|---|---|---|---|
| `invalid-argument` | URL not HTTPS outside loopback, unparsable or over 8 KiB; body over 8 MiB; timeout outside 1 to 300,000 ms; over 64 headers; method not in the list | at the call | no, fix the call |
| `not-permitted` | no grant for the host; the admission policy refused an AI provider call; a redirect to an ungranted host | at the call, or in the completion for redirects and policy | no |
| `busy` | 64 pending requests (256 at T1) | at the call | yes, later |
| `quota-exceeded` | response body over the cap; CPU budget of the quota group spent | in the completion, or at the call with `retry-after-ms` | after freeing resources or after `retry-after-ms` |
| `rate-limited` | the admission gate throttled a provider host | in the completion, with `retry-after-ms` | yes, after `retry-after-ms` |
| `network` | DNS, connect or TLS failure; more than 5 redirects | in the completion | yes |
| `timed-out` | the exchange did not finish within `timeout_millis` | in the completion | yes |
| `revoked`, `internal` | the grant changed during the call; a host bug | at the call or in the completion | `revoked`: the plugin is re-activated |

### Edge cases

- A `404` or a `503` from the server is an `ok` response with that status. Only transport failures are errors.
- A wildcard grant does not cover the bare domain, and a grant without a port does not cover other ports.
- A redirect chain is checked hop by hop. A granted first host that redirects to an ungranted second host fails with `failed(not-permitted)`.
- `timeout_millis` includes redirects and the body download. A slow body fails with `failed(timed-out)` even when the headers arrived in time.
- A request from a job uses the job's side. Declare the grant on the side that runs the job.
- A scene declares its host grants under `capabilities.scene`.

### Who requests it

First-party plugins that use this grant:

- 50 Voice session: `http:<provider-host>`.
- 84 AI providers: one `http:<provider-host>` grant for each provider.
- 88 Reset planning: `http:<feed-host>`.
- 99 Live-data scenes: one `http:<data-host>` grant for each source.
- 100 Maps, space, Wikipedia and voxel saved worlds: `http:*.openstreetmap.org` and `http:*.wikipedia.org`.

### See also

- [inference](/pages/capability-inference/) for requests that go through the AI admission gate.
- [jobs.run](/pages/capability-jobs-run/) for long downloads.
- [api-http](/pages/api-http/) for the full function reference.
- [Errors and limits](/pages/reference-errors-and-limits/) for the error code table and every size limit.
- [capability-http-api-route](/pages/capability-http-api-route/) for the opposite direction: local requests that reach your plugin.

Checked against `ilium:plugin@0.1.0` (API draft of 2026-10-11).

### Security

Address checks, redirects, TLS and the offline switch are defined in [Network egress](/pages/reference-network-egress/). Extra `not-permitted` reasons: `private-address`, `downgrade`, `reserved`, `offline`, `policy`.
