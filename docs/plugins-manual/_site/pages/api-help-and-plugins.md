# help and plugins

## help and plugins

The `help` and `plugins` imports cover two related jobs. The `help` import reads the Markdown help topics that installed plugins ship in their packages. The `plugins` import lists every installed plugin and turns plugins on or off. Both imports are in the client world, so a client-side plugin can call them and a server-side plugin cannot.

Reading help needs an ordinary grant. Managing plugins needs a high-risk grant (H), and only first-party bundled plugins at tier T1 can hold it. A plugin that turns another plugin off stops that plugin's work at once, so the user must allow the grant explicitly.

<figure>
<svg viewBox="0 0 300 232" role="img" aria-label="A reading plugin calls help topic, the kernel help registry reads the package help file">
  <defs>
    <marker id="arr-help-topic" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="20" y="10" width="260" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="150" y="36" text-anchor="middle" fill="currentColor" font-size="12">Reading plugin (help.read)</text>
  <line x1="150" y1="54" x2="150" y2="86" stroke="currentColor" marker-end="url(#arr-help-topic)"/>
  <text x="160" y="74" fill="currentColor" font-size="10">topic(id)</text>
  <rect x="20" y="88" width="260" height="44" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="150" y="114" text-anchor="middle" fill="currentColor" font-size="12">Kernel help registry</text>
  <line x1="150" y1="132" x2="150" y2="164" stroke="currentColor" marker-end="url(#arr-help-topic)"/>
  <text x="160" y="152" fill="currentColor" font-size="10">reads help file</text>
  <rect x="20" y="166" width="260" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="150" y="192" text-anchor="middle" fill="currentColor" font-size="12">Installed package help/ folder</text>
  <text x="150" y="226" text-anchor="middle" fill="currentColor" font-size="10">Returns Markdown, or none if unknown.</text>
</svg>
<figcaption>A reading plugin passes a topic id to the help registry. The registry reads the Markdown file from the installed package and returns it, or none when no installed plugin declares the id.</figcaption>
</figure>

### What you need

| Requirement | Detail |
|---|---|
| Grant `help.read` | Reads help topics. The grant takes no argument and has ordinary risk. |
| Grant `plugins.manage` | Lists plugins and turns them on or off. High risk (H), and T1 only. |
| Side | `client`. Both imports are in the `ilium:client/base` world. |
| Language | Rust with `ilium-plugin-sdk`, or TypeScript with `@ilium/plugin-sdk`. |
| Manifest | Each help topic is declared under `contributes.help`, and its Markdown file sits in `help/`. |

A T2 or T3 package that asks for `plugins.manage` is refused at install with `manifest.grant-tier`. A T1 bundled plugin with H capabilities is activated first without them. Ilium then shows a prompt, "Plugin <name> asks for: <list>", with the buttons `Allow selected` and `Not now`. `Not now` asks again at the next server start, at most three prompts in total. The user can allow the grant later in Settings > Plugins. Until then, the calls behave as the edge-case table below describes.

### The surface

Rust paths start at `ilium_plugin_sdk::host`. TypeScript paths start at `@ilium/plugin-sdk/host`.

#### Help topics (`help.read`)

| Function | Rust | TypeScript | WIT | Meaning |
|---|---|---|---|---|
| `topic(id)` | `help::topic(id: &str) -> Option<String>` | `topic(id)`, returns `string` or `undefined` | `option<string>` | Returns the Markdown of the topic with that global id, or none. |

Topic ids are global. A topic id is `<plugin-id>/<local-id>`, where `<local-id>` is the `id` the manifest gives the topic. For example, `com.example.reader/quick-start`. The kernel's own topics use the prefix `ilium/`. The function can read any installed topic, not only the caller's own.

#### Plugin management (`plugins.manage`)

| Function | Rust | TypeScript | WIT | Meaning |
|---|---|---|---|---|
| `list()` | `plugins::list() -> Vec<PluginInfo>` | `list()`, returns `PluginInfo[]` | `list<plugin-info>` | Returns every installed plugin, including disabled and failed ones. |
| `set-enabled(plugin, enabled, scope)` | `plugins::set_enabled(plugin: &str, enabled: bool, scope: &str) -> Result<(), PluginError>` | `setEnabled(plugin, enabled, scope)`, throws `PluginError` | `result<_, plugin-error>` | Turns a plugin on or off. The call returns once the change is recorded. |

