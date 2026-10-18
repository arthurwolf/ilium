---
layout: page
title: Text
permalink: /pages/widgets-text/
---

## Text

Text widgets show words. Five widgets make up this family. `paragraph` wraps and aligns styled lines, and its spans can be clickable. `markdown` renders a Markdown source with Ilium's own renderer, the same one the help pages use. `leader-row` puts a label and a value on one line with dotted leaders. `key-hints` draws a line of key chords. `diff-view` shows a diff that the plugin supplies.

The plugin describes the text as data inside a UI tree. The host measures the text, wraps it, aligns it and paints it with the active theme. The plugin never computes a line break or a column position.

<figure>
<svg viewBox="0 0 420 210" role="img" aria-label="A plugin sends a ui-tree with text widgets, and the host lays them out and draws them">
  <title>Text widgets and the host</title>
  <desc>The plugin builds a ui-tree. The host lays out, wraps and draws five text widgets and sends ui-action events back for clicks.</desc>
  <defs>
    <marker id="tw-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="110" y="6" width="200" height="42" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="210" y="24" font-size="12" text-anchor="middle" fill="currentColor">Plugin (client side)</text>
  <text x="210" y="40" font-size="10" text-anchor="middle" fill="currentColor">builds a ui-tree in render</text>
  <line x1="160" y1="48" x2="160" y2="78" stroke="currentColor" marker-end="url(#tw-arrow)"/>
  <text x="168" y="66" font-size="10" fill="currentColor">ui-tree</text>
  <line x1="280" y1="78" x2="280" y2="48" stroke="currentColor" marker-end="url(#tw-arrow)"/>
  <text x="288" y="66" font-size="10" fill="currentColor">ui-action</text>
  <rect x="8" y="78" width="404" height="124" rx="8" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="18" y="96" font-size="11" fill="currentColor">Host lays out, wraps and draws the text</text>
  <rect x="14" y="106" width="74" height="28" rx="6" fill="none" stroke="currentColor"/>
  <text x="51" y="124" font-size="10" text-anchor="middle" fill="currentColor">paragraph</text>
  <rect x="94" y="106" width="74" height="28" rx="6" fill="none" stroke="currentColor"/>
  <text x="131" y="124" font-size="10" text-anchor="middle" fill="currentColor">markdown</text>
  <rect x="174" y="106" width="74" height="28" rx="6" fill="none" stroke="currentColor"/>
  <text x="211" y="124" font-size="10" text-anchor="middle" fill="currentColor">leader-row</text>
  <rect x="254" y="106" width="74" height="28" rx="6" fill="none" stroke="currentColor"/>
  <text x="291" y="124" font-size="10" text-anchor="middle" fill="currentColor">key-hints</text>
  <rect x="334" y="106" width="74" height="28" rx="6" fill="none" stroke="currentColor"/>
  <text x="371" y="124" font-size="10" text-anchor="middle" fill="currentColor">diff-view</text>
  <text x="20" y="158" font-size="10" fill="currentColor">A clickable span, a markdown link or task and a diff hunk send ui-action.</text>
  <text x="20" y="176" font-size="10" fill="currentColor">Key hints run commands on the host. Wrapping and leaders belong to the host.</text>
</svg>
<figcaption>The plugin describes the text, and the host wraps, aligns and draws it. A clickable span, a markdown link or task and a diff hunk send an action back.</figcaption>
</figure>

## You need

- The `ui.panels` grant on the client side, to show a view. A basic pane uses `panes.provide-kind` for its view instead. See [UI panels](/pages/capability-ui-panels/).
- The `commands.invoke` grant only when a link, span or key hint runs a command that the plugin does not own. A plugin's own commands need no grant. See [Commands invoke](/pages/capability-commands-invoke/).
- Rust with the `ilium-plugin-sdk` crate, or TypeScript with `@ilium/plugin-sdk`. Both build the tree with the same widget names.
- The [widget catalogue](/pages/widgets-index/) for keys and host-owned state.

Text values are `styled-text`: a list of spans, and each span holds a string and a style. A `styled-lines` value is a list of `styled-text` values, one per line. In Rust, `Span::plain("x")` builds an unstyled span and `Span::styled("x", Style::role(ColorRole::Warning))` builds a coloured one. In TypeScript, `span('x')` and `span('x', { role: 'warning' })` do the same.

A `rich-span` adds two fields to a span: a `target` that makes the span clickable, and a `tooltip`. A `rich-text` is a list of rich spans and a `rich-lines` is a list of those.

