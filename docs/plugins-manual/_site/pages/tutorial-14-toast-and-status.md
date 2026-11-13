# 14. A toast and a status-bar segment

## 14. A toast and a status-bar segment

This tutorial gives a plugin two ways to speak to the user without opening a pane. The first is a **toast**, a short message that appears for a few seconds when something happens. The second is a **status-bar segment**, a small piece of text that stays at the bottom of the screen and always shows the latest count.

The plugin watches agents. When an agent finishes, it shows a toast. The whole time, it keeps a segment that reads `2 working`, and it removes the segment when no agent is working.

This tutorial is standalone. It starts a fresh plugin, so you do not need the earlier tutorials. You only need to have finished [Your first plugin in five minutes](/pages/getting-started-first-plugin/).

Some words first.

- An **event** is a message from Ilium to the plugin. Here it is `agent-activity-changed`, which says that an agent started working, went idle or finished.
- An **intent** is a request the plugin hands back, such as "show this toast". The plugin returns a list of them, and Ilium carries them out. One item of that list is a **submission**.
- A **grant** is a permission that the user allows at install.
- A **dedup key** is a label on a submission. If a second submission carries the same label within 10 minutes, Ilium ignores it. The word means "remove duplicates".
- A **status bar** is the one-line strip at the bottom of the Ilium window. A **segment** is one piece of text in it.

Tutorial plugin: `example.status-demo`. Side: client, because toasts and the status bar are part of the client window. Languages: Rust (target `wasm32-wasip2`) and TypeScript. Every listing is given in both.

### You will need

- [Your first plugin in five minutes](/pages/getting-started-first-plugin/) finished. Nothing else.
- Three grants, all on the client side:

| Grant | Dialog row text | What the plugin may do with it |
|---|---|---|
| `agent.read` | See agent status | Receive `agent-activity-changed` events |
| `notify.toast` | Show toasts | Send `toast` and `dismiss-toast` intents |
| `ui.chrome` | Add controls to the client frame | Send `set-status-segment` and `clear-status-segment` intents |

- Create the project:

```bash
ilium plugin new status-demo --id example.status-demo --lang rust --side client --name "Status demo"
```

Use `--lang typescript` for the TypeScript version.

### What this plugin does

- When an agent reaches the activity `done`, the plugin shows the toast `An agent finished`.
- After every batch of events, the plugin shows the number of working agents in the segment `working`. At zero it removes the segment.

### The surface this step uses

| Name | Kind | Fields and types | Meaning |
|---|---|---|---|
| `agent-activity-changed` | event | `node: node-id`, `previous`, `current: agent-activity`, `revision: u64` | Fires on every activity change. Needs `agent.read`. |
| `toast` | intent | `id`, `tone`, `title` (60 characters), `body` (200), `actions` (2), `duration-ms` | Shows or replaces a toast. Needs `notify.toast`. |
| `set-status-segment` | intent | `segment`, `content: rich-text`, `tooltip`, `tone` | Fills a segment. Needs `ui.chrome`. At most 10 per second per slot (the segment id is the slot). |
| `clear-status-segment` | intent | `segment` | Removes the segment. |
| `status_segments` | manifest key | `id`, `priority` (0 to 100), `max_width` (4 to 40 cells, default 24) | Declares a segment. At most 2 per plugin. |
| `dedup-key` | submission field | 1 to 128 characters from `A-Z a-z 0-9 . _ : / -` | A repeat within 10 minutes completes as `duplicate`. |
| `request-completed` | event | `request`, `outcome` | Reports how each submission ended. |

### Manifest

A segment must be declared before the plugin may fill it. The declaration says how important the segment is and how wide it may grow.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.status-demo",
  "name": "Status demo",
  "version": "0.1.0",
  "publisher": "Example Developer",
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": []
  },
  "capabilities": {
    "client": ["agent.read", "notify.toast", "ui.chrome"]
  },
  "contributes": {
    "status_segments": [
      { "id": "working", "priority": 40, "max_width": 16 }
    ]
  }
}
```

Two numbers deserve a closer look.

- `priority` runs from 0 to 100. When the window is too narrow for every segment, Ilium drops the lowest priority first. A higher number survives longer.
- `max_width` counts terminal cells, from 4 to 40. Text longer than this is cut and ends with `…`. The default is 24.

### The smallest plugin

The smallest version keeps a set of the agents that are working. After each event it writes the count into the segment, and on a finish it shows a toast.

**Rust**

```rust
// src/lib.rs. A toast on finish and a working count in the status bar.
use std::collections::BTreeSet;

