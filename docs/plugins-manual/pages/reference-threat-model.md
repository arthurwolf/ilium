---
layout: page
title: Threat model
permalink: /pages/reference-threat-model/
---

## Threat model

This page says what the plugin platform defends, against whom, and how well. The [Security model](/pages/security/) describes the mechanisms: tiers, the sandbox, grants and the authorization epoch. This page names the attackers those mechanisms are meant to stop. A statement here is a security guarantee only if it carries a requirement id such as [SEC-004]. Anything else is advice or an accepted risk.

The words MUST, SHOULD and MAY are used as in RFC 2119. Requirement ids are collected in [Security and policy requirements](/pages/reference-requirements-security-policy/). The contract is `ilium:plugin@0.1.0`. The platform is not implemented yet, so this page describes the contract that tests and the implementation follow.

### You will need

- The tier names T0 to T3 from [Security model](/pages/security/).
- The grant names from the [Capability table](/pages/reference-capabilities/).
- No tools. This page is reference text.

## Assets

An asset is something an attacker wants to read, change or abuse. Ilium has more sensitive assets than most programs, because it hosts terminals and AI agents that hold the user's credentials.

| Asset | Where it lives | Why it matters |
|---|---|---|
| Terminal screen and scrollback | Server memory, pane journal, session snapshots | Holds anything the user typed or printed, including secrets |
| Typed input and passwords | The input path from client to PTY | A password typed in a pane is visible only as keystrokes, and echo is off |
| Agent transcripts and prompts | Agent session folders, the server's parsed copies | Hold source code, tokens pasted into prompts and private decisions |
| Agent credentials | Agent configuration folders such as `~/.claude/`, `~/.codex/` | OAuth tokens and API keys that let an attacker act as the user |
| Home secrets | `~/.ssh`, `~/.gnupg`, `~/.aws`, `~/.kube`, `~/.netrc`, browser profiles | Long-lived credentials |
| Plugin secrets and provider API keys | The secrets file, see [Secrets](/pages/reference-secrets/) | Paid or privileged accounts |
| Session snapshots | `<project>/.ilium/sessions/*.json` | Commands, titles, scrollback excerpts |
| HTTP API token | `$XDG_RUNTIME_DIR/ilium/http-api.token` | Whoever holds it can drive Ilium through the HTTP API |
| Trust store | Publisher keys the user trusts | Decides which packages run as T2 |
| Grant store and consent store | See [Grant review and consent](/pages/reference-grant-review-and-consent/) | Decide what each plugin may do |
| Audit log | See [Audit log](/pages/reference-audit-log/) | The record of security decisions |
| Keyboard focus and clipboard | Client | Typing into the wrong pane, or pasting a command, runs code |
| Integrity of what the user sees | Tree, dialogs, terminal output | A spoofed dialog or a hidden pane tricks the user into approving something |

**[SEC-001]** The host MUST treat every asset in this table as protected: no plugin of any tier may read, change or delete one except through a grant that names it, or through an interface the owning kernel component exposes.

## Trust boundaries

A trust boundary is a line across which data passes from a more trusted to a less trusted party, or the other way. Every boundary is a place where the host validates its input.