```wit
// Excerpt of ilium:types/text in reference-wit
record rich-span { text: string, style: style, target: span-target, tooltip: option<string> }
```

The `span-target` cases are `none`, `action(string)`, `command(string)` and `link(string)`. A link opens only through the open-external rules.

## Overview

| Widget | Use it for | Action sent to the plugin |
|---|---|---|
| `paragraph` | Several styled lines in a block, with clickable spans | `scrolled` when `scrollable`; `activated` for an `action` span |
| `markdown` | Help text and rich notes | `activated` for a link with no `link-command`; `toggled` with `task-click` |
| `leader-row` | A label and a read-only value | None |
| `key-hints` | One line of key chords | None. A chord runs its command on the host |
| `diff-view` | A diff that the plugin supplies | `selected` and `activated` with the hunk or line index |

Use a `toggle`, `choice` or `number` widget, not a `leader-row`, when the user must change a value. See [Input](/pages/widgets-input/).

## paragraph

`paragraph` mirrors `ratatui::widgets::Paragraph`. Each entry of `lines` is one line. Set `wrap` when long lines must break at the edge of the widget. Set `scrollable` when the text can be taller than the area. The host then owns the scroll offset.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `lines` | `styled-lines` | Required | The lines to show, one entry per line. |
| `rich` | `rich-lines` | none | Lines with clickable spans. When set it replaces `lines`. |
| `wrap` | `option<bool>` | `none` | `none` cuts long lines at the right edge. `some(false)` wraps. `some(true)` wraps and removes leading spaces of wrapped lines. |
| `alignment` | `left`, `center` or `right` | `left` | Horizontal alignment of every line. |
| `style` | `style` | Empty, so the theme applies | Default colours and modifiers. |
| `scrollable` | `bool` | `false` | Whether the host keeps a scroll offset for the widget. |

Keys: Up and Down scroll a scrollable paragraph. Mouse: the wheel scrolls it, and a click on a clickable span activates it. Roles: as styled; a clickable span uses `accent` and an underline.

<figure>
<pre class="term">╭─ Server ───────────────────────────────╮
│ Connected to the server.               │
│ Last sync 2026-10-11 14:02:07.         │
│ <span class="t-warn">2 plugins wait for a grant.</span>            │
│ Lines longer than the block wrap to    │
│ the next row.                          │
╰────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 42 columns. A <code>paragraph</code> with <code>wrap</code> set inside a block. The warning line uses the <code>warning</code> role.</figcaption>
</figure>

**Rust**

```rust
let status = tree
    .paragraph(vec![
        vec![Span::plain("Connected to the server.")],
        vec![Span::styled("2 plugins wait for a grant.", Style::role(ColorRole::Warning))],
    ])
    .key("status")
    .wrap(Some(true));
```

**TypeScript**

```ts
const status = paragraph([
  [span('Connected to the server.')],
  [span('2 plugins wait for a grant.', { role: 'warning' })],
], { key: 'status', wrap: true });
```

### Clickable spans

A span with a `target` becomes clickable. The host draws it in the accent colour with an underline and shows its `tooltip` after 500 ms. Each target behaves in one way.

| Target | A click does |
|---|---|
| `action(id)` | Sends `ui-action` with kind `activated` and the value `text(id)` to the paragraph's key. |
| `command(name)` | Runs the command through the command registry. The plugin receives `command-invoked` with the origin `text`. It needs `commands.invoke` for commands the plugin does not own. |
| `link(url)` | Opens the URL through the open-external rules. |
| `none` | Nothing. |

<figure>
<pre class="term">╭─ Advice ───────────────────────────────────╮
│ Threshold too low. <span class="t-hi">Apply 180k</span>              │
│ <span class="t-dim">Hover for details, click to apply.</span>         │
╰────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 46 columns. A <code>paragraph</code> with a clickable <code>rich-span</code>. The span is drawn in the accent colour and underlined (shown here as the highlight). A click sends <code>activated</code> with <code>text("apply-180k")</code>.</figcaption>
</figure>

**Rust**

```rust
// types::text::{RichSpan, SpanTarget}
let advice = vec![vec![
    RichSpan::plain("Threshold too low. "),
    RichSpan::new("Apply 180k", Style::default())
        .target(SpanTarget::Action("apply-180k".into()))
        .tooltip("Writes the recommended threshold and keeps a revert point"),
]];
let text = tree.paragraph(Vec::new()).rich(advice).key("advice");
```

