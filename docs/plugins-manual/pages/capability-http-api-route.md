---
layout: page
title: "Capability: http-api.route"
permalink: /pages/capability-http-api-route/
---

## http-api.route:&lt;path&gt;

The `http-api.route:<path>` capability lets a server-side plugin answer requests on one path of the Ilium loopback HTTP API. The kernel owns the listener. It listens on the local machine only, checks each request, and sends every request for a granted route to the plugin as an `http-called` event. The plugin answers with one `respond` call. A request that matches no granted route never reaches the plugin.

Use a route when a program that the user runs on this computer needs an endpoint that the plugin serves. A route cannot be reached from the network, and a web page in a browser cannot call it.

**You will need:**

- One `http-api.route:<path>` grant per route, in `capabilities.server`. The grant is valid on the server side only.
- One `http_routes` entry per route under `contributes`, with the same path and method as the grant.
- The SDK for your language: `ilium_plugin_sdk` for Rust, or `@ilium/plugin-sdk` for TypeScript. Both expose the same surface. See the [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/) pages.
- The local bearer token, which the caller must send with every request. See [Authentication](#authentication).

### Grant syntax

The argument is the route path. It starts with `/`, may contain `{name}` segments, and is required.

| Grant | Valid | Reason |
|---|---|---|
| `http-api.route:/notes/search` | yes | one literal path |
| `http-api.route:/notes/{id}` | yes | `{id}` matches one non-empty path segment |
| `http-api.route` | no | the argument is required |
| `http-api.route:*` | no | a wildcard is not accepted |
| `http-api.route:notes/search` | no | the path must start with `/` |
| `http-api.route:https://example.com/notes` | no | a URL is not a route path; the listener is loopback only |

### Manifest entry

Each route needs one `http_routes` entry and one grant. The manifest must contain both. Install refuses a manifest where they do not match (`manifest.grant-argument`).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.notes",
  "capabilities": {
    "server": ["http-api.route:/notes/search", "http-api.route:/notes/{id}"]
  },
  "contributes": {
    "http_routes": [
      { "method": "GET", "path": "/notes/search" },
      { "method": "GET", "path": "/notes/{id}" }
    ]
  }
}
```

The method is an upper-case HTTP method name. Each route is served at `/plugins/<plugin-id>/<path>`, so `/notes/{id}` of `com.example.notes` is served at `/plugins/com.example.notes/notes/{id}`. Two plugins cannot collide, because each plugin owns its own prefix.

### Surface

The capability adds one import and one event. Each `respond` call answers exactly one `http-called` event.

```wit
respond: func(call: u64, status: u16, headers: list<tuple<string, string>>,
              body: list<u8>) -> result<_, plugin-error>;

record http-called-event { call: u64, method: string, path: string,
                           headers: list<tuple<string, string>>, body: list<u8> }
