# Plugin manual

## Ilium plugin manual

A plugin adds features to Ilium. It can decorate tree rows with badges, add commands and keybindings, add settings pages, open panels and dialogs, provide a pane kind, add toolbar buttons to agent panes, raise desktop notices and toasts, extend the tree toolbar and the status bar, and draw an ambient scene. The kernel owns the project tree, the panes and the terminals. A plugin never changes them directly. It receives events, returns intents for the kernel to apply, and calls host functions for everything else.

The contract is wide enough that every feature of the Ilium client that is not kernel machinery can be written as a plugin. This manual explains each part of that contract, how to build and install a plugin, and how the kernel loads, restarts and disables one. Every example is given in Rust and in TypeScript. If you are new to plugins, read [What a plugin is](/pages/getting-started-overview/) first, then [Set up a toolchain](/pages/getting-started-toolchain/). The tutorial builds plugins in eleven steps, and each step adds one concept. Twelve standalone tutorials follow: six on basic topics and six on advanced ones, including extension points, hooks and replacing a core plugin. To change how Ilium itself behaves, read [Extending core plugins](/pages/extending-core-plugins/).

### What a plugin draws

The screen below is a full Ilium window with three plugin contributions. The agent icons and spinners in the tree on the left come from a server plugin that decorates rows. The panel on the right is a client panel that draws a `table` and three `leader-row` lines. The status bar segment and the key hints at the bottom are the kernel's, with one segment added by the plugin. None of these is part of the kernel.

<figure>
<pre class="term">╭  ≡ ● · Sessions ───────────────╮╭  ≡ ● · Agents ─────────────────────────────╮
│ ▼ 📂 shop                      ││ <span class="t-hi">Pane</span>          Now    Signature             │
│ › ▼ 📂 api                     ││ ──────────────────────────────────         │
│ <span class="t-hi">››   🦀 ⠋ Fix login redirect</span>   ││ api/login      <span class="t-ok">⠋</span>      claude               │
│ ››   🐢 <span class="t-dim">●</span> Review schema        ││ api/schema     <span class="t-dim">●</span>      codex                │
│ ››   🖥️    Shell               ││ web/build      <span class="t-warn">✋</span>      claude              │
│ › ▶ 📂 web                     ││ web/release    <span class="t-ok">🔔</span>      codex               │
│ ›   🦀 <span class="t-warn">✋</span> Build pipeline       ││                                            │
│ ›   🐢 <span class="t-ok">🔔</span> Release notes        ││ Objective ………………… fix the login redirect   │
│ ›   📝    notes.md             ││ Cost ……………………………… ▰▰▰▱▱                    │
│                                ││ Idle agents ………… [ Off ]                   │
╰────────────────────────────────╯╰────────────────────────────────────────────╯
 ℹ  shop  -  4 agents  -  1 waiting for approval
 ↑/↓ move · Enter open · Tab switch pane · F1 help</pre>
<figcaption>Illustrative, 80 columns, dark theme. Panels have rounded borders only. The selected tree row is drawn with the accent colours. The tree shows a group, panes with agent identity icons and the Now column (a spinner, a hand for approval, a bell for done), and the panel shows the leader dots that join each label to its value.</figcaption>
</figure>

### Contract version

Every plugin declares the contract it targets in the manifest key `api`. The contract is `ilium:plugin@0.1.0`. The contract `ilium:plugin@0.1.0` has 56 events, 42 intents, 51 widgets, 18 settings row types, 68 capabilities, 39 host interfaces and 16 export interfaces. Every plugin declares `ilium:plugin@0.1.0`. Each component carries an `ilium:api-version` section with the same version, and the host refuses a package where the two differ. A package whose `api` is not 0.1.x is refused. Items marked unstable (`gpu.kernels`, WebSocket, `ui-calendar`) need an entry in the manifest `unstable` list.

### What you need

| Need | Rule | Used for |
|---|---|---|
| Ilium | Installing Ilium installs the `ilium plugin` commands. There is no separate tool. | Build, pack, install, dev reload |
| Rust | Stable 1.90 or later, with the target `wasm32-wasip2` | Rust plugins, crate `ilium-plugin-sdk` |
| TypeScript | Bun 1.2 or later | TypeScript plugins, package `@ilium/plugin-sdk` |
| Grants | Each capability is listed in `capabilities` for its side | Every import that needs one |
| Publisher key | Only for signed packages | Tier T2 installs, see sharing |