use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::intents::{Intent, SetStatusSegmentSpec, ToastSpec};
use ilium_plugin_sdk::types::tree::AgentActivity;
use ilium_plugin_sdk::types::text::{RichText, Tone};

const SEGMENT: &str = "working";

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct StatusDemo {
    /// The nodes whose agent is working right now.
    working: BTreeSet<u64>,
}

impl Lifecycle for StatusDemo {}

/// The segment text, or `None` when nothing is working.
pub fn segment_text(count: usize) -> Option<String> {
    (count > 0).then(|| format!("{count} working"))
}

impl Handler for StatusDemo {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::AgentActivityChanged(AgentActivityEvent { node, current, .. }) = event
            else {
                continue;
            };
            if current == AgentActivity::Working {
                self.working.insert(node);
            } else {
                self.working.remove(&node);
            }
            if current == AgentActivity::Done {
                submissions.push(Submission::new(Intent::Toast(ToastSpec {
                    id: Some("finished".into()),
                    tone: Tone::Success,
                    title: "An agent finished".into(),
                    body: None,
                    actions: vec![],
                    duration_ms: None,
                })));
            }
            if let Some(text) = segment_text(self.working.len()) {
                submissions.push(Submission::new(Intent::SetStatusSegment(SetStatusSegmentSpec {
                    segment: SEGMENT.into(),
                    content: RichText::plain(text),
                    tooltip: None,
                    tone: Some(Tone::Info),
                })));
            }
        }
        submissions
    }
}
```

**TypeScript**

```ts
// src/client.ts. A toast on finish and a working count in the status bar.
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { richText } from '@ilium/plugin-sdk/ui/build';

const SEGMENT = 'working';
const working = new Set<bigint>();
let nextCorrelation = 1n;

/** The segment text, or undefined when nothing is working. */
export function segmentText(count: number): string | undefined {
  return count > 0 ? `${count} working` : undefined;
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'agent-activity-changed') continue;
      const { node, current } = event.val;
      if (current === 'working') working.add(node);
      else working.delete(node);
      if (current === 'done') {
        submissions.push({
          intent: {
            tag: 'toast',
            val: { id: 'finished', tone: 'success', title: 'An agent finished',
              body: undefined, actions: [], durationMs: undefined },
          },
          correlation: nextCorrelation++,
          dedupKey: undefined,
        });
      }
      const text = segmentText(working.size);
      if (text !== undefined) {
        submissions.push({
          intent: {
            tag: 'set-status-segment',
            val: { segment: SEGMENT, content: richText(text), tooltip: undefined, tone: 'info' },
          },
          correlation: nextCorrelation++,
          dedupKey: undefined,
        });
      }
    }
    return submissions;
  },
});
```

### Line by line

1. The set `working` is the plugin's memory. The handler runs once per batch, so the plugin must remember what it saw before.
2. The event carries `current`, the new activity. The code adds the node when it is `working` and removes it otherwise. That covers `idle`, `done`, `awaiting-approval` and `menu`.
3. The toast has the stable `id` `finished`. Two finishes close together replace one toast instead of stacking two.
4. When the count is zero, this version sends nothing. The old text would stay on screen. The next section fixes that.

### One improvement: clean updates

The smallest version has three flaws.

- **A stale segment.** At zero nothing clears the segment.
- **Too many updates.** Ilium accepts at most 10 `set-status-segment` intents per second for one segment (the segment id is the slot). A burst of events would trip `rate-limited` and the extra updates would be refused.
- **A repeated toast.** If the same finish is delivered twice, the user sees two toasts.

The fix is small. Update the segment once per batch, and only when the count changed. Clear the segment at zero. Put a dedup key on the toast, built from the node and the `revision` that the event carries. The revision grows with every change, so a new finish has a new key.

**Rust**

```rust
#[derive(Default)]
struct StatusDemo {
    working: BTreeSet<u64>,
    /// The count the status bar shows now. `None` before the first update.
    shown: Option<usize>,
}

