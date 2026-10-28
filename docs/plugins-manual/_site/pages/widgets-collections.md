# Collections

## Collections

Collections draw many items and let the user pick, toggle or edit them. The family has six widgets: `%list`, `table`, `tree-list`, `menu`, `check-list` and `list-editor`. The host owns the state of every collection: selection, scroll offset, expansion, filter text and the text being edited. The plugin sends items and hears about changes.

| Widget | Shows | Main event | Value |
|---|---|---|---|
| `%list` | rows of styled lines | `selected`, `activated` | `index`, or `ids` with `multi-select` |
| `table` | rows and columns with a header | `selected`, `activated` | `index`, `ids`, or `text(column id)` for a header click |
| `tree-list` | an expandable tree | `selected`, `activated`, `toggled` | `text(item id)` |
| `menu` | commands with submenus | `activated`, `closed` | `text(item id)` |
| `check-list` | boxes with a tri-state | `changed` | `item-state(id, state)` |
| `list-editor` | an editable list of strings or objects | `changed`, `selected` | `text(json array)`, `index` |

### You will need

- A view that can hold the widget: a panel, popup, dialog, pane view or settings page. A `menu` is refused in a settings page, and so is `list-editor` (use a `list` row with `schema-form` there). See [UI model](/pages/ui-model/).
- The `ui.panels` grant for panels and popups. See [ui.panels](/pages/capability-ui-panels/).
- Items with stable ids. A collection that sends `ids` or `text(id)` needs an `id` on every item.

### Host-owned state

| Widget | State the host keeps | Read with `widget-state` | Set with `set-widget-state` |
|---|---|---|---|
| `%list` | selected index, scroll offset, filter text, multi-selection | `selected`, `offset`, `text`, `ids` | `selected`, `offset`, `text`, `ids` |
| `table` | selected row, scroll offset, sort, multi-selection | `selected`, `offset`, `ids` | `selected`, `offset`, `ids` |
| `tree-list` | selected item, expansion, scroll offset | `selected`, `ids` | `selected`, `expanded`, `ids` |
| `check-list` | the state of each item | `tri` | `tri` |
| `list-editor` | the item being edited and its draft text | `selected` | `selected` |

When the plugin renders a new tree, the host keeps this state by key. A selection of an item that is no longer present moves to the nearest row. A widget without a `key` loses its state on every render.

### Props every collection accepts

`key`, `tooltip`, `disabled-reason`, `tab-index` and `hover-events` work as for every input widget, see [Input](/pages/widgets-input/). `item-tooltip` on `%list` shows each item's `tooltip`.

#### %list

**Purpose.** A scrolling list of items, each made of one or more styled lines. It is the list of the Agents panel and of the choice dialog. The selected row carries the highlight symbol. The filter row narrows the list while the user types, and `follow-tail` keeps the newest row visible for logs.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `items` | `list<list-item>` | required | The rows. Item fields: `lines`, `style`, `id`, `icon`, `trailing`, `tooltip`, `disabled-reason`. |
| `highlight-symbol` | `string` | `▶ ` | Drawn before the selected row (`> ` in stable glyph mode). |
| `highlight-style` | `style` | selection | The style of the selected row. |
| `highlight-spacing` | `highlight-spacing` | `when-selected` | `always`, `when-selected` or `never`. |
| `repeat-highlight-symbol` | `bool` | `false` | Draws the symbol on each line of a multi-line item. |
| `direction` | `list-direction` | `top-to-bottom` | Or `bottom-to-top`. |
| `filter` | `bool` | `false` | Shows the `Filter:` row above the list. |
| `filter-placeholder` | `option<string>` | none | Hint text of the filter row. |
| `multi-select` | `bool` | `false` | Space toggles the row. The value becomes `ids`. |
| `follow-tail` | `bool` | `false` | Keeps the last item visible until the user scrolls up. |
| `empty` | `option<styled-lines>` | none | Shown when there are no items. |
| `item-tooltip` | `bool` | `false` | Shows each item's tooltip on hover. |