`bun install` in a TypeScript plugin installs the component tooling locally. Nothing is installed globally.

Each plugin has one tier. T0 is the kernel itself. T1 is bundled with Ilium. T2 is signed by a publisher key the user trusts. T3 is an unsigned local package. A T3 package installs only with `--allow-unsigned` or through `ilium plugin dev`. It carries a `dev` badge and runs under the T2 sandbox and ceilings. A T1 plugin may set `limits_tier: "first-party"` to use the higher resource ceilings that a feature as large as a board or an editor needs.

### Reading order

<figure>
<svg viewBox="0 0 320 170" role="img" aria-label="Reading order of the plugin manual">
  <title>Reading order of the plugin manual</title>
  <desc>Two getting started pages lead into an eleven step tutorial and twelve standalone ones. The reference pages follow.</desc>
  <defs>
    <marker id="reading-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="8" y="10" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="20" y="35" font-size="11" fill="currentColor">1. Getting started: what a plugin is</text>
  <rect x="8" y="62" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="20" y="87" font-size="11" fill="currentColor">2. Getting started: toolchain, first plugin</text>
  <rect x="8" y="114" width="300" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="20" y="139" font-size="11" fill="currentColor">3. Tutorial: eleven steps</text>
  <line x1="158" y1="50" x2="158" y2="62" stroke="currentColor" marker-end="url(#reading-arrow)"/>
  <line x1="158" y1="102" x2="158" y2="114" stroke="currentColor" marker-end="url(#reading-arrow)"/>
  <text x="20" y="166" font-size="11" fill="currentColor">Then the reference pages, as mapped below.</text>
</svg>
<figcaption>Read the getting started pages once, then work through the tutorial in order.</figcaption>
</figure>

### Sides and worlds

Each plugin has one component per side it uses. A side runs in one kernel process, and each side has its own world, which fixes what the component may import and export.

| Side | Manifest key | World | Required exports | Optional exports |
|---|---|---|---|---|
| Server | `server` | `ilium:server/base@0.1.0` | `lifecycle`, `handler` | `detector`, `job-runner`, `service-provider`, `inference-provider`, `catalog-provider`, `virtual-rows-provider` |
| Client | `client` | `ilium:client/base@0.1.0` | `lifecycle`, `handler` | `views`, `annotator`, `job-runner`, `service-provider`, `inference-provider`, `catalog-provider`, `menu-provider`, `virtual-rows-provider`, `token-provider` |
| Scene | `scene` | `ilium:anim/base@0.1.0` | `lifecycle`, `renderer` | `job-runner` |

The client world adds the UI, command, key, clipboard, audio, media, location, geocoding, help, keymap and plugin-management imports, plus the editor and basic primitives. The server world adds the CLI, HTTP API, workspace, cross-session and long-lived child process imports. The scene world is the smallest: logging, clocks, storage, file access, codecs, jobs, HTTP, audio, media, location and the scene surface. Required exports are never listed in `exports`. The manifest lists only the optional ones, by interface name, and the host refuses a component whose exports differ from that set. [Sides and worlds](/pages/worlds-and-sides/) has the full import matrix.

### The map of the plugin surface

Everything a plugin can use falls into one of the groups below. Each group has its own page with every function, field and case.

#### Exports and host imports

