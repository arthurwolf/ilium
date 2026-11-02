# Requests and outcomes

## Requests and outcomes

Some host calls take longer than a handler can wait. A handler has a deadline of 20 ms by default, so an HTTP request, a whole-file read or a process run cannot finish inside one `on-events` call. For those calls the host returns a `request-id` at once. The typed result arrives later, as a `request-completed` event that carries the id, and the plugin takes it with a `take-*` function.

Submissions follow the same pattern. Every submission that a handler returns completes exactly once with an outcome, delivered as a `request-completed` event too.

## You will need

- [Events and intents](/pages/events-and-intents/) for the event batch and for the submissions that a handler returns.
- The grant for the call. `http.fetch` needs `http:<host-pattern>`, for example `http:api.example.com`. `fs.read-all` needs `fs.read:<path>`. Without the grant the call returns `not-permitted` at once.
- The grant for the intent of a submission. The `notify` intent needs `notify.desktop`. Without it the submission completes as `refused(not-permitted)`.
- The Rust SDK (`ilium-plugin-sdk`) or the TypeScript SDK (`@ilium/plugin-sdk`).

## Two kinds of identifier

| Identifier | Type | Chosen by | Names | Appears in |
|---|---|---|---|---|
| `correlation` | `u64` | The plugin | One submission | `submission.correlation`, and `request-ref` `submission(n)` |
| `request-id` | `u64` | The host | One slow host call | The raw call's return value, `request-ref` `call(id)`, and every `take-*` function. Unique per plugin side for the whole server run, starts at 1 at each server start and is never reused across plugin-instance restarts [ABI-040] |

The plugin picks correlation numbers below 2^63. The SDK allocates its own numbers from 2^63 upward.

## The surface

### Types

The WIT shapes are `submission { intent, correlation, dedup-key }`, `request-ref { submission(u64), call(request-id) }`, `outcome { ok, ok-node, duplicate, refused, failed, cancelled, ok-epoch }` and `request-result { request, outcome }`. Each shape maps to the SDK forms below.

| WIT | Rust | TypeScript |
|---|---|---|
| `submission` | `Submission { intent, correlation, dedup_key }` | `{ intent, correlation: bigint, dedupKey: string \| undefined }` |
| `request-ref` `call(id)` | `RequestRef::Call(id)` | `{ tag: 'call', val: id }` |
| `request-ref` `submission(n)` | `RequestRef::Submission(n)` | `{ tag: 'submission', val: n }` |
| `outcome` `ok` | `Outcome::Ok` | `{ tag: 'ok' }` |
| `outcome` `ok-node(id)` | `Outcome::OkNode(id)` | `{ tag: 'ok-node', val: id }` |
| `outcome` `duplicate(r)` | `Outcome::Duplicate(r)` | `{ tag: 'duplicate', val: r }` |
| `outcome` `refused(e)` | `Outcome::Refused(e)` | `{ tag: 'refused', val: e }` |
| `outcome` `failed(e)` | `Outcome::Failed(e)` | `{ tag: 'failed', val: e }` |
| `outcome` `cancelled` | `Outcome::Cancelled` | `{ tag: 'cancelled' }` |

`plugin-error` has a `code`, a `message` and an optional `retry-after-ms`. Its codes are listed in [Errors and limits](/pages/reference-errors-and-limits/).

### Slow calls

Each slow call exists in two forms. The async wrapper is the normal form. The raw form returns the `request-id` and leaves the completion to `on-events`.

