# terminal.write

The `terminal.write` grant lets a plugin type text into a terminal pane, as if the user had typed it. It is the only way a plugin sends input to a running agent or shell. A plugin uses it to run a scheduled command in a shell, to answer an agent's approval prompt, to transfer a screen between panes or to pass a message to an agent. Each send is a `send-input` intent. The plugin names a pane and a text, and the kernel checks the request in a fixed order before it writes the text to the pane.

The grant is high risk (H): whatever a plugin sends runs with the rights of the process in that pane, so the user must grant it explicitly, even for a T1 bundled plugin.

<div class="warn">

Input sent to a pane runs there with the permissions of that process. A plugin with this grant can start commands in a shell, approve an agent's prompt or submit text to an agent. Grant it only to plugins whose purpose needs it, and check the plugin's source before you grant it.

</div>

## You will need

- The grant `terminal.write`, on each side (server or client) whose code sends input. Each side needs its own grant.
- The SDK for your language: the Rust crate `ilium-plugin-sdk` or the npm package `@ilium/plugin-sdk`, at the versions pinned by the SDK release.
- [panes.read](/pages/capability-panes-read/) to find the pane and check that it is restored and ready.
- [agent.read-prompts](/pages/capability-agent-read-prompts/) when the plugin needs the `input-submitted` event, which carries the prompt epoch.
- [terminal.read-screen](/pages/capability-terminal-read-screen/) when the plugin reads the prompt text before it decides what to send.

## Grant syntax

- Valid: `terminal.write`
- Invalid: `terminal.write:agents`. The grant takes no argument, and the manifest check refuses the scoped form with `manifest.grant-argument`. Choosing the pane is part of the intent.

## The surface

| Name | Kind | Type | Meaning |
|---|---|---|---|
| `send-input` | intent | record `send-input-spec { pane, text, delivery, prompt-epoch }` | Writes `text` to the pane. Rust: `SendInputSpec` in `ilium_plugin_sdk::types::intents`. TypeScript: `{ tag: 'send-input', val: {...} }`. |
| `pane` | field | `node-id` | The terminal pane that receives the text. |
| `text` | field | `string`, at most 64 KiB | The text to type. |
| `delivery` | field | enum `typed`, `submitted`, `pasted` | How the text is written (see Delivery modes). |
| `prompt-epoch` | field | `option<u64>` | Optional. The prompt epoch the plugin read before it decided to send. |
| `dedup-key` | submission field | `string`, 1 to 128 characters | Mandatory for `send-input`. See Dedup keys. |
| `terminal.prompt-epoch` | sync function | `func(pane: node-id) -> option<u64>` | Returns the pane's current prompt epoch. |
| `epoch` | field of `input-submitted-event` | `u64` | The epoch of the prompt recorded by that event. Needs `agent.read-prompts`. |
| `submit_keys` | signature field | string, default `"\r"` | The keys that `submitted` sends for panes of that signature. |
| `initial-input` | field of `create-pane-spec` | `string`, at most 64 KiB | Text typed into a new pane. It needs this grant as well. |

Rust names are snake_case: `Delivery::Submitted`, `prompt_epoch`, `ilium_plugin_sdk::primitives::terminal::prompt_epoch`. TypeScript names are camelCase: `promptEpoch`, `promptEpoch(pane)` from `@ilium/plugin-sdk/primitives/terminal`. The `delivery` strings in TypeScript are `'typed'`, `'submitted'` and `'pasted'`.

## Submitting text

The kernel checks every `send-input` submission in this order. The first failing check decides the outcome, and a submission that fails any check does not reach the pane.

