---
layout: page
title: "panes.signal"
permalink: /pages/capability-panes-signal/
---

## Overview

The `panes.signal` grant lets a server-side plugin send a POSIX signal to the process that runs inside a terminal pane. The plugin does this by submitting the `signal-pane` intent with a pane id and one of four signal names: `interrupt`, `terminate`, `kill` or `hangup`. The host checks the grant, sends the signal and completes the submission with `ok`.

The grant is high risk (H). Stopping a process can lose the user's work, so the user must check it in the review dialog, even for a bundled T1 plugin. It covers stopping only. It does not let a plugin read a pane's output or type into it; those need [terminal.read-screen](/pages/capability-terminal-read-screen/) and [terminal.write](/pages/capability-terminal-write/).

<figure>
<svg viewBox="0 0 420 264" role="img" aria-label="A plugin submits signal-pane, the grant is checked, then the pane process is signalled or the submission is refused">
  <defs><marker id="arr-panes-signal" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="10" width="220" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="130" y="33" text-anchor="middle" font-size="12" fill="currentColor">Plugin submits signal-pane</text>
  <line x1="130" y1="46" x2="130" y2="70" stroke="currentColor" marker-end="url(#arr-panes-signal)"/>
  <text x="138" y="62" font-size="10" fill="currentColor">signal-pane</text>
  <rect x="20" y="70" width="220" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="130" y="93" text-anchor="middle" font-size="12" fill="currentColor">Grant check: panes.signal</text>
  <line x1="130" y1="106" x2="130" y2="130" stroke="currentColor" marker-end="url(#arr-panes-signal)"/>
  <text x="138" y="122" font-size="10" fill="currentColor">granted</text>
  <rect x="20" y="130" width="220" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="130" y="153" text-anchor="middle" font-size="12" fill="currentColor">Signal sent to pane process</text>
  <line x1="130" y1="166" x2="130" y2="190" stroke="currentColor" marker-end="url(#arr-panes-signal)"/>
  <rect x="20" y="190" width="220" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="130" y="213" text-anchor="middle" font-size="12" fill="currentColor">Outcome: ok</text>
  <line x1="240" y1="88" x2="290" y2="88" stroke="currentColor" marker-end="url(#arr-panes-signal)"/>
  <text x="244" y="82" font-size="10" fill="currentColor">denied</text>
  <rect x="290" y="64" width="118" height="48" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="349" y="85" text-anchor="middle" font-size="11" fill="currentColor">refused</text>
  <text x="349" y="102" text-anchor="middle" font-size="10" fill="currentColor">not-permitted</text>
  <text x="20" y="252" font-size="11" fill="currentColor">When the grant is refused, the pane process keeps running.</text>
</svg>
<figcaption>The host checks the panes.signal grant before it signals the pane process. A refused submission leaves the process untouched.</figcaption>
</figure>

<div class="warn">

A signal can end a running agent or a build without warning, and `kill` cannot be caught, so the process gets no chance to save its work. Grant `panes.signal` only to a plugin whose purpose is stopping processes, and read the plugin's source before you grant it.

</div>

## You will need

- **The grant** `panes.signal` in `capabilities.server` of the manifest. The grant takes no argument.
- **Server side only.** Submit the intent from the server handler. Do not declare the grant under `capabilities.client`.
- **A pane id.** The intent needs a `NodeId` (Rust) or `bigint` (TypeScript). Get it from the `pane-focused` or `pane-exited` events, which need [panes.read](/pages/capability-panes-read/). Without `panes.read`, the plugin receives no pane events, so it can only signal panes whose ids it already holds.
- **The SDK path.** Rust `ilium_plugin_sdk::types::tree::Signal` and `ilium_plugin_sdk::types::intents::SignalPaneSpec` with `Intent::SignalPane`. TypeScript `@ilium/plugin-sdk/types` for the `Signal` type, and the `signal-pane` tag in an intent literal.

## Grant syntax

| Manifest entry | Result |
|---|---|
| `"panes.signal"` | Valid. |
| `"panes.signal:kill"` | Refused at install with `manifest.grant-argument`: `capability "panes.signal" takes no argument (got "panes.signal:kill")`. |

The signal is a field of the intent, chosen on every call. The grant itself never names a signal.

## The intent: signal-pane

The intent is declared in `ilium:types/intents`, which imports the `signal` enum from `ilium:types/tree`:

