---
layout: page
title: "20. Extend six core plugins from one package: a team pack"
permalink: /pages/tutorial-20-extend-six-core-plugins/
---

## 20. Extend six core plugins from one package: a team pack

This step is mostly manifest work. The plugin is `example.team-pack`, a package that a team installs once. It adds one entry to each of six core plugins: a recogniser for Smart Copy, a rule pack for Text Triggers, a price source for Cost, a palette for Ambient, a key hint for Keys and help, and an instruction block for Agent setup. Almost all of it is data in `contributes.extensions`, and it needs no code. One part needs code: the per-project rules. The team keeps a file `.ilium/team-rules.json` in each repository, and the plugin registers its rules at run time with `register`.

You start a fresh plugin. You have finished tutorials 1 to 9 and read [Extension points](/pages/api-extension-points/). The points of every core plugin are in [Reference: core extension points](/pages/reference-core-extension-points/). [Extending core plugins](/pages/extending-core-plugins/) compares the ways to extend a core feature. [Tutorial 18](/pages/tutorial-18-add-an-extension-point/) shows the owner side, and [tutorial 19](/pages/tutorial-19-stop-core-actions-with-hooks/) the hook side. This page does not repeat the reference tables.

Tutorial plugin: `example.team-pack`, version 0.1.0, API `ilium:plugin@0.1.0`. Rust and TypeScript behave the same in every step.

### You will need

- Rust: stable 1.90 or later with the `wasm32-wasip2` target, and the crates `ilium-plugin-sdk` and `serde_json`. TypeScript: Bun 1.2 or later and `@ilium/plugin-sdk`.
- Seven grants: one `extensions.contribute:<point>` for each of the six points, and `fs.read:project` for the rules file. Step 1 lists them.
- A file `help/team-style.md` in the package. The instruction block points to it.
- A scratch folder for package inspection: `/media/arthur/tmp/team-pack/` with a `.scratch-reaper` file in it.

### The surface this step uses

| Core plugin | Point (global id) | Kind | Entry fields |
|---|---|---|---|
| Smart Copy | `ilium.core.smart-copy/recognisers` | collect | `id`, `kind`, `regex`, `multiline`, `label` |
| Text Triggers | `ilium.core.text-triggers/rule-packs` | collect | `id`, `title`, `rules` (a list of `pattern`, `reply`, `enabled`) |
| Cost | `ilium.core.cost/price-sources` | collect | `id`, `model_pattern`, `input_per_mtok`, `output_per_mtok`, `cache_read_per_mtok`, `currency` |
| Ambient | `ilium.core.ambient/palettes` | collect | `id`, `title`, `stops` |
| Keys and help | `ilium.core.keys-and-help/key-hints` | collect | `id`, `context`, `chord`, `label` |
| Agent setup | `ilium.core.agent-setup/instruction-blocks` | collect | `id`, `title`, `marker`, `file` |

The host calls are `extensions::register` and `unregister` (TypeScript `register` and `unregister`) for the run-time part, and `fs::read_all` for the rules file. The kernel topic `ilium.extensions/changed` goes to each owner when your entries change.

### The flow

Install does the six static contributions by itself. The code runs afterwards and adds the per-project rules.

