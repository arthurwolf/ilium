# command palette

## command palette

The command palette is a kernel surface: one filterable list of every command the user may run, opened by a chord. A plugin does not draw it and cannot replace it. A plugin appears in it by declaring commands, and two manifest fields control how a command is found: `category`, a heading under which it is grouped, and `keywords`, extra words the filter matches.

### What you need

- **Grant.** None to appear in the palette. Commands the plugin declares are listed; a command that needs a grant is shown disabled with the reason until the user grants it.
- **Manifest.** Entries in `contributes.commands`.

```json
"contributes": { "commands": [
  { "id": "scan", "title": "Scan the project for TODO comments",
    "category": "Todo", "keywords": ["fixme", "comment", "task"] }
] }
```

### Fields

| Field | Type | Meaning |
|---|---|---|
| `id` | string | Unique within the plugin. The host namespaces it as `<plugin-id>.<id>`. |
| `title` | string | Shown in the palette and in menus. At most 80 characters. |
| `category` | string, optional | Group heading. At most 32 characters. Commands without one fall under the plugin name. |
| `keywords` | list of string, optional | At most 8 words matched by the filter but not shown. |
| `palette` | boolean, optional | `false` hides the command from the palette (default `true`). |

The filter is the host's, as for [catalogues](/pages/api-catalogs/): case-insensitive substring over title, category and keywords, prefix matches first. Enabled state and the check mark come from `commands.set-state` (see [commands](/pages/api-commands/)). A disabled command stays visible and dim.

<figure>
<pre class="term">╭  ≡ ● · Commands ──────────────────────────────╮
│ > fix▏                                        │
│ Todo                                          │
│ ▸ Scan the project for TODO comments          │
│   Open the scan report                        │
│ Editor                                        │
│   Fix indentation                             │
│ Esc close · Enter run · ↑/↓ choose            │
╰───────────────────────────────────────────────╯
</pre>
<figcaption>The palette filtered by "fix". The first command matches through its keyword "fixme", which the palette does not show. Rounded border and the dotted hint bar follow the client's dialog style; the exact layout of the palette is the kernel's.</figcaption>
</figure>

### Running a command

Choosing an entry invokes it: the plugin receives `command-invoked` with origin `palette`. The command can also be bound to a key through the keymap, shown in a menu, run by a toast button or a row action; see [commands](/pages/api-commands/).

**Rust**

```rust
fn on_event(&mut self, event: Event) -> Vec<Submission> {
    match event {
        Event::CommandInvoked(invoked) if invoked.command == "scan" => self.start_scan(),
        _ => Vec::new(),
    }
}
```

**TypeScript**

```ts
onEvents(batch) {
  return batch.flatMap((event) =>
    event.tag === 'command-invoked' && event.val.command === 'scan' ? startScan() : []);
}
```

### Errors and limits

More commands than the plugin's command cap fail the install with `manifest.contribution-limit`. See [Errors and limits](/pages/reference-errors-and-limits/).

### WIT

The contract below is an excerpt. The owner is the [WIT reference](/pages/reference-wit/), and that page wins if the two differ.

```wit
// Excerpt of ilium:host/commands in reference-wit
/// Own commands only. Client only.
set-state: func(command: string, enabled: bool, checked: option<bool>) -> result<_, plugin-error>;
/// Async. Completes with ok once the command has run. Errors: not-found. Times out after 30 s.
invoke: func(command: string, arguments-json: string) -> result<request-id, plugin-error>;
```

### See also

- [commands](/pages/api-commands/), [keys](/pages/api-keys/), [menus](/pages/api-menus/) and [toast](/pages/api-toast/).

### Security

Palette entries show the plugin name and are sanitised ([SEC-378], [SEC-380]).
