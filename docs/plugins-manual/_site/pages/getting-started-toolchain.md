# Set up a toolchain

## Set up a toolchain

This page explains what you install to build a plugin in Rust or TypeScript, and what each `ilium plugin` command does with your source. A plugin is built as a WebAssembly component that targets WASI Preview 2. You install the language tools once. The `ilium` binary runs the rest of the pipeline: it compiles the source, wraps the result as a component, and later packs the components into a `.iliumpkg` file that the host loads.

The `ilium plugin` commands are part of the `ilium` binary. Installing Ilium installs them. There is no separate tool to add.

### What you need

| Requirement | Rust plugin | TypeScript plugin |
|---|---|---|
| Ilium with the `ilium plugin` commands | Yes, from the Ilium install | Yes, from the Ilium install |
| Language toolchain | Rust stable 1.90 or later, with the `wasm32-wasip2` target | Bun 1.2 or later |
| Other global tools | None. The component step is built into `ilium` | None. `bun install` fetches the rest |
| Grants | None | None |

Grants apply to a plugin, not to its build. You grant capabilities when you install the package on your machine, and the review dialog lists them. Read [What a plugin is](/pages/getting-started-overview/) first if you have not seen the three sides of a plugin.

Plugin builds run on your own machine. You do not need a remote service to build or install a plugin.

### Install the Rust toolchain

**Rust**

```bash
rustup target add wasm32-wasip2
rustc --version
```

`rustc --version` must print 1.90.0 or later. The build needs no other tool. The component step uses the `wit-component` library linked into `ilium`, so `wasm-tools` does not need to be on your path.

### Install the TypeScript toolchain

Bun is the only global tool for TypeScript. Inside the plugin directory, run `bun install`. It reads the `@ilium/plugin-sdk` dependency and installs the exact versions of `componentize-js` and `jco` that the SDK pins. Nothing is installed globally.

**TypeScript**

```bash
bun --version
bun install
```

The build reads the WIT files from `node_modules/@ilium/plugin-sdk/wit/`. It never copies them into your project. You do not call `componentize-js` or `jco` yourself.

### The ilium plugin commands

The full list of commands, flags, exit codes and output records is [Plugin command-line reference](/pages/reference-plugin-cli/). The four commands you use first:

| Command | What it does |
|---|---|
| `ilium plugin new <dir> --id <plugin-id> --lang <rust or typescript> --template <name>` | Writes a template project. `--side server`, `client`, `both` or `scene` is an alias for the template of that name. The eight templates and their files are in [Project templates](/pages/reference-project-templates/). |
| `ilium plugin build [<dir>]` | Compiles the project and writes the components to `<dir>/dist/`. `--side <side>` builds one side, `--debug` keeps debug information, `--watch` rebuilds on each change. |
| `ilium plugin dev [<dir>]` | Builds, installs as T3, activates, and rebuilds and reloads after each change. |
| `ilium plugin pack [<dir>]` | Writes `<dir>/dist/<id>-<version>.iliumpkg`. It refuses a debug build and does not sign. |

Other commands: `check` and `lint` validate a project, `test` runs the tests, `inspect` and `doctor` show the state of a package or the machine, and `logs`, `trace` and `profile` show a running plugin ([Debugging tools](/pages/reference-debugging-tools/)). `ilium plugin doctor` is the quick way to verify this page: it reports each missing tool with the fix.

`<dir>` defaults to the current directory. The build detects the language from the files. A `Cargo.toml` means Rust, and a `package.json` means TypeScript. If both files exist, or neither does, the build stops with an error (see the error table below). `--debug` keeps DWARF debug information, so traps show `file:line` in the plugin log. Read [Debugging](/pages/debugging/) before you use it.

