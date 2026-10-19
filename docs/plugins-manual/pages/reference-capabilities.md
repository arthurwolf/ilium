---
layout: page
title: Capability table
permalink: /pages/reference-capabilities/
---

## Capability table

A capability is a permission that unlocks one group of host functions or events. Every import a plugin can call, and every event it can receive, belongs to exactly one capability. The plugin lists the capabilities it needs in the `capabilities` object of its manifest, with one list per side. At install time the user grants them. First-party bundled (T1) plugins receive every capability up to the manifest ceiling automatically, except the high-risk ones marked H, which always need an explicit grant.

A declined capability does not stop the plugin. Its imports return `not-permitted`, its events are never sent, and the plugin keeps running. A call to a function whose grant the manifest never requested for that side is different: it is a trap `host.capability-not-requested`, which counts as a plugin failure.

### You will need

- A manifest with a `capabilities` object. Its keys are `server`, `client` and `scene`, and each value is a list of grant strings. The full manifest layout is in the [Manifest reference](/pages/reference-manifest/).
- For Rust: stable Rust 1.90 or later with the `wasm32-wasip2` target, and the crate `ilium-plugin-sdk`. For TypeScript: Bun 1.2 or later and the package `@ilium/plugin-sdk`. Both SDKs pin their binding generators, so nothing is installed globally.
- A working scaffold. The [first plugin guide](/pages/getting-started-first-plugin/) builds one step by step.

```json
{
  "capabilities": {
    "server": ["fs.read:project", "http:api.example.com"],
    "client": ["ui.panels"]
  }
}
```

The example omits the other manifest keys. Grants are per side, so a `server` grant never applies to `client`.

## Grant syntax

A grant is a name, optionally followed by a colon and an argument. The first colon separates the name from the argument, and later colons belong to the argument. So `services.use:ilium:inference/provider` has the name `services.use` and the argument `ilium:inference/provider`.

| Argument form | Grant | Example | Rejected |
|---|---|---|---|
| Exact global command id | `commands.invoke:<pattern>` | `commands.invoke:ilium.git/status` | `*/status` |
| All commands of one plugin | `commands.invoke:<plugin-id>/*` | `commands.invoke:ilium.git/*` | `ilium.git/prefix*` |
| Exact host, or any subdomain | `http:<host-pattern>` | `http:api.example.com`, `http:*.openstreetmap.org` | a bare `*`, a path |
| Loopback host with a port, plain HTTP allowed (H) | `http:localhost:<port>`, `http:127.0.0.1:<port>` | `http:localhost:8080` | `http:localhost` without a port (`manifest.grant-argument`) |
| Program name or absolute path | `process.run:<program>` | `process.run:git` | any wildcard, whitespace, an empty value, a relative path with `/`, or `..` (`manifest.grant-program`) |
| Program name or absolute path, same validation as `process.run` (H) | `panes.create-command:<program>` | `panes.create-command:htop` | the same values as `process.run` (`manifest.grant-program`) |
| Exact topic or prefix (normal); bare `*` for all topics (H) | `topics.subscribe:<pattern>` | `topics.subscribe:ilium.progress/*` | `*` in another position |
| Exact peer plugin id | `messages.peer:<plugin-id>` | `messages.peer:com.example.logger` | any wildcard |
| Service name without version | `services.use:<name>` | `services.use:com.example.git/status` | a version suffix |
| Route path with `{name}` segments | `http-api.route:<path>` | `http-api.route:/notes/{id}` | `*` |

A wildcard in a position that the table does not allow is refused at install with `manifest.grant-wildcard`. A `*` never crosses a `/` in a command id, except the bare `*`, which means every command.

### File scopes

The `fs` grants take a scope argument. Each scope has a fixed preopen name in the plugin's file view and a root on disk.

| Scope | Preopen | Root | Default |
|---|---|---|---|
| `package` | `/package` | the unpacked package `assets/` folder | always, read only |
| `data` | `/data` | the plugin's private data folder | always, read and write |
| `cache` | `/cache` | the plugin's private cache folder, evictable | always, read and write |
| `project` | `/project` | the session's canonical project folder | grant |
| `worktrees` | `/worktrees` | the workspace worktree root | grant, H for write |
| `picked` | `/picked/<n>` | each path the user chose in `fs.pick` | grant per pick |
| `agent-sessions:<agent>` | `/agent-sessions/<agent>` | the agent's transcript folder, read only | grant, H, read and watch only |
| `agent-config:<agent>` | `/agent-config/<agent>` | the agent's configuration folder | grant, H for read; a write grant is refused, changes go through `agent-config.propose:<agent>` (H) |
| `minecraft-saves` | `/minecraft-saves` | the Minecraft saves folder | grant |
| `path:<template>` | `/paths/<n>` | a template with `{home}`, `{config}`, `{data}`, `{cache}`, `{project}` or `{worktrees}` | grant |