The record `plugin-info` has nineteen fields. The WIT in [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/) adds `state`, `side-states`, `scope`, `disabled-reason` and `health` to the six below, and eight distribution fields: `publisher`, `publisher-key`, `source`, `channel`, `pinned`, `update-available`, `deprecated` and `yanked`. `list()` never contacts a registry. `set-enabled` also takes a `scope` (`global`, `project` or `session`) and a global change reaches every running session within 2 seconds ([Plugin scope and sessions](/pages/reference-plugin-scope-and-sessions/)). The first six fields:

| Field | Rust and TypeScript type | Meaning |
|---|---|---|
| `id` | `String` and `string` | Plugin id, such as `com.example.reader`. |
| `name` | `String` and `string` | Display name from the manifest. |
| `version` | `String` and `string` | Version of the installed package. |
| `tier` | `String` and `string` | `T1` (bundled), `T2` (signed) or `T3` (unsigned local). |
| `enabled` | `bool` and `boolean` | `true` when the plugin is turned on. |
| `failed` | `bool` and `boolean` | `true` when the host turned the plugin off after repeated failures. |

### Worked example in Rust

This client plugin adds a palette command. The command reads its own help topic and writes the first line to the plugin log. It needs `help.read`.

The manifest declares the command and the topic:

```json
{
  "id": "com.example.reader",
  "name": "Quick reader",
  "version": "0.1.0",
  "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": [] },
  "capabilities": { "client": ["help.read"] },
  "contributes": {
    "commands": [{ "id": "show-topic", "title": "Show quick start" }],
    "help": [{ "id": "quick-start", "file": "help/quick-start.md", "title": "Quick start" }]
  }
}
```

The Rust source:

```rust
use ilium_plugin_sdk::host::help;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::CommandInvokedEvent;

const QUICK_START: &str = "com.example.reader/quick-start";

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct Reader;

impl Lifecycle for Reader {}

impl Handler for Reader {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::CommandInvoked(CommandInvokedEvent { command, .. }) = event else {
                continue;
            };
            if command == "com.example.reader/show-topic" {
                show_quick_start();
            }
        }
        Vec::new()
    }
}

fn show_quick_start() {
    match help::topic(QUICK_START) {
        Some(markdown) => {
            let heading = markdown.lines().next().unwrap_or("(empty topic)");
            log::log(Level::Info, &format!("quick start opens with: {heading}"));
        }
        None => log::log(Level::Warn, "quick start topic is not installed"),
    }
}
```

Running the palette command writes one line to the plugin log. When the topic is missing, the command writes a warning and returns normally.

### Worked example in TypeScript

This T1 setup plugin turns back on every plugin that the host turned off after failures. It needs `plugins.manage`, which the user must allow. Its command id is `ilium.setup/restore-failed`.

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';
import { list, setEnabled } from '@ilium/plugin-sdk/host/plugins';

const RESTORE_COMMAND = 'ilium.setup/restore-failed';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {},
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === RESTORE_COMMAND) {
        restoreFailedPlugins();
      }
    }
    return [];
  },
});