**TypeScript**

```ts
import { paragraph, richSpan } from '@ilium/plugin-sdk/ui/build';

const text = paragraph([], {
  key: 'advice',
  rich: [[
    richSpan('Threshold too low. '),
    richSpan('Apply 180k', {
      target: { tag: 'action', val: 'apply-180k' },
      tooltip: 'Writes the recommended threshold and keeps a revert point',
    }),
  ]],
});
```

A click arrives as `ui-action { key: "advice", kind: activated, value: text("apply-180k") }`.

When `scrollable` is true, every change of the top row sends a `ui-action` of kind `scrolled` with `index(u32)`, the new top row offset. The host coalesces these events, so a fast wheel sends one event with the latest offset. The plugin reads the offset with `ui::widget_state(view, key)`, which returns `offset(n)`. It moves the view with `ui::set_widget_state(view, key, offset(n))`.

<div class="warn">
<p>A scrollable paragraph without a <code>key</code> loses its offset on every render. Give each scrollable widget a key that is unique across the whole view.</p>
</div>

## markdown

`markdown` mirrors the Ilium Markdown renderer that the help pages use. The host renders `source` with the shared theme, so headings, emphasis and links take their colours from the theme. The plugin never styles the Markdown itself.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `source` | `string` | Required | The Markdown text to render. |
| `link-command` | `option<string>` | `none` | The command that runs when the user activates a link. |
| `task-click` | `bool` | `false` | Task list items can be clicked. |
| `images` | `bool` | `false` | Images render as pictures instead of their alt text. |
| `base` | `option<fs-path>` | `none` | The folder against which relative image paths are resolved. |

Keys: Up and Down scroll, Tab walks the links and Enter follows one. Mouse: a click on a link or a task box. Roles: the theme's Markdown roles. Bullets are `•` and task boxes are `☐` and `☑`.

A link behaves in one of two ways.

- With `link-command` set, the host runs that command through the command registry on activation. It passes the arguments `{"target": "<link target>"}`. The command must be one of the plugin's own commands or covered by its `commands.invoke` grant. The plugin then receives `command-invoked`.
- Without `link-command`, the plugin receives a `ui-action` of kind `activated` whose value is `text(<link target>)`. The host never opens a browser itself.

With `task-click`, a click on a task item sends `toggled` with `text("<line>")`, the one-based line number of the item in the source text (the first line is `1`; lines are split at `\n`). The plugin rewrites its source, for example `- [ ]` to `- [x]`, and renders again. With `images`, an image written as `![alt](asset:path)`, or a file inside a granted `fs` scope, renders through the rules of the `image` widget. Any other image shows its alt text.

<figure>
<pre class="term">╭─ Help: keys ─────────────────────────────────────────╮
│ <span class="t-hi">Keys</span>                                                 │
│                                                      │
│ Ctrl+B is the prefix key.                            │
│ • Tab moves focus between panes                      │
│ ☑ Read the key list                                  │
│ ☐ Remap one key                                      │
│ See the <span class="t-hi">command list</span>.                                │
╰──────────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 56 columns. A <code>markdown</code> view with <code>task-click</code>. The heading and the link text use the theme. Bullets are <code>•</code> and task boxes are <code>☐</code> and <code>☑</code>.</figcaption>
</figure>

**Rust**

```rust
let notes = tree
    .markdown(self.source.clone())
    .key("notes")
    .task_click(true)
    .images(true);

// In on_events:
if let (ActionKind::Toggled, ActionValue::Text(line)) = (action.kind, action.value) {
    if let Ok(line_number) = line.parse::<usize>() {
        self.toggle_task(line_number);
        ui::invalidate(view.clone());
    }
}
```

**TypeScript**

```ts
const notes = markdown(source, { key: 'notes', taskClick: true, images: true });

// In onEvents:
if (action.key === 'notes' && action.kind === 'toggled' && action.value.tag === 'text') {
  toggleTask(Number(action.value.val));
  ui.invalidate(view);
}
```

The props table has no scroll setting for a source taller than the area, so the host scrolls it with the keys above.

## leader-row

`leader-row` mirrors the `leader_span` helper that every settings row uses. The label sits at the left, the value at the right, and grey dotted leaders (the `…` character) fill the gap between them. The rule has four parts.

