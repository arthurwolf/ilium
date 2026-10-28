# Porting from other plugin systems

## Porting from other plugin systems

This guide helps an author who knows VS Code extensions, Neovim plugins or tmux plugins (TPM) move an idea to Ilium. It maps concepts, lists what has no equivalent, and shows a small port of each. Requirement ids of this page are in the range `AUT-700` to `AUT-799`.

The three systems are different from Ilium in one main way. Their plugins run with the full rights of the user, in a process they share with the editor or can start by shell. An Ilium plugin is a WebAssembly component in a sandbox. It can do only what its grants allow, in the time its deadlines allow. A port starts from that fact. [Security](/pages/security/) and [Capabilities](/pages/capabilities/) say more.

### Choosing the sides

Ilium plugins have up to three sides: server (S), client (C) and scene (A). The server owns terminals and agents. The client draws the interface.

| Your plugin does | Side |
|---|---|
| Watches terminals or agents, reacts to output | S |
| Draws a panel, pane kind or settings page | C |
| Draws an animated background | A |
| Runs a command in the palette | The side that owns the command's data |

A port SHOULD put logic about panes and agents on S and drawing on C, and SHOULD send messages between them rather than share state. [AUT-701] See [Patterns cookbook](/pages/guide-patterns-cookbook/) for the message pattern.

## From VS Code

### Concept map

| VS Code | Ilium | Notes |
|---|---|---|
| `package.json` `contributes.commands` | Manifest `contributes.commands` | Id becomes `<plugin-id>/<id>` |
| `contributes.keybindings` | `contributes.keybindings` | Chords and modes `global`, `prefix`, `pane:<kind>` |
| `contributes.configuration` | `contributes.settings` | A JSON schema subset. See [Settings schema](/pages/reference-settings-schema/) |
| `activationEvents` | `activation` | `startup`, `command:<id>`, `view:<id>` and others. Lazy by default |
| `vscode.window.showInformationMessage` | A toast intent | Rate limited to 3 per 10 s |
| WebView panel | `panels` with widgets | No HTML. You return a widget tree |
| TreeView | `virtual_rows` or a `list` widget | Sidebar rows or a list in a panel |
| Status bar item | Status segment | 10 updates per second per slot |
| `workspaceState`, `globalState` | Storage buckets `project`, `global`, `session` | Key 256 bytes, value 1 MiB |
| `context.subscriptions` | `deactivate` | There is no disposable list. Cancel your timers and requests yourself |
| `fs` | `fs.read:<scope>`, `fs.write:<scope>` | Scopes, not paths |
| `child_process` | `process.run:<program>` | High-risk grant, no network |
| `fetch` | `http:<host-pattern>` | Host allowlist |
| `vscode.workspace.onDidChange...` | Events | Delivered in batches |
| Language server client | No equivalent | See below |
| Debug adapter | No equivalent | |
| Extension API of another extension | `plugin-services` (unstable) | Four unstable features exist. Avoid them in a release |

### What does not port

Ilium has no editor-language protocol, no debugger protocol, no webview, no extension-to-extension object access, no Node.js and no way to load native modules. A plugin cannot run arbitrary code in the host. Work that needs a long process is started with `process.spawn` under the `process.long-lived` grant, and then the plugin only reads its output.

### Example: a "Word count" command

VS Code:

```ts
vscode.commands.registerCommand('wc.count', () => {
  const text = vscode.window.activeTextEditor?.document.getText() ?? '';
  vscode.window.showInformationMessage(`${text.split(/\s+/).length} words`);
});
```

Ilium (TypeScript, client side). The editor primitive gives the lines of the active buffer, and a toast shows the count. The command id is declared in the manifest. The sketch below shows the shape only.

