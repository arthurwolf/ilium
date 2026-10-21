---
layout: page
title: intents
permalink: /pages/api-intents/
---

## intents

An intent is a request that asks the kernel to change something: rename a node, close a pane, type into a terminal, show a notice. A plugin never changes the tree, a pane or a window itself. It returns submissions from `on-events`, or calls `intents.submit` from a timer or request callback. The kernel checks each batch against the plugin's grants, applies the accepted submissions in order, and gives every submission exactly one outcome.

**You need:** the grant listed for each intent in the catalogue below, declared in the manifest for the side that submits it (see [capabilities](/pages/capabilities/)). Results that arrive later come as `request-completed` in [events](/pages/api-events/). In Rust, add the `ilium_plugin` attribute to a struct and implement `Handler` (see [Rust SDK](/pages/rust-sdk/)). In TypeScript, export the object from `definePlugin` (see [TypeScript SDK](/pages/typescript-sdk/)).

### Interface

The records, the outcome and the submit function, as declared in WIT:

```wit
record submission { intent: intent, correlation: u64, dedup-key: option<string> }
variant request-ref { submission(u64), call(request-id) }
variant outcome {
  ok, ok-node(node-id), duplicate(request-ref), refused(plugin-error),
  failed(plugin-error), cancelled,
  ok-epoch(u64),   // send-input on an agent pane, the new prompt epoch
}
record request-result { request: request-ref, outcome: outcome }

submit: func(submission: submission) -> result<request-id, plugin-error>;
```

The intent variant has one case per catalogue row. A case with one value carries that value. A case with several fields carries a record named `<case>-spec`:

```wit
// Excerpt of ilium:types/intents in reference-wit
variant intent {
  rename-node(rename-node-spec),
  move-node(move-node-spec),
  close-node(node-id),
  set-node-locked(set-node-locked-spec),
  create-pane(create-pane-spec),
  signal-pane(signal-pane-spec),
  send-input(send-input-spec),
  agent-action(agent-action-spec),
  set-decoration(set-decoration-spec),
  clear-decoration(decoration-key),
  set-virtual-rows(set-virtual-rows-spec),
  play-sound(string),
  notify(notify-spec),
  open-view(open-view-spec),
  close-view(view-target),
  invoke-command(invoke-command-spec),
  message(message-spec),
  editor-apply-edits(editor-apply-edits-spec),
  editor-open(editor-open-spec),
  // appended in this order
  freeze-pane(freeze-pane-spec),
  unfreeze-pane(unfreeze-pane-spec),
  replace-pane-command(replace-pane-command-spec),
  terminate-pane-process(terminate-pane-process-spec),
  resize-pane(resize-pane-spec),
  create-group(create-group-spec),
  create-folder(create-folder-spec),
  create-project(create-project-spec),
  create-split(create-split-spec),
  set-bookmarked(set-bookmarked-spec),
  set-expanded(set-expanded-spec),
  restructure(restructure-spec),
  revert-restructure(u64),
  set-inferred-icon(set-inferred-icon-spec),
  mark-attention(mark-attention-spec),
  ack-attention(node-id),
  open-external(open-external-spec),
  set-status-segment(set-status-segment-spec),
  clear-status-segment(string),
  set-frame-slot(set-frame-slot-spec),
  clear-frame-slot(clear-frame-slot-spec),
  toast(toast-spec),
  dismiss-toast(string),
}
```

New cases are only ever appended, so existing plugins keep decoding. The record names follow the same `<case>-spec` rule; the fields of each record are the payload columns of the tables below. A case with exactly one value carries that value, (`revert-restructure` carries the token, `dismiss-toast` the toast id).

Import paths:

- Rust: `ilium_plugin_sdk::types::intents::{Intent, Submission}` and `ilium_plugin_sdk::host::intents::submit`. The prelude (`use ilium_plugin_sdk::prelude::*;`) also brings in `Intent`, `Submission`, `Outcome` and `RequestRef`.
- TypeScript: `Submission` and `Intent` from `@ilium/plugin-sdk/types`, and `submit` from `@ilium/plugin-sdk/host/intents`.

`submit` returns a `request-id` at once. Its completion arrives later as `request-completed`, with `request-ref` set to `call(<request-id>)`. Use it only from timers and request callbacks. Inside `on-events`, return submissions instead.

### Batches