impl StatusDemo {
    /// One submission when the count changed since the last update.
    fn refresh_segment(&mut self) -> Option<Submission> {
        let count = self.working.len();
        if self.shown == Some(count) {
            return None;
        }
        self.shown = Some(count);
        let intent = match segment_text(count) {
            Some(text) => Intent::SetStatusSegment(SetStatusSegmentSpec {
                segment: SEGMENT.into(),
                content: RichText::plain(text),
                tooltip: None,
                tone: Some(Tone::Info),
            }),
            None => Intent::ClearStatusSegment(SEGMENT.into()),
        };
        Some(Submission::new(intent))
    }
}

fn finished_toast(node: u64, revision: u64) -> Submission {
    let toast = Intent::Toast(ToastSpec {
        id: Some("finished".into()),
        tone: Tone::Success,
        title: "An agent finished".into(),
        body: None,
        actions: vec![],
        duration_ms: None,
    });
    Submission::keyed(toast, format!("finished:{node}:{revision}"))
}

// In on_events: call finished_toast(node, revision) when current is Done, and
// after the loop add:
//     submissions.extend(self.refresh_segment());
```

**TypeScript**

```ts
let shown: number | undefined;

/** One submission when the count changed since the last update. */
function refreshSegment(): Submission | undefined {
  const count = working.size;
  if (shown === count) return undefined;
  shown = count;
  const text = segmentText(count);
  const intent: Submission['intent'] = text === undefined
    ? { tag: 'clear-status-segment', val: SEGMENT }
    : { tag: 'set-status-segment',
        val: { segment: SEGMENT, content: richText(text), tooltip: undefined, tone: 'info' } };
  return { intent, correlation: nextCorrelation++, dedupKey: undefined };
}

function finishedToast(node: bigint, revision: bigint): Submission {
  return {
    intent: {
      tag: 'toast',
      val: { id: 'finished', tone: 'success', title: 'An agent finished',
        body: undefined, actions: [], durationMs: undefined },
    },
    correlation: nextCorrelation++,
    dedupKey: `finished:${node}:${revision}`,
  };
}

// In onEvents: call finishedToast(node, revision) when current is 'done', and
// after the loop add:
//     const update = refreshSegment();
//     if (update !== undefined) submissions.push(update);
```

The dedup key uses only allowed characters: letters, digits, `:` and `-`. A key that breaks the rule refuses the whole batch with `invalid-argument`.

### Narrow terminals

A segment never grows past its `max_width`. Longer text is cut with `…`. When the bar is still too narrow, the lowest `priority` goes first, and it returns when the window widens. None of this is an error. Write short text: `2 working` fits `max_width: 16`.

### Points to notice

- The toast is fire-and-forget: it needs no reply, and a failed toast never stops the status-bar update.
- The segment is set again on every change and removed with a clear call when the count reaches 0, so no stale `0 working` text stays.
- Text longer than `max_width` is cut with `…` and the lowest `priority` leaves first; none of this is an error.

### Edge cases and errors

| Case | What happens | Code or result |
|---|---|---|
| Same finish delivered twice | The second toast is skipped | `duplicate(original)` |
| Four toasts within 10 seconds | The fourth is dropped | `refused(rate-limited)` |
| More than 10 segment updates in a second | The extra updates are dropped | `refused(rate-limited)` with `retry-after-ms` |
| Segment id not declared in the manifest | The update is refused | `refused(invalid-argument)` |
| `ui.chrome` declined | The segment is not drawn | `refused(not-permitted)` |
| `notify.toast` declined | The toast is refused | `refused(not-permitted)` |

### Common mistakes

| Mistake | What you see | Fix |
|---|---|---|
| Declared three segments | Install refused with `manifest.contribution-limit` | A plugin may declare at most 2 segments. |
| Toast title over 60 characters | `refused(invalid-argument)` | Shorten the title or move words to the body. |
| Dedup key with a space | `refused(invalid-argument)`, whole batch refused | Use only `A-Z a-z 0-9 . _ : / -`. |
| Updated the segment on every event | `refused(rate-limited)` in bursts | Update once per batch and only on change. |

### Test it

The text function needs no host. Test it with a plain test. Then test the handler with the fake host, which keeps the real grants and limits.

**Rust**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ilium_plugin_sdk::testing::FakeHost;

    fn changed(node: u64, current: AgentActivity) -> Event {
        Event::AgentActivityChanged(AgentActivityEvent {
            node,
            previous: AgentActivity::Idle,
            current,
            revision: 1,
        })
    }

    #[test]
    fn text_is_empty_at_zero() {
        assert_eq!(segment_text(0), None);
        assert_eq!(segment_text(2).as_deref(), Some("2 working"));
    }

    #[test]
    fn a_finish_shows_one_toast_and_clears_the_segment() {
        let host = FakeHost::new().grant("agent.read").grant("notify.toast").grant("ui.chrome");
        let mut host = host;
        host.activate::<StatusDemo>();
        host.deliver(vec![changed(42, AgentActivity::Working)]);
        let submissions = host.deliver(vec![changed(42, AgentActivity::Done)]);
        assert!(matches!(submissions[0].intent, Intent::Toast(_)));
        assert!(matches!(submissions[1].intent, Intent::ClearStatusSegment(_)));
    }
}
```

