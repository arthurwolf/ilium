# Security model

## Security model

A plugin gets only the rights its manifest asks for and the user grants. First-party code runs inside the Ilium process. Third-party code runs in a separate helper process that the operating system confines. Every host call is checked against the grants in force at that moment. When a grant is revoked, the next call fails, and a call that was already running fails with `revoked`.

This page is the overview of the security rules and the entry point to the detailed pages. Statements that carry an id such as [SEC-013] are normative: they use MUST, SHOULD and MAY as in RFC 2119 and are collected in [Security and policy requirements](/pages/reference-requirements-security-policy/). The pages [Threat model](/pages/reference-threat-model/) and [Sandbox reference](/pages/reference-sandbox/) say what is defended and how.

**[SEC-013]** Every host call MUST be checked against the grants in force at that moment, for every tier except T0. A grant that is missing, declined, revoked or forbidden by policy MUST refuse the call with the form given under "What a call returns without a grant".

**[SEC-014]** A third-party package MUST never run in the Ilium process. If the sandbox cannot be built, the package does not load ([SEC-055]).

### Security pages

| Page | What it defines |
|---|---|
| [Threat model](/pages/reference-threat-model/) | Assets, attackers, mitigations, non-goals, side channels |
| [Sandbox reference](/pages/reference-sandbox/) | Helpers, namespaces, seccomp, cgroups, file scopes, escape hatches |
| [Network egress](/pages/reference-network-egress/) | Address checks, redirects, TLS, the HTTP API listener, the egress inventory |
| [Grant review and consent](/pages/reference-grant-review-and-consent/) | The install pipeline, dialogs, stores, revocation |
| [Trust and supply chain](/pages/reference-trust-and-supply-chain/) | Signatures, key pinning, blocklists, archive rules |
| [Admin policy](/pages/reference-admin-policy/) and [Enterprise deployment](/pages/guide-enterprise-deployment/) | Policy file and how to use it |
| [Audit log](/pages/reference-audit-log/) | Security records |
| [Secrets](/pages/reference-secrets/) | Storage and redaction of secrets |
| [Extension security](/pages/reference-extension-security/) | Hooks, replacement, project trust |
| [Security response](/pages/reference-security-response/) | Reporting, takedown, safe mode |
| [Security testing](/pages/reference-security-testing/) | Fuzzing and the test checklist |
| [Plugin security and privacy for users](/pages/user-plugin-security-and-privacy/) | The end-user view |

### You will need

- The capability table, which lists every grant and its risk mark: [Capability table](/pages/reference-capabilities/).
- The manifest reference, for the `capabilities` object, the `signature` entry, the `publisher` key and `limits_tier`: [Manifest reference](/pages/reference-manifest/).
- The package layout and install checks: [Packages](/pages/packages/).
- A language: the rules are the same in Rust and TypeScript. Each example below is shown in both, with the SDK paths of the [Rust SDK](/pages/rust-sdk/) and the [TypeScript SDK](/pages/typescript-sdk/).
- A Linux machine for any T2 or T3 package. The sandbox runs on Linux only ([SEC-057]).

## Trust tiers

| Tier | What it is | Where it runs | Grants |
|---|---|---|---|
| T0 kernel | Native code built into Ilium | In the server or client process | Everything |
| T1 bundled | First-party WebAssembly embedded in the release | In-process Wasmtime | The manifest ceiling, granted automatically. High-risk (H) grants are asked for at first activation, and the question returns at each server start until the user has answered Not now three times ([SEC-111]) |
| T2 signed | Packages signed by a publisher key the user trusts | Sandboxed plugin helper, mandatory | A review dialog. Grants are remembered |
| T3 local and dev | Unsigned local packages | Sandboxed plugin helper, mandatory | The same review dialog, and a persistent `dev` badge |

In-process Wasmtime is trusted only for T1 code that the project builds itself. A third-party package never runs in that process. A T3 package is installed with `--allow-unsigned` or through `ilium plugin dev`.

The tier also decides three other things. These rules are enforced when the package is installed, not when the plugin runs.

| Rule | T1 bundled | T2 signed | T3 local |
|---|---|---|---|
| `limits_tier: "first-party"` (8 tabs, 2,048 value keys, 64 rows per section, 1,024 list items, 512 choice options) | Only when the id starts with `ilium.` | Refused: `manifest.limits-tier` | Refused: `manifest.limits-tier` |
| Package assets (`package_assets_mib`) | Up to 256 MiB | Up to 64 MiB | Up to 64 MiB |
| `sessions.cross` (commands in other running sessions) | Allowed with an explicit grant | Allowed with an explicit grant | Refused: `manifest.grant-tier` |
| `process.run:sh` | Allowed | Refused: `manifest.grant-tier` | Refused: `manifest.grant-tier` |

The first row is decision D12 and the second is D24. The `sessions.cross` row is D16. The `process.run:sh` row is [SEC-031]. All three are final decisions; see [Design decisions](/pages/design-decisions/).

<figure>
<svg viewBox="0 0 360 170" role="img" aria-label="Sandbox boundary between the kernel and third-party plugins">
  <title>Where each tier runs</title>
  <desc>The kernel and T1 plugins share one process. T2 and T3 plugins run in a helper behind a sandbox boundary.</desc>
  <defs>
    <marker id="tier-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="6" y="6" width="348" height="86" rx="8" fill="none" stroke="currentColor"/>
  <text x="16" y="24" fill="currentColor" font-size="11">Ilium process (T0 kernel, T1 bundled plugins)</text>
  <rect x="20" y="36" width="90" height="42" rx="6" fill="none" stroke="currentColor"/>
  <text x="65" y="60" text-anchor="middle" fill="currentColor" font-size="11">Kernel</text>
  <rect x="160" y="36" width="100" height="42" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="210" y="60" text-anchor="middle" fill="currentColor" font-size="11">T1 plugins</text>
  <rect x="6" y="108" width="348" height="56" rx="8" fill="none" stroke="currentColor" stroke-width="3"/>
  <text x="16" y="124" fill="currentColor" font-size="11">Helper process (sandbox, Linux only)</text>
  <rect x="20" y="130" width="150" height="26" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="95" y="147" text-anchor="middle" fill="currentColor" font-size="10">T2 and T3 plugins</text>
  <text x="186" y="147" fill="currentColor" font-size="10">only granted dirs open</text>
  <path d="M110 57 L158 57" stroke="currentColor" fill="none" marker-end="url(#tier-arrow)"/>
  <path d="M95 128 L95 110" stroke="currentColor" fill="none" marker-end="url(#tier-arrow)"/>