<figure>
<svg viewBox="0 0 480 310" role="img" aria-label="Sequence of installing the team pack">
  <title>Installing the team pack</title>
  <desc>The user installs the package. The kernel checks each entry against the owner's schema and tells each owner. Later the plugin reads the rules file and registers a rule pack with Text Triggers.</desc>
  <defs>
    <marker id="t20-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="8" width="70" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="45" y="25" font-size="11" text-anchor="middle" fill="currentColor">User</text>
  <rect x="140" y="8" width="90" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="185" y="25" font-size="11" text-anchor="middle" fill="currentColor">Kernel</text>
  <rect x="290" y="8" width="80" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="330" y="25" font-size="11" text-anchor="middle" fill="currentColor">Team pack</text>
  <rect x="400" y="8" width="70" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="435" y="25" font-size="11" text-anchor="middle" fill="currentColor">Owners</text>
  <line x1="45" y1="34" x2="45" y2="300" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="185" y1="34" x2="185" y2="300" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="330" y1="34" x2="330" y2="300" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="435" y1="34" x2="435" y2="300" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="45" y1="60" x2="185" y2="60" stroke="currentColor" marker-end="url(#t20-arrow)"/>
  <text x="115" y="54" font-size="10" text-anchor="middle" fill="currentColor">install, allow grants</text>
  <text x="250" y="100" font-size="10" text-anchor="middle" fill="currentColor">check six entries</text>
  <line x1="185" y1="128" x2="435" y2="128" stroke="currentColor" stroke-dasharray="5 3" marker-end="url(#t20-arrow)"/>
  <text x="310" y="122" font-size="10" text-anchor="middle" fill="currentColor">changed (one per owner)</text>
  <line x1="185" y1="168" x2="330" y2="168" stroke="currentColor" marker-end="url(#t20-arrow)"/>
  <text x="258" y="162" font-size="10" text-anchor="middle" fill="currentColor">activate</text>
  <text x="330" y="204" font-size="10" text-anchor="middle" fill="currentColor">read team-rules.json</text>
  <line x1="330" y1="234" x2="435" y2="234" stroke="currentColor" marker-end="url(#t20-arrow)"/>
  <text x="383" y="228" font-size="10" text-anchor="middle" fill="currentColor">register</text>
  <line x1="435" y1="272" x2="330" y2="272" stroke="currentColor" stroke-dasharray="5 3" marker-end="url(#t20-arrow)"/>
  <text x="383" y="266" font-size="10" text-anchor="middle" fill="currentColor">ok or error</text>
</svg>
<figcaption>The six static entries are active as soon as their owners exist. The dashed arrows are replies or kernel topics, not calls from your code.</figcaption>
</figure>

### Step 1: the manifest

