# virtual rows provider

## virtual rows provider

A virtual row is a row in the project tree that does not belong to a pane or a group. The Chatroom row is one: it has a title, an icon, and it opens a panel when activated. A plugin declares a kind of virtual row in `contributes.virtual_rows` and fills it with the intent `set-virtual-rows`. The `virtual-rows-provider` export adds the missing piece: rows that have children, loaded when the user expands the row, a page at a time. A plugin that shows a folder explorer, a list of remote sessions or a mailbox in the tree uses it.

### You will need

| Need | Value |
|---|---|
| Side | Server and client. The export fragment is `with-virtual-rows`, in `ilium:server/base` and `ilium:client/base`. The row is owned by the side that sends `set-virtual-rows`. |
| Manifest | `"exports": ["virtual-rows-provider"]` and a `virtual_rows` entry with `"expandable": true` and `"children": "provider"`. |
| Grant | `tree.decorate` for `set-virtual-rows`. The export itself needs no grant. |
| Rust | Crate `ilium-plugin-sdk`, trait `ilium_plugin_sdk::exports::VirtualRowsProvider`. |
| TypeScript | Package `@ilium/plugin-sdk`, member `listVirtualChildren` of `definePlugin`. |
| Activation | The trigger `virtual-rows:<row-kind>` starts a stopped instance when a provider row expands. |

### Manifest

```json
{ "contributes": { "virtual_rows": [
  { "id": "inbox", "expandable": true, "children": "provider",
    "accepts_drop": ["file", "text"], "drop_command": "mail/drop-on-folder" }
] } }
```

`children` is `"static"` when the child rows come from `set-virtual-rows` with a `parent`, and `"provider"` when the export supplies them. `accepts_drop` lists what the row accepts from a drag: `pane`, `file` or `text`. A drop runs `drop_command` with the origin `virtual-row-drop`, and the `context` of the event carries `virtual-row` with the id of the row that received it.

### Interface

```wit
// Excerpt of ilium:exports/virtual-rows-provider in reference-wit
record virtual-row-child {
  id: string,                         // stable within the row kind; also used as the parent of deeper rows
  label: rich-text,                   // clickable spans allowed
  icon: option<string>,               // catalogue id or glyph
  expandable: bool,                   // true when the child can itself be expanded
  command: option<string>,            // global command id run on activation
  tooltip: option<tooltip>,
  accepts-drop: bool,
}

list-virtual-children: func(row-kind: string, parent: string, cursor: option<string>)
    -> result<tuple<list<virtual-row-child>, option<string>>, plugin-error>;
```

The result holds one page of children and the cursor of the next page. A `none` cursor means the last page.

### Functions and related intents

| Item | Rust | TypeScript | Meaning |
|---|---|---|---|
| `list-virtual-children` | `VirtualRowsProvider::list_virtual_children(&mut self, row_kind: &str, parent: &str, cursor: Option<String>) -> Result<(Vec<VirtualRowChild>, Option<String>), PluginError>` | `listVirtualChildren(rowKind: string, parent: string, cursor?: string): [VirtualRowChild[], string \| undefined]`, throws `PluginError` | Returns one page of children of `parent`. |
| `set-virtual-rows` | `Intent::SetVirtualRows(SetVirtualRowsSpec)` | `{ tag: 'set-virtual-rows', val: ... }` | Sets the top-level rows of a kind. Rows gain `parent`, `expandable`, `accepts-drop`, `tooltip`, a `rich-text` label and `trailing`. |
| `set-expanded` | `Intent::SetExpanded(SetExpandedSpec)` | `{ tag: 'set-expanded', val: ... }` | Expands or collapses a row from the plugin. Needs `tree.write` for nodes; own virtual rows need no extra grant. |

### How the call behaves

- **When.** The host calls the export when the user expands a row of a kind declared with `children: "provider"`. `parent` is the id of the expanded row, and the first call has `cursor` unset. The host asks again with the returned cursor when the user chooses the row `Load more`.
- **Deadline.** 20 ms per call. A page that is not ready in time is shown as a row `…` with a spinner. A plugin that needs longer returns what it has cached and sends a fresh `set-virtual-rows` when the data arrives.
- **Page size.** At most 256 children per page.
- **Expansion state.** The host keeps which rows are expanded. The plugin may change it with `set-expanded`.
- **Cancel.** Collapsing a row discards its pending page. A late result for a collapsed row is dropped.
- **Errors.** The contract does not define how a failed page is drawn. Return a page whose first child explains the problem when the user must see it.