</svg>
<figcaption>T0 and T1 share the Ilium process. T2 and T3 run behind the sandbox boundary, in a helper that sees only the directories the user granted.</figcaption>
</figure>

## The sandbox

Third-party code runs in a helper process that the operating system confines. The full requirements are in [Sandbox reference](/pages/reference-sandbox/); this section is the summary.

- **[SEC-020]** One helper per plugin and side. Plugins never share a helper.
- **[SEC-021]** The helper has its own user, mount, PID, IPC, UTS and network namespaces, no network, no controlling terminal, a rebuilt environment and only the dispatcher channel as an open file ([SEC-022] to [SEC-028]).
- **[SEC-029]** Memory, process count and CPU are limited by a cgroup per helper.
- **[SEC-024]** A seccomp allowlist and `no_new_privs` apply before the first plugin byte is read.
- **[SEC-032]** A child started with `process.run` or `process.spawn` runs in the same confinement with no network, at low priority, under the plugin's CPU quota. Dropping the child handle or stopping the plugin ends the child with SIGTERM, and SIGKILL after 2 s.
- **[SEC-055]** When the sandbox cannot be built, T2 and T3 packages refuse to load with `sandbox.unavailable`. There is no unconfined fallback.

On Windows and macOS a T2 or T3 package is refused at install, and only T1 bundled plugins run (decision D5). The lines below are wrapped to fit the page. macOS shows the same text with its own name.

<figure>
<pre class="term"><span class="t-err">Cannot install example.demo 0.1.0.</span>
Third-party plugins need the plugin
sandbox, which is not available on
Windows yet.
Bundled plugins keep working.
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The install refusal for a T2 package on Windows. On macOS the same text names macOS.</figcaption>
</figure>

## Verifying a package