The manifest is the larger part of the package. It has seven grants, six extensions and one export. It has a `server` side for the run-time part only. The extensions do not belong to a side: a collect entry is data.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.team-pack",
  "name": "Team pack",
  "version": "0.1.0",
  "publisher": "Example Developer",
  "description": "Adds the team's patterns, prices, palette, hints and agent notes.",
  "icon": "package",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": []
  },
  "capabilities": {
    "server": [
      "extensions.contribute:ilium.core.smart-copy/recognisers",
      "extensions.contribute:ilium.core.text-triggers/rule-packs",
      "extensions.contribute:ilium.core.cost/price-sources",
      "extensions.contribute:ilium.core.ambient/palettes",
      "extensions.contribute:ilium.core.keys-and-help/key-hints",
      "extensions.contribute:ilium.core.agent-setup/instruction-blocks",
      "fs.read:project"
    ]
  },
  "activation": ["startup"],
  "contributes": {
    "commands": [
      { "id": "reload-rules", "title": "Team pack: reload project rules" }
    ],
    "extensions": [
      { "point": "ilium.core.smart-copy/recognisers@0.1.0", "id": "ticket",
        "entry": { "id": "ticket", "kind": "ticket", "regex": "TEAM-[0-9]{1,6}",
                   "multiline": false, "label": "Ticket key" } },
      { "point": "ilium.core.text-triggers/rule-packs@0.1.0", "id": "team-basics",
        "entry": { "id": "team-basics", "title": "Team basics",
                   "rules": [
                     { "pattern": "Run the tests\\? \\(y/n\\)", "reply": "y\n", "enabled": false }
                   ] } },
      { "point": "ilium.core.cost/price-sources@0.1.0", "id": "internal-model",
        "entry": { "id": "internal-model", "model_pattern": "^team-llm-.*",
                   "input_per_mtok": 1.5, "output_per_mtok": 6.0,
                   "cache_read_per_mtok": 0.15, "currency": "EUR" } },
      { "point": "ilium.core.ambient/palettes@0.1.0", "id": "harbour",
        "entry": { "id": "harbour", "title": "Harbour",
                   "stops": ["#0b1d2a", "#1f6f8b", "#99d5c9", "#f6f1d1"] } },
      { "point": "ilium.core.keys-and-help/key-hints@0.1.0", "id": "board-new",
        "entry": { "id": "board-new", "context": "pane:board", "chord": "n",
                   "label": "New card" } },
      { "point": "ilium.core.agent-setup/instruction-blocks@0.1.0", "id": "team-style",
        "entry": { "id": "team-style", "title": "Team style notes",
                   "marker": "team-style", "file": "help/team-style.md" } }
    ]
  }
}
```

Points to notice:

- Each extension needs its own grant, and each grant names one point. There is no "contribute to any" grant, and a wildcard fails with `manifest.grant-wildcard`. A missing grant fails with `manifest.grant-missing-extension`.
- `point` carries `@0.1.0`. The version is a caret range: the owner must offer `>=0.1.0 <0.2.0`. Otherwise the entry stays dormant.
- `priority` is left out. The default is 50. Collect entries are listed in priority order, so priority matters only when two entries compete, such as two recognisers that match the same text.
- Backslashes in a JSON string are doubled. The Text Triggers pattern `Run the tests\\? \\(y/n\\)` reaches the owner as `Run the tests\? \(y/n\)`.
- The trigger rule is `enabled: false`. A rule pack is a set the user turns on. It never starts sending text by itself.
- The price source is data. It makes Cost able to price a model the built-in table does not know. It does not change the prices of known models.
- `help/team-style.md` must exist in the package. A missing file fails with `manifest.extension-entry-invalid`, because the entry names a file the package does not hold.
- `reload-rules` is a command of the plugin, as in tutorial 3. It reads the file again without a restart.
- The plugin has no `hook-handler` export. It contributes no hook.

### Step 2: what the schema checks

The kernel checks each entry against the owner's `entry_schema` when the package is installed. It does not run any of your code for this. If one entry fails, the whole install fails, and no extension of the package is added.

Break the price source on purpose. Change `"input_per_mtok": 1.5` to `"input_per_mtok": "1.5"` and run `ilium plugin build`. The error names the entry and the field:

<figure>
<pre class="term">
<span class="t-prompt">$</span> ilium plugin build
<span class="t-err">error manifest.extension-entry-invalid</span>
  contributes.extensions[2].entry.input_per_mtok
  expected number, found string
  point ilium.core.cost/price-sources
<span class="t-dim">1 error, 0 warnings</span>
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The path counts extensions from 0, so index 2 is the third entry, the price source.</figcaption>
</figure>

Restore the number. The same code covers a missing required field, a string over its `maxLength`, a list over `maxItems`, an entry over 16 KiB and a field that is not in `properties` when the schema forbids extra fields. A palette with only one stop fails if the owner's schema requires at least two.

### Step 3: the run-time part

Teams keep different rules per repository. Build the rule pack from the file `.ilium/team-rules.json` of the open project, and register it with `register`. The file looks like this:

```json
{
  "packs": [
    { "id": "release", "title": "Release checks",
      "rules": [ { "pattern": "Publish\\? \\(y/n\\)", "reply": "n\n", "enabled": false } ] }
  ]
}
```

The code has three parts. A pure function turns the file text into entries. A function registers the entries through a callback, so a test can replace the host call. The handler runs both on start and on the command.

Two facts matter. First, `register` works only on a point that is `dynamic`. If the Text Triggers point is static, `register` fails with `extension.not-dynamic` (the WIT case `not-dynamic`, `NotDynamic` in Rust). The plugin does not crash. It logs one line and keeps the static entries. Second, `register` validates the entry again, so a bad rule in the file gives `schema-mismatch` for that pack only, and the other packs still register.

**Rust**

```rust
// src/lib.rs
use ilium_plugin_sdk::host::extensions;
use ilium_plugin_sdk::host::fs::{self, FsPath};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::task;
use serde_json::Value;

const RULE_PACKS: &str = "ilium.core.text-triggers/rule-packs";
const RULES_FILE: &str = ".ilium/team-rules.json";
const MAX_PACKS: usize = 16;

/// Turns the text of the rules file into (extension id, entry JSON) pairs.
/// A pack that is not an object with an id is skipped, and the pack count is capped.
pub fn packs_from_file(text: &str) -> Vec<(String, String)> {
    let Ok(file) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    let Some(packs) = file["packs"].as_array() else { return Vec::new() };
    packs
        .iter()
        .take(MAX_PACKS)
        .filter_map(|pack| {
            let id = pack["id"].as_str()?;
            // The file is untrusted. Force every rule to disabled.
            let mut entry = pack.clone();
            for rule in entry["rules"].as_array_mut()? {
                rule["enabled"] = Value::Bool(false);
            }
            Some((format!("project-{id}"), entry.to_string()))
        })
        .collect()
}

