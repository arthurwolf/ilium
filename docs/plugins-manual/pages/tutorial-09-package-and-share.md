---
layout: page
title: "9. Package and share"
permalink: /pages/tutorial-09-package-and-share/
---

## 9. Package and share

This step packs the project into one `.iliumpkg` file that another person can install. The archive holds the manifest, the components built in steps 7 and 8, the assets, the help topic and the sounds. It also holds an inventory with the length and the SHA-256 of every entry. The person who receives the file sees a review dialog that lists each capability by side before the plugin is enabled.

Packing needs no grant and no code change, and it does not care which language made the components. A package can mix Rust and TypeScript components. The steps below produce an unsigned package, which the platform classes as tier T3. The section "Sign for T2" shows how to add a publisher signature, which makes the package tier T2.

### You will need

- [Tutorial step 7](/pages/tutorial-07-ambient-scene/) finished, with `server.wasm`, `client.wasm` and `scene.wasm` built. `ilium plugin build` writes them to `dist/`.
- [Tutorial step 8](/pages/tutorial-08-typescript/) is optional. A Rust build and a TypeScript build pack the same way.
- The `ilium` command with its `plugin` subcommands. `ilium plugin --help` lists them.
- A WAV (PCM) or OGG Vorbis file for each `contributes.sounds` entry, and a Markdown file for each help topic. A missing or differently formatted file fails the install check.
- Assets stay within the package size cap: 64 MiB for T2 and T3 packages, and 256 MiB for T1 bundled plugins. A larger package is refused with `manifest.asset-size`, as is a font asset over 4 MiB.
- A Linux machine for the person who installs the package when it is T2 or T3. Windows and macOS refuse T2 and T3 packages at install.

### Build the components first

`ilium plugin pack` packs what `ilium plugin build` wrote to `dist/`. Run the build for each project once, in the language you used. Both builds write the same files.

**Rust**

```bash
cd agent-tools
ilium plugin build
```

**TypeScript**

```bash
cd agent-tools-ts
bun install
bun test
ilium plugin build
```

### Commands of this step

This step uses `ilium plugin pack`, `inspect --verify`, `keygen`, `sign`, `trust` and `install`. Every flag, exit code and output record is in [Plugin command-line reference](/pages/reference-plugin-cli/).

### What changed from step 7

- `manifest.json` gains a `description`, which the review dialog shows. The `publisher` field is shown in the dialog too.
- A source manifest may omit `files`. `pack` writes that field, so you never type the lengths or hashes yourself.
- `contributes.help` names the one help topic in `help/`. `contributes.sounds` names the sound file from step 4.
- No Rust or TypeScript code changes. The contract also has manifest keys and grants, shown under "Manifest keys for contributions and grants" below. A plugin that does not use them packs exactly as before.

### The package layout

The package is a deterministic ZIP32 archive. Its entries are stored without compression, the paths are sorted, the timestamps are fixed and no extra fields are written. Two builds of the same files therefore produce the same bytes.

| Entry | Required | Content |
|---|---|---|
| `manifest.json` | yes | The manifest, with the `files` inventory that `pack` writes. |
| `server.wasm`, `client.wasm`, `scene.wasm` | for each declared side | Components that target `ilium:server/base`, `ilium:client/base` or `ilium:anim/base`. |
| `assets/**` | no | Read-only files, visible through the `package` file scope. |
| `help/*.md` | no | Help topics named in `contributes.help`. |
| `sounds/*` | no | Sound files named in `contributes.sounds`. |
| `src/**` | no | Sources for review. The host never loads them. |
| `signature` | T2 only | A detached Ed25519 signature over the canonical bytes of `manifest.json`. |

Every entry except `manifest.json` and `signature` must appear in `files`, with its length in bytes and its SHA-256 in lowercase hexadecimal. Each component also carries an `ilium:api-version` section. Its version must equal the manifest `api` value.

### Manifest

