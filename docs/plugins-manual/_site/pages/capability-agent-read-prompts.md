# agent.read-prompts

The `agent.read-prompts` grant lets a plugin read the text of every prompt that is submitted to an agent pane. A prompt is the text that a user, a plugin, a schedule or voice input sends to the agent. Prompts often contain private code, file paths, customer details and secrets. For that reason the grant is high risk (H). The user must grant it explicitly, even for a T1 bundled plugin.

<div class="warn">

Prompts are the user's own work, and they often contain secrets or private information. A plugin with this grant receives the full text of every prompt sent to every agent pane the kernel tracks. Ask for the grant only when the feature needs the text. The fact that a prompt was sent is not available without the grant, not even as a length or a hash.

</div>

## You will need

- The grant `agent.read-prompts` on each side whose code receives the event: `server`, `client`, or both. Grants are recorded per side (see [Capabilities](/pages/capabilities/)).
- [agent.read](/pages/capability-agent-read/), which the feature usually needs as well, to tie a prompt to the agent's state. The host does not check `agent.read` for the prompt event, which carries only the pane.
- The SDK for the language you write in: [Rust SDK](/pages/rust-sdk/) or [TypeScript SDK](/pages/typescript-sdk/).

## Grant syntax

- Valid: `agent.read-prompts`.
- Invalid: `agent.read-prompts:user`. The install fails with `manifest.grant-argument` and the text `capability "agent.read-prompts" takes no argument`. The source of a prompt is a field of the event.

```json
{
  "capabilities": {
    "server": ["agent.read", "agent.read-prompts"]
  }
}
```

## Surface

| Name | Rust | TypeScript | Meaning |
|---|---|---|---|
| `input-submitted` event | `Event::InputSubmitted(InputSubmittedEvent { node, text, source, epoch })` | `{ tag: 'input-submitted', val: { node, text, source, epoch } }` | A prompt was submitted to an agent pane. Never dropped. |
| `node` | `node: NodeId` (`u64`) | `node: bigint` | The agent pane that received the prompt. |
| `text` | `text: String` | `text: string` | The full prompt text. The host sets no size cap. |
| `source` | `source: InputSource` | `source: InputSource` | Who sent the prompt (see below). |
| `epoch` | `epoch: u64` | `epoch: bigint` | The prompt counter of the pane after this prompt (see below). |
| `last-prompt` | `primitives::agent::last_prompt(pane) -> Option<String>` | `lastPrompt(pane: bigint): string \| undefined` | The most recent prompt text of one pane, or none. Synchronous. |

Import paths: `ilium_plugin_sdk::primitives::agent` in Rust and `@ilium/plugin-sdk/primitives/agent` in
TypeScript. The event type comes from `ilium_plugin_sdk::types::events` and from `@ilium/plugin-sdk/types`.

The `InputSource` enum has four cases:

| Source | Rust case | TypeScript value | Sent by |
|---|---|---|---|
| `user` | `InputSource::User` | `'user'` | the user typed the prompt in the pane |
| `plugin` | `InputSource::Plugin` | `'plugin'` | a plugin used `send-input` |
| `scheduled` | `InputSource::Scheduled` | `'scheduled'` | a schedule started the prompt |
| `voice` | `InputSource::Voice` | `'voice'` | voice input dictated the prompt |

### The prompt epoch

The kernel keeps one `u64` counter per terminal pane, starting at 0. It increments the counter each time it records a submitted prompt, at the same moment it emits `input-submitted`. The `epoch` field carries the value after that increment. A plugin can read the current value with `terminal.prompt-epoch(pane)`, which needs the grant `terminal.write`. It can pass that value as `prompt-epoch` in a `send-input` submission. If the value is stale, the submission completes as `refused(conflict)`. Reading the epoch from the event needs `agent.read-prompts`, and reading it from the function needs `terminal.write`.