| Event | Value | Sent when |
|---|---|---|
| `selected` | `index(u32)` | The highlight moves. |
| `activated` | `index(u32)` | Enter or double click. |
| `changed` | `ids(list<string>)` | The multi-selection changes. |
| `filter-changed` | `text(string)` | The filter text changes, after 150 ms. |

| Input | Behaviour |
|---|---|
| Up, Down, PageUp, PageDown, Home, End | Move the highlight. A disabled item is skipped. |
| Enter | Activates the selected item. |
| Space | Toggles the item in `multi-select`. |
| Typing | Filters, when `filter` is true. |
| Left click, double click | Select, activate. |
| Mouse wheel | Scrolls three rows per notch. |

Theme roles: `selection` for the highlighted row, `accent` for the highlight symbol, `muted` for disabled items and trailing text.

<figure>
<pre class="term">╭  ≡ ● · Agents────────────────────────────────────────────────╮
│ Filter: co_                                                  │
│   Claude Code                                           idle │
│ <span class="t-hi">▶ Codex</span>                                              <span class="t-hi">working</span> │
│ <span class="t-dim">  Gemini</span>                                         <span class="t-dim">unavailable</span> │
│   Aider                                    awaiting approval │
│                                                              │
╰──────────────────────────────────────────────────────────────╯
</pre>
<figcaption>A %list with a filter row above it. The highlight symbol is ▶ by default (> in stable glyph mode) and the selected row is drawn in the accent colours. The third item is disabled and shows its reason as the trailing text and as a tooltip.</figcaption>
</figure>

<figure>
<pre class="term">╭  ≡ ● · Search results────────────────────────╮
│                                              │
│ <span class="t-dim">No matching sessions</span>                         │
│                                              │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>The empty prop replaces a list with no items. The text is plain styled lines.</figcaption>
</figure>

**Rust**

```rust
let rows = self.agents.iter().map(|agent| {
    ListItem::new(vec![agent.name.clone()]).id(&agent.id).trailing(&agent.state)
        .disabled_reason_opt(agent.unavailable.clone())
}).collect::<Vec<_>>();
let list = tree.list(rows).key("agents").filter(true).filter_placeholder("Filter…")
    .empty(vec!["No matching sessions".into()]);
// ("agents", ActionKind::Activated, ActionValue::Index(i)) => self.open(i),
```

**TypeScript**

```ts
const rows = state.agents.map((agent) => ({
  id: agent.id, lines: [agent.name], trailing: agent.state, disabledReason: agent.unavailable,
}));
const agents = list(rows, { key: 'agents', filter: true, filterPlaceholder: 'Filter…', empty: ['No matching sessions'] });
// if (kind === 'activated' && value.tag === 'index') open(value.val);
```

#### table

**Purpose.** Rows and columns with a header. Columns have an id, a title, a width, a minimum width and a priority. When the area is too narrow the host drops the columns with the lowest priority first and never shrinks one below its minimum width. A sortable column shows `▲` or `▼`.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `columns` | `list<table-column>` | required | `id`, `title`, `width` (a constraint), `min-width`, `priority`, `align`, `sortable`. |
| `rows` | `list<table-row>` | required | `cells`, `height`, `style`, and an optional `id`. |
| `sort` | `option<tuple<string, bool>>` | none | The sort column id and whether it is ascending. |
| `multi-select` | `bool` | `false` | Space toggles a row. |
| `follow-tail` | `bool` | `false` | Keeps the last row visible. |
| `column-spacing` | `u16` | `2` | Cells between columns. |
| `highlight-symbol` | `string` | `▶ ` | Before the selected row. |
| `highlight-spacing` | `highlight-spacing` | `when-selected` | As for `%list`. |
| `widths`, `header` | | | Prefer `columns`. |