- `on-events` returns a list of `Submission` values. The kernel checks the batch against the grants and applies the submissions in order.
- A node removed by an earlier submission in the same batch makes a later submission on that node fail with `failed(not-found)`.
- A return of more than 256 submissions traps the plugin. None of them is applied.
- A batch is refused as a whole in two cases, both checked over the whole list in index order before any effect: a malformed submission (shape, then a missing or malformed dedup key), and a missing grant. The lowest-index offender gets `refused(invalid-argument)` or `refused(not-permitted)` (shape before grant inside one submission). Every other submission gets `refused(cancelled)`. All carry the message `batch refused: submission <index>`, with the 0-based index of the offender. Nothing is half applied and no dedup key is reserved.
- Every other check is per submission. A missing target, a payload value that is well formed but invalid (for example a text node id), a stale revision or a rate limit affects only its own submission.
- `correlation` is a number the plugin chooses. It is returned as `request-ref` `submission(<correlation>)` in `request-completed`. Plugin numbers stay below 2^63. `Submission::new` in Rust takes the next SDK number, which starts at 2^63.

<figure>
<svg viewBox="0 0 400 236" role="img" aria-label="How the kernel applies a submission batch">
  <defs>
    <marker id="intent-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7"
            markerHeight="7" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="110" y="8" width="180" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="30" font-size="12" text-anchor="middle" fill="currentColor">Batch from on-events</text>
  <line x1="200" y1="42" x2="200" y2="62" stroke="currentColor" marker-end="url(#intent-arrow)"/>
  <rect x="110" y="62" width="180" height="34" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="84" font-size="12" text-anchor="middle" fill="currentColor">Check batch and grants</text>
  <line x1="170" y1="96" x2="84" y2="122" stroke="currentColor" marker-end="url(#intent-arrow)"/>
  <line x1="230" y1="96" x2="316" y2="122" stroke="currentColor" marker-end="url(#intent-arrow)"/>
  <rect x="8" y="122" width="152" height="54" rx="6" fill="none" stroke="currentColor"/>
  <text x="84" y="144" font-size="12" text-anchor="middle" fill="currentColor">All checks pass</text>
  <text x="84" y="162" font-size="11" text-anchor="middle" fill="currentColor">apply in order</text>
  <rect x="240" y="122" width="152" height="54" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="316" y="144" font-size="12" text-anchor="middle" fill="currentColor">Malformed or no grant</text>
  <text x="316" y="162" font-size="11" text-anchor="middle" fill="currentColor">refuse the batch</text>
  <line x1="84" y1="176" x2="150" y2="200" stroke="currentColor" marker-end="url(#intent-arrow)"/>
  <line x1="316" y1="176" x2="250" y2="200" stroke="currentColor" marker-end="url(#intent-arrow)"/>
  <rect x="90" y="200" width="220" height="28" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="218" font-size="12" text-anchor="middle" fill="currentColor">request-completed events</text>
</svg>
<figcaption>A batch is all or nothing at the grant and shape level. Every submission then gets one outcome.</figcaption>
</figure>

### Functions

| Function | Signature | Meaning |
|---|---|---|
| `intents.submit` | `submit(submission) -> result<request-id, plugin-error>` | Queues one submission from a timer or request callback. Errors: `invalid-argument` for a malformed submission or a missing mandatory key, `not-permitted` at once when the grant is missing, `rate-limited`. The completion carries `submission(correlation)` from the submission passed in. The returned request id is informational and never appears in a completion. |

Rust: `host::intents::submit(submission)` returns `Result<RequestId, PluginError>`. TypeScript: `submit(submission)` returns the request id as `bigint` and throws `PluginError`.

### Dedup keys

A submission may carry a `dedup-key`. The rules:

- Format: 1 to 128 characters from `A-Z`, `a-z`, `0-9`, `.`, `_`, `:`, `/` and `-`. Any other character makes the submission malformed.
- Scope: the pair (plugin id, key), at the kernel that applies the intent. The server and each client keep separate tables.
- Window: 10 minutes from the moment the key is reserved.
- Duplicate: a submission with a reserved key completes as `duplicate(<original>)` and has no effect. `original` is the `request-ref` of the submission that reserved the key.
- A key is reserved only after every acceptance check passes. A refused submission keeps its key free, so a retry with the same key after fixing the cause is valid. A submission that was accepted keeps its key even if it later fails. Retry a failed action with a new key.
- `send-input` and `agent-action` require a key. Without one, the batch is refused. The kernel journals these keys before the write, for 24 hours or 4,096 entries per session, whichever is smaller. A restart therefore does not repeat the input.

The SDK helper `dedup::key_for(&["finish", &node.to_string()])` joins its parts with `:` and replaces disallowed characters with `-`. TypeScript plugins write the key string directly.

### Intent catalogue

