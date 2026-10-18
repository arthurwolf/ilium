---
layout: page
title: Status and feedback
permalink: /pages/widgets-status-and-feedback/
---

## Status and feedback

Status widgets tell the user that something is happening, that something changed, or that something needs attention. The family has four widgets: `badge`, `spinner`, `notice` and `icon`. They draw small, quiet signs and never take focus unless they have an action.

| Widget | Shows | Event |
|---|---|---|
| `badge` | A dot, a label or a count | `activated` when it has a `key` (a keyed badge needs `flags.focusable` to take focus) |
| `spinner` | A moving frame set with an optional label | none |
| `notice` | An inline banner with a tone, text and actions | `activated`, `closed` |
| `icon` | One glyph from a slot or a literal | none |

### You will need

- A view that can hold the widget. All four work in panels, popups, dialogs, pane views and settings pages. See [UI model](/pages/ui-model/).
- For the look in the tree, tabs and toolbars: a badge in a tree row is a decoration, see [Tree decorations](/pages/capability-tree-decorate/).
- A short-lived message that is not part of a view is a toast, see [Toasts](/pages/api-toast/).

### Tone

A tone is one of `info`, `success`, `warning`, `danger` or `muted`. It sets the colour of a `badge` and the colour, icon and border of a `notice`. The tones map to theme roles, so a plugin never picks a colour.

| Tone | Notice icon | Stable glyph | Theme role |
|---|---|---|---|
| `info` | `ℹ` | `i` | `info` (token of `theme-extras`) |
| `success` | `✓` | `+` | `success` |
| `warning` | `!` | `!` | `warning` |
| `danger` | `✗` | `x` | `danger` |
| `muted` | none | none | `muted` |

#### badge

**Purpose.** A small sign next to a name: an unread dot, a count, a short label. A dot is the smallest form. A count above 99 is drawn `99+`. A label is at most 12 cells. A badge with a `key` and `flags.focusable` can take focus and send `activated`.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `label` | `option<string>` | none | Text of up to 12 cells, drawn in brackets. |
| `count` | `option<u32>` | none | A number. Above 99 it shows `99+`. |
| `dot` | `bool` | `false` | Draws `●` and ignores `label` and `count`. |
| `tone` | `tone` | `info` | The colour. |

| Event | Value | Sent when |
|---|---|---|
| `activated` | `none` | Enter or a click, when the badge has a `key`. |

| Input | Behaviour |
|---|---|
| Enter | Activates a badge that has a `key`. |
| Left click | Focuses and activates. |

Theme roles: the `tone` role.

<figure>
<pre class="term">╭  ≡ ● · Badges────────────────────────────────────────────────╮
│ Unread <span class="t-dim">……………………………………………………………</span> <span class="t-hi">●</span>                             │
│ Messages <span class="t-dim">………………………………………………………</span> <span class="t-ok">[ 3 ]</span>                         │
│ Errors <span class="t-dim">……………………………………………………………</span> <span class="t-err">[ 99+ ]</span>                       │
│ Mode <span class="t-dim">………………………………………………………………</span> [ new ]                        │
│ Tabs: Agents <span class="t-warn">●</span> │ Notes │ Log <span class="t-ok">[ 2 ]</span>                           │
╰──────────────────────────────────────────────────────────────╯
</pre>
<figcaption>Badges. A dot (first row) is the smallest form. A count is drawn in brackets and shows 99+ above 99. A label of up to 12 cells is drawn the same way. The tone sets the colour: info, success, warning, danger or muted.</figcaption>
</figure>

**Rust**

```rust
let unread = tree.badge().dot(true).tone(Tone::Info);
let errors = tree.badge().count(142).tone(Tone::Danger).key("errors");
let mode = tree.badge().label("new");
// ("errors", ActionKind::Activated, _) => self.show_errors(),
```

**TypeScript**

```ts
const unread = badge({ dot: true, tone: 'info' });
const errors = badge({ count: 142, tone: 'danger', key: 'errors' });
const mode = badge({ label: 'new' });
// if (key === 'errors' && kind === 'activated') showErrors();
```

#### spinner

**Purpose.** Shows that work is running. The host steps the frames and the plugin makes no calls while it spins. Reduced motion shows one static frame `⠿`, so the spinner stays readable and costs nothing.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `frames` | `spinner-frames` | `braille` | `braille`, `blocks`, `moons`, `quarters`, or `custom(list<string>)` of 2 to 16 frames. |
| `interval-ms` | `u16` | `100` | Time per frame, 80 to 1,000. |
| `label` | `option<styled-text>` | none | Text after the frame. |

A spinner has no event, no keyboard input and no mouse input. Theme role: `accent`.