<figure>
<svg viewBox="0 0 420 330" role="img" aria-label="Checks that send-input passes before the text reaches the pane">
<title>How send-input is checked</title>
<desc>Eight checks run in order. A failed check refuses the submission with a named outcome. Only a submission that passes all checks is written to the pane.</desc>
<defs>
<marker id="arr-terminal-write" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
<path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="10" y="10" width="200" height="26" rx="6" fill="none" stroke="currentColor"/>
<text x="110" y="27" text-anchor="middle" font-size="11" fill="currentColor">1. Well formed, key present</text>
<rect x="10" y="46" width="200" height="26" rx="6" fill="none" stroke="currentColor"/>
<text x="110" y="63" text-anchor="middle" font-size="11" fill="currentColor">2. Grant terminal.write held</text>
<rect x="10" y="82" width="200" height="26" rx="6" fill="none" stroke="currentColor"/>
<text x="110" y="99" text-anchor="middle" font-size="11" fill="currentColor">3. Dedup key is free</text>
<rect x="10" y="118" width="200" height="26" rx="6" fill="none" stroke="currentColor"/>
<text x="110" y="135" text-anchor="middle" font-size="11" fill="currentColor">4. Pane exists, is a terminal</text>
<rect x="10" y="154" width="200" height="26" rx="6" fill="none" stroke="currentColor"/>
<text x="110" y="171" text-anchor="middle" font-size="11" fill="currentColor">5. Pane restored and running</text>
<rect x="10" y="190" width="200" height="26" rx="6" fill="none" stroke="currentColor"/>
<text x="110" y="207" text-anchor="middle" font-size="11" fill="currentColor">6. Rate within 20 per second</text>
<rect x="10" y="226" width="200" height="26" rx="6" fill="none" stroke="currentColor"/>
<text x="110" y="243" text-anchor="middle" font-size="11" fill="currentColor">7. Prompt epoch is current</text>
<rect x="10" y="262" width="200" height="26" rx="6" fill="none" stroke="currentColor"/>
<text x="110" y="279" text-anchor="middle" font-size="11" fill="currentColor">8. Key reserved, text written</text>
<rect x="10" y="298" width="200" height="26" rx="13" fill="none" stroke="currentColor"/>
<text x="110" y="315" text-anchor="middle" font-size="11" fill="currentColor">ok</text>
<line x1="110" y1="36" x2="110" y2="46" stroke="currentColor" marker-end="url(#arr-terminal-write)"/>
<line x1="110" y1="72" x2="110" y2="82" stroke="currentColor" marker-end="url(#arr-terminal-write)"/>
<line x1="110" y1="108" x2="110" y2="118" stroke="currentColor" marker-end="url(#arr-terminal-write)"/>
<line x1="110" y1="144" x2="110" y2="154" stroke="currentColor" marker-end="url(#arr-terminal-write)"/>
<line x1="110" y1="180" x2="110" y2="190" stroke="currentColor" marker-end="url(#arr-terminal-write)"/>
<line x1="110" y1="216" x2="110" y2="226" stroke="currentColor" marker-end="url(#arr-terminal-write)"/>
<line x1="110" y1="252" x2="110" y2="262" stroke="currentColor" marker-end="url(#arr-terminal-write)"/>
<line x1="110" y1="288" x2="110" y2="298" stroke="currentColor" marker-end="url(#arr-terminal-write)"/>
<line x1="212" y1="23" x2="222" y2="23" stroke="currentColor" stroke-dasharray="3 3" marker-end="url(#arr-terminal-write)"/>
<line x1="212" y1="59" x2="222" y2="59" stroke="currentColor" stroke-dasharray="3 3" marker-end="url(#arr-terminal-write)"/>
<line x1="212" y1="95" x2="222" y2="95" stroke="currentColor" stroke-dasharray="3 3" marker-end="url(#arr-terminal-write)"/>
<line x1="212" y1="131" x2="222" y2="131" stroke="currentColor" stroke-dasharray="3 3" marker-end="url(#arr-terminal-write)"/>
<line x1="212" y1="167" x2="222" y2="167" stroke="currentColor" stroke-dasharray="3 3" marker-end="url(#arr-terminal-write)"/>
<line x1="212" y1="203" x2="222" y2="203" stroke="currentColor" stroke-dasharray="3 3" marker-end="url(#arr-terminal-write)"/>
<line x1="212" y1="239" x2="222" y2="239" stroke="currentColor" stroke-dasharray="3 3" marker-end="url(#arr-terminal-write)"/>
<text x="226" y="27" font-size="11" fill="currentColor">refused: invalid-argument</text>
<text x="226" y="63" font-size="11" fill="currentColor">refused: not-permitted</text>
<text x="226" y="99" font-size="11" fill="currentColor">duplicate(original)</text>
<text x="226" y="135" font-size="11" fill="currentColor">refused: not-found</text>
<text x="226" y="171" font-size="11" fill="currentColor">refused: pane-not-ready</text>
<text x="226" y="207" font-size="11" fill="currentColor">refused: rate-limited</text>
<text x="226" y="243" font-size="11" fill="currentColor">refused: conflict</text>
</svg>
<figcaption>The eight checks of a send-input submission. A failed check stops the submission and returns the outcome beside it. The key of a refused submission is not reserved, so a retry with the same key is valid after the cause is fixed.</figcaption>
</figure>

Two rules apply to the whole batch, not to one submission. A batch is refused as a whole when a submission is malformed (wrong shape, a missing `dedup-key`, or more than 256 submissions) or when the plugin lacks the grant. In that case every submission of the batch completes as `refused(cancelled)` with the message `batch refused: submission <index>`, except the offending one. All other checks run per submission.

## Dedup keys

A plugin can lose track of whether a send worked, for example after a restart. Sending the same text again would then type it twice. The `dedup-key` prevents that.

