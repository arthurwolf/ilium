# Text triggers

## Text triggers

Text triggers are regular-expression rules that watch terminal output and send text to the pane that printed it. A typical rule answers a confirmation prompt such as `Continue? (y/n)` with `y`. The plugin id is `ilium.core.text-triggers` and it runs on both sides: the server side matches output and sends input, the client side draws the Text Triggers settings tab.

The pattern matching itself is done by the host filter, so the plugin never reads the full output stream. The plugin turns each rule into one output filter and answers each `output-matched` event with a `send-input` intent.

### What it covers

- Text triggers: rules with a pattern, a reply and an enabled switch, described in the Text triggers section of the automation guide.
- The Text Triggers settings tab, with one row group per rule.
- A notice for a pattern that does not compile, and a count of dropped matches when a pane floods the filter.

### See it

<figure><img src="/assets/demos/16-text-triggers.gif" alt="A terminal asks for confirmation and a text trigger types the answer" loading="lazy"><figcaption>A rule matches a confirmation prompt and the reply is typed into the same pane.</figcaption></figure>

### What the user sees

<figure>
<pre class="term">╭─ Text Triggers ────────────────────────────────╮
│ Confirm prompts …………………………………… <span class="t-ok">on</span>              │
│   Pattern ……………………………… <span class="t-hi">\(y/n\)\s*$</span>             │
│   Reply ………………………………………………………… y               │
│ Accept edits ………………………………………… <span class="t-dim">off</span>              │
│   Pattern ……………………………… <span class="t-hi">Apply changes\?</span>         │
│   Reply ………………………………………………………… yes             │
╰────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative: the toggle and group glyphs depend on the icon set. Each rule is a group of a pattern, a reply and an enabled toggle.</figcaption>
</figure>