<figure>
<pre class="term">╭  ≡ ● · Spinner frame sets────────────────────────────────────╮
│ braille    ⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏                               │
│ blocks     ▁ ▂ ▃ ▄ ▅ ▆ ▇ █                                   │
│ moons      🌑 🌒 🌓 🌔 🌕                                    │
│ quarters   ○ ◔ ◑ ◕ ●                                         │
│ reduced    ⠿                                                 │
│                                                              │
│ <span class="t-hi">⠹</span> Indexing 412 sessions                                      │
╰──────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The four built-in frame sets of the spinner, each shown frame by frame, and the single static frame ⠿ that appears when the user prefers reduced motion. The last line shows the spinner in use, frozen on its third frame, followed by its label.</figcaption>
</figure>

**Rust**

```rust
let busy = tree.spinner().frames(SpinnerFrames::Braille).label("Indexing 412 sessions");
let custom = tree.spinner().frames(SpinnerFrames::Custom(vec!["-".into(), "\\".into(), "|".into(), "/".into()]));
```

**TypeScript**

```ts
const busy = spinner({ frames: { tag: 'braille' }, label: 'Indexing 412 sessions' });
const custom = spinner({ frames: { tag: 'custom', val: ['-', '\\', '|', '/'] }, intervalMs: 120 });
```

#### notice

**Purpose.** An inline banner inside a view. It has a tone, an optional title, rich text, up to three action buttons and an optional dismiss `✗`. The `notice` row of the Settings screen is a reduced form of this widget: it has only `id` (required), `text`, `tone` and `dismissible`, so it has no title, no actions and no `muted` tone. A dismissal is stored per `id` when the notice is a settings row.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `tone` | `tone` | `info` | The colour and the icon. |
| `title` | `option<string>` | none | Bold first line next to the icon. |
| `text` | `rich-lines` | required | The body. Spans can be styled or clickable. |
| `actions` | `list<tuple<string, string>>` | empty | Pairs of id and label. At most 3. |
| `dismissible` | `bool` | `false` | Shows `✗` at the top right. |

| Event | Value | Sent when |
|---|---|---|
| `activated` | `text(action id)` | An action button is pressed. |
| `closed` | `none` | The user dismisses the notice. |

| Input | Behaviour |
|---|---|
| Tab | Moves between the actions. |
| Enter | Presses the focused action. |
| Escape | Dismisses, when `dismissible`. |
| Left click on an action or on `✗` | Presses, dismisses. |

Theme roles: the `tone` role for the icon, the title and the `border`.

<figure>
<pre class="term">╭  ≡ ● · Notices─────────────────────────────────────────────────╮
│ <span class="t-info">╭────────────────────────────────────────────────────────────╮</span> │
│ <span class="t-info">│</span> <span class="t-info">ℹ Update available</span>                                         <span class="t-info">│</span> │
│ <span class="t-info">│</span> Version 0.9.4 is ready.                                    <span class="t-info">│</span> │
│ <span class="t-info">╰────────────────────────────────────────────────────────────╯</span> │
│ <span class="t-ok">╭────────────────────────────────────────────────────────────╮</span> │
│ <span class="t-ok">│</span> <span class="t-ok">✓ Saved</span>                                                    <span class="t-ok">│</span> │
│ <span class="t-ok">│</span> The settings were written.                                 <span class="t-ok">│</span> │
│ <span class="t-ok">╰────────────────────────────────────────────────────────────╯</span> │
│ <span class="t-warn">╭────────────────────────────────────────────────────────────╮</span> │
│ <span class="t-warn">│</span> <span class="t-warn">! Privacy</span>                                                  <span class="t-warn">│</span> │
│ <span class="t-warn">│</span> The provider sends place names to photon.komoot.io.        <span class="t-warn">│</span> │
│ <span class="t-warn">╰────────────────────────────────────────────────────────────╯</span> │
│ <span class="t-err">╭────────────────────────────────────────────────────────────╮</span> │
│ <span class="t-err">│</span> <span class="t-err">✗ Failed</span>                                                   <span class="t-err">│</span> │
│ <span class="t-err">│</span> The key was rejected by the server.                        <span class="t-err">│</span> │
│ <span class="t-err">╰────────────────────────────────────────────────────────────╯</span> │
╰────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>Four notices in the info, success, warning and danger tones, with the tone icons ℹ ✓ ! ✗ and a border in the tone colour. The title takes the tone colour, the text is plain.</figcaption>
</figure>

<figure>
<pre class="term">╭  ≡ ● · Notice with actions─────────────────────────────────────╮
│ <span class="t-warn">╭────────────────────────────────────────────────────────────╮</span> │
│ <span class="t-warn">│</span> <span class="t-warn">! Privacy</span>                                               <span class="t-warn">✗</span>  <span class="t-warn">│</span> │
│ <span class="t-warn">│</span> The provider sends place names to photon.komoot.io.        <span class="t-warn">│</span> │
│ <span class="t-warn">│</span> <span class="t-hi">[ Understood ]</span>  [ Open settings ]                          <span class="t-warn">│</span> │
│ <span class="t-warn">╰────────────────────────────────────────────────────────────╯</span> │
╰────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>A dismissible notice with two actions. The ✗ at the top right closes it and sends closed; each action button sends activated with its id. The first action has focus.</figcaption>
</figure>

