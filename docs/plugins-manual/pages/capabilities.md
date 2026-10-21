---
layout: page
title: Capabilities and grants
permalink: /pages/capabilities/
---

## Capabilities and grants

A plugin can use only the parts of the host that it asks for. It lists the capabilities it needs in
its manifest, one list per side. The user grants them when a package is installed or first
activated. For bundled plugins the kernel grants everything up to the manifest ceiling, except the
high-risk capabilities.

This page covers the grant syntax, the full capability surface, the tiers and the review dialog,
what happens to an import that is granted, declined or never requested, the always-granted set,
revocation and the error codes.

## You will need

- A manifest with a `capabilities` object. See [Packages and manifests](/pages/packages/).
- The server, client and scene sides. See [Worlds and sides](/pages/worlds-and-sides/).
- The Rust SDK (`ilium-plugin-sdk`) or the TypeScript SDK (`@ilium/plugin-sdk`). Both expose the
  granted list in the activation context. See [Rust SDK](/pages/rust-sdk/) and
  [TypeScript SDK](/pages/typescript-sdk/).

## Grant syntax

A grant is a name, optionally followed by a colon and an argument. The first colon separates the
name from the argument. Later colons belong to the argument.

```text
grant    = name [ ":" argument ]
name     = segment *( "." segment )      segment = [a-z][a-z0-9-]*
argument = 1*char                         everything after the first colon
```

The grants `commands.invoke`, `fs.read`, `fs.write`, `fs.watch`, `http`, `process.run`,
`services.use`, `services.provide`, `messages.peer`, `topics.subscribe`, `http-api.route`, `settings.read-kernel`, `settings.contribute`, `settings.write-kernel`, `extensions.provide`, `extensions.contribute`, `agent-config`, `agent-config.propose`, `agent-sessions`, `panes.create-command`, `path` require
an argument. Every other grant takes no argument, and `tree.read:x` is a manifest error. A service
name has no version. The grant `services.use:ilium:inference/provider` has the name
`services.use` and the argument `ilium:inference/provider`.

The manifest holds one list per side under `capabilities`. The scene side uses the key `scene`:

```json
"capabilities": {
  "server": ["tree.read", "fs.read:project", "http:api.example.com", "http:*.example.org"],
  "client": ["ui.panels", "panes.read"],
  "scene": []
}
```

Grants are per side. A `server` grant does not apply to the `client` side, and each side keeps its
own list.

## Capability surface

Each grant below unlocks the functions or events named in the API specification. **H** marks a
high-risk grant. It always needs an explicit grant, even for a T1 bundled plugin.