| Interface | Raw function | Take function | Typed result |
|---|---|---|---|
| `fs` | `read-all`, `scan`, `lock`, `pick`, `read-with-revision`, `write-atomic` | `take-bytes`, `take-entries`, `take-lock`, `take-picked`, `take-revisioned`, `take-written` | Bytes, entries, lock id, path, bytes with a revision, revision token |
| `fs` | `write`, `remove`, `rename` | None | `ok` or `failed` only |
| `http` | `fetch` | `take-response` | `http-response` |
| `jobs` | `start` | `take-output` | Output bytes |
| `process` | `run` | `take-exit` | `process-exit` |
| `inference` | `complete`, `models` | `take-completion`, `take-models` | Completion, model names |
| `vector` | `embed` | `take-vectors` | One vector of `f32` per input |
| `services` | `call` | `take-reply` | JSON text |
| `editor` | `load-file` | `take-buffer` | Buffer id |
| `editor` | `save-buffer` | None | `ok` or `failed` only |
| `catalogs` | `query-async` | `take-page` | `catalog-page` |
| `geocode` | `search`, `reverse` | `take-places` | List of `geo-place` |
| `sessions` | `send` | `take-reply` | JSON text |
| `client-requests` | `request` | `take-answer` | JSON text |
| `extensions` | `run-hook` | `take-reply` | JSON text |
| `terminal` | `history-text`, `search-history` | `take-history`, `take-matches` | Text, list of `history-match` |
| `agent` | `transcript` | `take-transcript` | List of `transcript-turn` |
| `cli` | `read-stdin` | `take-stdin` | Bytes |
| `commands` | `invoke` | None | `ok` or `failed` only |
| `audio` | `play` | None | `ok` or `failed` only |
| `intents-host` | `submit` | None | The outcome of the submission |

