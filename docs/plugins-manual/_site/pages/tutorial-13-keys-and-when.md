# 13. Keybindings, chords and when expressions

## 13. Keybindings, chords and when expressions

This tutorial binds keys to a command. The command is tiny: it shows a short message called a toast. The point is the keys. You will bind one command to a global key, then to a key behind the prefix key, then to a key that works only in one kind of pane. You will also limit when the command is offered, and see what happens when two plugins want the same key.

This tutorial is standalone. It starts a fresh plugin, so you do not need the earlier tutorials. You only need to have finished [Your first plugin in five minutes](/pages/getting-started-first-plugin/).

Some words first.

- A **command** is a named action that a plugin adds. The user can run it from the command palette, from a key, from a menu and from other places.
- A **keybinding** joins a command to a **chord**. A chord is one key press, such as `p`, or a press with modifier keys held down, such as `ctrl+alt+p`.
- A **toast** is a short message that Ilium draws over the workspace for a few seconds.
- A **grant** is a permission that the user allows when the plugin is installed.
- A **when expression** is a small condition, such as `pane.primitive == "terminal"`. It decides when a command is shown.

The plugin never reads the keyboard. Ilium reads the keys, finds the binding, and sends the plugin a `command-invoked` event. That is why one command can run from a key, the palette and a menu with no extra code.

Tutorial plugin: `example.key-demo`. Side: client, because keys belong to the client. Languages: Rust (target `wasm32-wasip2`) and TypeScript. Every listing is given in both.

### You will need

- [Your first plugin in five minutes](/pages/getting-started-first-plugin/) finished. Nothing else.
- One grant, on the client side: `notify.toast`. It lets the plugin show toasts. Keybindings and commands need no grant.
- Create the project with `--side client`:

```bash
ilium plugin new key-demo --id example.key-demo --lang rust --side client --name "Key demo"
```

Use `--lang typescript` for the TypeScript version.

### What this plugin does

The plugin has one command, `ping`. Pressing the key shows a toast with the title `Ping`. The tutorial then adds a second command that only some panes offer.

### The surface this step uses

| Name | Kind | Fields and types | Meaning |
|---|---|---|---|
| `contributes.commands` | manifest key | `id`, `title`, `palette`, `when`, `category`, `keywords` | Declares a command. Its global id is `<plugin-id>/<id>`. |
| `contributes.keybindings` | manifest key | `command`, `chord`, `mode` | Joins a chord to a command. |
| `mode` | string | `global` (default), `prefix` or `pane:<kind>` | Where the chord is active. |
| `command-invoked` | event | `command: string`, `arguments-json: string`, `origin`, `context` | Delivered to the plugin that owns the command. `origin` is `keybinding` for a key. |
| `toast` | intent | `id`, `tone`, `title`, `body`, `actions`, `duration-ms` | Shows a toast. Needs `notify.toast`. |
| `when` | expression | keys `pane.primitive`, `pane.kind`, `agent.activity` and more | Shows the command only when it holds. |

### The grammar of a chord

A chord is written in lower case. It has zero or more modifiers, each followed by `+`, and then one key.

- The modifiers are `ctrl`, `alt`, `shift` and `super`, in any order.
- A key is one printable character, such as `p` or `1`. Shift plus a letter is `shift+p`. The plus sign itself is written `plus`, and the space bar is `space`.
- A named key is one of `enter`, `escape`, `tab`, `backspace`, `delete`, `insert`, `home`, `end`, `page-up`, `page-down`, `up`, `down`, `left`, `right`, or `f1` to `f24`.

Examples: `p`, `ctrl+alt+p`, `shift+tab`, `ctrl+plus`, `f5`.

### Manifest: one global key