/// Registers every pack. Returns how many were accepted.
pub fn register_packs(
    packs: &[(String, String)],
    register: &mut dyn FnMut(&str, &str) -> Result<(), String>,
) -> usize {
    let mut accepted = 0;
    for (extension, entry_json) in packs {
        match register(extension, entry_json) {
            Ok(()) => accepted += 1,
            Err(reason) if reason.contains("NotDynamic") || reason.contains("not-dynamic") => {
                log::log(Level::Warn, "rule-packs is not dynamic; project rules skipped");
                break;
            }
            Err(reason) => log::log(Level::Warn, &format!("{extension} refused: {reason}")),
        }
    }
    accepted
}

async fn load_project_rules() {
    let path = FsPath { scope: "project".into(), path: RULES_FILE.into() };
    let bytes = match fs::read_all(&path).await {
        Ok(bytes) => bytes,
        Err(code) => {
            // A project without the file is normal.
            log::log(Level::Debug, &format!("no team rules: {code}"));
            return;
        }
    };
    let packs = packs_from_file(&String::from_utf8_lossy(&bytes));
    let mut register = |extension: &str, entry_json: &str| {
        extensions::register(RULE_PACKS, extension, 50, entry_json).map_err(|error| format!("{error:?}"))
    };
    let accepted = register_packs(&packs, &mut register);
    log::log(Level::Info, &format!("{accepted} of {} project packs registered", packs.len()));
}

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct TeamPack;

impl Lifecycle for TeamPack {
    fn activate(&mut self) {
        task::spawn(load_project_rules());
    }
}

impl Handler for TeamPack {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if let Event::CommandInvoked(invoked) = event {
                if invoked.command == "example.team-pack/reload-rules" {
                    task::spawn(load_project_rules());
                }
            }
        }
        Vec::new()
    }
}
```

**TypeScript**

```ts
// src/server.ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { register } from '@ilium/plugin-sdk/host/extensions';
import { readAll } from '@ilium/plugin-sdk/host/fs';
import { log } from '@ilium/plugin-sdk/host/log';

const RULE_PACKS = 'ilium.core.text-triggers/rule-packs';
const RULES_FILE = '.ilium/team-rules.json';
const MAX_PACKS = 16;

type Pack = { extension: string; entryJson: string };

/** Turns the text of the rules file into extension ids and entry JSON. */
export function packsFromFile(text: string): Pack[] {
  let file: { packs?: Array<{ id?: unknown; rules?: Array<Record<string, unknown>> }> };
  try {
    file = JSON.parse(text);
  } catch {
    return [];
  }
  if (!Array.isArray(file.packs)) return [];
  return file.packs.slice(0, MAX_PACKS).flatMap((pack) => {
    if (typeof pack.id !== 'string' || !Array.isArray(pack.rules)) return [];
    // The file is untrusted. Force every rule to disabled.
    const rules = pack.rules.map((rule) => ({ ...rule, enabled: false }));
    return [{ extension: `project-${pack.id}`, entryJson: JSON.stringify({ ...pack, rules }) }];
  });
}

/** Registers every pack. Returns how many were accepted. */
export function registerPacks(packs: Pack[], put: (extension: string, entryJson: string) => void): number {
  let accepted = 0;
  for (const pack of packs) {
    try {
      put(pack.extension, pack.entryJson);
      accepted += 1;
    } catch (error) {
      const code = String((error as { code?: string }).code ?? error);
      if (code.includes('not-dynamic')) {
        log('warn', 'rule-packs is not dynamic; project rules skipped');
        break;
      }
      log('warn', `${pack.extension} refused: ${code}`);
    }
  }
  return accepted;
}