<figure>
<pre class="term">╭─ Text Triggers ────────────────────────────────╮
│ <span class="t-warn">Pattern "(y/n" is not valid</span>                    │
│ <span class="t-dim">  unclosed group. The rule is switched off.</span>    │
│ Confirm prompts …………………………………… <span class="t-dim">off</span>             │
│   Pattern ……………………………… <span class="t-err">(y/n</span>                    │
╰────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A <code>notice</code> row warns about an invalid pattern.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

The contract version is 0.1.

```json
{
  "id": "ilium.core.text-triggers",
  "capabilities": {
    "server": ["terminal.output-filter", "terminal.write"],
    "client": ["ui.panels"]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.text-triggers",
  "name": "Text triggers",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "Text triggers are regular-expression rules that watch terminal output and send text to the pane that printed it.",
  "license": "MIT",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0"
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0"
  },
  "capabilities": {
    "server": [
      "terminal.output-filter",
      "terminal.write"
    ],
    "client": [
      "ui.panels",
      "settings.write-kernel:text-triggers"
    ]
  },
  "activation": [
    "startup",
    "view:text-triggers.rules"
  ],
  "replaceable": true,
  "surface": [
    "services:test-rule",
    "points:rule-packs",
    "points:before-fire",
    "topics:fired"
  ],
  "limits_tier": "first-party"
}
```

### Kernel tabs this plugin owns

This plugin is the single owner of the following built-in Settings tabs. It holds `settings.write-kernel:<tab>` for each one (T1 only, one holder per tab, see [kernel settings](/pages/api-kernel-settings-and-keymap/#writing-a-kernel-tab)). The values stay in the same `config.toml` tables as before, so no user setting moves. [COR-030] [COR-033]

| Tab id | Label | Config table | Readers |
|---|---|---|---|
| `text-triggers` | Text Triggers | `[text_triggers]` | none |

If the plugin is disabled, the tab stays in the rail with its stored values shown read-only and the notice "Text triggers is disabled. Values are kept." The kernel keeps reading the keys it consumes itself (listed in the Consumer column of the key table in [config and data migration](/pages/reference-config-and-data-migration/)). Behaviour that only this plugin provides is off. The exact result for this plugin is its row in [kernel and boot](/pages/reference-kernel-and-boot/). [COR-022]

### Samples

#### Declare the rule list

The rules live in a `group` row. Inside it, a `list` of strings with `item_format: "regex"` holds the patterns, which the host checks before writing, and a second `list` holds the replies in the same order. The panel draws each pattern and reply pair with an enabled `toggle`. See [settings extension](/pages/settings-extension/) and [settings row types](/pages/reference-settings-rows/).

**Rust**

```rust
// Manifest fragment: contributes.settings (a plain JSON string in the SDK).
const RULES_ROW: &str = r#"
{ "type": "group", "title": "Rules", "collapsed": false, "rows": [
  { "type": "list", "key": "patterns", "title": "Patterns",
    "item": { "type": "string" }, "item_format": "regex",
    "max_items": 32, "reorderable": true, "default": [] },
  { "type": "list", "key": "replies", "title": "Replies",
    "item": { "type": "string" }, "max_items": 32, "reorderable": true, "default": [] }
] }
"#;
```

**TypeScript**

```ts
// Manifest fragment: contributes.settings.
export const rulesRow = {
  type: 'group', title: 'Rules', collapsed: false, rows: [
    { type: 'list', key: 'patterns', title: 'Patterns',
      item: { type: 'string' }, item_format: 'regex',
      max_items: 32, reorderable: true, default: [] },
    { type: 'list', key: 'replies', title: 'Replies',
      item: { type: 'string' }, max_items: 32, reorderable: true, default: [] },
  ],
};
```

#### Match output and answer

Each rule becomes one output filter on every agent pane. A match raises `output-matched`, and the plugin answers with `send-input`. See [terminal.output-filter](/pages/capability-terminal-output-filter/), [events](/pages/api-events/) and [send-input](/pages/api-intent-send-input/). The dedup key stops a repeated line from answering twice.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::primitives::terminal::{add_output_filter, OutputFilter};
use ilium_plugin_sdk::types::events::OutputMatchedEvent;
use ilium_plugin_sdk::types::intents::{Delivery, SendInputSpec};

fn watch_pane(pane: NodeId, rule_id: &str, regex: &str) -> Result<(), PluginError> {
    add_output_filter(pane, OutputFilter {
        id: rule_id.to_string(),
        regex: regex.to_string(),
        rate_per_second: 2,
    })
}

fn answer(matched: &OutputMatchedEvent, reply: &str) -> Submission {
    let spec = SendInputSpec {
        pane: matched.node,
        text: reply.to_string(),
        delivery: Delivery::Submitted,
        prompt_epoch: None,
        stages: Vec::new(),
        raw: None,
        mouse: None,
        if_current: None,
        lock_user_input: false,
        journal: false,
    };
    let key = format!("trigger-{}-{}-{}", matched.filter_id, matched.node, matched.row);
    Submission::keyed(Intent::SendInput(spec), key)
}
```

**TypeScript**

```ts
import { addOutputFilter } from '@ilium/plugin-sdk/primitives/terminal';
import type { Submission } from '@ilium/plugin-sdk/types';

export function watchPane(pane: bigint, ruleId: string, regex: string): void {
  addOutputFilter(pane, { id: ruleId, regex, ratePerSecond: 2 });
}

export function answer(
  m: { node: bigint; filterId: string; row: number },
  reply: string,
): Submission {
  return {
    intent: { tag: 'send-input', val: {
      pane: m.node, text: reply, delivery: 'submitted', promptEpoch: undefined,
      stages: [], raw: undefined, mouse: undefined, ifCurrent: undefined,
      lockUserInput: false, journal: false,
    } },
    correlation: 1n,
    dedupKey: `trigger-${m.filterId}-${m.node}-${m.row}`,
  };
}
```

#### Count matches

Each rule keeps a match count in session storage and writes one `info` log line per match. See [storage](/pages/api-storage/) and [log](/pages/api-log/).

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::storage::{self, Bucket};

fn count_match(rule_id: &str, line: &str) {
    let key = format!("count/{rule_id}");
    let current = storage::get(Bucket::Session, &key)
        .and_then(|b| b.try_into().ok())
        .map(u64::from_le_bytes)
        .unwrap_or(0);
    let _ = storage::put(Bucket::Session, &key, &(current + 1).to_le_bytes());
    log::log(Level::Info, &format!("rule {rule_id} matched: {line}"));
}
```

**TypeScript**

```ts
import { log } from '@ilium/plugin-sdk/host/log';
import * as storage from '@ilium/plugin-sdk/host/storage';

export function countMatch(ruleId: string, line: string): void {
  const key = `count/${ruleId}`;
  const bytes = storage.get('session', key);
  const current = bytes && bytes.length === 8
    ? new DataView(bytes.buffer, bytes.byteOffset, 8).getBigUint64(0, true)
    : 0n;
  const next = new Uint8Array(8);
  new DataView(next.buffer).setBigUint64(0, current + 1n, true);
  storage.put('session', key, next);
  log('info', `rule ${ruleId} matched: ${line}`);
}
```

#### Floods and invalid patterns

When a pane prints the same prompt faster than the rate limit, the host drops matches and the next batch starts with `events-dropped`. The server logs the count. The client panel shows a `notice` for a pattern that does not compile and a `toggle` per rule. See [events and intents](/pages/events-and-intents/), [status and feedback widgets](/pages/widgets-status-and-feedback/), [events](/pages/api-events/) and [input widgets](/pages/widgets-input/).

**Rust**

```rust
// Server side, inside on_events:
if let Event::EventsDropped(counts) = event {
    for dropped in counts.iter().filter(|d| d.event == "output-matched") {
        log::log(Level::Warn, &format!(
            "{} matches dropped for rule {:?}", dropped.count, dropped.source));
    }
}

// Client side, inside Views::render:
fn rule_row(tree: &mut Tree, index: usize, enabled: bool, error: Option<&str>) -> NodeHandle {
    let toggle = tree.toggle("Enabled", enabled).key(&format!("rule-{index}"));
    match error {
        Some(text) => {
            let warn = tree.notice(vec![text.to_string()]).tone(Tone::Warning);
            tree.layout(Direction::Vertical, [Constraint::Length(2), Constraint::Length(1)])
                .child(warn)
                .child(toggle)
        }
        None => toggle,
    }
}
```

**TypeScript**

```ts
import { log } from '@ilium/plugin-sdk/host/log';
import { layout, length, notice, toggle } from '@ilium/plugin-sdk/ui/build';

// Server side, inside onEvents:
//   case 'events-dropped':
//     for (const d of event.val.filter((x) => x.event === 'output-matched'))
//       log('warn', `${d.count} matches dropped for rule ${d.source}`);

// Client side, inside views.render:
export function ruleRow(index: number, enabled: boolean, error?: string) {
  const switchRow = toggle('Enabled', enabled, { key: `rule-${index}` });
  if (error === undefined) return switchRow;
  const warn = notice([error], { tone: 'warning' });
  return layout({ direction: 'vertical', constraints: [length(2), length(1)] },
    [warn, switchRow]);
}
```

### Extension points

Other plugins change what `ilium.core.text-triggers` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.text-triggers/<point>`). Contributors need `extensions.contribute:ilium.core.text-triggers/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `rule-packs` | collect | Ship a group of rules the user can enable as a set. | entry: id, title, rules (list of pattern, reply, enabled) | high |
| `before-fire` | hook (veto) | Refuse or end a rule that is about to send text. | rule, pane, match, reply_length | high |

**Services** (grant `services.use:<name>`):

- `ilium.core.text-triggers/test-rule`: test: run a pattern against sample text

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.text-triggers/fired`: { rule, pane }

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.text-triggers"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Refuses rules marked `office` outside 09:00 to 18:00

A package adds a `veto` handler. Handlers run until one refuses.

```json
{
  "api": "ilium:plugin@0.1.0",
  "exports": [
    "hook-handler"
  ],
  "capabilities": { "server": ["extensions.contribute:ilium.core.text-triggers/before-fire"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.text-triggers/before-fire",
        "id": "working-hours",
        "priority": 50,
        "handler": "working_hours"
      }
    ]
  }
}
```

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;

impl HookHandler for Plugin {
    fn handle_hook(&mut self, call: HookCall) -> HookReply {
        match call.extension.as_str() {
            "working_hours" => {
                // call.payload_json holds: rule, pane, match, reply_length
                let _payload: serde_json::Value = match serde_json::from_str(&call.payload_json) {
                    Ok(value) => value,
                    Err(_) => return HookReply::Pass,
                };
                HookReply::Refuse("reason".into())
            }
            _ => HookReply::Pass,
        }
    }
}
```

**TypeScript**

```ts
definePlugin({
  handleHook(call) {
    if (call.extension !== 'working_hours') return { tag: 'pass' };
    // call.payloadJson holds: rule, pane, match, reply_length
    const payload = JSON.parse(call.payloadJson);
    void payload;
    return { tag: 'refuse', val: 'reason' };
  },
});
```

A handler that is late, traps or returns a reply the mode does not allow counts as `pass`. See [Extension points](/pages/api-extension-points/#hooks).

### ABI used

- [Settings extension](/pages/settings-extension/) and [settings row types](/pages/reference-settings-rows/)
- [terminal.output-filter](/pages/capability-terminal-output-filter/)
- [Events](/pages/api-events/) and [events and intents](/pages/events-and-intents/)
- [send-input intent](/pages/api-intent-send-input/)
- [Storage](/pages/api-storage/) and [log](/pages/api-log/)
- [Status and feedback widgets](/pages/widgets-status-and-feedback/) and [input widgets](/pages/widgets-input/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Scheduler](/pages/core-plugin-scheduler/), which also sends text to panes

### Security

Risk marks of this plugin's extension points follow the corrected table in [Extension security](/pages/reference-extension-security/) ([SEC-293]).
