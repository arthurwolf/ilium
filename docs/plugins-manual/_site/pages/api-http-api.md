# http-api

## http-api

The `http-api` import lets a server-side plugin answer requests on the Ilium loopback HTTP listener. The kernel owns the listener, the socket, the token check and the request parsing. The plugin declares each route in its manifest, receives every matching request as an `http-called` event, and answers each request with one call to `respond`. The listener binds to the local machine only, so the routes reach the local user and nothing else.

**You will need:**

- A server-side grant `http-api.route:<path>` for each route, listed in `capabilities.server`. A route without its grant is not registered: requests to it get `404`.
- A matching `http_routes` entry under `contributes`, with the same method and path.
- A server-side plugin. Rust code imports the SDK as described in the [Rust SDK](/pages/rust-sdk/) page; TypeScript code imports `@ilium/plugin-sdk` as described in the [TypeScript SDK](/pages/typescript-sdk/) page.
- The local token. Every request must send it; see [Authentication](#authentication).

### Manifest

Each route is one `http_routes` entry. The method is an upper-case HTTP method name, such as `GET` or `POST`. The path starts with `/`. A path may contain `{name}` segments, and each one matches exactly one non-empty path segment.

```json
"contributes": {
  "http_routes": [
    { "method": "POST", "path": "/notes/new" },
    { "method": "GET", "path": "/notes/{id}" }
  ]
},
"capabilities": {
  "server": ["http-api.route:/notes/new", "http-api.route:/notes/{id}"]
}
```

The listener matches on the method and the path together. A `POST` route does not answer a `GET` to the same path; that request gets `405`. Plugin routes are mounted under `/plugins/<plugin-id>`, so `/notes/new` in a plugin whose id is `example-notes` is served at `/plugins/example-notes/notes/new`.

### Surface

The import has one function and one event. Each call to `respond` answers exactly one `http-called` event.

```wit
respond: func(
    call: u64, status: u16, headers: list<tuple<string, string>>, body: list<u8>,
) -> result<_, plugin-error>;
```

| Name | Rust | TypeScript | Meaning |
|---|---|---|---|
| `respond` | `http_api::respond(call, status, &headers, &body)` returns `Result<(), PluginError>` | `respond(call, status, headers, body)` returns `void` and throws `PluginError` | Sends the response for one open call. |
| `http-called` | `Event::HttpCalled(HttpCalledEvent)` | `{ tag: 'http-called', val: HttpCalledEvent }` | A request matched a route the plugin holds a grant for. Delivered in `on_events`. Never dropped. |
| `call` | `u64` | `bigint` | Identifies one open request. Pass it back unchanged to `respond`. |
| `method` | `String` | `string` | The HTTP method of the request, in upper case. |
| `path` | `String` | `string` | The concrete path, with `{name}` values filled in and any query string kept. |
| `headers` | `Vec<(String, String)>` | `[string, string][]` | The request headers. |
| `body` | `Vec<u8>` | `Uint8Array` | The request body, at most 1 MiB. |

A plugin parses `{name}` values and query parameters from `path` itself. The listener does not extract them.

### Limits

| Limit | Value | Result when exceeded |
|---|---|---|
| Request body | 1 MiB | The listener answers `413` and the plugin never sees the request. |
| Response body | 8 MiB | `respond` returns `invalid-argument`. |
| Response status | 100 to 599 | `respond` returns `invalid-argument`. |
| Response headers | No hop-by-hop headers | `respond` returns `invalid-argument`. |
| Time to answer | 30 s | The listener answers `504` and the call closes. |

The hop-by-hop headers are `Connection`, `Keep-Alive`, `Proxy-Authenticate`, `Proxy-Authorization`, `TE`, `Trailer`, `Transfer-Encoding` and `Upgrade`.

### Authentication

Every request must carry the token in an `Authorization: Bearer <token>` header. The server writes a fresh token at each start, plus the port, into two owner-only files in `$XDG_RUNTIME_DIR/ilium/` beside the session socket (`http-api.token` and `http-api.port`). When no runtime directory exists, the OS temporary directory is used instead.

The listener also refuses any `Host` header other than `127.0.0.1:<port>` or `localhost:<port>`, and any request that has an `Origin` header. That second rule blocks web pages in the user's browser from calling the API.

### Example: a POST route that creates a note

The route reads a JSON body with a `title` field, creates a note, and answers `201` with the new id. A body that is not valid JSON gets `400`. `NewNote`, `parse_new_note` and `create_note` are the plugin's own code, not part of the SDK.

**Rust**

```rust
use ilium_plugin_sdk::host::http_api;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::HttpCalledEvent;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct NotesApi;

impl Handler for NotesApi {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if let Event::HttpCalled(request) = event {
                answer_new_note(&request);
            }
        }
        Vec::new()
    }
}

fn answer_new_note(request: &HttpCalledEvent) {
    // The path keeps its query string; compare only the part before "?".
    let route_path = request.path.split('?').next().unwrap_or("");
    if request.method != "POST" || route_path != "/notes/new" {
        let _ = http_api::respond(request.call, 404, &[], &[]);
        return;
    }
    let Ok(note) = serde_json::from_slice::<NewNote>(&request.body) else {
        let _ = http_api::respond(request.call, 400, &[], br#"{"error":"invalid json"}"#);
        return;
    };
    let note_id = create_note(&note.title);
    let reply = format!(r#"{{"id":"{note_id}"}}"#);
    let headers = vec![("content-type".to_string(), "application/json".to_string())];
    // An error here means the caller already left or the 30 s limit passed. Nothing to send.
    let _ = http_api::respond(request.call, 201, &headers, reply.as_bytes());
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { respond } from '@ilium/plugin-sdk/host/http-api';
import { log } from '@ilium/plugin-sdk/host/log';
import type { Event, HttpCalledEvent } from '@ilium/plugin-sdk/types';
import { createNote, parseNewNote } from './notes';

export const { handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]) {
    for (const event of batch) {
      if (event.tag === 'http-called') {
        answerNewNote(event.val);
      }
    }
    return [];
  },
});

function answerNewNote(request: HttpCalledEvent): void {
  const routePath = request.path.split('?')[0];
  if (request.method !== 'POST' || routePath !== '/notes/new') {
    reply(request.call, 404, [], new Uint8Array());
    return;
  }
  const note = parseNewNote(request.body);
  if (note === undefined) {
    reply(request.call, 400, [['content-type', 'application/json']],
          new TextEncoder().encode('{"error":"invalid json"}'));
    return;
  }
  const noteId = createNote(note.title);
  reply(request.call, 201, [['content-type', 'application/json']],
        new TextEncoder().encode(JSON.stringify({ id: noteId })));
}

// respond throws not-found when the caller left or the call timed out.
function reply(call: bigint, status: number, headers: [string, string][], body: Uint8Array): void {
  try {
    respond(call, status, headers, body);
  } catch (error) {
    log('warn', `respond failed for call ${call}: ${String(error)}`);
  }
}
```

### What you see

Run the plugin, then call the route from a terminal. The token and port come from the two owner-only files.

<pre class="term"><span class="t-prompt">$</span> PORT=$(cat \
    "$XDG_RUNTIME_DIR/ilium/http-api.port")
<span class="t-prompt">$</span> TOKEN=$(cat \
    "$XDG_RUNTIME_DIR/ilium/http-api.token")
<span class="t-prompt">$</span> URL="http://127.0.0.1:$PORT\
/plugins/example-notes/notes/new"
<span class="t-prompt">$</span> curl -s -X POST "$URL" \
    -H "Authorization: Bearer $TOKEN" -d '{"title":"Plan"}' -w " %{http_code}\n"
{"id":"n-0142"} <span class="t-ok">201</span>
<span class="t-prompt">$</span> curl -s -X POST "$URL" \
    -H "Authorization: Bearer $TOKEN" -d 'oops' -w " %{http_code}\n"
{"error":"invalid json"} <span class="t-err">400</span>
<span class="t-prompt">$</span> curl -s -o /dev/null -X POST "$URL" \
    -d '{"title":"Plan"}' -w "%{http_code}\n"
<span class="t-err">401</span>
</pre>

Illustrative, not generated. The first two calls reach the plugin. The third has no token, so the listener answers before the plugin runs.

The user sees no window for these routes. At install time the review dialog lists each grant as "Answer local API requests at" the route path, and the user can leave a grant unchecked. A declined route answers `404`, the same as a path that does not exist.

<figure>
<svg viewBox="0 0 360 220" role="img" aria-label="Loopback request path to a plugin route">
  <defs>
    <marker id="http-api-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
      <path d="M0,0 L8,4 L0,8 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="20" y="10" width="320" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="180" y="33" font-size="12" text-anchor="middle" fill="currentColor">Client: POST with Authorization header</text>
  <rect x="20" y="76" width="320" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="180" y="99" font-size="12" text-anchor="middle" fill="currentColor">Kernel listener: token, Host, Origin, route</text>
  <rect x="20" y="142" width="320" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="180" y="165" font-size="12" text-anchor="middle" fill="currentColor">Plugin: on_events receives http-called</text>
  <line x1="180" y1="46" x2="180" y2="74" stroke="currentColor" marker-end="url(#http-api-arrow)"/>
  <text x="190" y="66" font-size="10" fill="currentColor">request</text>
  <line x1="120" y1="74" x2="120" y2="48" stroke="currentColor" marker-end="url(#http-api-arrow)"/>
  <text x="110" y="66" font-size="10" text-anchor="end" fill="currentColor">response</text>
  <line x1="180" y1="112" x2="180" y2="140" stroke="currentColor" marker-end="url(#http-api-arrow)"/>
  <text x="190" y="132" font-size="10" fill="currentColor">http-called</text>
  <line x1="300" y1="140" x2="300" y2="114" stroke="currentColor" marker-end="url(#http-api-arrow)"/>
  <text x="308" y="132" font-size="10" fill="currentColor">respond</text>
  <text x="20" y="206" font-size="11" fill="currentColor">Only granted routes reach a plugin.</text>
</svg>
<figcaption>The plugin never touches the socket. It answers each call by its call id.</figcaption>
</figure>

### Example: a route with a path value and a deferred answer

A `GET /notes/{id}` route reads the id from the path and answers `200` or `404`. A route does not have to answer inside `on_events`: the plugin may keep the `call` id and answer later, for example after an `fs` read completes, as long as it answers within 30 s. `load_note` is the plugin's own async function.

**Rust**

```rust
use ilium_plugin_sdk::host::http_api;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::HttpCalledEvent;

fn note_id(path: &str) -> Option<&str> {
    let route_path = path.split('?').next()?;
    route_path.strip_prefix("/notes/").filter(|id| !id.is_empty() && !id.contains('/'))
}

fn start_answer(request: HttpCalledEvent) {
    let Some(id) = note_id(&request.path).map(str::to_owned) else {
        let _ = http_api::respond(request.call, 404, &[], &[]);
        return;
    };
    task::spawn(async move {
        let headers = vec![("content-type".to_string(), "application/json".to_string())];
        let result = match load_note(&id).await {
            Ok(body) => http_api::respond(request.call, 200, &headers, &body),
            Err(_) => http_api::respond(request.call, 404, &[], &[]),
        };
        // not-found here means the caller left or 30 s passed; nothing to retry.
        let _ = result;
    });
}
```

**TypeScript**

```ts
import { respond } from '@ilium/plugin-sdk/host/http-api';
import type { HttpCalledEvent } from '@ilium/plugin-sdk/types';
import { loadNote } from './notes';

function noteId(path: string): string | undefined {
  const routePath = path.split('?')[0];
  const match = /^\/notes\/([^/]+)$/.exec(routePath);
  return match?.[1];
}

export function startAnswer(request: HttpCalledEvent): void {
  const id = noteId(request.path);
  if (id === undefined) {
    respond(request.call, 404, [], new Uint8Array());
    return;
  }
  loadNote(id)
    .then((body) => respond(request.call, 200, [['content-type', 'application/json']], body))
    .catch(() => {
      try {
        respond(request.call, 404, [], new Uint8Array());
      } catch {
        // the caller left or the 30 s limit passed
      }
    });
}
```

<figure>
<svg viewBox="0 0 440 250" role="img" aria-label="Sequence of a plugin route call with the three outcomes">
<title>Route call sequence</title>
<desc>The client sends a request. The listener checks token, host and origin, finds the route and sends http-called to the plugin. The plugin calls respond within 30 seconds and the listener returns the response. After 30 seconds the listener answers 504 instead.</desc>
<defs>
<marker id="hapi-seq-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker>
</defs>
<text x="60" y="16" font-size="12" text-anchor="middle" fill="currentColor">Caller</text>
<text x="220" y="16" font-size="12" text-anchor="middle" fill="currentColor">Listener</text>
<text x="380" y="16" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
<line x1="60" y1="24" x2="60" y2="242" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="220" y1="24" x2="220" y2="242" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="380" y1="24" x2="380" y2="242" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="60" y1="44" x2="218" y2="44" stroke="currentColor" marker-end="url(#hapi-seq-arrow)"/>
<text x="66" y="38" font-size="10" fill="currentColor">GET /plugins/id/notes/7</text>
<text x="226" y="64" font-size="10" fill="currentColor">token, Host, Origin, route</text>
<line x1="222" y1="84" x2="378" y2="84" stroke="currentColor" marker-end="url(#hapi-seq-arrow)"/>
<text x="228" y="78" font-size="10" fill="currentColor">http-called (call 12)</text>
<line x1="378" y1="112" x2="222" y2="112" stroke="currentColor" marker-end="url(#hapi-seq-arrow)"/>
<text x="374" y="106" font-size="10" text-anchor="end" fill="currentColor">respond(12, 200, ...)</text>
<line x1="218" y1="132" x2="62" y2="132" stroke="currentColor" marker-end="url(#hapi-seq-arrow)"/>
<text x="214" y="126" font-size="10" text-anchor="end" fill="currentColor">200 and body</text>
<text x="10" y="168" font-size="10" fill="currentColor">If the plugin does not call respond within 30 s:</text>
<line x1="218" y1="190" x2="62" y2="190" stroke="currentColor" marker-end="url(#hapi-seq-arrow)"/>
<text x="214" y="184" font-size="10" text-anchor="end" fill="currentColor">504 Gateway Timeout</text>
<line x1="378" y1="218" x2="222" y2="218" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#hapi-seq-arrow)"/>
<text x="374" y="212" font-size="10" text-anchor="end" fill="currentColor">late respond: not-found</text>
</svg>
<figcaption>Each call is answered once. A late answer returns not-found and sends nothing.</figcaption>
</figure>

### A route table in the manifest

Declare every method and path pair. A pair that is not declared is not routed, and a path declared only for one method answers the others with `405` and an `Allow` header.

```json
"contributes": { "http_routes": [
  { "method": "GET",  "path": "/notes/{id}" },
  { "method": "POST", "path": "/notes/new" },
  { "method": "GET",  "path": "/status" }
] },
"capabilities": { "server": [
  "http-api.route:/notes/{id}", "http-api.route:/notes/new", "http-api.route:/status"
] }
```

The grant argument is the route path, as in `http-api.route:/notes/{id}`. A path that does not start with `/`, or a bare `*`, is refused at install.

### Errors

`respond` returns these errors through `PluginError`:

| Code | When |
|---|---|
| `not-found` | The call was already answered, timed out after 30 s, or its caller disconnected. |
| `invalid-argument` | Status outside 100 to 599, body over 8 MiB, or a hop-by-hop header. |

The listener answers these statuses itself, so the plugin never sees the request:

| Status | When |
|---|---|
| `401 Unauthorized` | The token is missing or wrong. |
| `403 Forbidden` | The `Host` header is not the loopback address, or an `Origin` header is present. |
| `404 Not Found` | No granted route covers the path, or the route was declined. |
| `405 Method Not Allowed` | The path is known but the method is not declared. The response has an `Allow` header. |
| `413 Content Too Large` | The request body is over 1 MiB. |
| `503 Service Unavailable` | The plugin is stopped, failed or disabled. A call still open at that moment is also answered `503`. |
| `504 Gateway Timeout` | The plugin did not call `respond` within 30 s. |

Each open call takes exactly one `respond`. A second `respond` for the same call returns `not-found`. Two requests in the same event batch are two calls with two ids; answer each one.

### Used by

The kernel's own loopback routes (feature 58) do not use this grant. They are served under `/api/` with the same token.

### See also

- [Capability: http-api.route](/pages/capability-http-api-route/) for the grant and its review text.
- [http](/pages/api-http/) for the outbound HTTP client, the other HTTP import.
- [cli](/pages/api-cli/) for the other server-side entry point that answers over a stream.
- [Plugin exports](/pages/api-exports/) for the `handler` export that receives `http-called`.
- [Events and intents](/pages/events-and-intents/) for the event batch that carries `http-called`.
- [Errors and limits](/pages/errors-and-limits/) for the shared error model.

Checked against `ilium:plugin@0.1.0`.