Every row follows the take rules [ABI-014]: a result is held for 30 seconds, counted from the end of the `on-events` call that delivered the completion [ABI-012], and a take before completion, a second take or a take of an unknown id returns `none` (the wrapper returns `not-found`). The same rows are in the [host conformance matrix](/pages/reference-host-conformance/#2-import-matrix).

In Rust the wrapper is named after the WIT function, for example `http::fetch(request).await`. The raw form has the suffix `_request`, for example `http::fetch_request(request)`, and the take function keeps its name, `http::take_response(id)`. In TypeScript the same names appear in camelCase: `fetchRequest(request)` and `takeResponse(id)`.

## Slow call: fetch, complete, take

A plugin fetches a page from a host it is granted. The flow has three steps.

1. The plugin calls `http.fetch` with an `http-request`. The call returns a `request-id` at once, for example 5001.
2. The kernel completes the request later and delivers a `request-completed` event in a batch.
3. The plugin calls `take-response` with the request id and receives the typed result.

The completion event carries `request: call(5001)` and `outcome: ok`. The outcome `ok` means that the result is ready for `take-response`. It does not mean that the HTTP status was 200. The status is in the `http-response` that the take returns. The result is held for 30 seconds, or until it is taken once. The hold starts when `on-events` returns for the batch that delivered the `request-completed` event, not when the host emitted the event [ABI-037]. After the hold, `take-response` returns `none` and the request no longer counts toward `pending_requests`.

The sequence below shows the three steps with the kernel between the plugin and the HTTP service.

<figure>
<svg viewBox="0 0 400 250" role="img" aria-label="Sequence of an HTTP request with request id and take">
  <defs>
    <marker id="req-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <title>Request and outcome sequence</title>
  <desc>Three lanes: the plugin, the kernel and the HTTP service. The plugin calls fetch and gets a request id at once. The kernel forwards the request, the service replies, and the plugin receives a completion event before it takes the response.</desc>
  <line x1="70" y1="36" x2="70" y2="240" stroke="currentColor" stroke-dasharray="3 3"/>
  <line x1="200" y1="36" x2="200" y2="240" stroke="currentColor" stroke-dasharray="3 3"/>
  <line x1="330" y1="36" x2="330" y2="240" stroke="currentColor" stroke-dasharray="3 3"/>
  <text x="70" y="24" font-size="11" text-anchor="middle" fill="currentColor">Plugin</text>
  <text x="200" y="24" font-size="11" text-anchor="middle" fill="currentColor">Kernel</text>
  <text x="330" y="24" font-size="11" text-anchor="middle" fill="currentColor">HTTP service</text>
  <line x1="70" y1="56" x2="200" y2="56" stroke="currentColor" marker-end="url(#req-arrow)"/>
  <text x="135" y="50" font-size="10" text-anchor="middle" fill="currentColor">http.fetch</text>
  <line x1="200" y1="86" x2="70" y2="86" stroke="currentColor" marker-end="url(#req-arrow)"/>
  <text x="135" y="80" font-size="10" text-anchor="middle" fill="currentColor">request-id 5001</text>
  <line x1="200" y1="116" x2="330" y2="116" stroke="currentColor" marker-end="url(#req-arrow)"/>
  <text x="265" y="110" font-size="10" text-anchor="middle" fill="currentColor">request</text>
  <line x1="330" y1="146" x2="200" y2="146" stroke="currentColor" marker-end="url(#req-arrow)"/>
  <text x="265" y="140" font-size="10" text-anchor="middle" fill="currentColor">reply</text>
  <line x1="200" y1="176" x2="70" y2="176" stroke="currentColor" marker-end="url(#req-arrow)"/>
  <text x="135" y="170" font-size="10" text-anchor="middle" fill="currentColor">request-completed ok</text>
  <line x1="70" y1="206" x2="200" y2="206" stroke="currentColor" marker-end="url(#req-arrow)"/>
  <text x="135" y="200" font-size="10" text-anchor="middle" fill="currentColor">take-response 5001</text>
  <line x1="200" y1="236" x2="70" y2="236" stroke="currentColor" marker-end="url(#req-arrow)"/>
  <text x="135" y="230" font-size="10" text-anchor="middle" fill="currentColor">http-response</text>
</svg>
<figcaption>Time runs downwards. The plugin receives a request id at once, and the typed response only after the completion event and a take call.</figcaption>
</figure>

Take the result once. A second take returns `none`, as the replay below shows.

<pre class="term"><span class="t-prompt">$</span> ilium plugin test ./fetcher --fixture fetch.json
<span class="t-dim">2026-10-11 10:02:14</span>  <span class="t-hi">call</span>      http.fetch -> request 5001
<span class="t-dim">2026-10-11 10:02:14</span>  <span class="t-dim">batch</span>     on-events, completion pending
<span class="t-dim">2026-10-11 10:02:15</span>  <span class="t-ok">complete</span>  request 5001 outcome ok
<span class="t-dim">2026-10-11 10:02:15</span>  <span class="t-ok">take</span>      take-response 5001 -> http-response 200
<span class="t-dim">2026-10-11 10:02:16</span>  <span class="t-warn">take</span>      take-response 5001 -> none, already taken
</pre>

Illustrative, not generated. The second take returns none. An async wrapper turns that same none into `not-found`.

## The outcomes

Every submission completes exactly once. The table lists the outcomes and what each one means for the plugin.

| Outcome | When it happens | What the plugin does |
|---|---|---|
| `ok` | The intent was applied. For a call, the result is ready to take. | Take the result within 30 seconds. |
| `ok-node(node-id)` | The intent was applied and created a node. | Keep the node id if the plugin needs the node later. |
| `ok-epoch(u64)` | `send-input` on an agent pane applied. The value is the new prompt epoch. | Keep it to pass as `if-current` or `prompt-epoch` in a later input. |
| `duplicate(original)` | The dedup key was already taken within the dedup window. Nothing was applied. | The outcome says nothing about the result of the first submission, which may still be running or may have failed. Read the completion of the first submission when the result matters. |
| `refused(plugin-error)` | The submission failed a check at acceptance, so nothing was applied, for example `not-found` or `conflict`. | Read the code. `busy`, `rate-limited` and `conflict` can succeed later. |
| `failed(plugin-error)` | The submission was accepted, but applying it or running it failed, for example `io`, `timed-out` or `network`. | Read the code. A retry of an accepted submission needs a new dedup key. |
| `cancelled` | The request stopped before it finished, because the plugin stopped, the user cancelled, or the host shut down. A sibling of a refused submission is not `cancelled`: it completes as `refused(cancelled)` with the message `batch refused: submission <index>` [ABI-009]. | Do nothing, or start the work again on the next trigger. |

Submissions of one batch are applied in order. If an earlier submission removes a node, a later submission for that node completes as `failed(not-found)`.

A batch of more than 256 submissions traps the call [ABI-008]. A batch is refused as a whole only by the two batch-level checks, which run over the whole list in index order before any effect. Check 1 is shape (a valid variant tag, required fields, lengths within the ABI caps, a dedup key where the intent requires one, and a key of 1 to 128 characters from the documented set). Check 2 is the grant (requested in the manifest and currently granted). The lowest-index submission that fails completes as `refused(invalid-argument)` for shape or `refused(not-permitted)` for grant. Inside one submission shape is checked before grant, so a submission with both defects is reported as malformed [RQO-009]. Every other submission in the batch completes as `refused(cancelled)`. The offender and its siblings carry the message `batch refused: submission <index>` with the 0-based index of the offender, and the log line has the same text. Nothing in a refused batch is applied, and no dedup key of the batch is reserved [RQO-011]. Every other check (target exists, pane ready, payload value valid, rate, quota, conflict, `if-current`) is per submission and affects only that submission. A batch of more than 256 submissions is not a refusal: it is a trap that counts as one failure, no submission is applied, and none completes [RQO-010]. A missing grant on `intents-host.submit`, which carries a single submission, is reported as `err(not-permitted)` at once.

## Hook runs

`extensions.run-hook` is a request like a service call. It returns a request id at once. The result arrives as the usual `request-completed` event, with no new event type.

| Outcome | Meaning for the owner |
|---|---|
| `ok` | The chain finished. Call `take-reply` for the final payload (`transform`) or the answer (`first`). `observe` and `veto` give none |
| `refused(conflict)` with message prefix `extension.vetoed` | A `veto` handler refused. The message is `extension.vetoed: <contributor>: <reason>`. Do not perform the action |
| `failed(...)` with an `extension.*` message prefix | The call itself failed, for example `failed(invalid-argument)` with prefix `extension.schema-mismatch` |

A timeout in a handler is never a veto. The chain treats it as a pass. See [extension points](/pages/api-extension-points/).

## Uncertain input after a restart

`send-input` with `journal: true` writes a journal entry before the keys reach the pane. If the server restarts before the outcome is recorded, the plugin cannot know whether the keys arrived. After activation the plugin receives `input-uncertain` with the pane and the dedup key. The plugin decides: retry with a new key, or ask the user.

**Rust**

```rust
if let Event::InputUncertain(uncertain) = event {
    log::log(Level::Warn, &format!("check pane {} before resending", uncertain.pane));
}
```

**TypeScript**

```ts
if (event.tag === 'input-uncertain') {
  log('warn', `check pane ${event.val.pane} before resending`);
}
```

## Duplicate submissions and dedup keys

A submission carries an optional `dedup-key`. It has 1 to 128 characters from `A-Z`, `a-z`, `0-9`, `.`, `_`, `:`, `/` and `-`. The key is scoped to the plugin id for submissions the kernel handles itself, and it stays reserved for 10 minutes from the moment the kernel accepts the submission. A refused submission does not reserve its key, so a retry with the same key after fixing the cause is valid. An accepted submission keeps its key even if it later fails or is cancelled. A key is reserved while `now_wall_ms - accepted_wall_ms < 600,000`. The accepted time is persisted in the journal on the wall clock, so it survives a server restart, and at exactly 600,000 ms the key is free [ABI-034].

A submission that a client plugin half forwards to the server has a wider scope: the pair (plugin id, client id), held by the server for the same 10 minutes. Two clients that send the same key for the same plugin are two submissions. See [Plugin traffic over IPC](/pages/reference-host-ipc/).

A repeated key completes as `duplicate`, and the outcome names the first submission by the ref it was first submitted with, even across instances, server restarts and re-attach resends. If submission 17 repeats the key of submission 16, the event for 17 carries `duplicate(submission 16)`, and the kernel applies only the first one. `duplicate` means only that the key was taken within the window. It makes no claim about the result of the original. The host does not check that correlation numbers are unique: completions carry the number as given, and the plugin owns uniqueness [ABI-035].

`send-input` and `agent-action` must carry a key, and a submission of either without one refuses its batch. The kernel journals these keys before it writes to the terminal, so a duplicate is caught after a server restart. The journal keeps 24 hours or 4,096 entries per session, whichever is smaller. The journal and the dedup window are separate rules. The dedup window is always 10 minutes from reservation, also across a server restart: a repeat of a journaled key within 10 minutes completes as `duplicate`, and a repeat after 10 minutes is a new submission and is applied. The journal only lets the plugin learn, through `input-uncertain`, that a submission was in flight at a restart; it never makes a key unusable for longer than 10 minutes and never replays input [RQO-030].

The Rust helper `dedup::key_for(&["finish", "node-12"])` returns `finish:node-12`. TypeScript writes the key string directly.

## Limits

| Limit | Value | Result when exceeded |
|---|---|---|
| Pending requests per plugin (`pending_requests`) | 64 by default; ceiling 256 for tier T1, 64 for T2 and T3 | `busy`, synchronously |
| Pending requests per session | 4,096 | `busy`, synchronously |
| Hold of a completed result | 30 seconds, or until the first take | Take returns `none`; wrapper returns `not-found` |
| Command or service reply | 30 seconds | `failed(timed-out)` |
| Inference provider reply | 120 seconds | `failed(timed-out)` |
| Dedup window | 10 minutes | Duplicate outcome |
| Submissions per batch | 256 | Trap (counts as a failure, nothing applied, nothing completes) |

Per-tier ceilings are set by the manifest and clamped by the host. See [Security](/pages/security/) for the tiers.

## Worked example in Rust

This client-side plugin fetches release notes once, when it activates. The wrapper returns a response or an error, so the code reads as ordinary sequential code.

```rust
use ilium_plugin_sdk::host::http::{self, Header, HttpRequest};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct ReleaseNotes;

impl Lifecycle for ReleaseNotes {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        task::spawn(fetch_release_notes());
        Ok(())
    }
}

impl Handler for ReleaseNotes {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

async fn fetch_release_notes() {
    let request = HttpRequest {
        method: "GET".to_string(),
        url: "https://api.example.com/v1/release-notes".to_string(),
        headers: vec![Header {
            name: "accept".to_string(),
            value: "application/json".to_string(),
        }],
        body: None,
        timeout_millis: 5_000,
    };
    match http::fetch(request).await {
        Ok(response) if response.status == 200 => {
            let text = format!("release notes: {} bytes", response.body.len());
            log::log(Level::Info, &text);
        }
        Ok(response) => {
            let text = format!("feed returned status {}", response.status);
            log::log(Level::Warn, &text);
        }
        Err(error) if error.code == ErrorCode::Busy => {
            log::log(Level::Warn, "too many pending requests; try again later");
        }
        Err(error) => {
            log::log(Level::Error, &format!("feed failed: {}", error.message));
        }
    }
}
```

The `Busy` arm handles a full pending queue. Any other error, such as `revoked` or `timed-out`, reaches the last arm.

## Worked example in TypeScript

This example uses the raw form. `activate` starts the request and keeps its id. `onEvents` matches each completion with the id, takes the response, and submits a notification with a dedup key. The key is `feed:<name>:<day>`, so a second completion on the same day does not notify twice.

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { fetchRequest, takeResponse, type HttpRequest } from '@ilium/plugin-sdk/host/http';
import { log } from '@ilium/plugin-sdk/host/log';

// Request ids of the calls this plugin started, mapped to a feed name.
const feedByRequest = new Map<bigint, string>();

function startFeedFetch(): void {
  const request: HttpRequest = {
    method: 'GET',
    url: 'https://api.example.com/v1/release-notes',
    headers: [{ name: 'accept', value: 'application/json' }],
    body: undefined,
    timeoutMillis: 5000,
  };
  try {
    const requestId = fetchRequest(request);
    feedByRequest.set(requestId, 'release-notes');
  } catch (error) {
    log('error', `feed request refused: ${String(error)}`);
  }
}

function submissionsForCompletions(batch: Event[]): Submission[] {
  const submissions: Submission[] = [];
  for (const event of batch) {
    if (event.tag !== 'request-completed') continue;
    const { request, outcome } = event.val;
    if (request.tag !== 'call') continue;
    const feedName = feedByRequest.get(request.val);
    if (feedName === undefined) continue;
    feedByRequest.delete(request.val);
    if (outcome.tag !== 'ok') {
      log('warn', `feed ${feedName} did not complete: ${outcome.tag}`);
      continue;
    }
    const response = takeResponse(request.val);
    if (response === undefined || response.status !== 200) {
      log('warn', `feed ${feedName} gave no usable response`);
      continue;
    }
    const day = new Date().toISOString().slice(0, 10);
    submissions.push({
      intent: {
        tag: 'notify',
        val: {
          title: 'Release notes updated',
          body: feedName,
          urgency: 'normal',
          sound: undefined,
        },
      },
      correlation: BigInt(submissions.length + 1),
      dedupKey: `feed:${feedName}:${day}`,
    });
  }
  return submissions;
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {
    startFeedFetch();
  },
  onEvents(batch: Event[]): Submission[] {
    return submissionsForCompletions(batch);
  },
});
```

`fetchRequest` throws a `PluginError` when the call is refused at once. Raw completions reach `onEvents` unchanged, so the plugin matches ids itself.

## Edge cases and errors

| Situation | Result | Retry |
|---|---|---|
| No grant for the host | `not-permitted`, at once | No |
| Pending requests full | `busy`, at once | Yes, later |
| CPU budget spent | `quota-exceeded` with `retry-after-ms`, at once | Yes, after the delay |
| Timeout passed before the response | `failed(timed-out)` | Yes |
| More than 5 redirects | `failed(network)` | Yes |
| Response body too large to hold | `failed(quota-exceeded)` | No |
| Grant withdrawn during the call | `failed(revoked)`; the plugin is re-activated | No |
| Plugin stopped, restarted or disabled | Pending requests are cancelled; a restarted instance does not receive their completions | Start the work again |
| Take after the 30-second hold, second take, or unknown id | `none`; wrapper returns `not-found` | Start the call again |
| Refused submission retried with the same key | Valid; the refusal did not reserve the key | Yes |
| Accepted submission retried with the same key | `duplicate`, no effect | Use a new key |
| User cancels `fs.pick` | `cancelled`; the wrapper fails with `cancelled` | Yes, if the user wants |

## What the user sees

Most requests run without visible UI. A few do:

- A file picker (`fs.pick`) shows a modal dialog. Cancelling it completes the request as `cancelled`.
- A `notify` intent shows a desktop notification, limited to 6 per minute.
- If a plugin fails three times within 10 minutes, the host disables it on that side and shows a notice in the tree footer and in Settings:

<pre class="term"><span class="t-warn">warn</span> Plugin demo-fetcher stopped after 3 failures in 10
     minutes: handler.on-events trap. Turn it on again in
     Settings &gt; Plugins.
</pre>

Illustrative, not generated. The notice shows the export that failed. It names a view only when the failing export is `views.render`.

## Rules

- Treat `ok` as "ready to take", not as "done with the result". Take it within 30 seconds.
- Handle `duplicate` as success. Use a new dedup key for each new attempt of an accepted action.
- Expect `cancelled` when the plugin stops. Pending host requests are cancelled at shutdown and at disable.

Requirement ids for this page: [EVT-021], [EVT-022], [RQO-001], [RQO-002], [RQO-003], [RQO-004], [RQO-005], [RQO-006], [RQO-007], [RQO-008], [RQO-009], [RQO-010], [RQO-011], [RQO-020], [RQO-021], [RQO-022], [RQO-030], [RQO-031], [RQO-040]. See the [requirements index](/pages/reference-requirements/).

## See also

- [Events and intents](/pages/events-and-intents/) for the event batch and the submission record.
- [Intents](/pages/api-intents/) for the intent catalogue and per-intent outcomes.
- [Background jobs](/pages/background-jobs/) and [Jobs](/pages/api-jobs/) for `jobs.wait-request`.
- [Errors and limits](/pages/reference-errors-and-limits/) for the full error code table.
- [Extension points](/pages/api-extension-points/) for `run-hook`.
- [Rust SDK](/pages/rust-sdk/), [TypeScript SDK](/pages/typescript-sdk/) and [Testing](/pages/testing/).

API version checked: `ilium:plugin@0.1.0`.

Requirements for this page: [Requests and outcomes requirements](/pages/reference-requirements-requests-outcomes/).