| Event | Value | Sent when |
|---|---|---|
| `selected` | `index(u32)` | The highlight moves. |
| `activated` | `index(u32)` | Enter or double click. |
| `activated` | `text(column id)` | A click on a sortable header. The plugin sorts and sets `sort`. |
| `changed` | `ids(list<string>)` | The multi-selection changes. |

| Input | Behaviour |
|---|---|
| Up, Down, PageUp, PageDown, Home, End | Move the highlight. |
| Left, Right | Scroll hidden columns. |
| Enter | Activates the row. |
| Left click on a row, on a header | Select, sort. |

Theme roles: `selection`, `muted` for the header and the rule, `border` for the rule.

<figure>
<pre class="term">╭  ≡ ● · Plugins───────────────────────────────────────────────╮
│ <span class="t-dim">  Name              Version  State        Size ▼</span>             │
│ <span class="t-dim">  ────────────────────────────────────────────────────</span>       │
│   demo-plugin       0.1.0    enabled      1.2 MiB            │
│ <span class="t-hi">  chatroom          0.4.2    enabled      812 KiB</span>            │
│   progress-hooks    0.1.1    enabled      96 KiB             │
│   remote-sound      0.1.0    <span class="t-dim">disabled</span>     2.4 MiB            │
╰──────────────────────────────────────────────────────────────╯
</pre>
<figcaption>A sortable table. The header is bold in the real client; here it is dimmed. ▼ marks the sort column and its direction (▲ ascending). Rows are plain lines, the selected row uses the accent colours. In narrower areas the Size column goes first, then the others by priority.</figcaption>
</figure>

**Rust**

```rust
let columns = vec![
    TableColumn::new("name", "Name").width(Constraint::Length(18)).priority(9),
    TableColumn::new("state", "State").width(Constraint::Length(10)).priority(5),
    TableColumn::new("size", "Size").width(Constraint::Length(8)).priority(1).sortable(true),
];
let table = tree.table(columns, rows).key("plugins").sort(Some(("size".into(), false)));
// ("plugins", ActionKind::Activated, ActionValue::Text(column)) => self.sort_by(&column),
```

**TypeScript**

```ts
const t = table([
  { id: 'name', title: 'Name', width: { tag: 'length', val: 18 }, priority: 9 },
  { id: 'state', title: 'State', width: { tag: 'length', val: 10 }, priority: 5 },
  { id: 'size', title: 'Size', width: { tag: 'length', val: 8 }, priority: 1, sortable: true },
], rows, { key: 'plugins', sort: ['size', false] });
// if (kind === 'activated' && value.tag === 'text') sortBy(value.val);
```

#### tree-list

**Purpose.** An expandable tree, with the glyphs of the session tree: `▼` expanded, `▶` collapsed, two blank cells for a leaf and one dim `›` per depth level. Items can carry an icon, a trailing text and a badge, and can be dragged onto each other.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `items` | `list<tree-item>` | required | `id`, `label`, `depth`, `expandable`, `expanded`, `icon`, `trailing`, `badge`, `tooltip`, `draggable`, `drop-target`. |
| `multi-select` | `bool` | `false` | Space toggles an item. |

The items form a flat list in display order. `depth` places an item, and `expandable` says whether it has children. The plugin sends only the visible items. The host keeps `expanded` per item id and sends `toggled` when it changes.

| Event | Value | Sent when |
|---|---|---|
| `selected` | `text(id)` | The highlight moves. |
| `activated` | `text(id)` | Enter or double click. |
| `toggled` | `text(id)` | An item is expanded or collapsed. |
| `changed` | `ids(list<string>)` | The multi-selection changes. |
| `dropped` | `text(target id)` | A draggable item is dropped on a drop target. |

| Input | Behaviour |
|---|---|
| Up, Down | Move the highlight. |
| Right | Expands, or goes to the first child. |
| Left | Collapses, or goes to the parent. |
| Enter | Activates. |
| Click on `▶` or `▼`, click on the label | Toggles, selects. |
| Drag | Moves a draggable item. |