<figure>
<svg viewBox="0 0 560 330" role="img" aria-label="Trust boundaries of the plugin platform">
  <title>Trust boundaries</title>
  <desc>The kernel and T1 plugins share one process. Each T2 or T3 plugin side runs in its own helper. Children, the HTTP API, the CLI in a pane and the project folder are separate zones.</desc>
  <defs>
    <marker id="tb-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="6" y="6" width="270" height="120" rx="8" fill="none" stroke="currentColor" stroke-width="2"/>
  <text x="16" y="24" fill="currentColor" font-size="11">Ilium process: kernel and T1 (trusted)</text>
  <rect x="20" y="40" width="100" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="70" y="64" text-anchor="middle" fill="currentColor" font-size="11">Kernel</text>
  <rect x="150" y="40" width="110" height="40" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="205" y="64" text-anchor="middle" fill="currentColor" font-size="11">T1 plugins</text>
  <text x="16" y="108" fill="currentColor" font-size="10">Boundary 1: dispatcher channel to helpers</text>
  <rect x="300" y="6" width="254" height="120" rx="8" fill="none" stroke="currentColor" stroke-width="3"/>
  <text x="310" y="24" fill="currentColor" font-size="11">Helpers, one per plugin and side (hostile)</text>
  <rect x="314" y="40" width="108" height="34" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="368" y="61" text-anchor="middle" fill="currentColor" font-size="10">T2/T3 plugin A</text>
  <rect x="434" y="40" width="108" height="34" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="488" y="61" text-anchor="middle" fill="currentColor" font-size="10">T2/T3 plugin B</text>
  <text x="310" y="108" fill="currentColor" font-size="10">Boundary 2: no network, no home, no runtime dir</text>
  <path d="M276 60 L300 60" stroke="currentColor" fill="none" marker-end="url(#tb-arrow)"/>
  <rect x="6" y="150" width="170" height="70" rx="8" fill="none" stroke="currentColor"/>
  <text x="16" y="170" fill="currentColor" font-size="11">process.run children</text>
  <text x="16" y="188" fill="currentColor" font-size="10">same sandbox, no network (B3)</text>
  <rect x="195" y="150" width="170" height="70" rx="8" fill="none" stroke="currentColor"/>
  <text x="205" y="170" fill="currentColor" font-size="11">Host-initiated egress</text>
  <text x="205" y="188" fill="currentColor" font-size="10">http, inference, geocode (B4)</text>
  <rect x="384" y="150" width="170" height="70" rx="8" fill="none" stroke="currentColor"/>
  <text x="394" y="170" fill="currentColor" font-size="11">HTTP API listener</text>
  <text x="394" y="188" fill="currentColor" font-size="10">token, Host and Origin (B5)</text>
  <rect x="6" y="245" width="170" height="70" rx="8" fill="none" stroke="currentColor"/>
  <text x="16" y="265" fill="currentColor" font-size="11">CLI run in a pane</text>
  <text x="16" y="283" fill="currentColor" font-size="10">same user, prompt-injectable (B6)</text>
  <rect x="195" y="245" width="170" height="70" rx="8" fill="none" stroke="currentColor"/>
  <text x="205" y="265" fill="currentColor" font-size="11">Project folder</text>
  <text x="205" y="283" fill="currentColor" font-size="10">.ilium/ files are hostile (B7)</text>
  <rect x="384" y="245" width="170" height="70" rx="8" fill="none" stroke="currentColor"/>
  <text x="394" y="265" fill="currentColor" font-size="11">Terminal text</text>
  <text x="394" y="283" fill="currentColor" font-size="10">hostile input to detectors (B8)</text>
</svg>
<figcaption>The eight trust boundaries. B1 and B2 enclose the helpers. B3 to B8 are the places where the host receives data from a party it does not control.</figcaption>
</figure>

**[SEC-002]** The host MUST validate all data that crosses boundaries B1 to B8 before using it. Validation includes length, encoding, schema and, for identity, the channel it arrived on.

**[SEC-003]** The kernel and T1 plugins share one address space. The platform therefore treats T1 code as trusted code that the project builds, signs and tests. A flaw in T1 code is a vulnerability in Ilium, not a plugin attack.

## Attacker classes

Each class below is a party with a goal and a way in. The mitigation table that follows gives the defence and the residual risk for each.