```

| Name | Rust | TypeScript | Meaning |
|---|---|---|---|
| `respond` (import) | `http_api::respond(call, status, &headers, &body)` returns `Result<(), PluginError>` | `respond(call, status, headers, body)` returns `void` and throws `PluginError` | Sends the response for one open call. Import path: `ilium_plugin_sdk::host::http_api` and `@ilium/plugin-sdk/host/http-api`. |
| `http-called` (event) | `Event::HttpCalled(HttpCalledEvent)` | `{ tag: 'http-called', val: HttpCalledEvent }` | A request matched a granted route. Delivered in `on_events`. Never dropped. |
| `call` | `u64` | `bigint` | Identifies one open request. Pass it back unchanged to `respond`. |
| `method` | `String` | `string` | The request method in upper case. |
| `path` | `String` | `string` | The concrete path with `{name}` values in place, and the query string kept. |
| `headers` | `Vec<(String, String)>` | `[string, string][]` | The request headers. |
| `body` | `Vec<u8>` | `Uint8Array` | The request body, at most 1 MiB. |
| `status` (argument) | `u16` | `number` | 100 to 599. |
| `headers` (argument) | `&[(String, String)]` | `[string, string][]` | Response headers. No hop-by-hop headers. |
| `body` (argument) | `&[u8]` | `Uint8Array` | Response body, at most 8 MiB. |

The listener does not parse `{name}` values or the query string. The plugin reads them from `path`.

### Matching rules

- The listener matches the method and the path without the query string. `GET /notes/42?full=1` matches `GET /notes/{id}`.
- `{name}` matches exactly one non-empty segment. `/notes/{id}` matches `/notes/42` but not `/notes/` or `/notes/42/tags`.
- A method must be declared. A `POST` to a path that only declares `GET` gets `405 Method Not Allowed` with an `Allow` header.
- The plugin receives `path` with the query string, so it must strip the query string before comparing routes.

### Limits and timing

| Limit | Value | Result when exceeded |
|---|---|---|
| Request body | 1 MiB | The listener answers `413 Content Too Large`. The plugin never sees the request. |
| Response body (`respond`) | 8 MiB | `invalid-argument` |
| Response status (`respond`) | 100 to 599 | `invalid-argument` |
| Response headers (`respond`) | No hop-by-hop header | `invalid-argument` |
| Time to answer | 30 s | The listener answers `504 Gateway Timeout` and closes the call. |

The hop-by-hop headers are `Connection`, `Keep-Alive`, `Proxy-Authenticate`, `Proxy-Authorization`, `TE`, `Trailer`, `Transfer-Encoding` and `Upgrade`. The check ignores case.

### Authentication

The route is protected by a bearer token. The listener checks, in this order:

1. The `Host` header must be `127.0.0.1:<port>` or `localhost:<port>`. Anything else gets `403 Forbidden`.
2. A request with an `Origin` header gets `403 Forbidden`. This blocks web pages in the user's browser.
3. The request must carry `Authorization: Bearer <token>`. A missing or wrong token gets `401 Unauthorized`.
4. The route and method must match a granted route. Otherwise the listener answers `404` or `405`.
5. The body must be at most 1 MiB. Otherwise `413`.

Only then does the plugin receive `http-called`. The server writes a fresh token at each start to `http-api.token`, and the port to `http-api.port`. Both files are owner-only and sit beside the session socket in `$XDG_RUNTIME_DIR/ilium/`. When no runtime directory exists, the OS temporary directory is used. A program run by the same user can read these files. A program run by another user cannot.

### Risk, review and tiers

- The grant has normal risk. It is not marked high risk (H), so a T2 or T3 package lists it checked by default in the install review. A T1 bundled plugin gets it up to its manifest ceiling.
- The grant is server side only. The review dialog shows one row per route, with the grant string on the right.
- A declined route is not served. A revoked grant withdraws the route, so later requests get `404 Not Found`. This is the same answer a path that does not exist gets, so a caller cannot tell a declined route from a missing one.

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.http 0.1.0 by Example Ltd                                      │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Serves local API routes <span class="t-dim">…………………………………………………………</span> http-api.route:/notes/* │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests http-api.route:/notes/*, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

### Routing flow

<figure>
<svg viewBox="0 0 360 280" role="img" aria-label="Route flow: client request, listener checks, http-called event, respond">
  <defs>
    <marker id="hr-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="8" width="340" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="180" y="32" text-anchor="middle" font-size="11" fill="currentColor">1. Client sends GET /notes/42 to the loopback API</text>
  <line x1="180" y1="48" x2="180" y2="68" stroke="currentColor" marker-end="url(#hr-arrow)"/>
  <rect x="10" y="68" width="340" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="180" y="92" text-anchor="middle" font-size="11" fill="currentColor">2. Listener checks Host, Origin, token and route</text>
  <line x1="180" y1="108" x2="180" y2="128" stroke="currentColor" marker-end="url(#hr-arrow)"/>
  <rect x="10" y="128" width="340" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="180" y="152" text-anchor="middle" font-size="11" fill="currentColor">3. Plugin receives http-called with the request</text>
  <line x1="180" y1="168" x2="180" y2="188" stroke="currentColor" marker-end="url(#hr-arrow)"/>
  <rect x="10" y="188" width="340" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="180" y="212" text-anchor="middle" font-size="11" fill="currentColor">4. Plugin calls respond(call, status, headers, body)</text>
  <text x="180" y="260" text-anchor="middle" font-size="10" fill="currentColor">Dashed box: kernel policy, not plugin code.</text>
</svg>
<figcaption>The listener rejects a request before the plugin sees it. The plugin answers each call once.</figcaption>
</figure>

### Responses from the listener

The listener answers these cases itself. The plugin does not run for them.

| Status | When |
|---|---|
| `401 Unauthorized` | The bearer token is missing or wrong. |
| `403 Forbidden` | The `Host` header is not loopback for this port, or the request has an `Origin` header. |
| `404 Not Found` | No granted route covers the path, or the route was declined or revoked. |
| `405 Method Not Allowed` | The path is known, but the method is not declared. The `Allow` header lists the declared methods. |
| `413 Content Too Large` | The request body is over 1 MiB. |
| `503 Service Unavailable` | The plugin is stopped, failed or disabled, or the call was open when the plugin stopped or the grant was revoked. |
| `504 Gateway Timeout` | The plugin did not call `respond` within 30 s. |

### Errors from respond

| Code | When |
|---|---|
| `not-found` | The call was already answered, timed out or lost its caller. A second `respond` for the same call gets this code. |
| `invalid-argument` | The status is outside 100 to 599, the body is over 8 MiB, or a header is hop-by-hop. |

A `not-found` error means no response is needed, because the caller has already gone or been answered. Do not retry it.

### Worked example: GET /notes/{id} in Rust

The route returns one note as JSON. An unknown id gets `404` with a JSON error body. `find_note_json` is the plugin's own code and is not part of the SDK. It returns `Option<String>`.

**Rust**

```rust
use ilium_plugin_sdk::host::http_api;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::HttpCalledEvent;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct NoteLookup;

impl Handler for NoteLookup {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if let Event::HttpCalled(request) = event {
                answer_note_lookup(&request);
            }
        }
        Vec::new()
    }
}

