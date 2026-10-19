# Glossary

## Glossary

This page defines the terms that the other pages of the plugin manual use. Each entry gives the meaning in the contract `ilium:plugin@0.1.0`. Where a term has a page of its own, the entry links to it. Read this page first when a term is unfamiliar.

A plugin is a WebAssembly component that the host loads, checks and runs. It reacts to events, sends intents that change Ilium, and uses only the rights that the user has granted. Most terms below describe one of four things: the package that carries the plugin, the rights it holds, the calls it makes, and the lifecycle that runs it.

### You will need

- Nothing installed, to read this page.
- To write a plugin, the toolchain from [Getting started: toolchain](/pages/getting-started-toolchain/). The `ilium plugin` commands are part of the `ilium` binary, so installing Ilium installs them.
- A Rust or TypeScript project. The examples below use the SDK import paths from the API pages.
- Basic familiarity with JSON, because the manifest is a JSON file.

## Terms

**Action.** A user interaction with a widget, delivered to the client side as a `ui-action` event with a kind, such as `activated` or `changed`, and a value. See [Events and intents](/pages/reference-events-and-intents/).

**Activation.** The moment the host instantiates a plugin and calls `lifecycle.activate`. A plugin is activated at startup, on the first use of one of its contributions, or on one of the triggers in its manifest `activation` list. See [Lifecycle](/pages/lifecycle/).

**Authorization epoch.** A counter that the host increases each time a grant is revoked or added. A host call compares the epoch it started with against the current one. A call that is still in flight when the epoch changes returns `revoked`.

**Bundle.** A folder that holds signed registry metadata and the packages of chosen plugins, copied to a machine with no network. `ilium plugin export-bundle` writes it and `install --from-bundle` reads it. See [Registry reference](/pages/reference-registry/).

**Capability.** A named right that a plugin requests in its manifest and the user grants. Each host import and each event is behind one. See [Capability table](/pages/reference-capabilities/).

**Channel.** A release track of a plugin, `stable` or `beta`. A plugin follows one channel for updates. See [Versions and dependencies](/pages/reference-dependency-resolution/).

**Component.** A WebAssembly component: the compiled form of one side of a plugin, named `server.wasm`, `client.wasm` or `scene.wasm`. A package holds one component per side that the plugin uses. See [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/).

**Contribution.** Something a plugin adds to Ilium: a command, a keybinding, a settings page, a panel, a pane kind, a decoration slot, a scene or a help topic. A static contribution is declared in the manifest. A dynamic one is pushed at runtime. See [Manifest reference](/pages/reference-manifest/).

**`.cwasm`.** A WebAssembly component that has already been compiled for the host. The release embeds the bundled first-party plugins in this form, so they start without a compile step. See [Design decisions](/pages/design-decisions/).

**Decoration.** A short styled text or a Tabler icon that a plugin sets on a tree row, in a slot that the manifest declares, such as a badge or a footer. A decoration with `persist: true` survives a server restart, marked as stale. See [Manifest reference](/pages/reference-manifest/).

**De-duplication key.** The optional `dedup-key` field of a submission: 1 to 128 characters from `[A-Za-z0-9._:/-]`. A submission whose key the same plugin reserved within the last 10 minutes completes as `duplicate(original)` and has no effect. A refused submission does not reserve its key. The Rust helper `dedup::key_for` joins its parts with `:`. A `send-input` or `agent-action` submission must carry a key. See [Events and intents](/pages/reference-events-and-intents/).

**Deprecate.** To mark a whole plugin as no longer maintained, with an optional successor. Installs still work. See [Registry reference](/pages/reference-registry/).

**Editor primitive.** The host-provided pane kind for editing text. It is native. A plugin may provide an editor only after v1. See [Design decisions](/pages/design-decisions/).

**Extension point.** A named slot that one plugin owns and others fill. A `collect` point gathers data entries. A `hook` point runs handlers from other plugins in the owner's flow, in one of four modes: observe, transform, veto or first. Its id is `<plugin-id>/<point>`. See [Extension points](/pages/api-extension-points/).

**Grant.** The user's approval of one capability for one side of a plugin. A grant string is a name, or a name with an argument, such as `fs.read:project`. See [Capability table](/pages/reference-capabilities/).