Sides: S is the server world, C is the client world and A is the animation (scene) world. Each page link leads to the full description of the grant.

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
| `fs.read:<scope>` | scope, including agent-sessions:gemini, agent-config:gemini, agent-config:antigravity | S C A | normal; high for agent-sessions and agent-config | [page](/pages/capability-fs-read/) | [api-fs](/pages/api-fs/) |
| `fs.write:<scope>` | scope; `agent-sessions` and `agent-config` are refused | S C A | H outside data and cache | [page](/pages/capability-fs-write/) | [api-fs](/pages/api-fs/) |
| `fs.watch:<scope>` | scope; `agent-config` is refused | S C | normal; high for `agent-sessions` | [page](/pages/capability-fs-watch/) | [api-fs](/pages/api-fs/) |
| `agent-config.propose:<agent>` | one agent; needs `fs.read:agent-config:<agent>` | S C | **H** | [page](/pages/capability-agent-config-propose/) | [api-fs](/pages/api-fs/) |
| `fs.pick` | none | C | normal | [page](/pages/capability-fs-pick/) | [api-fs](/pages/api-fs/) |
| `jobs.run` | none | S C A | normal | [page](/pages/capability-jobs-run/) | [api-jobs](/pages/api-jobs/) |
| `http:<host-pattern>` | host pattern, or http:setting:<key> | S C A | normal; H for loopback and private-address literals | [page](/pages/capability-http/) | [api-http](/pages/api-http/) |
| `process.run:<program>` | program name or absolute path | S C | **H** | [page](/pages/capability-process-run/) | [api-process-control](/pages/api-process-control/) |
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
| `settings.read-all`, `settings.write-all` | none (T1 only) | C | **H** | [page](/pages/capability-settings-all/) | [api-settings](/pages/api-settings/) |
| `settings.contribute:<kernel-tab-id>` | one kernel tab id, no wildcard | S C | normal | [page](/pages/capability-settings-contribute/) | [api-settings](/pages/api-settings/) |
| `settings.read-kernel:<kernel-tab-id>` | one kernel tab id, no wildcard | S C | normal; high for api | [page](/pages/capability-settings-read-kernel/) | [api-settings](/pages/api-settings/) |
| `plugins.manage` | none (T1 only) | C | **H** | [page](/pages/capability-plugins-manage/) | [api-help-and-plugins](/pages/api-help-and-plugins/) |
| `help.read` | none | C | normal | [page](/pages/capability-help-read/) | [api-help-and-plugins](/pages/api-help-and-plugins/) |
| `journal.read` | none | S C | normal | [page](/pages/capability-journal-read/) | [api-clock-and-timers](/pages/api-clock-and-timers/) |
| `gpu.kernels` | none; development builds only | A | normal (unstable) | [page](/pages/capability-gpu-kernels/) | [api-scenes](/pages/api-scenes/) |
| `extensions.provide:<plugin-id>/<point>` | one global point id of the plugin's own, no wildcard | S C | normal | [page](/pages/capability-extensions-provide/) | [api-extension-points](/pages/api-extension-points/) |
| `extensions.contribute:<plugin-id>/<point>` | one global point id, no wildcard | S C | normal; high when the point declares `risk: "high"` | [page](/pages/capability-extensions-contribute/) | [api-extension-points](/pages/api-extension-points/) |

### Extended grants