The smallest version has one command and one global binding. A **global** binding is active wherever the focus is.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.key-demo",
  "name": "Key demo",
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
      { "id": "ping", "title": "Ping toast", "category": "Key demo" }
    ],
    "keybindings": [
      { "command": "example.key-demo/ping", "chord": "ctrl+alt+p", "mode": "global" }
    ]
  }
}
```

The binding uses the global command id, which is the plugin id, a slash, and the local id from `commands`.

### The smallest plugin

The handler waits for `command-invoked`. When the command is `ping`, it returns one toast intent. An **intent** is a request the plugin hands back to Ilium, such as "show this toast". The plugin returns a list of intents, and Ilium carries them out.

**Rust**

```rust
// src/lib.rs. The ping command shows a toast.
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::CommandInvokedEvent;
use ilium_plugin_sdk::types::intents::{Intent, ToastSpec};
use ilium_plugin_sdk::types::text::Tone;

const PING: &str = "example.key-demo/ping";

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct KeyDemo;

impl Lifecycle for KeyDemo {}

impl Handler for KeyDemo {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::CommandInvoked(CommandInvokedEvent { command, .. }) = event else {
                continue;
            };
            if command == PING {
                submissions.push(ping_toast("Ping"));
            }
        }
        submissions
    }
}

fn ping_toast(title: &str) -> Submission {
    Submission::new(Intent::Toast(ToastSpec {
        id: Some("ping".into()),
        tone: Tone::Info,
        title: title.into(),
        body: None,
        actions: vec![],
        duration_ms: None,
    }))
}
```

**TypeScript**

```ts
// src/client.ts. The ping command shows a toast.
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';

const PING = 'example.key-demo/ping';
let nextCorrelation = 1n;

function pingToast(title: string): Submission {
  return {
    intent: {
      tag: 'toast',
      val: { id: 'ping', tone: 'info', title, body: undefined, actions: [], durationMs: undefined },
    },
    correlation: nextCorrelation++,
    dedupKey: undefined,
  };
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === PING) {
        submissions.push(pingToast('Ping'));
      }
    }
    return submissions;
  },
});
```

### Line by line

1. The `id: "ping"` on the toast is a stable name. Pressing the key twice replaces the toast on screen instead of stacking two copies.
2. `tone` picks the border colour. `info` is neutral and calm.
3. `body`, `actions` and `duration-ms` are empty. A toast with no duration stays 5,000 ms.
4. The handler ignores every event that is not `command-invoked`. It never asks which key was pressed. The key already became a command.

### Three modes of binding

The `mode` key says where a chord is active.

| Mode | Active when | Chord means |
|---|---|---|
| `global` | Always. This is the default. | The full chord, usually with modifiers, such as `ctrl+alt+p`. |
| `prefix` | After the user presses the prefix key. | The key after the prefix. The default prefix is `ctrl+b`. |
| `pane:<kind>` | Only while a pane of that kind has focus. | The full chord. The kind is the global id of a pane kind. |

Why three? A global key must not collide with what you type into a terminal, so global chords carry modifiers. The **prefix** key is a held-back first press: the user presses `ctrl+b`, lets go, and then a single plain key such as `p` runs the command. That leaves plain letters free. A **pane-kind** key is for a pane that owns its own screen, such as a notes board, where a plain letter is safe.

### One improvement: three bindings and a when expression

Add a second binding for the same command in prefix mode, a second command that only some panes offer, and a pane-kind binding for it.

The pane-kind binding needs the id of a pane kind. This page uses `example.notes.note` as a stand-in, the kind from the notes plugin of the manual. Use the id of any pane kind you have installed. The binding is active only while a pane of that kind has focus.

```json
"contributes": {
  "commands": [
    { "id": "ping", "title": "Ping toast", "category": "Key demo" },
    {
      "id": "note-ping",
      "title": "Ping from a note",
      "category": "Key demo",
      "when": "pane.kind == \"example.notes.note\""
    },
    {
      "id": "terminal-ping",
      "title": "Ping from a terminal",
      "category": "Key demo",
      "when": "pane.primitive == \"terminal\" && !(agent.activity == \"working\")"
    }
  ],
  "keybindings": [
    { "command": "example.key-demo/ping", "chord": "ctrl+alt+p", "mode": "global" },
    { "command": "example.key-demo/ping", "chord": "p", "mode": "prefix" },
    { "command": "example.key-demo/note-ping", "chord": "k", "mode": "pane:example.notes.note" }
  ]
}
```

Now the handler shows a different title for each command. Only the part that changes is shown.

**Rust**

```rust
const NOTE_PING: &str = "example.key-demo/note-ping";
const TERMINAL_PING: &str = "example.key-demo/terminal-ping";