A `path:` template ends either with a name (that file or folder, plus the direct children of a folder) or with `/**` (the whole subtree). A literal absolute path such as `path:/srv/logs/**` is always H. A path that escapes its scope root with `..`, or through a symbolic link, is refused with `not-permitted`.

<div class="note">

Scope examples: `fs.read:project` reads the project folder. It is a normal-risk grant and starts checked. `fs.read:path:{home}/Pictures/**` reads a subtree of the user's Pictures folder.

</div>

## Always granted

These need no manifest entry and no user grant:

- `log`, `clock`, `timers`, `storage` (the plugin's own buckets), `settings` (the plugin's own values) and `codec`.
- `fs.read:package`, `fs.read:data`, `fs.write:data`, `fs.read:cache` and `fs.write:cache`, and `fs.watch:package`, `fs.watch:data` and `fs.watch:cache`. `fs.watch:<scope>` has the same risk as `fs.read:<scope>`. A watch on a scope without a grant returns `not-permitted` with the reason `not-granted`.
- The plugin's own-side `messages`, and `ui.invalidate` for the views the plugin contributes.

## Table

Sides: S is the server world, C is the client world and A is the animation (scene) world. Risk H means the capability always needs an explicit grant. The last column names what the grant unlocks.

Every grant is listed once. Capabilities with several grants share one row.