```wit
record signal-pane-spec { pane: node-id, signal: signal }
// enum signal { interrupt, terminate, kill, hangup } is declared in ilium:types/tree
// in variant intent: signal-pane(signal-pane-spec)
```

| Name | Rust | TypeScript | Meaning |
|---|---|---|---|
| Intent | `Intent::SignalPane(SignalPaneSpec { pane, signal })` | `{ tag: 'signal-pane', val: { pane, signal } }` | Asks the host to send the signal. |
| `pane` | `NodeId` | `bigint` | The pane whose process receives the signal. |
| `signal` | `Signal` | `'interrupt' \| 'terminate' \| 'kill' \| 'hangup'` | The signal to send. |
| Outcome | completes with `ok` or a refusal | same | See the outcome table below. |

The signal values map to POSIX signals as follows:

| Value | POSIX signal | Can the process catch it | Typical use |
|---|---|---|---|
| `interrupt` | `SIGINT` | yes | Ask a program to stop its current task, as Ctrl+C does. |
| `terminate` | `SIGTERM` | yes | Ask a program to exit. |
| `kill` | `SIGKILL` | no | Stop a process that ignored `terminate`. |
| `hangup` | `SIGHUP` | yes | Tell a process that its terminal went away. |

Escalate in order: send `interrupt`, then `terminate`, and send `kill` only when no `pane-exited` event has arrived 2 seconds after `terminate`. The signal goes to the child process that the pane started. An `ok` outcome means the signal was sent, not that the process exited. To confirm the exit, wait for the `pane-exited` event. It carries `status` and `signal`, for example `SIGKILL`, and `status` is `None` when a signal ended the process.

Each submission completes exactly once. The completion arrives as a `request-completed` event whose request reference is `submission(<correlation>)`. The `signal-pane` intent does not need a dedup key, because only `send-input` and `agent-action` require one. Without a key, every submission sends its signal. With a key, a repeat of that key within 10 minutes completes as `duplicate` and sends nothing.

## Outcomes and errors

| Situation | Outcome | Retry |
|---|---|---|
| The signal was sent | `ok` | The process may still run. Watch `pane-exited`. |
| The grant is missing or the user declined it | `refused(not-permitted)` for the signal. The whole batch is refused: every other submission in it completes as `refused(cancelled)` with `batch refused: submission <index>`. | No. Ask the user to grant it. |
| The pane id names no node, or a node that is not a pane | `refused(not-found)` | No. |
| The process has exited, or the pane has no process (for example a basic pane) | `refused(not-found)` with the message `no running process` | No. |
| A restored pane whose process has not started | `refused(pane-not-ready)` | Yes, after `pane-created` or `tree-changed` for that pane. |

Edge cases to handle in code:

- **Refused batches.** When the grant is missing, the whole batch is refused, including other submissions such as `send-input`. Submit signals in their own batch when the other submissions must still run.
- **Pane closed before the host handles the signal.** The outcome is `refused(not-found)`. Treat the pane as already stopped.
- **Ignored signals.** A program may ignore `interrupt` or `terminate`. Check `pane-exited` first, then escalate only after a grace of 2 seconds (the same grace as [long-lived processes](/pages/capability-process-long-lived/)) has passed without it.
- **Revocation.** The user can revoke the grant in Settings > Plugins > the plugin > Capabilities, or with `ilium plugin grants <id> --revoke panes.signal`. The plugin then receives `deactivate(revoked)`. Pending requests that need the revoked capability complete as `failed(revoked)`.
- **Grant added later.** When the user grants `panes.signal` after install, the plugin receives `deactivate(granted)` and is re-activated with the new set.

## Risk and tier rules

- **H grant.** The user must check it. It is never granted by default.
- **T2 and T3 packages.** The install review dialog lists it as a HIGH row, unchecked. The plugin installs with the rows the user leaves checked.
- **T1 bundled plugins.** The plugin activates first without the grant. On first activation the user sees one prompt, `Plugin <name> asks for: <list>`. The prompt has a checkbox per H capability and the buttons `Allow selected` and `Not now`. `Not now` asks again at the next server start, at most three times, and then never again. Settings > Plugins can grant it at any time.
- **No prompt per call.** A granted `panes.signal` stays in force until it is revoked.

## What the user sees

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.panes 0.1.0 by Example Ltd                                     │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ <span class="t-warn">! [ ] Interrupts or stops programs in panes</span> <span class="t-dim">……………………………………………</span> <span class="t-warn">panes.signal</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests panes.signal, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

