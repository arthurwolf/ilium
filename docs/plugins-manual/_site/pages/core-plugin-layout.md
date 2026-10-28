# Layout

## Layout

The Layout plugin (`ilium.core.layout`, side both) owns split views and the arrangement of panes on screen: splitting four ways, mixed splits of terminal, editor and board panes, focus and navigation, and resizing with the mouse. The kernel keeps the layout solver. The plugin reads pane geometry, draws a layout preview panel and asks for splits and sizes with intents.

### What it covers

- Split views and layouts, four ways (Panes and layout, Split views).
- Mouse routing, hit targets, focus and navigation (Getting started, Using the mouse; Panes and layout, Focus and navigation).
- Board panes and editor panes in splits, so one screen can mix terminals, editors and boards (Panes and layout, Board panes and Split views).

### See it

<figure><img src="/assets/demos/03-mixed-splits.gif" alt="Two local fixture terminals, a text editor and a live clock share a four-pane split" loading="lazy"><figcaption>A four-pane split combines two labelled local terminal fixtures, a text editor and a live clock. Focus moves between panes.</figcaption></figure>

### What the user sees

<figure>
<pre class="term">╭─ api ──────────────────╮╭─ main.rs ────────────╮
│<span class="t-dim">$ cargo test</span>            ││<span class="t-hi">  1 fn main() {</span>       │
│<span class="t-ok">test result: ok</span>         ││<span class="t-dim">  2     run();</span>        │
│<span class="t-dim">$</span>                      ││<span class="t-dim">  3 }</span>                  │
╰────────────────────────╯╰──────────────────────╯
╭─ Board ────────────────╮╭─ web ────────────────╮
│<span class="t-dim">Todo   Doing   Done</span>     ││<span class="t-dim">$ bun run dev</span>         │
│<span class="t-dim">  3      1       8</span>       ││<span class="t-dim">ready on :5173</span>       │
╰────────────────────────╯╰──────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A four-way split of a terminal, an editor, a board and a second terminal. The focused pane is the editor and its frame is highlighted.</figcaption>
</figure>

