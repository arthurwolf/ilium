---
layout: page
title: Packages and manifests
permalink: /pages/packages/
---

## Packages and manifests

A plugin ships as one `.iliumpkg` file. It is a ZIP archive with a fixed layout and a `manifest.json` at its root. The manifest names the plugin, lists its components, states the grants it needs and declares what it adds to Ilium. Ilium reads the manifest and the contributions before it runs any plugin code, so menus, help and settings work before the plugin starts.

This page covers the package layout, every manifest key, the contribution shapes, the `ilium plugin` commands, the install checks and their error codes.

## What you need

- A plugin id in reverse-DNS form, such as `example.notes`. The prefix `ilium.` is reserved for first-party plugins.
- The worlds and sides, from [Worlds and sides](/pages/worlds-and-sides/), and the grant strings, from [Capabilities and grants](/pages/capabilities/).
- The `ilium` binary, which includes the `ilium plugin` commands. Rust plugins need a Rust toolchain. TypeScript plugins need Bun 1.2 or later. See [Getting started toolchain](/pages/getting-started-toolchain/).

## Package layout

A `.iliumpkg` file is a deterministic ZIP32 archive. Entries are stored without compression, sorted by path, with fixed timestamps, so two builds of the same sources are byte-identical.

| Path | Required | Content |
|---|---|---|
| `manifest.json` | yes | The manifest. |
| `server.wasm`, `client.wasm`, `scene.wasm` | if the side is declared | Components for the `server`, `client` or `scene` side. |
| `assets/**` | no | Read-only files, visible through the `package` file scope. |
| `help/*.md` | no | Help topics referenced by `contributes.help`. |
| `sounds/*` | no | Sound files referenced by `contributes.sounds`. |
| `src/**` | no | Sources and source maps, for review only. Never loaded. |
| `signature` | for tier T2 | Detached Ed25519 signature over the canonical bytes of `manifest.json`. |

Every entry except `manifest.json` and `signature` must appear in the `files` list with its length in bytes and its lowercase SHA-256. Each component also carries an `ilium:api-version` custom section (three big-endian `u16` values: major, minor, patch) that must equal the manifest `api`.

