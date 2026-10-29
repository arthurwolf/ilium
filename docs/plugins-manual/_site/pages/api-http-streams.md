# http streams

## http streams

A stream is an HTTP response that the plugin reads piece by piece while it is still arriving. Use it for server-sent events, newline-delimited JSON, long downloads and live feeds, where `fetch` (which waits for the whole body) does not fit. Streams are specified in the `http-stream` functions of the [WIT reference](/pages/reference-wit/#512-http). They use the same `http:<host-pattern>` grant as [http](/pages/api-http/), so read that page for request, header and host-pattern rules.

### What you need

- The grant `http:<host-pattern>` of the target host, on the side that opens the stream. There is no separate stream grant.
- Optionally `http:setting:<key>`, when the host comes from a setting the user edits. See [Hosts from a setting](#hosts-from-a-setting).
- Room under the `http_streams` limit: 4 open streams per plugin by default, 16 at the T1 ceiling, 4 at the T2 and T3 ceiling, and 64 for the whole session.
- Sides: server, client and scene (animation) worlds.

```json
"capabilities": { "server": ["http:stream.example.com"], "client": ["http:setting:feed_host"] }
```

### Signatures

```wit
// Excerpt of ilium:host/http in reference-wit
enum stream-framing { raw, lines, server-sent-events, ndjson }
record stream-request { request: http-request, framing: stream-framing, max-chunk-bytes: u32 }   // <= 1 MiB
type stream-id = u64;
open-stream: func(request: stream-request) -> result<stream-id, plugin-error>;
close-stream: func(stream: stream-id);
// @unstable(feature = websocket)
open-websocket: func(url: string, protocols: list<string>) -> result<stream-id, plugin-error>;
send-websocket: func(stream: stream-id, text: string) -> result<_, plugin-error>;
```

Events (defined in the [WIT reference](/pages/reference-wit/#412-events)):

| Event | Payload | Delivery |
|---|---|---|
| `http-stream-data` | `stream: stream-id`, `status: option<u16>`, `headers: option<list<header>>` (`some` on the first chunk only), `chunk: list<u8>` | Never dropped. The host stops reading the socket while the queue is full. |
| `http-stream-closed` | `stream: stream-id`, `outcome: stream-outcome` (`ended`, `cancelled`, `failed(plugin-error)`) | Never dropped. |

| Rust | TypeScript |
|---|---|
| `http::open_stream(StreamRequest { request, framing, max_chunk_bytes })?` returns `StreamId` | `openStream({ request, framing, maxChunkBytes })` returns `bigint` |
| `http::close_stream(stream)` | `closeStream(stream)` |
| `Event::HttpStreamData(HttpStreamDataEvent)`, `Event::HttpStreamClosed(HttpStreamClosedEvent)` | `{ tag: 'http-stream-data', val }`, `{ tag: 'http-stream-closed', val }` |
| `http::open_websocket(&url, &protocols)?` (unstable) | `openWebsocket(url, protocols)` (unstable) |
| `http::send_websocket(stream, &text)?` (unstable) | `sendWebsocket(stream, text)` (unstable) |

`open-stream` returns at once with the id; connecting, status and data all arrive as events. A stream is owned by the plugin that opened it. It closes when the server ends the body, when the plugin calls `close-stream`, when the connection fails, or when the plugin deactivates, traps or loses the grant. On those last three the host does not deliver `http-stream-closed` to the dead instance.

### Framing

Framing decides what one `http-stream-data` chunk contains. With every framing except `raw`, the host splits the body, so a chunk is one complete record and the plugin never reassembles lines.

| Framing | One chunk is | Notes |
|---|---|---|
| `raw` | Whatever bytes arrived, at most `max-chunk-bytes` | Larger network reads are split. Use it for downloads and binary data. |
| `lines` | One line without its terminator | `\n` and `\r\n` both end a line. |
| `server-sent-events` | The payload of one event | The host does the SSE parsing, so the plugin sees records, not `data:` prefixes. |
| `ndjson` | One JSON text line | The host does not parse the JSON. A blank line is skipped. |

`max-chunk-bytes` is at most 1 MiB. A record longer than that is delivered in several chunks.

<figure>
<svg viewBox="0 0 440 270" role="img" aria-label="Sequence of an HTTP stream from open to close">
<title>HTTP stream sequence</title>
<desc>The plugin calls open-stream and receives a stream id. The host connects, the first http-stream-data event carries the status and headers, later events carry chunks, and http-stream-closed ends the stream. The plugin may close earlier.</desc>
<defs>
<marker id="hs-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker>
</defs>
<text x="70" y="16" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
<text x="220" y="16" font-size="12" text-anchor="middle" fill="currentColor">Host</text>
<text x="370" y="16" font-size="12" text-anchor="middle" fill="currentColor">Server</text>
<line x1="70" y1="24" x2="70" y2="262" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="220" y1="24" x2="220" y2="262" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="370" y1="24" x2="370" y2="262" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="70" y1="44" x2="218" y2="44" stroke="currentColor" marker-end="url(#hs-arrow)"/>
<text x="76" y="38" font-size="10" fill="currentColor">open-stream</text>
<line x1="218" y1="62" x2="72" y2="62" stroke="currentColor" marker-end="url(#hs-arrow)"/>
<text x="214" y="56" font-size="10" text-anchor="end" fill="currentColor">stream id</text>
<line x1="220" y1="86" x2="368" y2="86" stroke="currentColor" marker-end="url(#hs-arrow)"/>
<text x="226" y="80" font-size="10" fill="currentColor">GET (grant, rate limit)</text>
<line x1="368" y1="110" x2="222" y2="110" stroke="currentColor" marker-end="url(#hs-arrow)"/>
<text x="364" y="104" font-size="10" text-anchor="end" fill="currentColor">200, body begins</text>
<line x1="218" y1="134" x2="72" y2="134" stroke="currentColor" marker-end="url(#hs-arrow)"/>
<text x="214" y="128" font-size="10" text-anchor="end" fill="currentColor">http-stream-data (status, chunk)</text>
<line x1="218" y1="158" x2="72" y2="158" stroke="currentColor" marker-end="url(#hs-arrow)"/>
<text x="214" y="152" font-size="10" text-anchor="end" fill="currentColor">http-stream-data (chunk)</text>
<line x1="70" y1="190" x2="218" y2="190" stroke="currentColor" marker-end="url(#hs-arrow)"/>
<text x="76" y="184" font-size="10" fill="currentColor">close-stream (optional)</text>
<line x1="368" y1="214" x2="222" y2="214" stroke="currentColor" marker-end="url(#hs-arrow)"/>
<text x="364" y="208" font-size="10" text-anchor="end" fill="currentColor">end of body or error</text>
<line x1="218" y1="244" x2="72" y2="244" stroke="currentColor" marker-end="url(#hs-arrow)"/>
<text x="214" y="238" font-size="10" text-anchor="end" fill="currentColor">http-stream-closed (outcome)</text>
</svg>
<figcaption>The first data event carries the status and headers. The closed event always ends a stream that the server or the network ends.</figcaption>
</figure>

### Example: follow a server-sent event feed

The plugin opens one feed when it activates, keeps the stream id, and handles each event as one chunk. When the stream closes with a retryable error, it reopens after the delay in `retry-after-ms`. `apply_update` and `applyUpdate` are the plugin's own code.

**Grants:** `http:feed.example.com`

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::http::{self, Header, HttpRequest, StreamFraming, StreamRequest};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::types::events::{HttpStreamClosedEvent, HttpStreamDataEvent};

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct FeedReader {
    stream: Option<u64>,
}

impl FeedReader {
    fn open(&mut self) -> Result<(), PluginError> {
        let request = HttpRequest {
            method: "GET".to_string(),
            url: "https://feed.example.com/v1/events".to_string(),
            headers: vec![Header { name: "accept".into(), value: "text/event-stream".into() }],
            body: None,
            timeout_millis: 300_000,
        };
        self.stream = Some(http::open_stream(StreamRequest {
            request,
            framing: StreamFraming::ServerSentEvents,
            max_chunk_bytes: 65_536,
        })?);
        Ok(())
    }
}

impl Lifecycle for FeedReader {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        self.open()
    }
    fn deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        if let Some(stream) = self.stream.take() {
            http::close_stream(stream);
        }
        None
    }
}

impl Handler for FeedReader {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::HttpStreamData(HttpStreamDataEvent { stream, chunk, .. })
                    if self.stream == Some(stream) =>
                {
                    apply_update(&chunk);
                }
                Event::HttpStreamClosed(HttpStreamClosedEvent { stream, outcome })
                    if self.stream == Some(stream) =>
                {
                    self.stream = None;
                    if let Err(error) = outcome {
                        log::log(Level::Warn, &format!("feed closed: {:?}", error.code));
                    }
                }
                _ => {}
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { closeStream, openStream } from '@ilium/plugin-sdk/host/http';
import { log } from '@ilium/plugin-sdk/host/log';
import { applyUpdate } from './feed';

let stream: bigint | undefined;

function open(): void {
  stream = openStream({
    request: {
      method: 'GET',
      url: 'https://feed.example.com/v1/events',
      headers: [{ name: 'accept', value: 'text/event-stream' }],
      body: undefined,
      timeoutMillis: 300000,
    },
    framing: 'server-sent-events',
    maxChunkBytes: 65536,
  });
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate() {
    open();
  },
  deactivate() {
    if (stream !== undefined) {
      closeStream(stream);
      stream = undefined;
    }
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'http-stream-data' && event.val.stream === stream) {
        applyUpdate(event.val.chunk);
      } else if (event.tag === 'http-stream-closed' && event.val.stream === stream) {
        stream = undefined;
        if (event.val.outcome.tag === 'err') {
          log('warn', `feed closed: ${event.val.outcome.val.code}`);
        }
      }
    }
    return [];
  },
});
```

### Example: NDJSON progress lines

The same code with `framing: NDJSON` (`'ndjson'` in TypeScript) delivers one JSON object per chunk. Parse inside the handler and ignore a line that does not parse; a server may interleave comments or keep-alive text.

**Rust**

```rust
if let Ok(update) = serde_json::from_slice::<Update>(&chunk) {
    self.progress = update.percent;
}
```

**TypeScript**

```ts
try {
  const update = JSON.parse(new TextDecoder().decode(chunk)) as { percent: number };
  progress = update.percent;
} catch {
  // ignore a line that is not JSON
}
```

For an unbounded download, use `raw` and write each chunk with `fs.write` in `append` mode; the host's back-pressure then paces the socket to the disk.

### Politeness

The host, not the plugin, keeps requests polite, so several plugins cannot flood one server.

| Mechanism | Effect |
|---|---|
| Per-host rate limiter | Requests to one host are spaced by the host. A request over the allowance fails with `rate-limited`. |
| `Retry-After` | A server `Retry-After` makes the call, or the stream, fail with `rate-limited` and `retry-after-ms` set. Wait that long before the next open. |
| ETag cache for `fetch` | The host sends `If-None-Match` for a URL it has seen and returns a cached `304` as the cached `200`. The plugin always sees a `200` with the full body. |
| Back-pressure | While the plugin's event queue is full, the host stops reading the socket instead of dropping data. |
| Session cap | At most 64 open streams for the whole session and 32 open network connections (one TCP and TLS connection to a remote host; streams to one host share a connection only when the server speaks HTTP/2). Over either cap `open-stream` fails at once with `quota-exceeded`. |

### Hosts from a setting

When the user chooses the server, as with an Overpass or tile mirror, the manifest declares `http:setting:<key>` instead of a fixed host. `<key>` must be one of the plugin's own setting keys (otherwise `manifest.grant-argument`). The setting is a `text` row with `item_format: "url"` or `"host"`, or a `list` of them. Wildcards in a stored value are refused.

```json
{
  "contributes": { "settings": { "sections": [ { "id": "feed", "rows": [
    { "key": "feed_host", "type": "text", "label": "Feed server", "item_format": "host", "default": "feed.example.com" }
  ] } ] } },
  "capabilities": { "client": ["http:setting:feed_host"] }
}
```

The host checks the current value at `open-stream` and at `fetch`. When the value changes to a host the user has not confirmed, the user is asked once, before the first request. Until the answer, calls to that host fail with `not-permitted`.

<pre class="term"><span class="t-dim">╭  ≡ ● · Allow new host──────────────────────────╮</span>
<span class="t-dim">│</span> example.map wants to connect to:               <span class="t-dim">│</span>
<span class="t-dim">│</span>   <span class="t-hi">overpass.local.example:8443</span>                  <span class="t-dim">│</span>
<span class="t-dim">│</span> This host comes from its setting "Feed server".<span class="t-dim">│</span>
<span class="t-dim">│</span>                                                <span class="t-dim">│</span>
<span class="t-dim">│</span>        Esc Deny              Enter Allow       <span class="t-dim">│</span>
<span class="t-dim">│</span> Esc cancel · Enter allow                       <span class="t-dim">│</span>
<span class="t-dim">╰────────────────────────────────────────────────╯</span>
</pre>

Illustrative, not generated. The confirmation after the user edits a host setting, drawn in the style of the client's confirm dialogs: rounded border, chrome title, key hints and a dim hint line. The exact wording is the host's.

The setting row itself looks like any text row, with leaders:

<pre class="term">Feed server …………………………………… [ overpass.local.example:8443 ]
</pre>

### WebSocket (unstable)

`open-websocket` and `send-websocket` exist only when the feature `websocket` is enabled for development builds (`@unstable(feature = websocket)`). A plugin that does not opt in to the feature cannot import them. On a host built without the feature, a plugin that did opt in gets `unsupported` from `open-websocket`. Server frames arrive as `http-stream-data` events and the end as `http-stream-closed`. Send text with `send-websocket`. The grant is the same `http:<host-pattern>`, for a `wss://` URL.