<figure>
<pre class="term">╭─ Layout ─────────────────────────────────────╮
│ <span class="t-hi">▌api  ▐</span> ┃ <span class="t-dim">main.rs</span>                            │
│ <span class="t-hi">▌     ▐</span> ┃                                    │
│ <span class="t-dim">────────</span> ╋ <span class="t-dim">──────────</span>                        │
│ <span class="t-dim">Board</span>    ┃ <span class="t-dim">web</span>                               │
│ <span class="t-dim">80x24 | 61x24 | 80x12 | 61x12</span>                │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The layout preview panel: nested blocks with separators and a canvas thumbnail of the pane rectangles. The last row lists each pane size.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.layout",
  "name": "Layout",
  "version": "0.1.0",
  "publisher": "Ilium",
  "capabilities": {
    "server": ["panes.read", "panes.lifecycle", "panes.create", "tree.restructure"],
    "client": ["panes.read", "ui.panels"]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.layout",
  "name": "Layout",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "The Layout plugin (ilium.core.layout, side both) owns split views and the arrangement of panes on screen: splitting four ways, mixed splits of terminal, editor and board panes, focus and navigation, and resizing with the mouse.",
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
      "panes.read",
      "panes.lifecycle",
      "panes.create",
      "tree.restructure"
    ],
    "client": [
      "panes.read",
      "ui.panels"
    ]
  },
  "activation": [
    "command:layout.preset",
    "view:layout.presets"
  ],
  "limits_tier": "first-party"
}
```

The server side holds `panes.create` because the `create-pane` intent that the split samples submit is opened only by that grant ([panes.create](/pages/capability-panes-create/)). The two activation triggers name a command and a view that the plugin registers itself at run time through the command and view registries ([commands](/pages/api-commands/), [UI model](/pages/ui-model/)), not through `contributes`. The trigger `command:layout.preset` starts the plugin when that command is first run, and `view:layout.presets` when the presets panel is first shown. A trigger whose name the plugin never registers never fires, and the plugin then starts only at the next explicit use.

### Samples

#### Split four ways

A command splits the focused pane right, left, up or down. The client commands `Split right`, `Split left`, `Split up` and `Split down` send the focused pane to the server side, which submits `create-split` with a new terminal pane. Pages: [tree structure intents](/pages/api-intent-tree-structure/), [tree.restructure](/pages/capability-tree-restructure/), [commands](/pages/api-commands/), [events](/pages/api-events/) and [messages and topics](/pages/api-messages-and-topics/). The direction of a split is `horizontal` or `vertical`; left and up place the new pane first.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{CreatePaneSpec, CreateSplitSpec, MessageSpec, MessageTarget, PluginSide};
use ilium_plugin_sdk::types::tree::SplitDirection;

// Client side: Event::CommandInvoked with command "split-left" and so on.
fn forward(command: &str, focused: NodeId) -> Submission {
    Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget { plugin: "ilium.core.layout".into(), side: PluginSide::Server, client: None },
        channel: command.into(),
        payload: focused.to_le_bytes().to_vec(),
    }))
}

// Server side: create the pane first; its ok-node outcome carries the new id.
fn new_pane() -> Submission {
    Submission::new(Intent::CreatePane(CreatePaneSpec::kind("ilium.terminal")))
}

fn join(focused: NodeId, created: NodeId, channel: &str) -> Submission {
    let (panes, direction) = match channel {
        "split-right" => (vec![focused, created], SplitDirection::Vertical),
        "split-left" => (vec![created, focused], SplitDirection::Vertical),
        "split-down" => (vec![focused, created], SplitDirection::Horizontal),
        _ => (vec![created, focused], SplitDirection::Horizontal),
    };
    Submission::new(Intent::CreateSplit(CreateSplitSpec { panes, direction }))
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk/types';

// Client side: event.tag === 'command-invoked' with command 'split-left' and so on.
const forward = (command: string, focused: bigint): Submission => ({
  intent: { tag: 'message', val: {
    to: { plugin: 'ilium.core.layout', side: 'server', client: undefined },
    channel: command,
    payload: new Uint8Array(new BigUint64Array([focused]).buffer),
  } },
  correlation: 1n,
  dedupKey: undefined,
});

// Server side: create the pane first; its ok-node outcome carries the new id.
const newPane = (): Submission => ({
  intent: { tag: 'create-pane', val: {
    parent: undefined, title: 'shell', contentKind: 'ilium.terminal',
    cwd: undefined, command: undefined, initialInput: undefined,
  } },
  correlation: 2n,
  dedupKey: undefined,
});

const join = (focused: bigint, created: bigint, channel: string): Submission => {
  const vertical = channel === 'split-right' || channel === 'split-left';
  const after = channel === 'split-right' || channel === 'split-down';
  return {
    intent: { tag: 'create-split', val: {
      panes: after ? [focused, created] : [created, focused],
      direction: vertical ? 'vertical' : 'horizontal',
    } },
    correlation: 3n,
    dedupKey: undefined,
  };
};
```

#### Follow focus and size

The client keeps a map from pane to columns and rows from `pane-resized`. On `pane-focused` it reads `panes.info` and shows the title in its panel. Pages: [panes](/pages/api-panes/), [panes.read](/pages/capability-panes-read/) and [events](/pages/api-events/).

**Rust**

```rust
use std::collections::HashMap;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::panes;

#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
struct FocusTracker {
    sizes: HashMap<NodeId, (u16, u16)>,
    focused_title: String,
}

impl Lifecycle for FocusTracker {}

impl Handler for FocusTracker {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::PaneResized(resized) => {
                    self.sizes.insert(resized.node, (resized.columns, resized.rows));
                }
                Event::PaneFocused(Some(node)) => {
                    if let Some(info) = panes::info(node) {
                        self.focused_title = info.title;
                    }
                }
                _ => {}
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import * as panes from '@ilium/plugin-sdk/host/panes';
import type { Event, Submission } from '@ilium/plugin-sdk/types';

const sizes = new Map<bigint, [number, number]>();
let focusedTitle = '';

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'pane-resized') {
        sizes.set(event.val.node, [event.val.columns, event.val.rows]);
      } else if (event.tag === 'pane-focused' && event.val !== undefined) {
        focusedTitle = panes.info(event.val)?.title ?? focusedTitle;
      }
    }
    return [];
  },
  render() { /* see the preview panel below */ return tree(paragraph([[span(focusedTitle)]])); },
});
```

