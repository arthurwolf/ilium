---
layout: page
title: Project templates
permalink: /pages/reference-project-templates/
---

## Project templates

`ilium plugin new` writes a complete, working project from a template. This page lists the exact files that each template writes in each language, the flags of the command, the refusal codes, the placeholder rules and the guarantees about a fresh project. The older pages showed only the first three files. This page replaces those lists.

A template is a plain set of files. The command copies them and replaces a few placeholders. It runs no build and needs no network. A project made from a template builds, passes its own tests, and is ready to edit.

Terms:

- **Template**: a named set of files, one set per language.
- **Side**: one of `server`, `client` or `scene`. A plugin has one or more sides, and each side builds to its own component.
- **Placeholder**: a token such as `@@id@@` that `new` replaces when it writes a file.

## You will need

- The `ilium` binary. The command works offline and needs no toolchain.
- A plugin id, for example `example.agent-tools`, and a language, `rust` or `typescript`.
- Background reading: [Set up a toolchain](/pages/getting-started-toolchain/) and [Plugin command-line reference](/pages/reference-plugin-cli/).

## The command

`ilium plugin new <dir> --id <id> --lang <rust|typescript> (--template <name> | --side <name>) [--name <text>] [--publisher <text>] [--first-party] [--ci <github|gitlab|none>] [--force]`

| Flag | Default | Rule |
|---|---|---|
| `--id` | none, required | Matches `[a-z0-9-]+(\.[a-z0-9-]+)+`. Fails with `new.id-invalid`. |
| `--lang` | none, required | `rust` or `typescript`. |
| `--template` | none | One of the eight names below. |
| `--side` | none | An alias for `server`, `client`, `both` or `scene`. |
| `--name` | the last label of the id, with the first letter in capitals | The display name. |
| `--publisher` | the operating system user name | The `publisher` value. |
| `--first-party` | off | Allows an `ilium.` id. Needed for `core-style`. |
| `--ci` | `none` | Adds a CI file. See [CI recipes](/pages/guide-ci-recipes/). |
| `--force` | off | Writes into a non-empty folder without deleting any file. |

- **[DEV-260]** `--template` and `--side` MUST name the same template when both are given. `--side` accepts only `server`, `client`, `both` and `scene`. All other templates are reachable only through `--template`.
- **[DEV-261]** `new` MUST fail with `new.dir-not-empty` for a folder that holds files, `new.id-invalid` for a bad id, `new.id-reserved` for an `ilium.` id without `--first-party`, and `new.template-unknown` for an unknown name. No file MUST be written when a check fails.
- **[DEV-262]** `core-style` MUST require `--first-party` and an id that starts with `ilium.`. Every other template MUST refuse an `ilium.` id, even with `--first-party`, with `new.id-reserved`. A package built from `core-style` that is not bundled in the Ilium source tree is refused at install by `manifest.id-reserved`.
- **[DEV-263]** `new` MUST be deterministic. The same flags and the same `ilium` version MUST write byte-identical files.
- **[DEV-264]** The placeholders are `@@id@@`, `@@name@@`, `@@publisher@@`, `@@package@@` (the last label of the id, which is the Cargo package name or the npm package name) and `@@sdk@@` (the SDK version). `new` MUST replace them in the files marked in the tables below and MUST leave no `@@` token in any written file.
- **[DEV-265]** `--name` and `--publisher` MUST be written as JSON strings or TOML strings with escaping, so a quote or a backslash in the value cannot break the file.
- **[DEV-266]** The entry source of a TypeScript side MUST be `src/<side>.ts`: `src/server.ts`, `src/client.ts` or `src/scene.ts`. The entry source of a single-side Rust project MUST be `src/lib.rs`. This replaces the earlier spellings `src/main.ts`, `src/plugin.ts` and `src/index.ts` on other pages.
- **[DEV-267]** A Rust project with two sides MUST be a Cargo workspace with one crate per side (`server/`, `client/`) and one shared crate (`shared/`). Each side crate MUST declare its side in `[package.metadata.ilium]` as `side = "server"` or `side = "client"`, which is how `build` finds the crate for a side.
- **[DEV-268]** Every template MUST write `.gitignore` with the lines `dist/`, `.ilium/`, `target/` and `node_modules/`.
- **[DEV-269]** Every template MUST write `README.md` and `CHANGELOG.md` skeletons and a `help/overview.md` topic that `contributes.help` lists, so `lint.readme-missing`, `lint.changelog-missing` and `lint.help-topic-missing` pass on a fresh project.