<figure>
<svg viewBox="0 0 400 230" role="img" aria-label="Four prompt sources reach the agent pane. The pane emits input-submitted, the high-risk grant check passes text to the plugin, and a declined grant sends nothing.">
<defs>
<marker id="prompts-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="25" y="10" width="80" height="28" rx="6" fill="none" stroke="currentColor"/>
<text x="65" y="28" font-size="10" text-anchor="middle" fill="currentColor">user</text>
<rect x="115" y="10" width="80" height="28" rx="6" fill="none" stroke="currentColor"/>
<text x="155" y="28" font-size="10" text-anchor="middle" fill="currentColor">plugin</text>
<rect x="205" y="10" width="80" height="28" rx="6" fill="none" stroke="currentColor"/>
<text x="245" y="28" font-size="10" text-anchor="middle" fill="currentColor">scheduled</text>
<rect x="295" y="10" width="80" height="28" rx="6" fill="none" stroke="currentColor"/>
<text x="335" y="28" font-size="10" text-anchor="middle" fill="currentColor">voice</text>
<line x1="65" y1="38" x2="65" y2="60" stroke="currentColor" marker-end="url(#prompts-arrow)"/>
<line x1="155" y1="38" x2="155" y2="60" stroke="currentColor" marker-end="url(#prompts-arrow)"/>
<line x1="245" y1="38" x2="245" y2="60" stroke="currentColor" marker-end="url(#prompts-arrow)"/>
<line x1="335" y1="38" x2="335" y2="60" stroke="currentColor" marker-end="url(#prompts-arrow)"/>
<rect x="25" y="60" width="350" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="200" y="79" font-size="11" text-anchor="middle" fill="currentColor">Agent pane records the prompt</text>
<line x1="200" y1="90" x2="200" y2="112" stroke="currentColor" marker-end="url(#prompts-arrow)"/>
<text x="208" y="104" font-size="10" fill="currentColor">input-submitted</text>
<rect x="25" y="112" width="350" height="30" rx="4" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="200" y="131" font-size="11" text-anchor="middle" fill="currentColor">agent.read-prompts check (H)</text>
<line x1="110" y1="142" x2="110" y2="166" stroke="currentColor" marker-end="url(#prompts-arrow)"/>
<text x="118" y="158" font-size="10" fill="currentColor">text, source, epoch</text>
<rect x="25" y="166" width="170" height="30" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="110" y="185" font-size="11" text-anchor="middle" fill="currentColor">Plugin receives text</text>
<line x1="310" y1="142" x2="310" y2="166" stroke="currentColor" marker-end="url(#prompts-arrow)"/>
<rect x="235" y="166" width="150" height="30" rx="6" fill="none" stroke="currentColor"/>
<text x="310" y="185" font-size="11" text-anchor="middle" fill="currentColor">Declined: nothing sent</text>
<text x="25" y="218" font-size="10" fill="currentColor">H marks the high-risk grant check.</text>
</svg>
<figcaption>Every prompt passes the high-risk check before a plugin sees its text. Without the grant, the event is never delivered and nothing derived from it is sent.</figcaption>
</figure>

## Prompt sources in practice

- Treat `user` prompts as the core signal. A feature that reacts to what the person asked, such as a banner or a counter, usually filters for `user`.
- Treat `plugin` prompts as your own. Prompts that another plugin submitted through `send-input` arrive here too, so a plugin that submits prompts must not count its own as user work.
- `scheduled` and `voice` prompts arrive with their own source value. Filter on the value you need rather than on the absence of other values.

## Worked example in Rust: count user prompts without keeping their text

This server-side plugin counts the user's prompts for the session and logs the source and epoch of each prompt. It binds the text field with `..` so the text is never read into a variable, and it never writes the text to the log.

Manifest excerpt for the server side: `"capabilities": { "server": ["agent.read-prompts"] }` and
`"server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": [] }`.

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::{InputSource, InputSubmittedEvent};

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct PromptCounter {
    user_prompts: u64,
}

impl Lifecycle for PromptCounter {}

impl Handler for PromptCounter {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::InputSubmitted(InputSubmittedEvent { node, source, epoch, .. }) = event
            else {
                continue;
            };
            let source_label = match source {
                InputSource::User => {
                    self.user_prompts += 1;
                    "user"
                }
                InputSource::Plugin => "plugin",
                InputSource::Scheduled => "scheduled",
                InputSource::Voice => "voice",
            };
            log::log(
                Level::Info,
                &format!("prompt from {source_label} in pane {node}, epoch {epoch}"),
            );
        }
        Vec::new()
    }
}
```

The `Lifecycle` methods all have defaults, so the empty impl is enough. The `handler` export is
required and the `exports` list stays empty. The test for this plugin uses `FakeHost` (see [Testing](/pages/testing/)).

## Worked example in TypeScript: a last-prompt banner

This client-side plugin shows the first line of the most recent prompt of the last pane that received
one. It keeps only the pane id in memory. It reads the text from the host each time the view renders,
so no copy of the prompt text is stored.

Manifest excerpt for the client side: `"client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": ["views"] }`
and `"capabilities": { "client": ["agent.read-prompts"] }`.

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { lastPrompt } from '@ilium/plugin-sdk/primitives/agent';
import { block, paragraph, tree } from '@ilium/plugin-sdk/ui/build';

let promptsGranted = false;
let watchedPane: bigint | undefined;

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  activate(context) {
    promptsGranted = context.granted.includes('agent.read-prompts');
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'input-submitted') {
        watchedPane = event.val.node;
      }
    }
    return [];
  },
  render(view, area) {
    if (!promptsGranted) {
      return tree(block({ title: 'Last prompt' }, [paragraph(['Prompts are not available.'])]));
    }
    const text = watchedPane === undefined ? undefined : lastPrompt(watchedPane);
    const firstLine = text?.split('\n')[0] ?? 'No prompt yet.';
    return tree(block({ title: 'Last prompt' }, [paragraph([firstLine])]));
  },
});
```