#### Layout preview panel

A client panel draws the current split tree as boxes. It renders nested `layout` and `split` widgets with `separator` rules for the live split tree and a `canvas` thumbnail of the pane rectangles. Pages: [layout and containers](/pages/widgets-layout-and-containers/), [UI model](/pages/ui-model/), [retained mode](/pages/retained-mode/), [ui.panels](/pages/capability-ui-panels/) and [data display](/pages/widgets-data-display/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::layout::{Area, Constraint, Direction};
use ilium_plugin_sdk::types::ui::{Shape, UiTree, ViewTarget};
use ilium_plugin_sdk::types::text::{Color, ColorRole};
use ilium_plugin_sdk::ui::build::Tree;

impl Views for FocusTracker {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let left = tree.paragraph(vec![vec![Span::plain("api")]]).key("left");
        let right = tree.paragraph(vec![vec![Span::plain("main.rs")]]).key("right");
        let panes = tree.split(Direction::Horizontal).key("preview").position(500).child(left).child(right);
        let rule = tree.separator(Direction::Horizontal).label("Pane sizes");
        let thumbnail = tree
            .canvas((0.0, 100.0), (0.0, 50.0))
            .shapes(vec![
                Shape::rectangle(0.0, 0.0, 50.0, 50.0, Color::role(ColorRole::Accent)),
                Shape::rectangle(50.0, 0.0, 50.0, 50.0, Color::role(ColorRole::Muted)),
            ])
            .key("thumbnail");
        let root = tree
            .layout(Direction::Vertical, [Constraint::Fill(1), Constraint::Length(1), Constraint::Length(6)])
            .child(panes)
            .child(rule)
            .child(thumbnail);
        tree.finish(root)
    }
}
```

**TypeScript**

```ts
import {
  canvas, fill, layout, length, paragraph, separator, span, split, tree,
} from '@ilium/plugin-sdk/ui/build';
import { role } from '@ilium/plugin-sdk/types';

function renderPreview() {
  const left = paragraph([[span('api')]], { key: 'left' });
  const right = paragraph([[span('main.rs')]], { key: 'right' });
  const panes = split({ direction: 'horizontal', key: 'preview', position: 500 }, [left, right]);
  const rule = separator({ direction: 'horizontal', label: 'Pane sizes' });
  const thumbnail = canvas([0, 100], [0, 50], {
    key: 'thumbnail',
    shapes: [
      { tag: 'rectangle', x: 0, y: 0, width: 50, height: 50, color: role('accent') },
      { tag: 'rectangle', x: 50, y: 0, width: 50, height: 50, color: role('muted') },
    ],
  });
  return tree(layout({ direction: 'vertical', constraints: [fill(1), length(1), length(6)] },
    [panes, rule, thumbnail]));
}
```

#### Drag the divider

Dragging the divider in the preview resizes the real panes. The client handles the `view-pointer` drag on the `split` divider, converts it to columns and messages the server side, which submits `resize-pane`. It pauses while `view-visibility` is false. `api-pointer` describes the `view-pointer` record. Pages: [pointer](/pages/api-pointer/), [pane lifecycle intents](/pages/api-intent-pane-lifecycle/), [panes.lifecycle](/pages/capability-panes-lifecycle/), [events](/pages/api-events/) and [messages and topics](/pages/api-messages-and-topics/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{MessageSpec, MessageTarget, PluginSide, ResizePaneSpec};

// Client side
fn on_drag(&mut self, divider_column: u16) -> Option<Submission> {
    if !self.visible { return None; } // set from Event::ViewVisibility
    let columns = divider_column.saturating_mul(self.columns_per_cell);
    Some(Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget { plugin: "ilium.core.layout".into(), side: PluginSide::Server, client: None },
        channel: "resize".into(),
        payload: [self.focused.to_le_bytes().to_vec(), columns.to_le_bytes().to_vec()].concat(),
    })))
}

// Server side: a size that a neighbour blocks is refused with `conflict`.
fn resize(pane: NodeId, columns: u16) -> Submission {
    Submission::new(Intent::ResizePane(ResizePaneSpec { pane, columns: Some(columns), rows: None }))
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk/types';

// Client side
function onDrag(dividerColumn: number): Submission | undefined {
  if (!visible) return undefined; // set from the 'view-visibility' event
  const columns = dividerColumn * columnsPerCell;
  const payload = new Uint8Array(10);
  new DataView(payload.buffer).setBigUint64(0, focused, true);
  new DataView(payload.buffer).setUint16(8, columns, true);
  return {
    intent: { tag: 'message', val: {
      to: { plugin: 'ilium.core.layout', side: 'server', client: undefined },
      channel: 'resize', payload,
    } },
    correlation: 4n,
    dedupKey: undefined,
  };
}

// Server side: a size that a neighbour blocks is refused with `conflict`.
const resize = (pane: bigint, columns: number): Submission => ({
  intent: { tag: 'resize-pane', val: { pane, columns, rows: undefined } },
  correlation: 5n,
  dedupKey: undefined,
});
```