**TypeScript**

```ts
// src/client.test.ts. Run with: bun test
import { expect, test } from 'bun:test';
import type { Event } from '@ilium/plugin-sdk/types';
import { createFakeHost } from '@ilium/plugin-sdk/testing';
import { handler, lifecycle, segmentText } from './client';

function changed(node: bigint, current: 'working' | 'done'): Event {
  return { tag: 'agent-activity-changed', val: { node, previous: 'idle', current, revision: 1n } };
}

test('text is empty at zero', () => {
  expect(segmentText(0)).toBeUndefined();
  expect(segmentText(2)).toBe('2 working');
});

test('a finish shows one toast and clears the segment', async () => {
  const host = createFakeHost().grant('agent.read').grant('notify.toast').grant('ui.chrome');
  await host.activate({ lifecycle, handler });
  host.deliver([changed(42n, 'working')]);
  const submissions = host.deliver([changed(42n, 'done')]);
  expect(submissions[0].intent.tag).toBe('toast');
  expect(submissions[1].intent.tag).toBe('clear-status-segment');
});
```

Run them with `cargo test` or `bun test`. Read live refusals with `ilium plugin logs example.status-demo`.

### What you see

With two agents working and a wide window, the segment shows between the other pieces of the bar.

<figure>
<pre class="term"><span class="t-hi">ℹ Tree</span> · Ctrl+B menu · <span class="t-ok">2 working</span> · <span class="t-dim">5 panes</span>
</pre>
<figcaption>Rendered at 80 columns or fewer. Illustrative. The segment sits among the kernel pieces of the status bar, separated by dots.</figcaption>
</figure>

When one agent finishes, the toast opens at the same time as the count falls to one.

<figure>
<pre class="term">╭  ≡ ● · An agent finished ────────────────────╮
│ ━━━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────│
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A success toast with a title and no body. The gauge shrinks until the toast closes after about 5 seconds.</figcaption>
</figure>

### Try it yourself

1. Change `max_width` to 6 and read the cut text.
2. Add a `body` to the toast that names the node, and check the 200-character limit by sending a long body.
3. Add a second segment `awaiting` for agents in `awaiting-approval`.

### See also

- [Toast API](/pages/api-toast/) has every toast field, the rules and the errors.
- [Tree chrome API](/pages/api-tree-chrome/) covers `status_segments`, `set-status-segment` and the other chrome intents.
- [Capability: ui.chrome](/pages/capability-ui-chrome/) describes the grant and its declined behaviour.
- [Capability: notify.toast](/pages/capability-notify-toast/) describes the toast grant.
- [Intents and outcomes](/pages/api-intents/) explains submissions, dedup keys and outcomes.
- [Tutorial 13](/pages/tutorial-13-keys-and-when/) binds keys to a toast command.

API checked against `ilium:plugin@0.1.0`.