- The key is mandatory for `send-input` and `agent-action`. A submission without one is malformed.
- The key has 1 to 128 characters from `[A-Za-z0-9._:/-]`. Another character makes the submission malformed.
- The key is reserved for the pair of plugin id and key, for 10 minutes. From build phase 5, the kernel also journals `send-input` keys in the session snapshot before the write. A server restart then does not repeat input. Before that phase, keys live in memory only.
- A submission whose key is reserved completes as `duplicate(original)`, where `original` names the submission that reserved the key. It has no effect.
- A key is reserved only after every check passed. A refused submission keeps its key free, so a retry is valid. An accepted submission keeps its key reserved even if it later fails, so a retry of a failed send needs a new key.

Derive the key from the work, never from the clock or a random value. A new random key on every attempt defeats the purpose. The SDK helper `dedup::key_for` (Rust) joins its parts with `:` and replaces disallowed characters with `-`. TypeScript has no helper, so the plugin builds the string itself.

## Delivery modes

| Delivery | What the pane receives |
|---|---|
| `typed` | The text only. The program sees the characters as if typed. |
| `submitted` | The text, then the submit keys of the pane's signature. The default submit key is `"\r"`. A signature can set `submit_keys` to another sequence. |
| `pasted` | The text inside bracketed paste markers when the program enabled bracketed paste (DEC mode 2004). Otherwise the text is sent like `typed`. |

A pane without a signature always receives `"\r"` for `submitted`.

```json
{
  "contributes": {
    "signatures": [
      { "id": "example-repl", "process_names": ["example-repl"], "submit_keys": "\r\n" }
    ]
  }
}
```

## Prompt epochs

A prompt epoch identifies the prompt an agent pane was showing. It guards against answering a prompt the plugin did not read.

- The kernel keeps a `u64` counter for each terminal pane. It starts at 0 and increases each time the kernel records a submitted prompt for that pane. That is the same moment it emits `input-submitted`. The counter is saved in the snapshot.
- `terminal.prompt-epoch(pane)` returns the current value, or `none` when the pane has no epoch.
- A `send-input` with `prompt-epoch: some(n)` is refused with `conflict` when `n` is not the current epoch. The check runs after the rate check and before the write. The plugin reads the screen again and decides again.
- Without the epoch, the answer reaches whatever prompt is on the screen at write time.

## Risk and tier rules

- This grant is high risk (H). A T1 bundled plugin does not receive it automatically. The user must grant it explicitly.
- A T2 or T3 plugin needs the same explicit grant, and the review dialog shows the line with the H mark.
- Grants are per side. The server and client sides each need their own explicit grant.

## What the review dialog says

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.terminal 0.1.0 by Example Ltd                                  │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ <span class="t-warn">! [ ] Types into terminals</span> <span class="t-dim">……………………………………………………………………………………</span> <span class="t-warn">terminal.write</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests terminal.write, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

## When the grant is declined

A declined grant refuses the batch with `refused(not-permitted)`, and no text is written. A plugin that schedules or queues input must keep its queue, mark the item as not sent and tell the user that sending is unavailable. It must not drop the item.

## Worked example in Rust

This server-side function builds the submission for one due run of a scheduled command. The key comes from the schedule id and the due time, so a restart that replays the same due run is answered with `duplicate(original)`.

**Rust**

```rust
use ilium_plugin_sdk::dedup;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{Delivery, SendInputSpec};

/// Builds the submission for one due run of a scheduled command.
/// The due time comes from the schedule's own state, never from the clock.
pub fn scheduled_run(
    pane: NodeId,
    schedule_id: &str,
    due_at: &str,
    command: &str,
) -> Submission {
    let key = dedup::key_for(&["nightly", schedule_id, due_at]);
    let spec = SendInputSpec {
        pane,
        text: command.to_string(),
        delivery: Delivery::Submitted,
        prompt_epoch: None,
    };
    Submission::keyed(Intent::SendInput(spec), key)
}
```

The plugin calls `scheduled_run` from its `on_events` when a schedule comes due, and it returns the result in the list of submissions. With `prompt_epoch: None`, the kernel skips the epoch check, which is correct for a shell that has no prompt to answer.

## Worked example in TypeScript

This server-side plugin answers an agent's approval prompt. It stores the epoch of the prompt when the agent enters `awaiting-approval`. When the user presses an approve command, it sends `y` with that stored epoch. If the prompt changed in between, the kernel refuses the send with `conflict`.

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';
import { promptEpoch } from '@ilium/plugin-sdk/primitives/terminal';
import type { Event, Submission } from '@ilium/plugin-sdk/types';

const APPROVE_COMMAND = 'com.example.approve/yes';
const APPROVAL_ACTIVITY = 'awaiting-approval';