- One blank cell is left on each side of the dots.
- A gap shorter than five cells stays blank, with no dots.
- Leaders are never part of a hit target.
- Leaders never overwrite label text, glyphs or the value.

| Prop | Type | Meaning |
|---|---|---|
| `label` | `styled-text` | The text at the left. |
| `value` | `styled-text` | The text at the right. |

Roles: `foreground` for the label, `muted` for the leaders, and the value as styled. The widget sends no actions and takes no keys.

<figure>
<pre class="term">╭─ Settings ───────────────────────────╮
│ Font size <span class="t-dim">……………………………………………………………</span> 14 │
│ Restore previous sessions on boot on │
│ Save every <span class="t-dim">………………………………………</span> 5 minutes │
╰──────────────────────────────────────╯
</pre>
<figcaption>Rendered at 40 columns. Three <code>leader-row</code> widgets. The first and third gaps are five cells or more, so they draw grey <code>…</code> leaders with one blank cell on each side. The second gap is shorter than five cells and stays blank.</figcaption>
</figure>

**Rust**

```rust
let font = tree
    .leader_row(vec![Span::plain("Font size")], vec![Span::plain("14")])
    .key("font-size");
```

**TypeScript**

```ts
const font = leaderRow([span('Font size')], [span('14')], { key: 'font-size' });
```

When the label and the value together are wider than the row, the label is cut at its right edge first. The value is cut only when it does not fit on its own.

## key-hints

`key-hints` mirrors the status-line hints of the Ilium client. Each hint is a record with three fields: `keys` (the chord text), `label` (the short description) and `command` (an optional command name). The host draws the chords in the accent colour and keeps the whole list on one line, joined by the separator ` · `.

```text
record key-hint { keys: string, label: string, command: option<string> }
```

Pressing a hint's chord while the view has focus runs its `command` through the command registry, and so does a click on the hint. The same grant rule as a markdown link applies. No `ui-action` is sent. A hint with no `command`, or with an empty string, is a label only, and the host draws it in the muted colour. Arrow keys appear as `↑ ↓ ← →` and the page keys as `Pg↑ Pg↓`.

| Prop | Type | Meaning |
|---|---|---|
| `hints` | `list<key-hint>` | The hints to show, in display order. |

Roles: `accent` for the keys and `muted` for the labels.

<figure>
<pre class="term"><span class="t-hi">Enter</span> open · <span class="t-hi">q</span> close · <span class="t-dim">? keys</span>

<span class="t-hi">Enter</span> open · <span class="t-hi">q</span> close · <span class="t-hi">/</span> filter · <span class="t-dim">… 3 more</span>
</pre>
<figcaption>Two <code>key-hints</code> lines. The chords use the <code>accent</code> role and the labels use <code>muted</code>. When the row is too narrow the host cuts the list and appends <code>… N more</code>, as in the second line. The last hint of the first line has no command and is a label only.</figcaption>
</figure>

**Rust**

```rust
let hints = tree
    .key_hints(vec![
        KeyHint::new("Enter", "open").command("my.plugin/open"),
        KeyHint::new("q", "close").command("my.plugin/close"),
        KeyHint::new("?", "keys"),
    ])
    .key("hints");
```

**TypeScript**

```ts
const hints = keyHints([
  { keys: 'Enter', label: 'open', command: 'my.plugin/open' },
  { keys: 'q', label: 'close', command: 'my.plugin/close' },
  { keys: '?', label: 'keys' },
], { key: 'hints' });
```

## diff-view

`diff-view` shows a diff that the plugin supplies. The plugin builds the hunks and the host colours the lines, highlights changed words and draws the tokens of a language.

```wit
// Excerpt of ilium:types/ui in reference-wit
record diff-hunk { header: string, lines: list<diff-line> }
record diff-line { kind: diff-line-kind, old-line: option<u32>, new-line: option<u32>, text: string }
enum diff-line-kind { added, removed, context }
```

| Prop | Type | Meaning |
|---|---|---|
| `hunks` | `list<diff-hunk>` | The hunks to show. |
| `layout` | `unified` or `split` | One column, or old and new side by side. |
| `language` | `option<string>` | The language whose tokens the host highlights. |
| `word-diff` | `bool` | Highlight the changed words inside a changed line. |

Actions: `selected` with `index(u32)` (the hunk) and `activated` with `index(u32)`. Keys: Up and Down move by line, `n` and `p` jump to the next and the previous hunk, and Enter activates. Mouse: a click on a line. Roles: `success` for added lines, `danger` for removed lines, and `muted` for context and the gutter. The gutter shows `+`, `-` or a blank.