| Class | Description | Starting position |
|---|---|---|
| A1 Malicious T3 package | An unsigned package the user installed from a file or a chat message | Runs in a helper with the grants the user approved |
| A2 Malicious T2 publisher | A publisher whose key is trusted, or whose key was stolen | Same as A1, but with no `--allow-unsigned` warning |
| A3 Buggy plugin fed hostile text | A benign plugin that parses terminal text, transcripts or files an attacker controls | Memory-safe guest, but logic errors can misuse its grants |
| A4 Hook contributor | A plugin that contributes handlers to another plugin's extension point | Sees the payload, can veto or rewrite |
| A5 Extension point owner | A plugin that owns a point and receives contributors' entries | Can pass contributors' data on (data laundering) |
| A6 Core replacement | A third-party plugin that replaces a core plugin | Takes over a whole product surface |
| A7 Hostile repository | A cloned project whose `.ilium/` files carry values and rules | Files are read the moment the project opens |
| A8 Prompt-injected agent | An AI agent in a pane that obeyed text from a web page or file | Runs commands with the user's privileges, including `ilium plugin ...` |
| A9 Other local process | Other code running as the same user | Out of scope, see non-goals |
| A10 Web page | A page in the user's browser that sends requests to the HTTP API port | Can send cross-origin requests, cannot read the token file |
| A11 Upstream bug | A flaw in Wasmtime, a codec or the OS sandbox primitives | Reaches Ilium through its dependencies |
| A12 Network attacker | Controls DNS or the path to a granted host | Can spoof DNS, intercept plaintext |
| A13 Other OS user | Another account on a shared machine | Can list files and processes the OS allows |

## Mitigations

| Class | Attack | Mitigation | Requirements | Residual risk |
|---|---|---|---|---|
| A1 | Read files outside the granted scopes | Mount namespace holds only granted scopes; path resolution with beneath-only opens | [SEC-021], [SEC-023], [SEC-058] | Kernel and Wasmtime bugs |
| A1 | Run a program to escape | Programs need `process.run:<program>`; children keep the sandbox; `panes.create` commands need their own grant | [SEC-031], [SEC-032], [SEC-040] | A granted program can run other programs |
| A1 | Steal data over the network | No network in the helper; `http` grants checked per host and per resolved address | [SEC-022], [SEC-072] | Read grant plus egress grant, see toxic combinations |
| A1 | Attack another plugin | One helper per plugin and side; identity from the channel | [SEC-020], [SEC-050] | Kernel-level isolation bugs |
| A1 | Trick the user in the review dialog | Kernel-drawn dialog, default focus on Cancel, key-repeat guard | [SEC-102], [SEC-103] | A user who approves everything |
| A2 | Ship a malicious update | Key pinning per plugin id; update dialog lists new and widened grants | [SEC-142], [SEC-108] | A trusted key that is stolen and used on a new id |
| A2 | Keep running after takedown | Signed blocklist and policy blocklist | [SEC-146], [SEC-147] | Offline machines without a fresh blocklist |
| A3 | Hostile terminal text steers a plugin | Detectors and parsers in plugins are guest code in a helper; payload size caps; fuzzing | [SEC-340], [SEC-341] | Logic bugs in the plugin |
| A4 | Rewrite data before a guard sees it | Handlers re-validated against `payload_schema`; veto handlers see the final payload | [SEC-284], [SEC-286] | Owner's design choices |
| A4 | Starve later handlers | Per-handler deadline and a hard 100 ms chain cap | [HST-472] | Later handlers can be skipped |
| A5 | Launder sensitive data to a contributor without the grant | Payload data classes force risk `high` and require matching grants | [SEC-281], [SEC-282] | Classes the owner mislabels |
| A6 | Replace a core plugin to gain its power | Replacement inherits no grants; T3 cannot replace; kernel keeps security functions | [SEC-300] to [SEC-306] | A user who confirms a hostile replacement |
| A7 | Plugin behaviour set by `.ilium/` values | Project trust prompt; untrusted projects ignore plugin values | [SEC-310], [SEC-311] | A user who trusts a hostile repository |
| A8 | Run `ilium plugin install --allow-unsigned` | Approvals need an interactive client; no flag grants H capabilities headlessly | [SEC-116], [SEC-117] | An attached TUI session where the user clicks yes without reading |
| A8 | Edit config or grant files | Grant, trust and consent stores are owner-only and outside every plugin scope; external edits are audited and do not add H grants | [SEC-112], [SEC-113] | Same-user write access is out of scope for the OS, see A9 |
| A8 | Call the HTTP API | The API token is shell-equivalent; plugins cannot obtain it | [SEC-097], [SEC-098] | Any process of the same user can read the token file |
| A9 | Read process memory, files, token | Out of scope | [SEC-008] | Total |
| A10 | Cross-origin request to the API | Host and Origin checks; token required | [SEC-097] | DNS rebinding to a non-loopback name is refused by the Host check |
| A11 | Wasmtime sandbox escape | Process sandbox as a second layer; pinned version; advisory response | [SEC-025], [SEC-170], [SEC-171] | Zero-day with a kernel exploit |
| A12 | DNS rebinding to local services | Resolved-address checks and connection pinning | [SEC-072], [SEC-073] | Hosts on private ranges the user granted by literal |
| A12 | Interception | HTTPS only, TLS 1.2 minimum, no plugin-controlled verification bypass | [SEC-077] | Compromised system roots |
| A13 | Read another user's stores | Owner-only modes on every store, per-user directories, no cross-user sessions | [SEC-112], [SEC-199] | Root on the machine |

