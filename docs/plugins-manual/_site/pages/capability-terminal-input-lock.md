# Capability: terminal.input-lock

## terminal.input-lock

The `terminal.input-lock` capability lets a plugin hold the user's own keystrokes for a very short time so that a multi-stage input from the plugin cannot interleave with typing. Held keys are queued, not lost, and are delivered when the lock ends. The lock is high risk (**H**) because a plugin that holds input can make a pane unresponsive.

The lock has a hard ceiling of 2,000 ms, and only one lock exists per pane at a time.

**You will need:**

- A `terminal.input-lock` grant (explicit, high risk). It usually goes together with [terminal.write](/pages/capability-terminal-write/), which sends the input.
- A staged `send-input` or another reason to hold the keys for less than 2 s.

<figure>
<svg viewBox="0 0 420 252" role="img" aria-label="A lock holds user keys, then releases them in order">
  <defs><marker id="arr" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor"/></marker></defs>
  <rect x="20" y="8" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="27" text-anchor="middle" fill="currentColor" font-size="12">Plugin calls lock-input(pane, millis) or sends lock-user-input</text>
  <text x="210" y="43" text-anchor="middle" fill="currentColor" font-size="10">at most 2,000 ms</text>
  <line x1="210" y1="52" x2="210" y2="72" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="72" width="380" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="210" y="91" text-anchor="middle" fill="currentColor" font-size="12">Host checks the grant and that no other lock holds the pane</text>
  <text x="210" y="107" text-anchor="middle" fill="currentColor" font-size="10">one lock per pane</text>
  <line x1="210" y1="116" x2="210" y2="136" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="136" width="380" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="155" text-anchor="middle" fill="currentColor" font-size="12">User keys are queued until unlock or timeout</text>
  <text x="210" y="171" text-anchor="middle" fill="currentColor" font-size="10">nothing is dropped</text>
  <line x1="210" y1="180" x2="210" y2="200" stroke="currentColor" marker-end="url(#arr)"/>
  <rect x="20" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="110" y="224" text-anchor="middle" fill="currentColor" font-size="11">ok, keys delivered at unlock</text>
  <rect x="220" y="200" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="310" y="224" text-anchor="middle" fill="currentColor" font-size="11">conflict or not-permitted</text>
</svg>
<figcaption>The lock ends on unlock-input, on the timeout, or when the plugin stops. Queued keys are then delivered in order.</figcaption>
</figure>

### Grant syntax

| Grant | Valid | Reason |
|---|---|---|
| `terminal.input-lock` | yes | the only form |
| `terminal.input-lock:5000` | no | a duration is not a grant argument; the ceiling is fixed at 2,000 ms |
| `terminal.lock` | no | unknown grant name |

High risk. The grant needs an explicit user decision even for a bundled plugin. The host also enforces the ceiling, so a plugin cannot extend a lock by calling `lock-input` again before the first lock ended: the second call is `conflict`.

### What it unlocks

| Name | Kind | Meaning |
|---|---|---|
| `terminal.lock-input` | function | `lock-input(pane, millis) -> result<_, plugin-error>`. `millis` at most 2,000. |
| `terminal.unlock-input` | function | `unlock-input(pane)`. Releases the lock and delivers the queue. |
| `send-input.lock-user-input` | field | `bool` on the intent `send-input`: hold the keys for the duration of the stages, at most 2 s. See [Intents](/pages/api-intents/). |

Staged input is described on [Terminal primitives](/pages/primitives-terminal/). The input rules of the intent are on [Intents](/pages/api-intents/).

### Manifest

The grant goes into `manifest.json` under `capabilities`, in the list of the side that uses it (server or client).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.stagedinput",
  "capabilities": {
    "server": [
      "panes.read",
      "terminal.write",
      "terminal.input-lock"
    ]
  }
}
```

### Worked example

This plugin sends a slash command in two stages and holds the user keys for the duration, so typing cannot land between the two stages.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::Event;
use ilium_plugin_sdk::types::intents::{Intent, Submission};
use ilium_plugin_sdk::types::intents::{InputStage, SendInputSpec};

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct StagedInput;

impl Handler for StagedInput {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::CommandInvoked(invoked) = event else { continue };
            if invoked.command != "com.example.stagedinput/compact" { continue; }
            let Some(pane) = invoked.context.pane else { continue };
            submissions.push(Submission::new(Intent::SendInput(SendInputSpec {
                pane,
                stages: vec![
                    InputStage::keys("/compact", 0),
                    InputStage::keys("\r", 120),
                ],
                lock_user_input: true,
                ..SendInputSpec::default()
            })));
        }
        submissions
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'command-invoked') continue;
      if (event.val.command !== 'com.example.stagedinput/compact' || event.val.context.pane === undefined) continue;
      submissions.push({
        intent: {
          tag: 'send-input',
          val: {
            pane: event.val.context.pane,
            stages: [
              { text: '/compact', delayMs: 0 },
              { text: '\r', delayMs: 120 },
            ],
            lockUserInput: true,
          },
        },
        correlation: 1n,
        dedupKey: undefined,
      });
    }
    return submissions;
  },
});
```

Without the grant the whole `send-input` is refused with `not-permitted`; a plugin that only wants staged input without the lock leaves `lock-user-input` out.

### Errors and edge cases

| Situation | Outcome | What the plugin does |
|---|---|---|
| Grant missing or declined | `refused(not-permitted)` on the intent, `not-permitted` on the function | Send the stages without `lock-user-input`. |
| `millis` above 2,000 | `invalid-argument` | Use a shorter lock. |
| Pane already locked | `conflict` | Wait for the unlock; one lock per pane at a time. |
| Plugin stops while locked | lock released, queue delivered | Nothing. |
| Pane closed while locked | lock dropped with the pane | Nothing. |
| Session limit on locks (one lock per pane, so at most the number of open panes; no other cap) | `busy` | Retry later. |

Check order for `send-input` is: capability, stage and size rules, `if-current` precondition, then the lock. A failed precondition refuses the intent before any lock is taken.

### What the user sees

The review dialog lists the grant when a package is installed or first activated. The line is high risk and starts unchecked.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.stagedinput 0.1.0 by Example Ltd                               │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ <span class="t-warn">! [ ] Holds your typed keys for up to 2 s</span> <span class="t-dim">………………………………</span> <span class="t-warn">terminal.input-lock</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a package that requests terminal.input-lock, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar.</figcaption>
</figure>

Listing the grants of an installed plugin from the terminal:

<figure>

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants com.example.stagedinput
<span class="t-dim">Grant                             Risk     State</span>
terminal.input-lock               high     <span class="t-warn">declined</span>
</pre>

<figcaption>Output of the grant listing for this plugin. A high-risk grant that the user left unchecked shows as declined.</figcaption>
</figure>

The user can change the grant at any time in Settings > Plugins > com.example.stagedinput > Capabilities. A revocation re-activates the plugin with the reduced grant set, and the stubs of the declined imports return `not-permitted`.

### Related pages

- [terminal.write](/pages/capability-terminal-write/).
- [terminal.raw-input](/pages/capability-terminal-raw-input/).
- [Intents](/pages/api-intents/).
- [Terminal primitives](/pages/primitives-terminal/).

Requirement ids for this page: [PRI-004]. See the [requirements index](/pages/reference-requirements/).