The source manifest in the project omits `files`:

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "com.example.agent-tools",
  "name": "Agent tools",
  "version": "0.1.0",
  "publisher": "Example Developer",
  "description": "Agent badges, a TODO scanner, a notes pane and an ambient scene.",
  "server": {
    "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": ["job-runner"]
  },
  "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": ["views"] },
  "scene": { "entry": "scene.wasm", "world": "ilium:anim/base@0.1.0", "exports": [] },
  "capabilities": {
    "server": ["agent.read", "agent.control", "tree.read", "tree.decorate", "notify.desktop",
      "sound.play", "fs.read:project", "jobs.run"],
    "client": ["agent.read", "tree.read", "tree.metadata", "ui.panels", "editor.edit",
      "topics.subscribe:com.example.agent-tools/*", "panes.create", "panes.provide-kind"],
    "scene": []
  },
  "contributes": {
    "help": [{ "id": "agent-tools", "file": "help/agent-tools.md", "title": "Agent tools" }],
    "sounds": [{ "id": "finished", "file": "sounds/finished.wav" }]
  }
}
```

The packed `manifest.json` inside the archive gains the inventory. The entries are sorted by path:

```json
"files": [
  { "path": "client.wasm", "length": 524288, "sha256": "<64 lowercase hex digits>" },
  { "path": "help/agent-tools.md", "length": 2048, "sha256": "<64 lowercase hex digits>" },
  { "path": "scene.wasm", "length": 262144, "sha256": "<64 lowercase hex digits>" },
  { "path": "server.wasm", "length": 1048576, "sha256": "<64 lowercase hex digits>" },
  { "path": "sounds/finished.wav", "length": 40960, "sha256": "<64 lowercase hex digits>" }
]
```

The lengths and hashes shown here are placeholders. Your build writes the real values.

Points to notice:

- The help topic gets a global id, `com.example.agent-tools/agent-tools`. Topic ids always start with the plugin id.
- `description` and `publisher` appear in the review dialog. The dialog does not show the code.
- The `files` entry is required in a packed manifest only. The source manifest may leave it out.

### Manifest keys for contributions and grants

Contribution keys and grants need nothing special at pack time, because `pack` hashes whatever the manifest names. This fragment adds a tree toolbar button and a status bar segment to the source manifest. Both belong to the `ui.chrome` grant, which goes into the client list. The command id refers to a command that the plugin declares under `contributes.commands`, as in step 2.

```json
{
  "capabilities": {
    "client": ["ui.panels", "ui.chrome", "notify.toast"]
  },
  "contributes": {
    "tree_toolbar": [
      { "id": "scan", "icon": "search", "label": "Scan TODO", "command": "com.example.agent-tools/scan",
        "caption": "TODO" }
    ],
    "status_segments": [
      { "id": "todo-count", "priority": 40, "max_width": 16 }
    ]
  }
}
```

Fixed caps apply per plugin: 4 `tree_toolbar` entries and 2 `status_segments`. More than the cap refuses the package with `manifest.contribution-limit`. The key `limits_tier: "first-party"` is accepted only for bundled T1 plugins whose id starts with `ilium.`; any other package that sets it is refused with `manifest.limits-tier` (decision D12). A third-party package therefore never sets it.

Many grants are high risk. The full set is every row marked **H** in [Capabilities](/pages/reference-capabilities/); it includes `agent.control` from the example above and, among the newer grants, `terminal.history`, `terminal.input-lock`, `terminal.raw-input`, `panes.lifecycle`, `tree.restructure`, `sessions.cross`, `process.long-lived`, `agent.transcript`, `settings.read-kernel:api` and `settings.write-kernel:<tab-id>`. High-risk grants always need an explicit choice, even for a T1 plugin, and the dialog draws them in the warning style.

### The path from project to install

<figure>
<svg viewBox="0 0 400 320" role="img" aria-label="Steps from pack to an enabled plugin">
  <defs>
    <marker id="pkg-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="40" y="8" width="320" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="26" font-size="12" text-anchor="middle" fill="currentColor">1. Pack</text>
  <text x="200" y="42" font-size="10" text-anchor="middle" fill="currentColor">writes the .iliumpkg into dist/</text>
  <path d="M200 52 L200 72" fill="none" stroke="currentColor" marker-end="url(#pkg-arrow)"/>
  <rect x="40" y="72" width="320" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="5 3"/>
  <text x="200" y="90" font-size="12" text-anchor="middle" fill="currentColor">2. Sign (T2 only, optional)</text>
  <text x="200" y="106" font-size="10" text-anchor="middle" fill="currentColor">ilium plugin sign adds the signature entry</text>
  <path d="M200 116 L200 136" fill="none" stroke="currentColor" marker-end="url(#pkg-arrow)"/>
  <rect x="40" y="136" width="320" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="154" font-size="12" text-anchor="middle" fill="currentColor">3. Install</text>
  <text x="200" y="170" font-size="10" text-anchor="middle" fill="currentColor">checks the inventory before any code loads</text>
  <path d="M200 180 L200 200" fill="none" stroke="currentColor" marker-end="url(#pkg-arrow)"/>
  <rect x="40" y="200" width="320" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="218" font-size="12" text-anchor="middle" fill="currentColor">4. Review dialog</text>
  <text x="200" y="234" font-size="10" text-anchor="middle" fill="currentColor">capabilities per side, HIGH rows unchecked</text>
  <path d="M200 244 L200 264" fill="none" stroke="currentColor" marker-end="url(#pkg-arrow)"/>
  <rect x="40" y="264" width="320" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="200" y="282" font-size="12" text-anchor="middle" fill="currentColor">5. Enabled</text>
  <text x="200" y="298" font-size="10" text-anchor="middle" fill="currentColor">a T3 package carries a dev badge</text>
</svg>
<figcaption>Each step reads the result of the step before it. A T3 package skips step 2 and goes from pack to install.</figcaption>
</figure>

### Pack and verify

Run the pack command from the project folder:

```bash
ilium plugin pack
```

The command prints one line per event. For this package the output has this shape:

```jsonl
{"type":"progress","step":"pack","message":"Computing the files inventory of 5 entries"}
{"type":"artifact","path":"dist/com.example.agent-tools-0.1.0.iliumpkg","sha256":"<hex>","bytes":187941}
{"type":"summary","duration_ms":412,"warnings":0,"errors":0}
{"type":"result","ok":true,"exit_code":0}
```

The `artifact` line gives the path relative to the project folder, the SHA-256 of the archive and its size in bytes. Use `--out <file>` to write the archive somewhere else.

To check the archive without installing it, run the verify command. It recomputes every hash of the package and compares it with the `files` list:

```bash
ilium plugin inspect dist/com.example.agent-tools-0.1.0.iliumpkg --verify
```

A clean package prints no `diff` record and exits with 0. A changed file prints one `diff` record per difference, and the command exits with 1 and the code `inspect.hash-mismatch`. The command works offline and needs no running Ilium.

### Sign for T2 (optional)

A T2 package is signed by a publisher key that the user trusts. It runs in the same sandbox as a T3 package, and the review dialog shows its signature state. Create the key pair once for each publisher, and keep the private key outside the project tree:

```bash
ilium plugin keygen --publisher-id com.example --publisher "Example Developer" --out ~/keys/example
ilium plugin sign dist/com.example.agent-tools-0.1.0.iliumpkg --key ~/keys/example/publisher.key
```

The `--publisher` value must match the `publisher` field of the manifest, and the plugin `id` must start with the publisher id and a dot (`com.example.`). Otherwise `sign` stops with `sign.publisher-mismatch`, or the install stops with `manifest.publisher-mismatch` or `manifest.id-publisher`.

Sign the archive last. A later `pack` writes a new archive without a signature, so the signature must be added again. The person who installs the package needs your `publisher.pub` file. They add it once:

```bash
ilium plugin trust add ~/keys/example/publisher.pub
ilium plugin install dist/com.example.agent-tools-0.1.0.iliumpkg
```

If the key is not trusted yet, the install dialog offers `Trust publisher` and shows the key fingerprint. Declining refuses the install.

### Review dialog

The dialog appears before the plugin is enabled. It shows the name, the version, the publisher, the package state and the description. Below that it lists one row per capability, grouped by side. A row with a HIGH label needs an explicit choice from the user.

The dialog is the canonical review dialog of [Security](/pages/security/#verifying-a-package). This is the same dialog for an unsigned package.

<figure>
<pre class="term"><span class="t-hi">╭─ Install com.example.agent-tools 0.1.0 ──╮</span>
│ Name (self-declared): Example Developer      │
│ Unsigned T3 · dev badge after install        │
│                                              │
│ <span class="t-dim">Server</span>                                       │
│ [x] Read agent events ………………… agent.read     │
│ [ ] <span class="t-warn">HIGH</span> Control agents … agent.control      │
│ [x] Read the workspace tree …… tree.read     │
│ [x] Read project files …… fs.read:project    │
│ <span class="t-dim">Client</span>                                       │
│ [x] Show panels …………………………… ui.panels        │
│ [ ] <span class="t-warn">HIGH</span> Read the agent transcript           │
│     agent.transcript                         │
│                                              │
│ <span class="t-hi"> Esc </span> Cancel   <span class="t-hi"> Enter </span> Install               │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The review dialog for an unsigned package. HIGH rows start unchecked. The rows are shortened for the page, and the real dialog lists every grant.</figcaption>
</figure>

Points to notice:

- Normal rows are checked by default. HIGH rows are unchecked, and the user checks each one. `agent.control` is HIGH in this example.
- Keys: Up and Down move between rows, Space toggles a row, Enter activates the focused button and Escape cancels.
- A declined capability stays unchecked and dimmed. The plugin still loads without it. Its calls for that capability return `not-permitted`, and each first refusal writes a warn line to the log.
- Grants are per side. `agent.read` on the server and `agent.read` on the client are two separate rows.
- The high-risk grants (`agent.transcript` above) are drawn the same way as `agent.control`: unchecked, with the word HIGH.
- `Details` shows the description, the publisher key, the number and size of the files, the requested limits and the contributions.

Some capabilities ask again at first use, because the grant alone does not name the target. These are separate small prompts and not part of the install dialog:

- `open-external` asks once for each target origin, and the answer is remembered for the plugin (SEC-043).
- `http:setting:<key>` hosts, which the plugin reads from its own setting, ask for each new host before the first request (SEC-074).
- Geocoding through `location.geocode` shows a consent notice on first use, because place names go to the host's geocoding provider (see [Design decisions](/pages/design-decisions/), D14).

<figure>
<pre class="term"><span class="t-hi">╭  ≡ ● · Open link─────────────────────────────────────╮</span>
│ com.example.agent-tools wants to open                │
│ https://docs.example.com/aurora in your browser.     │
│                                                      │
│        <span class="t-dim">N Cancel</span>        Y Open                        │
│ <span class="t-dim">Y/N answer · Enter confirm · Esc cancel</span>              │
╰──────────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The confirmation for <code>open-external</code>, in the confirm style of the real client with the <code>Y</code> and <code>N</code> key hints.</figcaption>
</figure>

Install the unsigned package with the flag that the T3 tier needs:

```bash
ilium plugin install dist/com.example.agent-tools-0.1.0.iliumpkg --allow-unsigned
```

Without `--allow-unsigned`, the install stops before the dialog.

### After install

The plugin appears in Settings > Plugins. A T3 plugin keeps a `dev` badge there. Read the first lines of its log to confirm that the archive loads:

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs com.example.agent-tools --tail 3
<span class="t-dim">2026-10-11 09:14:02</span> <span class="t-ok">info </span>  com.example.agent-tools activated
<span class="t-dim">2026-10-11 09:14:03</span> <span class="t-ok">info </span>  com.example.agent-tools ready
<span class="t-dim">2026-10-11 09:14:04</span> <span class="t-warn">warn </span>  com.example.agent-tools refused notify: not-permitted
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The warn line shows a declined grant. A clean log has no warn or error lines from the first activation.</figcaption>
</figure>

Revoke a grant later with Settings > Plugins > com.example.agent-tools > Capabilities, or with `ilium plugin grants com.example.agent-tools --revoke notify.desktop`.

### Share

A registry is optional (decision D27). Without one, you share the file. With one, `ilium plugin publish <file> --registry <url>` uploads a signed package to a registry ([Distribution commands](/pages/reference-plugin-distribution-cli/)). The package is a file, and you pass it on by any channel you choose, such as e-mail, a shared folder or a file host. If the package is signed, send `publisher.pub` with it, and send the SHA-256 of the package and the key fingerprint by another route. The registry protocol is in [Registry reference](/pages/reference-registry/), and the steps in [Publishing guide](/pages/guide-publishing/). The full release routine, with changelog, support and key loss, is in [Publishing and support](/pages/guide-publishing-and-support/). Before every release you MUST run `ilium plugin check`, `lint` and `test`. [AUT-320]

- A newer version of an installed id is an update. The dialog shows only the new capabilities.
- A lower version is refused unless the person adds `--allow-downgrade`.
- Before you share, install the archive once on your own machine and read the log.

### Edge cases and errors

Each install check that fails writes one JSONL `error` line with its code and, when known, the file. The install stops before any code loads:

```jsonl
{"type":"error","code":"manifest.file-hash","message":"SHA-256 differs","file":"server.wasm"}
```

| Code or result | Cause | Fix |
|---|---|---|
| `manifest.file-unlisted` | An entry is in the archive but not in `files`. | Run `pack` again. |
| `manifest.file-missing` | A listed file is absent, for example a declared `scene.wasm` that was never built, or a sound or help file that the manifest names. | Build the component or add the file, then pack again. |
| `manifest.file-length`, `manifest.file-hash` | A file changed after `pack`. | Pack again, and sign again if the package is T2. |
| `manifest.api-version` | A component was built against an older API, so its `ilium:api-version` differs from `api`. | Rebuild with the current SDK. |
| `manifest.signature-invalid` | The T2 signature does not verify. | Sign the final archive again with the right key. |
| `manifest.publisher-mismatch` | The manifest `publisher` differs from the key's recorded publisher. | Use the same name in `keygen` and in the manifest. |
| `manifest.grant-tier` | A T2 or T3 package asks for `process.run:sh`. The shell grant is for T1 bundled plugins only. | Remove the grant, or use a fixed program name. |
| `manifest.id-reserved` | The id starts with `ilium.`, which first-party plugins use. | Choose your own reverse-DNS id. |
| `manifest.unknown-key` | A manifest key is outside the schema, often a typo. | Remove or correct the key. |
| `manifest.sound-id-taken` | A sound id repeats, or it equals `chirping`, the bundled sound. | Give the sound a unique id. |
| `manifest.asset-size` | Package assets over 64 MiB (256 MiB for T1), or a font over 4 MiB. | Drop or shrink assets. |
| `manifest.limits-tier` | `limits_tier: "first-party"` in a package that is not a bundled T1 `ilium.` plugin. | Remove the key. |
| `manifest.contribution-limit`, `manifest.contribution-target` | More entries of a contribution key than its cap, or an `applies_to` or `slot` that names an unknown target or another plugin's slot. | Reduce the entries or name an own slot. |
| `manifest.when-key`, `manifest.visible-when`, `manifest.menu-depth` | An unknown `when` key, a `visible_when` that points to another plugin's key or forms a cycle, or a static menu nested deeper than 2. | Correct the key or flatten the menu. |
| `manifest.catalog-unknown`, `manifest.settings-source` | A `source` that names no known catalogue, or a row with both `options` and `source`, or neither. | Declare the catalogue or fix the row. |
| `manifest.row-format`, `manifest.cli-field`, `manifest.client-request-unknown`, `manifest.scene-prerender` | An invalid `item_format`, `steps` ladder or `special_values`, a bad `cli` field, a `client_requests` entry that the other side does not match, or `prerender` values out of range. | Correct the named key path. |
| Install refused, no flag | An unsigned package was installed without `--allow-unsigned`. | Sign the package, or add the flag. |
| Install refused, version | A lower version was installed over a newer one. | Add `--allow-downgrade` if the downgrade is intended. |
| Install refused, platform | A T2 or T3 package on Windows or macOS. | Install on Linux, or use a T1 bundled plugin. |

### See also

- [Tutorial step 1](/pages/tutorial-01-agent-badge/) is where the project started.
- [Tutorial step 7](/pages/tutorial-07-ambient-scene/) is the last code step before packing.
- [Tutorial step 8](/pages/tutorial-08-typescript/) shows the TypeScript build that packs the same way.
- [Packages](/pages/packages/) and [Manifest reference](/pages/reference-manifest/) list every manifest key.
- [Reference: errors and limits](/pages/reference-errors-and-limits/) lists every manifest error code.
- [Publishing and support](/pages/guide-publishing-and-support/) covers releases, changelogs, keys and support.
- [Security](/pages/security/) explains the trust tiers, the sandbox and the install checks.
- [Help topics](/pages/capability-help-read/) and [Notifications](/pages/api-notifications/) cover the help and sound files.

API checked against `ilium:plugin@0.1.0` including contribution keys and grants.
