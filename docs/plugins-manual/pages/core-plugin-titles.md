---
layout: page
title: Titles and organisation
permalink: /pages/core-plugin-titles/
---

## Titles and organisation

Titles and organisation is the plugin form of Ilium's AI naming and tree tidying. It turns each prompt you send to an agent into a short pane title, picks an icon for the row, names new sessions, and can reorganise the whole tree into groups that you undo in one step. Its id is `ilium.core.titles` and it runs on both sides: the server side asks the model and edits the tree, and the client side shows the preview panel. It needs a configured AI provider, which [Inference providers](/pages/core-plugin-inference/) supplies.

Ilium keeps one rule about names: a title that you typed by hand is never replaced by an inferred one. The host enforces it, so the plugin only has to submit and read the outcome.

### What it covers

- **Automatic pane titles and title style.** A submitted prompt yields a short title in the style you chose (Automatic titles, Title style in the user documentation).
- **Session naming.** New sessions get a name, and you can trigger a name by hand (Triggering titles manually).
- **Tree organisation.** The model proposes groups, and one undo step reverts them (Tree organization instructions; Tree organization in the privacy guide).
- **AI triggers.** Toggles decide which events start titling (the Triggers tab of Settings).
- **Custom instruction inputs.** Seven text inputs steer the titles. They live in the shared `[inference.instructions]` table, which Inference, Titles, Recap and Smart copy all read (see [config and data migration](/pages/reference-config-and-data-migration/) and the LLM Instructions tab in [Inference](/pages/core-plugin-inference/)).
- **Inferred-title icons.** The title also picks the icon of the row.

### See it

<figure><img src="/assets/demos/03-contextual-titles.gif" alt="Pane titles change as the user sends prompts to agents" loading="lazy"><figcaption>Pane titles follow the prompts sent to each agent.</figcaption></figure>

<figure><img src="/assets/demos/01-ai-tree.gif" alt="The tree is reorganised into groups by the model" loading="lazy"><figcaption>The tree organised into groups.</figcaption></figure>

<figure><img src="/assets/demos/07c-settings-tour-c.gif" alt="A tour of the last Settings tabs, including the title options" loading="lazy"><figcaption>The title options in Settings.</figcaption></figure>

### What the user sees

The first render shows the tree before and after an organisation, as a diff view in the preview panel. The second shows the settings tab.

<figure>
<pre class="term">╭─ Preview of the change ──────────────────────╮
│<span class="t-dim">@@ tree, 4 panes @@</span>                           │
│<span class="t-err">- shell-1</span>                                     │
│<span class="t-err">- shell-2</span>                                     │
│<span class="t-ok">+ Billing API</span>                                 │
│<span class="t-ok">+   claude</span>                                    │
│<span class="t-ok">+   shell-1</span>                                   │
│<span class="t-ok">+ Web client</span>                                  │
│<span class="t-ok">+   codex</span>                                     │
│<span class="t-ok">+   shell-2</span>                                   │
│<span class="t-dim">Moves: 4, new groups: 2</span>                       │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The proposed tree as a unified <code>diff-view</code>; removed rows use <code>danger</code>, added rows use <code>success</code>.</figcaption>
</figure>

