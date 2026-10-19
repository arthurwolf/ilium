# Worktrees

## Worktrees

A worktree agent works in its own Git worktree, a second checkout of the repository on its own branch, so several agents can change the same project without stepping on each other. The plugin id is `ilium.core.worktrees` and it runs on both sides: the server side runs Git, creates panes and records worktree custody, and the client side offers the command, the tree status line and the Git settings tab.

The kernel never runs Git itself. The plugin runs `git worktree add` through process control and then registers the result with the kernel, as described on the [workspace](/pages/api-workspace/) page.

### What it covers

- Start an agent in a new worktree or in an existing one, described in the worktrees guide.
- The status and branch line in the tree.
- Cleanup: pruning, the removal blockers and branch deletion.
- Copying local files with `.worktreeinclude` and running the post-create command.
- The Git settings tab, with the default worktree path and the post-create command.

### See it

<figure><img src="/assets/demos/25-worktree-agent.gif" alt="A new agent starts in a fresh Git worktree and its branch shows in the tree" loading="lazy"><figcaption>The command creates the worktree, opens a pane inside it and shows the branch in the tree.</figcaption></figure>

### What the user sees

<figure>
<pre class="term">╭─ Tree ─────────────────────────────────────────╮
│ ▼ 📂 Ilium                                     │
│ › 🦀 ⠋ Fix auth                                │
│ ›   🌿 fix-auth ………………………… <span class="t-ok">clean</span>               │
│ › 🦀   Docs sweep                              │
│ ›   🌿 docs-sweep ……………………… <span class="t-warn">2 changed</span>          │
╰────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative: the icons depend on the icon set. A worktree agent row, then a branch line with a dotted leader and its state.</figcaption>
</figure>

<figure>
<pre class="term">╭─ Git ──────────────────────────────────────────╮
│ Worktree folder …………………… ~/dev/ilium.worktrees │
│ Post-create command …………………………………… bun install │
│ ! docs-sweep cannot be removed                 │
│   It has uncommitted changes.                  │
╰────────────────────────────────────────────────╯</pre>
<figcaption>Rendered at 48 columns. Illustrative. The Git settings tab with a <code>path</code> row, a <code>text</code> row and a removal-blocker <code>notice</code>.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

The contract version is 0.1. The grants follow the file scope rules: the worktree folder is the `worktrees` scope, and Git is a named program.

```json
{
  "id": "ilium.core.worktrees",
  "capabilities": {
    "server": [
      "workspace.manage", "process.run:git", "process.run:sh",
      "panes.create", "panes.read",
      "fs.read:project", "fs.write:worktrees"
    ],
    "client": ["ui.panels"]
  },
  "contributes": {
    "commands": [{ "id": "new-agent", "title": "New agent in worktree", "category": "Git" }],
    "keybindings": [{ "command": "new-agent", "chord": "w", "mode": "prefix" }]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.worktrees",
  "name": "Worktrees",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "A worktree agent works in its own Git worktree, a second checkout of the repository on its own branch, so several agents can change the same project without stepping on each other.",
  "license": "MIT",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0"
  },
  "capabilities": {
    "server": [
      "workspace.manage",
      "process.run:git",
      "process.run:sh",
      "panes.create",
      "panes.read",
      "fs.read:project",
      "fs.write:worktrees"
    ],
    "client": [
      "ui.panels",
      "settings.write-kernel:git"
    ]
  },
  "activation": [
    "command:new-agent",
    "service:create"
  ],
  "contributes": {
    "commands": [
      {
        "id": "new-agent",
        "title": "New agent in worktree",
        "category": "Git"
      }
    ],
    "keybindings": [
      {
        "command": "new-agent",
        "chord": "w",
        "mode": "prefix"
      }
    ]
  },
  "limits_tier": "first-party"
}
```

### Kernel tabs this plugin owns

This plugin is the single owner of the following built-in Settings tabs. It holds `settings.write-kernel:<tab>` for each one (T1 only, one holder per tab, see [kernel settings](/pages/api-kernel-settings-and-keymap/#writing-a-kernel-tab)). The values stay in the same `config.toml` tables as before, so no user setting moves. [COR-030] [COR-033]

| Tab id | Label | Config table | Readers |
|---|---|---|---|
| `git` | Git | `[git]` | Fleet |

If the plugin is disabled, the tab stays in the rail with its stored values shown read-only and the notice "Worktrees is disabled. Values are kept." The kernel keeps reading the keys it consumes itself (listed in the Consumer column of the key table in [config and data migration](/pages/reference-config-and-data-migration/)). Behaviour that only this plugin provides is off. The exact result for this plugin is its row in [kernel and boot](/pages/reference-kernel-and-boot/). [COR-022]

### Samples

#### New agent in a worktree (Ctrl+B W)

The command carries a keybinding in prefix mode, so the user presses the prefix key and then `w`. On `command-invoked` the client side sends the selected branch to the server side. See [commands](/pages/api-commands/), [the command palette](/pages/api-command-palette/), [events](/pages/api-events/) and [messages and topics](/pages/api-messages-and-topics/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::CommandInvokedEvent;
use ilium_plugin_sdk::types::intents::{MessageSpec, MessageTarget, PluginSide};

fn on_command(event: CommandInvokedEvent) -> Vec<Submission> {
    if event.command != "ilium.core.worktrees/new-agent" {
        return Vec::new();
    }
    // arguments_json holds the branch the user picked, for example {"branch":"fix-auth"}.
    vec![Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget { plugin: "ilium.core.worktrees".into(), side: PluginSide::Server, client: None },
        channel: "new-agent".into(),
        payload: event.arguments_json.into_bytes(),
    }))]
}
```

**TypeScript**

```ts
import type { Event, Submission } from '@ilium/plugin-sdk/types';