A person who only installs and manages plugins does not need this toolchain. They use [Plugins for users](/pages/user-plugins-overview/) and [Install and manage plugins](/pages/user-install-and-manage/). To open the Plugins tab, press `Ctrl+B` then `:` and choose Plugins ([Settings plugin](/pages/core-plugin-settings/#the-plugins-tab)).

The install, signing and trust commands (`ilium plugin install`, `keygen`, `sign` and `trust`) are covered in [Package and share](/pages/tutorial-09-package-and-share/) and defined in [Distribution commands](/pages/reference-plugin-distribution-cli/).

### The sides and the toolchain

`--side both` writes a server and a client entry in one project, and `--side scene` writes a scene entry. The component step still produces one component per side. A plugin sets `"api": "ilium:plugin@0.1.0"` in the manifest, and the build checks the component against that version. The host refuses a plugin built for a newer minor than it supports.

### Output lines

Every command prints one JSON object per line, each with a `type` field. A script can read the output without parsing free text.

| Type | Fields | Printed by |
|---|---|---|
| `progress` | `step` (`compile`, `bundle`, `component` or `pack`), `message` | all commands |
| `warning` | `code`, `message` | all commands |
| `error` | `code`, `message`, and `file` and `line` when known | all commands |
| `artifact` | `path`, `sha256`, `bytes` | `build`, `pack` |
| `summary` | `duration_ms`, `warnings`, `errors` | all commands |
| `result` | `ok` (`true` or `false`) | all commands |
| `reload` | `plugin`, `version`, `restored_bytes` | `dev` |
| `log` | `plugin`, `side` (`server`, `client` or `scene`), `level`, `message` | `dev` |

### Worked example: a server plugin

This example creates a server-side plugin with no exports. It logs once on activation. The two languages use the same command with a different `--lang`.

**Rust**

```bash
ilium plugin new agent-tools --id example.agent-tools --lang rust --side server \
  --name "Agent tools"
cd agent-tools
ilium plugin build
```

**TypeScript**

```bash
ilium plugin new agent-tools --id example.agent-tools --lang typescript --side server \
  --name "Agent tools"
cd agent-tools
bun install
ilium plugin build
```

The command writes three files in either language. `manifest.json` declares the server side:

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.agent-tools",
  "name": "Agent tools",
  "version": "0.1.0",
  "publisher": "Example Developer",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": []
  }
}
```

**Rust**: `Cargo.toml` builds a library for the component and depends on the SDK:

```toml
[package]
name = "agent-tools"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
ilium-plugin-sdk = "0.1"
```

**Rust**: `src/lib.rs` holds the plugin. The `#[ilium_plugin]` attribute generates the component glue, and the
default `Lifecycle` methods cover activation:

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct AgentTools;

impl Lifecycle for AgentTools {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        log::log(Level::Info, &format!("hello from {}", context.plugin));
        Ok(())
    }
}
```

**TypeScript**: `package.json` pins the SDK version exactly, and `src/server.ts` is the entry module:

```json
{
  "name": "agent-tools",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "dependencies": {
    "@ilium/plugin-sdk": "0.1.0"
  }
}
```

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate(context) {
    log('info', `hello from ${context.plugin}`);
  },
  onEvents() {
    return [];
  },
});
```

### Worked example: a client plugin

This example creates a client-side plugin. In TypeScript it needs `bun install` once before the first build.

**TypeScript**

```bash
ilium plugin new agent-tools --id example.agent-tools --lang typescript --side client \
  --name "Agent tools"
cd agent-tools
bun install
ilium plugin build
```

**Rust**

```bash
ilium plugin new agent-tools --id example.agent-tools --lang rust --side client \
  --name "Agent tools"
cd agent-tools
ilium plugin build
```

The `package.json` of the TypeScript project is the one shown above. The manifest of a client plugin names the client world:

```json
"client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": [] }
```

**TypeScript**: `src/client.ts` is the entry module. `ilium plugin build` bundles it with Bun and treats `ilium:*` and
`wasi:*` as external imports:

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate(context) {
    log('info', `hello from ${context.plugin}`);
  },
  onEvents() {
    return [];
  },
});
```

**Rust**: `src/lib.rs` of the client plugin differs from the server plugin only in the side name:

```rust
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct AgentTools;