<figure>
<pre class="term">╭─ Titles ─────────────────────────────────────╮
│ Title style ……………………………………… ‹ Short phrase › │
│ Name panes from prompts ………………………………… ‹ On › │
│ Name new sessions ………………………………………………… ‹ On › │
│ Rename on session change …………………………… ‹ Off › │
│ Instructions for titles                      │
│ ╭──────────────────────────────────────────╮ │
│ │ Keep titles under four words.            │ │
│ │ Use the repository name first.           │ │
│ ╰──────────────────────────────────────────╯ │
│<span class="t-dim">  Saved 600 ms after you stop typing.</span>         │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Title style, the prompt and session toggles, and the instruction text area, with leaders between label and value.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.titles",
  "capabilities": {
    "server": ["inference", "agent.read-prompts", "tree.read", "tree.write",
               "tree.decorate", "tree.restructure"],
    "client": ["ui.panels"]
  }
}
```

`tree.restructure`, `agent.read-prompts`, and the `settings.write-kernel` grants for the Titles and Triggers tabs are high-risk grants: the user confirms them at install. Everything else is normal risk.

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.titles",
  "name": "Titles",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "Titles and organisation is the plugin form of Ilium's AI naming and tree tidying.",
  "license": "MIT",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0"
  },
  "capabilities": {
    "server": [
      "inference",
      "agent.read-prompts",
      "tree.read",
      "tree.write",
      "tree.decorate",
      "tree.restructure",
      "settings.write-kernel:titles",
      "settings.write-kernel:triggers",
      "extensions.provide:ilium.core.titles/before-apply-title",
      "extensions.provide:ilium.core.titles/title-styles"
    ],
    "client": [
      "ui.panels"
    ]
  },
  "contributes": {
    "extension_points": [
      {
        "id": "before-apply-title",
        "kind": "hook",
        "mode": "transform",
        "title": "Before apply title",
        "payload_schema": {
          "type": "object",
          "required": [
            "node",
            "proposed",
            "source",
            "previous"
          ],
          "properties": {
            "node": {
              "type": "string"
            },
            "proposed": {
              "type": "string"
            },
            "source": {
              "type": "string",
              "enum": [
                "transcript",
                "process",
                "manual-trigger"
              ]
            },
            "previous": {
              "type": "string"
            }
          }
        },
        "reply_schema": {
          "type": "object",
          "required": [
            "proposed"
          ],
          "properties": {
            "proposed": {
              "type": "string"
            }
          }
        },
        "risk": "standard",
        "deadline_ms": 20,
        "max_handlers": 8,
        "version": "0.1.0"
      },
      {
        "id": "title-styles",
        "kind": "collect",
        "title": "Title styles",
        "entry_schema": {
          "type": "object",
          "required": [
            "id",
            "title",
            "instruction"
          ],
          "properties": {
            "id": {
              "type": "string"
            },
            "title": {
              "type": "string"
            },
            "instruction": {
              "type": "string"
            }
          }
        },
        "max_entries": 64,
        "dynamic": false,
        "version": "0.1.0"
      }
    ]
  },
  "activation": [
    "startup"
  ],
  "limits_tier": "first-party"
}
```

### Kernel tabs this plugin owns