export function onCommand(event: Event): Submission[] {
  if (event.tag !== 'command-invoked') return [];
  if (event.val.command !== 'ilium.core.worktrees/new-agent') return [];
  // argumentsJson holds the branch the user picked, for example {"branch":"fix-auth"}.
  return [{
    intent: { tag: 'message', val: {
      to: { plugin: 'ilium.core.worktrees', side: 'server', client: undefined },
      channel: 'new-agent',
      payload: new TextEncoder().encode(event.val.argumentsJson) } },
    correlation: 1n, dedupKey: undefined,
  }];
}
```

#### Create the worktree and the pane

The server runs `git worktree add`, submits `create-pane` with the worktree as its start folder, and on `pane-created` records the new pane as the owner of the worktree. See [workspace](/pages/api-workspace/), [workspace.manage](/pages/capability-workspace-manage/), [panes.create](/pages/capability-panes-create/), [process control](/pages/api-process-control/), [intents](/pages/api-intents/) and [events](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::host::process::{self, ProcessSpec};
use ilium_plugin_sdk::host::workspace::{self, WorkspaceSpec};
use ilium_plugin_sdk::types::intents::CreatePaneSpec;

impl Worktrees {
async fn add_worktree(&mut self, repository: FsPath, folder: FsPath, host_path: String, branch: String)
    -> Result<Submission, PluginError>
{
    let git = ProcessSpec {
        program: "git".into(),
        arguments: vec!["worktree".into(), "add".into(), "-b".into(), branch.clone(), host_path, "HEAD".into()],
        cwd: Some(repository), stdin: None, environment: Vec::new(), timeout_millis: 60_000,
    };
    process::run(git).await?;
    self.pending.insert(branch.clone(), folder.clone());
    Ok(Submission::new(Intent::CreatePane(CreatePaneSpec {
        parent: None, title: branch, content_kind: "ilium.terminal".into(),
        cwd: Some(folder), command: Some(vec!["claude".into()]), initial_input: None,
    })))
}
}

// Event::PaneCreated(pane): look up pane.title in self.pending, then
// workspace::register(WorkspaceSpec { repository, path, branch, base: Some("HEAD".into()), owner: Some(pane.node) }).
```

**TypeScript**

```ts
import { run } from '@ilium/plugin-sdk/host/process';
import { register } from '@ilium/plugin-sdk/host/workspace';
import type { FsPath, Submission } from '@ilium/plugin-sdk/types';

export async function addWorktree(
  repository: FsPath, folder: FsPath, hostPath: string, branch: string,
): Promise<Submission> {
  await run({ program: 'git', arguments: ['worktree', 'add', '-b', branch, hostPath, 'HEAD'],
    cwd: repository, stdin: undefined, environment: [], timeoutMillis: 60_000 });
  pending.set(branch, { repository, folder });
  return {
    intent: { tag: 'create-pane', val: { parent: undefined, title: branch,
      contentKind: 'ilium.terminal', cwd: folder, command: ['claude'], initialInput: undefined } },
    correlation: 1n, dedupKey: undefined,
  };
}

// case 'pane-created': const p = pending.get(event.val.title);
//   register({ repository: p.repository, path: p.folder, branch: event.val.title, base: 'HEAD', owner: event.val.node });
```

#### Copy .worktreeinclude files

`.worktreeinclude` lists ignored local files, such as `.env`, that a new worktree needs. The plugin reads the list from the project scope, reads each file there and writes it into the worktree scope. See [fs](/pages/api-fs/), [fs.read](/pages/capability-fs-read/) and [file access](/pages/file-access/).

**Rust**

```rust
use ilium_plugin_sdk::host::fs::{self, WriteMode};
use ilium_plugin_sdk::types::paths::FsPath;

async fn copy_includes(worktree_dir: &str) -> Result<(), PluginError> {
    let list_path = FsPath { scope: "project".into(), path: ".worktreeinclude".into() };
    let list = String::from_utf8_lossy(&fs::read(&list_path).await?).to_string();
    for relative in list.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let source = FsPath { scope: "project".into(), path: relative.into() };
        let target = FsPath { scope: "worktrees".into(), path: format!("{worktree_dir}/{relative}") };
        let bytes = fs::read(&source).await?;
        fs::write(&target, bytes, WriteMode::Replace).await?;
    }
    Ok(())
}
```