### Errors and limits

| Code | Where | When |
|---|---|---|
| `not-permitted` | `open-stream`, at once | Host not granted, not yet confirmed (setting host), or a redirect led to an ungranted host. |
| `quota-exceeded` | `open-stream`, at once | `http_streams` reached for the plugin or the session. |
| `invalid-argument` | `open-stream`, at once | `max-chunk-bytes` over 1 MiB, or the request rules of [http](/pages/api-http/) are broken. |
| `unsupported` | `open-websocket` | The feature `websocket` is off. |
| `network` | `http-stream-closed` | DNS, TLS or connect failure, or a dropped connection. |
| `timed-out` | `http-stream-closed` | No byte arrived before `timeout_millis`. |
| `rate-limited` | `http-stream-closed` or at once | The per-host limiter or a `Retry-After`; `retry-after-ms` is set. |
| `revoked` | `http-stream-closed` | The grant changed while the stream was open. |

| Limit | Value |
|---|---|
| `http_streams` per plugin | 4 default, 16 T1 ceiling, 4 T2 and T3 ceiling |
| Streams per session | 64 (`quota-exceeded` over the cap) |
| Open connections per session | 32 (`quota-exceeded` over the cap) |
| Chunk | at most 1 MiB; larger records are split |

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs example.feed --tail 4
<span class="t-dim">09:30:00</span> <span class="t-ok">info</span>  http.open-stream feed.example.com sse
<span class="t-dim">09:30:00</span> <span class="t-ok">info</span>  stream 7 opened: status 200
<span class="t-dim">09:41:12</span> <span class="t-warn">warn</span>  stream 7 closed: rate-limited, retry in 30000 ms
<span class="t-dim">09:41:42</span> <span class="t-ok">info</span>  http.open-stream feed.example.com sse
</pre>

Illustrative, not generated. A plugin log of a stream that the server closed with a `Retry-After`.

### See also

- [http](/pages/api-http/) for `fetch`, host patterns and the request record.
- [Capability: http](/pages/capability-http/) for the grant and its review text.
- [Events](/pages/api-events/) for the delivery policy of stream events.
- [Settings](/pages/api-settings/) for the rows that hold a host.
- [Errors and limits](/pages/reference-errors-and-limits/) for the shared codes.

### Security

Stream size and egress budget limits are in [Network egress](/pages/reference-network-egress/) ([SEC-082], [SEC-083]).