impl Lifecycle for AgentTools {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        log::log(Level::Info, &format!("hello from {}", context.plugin));
        Ok(())
    }
}

impl Handler for AgentTools {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}
```

### Build, watch and reload

`ilium plugin build` writes one component per declared side into `dist/`, and prints one `artifact` line for
each. The figure shows a successful build of the server plugin, which writes `dist/server.wasm`. Hashes
are shortened.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin build
{"type":"progress","step":"compile","message":"Compiling agent-tools"}
{"type":"progress","step":"component","message":"Writing server.wasm"}
{"type":"artifact","path":"dist/server.wasm","sha256":"3f9a..","bytes":412806}
{"type":"summary","duration_ms":8120,"warnings":0,"errors":0}
{"type":"result","ok":true}
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Hashes are shortened, and the timings are examples.</figcaption>
</figure>

`ilium plugin dev` runs the same build, then activates the plugin and watches the project. It starts a rebuild
300 ms after the last change. A failed rebuild keeps the running instance, prints the compiler error, and
sends no `deactivate`. A successful rebuild deactivates the old instance with the reason `upgrade`, activates
the new build, and passes the bytes that the old `deactivate` returned as `restored`. A plugin that returns
no bytes reports `restored_bytes` as 0.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin dev
{"type":"progress","step":"compile","message":"Compiling agent-tools"}
{"type":"reload","plugin":"example.tools","version":"0.1.0","restored_bytes":0}
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The reload follows a change to <code>src/lib.rs</code>. The plugin
id is shortened to fit the figure. Each line is one JSON object, as in the table above.</figcaption>
</figure>

### The build pipeline

Source becomes a component, a component goes into a package, and the host loads the package at install time.
The package format is `.iliumpkg`, a deterministic ZIP archive that holds `manifest.json`, one `.wasm` file per
declared side, and an inventory of every file. Each `.wasm` file is a component that targets the world named
in the manifest.

<figure>
<svg viewBox="0 0 420 130" role="img" aria-label="Toolchain pipeline from source to host">
  <title>Toolchain pipeline</title>
  <desc>Source is compiled into components. The pack step writes a package. The host loads the package at install.</desc>
  <defs>
    <marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M 0 0 L 10 5 L 0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="8" y="52" width="80" height="36" rx="4" fill="none" stroke="currentColor"/>
  <text x="48" y="74" font-size="11" text-anchor="middle" fill="currentColor">Source</text>
  <rect x="116" y="52" width="100" height="36" rx="4" fill="none" stroke="currentColor"/>
  <text x="166" y="74" font-size="11" text-anchor="middle" fill="currentColor">Components</text>
  <rect x="244" y="52" width="84" height="36" rx="4" fill="none" stroke="currentColor"/>
  <text x="286" y="74" font-size="11" text-anchor="middle" fill="currentColor">.iliumpkg</text>
  <rect x="356" y="52" width="56" height="36" rx="4" fill="none" stroke="currentColor" stroke-dasharray="5 3"/>
  <text x="384" y="74" font-size="11" text-anchor="middle" fill="currentColor">Host</text>
  <line x1="88" y1="70" x2="114" y2="70" stroke="currentColor" marker-end="url(#arrow)"/>
  <line x1="216" y1="70" x2="242" y2="70" stroke="currentColor" marker-end="url(#arrow)"/>
  <line x1="328" y1="70" x2="354" y2="70" stroke="currentColor" marker-end="url(#arrow)"/>
  <text x="116" y="38" font-size="10" fill="currentColor">ilium plugin build</text>
  <text x="244" y="38" font-size="10" fill="currentColor">ilium plugin pack</text>
  <text x="8" y="112" font-size="10" fill="currentColor">ilium plugin dev rebuilds and reloads on each change.</text>
</svg>
<figcaption>Build writes the components, pack writes the package, and the host reads the package at install.
The dashed box is the running host, which is not part of the toolchain.</figcaption>
</figure>

### How a development plugin looks to the user

While `ilium plugin dev` runs, the plugin is installed as a T3 package. Settings > Plugins lists it with a `dev` badge next to its id. The user can open its details, review the grants, and read its log from the same list.

<figure>
<pre class="term">╭  ≡ ● · Settings · Plugins ───────────────────╮
│ <span class="t-hi">Plugin</span>              Tier  State   Grants     │
│ ──────────────────────────────────────────   │
│ example.agent-tools [dev] T3 <span class="t-ok">running</span>  2/2    │
│ ilium.badges               T1 <span class="t-ok">running</span>  3/3   │
│ ilium.tasks                T1 <span class="t-dim">disabled</span> 0/1   │
│                                              │
│ <span class="t-dim">↑/↓ move · Enter details · Space enable</span>      │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The panel uses a rounded border and the chrome title. The header row has a <code>─</code> rule under it, and the hint line below the rows uses <code>·</code> separators.</figcaption>
</figure>

### Pinned versions

| Component | Version | Where it comes from |
|---|---|---|
| Rust | Stable 1.90 or later | `rustup` |
| Rust target | `wasm32-wasip2` | `rustup target add` |
| Bun | 1.2 or later | Installed by you, the only global TypeScript tool |
| `@ilium/plugin-sdk` | 0.1.0 | `package.json` in the TypeScript project |
| `componentize-js`, `jco` | Pinned by the SDK | `bun install` |
| `ilium-plugin-sdk` | 0.1 | `Cargo.toml` in the Rust project |
| `wit-bindgen`, `wit-component` | Pinned by the SDK to match the host | Cargo, through the SDK |

### Errors

Build and dev stop with an `error` line and exit with status 1. The `file` and `line` fields are set when a
file is involved. Manifest errors, such as an unknown capability name, use the `manifest.*` codes listed in
[Packages and manifests](/pages/packages/). Errors from the compiler are printed with the compiler's text.

| Code | Cause | Fix |
|---|---|---|
| `toolchain.rust-target` | The `wasm32-wasip2` target is not installed | Run `rustup target add wasm32-wasip2` |
| `toolchain.rust-version` | `rustc` is older than 1.90 | Run `rustup update stable` |
| `toolchain.bun-version` | Bun is older than 1.2 | Upgrade Bun |
| `toolchain.sdk-missing` | `node_modules/@ilium/plugin-sdk` is not installed | Run `bun install` in the project |
| `toolchain.language-ambiguous` | Both `Cargo.toml` and `package.json` exist | Keep one of the two files |
| `toolchain.language-unknown` | Neither `Cargo.toml` nor `package.json` exists | Run the command in the project directory |

<div class="note">
<p>Under <code>dev</code>, a failed rebuild keeps the running instance and sends it no <code>deactivate</code>.
Fix the error, and the next change triggers a new build.</p>
</div>

### Check the setup

Run `ilium plugin build` in the template project. A successful build prints `artifact` lines for each side
and ends with `"ok":true` on the `result` line. If the build fails on the target, check the `rustup` output
first. A TypeScript build that fails with `toolchain.sdk-missing` needs `bun install`, not a global install.

### See also

- [Your first plugin in five minutes](/pages/getting-started-first-plugin/) builds the template with this toolchain.
- [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/) describe the imports used above.
- [Packages and manifests](/pages/packages/) and [Manifest reference](/pages/reference-manifest/) describe the files the build reads.
- [Debugging](/pages/debugging/) covers `--debug` and the plugin log.
- [Plugin command-line reference](/pages/reference-plugin-cli/) and [Project templates](/pages/reference-project-templates/) define every command and template file.
- [Package and share](/pages/tutorial-09-package-and-share/) covers `pack`, signing and installing.

API checked against `ilium:plugin@0.1.0`.