| Intent | Payload | Grant | Outcome, beyond `ok` |
|---|---|---|---|
| `rename-node` | `node: node-id`, `title: string` | `tree.write` | `refused(conflict)` when the user named the node by hand and the plugin is a titler; `refused(not-found)` |
| `move-node` | `node`, `target: node-id`, `placement` (before, after, inside-first, inside-last) | `tree.write` | `refused(invalid-argument)` when the target is inside the moved node |
| `close-node` | `node` | `tree.write` | `refused(conflict)` on a locked node |
| `set-node-locked` | `node`, `locked: bool` | `tree.write` | `refused(not-found)` |
| `create-pane` | `parent: option<node-id>`, `title`, `content-kind: string`, `cwd: option<fs-path>`, `command: option<list<string>>`, `initial-input: option<string>` | `panes.create`, plus `terminal.write` when `initial-input` is set | `ok-node(node-id)`; `refused(not-found)` for a missing parent or `cwd`; `failed(not-found)` for a missing program; `failed(io)` on spawn failure |
| `signal-pane` | `pane: node-id`, `signal` (interrupt, terminate, kill, hangup) | `panes.signal` | `refused(not-found)` with no running process; `refused(pane-not-ready)` on a restored pane |
| `send-input` | `pane: node-id`, `text: string`, `delivery` (typed, submitted, pasted), `prompt-epoch: option<u64>`, key required | `terminal.write` | `refused(pane-not-ready)`; `refused(conflict)` on a stale epoch; `refused(rate-limited)` |
| `agent-action` | `pane: node-id`, `action-id: string`, key required | `agent.control` | `refused(unsupported)` when the signature has no such action; `refused(conflict)` when the activity is not allowed |
| `set-decoration` | `node`, `slot: string`, `content: styled-text`, `icon: option<string>`, `sort-key: option<s64>`, `persist: bool` | `tree.decorate` | `refused(invalid-argument)` for an undeclared slot; `refused(quota-exceeded)` |
| `clear-decoration` | `node`, `slot` | `tree.decorate` | `refused(invalid-argument)` for an undeclared slot |
| `set-virtual-rows` | `row-kind: string`, `rows: list<virtual-row>` (id, label, icon, command) | `tree.decorate` | `refused(invalid-argument)` for an undeclared row kind |
| `play-sound` | sound id: `string` | `sound.play` | `refused(not-found)` for an id outside the catalogue; `refused(rate-limited)` |
| `notify` | `title`, `body`, `urgency` (low, normal, high), `sound: option<string>` | `notify.desktop` | `refused(rate-limited)` |
| `open-view` | `view-target`, `focus: bool` | `ui.panels` | `refused(not-found)` for an undeclared view |
| `close-view` | `view-target` | `ui.panels` | none beyond the universal codes |
| `invoke-command` | `command: string`, `arguments-json: string` | `commands.invoke:<pattern>` | `ok` once the command has run; `refused(not-found)`; the command's own `failed` code; `failed(timed-out)` after 30 s |
| `message` | `to: message-target` (the plugin id, its side and an optional client id), `channel: string`, `payload: list<u8>` | `messages.peer:<plugin-id>` | `ok` when queued; `refused(not-found)` when the target is not installed; `refused(invalid-argument)` over 1 MiB |
| `editor-apply-edits` | `buffer`, `base-revision: u64`, `edits: list<text-edit>` | `editor.edit` | `refused(conflict)` on a stale revision; `refused(invalid-argument)` for overlapping edits |
| `editor-open` | `path: fs-path`, `line: option<u32>`, `placement` (new-pane, split, replace) | `editor.edit` and `fs.read` for the path | `ok-node(node-id)`; `failed(not-found)`; `failed(io)` |

`send-input` and `set-decoration` also accept the optional fields listed under "Further intents and extended intents" below.

Every intent can also return `refused(not-permitted)` when its grant is missing. Its submission gets that code, and the other submissions in the batch get `refused(cancelled)`.

### Which side applies an intent

The tree, the panes and the terminals live in the session server. A plugin half in a client therefore cannot apply most intents itself. The client forwards them to the server and the outcome returns to the client that submitted them.

| Intents | Applied by | Path from a client plugin |
|---|---|---|
| `rename-node`, `move-node`, `close-node`, `set-node-locked`, `set-bookmarked`, `set-expanded` | Server | Forwarded |
| `create-pane`, `signal-pane`, `freeze-pane`, `unfreeze-pane`, `replace-pane-command`, `terminate-pane-process`, `resize-pane` | Server | Forwarded |
| `create-group`, `create-folder`, `create-project`, `create-split`, `restructure`, `revert-restructure` | Server | Forwarded |
| `send-input`, `agent-action` | Server | Forwarded; a dedup key is required |
| `set-decoration`, `clear-decoration`, `set-virtual-rows`, `set-inferred-icon`, `mark-attention`, `ack-attention` | Server | Forwarded; the server replicates the result to every client |
| `notify`, `play-sound` | Server (desktop and sound queue) | Forwarded |
| `message` | Server routes it to the peer side | Forwarded |
| `invoke-command` | The side that registered the command | Forwarded when the command is a server command |
| `open-view`, `close-view`, `toast`, `dismiss-toast`, `set-status-segment`, `clear-status-segment`, `set-frame-slot`, `clear-frame-slot`, `open-external` | The client kernel of the submitting plugin half | Applied locally; nothing is forwarded |
| `editor-apply-edits`, `editor-open` | The client kernel that holds the buffer | Applied locally |

