# Titles and organisation

## Titles and organisation

Titles and organisation is the plugin form of Ilium's AI naming and tree tidying. It turns each prompt you send to an agent into a short pane title, picks an icon for the row, names new sessions, and can reorganise the whole tree into groups that you undo in one step. Its id is `ilium.core.titles` and it runs on both sides: the server side asks the model and edits the tree, and the client side shows the preview panel. It needs a configured AI provider, which [Inference providers](/pages/core-plugin-inference/) supplies.

Ilium keeps one rule about names: a title that you typed by hand is never replaced by an inferred one. The host enforces it, so the plugin only has to submit and read the outcome.

### What it covers

- **Automatic pane titles and title style.** A submitted prompt yields a short title in the style you chose (Automatic titles, Title style in the user documentation).
- **Session naming.** New sessions get a name, and you can trigger a name by hand (Triggering titles manually).
- **Tree organisation.** The model proposes groups, and one undo step reverts them (Tree organization instructions; Tree organization in the privacy guide).
- **AI triggers.** Toggles decide which events start titling (the Triggers tab of Settings).
- **Custom instruction inputs.** Seven text inputs steer the titles (The seven instruction inputs).
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
│<span class="t-dim">  Each change is saved at once.</span>               │
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

`tree.restructure` is a high-risk grant: the user confirms it at install. Everything else is normal risk.

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
      "tree.restructure"
    ],
    "client": [
      "ui.panels",
      "settings.write-kernel:titles",
      "settings.write-kernel:triggers"
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

### Samples

#### 1. Automatic pane titles

On `input-submitted` the server asks [inference](/pages/api-inference/) for a title, with the [inference grant](/pages/capability-inference/) and the [prompt-reading grant](/pages/capability-agent-read-prompts/). The call returns a request id at once, so the plugin keeps the pair of request and pane, and on `request-completed` it submits `rename-node` with `source: inferred`. See [events](/pages/api-events/) and [intents](/pages/api-intents/).

**Rust**

```rust
use ilium_plugin_sdk::host::inference::{self, ChatMessage, CompletionRequest};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::RenameNodeSpec;

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

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.titles"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replace)).

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

### ABI used

- [inference](/pages/api-inference/) and [the inference grant](/pages/capability-inference/)
- [agent.read-prompts](/pages/capability-agent-read-prompts/)
- [events](/pages/api-events/) and [intents](/pages/api-intents/)
- [tree chrome intents](/pages/api-tree-chrome/)
- [tree.restructure](/pages/capability-tree-restructure/) and [tree structure intents](/pages/api-intent-tree-structure/)
- [text widgets](/pages/widgets-text/)
- [settings extension](/pages/settings-extension/) and [settings row types](/pages/reference-settings-rows/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Inference providers](/pages/core-plugin-inference/), which supplies the model
- [Contract overview](/pages/getting-started-overview/)