Theme roles: `selection`, `muted` for the guides and for trailing text.

<figure>
<pre class="term">╭  ≡ ● · Project───────────────────────────────╮
│ <span class="t-hi">▼ 📂 src</span>                                     │
│ <span class="t-dim">›</span>   ▶ 📁 ui                                  │
│ <span class="t-dim">›</span>   ▼ 📂 app                                 │
│ <span class="t-dim">››</span>    📄 main.rs                             │
│ <span class="t-dim">››</span>    📄 lib.rs                              │
│ ▶ 📁 docs                                    │
│   📄 README.md                               │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>A tree-list with the glyphs of the session tree: ▼ expanded, ▶ collapsed, two blank cells for a leaf, and one dim › per depth level as indentation. The selected row is reversed. Icons come from the item icon field.</figcaption>
</figure>

**Rust**

```rust
let items = vec![
    TreeItem::new("src", "src").depth(0).expandable(true).icon("📂"),
    TreeItem::new("main", "main.rs").depth(2).icon("📄"),
];
let tree_list = tree.tree_list(items).key("files");
// ("files", ActionKind::Toggled, ActionValue::Text(id)) => self.reload_children(&id),
```

**TypeScript**

```ts
const files = treeList([
  { id: 'src', label: 'src', depth: 0, expandable: true, icon: '📂' },
  { id: 'main', label: 'main.rs', depth: 2, icon: '📄' },
], { key: 'files' });
// if (kind === 'toggled' && value.tag === 'text') reloadChildren(value.val);
```

#### menu

**Purpose.** A list of commands in a popup, the same widget as the terminal context menu. Use it inside a `popup` anchored `at-pointer` and open it when the host sends `context-requested`. Entries can have an icon, a shortcut, a check, a submenu, a disabled reason and a danger flag.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `entries` | `list<menu-entry>` | required | `id`, `label`, `icon`, `shortcut`, `parent`, `is-submenu`, `separator-before`, `checked`, `disabled-reason` (up to 40 characters), `tooltip`, `danger`. |
| `items` | `list<menu-item>` | none | `entries` replaces it when present. |

A menu has at most 64 entries and two levels of submenu. An entry names its submenu with `parent`.

| Event | Value | Sent when |
|---|---|---|
| `activated` | `text(id)` | An entry runs. The popup then closes. |
| `closed` | `none` | The user leaves with Escape or a click outside. |

| Input | Behaviour |
|---|---|
| Up, Down | Move the highlight. A disabled entry is skipped. |
| Right | Opens a submenu. |
| Left | Closes the submenu. |
| Enter | Runs the entry. |
| Escape | Closes. |
| Click, hover | Runs, or opens a submenu after 200 ms. |

Theme roles: `selection`, `muted` for shortcuts and disabled entries, `danger` for danger entries.

<figure>
<pre class="term">╭  ≡ ● · File────────────────────────╮
│ 📂 Open file                 <span class="t-dim">Enter</span> │
│ <span class="t-hi">✎ Rename                        F2 </span>│
│ ✓ Show hidden                      │
│ 🔀 Open with                     › │
│ <span class="t-dim">⛔ Delete  (read-only volume)</span>      │
│ <span class="t-err">🚫 Move to trash</span>                   │
╰────────────────────────────────────╯
</pre>
<figcaption>A menu in a popup. The selected row is reversed and bold, a check is ✓ (– for mixed), a submenu is marked ›, a disabled entry is dim with its reason, and a danger entry uses the danger colour. The block title uses the chrome icons.</figcaption>
</figure>

**Rust**

```rust
let entries = vec![
    MenuEntry::new("open", "Open file").shortcut("Enter"),
    MenuEntry::new("hidden", "Show hidden").checked(TriState::On),
    MenuEntry::new("with", "Open with").submenu(true),
    MenuEntry::new("delete", "Delete").disabled_reason("read-only volume"),
    MenuEntry::new("trash", "Move to trash").danger(true),
];
let menu = tree.menu(entries).key("file-menu");
// ("file-menu", ActionKind::Activated, ActionValue::Text(id)) => self.run(&id),
```