### Extension points

Other plugins change what `ilium.core.layout` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.layout/<point>`). Contributors need `extensions.contribute:ilium.core.layout/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `layout-presets` | collect | Offer a named split layout in the layout chooser. | entry: id, title, template (a preset template string, grammar below) | standard |
| `before-split` | hook (veto) | Refuse a split, for example above a pane count the plugin allows. | node, direction, pane_count | standard |

**Preset template grammar.** A `template` is a string. `h(a, b, ...)` is a horizontal split (children side by side) and `v(a, b, ...)` is a vertical split (children stacked), each with two or more children and each child either a nested split or a pane-kind placeholder. A placeholder is a bare word naming a pane kind: `terminal`, `agent`, `editor` or `board`. Whitespace between tokens is ignored and names are lowercase. A template that does not parse, has a split with fewer than two children, or names an unknown kind is refused with `invalid-argument` when the contribution is validated at load, and the preset is not offered. Example: `h(agent, v(editor, terminal))` puts the agent pane left and an editor above a terminal on the right.

**Services** (grant `services.use:<name>`):

- `ilium.core.layout/presets`: list, apply: list the layout presets and apply one to a group

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.layout/changed`: { group, split_count } after a split is added, removed or resized

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.layout"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Adds a three-pane review layout to the layout chooser

A package adds this entry in its manifest. It needs no code.

```json
{
  "api": "ilium:plugin@0.1.0",
  "capabilities": { "server": ["extensions.contribute:ilium.core.layout/layout-presets"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.layout/layout-presets",
        "id": "review-trio",
        "priority": 50,
        "entry": {
          "title": "Review trio",
          "template": "h(agent, v(editor, terminal))"
        }
      }
    ]
  }
}
```

### ABI used

- [Panes](/pages/api-panes/), [panes.read](/pages/capability-panes-read/), [panes.lifecycle](/pages/capability-panes-lifecycle/), [Pane lifecycle intents](/pages/api-intent-pane-lifecycle/)
- [Tree structure intents](/pages/api-intent-tree-structure/), [tree.restructure](/pages/capability-tree-restructure/)
- [Layout and containers](/pages/widgets-layout-and-containers/), [Data display](/pages/widgets-data-display/), [UI model](/pages/ui-model/), [Retained mode](/pages/retained-mode/), [ui.panels](/pages/capability-ui-panels/)
- [Pointer](/pages/api-pointer/), [Events](/pages/api-events/), [Commands](/pages/api-commands/), [Messages and topics](/pages/api-messages-and-topics/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Tree plugin](/pages/core-plugin-tree/)
- [Terminal plugin](/pages/core-plugin-terminal/)