The grants below cover the wider kernel surface: tree restructuring, pane lifecycle, terminal and agent data, chrome, sessions, inference and process control. Twelve of them are high risk and always need an explicit grant, even for a T1 bundled plugin: `terminal.history`, `terminal.input-lock`, `terminal.raw-input`, `panes.lifecycle`, `tree.restructure`, `sessions.cross`, `process.long-lived`, `agent.transcript`, `settings.read-kernel:api`, `settings.write-kernel:<tab>`, `panes.create-command:<program>` and `agent-config.propose:<agent>`. The grant `gpu.kernels` is unstable and exists only in development builds. Both `extensions.*` grants take exactly one global point id and refuse wildcards (`manifest.grant-wildcard`). `extensions.contribute` is high risk only for a point that declares `risk: "high"`; the review dialog then shows a warning. Four argument forms are worth knowing: `http:setting:<key>` (the host comes from one of the plugin's own settings, and the user confirms a changed host before the first request), `fs.read:agent-sessions:gemini` with `fs.read:agent-config:gemini`, `fs.read:agent-config:antigravity`, and `settings.read-kernel:<kernel-tab-id>` (argument required, no wildcard).

| New grant | Page |
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
| `process.long-lived` | [page](/pages/capability-process-long-lived/) |
| `inference.embed` | [page](/pages/capability-inference-embed/) |
| `notify.toast` | [page](/pages/capability-notify-toast/) |
| `open-external` | [page](/pages/capability-open-external/) |
| `location.geocode` | [page](/pages/capability-location-geocode/) |
| `sessions.cross` | [page](/pages/capability-sessions-cross/) |
| `settings.read-kernel:<kernel-tab-id>` | [page](/pages/capability-settings-read-kernel/) |
| `journal.read` | [page](/pages/capability-journal-read/) |
| `gpu.kernels` | [page](/pages/capability-gpu-kernels/) |
| `extensions.provide:<plugin-id>/<point>` | [page](/pages/capability-extensions-provide/) |
| `extensions.contribute:<plugin-id>/<point>` | [page](/pages/capability-extensions-contribute/) |
| `agent-config.propose:<agent>` | [page](/pages/capability-agent-config-propose/) |

The always-granted set needs no entry in the manifest. See [Always granted](#always-granted).

### Wildcards and scopes

- `commands.invoke:<pattern>` takes an exact global id, `<plugin-id>/*` for all commands of one
  plugin, or a bare `*` for all commands (H). A `*` never crosses `/`.
- `http:<host>` takes an exact host, `*.<domain>` for any subdomain at any depth (not the bare
  domain), and an optional port. The loopback hosts `localhost`, `127.0.0.1` and `[::1]` allow plain
  HTTP only with a port, as `http:localhost:<port>`. A bare `*` is refused.
- `topics.subscribe:<pattern>` takes an exact topic, `<prefix>/*` or a bare `*`. Exact and prefix patterns are normal risk; only the bare `*` is high risk (H).
- `process.run:*` is refused with `manifest.grant-program`. `process.run:sh` is valid and H.
- `messages.peer:<plugin-id>` takes one exact plugin id and no wildcard.
- `extensions.provide` and `extensions.contribute` take one exact `<plugin-id>/<point>` and no wildcard. There is no `extensions.contribute-any`. List each point.

File scopes are the argument of `fs.read`, `fs.write` and `fs.watch`:

- `project` is the project folder. It excludes the project's `.ilium` folder.
- `worktrees` is the project's Git worktrees. `fs.write:worktrees` is H.
- `picked` is each path the user chose in `fs.pick`.
- `data` and `cache` are the plugin's own folders. Their read and write grants are always granted.
- `package` is the plugin's own package. It is always readable and never watchable.
- `agent-sessions:<agent>` holds transcripts and is H. It is read only.
- `agent-config:<agent>` holds agent configuration. Reads are H. Writes and watches are refused; a plugin proposes changes with the H grant `agent-config.propose:<agent>`, and the host applies only what the user accepts.
- `minecraft-saves` is the platform's saves folder.
- `path:<template>` is a path template. It starts with a placeholder (`{home}`, `{config}`,
  `{data}`, `{cache}`, `{project}`, `{worktrees}`) or with a literal absolute path. A literal path
  is always H. The template ends with a name or with `/**`. A `*` anywhere else is refused.

## Tiers and the review dialog

The kernel grants capabilities differently depending on the tier of the package.

| Tier | Source | Grants |
|---|---|---|
| T1 | First-party component embedded in the release | The manifest ceiling is granted automatically, except H capabilities. |
| T2 | Package signed by a key the user trusts | The review dialog shows each capability. The user's choices are remembered. |
| T3 | Unsigned local or development package | The same review dialog, with a persistent development badge. |

Install of a T2 or T3 package shows the review dialog. The plugin installs with the grants that the
user leaves checked. H rows start unchecked. A declined row is dimmed and its grant is absent from
the granted list.

For a T1 bundled plugin, activation starts without its H capabilities. The first activation shows
one prompt per plugin, `Plugin <name> asks for: <list>`, with the buttons `Allow selected` and
`Not now`. `Not now` asks again at the next server start; the user sees at most three prompts in total, and then no more. The user can grant the capability at any time from Settings > Plugins.

There is no prompt per call. A grant stays in force until it is revoked. The dialog below is drawn
at 72 columns for a signed notes package.

<figure>

<pre class="term">╭─<span class="t-hi"> Review permissions </span>───────────────────────────────────────────────────────╮
│ Notes 0.1.0 by Example Labs (T2, signed)                                   │
│                                                                            │
│ <span class="t-hi">Server side</span>                                                                │
│ [x] Read the workspace tree <span class="t-dim">……………………………………………………………………………………………</span> tree.read │
│ [x] Reads files in this project <span class="t-dim">……………………………………………………………………</span> fs.read:project │
│ <span class="t-warn">! [ ] Runs the program git</span> <span class="t-dim">…………………………………………………………………………………</span> <span class="t-warn">process.run:git</span> │
│ [x] Connects to api.example.com <span class="t-dim">………………………………………………………</span> http:api.example.com │
│                                                                            │
│ <span class="t-hi">Client side</span>                                                                │
│ [x] Shows panels and settings pages <span class="t-dim">…………………………………………………………………………</span> ui.panels │
│ <span class="t-warn">! [ ] Reads the scrollback history</span> <span class="t-dim">…………………………………………………………</span> <span class="t-warn">terminal.history</span> │
│                                                                            │
│ <span class="t-warn">! marks a high-risk grant. It starts unchecked and is</span>                      │
│ <span class="t-warn">always asked for, even for bundled plugins.</span>                                │
│                                                                            │
│ <span class="t-dim">Space toggle · Enter allow · Esc decline</span>                                   │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>

<figcaption>The review dialog for a signed package, drawn with the client's dialog characters: rounded corners, dotted leaders and the hint bar. Checked rows are granted; high-risk rows start unchecked.</figcaption>
</figure>

## Declined, unrequested and granted

The kernel links each import to one of three targets. The diagram shows them side by side.

<figure>
<svg viewBox="0 0 420 220" role="img" aria-label="Granted, declined and unrequested capabilities">
  <defs>
    <marker id="cap-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6"
            orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="6" y="86" width="96" height="48" rx="6" fill="none" stroke="currentColor"/>
  <text x="54" y="106" font-size="11" text-anchor="middle" fill="currentColor">Manifest</text>
  <text x="54" y="122" font-size="10" text-anchor="middle" fill="currentColor">User decision</text>
  <line x1="102" y1="110" x2="128" y2="44" stroke="currentColor" marker-end="url(#cap-arrow)"/>
  <line x1="102" y1="110" x2="128" y2="110" stroke="currentColor" marker-end="url(#cap-arrow)"/>
  <line x1="102" y1="110" x2="128" y2="176" stroke="currentColor" stroke-dasharray="4 3"
        marker-end="url(#cap-arrow)"/>
  <rect x="130" y="20" width="124" height="48" rx="6" fill="none" stroke="currentColor"
        stroke-width="2"/>
  <text x="192" y="40" font-size="11" text-anchor="middle" fill="currentColor">Granted</text>
  <text x="192" y="56" font-size="10" text-anchor="middle" fill="currentColor">Real host function</text>
  <rect x="130" y="86" width="124" height="48" rx="6" fill="none" stroke="currentColor"/>
  <text x="192" y="106" font-size="11" text-anchor="middle" fill="currentColor">Declined</text>
  <text x="192" y="122" font-size="10" text-anchor="middle" fill="currentColor">Stub: not-permitted</text>
  <rect x="130" y="152" width="124" height="48" rx="6" fill="none" stroke="currentColor"
        stroke-dasharray="6 4"/>
  <text x="192" y="172" font-size="11" text-anchor="middle" fill="currentColor">Not requested</text>
  <text x="192" y="188" font-size="10" text-anchor="middle" fill="currentColor">Import traps</text>
  <line x1="254" y1="44" x2="274" y2="44" stroke="currentColor" marker-end="url(#cap-arrow)"/>
  <line x1="254" y1="110" x2="274" y2="110" stroke="currentColor" marker-end="url(#cap-arrow)"/>
  <line x1="254" y1="176" x2="274" y2="176" stroke="currentColor" stroke-dasharray="4 3"
        marker-end="url(#cap-arrow)"/>
  <text x="280" y="48" font-size="10" fill="currentColor">Plugin keeps running</text>
  <text x="280" y="114" font-size="10" fill="currentColor">Plugin gets an error</text>
  <text x="280" y="180" font-size="10" fill="currentColor">Instance ends, logged</text>
</svg>
<figcaption>The same import behaves differently depending on whether the user granted it, declined
it or the manifest never requested it.</figcaption>
</figure>

- **Granted.** The import works as its API page describes.
- **Declined.** The import links to a stub. Functions with a result return `not-permitted`, and
  the plugin keeps running. Some functions have no error result, so they return a value instead.
  `clipboard` `read-text` and `location` `current` return `none`. `plugins.list` returns an empty
  list. A declined `http-api.route` answers `404 Not Found`. A declined topic subscription delivers
  no events and returns no error. A refused call is written to the plugin log.
- **Not requested.** The manifest never asked for the capability, so the import traps. The
  manifest and the code disagree, which is a plugin bug. The trap ends the instance and the kernel
  logs it.

## Always granted

Every plugin has these without a request, and it need not list them: `log`, `clock`, `timers`,
`storage` for its own buckets, `settings` for its own values, `codec`, read access to its package,
read and write access to its own `data` and `cache` folders, messages between its own two sides,
and `ui.invalidate` for the views it contributes. A manifest without `capabilities` behaves exactly
like `"capabilities": {}`.

## Reading the granted list

The activation context carries `granted`, the list of capabilities that were actually granted. A
plugin should read it on every activation, because a revocation changes it. The example below
hides the file feature when the project grant is missing and keeps the rest of the plugin working.
The grant list is a hint, not a guarantee. A call can still return `not-permitted` or `revoked`.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::{fs, log::{self, Level}};

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Notes;

impl Lifecycle for Notes {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        if context.granted.iter().any(|grant| grant == "fs.read:project") {
            task::spawn(load_readme());
        }
        Ok(())
    }

    fn deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        None
    }
}