const epochByPane = new Map<bigint, bigint>();
let nextCorrelation = 1n;

function rememberEpoch(pane: bigint, current: string): void {
  if (current !== APPROVAL_ACTIVITY) {
    epochByPane.delete(pane);
    return;
  }
  const epoch = promptEpoch(pane);
  if (epoch !== undefined) {
    epochByPane.set(pane, epoch);
  }
}

function approve(command: string, pane: bigint | undefined): Submission | undefined {
  if (command !== APPROVE_COMMAND || pane === undefined) {
    return undefined;
  }
  const epoch = epochByPane.get(pane);
  if (epoch === undefined) {
    log('info', 'No approval prompt was read for this pane. Nothing was sent.');
    return undefined;
  }
  const correlation = nextCorrelation;
  nextCorrelation += 1n;
  return {
    intent: {
      tag: 'send-input',
      val: { pane, text: 'y', delivery: 'submitted', promptEpoch: epoch },
    },
    correlation,
    dedupKey: `approve:${pane}:${epoch}`,
  };
}

export const { handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'agent-activity-changed') {
        rememberEpoch(event.val.node, event.val.current);
      } else if (event.tag === 'command-invoked') {
        const submission = approve(event.val.command, event.val.context.pane);
        if (submission !== undefined) {
          submissions.push(submission);
        }
      }
    }
    return submissions;
  },
});
```

The manifest needs `"server": ["agent.read", "terminal.write"]` for this plugin. The `dedupKey` uses the epoch, so a double click on the same prompt sends once. A refused `conflict` reserves nothing, so the plugin can read the prompt again and retry.

## What the user sees

The text appears at the pane's prompt as if the user had typed it. With `submitted`, the program then receives the submit keys, so a shell runs the command or an agent starts its turn. The grant is the only consent: the kernel does not ask the user before each send.

<figure>
<pre class="term"><span class="t-dim">Shell pane 42, written by the scheduled run</span>
<span class="t-prompt">$</span> cargo test --workspace
<span class="t-ok">test result: ok. 412 passed; 0 failed; 0 ignored</span>
<span class="t-prompt">$</span> <span class="t-hi">_</span>
</pre>
<figcaption>Illustrative, not generated. The shell pane after a submitted send of cargo test.</figcaption>
</figure>

## Errors and edge cases

| Outcome | Cause | Retry |
|---|---|---|
| `ok` | The text was written. | Not needed. |
| `duplicate(original)` | The key is reserved within the last 10 minutes. | No. The original was written. |
| `refused(invalid-argument)` | Text over 64 KiB, a missing or malformed key, or a malformed batch. | No. Fix the call. |
| `refused(not-permitted)` | The grant is missing or declined. | No. Ask the user to grant it. |
| `refused(not-found)` | The pane no longer exists. | No. |
| `refused(unsupported)` | The node is not a terminal pane. | No. |
| `refused(pane-not-ready)` | A restored pane has not started its process. | Yes, after `pane-created` or `tree-changed` for that pane. |
| `refused(rate-limited)` | More than 20 submissions per second to one pane. | Yes, after `retry-after-ms`. |
| `refused(conflict)` | The prompt epoch is stale. | Yes, after reading the prompt again. |
| `refused(cancelled)` | Another submission of the same batch was refused. | Maybe. |

Check `is-restored` again before each write, because a pane can be restored after the plugin read it. Keep the queued text until you see `ok`, `duplicate(original)` or a refusal that will not clear.

Limits:

| Limit | Value |
|---|---|
| Text per submission | 64 KiB |
| Dedup key | 1 to 128 characters |
| Dedup window | 10 minutes |
| Dedup journal | 24 hours or 4,096 entries per session, whichever is smaller |
| Rate | 20 per second per pane per plugin |
| Submissions per batch | 256 |

## First-party features that request it

- Feature 17, screen transfer between panes.
- Feature 23, create an agent from an editor line, through `initial-input`.
- Feature 36, agent messaging.
- Feature 51, voice confirmations.
- Feature 55, scheduled input.
- Feature 56, text triggers, from Phase 5.
- Feature 57, prompt queue.
- Feature 62, chatroom, for the `@name` routing.
- Feature 80, remote compaction.
- Feature 110, `ilium broadcast`.

## See also

- [agent.control](/pages/capability-agent-control/) for sending an agent's own action keys, which is a separate grant with its own checks.
- [panes.create](/pages/capability-panes-create/) for the `initial-input` field of `create-pane`.
- [panes.signal](/pages/capability-panes-signal/) for signals sent to a pane's process, which need no text.
- [errors and limits](/pages/errors-and-limits/) for the full table of error codes.

Checked against the API version `ilium:plugin@0.1.0`.