**TypeScript**

```ts
const fileMenu = menu([
  { id: 'open', label: 'Open file', shortcut: 'Enter' },
  { id: 'hidden', label: 'Show hidden', checked: 'on' },
  { id: 'with', label: 'Open with', isSubmenu: true },
  { id: 'delete', label: 'Delete', disabledReason: 'read-only volume' },
  { id: 'trash', label: 'Move to trash', danger: true },
], { key: 'file-menu' });
// if (kind === 'activated' && value.tag === 'text') run(value.val);
```

#### check-list

**Purpose.** A list of boxes. Each item is `[x]`, `[ ]` or `[-]`. A parent item names its `children`. Its box is `[-]` when the children differ, and toggling it sets all of them. With `all-toggle` the key `a` toggles every item. The host owns the states and reports them through `widget-state::tri`.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `items` | `list<check-item>` | required | `id`, `label`, `state` (`on`, `off`, `mixed`), `disabled-reason`, `children`. At most 256 items. |
| `all-toggle` | `bool` | `false` | The `a` key toggles all items. |
| `max-height` | `u16` | none | Rows shown before the list scrolls. |

| Event | Value | Sent when |
|---|---|---|
| `changed` | `item-state(id, state)` | An item changes. A parent sends one event per child that changed. |

| Input | Behaviour |
|---|---|
| Up, Down | Move the highlight. |
| Space | Toggles the item. |
| `a` | Toggles all items, with `all-toggle`. |
| Click on the box or label | Toggles. |

Theme roles: `accent` for a checked box, `muted` for a disabled item.

<figure>
<pre class="term">╭  ≡ ● · Group members─────────────────────────────────╮
│ <span class="t-hi">[-] All agents</span>                                       │
│     [x] claude: refactor                             │
│     [ ] codex: tests                                 │
│     <span class="t-dim">[ ] gemini: docs  (frozen)</span>                       │
│ [x] Shell                                            │
│                                                      │
│ <span class="t-dim">Space toggles · a toggles all · Enter applies</span>        │
╰──────────────────────────────────────────────────────╯
</pre>
<figcaption>A check-list with a parent row. The parent shows [-] because its children differ and toggles all of them. A disabled child is dim and carries its reason. The key line below is a key-hints widget, not part of the check-list.</figcaption>
</figure>

**Rust**

```rust
let items = vec![
    CheckItem::new("all", "All agents").children(vec!["claude", "codex", "gemini"]),
    CheckItem::new("claude", "claude: refactor").state(TriState::On),
    CheckItem::new("codex", "codex: tests"),
    CheckItem::new("gemini", "gemini: docs").disabled_reason("frozen"),
];
let list = tree.check_list(items).key("members").all_toggle(true);
// ("members", ActionKind::Changed, ActionValue::ItemState(id, state)) => self.set(&id, state),
```

**TypeScript**

```ts
const members = checkList([
  { id: 'all', label: 'All agents', children: ['claude', 'codex', 'gemini'] },
  { id: 'claude', label: 'claude: refactor', state: 'on' },
  { id: 'codex', label: 'codex: tests' },
  { id: 'gemini', label: 'gemini: docs', disabledReason: 'frozen' },
], { key: 'members', allToggle: true });
// if (value.tag === 'item-state') set(value.val[0], value.val[1]);
```

#### list-editor