function restoreFailedPlugins(): void {
  for (const plugin of list()) {
    if (plugin.enabled || !plugin.failed) continue;
    try {
      setEnabled(plugin.id, true, 'global');
      log('info', `turned on ${plugin.name} ${plugin.version} (${plugin.tier})`);
    } catch (error) {
      const code = (error as { code?: string }).code ?? 'unknown';
      log('warn', `could not turn on ${plugin.id}: ${code}`);
    }
  }
}
```

The loop logs one line per plugin. A failed call, such as `not-permitted` when the grant is declined, is logged and the loop moves on to the next plugin.

### What the user sees

- The help pane shows the topics that installed plugins register. Opening a topic shows its Markdown.
- Settings > Plugins lists the installed plugins and their state, with a switch for each one. A switch takes effect at once.
- A plugin that the host turned off after failures shows a notice. The notice reads `Plugin <name> stopped after 3 failures in 10 minutes: <export> <code or trap> (<view>).` followed by `Turn it on again in Settings > Plugins.` The part in parentheses appears only when the failing export was `views.render`.

<pre class="term"><span class="t-prompt">help</span> <span class="t-dim">Quick reader &gt; Quick start</span>
<span class="t-dim">╭  ≡ ● · Quick start─────────────────────────────────────────────╮</span>
<span class="t-dim">│</span> Open the palette and run Show quick start. The plugin writes   <span class="t-dim">│</span>
<span class="t-dim">│</span> the first line of this topic to the plugin log.                <span class="t-dim">│</span>
<span class="t-dim">│</span>                                                                <span class="t-dim">│</span>
<span class="t-dim">│</span> <span class="t-dim">Source: com.example.reader 0.1.0</span>                               <span class="t-dim">│</span>
<span class="t-dim">╰────────────────────────────────────────────────────────────────╯</span>
</pre>

<pre class="term"><span class="t-prompt">Settings</span> &gt; <span class="t-hi">Plugins</span>
 Plugin        Tier  Version  State
 Quick reader  T1    0.1.0    <span class="t-ok">on</span>
 Notes panel   T2    1.4.1    <span class="t-err">failed</span>

<span class="t-warn">Plugin Notes panel stopped after 3 failures in 10 minutes: views.render</span>
<span class="t-warn">trap (notes-list). Turn it on again in Settings &gt; Plugins.</span>
</pre>

Illustrative, not generated: the help pane for the quick start topic, and the Settings > Plugins page after one plugin has failed.

### Lifecycle effects

`set-enabled(id, false)` takes effect at once. The call returns `ok` when the change is recorded. The plugin then receives `deactivate(disabled)` within its deadline. Its contributions are withdrawn, and its pending requests are cancelled. Enabling records the change and activates the plugin by its activation rules: a `startup` plugin at once, and a lazy plugin at its next trigger. See [lifecycle](/pages/lifecycle/) for the full order of activation and deactivation.

<div class="note">
<p>Turning a plugin on clears its <code>failed</code> flag and starts its failure count again. The plugin does not stay in the failed state after a successful enable.</p>
</div>

### Edge cases and errors

| Case | Result |
|---|---|
| `topic` with an unknown id | `none`. The call does not fail. |
| `help.read` declined or revoked | `none`, and the plugin log gets a `warn` line. |
| Topic of a disabled plugin | Still readable. The Markdown file belongs to the installed package, not to the running code. |
| Topic of a removed plugin | `none`. |
| `plugins.manage` declined or revoked | `list()` returns an empty list, and `set-enabled` returns `not-permitted`. |
| `set-enabled` with an unknown plugin id | `not-found`. |
| `set-enabled` with scope `project` when no project is open, or the open project is not trusted | `not-permitted`, and nothing changes. |
| `set-enabled` to the state a plugin already has | `ok`, and nothing changes. |
| `set-enabled(id, true)` on a failed plugin | `ok`. The `failed` flag is cleared. |
| Revocation during the call | `revoked`. The plugin is re-activated with the reduced grant set. |
| Host fault inside the call | `internal`. |

`not-permitted`, `revoked` and `internal` are universal codes for every call that returns a `result`. Only `not-found` is specific to `set-enabled`. The full table of codes and limits is in [errors and limits](/pages/reference-errors-and-limits/).

### Help topics and settings rows

A settings row in a plugin tab can name a help topic with its `help` key. The topic id must be declared in `contributes.help`, or the install is refused with `manifest.settings-help` and the message `help topic "<id>" at <json path> is not in contributes.help`. The user opens the topic from the row, and the same Markdown is what `help.topic(id)` returns. A plugin therefore needs no `help.read` grant for the help of its own rows; the grant is for code that reads a topic itself. See [settings](/pages/api-settings/).

### Log capture

A plugin that reads a topic and a setup plugin that restores failed plugins write lines like these to the plugin log:

<pre class="term">2026-10-12 10:15:02 example.quick info  help quick-start: 38 lines
2026-10-12 10:15:44 ilium.setup   info  plugins: 14 installed, 1 failed
2026-10-12 10:15:44 ilium.setup   info  set-enabled example.broken true: ok
2026-10-12 10:15:51 example.quick warn  help missing-topic: none
</pre>

The capture is hand-composed to show the log format; the ids are examples.

### WIT

The contract below is an excerpt. The owner is the [WIT reference](/pages/reference-wit/), and that page wins if the two differ.

```wit
// Excerpt of ilium:host/help in reference-wit
/// Markdown. none for an unknown topic.
topic: func(id: string) -> option<string>;
```

### See also

- [Capability: help.read](/pages/capability-help-read/) for the grant review and the manifest rules.
- [Capability: plugins.manage](/pages/capability-plugins-manage/) for the high-risk grant review.
- [Lifecycle](/pages/lifecycle/) for `deactivate(disabled)` and the failure notice.
- [Settings screens](/pages/settings-screens/) for the Settings > Plugins page.
- [Manifest reference](/pages/reference-manifest/) for every `contributes` key.
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/) for the attribute and the `definePlugin` shape.
- [Errors and limits](/pages/reference-errors-and-limits/) for the error codes.
- [Security](/pages/security/) for the tier rules that limit `plugins.manage`.

Checked against `ilium:plugin@0.1.0`.

### Security

Help text is sanitised ([SEC-380]).