## The guarantees about a fresh project

- **[DEV-270]** A project made by `new`, with no edits, MUST pass `ilium plugin check`, `ilium plugin lint --deny warnings`, `ilium plugin build` and then `ilium plugin test` on a machine where `ilium plugin doctor` reports no `fail`, with zero warnings.
- **[DEV-271]** A fresh project MUST pass its own native unit tests (`cargo test`, `bun test`).
- **[DEV-272]** A fresh project MUST request the smallest grant set that its template needs and no more.
- **[DEV-273]** A fresh project MUST run activation without logging at `warn` or `error`.

## Common files

These files are the same in every template, with the placeholders replaced.

| Path | Content |
|---|---|
| `manifest.json` | The manifest of the template, see below. Placeholders in `id`, `name`, `publisher`. |
| `.gitignore` | `dist/`, `.ilium/`, `target/`, `node_modules/`. |
| `README.md` | Title `@@name@@`, one paragraph, the build and test commands, the list of requested grants. |
| `CHANGELOG.md` | Keep a Changelog header with an `Unreleased` section. |
| `help/overview.md` | A help topic with a title line and two short paragraphs. |
| `tests/fixtures/sample.json` | A fixture, see [Test harness reference](/pages/reference-test-harness/). |
| `tests/fixtures/sample.expect.json` | The expectation of that fixture. |
| `tests/golden/overview.txt` | The golden render, in templates with a view. |
| `.github/workflows/plugin.yml` or `.gitlab-ci.yml` | Only with `--ci github` or `--ci gitlab`. |

The README skeleton:

```markdown
# @@name@@

A plugin for Ilium. Describe what it does in one paragraph.

## Build and test

    ilium plugin check
    ilium plugin lint --deny warnings
    ilium plugin test
    ilium plugin build

## Grants

List each requested grant and why the plugin needs it.
```

The CHANGELOG skeleton:

```markdown
# Changelog

## [Unreleased]

### Added

- First version.
```

The help topic skeleton:

```markdown
# @@name@@

One sentence that says what the plugin adds.

Name each command, setting and view the plugin adds, one line each.
```

## The eight templates

| Template | Sides | Purpose | Requested grants |
|---|---|---|---|
| `server` | server | Reacts to events and sets tree decorations | `tree.read`, `tree.decorate` |
| `client` | client | A panel with one view | `ui.panels` |
| `both` | server, client | A server that sends one message to a client view | `ui.panels` |
| `scene` | scene | An ambient background scene | none |
| `pane-kind` | client | A new kind of pane with a view | `ui.panels`, `panes.provide-kind` |
| `core-style` | server, client | A first-party replaceable plugin with an extension point | `tree.read`, `ui.panels` |
| `service-provider` | server | Provides a named service | `services.provide:@@id@@/greeter` |
| `extension` | server | Fills a hook point of a core plugin | `extensions.contribute:ilium.core.smart-copy/after-copy` |

- **[DEV-274]** The requested grants of each template MUST be those in the table, and the manifest `capabilities` object MUST list them per side.

### server

Files, Rust: `manifest.json`, `Cargo.toml`, `src/lib.rs`, `tests/handler.rs`, `tests/fixtures/sample.json`, `tests/fixtures/sample.expect.json`, `.gitignore`, `README.md`, `CHANGELOG.md`, `help/overview.md`.

Files, TypeScript: `manifest.json`, `package.json`, `tsconfig.json`, `bunfig.toml`, `src/server.ts`, `tests/handler.test.ts`, and the same fixtures and documents.