The install checks run in one fixed order of 11 stages, and each failure has an error code. The stages are defined once, in [Package format](/pages/reference-package-format/#the-validation-order), and the codes in [Manifest schema and error catalogue](/pages/reference-manifest-schema/#error-catalogue). [Security](/pages/security/#verifying-a-package) and [Grant review and consent](/pages/reference-grant-review-and-consent/#the-install-pipeline) describe the same stages from the user's side. A failed check stops the install, no component is loaded, and no plugin code runs.

## Manifest keys

The manifest is JSON with snake_case keys. Unknown keys fail the install with `manifest.unknown-key`. A source manifest may omit `files`, because `ilium plugin pack` writes it. A packed manifest must contain it.

| Key | Type | Required | Meaning |
|---|---|---|---|
| `api` | string | yes | Contract version, `ilium:plugin@0.1.0`. |
| `id` | string | yes | Matches `[a-z0-9-]+(\.[a-z0-9-]+)+`. |
| `name`, `version` | string | yes | Display name and semantic version. |
| `publisher` | string | yes | Must equal the publisher recorded for the signing key in a T2 package. |
| `description`, `license`, `icon` | string | no | One paragraph, an SPDX expression, and a Tabler icon name. |
| `server`, `client`, `scene` | object | at least one | `{ entry, world, exports }`. `exports` lists optional exports only. |
| `capabilities` | object | no | Grant strings per side: `server`, `client` or `scene`. |
| `activation` | list of strings | no | Triggers such as `startup` or `topic:<topic>`. Default: first use of a contribution. |
| `requires` | object | no | `{ "services": ["ilium:inference/provider@0.1.0"] }`. Activation waits for a provider. |
| `replaces` | list of strings | no | At most 4 ids of replaceable core plugins. The user confirms the replacement. |
| `replaceable`, `surface` | boolean, list | core plugins | `replaceable: true` plus the `kind:name` items a replacement must provide. |
| `contributes` | object | no | Static contributions, below. |
| `limits` | object | no | Requested limits. The host clamps each value to the tier ceiling. |
| `limits_tier` | string | no | `"standard"` (default) or `"first-party"`. Raises the settings caps. Accepted only for bundled T1 plugins whose id starts with `ilium.`, otherwise `manifest.limits-tier`. |
| `unstable` | list of strings | no | Opt-in names. Four exist in 0.1.0: `ui-calendar`, `plugin-services`, `gpu-kernels` and `websocket`. |
| `files` | list | packed only | `{ "path", "length", "sha256" }` for each entry. |

The `exports` list names only the optional exports: `detector`, `views`, `annotator`, `job-runner`, `service-provider`, `inference-provider`, `settings-validate`, `catalog-provider`, `menu-provider`, `virtual-rows-provider`, `token-provider`, `hook-handler` and `migrator`, each only on the sides that accept it (see [Worlds and sides](/pages/worlds-and-sides/)). Required exports (`lifecycle`, `handler`, `renderer`) are never listed. A component whose real exports differ from required plus listed fails with `manifest.exports-mismatch`.

## Contributions

`contributes` declares what Ilium shows without running plugin code. Ids are local. The host builds global ids as `<plugin-id>/<id>` for commands, settings pages and help topics, and `<plugin-id>.<id>` for pane kinds.

| Key | Entry shape |
|---|---|
| `commands` | `{ id, title, arguments_schema?, voice?, palette?, icon?, when? }`. `palette` defaults to `true`. `voice` is 1 to 8 phrases. |
| `keybindings` | `{ command, chord, mode }`. Mode is `prefix`, `global` or `pane:<kind>`. |
| `settings` | `{ kind, ... }`. `kind` is `tab` (with `"render": "view"` for a custom page) or `section` (with `tab`, `anchor?`, `scope` of `server`, `client` or `project`, and rows). See [Plugin settings extension](/pages/settings-extension/). |
| `help` | `{ id, file, title }`. The file is Markdown under `help/`. |
| `decoration_slots` | `{ id, placement, priority }`. Placement is `badge`, `icon`, `subtitle` or `footer`. |
| `virtual_rows` | `{ id, title, icon, parent }`. Parent is `project` or `session`. |
| `toolbar_buttons` | `{ id, icon, label, command, applies_to }`. |
| `context_menu` | `{ id, title, command, target }`. Target is `terminal`, `tree-node`, `editor` or `pane:<kind>`. |
| `panels` | `{ id, title, icon?, default_size?, modal }`. |
| `pane_kinds` | `{ id, title, icon, primitive, wants_keys, persist }`. Primitive is `basic`. Persist is `metadata` or `none`. |
| `scenes` | `{ id, title, family, modes, settings_schema, frame_rate_cap? }`. Needs a `scene` side. |
| `signatures` | `{ id, process_names, aliases, icon, resume?, actions?, session_scope? }`. |
| `output_filters` | `{ id, regex, panes, rate_per_second }`. Panes are `all`, `agents` or `terminals`. |
| `link_detectors` | `{ id, regex, command }`. |
| `editor_line_actions` | `{ id, title, regex?, command }`. |
| `sounds` | `{ id, file }`. |
| `cli` | `{ name, summary, arguments_schema, output }`, or a list of at most 16 such objects. `output` is `jsonl`, `text` or `mixed`. |
| `http_routes` | `{ method, path }`. Needs the matching `http-api.route:<path>` grant. |
| `services` | `{ provides }`. Interface names. |
| `topics` | `{ publishes, subscribes }`. Publishing stays under the plugin's id prefix. |
| `jobs` | `{ kind, deadline_ms?, memory_mib? }`. |
| `onboarding_steps` | `{ id, title, view, order }`. |
| `extension_points` | `{ id, kind, mode?, title, entry_schema?, payload_schema?, reply_schema?, risk?, deadline_ms?, max_handlers?, max_entries?, dynamic?, version }`. At most 16. |
| `extensions` | `{ point, id, priority?, entry?, handler? }`. At most 64. Needs `extensions.contribute`. |

The `themes` key is not part of the contract. Plugins ship no colour themes.

## Worked example: a Rust plugin

This plugin counts words in the current note. Its source manifest has no `files` list, and it needs no grants, because the always-granted set covers it.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.word-count",
  "name": "Word count",
  "version": "0.1.0",
  "publisher": "Example Labs",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": []
  },
  "activation": ["startup"],
  "contributes": {
    "commands": [
      { "id": "count", "title": "Count words in note", "icon": "calculator" }
    ]
  }
}
```

`ilium plugin new ./word-count --id example.word-count --lang rust --side server` creates `manifest.json`, `Cargo.toml` and `src/lib.rs`. Then build and pack:

<pre class="term"><span class="t-prompt">$</span> ilium plugin build ./word-count
<span class="t-ok">ok</span>   server.wasm           component, 412 KiB
<span class="t-prompt">$</span> ilium plugin pack ./word-count
<span class="t-dim">step</span>   inventory            hashing 1 file
<span class="t-ok">ok</span>   artifact             dist/example.word-count-0.1.0.iliumpkg
<span class="t-hi">done</span> result               unsigned, tier T3
</pre>

Rendered at 48 columns. Illustrative. The commands print JSON lines with `--json`. This render shows the same fields as plain text, with the hash omitted.

## Worked example: a TypeScript plugin with a notes pane

This manifest serves a notes plugin with a server and a client component. The same manifest works for Rust or TypeScript. `ilium plugin build` detects the toolchain from the project files: `Cargo.toml` means Rust, and `package.json` means TypeScript. A TypeScript project holds `manifest.json`, `package.json`, `src/server.ts` and `src/client.ts`.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.notes",
  "name": "Notes",
  "version": "0.1.0",
  "publisher": "Example Labs",
  "description": "Keeps a notes pane for each project.",
  "license": "MIT",
  "icon": "notes",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": []
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": ["views"]
  },
  "capabilities": {
    "server": ["tree.read"],
    "client": ["ui.panels", "panes.read", "panes.provide-kind"]
  },
  "contributes": {
    "commands": [
      { "id": "new-note", "title": "New note", "icon": "plus" }
    ],
    "pane_kinds": [
      {
        "id": "notes",
        "title": "Notes",
        "icon": "notes",
        "primitive": "basic",
        "wants_keys": true,
        "persist": "metadata"
      }
    ],
    "help": [
      { "id": "overview", "file": "help/overview.md", "title": "Notes" }
    ]
  },
  "files": [
    { "path": "server.wasm", "length": 412390, "sha256": "<64 hex characters>" },
    { "path": "client.wasm", "length": 905112, "sha256": "<64 hex characters>" },
    { "path": "help/overview.md", "length": 1204, "sha256": "<64 hex characters>" }
  ]
}
```