| Area | What it holds | Page |
|---|---|---|
| Exports | `lifecycle` (`activate`, `deactivate`), `handler` (`on-events`), `views` (`render`), `detector` (`identify`, `classify`), `job-runner` (`run`), `service-provider` (`handle`), `inference-provider` (`complete`, `models`), `annotator` (`annotate`), `renderer` (scenes), and the four optional exports: `catalog-provider`, `menu-provider`, `virtual-rows-provider`, `token-provider` | [Exports](/pages/api-exports/), [Lifecycle](/pages/lifecycle/) |
| Core imports | `log`, `clock`, `timers` (including durable timers), `storage`, `settings`, `codec`, `fs`, `jobs` | [Log](/pages/api-log/), [Clock and timers](/pages/api-clock-and-timers/), [Storage](/pages/api-storage/), [Settings](/pages/api-settings/), [Codec](/pages/api-codec/), [Files](/pages/api-fs/), [Jobs](/pages/api-jobs/) |
| Tree and panes | `tree`, `panes`, `intents`, `workspace` | [Tree](/pages/api-tree/), [Panes](/pages/api-panes/), [Intents](/pages/api-intents/), [Workspace](/pages/api-workspace/) |
| Network and processes | `http` (including streaming), `process` (including long-lived children), `http-api`, `cli` | [HTTP](/pages/api-http/), [Process control](/pages/api-process-control/), [HTTP API](/pages/api-http-api/), [CLI](/pages/api-cli/) |
| Client surface | `ui`, `commands`, `keys`, `clipboard`, `audio`, `media`, `location`, `help`, `plugins`, `scene` | [UI](/pages/api-ui/), [Commands](/pages/api-commands/), [Keys](/pages/api-keys/), [Clipboard](/pages/api-clipboard/), [Audio](/pages/api-audio/), [Media](/pages/api-media/), [Location](/pages/api-location/), [Help and plugins](/pages/api-help-and-plugins/), [Scenes](/pages/api-scenes/) |
| Services and messages | `services`, `topics`, `messages`, `inference` | [Services](/pages/api-services/), [Messages and topics](/pages/api-messages-and-topics/), [Inference](/pages/api-inference/), [Notifications](/pages/api-notifications/) |
| Primitives | `terminal` (screen and cell reads, history, output filters, pointer, highlights), `agent` (agents, signatures, transcript, toolbar state), `editor` (buffers, decorations, gutter, viewport, minimap marks), `basic` (title, status, focus, activity) | [Primitives](/pages/primitives/), [Terminal](/pages/primitives-terminal/), [Agent](/pages/primitives-agent/), [Editor](/pages/primitives-editor/), [Basic](/pages/primitives-basic/) |
| Events | 56 event cases, for example `tree-changed`, `agent-activity-changed`, `input-submitted`, `screen-changed`, `signal-changed`, `request-completed` | [Events and intents](/pages/events-and-intents/), [Events](/pages/api-events/) |
| Intents | 42 intent cases, for example `set-decoration`, `notify`, `toast`, `send-input`, `create-pane`, `restructure`, `editor-apply-edits` | [Events and intents](/pages/events-and-intents/), [Requests and outcomes](/pages/requests-and-outcomes/) |

#### Widgets

51 widgets in 7 families. A view returns a tree of nodes, and the host draws it with the same glyphs and colours as the built-in panels.

| Family | Widgets | Page |
|---|---|---|
| Layout and containers | `layout`, `block`, `tabs`, `popup`, `clear`, `scrollbar`, `split`, `separator`, `dialog`, `scroll-view` | [Layout and containers](/pages/widgets-layout-and-containers/) |
| Text | `paragraph`, `markdown`, `leader-row`, `key-hints`, `diff-view` | [Text](/pages/widgets-text/) |
| Collections | `%list`, `table`, `tree-list`, `menu`, `check-list`, `list-editor` | [Collections](/pages/widgets-collections/) |
| Data display | `gauge`, `line-gauge`, `sparkline`, `bar-chart`, `chart`, `canvas`, `calendar`, `progress-row`, `meter`, `minimap` | [Data display](/pages/widgets-data-display/) |
| Input | `button`, `toggle`, `choice`, `number`, `text-input`, `text-area`, `schema-form`, `key-capture`, `color-picker`, `path-field`, `location-picker` | [Input](/pages/widgets-input/) |
| Status and feedback | `badge`, `spinner`, `notice`, `icon` | [Widget catalogue](/pages/widgets-index/) |
| Media and embedded | `image`, `surface`, `code-view`, `terminal-view`, `video` | [Media and embedded](/pages/widgets-media-and-embedded/) |