A server plugin submits all of these to the server directly. The chrome intents of a server plugin are applied by the server and replicated to every client as retained contributions. A client plugin's chrome intents change only that client.

For a forwarded intent:

- The outcome returns only to the client that submitted it, as `request-completed`.
- The tree change that the intent caused reaches that client before the outcome (see [Plugin traffic over IPC](/pages/reference-host-ipc/)).
- The added latency is 3 ms at the 99th percentile on a local connection (benchmark B15).
- If the client is detached, the submission waits in a queue of 256. The 257th queued submission completes as `refused(busy)` at once, and submissions still queued when the client exits complete as `cancelled`.

- **[HST-490]** Every intent MUST be applied by exactly one side as listed above, and an intent applied by the server MUST be forwarded when the submitter is a client plugin half.
- **[HST-491]** The outcome of a forwarded submission MUST be delivered only to the submitting client.
- **[HST-492]** The added latency of a forwarded submission MUST be at most 3 ms at the 99th percentile on an idle local connection (B15).

### Send-input check order

For `send-input`, two checks run at batch level before the list below, over the whole batch: shape (key present, total text across all stages at most 65,536 bytes, otherwise `invalid-argument`) and the grants: `terminal.write`, then `terminal.raw-input` if `raw` or `mouse` is set, then `terminal.input-lock` if `lock-user-input` is set (otherwise `not-permitted`). The page [send-input](/pages/api-intent-send-input/) has the full order. The kernel then checks each submission in this order. `agent-action` uses the same order, with its own grant. The first failing check decides the outcome.

1. Dedup key. A reserved key gives `duplicate(<original>)`.
2. The pane exists and is a terminal pane. Otherwise `not-found` or `unsupported`.
3. The pane is not waiting to start: `is-restored` is false, which means its process has started. Otherwise `pane-not-ready`.
4. The rate limit has room: 20 per second per pane and plugin. Otherwise `rate-limited`.
5. The epoch, if given, equals the pane's current prompt epoch. Otherwise `conflict`.
6. The `if-current` precondition, if given, matches the pane. Otherwise `conflict` with the message `precondition: <field>`.
7. No other plugin holds the input lock of the pane. Otherwise `conflict` with the message `input-locked`.
8. The input is accepted: the key is reserved, the rate token is taken and the text is written. A refused attempt never takes a rate token.

`agent-action` is refused with `conflict` when the current activity is not allowed. The message names that activity, for example `agent is working` or `agent is idle`. By default an action is allowed in the `idle`, `done`, `awaiting-approval` and `menu` states, unless its signature says otherwise.

### Limits

| Item | Value |
|---|---|
| Submissions per `on-events` return | 256; more traps |
| `send-input` text | 64 KiB |
| `message` payload | 1 MiB |
| `invoke-command` `arguments-json` | one JSON object, at most 64 KiB |
| `notify` rate | 6 per minute per plugin |
| `toast` rate | 3 per 10 s per plugin (`rate-limited`) |
| `set-status-segment`, `set-frame-slot` rate | 10 per second per slot |
| `restructure` operations | 256 |
| `send-input` stages and total stage delay | 16 and 10 s |
| `send-input` `raw` bytes | 64 KiB |
| Input lock | 2 s; one lock per pane |
| `terminate-pane-process` `grace-ms` | at most 10,000 |
| Decoration frames, frame time | 2 to 16, at least 90 ms |
| `toast` title, body, actions | 60, 200 characters, 2 |
| `mark-attention` reason | 80 characters |
| `play-sound` rate | 4 per second per plugin |
| `send-input` rate | 20 per second per pane and plugin |
| Dedup window | 10 minutes |

### Example: rename a pane when its agent finishes (Rust)

This server-side plugin renames the pane when its agent reaches `done`. The key stops a repeated `done` from renaming the pane again within 10 minutes. The plugin logs any refused outcome from the next batch. The manifest grants `tree.write` to the server side.

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::dedup;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::intents::RenameNodeSpec;
use ilium_plugin_sdk::types::tree::{AgentActivity, TitleSource};

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct FinishTitler;