<figure>
<pre class="term">╭─ Config diff ──────────────────────────────╮
│<span class="t-dim">@@ -11,3 +11,3 @@ [budget]</span>                  │
│<span class="t-dim"> 11  11   [budget]</span>                          │
│<span class="t-err"> 12     - threshold = 150000</span>                │
│<span class="t-ok">     12 + threshold = 180000</span>                │
│<span class="t-dim"> 13  13   unit = "tokens"</span>                   │
╰────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 46 columns. A unified <code>diff-view</code>. Added lines use <code>success</code>, removed lines use <code>danger</code>, and context and the gutter use <code>muted</code>. The gutter shows the old and new line numbers and the mark <code>+</code>, <code>-</code> or a blank.</figcaption>
</figure>

<figure>
<pre class="term">╭─ Config diff ───────────────────────────────────────╮
│<span class="t-dim">@@ -11,3 +11,3 @@                                    </span>│
│<span class="t-dim"> 11   [budget]            </span>│<span class="t-dim"> 11   [budget]            </span>│
│<span class="t-err"> 12 - threshold = 150000  </span>│<span class="t-ok"> 12 + threshold = 180000  </span>│
│<span class="t-dim"> 13   unit = "tokens"     </span>│<span class="t-dim"> 13   unit = "tokens"     </span>│
╰─────────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 55 columns. The same diff with <code>layout: split</code>. The old text is on the left and the new text on the right, with one <code>│</code> divider between them.</figcaption>
</figure>

**Rust**

```rust
// types::ui::{DiffHunk, DiffLine, DiffLineKind, DiffLayout}
let hunk = DiffHunk {
    header: "@@ -11,3 +11,3 @@ [budget]".into(),
    lines: vec![
        DiffLine::context(11, 11, "[budget]"),
        DiffLine::removed(12, "threshold = 150000"),
        DiffLine::added(12, "threshold = 180000"),
        DiffLine::context(13, 13, "unit = \"tokens\""),
    ],
};
let diff = tree
    .diff_view(vec![hunk])
    .layout(DiffLayout::Split)
    .language("toml")
    .word_diff(true)
    .key("diff");
```

**TypeScript**

```ts
import { diffView } from '@ilium/plugin-sdk/ui/build';

const hunk = {
  header: '@@ -11,3 +11,3 @@ [budget]',
  lines: [
    { kind: 'context', oldLine: 11, newLine: 11, text: '[budget]' },
    { kind: 'removed', oldLine: 12, text: 'threshold = 150000' },
    { kind: 'added', newLine: 12, text: 'threshold = 180000' },
    { kind: 'context', oldLine: 13, newLine: 13, text: 'unit = "tokens"' },
  ],
};
const diff = diffView([hunk], { key: 'diff', layout: 'split', language: 'toml', wordDiff: true });
```

## Worked example in Rust: a server status panel

A client view with a status paragraph, a clickable fix, a settings-style row and a hint line.

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::ui;
use ilium_plugin_sdk::types::layout::{Area, Constraint, Direction};
use ilium_plugin_sdk::types::text::{RichSpan, SpanTarget};
use ilium_plugin_sdk::types::ui::{ActionKind, ActionValue, UiTree, ViewTarget};
use ilium_plugin_sdk::ui::build::{KeyHint, Tree};

const PANEL: &str = "server-status";

#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
struct ServerStatus {
    threshold: u32,
}

impl Handler for ServerStatus {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::UiAction(action) = event else { continue };
            let (ActionKind::Activated, ActionValue::Text(id)) = (action.kind, action.value) else { continue };
            if action.key == "advice" && id == "apply-180k" {
                self.threshold = 180_000;
                ui::invalidate(ViewTarget::Panel(PANEL.to_string()));
            }
        }
        Vec::new()
    }
}