[Widget catalogue](/pages/widgets-index/) holds the common props, keys, focus rules, tooltips and the overlays that are built from these parts (tooltip, popover, context menu, toast, command palette). [UI model](/pages/ui-model/), [Layout](/pages/layout/), [Retained mode](/pages/retained-mode/) and [Accessibility and theming](/pages/accessibility-and-theming/) explain how a tree becomes pixels of text.

#### Settings

A plugin adds a settings tab, a section in a kernel tab, or its own view-rendered tab. There are 18 row types, from `toggle` and `choice` to `location`, `map`, `multi-choice` and `meter`. See [Settings extension](/pages/settings-extension/), [Settings screens](/pages/settings-screens/) and [Settings schema reference](/pages/reference-settings-schema/).

#### Capabilities

One grant per feature, such as `tree.decorate`, `agent.read` or `fs.read:<scope>`. There are 19 further grants, nine of them high risk. The user sees every grant in the review dialog before it takes effect. The [capability reference](/pages/capabilities/) lists them all with their sides and risk. The pages in the groups below describe each grant and what it unlocks.

| Group | Grants |
|---|---|
| Tree | [`tree.read`](/pages/capability-tree-read/), [`tree.write`](/pages/capability-tree-write/), [`tree.decorate`](/pages/capability-tree-decorate/), [`tree.metadata`](/pages/capability-tree-metadata/) |
| Panes and terminal | [`panes.read`](/pages/capability-panes-read/), [`panes.create`](/pages/capability-panes-create/), [`panes.signal`](/pages/capability-panes-signal/), [`panes.provide-kind`](/pages/capability-panes-provide-kind/), [`terminal.read-screen`](/pages/capability-terminal-read-screen/), [`terminal.write`](/pages/capability-terminal-write/), [`terminal.output-filter`](/pages/capability-terminal-output-filter/), [`terminal.pointer`](/pages/capability-terminal-pointer/) |
| Agents | [`agent.read`](/pages/capability-agent-read/), [`agent.read-prompts`](/pages/capability-agent-read-prompts/), [`agent.control`](/pages/capability-agent-control/), [`agent.signatures`](/pages/capability-agent-signatures/) |
| Editor | [`editor.read`](/pages/capability-editor-read/), [`editor.edit`](/pages/capability-editor-edit/), [`editor.provide`](/pages/capability-editor-provide/) |
| Files | [`fs.read`](/pages/capability-fs-read/), [`fs.write`](/pages/capability-fs-write/), [`fs.watch`](/pages/capability-fs-watch/), [`fs.pick`](/pages/capability-fs-pick/) |
| Network and processes | [`http`](/pages/capability-http/), [`http-api.route`](/pages/capability-http-api-route/), [`process.run`](/pages/capability-process-run/), [`jobs.run`](/pages/capability-jobs-run/), [`cli.contribute`](/pages/capability-cli-contribute/) |
| UI and desktop | [`ui.panels`](/pages/capability-ui-panels/), [`keys.intercept`](/pages/capability-keys-intercept/), [`commands.invoke`](/pages/capability-commands-invoke/), [`clipboard`](/pages/capability-clipboard/), [`notify.desktop`](/pages/capability-notify-desktop/), [`sound.play`](/pages/capability-sound-play/), [`audio`](/pages/capability-audio/), [`media.video`](/pages/capability-media-video/), [`location.observer`](/pages/capability-location-observer/) |
| Platform | [`services`](/pages/capability-services/), [`topics.subscribe`](/pages/capability-topics-subscribe/), [`messages.peer`](/pages/capability-messages-peer/), [`inference`](/pages/capability-inference/), [`inference.provide`](/pages/capability-inference-provide/), [`workspace.manage`](/pages/capability-workspace-manage/), [`settings.all`](/pages/capability-settings-all/), [`plugins.manage`](/pages/capability-plugins-manage/), [`help.read`](/pages/capability-help-read/), [always granted](/pages/capability-always-granted/) |

#### SDKs, agent documentation and project guides