The manifest:

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "@@id@@",
  "name": "@@name@@",
  "version": "0.1.0",
  "publisher": "@@publisher@@",
  "description": "Marks agents that start working.",
  "license": "MIT",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": []
  },
  "capabilities": { "server": ["tree.read", "tree.decorate"] },
  "contributes": {
    "decoration_slots": [{ "id": "work", "placement": "badge", "priority": 50 }],
    "help": [{ "id": "overview", "file": "help/overview.md", "title": "@@name@@" }]
  }
}
```

The Rust `Cargo.toml`:

```toml
[package]
name = "@@package@@"
version = "0.1.0"
edition = "2021"
rust-version = "1.90"

[package.metadata.ilium]
side = "server"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
ilium-plugin-sdk = "@@sdk@@"

[dev-dependencies]
ilium-plugin-sdk = { version = "@@sdk@@", features = ["testing"] }
```

The Rust `src/lib.rs`:

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::intents::SetDecorationSpec;
use ilium_plugin_sdk::types::tree::AgentActivity;

#[ilium_plugin(side = "server")]
#[derive(Default)]
pub struct Plugin;

impl Lifecycle for Plugin {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        log::log(Level::Info, &format!("activate: {} ready", context.plugin));
        Ok(())
    }
}

impl Handler for Plugin {
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
                slot: "work".into(),
                content: vec![Span::plain("work")],
                icon: None,
                sort_key: None,
                persist: false,
            })));
        }
        submissions
    }
}
```

The Rust `tests/handler.rs`:

```rust
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::tree::AgentActivity;
use @@package_ident@@::Plugin;

#[test]
fn working_agent_gets_a_badge() {
    let mut host = FakeHost::for_manifest("manifest.json").expect("manifest");
    host.activate::<Plugin>();
    let submissions = host.deliver(vec![Event::AgentActivityChanged(AgentActivityEvent {
        node: 42,
        previous: AgentActivity::Idle,
        current: AgentActivity::Working,
        revision: 1,
    })]);
    assert_eq!(submissions.len(), 1);
}
```

`@@package_ident@@` is the package name with hyphens replaced by underscores.

- **[DEV-275]** `new` MUST replace `@@package_ident@@`, the package name with hyphens replaced by underscores, in the Rust test files, in addition to the placeholders of rule DEV-264.

The TypeScript `package.json`:

```json
{
  "name": "@@package@@",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "dependencies": { "@ilium/plugin-sdk": "@@sdk@@" },
  "scripts": { "test": "bun test" }
}
```

The TypeScript `tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "noEmit": true,
    "types": ["bun-types"]
  },
  "include": ["src", "tests"]
}
```

The TypeScript `bunfig.toml`:

```toml
[test]
preload = ["@ilium/plugin-sdk/testing/setup"]
```

The preload registers the reset hook of the harness, which gives each test a fresh host (see the isolation rules in [Test harness reference](/pages/reference-test-harness/)). The TypeScript `src/server.ts`:

```ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';
import { span } from '@ilium/plugin-sdk/types';

let nextCorrelation = 1n;

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate(context) {
    log('info', `activate: ${context.plugin} ready`);
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
            slot: 'work',
            content: [span('work')],
            icon: undefined,
            sortKey: undefined,
            persist: false,
          },
        },
        correlation: nextCorrelation++,
        dedupKey: undefined,
      });
    }
    return submissions;
  },
});
```

The TypeScript `tests/handler.test.ts`:

```ts
import { expect, test } from 'bun:test';
import { createFakeHost } from '@ilium/plugin-sdk/testing';

test('working agent gets a badge', async () => {
  const host = createFakeHost().forManifest('manifest.json');
  await host.activateModule('../src/server');
  const submissions = host.deliver([
    { tag: 'agent-activity-changed', val: { node: 42n, previous: 'idle', current: 'working', revision: 1n } },
  ]);
  expect(submissions.length).toBe(1);
});
```

The fixture `tests/fixtures/sample.json` and its expectation:

```json
{
  "schema": 1,
  "events": [
    { "tag": "agent-activity-changed", "val": { "node": 42, "previous": "idle", "current": "working", "revision": 1 } }
  ]
}
```

```json
{
  "schema": 1,
  "submissions": { "match": "ordered", "items": [ { "intent": "set-decoration", "node": 42, "slot": "work" } ] }
}
```

### client