<figure>
<pre class="term"><span class="t-hi">╭─ Plugin demo-signal asks for ────────────────────────────────────────────╮</span>
│ [ ] HIGH  panes.signal   Interrupt or stop programs in panes             │
│                                                                          │
│ Not now asks again at the next server start, up to three times.          │
│                 [ Allow selected ]   [ Not now ]                         │
╰──────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>Illustrative, not generated. The first-activation prompt for a T1 bundled plugin. The grant is off until the user checks it and chooses Allow selected.</figcaption>
</figure>

## Worked examples

### Example 1: interrupt on a key, kill on a second key in Rust

**Grants:** `panes.read` and `panes.signal`, both in `capabilities.server`.

The plugin tracks the focused pane. The `runner/interrupt` command sends `interrupt`, and the `runner/kill` command sends `kill`. Declare both commands in the manifest's `contributes.commands`, as the [commands](/pages/api-commands/) page describes.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::tree::Signal;
use ilium_plugin_sdk::types::intents::SignalPaneSpec;

const INTERRUPT_COMMAND: &str = "com.example.runner/interrupt";
const KILL_COMMAND: &str = "com.example.runner/kill";

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct PaneSignaller {
    focused_pane: Option<NodeId>,
}

impl Lifecycle for PaneSignaller {}

impl Handler for PaneSignaller {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::PaneFocused(pane) => self.focused_pane = pane,
                Event::CommandInvoked(invoked) => {
                    let signal = match invoked.command.as_str() {
                        INTERRUPT_COMMAND => Signal::Interrupt,
                        KILL_COMMAND => Signal::Kill,
                        _ => continue,
                    };
                    let Some(pane) = self.focused_pane else { continue };
                    let spec = SignalPaneSpec { pane, signal };
                    submissions.push(Submission::new(Intent::SignalPane(spec)));
                }
                _ => {}
            }
        }
        submissions
    }
}
```

`Submission::new` assigns the next correlation number, so the plugin needs no counter. The `ok` outcome arrives later as a `request-completed` event, and the plugin ignores it here.

### Example 2: the same escalation in TypeScript

**Grants:** `panes.read` and `panes.signal`, both in `capabilities.server`.

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import type { NodeId, Signal } from '@ilium/plugin-sdk/types';

const SIGNAL_BY_COMMAND: Record<string, Signal> = {
  'com.example.runner/interrupt': 'interrupt',
  'com.example.runner/kill': 'kill',
};
let focusedPane: NodeId | undefined;
let nextCorrelation = 1n;

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'pane-focused') {
        focusedPane = event.val;
        continue;
      }
      if (event.tag !== 'command-invoked') continue;
      const signal = SIGNAL_BY_COMMAND[event.val.command];
      if (signal === undefined || focusedPane === undefined) continue;
      submissions.push({
        intent: { tag: 'signal-pane', val: { pane: focusedPane, signal } },
        correlation: nextCorrelation,
        dedupKey: undefined,
      });
      nextCorrelation += 1n;
    }
    return submissions;
  },
});
```

Both examples use the generated names of the SDK binding rules: `Signal` and `SignalPaneSpec` in Rust, and the kebab-case tag `signal-pane` with string values such as `'interrupt'` in TypeScript. The Rust `Lifecycle` implementation uses the default `activate` and `deactivate`. Keep `correlation` values unique within the plugin, as the TypeScript example does.

## First-party features that request it

No first-party feature in v1 requests `panes.signal`. The progress monitor runs its probe commands under `process.run:sh`, which is a different grant. A plugin that stops work must explain to the user, in its description, why it needs `panes.signal`.

## See also

- [panes.create](/pages/capability-panes-create/) for panes that a plugin starts itself.
- [panes.read](/pages/capability-panes-read/) for the pane events that supply pane ids and exit status.
- [Process control](/pages/api-process-control/) for the `process` import and the same `signal-pane` intent in context.
- [Intents](/pages/api-intents/) for submissions, correlation numbers and the outcome shapes.
- [Errors and limits](/pages/reference-errors-and-limits/) for the full error code table.
- [Security](/pages/security/) for the risk tiers behind high-risk grants.

Checked against the API version `ilium:plugin@0.1.0`.

### Security

Signals to panes need the high-risk grant; see [Threat model](/pages/reference-threat-model/).