impl Handler for FinishTitler {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::AgentActivityChanged(AgentActivityEvent { node, current, .. })
                    if current == AgentActivity::Done =>
                {
                    let spec = RenameNodeSpec {
                        node,
                        title: "Agent finished".to_string(),
                        source: TitleSource::Plugin,
                        expected_revision: None,
                    };
                    let key = dedup::key_for(&["finished", &node.to_string()]);
                    submissions.push(Submission::keyed(Intent::RenameNode(spec), key));
                }
                Event::RequestCompleted(result) => {
                    if let Outcome::Refused(error) = result.outcome {
                        log::log(Level::Warn, &format!("rename refused: {}", error.message));
                    }
                }
                _ => {}
            }
        }
        submissions
    }
}
```

### Example: open a panel and notify once (TypeScript)

This client-side plugin opens its status panel when the user runs its command. It sends a notice with a key, so a second run within 10 minutes opens the panel again but shows no second notice. The manifest grants `ui.panels` and `notify.desktop` on the client side.

```ts
import { definePlugin, type Event } from '@ilium/plugin-sdk';
import type { Submission } from '@ilium/plugin-sdk/types';
import { log } from '@ilium/plugin-sdk/host/log';

const STATUS_COMMAND = 'com.example.notes/open-status';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate(context) {
    log('info', `ready in ${context.plugin}`);
  },
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === STATUS_COMMAND) {
        submissions.push({
          intent: { tag: 'open-view', val: { view: { tag: 'panel', val: 'status' }, focus: true } },
          correlation: 1n,
          dedupKey: undefined,
        });
        submissions.push({
          intent: {
            tag: 'notify',
            val: { title: 'Status', body: 'Panel opened', urgency: 'low', sound: undefined },
          },
          correlation: 2n,
          dedupKey: 'status:opened',
        });
      }
      if (event.tag === 'request-completed' && event.val.outcome.tag === 'refused') {
        log('warn', 'an intent was refused; the outcome names the reason');
      }
    }
    return submissions;
  },
});
```

### Further intents and extended intents

The contract has 23 further intent cases and extends 5 existing ones. The batch rule, the dedup rule and the outcome rules above apply unchanged. `restructure` is atomic inside the batch. Four deep pages cover the groups: [send-input stages](/pages/api-intent-send-input/), [pane lifecycle](/pages/api-intent-pane-lifecycle/), [tree structure and restructure](/pages/api-intent-tree-structure/) and [toast, status segments and open-external](/pages/api-intent-chrome/).

The new outcome case is `ok-epoch(u64)`. `send-input` returns it on an agent pane, carrying the pane's new prompt epoch. A plugin that tests only for `ok` still works through the SDK: `is_ok()` in Rust and `isOk()` in TypeScript are true for both `ok` and `ok-epoch`.

**Rust**

```rust
match result.outcome {
    Outcome::OkEpoch(epoch) => self.prompt_epoch.insert(pane, epoch),
    outcome if outcome.is_ok() => None,
    Outcome::Refused(error) | Outcome::Failed(error) => {
        log::log(Level::Warn, &format!("{}: {}", error.code, error.message));
        None
    }
    _ => None,
};
```

**TypeScript**

```ts
const { outcome } = event.val;
if (outcome.tag === 'ok-epoch') {
  promptEpochs.set(pane, outcome.val);           // bigint
} else if (outcome.tag === 'refused' || outcome.tag === 'failed') {
  log('warn', `${outcome.val.code}: ${outcome.val.message}`);
}
```

#### Pane lifecycle (grant `panes.lifecycle`, high risk, server only)

| Intent | Payload | Outcome |
|---|---|---|
| `freeze-pane` | `pane` | `ok`; the process is stopped (SIGSTOP on Unix), output stops, the pane shows the frozen mark; `refused(unsupported)` on Windows |
| `unfreeze-pane` | `pane` | `ok`; `refused(conflict)` when the pane is not frozen |
| `replace-pane-command` | `pane`, `command: list<string>`, `cwd: option<fs-path>`, `keep-scrollback: bool` | `ok` once the new process runs; the durable resume binding stays with the kernel |
| `terminate-pane-process` | `pane`, `grace-ms: u32` (at most 10,000) | `ok` when the process has exited; the pane stays open with its exit status |
| `resize-pane` | `pane`, `columns: option<u16>`, `rows: option<u16>` | `ok`; `refused(conflict)` when the layout cannot give the size, and the message names the limiting neighbour |

#### Tree structure and attention (grants `tree.restructure`, `tree.write`, `tree.decorate`, `tree.attention`)

| Intent | Payload | Grant | Outcome |
|---|---|---|---|
| `create-group` | `parent`, `title`, `placement`, `children: list<node-id>` | `tree.restructure` | `ok-node` |
| `create-folder` | `parent`, `title`, `path: option<fs-path>` | `tree.restructure` | `ok-node` |
| `create-project` | `title`, `root: fs-path` | `tree.restructure` | `ok-node` |
| `create-split` | `panes: list<node-id>` (2 to 4), `direction` | `tree.restructure` | `ok-node` (the split container) |
| `set-bookmarked` | `node`, `bookmarked: bool` | `tree.write` | `ok` |
| `set-expanded` | `node` or `virtual-row: string`, `expanded: bool` | `tree.write` for nodes, own `virtual_rows` for rows | `ok` |
| `restructure` | `operations: list<restructure-op>` (move, create-group, ungroup, rename; at most 256), `label: string` | `tree.restructure` | `ok-node` carrying an undo token (0 when none); all or nothing: one failing operation refuses the whole intent with `refused(conflict)` and the operation index in the message |
| `revert-restructure` | `token: u64` | `tree.restructure` | `ok`; `refused(conflict)` when a later change touched the same nodes |
| `set-inferred-icon` | `node`, `icon: option<string>` (catalogue id or glyph), `source: string` | `tree.decorate` | `ok`; a manual icon wins (`refused(conflict)`) |
| `mark-attention` | `node`, `reason: string` (at most 80) | `tree.attention` | `ok`; the attention policy (done prefix, sound) stays with the kernel |
| `ack-attention` | `node` | `tree.attention` | `ok` |

#### Chrome and feedback (grants `open-external`, `ui.chrome`, `notify.toast`)

| Intent | Payload | Grant | Outcome |
|---|---|---|---|
| `open-external` | `target`: `url(string)`, `file(fs-path)` or `settings(string)` for `ilium://settings/...` | `open-external` | `ok` after the desktop handler started; `refused(not-permitted)` when the user declines; only `http`, `https` and `mailto` URLs |
| `set-status-segment` | `segment` (own `status_segments` id), `content: rich-text`, `tooltip: option<tooltip>`, `tone: option<tone>` | `ui.chrome` | `ok`; at most 10 per second per slot (the segment id is the slot) |
| `clear-status-segment` | `segment` | `ui.chrome` | `ok` |
| `set-frame-slot` | `pane`, `slot` (own `pane_frame_slots` id), `content: rich-text`, `icon: option<string>`, `tooltip` | `ui.chrome` | `ok`; `refused(invalid-argument)` when the pane does not match the slot's `applies_to` |
| `clear-frame-slot` | `pane`, `slot` | `ui.chrome` | `ok` |
| `toast` | `id: option<string>`, `tone`, `title` (60), `body: option<rich-text>` (200 characters), `actions: list<toast-action>` (2), `duration-ms: option<u32>` | `notify.toast` | `ok`; 3 per 10 s per plugin (`rate-limited`); a shown `id` is replaced |
| `dismiss-toast` | `id` | `notify.toast` | `ok` |