| Grant | Argument form | Sides | Risk | Page | API pages |
|---|---|---|---|---|---|
| `tree.read` | none | S C | normal | [page](/pages/capability-tree-read/) | [api-tree](/pages/api-tree/) |
| `tree.write` | none | S C | normal | [page](/pages/capability-tree-write/) | [api-tree](/pages/api-tree/), [api-intents](/pages/api-intents/) |
| `tree.metadata` | none | S C | normal | [page](/pages/capability-tree-metadata/) | [api-tree](/pages/api-tree/) |
| `tree.decorate` | none | S C | normal | [page](/pages/capability-tree-decorate/) | [api-tree](/pages/api-tree/) |
| `tree.restructure` | none | S C | **H** | [page](/pages/capability-tree-restructure/) | [api-tree](/pages/api-tree/), [api-intents](/pages/api-intents/) |
| `tree.attention` | none | S C | normal | [page](/pages/capability-tree-attention/) | [api-tree](/pages/api-tree/), [api-events](/pages/api-events/) |
| `panes.read` | none | S C | normal | [page](/pages/capability-panes-read/) | [api-panes](/pages/api-panes/) |
| `panes.create` | none | S C | normal | [page](/pages/capability-panes-create/) | [api-panes](/pages/api-panes/) |
| `panes.signal` | none | S | **H** | [page](/pages/capability-panes-signal/) | [api-panes](/pages/api-panes/) |
| `panes.lifecycle` | none | S | **H** | [page](/pages/capability-panes-lifecycle/) | [api-panes](/pages/api-panes/), [api-intents](/pages/api-intents/) |
| `panes.provide-kind` | none | C | normal | [page](/pages/capability-panes-provide-kind/) | [api-panes](/pages/api-panes/) |
| `terminal.read-screen` | none | S C | normal | [page](/pages/capability-terminal-read-screen/) | [primitives-terminal](/pages/primitives-terminal/) |
| `terminal.read-cells` | none | S C | normal | [page](/pages/capability-terminal-read-cells/) | [primitives-terminal](/pages/primitives-terminal/) |
| `terminal.history` | none | S C | **H** | [page](/pages/capability-terminal-history/) | [primitives-terminal](/pages/primitives-terminal/) |
| `terminal.output-filter` | none | S | normal | [page](/pages/capability-terminal-output-filter/) | [primitives-terminal](/pages/primitives-terminal/) |
| `terminal.write` | none | S C | **H** | [page](/pages/capability-terminal-write/) | [primitives-terminal](/pages/primitives-terminal/), [api-intents](/pages/api-intents/) |
| `terminal.input-lock` | none | S C | **H** | [page](/pages/capability-terminal-input-lock/) | [primitives-terminal](/pages/primitives-terminal/), [api-intents](/pages/api-intents/) |
| `terminal.raw-input` | none | S C | **H** | [page](/pages/capability-terminal-raw-input/) | [primitives-terminal](/pages/primitives-terminal/), [widgets-media-and-embedded](/pages/widgets-media-and-embedded/) |
| `terminal.pointer` | none | C | normal | [page](/pages/capability-terminal-pointer/) | [primitives-terminal](/pages/primitives-terminal/) |
| `agent.read` | none | S C | normal | [page](/pages/capability-agent-read/) | [primitives-agent](/pages/primitives-agent/) |
| `agent.read-prompts` | none | S C | **H** | [page](/pages/capability-agent-read-prompts/) | [primitives-agent](/pages/primitives-agent/) |
| `agent.control` | none | S C | **H** | [page](/pages/capability-agent-control/) | [primitives-agent](/pages/primitives-agent/) |
| `agent.signatures` | none | S | normal | [page](/pages/capability-agent-signatures/) | [primitives-agent](/pages/primitives-agent/) |
| `agent.transcript` | none | S C | **H** | [page](/pages/capability-agent-transcript/) | [primitives-agent](/pages/primitives-agent/) |
| `editor.read` | none | C | normal | [page](/pages/capability-editor-read/) | [primitives-editor](/pages/primitives-editor/) |
| `editor.edit` | none | C | normal | [page](/pages/capability-editor-edit/) | [primitives-editor](/pages/primitives-editor/) |
| `editor.provide` | none (reserved, refused in 0.1.0) | C | **H** | [page](/pages/capability-editor-provide/) | [primitives-editor](/pages/primitives-editor/) |
| `ui.panels` | none | C | normal | [page](/pages/capability-ui-panels/) | [api-ui](/pages/api-ui/), [widgets-index](/pages/widgets-index/) |
| `ui.chrome` | none | S C | normal | [page](/pages/capability-ui-chrome/) | [api-ui](/pages/api-ui/), [api-intents](/pages/api-intents/) |
| `keys.intercept` | none | C | **H** | [page](/pages/capability-keys-intercept/) | [api-keys](/pages/api-keys/) |
| `keymap.read` | none | C | normal | [page](/pages/capability-keymap-read/) | [api-keys](/pages/api-keys/) |
| `commands.invoke:<pattern>` | pattern; exact id, plugin-id/*, bare * (high) | S C | H if * | [page](/pages/capability-commands-invoke/) | [api-commands](/pages/api-commands/) |
| `fs.read:<scope>` | scope, including agent-sessions:gemini, agent-config:gemini, agent-config:antigravity | S C A | normal; H for agent-sessions and agent-config | [page](/pages/capability-fs-read/) | [api-fs](/pages/api-fs/) |
| `fs.write:<scope>` | scope; `agent-sessions` is refused with `manifest.grant-scope`; `agent-config` is refused with `manifest.grant-scope` (use `agent-config.propose:<agent>`) | S C A | H outside data and cache | [page](/pages/capability-fs-write/) | [api-fs](/pages/api-fs/) |
| `fs.watch:<scope>` | scope; `agent-config` is refused with `manifest.grant-scope` | S C | same as `fs.read:<scope>`; `package`, `data` and `cache` are always granted | [page](/pages/capability-fs-watch/) | [api-fs](/pages/api-fs/) |
| `agent-config.propose:<agent>` | one agent: `claude`, `codex`, `gemini` or `antigravity`; no wildcard (`manifest.grant-wildcard`); unknown agent `manifest.grant-scope`; needs `fs.read:agent-config:<agent>` on the same side, else `manifest.grant-dependency` | S C | **H** | [page](/pages/capability-agent-config-propose/) | [api-fs](/pages/api-fs/) |
| `fs.pick` | none | C | normal | [page](/pages/capability-fs-pick/) | [api-fs](/pages/api-fs/) |
| `jobs.run` | none | S C A | normal | [page](/pages/capability-jobs-run/) | [api-jobs](/pages/api-jobs/) |
| `http:<host-pattern>` | host pattern, `http:setting:<key>`, or loopback with a port | S C A | normal; H for loopback and private-address literals | [page](/pages/capability-http/) | [api-http](/pages/api-http/) |
| `process.run:<program>` | program name or absolute path | S C | **H** | [page](/pages/capability-process-run/) | [api-process-control](/pages/api-process-control/) |
| `panes.create-command:<program>` | program name or absolute path; requires `panes.create` in the same side, else `manifest.grant-dependency` | S C | **H** | [page](/pages/capability-panes-create/) | [api-panes](/pages/api-panes/) |
| `process.long-lived` | none; needs process.run:<program> | S C | **H** | [page](/pages/capability-process-long-lived/) | [api-process-control](/pages/api-process-control/) |
| `inference` | none | S C | normal | [page](/pages/capability-inference/) | [api-inference](/pages/api-inference/) |
| `inference.provide` | none | S C | normal | [page](/pages/capability-inference-provide/) | [api-inference](/pages/api-inference/) |
| `inference.embed` | none | S C | normal | [page](/pages/capability-inference-embed/) | [api-inference](/pages/api-inference/) |
| `services.use:<name>`, `services.provide:<name>` | service name | S C | normal | [page](/pages/capability-services/) | [api-services](/pages/api-services/) |
| `sound.play` | none | S C | normal | [page](/pages/capability-sound-play/) | [api-notifications](/pages/api-notifications/) |
| `notify.desktop` | none | S C | normal | [page](/pages/capability-notify-desktop/) | [api-notifications](/pages/api-notifications/) |
| `notify.toast` | none | S C | normal | [page](/pages/capability-notify-toast/) | [api-notifications](/pages/api-notifications/), [api-intents](/pages/api-intents/) |
| `open-external` | none | S C | normal | [page](/pages/capability-open-external/) | [api-intents](/pages/api-intents/) |
| `clipboard.read`, `clipboard.write` | none | C | read is high | [page](/pages/capability-clipboard/) | [api-clipboard](/pages/api-clipboard/) |
| `audio.capture`, `audio.loopback`, `audio.playback` | none | C A | capture and loopback high | [page](/pages/capability-audio/) | [api-audio](/pages/api-audio/) |
| `media.video` | none | C A | normal | [page](/pages/capability-media-video/) | [api-media](/pages/api-media/) |
| `location.observer` | none | C A | **H** | [page](/pages/capability-location-observer/) | [api-location](/pages/api-location/) |
| `location.geocode` | none | C A | normal | [page](/pages/capability-location-geocode/) | [api-location](/pages/api-location/) |
| `messages.peer:<plugin-id>` | one plugin id, no wildcard | S C | normal | [page](/pages/capability-messages-peer/) | [api-messages-and-topics](/pages/api-messages-and-topics/) |
| `topics.subscribe:<topic-pattern>` | topic pattern | S C | normal; bare * high | [page](/pages/capability-topics-subscribe/) | [api-messages-and-topics](/pages/api-messages-and-topics/) |
| `sessions.cross` | none | S | **H** | [page](/pages/capability-sessions-cross/) | [api-commands](/pages/api-commands/) |
| `cli.contribute` | none | S | normal | [page](/pages/capability-cli-contribute/) | [api-cli](/pages/api-cli/) |
| `http-api.route:<path>` | route path | S | normal | [page](/pages/capability-http-api-route/) | [api-http-api](/pages/api-http-api/) |
| `workspace.manage` | none | S | **H** | [page](/pages/capability-workspace-manage/) | [api-workspace](/pages/api-workspace/) |
| `settings.read-all`, `settings.write-all` | none (T1 only; `manifest.grant-tier` otherwise) | C | **H**, T1 only | [page](/pages/capability-settings-all/) | [api-settings](/pages/api-settings/) |
| `settings.contribute:<kernel-tab-id>` | one kernel tab id, no wildcard | S C | normal | [page](/pages/capability-settings-contribute/) | [api-settings](/pages/api-settings/) |
| `settings.write-kernel:<kernel-tab-id>` | one built-in tab id, no wildcard (`manifest.grant-argument` otherwise); T2 and T3 get `manifest.grant-tier`; at most one installed plugin holds each tab (`manifest.settings-owner` for a second) | S | **H**, T1 only | [page](/pages/capability-settings-write-kernel/) | [api-settings](/pages/api-settings/) |
| `settings.read-kernel:<kernel-tab-id>` | one kernel tab id, no wildcard | S C | normal; H for api, at every tier | [page](/pages/capability-settings-read-kernel/) | [api-settings](/pages/api-settings/) |
| `plugins.manage` | none (T1 only; `manifest.grant-tier` otherwise) | C | **H**, T1 only | [page](/pages/capability-plugins-manage/) | [api-help-and-plugins](/pages/api-help-and-plugins/) |
| `help.read` | none | C | normal | [page](/pages/capability-help-read/) | [api-help-and-plugins](/pages/api-help-and-plugins/) |
| `journal.read` | none | S C | normal | [page](/pages/capability-journal-read/) | [api-clock-and-timers](/pages/api-clock-and-timers/) |
| `gpu.kernels` | none; development build only, `@unstable(feature = gpu-kernels)`; a release install gives `manifest.grant-unknown` | A | normal (unstable) | [page](/pages/capability-gpu-kernels/) | [api-scenes](/pages/api-scenes/) |
| `extensions.provide:<plugin-id>/<point>` | one global point id of the plugin's own, no wildcard | S C A | normal | [page](/pages/capability-extensions-provide/) | [api-extension-points](/pages/api-extension-points/) |
| `extensions.contribute:<plugin-id>/<point>` | one global point id, no wildcard | S C | normal; high when the point declares `risk: "high"` | [page](/pages/capability-extensions-contribute/) | [api-extension-points](/pages/api-extension-points/) |

### Newer grants

The grants below extend the base set. Twelve of them are high risk and always need an explicit grant, even for a T1 bundled plugin: `terminal.history`, `terminal.input-lock`, `terminal.raw-input`, `panes.lifecycle`, `tree.restructure`, `sessions.cross`, `process.long-lived`, `agent.transcript`, `settings.read-kernel:api`, `settings.write-kernel:<tab-id>` (T1 only), `panes.create-command:<program>` and `agent-config.propose:<agent>`. The grant `gpu.kernels` is unstable and exists only in development builds. Both `extensions.*` grants take exactly one global point id and refuse wildcards (`manifest.grant-wildcard`). `extensions.contribute` is high risk only for a point that declares `risk: "high"`; the review dialog then shows a warning. Four argument forms are less common: `http:setting:<key>` (the host comes from one of the plugin's own settings, and the user confirms a changed host before the first request), `fs.read:agent-sessions:gemini` with `fs.read:agent-config:gemini`, `fs.read:agent-config:antigravity`, and `settings.read-kernel:<kernel-tab-id>` (argument required, no wildcard).

| Grant | Page |
|---|---|
| `tree.restructure` | [page](/pages/capability-tree-restructure/) |
| `tree.attention` | [page](/pages/capability-tree-attention/) |
| `panes.lifecycle` | [page](/pages/capability-panes-lifecycle/) |
| `terminal.read-cells` | [page](/pages/capability-terminal-read-cells/) |
| `terminal.history` | [page](/pages/capability-terminal-history/) |
| `terminal.input-lock` | [page](/pages/capability-terminal-input-lock/) |
| `terminal.raw-input` | [page](/pages/capability-terminal-raw-input/) |
| `agent.transcript` | [page](/pages/capability-agent-transcript/) |
| `ui.chrome` | [page](/pages/capability-ui-chrome/) |
| `keymap.read` | [page](/pages/capability-keymap-read/) |
| `panes.create-command:<program>` | [page](/pages/capability-panes-create/) |
| `process.long-lived` | [page](/pages/capability-process-long-lived/) |
| `inference.embed` | [page](/pages/capability-inference-embed/) |
| `notify.toast` | [page](/pages/capability-notify-toast/) |
| `open-external` | [page](/pages/capability-open-external/) |
| `location.geocode` | [page](/pages/capability-location-geocode/) |
| `sessions.cross` | [page](/pages/capability-sessions-cross/) |
| `settings.read-kernel:<kernel-tab-id>` | [page](/pages/capability-settings-read-kernel/) |
| `settings.write-kernel:<kernel-tab-id>` | [page](/pages/capability-settings-write-kernel/) |
| `journal.read` | [page](/pages/capability-journal-read/) |
| `gpu.kernels` | [page](/pages/capability-gpu-kernels/) |
| `extensions.provide:<plugin-id>/<point>` | [page](/pages/capability-extensions-provide/) |
| `extensions.contribute:<plugin-id>/<point>` | [page](/pages/capability-extensions-contribute/) |
| `agent-config.propose:<agent>` | [page](/pages/capability-agent-config-propose/) |

Each capability has its own page with the full behaviour, for example [capability-http](/pages/capability-http/), [capability-fs-read](/pages/capability-fs-read/), [capability-tree-read](/pages/capability-tree-read/) and [capability-process-run](/pages/capability-process-run/).

## Who grants what

The risk tier decides how the user sees a capability.

- **Normal.** Listed in the review dialog and checked by default. A T2 or T3 package installs with the grants the user leaves checked.
- **High (H).** Listed but unchecked, so the user must tick each one. A T1 bundled plugin starts without its H capabilities. At its first activation the user sees one prompt, "Plugin <name> asks for: <list>", with the buttons `Allow selected` and `Not now`. `Not now` asks again at the next server start, at most three times, and the user can grant later from Settings > Plugins.

There is no prompt for each call. A grant is a standing permission until it is revoked. The review dialog shows one row per capability, grouped by side, with the grant string at the end of the row:

<pre class="term"><span class="t-dim">╭─ Install com.example.notes 1.2.0 ──────────────────────────────────╮</span>
<span class="t-dim">│</span> Example Corp · signed (T2) · fingerprint 3F2A…9C01                 <span class="t-dim">│</span>
<span class="t-dim">│</span>                                                                    <span class="t-dim">│</span>
<span class="t-dim">│</span> <span class="t-hi">Server</span>                                                             <span class="t-dim">│</span>
<span class="t-dim">│</span> <span class="t-ok">[x]</span> Read the workspace tree ………………………………………………………………………… tree.read <span class="t-dim">│</span>
<span class="t-dim">│</span> <span class="t-ok">[x]</span> Read files in this project ………………………………………………… fs.read:project <span class="t-dim">│</span>
<span class="t-dim">│</span> <span class="t-dim">[ ]</span> Connect to api.example.com …………………………………… http:api.example.com <span class="t-dim">│</span>
<span class="t-dim">│</span>                                                                    <span class="t-dim">│</span>
<span class="t-dim">│</span> Unchecked capabilities are not granted. Turn on the ones you need. <span class="t-dim">│</span>
<span class="t-dim">│</span>               [ Install ]   [ Details ]   <span class="t-hi">[ Cancel ]</span>               <span class="t-dim">│</span>
<span class="t-dim">╰────────────────────────────────────────────────────────────────────╯</span>
</pre>

Illustrative, not generated. The user left the HTTP grant unchecked, so the plugin's `granted` list omits it. Focus starts on Cancel ([SEC-103]). Up and Down move, Space toggles, Enter activates and Escape cancels.

## Changing grants while the plugin runs

A grant change never restarts a plugin silently. The rules are:

- **Grant added.** An added grant bumps the authorization epoch. The plugin receives `deactivate(granted)` and is re-activated with the new set.
- **Grant revoked.** Revoke in Settings > Plugins > <plugin> > Capabilities, or with `ilium plugin grants <id> --revoke <grant>`. A host call records the epoch at entry. If the epoch changes before the call returns and the change removed a grant the call needs, the call returns `revoked` (an intent completes `failed(revoked)`); otherwise it completes normally. A call that enters after the bump is checked against the new set and returns `not-permitted` with a message starting `revoked: `, until re-activation and after it. The plugin receives `deactivate(revoked)` and is re-activated with the reduced set and its restore bytes. Contributions that need the grant (decorations, grabs, claims, filters, watches, routes) are withdrawn at once.
- **Failure counting.** A grant change never counts as a plugin failure.

The activation context carries `granted: list<string>`. Each entry is the grant string exactly as the manifest spells it, such as `http:api.example.com`, so a plugin can test for it directly. Declined grants are absent from the list.

## Worked example in Rust

This server-side plugin reads a project file and calls one HTTP host. It starts the HTTP call only when the user granted that host. The example uses the manifest from the top of this page.

**Rust**

```rust
use ilium_plugin_sdk::host::{fs, http, log::{self, Level}};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::exports::Activation;

const FETCH_GRANT: &str = "http:api.example.com";

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct NotesReader;

impl Lifecycle for NotesReader {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        // fs.read:project is in the manifest, so this read is granted unless declined.
        task::spawn(async {
            let path = FsPath { scope: "project".into(), path: "NOTES.md".into() };
            match fs::read_all(path).await {
                Ok(bytes) => log::log(Level::Info, &format!("read {} bytes", bytes.len())),
                Err(error) => log::log(Level::Warn, &format!("read failed: {:?}", error.code)),
            }
        });
        // A declined grant is missing from `granted`, so the feature is skipped, not called.
        if context.granted.iter().any(|grant| grant == FETCH_GRANT) {
            task::spawn(async {
                let request = http::HttpRequest {
                    method: "GET".into(),
                    url: "https://api.example.com/v1/status".into(),
                    headers: Vec::new(),
                    body: None,
                    timeout_millis: 5_000,
                };
                match http::fetch(request).await {
                    Ok(response) => log::log(Level::Info, &format!("status {}", response.status)),
                    Err(error) => log::log(Level::Warn, &format!("fetch failed: {:?}", error.code)),
                }
            });
        }
        Ok(())
    }
}
```

Each `async` wrapper resolves when its `request-completed` event arrives.

## Worked example in TypeScript

**TypeScript**

```ts
import { definePlugin, PluginError } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';
import { readAll } from '@ilium/plugin-sdk/host/fs';
import { fetch as httpFetch } from '@ilium/plugin-sdk/host/http';

const FETCH_GRANT = 'http:api.example.com';

async function readNotes(): Promise<void> {
  try {
    const bytes = await readAll({ scope: 'project', path: 'NOTES.md' });
    log('info', `read ${bytes.length} bytes`);
  } catch (error) {
    if (error instanceof PluginError) log('warn', `read failed: ${error.code}`);
  }
}

async function fetchStatus(): Promise<void> {
  try {
    const response = await httpFetch({
      method: 'GET',
      url: 'https://api.example.com/v1/status',
      headers: [],
      body: undefined,
      timeoutMillis: 5000,
    });
    log('info', `status ${response.status}`);
  } catch (error) {
    if (error instanceof PluginError) log('warn', `fetch failed: ${error.code}`);
  }
}

export const { lifecycle } = definePlugin({
  side: 'server',
  activate(context) {
    void readNotes();
    if (context.granted.includes(FETCH_GRANT)) void fetchStatus();
  },
});
```

The `granted` check only saves a refused call. A call without a grant returns `not-permitted` anyway.

## Errors and edge cases

Every function that returns a result can also return `not-permitted`, `revoked` and `internal`. The table shows what each capability situation looks like to the plugin.

| Situation | Result |
|---|---|
| Import of an interface that no requested capability on that side unlocks | The install fails with `manifest.import-not-requested`. |
| Call of a function whose grant the manifest never requested for that side (also when it is requested only for the other side) | Trap `host.capability-not-requested`, counted as a plugin failure. |
| User left the grant unchecked or declined it | `not-permitted` at once, synchronously, with a message starting `declined: `. The plugin keeps running. |
| Argument outside the grant (host, scope, program, command) | `not-permitted` at once, with the reason `not-granted` (`scope` for a path outside the granted scope). |
| `create-pane` with a command whose program matches no `panes.create-command` grant | `refused(not-permitted)` with the reason `not-granted` (`declined` when the grant was requested but unchecked). A command-less `create-pane` needs only `panes.create`. |
| Path escapes the scope with `..` or a symbolic link | `not-permitted`. |
| Redirect to a host without a grant | Completion `failed(not-permitted)`. |
| Grant revoked while a call is in flight | `revoked`. The plugin is re-activated. |
| Declined CLI subcommand | Hidden from help. `ilium <name>` fails with `unknown command`. |
| Declined HTTP API route | `404 Not Found`, the same as an unknown path. |
| Declined command invocation | `not-permitted`, and the command does not run. |

Manifest mistakes stop the install. Each one is reported as a JSONL `error` line by `ilium plugin install` and `ilium plugin build`:

| Code | Text |
|---|---|
| `manifest.grant-unknown` | `capability "<grant>" is not known to plugin API <api>`. Also used for a reserved grant (`editor.provide`), an alias (the message names the right grant, for example `use terminal.raw-input`) and `gpu.kernels` in a release build |
| `manifest.grant-argument` | `capability "<name>" takes no argument (got "<grant>")` |
| `manifest.grant-argument-missing` | `capability "<name>" needs an argument, for example "<name>:<example>"` |
| `manifest.grant-wildcard` | `capability "<grant>" uses a wildcard that "<name>" does not accept` |
| `manifest.grant-scope` | `file scope "<scope>" is not known or does not accept "<access>"` |
| `manifest.grant-side` | `capability "<name>" is not available on the <side> side` |
| `manifest.grant-tier` | `capability "<name>" is available to bundled plugins only` |
| `manifest.grant-dependency` | `capability "<grant>" also needs "<other>"` |
| `manifest.grant-program` | `capability "<name>" takes a program name or an absolute path (got "<value>")` |
| `manifest.import-not-requested` | `import "<interface>" is not unlocked by any capability requested on the <side> side` |
| `manifest.settings-owner` | `another plugin already owns the settings tab "<tab>"` |
| `manifest.export-grant` | `export "<export>" needs capability "<grant>"` |

`settings.write-all` without `settings.read-all` fails with `manifest.grant-dependency`, and so does `fs.pick` without `fs.read:picked` or `fs.write:picked`.

<div class="warn">

A misspelled host such as `http:api.example.co` is valid syntax. It installs, and every call to the real host returns `not-permitted`.

</div>

## What the user sees

Each refused call is logged. The text form of a refusal is `refused <grant> by <function> (<reason>)`. The log file holds the same refusal as one JSON line, `{"type":"refusal","grant":"<grant>","function":"<function>","reason":"<reason>"}`, and `ilium plugin logs <id>` shows it in plain text. The reason is one of the closed list in [Refusal reasons](/pages/reference-errors-and-limits/#refusal-reasons). Writes outside `data` and `cache` are logged at info level.

<pre class="term"><span class="t-prompt">$</span> ilium plugin logs com.example.notes
<span class="t-dim">2026-10-11 14:20:33</span> <span class="t-ok">info</span> read 1842 bytes
<span class="t-dim">2026-10-11 14:20:35</span> <span class="t-warn">warn</span> refused http:api.example.com by http.fetch (declined)
<span class="t-dim">2026-10-11 14:20:35</span> <span class="t-err">err</span>  fetch failed: not-permitted
</pre>

Illustrative, not generated. The plugin keeps running after the refusal. The HTTP call is skipped on the next activation because the grant is still declined.

Clipboard reads also show in the client footer as `clipboard read by <plugin name>` for 3 seconds after each read.

The figure shows how one import call is decided.

<figure>
<svg viewBox="0 0 400 230" role="img" aria-label="Manifest and user grant feed a host check that runs or refuses an import">
  <defs>
    <marker id="cap-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="20" width="150" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="85" y="44" text-anchor="middle" font-size="11" fill="currentColor">Manifest capabilities</text>
  <rect x="240" y="20" width="150" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="315" y="44" text-anchor="middle" font-size="11" fill="currentColor">User grant</text>
  <rect x="110" y="100" width="180" height="40" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="200" y="118" text-anchor="middle" font-size="11" fill="currentColor">Host grant check</text>
  <text x="200" y="132" text-anchor="middle" font-size="10" fill="currentColor">per calling side</text>
  <path d="M85 60 L140 100" stroke="currentColor" fill="none" marker-end="url(#cap-arrow)"/>
  <path d="M315 60 L260 100" stroke="currentColor" fill="none" marker-end="url(#cap-arrow)"/>
  <path d="M150 140 L80 190" stroke="currentColor" fill="none" marker-end="url(#cap-arrow)"/>
  <path d="M250 140 L320 190" stroke="currentColor" fill="none" marker-end="url(#cap-arrow)"/>
  <rect x="10" y="190" width="150" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="85" y="209" text-anchor="middle" font-size="11" fill="currentColor">Import runs</text>
  <rect x="240" y="190" width="150" height="30" rx="6" fill="none" stroke="currentColor" stroke-dasharray="5 4"/>
  <text x="315" y="209" text-anchor="middle" font-size="11" fill="currentColor">not-permitted</text>
  <text x="100" y="166" text-anchor="end" font-size="10" fill="currentColor">granted</text>
  <text x="340" y="166" text-anchor="start" font-size="10" fill="currentColor">declined</text>
</svg>
<figcaption>The manifest and the user grant feed one check per side. A granted call runs the import. A declined call returns not-permitted, and the plugin keeps running.</figcaption>
</figure>

## Animation rights mapping

Old animation rights map to grants: `network.http` becomes `http:<host>`, and `audio.microphone` becomes `audio.capture`. The full list is in [Porting from .iliumanim](/pages/porting-iliumanim/).

Requirement ids for this page: [CAP-001], [CAP-002], [CAP-003], [CAP-004], [CAP-005], [CAP-006], [CAP-007], [CAP-010], [CAP-011], [CAP-012], [CAP-013], [CAP-014], [CAP-015], [CAP-016], [CAP-017], [CAP-018]. See the [requirements index](/pages/reference-requirements/).

## See also

- [Plugin settings extension](/pages/settings-extension/) for `settings.contribute:<tab-id>` and the built-in tab ids
- [Manifest reference](/pages/reference-manifest/)
- [Lifecycle](/pages/lifecycle/)
- [Security model](/pages/security/)
- [Events and intents](/pages/reference-events-and-intents/)
- [Errors and limits](/pages/reference-errors-and-limits/)

Checked against the API version `ilium:plugin@0.1.0`.

Requirements for this page: [Capabilities requirements](/pages/reference-requirements-capabilities/).