The `pane_kinds` entry has the global id `example.notes.notes`. The `help` entry creates the topic `example.notes/overview`. The hashes and lengths above are placeholders that the packer replaces.

<div class="warn">
<p>A package that holds a file not listed in <code>files</code> is refused. So is a listed file whose length or hash differs from the file on disk. Run <code>ilium plugin pack</code> after every rebuild.</p>
</div>

<pre class="term"><span class="t-prompt">$</span> ilium plugin install dist/example.notes-0.1.0.iliumpkg
<span class="t-err">err</span>  manifest.file-length  client.wasm: 905120 bytes, inventory says 905112
<span class="t-err">err</span>  manifest.file-unlisted  src/notes.ts is not in the files list
<span class="t-warn">warn</span> no plugin code was loaded or run
</pre>

Illustrative, not generated. An install that fails the inventory check. Nothing from the package runs.

## Commands

The build commands are defined in [Plugin command-line reference](/pages/reference-plugin-cli/). The commands that install, update, search, publish and sign are in [Distribution commands](/pages/reference-plugin-distribution-cli/), with one exit-code table for both. Commands that build, check, pack and sign print JSON lines when you pass `--json`. The commands that read a running plugin (`logs`, `list`, `trace`) print text by default. Registry commands such as `verify`, `publish` and `update` are in [Distribution commands](/pages/reference-plugin-distribution-cli/). The ones you use for packages are `new`, `build`, `pack`, `check`, `lint`, `test`, `inspect --verify`, `keygen`, `sign`, `trust`, `install`, `dev` and `grants`.

## Install errors

The install runs the checks in the diagram order. Each failure prints one `error` line and stops the install. The grant checks use the `manifest.grant-*` family from [Capabilities and grants](/pages/capabilities/).