```wit
// Excerpt of ilium:types/intents in reference-wit
record toast-action { label: string, command: string }
```

#### Extended intents

The extended intents add optional fields. A submission that leaves them out behaves as it does without them.

| Intent | New fields | Rule |
|---|---|---|
| `send-input` | `stages`, `raw`, `mouse`, `if-current`, `lock-user-input`, `journal` | The check order is the one above. `refused(conflict)` names the failed precondition. See [send-input stages](/pages/api-intent-send-input/). |
| `set-decoration` | `spans: option<rich-text>`, `tooltip: option<tooltip>`, `frames: list<styled-text>` (2 to 16), `frame-ms: u16` (at least 90), `trailing: bool`, `visibility` (`always`, `on-hover`), `tone` | The host steps the frames with no plugin call. Reduced motion shows the first frame. See [toast, status segments and open-external](/pages/api-intent-chrome/). |
| `rename-node` | `source` (`plugin`, `inferred`), `expected-revision: option<u64>` | A stale revision is `refused(conflict)`. An `inferred` title never replaces a manual one. |
| `notify` | `coalesce-key: option<string>`, `coalesce-window-ms: u32` (at most 60,000) | Notifications with the same key inside the window replace each other. |
| `set-virtual-rows` | rows gain `parent: option<string>`, `expandable`, `accepts-drop`, `tooltip`, `label: rich-text`, `trailing` | Kinds declared with `children: "provider"` ask the provider export instead (see [plugin exports](/pages/api-exports/)). |

#### Error codes of the further intents

No error code was added: the intents reuse the 15 codes of the error model. Beyond the universal codes (`not-permitted`, `invalid-argument`, `cancelled`), these arise:

| Code | Intents and cause |
|---|---|
| `unsupported` | `freeze-pane` on Windows; `send-input` `mouse` while mouse reporting is off |
| `conflict` | `unfreeze-pane` on a pane that is not frozen; `resize-pane` the layout cannot satisfy; `restructure` with a failing operation (index in the message); `revert-restructure` after a later change; `set-inferred-icon` and `rename-node` against a manual value or a stale revision; `send-input` with a failed `if-current` or a held input lock |
| `invalid-argument` | more than 16 stages, more than 10 s of delay, more than 64 KiB of `raw`, a lock over 2 s, more than 256 operations, frames outside 2 to 16 or under 90 ms, a slot whose `applies_to` does not match |
| `rate-limited` | `toast`, `set-status-segment`, `set-frame-slot`, with `retry-after-ms` |
| `not-permitted` | a missing grant, or `open-external` declined by the user |
| `pane-not-ready` | `send-input` and the lifecycle intents on a restored pane that has not started |
| `not-found` | a node, pane, slot or segment that does not exist or is not the plugin's own |

#### Sequence: a staged send with a precondition

<figure>
<svg viewBox="0 0 420 250" role="img" aria-label="Sequence of a send-input submission with stages and a precondition">
  <defs>
    <marker id="intents-seq-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M 0 0 L 10 5 L 0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <text x="70" y="18" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
  <text x="210" y="18" font-size="12" text-anchor="middle" fill="currentColor">Kernel</text>
  <text x="350" y="18" font-size="12" text-anchor="middle" fill="currentColor">Terminal pane</text>
  <line x1="70" y1="26" x2="70" y2="240" stroke="currentColor" stroke-dasharray="3 3"/>
  <line x1="210" y1="26" x2="210" y2="240" stroke="currentColor" stroke-dasharray="3 3"/>
  <line x1="350" y1="26" x2="350" y2="240" stroke="currentColor" stroke-dasharray="3 3"/>
  <line x1="70" y1="46" x2="208" y2="46" stroke="currentColor" marker-end="url(#intents-seq-arrow)"/>
  <text x="140" y="40" font-size="10" text-anchor="middle" fill="currentColor">send-input + key</text>
  <text x="214" y="68" font-size="10" fill="currentColor">grant, key, if-current, lock</text>
  <line x1="210" y1="86" x2="348" y2="86" stroke="currentColor" marker-end="url(#intents-seq-arrow)"/>
  <text x="280" y="80" font-size="10" text-anchor="middle" fill="currentColor">stage 1: /compact</text>
  <text x="214" y="108" font-size="10" fill="currentColor">wait delay-ms</text>
  <line x1="210" y1="126" x2="348" y2="126" stroke="currentColor" marker-end="url(#intents-seq-arrow)"/>
  <text x="280" y="120" font-size="10" text-anchor="middle" fill="currentColor">stage 2: submit</text>
  <line x1="208" y1="170" x2="72" y2="170" stroke="currentColor" marker-end="url(#intents-seq-arrow)"/>
  <text x="140" y="164" font-size="10" text-anchor="middle" fill="currentColor">request-completed</text>
  <text x="140" y="188" font-size="10" text-anchor="middle" fill="currentColor">ok or ok-epoch(7)</text>
  <text x="140" y="222" font-size="10" text-anchor="middle" fill="currentColor">or refused(conflict)</text>
</svg>
<figcaption>The kernel checks the whole submission before the first byte is written. The outcome arrives once, after the last stage was sent.</figcaption>
</figure>

#### Example: freeze a pane, then show a toast (Rust and TypeScript)

This server-side plugin freezes a runaway terminal and tells the user how to resume it. It holds `panes.lifecycle` on the server side and `notify.toast` where the toast is shown. The freeze carries a key, so a repeated command does not freeze twice. The toast `id` makes a second toast replace the first.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{FreezePaneSpec, ToastSpec};
use ilium_plugin_sdk::types::text::Tone;

