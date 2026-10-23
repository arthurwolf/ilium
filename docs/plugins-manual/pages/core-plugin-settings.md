---
layout: page
title: Settings
permalink: /pages/core-plugin-settings/
---

## Settings

The Settings plugin (id `ilium.core.settings`, side: client) is the plugin form of the Settings screen. It owns the tab strip, the auto-save behaviour, the guided setup shown on first run, the User Interface tab (theme, motion, density, stable glyphs), the Icons, Debug and About tabs, and the file logging switch.

The screen itself and the files it writes (`config.toml` in the user's configuration folder and `.ilium/config.yaml` in a project) belong to the kernel, which keeps them valid. The plugin draws the tabs through the contract, adds its own rows to the User Interface tab, and reads or writes other plugins' values through the settings-all grants.

### What it covers

- **Settings UI and auto-save.** Toggles and choices save at once. Text and number inputs save after 600 ms. See Opening and using Settings and Auto-save behaviour in the Settings page of the user documentation.
- **Guided setup and onboarding.** First-run steps, described under Guided setup and onboarding.
- **Configuration files.** `config.toml` and `.ilium/config.yaml`, described under Configuration files.
- **User Interface tab.** Theme, motion, density and stable glyphs (see Tab reference, User Interface).
- **Icons tab.** Configurable glyphs (see Tab reference, Icons).
- **Debug tab.** File logging, described under Debug and in Diagnostics and logs.
- **About tab.** Version and build details (see Tab reference, About).

### See it

<figure><img src="/assets/demos/07a-settings-tour-a.gif" alt="A tour of the first Settings tabs" loading="lazy"><figcaption>The Settings screen: the tab strip, leader-dotted rows and auto-save.</figcaption></figure>

### What the user sees

<figure><pre class="term">╭─ Settings ───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│ <span class="t-hi">User Interface</span>  Terminal  Editor  Icons                                                                                                                                                                                          │
│ <span class="t-dim">────────────────────────────────────────────</span>                                                                                                                                                                                     │
│ Theme ……………………………………………………………………………… ‹ Dark ›                                                                                                                                                                                    │
│ Reduce motion …………………………………………………………… ‹ Off ›                                                                                                                                                                                    │
│ Density ……………………………………………………… ‹ Comfortable ›                                                                                                                                                                                    │
│ Stable glyphs …………………………………………………………… ‹ Off ›                                                                                                                                                                                    │
│ <span class="t-dim">Saved</span>                                                                                                                                                                                                                            │
│                                                                                                                                                                                                                                  │
╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯</pre><figcaption>Rendered at 48 columns. Illustrative. The tab strip comes first. Rows below it join the label and the control with <code>…</code> leaders. The word Saved shows that the last change was written.</figcaption></figure>
<figure><pre class="term">╭─ Welcome ─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│ <span class="t-dim">Step 2 of 3</span>                                                                                                                                                                               │
│                                                                                                                                                                                           │
│ Notify when an agent finishes …………………… ‹ On ›                                                                                                                                             │
│ Play a sound ……………………………………………………………… ‹ Off ›                                                                                                                                             │
│ Desktop notifications ………………………………………… ‹ On ›                                                                                                                                             │
│                                                                                                                                                                                           │
│ <span class="t-hi">Enter</span> next   <span class="t-hi">Esc</span> skip                                                                                                                                                                     │
│                                                                                                                                                                                           │
╰───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯</pre><figcaption>Rendered at 48 columns. Illustrative. A guided setup page. The rows are generated from a schema by a <code>schema-form</code>; the host saves each change.</figcaption></figure>
### The Plugins tab

The Plugins tab is the one place where a user manages plugins. It belongs to this plugin. Its commands are the same as `ilium plugin ...` on the command line, and the recovery page offers the same actions when this plugin is off (see [Settings screens](/pages/settings-screens/#the-kernel-recovery-page)).

**How to open it.** Press `Ctrl+B` then `:` to open Settings, or press the gear in the tree footer, then choose **Plugins** with `Tab`. A link such as `[ Open Plugins page ]` on a dimmed plugin row opens the tab with that plugin selected. When this plugin is off, the same key opens the kernel recovery page.

**What it shows.** One row per plugin: the id, the tier badge (`T1`, `T2`, `T3`, with `dev` for a development install) and the state. Below the list: Install from file. Selecting a row opens the plugin page with the tabs Overview, Capabilities, Extensions, Log, Details and Settings.

<figure>
<pre class="term"><span class="t-hi">╭─ Plugins ──────────────────────────────────────────────╮</span>
│ ilium.core.tree        T1  <span class="t-ok">On</span>                          │
│ ilium.core.cost        T1  <span class="t-dim">Off</span>                         │
│ com.example.notes      T2  <span class="t-ok">On</span>                          │
│ com.example.cursor     T2  <span class="t-err">Failed</span>                      │
│ com.example.tidy       T2  <span class="t-dim">Waiting for ilium.core.cost</span> │
│ vendor.ext             T3  <span class="t-warn">Refused: no sandbox</span>         │
│                                                        │
│ <span class="t-hi">Enter</span> open  <span class="t-hi">Space</span> on or off  <span class="t-hi">i</span> install file            │
╰────────────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The Plugins tab with one row for each plugin state.</figcaption>
</figure>

#### Plugin row states

The Plugins tab, `ilium plugin list`, the recovery page and every user page use these state words. This table is the only definition. Each word maps to the package state and health of [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/).

| Row word | Meaning | Package state, health | What the user can do |
|---|---|---|---|
| On | Enabled and running | `installed-enabled`, `healthy` | Turn off, update, remove |
| On, slow | Enabled and running, but over a soft limit | `installed-enabled`, `degraded` | Turn off, read the log |
| Restarting | Stopped by an error, a restart is scheduled | `installed-enabled`, `restarting` | Turn off |
| Failed | Stopped after 3 failures in 10 minutes | `installed-enabled` with a side `stopped-by-failure`, `failed` | Turn on (clears the count), reset failures, remove |
| Off | Turned off by the user, the CLI or safe mode | `installed-disabled-by-user`, `disabled` | Turn on |
| Waiting for <owner> | Needs a plugin that is off or missing | `dormant` | Turn on or install the owner |
| Blocked by policy | An administrator rule forbids it | `disabled-by-policy`, `disabled` | Nothing; ask the administrator |
| Blocked | Its publisher key was revoked as `compromised`, or a registry takedown names its version | `blocked`, `disabled` | `ilium plugin enable <id> --accept-risk`, or install an update signed by another trusted key |
| Refused: <reason> | The host refused to run it, for example `no sandbox` | `refused-at-load`, `refused` | Remove, or reinstall a fixed package |

In safe mode, a third-party row reads `Off, safe mode`. A dimmed row of a plugin that is off keeps its settings rows read-only, with the link to this tab.

- [USR-161] The Plugins tab, `ilium plugin list` and the recovery page MUST use the row words of this table for the corresponding states, and MUST NOT introduce another word for the same state. Test: put one plugin in each state and compare the words on the three surfaces.
- [USR-162] The Plugins tab MUST list every installed plugin including core plugins, with its tier badge and state. Test: install two plugins, count the rows, and compare with `ilium plugin list`.
- [USR-163] The tab MUST offer **Forget settings** on a plugin's Details page. It MUST delete that plugin's saved settings values for the user and the open project, MUST ask for confirmation, and MUST NOT touch permissions or data. Test: set a value, forget, assert the value is the default and the grant file is unchanged.
- [USR-164] The state words MUST be the same in the footer notice N14, which is defined in [Lifecycle](/pages/lifecycle/#failure-counting-and-restart). Test: stop a plugin and compare.

### The 26 tabs and who owns them

The Settings plugin draws the tab strip and the auto-save behaviour. Each tab's rows and values belong to the owner plugin in the last column, which writes them with `settings.write-kernel:<tab>`. The Settings plugin itself owns four tabs. [COR-030]

| Position | Tab id | Label | Owner plugin |
|---|---|---|---|
| 1 | `user-interface` | User Interface | [Settings](/pages/core-plugin-settings/) |
| 2 | `padding` | Padding | [Settings](/pages/core-plugin-settings/) |
| 3 | `icons` | Icons | [Settings](/pages/core-plugin-settings/) |
| 4 | `agent-monitoring` | Agent Monitoring | [Agent detect](/pages/core-plugin-agent-detect/) |
| 5 | `agent-cost` | Agent Cost | [Cost](/pages/core-plugin-cost/) |
| 6 | `optimization` | Optimization | [Compaction](/pages/core-plugin-compaction/) |
| 7 | `remote-compaction` | Remote compaction | [Compaction](/pages/core-plugin-compaction/) |
| 8 | `keyboard` | Keyboard | [Keys and help](/pages/core-plugin-keys-and-help/) |
| 9 | `terminal` | Terminal | [Terminal](/pages/core-plugin-terminal/) |
| 10 | `editor` | Editor | [Editor (kernel while native)](/pages/core-plugin-editor/) |
| 11 | `session` | Session | [Sessions](/pages/core-plugin-sessions/) |
| 12 | `git` | Git | [Worktrees](/pages/core-plugin-worktrees/) |
| 13 | `kanban-board` | Kanban Board | [Kanban (kernel while native)](/pages/core-plugin-kanban/) |
| 14 | `animations` | Animations | [Ambient look](/pages/core-plugin-ambient/) |
| 15 | `sound` | Sound | [Notifications](/pages/core-plugin-notifications/) |
| 16 | `voice-control` | Voice control | [Voice](/pages/core-plugin-voice/) |
| 17 | `reset-planning` | Reset planning | [Cost](/pages/core-plugin-cost/) |
| 18 | `inference` | Inference | [Inference](/pages/core-plugin-inference/) |
| 19 | `llm-instructions` | LLM Instructions | [Inference](/pages/core-plugin-inference/) |
| 20 | `titles` | Titles | [Titles](/pages/core-plugin-titles/) |
| 21 | `triggers` | Triggers | [Titles](/pages/core-plugin-titles/) |
| 22 | `text-triggers` | Text Triggers | [Text triggers](/pages/core-plugin-text-triggers/) |
| 23 | `debug` | Debug | [Settings](/pages/core-plugin-settings/) |
| 24 | `api` | API | [Fleet](/pages/core-plugin-fleet/) |
| 25 | `about` | About | [Settings](/pages/core-plugin-settings/) |
| 26 | `setup` | Setup | [Agent setup](/pages/core-plugin-agent-setup/) |

The table order is the rail order, equal to `src/docs/settings.md`. Plugin tabs follow after About and Setup. The labels are the baseline labels; the high-risk marking of the API tab is a notice row inside the tab, not part of the label.

### Guided setup and per-option help

The Settings plugin owns the guided setup (`ilium --onboarding`) as one flow. Each step is a section owned by the plugin that the step configures: the Voice step shows the voice demo of the [voice](/pages/core-plugin-voice/) plugin, and the AI step calls the [inference](/pages/core-plugin-inference/) plugin. The setup keeps its own state until the user finishes, and automatic AI stays off until then. The per-option help (the topics of `settings_help/catalog/topics.json`, about 110 topics) belongs to [keys and help](/pages/core-plugin-keys-and-help/). Each owner plugin contributes the topics of its own rows through the `help` contribution, and the Settings screen shows them next to the row. [COR-011]

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

The plugin ships with Ilium as a first-party (T1) package. `settings.read-all` and `settings.write-all` are limited to bundled plugins of this tier.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.settings",
  "name": "Settings",
  "version": "0.1.0",
  "publisher": "Ilium",
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": ["views"]
  },
  "capabilities": {
    "client": ["settings.contribute:user-interface", "settings.read-all",
               "settings.write-all", "ui.panels"]
  },
  "contributes": {
    "settings": [
      {
        "kind": "section",
        "id": "appearance",
        "tab": "user-interface",
        "placement": "end",
        "title": "Appearance",
        "scope": "client",
        "rows": [
          { "type": "choice", "key": "density", "title": "Density",
            "default": "comfortable",
            "options": [
              { "value": "compact", "title": "Compact" },
              { "value": "comfortable", "title": "Comfortable" }
            ] },
          { "type": "color", "key": "accent", "title": "Accent colour",
            "swatches": "theme", "default": "#4a9eff" }
        ]
      }
    ],
    "onboarding_steps": [{ "id": "first-run", "title": "Welcome", "order": 10 }],
    "panels": [{ "id": "overview", "title": "All settings", "modal": false }]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.settings",
  "name": "Settings",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "The Settings plugin (id ilium.core.settings, side: client) is the plugin form of the Settings screen.",
  "license": "MIT",
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": [
      "views"
    ]
  },
  "capabilities": {
    "client": [
      "settings.contribute:user-interface",
      "settings.read-all",
      "settings.write-all",
      "ui.panels",
      "settings.write-kernel:user-interface",
      "settings.write-kernel:icons",
      "settings.write-kernel:debug",
      "settings.write-kernel:about",
      "extensions.provide:ilium.core.settings/config-importers",
      "extensions.provide:ilium.core.settings/onboarding-steps"
    ]
  },
  "activation": [
    "startup"
  ],
  "contributes": {
    "extension_points": [
      {
        "id": "config-importers",
        "kind": "collect",
        "title": "Config importers",
        "entry_schema": {"type": "object", "required": ["id", "title", "service"], "properties": {"id": {"type": "string"}, "title": {"type": "string"}, "service": {"type": "string"}}},
        "risk": "standard",
        "max_entries": 64,
        "dynamic": false,
        "version": "0.1.0"
      },
      {
        "id": "onboarding-steps",
        "kind": "collect",
        "title": "Onboarding steps",
        "entry_schema": {"type": "object", "required": ["id", "title", "order"], "properties": {"id": {"type": "string"}, "title": {"type": "string"}, "order": {"type": "integer"}}},
        "risk": "standard",
        "max_entries": 64,
        "dynamic": false,
        "version": "0.1.0"
      }
    ],
    "settings": [
      {
        "kind": "section",
        "id": "appearance",
        "tab": "user-interface",
        "placement": "end",
        "title": "Appearance",
        "scope": "client",
        "rows": [
          {
            "type": "choice",
            "key": "density",
            "title": "Density",
            "default": "comfortable",
            "options": [
              {
                "value": "compact",
                "title": "Compact"
              },
              {
                "value": "comfortable",
                "title": "Comfortable"
              }
            ]
          },
          {
            "type": "color",
            "key": "accent",
            "title": "Accent colour",
            "swatches": "theme",
            "default": "#4a9eff"
          }
        ]
      }
    ],
    "onboarding_steps": [
      {
        "id": "first-run",
        "title": "Welcome",
        "order": 10
      }
    ],
    "panels": [
      {
        "id": "overview",
        "title": "All settings",
        "modal": false
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
| `user-interface` | User Interface | `[ui]`, `[theme]` | Tree, Agent toolbar, Titles, Diagnostics, Terminal |
| `icons` | Icons | `[ui.icons]` | Tree, Agent toolbar |
| `debug` | Debug | `[debug]` | Diagnostics |
| `about` | About | none | none |

If the plugin is disabled, the tab stays in the rail with its stored values shown read-only and the notice "Settings is disabled. Values are kept." The kernel keeps reading the keys it consumes itself (listed in the Consumer column of the key table in [config and data migration](/pages/reference-config-and-data-migration/)). Behaviour that only this plugin provides is off. The exact result for this plugin is its row in [kernel and boot](/pages/reference-kernel-and-boot/). [COR-022]

### Samples

#### 1. A tab from the manifest

Rows that the manifest declares become part of the User Interface tab. The plugin reads its own values with `settings.get_json` and redraws when `settings-changed` names a key it cares about. See the [settings API](/pages/api-settings/), the [settings.contribute grant](/pages/capability-settings-contribute/), [extending settings](/pages/settings-extension/), the [settings screens](/pages/settings-screens/) and the [event catalogue](/pages/api-events/). `emptyTree` is a helper defined in the sample itself (it returns a tree with one empty root node); the SDK has no such function, and [the panel tutorial](/pages/tutorial-03-panel/) shows a full version.

**Rust**

```rust
use ilium_plugin_sdk::host::settings;
use ilium_plugin_sdk::host::ui;
use ilium_plugin_sdk::prelude::*;

// Rows come from the manifest: section "appearance" in tab "user-interface".
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
struct SettingsPlugin {
    density: String,
}

impl Lifecycle for SettingsPlugin {}

impl Handler for SettingsPlugin {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::SettingsChanged(change) = event else { continue };
            if change.keys.iter().any(|key| key == "density") {
                let values: serde_json::Value = serde_json::from_str(&settings::get_json())
                    .unwrap_or(serde_json::Value::Null);
                self.density = values["density"].as_str().unwrap_or("comfortable").to_string();
                ui::invalidate(ViewTarget::Panel("overview".into()));
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { getJson } from '@ilium/plugin-sdk/host/settings';
import { invalidate } from '@ilium/plugin-sdk/host/ui';

// Rows come from the manifest: section "appearance" in tab "user-interface".
let density = 'comfortable';

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  activate() {},
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'settings-changed' || !event.val.keys.includes('density')) continue;
      const values = JSON.parse(getJson()) as { density?: string };
      density = values.density ?? 'comfortable';
      invalidate({ tag: 'panel', val: 'overview' });
    }
    return [];
  },
  render() { return emptyTree(); },
});
```

#### 2. Guided setup

A first-run step shows one settings section as a `schema-form`. The host builds the rows from the section, saves each change, and sends `settings-changed` to the owning plugin, so the plugin writes nothing itself. See the [input widgets](/pages/widgets-input/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::ui::build::Tree;

// Manifest: "onboarding_steps": [{ "id": "first-run", "title": "Welcome", "order": 10 }]
// and a settings section "first-run" with the rows to ask for.
impl Views for SettingsPlugin {
    fn render(&mut self, view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let form = match view {
            ViewTarget::OnboardingStep(_) => {
                tree.schema_form("ilium.core.settings", "first-run").key("form")
            }
            _ => tree.schema_form("ilium.core.settings", "appearance").key("form"),
        };
        tree.finish(form)
    }
}
// The host saves every change. The plugin receives settings-changed.
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { schemaForm, tree } from '@ilium/plugin-sdk/ui/build';
import type { ViewTarget } from '@ilium/plugin-sdk/types';

// Manifest: "onboarding_steps": [{ "id": "first-run", "title": "Welcome", "order": 10 }]
// and a settings section "first-run" with the rows to ask for.
export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  activate() {},
  onEvents() { return []; },
  render(view: ViewTarget) {
    const section = view.tag === 'onboarding-step' ? 'first-run' : 'appearance';
    return tree(schemaForm('ilium.core.settings', section, { key: 'form' }));
  },
});
// The host saves every change. The plugin receives settings-changed.
```

#### 3. Colour scheme

A `color` row sets the accent. A panel shows the same choice in a `color-picker` and redraws when `theme-changed` arrives, so theme roles stay correct. See [pickers](/pages/widgets-pickers/), [settings rows](/pages/reference-settings-rows/) and the [event catalogue](/pages/api-events/).

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::ui::build::Tree;

// Manifest row: { "type": "color", "key": "accent", "title": "Accent colour",
//                 "swatches": "theme", "default": "#4a9eff" }
impl Handler for SettingsPlugin {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::ThemeChanged => ui::invalidate(ViewTarget::Panel("overview".into())),
                Event::UiAction(action) => {
                    if let ("accent", ActionKind::Changed, ActionValue::Text(hex)) =
                        (action.key.as_str(), action.kind, action.value)
                    {
                        self.accent = hex;
                    }
                }
                _ => {}
            }
        }
        Vec::new()
    }
}

impl Views for SettingsPlugin {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let picker = tree.color_picker(&self.accent).key("accent").allow_theme_roles(true);
        tree.finish(picker)
    }
}
```

**TypeScript**

```ts
import { colorPicker } from '@ilium/plugin-sdk/ui/build';
import { invalidate } from '@ilium/plugin-sdk/host/ui';
import type { Event, Submission } from '@ilium/plugin-sdk/types';

// Manifest row: { "type": "color", "key": "accent", "title": "Accent colour",
//                 "swatches": "theme", "default": "#4a9eff" }
let accent = '#4a9eff';

export function onEvents(batch: Event[]): Submission[] {
  for (const event of batch) {
    if (event.tag === 'theme-changed') {
      invalidate({ tag: 'panel', val: 'overview' });
    } else if (event.tag === 'ui-action') {
      const { key, kind, value } = event.val;
      if (key === 'accent' && kind === 'changed' && value.tag === 'text') accent = value.val;
    }
  }
  return [];
}

export const accentPicker = () => colorPicker(accent, { key: 'accent', allowThemeRoles: true });
```

#### 4. All settings in one place

`settings.read-all` lets the plugin list every plugin's tabs and read all values as JSON, and `settings.write-all` applies a merge patch. A `tabs` widget groups the result. Both grants are limited to bundled plugins; see [the settings-all grants](/pages/capability-settings-all/) and [layout widgets](/pages/widgets-layout-and-containers/).

**Rust**

```rust
use ilium_plugin_sdk::host::settings;
use ilium_plugin_sdk::ui::build::Tree;

// Needs settings.read-all (and settings.write-all to change a value).
fn overview_tabs() -> UiTree {
    let mut tree = Tree::new();
    let mut pages = Vec::new();
    let mut titles = Vec::new();
    for entry in settings::list_settings() {
        let json = settings::get_plugin_json(&entry.plugin).unwrap_or_default();
        titles.push(entry.title.clone());
        pages.push(tree.paragraph(vec![vec![Span::plain(&json)]]));
    }
    let mut strip = tree.tabs(titles).key("plugins");
    for page in pages {
        strip = strip.child(page);
    }
    tree.finish(strip)
}

fn reset_density(plugin: &str) -> Result<(), PluginError> {
    settings::set_plugin_json(plugin, r#"{ "density": "comfortable" }"#)
}
```

**TypeScript**

```ts
import { getPluginJson, listSettings, setPluginJson } from '@ilium/plugin-sdk/host/settings';
import { paragraph, span, tabs } from '@ilium/plugin-sdk/ui/build';

// Needs settings.read-all (and settings.write-all to change a value).
export function overviewTabs() {
  const entries = listSettings();
  const pages = entries.map((entry) =>
    paragraph([[span(getPluginJson(entry.plugin))]], { key: `page:${entry.plugin}` }),
  );
  return tabs(entries.map((entry) => entry.title), { key: 'plugins' }, pages);
}

export function resetDensity(plugin: string): void {
  setPluginJson(plugin, JSON.stringify({ density: 'comfortable' }));
}
```

### Extension points

Other plugins change what `ilium.core.settings` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.settings/<point>`). Contributors need `extensions.contribute:ilium.core.settings/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `config-importers` | collect | Offer to import settings from another tool. | entry: id, title, service (returns key and value pairs) | standard |
| `onboarding-steps` | collect | Add a step to the guided setup. | entry: id, title, order (the plugin fills it with a view) | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.settings/get`: get: read a setting by key
- `ilium.core.settings/set`: set: write a setting by key

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.settings/changed`: { key }

Plugin tabs and sections come from `contributes.settings`.

**Replaceable:** no. Its data and its role in the Plugins screen make a replacement unsafe, so `replaceable` is `false`.

#### Example: Adds an import for tmux key settings

A package adds this entry in its manifest. The entry names a service, so the package also ships a small server component that answers the service call. The manifest lists its capabilities in the `capabilities` object, per side, as every manifest does.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.tmux",
  "name": "tmux import",
  "version": "0.1.0",
  "publisher": "Example Labs",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": ["service-provider"]
  },
  "capabilities": {
    "client": ["extensions.contribute:ilium.core.settings/config-importers"],
    "server": ["services.provide:example.tmux/import"]
  },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.settings/config-importers",
        "id": "from-tmux",
        "priority": 50,
        "entry": {
          "title": "Import from tmux",
          "service": "example.tmux/import"
        }
      }
    ]
  }
}
```

### Extension points this plugin owns

The table gives the values that [EXT-052] requires for each point this plugin owns. The page does not state the version, deadline, max or dynamic values, so they come from the defaults in [Extension points](/pages/api-extension-points/) and are marked (default). Both points run on the client side, because the plugin has only a client component and fills each onboarding step with a view; the service in a `config-importers` entry is answered by the contributor's own server component, not by this plugin. The main manifest holds `extensions.provide:ilium.core.settings/config-importers` and `extensions.provide:ilium.core.settings/onboarding-steps` in its client `capabilities` and declares both points in `contributes.extension_points`.

| Point | Side | Version | Deadline | Max | Dynamic |
|---|---|---|---|---|---|
| `ilium.core.settings/config-importers` | client | 0.1.0 (default) | none (default) | 64 entries (default) | no (default) |
| `ilium.core.settings/onboarding-steps` | client | 0.1.0 (default) | none (default) | 64 entries (default) | no (default) |

### ABI used

- [Settings API](/pages/api-settings/) and [settings.contribute grant](/pages/capability-settings-contribute/)
- [Settings extension](/pages/settings-extension/) and [settings screens](/pages/settings-screens/)
- [settings.read-all and settings.write-all](/pages/capability-settings-all/)
- [Settings rows](/pages/reference-settings-rows/)
- [Input widgets](/pages/widgets-input/) and [pickers](/pages/widgets-pickers/)
- [Layout widgets](/pages/widgets-layout-and-containers/)
- [Events](/pages/api-events/)

### See also

- [Core plugins](/pages/core-plugins/)
- [Compaction plugin](/pages/core-plugin-compaction/), which adds the Optimization tab
- [Keys and help plugin](/pages/core-plugin-keys-and-help/), which reads the Keyboard tab

### Security

Project-scope values are ignored until the project is trusted ([SEC-311]). See [Extension security](/pages/reference-extension-security/).

## Production additions: T6-core-plugins

- [COR-215] While ilium.core.settings is down, the kernel MUST serve the last committed settings values read-only to all plugins, reject writes with settings.unavailable, and open the recovery page on the key bound to recovery.