| Topic | Pages |
|---|---|
| SDKs | [Rust SDK](/pages/rust-sdk/) (crate `ilium-plugin-sdk`), [TypeScript SDK](/pages/typescript-sdk/) (package `@ilium/plugin-sdk`), [Reference](/pages/reference/) |
| Agent documentation | [Agent docs](/pages/agent-docs/) and [the llms.txt files](/pages/agent-docs-llms-txt/) give a coding agent the contract in a form it can read |
| Packaging | [Packages](/pages/packages/), [Manifest reference](/pages/reference-manifest/), [Versioning](/pages/versioning/) |
| Quality | [Testing](/pages/testing/), [Debugging](/pages/debugging/), [Performance](/pages/performance/), [Errors and limits](/pages/errors-and-limits/), [Security](/pages/security/) |
| Concepts | [Architecture](/pages/architecture/), [Events and intents](/pages/events-and-intents/), [Requests and outcomes](/pages/requests-and-outcomes/), [Lifecycle](/pages/lifecycle/), [Services](/pages/services/), [Background jobs](/pages/background-jobs/), [File access](/pages/file-access/) |
| Background | [Design decisions](/pages/design-decisions/), [Roadmap](/pages/roadmap/), [Glossary](/pages/glossary/), [Porting iliumanim](/pages/porting-iliumanim/) |

#### Tutorial

| Step | Plugin | What it adds |
|---|---|---|
| 1 | [Agent badge](/pages/tutorial-01-agent-badge/) | Events, intents, a decoration slot |
| 2 | [Command and settings](/pages/tutorial-02-command-and-settings/) | A command, a keybinding, the `show_idle` setting |
| 3 | [Panel](/pages/tutorial-03-panel/) | A client view with a table |
| 4 | [Finished notice](/pages/tutorial-04-finished-notice/) | Notices and toasts |
| 5 | [Todo scanner](/pages/tutorial-05-todo-scanner/) | Files, jobs and requests |
| 6 | [Notes pane](/pages/tutorial-06-notes-pane/) | A pane kind |
| 7 | [Ambient scene](/pages/tutorial-07-ambient-scene/) | A scene |
| 8 | [TypeScript](/pages/tutorial-08-typescript/) | The same ideas from the TypeScript toolchain |
| 9 | [Package and share](/pages/tutorial-09-package-and-share/) | Signing, installing, sharing |
| 10 | [New settings tab](/pages/tutorial-10-settings-new-tab/) | A settings tab with a view |
| 11 | [Extend a kernel tab](/pages/tutorial-11-settings-extend-tab/) | A section in a kernel settings tab |
| 12 | [Your first CLI subcommand](/pages/tutorial-12-hello-cli/) | Basics: a CLI subcommand |
| 13 | [Keybindings, chords and when expressions](/pages/tutorial-13-keys-and-when/) | Basics: keys, chords, when expressions |
| 14 | [A toast and a status-bar segment](/pages/tutorial-14-toast-and-status/) | Basics: a toast and a status segment |
| 15 | [A context-menu entry](/pages/tutorial-15-context-menu/) | Basics: a context-menu entry |
| 16 | [Remember a counter in storage](/pages/tutorial-16-remember-a-counter/) | Basics: storage |
| 17 | [A help topic and a custom sound](/pages/tutorial-17-help-and-sound/) | Basics: a help topic and a sound |
| 18 | [Offer an extension point: a link preview plugin](/pages/tutorial-18-add-an-extension-point/) | Advanced: offer an extension point |
| 19 | [Stop core actions with hooks: quiet hours](/pages/tutorial-19-stop-core-actions-with-hooks/) | Advanced: veto and transform hooks |
| 20 | [Extend six core plugins from one package: a team pack](/pages/tutorial-20-extend-six-core-plugins/) | Advanced: extend six core plugins |
| 21 | [Replace a core plugin: Titles Lite](/pages/tutorial-21-replace-a-core-plugin/) | Advanced: replace a core plugin |
| 22 | [A versioned service with fallbacks](/pages/tutorial-22-versioned-service-with-fallbacks/) | Advanced: a versioned service |
| 23 | [Drive the fleet: a review swarm](/pages/tutorial-23-review-swarm/) | Advanced: drive the fleet |

How the pieces meet at run time:

<figure>
<svg viewBox="0 0 400 220" role="img" aria-label="Message flow between the kernel, a plugin and host imports">
  <title>Message flow between the kernel and a plugin</title>
  <desc>The kernel sends event batches to the plugin. The plugin returns submissions and calls host imports.</desc>
  <defs>
    <marker id="flow-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="120" y="10" width="180" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="34" font-size="11" text-anchor="middle" fill="currentColor">Kernel (server or client)</text>
  <rect x="120" y="90" width="180" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="114" font-size="11" text-anchor="middle" fill="currentColor">Plugin component</text>
  <rect x="120" y="170" width="180" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="194" font-size="11" text-anchor="middle" fill="currentColor">Host imports and services</text>
  <line x1="140" y1="50" x2="140" y2="90" stroke="currentColor" marker-end="url(#flow-arrow)"/>
  <text x="8" y="74" font-size="11" fill="currentColor">event batches</text>
  <line x1="280" y1="90" x2="280" y2="50" stroke="currentColor" marker-end="url(#flow-arrow)"/>
  <text x="310" y="74" font-size="11" fill="currentColor">submissions</text>
  <line x1="200" y1="130" x2="200" y2="170" stroke="currentColor" marker-end="url(#flow-arrow)"/>
  <text x="212" y="154" font-size="11" fill="currentColor">calls and requests</text>
  <text x="8" y="212" font-size="10" fill="currentColor">Replies return as request-completed events.</text>
</svg>
<figcaption>Events flow down to the plugin. Submissions and calls flow back to the kernel.</figcaption>
</figure>

### Worked example: a server plugin

This plugin logs when it activates. It puts a badge on each pane whose agent starts working. The manifest comes first and is the same for both languages.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.hello",
  "name": "Hello",
  "version": "0.1.0",
  "publisher": "Example",
  "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": [] },
  "capabilities": { "server": ["agent.read", "tree.decorate"] },
  "contributes": {
    "decoration_slots": [{ "id": "hello", "placement": "badge", "priority": 50 }]
  }
}
```

The Rust code uses the SDK paths of the Rust module layout. The `#[ilium_plugin]` attribute generates the export glue, so the struct only implements traits.

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::exports::Activation;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::intents::SetDecorationSpec;
use ilium_plugin_sdk::types::tree::AgentActivity;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Hello;

impl Lifecycle for Hello {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        log::log(Level::Info, &format!("hello from {}", context.plugin));
        Ok(())
    }
}

impl Handler for Hello {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::AgentActivityChanged(AgentActivityEvent { node, current, .. }) = event
            else {
                continue;
            };
            if current != AgentActivity::Working {
                continue;
            }
            submissions.push(Submission::new(Intent::SetDecoration(SetDecorationSpec {
                node,
                slot: "hello".into(),
                content: vec![Span::plain("hi")],
                icon: None,
                sort_key: None,
                persist: false,
            })));
        }
        submissions
    }
}
```

`Submission::new` takes the next SDK correlation number and no dedup key. `Submission::keyed(intent, key)` sets a key.

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate(context) {
    log('info', `hello from ${context.plugin}`);
  },
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'agent-activity-changed') continue;
      if (event.val.current !== 'working') continue;
      submissions.push({
        intent: {
          tag: 'set-decoration',
          val: {
            node: event.val.node,
            slot: 'hello',
            content: [{ text: 'hi' }],
            icon: undefined,
            sortKey: undefined,
            persist: false,
          },
        },
        correlation: 1n,
        dedupKey: undefined,
      });
    }
    return submissions;
  },
});
```

