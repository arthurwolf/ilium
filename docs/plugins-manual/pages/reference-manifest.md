---
layout: page
title: Manifest reference
permalink: /pages/reference-manifest/
---

## Manifest reference

The manifest is the `manifest.json` file at the root of every `.iliumpkg` package. The host reads it before any code runs. It names the plugin and its publisher, lists one WebAssembly component per side, requests grants, declares when the plugin starts, and inventories every other file in the package. Keys are JSON and snake_case. A key the schema does not define is refused.

This page is the reference for every key: its type, whether it is required, its default, its limits and the error code for a violation. Each rule has an id such as `PKG-013` that a test can cite. The machine-readable schema and the full list of error codes with message templates are in [Manifest schema and error catalogue](/pages/reference-manifest-schema/). The order in which the installer checks the rules is in [Package format](/pages/reference-package-format/#the-validation-order). Requirement ids are collected in [Requirements: manifest and package](/pages/reference-requirements-manifest-package/).

### Contents

- A complete client example
- Document rules
- Top-level keys
- Components and exports
- The capabilities object
- Activation triggers
- Contributes keys
- Chrome and contribution keys
- Settings rows
- Limits object
- Validation and error model
- Worked examples
- Edge cases

### You will need

- The package layout. Required entries: `manifest.json`, one component per side, and the `files` inventory. Optional folders: `assets/`, `help/`, `sounds/` and `src/`. The `signature` entry is required only for tier T2.
- The grant names and risk marks: [Capability table](/pages/reference-capabilities/).
- The SDK for your language, when you write the code that fills the contributions: [Rust SDK](/pages/rust-sdk/) or [TypeScript SDK](/pages/typescript-sdk/). The manifest is the same JSON for both.

<figure>
<svg viewBox="0 0 360 170" role="img" aria-label="Package layout: manifest and component are required, optional folders are dashed">
  <rect x="6" y="6" width="348" height="158" rx="8" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="18" y="26" font-size="11" fill="currentColor">example.demo.iliumpkg</text>
  <rect x="18" y="38" width="130" height="36" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="83" y="60" font-size="11" text-anchor="middle" fill="currentColor">manifest.json</text>
  <rect x="18" y="86" width="130" height="36" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="83" y="108" font-size="11" text-anchor="middle" fill="currentColor">client.wasm</text>
  <rect x="200" y="38" width="144" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="272" y="60" font-size="11" text-anchor="middle" fill="currentColor">assets/ help/ sounds/</text>
  <rect x="200" y="86" width="144" height="36" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="272" y="108" font-size="11" text-anchor="middle" fill="currentColor">signature (T2 only)</text>
  <text x="180" y="146" font-size="10" text-anchor="middle" fill="currentColor">Solid: required. Dashed: optional.</text>
</svg>
<figcaption>A package holds the manifest and one component per side. The optional folders and the signature are dashed.</figcaption>
</figure>

## A complete client example

This client plugin has one command, one settings tab and one panel. Do not write the `files` values by hand. `ilium plugin pack` computes the length in bytes and the SHA-256 of every entry.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.demo",
  "name": "Example demo",
  "version": "0.1.0",
  "publisher": "Example Publisher",
  "description": "Shows how a small client plugin declares its contributions.",
  "license": "MIT",
  "icon": "puzzle",
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": ["views"]
  },
  "capabilities": {
    "client": ["tree.read", "ui.panels", "settings.read-all"]
  },
  "activation": ["command:example.demo/show"],
  "contributes": {
    "commands": [
      { "id": "show", "title": "Show demo panel", "icon": "layout" }
    ],
    "settings": [
      {
        "kind": "tab",
        "id": "demo",
        "title": "Demo",
        "sections": [
          {
            "id": "badges",
            "title": "Badges",
            "scope": "client",
            "rows": [
              { "type": "toggle", "key": "show_badges", "title": "Show badges", "default": true }
            ]
          }
        ]
      }
    ],
    "panels": [
      { "id": "demo", "title": "Demo", "icon": "layout", "modal": false }
    ]
  },
  "files": [
    { "path": "client.wasm", "length": 183204, "sha256": "<64 hex digits>" }
  ]
}
```

`settings.read-all` is a high-risk grant that only bundled (T1) plugins may request. A third-party package that asks for it is refused with `manifest.grant-tier`.

<div class="warn">

A capability listed in `capabilities` appears in the review dialog at install time. Ask only for what the plugin uses. A declined grant does not stop the plugin from loading, but each import that needs it returns `not-permitted`.

</div>

## Document rules

These rules apply to the whole file before any key is read.

| Id | Rule | Code |
|---|---|---|
| [PKG-001] | The manifest MUST be the entry `manifest.json` at the root of the package, at most 1 MiB, encoded as UTF-8 without a byte order mark and valid under RFC 8259. A source manifest in a project folder has the same form. | `manifest.json-invalid`, `package.manifest-size` |
| [PKG-002] | The top-level value MUST be an object. A JSON array, string, number, `true`, `false` or `null` is refused. | `manifest.schema` |
| [PKG-003] | No object may hold the same member name twice. The check applies at every depth and before any key is interpreted. It stops the validation. | `manifest.duplicate-key` |
| [PKG-004] | Comments, trailing commas, single-quoted strings and unquoted keys are not JSON and MUST be refused. | `manifest.json-invalid` |
| [PKG-005] | Every key MUST be one the schema defines for that object. An unknown key is refused at every depth. The only objects that accept other names are the JSON Schema subsets (`arguments_schema`, `entry_schema`, `payload_schema`, `reply_schema`, `settings_schema`) and the `entry` object of an extension, which are checked against their own schema. | `manifest.unknown-key` |
| [PKG-006] | `null` is not a value. A key that is not needed MUST be omitted. | `manifest.schema` |
| [PKG-007] | A number that must be an integer MUST be written without a fraction or an exponent (`10`, not `10.0` or `1e1`) and MUST be inside the range of its key. Numbers that are not integers are used only where a key says `number`. | `manifest.schema` |
| [PKG-008] | Nesting MUST NOT be deeper than 32 levels. | `manifest.schema` |
| [PKG-009] | Display text (`name`, `publisher`, `description`, titles, labels) MUST NOT contain control characters, line or paragraph separators (U+2028, U+2029) or bidirectional override and isolate characters (U+202A to U+202E, U+2066 to U+2069). | `manifest.display-name` |
| [PKG-010] | The order of keys does not change the meaning. `ilium plugin pack` writes them in the order of the table below and `files` last. The signature covers the exact bytes of the packed file, so the order after packing MUST NOT be changed. | none |
| [PKG-011] | A source manifest MAY omit `files`. A packed manifest MUST contain it. | `manifest.key-missing` |
| [PKG-012] | A manifest MUST declare at least one of `server`, `client` and `scene`. | `manifest.key-missing` |

## Top-level keys

| Key | Type | Required | Default | Constraint | Id |
|---|---|---|---|---|---|
| `api` | string | yes | none | Exactly `"ilium:plugin@0.1.0"` | PKG-013 |
| `id` | string | yes | none | 2 to 8 segments joined by `.`, see below. At most 128 characters | PKG-014 |
| `name` | string | yes | none | 1 to 64 characters of display text | PKG-015 |
| `version` | string | yes | none | SemVer 2.0.0, at most 64 characters | PKG-016 |
| `publisher` | string | yes | none | 1 to 64 characters of display text | PKG-017 |
| `description` | string | no | empty | At most 500 characters, no line breaks | PKG-018 |
| `license` | string | no | none | An SPDX expression, 1 to 128 characters | PKG-019 |
| `icon` | string | no | none | A Tabler icon name: `[a-z0-9-]`, 1 to 64 characters | PKG-020 |
| `categories` | list of string | no | none | 1 to 3 distinct values from the [category list](/pages/reference-registry/#search-and-categories) | PKG-021 |
| `keywords` | list of string | no | none | At most 8 distinct words of 1 to 24 characters without spaces | PKG-022 |
| `homepage`, `repository` | string | no | none | An `https://` address of at most 256 characters | PKG-023 |
| `data_use` | list of object | no | none | At most 8 entries `{ "what", "where", "purpose", "opt_in" }`, strings of at most 200 characters, `opt_in` boolean | SEC-267 |
| `security_contact` | string | no | none | An email address or an `https://` address, at most 200 characters | SEC-268 |
| `server`, `client`, `scene` | object | at least one | none | One component per side, see [Components and exports](#components-and-exports) | PKG-024 |
| `capabilities` | object | no | none | One grant list per side | PKG-025 |
| `activation` | list of string | no | lazy | At most 32 distinct triggers | PKG-026 |
| `requires` | object | no | none | Host range, services and plugins that must be present. See [Versions and dependencies](/pages/reference-dependency-resolution/#the-requires-object) | PKG-027 |
| `replaces` | list of string | no | none | At most 4 distinct ids of replaceable core plugins. A longer list fails with `manifest.schema` | PKG-028 |
| `accepts_messages_from` | list of string | no | none | At most 32 distinct plugin ids. A plugin receives `messages.peer` messages only from these ids. See [Messages and topics](/pages/api-messages-and-topics/) | PKG-579 |
| `replaceable`, `surface` | boolean, list | core plugins | `false` | `replaceable: true` needs a non-empty `surface` | PKG-029 |
| `contributes` | object | no | none | Static contributions, see [Contributes keys](#contributes-keys) | PKG-030 |
| `limits` | object | no | host defaults | Requested limits, clamped to the tier ceiling | PKG-031 |
| `data_version` | integer | no | 1 | 1 to 4,294,967,295 | PKG-032 |
| `limits_tier` | string | no | `"standard"` | `"standard"` or `"first-party"` | PKG-033 |
| `unstable` | list of string | no | none | At most 8 of `ui-calendar`, `plugin-services`, `gpu-kernels`, `websocket` | PKG-034 |
| `files` | list | packed only | none | `path`, `length` and `sha256` per entry. See [Package format](/pages/reference-package-format/#the-inventory) | PKG-035 |

There is no key for the tier and none for the publisher id. The tier comes from the signature and the trust store (PKG-235), and the publisher id comes from the signature and must be the start of `id` (PKG-218).

| Id | Rule | Code |
|---|---|---|
| [PKG-013] | `api` MUST be the string `ilium:plugin@0.1.0`. A well-formed `ilium:plugin@X.Y.Z` outside the window of the host is refused at stage 3 as unsupported. Any other string is a schema error. | `package.api-unsupported`, `manifest.schema` |
| [PKG-014] | `id` MUST have 2 to 8 segments joined by `.`. A segment has 1 to 32 characters from `a-z`, `0-9` and `-`, and does not start or end with `-`. The whole id has at most 128 characters. The prefix `ilium.` is reserved for the release, and `core`, `local` and `test` are reserved as first segments (PKG-220). Valid: `example.demo`, `com.acme.git-tools`. Invalid: `demo`, `Example.Demo`, `example..demo`, `example.-demo`. | `manifest.id-invalid`, `manifest.id-reserved` |
| [PKG-015] | `name` is the title that the review dialog, the store and the Plugins screen show. It MUST follow PKG-009. | `manifest.schema`, `manifest.display-name` |
| [PKG-016] | `version` MUST be a SemVer 2.0.0 string with three numeric parts and an optional prerelease and build part. A leading `v` is refused. Details: [Versions and dependencies](/pages/reference-dependency-resolution/#plugin-versions). | `manifest.version-invalid` |
| [PKG-017] | `publisher` is a display name. It MUST equal the display name stored with the signing key (PKG-219) for a signed package. It is not an identity. | `manifest.publisher-mismatch`, `manifest.display-name` |
| [PKG-018] | `description` is one paragraph shown in the review dialog and the store. | `manifest.display-name` |
| [PKG-019] | `license` is shown and stored. The host refuses an empty value or one over 128 characters. A registry publish may require a known SPDX expression. | `manifest.schema` |
| [PKG-020] | An icon name that the client does not know is drawn as the default plugin icon. It is a warning in `check` and never an install error. | none (warning only) |
| [PKG-021] | `categories` is required to publish to a registry. A value outside the list is refused at publish. | `manifest.schema`, `registry.category-unknown` |
| [PKG-022] | `keywords` feed search only. More than 8 items, or an item outside 1 to 24 characters, is refused. | `manifest.schema` |
| [PKG-023] | `homepage` and `repository` are shown as text and as a link that the user opens on purpose. The host never fetches them. | `manifest.schema` |
| [PKG-024] | A side that is declared MUST have its component in the package (PKG-125). | `manifest.file-missing` |
| [PKG-025] | `capabilities` MAY have the keys `server`, `client` and `scene` only. A list for a side without a component is refused. Each list holds at most 256 distinct grants. | `manifest.grant-side`, `manifest.unknown-key` |
| [PKG-026] | `activation` entries follow [Activation triggers](#activation-triggers). | `manifest.schema` |
| [PKG-027] | `requires` is defined in one place only: [Versions and dependencies](/pages/reference-dependency-resolution/#the-requires-object). | `manifest.requires-invalid`, `manifest.requires-self` |
| [PKG-028] | Each entry of `replaces` MUST be a core plugin id. The rules are under [Replacement](#replacement). | `manifest.replaces-unknown` |
| [PKG-029] | `replaceable` and `surface` are for core plugins. A third-party package that sets `replaceable` is refused. `surface` entries are `kind:name` with `kind` one of `services`, `points`, `commands`, `pane-kinds`, `topics` and `cli`. | `manifest.limits-tier`, `manifest.schema` |
| [PKG-030] | `contributes` accepts only the keys of the tables below. | `manifest.unknown-key` |
| [PKG-031] | `limits` accepts only the keys of the [Limits object](#limits-object). A request above the ceiling is clamped, never refused. | `manifest.unknown-key` |
| [PKG-032] | `data_version` is the version of the data layout the plugin writes. A stored version above the manifest value follows the downgrade rule of the [data migration guide](/pages/guide-data-migration/). Absent means 1. 0 and negative values are refused (rule LIF-100). | `manifest.data-version` |
| [PKG-033] | `limits_tier: "first-party"` raises the settings caps for bundled T1 plugins with an `ilium.` id. Any other package that sets it is refused at stage 9. | `manifest.limits-tier` |
| [PKG-034] | An `unstable` name MUST be a feature of this host. A T2 or T3 package can list one only in development mode. A component that imports an unstable item not named here is refused. | `manifest.unstable-unknown`, `manifest.unstable-tier`, `manifest.unstable-missing` |
| [PKG-035] | `files` lists every entry except `manifest.json` and `signature`, sorted in archive order, with no path twice. | `manifest.files-order`, `manifest.file-*` |

## Components and exports

Each side has one object:

```json
{ "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": ["detector"] }
```

| Id | Rule | Code |
|---|---|---|
| [PKG-040] | `entry` MUST be `server.wasm` for `server`, `client.wasm` for `client` and `scene.wasm` for `scene`. | `manifest.entry-invalid` |
| [PKG-041] | `world` MUST be `ilium:server/base@0.1.0` for `server`, `ilium:client/base@0.1.0` for `client` and `ilium:anim/base@0.1.0` for `scene`. | `manifest.schema` |
| [PKG-042] | [ABI-018] `exports` lists only the optional exports of that side, once each, from the table below. The required exports (`lifecycle` and `handler`, and `renderer` for a scene) are never listed. Default: the empty list. | `manifest.schema` |
| [PKG-043] | [ABI-006] The list MUST equal the optional exports that the component really has. Both a missing and an extra name are refused. | `manifest.exports-mismatch` |
| [PKG-044] | A side that lists `hook-handler` MUST have at least one extension with a `handler` on that side. A side with such extensions MUST list it. | `manifest.exports-mismatch` |
| [PKG-045] | A manifest with `data_version` above 1 MUST list `migrator` in `exports` for each of its `server` and `client` components (a `scene` component has no `migrator`), and each of those components MUST export it. A component with nothing to migrate exports a `migrator` that returns `ok`. Checked at stage 6 with `manifest.exports-mismatch` (rule LIF-278). | `manifest.exports-mismatch` |

| Optional export | Server | Client | Scene | Serves |
|---|---|---|---|---|
| `views` | no | yes | no | Panels and views drawn by the plugin |
| `annotator` | no | yes | no | Decorations on tree rows |
| `detector` | yes | no | no | Agent identity and activity |
| `job-runner` | yes | yes | yes | Background jobs |
| `service-provider` | yes | yes | no | Services that other plugins call |
| `inference-provider` | yes | yes | no | Model completion |
| `settings-validate` | yes | yes | no | Checks a settings change before it is saved |
| `catalog-provider` | yes | yes | no | `catalogs` and plugin-owned `choice` sources (needs a `catalogs` entry, `manifest.catalog-unknown` otherwise) |
| `menu-provider` | no | yes | no | `dynamic` context menus and toolbar submenus |
| `virtual-rows-provider` | yes | yes | no | `virtual_rows` with `children: "provider"` |
| `token-provider` | no | yes | no | `languages` (needs `editor.read`) |
| `hook-handler` | yes | yes | no | `extensions` entries with a `handler` |
| `migrator` | yes | yes | no | Data migration when `data_version` rises |

This table is the only list of optional exports. Pages that show a shorter list show the exports that matter for their topic. The worlds and the WIT fragments are in [Exports](/pages/api-exports/) and [Worlds and sides](/pages/worlds-and-sides/).

### The capabilities object

Each entry is a grant string: a bare name such as `tree.read`, or a name and argument such as `http:api.example.com`. The first colon separates them. These grants need an argument, and a bare name is refused with `manifest.grant-argument-missing`:

| Grant | Argument form |
|---|---|
| `commands.invoke` | `<plugin-id>/<id>`, `<plugin-id>/*` or `*` |
| `http` | `api.example.com`, `*.example.com` (subdomains only), optional `:<port>`, or `localhost` |
| `process.run` | a program name on `PATH`, or an absolute path |
| `fs.read`, `fs.write`, `fs.watch` | a scope such as `worktrees`, or `path:<template>` |
| `services.use`, `services.provide` | a name without version, such as `com.example.git/status` |
| `messages.peer` | one exact plugin id |
| `topics.subscribe` | an exact topic, `<prefix>/*`, or `*` |
| `http-api.route` | a path starting with `/`, with `{name}` segments |

Path templates start with `{home}`, `{config}`, `{data}`, `{cache}`, `{project}`, `{worktrees}` or a literal absolute path. They end with a name or with `/**`. Any other `*` is refused.

Risk marks: `normal` grants are checked by default in the review dialog. High-risk (H) grants are listed unchecked. A bundled T1 plugin is asked for its H grants at first activation.

| Id | Rule | Code |
|---|---|---|
| [PKG-050] | A grant is a string of 3 to 256 characters: a name of lower-case words joined by `.` or `-`, optionally followed by `:` and an argument. The first colon separates them. | `manifest.grant-unknown`, `manifest.grant-argument` |
| [PKG-051] | A grant MUST be a grant of this API version, in the [Capability table](/pages/reference-capabilities/). | `manifest.grant-unknown` |
| [PKG-052] | A grant that needs an argument and has none is refused. A grant that takes no argument and has one is refused. | `manifest.grant-argument-missing`, `manifest.grant-argument` |
| [PKG-053] | A wildcard is allowed only where the grant's grammar says so. | `manifest.grant-wildcard` |
| [PKG-054] | A grant MUST be available on the side that lists it. | `manifest.grant-side` |
| [PKG-055] | A grant that needs another grant (`fs.pick` needs `fs.read:picked` or `fs.write:picked`) MUST find it in the same side list. | `manifest.grant-dependency` |
| [PKG-056] | An `fs.*` scope MUST be known and accept the access. A `process.run` argument MUST be a program name or an absolute path, without arguments. | `manifest.grant-scope`, `manifest.grant-program` |
| [PKG-057] | A grant for bundled plugins only (high-risk `settings.read-all`, `sessions.cross` for T3) is refused for any other tier. The tier is known only after the signature, so this check runs at stage 9. | `manifest.grant-tier` |
| [PKG-058] | Declining a grant at install never fails the install. Each import that needs a declined grant returns `not-permitted`. | none |

## Release information the manifest does not carry

The manifest has no key for a changelog, a deprecation notice or a replacement name in `ilium:plugin@0.1.0`. The keys that describe a plugin to a person are `name`, `publisher`, `description` (one paragraph; the hard limit is 500 characters, and `lint.description-missing` warns above 280 characters), `license`, `icon`, `homepage`, `repository` and `security_contact`. An author MUST put the changelog in `README.md` and in the `description`, and MUST NOT add other metadata keys, because an unknown key is refused with `manifest.unknown-key`. [AUT-321] Deprecation and a successor are properties of a registry entry, not of the manifest. See [Publishing and support](/pages/guide-publishing-and-support/).

## Activation triggers

Without `activation`, a plugin starts on the first use of any contribution (lazy).

| Trigger | Starts the plugin when |
|---|---|
| `startup` | on the side that declares the trigger: the server starts and the plugin host has activated its dependencies. The host starts after `restore_snapshot`; see [Persistence and restart](/pages/reference-persistence-and-restart/) |
| `event:<event-name>` | the first event of that name arrives |
| `command:<id>` | the global command is invoked, for example `command:example.demo/show` |
| `view:<id>` | that view opens |
| `pane-kind:<id>` | a pane of that global kind is created |
| `scene:<id>` | that scene is selected |
| `cli:<name>` | `ilium <name>` runs |
| `http:<path>` | a request reaches that loopback route |
| `service:<interface>` | another plugin calls that service |
| `topic:<topic>` | a message on that exact topic is published |
| `settings:<tab-id>` | that plugin's own settings tab opens, or a built-in tab that carries one of its sections opens |
| `catalog:<id>` | a row or widget opens one of the plugin's own catalogues |
| `menu:<id>` | a `dynamic` menu of the plugin opens |
| `virtual-rows:<row-kind>` | a provider-backed virtual row expands |
| `language:<id>` | a buffer of a language the plugin supplies becomes visible |
| `client-request:<kind>` | the server side of the same plugin sends a request to its client side |
| `durable-timer` | a durable timer of the plugin is due |

A lazy plugin that is not active receives no topic events. Use `startup` for a pattern subscription.

| Id | Rule | Code |
|---|---|---|
| [PKG-059] | `activation` is a list of at most 32 distinct triggers. Each is `startup`, `durable-timer`, or a kind from the table followed by `:` and a name without spaces. An unknown kind is refused. Without `activation`, the plugin is lazy. | `manifest.schema` |
| [PKG-060] | A trigger that names an own contribution (`command:`, `view:`, `pane-kind:`, `scene:`, `cli:`, `settings:`, `catalog:`, `menu:`, `virtual-rows:`) MUST name one that exists in `contributes`. | `manifest.contribution-target` |

## Contributes keys

| Key | Entry fields | Notes |
|---|---|---|
| `commands` | `id`, `title`, `palette`, `voice`, `arguments_schema`, `icon`, `when`, `side` | Global id `<plugin-id>/<id>`. `palette` is a boolean, default `true`. `voice` is 1 to 8 phrases of 1 to 64 characters. `side` is `server` or `client`: the side whose `handler` receives `command-invoked`. Default: the only declared side, or `client` when both exist. A side without a component is `manifest.contribution-target` (PKG-061) |
| `keybindings` | `command`, `chord`, `mode` | `mode` is `global` (default), `prefix` or `pane:<kind>` |
| `settings` | `kind` (`tab` or `section`), then for `tab`: `id`, `title`, `short_title`, `icon`, `glyph`, `after`, `order`, `render`, `keywords`, `sections`; for `section`: `id`, `tab`, `placement`, `order`, `title`, `scope`, `rows`, `collapsed` | A `tab` adds a page to the Settings rail, with `render` `rows` (default) or `view` (drawn by the `views` export, target `settings-page("<tab-id>")`). A `section` adds rows to a built-in tab (needs `settings.contribute:<tab-id>`) or to one of the plugin's own tabs (`self:<tab-id>`). `scope` is `server` or `client`, default `client`. Full field tables: [Plugin settings extension](/pages/settings-extension/) and [settings schema reference](/pages/reference-settings-schema/) |
| `help` | `id`, `file`, `title` | Markdown under `help/`, required to exist |
| `decoration_slots` | `id`, `placement`, `priority` | `placement` is `badge`, `icon`, `subtitle` or `footer` |
| `virtual_rows` | `id`, `title`, `icon`, `parent` | `parent` is `project` or `session` |
| `toolbar_buttons` | `id`, `icon`, `label`, `command`, `applies_to`, `requires_action` | `applies_to` is signature ids or `all` |
| `context_menu` | `id`, `title`, `command`, `target` | `target` is `terminal`, `tree-node`, `editor` or `pane:<kind>` |
| `panels` | `id`, `title`, `icon`, `default_size`, `modal` | Needs `ui.panels` |
| `pane_kinds` | `id`, `title`, `icon`, `primitive`, `wants_keys`, `persist` | `primitive` is `basic`. `persist` is `metadata` or `none` |
| `scenes` | `id`, `title`, `family`, `modes`, `settings_schema`, `frame_rate_cap` | Requires a `scene` component |
| `signatures` | `id`, `process_names`, `aliases`, `icon`, `resume`, `actions`, `submit_keys`, `session_scope` | Agent primitive. `resume` is an argv template with `{session}` |
| `output_filters` | `id`, `regex`, `panes`, `rate_per_second` | `panes` is `all`, `agents` or `terminals` |
| `link_detectors` | `id`, `regex`, `command` | Terminal primitive |
| `editor_line_actions` | `id`, `title`, `regex`, `command` | Editor primitive |
| `sounds` | `id`, `file` | Files under `sounds/`. The id `chirping` is reserved |
| `cli` | `name`, `summary`, `arguments_schema`, `output`, plus `aliases` | One object, or a list of at most 8 objects, one per subcommand. Names are unique inside the plugin. `aliases` lists further names. `output` is `jsonl`, `text` or `mixed` |
| `http_routes` | `method`, `path` | Loopback routes |
| `services` | `provides` | Service names offered |
| `topics` | `publishes`, `subscribes` | Publish only under the plugin's own id prefix |
| `jobs` | `kind`, `deadline_ms`, `memory_mib` | Override the job limits, within the ceilings. `deadline_ms` is the CPU-time limit of the job (the `job-runner.run` CPU row of the scheduling table); the wall-time limit is the `limits` key `job_wall_ms` (see the Limits object), not a `jobs` key |
| `onboarding_steps` | `id`, `title`, `order` | Target `onboarding-step("<id>")` |
| `extension_points` | `id`, `kind`, `mode`, `title`, `entry_schema`, `payload_schema`, `reply_schema`, `risk`, `deadline_ms`, `max_handlers`, `max_entries`, `dynamic`, `version` | Points other plugins fill. Cap 16. Needs `extensions.provide`. See [Extension points and replacement](#extension-points-and-replacement) |
| `extensions` | `point`, `id`, `priority`, `entry`, `handler` | Entries or handlers added to a point. Cap 64. Needs `extensions.contribute` |

`themes` is not a manifest key. Plugins ship no colour themes. The chrome and contribution keys are listed in [Chrome and contribution keys](#chrome-and-contribution-keys) below.

| Id | Rule | Code |
|---|---|---|
| [PKG-061] | Each list under `contributes` is an array with at most the number of entries in its cap (32 where the table gives none). An entry `id` is 1 to 64 characters of `a-z`, `0-9` and `-`, unique inside its list. | `manifest.contribution-limit`, `manifest.schema` |
| [PKG-062] | A reference to another contribution (`command`, `popup_view`, `slot`, `parent`, `help`) MUST name an entry of this plugin that exists, unless the field says that another plugin's id is allowed. | `manifest.contribution-target`, `manifest.settings-command`, `manifest.settings-help` |
| [PKG-063] | A file named by `help` or `sounds` MUST exist in the package under `help/` or `sounds/` and be listed in `files`. | `manifest.file-missing` |
| [PKG-064] | `contributes.scenes` MUST NOT be present unless the manifest declares a `scene` component. | `manifest.contribution-target` |
| [PKG-065] | A `cli` name, a `sound` id and a `commands` global id MUST NOT collide with another entry of the same plugin. A collision with another installed plugin is found when the second one is installed. | `manifest.cli-name-taken`, `manifest.sound-id-taken` |

### Keybinding chords

A chord is `*(modifier "+") key` in lower case. Modifiers are `ctrl`, `alt`, `shift` and `super`, in any order. The key is one printable character, or `enter`, `escape`, `tab`, `backspace`, `delete`, `insert`, `home`, `end`, `page-up`, `page-down`, `up`, `down`, `left`, `right` or `f1` to `f24`. Write `+` as `plus`. Shift+A is `shift+a`.

On a clash the user's remap wins, then kernel built-ins, then T1, then T2 and T3 plugins. Within a rank, the first installed plugin wins. The losing binding is inactive and listed in Settings > Keys.

### The when expression

`when` joins comparisons with `&&`, `||`, `!` and parentheses. A comparison is `key == "text"` or `key != "text"`. The keys are `pane.primitive` (`basic`, `terminal`, `agent`, `editor`, `none`), `pane.kind`, `agent.activity`, `node.kind`, `node.file_extension`, `agent.signature`, `view.focused` and `display_mode`. All eight are valid in `commands`, `toolbar_buttons`, `context_menu`, `row_actions` and `tree_toolbar`; see [the `when` keys table](/pages/api-commands/). A key with no pane in scope is false. `when` only hides entries. It never blocks `commands.invoke`.

### Settings schema subset

Command `arguments_schema` uses JSON Schema draft 2020-12, restricted to: `type` (`object`, `boolean`, `integer`, `number`, `string`), `properties`, `required`, `enum`, `title`, `description`, `default`, `minimum`, `maximum`, `pattern`, `maxLength`, and the Ilium keywords `help`, `step`, `unit`, `secret`, `multiline`, `enum_titles` and `picker` (`file` or `directory`, with `extensions`). Other keywords are refused with `manifest.schema-keyword`.

Settings rows do not carry a schema of their own. Each value row (`toggle`, `choice`, `number`, `text`, `text-area`, `path`, `color`, `keybinding`, `list`, `location`, `map`, `multi-choice`) compiles to a property of the plugin's settings schema, which uses the same keywords and also `type: array` with `items` and `maxItems` (at most 64) for `list` rows. The row fields and their compiled form are in the [settings schema reference](/pages/reference-settings-schema/).

### Signature entries

`id` is 1 to 64 characters from `[a-z0-9-]`. `process_names` holds 1 to 32 non-empty names of at most 255 bytes, without `/` unless absolute. `aliases` holds at most 32 entries. `actions` holds at most 32, each with `keys` of at most 1 KiB. The global id is `<plugin-id>.<id>`.

## Chrome and contribution keys

A manifest that does not use these keys is read without them. The manifest is JSON for both SDKs, so the tables below apply to Rust and TypeScript plugins alike. The language-specific part is the code that fills the contribution at run time, shown further down.

### New contributes keys

Counts per plugin are fixed caps. A manifest above a cap is refused with `manifest.contribution-limit`.

| Key | Entry fields | Grant | Cap | Meaning |
|---|---|---|---|---|
| `tree_toolbar` | `id`, `icon`, `label`, `command`, `caption`, `popup_view`, `when` | `ui.chrome` | 4 | A button in the tree toolbar. `label` is at most 24 characters. `caption`, shown beside the icon when the sidebar is wide, is at most 12 cells. `popup_view` is an own `panels` id opened as a popover anchored at the button instead of running `command` |
| `row_actions` | `id`, `icon`, `label`, `command`, `applies_to`, `when`, `order` | `ui.chrome` | 4 | An icon in the hover strip of tree rows. `applies_to` is a list of `pane`, `group`, `folder`, `project`, `agent` or `virtual:<row-kind>`. The click arrives as `command-invoked` with origin `row-action`. `order` is 0 to 1000, default 500 |
| `tree_orderings` | `id`, `title`, `slot` | `tree.decorate` | 4 | An entry in the tree's Order By choice. Siblings sort by the `sort-key` of the plugin's decoration in `slot`, ascending. Nodes without a key keep their manual order after the keyed ones |
| `status_segments` | `id`, `priority`, `max_width` | `ui.chrome` | 2 | A segment of the status bar, filled with the intent `set-status-segment`. `priority` is 0 to 100 (higher survives narrow widths). `max_width` is 4 to 40 cells, default 24 |
| `pane_frame_slots` | `id`, `placement`, `applies_to` | `ui.chrome` | 4 | A place in pane frames filled with `set-frame-slot`. `placement` is `header-icon`, `header-text`, `footer-row` or `banner`. `applies_to` is `all`, `agents`, `terminals`, `pane:<kind>` or a list of signature ids |
| `display_modes` | `id`, `title`, `icon`, `slot`, `hide_unmarked` | `tree.decorate` | 2 | A tree display mode next to Normal and Attention. The tree shows the decorations of `slot` first and, with `hide_unmarked: true`, hides panes without one. Changes arrive as `display-mode-changed` |
| `catalogs` | `id`, `title`, `item_kind` | none | 8 | An option catalogue served by the `catalog-provider` export. `item_kind` is `text`, `glyph`, `swatch` or `pattern`. Global id `<plugin-id>/<id>`. Usable as a `choice` source by the plugin's own rows and widgets |
| `client_requests` | `id`, `timeout_ms` | none | 16 | Request kinds the plugin's client side answers for its own server side. `timeout_ms` is 1 to 30,000, default 5,000 |
| `languages` | `id`, `title`, `extensions`, `file_names` | `editor.read` | 8 | Languages whose tokens the `token-provider` export supplies to the host editor and to `code-view`. `extensions` are written without the dot |

### Fields added to existing entries

All of these are optional. Each key has a default.

| Entry | New fields | Rule |
|---|---|---|
| `commands` | `category`, `keywords` | Palette grouping (at most 32 characters) and search terms (at most 8, each at most 24 characters). The command palette itself is a kernel surface |
| `commands`, `toolbar_buttons`, `context_menu`, `row_actions`, `tree_toolbar` | `when` keys | The context grammar gains `node.kind` (`pane`, `group`, `folder`, `project`), `pane.kind`, `node.file_extension`, `agent.signature`, `agent.activity`, `view.focused` and `display_mode`. An unknown key is `manifest.when-key` |
| `context_menu` | `icon`, `when`, `dynamic`, `items` | `dynamic: true` makes the host call `menu-items` each time the menu opens. `items` is a static list of menu entries with `parent` references, at most 2 levels deep (`manifest.menu-depth`) |
| `toolbar_buttons` | `submenu`, `right_click_command` | `submenu` is a static `items` list or `"dynamic"`. A left click opens the submenu instead of running `command` |
| `virtual_rows` | `expandable`, `children`, `accepts_drop`, `drop_command` | `children` is `"static"` (rows from `set-virtual-rows` with `parent`) or `"provider"` (the `virtual-rows-provider` export). `accepts_drop` is a list of `pane`, `file`, `text`. A drop runs `drop_command` with origin `virtual-row-drop` |
| `decoration_slots` | `visibility`, `trailing` | `visibility` is `always` (default) or `on-hover`. `trailing: true` places the slot right-aligned after the title, as the cost columns do |
| `panels` | `placement`, `min_size`, `mode_label`, `wants_keys` | `placement` is `overlay` (default), `left`, `right` or `bottom`. `min_size` is in cells. `mode_label` (at most 16 cells) is shown in the status bar while the panel has focus. `wants_keys: true` delivers unconsumed keys as `key-pressed` |
| `cli` | `output`, `requires_server`, `stdin`, `argument_groups` | `output` is `jsonl` (default), `text` or `mixed` (both write calls allowed). `requires_server: false` runs the command in a transient plugin host inside the CLI process when no server runs. `stdin` is `none` (default), `optional` or `required`. `argument_groups: ["pane-selector"]` adds `--pane <id\|title\|.>` and `--all-panes`. A bad value is `manifest.cli-field` |
| `scenes` | `prerender`, `glyph_plane` | `prerender` is `{ frames, fps, cache_mib }` (frames 1 to 3,600, fps 1 to 30, cache 1 to 128 MiB; `manifest.scene-prerender`). `glyph_plane: true` lets the scene write text cells over its pixels |
| `signatures` | staged `actions` | An action may be `{ "stages": [{ "keys": "/compact", "delay_ms": 0 }, { "keys": "\r", "delay_ms": 120 }] }`, with the stage rules of `send-input` |

Package assets (`assets/**`, `help/*.md`, `sounds/*` and fonts) may total 64 MiB, or up to 256 MiB for T1 bundled plugins. A larger package is refused with `manifest.asset-size`. A single font asset may not exceed 4 MiB.

### What the contributions look like

The render shows the tree sidebar with one `tree_toolbar` button, the `row_actions` strip on the hovered row, a `tree_orderings` entry, and a `status_segments` entry in the status bar. The glyphs are the ones the real client draws. A plugin chooses an icon by name; the host draws it in the matching slot.

<figure>
<pre class="term">╭  ≡ ● · Sessions ─────────────────────────────╮
│ ★ 🔎 ⟳        <- tree_toolbar                │
│ ▼ Ilium                                      │
│ ›   ⠋ Fix auth     ✎ ↑ ↓ ×  <- row_actions   │
│ ›     Shell                                  │
│ ›   ● Tests       ❄                          │
│                                              │
│ Order by: Manual · Cost <- tree_orderings    │
╰──────────────────────────────────────────────╯
 ℹ NORMAL  -  quota 61%  <- status_segments
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Tree sidebar contributions.</figcaption>
</figure>

The strip `✎ ↑ ↓ ×` is the stable-glyph form of the built-in row actions. A plugin's `row_actions` entry joins that strip in `order`. The decoration `❄` on the last row is a `decoration_slots` entry with `visibility: "always"`. The status bar caps are Nerd Font characters and are not shown here.

### Declaring the contributions

This manifest fragment declares one of each of the commonly used chrome keys. It is the same JSON for both SDKs.

```json
"capabilities": { "client": ["ui.chrome", "tree.decorate"] },
"contributes": {
  "tree_toolbar":    [{ "id": "quota", "icon": "gauge", "label": "Quota", "caption": "Quota",
                        "command": "show-quota" }],
  "row_actions":     [{ "id": "freeze", "icon": "snowflake", "label": "Freeze",
                        "command": "freeze-row", "applies_to": ["pane", "agent"], "order": 600 }],
  "status_segments": [{ "id": "quota", "priority": 60, "max_width": 24 }],
  "decoration_slots":[{ "id": "cost", "placement": "badge", "priority": 10,
                        "visibility": "on-hover", "trailing": true }],
  "tree_orderings":  [{ "id": "by-cost", "title": "Cost", "slot": "cost" }]
}
```

### Filling a segment at run time

The manifest reserves the place. The plugin fills it with an intent at run time. The Rust and TypeScript listings for `set-status-segment`, the `row-action` click and the other chrome intents are on [Chrome intents](/pages/api-intent-chrome/) and [Tree chrome](/pages/api-tree-chrome/). The filled segment is cleared when the plugin stops, and the host drops updates above 10 per second per slot (the segment id is the slot).

### Services

A service is a typed function set that one plugin offers and others call. The call contract is in [Services](/pages/api-services/). The manifest side is here.

**Provider.** `contributes.services.provides` lists up to 16 objects.

| Field | Type | Required | Rule |
|---|---|---|---|
| `name` | string | yes | `<plugin-id>/<name>` with `<name>` of 1 to 64 characters from `a-z`, `0-9` and `-`. The prefix MUST be the id of this plugin, or the id of a core plugin named in this package's `replaces` list; such names are accepted only for items listed in that core plugin's surface. A kernel-known name has the form `ilium:<package>/<interface>` and only bundled plugins may offer one |
| `version` | string | yes | SemVer of the service contract. Callers match it with the minimum version in `requires.services` under the compatibility rule of [Services](/pages/services/#names-versions-and-grants) |
| `schema` | string | yes | Path of a JSON file in the package that maps each method to a `params` and a `result` schema, using the keywords of the [Data schema subset](/pages/reference-settings-schema/#data-schema-subset). At most 256 KiB |

**Caller.** `requires.services` lists strings of the form `<name>@<version>` with a full `major.minor.patch` version, which means the minimum compatible version. Caret, tilde and ranges are refused with `manifest.service-version`. The grant is `services.use:<name>`; the provider's grant is `services.provide:<name>`. A grant never carries a version. A cycle of service requirements is refused with `manifest.service-cycle` [PKG-654].

| Id | Rule | Code |
|---|---|---|
| [PKG-079] | A provider name with a foreign prefix, a duplicate name or a missing schema file is refused. | `manifest.service-name`, `manifest.file-missing` |

### Extension points and replacement

An extension point is a slot that one plugin owns and others fill. The full contract is in [Extension points](/pages/api-extension-points/). The manifest side is below.

**Owner.** `contributes.extension_points` declares up to 16 points. The global id is `<plugin-id>/<id>`, and `id` is 1 to 64 characters of `a-z`, `0-9` and `-`. Each entry needs the grant `extensions.provide:<plugin-id>/<point>`.

| Field | Applies to | Rule |
|---|---|---|
| `id` | both | 1 to 64 characters from `a-z`, `0-9` and `-`, unique among the plugin's points |
| `title` | both | Name shown in the review dialog and on the Extensions tab. Display text of 1 to 64 characters |
| `kind` | both | `collect` (data entries) or `hook` (code that runs in the owner's flow) |
| `mode` | hook | `observe`, `transform`, `veto` or `first` |
| `entry_schema` | collect | [Data schema subset](/pages/reference-settings-schema/#data-schema-subset). An entry is at most 16 KiB |
| `payload_schema`, `reply_schema` | hook | [Data schema subset](/pages/reference-settings-schema/#data-schema-subset). Each at most 64 KiB. `reply_schema` is required for `transform` and `first` and refused (`manifest.extension-point-invalid`) for `observe` and `veto`. On `transform` it lists the changeable fields only |
| `risk` | both | `standard` (default) or `high`. `high` makes `extensions.contribute` for this point a high-risk grant |
| `deadline_ms` | hook | 5 to 50 for every tier, default 20 |
| `max_handlers` | hook | 1 to 8, default 8 |
| `max_entries` | collect | 1 to 256, default 64 |
| `dynamic` | collect | `true` allows `register` and `unregister` at run time |
| `on_failure` | hook | `pass` or `refuse`. Allowed only when `risk` is `high` and `mode` is `veto` or `transform`; default `refuse` for high points; `manifest.extension-point-invalid` otherwise |
| `final_check` | hook | Boolean, default `false`. `transform` only. When `true`, contributors that set `guard` are called once on the final payload |
| `version` | both | Semver of the point's schema, default `0.1.0` when omitted. See [Versioning](/pages/versioning/) |

**Contributor.** `contributes.extensions` holds up to 64 entries. Each needs `extensions.contribute:<plugin-id>/<point>`, one exact point and no wildcard.

| Field | Rule |
|---|---|
| `point` | `<plugin-id>/<id>`, optionally `<plugin-id>/<id>@<minimum-version>`. Without `@version` the contribution matches any owner version. With it, the version is a bare version and so a caret range (PKG-317, PKG-318, PKG-328 in [Dependency resolution](/pages/reference-dependency-resolution/)): `@1.2.0` accepts `>=1.2.0 <2.0.0`, `@0.1.0` accepts `>=0.1.0 <0.2.0` |
| `id` | Unique per contributor and point |
| `priority` | 0 to 100, default 50. Lower runs first. Ties go by plugin id |
| `entry` | Collect points. Validated against `entry_schema` at install (`manifest.extension-entry-invalid`) |
| `handler` | Hook points. A name that the component's `hook-handler` export dispatches on |
| `guard` | Boolean, default `false`. Allowed only on a point with `final_check: true` and with a `handler`. A guard handler is called once after the transform chain with the final payload and replies `pass` or `refuse` |

```json
"contributes": {
  "extensions": [
    { "point": "ilium.core.smart-copy/recognisers", "id": "jira-key", "priority": 50,
      "entry": { "id": "jira", "kind": "ticket", "regex": "[A-Z][A-Z0-9]+-[0-9]+" } },
    { "point": "ilium.core.smart-copy/after-copy", "id": "strip-tokens", "priority": 30,
      "handler": "strip_tokens" }
  ]
}
```

List `hook-handler` in `exports` on every side that has hook extensions. The handler lives on the side that makes the owner's calls: the server for a server-owned point, the client for a client-owned point (`manifest.extension-side` otherwise). The sample is an illustration of the shape and is not a shipped plugin.

A contribution whose owner is not installed or not enabled is dormant. Install does not fail. See [Lifecycle](/pages/lifecycle/).

#### Replacement

`"replaces": ["ilium.core.titles"]` names core plugins to replace. Only plugins with `"replaceable": true` can be replaced. Their `surface` lists what the replacement must provide, for example `["services:suggest", "points:before-apply-title", "commands:...", "pane-kinds:...", "topics:title-applied"]`. A replacement that misses an item is refused with `manifest.replaces-surface-incomplete`, which lists the missing items. `replaces` needs no grant. The user confirms it in the review dialog. `ilium.core.sessions` and `ilium.core.settings` are not replaceable. See [Extending core plugins](/pages/extending-core-plugins/).

### Export fragments

The optional exports and their sides are listed once, in [Components and exports](#components-and-exports). The contributions that need an export are checked at install: a `catalogs` entry needs `catalog-provider`, a `dynamic` menu needs `menu-provider`, a `virtual_rows` entry with `children: "provider"` needs `virtual-rows-provider`, a `languages` entry needs `token-provider` and `editor.read`, hook extensions need `hook-handler` and a rising `data_version` needs `migrator`. The deadlines are 20 ms for `query-catalog` and `menu-items` and 10 ms for `tokens`. A breach of `menu-items` or `tokens` falls back to static content and does not fail the user's action.

The Rust and TypeScript listings for providers are on [Catalogs](/pages/api-catalogs/), [Menu provider](/pages/api-menu-provider/), [Virtual rows provider](/pages/api-virtual-rows-provider/) and [Token provider](/pages/api-token-provider/).

### Settings rows

The row table of the [settings schema reference](/pages/reference-settings-schema/) stays the single authoritative table. It lists 18 row types.

| `type` | Value | Specific fields | Written |
|---|---|---|---|
| `location` | `{ lat, lon, label }` | `search` (default true), `allow_current` (default false; needs `location.observer`), `precision` (0 to 6, default 4) | when the user confirms `Use location` |
| `preview` | none | `view` (an own `panels` id), `height` (4 to 24 rows, default 8), `live` | not a value; own tabs only |
| `notice` | none | `id`, `text` (at most 200 characters), `tone` (`info`, `success`, `warning`, `danger`), `dismissible` | not a value; a dismissal is stored per `id` |
| `map` | object of string keys | `value_type`, `key_pattern`, `max_entries` (1 to 64, default 16), `key_title`, `value_title` | on each committed entry |
| `multi-choice` | array of option values | `options` (2 to 64) or `source`, `min_items`, `max_items`, `style` (`checklist` or `dialog`) | at once on each toggle |
| `meter` | none | `id`, `title`, `segments` (at most 8), `max` (default 1), `show_values` | not a value; updated with `settings.set-meter` |

Fields added to the existing row types:

| Row | New fields | Rule |
|---|---|---|
| `toggle` | `style` | `switch` (default) or `checkbox` |
| `choice` | `style: "dialog"`, option fields `description`, `disabled_reason`, `preview`, `help`, and `source`, `filter`, `allow_custom`, `custom_pattern`, `view_modes` | `source: { "catalog": "<id>" }` replaces `options` with a host catalogue (`ilium.icons`, `ilium.palettes`, `ilium.dithers`, `ilium.filters`, `ilium.style-presets`, `ilium.sounds`, `ilium.models`) or an own one (`self:<id>`). `source` together with `options` is `manifest.settings-source` |
| `number` | `style`, `steps`, `special_values`, `debounce_ms` | `style` is `stepper` (default) or `slider`. `steps` is an ascending ladder of 2 to 64 numbers. `special_values` is at most 8 `{ value, title }` pairs |
| `text` | `reveal_last`, `debounce_ms` | `reveal_last` 0 to 8 with `secret: true` shows `••••a1b2` |
| `text-area` | `debounce_ms` | 300 to 2,000 ms |
| `list` | `item_format`, `style`, `separator`, `reorderable`, `remote`, `item_status` | `item_format` is `glob`, `url`, `host`, `path`, `regex` or `chord`. Object items may have up to 8 primitive fields |
| `info` | `rich` | A list of spans `{ text, tone, bold, command }`, at most 120 characters in total |
| every value row | `visible_when`, `scope`, `applies` | `visible_when` is `{ key, equals \| in \| not }` on one own key. `scope: "project"` stores the value in the project's `.ilium/` settings. `applies: "restart"` draws the `↻` marker |

This section of a `contributes.settings` entry uses most of them. The render below it is how the client draws it.

```json
{ "id": "look", "title": "Look", "scope": "client", "rows": [
  { "type": "choice", "key": "palette", "title": "Palette", "style": "dialog",
    "source": { "catalog": "ilium.palettes" } },
  { "type": "number", "key": "speed", "title": "Speed", "style": "slider",
    "minimum": 0, "maximum": 4, "steps": [0, 0.25, 0.5, 1, 2, 4],
    "special_values": [{ "value": 0, "title": "Paused" }], "default": 1 },
  { "type": "toggle", "key": "stars", "title": "Show stars", "style": "checkbox", "default": true },
  { "type": "number", "key": "star_count", "title": "Stars", "minimum": 10, "maximum": 2000,
    "default": 300, "visible_when": { "key": "stars", "equals": true }, "applies": "restart" },
  { "type": "meter", "id": "budget", "title": "Budget used", "max": 5, "show_values": true },
  { "type": "notice", "id": "privacy", "tone": "warning", "dismissible": true,
    "text": "Place names are sent to the geocoding provider." }
]}
```

<figure>
<pre class="term">╭  ≡ ● · Look ─────────────────────────────────╮
│ Palette ………………… ← Sunset … → +               │
│ Speed ………… ───────●──────── - 1 + *          │
│ [x] Show stars                               │
│ Stars …………………… - 300 + *  ↻                  │
│     Applies after restart.                   │
│ Budget used …… ███████░░░░░░░ 3.1 / 5        │
│ ! Place names go to the geocoding provider.  │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Settings page rows.</figcaption>
</figure>

The runtime calls that update the `meter` row and mark a row busy are described, with Rust and TypeScript listings, on [Settings API](/pages/api-settings/).

## Limits object

Limits are integers in the unit of their name: MiB, milliseconds or a count. The plugin asks for a value and the host clamps it to the ceiling for its tier. A request above the ceiling is logged and clamped, never refused. The user sets the same keys under `[plugins.limits]` in `config.toml`. An unknown limit key is refused with `manifest.unknown-key`.

| Key | Default | T1 ceiling | T2, T3 ceiling |
|---|---|---|---|
| `memory_mib` | 32 | 256 | 128 |
| `table_elements` | 10000 | 50000 | 20000 |
| `event_deadline_ms` | 20 | 50 | 20 |
| `command_deadline_ms` | 50 | 100 | 50 |
| `render_deadline_ms` | 10 | 20 | 10 |
| `annotate_deadline_ms` | 10 | 20 | 10 |
| `frame_deadline_ms` | frame budget | 50 | 50 |
| `detector_deadline_ms` | 20 | 50 | 20 |
| `activate_deadline_ms` | 2000 | 5000 | 2000 |
| `deactivate_deadline_ms` | 500 | 1000 | 500 |
| `migrate_deadline_ms` | 30000 | 60000 | 30000 |
| `job_deadline_ms` | 2000 | 60000 | 10000 |
| `job_wall_ms` | 20000 | 600000 | 100000 |
| `job_memory_mib` | 64 | 512 | 256 |
| `job_io_mib` | 16 | 64 | 16 |
| `concurrent_jobs` | 2 | 8 | 2 |
| `job_queue` | 16 | 64 | 16 |
| `event_queue` | 1024 | 4096 | 1024 |
| `pending_requests` | 64 | 256 | 64 |
| `timers` | 64 | 256 | 64 |
| `output_filters` | 32 | 128 | 32 |
| `watches` | 64 | 256 | 64 |
| `key_grabs` | 8 | 32 | 8 |
| `ui_tree_nodes` | 4096 | 16384 | 4096 |
| `ui_tree_kib` | 1024 | 4096 | 1024 |
| `decorations` | 4096 | 16384 | 4096 |
| `node_metadata_kib` | 64 | 256 | 64 |
| `storage_bucket_mib` | 16 | 256 | 16 |
| `disk_mib` | 256 | 4096 | 256 |
| `fs_read_all_mib` | 64 | 512 | 64 |
| `fs_write_mib` | 16 | 64 | 16 |
| `http_response_mib` | 8 | 64 | 8 |
| `process_output_mib` | 4 | 32 | 4 |
| `log_lines_per_second` | 200 | 1000 | 200 |
| `open_videos` | 2 | 8 | 2 |
| `video_buffer_mib` | 64 | 256 | 64 |
| `audio_captures` | 2 | 4 | 2 |
| `http_streams` | 4 | 16 | 4 |
| `children` | 2 | 8 | 2 |
| `durable_timers` | 64 | 256 | 64 |
| `vector_indexes` | 4 | 16 | 4 |
| `vector_entries` | 10000 | 200000 | 50000 |
| `shared_cache_mib` | 64 | 256 | 128 |
| `journal_entries` | 5000 | 20000 | 5000 |
| `toasts_pending` | 3 | 3 | 3 |
| `package_assets_mib` | 64 | 256 | 64 |

| Id | Rule | Code |
|---|---|---|
| [PKG-070] | Every `limits` value is an integer from 0 to 4,294,967,295 in the unit of its key. | `manifest.schema` |
| [PKG-071] | A value above the ceiling of the tier is clamped to the ceiling and logged. It is never refused. A value of 0 means the default. | none |
| [PKG-072] | A key outside the table is refused. | `manifest.unknown-key` |
| [PKG-073] | The ceiling comes from the tier, which is known only after the signature check. The manifest cannot raise it. | none |
| [PKG-074] | The user may set the same keys under `[plugins.limits]` or `[plugins."<id>".limits]`. The effective value is `min(ceiling, v)`, where `v` is the first set of the `[plugins."<id>".limits]` value, the `[plugins.limits]` value, the manifest value and the default, in that order; 0 or a missing value means not set, so the next source applies. The user can raise a value up to the ceiling and can lower it (LIF-143). | none |

The nine keys not in the base set are the extended requestable limits. What a breach does (a trap, a refused call or a dropped event) is in the [errors and limits reference](/pages/reference-errors-and-limits/). A manifest requests limits the same way in both SDKs, because `limits` is part of the JSON.

With `"limits_tier": "first-party"` a bundled T1 `ilium.` plugin also lifts the settings caps (tabs per plugin 8, value keys 2,048, rows per section 64, list items 1,024, choice options 512). The ceilings in the table do not change.

## Validation and error model

The installer validates a package in 11 stages and never runs plugin code before the review. The stages, their order and the rule that a failing stage stops the later ones are in [Package format](/pages/reference-package-format/#the-validation-order). This page's rules run in stages 3 and 4 (schema and semantic), with the tier-dependent ones in stage 9.

| Id | Rule |
|---|---|
| [PKG-075] | A stage that collects errors MUST report every error it finds in the order of the keys of the manifest, up to 100. A test with two defects in one stage expects both codes. A test with defects in two stages expects only the earlier one. |
| [PKG-076] | Every error carries a `code`, a `message` from the template of the catalogue, and, when the error has a place in the document, a pointer (RFC 6901) or a path to it. |
| [PKG-077] | `ilium plugin check` on a source manifest runs stages 2 to 4. It reports the same codes and pointers as the installer. |
| [PKG-078] | The code of every error is in the [error catalogue](/pages/reference-manifest-schema/#error-catalogue). The older spellings are in the table of retired codes. |

This table maps each key group to the code families that can refuse it.

| Group | Codes |
|---|---|
| Document | `manifest.json-invalid`, `manifest.duplicate-key`, `manifest.schema`, `manifest.unknown-key`, `manifest.key-missing`, `manifest.display-name` |
| Identity | `manifest.id-invalid`, `manifest.id-reserved`, `manifest.version-invalid`, `manifest.id-publisher`, `manifest.publisher-mismatch` |
| Components | `manifest.entry-invalid`, `manifest.exports-mismatch`, `manifest.api-version`, `package.api-unsupported` |
| Grants | `manifest.grant-*`, `manifest.export-grant`, `manifest.import-not-requested` |
| Contributions | `manifest.contribution-*`, `manifest.cli-*`, `manifest.when-*`, `manifest.menu-depth`, `manifest.sound-id-taken`, `manifest.catalog-unknown`, `manifest.client-request-unknown` |
| Settings | `manifest.settings-*`, `manifest.visible-when`, `manifest.row-format` |
| Extension points | `manifest.extension-*`, `manifest.grant-missing-extension` |
| Replacement | `manifest.replaces-*` |
| Dependencies | `manifest.requires-*`, `manifest.service-cycle`, `dependency.*` |
| Limits and tier | `manifest.limits-tier`, `manifest.unstable-*`, `manifest.asset-size` |
| Inventory | `manifest.file-*`, `manifest.files-order` |

<figure>
<pre class="term">
<span class="t-prompt">$</span> ilium plugin install demo.iliumpkg
<span class="t-ok">ok</span>   manifest.json read, api ilium:plugin@0.1.0
<span class="t-ok">ok</span>   files: 3 entries, lengths and hashes match
<span class="t-err">error</span> manifest.grant-unknown
<span class="t-dim">   /capabilities/client/0: "tree.raed" is not a</span>
<span class="t-dim">   capability of ilium:plugin@0.1.0.</span>
<span class="t-dim">   Stopped. No code was loaded.</span>
</pre>
<figcaption>Rendered at 48 columns. Illustrative, not generated. A package refused for a misspelt grant.</figcaption>
</figure>

## Worked examples

### Example 1: a client plugin

The client example above is complete. Its component exports `handler`, which is required, and `views`, which the manifest lists. Its command is in `activation`, so the plugin loads only when the user runs `example.demo/show`.

### Example 2: a server plugin that detects an agent

This manifest declares a server component with the optional `detector` export, one signature and a `startup` trigger. Every server component must implement `lifecycle` and `handler`.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.todo",
  "name": "Example todo scanner",
  "version": "0.1.0",
  "publisher": "Example Publisher",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": ["detector"]
  },
  "capabilities": { "server": ["agent.read"] },
  "activation": ["startup"],
  "contributes": {
    "signatures": [
      { "id": "todo-runner", "process_names": ["todo-runner"], "icon": "checklist" }
    ]
  },
  "files": [
    { "path": "server.wasm", "length": 96112, "sha256": "<64 hex digits>" }
  ]
}
```

Every server component must implement `lifecycle` and `handler`. The Rust and TypeScript code of a detector is on [Exports](/pages/api-exports/#example-2-a-detector-on-the-server).

### Example 3: a plugin with a dependency

This manifest needs a minimum Ilium release and one other plugin. The range `^1.2.0` follows the Cargo caret rule: it accepts `1.2.0` and every later `1.x`, and refuses `2.0.0`.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.review",
  "name": "Review helper",
  "version": "1.0.0",
  "publisher": "Example Labs",
  "categories": ["tools"],
  "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0" },
  "requires": {
    "host": ">=0.1.0",
    "plugins": [ { "id": "com.example.notes", "version": "^1.2.0" } ]
  },
  "files": [
    { "path": "client.wasm", "length": 51200, "sha256": "<64 hex digits>" }
  ]
}
```

If `com.example.notes` is not installed or is at `2.0.0`, the install fails at stage 10 with `dependency.unresolved` and a message that names the range.

## Edge cases

| Situation | Result |
|---|---|
| A limit is above the tier ceiling | Clamped and logged |
| The user declines a grant | The plugin loads. Calls needing it return `not-permitted`. `granted` omits it |
| A grant is revoked later | The plugin is re-activated with the reduced set, and dependent contributions are withdrawn |
| An upgrade asks for new grants | The upgrade dialog shows only the new grants |
| A lower version is installed | Refused unless `--allow-downgrade` is given |
| A key appears twice in an object | `manifest.duplicate-key`; no other check runs |
| `"version": "v1.0.0"` | `manifest.version-invalid` |
| `"id": "demo"` (one segment) | `manifest.id-invalid` |
| `"description"` has a line break | `manifest.display-name` |
| `"limits": { "memory_mib": 4096 }` for a T2 package | Clamped to 128 MiB and logged; the install succeeds |
| `exports` lists a name that the side does not have | `manifest.exports-mismatch` |
| `exports` is empty but the component has `detector` | `manifest.exports-mismatch` |
| `data_version` is `0` | `manifest.data-version` |
| A command has `"side": "server"` and there is no `server` component | `manifest.contribution-target` |
| The manifest is an empty file | `manifest.json-invalid` |
| The manifest is 1 MiB plus one byte | `package.manifest-size` at stage 1 |

## What the user sees

The review dialog lists each grant of a T2 or T3 package with its risk mark, the publisher and the signature state. A T3 package carries a `dev` badge. The user checks the grants to keep. T1 bundled plugins are not shown this dialog. They ask for H grants at first activation.

Requirement ids for this page: [API-080]. See the [requirements index](/pages/reference-requirements/).

## See also

- [Publishing and support](/pages/guide-publishing-and-support/) for release metadata
- [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) for `version`, `api` and `data_version`
- [Capability table](/pages/reference-capabilities/)
- [Packages](/pages/packages/)
- [Capabilities](/pages/capabilities/)
- [Lifecycle](/pages/lifecycle/)
- [Settings screens](/pages/settings-screens/)
- [Plugin settings extension](/pages/settings-extension/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Versioning](/pages/versioning/)
- [Porting from .iliumanim](/pages/porting-iliumanim/)
- [Security model](/pages/security/)
- [Extension points](/pages/api-extension-points/)
- [Settings schema reference](/pages/reference-settings-schema/)
- [Events and intents](/pages/reference-events-and-intents/)
- [Manifest schema and error catalogue](/pages/reference-manifest-schema/)
- [Package format](/pages/reference-package-format/)
- [Signing and trust](/pages/reference-signing-and-trust/)
- [Versions and dependencies](/pages/reference-dependency-resolution/)
- [Requirements: manifest and package](/pages/reference-requirements-manifest-package/)

### Security

Security keys: `data_use` ([SEC-267]), `security_contact` ([SEC-268]) and the display name rule ([SEC-381], error `manifest.display-name`). See [User security and privacy](/pages/user-plugin-security-and-privacy/).