**TypeScript**

```ts
import { read, write } from '@ilium/plugin-sdk/host/fs';

export async function copyIncludes(worktreeDir: string): Promise<void> {
  const list = new TextDecoder().decode(await read({ scope: 'project', path: '.worktreeinclude' }));
  const lines = list.split('\n').map((l) => l.trim()).filter((l) => l && !l.startsWith('#'));
  for (const relative of lines) {
    const bytes = await read({ scope: 'project', path: relative });
    await write({ scope: 'worktrees', path: `${worktreeDir}/${relative}` }, bytes, 'replace');
  }
}
```

#### Status and branch line

The client renders one `leader-row` per worktree. An `icon` marks the branch and the state is the value, joined to the label by the dotted leader. See [text widgets](/pages/widgets-text/) and [status and feedback widgets](/pages/widgets-status-and-feedback/).

**Rust**

```rust
fn branch_line(tree: &mut Tree, branch: &str, state: &str) -> NodeHandle {
    let icon = tree.icon("🌿");
    let row = tree
        .leader_row(vec![Span::plain(branch)], vec![Span::plain(state)])
        .key(&format!("branch-{branch}"));
    tree.layout(Direction::Horizontal, [Constraint::Length(2), Constraint::Fill(1)])
        .child(icon)
        .child(row)
}
```

**TypeScript**

```ts
import { fill, icon, layout, leaderRow, length, span } from '@ilium/plugin-sdk/ui/build';

export function branchLine(branch: string, state: string) {
  const row = leaderRow([span(branch)], [span(state)], { key: `branch-${branch}` });
  return layout({ direction: 'horizontal', constraints: [length(2), fill(1)] }, [icon('🌿'), row]);
}
```

#### Git settings and removal blockers

The defaults live in settings: a `path` row for the worktree folder, a `text` row for the post-create command, and a static `notice` row that explains the removal blockers. A settings notice cannot change at run time, so the live blocker for one worktree is shown by a `notice` widget in the client panel. See [settings extension](/pages/settings-extension/) and [settings row types](/pages/reference-settings-rows/).

**Rust**

```rust
// Manifest fragment: contributes.settings.
const GIT_ROWS: &str = r#"[
  { "type": "path", "key": "worktree_root", "title": "Worktree folder", "picker": "directory" },
  { "type": "text", "key": "post_create", "title": "Post-create command", "default": "" },
  { "type": "notice", "id": "removal_blocker", "tone": "warning", "dismissible": false, "text": "A worktree with uncommitted changes or a running process cannot be removed." }
]"#;
```

**TypeScript**

```ts
// Manifest fragment: contributes.settings.
export const gitRows = [
  { type: 'path', key: 'worktree_root', title: 'Worktree folder', picker: 'directory' },
  { type: 'text', key: 'post_create', title: 'Post-create command', default: '' },
  { type: 'notice', id: 'removal_blocker', tone: 'warning', dismissible: false, text: 'A worktree with uncommitted changes or a running process cannot be removed.' },
];
```

### Extension points

Other plugins change what `ilium.core.worktrees` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.worktrees/<point>`). Contributors need `extensions.contribute:ilium.core.worktrees/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `before-create` | hook (transform) | Choose the branch name or refuse a new worktree. | repo, branch, base, agent; reply: branch | standard |
| `post-create-steps` | collect | Add a step that runs after a worktree is created. | entry: id, title, command, working_directory | standard |
| `before-remove` | hook (veto) | Refuse removing a worktree. | path, branch, dirty, merged | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.worktrees/create`: create: create a worktree and return its path

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.worktrees/changed`: { path, state }

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.worktrees"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replace)).

#### Example: Runs `bun install` in every new worktree

A package adds this entry in its manifest. It needs no code.

```json
{
  "api": "ilium:plugin@0.1.0",
  "capabilities": { "server": ["extensions.contribute:ilium.core.worktrees/post-create-steps"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.worktrees/post-create-steps",
        "id": "install-deps",
        "priority": 50,
        "entry": {
          "title": "Install dependencies",
          "command": "bun install"
        }
      }
    ]
  }
}
```

### ABI used

- [Commands](/pages/api-commands/) and [the command palette](/pages/api-command-palette/)
- [Workspace](/pages/api-workspace/) and [workspace.manage](/pages/capability-workspace-manage/)
- [panes.create](/pages/capability-panes-create/), [process control](/pages/api-process-control/), [intents](/pages/api-intents/) and [events](/pages/api-events/)
- [Messages and topics](/pages/api-messages-and-topics/)
- [fs](/pages/api-fs/), [fs.read](/pages/capability-fs-read/) and [file access](/pages/file-access/)
- [Text widgets](/pages/widgets-text/) and [status and feedback widgets](/pages/widgets-status-and-feedback/)
- [Settings extension](/pages/settings-extension/) and [settings row types](/pages/reference-settings-rows/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Sessions](/pages/core-plugin-sessions/)

### Security

Risk marks of this plugin's extension points follow the corrected table in [Extension security](/pages/reference-extension-security/) ([SEC-293]).