The plugin builds and installs with the commands below. The Rust and TypeScript projects use the same commands.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin build
{"type":"progress","step":"compile","message":"Compiling server.wasm"}
{"type":"artifact","path":"server.wasm","sha256":"3f2a9c01...","bytes":184320}
<span class="t-ok">{"type":"result","ok":true}</span>
<span class="t-prompt">$</span> ilium plugin install com.example.hello-0.1.0.iliumpkg
<span class="t-err">{"type":"error","code":"manifest.grant-unknown",</span>
<span class="t-err"> "message":"capability \"tree.wrte\" is not known to plugin API</span>
<span class="t-err"> ilium:plugin@0.1.0"}</span>
</pre>
<figcaption>Illustrative, not generated. The first lines show a successful build. The last lines show a manifest typo refused at install.</figcaption>
</figure>

And this is what the user sees in the tree while the agent works. The badge `hi` is drawn after the title, and the row keeps the identity icon and the spinner of the kernel.

<figure>
<pre class="term">╭  ≡ ● · Sessions ───────────────╮
│ ▼ 📂 shop                      │
│ ›   🦀 <span class="t-ok">⠋</span> Fix login redirect <span class="t-hi">hi</span> │
│ ›   🐢 <span class="t-dim">●</span> Review schema         │
╰────────────────────────────────╯
</pre>
<figcaption>Illustrative. One decoration slot, placed after the title. The spinner glyph steps through the ten Braille frames of the kernel.</figcaption>
</figure>

### Worked example: a client plugin

This client plugin answers its own command with a desktop notice. The manifest declares the command and the grant that the notice needs.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.hello",
  "name": "Hello",
  "version": "0.1.0",
  "publisher": "Example",
  "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": [] },
  "capabilities": { "client": ["notify.desktop"] },
  "contributes": {
    "commands": [{ "id": "greet", "title": "Say hello" }]
  }
}
```

The command id is `com.example.hello/greet`, because the global id is the plugin id, a slash and the local id.

**Rust**

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::exports::Activation;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::CommandInvokedEvent;
use ilium_plugin_sdk::types::intents::{NotifySpec, Urgency};

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct Hello;

impl Lifecycle for Hello {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        log::log(Level::Info, &format!("hello from {}", context.plugin));
        Ok(())
    }
}

impl Handler for Hello {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::CommandInvoked(CommandInvokedEvent { command, .. }) = event else {
                continue;
            };
            if command != "com.example.hello/greet" {
                continue;
            }
            submissions.push(Submission::new(Intent::Notify(NotifySpec {
                title: "Hello".into(),
                body: "from a plugin".into(),
                urgency: Urgency::Normal,
                sound: None,
            })));
        }
        submissions
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate(context) {
    log('info', `hello from ${context.plugin}`);
  },
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'command-invoked') continue;
      if (event.val.command !== 'com.example.hello/greet') continue;
      submissions.push({
        intent: {
          tag: 'notify',
          val: { title: 'Hello', body: 'from a plugin', urgency: 'normal', sound: undefined },
        },
        correlation: 1n,
        dedupKey: undefined,
      });
    }
    return submissions;
  },
});
```

`definePlugin` returns one object per export it uses. Here that is `lifecycle` and `handler`, which the entry module must export. Ids such as `correlation` are `bigint` values, because every `u64` is a `bigint` in TypeScript.

### Commands

| Command | Purpose |
|---|---|
| `ilium plugin new <dir> --id <id> --lang rust\|typescript --side server\|client\|both\|scene` | Create a project |
| `ilium plugin build [<dir>] [--debug] [--side <side>]` | Build the components of the project |
| `ilium plugin pack [<dir>] [--out <file>]` | Write `dist/<id>-<version>.iliumpkg` |
| `ilium plugin install <file> [--allow-unsigned]` | Install a package |
| `ilium plugin dev` | Watch the directory and reload on change |
| `ilium plugin logs [<id>]` and `ilium plugin grants <id>` | Read the plugin log, and list or revoke grants |

Every command prints JSONL with one object per line. The `type` field is `progress`, `warning`, `error`, `artifact` or `result`. Errors name a `code`, and when known, a `file` and `line`.

### Errors and edge cases

Install and build refuse a package with a code in the manifest group. The common codes are below. [Reference: errors and limits](/pages/reference-errors-and-limits/) lists the 15 runtime error codes.

| Code | Cause |
|---|---|
| `manifest.grant-unknown` | The grant is not known to the plugin API |
| `manifest.grant-argument-missing` | A grant such as `fs.read` needs an argument after the colon |
| `manifest.exports-mismatch` | The component exports differ from required plus listed exports |
| `manifest.cli-name-reserved` | The CLI name is a kernel command |
| `manifest.cli-name-taken` | Another installed plugin already owns the CLI name |
| `manifest.contribution-limit` | A contribution key holds more entries than its fixed cap |

