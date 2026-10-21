# Sessions

## Sessions

A session is one project's tree of groups and panes together with the processes that run in it. Sessions outlive the terminal window: the user can detach and reattach, and after a restart the tree comes back from a snapshot. The plugin id is `ilium.core.sessions` and it runs on the server side only.

The snapshot files, the rolling backups, the ordered event journal and the `ilium`, `new-session`, `ls` and `kill-session` commands are kernel-native: the kernel writes and reads them in its own process before any plugin runs. The plugin drives the parts a plugin can reach through the contract: it relaunches verified agent sessions, freezes panes during a restore, stops processes gracefully, follows client attach and detach events, and creates a project entry.

### What it covers

- JSON snapshots and rolling snapshot backups, described in the session recovery guide.
- Restore after a restart and the recovery policy.
- Resuming verified agent sessions, so an agent pane continues its own conversation.
- Detach and reattach, and sessions per project.
- The ordered event journal.
- The commands `ilium`, `ilium new-session`, `ilium ls` and `ilium kill-session`, described in the CLI reference.

### See it

<figure><img src="/assets/demos/19-snapshot-resume.gif" alt="After a restart the panes return and an agent resumes its previous session" loading="lazy"><figcaption>The snapshot restores the layout and an agent pane resumes with its verified session id.</figcaption></figure>

<figure><img src="/assets/demos/24-agents-survive-reboots.gif" alt="Agents come back after the machine restarts" loading="lazy"><figcaption>Agents come back after the machine restarts, with their scrollback kept.</figcaption></figure>

<figure><img src="/assets/demos/11-detach-reattach.gif" alt="The user detaches the client and reattaches to find the panes still running" loading="lazy"><figcaption>Detach leaves the panes running. Reattach shows the current screen.</figcaption></figure>

### What the user sees

<figure>
<pre class="term">╭─ Restored ─────────────────────────────────────╮
│ Panes restored ………………………………………………… <span class="t-ok">7 of 8</span>      │
│ Agents resumed ………………………………………………… <span class="t-ok">3 of 3</span>      │
│ <span class="t-warn">Docs sweep is frozen</span>                           │
│ <span class="t-dim">  Its folder is missing. Press u to unfreeze.</span>  │
╰────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The restore summary: panes restored, agents resumed and one pane frozen.</figcaption>
</figure>

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium ls
{"type":"session","name":"default","state":"attached","panes":8}
{"type":"session","name":"review","state":"detached","panes":3}
{"type":"summary","sessions":2}
</pre>
<figcaption>Rendered at 80 columns or fewer. Illustrative. <code>ilium ls</code> prints one JSON line per session with its attach state, then a summary line.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

The contract version is 0.1. The plugin has a server side only.

```json
{
  "id": "ilium.core.sessions",
  "capabilities": {
    "server": ["panes.read", "panes.lifecycle", "tree.restructure", "agent.read"]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.sessions",
  "name": "Sessions",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "A session is one project's tree of groups and panes together with the processes that run in it.",
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
      "tree.restructure",
      "agent.read"
    ],
    "client": [
      "settings.write-kernel:session"
    ]
  },
  "activation": [
    "startup",
    "view:sessions.list"
  ],
  "limits_tier": "first-party"
}
```

### Kernel tabs this plugin owns