impl Handler for Notes {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

async fn load_readme() {
    let path = FsPath { scope: "project".into(), path: "README.md".into() };
    match fs::read_all(path).await {
        Ok(bytes) => log::log(Level::Info, &format!("README has {} bytes", bytes.len())),
        Err(error) if error.code == ErrorCode::NotPermitted => {
            log::log(Level::Warn, "project files are not granted");
        }
        Err(error) => log::log(Level::Error, &format!("README read failed: {}", error.message)),
    }
}
```

**TypeScript**

```ts
import { definePlugin, PluginError } from '@ilium/plugin-sdk';
import type { Submission } from '@ilium/plugin-sdk/types';
import { readAll } from '@ilium/plugin-sdk/host/fs';
import { log } from '@ilium/plugin-sdk/host/log';

async function loadReadme(): Promise<void> {
  try {
    const bytes = await readAll({ scope: 'project', path: 'README.md' });
    log('info', `README has ${bytes.length} bytes`);
  } catch (error) {
    if (error instanceof PluginError && error.code === 'not-permitted') {
      log('warn', 'project files are not granted');
      return;
    }
    throw error;
  }
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate(context) {
    if (context.granted.includes('fs.read:project')) {
      loadReadme().catch((error) => log('error', `README read failed: ${String(error)}`));
    }
  },
  onEvents(): Submission[] {
    return [];
  },
});
```

## Changing grants

- **Revocation.** The user revokes a grant in Settings > Plugins > <plugin> > Capabilities, or with
  `ilium plugin grants <id> --revoke <grant>`. The kernel bumps the authorization epoch. Calls in
  flight return `revoked`. Pending requests that need the grant complete as `failed(revoked)`.
  Contributions that need the grant are withdrawn, including decorations, grabs, routes and
  watches. The plugin receives `deactivate(revoked)` and is activated again with the reduced list.
  This re-activation is not counted as a failure.
- **Later grants.** A grant that the user adds, or one that a plugin upgrade requests and the user
  accepts, bumps the epoch too. The plugin receives `deactivate(granted)` and is activated again
  with the new list.
- **Disable.** A disabled plugin receives `deactivate(disabled)` and its contributions are
  withdrawn. See [Lifecycle](/pages/lifecycle/).

## What the user sees

- The review dialog at install, and the prompt for H capabilities at first activation of a T1
  plugin.
- Settings > Plugins > <plugin> > Capabilities, which lists each grant with its state.
- `ilium plugin grants <id>`, which also shows declined grants.
- `ilium plugin logs <id>`, which shows each refusal, each file write outside `data` and `cache`,
  each process run with its arguments, and each clipboard read by length only.

<figure>
<pre class="term">
<span class="t-prompt">$</span> ilium plugin grants com.example.notes
server  tree.read          <span class="t-ok">granted</span>
server  fs.read:project    <span class="t-ok">granted</span>
server  process.run:git    <span class="t-dim">declined</span>
client  ui.panels          <span class="t-ok">granted</span>
<span class="t-prompt">$</span> ilium plugin grants com.example.notes \
    --revoke fs.read:project
<span class="t-ok">ok</span> revoked fs.read:project, plugin re-activated
</pre>
<figcaption>The grant commands for one plugin. The layout of the output is a sketch.</figcaption>
</figure>

## Errors

Manifest errors are reported as `error` lines by `ilium plugin install` and `ilium plugin build`,
one line per error.

| Code | Cause |
|---|---|
| `manifest.grant-unknown` | The name is not a capability of this plugin API version. |
| `manifest.grant-argument` | The grant has an argument, but the name takes none. |
| `manifest.grant-argument-missing` | The name needs an argument and none was given. |
| `manifest.grant-wildcard` | A wildcard was used where the name does not accept one. |
| `manifest.grant-scope` | The file scope is unknown or does not accept that access. |
| `manifest.grant-side` | The capability is not available on the side that requests it. |
| `manifest.grant-tier` | The capability is not available to this plugin's tier. |
| `manifest.grant-dependency` | The capability also needs another capability. |
| `manifest.export-grant` | An export needs a capability that the manifest does not request. |

Runtime codes that relate to grants are `not-permitted` (the capability is declined, revoked in
flight, or the argument is outside the grant), `revoked` (the authorization epoch changed during the
call) and `failed(revoked)` (a pending request lost its grant).

Edge cases to design for:

- `fs.pick` needs `fs.read:picked` or `fs.write:picked`. Without either it fails with
  `manifest.grant-dependency`.
- `settings.write-all` needs `settings.read-all`, and the review dialog cannot grant write alone.
- `settings.contribute:<tab-id>` lets a plugin add sections to one built-in tab, such as `settings.contribute:terminal`. It is normal risk and independent of `settings.read-all` and `settings.write-all`: it never implies them, and they never imply it. A plugin's own tabs need no grant. See [Plugin settings extension](/pages/settings-extension/).
- `fs.write:agent-sessions:<agent>` is refused at install with `manifest.grant-scope`. Transcripts
  are read only.
- An `inference-provider` export needs `inference.provide`. Without the grant the plugin installs
  but is never selected as a provider.
- A `process.run` call for a program outside its grant fails with `not-permitted` before the
  process starts. A `cwd` that the plugin cannot read is `not-permitted`.
- A `http:*` grant, or any other bare wildcard, is refused. A redirect follows the same host rule
  on each hop.

## Migration of animation rights

Earlier animation packages used a separate set of rights. The new names replace them. `network.http`
becomes `http:<host>`, `audio.microphone` becomes `audio.capture`, `state.persist` becomes the
always-granted `storage`, and `input.pointer` becomes the scene's `wants-pointer`. Animation
packages must be rebuilt, because there is no compatibility layer.

## See also

- [File access](/pages/file-access/)
- [Packages and manifests](/pages/packages/)
- [Lifecycle](/pages/lifecycle/)
- [Security](/pages/security/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Reference: capabilities](/pages/reference-capabilities/)
- [Plugin settings extension](/pages/settings-extension/)

API version checked: `ilium:plugin@0.1.0`.