fn freeze_and_tell(pane: NodeId) -> Vec<Submission> {
    vec![
        Submission::keyed(Intent::FreezePane(FreezePaneSpec { pane }), format!("freeze-{pane}")),
        Submission::new(Intent::Toast(ToastSpec {
            id: Some("frozen".into()),
            tone: Tone::Warning,
            title: "Pane frozen".into(),
            body: None,
            actions: vec![],
            duration_ms: Some(5_000),
        })),
    ]
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk/types';

function freezeAndTell(pane: bigint): Submission[] {
  return [
    { intent: { tag: 'freeze-pane', val: { pane } }, correlation: 1n, dedupKey: `freeze-${pane}` },
    {
      intent: {
        tag: 'toast',
        val: { id: 'frozen', tone: 'warning', title: 'Pane frozen', body: undefined, actions: [], durationMs: 5000 },
      },
      correlation: 2n,
      dedupKey: undefined,
    },
  ];
}
```

The log of the test harness for this batch, when the pane is already frozen:

<pre class="term"><span class="t-prompt">$</span> ilium plugin test ./freezer --fixture freeze-twice.json
<span class="t-ok">{"type":"outcome","i":0,"intent":"freeze-pane","outcome":"ok"}</span>
<span class="t-ok">{"type":"outcome","i":1,"intent":"toast","outcome":"ok"}</span>
{"type":"outcome","i":2,"intent":"freeze-pane","outcome":"duplicate(submission(0))"}
<span class="t-ok">{"type":"outcome","i":3,"intent":"toast","outcome":"ok"}</span>
<span class="t-dim">{"type":"summary","ok":3,"duplicate":1,"refused":0}</span>
</pre>

Illustrative, not generated: the JSONL form follows the harness output of the example earlier on this page; the fixture delivers the command twice.

### Edge cases and errors

- `refused(pane-not-ready)` means a restored pane has not started its process yet. Retry after `pane-created` or `tree-changed` for that pane.
- `refused(rate-limited)` always carries `retry-after-ms`. Wait that long before retrying.
- `refused(conflict)` means the state changed. Read the current state, then submit again. A refused submission keeps its key free, so the same key is valid for the retry.
- `failed(timed-out)` on `invoke-command` means the command did not reply within 30 seconds. The command may still finish later, so check its effect before a retry.
- `refused(cancelled)` means another submission in the same batch was refused. Fix the refused one, then resubmit the whole group.
- `duplicate(<original>)` is not an error. It confirms that an earlier submission with the same key already took effect.
- A trap ends the plugin instance. The kernel restarts it under the restart rule in [errors and limits](/pages/reference-errors-and-limits/).

### What the user sees

- A rename, a decoration or a closed node changes the tree at once. A badge is at most 12 cells wide. Longer text is cut and ends with `…`.
- `send-input` types into the pane as if the user typed the text. `submitted` also presses the submit keys of the pane's signature, which default to a carriage return. `pasted` uses bracketed paste when the program enables it.
- `notify` shows a desktop notification. When an attached client reports that the pane the notice is about has focus, the notice appears in the client footer instead.
- A refused outcome is silent to the user. Only the plugin sees it, in its log or in its own state. Use the test harness in [testing](/pages/testing/) to see outcomes before you ship.

<pre class="term">$ ilium plugin test ./titler --fixture done.json
<span class="t-hi">{"type":"submission","i":0,"intent":"rename-node"}</span>
<span class="t-ok">{"type":"outcome","i":0,"outcome":"ok"}</span>
<span class="t-hi">{"type":"submission","i":1,"intent":"rename-node"}</span>
{"type":"outcome","i":1,"outcome":"duplicate(submission(0))"}
<span class="t-dim">{"type":"summary","ok":1,"duplicate":1,"refused":0}</span>
</pre>

Illustrative, not generated: the JSONL that `ilium plugin test` prints for the Rust example when its fixture delivers the same `done` event twice.

### Units and conventions

Times are `epoch-ms` (`u64`, milliseconds since the Unix epoch, UTC) and durations are milliseconds. Correlation numbers below 2^63 belong to the plugin and the rest to the SDK. A limit in characters counts Unicode scalar values. The rules are stated once in the [conventions of the WIT reference](/pages/reference-wit/#2-conventions).

### See also

- [Events](/pages/api-events/): the events that start a batch, and `request-completed`.
- [Requests and outcomes](/pages/requests-and-outcomes/): how the outcome and the take functions relate.
- [Errors and limits](/pages/reference-errors-and-limits/): the error codes and the per-call caps.
- [Tree](/pages/api-tree/), [panes](/pages/api-panes/), [commands](/pages/api-commands/), [notifications](/pages/api-notifications/) and [messages and topics](/pages/api-messages-and-topics/): the reads that go with these intents.
- Capabilities: [tree.write](/pages/capability-tree-write/), [terminal.write](/pages/capability-terminal-write/), [agent.control](/pages/capability-agent-control/), [panes.signal](/pages/capability-panes-signal/), [notify.desktop](/pages/capability-notify-desktop/) and [editor.edit](/pages/capability-editor-edit/).
- Deep pages: [send-input stages](/pages/api-intent-send-input/), [pane lifecycle](/pages/api-intent-pane-lifecycle/), [tree structure](/pages/api-intent-tree-structure/) and [chrome intents](/pages/api-intent-chrome/).
- [Testing](/pages/testing/): fixtures and the `ilium plugin test` command.