**Purpose.** Edits a list of strings, for example globs, URLs, hosts or chords, or a list of objects with up to eight fields. It is the widget behind the `list` rows of Settings. A checked format (`glob`, `url`, `host`, `path`, `regex`, `chord`) draws an invalid item with `✗` and the reason, and the item is not written.

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `items` | `list<string>` | required | The values. |
| `item-format` | `option<item-format>` | none | `glob`, `url`, `host`, `path`, `regex` or `chord`. Checked on commit. |
| `object-fields` | `list<list-field>` | empty | `id`, `title`, `kind`, `width`. Up to 8. The items are then objects. |
| `reorderable` | `bool` | `true` | Shows the `⠿` grip and allows Alt+Up and Alt+Down. |
| `max-items` | `u32` | `64` | 1,024 with the first-party tier. |
| `item-states` | `list<row-state>` | empty | A status per item: `checking`, `ok` or `error`, with a message. |
| `placeholder` | `option<string>` | none | Shown in an empty new item. |

| Event | Value | Sent when |
|---|---|---|
| `changed` | `text(json array)` | An edit is committed. The whole array is sent. |
| `selected` | `index(u32)` | The highlight moves. |

| Input | Behaviour |
|---|---|
| Up, Down | Move the highlight. |
| Enter | Edits the item. |
| Insert or `+` | Adds an item. |
| Delete | Removes the item. |
| Alt+Up, Alt+Down | Reorders. |
| Click on `+`, `✗`; drag the `⠿` grip | Add, remove, reorder. |

Theme roles: `foreground` for the items, `danger` for an invalid item and its message, `muted` for the grip and the placeholder.