<figure>
<svg viewBox="0 0 420 220" role="img" aria-label="Sequence of expanding a provider virtual row">
<defs>
<marker id="vrp-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<title>Expanding a virtual row</title>
<desc>The user expands the row. The host shows a spinner row, asks the provider for a page, then draws the children and a Load more row when a cursor is returned.</desc>
<text x="50" y="16" font-size="12" text-anchor="middle" fill="currentColor">User</text>
<text x="190" y="16" font-size="12" text-anchor="middle" fill="currentColor">Host</text>
<text x="360" y="16" font-size="12" text-anchor="middle" fill="currentColor">Plugin</text>
<line x1="50" y1="24" x2="50" y2="210" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="190" y1="24" x2="190" y2="210" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="360" y1="24" x2="360" y2="210" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="50" y1="46" x2="188" y2="46" stroke="currentColor" marker-end="url(#vrp-arrow)"/>
<text x="56" y="40" font-size="10" fill="currentColor">expand row</text>
<line x1="190" y1="80" x2="358" y2="80" stroke="currentColor" marker-end="url(#vrp-arrow)"/>
<text x="196" y="74" font-size="10" fill="currentColor">list-virtual-children(kind, id, none)</text>
<line x1="358" y1="112" x2="192" y2="112" stroke="currentColor" marker-end="url(#vrp-arrow)"/>
<text x="204" y="106" font-size="10" fill="currentColor">page + next cursor</text>
<line x1="188" y1="142" x2="52" y2="142" stroke="currentColor" marker-end="url(#vrp-arrow)"/>
<text x="60" y="136" font-size="10" fill="currentColor">children and Load more</text>
<line x1="50" y1="174" x2="188" y2="174" stroke="currentColor" marker-end="url(#vrp-arrow)"/>
<text x="56" y="168" font-size="10" fill="currentColor">Enter on Load more</text>
<line x1="190" y1="202" x2="358" y2="202" stroke="currentColor" marker-end="url(#vrp-arrow)"/>
<text x="196" y="196" font-size="10" fill="currentColor">list-virtual-children(..., cursor)</text>
</svg>
<figcaption>Lazy loading of children. The host owns expansion state and paging; the plugin returns pages.</figcaption>
</figure>

### Example: a mailbox with folders

The plugin keeps a list of folders on the server side and returns them page by page. The messages of a folder are children of the folder row.

**Grants:** `tree.decorate`

**Rust**

```rust
use ilium_plugin_sdk::exports::VirtualRowsProvider;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{SetVirtualRowsSpec, VirtualRow};
use ilium_plugin_sdk::exports::virtual_rows_provider::VirtualRowChild;

const PAGE: usize = 50;

impl VirtualRowsProvider for Plugin {
    fn list_virtual_children(
        &mut self,
        row_kind: &str,
        parent: &str,
        cursor: Option<String>,
    ) -> Result<(Vec<VirtualRowChild>, Option<String>), PluginError> {
        if row_kind != "inbox" {
            return Err(PluginError::not_found("unknown row kind"));
        }
        let start: usize = cursor.as_deref().and_then(|c| c.parse().ok()).unwrap_or(0);
        let all = self.messages_of(parent);
        let page = all.iter().skip(start).take(PAGE).map(|message| {
            VirtualRowChild::new(&message.id, &message.subject)
                .icon("mail")
                .command("mail/open")
        });
        let next = (start + PAGE < all.len()).then(|| (start + PAGE).to_string());
        Ok((page.collect(), next))
    }
}

// top level rows, sent from on_events
fn top_rows(&self) -> Submission {
    Submission::new(Intent::SetVirtualRows(SetVirtualRowsSpec {
        row_kind: "inbox".into(),
        rows: self.folders().iter()
            .map(|f| VirtualRow::new(&f.id, &f.name).expandable(true))
            .collect(),
    }))
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { VirtualRowChild, Submission } from '@ilium/plugin-sdk/types';

const PAGE = 50;

export const { virtualRowsProvider } = definePlugin({
  side: 'server',
  listVirtualChildren(rowKind, parent, cursor): [VirtualRowChild[], string | undefined] {
    if (rowKind !== 'inbox') throw { code: 'not-found', message: 'unknown row kind' };
    const start = cursor === undefined ? 0 : Number(cursor);
    const all = messagesOf(parent);
    const page = all.slice(start, start + PAGE).map((message) => ({
      id: message.id,
      label: [{ text: message.subject, style: {}, target: { tag: 'none' } }],
      icon: 'mail',
      expandable: false,
      command: 'mail/open',
      acceptsDrop: false,
    }));
    return [page, start + PAGE < all.length ? String(start + PAGE) : undefined];
  },
});

function topRows(): Submission {
  return {
    intent: { tag: 'set-virtual-rows', val: {
      rowKind: 'inbox',
      rows: folders().map((f) => ({ id: f.id, label: f.name, expandable: true })),
    } },
    correlation: 0n, dedupKey: undefined,
  };
}
```

### What the user sees

The expanded row loads its children; the first page ends with a row `Load more` when a cursor was returned. While a page is pending the child area shows `…` and a spinner. Rows follow the tree layout of the client: an indent marker `›` per depth, an expand symbol, the icon, the title.

<pre class="term">
▼ 📂 Ilium
›   🦀 ⠋ Fix auth
›   🖥️    Shell
▼ 📬 Inbox
› ▶ 📁 Work
› ▼ 📁 Reviews
›   ›   ✉ Review request: tree rows
›   ›   ✉ Re: provider paging
›   ›   Load more
</pre>

The glyph set for the group and agent rows is the one the real client draws (see the glyph reference); the mailbox icons are examples of what a plugin may pass as `icon`.

### Errors and limits

| Situation | Result |
|---|---|
| Row kind not declared | `set-virtual-rows` completes as `refused(invalid-argument)`. |
| More than 256 children in a page | The page is cut to 256. |
| Deadline of 20 ms exceeded | The breach is counted; the host shows `…` with a spinner while the page is pending. |
| Trap in the provider | The plugin restarts under the usual rule. |

### See also

- [Plugin exports](/pages/api-exports/) for the export list and deadlines.
- [Intents](/pages/api-intents/) for `set-virtual-rows` and `set-expanded`.
- [Events](/pages/api-events/) for `command-invoked` with the `virtual-row-drop` origin.
- [capability: tree decorate](/pages/capability-tree-decorate/) for `tree.decorate`.
- [Tree](/pages/api-tree/) for reading the real tree.

Contract: `ilium:plugin@0.1.0`, export fragment `with-virtual-rows`.