The `activate` hook reads the activation context, which lists only the grants that were accepted. When the
grant is declined, the banner says so rather than showing an empty box. The `render` function runs under
the 10 ms `render_deadline_ms` default (see [Limits](/pages/reference-errors-and-limits/)), and `lastPrompt` is a
synchronous call, so the read fits.

## What the user sees

When a T2 or T3 plugin is installed, the review dialog lists the grant with the `!` mark (high risk). The row is
unchecked until the user checks it.

<figure>
<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ com.example.agent 0.1.0 by Example Ltd                                     │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ <span class="t-warn">! [ ] Reads the prompts you submit</span> <span class="t-dim">……………………………………………………</span> <span class="t-warn">agent.read-prompts</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The review dialog for a package that requests agent.read-prompts, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; a high-risk row starts unchecked.</figcaption>
</figure>

A T1 bundled plugin is activated first without the grant. On its first activation, the user sees one prompt,
`Plugin <name> asks for: agent.read-prompts`, with the buttons `Allow selected` and `Not now`. `Not now`
asks again at the next server start, up to three times. After that, the user can grant it from Settings >
Plugins.

Once the banner is running, it looks like this:

<figure>
<pre class="term">
<span class="t-hi">╭─ Last prompt ──────────────────────────────────────────────────────────────╮</span>
│ Fix the failing parser test and explain the cause                          │
<span class="t-dim">╰────────────────────────────────────────────────────────────────────────────╯</span>
</pre>
<figcaption>Illustrative, not generated. The banner from the TypeScript example after the user submits one prompt. The text is a placeholder.</figcaption>
</figure>

## Errors and edge cases

The prompt event and `last-prompt` return no error codes. Their only failure is absence: a prompt that
is not delivered, or a value of none.

| Situation | What the plugin gets |
|---|---|
| The grant is not held, or was declined | No `input-submitted` event. `last-prompt` returns none. `activate` sees no `agent.read-prompts` in `granted`. |
| The grant is revoked while the plugin runs | In-flight calls return `revoked`. The plugin receives `deactivate(revoked)`, is re-activated with the smaller set, and no more events arrive. A revocation does not count as a failure. |
| The pane is unknown or closed | `last-prompt` returns none. |
| A restored pane has no prompt since the server started | `last-prompt` returns none. The prompt text is not kept in the snapshot (see the retention rule below). |
| The plugin is lazy and not active when the prompt is submitted | No event is delivered, and nothing is queued for later. |
| The event queue is full | The event is never dropped. A full queue counts as a stuck plugin and restarts it under the restart rule. |
| A `send-input` uses a stale `prompt-epoch` | The submission completes as `refused(conflict)`. Re-read the epoch and retry. |

The retention rule: the kernel keeps the latest prompt text of each pane in server memory for the life
of the server process. A server restart clears it. The snapshot stores the epoch counter but not the text.

## Limits

- `event_queue` is 1,024 events by default and 4,096 at the T1 ceiling (see [Errors and limits](/pages/reference-errors-and-limits/)). Prompt events are never dropped, so a slow handler fills the queue quickly.
- Prompt text that the plugin keeps counts against its `memory_mib` budget (32 MiB by default). The banner example keeps none.
- Prompt text stored with `storage.put` counts against the storage bucket (16 MiB by default). Store only what the feature needs.

## Privacy practices

- Keep the smallest value that the feature needs. A last-prompt banner needs one line of one prompt, not a log of all of them.
- Do not write prompt text to the log, to storage or to a decoration on a tree node. Decorations show in the tree to everyone who looks at the screen.
- Use the `source` field to drop prompts that the feature should ignore, and do so before anything is kept.
- Read the prompt only when the banner or view is about to render. The `last-prompt` call is cheap enough to repeat.

## Risk and tier rules

- The grant is high risk (H). A T1 bundled plugin does not receive it automatically: the user grants it at the first activation prompt.
- A T2 or T3 plugin lists the grant on the review dialog, unchecked. The user checks it, or the plugin installs without it.
- The grant is a standing permission. There is no prompt for each call.

## First-party features that request it

- Feature 33, the last-prompt banner. It also reads the transcript scopes of agent sessions, which come from [agent.read](/pages/capability-agent-read/).

## See also

- [agent.read](/pages/capability-agent-read/) for the agent state that goes with each prompt.
- [agent.control](/pages/capability-agent-control/) for the other high-risk agent grant.
- [terminal.write](/pages/capability-terminal-write/) for `send-input` and the `terminal.prompt-epoch` function.
- [agent primitives](/pages/primitives-agent/) for the full `agent` interface.
- [Events](/pages/api-events/) for the delivery rules of every event.
- [Security](/pages/security/) for how the grant fits the rest of the permission model.

Checked against the API version `ilium:plugin@0.1.0`.

### Security

Prompts may contain secrets; see [Secrets](/pages/reference-secrets/) and [Threat model](/pages/reference-threat-model/).