fn answer_note_lookup(request: &HttpCalledEvent) {
    // The path keeps its query string. Only the part before "?" is the route.
    let route_path = request.path.split('?').next().unwrap_or("");
    let Some(note_id) = route_path.strip_prefix("/notes/") else {
        // The listener only sends granted paths, so this is a defensive answer.
        let _ = http_api::respond(request.call, 404, &[], &[]);
        return;
    };
    let Some(note_json) = find_note_json(note_id) else {
        let body = br#"{"error":"note not found"}"#;
        let _ = http_api::respond(request.call, 404, &json_headers(), body);
        return;
    };
    // An Err here means the call already closed. Nothing is left to send.
    let _ = http_api::respond(request.call, 200, &json_headers(), note_json.as_bytes());
}

fn json_headers() -> Vec<(String, String)> {
    vec![("content-type".to_string(), "application/json".to_string())]
}
```

### Worked example: GET /notes/{id} in TypeScript

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { respond } from '@ilium/plugin-sdk/host/http-api';
import type { Event, HttpCalledEvent } from '@ilium/plugin-sdk/types';
import { findNoteJson } from './notes';

const JSON_HEADERS: [string, string][] = [['content-type', 'application/json']];

export const { handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]) {
    for (const event of batch) {
      if (event.tag === 'http-called') {
        answerNoteLookup(event.val);
      }
    }
    return [];
  },
});

function answerNoteLookup(request: HttpCalledEvent): void {
  const encoder = new TextEncoder();
  const routePath = request.path.split('?')[0];
  const noteId = routePath.startsWith('/notes/') ? routePath.slice('/notes/'.length) : '';
  const noteJson = noteId === '' ? undefined : findNoteJson(noteId);
  if (noteJson === undefined) {
    reply(request.call, 404, JSON_HEADERS, encoder.encode('{"error":"note not found"}'));
    return;
  }
  reply(request.call, 200, JSON_HEADERS, encoder.encode(noteJson));
}

// respond throws PluginError when the call already closed. Nothing is left to send.
function reply(
  call: bigint,
  status: number,
  headers: [string, string][],
  body: Uint8Array,
): void {
  try {
    respond(call, status, headers, body);
  } catch {
    // The caller left, or the 30 s reply limit passed.
  }
}
```

### What the caller sees

Run the plugin, then call the route from a terminal. The token and port come from the two owner-only files.

<figure>
<pre class="term"><span class="t-prompt">$</span> TOKEN=$(cat "$XDG_RUNTIME_DIR/ilium/http-api.token")
<span class="t-prompt">$</span> PORT=$(cat "$XDG_RUNTIME_DIR/ilium/http-api.port")
<span class="t-prompt">$</span> BASE="http://127.0.0.1:$PORT/plugins/com.example.notes"
<span class="t-prompt">$</span> curl -s -w "\n%{http_code}\n" \
    -H "Authorization: Bearer $TOKEN" "$BASE/notes/42"
{"id":"42","title":"Weekly plan"}
<span class="t-ok">200</span>
<span class="t-prompt">$</span> curl -s -o /dev/null -w "%{http_code}\n" "$BASE/notes/42"
<span class="t-err">401</span>
<span class="t-prompt">$</span> curl -s -o /dev/null -w "%{http_code}\n" \
    -H "Authorization: Bearer $TOKEN" "$BASE/notes/7/tags"
<span class="t-warn">404</span>
</pre>
<figcaption>Illustrative, not generated. A request with the token, one without it, and a path that no route covers.</figcaption>
</figure>

### Edge cases

- **Query strings.** `path` keeps the query string. Route matching ignores it, so `/notes/42?full=1` reaches the same route.
- **Several parameters.** A route may declare more than one segment, such as `/notes/{id}/tags/{tag}`. Each segment matches one non-empty value.
- **Empty bodies.** A reply with no body is valid: `respond(call, 204, [], [])` in Rust takes `&[]` for the body.
- **Stopped plugin.** If the plugin stops while a call is open, the caller gets `503`. Any later `respond` for that call returns `not-found`.
- **Slow handlers.** The 30 s limit covers the whole time from the event to `respond`. Start slow work in a task or a job, and answer when it completes.
- **Kernel routes.** The kernel's own API routes, such as the agent creation route, are served under `/api/` with the same token. They do not use this grant.

### See also

- [http-api](/pages/api-http-api/) for the `http-api` import and its event, with the full SDK examples.
- [capabilities](/pages/capabilities/) for how grants are requested, reviewed and revoked.
- [reference-manifest](/pages/reference-manifest/) for the `http_routes` field.
- [cli.contribute](/pages/capability-cli-contribute/) for a subcommand that gives the same kind of output on the command line.
- [http](/pages/capability-http/) for requests that go out from the plugin instead of in.

Checked against `ilium:plugin@0.1.0` (API draft of 2026-10-11).

### Security

Listener rules and token handling are in [Network egress](/pages/reference-network-egress/) ([SEC-097] to [SEC-099]).