<figure>
<pre class="term">╭  ≡ ● · Ignore globs──────────────────────────────────────────────╮
│ ⠿  node_modules/**                                               │
│ ⠿  <span class="t-hi">target/**/*.rlib</span>                                        ✗     │
│ ⠿  <span class="t-err">[unclosed</span>                                              <span class="t-err">✗</span>      │
│ ⠿  <span class="t-dim">add a pattern…</span>                                                │
│ +                                                                │
│ <span class="t-err">✗ Invalid glob: unclosed [</span>                                       │
╰──────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>A list-editor for glob patterns. Each row has a grip ⠿ for dragging, the inline editable text, and ✗ to remove it. The second row is being edited. An invalid item (third row) is drawn in the danger colour and the message is on the last line. The + at the bottom adds an item.</figcaption>
</figure>

<figure>
<pre class="term">╭  ≡ ● · Image URLs────────────────────────────────────────────────╮
│ ⠿  https://example.org/a.jpg                  <span class="t-ok">✓</span>                  │
│ ⠿  https://example.org/b.jpg                  <span class="t-dim">…</span>                  │
│ ⠿  ftp://example.org/c.jpg                    <span class="t-err">✗</span>                  │
│ +                                                                │
│ <span class="t-err">✗ Only http and https URLs are accepted</span>                          │
╰──────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>A list-editor with item-format url and item states. The check marks that the second item is being checked on a remote host (the ellipsis) and that the first passed. The third item failed the format check.</figcaption>
</figure>

With `object-fields` each item is one row with a column per field. The commit sends the whole array of objects as JSON.

<figure>
<pre class="term">╭  ≡ ● · Providers─────────────────────────────────────────────╮
│ <span class="t-dim">⠿  Name         Host                 Port</span>                    │
│ ⠿  photon       photon.komoot.io     443                     │
│ ⠿  <span class="t-hi">nominatim</span>    nominatim.local      8080          ✗         │
│ +                                                            │
╰──────────────────────────────────────────────────────────────╯
</pre>
<figcaption>A list-editor with object-fields: up to 8 fields per item, one column each. The commit sends the whole array as JSON objects.</figcaption>
</figure>

**Rust**

```rust
let globs = tree.list_editor(self.globs.clone()).key("globs").item_format(ItemFormat::Glob);
let hosts = tree.list_editor(Vec::new()).key("providers").object_fields(vec![
    ListField::new("name", "Name", FieldKind::Text).width(12),
    ListField::new("host", "Host", FieldKind::Text).width(20),
    ListField::new("port", "Port", FieldKind::Number).width(6),
]);
// ("globs", ActionKind::Changed, ActionValue::Text(json)) => self.globs = serde_json::from_str(&json)?,
```

**TypeScript**

```ts
const globs = listEditor(state.globs, { key: 'globs', itemFormat: 'glob' });
const providers = listEditor([], {
  key: 'providers',
  objectFields: [
    { id: 'name', title: 'Name', kind: 'text', width: 12 },
    { id: 'host', title: 'Host', kind: 'text', width: 20 },
    { id: 'port', title: 'Port', kind: 'number', width: 6 },
  ],
});
// if (value.tag === 'text') state.globs = JSON.parse(value.val);
```

### Worked example: an agents table

The plugin shows every session in a table, sorts it by the column the user clicks and opens the selected one on Enter.

**Rust**

```rust
impl Views for Agents {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let columns = vec![
            TableColumn::new("name", "Name").width(Constraint::Length(18)).priority(9).sortable(true),
            TableColumn::new("state", "State").width(Constraint::Length(10)).priority(5),
        ];
        let rows = self.rows.iter().map(|row| TableRow::new(vec![row.name.clone(), row.state.clone()]).id(&row.id)).collect();
        let table = tree.table(columns, rows).key("agents").sort(Some((self.sort.clone(), self.ascending)));
        tree.finish(table)
    }
}

impl Handler for Agents {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::UiAction(action) = event else { continue };
            match (action.key.as_str(), action.kind, action.value) {
                ("agents", ActionKind::Activated, ActionValue::Text(column)) => {
                    self.ascending = self.sort == column && !self.ascending;
                    self.sort = column;
                }
                ("agents", ActionKind::Activated, ActionValue::Index(row)) => self.open(row as usize),
                _ => {}
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
export const { handler, views } = definePlugin({
  side: 'client',
  render() {
    const columns = [
      { id: 'name', title: 'Name', width: { tag: 'length', val: 18 }, priority: 9, sortable: true },
      { id: 'state', title: 'State', width: { tag: 'length', val: 10 }, priority: 5 },
    ];
    const rows = state.rows.map((row) => ({ id: row.id, cells: [row.name, row.state] }));
    return tree(table(columns, rows, { key: 'agents', sort: [state.sort, state.ascending] }));
  },
  onEvents(batch) {
    for (const event of batch) {
      if (event.tag !== 'ui-action' || event.val.key !== 'agents') continue;
      const { kind, value } = event.val;
      if (kind === 'activated' && value.tag === 'text') {
        state.ascending = state.sort === value.val ? !state.ascending : true;
        state.sort = value.val;
      } else if (kind === 'activated' && value.tag === 'index') open(value.val);
    }
    return [];
  },
});
```

### Edge cases and errors

| Case | Result |
|---|---|
| More than 256 `check-list` items, or more than `max-items` in a `list-editor` | The render traps with `ui.invalid-tree`; the previous tree stays, marked stale. |
| More than 64 menu entries, or a submenu deeper than two levels | The menu is truncated with `… N more`. |
| Two items with the same `id` | The tree is invalid and the render traps with `ui.duplicate-key`. |
| An invalid item in a `list-editor` | Drawn with `✗` and the reason. It is not written and `changed` is not sent for it. |
| A selected item removed by the next render | The selection moves to the nearest row. |
| `menu` or `list-editor` in a settings page | The render traps with `ui.widget-not-allowed`. |
| `set-widget-state` with a case the widget does not take | `invalid-argument`. |
| A `disabled-reason` longer than 40 characters | The text is cut at 40 characters and ends with `…`; the entry stays disabled and the render succeeds. |

### See also

- [Input](/pages/widgets-input/)
- [Pickers](/pages/widgets-pickers/)
- [Status and feedback](/pages/widgets-status-and-feedback/)
- [Data display](/pages/widgets-data-display/)
- [Widget catalogue](/pages/widgets-index/)
- [UI model](/pages/ui-model/)
- [ui.panels](/pages/capability-ui-panels/)
- [Menus](/pages/api-menus/)