| Code | When it fires |
|---|---|
| `manifest.unknown-key` | A manifest key is not in the schema. |
| `manifest.key-missing` | A packed manifest has no `files` key. |
| `manifest.id-invalid` | The id does not match the pattern. |
| `manifest.id-reserved` | A third-party package uses an `ilium.` id. |
| `manifest.file-unlisted` | A package entry is missing from `files`. |
| `manifest.file-missing` | A listed file is absent from the package. |
| `manifest.file-length`, `manifest.file-hash` | A listed length or SHA-256 differs from the file. |
| `manifest.api-version` | A component's `ilium:api-version` differs from `api`. |
| `manifest.signature-invalid` | A T2 signature does not verify. |
| `manifest.publisher-mismatch` | `publisher` differs from the key's recorded publisher. |
| `manifest.exports-mismatch` | The component exports differ from the manifest list. |
| `manifest.export-grant` | An export needs a grant the manifest does not request. |
| `manifest.grant-wildcard` | A grant argument is `*` where no wildcard is allowed. |
| `manifest.grant-unknown`, `manifest.grant-tier`, `manifest.grant-dependency` | A grant is unknown, not available to the plugin's tier, or missing a required companion grant. |
| `manifest.schema-keyword` | A settings schema uses a keyword outside the allowed subset. |
| `manifest.settings-key-clash`, `manifest.settings-id-clash` | A settings key or tab or section id is declared twice. |
| `manifest.cli-name-reserved` | The CLI name is a kernel command. |
| `manifest.cli-name-taken` | The CLI name belongs to another installed plugin. |
| `manifest.sound-id-taken` | A sound id repeats, or equals the bundled id `chirping`. |
| `manifest.catalog-unknown` | A row `source` names an unknown catalogue, or a `catalogs` entry has no `catalog-provider` export. |
| `manifest.contribution-limit` | More entries of a contribution key than its cap. |
| `manifest.contribution-target` | `applies_to` or `slot` names an unknown target or another plugin's slot. |
| `manifest.row-format` | An unknown `item_format`, a bad `steps` ladder or `special_values` out of bounds. |
| `manifest.limits-tier` | `limits_tier: "first-party"` on a package that is not a bundled T1 `ilium.*` plugin. |
| `manifest.when-key` | An unknown `when` context key. |
| `manifest.client-request-unknown` | A client-request export is inconsistent with the other side's manifest. |
| `manifest.asset-size` | Package assets over the `package_assets_mib` limit: 64 MiB, or up to 256 MiB for a T1 plugin that raises it (T2 and T3: 64 MiB, see [Performance](/pages/performance/)). Also a font asset over 4 MiB. |
| `manifest.visible-when` | `visible_when` names another plugin's key, an unknown key, or forms a cycle with `depends_on`. |
| `manifest.menu-depth` | Menu items nested deeper than 2. |
| `manifest.settings-source` | A `choice` row `source` with both `options` and `source`, or neither. |
| `manifest.cli-field` | An invalid `output`, `stdin` or `argument_groups` value on a `cli` entry. |
| `manifest.scene-prerender` | `prerender` out of range. |

This table lists the common codes. The complete list of `manifest.` and `package.` codes, with the page that defines each, is the install code index in [Errors and limits reference](/pages/reference-errors-and-limits/).

## Tiers and the review dialog

| Tier | Who | Install behaviour |
|---|---|---|
| T1 | Bundled first-party plugins | Grants up to the manifest ceiling. High-risk grants are asked at first activation, and again at later server starts after each **Not now**, at most three prompts in total. |
| T2 | Signed by a publisher key the user trusts | The review dialog shows every grant. High-risk grants start unchecked. |
| T3 | Unsigned local packages | Needs `--allow-unsigned` or `ilium plugin dev`. Shows a `dev` badge and runs under the T2 sandbox and ceilings. |

A package signed by an unknown key is offered for trust, with the key fingerprint. Declining refuses the install. A T2 or T3 install shows the review dialog. The plugin installs with the grants that stay checked:

<pre class="term">╭─ Install example.notes 0.1.0 ────────────────────────────────────╮
│ Example Labs · signed (T2) · fingerprint 3F2A…9C01               │
│ Keeps a notes pane for each project.                             │
│                                                                  │
│ Server                                                           │
│ [x] Read the workspace tree …………………………………………………………………… tree.read │
│ Client                                                           │
│ [x] Show panels and settings pages ………………………………………………… ui.panels │
│ [x] See pane sizes, titles and focus ………………………………………… panes.read │
│ [x] Provide its own kind of pane ……………………………… panes.provide-kind │
│                                                                  │
│              [ Install ]   [ Details ]   [ Cancel ]              │
╰──────────────────────────────────────────────────────────────────╯
</pre>

Illustrative, not generated. Each row is one capability with its grant string. Rows marked `HIGH` start unchecked.