## Non-goals

The platform does not defend against these. A non-goal is a decision, so it is stated and tested only as far as shown.

**[SEC-008]** Same-user malware is out of scope. A process that runs as the user can read the user's files, memory and the HTTP API token. The platform MUST NOT claim to stop it. The consequence for the HTTP API is stated in [Network egress](/pages/reference-network-egress/): whoever holds the token has the power of a shell in the user's session.

**[SEC-009]** A root attacker, physical access, and a compromised kernel are out of scope.

**[SEC-010]** Malicious T1 code is out of scope. T1 code is built by the project and its integrity is checked at load ([SEC-176]).

**[SEC-011]** Third-party sandboxing on Windows and macOS is refused, not weakened. T2 and T3 packages do not load there (decision D5). The platform MUST NOT offer an in-process fallback on any system.

**[SEC-012]** Side channels between a plugin and the machine hardware, such as cache timing between processes, are reduced but not eliminated. See the side-channel section below.

## Toxic grant combinations

Some dangers come from two grants that are each harmless on their own.

Read grants of normal risk expose private data: `terminal.read-screen`, `terminal.read-cells`, `fs.read:project`, `tree.read`, `agent.read`, `journal.read`. Egress grants of normal risk move data off the machine: `http`, `inference`, notify channels, `clipboard.write`, `messages.peer`, `services.use`. A plugin that holds one grant from each list can read a password echoed on screen, an API key in a project file or an agent's reply, and send it to a server. No high-risk grant is involved, so a user who only checks the pre-ticked rows is exposed.

**[SEC-004]** The decision: the platform does not make these grants high risk, because most useful plugins hold such a pair. Instead the review dialog MUST show a **toxic combination row** whenever the requested set contains at least one data-read grant and at least one egress grant. The row is drawn in the warning style and reads: `This plugin can read your data and send data out.` The row is informational and has no checkbox of its own. The user can still refuse the grants it lists. When the admin policy forbids the combination (SEC-005), the row instead reads `Blocked by your administrator`, and the grants of the combination are shown locked off. It lists the two groups of grants that triggered it. Details are in [Grant review and consent](/pages/reference-grant-review-and-consent/).

**[SEC-005]** The admin policy MAY forbid the combination for chosen plugins with `[grants] deny_combinations`, see [Admin policy](/pages/reference-admin-policy/).

**[SEC-006]** Data-read and egress grant lists are fixed by the contract. A grant that moves between lists is a contract change.