async function loadProjectRules(): Promise<void> {
  let bytes: Uint8Array;
  try {
    bytes = await readAll({ scope: 'project', path: RULES_FILE });
  } catch (error) {
    // A project without the file is normal.
    log('debug', `no team rules: ${String(error)}`);
    return;
  }
  const packs = packsFromFile(new TextDecoder().decode(bytes));
  const accepted = registerPacks(packs, (extension, entryJson) => register(RULE_PACKS, extension, 50, entryJson));
  log('info', `${accepted} of ${packs.length} project packs registered`);
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate() {
    void loadProjectRules();
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'command-invoked' && event.val.command === 'example.team-pack/reload-rules') {
        void loadProjectRules();
      }
    }
    return [];
  },
});
```

Points to notice:

- `fs.read:project` reads one file. The plugin has no write grant, so it cannot change the repository.
- The file belongs to the repository, so anyone who can commit to it can write rules. That is why `enabled` is forced to `false`. A cloned repository must not make an agent answer prompts by itself. The user enables a pack on the Text Triggers screen.
- The pack ids start with `project-`, so they never clash with the static `team-basics`. Registering an id again replaces that entry, so the command can be run any number of times.
- `register` is bounded. The point has `max_entries` and a pack has a size cap of 16 KiB. The plugin caps itself at 16 packs and stays inside both.
- `load_project_rules` is async because `read_all` is async. The activation returns at once, and the work finishes later. A hook does not wait for it.
- Entries registered at run time are not kept when the plugin restarts. `activate` registers them again.
- To drop a pack that has left the file, keep the ids of the previous load and call `unregister` for each id that is gone. A plugin can remove only entries it registered.

### Step 4: dormant, updated and disabled contributions

The package works only when the owners are present.

| Owner state | Your entry | What the Plugins screen shows |
|---|---|---|
| Installed and enabled | Active | The entry with its toggle on |
| Not installed | Dormant | `waiting for ilium.core.cost` |
| Disabled by the user | Dormant, toggle kept | `waiting for ilium.core.cost` |
| Installed, point version too old | Dormant | The version the owner must reach |
| Installed, point removed | Dormant | `waiting for ilium.core.cost` |

Install never fails because an owner is missing. All six owners are bundled core plugins, so in practice they are present.

When you ship the next version of the package, the kernel checks every entry again. Suppose the Cost owner releases a new major version of its point with a changed schema. Your entry, which asks for `@0.1.0`, no longer matches, and that entry goes dormant. The other five stay active, and the user sees a line for the entry. Raise your entry to the new version and change its fields.

When the package is updated, the owners hear `ilium.extensions/changed` once for each point whose entries changed. A point whose entries did not change hears nothing.

### Step 5: pack, review and install

Build and pack as in [tutorial 9](/pages/tutorial-09-package-and-share/): `ilium plugin build`, `ilium plugin pack`, `ilium plugin keygen --publisher "Example Developer" --out <dir>`, `ilium plugin sign <file> --key <publisher.key>` and `ilium plugin trust add <publisher.pub>` once. Then `ilium plugin install dist/example.team-pack-0.1.0.iliumpkg`. An unsigned package is T3 and needs `--allow-unsigned`.

#### What you see

The review dialog lists the six contributions under **Adds to other plugins**, by owner. All six grants are normal risk, so all are checked. The file grant is listed with them.

<figure>
<pre class="term">
╭──────────────────────────────────────────────╮
│ Review: example.team-pack 0.1.0              │
│                                              │
│ Adds to other plugins                        │
│   Smart Copy > Recognisers                   │
│     adds a text pattern                      │
│   Text triggers > Rule packs                 │
│     adds a set of rules                      │
│   Cost > Price sources                       │
│     adds prices for a model                  │
│   Ambient > Palettes                         │
│     adds a colour palette                    │
│   Keys and help > Key hints                  │
│     adds a hint for a key                    │
│   Agent setup > Instruction blocks           │
│     adds a block to agent files              │
│                                              │
│ Reads                                        │
│   [x] fs.read: project files                 │
│                                              │
│   [allow]  [decline]                         │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Seven grants are checked. None is high risk.</figcaption>
</figure>

After install, the Extensions tab of the plugin lists what it adds, with a toggle for each. A user who does not want the key hint turns that one off. The others stay on.

<figure>
<pre class="term">
╭──────────────────────────────────────────────╮
│ Team pack > Extensions                       │
│                                              │
│ Smart Copy > Recognisers                     │
│   [x] ticket                                 │
│ Text triggers > Rule packs                   │
│   [x] team-basics                            │
│   [x] project-release                        │
│ Cost > Price sources                         │
│   [x] internal-model                         │
│ Ambient > Palettes                           │
│   [x] harbour                                │
│ Keys and help > Key hints                    │
│   [ ] board-new                              │
│ Agent setup > Instruction blocks             │
│   [x] team-style                             │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The row <code>project-release</code> was registered at run time, and it is listed beside the static entries.</figcaption>
</figure>

The log of the plugin shows what the run-time part did. Run `ilium plugin logs example.team-pack --tail 5`:

<figure>
<pre class="term">
<span class="t-dim">2026-10-12 09:30:11 info example.team-pack</span>
<span class="t-ok">1 of 1 project packs registered</span>
<span class="t-dim">2026-10-12 09:41:52 warn example.team-pack</span>
<span class="t-warn">project-broken refused: SchemaMismatch</span>
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The second line shows one bad pack in the file. The others still registered.</figcaption>
</figure>

### Step 6: test it

The fake host described in [Testing](/pages/testing/) has no registry, so it cannot run the owners. Test what you wrote. That is the manifest, the pure function that reads the file and the function that registers the packs.

1. **Manifest.** Run `ilium plugin build` with each entry broken in turn: a number as a string, a missing `file`, and a removed grant. Expect `manifest.extension-entry-invalid` and `manifest.grant-missing-extension`.
2. **File parser.** Feed `packs_from_file` or `packsFromFile` a good file, a file that is not JSON, a file without `packs`, a pack without an id and a pack with 20 entries.
3. **Registration.** Give `register_packs` or `registerPacks` a callback that fails with `NotDynamic`, one that fails with `SchemaMismatch` for the first pack only, and one that always succeeds.

**Rust**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{"packs":[{"id":"release","title":"Release","rules":[{"pattern":"p","reply":"n\n","enabled":true}]}]}"#;

    #[test]
    fn every_rule_is_forced_to_disabled() {
        let packs = packs_from_file(GOOD);
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].0, "project-release");
        assert!(packs[0].1.contains(r#""enabled":false"#));
    }

    #[test]
    fn bad_files_give_no_packs() {
        assert!(packs_from_file("not json").is_empty());
        assert!(packs_from_file("{}").is_empty());
        assert!(packs_from_file(r#"{"packs":[{"title":"no id","rules":[]}]}"#).is_empty());
    }

    #[test]
    fn pack_count_is_capped() {
        let many = (0..20).map(|n| format!(r#"{{"id":"p{n}","rules":[]}}"#)).collect::<Vec<_>>().join(",");
        assert_eq!(packs_from_file(&format!(r#"{{"packs":[{many}]}}"#)).len(), MAX_PACKS);
    }

    #[test]
    fn a_static_point_stops_the_loop() {
        let packs = packs_from_file(GOOD);
        let mut calls = 0;
        let mut register = |_: &str, _: &str| {
            calls += 1;
            Err("NotDynamic".to_string())
        };
        assert_eq!(register_packs(&packs, &mut register), 0);
        assert_eq!(calls, 1);
    }

    #[test]
    fn one_refused_pack_does_not_stop_the_others() {
        let two = r#"{"packs":[{"id":"a","rules":[]},{"id":"b","rules":[]}]}"#;
        let mut register = |extension: &str, _: &str| {
            if extension == "project-a" { Err("SchemaMismatch".to_string()) } else { Ok(()) }
        };
        assert_eq!(register_packs(&packs_from_file(two), &mut register), 1);
    }
}
```