This plugin is the single owner of the following built-in Settings tabs. It holds `settings.write-kernel:<tab>` for each one (T1 only, one holder per tab, see [kernel settings](/pages/api-kernel-settings-and-keymap/#writing-a-kernel-tab)). The values stay in the same `config.toml` tables as before, so no user setting moves. [COR-030] [COR-033]

| Tab id | Label | Config table | Readers |
|---|---|---|---|
| `titles` | Titles | `[inference]` | none |
| `triggers` | Triggers | `[triggers]` | none |

If the plugin is disabled, the tab stays in the rail with its stored values shown read-only and the notice "Titles and organisation is disabled. Values are kept." The kernel keeps reading the keys it consumes itself (listed in the Consumer column of the key table in [config and data migration](/pages/reference-config-and-data-migration/)). Behaviour that only this plugin provides is off. The exact result for this plugin is its row in [kernel and boot](/pages/reference-kernel-and-boot/). [COR-022]

### Inference unavailable, slow or over budget

Naming calls a model for every pane, so the plugin has a defined path for each way the call can fail. The title of a pane never depends on the model: the model only improves it. [COR-011]

**The non-AI path.** When the [inference](/pages/api-inference/) call fails with `not-found` (no provider selected, reported to the user as `inference.unavailable`) or `not-permitted` (grant declined or purpose refused), the plugin makes no further call for that event and builds the title without a model. The fallback title is the name of the foreground process of the pane, then a space and the last path segment of its working directory, cut to the title length limit (for example `claude billing-api`). A pane that has neither keeps its current title. The fallback goes through the same `ilium.core.titles/before-apply-title` hook and the same `rename-node` as an inferred title, so a hand-typed title is still never replaced. `not-found` and `not-permitted` are not retried; they are re-checked only when the provider setting or the grant changes (`kernel-settings-changed` for `inference`, or a grant change).

**Retry after a failure.** Retryable failures are `timed-out`, `network`, `busy`, `rate-limited` and `quota-exceeded` (see [the error table](/pages/api-inference/#errors)). The plugin applies the fallback title at once, then retries the request for that pane at most 3 times, after 30 s, 2 min and 10 min. When the error carries `retry-after-ms`, the longer of that delay and the step above is used. After the third failure the pane keeps the fallback title until its next trigger (a new prompt, a session change). A failure of one pane never delays another pane, and the plugin never runs more than 2 title requests at the same time, so a provider outage does not become a retry storm.

**Call caps.** The plugin sends at most 6 model calls per pane per hour and at most 120 per session per hour. A trigger over a cap is answered with the fallback title and no call; the counters are in memory and restart at zero with the server. Tree organisation counts as one call of the session. The cap is checked before the call, so a refused trigger costs nothing and writes no privacy line.

**Token budget.** One title request sends at most 2,000 input tokens (the latest prompt cut to fit, plus the style instruction and the custom instructions) and asks for at most 32 output tokens. One tree organisation request sends at most 8,000 input tokens (pane titles and group names, without prompt text) and asks for at most 1,500 output tokens. The per-plugin spending budget of the host, described in [inference budgets](/pages/api-inference/), stays above these limits: when it refuses a call the plugin treats it as `quota-exceeded` and uses the fallback.

**Timeout.** A suggestion that has not arrived 20 s after the request was sent is dropped: the plugin cancels the request (the result is `cancelled`, nothing is applied) and uses the fallback title. A reply that arrives after the drop is discarded and is never applied. The tree organisation request has a 60 s limit and, on timeout, leaves the tree unchanged.

**A manual title ends inference for that pane.** Once the user has typed a title for a pane, the plugin makes no inference call for that pane for any trigger, cap or retry, and cancels any request already pending for it. Removing the manual title (the rename dialog with an empty value) makes the pane eligible again.

**What the user sees while a title is pending.** The row keeps its previous title, or the fallback title if it never had one, and shows a dim `…` after the title for as long as the request is pending, at most 20 s. No dialog, toast or error line appears for a single failure. After the third failed retry the row shows the fallback title without the marker, and Settings > Titles shows one line, "Titles are not using the model: <reason>", where the reason is the error code in words ("no provider selected", "rate limited", "budget spent"). The line clears at the next successful title.

| Situation | Model call | Title shown | Retry |
|---|---|---|---|
| `not-found` or `not-permitted` | none after the first | process name and working directory | on a settings or grant change |
| `timed-out`, `network`, `busy` | yes | fallback, then the model title if a retry succeeds | 30 s, 2 min, 10 min |
| `rate-limited`, `quota-exceeded` | yes | fallback | after `retry-after-ms` or the step, whichever is longer |
| Per-pane or per-session cap reached | none | fallback | when the hour window moves |
| Manual title on the pane | none | the manual title | never |
| Suggestion pending over 20 s | cancelled | fallback | next trigger |

The numeric limits in this section (3 retries, 2 concurrent requests, 6 and 120 calls per hour, 2,000 and 8,000 input tokens, 20 s and 60 s) are the proposed production defaults for this page; the owner confirms them.

### Ownership of the `[inference]` table and the tree-tidy command

The replacement matrix leaves two decisions open ([decision 5 and decision 6](/pages/reference-core-replacement-matrix/)). This page resolves them so that every stored section and every command has exactly one owner. [COR-203] [COR-011]

**Decision 5: `ilium.core.inference` owns `[inference]`.** The config table `[inference]` (and its child `[inference.instructions]`) has one owner, [Inference providers](/pages/core-plugin-inference/). Only that plugin holds `settings.write-kernel:inference` and `settings.write-kernel:llm-instructions`. This plugin holds no write grant for either table. The `titles` tab above keeps its place in the Settings rail and keeps its rows (Title style, the prompt and session toggles, Instructions for titles), but their values are stored under the keys that `ilium.core.inference` owns, and a change in the tab reaches the owner as a settings write that the host routes to it. The `Config table` cell of the `titles` row therefore reads "view of `[inference]`; written by `ilium.core.inference`", and its `Readers` cell reads "Titles". `[triggers]` stays owned by this plugin.

Titles reads the table through `settings.read-kernel:inference` (server and client side; add it to the `capabilities` of the manifest). It reads once at activation and again on every `kernel-settings-changed` event whose `tab` is `inference` or `llm-instructions`, using the `keys` list to refresh only what changed (see [kernel settings](/pages/api-kernel-settings-and-keymap/)). A change takes effect for the next trigger; a request already in flight finishes with the values it was sent with. If the read fails with `not-permitted` (grant revoked), Titles uses the built-in defaults, shows the "Titles are not using the model" line and keeps the fallback path of the previous section. If `ilium.core.inference` is disabled, the `[inference]` values stay as stored, `inference.unavailable` follows, and Titles uses the fallback path.

**Decision 6: the tree-tidy command is declared by Titles.** The command that starts tree organisation, `ilium.core.titles/tidy-tree`, is declared in the `commands` of the Titles manifest, with the server side handler that asks the model and the client side preview panel. The matrix row keeps it as the single owner. A second plugin that declares the same command name is refused at install with the command-collision error of [commands](/pages/api-commands/), so a replacement of Titles takes the command with it and a disabled Titles removes the command from the palette and from key binding choices, instead of leaving a command that nobody answers. The one-step undo of an organisation stays with the tree, not with the command, so it works while Titles is disabled.

| Item | Owner | Other plugins |
|---|---|---|
| Config table `[inference]` and `[inference.instructions]` | `ilium.core.inference` | Titles, Recap, Search, Smart copy, Compaction and Voice read it with `settings.read-kernel` |
| Config table `[triggers]` | `ilium.core.titles` | none |
| Command `tidy-tree` | `ilium.core.titles` | none |
| Tab `titles` (rows) | `ilium.core.titles` | values stored by `ilium.core.inference` |

The Contract section of this page, the replacement matrix row, and the complete manifest above must show these values, which is what [COR-203] checks. The manifest `capabilities` change is: add `settings.read-kernel:inference` and `settings.read-kernel:llm-instructions` on both sides, remove `settings.write-kernel:inference`, and add the `tidy-tree` entry to `commands`. The `Kernel tabs this plugin owns` table above and the manifest block are not rewritten by this addition; they are changed together with the code in the same change.

### Samples

#### 1. Automatic pane titles

On `input-submitted` the server asks [inference](/pages/api-inference/) for a title, with the [inference grant](/pages/capability-inference/) and the [prompt-reading grant](/pages/capability-agent-read-prompts/). The call returns a request id at once, so the plugin keeps the pair of request and pane, and on `request-completed` it submits `rename-node` with `source: inferred`. See [events](/pages/api-events/) and [intents](/pages/api-intents/).

**Rust**

```rust
use ilium_plugin_sdk::host::inference::{self, ChatMessage, CompletionRequest};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::RenameNodeSpec;
use ilium_plugin_sdk::types::tree::TitleSource;

// manifest: "capabilities": { "server": ["inference",
//   "agent.read-prompts", "tree.read", "tree.write"] }
#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Titler {
    pending: Vec<(RequestId, NodeId)>,
}

impl Lifecycle for Titler {}

impl Handler for Titler {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::InputSubmitted(input) => {
                    let request = CompletionRequest {
                        messages: vec![
                            ChatMessage {
                                role: "system".into(),
                                content: "Reply with a title of at most four words.".into(),
                            },
                            ChatMessage { role: "user".into(), content: input.text },
                        ],
                        max_output_tokens: None,
                        json_schema: None,
                        purpose: "pane-title".into(),
                    };
                    match inference::complete_request(request) {
                        Ok(id) => self.pending.push((id, input.node)),
                        Err(error) => log::log(Level::Warn, &format!("no title: {error:?}")),
                    }
                }
                Event::RequestCompleted(done) => {
                    let Some(at) = self.pending.iter().position(|(id, _)| *id == done.request)
                    else { continue };
                    let (_, node) = self.pending.remove(at);
                    let Some(completion) = inference::take_completion(done.request) else { continue };
                    submissions.push(Submission::new(Intent::RenameNode(RenameNodeSpec {
                        node,
                        title: completion.text.trim().to_string(),
                        source: TitleSource::Inferred,
                        expected_revision: None,
                    })));
                }
                _ => {}
            }
        }
        submissions
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { completeRequest, takeCompletion } from '@ilium/plugin-sdk/host/inference';

// manifest: "capabilities": { "server": ["inference",
//   "agent.read-prompts", "tree.read", "tree.write"] }
const pending = new Map<bigint, bigint>(); // request id -> node
let correlation = 1n;

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'input-submitted') {
        const id = completeRequest({
          messages: [
            { role: 'system', content: 'Reply with a title of at most four words.' },
            { role: 'user', content: event.val.text },
          ],
          maxOutputTokens: undefined,
          jsonSchema: undefined,
          purpose: 'pane-title',
        });
        pending.set(id, event.val.node);
      } else if (event.tag === 'request-completed' && pending.has(event.val.request)) {
        const node = pending.get(event.val.request)!;
        pending.delete(event.val.request);
        const completion = takeCompletion(event.val.request);
        if (completion === undefined) continue;
        submissions.push({
          intent: {
            tag: 'rename-node',
            val: { node, title: completion.text.trim(), source: 'inferred', expectedRevision: undefined },
          },
          correlation: correlation++,
          dedupKey: undefined,
        });
      }
    }
    return submissions;
  },
});
```

#### 2. Inferred-title icons

The title also picks an icon. The plugin maps the topic of the title to an icon and submits `set-inferred-icon`, one of the [tree chrome intents](/pages/api-tree-chrome/). An icon that you set by hand is never replaced: the host answers `refused(conflict)`.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::SetInferredIconSpec;

// manifest: "capabilities": { "server": ["tree.decorate"] }

/// A small topic table. The plugin's own code, not part of the SDK.
fn icon_for_title(title: &str) -> Option<&'static str> {
    let lower = title.to_lowercase();
    const TOPICS: [(&str, &str); 3] = [("fix", "🔧"), ("test", "🧪"), ("deploy", "🚀")];
    TOPICS.iter().find(|(word, _)| lower.contains(word)).map(|(_, icon)| *icon)
}

/// Submitted right after the rename. A manual icon wins: the host
/// answers refused(conflict) and the plugin leaves the row alone.
fn icon_submission(node: NodeId, title: &str) -> Option<Submission> {
    let icon = icon_for_title(title)?;
    Some(Submission::new(Intent::SetInferredIcon(SetInferredIconSpec {
        node,
        icon: Some(icon.into()),
        source: "titler".into(),
    })))
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk';

// manifest: "capabilities": { "server": ["tree.decorate"] }
const TOPICS: Array<[string, string]> = [['fix', '🔧'], ['test', '🧪'], ['deploy', '🚀']];

function iconForTitle(title: string): string | undefined {
  const lower = title.toLowerCase();
  return TOPICS.find(([word]) => lower.includes(word))?.[1];
}

// A manual icon wins: the host answers refused(conflict).
export function iconSubmission(node: bigint, title: string, correlation: bigint): Submission | undefined {
  const icon = iconForTitle(title);
  if (icon === undefined) return undefined;
  return {
    intent: { tag: 'set-inferred-icon', val: { node, icon, source: 'titler' } },
    correlation,
    dedupKey: undefined,
  };
}
```

#### 3. Organise the tree, with undo

With the [tree.restructure grant](/pages/capability-tree-restructure/) the plugin submits `restructure` with up to 256 operations and a label, described under [tree structure intents](/pages/api-intent-tree-structure/). It keeps the returned token and submits `revert-restructure` when the user presses Undo. The token does not survive a server restart.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::RestructureSpec;

// manifest: "capabilities": { "server": ["tree.read", "tree.restructure"] },
//           "contributes": { "commands": [{ "id": "undo-organise", ... }] }
impl Titler {
    /// Applies the proposal as one change. `plan_operations` is the
    /// plugin's own code: it builds move, create-group, ungroup and
    /// rename operations from the model's answer (at most 256).
    fn organise(&mut self) -> Submission {
        Submission::keyed(
            Intent::Restructure(RestructureSpec {
                operations: self.plan_operations(),
                label: "Group panes by project".into(),
            }),
            "organise".into(),
        )
    }

    fn on_event(&mut self, event: &Event) -> Vec<Submission> {
        match event {
            // ok-node carries the undo token; 0 means nothing changed.
            Event::RequestCompleted(done) => {
                if let Outcome::OkNode(token) = done.outcome {
                    if token != 0 {
                        self.undo_token = Some(token);
                    }
                }
                Vec::new()
            }
            Event::CommandInvoked(invoked) if invoked.command.ends_with("/undo-organise") => {
                self.undo_token
                    .take()
                    .map(|token| Submission::new(Intent::RevertRestructure(token)))
                    .into_iter()
                    .collect()
            }
            _ => Vec::new(),
        }
    }
}
```

**TypeScript**

```ts
import type { Event, Submission } from '@ilium/plugin-sdk';

// manifest: "capabilities": { "server": ["tree.read", "tree.restructure"] },
//           "contributes": { "commands": [{ "id": "undo-organise", ... }] }
let undoToken: bigint | undefined;

// planOperations is the plugin's own code: move, create-group, ungroup
// and rename operations from the model's answer (at most 256).
export function organise(): Submission {
  return {
    intent: { tag: 'restructure', val: { operations: planOperations(), label: 'Group panes by project' } },
    correlation: 1n,
    dedupKey: 'organise',
  };
}

export function onTreeEvent(event: Event): Submission[] {
  if (event.tag === 'request-completed') {
    const outcome = event.val.outcome;
    if (outcome.tag === 'ok-node' && outcome.val !== 0n) undoToken = outcome.val;
  } else if (event.tag === 'command-invoked' && event.val.command.endsWith('/undo-organise')) {
    if (undoToken === undefined) return [];
    const token = undoToken;
    undoToken = undefined;
    return [{ intent: { tag: 'revert-restructure', val: token }, correlation: 2n, dedupKey: undefined }];
  }
  return [];
}
```

#### 4. Preview the change

Before applying, the client panel shows the old and new outlines as a `diff-view` with a `paragraph` summary of the moves, both from [text widgets](/pages/widgets-text/). The server sends the two outlines to the client, which draws them; the apply button then calls the first function of the previous sample.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::layout::Area;
use ilium_plugin_sdk::types::ui::{DiffHunk, DiffLayout, DiffLine, UiTree, ViewTarget};
use ilium_plugin_sdk::ui::build::Tree;

// manifest: "capabilities": { "client": ["ui.panels"] }
impl Views for TitlesPanel {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let mut lines = vec![DiffLine::context(1, 1, "tree, 4 panes")];
        for (number, old) in self.old_outline.iter().enumerate() {
            lines.push(DiffLine::removed(number as u32 + 2, old));
        }
        for (number, new) in self.new_outline.iter().enumerate() {
            lines.push(DiffLine::added(number as u32 + 2, new));
        }
        let hunk = DiffHunk { header: "@@ tree @@".into(), lines };
        let diff = tree.diff_view(vec![hunk]).layout(DiffLayout::Unified).key("diff");
        let summary = tree
            .paragraph(vec![vec![Span::plain(format!(
                "Moves: {}, new groups: {}",
                self.moves, self.groups
            ))]])
            .key("summary");
        let root = tree
            .layout(Direction::Vertical, [Constraint::Fill(1), Constraint::Length(1)])
            .child(diff)
            .child(summary);
        tree.finish(root)
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { diffView, fill, layout, length, paragraph, span, tree } from '@ilium/plugin-sdk/ui/build';

// manifest: "capabilities": { "client": ["ui.panels"] }
export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  render() {
    const lines = [
      { kind: 'context', oldLine: 1, newLine: 1, text: 'tree, 4 panes' },
      ...state.oldOutline.map((text, i) => ({ kind: 'removed', oldLine: i + 2, text })),
      ...state.newOutline.map((text, i) => ({ kind: 'added', newLine: i + 2, text })),
    ];
    const diff = diffView([{ header: '@@ tree @@', lines }], { key: 'diff', layout: 'unified' });
    const summary = paragraph(
      [[span(`Moves: ${state.moves}, new groups: ${state.groups}`)]],
      { key: 'summary' },
    );
    return tree(layout({ direction: 'vertical', constraints: [fill(1), length(1)] }, [diff, summary]));
  },
});
```

#### 5. Instructions and triggers

Settings hold the instruction text and which events trigger titling. The plugin declares `text-area` and `toggle` rows (see [settings extension](/pages/settings-extension/) and [settings row types](/pages/reference-settings-rows/)) and re-reads its values on `settings-changed`, an [event](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::host::settings;
use ilium_plugin_sdk::prelude::*;
use serde::Deserialize;

// manifest settings rows (server scope):
//   { "type": "text-area", "key": "instructions", "title": "Instructions",
//     "max_length": 2000, "default": "" }
//   { "type": "toggle", "key": "name_from_prompts", "title": "Name panes",
//     "default": true }
#[derive(Deserialize, Default, Clone)]
struct TitleSettings {
    instructions: String,
    name_from_prompts: bool,
}

fn load_settings() -> TitleSettings {
    serde_json::from_str(&settings::get_json()).unwrap_or_default()
}

impl Handler for Titler {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            // Re-read on every change so the next title uses the new text.
            if let Event::SettingsChanged(_) = event {
                self.settings = load_settings();
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { getJson } from '@ilium/plugin-sdk/host/settings';

// manifest settings rows (server scope):
//   { "type": "text-area", "key": "instructions", "title": "Instructions",
//     "max_length": 2000, "default": "" }
//   { "type": "toggle", "key": "name_from_prompts", "title": "Name panes",
//     "default": true }
interface TitleSettings { instructions: string; name_from_prompts: boolean }

let settings: TitleSettings = JSON.parse(getJson());

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      // Re-read on every change so the next title uses the new text.
      if (event.tag === 'settings-changed') settings = JSON.parse(getJson());
    }
    return [];
  },
});
```

### Extension points

Other plugins change what `ilium.core.titles` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.titles/<point>`). Contributors need `extensions.contribute:ilium.core.titles/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `before-apply-title` | hook (transform) | Change a generated title before it is applied. | node, proposed, source (transcript, process, manual-trigger), previous; reply: proposed | standard |
| `title-styles` | collect | Add a title style to the style choice. | entry: id, title, instruction | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.titles/suggest`: title: ask for a title suggestion for text

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.titles/title-applied`: { node, title }

Calls `ilium:inference/provider` through the inference plugin.

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.titles"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Adds a style that puts the ticket key first

A package adds this entry in its manifest. It needs no code.

```json
{
  "api": "ilium:plugin@0.1.0",
  "capabilities": { "client": ["extensions.contribute:ilium.core.titles/title-styles"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.titles/title-styles",
        "id": "ticket-first",
        "priority": 50,
        "entry": {
          "title": "Ticket first",
          "instruction": "Start the title with the ticket key when the text names one."
        }
      }
    ]
  }
}
```

### Extension points this plugin owns

Values marked (default) come from the defaults in [Extension points](/pages/api-extension-points/). Both points run on the server: the manifest grants `extensions.provide` for each on the server side.

| Point | Side | Version | Deadline | Max | Dynamic |
|---|---|---|---|---|---|
| `ilium.core.titles/before-apply-title` | server | 0.1.0 (default) | 20 ms (default) | 8 handlers (default) | no (default) |
| `ilium.core.titles/title-styles` | server | 0.1.0 (default) | none (default) | 64 entries (default) | no (default) |

Side basis: `ilium.core.titles/before-apply-title` is server because the server side generates the proposed title and submits the `rename-node` that applies it, so the hook runs on the server before that apply step.
Side basis: `ilium.core.titles/title-styles` is server because the style `instruction` text it collects is the text the server side sends to the model when it asks for a title.

### ABI used

- [inference](/pages/api-inference/) and [the inference grant](/pages/capability-inference/)
- [agent.read-prompts](/pages/capability-agent-read-prompts/)
- [events](/pages/api-events/) and [intents](/pages/api-intents/)
- [tree chrome intents](/pages/api-tree-chrome/)
- [tree.restructure](/pages/capability-tree-restructure/) and [tree structure intents](/pages/api-intent-tree-structure/)
- [text widgets](/pages/widgets-text/)
- [settings extension](/pages/settings-extension/) and [settings row types](/pages/reference-settings-rows/)

### See also

- [Core replacement matrix](/pages/reference-core-replacement-matrix/) for the surface, stored data, CLI names and install codes that a replacement of this plugin must keep.
- [Core plugins](/pages/core-plugins/)
- [Inference providers](/pages/core-plugin-inference/), which supplies the model
- [Contract overview](/pages/getting-started-overview/)
