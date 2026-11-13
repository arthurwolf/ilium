---
layout: page
title: "15. A context-menu entry"
permalink: /pages/tutorial-15-context-menu/
---

## 15. A context-menu entry

This tutorial adds an entry to the menu that opens when the user right-clicks a node in the tree. Choosing the entry runs a command of the plugin. The command shows a toast that names what was clicked. The improvement adds an entry that is computed when the menu opens, so it can depend on the node.

This tutorial is standalone. It starts a fresh plugin, so you do not need the earlier tutorials. You only need to have finished [Your first plugin in five minutes](/pages/getting-started-first-plugin/).

Some words first.

- A **context menu** is the short menu that opens at the pointer on a right-click. Its content depends on what was clicked.
- A **target** says which context menu an entry belongs to. Here it is `tree-node`, the menu of a row in the tree.
- A **command** is a named action. The entry does not hold code. It points at a command.
- A **handler** is the function of the plugin that receives events. The click arrives there as the event `command-invoked`.
- An **intent** is a request the plugin hands back, such as "show this toast".
- A **grant** is a permission that the user allows at install.
- A **provider** is a plugin function that Ilium calls to ask for menu entries on demand.

Tutorial plugin: `example.menu-demo`. Side: client, because menus belong to the client window. Languages: Rust (target `wasm32-wasip2`) and TypeScript. Every listing is given in both.

### You will need

- [Your first plugin in five minutes](/pages/getting-started-first-plugin/) finished. Nothing else.
- One grant, on the client side: `notify.toast`, for the confirmation toast. A static menu entry needs no grant.
- Create the project:

```bash
ilium plugin new menu-demo --id example.menu-demo --lang rust --side client --name "Menu demo"
```

Use `--lang typescript` for the TypeScript version.

### What this plugin does

Right-click a row in the tree and the menu has a new entry, `Say hello`. Choosing it shows the toast `Hello from the menu`.

### The surface this step uses

| Name | Kind | Fields and types | Meaning |
|---|---|---|---|
| `contributes.commands` | manifest key | `id`, `title`, `palette` | Declares the command that the entry runs. |
| `contributes.context_menu` | manifest key | `id`, `title`, `command`, `target`, `icon`, `when`, `dynamic`, `items` | Declares a menu entry. |
| `target` | string | `terminal`, `tree-node`, `editor` or `pane:<kind>` | Which context menu gets the entry. |
| `command-invoked` | event | `command`, `arguments-json`, `origin`, `context` | The click. `origin` is `menu`. |
| `toast` | intent | `id`, `tone`, `title`, `body`, `actions`, `duration-ms` | Shows a toast. Needs `notify.toast`. |
| `menu-provider` | export | `menu-items(context) -> list<menu-entry>` | Computes entries when the menu opens. |

### Manifest

The manifest declares the command first, then the menu entry that points at it. The entry needs the global command id, which is the plugin id, a slash and the local id.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.menu-demo",
  "name": "Menu demo",
  "version": "0.1.0",
  "publisher": "Example Developer",
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": []
  },
  "capabilities": {
    "client": ["notify.toast"]
  },
  "contributes": {
    "commands": [
      { "id": "hello", "title": "Say hello", "palette": false }
    ],
    "context_menu": [
      {
        "id": "hello",
        "title": "Say hello",
        "command": "example.menu-demo/hello",
        "target": "tree-node"
      }
    ]
  }
}
```

`palette: false` keeps the command out of the command palette, because it is meant for the menu. The command still exists and other plugins with the grant `commands.invoke` may run it.

### The smallest plugin

The handler waits for `command-invoked`. When the command is ours, it returns one toast.

**Rust**

```rust
// src/lib.rs. The menu entry runs a command that shows a toast.
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::CommandInvokedEvent;
use ilium_plugin_sdk::types::intents::{Intent, ToastSpec};
use ilium_plugin_sdk::types::text::Tone;

const HELLO: &str = "example.menu-demo/hello";

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct MenuDemo;

impl Lifecycle for MenuDemo {}

impl Handler for MenuDemo {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::CommandInvoked(CommandInvokedEvent { command, .. }) = event else {
                continue;
            };
            if command == HELLO {
                submissions.push(Submission::new(Intent::Toast(ToastSpec {
                    id: Some("hello".into()),
                    tone: Tone::Info,
                    title: "Hello from the menu".into(),
                    body: None,
                    actions: vec![],
                    duration_ms: None,
                })));
            }
        }
        submissions
    }
}
```

**TypeScript**

```ts
// src/client.ts. The menu entry runs a command that shows a toast.
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';