/// The toast title for each of our commands.
fn title_for(command: &str) -> Option<&'static str> {
    match command {
        PING => Some("Ping"),
        NOTE_PING => Some("Ping from a note"),
        TERMINAL_PING => Some("Ping from a terminal"),
        _ => None,
    }
}

// In on_events, replace the single `if command == PING` check with:
//     if let Some(title) = title_for(&command) {
//         submissions.push(ping_toast(title));
//     }
```

**TypeScript**

```ts
const TITLES: Record<string, string> = {
  'example.key-demo/ping': 'Ping',
  'example.key-demo/note-ping': 'Ping from a note',
  'example.key-demo/terminal-ping': 'Ping from a terminal',
};

// In onEvents, replace the single PING check with:
//     const title = TITLES[event.val.command];
//     if (event.tag === 'command-invoked' && title !== undefined) submissions.push(pingToast(title));
```

### The when grammar

A `when` value joins comparisons with `&&` (and), `||` (or), `!` (not) and parentheses. A comparison is `key == "text"` or `key != "text"`. The text is always in double quotes.

| Key | Values |
|---|---|
| `pane.primitive` | `basic`, `terminal`, `agent`, `editor`, `none` |
| `pane.kind` | the global id of a pane kind, such as `example.notes.note` |
| `agent.activity` | `idle`, `working`, `awaiting-approval`, `menu`, `done` |
| `node.kind` | `pane`, `group`, `folder`, `project` |
| `node.file_extension` | the extension of the node's file, without the dot |
| `agent.signature` | the id of the detected agent signature |
| `view.focused` | `true` or `false` |
| `display_mode` | `normal`, `attention` or a plugin mode id |

Rules that are easy to miss:

- A key that refers to a pane is false when no pane is in scope. `pane.primitive == "terminal"` is false on a group row.
- `when` only decides whether the command is shown in the palette, in menus and for its keybinding. It never blocks another plugin that holds the `commands.invoke` grant.
- A `when` with an unknown key is refused at install with `manifest.when-key`. A `when` with bad syntax or an unknown value is refused with `manifest.when-syntax`.

### When two plugins want the same key

Two bindings on the same chord are a conflict. Ilium resolves it in a fixed order:

1. The user's own remap wins.
2. Then the keys built into the Ilium kernel.
3. Then bundled plugins (tier T1), then signed plugins (T2), then local plugins (T3).
4. Inside one rank, the plugin installed first wins.

The losing binding is inactive. It is not an error, and nothing is removed. Ilium lists it in Settings > Keys, so the user can see why a key does nothing. For that reason, give every key a good reason to exist, and prefer a prefix key over a bare global one.

### Showing the chord in the palette

You do nothing to get this. The palette lists the command by its title, and draws its binding at the right edge. A command with `category` is grouped under that heading. A command whose `when` is false is not listed.

### Remapping by the user

The user can change any binding in Settings > Keys. A remap beats the default chord you wrote. If your plugin shows a hint such as "press ctrl+alt+p", read the live binding instead of hard-coding it:

- With the client grant `keymap.read`, `keymap::bindings()` lists the active bindings and `keymap::chord_label(chord)` gives the form the user sees, such as `Ctrl+Alt+P`.
- The event `keymap-changed` tells you to redraw the hint after a remap.

Your plugin can also put a key-capture row in its own settings page. See [Tutorial 10](/pages/tutorial-10-settings-new-tab/) for the `keybinding` row. Its command must be listed in `contributes.commands`, or the install fails with `manifest.settings-command`.

### Points to notice

- **Keys are data, not code.** The handler has no key logic.
- **One command, many ways in.** The same `command-invoked` event arrives from a key, the palette and a menu. The `origin` field tells them apart, but this plugin does not need it.
- **Plain letters are for prefix and pane modes.** Global chords carry modifiers.
- **A stable toast id** keeps repeated presses from filling the screen. At most 3 toasts live at once.

### Edge cases and errors

| Case | What happens | Code or result |
|---|---|---|
| Chord is also a kernel key | The kernel wins, your binding is inactive | Listed in Settings > Keys |
| Two plugins bind `ctrl+alt+p` | The first installed wins within the same tier | The other is inactive |
| The command has `when` that is false | The palette hides it and the key does nothing | No event is sent |
| Command is disabled with `set-state` | The key does nothing | `refused(conflict)` if invoked |
| User declined `notify.toast` | The toast is refused | `refused(not-permitted)` |
| Fourth toast of the plugin within 10 seconds | The toast is dropped | `refused(rate-limited)` |
| Press `p` without pressing the prefix first | Nothing, the key goes to the pane | No event |

### Common mistakes

| Mistake | What you see | Fix |
|---|---|---|
| Wrote `"when": "pane.type == \"terminal\""` | Install refused with `manifest.when-key` | Use `pane.primitive`. |
| Wrote `"when": "pane.primitive = terminal"` | Install refused with `manifest.when-syntax` | Use `==` and double quotes. |
| Used `Ctrl+Alt+P` as the chord | The chord does not parse | Write chords in lower case: `ctrl+alt+p`. |
| Bound a command that is not in `commands` | The binding does nothing | Declare the command first. |
| Forgot `notify.toast` in the manifest | `refused(not-permitted)` on every toast | Add the grant under `capabilities.client`. |
| Gave a toast title over 60 characters | `refused(invalid-argument)` | Shorten the title. |

### Test it

Press the keys in a running Ilium. Run the plugin in dev mode:

```bash
ilium plugin build
ilium plugin dev
```

Press `ctrl+alt+p`. Press the prefix key and then `p`. Focus a pane of the kind from your pane-kind binding and press `k`. Then open the palette and look for the category `Key demo`. If a key does nothing, open Settings > Keys and look for your chord. Check refusals with `ilium plugin logs example.key-demo`.

### What you see

The palette lists the commands with their bindings. The terminal-only command appears only while a terminal pane is focused.

<figure>
<pre class="term">╭  ≡ ● · Command palette ──────────────────────╮
│ <span class="t-prompt">&gt;</span> ping                                       │
│                                              │
│ <span class="t-dim">Key demo</span>                                     │
│ <span class="t-hi">Ping toast                        ctrl+alt+p </span>│
│ Ping toast                          prefix p │
│ Ping from a terminal                         │
│                                              │
│ <span class="t-dim">Enter run · Esc close</span>                        │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The palette lists the command with each binding at the right. The third command shows no binding, because only the first two commands have one that is active here.</figcaption>
</figure>

After the key, the toast appears. It does not take the focus away from the pane you are typing in.

<figure>
<pre class="term">╭  ≡ ● · Ping ─────────────────────────────────╮
│ ━━━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────│
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A toast with the title Ping and no body. The bar at the bottom shrinks until the toast closes.</figcaption>
</figure>

### Try it yourself

1. Change the global chord to `ctrl+alt+shift+p` and reload. Open Settings > Keys and find the new chord.
2. Change the `when` of `terminal-ping` to `pane.primitive == "agent"` and check which panes list the command.
3. Bind a second chord, `f6`, to `ping` in global mode. Notice that one command can have several bindings.

### See also

- [Commands API](/pages/api-commands/) has the command fields, the `when` grammar and the invocation origins.
- [Manifest reference](/pages/reference-manifest/) has the keybinding chords, the conflict order and the manifest errors.
- [Toast API](/pages/api-toast/) has every toast field, the rate limit and the errors.
- [Keys API](/pages/api-keys/) is for plugins that need to grab keys, which is a different job from a binding.
- [Kernel settings and keymap reads](/pages/api-kernel-settings-and-keymap/) reads the live bindings.
- [Command palette](/pages/api-command-palette/) explains categories and keywords.
- [Tutorial 14](/pages/tutorial-14-toast-and-status/) goes deeper into toasts and the status bar.

API checked against `ilium:plugin@0.1.0`.