Before any code runs, the loader checks the package. There is one ordered list of install checks, and each step has its error code. The list is the table in [Grant review and consent](/pages/reference-grant-review-and-consent/#the-install-pipeline) ([SEC-100]). The first failing step stops the install, and each refusal is written as one JSONL `error` line that carries its code. The operational pipeline of 12 steps in [Install, update and uninstall](/pages/reference-install-update-uninstall/#the-install-pipeline) wraps the same checks with locking, staging and commit. The table below says which check belongs to which step of each list.

| Check order | What is checked | Error codes | Step in the 12-step pipeline |
|---|---|---|---|
| 1 to 3 | Archive, layout, API version, file inventory | `package.zip-*`, `manifest.api-version`, `manifest.unknown-key`, `manifest.file-*`, `manifest.asset-size` | 2, 4, 5 |
| 4 | Signature (T2) and publisher match | `manifest.signature-invalid`, `manifest.publisher-mismatch` | 3 |
| 5 and 6 | Blocklist and administrator policy | `package.blocked`, `policy.*` | 6 |
| 7 | Key trust and key pinning | `trust.declined`, `package.publisher-key-changed` | 3 |
| 8 and 9 | Grant strings, tier rules. A grant denied by policy is not an install failure: it is locked off in the dialog | `manifest.grant-*`, `manifest.limits-tier` | 5, 6 |
| 10 | Review dialog | the user cancels | 7 |
| 11 | Commit | `install.write-failed` | 8 to 11 |

- [USR-170] Install, update and the Plugins screen MUST run the checks in the order of the table above, and MUST name the failing step's code in the error. Test: build one package for each failing check, install each, and assert the code and that nothing is written to the store.
- [USR-171] The error text for a refused install MUST begin `Cannot install <name> <version>.` and MUST name the check in plain words after it. Test: assert the first line for a hash failure and for a tier failure.

<figure>
<svg viewBox="0 0 440 130" role="img" aria-label="Install pipeline phases in order">
  <title>Install pipeline</title>
  <desc>Five phases run left to right: archive and layout, inventory, trust, policy and grants, review. A failing phase stops the install.</desc>
  <defs>
    <marker id="check-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <text x="220" y="20" text-anchor="middle" fill="currentColor" font-size="11">Phases run in order. A failure stops the install.</text>
  <rect x="6" y="34" width="76" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="44" y="58" text-anchor="middle" fill="currentColor" font-size="10">Archive</text>
  <rect x="94" y="34" width="76" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="132" y="58" text-anchor="middle" fill="currentColor" font-size="10">Inventory</text>
  <rect x="182" y="34" width="76" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="220" y="58" text-anchor="middle" fill="currentColor" font-size="10">Trust</text>
  <rect x="270" y="34" width="76" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="308" y="58" text-anchor="middle" fill="currentColor" font-size="10">Grants, policy</text>
  <rect x="358" y="34" width="76" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="396" y="58" text-anchor="middle" fill="currentColor" font-size="10">Review</text>
  <path d="M82 54 L94 54" stroke="currentColor" fill="none" marker-end="url(#check-arrow)"/>
  <path d="M170 54 L182 54" stroke="currentColor" fill="none" marker-end="url(#check-arrow)"/>
  <path d="M258 54 L270 54" stroke="currentColor" fill="none" marker-end="url(#check-arrow)"/>
  <path d="M346 54 L358 54" stroke="currentColor" fill="none" marker-end="url(#check-arrow)"/>
  <text x="220" y="100" text-anchor="middle" fill="currentColor" font-size="10">Archive, inventory and grants apply to every tier.</text>
  <text x="220" y="116" text-anchor="middle" fill="currentColor" font-size="10">The signature and key checks apply to T2 only.</text>
</svg>
<figcaption>The install phases in order. The review dialog runs only after every earlier check passes.</figcaption>
</figure>

The review dialog is one dialog for every capability. Each row is a capability, grouped by side. A high-risk row is unchecked until the user checks it. The wording of each row is in the [Capability table](/pages/reference-capabilities/). A package that fills other plugins' extension points shows a separate group, **Adds to other plugins**, a package with `replaces` asks for a separate confirmation, and a package that reads data and sends data out shows the toxic combination row ([SEC-004]). This render is the canonical review dialog: other pages link here and do not redraw it. The dialog is drawn by the client with the same rounded border, grey `…` leaders and footer hint bar as every other Ilium dialog; a plugin never draws or restyles it ([SEC-102]).

<figure>
<pre class="term"><span class="t-hi">╭─ Install example.pane-tools 2.0.0 ───────────╮</span>
│ Name (self-declared): Example Corp           │
│ Signed T2 · key 3F2A91C0 · Tab for Details   │
│                                              │
│ <span class="t-dim">Server</span>                                       │
│ [x] Read terminal text   terminal.read-cells │
│ [ ] <span class="t-warn">HIGH</span> Read scrollback    terminal.history │
│ [ ] <span class="t-warn">HIGH</span> Run rg ………………………………… process.run:rg │
│ [x] Send data to api.example.com ……………… http │
│ <span class="t-dim">Client</span>                                       │
│ [x] Show toasts ……………………………………… notify.toast │
│                                              │
│ <span class="t-warn">This plugin can read your data and send</span>      │
│ <span class="t-warn">data out.</span>                                    │
│                                              │
│ <span class="t-hi"> Esc </span> Cancel   <span class="t-hi"> Enter </span> Install               │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The review dialog of a signed plugin. Focus starts on Cancel.</figcaption>
</figure>

## Grant strings

A grant is written as a name, or as a name and an argument separated by the first colon, for example `fs.read:project`. The name is one or more dot-separated lowercase segments. Later colons belong to the argument, so `services.use:ilium:inference/provider` has the name `services.use`.

| Grant | Argument | Examples |
|---|---|---|
| `fs.read`, `fs.write`, `fs.watch` | Scope name or `path:<template>` | `fs.read:project`, `fs.watch:data`, `fs.read:agent-sessions:gemini` |
| `http` | Host pattern, or `setting:<key>` | `http:api.example.com`, `http:*.example.com`, `http:setting:server_url` |
| `process.run` | Program name or absolute path | `process.run:git`, `process.run:/usr/bin/git` |
| `commands.invoke` | Global command id or prefix | `commands.invoke:ilium.git/*` |
| `services.use`, `services.provide` | Service name without version | `services.use:ilium:inference/provider` |
| `settings.contribute` | One built-in tab id, no `*` | `settings.contribute:terminal` |
| `extensions.provide`, `extensions.contribute` | One global point id `<plugin-id>/<point>`, no `*` | `extensions.contribute:ilium.core.smart-copy/recognisers` |
| `settings.read-kernel` | One built-in tab id, no `*` | `settings.read-kernel:terminal` |

Four argument forms deserve a note: `http:setting:<key>`, `settings.read-kernel:<kernel-tab-id>`, and the agent scopes `agent-sessions:gemini`, `agent-config:gemini` and `agent-config:antigravity` of `fs.read` and `fs.watch`. The agent scopes are read only, and `fs.write:agent-sessions:<agent>` is refused at install with `manifest.grant-scope`.

Grants without an argument, such as `tree.read` or `agent.read`, take none. The [Capability table](/pages/reference-capabilities/) lists every grant.

Each grant has a risk mark. `always` grants are always on. `normal` grants are checked by default in the review dialog. High-risk (H) grants start unchecked, and the user checks each one. H grants include `panes.signal`, `terminal.write`, `agent.read-prompts`, `agent.control`, `process.run`, `panes.create-command`, `inference.provide`, `clipboard.read`, `location.observer`, file writes outside `data` and `cache`, file reads that reach a sensitive path ([SEC-061]), and the ten grants in the next section (nine in its table, plus `settings.write-kernel:<tab-id>`, which is T1 only and listed after it). Grants are recorded per plugin and per side in the grant store ([SEC-112]).

## More high-risk grants

These ten grants are high risk: the nine in the table and `settings.write-kernel:<tab-id>`, described after it. They follow the same rule as every other H grant: a T1 bundled plugin still needs an explicit grant for each of them, and the dialog shows them in the warning style, never pre-checked.

| Grant | Sides | What it allows | Why it is high risk | Extra rule |
|---|---|---|---|---|
| `terminal.history` | S C | Read the scrollback above the screen: `terminal.history-text`, `terminal.search-history` | Scrollback holds everything the user ever typed or printed in that pane, including secrets | A reply keeps the newest 4 MiB |
| `terminal.input-lock` | S C | Hold the user's keyboard for a short time with `terminal.lock-input` or the `lock-user-input` field of `send-input` | A plugin that holds the keyboard can hide what it types | At most 2 s per lock, one lock per pane. The user's keys are queued, not lost (D17) |
| `terminal.raw-input` | S C | Send raw bytes or mouse events with `send-input`, and make a `terminal-view` interactive | Raw bytes drive any program in the pane, as if the user typed them | Goes through `send-input`, with its check order and its dedup key |
| `panes.lifecycle` | S | Freeze, unfreeze, restart with a new command, terminate and resize panes | A plugin can stop or replace a running program | Freezing is refused on Windows with `unsupported` |
| `tree.restructure` | S C | Create groups, folders, projects and splits, and run atomic `restructure` batches | A batch can move or ungroup the whole tree | At most 256 operations per batch, all or nothing, and `revert-restructure` undoes one |
| `sessions.cross` | S | List the user's other running sessions and run one command of the same plugin there | It acts outside the session where the user granted it | T1 and signed T2 only, and only commands of the same plugin that the other session has also granted (D16) |
| `process.long-lived` | S C | Start a streaming child with `process.spawn` | A child outlives a single call | The program must also match a `process.run:<program>` grant. Children run in the helper's confinement |
| `agent.transcript` | S C | Read typed turns of an agent session | A transcript holds every prompt, answer and tool call | The host parses and redacts it, so a plugin never reads the raw file (D22) |
| `settings.read-kernel:api` | S C | Read the `api` tab of the built-in settings | The `api` tab holds the keys for model providers | Secrets are always returned as `<redacted>`. Other tabs are normal risk |

Four more grants are high risk and available to T1 bundled plugins only: `settings.read-all`, `settings.write-all`, `plugins.manage` and `settings.write-kernel:<tab-id>`. A T1 plugin gets each of them through the first-start prompt like any other H grant, and a T2 or T3 install that lists one fails with `manifest.grant-tier`. `settings.read-kernel:api` is high risk at every tier: T1 is asked at first activation, T2 and T3 see it unchecked in the review dialog, and no tier refuses it at install.

These grants are normal risk: `tree.attention`, `ui.chrome`, `open-external`, `keymap.read`, `location.geocode`, `inference.embed`, `extensions.provide:<point>`, `extensions.contribute:<point>` for a point with `risk: "standard"`, `notify.toast`, `journal.read`, `settings.read-kernel:<tab>` for a tab other than `api`, and `terminal.read-cells`. Two further grants are high risk because of what they receive or start: `panes.create-command:<program>` starts an unconfined program in a terminal pane ([SEC-040]), and `inference.provide` receives every prompt that other plugins send to the AI model ([SEC-007]). The development-only grant `gpu.kernels` is `@unstable(feature = gpu-kernels)` and appears only in development builds (D18).

### Consent that is not a grant

A grant is a standing permission. Four extended features also ask the user at the moment of first use, because the target is only known then. Each answer is stored in the consent store ([SEC-112]) and can be withdrawn in Settings > Plugins or with `ilium plugin consents` ([SEC-120]). Prompts are rate limited ([SEC-105]).

| Feature | Who is asked | When | If declined |
|---|---|---|---|
| `open-external` | The user | The first time a plugin opens each target origin (SEC-043) | The intent completes as `refused(not-permitted)` |
| `http:setting:<key>` | The user | When the setting holds a host the plugin has not used before (SEC-074) | The request fails with `not-permitted`, and the host is not contacted |
| `location.geocode` | The user | The first time a plugin geocodes. The host service is Photon, with Nominatim as fallback (decision D14 in the [Design decisions](/pages/design-decisions/)) | Calls return `not-permitted` |
| Bundled plugin with H grants | The user | Once, at the first activation of a T1 plugin | The plugin runs without them. The question comes back at the next server start, and after three answers of Not now it stops ([SEC-111]) |

<figure>
<pre class="term"><span class="t-hi">╭─ Bundled plugin, first start ────────────────╮</span>
│ ilium.pane-tools asks for:                   │
│                                              │
│ [ ] Read scrollback ………………… terminal.history │
│ [ ] Hold the keyboard    terminal.input-lock │
│ [ ] Read agent transcripts  agent.transcript │
│                                              │
│ Asked 1 of 3 times.                          │
│                                              │
│ <span class="t-hi"> Enter </span> Allow selected   <span class="t-hi"> Esc </span> Not now       │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Bundled plugins start without their high-risk grants, and every row starts unchecked. Focus starts on Not now ([SEC-111]).</figcaption>
</figure>
<figure>
<pre class="term"><span class="t-hi">╭─ Open link ──────────────────────────────────╮</span>
│ Notes Panel wants to open this link in your  │
│ browser:                                     │
│                                              │
│ https://status.example.com                   │
│                                              │
│ Your answer is remembered for this site and  │
│ this plugin.                                 │
│                                              │
│ <span class="t-hi"> Esc </span> Cancel        <span class="t-hi"> Enter </span> Open             │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The origin consent. Non-ASCII hosts also show their punycode form.</figcaption>
</figure>
<figure>
<pre class="term"><span class="t-hi">╭─ Look up places ─────────────────────────────╮</span>
│ Place names you type are sent to the         │
│ geocoding service. At most 1 request per     │
│ second leaves your computer.                 │
│                                              │
│ <span class="t-hi"> Esc </span> Decline        <span class="t-hi"> Enter </span> Allow           │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The one-time consent for place lookups.</figcaption>
</figure>
## What a call returns without a grant

A declined or revoked grant changes what a call returns. The function's signature decides the form, and the refusal is always written to the plugin log.

| Kind of function | Declined or revoked grant | Example |
|---|---|---|
| Returns `result<T, plugin-error>` | `not-permitted`, or `revoked` when the grant was removed during the call | `fs.read` on a path outside its scope, or `geocode.search` without consent |
| Returns `option<T>` (no error channel) | `none` | `location.current` returns `none` |
| Returns a list | An empty list | `tree.children` returns no nodes |
| A submission with an extended intent | `refused(not-permitted)`. The key is not reserved | `freeze-pane` without `panes.lifecycle` |
| Function whose grant the manifest never requested on that side (also when requested only for the other side) | A trap `host.capability-not-requested`. The instance ends and restarts after a delay. An import of an interface that no requested capability unlocks fails the install with `manifest.import-not-requested` | `process.spawn` called without a request |

The first refused call of each kind writes one `warn` line to the plugin log for each plugin instance. The line names the grant, the function and the reason.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin logs pane-tools --tail 4
<span class="t-dim">09:14:02</span> <span class="t-warn">warn</span> refused terminal.history
       by history-text (declined)
<span class="t-dim">09:14:09</span> <span class="t-warn">warn</span> refused open-external
       by open-external (declined)
<span class="t-dim">09:14:09</span> <span class="t-err">error</span> geocode failed: not-permitted
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Refusals of high-risk and consent-gated calls. The last line is the error the plugin logged itself.</figcaption>
</figure>

The reason is one word from the closed list in [Refusal reasons](/pages/reference-errors-and-limits/#refusal-reasons). The most common are `declined` (the user left the grant unchecked), `revoked` (the grant was removed), `policy` (the admin policy forbids it, [SEC-184]), `offline` (the offline switch is on, [SEC-084]) and `unavailable` (the target is absent or the caller lacks the grant to learn about it, [SEC-373]). A manifest that never asked for the grant is not a refusal reason: it is a trap. For `not-permitted`, `quota-exceeded` and `network` the error `message` starts with `<reason>: `. The log line has the form `refused <grant> by <function> (<reason>)`, and its JSON form is `{"type":"refusal","grant":"<grant>","function":"<function>","reason":"<reason>"}`.

## Revocation and the authorization epoch

Each plugin has an authorization epoch. Every grant change bumps it. A host call compares the epoch it started with against the current epoch, so a call in flight during a revocation fails closed with `revoked`.

Revoke a grant in Settings > Plugins > Capabilities, or from the command line:

```text
ilium plugin grants notes --revoke fs.read:project
```

<figure>
<pre class="term"><span class="t-hi">╭─ Plugins · pane-tools · Capabilities ────────╮</span>
│ terminal.read-cells ………………………………………… granted │
│ terminal.history …………………………………… <span class="t-warn">HIGH</span> granted │
│ process.long-lived …………………… <span class="t-warn">HIGH</span> not granted │
│ notify.toast …………………………………………………………… granted │
│ settings.read-kernel:api    <span class="t-warn">HIGH</span> not granted │
│                                              │
│ <span class="t-dim">Space grant or revoke · Esc close</span>            │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A high-risk grant carries the HIGH mark whether granted or not.</figcaption>
</figure>
A revocation has these effects:

- A call records the epoch at entry. When the epoch changes before the call returns and the change removed a grant that the call needs, the call returns `revoked`, and a pending request that needs the grant completes as `failed(revoked)`. A call that does not need the removed grant completes normally. A call that enters after the bump is checked against the new set and returns `not-permitted` with a message starting `revoked: `, until re-activation and after it.
- The plugin receives `deactivate(revoked)` and is activated again with the reduced grant set. The re-activation is not counted as a failure.
- Contributions that need the grant are withdrawn: decorations, toolbar state, grabs, claims, highlights, filters, watches, routes, and the status segments, frame slots, toolbar buttons and row actions of `ui.chrome`.

A grant added later bumps the epoch too. The plugin then receives `deactivate(granted)` and is activated again with the new set.

<figure>
<svg viewBox="0 0 400 120" role="img" aria-label="Trust chain from package to host call">
  <title>Trust chain</title>
  <desc>A package passes the signature check, then its grants are recorded. Each host call checks the epoch.</desc>
  <defs>
    <marker id="trust-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="6" y="40" width="86" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="49" y="59" text-anchor="middle" fill="currentColor" font-size="11">Package</text>
  <rect x="106" y="40" width="86" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="149" y="59" text-anchor="middle" fill="currentColor" font-size="11">Signature</text>
  <rect x="206" y="40" width="86" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="249" y="59" text-anchor="middle" fill="currentColor" font-size="11">Grants</text>
  <rect x="306" y="40" width="86" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="349" y="59" text-anchor="middle" fill="currentColor" font-size="11">Host call</text>
  <path d="M92 55 L106 55" stroke="currentColor" fill="none" marker-end="url(#trust-arrow)"/>
  <path d="M192 55 L206 55" stroke="currentColor" fill="none" marker-end="url(#trust-arrow)"/>
  <path d="M292 55 L306 55" stroke="currentColor" fill="none" marker-end="url(#trust-arrow)"/>
  <text x="200" y="94" text-anchor="middle" fill="currentColor" font-size="10">Each host call compares the epoch it started with.</text>
  <text x="200" y="110" text-anchor="middle" fill="currentColor" font-size="10">A revocation bumps the epoch, so the call fails with revoked.</text>
</svg>
<figcaption>A package must pass the signature and publisher checks before its grants are recorded. Each host call is then checked against those grants and the current epoch.</figcaption>
</figure>

## File access

Scopes are resolved by the kernel once per grant, to canonical directories. A plugin never learns a path it was not granted.

| Scope | Root | Default |
|---|---|---|
| `package` | Unpacked package `assets/`, read only | Always |
| `data` | The plugin's private data folder | Always, read and write |
| `cache` | The plugin's private cache folder, may be evicted | Always, read and write |
| `project` | The session's project folder | Grant |
| `picked` | Each path the user chose in `fs.pick` or a `path-field` | Grant per pick |
| `worktrees` | The setting `workspace.worktree_root` | Grant. Write is H |
| `agent-sessions:<agent>` | Agent transcript folder, read only. Includes `gemini` | Grant, H |
| `agent-config:<agent>` | Agent configuration folder, read and write (`fs.read` H, `fs.write` H). Includes `gemini` and `antigravity` | Grant, H for read and H for write |
| `path:<template>` | Template with `{home}`, `{config}`, `{data}`, `{cache}`, `{project}`, `{worktrees}` | Grant. Literal absolute paths are H |

Each write is atomic. The kernel writes a temporary sibling file and renames it over the target, so a reader never sees a half-written file. With the `write-atomic` call, the write also carries an expected revision and fails with `conflict` when the file changed. Writes outside `data` and `cache` need an explicit user grant, even for T1 plugins.

## Extension points and replacement

An extension point lets one plugin add data or code to another plugin's flow. It is a grant, not a back door.

| Grant | Risk | What it allows |
|---|---|---|
| `extensions.provide:<plugin-id>/<point>` | S | Own a point. A plugin may own only points that start with its own id |
| `extensions.contribute:<plugin-id>/<point>` | S, or H when the point declares `risk: "high"` | Add an entry or a handler to that point |

- There are no wildcard grants. `manifest.grant-wildcard` refuses them, and `extensions.contribute-any` does not exist. The user sees each point by name.
- The review dialog groups a package's contributions under **Adds to other plugins**, for example `Text triggers > Before fire: can stop a trigger from sending text (high risk)`.
- A handler gets only the payload of the owner's `payload_schema`. It runs inside its own plugin instance, with its own grants, under a deadline of 5 to 50 ms.
- Hooks fail open. A handler that times out, traps or errors counts as a pass, so a slow handler cannot veto anything. Three breaches in a row stop that extension until the plugin restarts, and the user sees a notice.
- `replaces` needs no grant, but the user confirms every replacement. The built-in plugin is turned off while the replacement is on. Its stored data stays on disk and the replacement does not read it. `ilium.core.sessions` and `ilium.core.settings` cannot be replaced.

See [Extension points](/pages/api-extension-points/) and [Extending core plugins](/pages/extending-core-plugins/).

## What the kernel never lets a plugin do

- Read or write outside a granted scope through `..`, an absolute path, a symlink or a hard link.
- Open a socket or an HTTP connection it was not granted. A component that imports `wasi:sockets` or `wasi:http` is refused at load [ABI-024].
- Run a program it was not granted. `process.run` and `process.spawn` need a grant for that exact program name or absolute path.
- Read or change another plugin's settings, unless it holds `settings.read-all` or `settings.write-all`. Only first-party T1 plugins can receive them.
- Write a kernel setting or the keymap, except a T1 holder of `settings.write-kernel:<tab-id>`, who can write the keys of that one tab. `settings.read-kernel` and `keymap.read` only read, and any other plugin that wants a change links the user to `ilium://settings/<tab-id>#<key>` through `open-external`.
- Send terminal input, signal a pane, drive an agent, freeze or restart a pane, or restructure the tree without the matching high-risk grant (`terminal.write`, `panes.signal`, `agent.control`, `panes.lifecycle`, `tree.restructure`).
- Hold the keyboard for more than 2 s, or hold two input locks on one pane.
- Read an agent transcript as a raw file. `agent.transcript` returns typed turns that the host parsed and redacted.
- Send a command to another session that is not its own command, or to a session where the grant is missing.
- Print raw terminal escape sequences. Plugins draw through the UI tree only. The extended widgets (`terminal-view`, `video`, `surface`, `location-picker`) are drawn by the host from typed props.
- Replace the kernel recovery page, or hide the switch that disables the plugin.
- Change a built-in setting value, or draw a section in a built-in tab without `settings.contribute:<tab-id>`. A built-in tab never shows a `preview` row or a view-rendered plugin widget (SE10, D19).
- Cover the screen with toasts. A plugin may send 3 toasts per 10 s with `notify.toast`, and at most 3 are pending (D20).
- Lose a never-dropped event silently. An overflow restarts the plugin instead.

## Limits that protect the user

The extended contract adds limits whose purpose is safety. They are fixed by the host. A manifest may ask for less, and for some of them for more up to the tier ceiling.

| Limit | Value | When exceeded |
|---|---|---|
| Input lock | 2 s per lock, one per pane | The lock ends. The user's keys were queued, not lost |
| Toasts | 3 per 10 s per plugin, 3 pending | The oldest pending toast is replaced |
| Status segments and frame slots | 10 updates per second per slot (a status segment id is a slot) | Updates are coalesced |
| Geocoding | 1 request per second per plugin, 10 s timeout, 24 h cache | `rate-limited` with `retry-after-ms` |
| Open HTTP streams | `http_streams` (default 4, T1 up to 16) and `session_http_streams` (64); see [Errors and limits](/pages/reference-errors-and-limits/#requestable-limits) | `quota-exceeded` |
| Pending HTTP requests | `http_pending` (default 256, T1 up to 1,024) | `busy` with `retry-after-ms` |
| Spawned children | `children` (default 2, T1 up to 8) and the shared `session_processes` (16) | `quota-exceeded` |
| Concurrent `process.run` | `process_runs` (default 4, T1 up to 16) and the shared `session_processes` (16) | `busy` with `retry-after-ms` |
| Durable timers | `durable_timers`: 64 (T1 up to 256) | `quota-exceeded` |
| Package assets | `package_assets_mib`: 64, T1 up to 256 | `manifest.asset-size` at install |
| `restructure` batch | 256 operations | The batch is refused |
| `screen-changed` events | 10 per second per pane | Coalesced |

The full list is in [Errors and limits](/pages/reference-errors-and-limits/).

## Worked example: a refused grant

The plugin reads a project note. If the grant is missing or was revoked, the plugin shows no note and logs a warning. Any other error is logged as an error. The first read asks for a path outside the scope, which the kernel refuses with `not-permitted` whatever the grants are. The second read is inside the scope.

**Manifest:** `"capabilities": { "server": ["fs.read:project"] }`

**Rust**

```rust
use ilium_plugin_sdk::host::fs;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;

const MAX_NOTE_BYTES: u32 = 65_536;

/// Returns the note text, or `None` when the note cannot be read right now.
fn read_note(path: &str) -> Option<String> {
    let target = FsPath { scope: "project".into(), path: path.into() };
    match fs::read(&target, 0, MAX_NOTE_BYTES) {
        Ok(bytes) => String::from_utf8(bytes).ok(),
        Err(error) => {
            match error.code {
                ErrorCode::NotPermitted | ErrorCode::Revoked => {
                    log::log(Level::Warn, &format!("note unavailable: {path}"));
                }
                _ => log::log(Level::Error, &format!("note read failed: {}", error.message)),
            }
            None
        }
    }
}

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Notes;

impl Lifecycle for Notes {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        let _ = read_note("../secrets.txt");
        let note = read_note("docs/notes.md");
        log::log(Level::Info, &format!("note loaded: {}", note.is_some()));
        Ok(())
    }
}

impl Handler for Notes {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin, PluginError } from '@ilium/plugin-sdk';
import { read } from '@ilium/plugin-sdk/host/fs';
import { log } from '@ilium/plugin-sdk/host/log';

const MAX_NOTE_BYTES = 65536;

/** Reads a file under the project scope. The kernel refuses paths that leave the scope. */
function readNote(path: string): string | undefined {
  try {
    const bytes = read({ scope: 'project', path }, 0n, MAX_NOTE_BYTES);
    return new TextDecoder().decode(bytes);
  } catch (error) {
    if (!(error instanceof PluginError)) {
      throw error;
    }
    if (error.code === 'not-permitted' || error.code === 'revoked') {
      log('warn', `note unavailable (${error.code}): ${path}`);
      return undefined;
    }
    log('error', `note read failed (${error.code}): ${error.message}`);
    return undefined;
  }
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate(context) {
    readNote('../secrets.txt');
    const note = readNote('docs/notes.md');
    log('info', `${context.plugin}: note loaded ${note !== undefined}`);
  },
  onEvents: () => [],
});
```

Outcome: `../secrets.txt` leaves the scope, so the call fails with `not-permitted` and the plugin logs a warning. With the grant declined, `activate` still succeeds and the warning is written once. A revocation during the read returns `revoked`, and the plugin is activated again without the grant.

## Worked example: consent that the user declines

A client plugin looks up a place name with the host's geocoding service. The first call asks the user for consent. If the user declines, the call returns `not-permitted` on every later call too, and the plugin falls back to a plain text field.

**Manifest:** `"capabilities": { "client": ["location.geocode"] }`

**Rust**

```rust
use ilium_plugin_sdk::host::geocode;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::task;
use ilium_plugin_sdk::host::geocode::GeocodeQuery;

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct Places;

impl Lifecycle for Places {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        task::spawn(async {
            let query = GeocodeQuery {
                text: "Lyon".into(),
                near: None,
                limit: 5,
                language: Some("fr".into()),
            };
            match geocode::search(query).await {
                Ok(places) => log::log(Level::Info, &format!("{} places found", places.len())),
                Err(error) => match error.code {
                    ErrorCode::NotPermitted => {
                        log::log(Level::Warn, "geocoding declined, the place stays plain text");
                    }
                    _ => log::log(Level::Error, &format!("geocode failed: {}", error.message)),
                },
            }
        });
        Ok(())
    }
}

impl Handler for Places {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}
```

**TypeScript**

```ts
import { definePlugin, PluginError } from '@ilium/plugin-sdk';
import { search } from '@ilium/plugin-sdk/host/geocode';
import { log } from '@ilium/plugin-sdk/host/log';

async function lookup(): Promise<void> {
  try {
    const places = await search({ text: 'Lyon', near: undefined, limit: 5, language: 'fr' });
    log('info', `${places.length} places found`);
  } catch (error) {
    if (!(error instanceof PluginError)) {
      throw error;
    }
    if (error.code === 'not-permitted') {
      log('warn', 'geocoding declined, the place stays plain text');
      return;
    }
    log('error', `geocode failed (${error.code}): ${error.message}`);
  }
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  activate() {
    void lookup();
  },
  onEvents: () => [],
});
```

Outcome: the consent dialog appears once. After a decline, the plugin logs the warning, the host never contacts the geocoding service, and the host writes one refusal line to the plugin log. A declined `open-external` is handled the same way, from the outcome `refused(not-permitted)` of its submission.

## The manifest side of high-risk grants

A manifest lists the grants per side. The manifest is JSON, so it is the same for Rust and TypeScript plugins.

```json
"capabilities": {
  "server": ["terminal.read-cells", "terminal.history", "process.run:rg", "process.long-lived"],
  "client": ["notify.toast", "ui.chrome", "settings.read-kernel:terminal"]
},
"limits_tier": "standard"
```

Three of the server grants here are high risk: `terminal.history`, `process.run:rg` and `process.long-lived`. `settings.read-kernel:terminal` is normal risk, because only the `api` tab is high. A manifest that wrote `settings.read-kernel:*` would be refused with `manifest.grant-wildcard`.

## Settings rows in built-in tabs

A plugin can add rows to a built-in Settings tab, but only under a label the user always sees, and only with its own values. The kernel draws every plugin row under a divider that names the plugin and the word `plugin`, with a dim `▏` gutter on each line, so the user always knows where the rows came from. The text of a plugin row (titles, descriptions, info text, notice text) is plain text: the kernel strips control characters and bidi overrides, so a plugin cannot draw a fake kernel divider or a fake close button. Plugin values are stored only under `[plugins."<plugin-id>"]`, and a plugin row never writes a built-in value. A built-in tab never runs plugin code while it paints. The grant `settings.contribute:<tab-id>` allows drawing sections in one built-in tab and nothing else. It is shown in the review dialog as `Adds settings to the <tab> tab`. See [Plugin settings extension](/pages/settings-extension/) for the rules.

<figure>
<pre class="term"><span class="t-hi">╭──≡ ● · Terminal────────────────────────────────────────────────╮</span>
│ Scrollback lines …………………………………………………………………………………… - 10000 + *  │
│ Cursor style ……………………………………………………………………………………………… ← Block → +  │
│<span class="t-dim">── Notes · plugin ──────────────────────────────────────────────</span>│
│<span class="t-dim">▏</span>Highlight colour ………………………………………………………………………………………… [ Amber ]  │
│<span class="t-dim">▏</span>Keep history lines …………………………………………………………………………………………… [ On ]  │
│<span class="t-dim">▏</span><span class="t-dim">Plain text from the plugin; the kernel drew this divider.</span>      │
╰────────────────────────────────────────────────────────────────╯
</pre>
<figcaption>Illustrative, not generated. A built-in Terminal tab with the section of a plugin. The kernel rows come first, then one divider for the plugin, and the dim gutter on every plugin line.</figcaption>
</figure>

Reading a built-in value is a separate, read-only grant. `settings.read-kernel:<tab-id>` returns the JSON values of one tab with secrets replaced by `<redacted>`, and a `kernel-settings-changed` event names the keys that changed. It does not allow any write.

## Signing and distribution

The rules for updates, trusted keys and lost keys carry the ids USR-120 to USR-129. They are in [Packages, updates, trust and keys](/pages/packages/#updates-trust-and-keys) and [Install and manage plugins](/pages/user-install-and-manage/).

Packages are files. A registry is optional (decision D27). In both cases the user decides whether to trust the key that signed a package, and a registry adds signed revocation. See [Signing and trust](/pages/reference-signing-and-trust/) and [Registry reference](/pages/reference-registry/).

```text
ilium plugin keygen --publisher "Example Corp" --out ~/keys/example
ilium plugin sign ./dist/example.notes-1.2.0.iliumpkg --key ~/keys/example/publisher.key
ilium plugin trust add ~/keys/example/publisher.pub
ilium plugin install ./dist/example.notes-1.2.0.iliumpkg
```

`keygen` writes `publisher.key`, an owner-only Ed25519 private key, and `publisher.pub`, which holds the publisher name and public key. `sign` adds the `signature` entry atomically. `trust add`, `trust list` and `trust remove` manage the trusted keys. Anyone who holds `publisher.key` can sign packages as that publisher.

An unsigned package installs only with `--allow-unsigned`, as a T3 package with a `dev` badge. A lower version of an installed id is refused unless `--allow-downgrade` is given.

## Edge cases and errors

| Situation | Result | Where you see it |
|---|---|---|
| Capability revoked during a call | `revoked`. The plugin re-activates without the grant. Not a failure | Plugin log |
| Path leaves its scope through `..`, a symlink or a hard link | `not-permitted` | Plugin log |
| Import the manifest never requested | Trap. The instance ends and restarts after a delay | Plugin log, with the trap frames |
| Install of a T2 or T3 package on Windows or macOS | Refused, with the message in the sandbox section | Install output |
| Unsigned package without `--allow-unsigned` | Install refused | Install output |
| Signing key the user does not trust | Install offers `Trust publisher` with the fingerprint. Declining refuses it | Install dialog |
| `publisher` differs from the key's recorded publisher | `manifest.publisher-mismatch`, install stops | Install error line |
| `process.run:sh` in a T2 or T3 package | `manifest.grant-tier`, install stops. The shell grant is for T1 only | Install error line |
| `limits_tier: "first-party"` in a package that is not a bundled `ilium.*` plugin | `manifest.limits-tier`, install stops | Install error line |
| Package assets above 64 MiB (T2, T3) or 256 MiB (T1) | `manifest.asset-size`, install stops | Install error line |
| `settings.read-kernel` without a tab, or with `*` | `manifest.grant-argument-missing` or `manifest.grant-wildcard` | Install error line |
| T1 plugin with a high-risk grant that the user never granted | The call is refused as `not-permitted` (reason `declined`), though the plugin is bundled | Plugin log |
| Server restarts between a journaled `send-input` and its outcome | One `input-uncertain` event. The input is never replayed (D21) | Event, plugin log |
| User declines `open-external` for an origin | `refused(not-permitted)` for each later open of that origin until the answer is withdrawn | Outcome |
| `sessions.cross` in a T3 package | Not available to T3 packages (D16) | Install output |

## What the user sees

The review dialog grants capabilities, Settings > Plugins > Capabilities shows and revokes them, and `ilium plugin logs <id>` shows every refusal, trap and process run. A first-start prompt appears for a bundled plugin with high-risk grants, and a small confirmation appears the first time a plugin opens a site, uses a new host from a setting, or geocodes a place.

#### Lifecycle and trust

A package that policy blocks, or that is refused at load, never runs plugin code. Safe mode ([Safe mode and recovery](/pages/user-safe-mode-and-troubleshooting/)) turns third-party plugins off without changing grants. Install scope is global in 0.1.0 so that opening a repository cannot run code ([Plugin scope and sessions](/pages/reference-plugin-scope-and-sessions/)). An uninstall removes grants; see [Install, update and uninstall](/pages/reference-install-update-uninstall/). [LIF-281]

- [LIF-281] Uninstall MUST remove the plugin grants, and a project folder MUST NOT be able to install or enable a plugin. Test: uninstall and read `grants.json`; list an uninstalled package in `plugins.toml`.


## Author duties

A signed package carries the trust of its publisher key. The author is responsible for the key and for the support of the release. An author MUST keep the secret key out of the package and out of version control, and MUST announce a leaked key with the fingerprint of its replacement. [AUT-322] The practice is in [Publishing and support](/pages/guide-publishing-and-support/), and the review list before each release is in [Quality checklist](/pages/guide-quality-checklist/).

## See also

- [Capability table](/pages/reference-capabilities/)
- [Publishing and support](/pages/guide-publishing-and-support/)
- [Manifest reference](/pages/reference-manifest/)
- [Packages](/pages/packages/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Lifecycle](/pages/lifecycle/)
- [Plugin settings extension](/pages/settings-extension/)
- [Debugging](/pages/debugging/)
- [Design decisions](/pages/design-decisions/)
- [Roadmap](/pages/roadmap/)

Contract: `ilium:plugin@0.1.0`.