const HELLO = 'example.menu-demo/hello';
let nextCorrelation = 1n;

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'command-invoked' || event.val.command !== HELLO) continue;
      submissions.push({
        intent: {
          tag: 'toast',
          val: { id: 'hello', tone: 'info', title: 'Hello from the menu',
            body: undefined, actions: [], durationMs: undefined },
        },
        correlation: nextCorrelation++,
        dedupKey: undefined,
      });
    }
    return submissions;
  },
});
```

### Line by line

1. `target: "tree-node"` puts the entry in the menu of a tree row. Other targets are `terminal`, `editor` and `pane:<kind>`.
2. The entry has a `command`. It has no code of its own. The click becomes a `command-invoked` event.
3. The handler checks the command id and ignores everything else.
4. The toast id `hello` makes a second click replace the first toast.

### One improvement: an entry computed when the menu opens

A static entry is the same for every node. Often the menu should depend on the click. For example, it should name the node, or it should be greyed out for a node that cannot take the action.

Ilium solves this with a **dynamic** entry. The manifest marks it `dynamic: true`. When the menu opens, Ilium calls the plugin export `menu-items` with a description of the click. The plugin returns a list of entries. The export is called a menu provider.

Change the manifest in three places. Add the export, add the trigger entry, and keep the static entry for the fallback.

```json
"client": {
  "entry": "client.wasm",
  "world": "ilium:client/base@0.1.0",
  "exports": ["menu-provider"]
},
"contributes": {
  "commands": [
    { "id": "hello", "title": "Say hello", "palette": false },
    { "id": "inspect", "title": "Inspect this node", "palette": false }
  ],
  "context_menu": [
    {
      "id": "hello",
      "title": "Say hello",
      "command": "example.menu-demo/hello",
      "target": "tree-node"
    },
    {
      "id": "extras",
      "title": "Menu demo",
      "command": "example.menu-demo/inspect",
      "target": "tree-node",
      "dynamic": true
    }
  ]
}
```

The context that Ilium passes has these fields.

| Field | Meaning |
|---|---|
| `menu` | The id of the menu entry that asked, here `extras`. |
| `node` | The node that was clicked, if any. |
| `pane`, `view` | The pane and view that were clicked, if any. |
| `key`, `buffer`, `line` | Set for an editor menu. |
| `selection` | The selected text, if any, cut at 64 KiB. |

Now the provider. It returns one entry, `Inspect this node`. When the click has no node, the entry stays but is disabled with a reason.

**Rust**

```rust
use ilium_plugin_sdk::exports::MenuProvider;
use ilium_plugin_sdk::exports::menu_provider::MenuContext;
use ilium_plugin_sdk::types::ui::MenuEntry;

impl MenuProvider for MenuDemo {
    fn menu_items(&mut self, context: MenuContext) -> Vec<MenuEntry> {
        let mut entry = MenuEntry::new("inspect", "Inspect this node");
        if context.node.is_none() {
            entry = entry.disabled_reason("No node here");
        }
        vec![entry]
    }
}
```

**TypeScript**

```ts
import type { MenuContext, MenuEntry } from '@ilium/plugin-sdk/types';