A package that fills other plugins' extension points shows them in a separate group, **Adds to other plugins**. Each line names the owner and the point, for example `Text triggers > Before fire: can stop a trigger from sending text (high risk)`. A package with `replaces` adds one more confirmation: `Replaces Titles and organisation. The built-in plugin is turned off while this one is on.` Turning the replacement off or uninstalling it turns the built-in plugin back on. Its stored data stays on disk and the replacement does not read it. See [Extending core plugins](/pages/extending-core-plugins/).

## Updates, trust and keys

The rules for a person who installs packages are in [Install and manage plugins](/pages/user-install-and-manage/). The rules that a package and its publisher must meet are these. Key formats, the trust store and registry updates are defined in [Signing and trust](/pages/reference-signing-and-trust/) and [Install and update](/pages/reference-install-and-update/).

- [USR-126] An update MUST keep the plugin id and MUST have a higher version. The signature MUST come from the key that signed the installed version, or from a key that the old key endorsed with a rotation statement (SEC-142). Any other key MUST be refused with `package.publisher-key-changed`. Test: offer an update signed by an unendorsed second key and assert it is refused with `package.publisher-key-changed`.
- [USR-127] Trust MUST be per key, MUST be stored in the user's trust list, and MUST NOT be created by installing a package without a prompt. Test: install with a new key in a script without `--yes` and assert the install is refused.
- [USR-128] A T2 package whose key was removed from the trust list MUST stop loading at the next activation and show `Publisher no longer trusted` (SEC-144); it is not uninstalled and its data stays. Trusting the key again reactivates it. A later update MUST ask for trust again. Test: remove the key, list, and offer an update.
- [USR-129] A publisher MUST keep the private key offline and MUST NOT put it in a package. A lost key means new packages need a new key and a new trust prompt. Test: review `ilium plugin keygen` output for the path of the private key and its mode `0600`.

## Edge cases

- **Clamped limits.** A limit above the tier ceiling is clamped and logged, never refused. The user can set limits in `config.toml` under `[plugins.limits]`.
- **Declined grants.** The plugin still installs. Imports for a declined capability return `not-permitted`.
- **Updates.** A newer version of an installed id is an update, and the dialog shows only new capabilities. A lower version needs `--allow-downgrade`.
- **Keybinding conflicts.** The user's remap wins, then kernel keys, then T1, then T2 and T3 plugins. The losing binding is inactive and listed in Settings > Keys.
- **Settings schema.** Allowed keywords are `type`, `properties`, `required`, `enum`, `title`, `description`, `default`, `minimum`, `maximum`, `pattern`, `maxLength`, `help`, `step`, `unit`, `secret`, `multiline`, `enum_titles` and `picker`.
- **Process grants.** `process.run:*` is refused with `manifest.grant-program`. `process.run:sh` is valid only in T1 packages.

## What the user sees

After a successful install, the plugin appears in Settings > Plugins with its grants. Its commands appear in the command palette, and its help topic `example.notes/overview` is readable. Revoking a grant in Settings > Plugins, or with `ilium plugin grants`, withdraws the contributions that need it and restarts the plugin with the smaller grant set. A failed install prints its errors and runs no plugin code.

### After install

Install, update, rollback and uninstall follow [Install, update and uninstall](/pages/reference-install-update-uninstall/). Data layout changes use [Data migration](/pages/guide-data-migration/). A development copy installed by `ilium plugin dev` reloads with the reason `upgrade`; see [Lifecycle](/pages/lifecycle/#development-reload).


## See also

- [Worlds and sides](/pages/worlds-and-sides/)
- [Capabilities and grants](/pages/capabilities/)
- [Lifecycle](/pages/lifecycle/)
- [Versioning](/pages/versioning/)
- [Getting started toolchain](/pages/getting-started-toolchain/)
- [Manifest reference](/pages/reference-manifest/)
- [Sharing a package](/pages/tutorial-09-package-and-share/)
- [Security](/pages/security/)
- [Package format](/pages/reference-package-format/)
- [Signing and trust](/pages/reference-signing-and-trust/)
- [Registry reference](/pages/reference-registry/)
- [Install and update](/pages/reference-install-and-update/)

API version checked: `ilium:plugin@0.1.0`.

### Security

The install pipeline is defined in [Grant review and consent](/pages/reference-grant-review-and-consent/) ([SEC-100]). Inspect a package with `ilium plugin inspect` and `ilium plugin verify` ([SEC-155]); archive rules are in [Trust and supply chain](/pages/reference-trust-and-supply-chain/) ([SEC-150]).