**Rust**

```rust
let notice = tree.notice(vec!["The provider sends place names to photon.komoot.io.".into()])
    .tone(Tone::Warning).title("Privacy").dismissible(true).key("privacy")
    .actions(vec![("ok".into(), "Understood".into()), ("settings".into(), "Open settings".into())]);
// ("privacy", ActionKind::Activated, ActionValue::Text(id)) => self.on_privacy(&id),
// ("privacy", ActionKind::Closed, _) => self.privacy_hidden = true,
```

**TypeScript**

```ts
const privacyNotice = notice(['The provider sends place names to photon.komoot.io.'], {
  key: 'privacy', tone: 'warning', title: 'Privacy', dismissible: true,
  actions: [['ok', 'Understood'], ['settings', 'Open settings']],
});
// if (kind === 'activated' && value.tag === 'text') onPrivacy(value.val);
// if (kind === 'closed') state.privacyHidden = true;
```

#### icon

**Purpose.** One glyph. It takes a slot name from the user's icon preset, or a literal glyph. A slot follows the preset, so the same slot can show an emoji or a Nerd Font glyph. The host knows the cell width of the glyph and reserves it, so a width-2 emoji does not push the next widget.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `name` | `string` | required | A slot such as `model.claude`, or a literal glyph. |
| `tone` | `option<tone>` | none | Colours the glyph. |

An icon has no event, no keyboard input and no mouse input. Theme role: the `tone` role, or `foreground`.

<figure>
<pre class="term">╭  ≡ ● · Icons─────────────────────────────────────────────────╮
│ Slot model.claude <span class="t-dim">……………………</span> 🦀  Claude                        │
│ Slot model.codex <span class="t-dim">………………………</span> 🐢  Codex                         │
│ Slot status.done <span class="t-dim">………………………</span> 🔔  Done                          │
│ Slot status.idle <span class="t-dim">………………………</span> ●  Idle                           │
│ Glyph "★" <span class="t-dim">………………………………………</span> ★  Bookmark                        │
╰──────────────────────────────────────────────────────────────╯
</pre>
<figcaption>The icon widget. A slot glyph follows the user icon preset, so the same slot can show an emoji or a Nerd Font glyph. Widths follow the host icon width rules: the emoji take two cells and the label starts after them.</figcaption>
</figure>

**Rust**

```rust
let claude = tree.icon("model.claude");
let star = tree.icon("★").tone(Tone::Warning);
```

**TypeScript**

```ts
const claude = icon('model.claude');
const star = icon('★', { tone: 'warning' });
```

### Worked example: a progress notice

The plugin shows a spinner while it indexes and a notice when it ends. The failure case has a retry action.

**Rust**

```rust
impl Views for Indexer {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let body = match &self.state {
            State::Running(count) => tree.spinner().label(format!("Indexing {count} sessions")).into_node(),
            State::Failed(reason) => tree.notice(vec![reason.clone()]).tone(Tone::Danger)
                .title("Indexing failed").actions(vec![("retry".into(), "Retry".into())]).key("failed").into_node(),
            State::Done(count) => tree.notice(vec![format!("{count} sessions indexed")])
                .tone(Tone::Success).dismissible(true).key("done").into_node(),
        };
        tree.finish(body)
    }
}
// ("failed", ActionKind::Activated, ActionValue::Text(id)) if id == "retry" => self.start(),
```

**TypeScript**

```ts
render() {
  const body =
    state.kind === 'running' ? spinner({ label: `Indexing ${state.count} sessions` })
    : state.kind === 'failed'
      ? notice([state.reason], { key: 'failed', tone: 'danger', title: 'Indexing failed', actions: [['retry', 'Retry']] })
      : notice([`${state.count} sessions indexed`], { key: 'done', tone: 'success', dismissible: true });
  return tree(body);
}
// if (key === 'failed' && kind === 'activated' && value.val === 'retry') start();
```

### Edge cases and errors

| Case | Result |
|---|---|
| A badge label longer than 12 cells | Truncated with `…`. |
| A notice with more than 3 actions | The render traps the instance with `ui.invalid-tree` (render has no result; see [UI model](/pages/ui-model/)). |
| A spinner with fewer than 2 or more than 16 custom frames | The render traps the instance with `ui.invalid-tree`. |
| A spinner `interval-ms` outside 80 to 1,000 | Clamped. |
| Reduced motion | A spinner shows `⠿`. A tone keeps its colour. |
| An icon slot that does not exist | The slot name is drawn as text. |

### See also

- [Input](/pages/widgets-input/)
- [Collections](/pages/widgets-collections/)
- [Data display](/pages/widgets-data-display/)
- [Widget catalogue](/pages/widgets-index/)
- [Toasts](/pages/api-toast/)
- [Tree decorations](/pages/capability-tree-decorate/)
- [Accessibility and theming](/pages/accessibility-and-theming/)