```json
{ "contributes": { "commands": [ { "id": "count", "title": "Count words" } ] },
  "activation": ["command:example.wc/count"],
  "capabilities": { "client": ["editor.read", "notify.toast"] } }
```

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { editor } from '@ilium/plugin-sdk/primitives/editor';
import { intents } from '@ilium/plugin-sdk/host/intents';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  handler: {
    onCommand(id: string) {
      if (id !== 'count') return;
      // Read the lines of the buffer in the focused pane (see the editor page).
      const text = readActiveBufferText(editor);
      const words = text.split(/\s+/).filter(Boolean).length;
      // Send a toast intent (grant notify.toast).
      intents.submitToast(`${words} words`);
    },
  },
});
```

`readActiveBufferText` and `submitToast` are placeholders for the calls described in the pages below. Take the exact signatures from [Editor primitive](/pages/primitives-editor/) and [Toast intents](/pages/api-intent-chrome/) before you copy them. A port MUST declare every grant its calls need, and `ilium plugin check` reports an import without a grant as `component.import-ungranted`. [AUT-702]

## From Neovim

### Concept map

| Neovim | Ilium | Notes |
|---|---|---|
| `plugin/*.lua` run at start | `activation: ["startup"]` | Lazy is the default and is better |
| `vim.api.nvim_create_user_command` | `contributes.commands` | Appears in the command palette |
| `vim.keymap.set` | `contributes.keybindings` | Or `keys.intercept` (high risk) for raw keys |
| `vim.g.*` options | `contributes.settings` | A schema, shown in Settings |
| Autocommands (`BufWritePost`) | Events | Match on event name in `handler` |
| `vim.loop` timers | `timers` | Minimum 50 ms, 64 held |
| `vim.fn.jobstart` | `jobs.run` (a declared job kind), or `process.run` | Jobs have 2 s CPU, 20 s wall, 64 MiB |
| Floating windows | `panels` with `modal`, popups | Anchors are in [UI model](/pages/ui-model/) |
| Statusline components | Status segment | |
| `vim.fn.system` | `process.run:<program>` | One program per grant, no network |
| `:h` help | `contributes.help` | Markdown topics, 20 KiB each |
| Lua `require` of other plugins | Not available | `plugin-services` is unstable |
| `nvim_buf_set_lines` | `editor.edit` | Through intents, not direct writes |
| Treesitter and LSP | No equivalent | |

### What does not port

A Neovim plugin may change anything at any time because it runs in the editor's process. An Ilium plugin changes things only through intents, and the host may refuse them. There is no `eval`, no global state shared between plugins and no synchronous block while a job runs. A port that waits in a loop MUST be rewritten as events: start the work, return, and handle the result when `request-completed` arrives. [AUT-703]

### Example: a timer that refreshes a status

Neovim:

```lua
local timer = vim.loop.new_timer()
timer:start(0, 5000, vim.schedule_wrap(function()
  vim.g.build_status = vim.fn.system("make -q") 
end))
```

Ilium (Rust, server side). The timer fires every 5 s. The handler starts a job of a kind declared in `contributes.jobs` (grant `jobs.run`) and returns at once.

```rust
fn activate() -> Result<(), PluginError> {
    timers::set(5_000, Some(5_000))?;
    Ok(())
}

fn handle(event: Event) {
    if let Event::TimerFired(_) = event {
        // Start the declared job kind; the result arrives later as `request-completed`.
        let _ = jobs::start_request("build-status", &[]);
    }
}
```

The fake host does not run timers by itself. A test calls `advance_clock(5_000)` and then `settle()`. See [Testing plugins](/pages/testing/).

## From tmux plugins (TPM)

tmux plugins are shell scripts that call `tmux` commands and set options. Ilium panes are not tmux panes, and a plugin cannot run shell code in the host.

| tmux plugin | Ilium |
|---|---|
| `set -g @plugin '...'` in `.tmux.conf` | `ilium plugin install <file>` |
| `bind-key` | `contributes.keybindings` with mode `prefix` |
| `status-right` with `#(script)` | A status segment filled by the plugin |
| `display-message` | A toast intent |
| `capture-pane -p` | `terminal.read-screen` |
| `send-keys` | `terminal.write` (high risk) |
| `split-window`, `new-window` | `panes.create`, the `create-split` intent (grant `tree.restructure`, high risk) |
| `run-shell` hooks on events | Events, with the matching grant |
| `pane-title-changed` hooks | Events from the tree and pane primitives |
| `resurrect`-style save and restore | Restore bytes and storage. See [Persistence and restart](/pages/reference-persistence-and-restart/) |
| Copy-mode plugins | Not available |

A tmux plugin that only wraps shell commands often becomes a plugin with a `process.run:<program>` grant. A port SHOULD prefer the typed primitive (`terminal.read-screen`) over a program, because the primitive needs a weaker grant. [AUT-704]

### Example: a "log the last screen" binding

tmux:

```bash
bind-key L run-shell "tmux capture-pane -p > ~/screen.log"
```

Ilium: a command with a key chord and two grants, `terminal.read-screen` and `fs.write:<scope>`. The plugin writes the file to a granted scope, not to an arbitrary path. A port MUST NOT ask for a write grant wider than the folder it writes to. [AUT-705]

## Cross-cutting differences

| Topic | Other systems | Ilium |
|---|---|---|
| Rights | Everything the user can do | Only granted capabilities |
| Time | Unbounded | Deadlines: events 20 ms, commands 50 ms, render 10 ms |
| Failure | An error message, the editor continues | A trap, restart after 1 s, then 5 s, disabled at the third failure in 10 minutes |
| State | Variables in the process | Lost on restart. Use storage or restore bytes |
| Language | Lua, JavaScript, shell | Rust or TypeScript, built to a component |
| Distribution | A git clone or a marketplace |  A signed or unsigned `.iliumpkg` file |
| UI | HTML, Lua floats, status strings | A widget tree. The host draws |
| Updates | The editor reloads | Staged update, `deactivate(upgrade)`, migrate, activate |

## A porting method

1. List what the old plugin does in one sentence each.
2. Mark each item as a command, an event reaction, a view or a background job.
3. Look up the capability for each in [Capabilities](/pages/capabilities/). An item without one is out of scope.
4. Write the manifest first, with the smallest grant set.
5. Port logic with tests on the fake host.
6. Run the [Quality checklist](/pages/guide-quality-checklist/).

A port SHOULD be written as a new plugin with a new id, and SHOULD NOT reuse the name of the original in a way that suggests it is the same product. [AUT-706]

## Edge cases

| Situation | Result |
|---|---|
| The old plugin needs a long-running shell command | Use `process.spawn` with `process.long-lived`, and read output only |
| The old plugin edits files directly | Use `fs.write:<scope>` for a granted scope |
| The old plugin reads the environment | Not available. Use a setting |
| The old plugin uses a network from the editor | `http:<host-pattern>`, one pattern per host |
| The old plugin needs a different language runtime | Not available. Compile to a component |
| The old plugin depends on another plugin | Use a documented extension point, not a private call |

## See also

- [Capabilities](/pages/capabilities/)
- [Manifest reference](/pages/reference-manifest/)
- [Patterns cookbook](/pages/guide-patterns-cookbook/)
- [Quality checklist](/pages/guide-quality-checklist/)
- [Tutorial: agent badge](/pages/tutorial-01-agent-badge/)
- [FAQ](/pages/guide-faq/)

API version checked: `ilium:plugin@0.1.0`.