**Hook.** A handler that a plugin contributes to a hook point. It answers `pass`, `replace`, `refuse` or `answer`, within a short deadline. A hook that is late or fails counts as `pass`. See [Extension points](/pages/api-extension-points/#hooks).

**Host.** The Ilium code that loads plugins, checks their calls and runs them. The host is the kernel together with the plugin host.

**`.iliumpkg`.** The one package format for plugins. It is a deterministic ZIP archive that holds a manifest, one or more components, assets, help topics and a file inventory. It replaces the old `.iliumanim` format. See [Packages](/pages/packages/) and [Porting from .iliumanim](/pages/porting-iliumanim/).

**Intent.** A request that a plugin sends to change Ilium, such as `rename-node` or `send-input`. An intent travels inside a submission. Each submission completes with exactly one outcome. See [Events and intents](/pages/reference-events-and-intents/).

**Job.** A unit of background work that a plugin starts with `jobs.start`. A job runs the `job-runner.run` export in a second instance of the same component, with its own deadline and memory limit. The plugin reads the result with `take-output`. See [Background jobs](/pages/background-jobs/).

**Kernel.** The part of Ilium that is native code and not a plugin (tier T0). It owns the tree, the panes, the settings store, the snapshot and the recovery page.

**Key id.** The fingerprint of a publisher public key: the SHA-256 of the key bytes, written as 8 groups of 4 hexadecimal characters. See [Signing and trust](/pages/reference-signing-and-trust/).

**Lock file.** The file `ilium-plugins.lock` in a project. It records the exact version and package hash of each plugin, so `install --locked` gives the same set everywhere. See [Versions and dependencies](/pages/reference-dependency-resolution/).

**Mirror.** A copy of a registry on another host or share. Clients check the signatures of the original registry, never the mirror operator. See [Registry reference](/pages/reference-registry/).

**Namespace.** The first part of a plugin id, such as `com.example`, that one publisher owns in a registry. See [Registry reference](/pages/reference-registry/).

**Outcome.** The result of a submission: `ok`, `ok-node`, `ok-epoch`, `duplicate`, `refused`, `failed` or `cancelled`. See the table in [Reference values](#reference-values).

**Pane.** A node of the tree that shows a running process, an agent, an editor or a plugin's own view.

**Pin.** To hold a plugin at one version so that updates skip it. See [Distribution commands](/pages/reference-plugin-distribution-cli/).

**Preopen.** A directory that the host opens for a granted file scope and passes to the component through WASI. A plugin sees only the preopens of the scopes it was granted, such as `/project` or `/data`.

**Primitive.** A host-provided pane kind that a plugin can build on or extend. The four primitives are `terminal`, `agent`, `editor` and `basic`, imported from `ilium_plugin_sdk::primitives` in Rust. See [Primitives](/pages/primitives/).

**Provider.** A plugin that supplies a service to others: a service provider for a named interface, or an inference provider for model calls. The host routes calls to the provider. See [Services](/pages/services/).

**Publisher id.** The identifier of a publisher, stored with the signing key and equal to the start of every plugin id that the key signs. It is different from the display name in `publisher`. See [Signing and trust](/pages/reference-signing-and-trust/).

**Registry.** An optional source of plugins: signed JSON files and package blobs on an HTTPS host or file share. Ilium ships with none configured (decision D27). See [Registry reference](/pages/reference-registry/).

**Replacement.** A plugin that lists another plugin in `replaces` and provides its whole `surface`. The replaced core plugin is turned off while the replacement is on. See [Extending core plugins](/pages/extending-core-plugins/#replace).

**Reproducible build.** A build in which two machines make the same package bytes from the same source. See [Publishing guide](/pages/guide-publishing/#reproducible-builds).

**Restored state.** The bytes that a plugin returns from `deactivate`. The host stores them and passes them back as `restored` on the next activation. The limit is 1 MiB. Larger bytes are dropped, and the next activation receives no value.

**Retained mode.** The rule that plugins run when an event arrives or a timer fires, and push their state to the host. The host keeps that state and draws it. A plugin's `views.render` is not called on every frame. See [Retained mode](/pages/retained-mode/).

**SBOM.** Software bill of materials: a list of the libraries inside a component, kept in `sbom.json`. See [Publishing guide](/pages/guide-publishing/#sbom).

**Scene.** An animated background, written as a `scene` component that implements the `renderer` export. A scene draws into a surface, and the host applies the shared look to the result. See [Scenes](/pages/api-scenes/).

**Scope.** A named area of the file system that a `fs` grant refers to, such as `project`, `data` or `cache`. See [File access](/pages/file-access/).

**Shared look.** The colour, style and dithering settings that the user sets once and the host applies to every scene and surface. A plugin does not add its own brightness or palette controls. See [Accessibility and theming](/pages/accessibility-and-theming/).

**Side.** One of the three places a plugin runs: `server` (S), `client` (C) or `scene` (A, animation). Each side has its own component, grant list, instance, state and failure count. See [Worlds and sides](/pages/worlds-and-sides/).

**Surface.** A pixel buffer that a plugin writes into. It is shown either by a `surface` widget or as the frame of a scene. The formats are listed in [Reference values](#reference-values).

**Tier.** The trust level of a plugin: T0 for the kernel, T1 for bundled first-party code, T2 for signed third-party code and T3 for unsigned local code. See [Security model](/pages/security/).

**Tree.** The hierarchy of nodes that Ilium shows in the sidebar, made of groups and panes.

**Trust store.** The file in which the host keeps the publisher keys that the user or administrator trusts, each with a tier. See [Signing and trust](/pages/reference-signing-and-trust/).

**Widget.** One visual element of a plugin's view, such as a `block`, a `paragraph`, a `table` or a `tabs` widget. See the widget pages, starting with [Widgets index](/pages/widgets-index/).

**WIT.** The WebAssembly Interface Types language. The plugin contract is written in WIT, and the Rust and TypeScript bindings are generated from it. Names stay kebab-case in WIT. The SDKs convert them, as shown in the naming rule below.

**World.** The WIT name for the set of imports and exports that one component may use, such as `ilium:server/base@0.1.0`. A manifest names the world of each component.

**Yank.** To withdraw one version from new installs and updates while installed copies keep working. See [Registry reference](/pages/reference-registry/).

## Further terms

**Cookbook.** The set of tested recipes in [Patterns cookbook](/pages/guide-patterns-cookbook/): debounce, throttle, cache, retry and others.

**Data version.** The integer `data_version` in the manifest. A change makes the host run `migrator` before `activate`. See [Data migration](/pages/guide-data-migration/).

**Display width.** The number of terminal cells a text takes. Wide characters take two. Measured with `display-width`. See [Internationalisation](/pages/guide-internationalisation/).

**Fingerprint.** The SHA-256 of a raw publisher public key, shown in the trust dialog. See [Publishing and support](/pages/guide-publishing-and-support/).


**Anchor.** A named place in a built-in Settings tab where a plugin section can attach. Anchors are only ever added. See [Plugin settings extension](/pages/settings-extension/).

**Attention mark.** A flag on a node set with `mark-attention` and cleared with `ack-attention`, behind the grant `tree.attention`. It drives the Attention display mode.

**Catalogue.** A named, typed list of options that a `choice` row or a widget uses as its `source`. A plugin declares its own in `catalogs`. The host serves read-only ones named `ilium.*`, such as `ilium.palettes`.

**Chrome.** The parts of the client around the content: tree toolbar, row actions, status bar segments and pane frame slots. A plugin fills them with the grant `ui.chrome`.

**Client request.** A call from one side of a plugin to its other side, declared in `client_requests` and sent with `client.request`. It is always granted and reaches only the plugin's own sides.

**Consent prompt.** A question the host asks the user at the first use of something that is not a grant by itself: the first geocoding lookup, the first `open-external` to each origin, and the first request to a host taken from a setting. See [Security model](/pages/security/).

**Durable timer.** A timer made with `timers.at` that survives a restart. It fires once, late, as `durable-timer-fired`.

**First-party limits tier.** The value `"first-party"` of the manifest key `limits_tier`. It raises the settings caps and is allowed only for bundled `ilium.*` plugins (error `manifest.limits-tier`).

**Frame slot.** A place in a pane frame (header icon, header text, footer row or banner) that a plugin fills with `set-frame-slot`.

**High-risk grant (H).** A grant that is off by default in every dialog and always needs an explicit user choice, even for a bundled plugin. Nine grants are high risk in the contract.

**Host catalogue.** A catalogue owned by the kernel, such as `ilium.palettes`, `ilium.dithers`, `ilium.filters`, `ilium.style-presets` or `ilium.icons`.

**Input journal.** The record of `send-input` submissions made with `journal: true`. An entry without a recorded outcome after a crash is reported once as `input-uncertain` and never replayed.

**Intent, lifecycle.** An intent that changes a pane's process: `freeze-pane`, `unfreeze-pane`, `replace-pane-command`, `terminate-pane-process` or `resize-pane`. It needs `panes.lifecycle`.

**Limits tier.** The class of caps that apply to a plugin: `standard`, or `first-party` for bundled plugins.

**Long-lived child.** A process started with `process.spawn` that keeps running and streams output as events. It needs `process.long-lived` and a matching `process.run:<program>` grant.

**Prompt epoch.** A counter of the prompts sent to an agent pane. `ok-epoch` returns the new value.

**Restructure batch.** A group of tree changes sent with the intent `restructure`, applied atomically and undone with `revert-restructure`. Undo tokens do not survive a restart.

**Row state.** The enabled, disabled or hidden state of a settings row, set with `settings.set-row-state` or computed from `visible_when`.

**Section.** A block of settings rows that a plugin adds to a tab. In a built-in tab it sits under a divider that names the plugin.

**Snapshot, frozen.** A copy of a terminal screen taken with `terminal.freeze-snapshot` so that a plugin reads one consistent state.

**Token provider.** An optional export that supplies syntax tokens for a language declared in `languages`.

**Toast.** A short message drawn by the client, sent with the intent `toast` (grant `notify.toast`).

**Unstable feature.** An item marked `@unstable(feature = <name>)`. It is visible only to a plugin that lists the name in `unstable` and only in development builds for T2 and T3. `gpu.kernels` is one.

**Plugin host.** The part of Ilium that loads components, links their imports, runs their exports under deadlines and enforces their limits. It is the crate `ilium-plugin-host`, and one instance of it runs in each process that hosts plugins. See [Host architecture](/pages/reference-host-architecture/).

**Dispatcher.** The task inside a plugin host that takes events, intents and calls from the kernel, batches them, and hands each call to the plugin executor in priority order. See [Host scheduling](/pages/reference-host-scheduling/).

**Execution quota group.** The token bucket `ilium-execution` of one plugin side: 2,000 ms of CPU time that refills at 500 ms per second. An empty bucket makes submissions return `quota-exceeded`. See [Errors and limits](/pages/reference-errors-and-limits/).

**Worker bank.** The pool of threads, `min(4, cores / 2)` and at least one, that runs background job instances so that jobs never block the plugin executor. See [Background jobs](/pages/background-jobs/).

**Store and instance.** In Wasmtime, a Store holds the memory and state of one running copy of a component, called an instance. Ilium uses one Store per plugin side instance. See [Host runtime](/pages/reference-host-runtime/).

**Epoch tick.** A 10 ms step of the engine clock that Wasmtime uses to interrupt running guest code. The host sets the deadline to `ceil(d / 10) + 1` ticks after the call starts, so a guest that spins traps no earlier than `d` and no later than `d + 20` ms. See [Host runtime](/pages/reference-host-runtime/).

**Sandbox helper.** The separate process `ilium-plugin-helper` that hosts one T2 or T3 plugin side inside a Linux sandbox. See [Sandbox helper protocol](/pages/reference-host-helper/).


**World variant.** A base world (`server/base`, `client/base`, `anim/base`) plus the optional export fragments, such as `with-hook-handler`, that a plugin implements. See [Worlds and sides](/pages/worlds-and-sides/).

**Plugin envelope.** The record `PluginEnvelope` that carries plugin traffic between a client and the server on the existing connection: submissions, outcomes, messages and control. See [Plugin traffic over IPC](/pages/reference-host-ipc/).

**Contribution delta.** The record `ContributionDelta` that carries changes of retained contributions (decorations, status segments, virtual rows) from the server to each client, as a snapshot followed by numbered increments. See [Plugin traffic over IPC](/pages/reference-host-ipc/).

**Caller deadline.** The timeout the caller sets on a request, for example the `timeout_millis` of an HTTP call or the client-request timeout. When it passes, the caller gets `timed-out`. See [Reply deadlines](/pages/reference-errors-and-limits/#reply-deadlines).

**Ceiling.** The largest value a tier may use for a `limits` key. The host clamps a request above the ceiling and logs it. See [Errors and limits reference](/pages/reference-errors-and-limits/).

**Grace.** The 100 ms after a soft deadline. A call that is still running when the grace ends traps. See [Host scheduling](/pages/reference-host-scheduling/).

**Hard deadline.** A deadline at which the host interrupts the call. The call traps and counts as a failure unless its call class says it does not. See [Host scheduling](/pages/reference-host-scheduling/).

**host-capacity.** The reason shown when the host refuses to start an instance because of session limits, such as the memory pool or the helper count. It is not a failure. See [Errors and limits reference](/pages/reference-errors-and-limits/).

**Near-miss.** A call that ended within its deadline but used at least 80% of it. See [Health and watchdog](/pages/reference-health-and-watchdog/).

**retry-after-ms.** A field of the `plugin-error` record. On `quota-exceeded` and `rate-limited` it says how long to wait before the same call can succeed. See [Errors and limits reference](/pages/reference-errors-and-limits/).

**Soft deadline.** A deadline that does not interrupt the call. The call finishes, the host discards a late result, and the breach counts toward the streak limit. See [Host scheduling](/pages/reference-host-scheduling/).

**Streak limit.** The number of consecutive soft-deadline breaches of one call class that trap the instance: 3, or 30 for scene frames. See [Host scheduling](/pages/reference-host-scheduling/).

**Trap.** The instance aborts. The host counts one failure and restarts the side after 1 s, then 5 s, and disables it at the third failure within 10 minutes. See [Errors and limits reference](/pages/reference-errors-and-limits/).

**ABI fault and semantic fault.** An ABI fault is a breach of the binary contract, such as a bad pointer or a malformed record, and traps the instance. A semantic fault is a valid call that the host refuses, and it returns an error code to the plugin. See [Errors and limits reference](/pages/reference-errors-and-limits/).

### Internal decision ids

Some pages cite short ids such as L2 to L6, AX5, E9, C78, A21 and X5. They come from the design notes. L2 to L6 are section labels in [Errors and limits reference](/pages/reference-errors-and-limits/), and AX5 is a decision in [Design decisions](/pages/design-decisions/). E9, C78, A21 and X5 have no page that defines them, and a reader can ignore them. The requirement ids written in square brackets with a prefix and a number are the ones that tests use.

## For users

These terms appear in the Plugins tab, in dialogs and in `ilium plugin list`. Each entry says what the word means for a person who installs and manages plugins. The longer pages are [Plugins for users](/pages/user-plugins-overview/), [Install and manage plugins](/pages/user-install-and-manage/) and [Permissions explained](/pages/user-permissions-explained/).

| Term | What it means for you |
|---|---|
| Plugin | A small program that adds a feature to Ilium. Thirty come with Ilium. Others are installed from a `.iliumpkg` file. |
| Core plugin | A plugin that comes with Ilium and has an id that starts with `ilium.core.`. Badge `T1`. See [Core plugins for users](/pages/user-core-plugins/). |
| Package | One `.iliumpkg` file. Ilium has no store. You get the file from the author. |
| Permission | The plain-language name of a grant: one thing a plugin may do, such as read files in this project. You answer for each one when you install. |
| High risk | A permission that can read private data or change things outside Ilium. It starts unchecked. |
| Signed | The package carries a signature from a publisher key. Badge `T2`. |
| Unsigned, `dev` | The package has no signature. Badge `T3` and `dev`. Install it only if you built it or reviewed it. |
| Publisher key | The key you trust to sign packages. You see its short fingerprint when you trust it. |
| On, Off, Failed | The state words in the Plugins tab. They are defined in [Settings, plugin rows](/pages/core-plugin-settings/#plugin-row-states). |
| Blocked by policy | An administrator rule forbids the plugin. See [Administration](/pages/guide-administration/). |
| Safe mode | A start of Ilium with third-party plugins off for that run. See [Troubleshooting plugins](/pages/user-troubleshooting/). |
| Recovery page | The simple plugin page that works when Settings does not. |

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin list
<span class="t-ok">ok</span>   ilium.core.tree 0.1.0       T1  On
<span class="t-ok">ok</span>   com.example.notes 0.2.0     T2  On
<span class="t-dim">off</span>  com.example.tidy 1.0.0      T3  Off  dev
<span class="t-err">err</span>  com.example.cursor 0.1.0    T2  Failed
     <span class="t-dim">Stopped after 3 failures. See notice N14.</span>
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The plugin list with one row for each of four states.</figcaption>
</figure>

## Reference values

### Tiers

| Tier | Who writes it | How it is loaded | Install rule |
|---|---|---|---|
| T0 | The kernel | Native code | Not a plugin |
| T1 | First-party, bundled | Precompiled `.cwasm` | Granted the manifest ceiling, except high-risk grants |
| T2 | Third parties, signed | `.iliumpkg` with an Ed25519 `signature` | Review dialog, normal grants checked |
| T3 | Local, unsigned | `.iliumpkg` without a signature | Review dialog, normal grants checked |

Windows and macOS refuse T2 and T3 packages until a sandbox exists on those systems. The refusal reads: `Cannot install <name> <version>.` followed by `Third-party plugins need the plugin sandbox, which is not available on Windows yet.` and `Bundled plugins keep working.`

### Sides and worlds

| Side | Component file | Base world |
|---|---|---|
| S, server | `server.wasm` | `ilium:server/base@0.1.0` |
| C, client | `client.wasm` | `ilium:client/base@0.1.0` |
| A, animation | `scene.wasm` | `ilium:anim/base@0.1.0` |

### Outcomes

| Outcome | Meaning |
|---|---|
| `ok` | The submission was applied. For a call, the typed result is ready to take. |
| `ok-node(node-id)` | The submission was applied and created this node. |
| `ok-epoch(u64)` | A `send-input` on an agent pane was applied and carries the new prompt epoch. The SDK treats it as `ok` where code matches only `ok`. |
| `duplicate(original)` | The dedup key was reserved. `original` is the reference of the first submission. |
| `refused(code)` | Not applied, for a check that ran before acceptance. The key is not reserved. |
| `failed(code)` | Accepted, but the host failed to apply it, for example with `io` or `timed-out`. |
| `cancelled` | Stopped by the plugin, the user, a shutdown, or a refusal of its batch. |

### Error codes

| Code | Meaning | Retry |
|---|---|---|
| `not-permitted` | No capability, the user declined it, or the path leaves its scope | No |
| `not-found` | The node, pane, file, command, plugin or other named thing is gone | No |
| `invalid-argument` | The call breaks a rule the plugin can check, such as a size cap | No, fix the call |
| `conflict` | The state changed, or a lock or claim is held | Yes, after re-reading the state |
| `quota-exceeded` | A stored or held quota is full | After freeing resources |
| `rate-limited` | A rate rule throttles the call | Yes, after `retry-after-ms` |
| `busy` | Transient capacity is full | Yes, later |
| `timed-out` | A deadline passed | Yes |
| `cancelled` | The request was cancelled | Maybe |
| `unsupported` | The host or platform cannot do it | No |
| `pane-not-ready` | A restored terminal pane has not started its process | Yes, after the pane event |
| `revoked` | The authorization epoch changed during the call | No, the plugin is re-activated |
| `io` | An operating system I/O error other than a missing file | Maybe |
| `network` | DNS, connect, TLS or transport failure | Yes |
| `internal` | A host bug, or a trap inside a job | No |

### Grants

A grant string is `name` or `name:argument`. The first colon separates the name from the argument, so later colons belong to the argument.

| Grant | Meaning |
|---|---|
| `agent.read` | Read agent state. No argument. |
| `fs.read:project` | Read the project directory, mounted as `/project`. |
| `http:api.example.com` | Call one exact host. |
| `http:*.example.com` | Call any subdomain, at any depth. The bare domain is not included. |
| `process.run:git` | Run the program `git`, resolved through `PATH`. |
| `topics.subscribe:build/*` | Receive every topic under the prefix `build/`. |

Grants are recorded per plugin, per side, and per user configuration. They are not recorded per project. A high-risk grant (H) is never checked by default in the install dialog.

### Further grants

Twenty-three grant rows are new. Ten are high risk (H).

| Grant | Sides | Meaning | Risk |
|---|---|---|---|
| `terminal.read-cells` | S C | Terminal cells, modes, foreground process, frozen snapshot, `screen-changed` | |
| `terminal.history` | S C | Scrollback above the screen | H |
| `terminal.input-lock` | S C | Lock user input for at most 2 s while typing | H |
| `terminal.raw-input` | S C | Raw bytes and mouse events to a pane | H |
| `panes.lifecycle` | S | Freeze, unfreeze, replace command, terminate, resize | H |
| `tree.restructure` | S C | Create groups, folders, projects, splits; restructure batches | H |
| `tree.attention` | S C | Attention marks | |
| `ui.chrome` | S C | Tree toolbar, row actions, status segments, frame slots | |
| `open-external` | S C | Open a link or file through the host | |
| `sessions.cross` | S | Reach the same plugin in other live sessions | H |
| `process.long-lived` | S C | Long-lived child processes | H |
| `settings.read-kernel:<tab>` | S C | Read one built-in tab; `api` is H | H for `api` |
| `settings.write-kernel:<kernel-tab-id>` | S | Own one built-in settings tab; T1 only | H |
| `keymap.read` | C | Read the keymap | |
| `agent.transcript` | S C | Typed agent transcript turns | H |
| `location.geocode` | C A | Place search through the host | |
| `inference.embed` | S C | Embeddings through the selected provider | |
| `notify.toast` | S C | Toasts | |
| `journal.read` | S C | Host and other plugins' journal entries | |
| `gpu.kernels` | A | Host GPU kernels, `@unstable`, development builds only | |
| `panes.create-command:<program>` | S C | Create a pane that runs one named program; needs `panes.create` | H |
| `extensions.provide:<plugin-id>/<point>` | S C A | Own one extension point and receive its contributions | |
| `extensions.contribute:<plugin-id>/<point>` | S C | Contribute to one extension point; high when the point declares `risk: "high"` | |

New argument forms: `http:setting:<key>`, `fs.read:agent-sessions:gemini`, `fs.read:agent-config:gemini`, `fs.read:agent-config:antigravity` and `settings.read-kernel:<kernel-tab-id>`.

### Further events and intents

| Kind | Names |
|---|---|
| Events | `screen-changed`, `user-line-submitted`, `signal-changed`, `agent-model-changed`, `pane-lifecycle-changed`, `attention-changed`, `kernel-settings-changed`, `keymap-changed`, `editor-pointer`, `editor-viewport-changed`, `durable-timer-fired`, `input-uncertain`, `display-mode-changed`, `client-request` |
| Intents | `freeze-pane`, `unfreeze-pane`, `replace-pane-command`, `terminate-pane-process`, `resize-pane`, `create-group`, `create-folder`, `create-project`, `create-split`, `restructure`, `revert-restructure`, `mark-attention`, `ack-attention`, `set-status-segment`, `clear-status-segment`, `set-frame-slot`, `open-external`, `toast`, `dismiss-toast`, `set-bookmarked`, `set-expanded`, `set-inferred-icon` |
| UI actions and values | `ui-action` kinds and values grow with the 51 widgets; see [Events and intents](/pages/reference-events-and-intents/) |

### Further manifest errors

`manifest.catalog-unknown`, `manifest.contribution-limit`, `manifest.contribution-target`, `manifest.row-format`, `manifest.limits-tier`, `manifest.when-key`, `manifest.client-request-unknown`, `manifest.asset-size`, `manifest.visible-when`, `manifest.menu-depth`, `manifest.settings-source`, `manifest.cli-field` and `manifest.scene-prerender`. Each refuses the install and names the key path. See [Errors and limits](/pages/errors-and-limits/).

### Renders

The status bar and a pane frame as the client draws them:

<pre class="term">╭──≡ ● · Fix auth <span class="t-hi">3 files</span>──────────────────────────╮
│<span class="t-warn">! Context 92% full</span>                                │
│$ cargo build                                     │
│   Compiling ilium-core v0.1.0                    │
│<span class="t-hi">build ok · 12 s</span>                                   │
╰──────────────────────────────────────────────────╯
 ℹ NORMAL  ·  <span class="t-hi">quota 71%</span>  ·  Fix auth
</pre>

Settings rows with `…` leaders and the real controls:

<pre class="term"><span class="t-hi">╭──≡ ● · Settings · Look───────────────────────────────────╮</span>
│ Observer location ……………………………………………………… [ Lyon, France ] │
│ Palette …………………………………………………………………………………………… ← Sunset → + │
│ Speed ………………………………………………………………………………………………………… - 1.5 + * │
│ Stars ………………………………………………………………………………………………………………………… [x] │
╰──────────────────────────────────────────────────────────╯
</pre>

A bundled plugin's first-start prompt:

<pre class="term"><span class="t-hi">╭─ Bundled plugin, first start ──────────────────────────────────────╮</span>
│ Plugin ilium.pane-tools asks for:                                  │
│                                                                    │
│ [x] Read terminal scrollback …………………………………………………… terminal.history │
│ [ ] Type into terminals, lock input ………………………… terminal.input-lock │
│ [ ] Read agent transcripts ………………………………………………………… agent.transcript │
│                                                                    │
│            <span class="t-hi"> Enter </span> Allow selected        <span class="t-hi"> Esc </span> Not now             │
│ <span class="t-dim">Space toggle · Enter allow selected · Esc not now</span>                  │
╰────────────────────────────────────────────────────────────────────╯
</pre>

A plugin stopped by the restart rule:

<pre class="term"><span class="t-prompt">$</span> ilium plugin list
<span class="t-ok">ok</span>   example.demo 0.1.0  tier T2  sides server, client
<span class="t-err">err</span>  example.hello-badge 0.1.1  stopped by the restart rule
     <span class="t-dim">Stopped after 3 failures: handler.on-events trap.</span>
     <span class="t-dim">See notice N14 in Lifecycle.</span>
</pre>

### File scopes and preopens

| Scope | Preopen | Default |
|---|---|---|
| `package` | `/package` | Always, read only |
| `data` | `/data` | Always, read and write |
| `cache` | `/cache` | Always, read and write, evictable |
| `project` | `/project` | Granted |
| `picked` | `/picked/<n>` | Granted per pick |
| `path:<template>` | `/paths/<n>` | Granted, high risk for a literal absolute path |

Templates can start with `{home}`, `{config}`, `{data}`, `{cache}`, `{project}` or `{worktrees}`.

### Activation triggers

`startup`, `event:<event-name>`, `command:<id>`, `view:<id>`, `pane-kind:<id>`, `scene:<id>`, `cli:<name>`, `http:<path>`, `service:<interface>`, `topic:<topic>` and `settings:<tab-id>`. Without an `activation` list, a plugin is activated on the first use of any of its contributions.

### Deactivation reasons

| Reason | When it happens |
|---|---|
| `shutdown` | The server or client stops. The plugin has its deactivate deadline (500 ms by default). |
| `disabled` | The user or `plugins.set-enabled` turns the plugin off. |
| `upgrade` | An update or `ilium plugin dev` replaces the running version. |
| `crash-restart` | Declared in the WIT enum but never delivered: a trap drops the instance without a call. |
| `revoked` | A capability was revoked. The plugin is re-activated with fewer grants. |
| `granted` | A capability was added. The plugin is re-activated with more grants. |
| `detached` | Not a deactivation reason. A client that disconnects cleanly deactivates its instance with `shutdown`, and the instance returns restore bytes for the next attach. |

### Surface formats

| Use | Formats |
|---|---|
| A scene surface (`ilium:anim/surface`) | `gray8`, `rgb8`, `rgba8` |
| A `surface` widget | `mask8`, `mono1`, `gray8`, `rgb8`, `rgba8` |

A scene row is `width` bytes for `gray8`, `3 x width` for `rgb8` and `4 x width` for `rgba8`.

## Worked examples

### Rust: a keyed submission

This server-side function builds a submission that sets a badge on one node. The key names the node and the change. The same change sent again within 10 minutes completes as `duplicate`.

```rust
use ilium_plugin_sdk::dedup;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::SetDecorationSpec;

/// Sets the `hello` badge on one node. The key names the node and the change.
fn badge_submission(node: NodeId, change: &str) -> Submission {
    let node_text = format!("{node:?}");
    let intent = Intent::SetDecoration(SetDecorationSpec {
        node,
        slot: "hello".into(),
        content: vec![Span::plain("hi")],
        icon: None,
        sort_key: None,
        persist: false,
    });
    let key = dedup::key_for(&["badge", node_text.as_str(), change]);
    Submission::keyed(intent, key)
}
```

### TypeScript: a client plugin that answers a command

This client-side plugin answers its own command with a desktop notice. The dedup key makes a second invocation within 10 minutes complete as `duplicate`.

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
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
        dedupKey: 'hello:greet',
      });
    }
    return submissions;
  },
});
```

### Manifest: the grants for each side

Grants are listed per side. Each side has its own grant list, and the host checks each component against its own list.

```json
"capabilities": {
  "server": ["agent.read", "tree.decorate", "fs.read:project"],
  "client": ["commands.invoke:com.example.hello/*"]
}
```

## What the user sees

When a plugin's first activation needs high-risk grants, the host shows one prompt per plugin. The user can allow the selected grants or refuse for now. A refusal is asked again at the next server start, at most three times.

<pre class="term"><span class="t-prompt">$</span> ilium plugin list
<span class="t-ok">ok</span>   example.demo 0.1.0  tier T2  sides server, client
<span class="t-err">err</span>  example.hello-badge 0.1.1  stopped by the restart rule
     <span class="t-dim">Stopped after 3 failures: handler.on-events trap.</span>
     <span class="t-dim">See notice N14 in Lifecycle.</span>
</pre>

Illustrative, not generated. A plugin list after the restart rule disabled one plugin.

<pre class="term"><span class="t-warn">Plugin example.hello-badge asks for:</span>
  [x] <span class="t-hi">agent.read</span>
  [x] <span class="t-hi">fs.read:project</span>
  [ ] <span class="t-hi">process.run:git</span>        high risk

      <span class="t-ok">[ Allow selected ]</span>
      <span class="t-dim">[ Not now ]</span>
</pre>

Illustrative, not generated. The grant prompt for a bundled plugin. `fs.read:project` is a normal-risk grant and starts checked. `process.run:git` is high risk and starts unchecked.

<figure>
<svg viewBox="0 0 400 180" role="img" aria-label="A package is loaded, activated and then runs as a plugin">
  <title>From package to running plugin</title>
  <desc>The package is verified and split into components. The grants are checked, the host activates the plugin, and the plugin code runs.</desc>
  <defs>
    <marker id="glossary-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6"
            markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="12" width="170" height="34" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="95" y="33" text-anchor="middle" font-size="12" fill="currentColor">Package (.iliumpkg)</text>
  <rect x="10" y="72" width="170" height="34" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="95" y="93" text-anchor="middle" font-size="12" fill="currentColor">Components per side</text>
  <rect x="10" y="132" width="170" height="34" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="95" y="153" text-anchor="middle" font-size="12" fill="currentColor">Host activates plugin</text>
  <rect x="220" y="72" width="170" height="34" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="305" y="93" text-anchor="middle" font-size="12" fill="currentColor">Plugin code runs</text>
  <path d="M95 46 L95 70" stroke="currentColor" fill="none" marker-end="url(#glossary-arrow)"/>
  <text x="105" y="62" font-size="10" fill="currentColor">verify, split</text>
  <path d="M95 106 L95 130" stroke="currentColor" fill="none" marker-end="url(#glossary-arrow)"/>
  <text x="105" y="122" font-size="10" fill="currentColor">grants checked</text>
  <path d="M180 149 L305 149 L305 108" stroke="currentColor" fill="none" marker-end="url(#glossary-arrow)"/>
  <text x="230" y="142" font-size="10" fill="currentColor">activate</text>
</svg>
<figcaption>A package becomes a running plugin in three steps: the host verifies it and splits it into components, checks the grants of each side, and then activates the plugin.</figcaption>
</figure>

## Edge cases

- A refused submission does not reserve its dedup key, so a retry with the same key after fixing the cause is correct.
- A submission that was accepted and then failed keeps its key. A retry of a failed action needs a new key.
- A capability revoked during a call makes that call return `revoked`. The plugin is re-activated with the smaller grant set, and this is not a failure.
- Three failures in 10 minutes disable a plugin on one side. The first failure restarts it after 1 s and the second after 5 s. The third disables it. Details: [Lifecycle](/pages/lifecycle/#failure-counting-and-restart). A manual re-enable resets the count.

**Component mode.** A test mode in which the harness loads the built `.wasm` component in a WebAssembly runtime and enforces the limits. See [Test harness reference](/pages/reference-test-harness/).

**Conformance case.** One check of the conformance suite with a stable id such as `CONF-LIFE-02`. See [Conformance suite](/pages/reference-conformance-suite/).

**Dev badge.** The mark that Settings > Plugins and the plugin list show for a plugin installed by `ilium plugin dev`. See [Debugging tools](/pages/reference-debugging-tools/).

**Expectation file.** A JSON file next to a fixture that states the submissions, logs and outcomes the plugin must produce, with the match modes `exact`, `ordered` and `subset`. See [Test harness reference](/pages/reference-test-harness/).

**FakeHost.** The test double that stands in for the host for one plugin on one side. See [Testing plugins](/pages/testing/).

**FakeSession.** The test double that runs several plugins, both sides and core extension point stubs in one fake session. See [Test harness reference](/pages/reference-test-harness/).

**Fixture.** A JSON file that sets the starting state of the fake host and lists events to deliver. See [Test harness reference](/pages/reference-test-harness/).

**Golden file.** A text file that holds the expected render of a view, compared by `assert_golden`. See [Testing plugins](/pages/testing/).

**Lint rule.** A named, severity-ranked check of `ilium plugin lint`, such as `lint.grant-unused`. See [Check and lint rules](/pages/reference-check-and-lint-rules/).

**Native mode.** A test mode in which the plugin runs as a plain library on the host machine, with no component and no enforced limits. See [Test harness reference](/pages/reference-test-harness/).

**Replay file.** A JSONL recording of the inputs of a session, written by `trace --record` and re-run by `replay`. See [Test harness reference](/pages/reference-test-harness/).

**Snapshot.** A stored value, pretty JSON with sorted keys, that a test compares with `assert_snapshot`. See [Test harness reference](/pages/reference-test-harness/).

**Trace.** The ring buffer of 1,024 records per plugin instance that shows events, submissions, host calls and outcomes. See [Debugging tools](/pages/reference-debugging-tools/).

## Conventions

**Correlation number.** The `u64` a plugin puts on a submission. Values below 2^63 belong to the plugin, values from 2^63 up to the SDK. See [WIT reference conventions](/pages/reference-wit/#2-conventions).

**Dedup key.** A string of 1 to 128 characters from `[A-Za-z0-9._:/-]` that makes a submission happen once within 10 minutes. See [Intents](/pages/api-intents/).

**epoch-ms.** A `u64` count of milliseconds since the Unix epoch, UTC. Durations are millisecond counts in fields that end in `-ms`.

**Colour.** A `u32` written `0x00RRGGBB`. The top byte is zero.

**Id types.** `node-id`, `request-id`, `client-id`, `buffer-id` and the other ids are unsigned integers that are opaque to the plugin.

**Character limit.** A limit in "characters" counts Unicode scalar values. A limit in "bytes" counts UTF-8 bytes.

## See also

- [Manifest reference](/pages/reference-manifest/)
- [Capability table](/pages/reference-capabilities/)
- [Events and intents](/pages/reference-events-and-intents/)
- [Errors and limits](/pages/errors-and-limits/)
- [Security model](/pages/security/)
- [Lifecycle](/pages/lifecycle/)
- [Debugging](/pages/debugging/)
- [Roadmap](/pages/roadmap/)

Checked against the API version `ilium:plugin@0.1.0`.

### Security

**Audit log**: security record of grants and policy events ([Audit log](/pages/reference-audit-log/)). **Admin policy**: system TOML file that limits plugins ([Admin policy](/pages/reference-admin-policy/)). **Project trust**: user decision that a repository's `.ilium/` values apply ([SEC-310]). **Blocklist**: signed list of banned packages ([SEC-146]). **Safe mode**: start with third-party code off ([SEC-328]). **Data class**: label for kind of data in a point field ([SEC-281]). **Toxic combination**: read grant plus egress grant ([SEC-004]). **Key pinning**: a package keeps its first signing key ([SEC-142]). **Rotation statement**: signed move to a new key ([SEC-143]).
