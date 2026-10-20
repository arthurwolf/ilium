---
layout: page
title: Kernel and boot reference
permalink: /pages/reference-kernel-and-boot/
---

## Kernel and boot reference

This page states what stays in the Ilium kernel when every feature becomes a core plugin, in which order the server and the client start, which core plugin starts at boot and which starts on first use, and what the user keeps when a core plugin is disabled, replaced or crashes. It also lists the environment variables of the platform and says which changes apply live and which need a restart.

Contract version: `ilium:plugin@0.1.0`. The platform is not implemented. This page is the specification that the implementation and its tests follow. Requirement ids have the form `[COR-nnn]` and are collected in [Core plugin parity requirements](/pages/reference-requirements-core-parity/).

<div class="note">
<p>The word <strong>kernel</strong> means the Ilium code that is not a plugin: the server and client crates, the tree, the terminal engine, the settings store and the plugin host. The word <strong>core plugin</strong> means one of the 30 first-party plugins in the table of <a href="/pages/core-plugins/">Core plugins</a>. A core plugin is bundled, has tier T1 and an id that starts with <code>ilium.core.</code>.</p>
</div>

### Contents

- What stays in the kernel
- Server boot order
- Client boot order
- The CLI-only transient host
- Startup and lazy activation of the 30 core plugins
- The minimum kernel
- Safe start
- What the user keeps when a plugin is off
- Replaceable and disableable
- Scene world and extension points
- Environment variables
- Live reload and restart
- Errors and edge cases
- Requirements

### What stays in the kernel

A feature moves into a plugin only when the kernel can still run, draw a frame and save a session without it. The table lists the parts that never move, with the reason. Everything not listed here is a core plugin or part of one.

| Kernel part | Why it stays | What a plugin gets instead |
|---|---|---|
| PTY spawn, the `vt100` screen model, resize and mouse encoding | A plugin cannot own a file descriptor. The terminal must work with no plugin. | Reads through `terminal.read-screen`, `terminal.read-cells`, `terminal.history`. Writes through `terminal.write` and `terminal.raw-input` |
| The tree model: nodes, groups, panes, ids, undo of structure changes | Every plugin refers to node ids, and a wrong tree breaks every consumer | `tree.read`, `tree.write`, `tree.metadata`, `tree.decorate`, `tree.restructure` |
| IPC, framing, the transport and the version handshake | The plugin host itself rides on them | Typed records in `ilium-ipc` |
| Session snapshot writer, atomic restore and the final snapshot | Crash safety. A failed plugin must not lose a session | The `snapshot-sections` point of `ilium.core.sessions`, where a plugin adds a section with a `schema_version` |
| `config.toml` and `.ilium/config.yaml`: parse, validate, atomic write, defaults | One reader and one writer keep the files valid | `settings` for a plugin's own values, `kernel-settings` for kernel tabs, `settings.write-kernel:<tab>` for the one owner of a tab |
| The settings store and the 25 kernel tab ids | Frozen ids are the contract (see [kernel settings and keymap](/pages/api-kernel-settings-and-keymap/)) | The Settings plugin draws the screen. Each tab has one owner plugin |
| The keymap: chord parser, resolver, user remaps, the kernel commands | A broken binding must never lock the user out | `keymap.read`, `contributes.keybindings`, `keys.intercept` |
| Layout solver, compositor, frame diff, `ratatui` rendering, the theme | One frame budget, one theme | `ui` trees (retained mode), `ui.panels`, `ui.chrome` |
| Animation look application, dithering and cell painting | The look is global and applies after the scene draws | Scenes draw tone into a raster. The Ambient look plugin owns the controls |
| The scene host, cached loops and frame pacing | Frame pacing cannot wait for a plugin | Scene components in the animation world |
| The plugin host: package store, manifest checks, grants, trust, policy, quotas, the restart rule | A plugin cannot police itself | `plugins.manage` for the Plugins screen (T1 only) |
| The kernel recovery page and the command `kernel.open-recovery` | It must work with every plugin off | None. A plugin cannot remove it |
| The loopback HTTP listener on the configured API port (default 8872) | One process binds one socket, and the bind must not fail because a plugin is off | `http-api.route` lets a plugin register routes (see below) |
| Process and OS facts: process tree, runtime dirs, locks, secure files (`ilium-platform`) | Only one crate reaches the operating system | Imports such as `process`, `fs` and `clock` |
| CLI parsing of kernel commands: attach, `new-session`, `ls`, `kill-session`, `new-pane`, `plugin` | The user needs a way to reach a broken session | `contributes.cli` for every other command |