impl Views for ServerStatus {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let advice = vec![vec![
            RichSpan::plain("Threshold too low. "),
            RichSpan::new("Apply 180k", Style::default())
                .target(SpanTarget::Action("apply-180k".into()))
                .tooltip("Writes the recommended threshold"),
        ]];
        let text = tree.paragraph(Vec::new()).rich(advice).key("advice");
        let value = format!("{}", self.threshold);
        let row = tree.leader_row(vec![Span::plain("Threshold")], vec![Span::plain(value)]).key("threshold");
        let hints = tree
            .key_hints(vec![KeyHint::new("Enter", "apply").command("server-status/apply")])
            .key("hints");
        let body = tree
            .layout(Direction::Vertical, [Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)])
            .child(text)
            .child(row)
            .child(hints);
        let root = tree.block().title("Server").child(body);
        tree.finish(root)
    }
}
```

## Worked example in TypeScript: a help view with tasks

A help view that renders a Markdown checklist and rewrites the source when a task is clicked.

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import * as ui from '@ilium/plugin-sdk/host/ui';
import { block, keyHints, layout, markdown, tree } from '@ilium/plugin-sdk/ui/build';
import { fill, length } from '@ilium/plugin-sdk/ui/build';

const PANEL = 'help';
let source = ['# Keys', '', 'Ctrl+B is the prefix key.', '- [x] Read the key list', '- [ ] Remap one key'].join('\n');

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'ui-action') continue;
      const action = event.val;
      if (action.key !== 'help-text' || action.kind !== 'toggled' || action.value.tag !== 'text') continue;
      const lines = source.split('\n');
      const index = Number(action.value.val);
      lines[index] = lines[index].includes('[ ]')
        ? lines[index].replace('[ ]', '[x]')
        : lines[index].replace('[x]', '[ ]');
      source = lines.join('\n');
      ui.invalidate({ tag: 'panel', val: PANEL });
    }
    return [];
  },
  render() {
    const body = markdown(source, { key: 'help-text', taskClick: true });
    const hints = keyHints([{ keys: 'Esc', label: 'close' }], { key: 'hints' });
    const page = layout({ direction: 'vertical', constraints: [fill(1), length(1)] }, [body, hints]);
    return tree(block({ titles: [{ text: 'Help: keys', position: 'top', alignment: 'left' }] }, [page]));
  },
});
```

The `toggled` value is the one-based line number of the task item in the source text: the first line of the source is `"1"`. Convert it with `lines[Number(value) - 1]` when you index an array.

## Width and language

Every text widget lays out text by cell width, and a wide character takes two cells. A plugin MUST measure text with `display-width` and MUST NOT size a column by character count. [AUT-617] See [Internationalisation](/pages/guide-internationalisation/).

## Edge cases and errors

The text widgets return no error codes of their own. The errors below come from the state calls, the command registry and the tree validator.

| Situation | Result |
|---|---|
| Two widgets with the same `key` in one view | The tree is invalid and the instance traps. |
| A tree with more than 4,096 nodes, or a child index out of range | The tree is invalid and the instance traps. |
| A scrollable paragraph with no `key` | Its offset resets on every render. |
| A tooltip longer than 200 characters | Not allowed. The limit is 200 characters. |
| `set-widget-state` with an unknown key | `not-found`. |
| `set-widget-state` with `offset` on a `leader-row` | `invalid-argument`, because the state kind does not fit the widget. |
| A link, span or hint whose command the plugin may not invoke | `not-permitted`. The command does not run. |
| A hint with an empty `command` | A label only, drawn muted, with no action. |
| `rich` and `lines` both set on a paragraph | `rich` is used. |
| Wrap off, and a line wider than the area | The line is cut at the right edge. |
| A paragraph line below the area, and `scrollable` false | The line is not drawn. |
| A leader row with a gap under five cells | The gap is blank and no dots are drawn. |
| Key hints wider than the row | The list is cut and ends with `… N more`. |
| A markdown image outside a granted scope | The alt text shows. |

The limits of the whole tree are in [Errors and limits](/pages/reference-errors-and-limits/). Trap behaviour is in the [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/) pages.

## What the user sees

Text takes its colours from the active theme. A clickable span or link is accent coloured and underlined, so it reads as a target; a task box toggles under the pointer; key hints look the same as the hints in the status bar; and a diff is coloured red and green on a muted gutter, like the diff views of the client.

Requirement ids for this page: [WID-025]. See the [requirements index](/pages/reference-requirements/).

## See also

- [Widget catalogue](/pages/widgets-index/)
- [Layout and containers](/pages/widgets-layout-and-containers/)
- [Media and embedded](/pages/widgets-media-and-embedded/) for `code-view`, the text editor
- [Input](/pages/widgets-input/) for editable values
- [Commands invoke](/pages/capability-commands-invoke/)
- [The UI model](/pages/ui-model/)
- [Errors and limits](/pages/reference-errors-and-limits/)

API version: `ilium:plugin@0.1.0`.