Files, Rust: `manifest.json`, `Cargo.toml`, `src/lib.rs`, `tests/view.rs`, `tests/handler.rs`, `tests/fixtures/sample.json`, `tests/fixtures/sample.expect.json`, `tests/golden/overview.txt`, and the common documents.

Files, TypeScript: `manifest.json`, `package.json`, `tsconfig.json`, `bunfig.toml`, `src/client.ts`, `tests/view.test.ts`, `tests/handler.test.ts`, and the same fixtures, golden file and documents.

The manifest differs from `server` in these keys:

```json
{
  "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": ["views"] },
  "capabilities": { "client": ["ui.panels"] },
  "contributes": {
    "commands": [{ "id": "open", "title": "Open @@name@@", "icon": "layout-sidebar" }],
    "panels": [{ "id": "main", "title": "@@name@@", "icon": "layout-sidebar", "modal": false }],
    "help": [{ "id": "overview", "file": "help/overview.md", "title": "@@name@@" }]
  }
}
```

The view draws a block with one text line. The golden file `tests/golden/overview.txt` is the render at 40 columns by 4 rows:

```text
╭  ≡ ● · Example ────────────────────────╮
│ Hello from the plugin                  │
│                                        │
╰────────────────────────────────────────╯
```

- **[DEV-276]** The golden file MUST be the exact output of the test render at the size used by the test, with Unicode box characters. The sample view test MUST compare against it with `assert_golden`.

### both

The manifest has both a `server` and a `client` entry, `capabilities` for the client only, and the client exports `views`. The server sends a `message` intent on channel `hello` when a client attaches, and the client view shows the received text. No grant is needed because messages between the sides of one plugin are always allowed.

Files, Rust: `Cargo.toml` (a workspace), `shared/Cargo.toml`, `shared/src/lib.rs` (the `Hello` message type with `serde`-free manual encoding), `server/Cargo.toml`, `server/src/lib.rs`, `client/Cargo.toml`, `client/src/lib.rs`, `manifest.json`, `tests/session.rs` (a `FakeSession` test), and the common documents and fixtures.

Files, TypeScript: `manifest.json`, `package.json`, `tsconfig.json`, `bunfig.toml`, `src/shared.ts`, `src/server.ts`, `src/client.ts`, `tests/session.test.ts`, and the common documents and fixtures.

The Rust root `Cargo.toml`:

```toml
[workspace]
members = ["shared", "server", "client"]
resolver = "2"
```

- **[DEV-277]** The `both` template test MUST use `FakeSession` to start the server and one client of the package and assert that the client view shows the message text after the server handles `client-attached`.

### scene

Files, Rust: `manifest.json`, `Cargo.toml`, `src/lib.rs`, `tests/frame.rs`, `tests/golden/frame-tones.txt`, and the common documents. Files, TypeScript: `manifest.json`, `package.json`, `tsconfig.json`, `bunfig.toml`, `src/scene.ts`, `tests/frame.test.ts`, `tests/golden/frame-tones.txt`, and the common documents.

The manifest differs from `server` in these keys:

```json
{
  "scene": { "entry": "scene.wasm", "world": "ilium:anim/base@0.1.0", "exports": [] },
  "contributes": {
    "scenes": [
      {
        "id": "waves",
        "title": "Waves",
        "family": "procedural",
        "modes": ["live"],
        "frame_rate_cap": 30,
        "settings_schema": { "type": "object", "properties": {} }
      }
    ]
  }
}
```

The scene draws tone only. Colour, dither and display settings are never scene settings, and the template adds none. The golden file holds a tone grid in digits 0 to 9 for a 16 by 4 surface at frame 3. See [Ambient scenes](/pages/api-scenes/).

### pane-kind

The manifest adds `pane_kinds` and the grant `panes.provide-kind`:

```json
{
  "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": ["views"] },
  "capabilities": { "client": ["ui.panels", "panes.provide-kind"] },
  "contributes": {
    "pane_kinds": [
      { "id": "board", "title": "@@name@@", "icon": "layout-board", "primitive": "basic", "wants_keys": true, "persist": "metadata" }
    ],
    "help": [{ "id": "overview", "file": "help/overview.md", "title": "@@name@@" }]
  }
}
```