**TypeScript**

```ts
// src/index.test.ts
import { expect, test } from 'bun:test';
import { packsFromFile, registerPacks } from './index';

const GOOD = '{"packs":[{"id":"release","title":"Release","rules":[{"pattern":"p","reply":"n\\n","enabled":true}]}]}';

test('every rule is forced to disabled', () => {
  const packs = packsFromFile(GOOD);
  expect(packs.length).toBe(1);
  expect(packs[0].extension).toBe('project-release');
  expect(packs[0].entryJson).toContain('"enabled":false');
});

test('bad files give no packs', () => {
  expect(packsFromFile('not json')).toEqual([]);
  expect(packsFromFile('{}')).toEqual([]);
  expect(packsFromFile('{"packs":[{"title":"no id","rules":[]}]}')).toEqual([]);
});

test('pack count is capped', () => {
  const many = Array.from({ length: 20 }, (_, n) => ({ id: `p${n}`, rules: [] }));
  expect(packsFromFile(JSON.stringify({ packs: many })).length).toBe(16);
});

test('a static point stops the loop', () => {
  let calls = 0;
  const accepted = registerPacks(packsFromFile(GOOD), () => {
    calls += 1;
    throw Object.assign(new Error('x'), { code: 'extension.not-dynamic' });
  });
  expect(accepted).toBe(0);
  expect(calls).toBe(1);
});

test('one refused pack does not stop the others', () => {
  const two = JSON.stringify({ packs: [{ id: 'a', rules: [] }, { id: 'b', rules: [] }] });
  const accepted = registerPacks(packsFromFile(two), (extension) => {
    if (extension === 'project-a') throw Object.assign(new Error('x'), { code: 'extension.schema-mismatch' });
  });
  expect(accepted).toBe(1);
});
```