Inter-plugin channels bypass grants too. Plugin B can receive scrollback from plugin A through `messages.peer`, a service or a topic, without holding a read grant. The platform's decisions on this are in [SEC-007] below and in [Extension security](/pages/reference-extension-security/).

**[SEC-007]** Data passed between plugins through `messages.peer`, `services`, topics and extension points is outside grant enforcement. This is a documented residual risk. To keep it visible:

- The review dialog MUST show a row `Receives data from other plugins` for a plugin that declares `messages.peer`, `services.use` of a service another plugin provides, or a topic subscription.
- A receiver of `messages.peer` MUST declare `accepts_messages_from` in the manifest, a list of plugin ids. Messages from other ids are dropped with a log line, so a plugin cannot be sent unsolicited data.
- `inference.provide` has normal risk (see the capability table) and is checked by default, but the provider receives every prompt that other plugins send. Its review wording is `Receives the text other plugins send to the AI model`.

## Side channels

**[SEC-370]** Guest-visible clocks (`clock` and the virtualised `wasi:clocks`) for T2 and T3 plugins MUST be coarsened to 1 ms and jittered by up to 1 ms, to limit timing attacks. T1 plugins get the same resolution unless their manifest requests a finer clock.

**[SEC-371]** The `screen-changed` event for a pane is coalesced to 10 per second. It MUST NOT be emitted while the terminal is in no-echo mode (the PTY has echo off, as for a password prompt). Detection uses the PTY ECHO flag. This keeps read grants from timing keystrokes of passwords.

**[SEC-372]** Each T2 or T3 plugin side runs in its own helper, so Spectre-style attacks between plugins need a kernel exploit, not a shared address space.

**[SEC-373]** An error returned to a caller without the right grant MUST be the same for "plugin not installed" and "plugin installed but not granted" (`not-permitted`, reason `unavailable`). Extension and peer calls MUST NOT reveal which plugin ids are installed.

**[SEC-374]** Errors returned to guests MUST NOT contain absolute paths of the host or ids of other plugins.

**[SEC-375]** Storage quota errors MUST NOT depend on the content of other plugins' data. A plugin sees only its own usage.

**[SEC-376]** Comparing the HTTP API token and the lock tokens MUST use a constant-time comparison.

Accepted risks, stated plainly: a plugin with read grants can still observe the coarse rhythm of output (10 events per second at most). A plugin can use a busy loop in a worker to build a timer finer than 1 ms; the CPU quota limits how well. Rate and size of responses of an HTTP API route reveal information to a client that holds the token, which is already shell-equivalent.

## How to read a row for tests

Every mitigation in the table has at least one test. [Security testing](/pages/reference-security-testing/) maps ids to fixtures. A test author derives cases by taking each requirement id, creating the attacker's starting position from the "Starting position" column and asserting the defined result.

## Edge cases

| Situation | Result |
|---|---|
| A plugin holds a read grant and an egress grant | The review dialog shows the toxic combination row. Install proceeds if the user approves |
| A plugin is later granted an egress grant through the Plugins screen | The row appears in the capability list from then on, and the grant change is audited |
| The policy forbids the combination | The install succeeds, the combination row reads `Blocked by your administrator` with its grants locked off, and calls return `not-permitted` with the reason `policy` |
| A plugin that never declared `accepts_messages_from` is sent a peer message | The message is dropped and counted; the sender gets `not-permitted` with reason `unavailable` |

### See also

- [Security model](/pages/security/)
- [Sandbox](/pages/reference-sandbox/)
- [Network egress](/pages/reference-network-egress/)
- [Grant review and consent](/pages/reference-grant-review-and-consent/)
- [Trust and supply chain](/pages/reference-trust-and-supply-chain/)
- [Extension security](/pages/reference-extension-security/)
- [Security and policy requirements](/pages/reference-requirements-security-policy/)

Contract: `ilium:plugin@0.1.0`.