The global id of the pane kind is `@@id@@.board`. The view test renders `ViewTarget::Pane` at 40 by 6 and compares the golden file. The handler test sends a key through the `key-pressed` event and asserts the next render.

### core-style

`core-style` writes a plugin in the style of the bundled first-party plugins: both sides, the `ilium.` id, tier T1, `limits_tier: "first-party"`, `replaceable: true`, and one declared extension point. It is for plugins that live in the Ilium source tree.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "@@id@@",
  "name": "@@name@@",
  "version": "0.1.0",
  "publisher": "Ilium",
  "limits_tier": "first-party",
  "replaceable": true,
  "surface": [],
  "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": [] },
  "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": ["views"] },
  "capabilities": { "server": ["tree.read"], "client": ["ui.panels"] },
  "contributes": {
    "extension_points": [
      { "id": "before-open", "kind": "hook", "mode": "veto", "title": "Before open", "deadline_ms": 20, "max_handlers": 8, "version": "0.1.0" }
    ],
    "help": [{ "id": "overview", "file": "help/overview.md", "title": "@@name@@" }]
  }
}
```

The `surface` list is empty until the plugin defines what a replacement must provide. The tests include a hook test that runs `before-open` against two stub contributors with `FakeSession`.

- **[DEV-278]** `core-style` MUST write the file `docs/EXTENDING.md` with the table of the plugin's extension points, so a plugin that extends it has a document to read.

### service-provider

```json
{
  "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": ["service-provider"] },
  "capabilities": { "server": ["services.provide:@@id@@/greeter"] },
  "contributes": { "services": [{ "provides": ["@@id@@/greeter"] }] }
}
```

The Rust handler implements `ServiceProvider::handle(call) -> ServiceReply` and replies with `{"greeting":"hello"}`. The test calls the service through `FakeSession` and a stub consumer, and asserts the reply JSON.

### extension

The `extension` template fills a hook of a core plugin. It requests `extensions.contribute:ilium.core.smart-copy/after-copy`, exports `hook-handler`, and declares one entry in `contributes.extensions`:

```json
{
  "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": ["hook-handler"] },
  "capabilities": { "server": ["extensions.contribute:ilium.core.smart-copy/after-copy"] },
  "contributes": {
    "extensions": [
      { "point": "ilium.core.smart-copy/after-copy@0.1.0", "id": "strip-keys", "priority": 70, "handler": "strip_keys" }
    ]
  }
}
```

The test builds a `HookCall` with a payload that holds a secret, runs the handler with `host.run_hook` and asserts `HookReply::Replace`. The core plugin point is described in [Core extension points](/pages/reference-core-extension-points/).

## Organisation and third-party templates

The eight built-in templates are not the only source. A team that ships several plugins can keep a house template (its CI file, licence, publisher key id and lint settings) in a folder or a git repository and pass it to `new`. The project keeps the template drift check of DEV-937, because the template origin is recorded in `.ilium-template.json`.

Command form:

`ilium plugin new <dir> --id <id> --lang <rust|typescript> --template <path|git-url> [--ref <git-ref>] [other flags of new]`

A `--template` value is a built-in name (DEV-260) when it matches one of the eight names. Otherwise it is a template location: a value that starts with `./`, `../` or `/`, or names an existing folder, is a local path. A value that starts with `https://`, `ssh://` or `git@` is a git url. Anything else fails with `new.template-unknown` (DEV-043) before any file is written. `--side` never accepts a location. When both flags are given they MUST name the same template or the command fails with `usage.conflict` (DEV-040).

### Template folder layout

```text
house-template/
  template.json            the descriptor, required
  files/
    rust/                  the files written for --lang rust
    typescript/            the files written for --lang typescript
    common/                the files written for every language
```

- Files in `common/` are written first, then the files of the chosen language. A path present in both is taken from the language folder.
- Paths are relative to the new project. A path with `..`, an absolute path, a symbolic link or a file name that differs only by case from another is refused with `new.template-invalid`.
- A file whose name ends in `.tmpl` is copied with placeholders replaced and the `.tmpl` suffix removed. Any other file is copied byte for byte, so binary files are safe.
- The folder MAY hold other files, such as a `README.md` for the template maintainers. Only `template.json` and `files/` are read.