At run time every fallible call returns a `plugin-error` with one of these codes. Retry means the same call may succeed later.

| Code | Meaning | Retry |
|---|---|---|
| `not-permitted` | No grant, the user declined it, or a policy forbids the action | No |
| `not-found` | The node, pane, file, command or other target does not exist | No |
| `invalid-argument` | A rule the plugin can check before the call failed | No, fix the call |
| `conflict` | State changed or is held, so re-read it first | Yes |
| `quota-exceeded` | A stored or held quota is full | After freeing resources |
| `rate-limited` | A rate rule throttled the call | Yes, after `retry-after-ms` |
| `busy` | Transient capacity is full | Yes, later |
| `timed-out` | A deadline passed | Yes |
| `cancelled` | The plugin, the user or a shutdown cancelled the request | Maybe |
| `unsupported` | The host or platform cannot do it | No |
| `pane-not-ready` | A restored terminal pane has not started its process | Yes, after `pane-created` |
| `revoked` | The grants changed during the call | No, the plugin is re-activated |
| `io` | An operating system I/O error other than a missing file | Maybe |
| `network` | DNS, connect, TLS or transport failure | Yes |
| `internal` | A host bug or a trap in a job instance | No |

A trap, which is a Rust panic or an uncaught TypeScript exception in a handler, counts as a failure. The kernel restarts the plugin after 1 second, then after 5 seconds. A third failure inside 10 minutes disables the plugin on that side. The user turns it on again in Settings > Plugins. A manual re-enable resets the count.

### What the user sees

- **Review dialog:** before a T2 or T3 package installs, the dialog lists each requested capability with a checkbox, drawn as `[x]` and `[ ]`. Declined rows are dimmed. High-risk capabilities start unchecked and are drawn in the warning style.
- **First activation of a bundled plugin:** a T1 plugin with high-risk capabilities asks once, with the buttons `Allow selected` and `Not now`. `Not now` asks again at the next server start, at most three times.
- **Disabled plugins:** the tree footer shows the notice below. Settings > Plugins lists the same plugin with its last error.

<figure>
<pre class="term">╭  ≡ ● · Plugins ────────────────────────────────────────────────────────────╮
│ <span class="t-warn">! Plugin Hello stopped after 3 failures in 10 minutes:</span>                     │
│ <span class="t-warn">on-events trap (handler). Turn it on again in Settings &gt; Plugins.</span>          │
╰────────────────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>Illustrative. The notice text is fixed by the kernel: <code>Plugin &lt;name&gt; stopped after 3 failures in 10 minutes: &lt;export&gt; &lt;code or trap&gt; (&lt;view&gt;). Turn it on again in Settings &gt; Plugins.</code></figcaption>
</figure>

- **Windows and macOS:** third-party plugins need the plugin sandbox, which is not available there. The install is refused with `Cannot install <name> <version>.` followed by `Third-party plugins need the plugin sandbox, which is not available on Windows yet.` Bundled plugins keep working. macOS uses the same text with `macOS`.

### Sharing

A package is a file. A registry is optional and none is set up by default. Pack the project with `ilium plugin pack`, sign it with a key from `ilium plugin keygen`, and pass the `.iliumpkg` file on. The recipient runs `ilium plugin install`. Tutorial step 9 walks through the whole path.

<div class="note">
<p>The renders on these pages are illustrative, not generated. They use the glyphs that the Ilium client draws, but exact text and spacing may differ in a release.</p>
</div>

### See also

- [What a plugin is](/pages/getting-started-overview/) and [Set up a toolchain](/pages/getting-started-toolchain/)
- [Your first plugin in five minutes](/pages/getting-started-first-plugin/)
- [Tutorial, step 1: agent badge](/pages/tutorial-01-agent-badge/) and [step 9: package and share](/pages/tutorial-09-package-and-share/)
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/)
- [Sides and worlds](/pages/worlds-and-sides/) and [Lifecycle](/pages/lifecycle/)
- [Capabilities](/pages/capabilities/) and [Security](/pages/security/)
- [Errors and limits](/pages/errors-and-limits/)