This plugin is the single owner of the following built-in Settings tabs. It holds `settings.write-kernel:<tab>` for each one (T1 only, one holder per tab, see [kernel settings](/pages/api-kernel-settings-and-keymap/#writing-a-kernel-tab)). The values stay in the same `config.toml` tables as before, so no user setting moves. [COR-030] [COR-033]

| Tab id | Label | Config table | Readers |
|---|---|---|---|
| `session` | Session | `[session]` | none |

If the plugin is disabled, the tab stays in the rail with its stored values shown read-only and the notice "Sessions is disabled. Values are kept." The kernel keeps reading the keys it consumes itself (listed in the Consumer column of the key table in [config and data migration](/pages/reference-config-and-data-migration/)). Behaviour that only this plugin provides is off. The exact result for this plugin is its row in [kernel and boot](/pages/reference-kernel-and-boot/). [COR-022]

### Samples

#### Resume verified agent sessions

After a restart each agent pane is a plain shell again. For every agent pane that has a verified session id, the plugin submits `replace-pane-command` with the resume command and `keep-scrollback`, so the old output stays above the resumed conversation. See [pane lifecycle](/pages/api-pane-lifecycle/), [panes.lifecycle](/pages/capability-panes-lifecycle/) and [pane lifecycle intents](/pages/api-intent-pane-lifecycle/).

**Rust**

```rust
use ilium_plugin_sdk::primitives::agent;
use ilium_plugin_sdk::types::intents::ReplacePaneCommandSpec;

fn resume_agent(pane: NodeId, template: &[String]) -> Option<Submission> {
    let session = agent::session(pane)?; // None until the id is verified
    // The signature's resume template holds the token {session}.
    let command: Vec<String> = template
        .iter()
        .map(|part| part.replace("{session}", &session.session_id))
        .collect();
    Some(Submission::keyed(
        Intent::ReplacePaneCommand(ReplacePaneCommandSpec {
            pane, command, cwd: None, keep_scrollback: true,
        }),
        format!("resume-{pane}"),
    ))
}
```

**TypeScript**

```ts
import { session } from '@ilium/plugin-sdk/primitives/agent';
import type { Submission } from '@ilium/plugin-sdk/types';

export function resumeAgent(pane: bigint, template: string[]): Submission | undefined {
  const ref = session(pane); // undefined until the id is verified
  if (ref === undefined) return undefined;
  const command = template.map((part) => part.replace('{session}', ref.sessionId));
  return {
    intent: { tag: 'replace-pane-command', val: { pane, command, cwd: undefined, keepScrollback: true } },
    correlation: 1n,
    dedupKey: `resume-${pane}`,
  };
}
```

#### Detach and reattach

Panes keep running while no client is attached. The plugin counts clients from `client-attached` and `client-detached`, and while the count is zero it slows its periodic work down. See [lifecycle](/pages/lifecycle/), [worlds and sides](/pages/worlds-and-sides/) and [events](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::host::timers;

fn on_event(&mut self, event: Event) {
    match event {
        Event::ClientAttached(_) => {
            self.clients += 1;
            self.set_poll(1_000);
        }
        Event::ClientDetached(_) => {
            self.clients = self.clients.saturating_sub(1);
            if self.clients == 0 {
                self.set_poll(30_000);
            }
        }
        _ => {}
    }
}

fn set_poll(&mut self, millis: u32) {
    if let Some(timer) = self.timer.take() {
        timers::cancel(timer);
    }
    self.timer = timers::set(millis, Some(millis)).ok();
}
```

**TypeScript**

```ts
import * as timers from '@ilium/plugin-sdk/host/timers';

let clients = 0;
let timer: bigint | undefined;

function setPoll(millis: number): void {
  if (timer !== undefined) timers.cancel(timer);
  timer = timers.set(millis, millis);
}

export function onEvent(event: { tag: string }): void {
  if (event.tag === 'client-attached') {
    clients += 1;
    setPoll(1_000);
  } else if (event.tag === 'client-detached') {
    clients = Math.max(0, clients - 1);
    if (clients === 0) setPoll(30_000);
  }
}
```

#### Freeze panes during a restore

A restored pane is paused until the layout is rebuilt, so no program prints into a half-built screen. The plugin submits `freeze-pane` for each restored pane and `unfreeze-pane` once every pane reports `running` through `pane-lifecycle-changed`. See [pane lifecycle](/pages/api-pane-lifecycle/), [pane lifecycle intents](/pages/api-intent-pane-lifecycle/) and [events](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::types::intents::{FreezePaneSpec, UnfreezePaneSpec};

fn freeze_all(panes: &[NodeId]) -> Vec<Submission> {
    panes.iter()
        .map(|pane| Submission::keyed(
            Intent::FreezePane(FreezePaneSpec { pane: *pane }), format!("freeze-{pane}")))
        .collect()
}

// Event::PaneLifecycleChanged(change) with change.state == "running":
//   self.waiting.remove(&change.pane); when empty, thaw everything.
fn thaw_all(panes: &[NodeId]) -> Vec<Submission> {
    panes.iter()
        .map(|pane| Submission::keyed(
            Intent::UnfreezePane(UnfreezePaneSpec { pane: *pane }), format!("thaw-{pane}")))
        .collect()
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk/types';

export const freezeAll = (panes: bigint[]): Submission[] =>
  panes.map((pane, index) => ({
    intent: { tag: 'freeze-pane', val: { pane } }, correlation: BigInt(index + 1), dedupKey: `freeze-${pane}`,
  }));

// case 'pane-lifecycle-changed': if (event.val.state === 'running') waiting.delete(event.val.pane);
//   when waiting.size === 0, call thawAll(restored).
export const thawAll = (panes: bigint[]): Submission[] =>
  panes.map((pane, index) => ({
    intent: { tag: 'unfreeze-pane', val: { pane } }, correlation: BigInt(index + 1), dedupKey: `thaw-${pane}`,
  }));
```

#### Stop gracefully before a snapshot

Panes that the recovery policy does not relaunch get a grace period to exit before the final snapshot. The plugin submits `terminate-pane-process` with `grace-ms` of 5000. See [pane lifecycle intents](/pages/api-intent-pane-lifecycle/).

**Rust**

```rust
use ilium_plugin_sdk::types::intents::TerminatePaneProcessSpec;

fn stop_gracefully(pane: NodeId) -> Submission {
    Submission::new(Intent::TerminatePaneProcess(TerminatePaneProcessSpec {
        pane,
        grace_ms: 5_000,
    }))
}
```

**TypeScript**

```ts
import type { Submission } from '@ilium/plugin-sdk/types';

export const stopGracefully = (pane: bigint): Submission => ({
  intent: { tag: 'terminate-pane-process', val: { pane, graceMs: 5000 } },
  correlation: 1n,
  dedupKey: undefined,
});
```

#### Sessions per project

Each project gets its own top-level entry in the tree. The plugin submits `create-project` for the canonical project directory and reads the outcome from the batch result. See [intents](/pages/api-intents/) and [tree structure intents](/pages/api-intent-tree-structure/).

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::types::intents::CreateProjectSpec;

fn open_project(root: &str, title: &str) -> Submission {
    Submission::keyed(
        Intent::CreateProject(CreateProjectSpec { title: title.into(), root: root.into() }),
        format!("project-{root}"),
    )
}

// Event::RequestCompleted(result): the outcome belongs to the submission's correlation.
//   log::log(Level::Info, &format!("create-project: {:?}", result.outcome));
```

**TypeScript**

```ts
import { log } from '@ilium/plugin-sdk/host/log';
import type { Submission } from '@ilium/plugin-sdk/types';

export const openProject = (root: string, title: string): Submission => ({
  intent: { tag: 'create-project', val: { title, root } },
  correlation: 1n,
  dedupKey: `project-${root}`,
});

// case 'request-completed': log('info', `create-project: ${event.val.outcome.tag}`);
```

### Extension points

Other plugins change what `ilium.core.sessions` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.sessions/<point>`). Contributors need `extensions.contribute:ilium.core.sessions/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `snapshot-sections` | collect | Store extra JSON in the session snapshot and receive it back on restore. | entry: id, max_bytes (the plugin answers `snapshot` and `restore` messages) | standard |
| `before-restore` | hook (observe) | Learn that a restore starts, to prepare state. | session, pane_count | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.sessions/snapshots`: list, save, restore: manage snapshots

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.sessions/saved`: { session, bytes }

**Replaceable:** no. Its data and its role in the Plugins screen make a replacement unsafe, so `replaceable` is `false`.

#### Example: Keeps a small JSON document with the session

A package adds this entry in its manifest. It needs no code.

```json
{
  "api": "ilium:plugin@0.1.0",
  "capabilities": { "server": ["extensions.contribute:ilium.core.sessions/snapshot-sections"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.sessions/snapshot-sections",
        "id": "my-notes",
        "priority": 50,
        "entry": {
          "max_bytes": 65536
        }
      }
    ]
  }
}
```

### ABI used

- [Pane lifecycle](/pages/api-pane-lifecycle/), [panes.lifecycle](/pages/capability-panes-lifecycle/) and [pane lifecycle intents](/pages/api-intent-pane-lifecycle/)
- [Lifecycle](/pages/lifecycle/), [worlds and sides](/pages/worlds-and-sides/) and [events](/pages/api-events/)
- [Intents](/pages/api-intents/) and [tree structure intents](/pages/api-intent-tree-structure/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Worktrees](/pages/core-plugin-worktrees/)