The HTTP listener is a kernel part because the port setting, the bind and the loopback rule belong to one owner. The Fleet plugin registers the route `POST /create_agent` with the grant `http-api.route:/create_agent`. If Fleet is off, the kernel answers `404` with `{"code":"not-found"}` for that path and keeps the socket open. See [HTTP API](/pages/api-http-api/).

### Server boot order

The server start in [Host architecture](/pages/reference-host-architecture/) has eight steps. This table adds the kernel parts that those steps refer to and the core plugins that take part. The restore always happens before any plugin exists, so no plugin sees the restore as a series of events.

| Step | Owner | What happens | If it fails |
|---|---|---|---|
| 1 | Kernel | Resolve runtime, config and data folders. Take the session lock. Read `config.toml` and `.ilium/config.yaml` | A file that fails to parse is moved aside as `config.toml.broken-<timestamp>` and defaults are used. The recovery page shows the message |
| 2 | Kernel | Restore the snapshot into the tree. Plugin sections stay as stored JSON. No plugin runs | A snapshot that fails to parse is moved aside the same way, and the session starts with one empty shell pane |
| 3 | Kernel | Apply the start flags `--no-plugins` and `--safe-plugins` and their environment variables | Both given: `--no-plugins` wins |
| 4 | Plugin host | Create the Engine, verify packages, register static contributions, build the service graph | A plugin that fails verification stays off. The others continue |
| 5 | Plugin host | Activate every server side whose `activation` contains `startup`, in dependency order | A failure counts for the restart rule |
| 6 | Plugin host | Deliver one `tree-changed` batch for the restored tree to each activated plugin that subscribes | None |
| 7 | Plugin host | Hand each plugin its stored snapshot section through the `snapshot-sections` point (see [the config migration page](/pages/reference-config-and-data-migration/#snapshot-sections)) | A section with an unknown `schema_version` is kept unchanged and the plugin is told `section-unsupported` |
| 8 | Kernel | Bind the IPC socket and accept clients. The HTTP listener binds here, and routes appear as their plugins register them | A client that connects early gets the retriable error `server.starting` |

[COR-020] has the order as a testable rule.

### Client boot order

| Step | Owner | What happens |
|---|---|---|
| 1 | Client kernel | Enter the alternate screen, read the theme and the keymap, connect to the socket, complete the handshake |
| 2 | Client kernel | Receive the tree, the pane viewports and the retained contribution replica |
| 3 | Client kernel | Draw the first frame with kernel chrome only. The tree uses its kernel rows (name and status glyph) until `ilium.core.tree` has drawn |
| 4 | Plugin host | Activate every client side with the `startup` trigger, in dependency order. The core plugins in this group are Tree, Settings, Ambient look, Agent toolbar, Terminal and Progress |
| 5 | Client kernel | Redraw with the plugin contributions. A plugin that is slow keeps its previous frame, and the client never waits for a plugin to draw |
| 6 | Plugin host | Activate other plugins when their trigger fires |

`startup` on a client side means the client host starts, and on a server side it means the server host starts. The same word is therefore used on both sides and starts only the side that declares it. See [Activation triggers](/pages/reference-manifest/#activation-triggers).

<figure>
<svg viewBox="0 0 520 170" role="img" aria-label="Server and client boot timelines">
<title>Boot timelines</title>
<desc>The server restores the snapshot, then the plugin host starts, then clients are accepted. The client draws a kernel-only first frame before its startup plugins.</desc>
<defs><marker id="boot-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker></defs>
<text x="8" y="22" font-size="12" fill="currentColor">Server</text>
<rect x="60" y="8" width="84" height="24" rx="4" fill="none" stroke="currentColor"/>
<text x="102" y="24" font-size="10" text-anchor="middle" fill="currentColor">config, lock</text>
<rect x="154" y="8" width="84" height="24" rx="4" fill="none" stroke="currentColor"/>
<text x="196" y="24" font-size="10" text-anchor="middle" fill="currentColor">restore</text>
<rect x="248" y="8" width="104" height="24" rx="4" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="300" y="24" font-size="10" text-anchor="middle" fill="currentColor">host, startup plugins</text>
<rect x="362" y="8" width="84" height="24" rx="4" fill="none" stroke="currentColor"/>
<text x="404" y="24" font-size="10" text-anchor="middle" fill="currentColor">accept clients</text>
<line x1="144" y1="20" x2="154" y2="20" stroke="currentColor" marker-end="url(#boot-arrow)"/>
<line x1="238" y1="20" x2="248" y2="20" stroke="currentColor" marker-end="url(#boot-arrow)"/>
<line x1="352" y1="20" x2="362" y2="20" stroke="currentColor" marker-end="url(#boot-arrow)"/>
<text x="8" y="92" font-size="12" fill="currentColor">Client</text>
<rect x="60" y="78" width="84" height="24" rx="4" fill="none" stroke="currentColor"/>
<text x="102" y="94" font-size="10" text-anchor="middle" fill="currentColor">connect</text>
<rect x="154" y="78" width="104" height="24" rx="4" fill="none" stroke="currentColor"/>
<text x="206" y="94" font-size="10" text-anchor="middle" fill="currentColor">kernel first frame</text>
<rect x="268" y="78" width="104" height="24" rx="4" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="320" y="94" font-size="10" text-anchor="middle" fill="currentColor">startup plugins</text>
<rect x="382" y="78" width="104" height="24" rx="4" fill="none" stroke="currentColor"/>
<text x="434" y="94" font-size="10" text-anchor="middle" fill="currentColor">full frame</text>
<line x1="144" y1="90" x2="154" y2="90" stroke="currentColor" marker-end="url(#boot-arrow)"/>
<line x1="258" y1="90" x2="268" y2="90" stroke="currentColor" marker-end="url(#boot-arrow)"/>
<line x1="372" y1="90" x2="382" y2="90" stroke="currentColor" marker-end="url(#boot-arrow)"/>
<line x1="404" y1="32" x2="102" y2="78" stroke="currentColor" stroke-dasharray="3 3" marker-end="url(#boot-arrow)"/>
<text x="60" y="140" font-size="10" fill="currentColor">Dashed boxes are plugin host steps. Solid boxes are kernel steps.</text>
<text x="60" y="156" font-size="10" fill="currentColor">The client is never blocked by a plugin: the kernel frame comes first.</text>
</svg>
<figcaption>Both timelines draw something usable before the first plugin runs. The dashed arrow shows that a client connects only after the server accepts clients.</figcaption>
</figure>

### The CLI-only transient host

A command from a plugin that declares `requires_server: false` can run with no session. The `ilium` process then creates a transient host. The rules:

- The transient host loads only the plugin that owns the command and the services that plugin `requires` (HST-019). It never activates another core plugin.
- Imports that need a session (`tree`, `panes` of the local session, `terminal`) return `unavailable` with the message `no session`. The command decides how to report that.
- Commands that look across sessions (`ilium panes`, `ilium broadcast`, `ilium recap`) list live sockets with `sessions.cross` and talk to each running server themselves. They do not need a local session.
- The transient host starts no server, takes no session lock and writes no snapshot.
- The host exits when the command finishes. Its time to first plugin call has the budget `CLI-COLD` in the performance budgets of the host.

The commands that work this way are `ilium panes`, `ilium broadcast`, `ilium recap` and `ilium voice say` (the last one only for its argument check). `ilium chat`, `ilium progress` and `ilium wait` need the session of the pane they run in and use the socket from `ILIUM_SESSION_SOCKET`. They set `requires_server: true`.

### Startup and lazy activation of the 30 core plugins

In the tables below, a `view:` trigger is written `view:<plugin short name>.<view id>`: the short name is the plugin id without its `ilium:` prefix, and the view id is the `id` declared in the plugin's `panels`, `views` or `overlays`. For Agent detect, `view:agent-detect.panel` is the view declared with the id `panel`.

A plugin that starts at boot costs time in every session. A plugin that starts on first use costs time at the first use. The rule used here: a plugin starts at boot only when it must see events from the first moment, must be present on the first frame, or must restore state that outlives the process. Everything else is lazy. The Plugins screen shows the column **Starts** with `at boot` or `on use`.

| Core plugin | Server side | Client side | Reason |
|---|---|---|---|
| Agent detect | `startup` | `view:agent-detect.panel` | Detection needs every screen update from the start |
| Agent setup | `settings:setup`, `command:refresh`, `service:install` | `settings:setup` | Used when the user opens Setup |
| Agent toolbar | `event:agent-activity-changed` | `startup` | Buttons are on the first frame |
| Ambient look | none | `startup` | The look is applied from the first frame |
| Chatroom | `startup` | `topic:chatroom` | File watch for `CHATROOM.md` and hook text must work without a client |
| Compaction | `event:agent-activity-changed`, `service:compact` | `view:compaction.optimizer` | Runs when an agent needs it |
| Cost | `startup` | `view:cost.panel` | Usage accrues from transcripts while nobody looks |
| Diagnostics | `startup` | `view:diagnostics.warnings` | Collects warnings from the first line of the log |
| Editor | none | `pane-kind:editor` | Native in v1 (see D4). In plugin form it starts when a file opens |
| Fleet | `cli:panes`, `cli:broadcast`, `http:/create_agent`, `service:broadcast` | `command:ask-for-update` | Used on demand |
| Inference | `service:ilium:inference/provider` | none | Starts when a consumer calls it |
| Kanban | none | `pane-kind:board` | Native in v1. In plugin form it starts with a board pane |
| Keys and help | none | `command:keys-and-help.open`, `settings:keyboard` | Used on demand. Bindings work without it |
| Layout | `command:layout.preset` | `view:layout.presets` | Used on demand. The kernel keeps the geometry |
| Notifications | `startup` | none | Must catch the first state change |
| Progress | `startup` | `startup` | Monitors are restored from storage and run without a client |
| Recap | `cli:recap`, `command:recap` | none | Used on demand |
| Scenes | none | `scene:*` | Starts when a scene is selected (scene side only) |
| Scheduler | `startup` | `view:scheduler.queue` | Due timers must fire after a restart |
| Search | `command:search`, `job:index` | `view:search.panel` | Used on demand |
| Session convert | `command:convert`, `service:convert` | `view:session-convert.progress` | Used on demand |
| Sessions | `startup` | `view:sessions.list` | Offers the snapshot sections to the kernel writer |
| Settings | none | `startup` | The Settings screen frame and the guided setup |
| Smart copy | none | `command:smart-copy` | Starts on the copy gesture |
| Terminal | `event:screen-changed` | `startup` | Links and the context menu need the first frame |
| Text triggers | `startup` | `view:text-triggers.rules` | The output filter must be in place before the first byte |
| Titles | `startup` | none | Triggers fire on agent events from the start |
| Tree | `startup` | `startup` | Rows, bookmarks and locks are on the first frame |
| Voice | `command:voice.toggle`, `cli:voice` | `command:voice.toggle` | The key F8 is a keybinding that fires the command, which starts the plugin |
| Worktrees | `command:new-agent`, `service:create` | none | Used on demand. `new-pane --worktree` calls the service |

[COR-020] and [COR-021] state the rules that make this table testable: the plugins marked `startup` appear in the activated list at boot, and the others do not appear until their trigger fires.

### The minimum kernel

The command `ilium --no-plugins` (or `ILIUM_NO_PLUGINS=1`) starts a session with the kernel only. The minimum the user must have is:

- terminal panes that run a shell, with input, output, scrolling, copy and resize;
- the tree with kernel rows (the node name and a status glyph), and new, close and rename;
- the layout geometry that was last saved;
- the default keymap, and the kernel commands `kernel.open-recovery`, `kernel.quit`, `kernel.detach`;
- the settings store, with the recovery page as the only Settings screen;
- the session snapshot writer.

<figure><pre class="term">╭─ ilium --no-plugins ─────────────────────────╮
│ Session: default   plugins: off              │
│                                              │
│ Pane 1  shell ………………………………………………………… running │
│                                              │
│ <span class="t-hi">Ctrl+Alt+R</span> recovery page                     │
╰──────────────────────────────────────────────╯</pre><figcaption>Rendered at 48 columns. Illustrative. The kernel frame with no plugin: one shell pane, a plain row and the key for the recovery page. There are no agent states, no cost, no toolbar and no animation.</figcaption></figure>

<figure><pre class="term">╭─ Recovery ───────────────────────────────────╮
│ Core plugins            <span class="t-dim">state</span>                │
│ ilium.core.fleet …………………………… off (3 crashes) │
│ ilium.core.tree ………………………………………………………………… on │
│ ilium.core.settings ……………………………………………………… on │
│                                              │
│ <span class="t-hi">Space</span> switch   <span class="t-hi">Esc</span> close                     │
╰──────────────────────────────────────────────╯</pre><figcaption>Rendered at 48 columns. Illustrative. The kernel recovery page lists each plugin with its state and a switch. It works with every plugin off.</figcaption></figure>

### Safe start

Two switches exist. They are defined in [Host architecture](/pages/reference-host-architecture/#safe-start) and are not repeated there under other names.

| Switch | Environment variable | Loads | Core plugins | Stored state |
|---|---|---|---|---|
| `ilium --safe-plugins` | `ILIUM_SAFE_PLUGINS=1` | Bundled T1 plugins with default grants | Run. Every `replaces` entry is ignored, so the built-in core plugin runs | Not changed |
| `ilium --no-plugins` | `ILIUM_NO_PLUGINS=1` | Nothing | Off. The kernel fallbacks in the next section apply | Not changed |

Neither switch is written to `installed.json` (see [LIF-004](/pages/reference-plugin-lifecycle-states/)). The state shown on the Plugins screen is `disabled-by-policy` with the reason `safe-mode` while the switch is on. The flags apply to the server that the launcher starts. A running server keeps its mode, so a user who needs safe mode runs `ilium kill-session <name>` first, then `ilium --safe-plugins`.

When a safe switch is on, the footer shows `Safe mode: third-party plugins are off.` or `Plugins are off. Press Ctrl+Alt+R for recovery.`

### What the user keeps when a plugin is off

A core plugin can be off for four reasons: the user turned it off, the restart rule turned it off after three failures within 10 minutes (see [Health and watchdog](/pages/reference-health-and-watchdog/)), a policy turned it off, or a safe switch is on. The effect is the same in all four. The kernel fallback column is what the user still has. Stored values, files and snapshot sections of an off plugin are never deleted.

| Core plugin | When it is off | Kernel fallback or what stays |
|---|---|---|
| Agent detect | Panes are plain terminals with no activity state. Plugins that consume agent events get none | Pane kind `terminal`, process name in the row |
| Agent setup | The Setup tab is gone. Managed blocks are not installed or refreshed | Blocks already on disk stay |
| Agent toolbar | No buttons on tree rows or agent frames | Palette commands of other plugins still work |
| Ambient look | The animation uses the kernel default look | Stored look values stay and apply when the plugin returns |
| Chatroom | `ilium chat` fails with `cli.owner-disabled`. Hooks print nothing | `CHATROOM.md` is untouched |
| Compaction | No compact action, no Optimization tab behaviour | Stored technique settings stay |
| Cost | No spend or quota decorations, no cost sort key. The Agent Cost and Reset planning tabs are read-only | Stored prices stay |
| Diagnostics | No report builder, no warnings panel | The kernel still writes the log file when the Debug switch is on |
| Editor | Files open through `open-external` | The editor is native in v1; its Plugins switch still works and the file opens externally |
| Fleet | `ilium panes`, `ilium broadcast` and `POST /create_agent` fail | `ilium new-pane` is a kernel command and stays |
| Inference | AI features report `inference.unavailable` and use their non-AI path | Provider settings stay |
| Kanban | Board panes show a placeholder. No new board | Board files stay on disk. In v1 the board is native |
| Keys and help | No key panel, hint bar or help browser | The keymap is kernel and works |
| Layout | Preset commands are gone | The last geometry is kept (HST) |
| Notifications | No sound, toast or desktop notice for agents | Kernel messages still show |
| Progress | `ilium progress` and `ilium wait` fail. Running monitors stop | Monitors registered before are restored when it returns |
| Recap | `ilium recap` fails | None |
| Scenes | No renderers. The background is blank | The look settings stay |
| Scheduler | Pending scheduled input does not fire and stays stored. Queued prompts are not sent when a turn finishes. The tree still shows "scheduled input pending" from the stored section | The section `ilium.core.scheduler` stays in the snapshot and fires on return, marked `late` |
| Search | No workspace search | None |
| Session convert | No conversion commands | Converted files stay |
| Sessions | No session list or recovery dialogs | The kernel snapshot writer still runs and keeps the last stored plugin sections |
| Settings | The Settings screen shows only the recovery page | `config.toml` can be edited by hand |
| Smart copy | Copy uses the plain selection | The kernel copy |
| Terminal | No links, no context-menu extras, no history transfer | The kernel terminal still works |
| Text triggers | No output filtering | Output reaches the screen unchanged |
| Titles | No automatic naming | Manual rename is a kernel command |
| Tree | Rows show the node name and a status glyph only | HST-042 |
| Voice | No voice. F8 is unbound | None |
| Worktrees | `ilium new-pane --worktree` fails with `cli.owner-disabled` | Plain `ilium new-pane` works |

The tree and the Settings screen are the only two core plugins whose absence changes the main screen. The kernel keeps a fallback for each, so the user can always reach the recovery page.

[COR-022] makes this table a test: for each row, disable the plugin, check the stated effect, and check that the kernel fallback and the recovery page work.

### Replaceable and disableable

Two properties are different.

- **Disableable.** The user can turn the plugin off in the Plugins screen. Every core plugin is disableable except the four essential plugins `ilium.core.tree`, `ilium.core.layout`, `ilium.core.settings` and `ilium.core.sessions` (LIF-183, LIF-276); Editor is disableable too, though it is native in v1 (see the table above).
- **Replaceable.** Another package may list the plugin id in `replaces` and provide its whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)). The manifest says `replaceable: true` and carries a `surface`.

Replaceable is stricter than disableable, because a replacement takes over stored data and a surface that other plugins call. Sessions and Settings are not replaceable (`replaceable: false`) because they own the stored sections and the settings screen that every other plugin depends on. The rest are replaceable. The rule for thin plugins that mostly drive a kernel part (Terminal, Tree, Layout) is: they are replaceable only if the replacement provides every service and point in their `surface`. A replacement that omits one is refused with `manifest.replaces-surface-incomplete`.

| Property | Sessions | Settings | Tree | All other core plugins |
|---|---|---|---|---|
| Disableable | yes | yes | yes | yes |
| Replaceable | no | no | yes | yes |
| Kernel fallback when off | snapshot writer | recovery page | plain rows | see the table above |

### Scene world and extension points

The scene world (`ilium:anim/base@0.1.0`) has no `extensions` import. A scene cannot provide or contribute to a point. This is the rule, and the earlier wording that a scene may extend the scene catalogue is withdrawn. Extension of scenes happens on the client and server sides of other plugins:

- A scene pack is a package with a scene side. The Scenes plugin lists packs through its point `scene-packs`, which a client or server side of the pack contributes to.
- A scene that needs data (a map, a weather feed) does not fetch it. The Scenes plugin runs the data source through `scene-data-sources` on a side that has `http`, and the scene reads the result through the `catalogs` and `geocode` imports.
- A scene that needs a shared look reads the palette with the `SceneEnv::palette` field in its constructor.

A package that has a scene side and wants to extend a point declares a second side (server or client) for the contribution. The manifest check refuses `extensions.*` grants under `capabilities.scene` with `manifest.grant-side`. See [Worlds and sides](/pages/worlds-and-sides/) and [Extension points](/pages/api-extension-points/).

### Environment variables

These variables are read by the platform. Variables that Ilium sets inside panes (`ILIUM_PANE_ID` and others) are in the [CLI reference](/pages/reference-cli-compatibility/#environment-variables-the-plugins-rely-on).

| Variable | Read by | Default | Effect |
|---|---|---|---|
| `ILIUM_NO_PLUGINS` | `ilium`, server | unset | `1` is the same as `--no-plugins` |
| `ILIUM_SAFE_PLUGINS` | `ilium`, server | unset | `1` is the same as `--safe-plugins`. Ignored when `ILIUM_NO_PLUGINS` is `1` |
| `ILIUM_CONFIG_DIR` | kernel | platform default | Folder of `config.toml`. Grants live in `$XDG_STATE_HOME/ilium/grants.json`, not here |
| `ILIUM_SOCKET_DIR` | kernel | runtime default | Folder of session sockets. Cross-session commands list this folder |
| `ILIUM_DEBUG_LOG_DIR` | kernel | platform default | Root of debug logs. The `log` import of a plugin writes there when Debug logging is on |
| `ILIUM_PLUGIN_LOG_LEVEL` | server | `info` | Default log threshold of plugins |
| `ILIUM_TRUST_STORE` | plugin CLI | per user | Absolute file path of the trust store |
| `ILIUM_PLUGIN_OFFLINE` | plugin CLI | unset | Same as `--offline` on `build` |
| `ILIUM_CHATROOM_AUTHOR`, `AGENT_NAME` | `ilium chat` | `agent` | Author of posted messages (read by the Chatroom plugin's command) |
| `NO_COLOR` | plugin CLI | unset | Turns colour off |
| `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, `XDG_RUNTIME_DIR` | kernel | system | Move the roots (see [Plugin files](/pages/reference-plugin-files/)) |

A plugin cannot read these variables. The `process` import passes only the environment that the manifest `process.run` grant allows, and nothing else (see [process.run](/pages/capability-process-run/)). A flag overrides the variable that has the same effect.

### Live reload and restart

Some changes apply at once. Some need the plugin, the client or the server to restart. The table is the contract.

| Change | Applies | Needs | Notes |
|---|---|---|---|
| A value on any kernel Settings tab | Live | Nothing | The kernel sends `kernel-settings-changed` and `settings-changed` |
| A plugin's own settings value | Live | Nothing | `settings-changed` to the plugin |
| A key binding | Live | Nothing | `keymap-changed` |
| Enable or disable a plugin | Live | Nothing | `deactivate(user)` or a lazy start |
| Update a plugin to a new version | Live for the side that is not busy | Plugin restart | The old version deactivates, the new one activates |
| Replace a core plugin | Live | Client reconnect for client sides | Stored data is handed over (see [Plugin lifecycle](/pages/lifecycle/)) |
| Grants (add, revoke) | Live | Nothing | A revoked grant ends open resources |
| `[plugins.limits]` in `config.toml` | Live for new instances | Plugin restart for running ones | The new caps apply to the next instance |
| Detection poll intervals, signatures, notification and sound rules, session recovery, progress enablement | Live | Nothing | Server-owned values reach the running server |
| The HTTP API port (`[api] port`) | At the next server start | Server restart | The listener is a kernel part. A change shows a notice `Applies after the server restarts` |
| `--no-plugins`, `--safe-plugins` | At server start | Server restart | A running server keeps its mode |
| A native Rust change of the kernel | Install of a new build | Server and client restart | The user restarts. Plugins are not involved |
| The WIT contract version of the kernel | Install of a new build | Server and client restart | A plugin built for another minor is refused or runs under the compatibility rule (see [Versioning](/pages/versioning/)) |
| Language of the interface | Live | Nothing | Not a plugin concern |

Code that reads a restart-only value gets the value that was current at the start. It never sees a half-applied change.

### Errors and edge cases

| Situation | Result | Code |
|---|---|---|
| A command of an off core plugin runs | Kernel prints the owner error and exits 1 | `cli.owner-disabled` |
| A client side activates with no server side of the same plugin | The client side runs with `unavailable` for server requests | `client-request.unavailable` |
| Two startup plugins require each other's services | Both stay off | `manifest.service-cycle` |
| `snapshot-sections` entry has an unknown `schema_version` | Section kept unchanged | `section-unsupported` |
| A route for `/create_agent` is registered twice | The second registration is refused | `http-api.route-taken` |
| `config.toml` fails to parse at boot | Moved aside, defaults used, recovery page opens | `config.parse-error` |
| A startup plugin takes longer than `HOST-START` | Boot continues. The plugin is reported late | `host.start-slow` (logged) |
| A scene side declares `extensions.provide` | Install refused | `manifest.grant-side` |

### Requirements

- [COR-020] The server MUST restore the snapshot before any plugin is instantiated, MUST activate plugins with the `startup` trigger in dependency order, and MUST bind the client socket last. Test: install a spy plugin with `startup` that records the tree on `activate`. The tree is already restored, and a client that connects before step 8 gets `server.starting`.
- [COR-021] Exactly the core plugins marked `startup` in the activation table MUST be active within `HOST-START` of boot, and every other core plugin MUST stay inactive until its trigger fires. Test: boot with no client, list active plugin sides through the Plugins screen, compare with the table.
- [COR-022] Disabling a core plugin MUST have the effect in the table "What the user keeps when a plugin is off", MUST NOT delete its stored data, and MUST leave the kernel fallback and the recovery page working. Test: for each of the 30 plugins, disable it, run the check listed in its row, enable it and compare stored data byte for byte.
- [COR-023] The scene world MUST NOT import `extensions`, and `capabilities.scene` MUST NOT contain `extensions.provide` or `extensions.contribute`. Test: a manifest with `extensions.contribute:…` under `capabilities.scene` fails with `manifest.grant-side`.
- [COR-024] Each change in the table "Live reload and restart" MUST apply as stated. Test: change one value of each kind with a client attached, and check whether the running side sees it without a restart.

### See also

- [Host architecture](/pages/reference-host-architecture/) and [Host runtime](/pages/reference-host-runtime/)
- [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/) and [Health and watchdog](/pages/reference-health-and-watchdog/)
- [Core plugins](/pages/core-plugins/) and [Extending core plugins](/pages/extending-core-plugins/)
- [Feature traceability](/pages/reference-feature-traceability/)
- [Config and data migration](/pages/reference-config-and-data-migration/)
- [CLI compatibility](/pages/reference-cli-compatibility/)
- [Roadmap](/pages/roadmap/)