Run `cargo test` or `bun test`. Then check it in the app. Install the package, open Settings > Text triggers and look for `Team basics`. Put the sample file in a project, run `Team pack: reload project rules` and look for `Release checks` in the same list, switched off. Copy a line with `TEAM-123` and check that Smart Copy offers it as a ticket. Open Settings > Plugins, disable Cost, and check that `internal-model` says `waiting for ilium.core.cost`.

### Security and consent

| Grant | Risk | Why |
|---|---|---|
| `extensions.contribute:` six points | normal | Each point is data. None is a hook. |
| `fs.read:project` | normal | The plugin reads one file, `.ilium/team-rules.json`. It cannot write. |

- A rule pack can make Text Triggers send text to a terminal once the user enables it. The package never enables a rule itself, and it forces every project rule to `enabled: false`.
- The instruction block is text that Agent setup writes into agent files between markers. The user runs Agent setup, and the block is shown before it is written. Keep the file free of secrets and of commands you do not want an agent to follow.
- A price source changes a number that the user reads. A wrong price misleads. Keep prices in the package, signed and reviewed.
- A recogniser can slow Smart Copy if its regex is expensive. Keep patterns anchored and short.

### What can go wrong

| Code or symptom | Cause | Fix |
|---|---|---|
| `manifest.extension-entry-invalid` | An entry fails the owner's `entry_schema`, or names a file that the package does not hold. | Match the schema. Include the file. |
| `manifest.grant-missing-extension` | An extension has no `extensions.contribute` grant for its point. | Add the exact grant. |
| `manifest.grant-wildcard` | A grant ends in `*`. | Name one point. |
| `manifest.extension-limit` | More than 64 extensions in one plugin. | Merge entries, or split the package. |
| Entry shows `waiting for <owner>` | The owner is disabled, or the point version is too new. | Enable the owner, or lower `@version`. |
| `extension.not-dynamic` | `register` on a point that is static. | Put the entry in the manifest. |
| `extension.schema-mismatch` | A run-time entry fails the schema. | Fix the pack in the file. |
| `extension.cap-exceeded` | The point holds `max_entries`, or the entry is over 16 KiB. | Register fewer or smaller packs. |
| `not-permitted` on the file | No `fs.read:project`, or the path leaves the project. | Add the grant. Use a path inside the project. |
| `no team rules: not-found` | The project has no rules file. | Nothing. This is normal. |
| Project pack is off | The plugin forces `enabled: false`. | The user enables it. |

### See also

- [Extension points](/pages/api-extension-points/) for the mechanism, `register` and the error codes.
- [Extending core plugins](/pages/extending-core-plugins/) for the ways to extend a core feature.
- [Reference: core extension points](/pages/reference-core-extension-points/) for every point and its entry fields.
- [Core plugin: smart copy](/pages/core-plugin-smart-copy/), [text triggers](/pages/core-plugin-text-triggers/), [cost](/pages/core-plugin-cost/), [ambient](/pages/core-plugin-ambient/), [keys and help](/pages/core-plugin-keys-and-help/) and [agent setup](/pages/core-plugin-agent-setup/).
- [Tutorial 18](/pages/tutorial-18-add-an-extension-point/) and [tutorial 19](/pages/tutorial-19-stop-core-actions-with-hooks/).
- [Tutorial 9: package and share](/pages/tutorial-09-package-and-share/) and [Manifest reference](/pages/reference-manifest/).

API checked against `ilium:plugin@0.1.0`.

### Security

Rule files under `.ilium/` must be ignored in untrusted projects and validated ([SEC-315]). See [Extension security](/pages/reference-extension-security/).

**Next:** [Tutorial 21, replace a core plugin](/pages/tutorial-21-replace-a-core-plugin/).