### The descriptor

`template.json` has these fields. A field that is not listed makes the template invalid.

| Field | Type | Rule |
|---|---|---|
| `schema` | integer | Must be `1`. Another value fails with `new.template-schema-unsupported`. |
| `name` | string | Matches `[a-z0-9-]+`. Must not equal a built-in template name. |
| `version` | string | A semantic version. Recorded in `.ilium-template.json`. |
| `languages` | array | One or both of `rust`, `typescript`. A folder `files/<lang>/` must exist for each. |
| `sides` | array | A non-empty subset of `server`, `client`, `scene`. Describes the sides the files implement. |
| `placeholders` | object | Optional extra placeholders, see below. |
| `min_cli` | string | The lowest `ilium` version that can use the template. |

- `new` MUST fail with `new.template-language-unsupported` when `--lang` is not in `languages`, and with `new.template-cli-too-old` when the installed CLI is older than `min_cli`. The message of the second one names both versions.
- `sides` is informational for `new`. It is shown by `ilium plugin template verify` and written to `.ilium-template.json`. The template files themselves must contain a valid manifest for those sides.

### Placeholders

A third-party template uses the same placeholders as the built-in ones: `@@id@@`, `@@name@@`, `@@publisher@@`, `@@package@@`, `@@sdk@@` (DEV-264) and `@@package_ident@@` (DEV-275). Replacement and escaping follow DEV-264 and DEV-265.

- The `placeholders` object adds house values, for example `{"publisher_key_id": "acme-2026-a", "licence": "Apache-2.0"}`. A name matches `[a-z_]+` and must not equal a built-in name. Its value is a string written as given and used for `@@publisher_key_id@@`. A user can override a house value with `--set <name>=<value>`; an unknown name fails with `new.placeholder-unknown`.
- After replacement `new` MUST find no `@@name@@` token in a `.tmpl` file. Any left over fails with `new.placeholder-unresolved` naming the file and line, and no project is written. A file that is not `.tmpl` is never scanned, so an `@@` inside it is kept.
- Replacement is a plain text substitution, not a template language. There are no loops, conditions or includes.

### Fetching a git template

- A git url is cloned shallow into a temporary folder. `--ref` selects a branch, tag or commit; without it the default branch is used. The commit that was cloned is recorded.
- Fetching uses the `git` program on the path. Ilium prompts for no credentials: a url that needs a password fails with `new.template-fetch-failed`. Use a credential helper or an ssh agent.
- The temporary folder is deleted when `new` ends, whether it succeeded or not. A network failure writes nothing into `<dir>`.
- A template is data. `new` never runs a script from it, and the template folder cannot contain a hook that runs.

### Verifying a template

`ilium plugin template verify <path|git-url> [--lang <lang>]` is the check that the guarantee of DEV-270 still holds for a template. It is the tool for the template maintainer, and a CI job for the template repository SHOULD run it on every change.

1. It reads and validates `template.json` and the folder layout.
2. For each language in `languages` (or the one in `--lang`) and each placeholder set it picks a generated id, such as `example.verify-check`, and runs `new` into a scratch folder.
3. In each project it runs `check`, `lint --deny warnings`, `build` and `test`, in this order, as DEV-270 states.
4. It prints one JSON line per step, with `lang`, `step` and `ok`, and a final `summary` line. The exit code is `0` when every step of every language passes and `1` otherwise. A step that fails prints the same diagnostics as the command would, with the code (`lint.readme-missing`, for example).
5. It deletes the scratch folders, even after a failure, unless `--keep` is given.

`verify` needs the toolchains of the languages it checks. `ilium plugin doctor` (DEV-270) reports whether they are present; a missing one stops `verify` with `verify.toolchain-missing` and no template result.

### Recording the origin

`new` writes `.ilium-template.json` in the project root for every template, built-in or not. DEV-937 uses it. Its content:

```json
{
  "schema": 1,
  "origin": "git",
  "location": "https://git.example.com/acme/plugin-template.git",
  "ref": "v3",
  "commit": "9f2c1e0",
  "name": "acme-house",
  "version": "3.1.0",
  "lang": "rust",
  "files": { "ci/plugin.yml": "sha256:...", "LICENSE": "sha256:..." }
}
```

- `origin` is `builtin`, `path` or `git`. For `builtin`, `location` is the template name and `version` is the `ilium` version.
- `files` lists each template-owned file with the hash of the template content after placeholder replacement. A file the user is expected to edit, such as `src/lib.rs`, is listed by the template author in `template.json` as `"editable": ["src/lib.rs"]`; editable files are not hashed and never drift.
- With `check --template-drift` (DEV-937) the CLI re-reads the template from `location` and `ref` (for a `path`, the folder as it is now; for a `git` template, the recorded ref again, fetched fresh). It prints one `template.drift` warning per file whose template content differs from the recorded hash and does not change the project. When the template cannot be reached it prints `template.drift-unavailable` as a warning and exits as without the flag.
- A project file edited by the user also differs from the hash. `template.drift` names which of the two changed: `template` when the template changed, `local` when only the project file changed, `both` when both did.
- A project with no `.ilium-template.json` (created by a CLI that did not write it) is not an error. The drift check prints `template.drift-no-record` and stops.

### Failure modes

| Code | When |
|---|---|
| `new.template-unknown` | The value is not a built-in name or a reachable location. |
| `new.template-invalid` | `template.json` or the folder layout breaks a rule above. |
| `new.template-schema-unsupported` | `schema` is not `1`. |
| `new.template-language-unsupported` | `--lang` is not in `languages`. |
| `new.template-cli-too-old` | The CLI is older than `min_cli`. |
| `new.template-fetch-failed` | `git` failed or timed out (60 s). |
| `new.placeholder-unknown` | `--set` names a placeholder the template does not declare. |
| `new.placeholder-unresolved` | A `@@token@@` is left in a `.tmpl` file. |

- `new` writes no file when any of these fails (DEV-261).
- A template cannot give an `ilium.` id to anyone: the reserved-id rule of DEV-262 applies to every template, including third-party ones.
- Because `new` is deterministic (DEV-263), the same template, the same commit, the same flags and the same `ilium` version give byte-identical projects.

## Edge cases

- **Existing folder.** An empty folder is accepted. A folder with only a `.git` directory counts as non-empty because of the file rule, so use `--force` for it.
- **Both language files.** The template never writes `Cargo.toml` and `package.json` together, because the build refuses that.
- **Case in the id.** Upper-case letters in the id fail the pattern. The error names the first bad character.
- **Template drift.** A new `ilium` release can change a template. The README of the project names the `ilium` version that wrote it, in a comment on its first line.

## See also

- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [Test harness reference](/pages/reference-test-harness/)
- [Check and lint rules](/pages/reference-check-and-lint-rules/)
- [Plugin documentation guide](/pages/guide-plugin-documentation/)
- [Set up a toolchain](/pages/getting-started-toolchain/)

API checked against `ilium:plugin@0.1.0`.

## Production additions: T5-devtools

- [DEV-935] `new` MUST write a `.gitignore` that lists `dist/`, `target/`, `node_modules/`, `publisher.key` and `*.tmp` (and `.ilium/`, as DEV-268 requires), and a `README.md` naming the build, test, dev and pack commands. `new --license <spdx-id>` MUST write a `LICENSE` and set the manifest `license` field; an id not in the SPDX list MUST fail with `new.license-unknown` before writing files. Test: run `new` with no licence flag and read the `.gitignore` and `README.md`; run `new --license MIT` and assert the `LICENSE` file and the manifest field; run it with an unknown id and assert `new.license-unknown` with no files written.
- [DEV-937] `check --template-drift` MUST compare the project's files that come from a template (listed in `.ilium-template.json` (proposed file name, owner to confirm) in the project root, written by `new`) with the template of the installed CLI and print one `warn` record `template.drift` per file whose template content changed, without modifying anything. Test: create a project, change one template file in a test CLI build, run `check --template-drift` and assert one `template.drift` warning per changed file, and compare checksums of the project before and after to assert that nothing changed.