export const menuProvider = {
  menuItems(context: MenuContext): MenuEntry[] {
    const entry: MenuEntry = {
      id: 'inspect',
      label: 'Inspect this node',
      icon: undefined,
      shortcut: undefined,
      parent: undefined,
      isSubmenu: false,
      separatorBefore: false,
      checked: undefined,
      disabledReason: context.node === undefined ? 'No node here' : undefined,
      tooltip: undefined,
      danger: false,
    };
    return [entry];
  },
};
```

In TypeScript, also pass `side: 'client'` and `menuItems` to `definePlugin` as in the other tutorials, and export `menuProvider` next to `lifecycle` and `handler`.

When the user picks the dynamic entry, the click arrives as `command-invoked`. Its `arguments-json` holds the entry id as `{"item":"inspect", ...}`. Read `item` to know which dynamic entry was chosen.

### The 20 ms deadline

A menu must open at once. Ilium gives the provider 20 ms. If the plugin is late, Ilium shows the static entries only and drops the dynamic ones for that opening. Keep the provider fast:

- Build the list from the context only. Do not read storage or wait for a request.
- Return few entries. Ilium shows at most 64 and ends the list with `… N more`.
- Keep entry ids stable, because the click reports the id.

### Points to notice

- **The entry is data.** A static entry needs no code beyond the command handler.
- **Menu and palette share the command.** The same command can be in both. `palette: false` hides it from the palette.
- **A provider adds, it does not replace.** Static entries are always there as the fallback.
- **The click carries the context.** The command gets the same fields the provider got.

### Edge cases and errors

| Case | What happens | Code or result |
|---|---|---|
| Provider is slower than 20 ms | Only static entries show | No error, a log line |
| Provider returns over 64 entries | The list is cut and ends with `… N more` | No error |
| Disabled reason over 40 characters | The reason is cut | No error |
| Menu is nested three levels deep | The install is refused | `manifest.menu-depth` |
| `when` is false for the clicked node | The entry is hidden | No event |
| `notify.toast` declined | The toast is refused | `refused(not-permitted)` |
| `dynamic: true` without the export | The entry shows nothing | Install error |

### Common mistakes

| Mistake | What you see | Fix |
|---|---|---|
| Wrote `command` as `hello` | The click does nothing | Use the global id `example.menu-demo/hello`. |
| Wrote `target: "node"` | The install is refused | Use `tree-node`. |
| Used a menu `when` with an unknown key | Install refused with `manifest.when-key` | Use the keys in the [Commands API](/pages/api-commands/). |
| Forgot `menu-provider` in `exports` | The dynamic entry never appears | Add it to `client.exports`. |
| Did slow work in the provider | The dynamic entries are missing | Compute from the context only. |

### Test it

A provider is a plain function, so test it without a host. Test the handler with the fake host.

**Rust**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ilium_plugin_sdk::testing::FakeHost;

    #[test]
    fn the_hello_command_shows_a_toast() {
        let mut host = FakeHost::new().grant("notify.toast");
        host.activate::<MenuDemo>();
        let submissions = host.deliver(vec![Event::CommandInvoked(CommandInvokedEvent {
            command: HELLO.into(),
            arguments_json: "{}".into(),
            origin: "menu".into(),
            context: Default::default(),
        })]);
        assert!(matches!(submissions[0].intent, Intent::Toast(_)));
    }
}
```

**TypeScript**

```ts
// src/client.test.ts. Run with: bun test
import { expect, test } from 'bun:test';
import { createFakeHost } from '@ilium/plugin-sdk/testing';
import { handler, lifecycle } from './client';

test('the hello command shows a toast', async () => {
  const host = createFakeHost().grant('notify.toast');
  await host.activate({ lifecycle, handler });
  const submissions = host.deliver([
    { tag: 'command-invoked', val: { command: 'example.menu-demo/hello',
      argumentsJson: '{}', origin: 'menu', context: {} } },
  ]);
  expect(submissions[0].intent.tag).toBe('toast');
});
```

Then run `ilium plugin dev`, right-click a tree row, and choose the entry. Read errors with `ilium plugin logs example.menu-demo`.

### What you see

The menu opens at the pointer. The new entries sit with the other plugin entries.

<figure>
<pre class="term">╭  ≡ ● · Tree node ────────────────────────────╮
│ Rename  F2                                   │
│ Duplicate                                    │
│ Say hello                                    │
│ Inspect this node                            │
│ Close                                        │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The right-click menu of a tree row with the two entries of the plugin in the middle.</figcaption>
</figure>

On a click with no node, the dynamic entry is shown but greyed out with its reason.

<figure>
<pre class="term">╭  ≡ ● · Pane menu ────────────────────────────╮
│ Say hello                                    │
│ <span class="t-dim">Inspect this node               No node here</span> │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The disabled entry keeps its place and shows the reason at the right.</figcaption>
</figure>

After the choice, the toast appears.

<figure>
<pre class="term">╭  ≡ ● · Hello from the menu ──────────────────╮
│ ━━━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────│
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The toast that the command shows after the click.</figcaption>
</figure>

### Try it yourself

1. Change `target` to `terminal` and right-click a terminal pane instead.
2. Add a `when` of `node.kind == "folder"` to the static entry and check which rows show it.
3. Make the provider add a second entry whose `danger` is true and see how it is drawn.

### See also

- [Commands API](/pages/api-commands/) has the command fields, `context_menu` and the invocation origins.
- [Menu provider API](/pages/api-menu-provider/) has the provider export, the context and the entry fields.
- [Menus API](/pages/api-menus/) covers submenus, separators and checked entries.
- [Manifest reference](/pages/reference-manifest/) lists `context_menu` and its errors.
- [Tutorial 13](/pages/tutorial-13-keys-and-when/) binds keys to a command.
- [Tutorial 14](/pages/tutorial-14-toast-and-status/) goes deeper into toasts.

API checked against `ilium:plugin@0.1.0`.

**Next:** [Tutorial 16, remember a counter in storage](/pages/tutorial-16-remember-a-counter/).
