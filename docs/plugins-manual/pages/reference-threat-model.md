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

## Registry and publishing threat model

The tables above cover an attacker who already runs a plugin on the user's machine. A public plugin ecosystem has a second attack surface that comes first in time: the chain that carries a package from the publisher to the user, made of the publisher's account, the CI token, the build machine, the registry, its signing keys, its mirrors and the review records it shows. This section names the attackers on that chain and maps each to its controls. The mechanisms are in [Registry reference](/pages/reference-registry/), [Registry key compromise](/pages/reference-registry-key-compromise/), [Signing and trust](/pages/reference-signing-and-trust/) and [Trust and supply chain](/pages/reference-trust-and-supply-chain/). The CVE rule [SEC-702] applies to "registry flaws": for the purpose of that rule, a registry flaw is any condition in which one of the attackers R1 to R8 below succeeds against a control listed for it.

### Registry assets

These rows extend the Assets table.

| Asset | Where it lives | Why it matters |
|---|---|---|
| Publisher signing key | The publisher's machine, ideally encrypted and backed up offline ([SEC-174]) | Whoever holds it can sign a release that installs as T2 for users who pinned the key |
| Registry account | The registry's account store, protected by a second factor ([SEC-723]) | Controls who may publish under a publisher id and who is a maintainer |
| Registry publish token | CI secret store of the publisher | Authorises publishes under one publisher id only ([PKG-968]) |
| Registry metadata | `root.json`, `timestamp.json`, `snapshot.json`, `targets.json` and the transparency log ([PKG-700]) | Decides which versions exist, which digests are valid and what clients may install |
| Registry role keys | Root keys (offline, threshold) and the online `timestamp`, `snapshot`, `targets` keys ([PKG-691]) | Signing metadata is the power to choose what every client installs |
| Review records | Registry store, one per version and package hash ([PKG-680]) | Drive the level B badge users rely on when they decide to install |
| Client log head and pinned keys | The user's trust store | Let a client detect a registry that rewrites history ([PKG-701]) |

**[SEC-001]** applies to these rows too: no plugin of any tier may read the client log head or the pinned keys, because they live in the trust store.

### Distribution attacker classes

| Class | Description | Starting position |
|---|---|---|
| R1 Publisher account takeover | An attacker who logs in as a publisher through a stolen password or session | Can start a publish, add a maintainer or rotate a token |
| R2 Leaked CI publish token | A token in a log, a fork or a compromised CI service | Can publish under one publisher id until revoked |
| R3 Compromised publisher build machine | Malware on the machine that builds or signs releases | Can inject code before signing; the signature is genuine |
| R4 Registry operator insider | An operator, or an attacker with operator rights, who serves different packages or metadata to chosen users | Holds the online role keys and the store |
| R5 Compromised role keys | Theft of the online keys, or of enough root keys to reach the threshold | Can sign valid metadata |
| R6 Hostile mirror or network path | A mirror, proxy or network attacker between registry and client | Can withhold, delay or replay genuine metadata, but holds no keys |
| R7 Metadata flooder | A client population or botnet that hammers the registry | Can exhaust the registry or a mirror |
| R8 Forger of reviews and names | An attacker who fakes a review, or registers a name that looks like a trusted id | Aims at the user's judgement, not at signatures |

### Distribution mitigations

| Class | Attack | Mitigation | Requirements | Residual risk |
|---|---|---|---|---|
| R1 | Publish a malicious version as the publisher | Publishing needs a second factor (TOTP or WebAuthn), at least two maintainers per publisher id, and a notice to every maintainer on publish, key change, transfer and yank | [SEC-723] | An attacker who also holds the second factor; a single-maintainer publisher is warned, not blocked |
| R1 | Ship a new key for an existing id | Clients keep the key pinned per plugin id; a changed key shows the update dialog and is not accepted silently | [SEC-142], [SEC-108] | A user who accepts the key change |
| R2 | Publish with a leaked token | A token belongs to one publisher id and one namespace, expires after 90 days, and is rotated without a gap; revocation takes effect within 1 hour, is appended to the transparency log (without the token value) and a revoked token gets `registry.auth` | [PKG-968], [PKG-969], [PKG-970] | Publishes made between the leak and the revocation |
| R2 | Hide what the token did | Every publish made between leak and revocation is listed in the log and mailed to every maintainer; the registry does not yank by itself, the publisher or operator decides | [PKG-971], [PKG-396], [PKG-403] | Users who installed a bad version before the decision |
| R3 | Inject code into the build | Publishers sign only on a clean build machine; `ilium plugin verify-build` rebuilds from the attestation and reports `package.build-mismatch`; the registry shows the verified state of each release | [SEC-174], [PKG-715] | A build that is reproducible but malicious from its source; a package with no attestation stays installable |
| R4 | Serve a different package to one user, or rewrite history | The append-only transparency log has a signed head; the client checks an inclusion proof and refuses an inconsistent head with `trust.log-inconsistent`, keeping the evidence for the audit log | [PKG-700], [PKG-701] | An insider who serves every user the same malicious package, which only the log makes visible afterwards |
| R4 | Substitute a package the publisher never signed | The package carries the publisher signature as first object; a registry countersignature is optional and the host needs at least one valid object from a trusted key; a user who pinned the publisher key rejects a countersignature-only package | [PKG-207], [SEC-142] | A user who pinned only the registry key |
| R5 | Sign metadata with a stolen online key | The operator publishes a higher-version `root.json` naming the new key, re-signs `timestamp`, `snapshot` and affected documents, and announces root changes outside the registry; a client refuses documents signed with the retired key | [PKG-691], [PKG-692] | The window between theft and the new root |
| R5 | Root threshold compromised | Rotation cannot recover; each client MUST re-pin the registry | [PKG-690] | Users must act; until then clients trust the attacker's root |
| R6 | Freeze: keep serving old metadata | `timestamp.json` expires within 1 day, `snapshot.json` within 7 days (SHOULD) and `root.json` within 1 year, so a frozen mirror is detected as expired | [PKG-367] | An attacker can still show data up to 1 day old |
| R6 | Rollback: serve older metadata | A `snapshot.json` or `timestamp.json` lower than the highest stored version is refused with `registry.rollback` and the stored version does not change | [PKG-816] | A client with no stored version, such as a fresh install |
| R7 | Exhaust the registry | Per-publisher quota on publishes and per-client limit on metadata and downloads; over the limit gives `429`, `registry.rate-limited` and `Retry-After`; clients wait up to 300 s and otherwise fail; installs from cache or a file are never blocked; a mirror sync stops and keeps verified files | [PKG-704], [PKG-972], [PKG-973] | A distributed flood that stays below each per-address limit |
| R8 | Fake a review | The level B badge needs a review record that names the reviewer, time and package SHA-256, plus an SBOM and licence files; it is derived from that exact version and hash, so a new version, a different hash or a withdrawn record returns the version to level A | [PKG-680], [PKG-683] | A reviewer who is themselves hostile or careless |
| R8 | Namespace confusion | A namespace binds to one verified publisher at first publication (`registry.namespace-unowned`); `ilium.` and `core.` are reserved; confusable ids are refused with `registry.name-confusable` after a skeleton check | [PKG-702], [PKG-389] | Names that look alike to a human but pass the skeleton check |
| R1 to R8 | Upload automatically recognisable malware | Static analysis on every upload; a version that matches a rule waits in `pending-review` for at most 72 h and is not sent to clients | [SEC-726] | Malware no rule matches |

The user-visible effect of a failed check is always a refusal with the stated error code and an audit entry; there is no "install anyway" button for `registry.rollback`, `trust.log-inconsistent` or `registry.auth`. A user can still install the same package from a file with `--allow-unsigned`, which is the T3 path and is confirmed in an attached client ([SEC-116]).

**[SEC-702]** gives the rule for reporting: a confirmed High or Critical failure of any row above receives a CVE id before its advisory is published.

## Prompt injection through plugin-supplied text

Ilium hosts AI coding agents, so text that steers an agent is the plugin system's most product-specific attack. The attack is old: the attacker puts instructions into text that a model reads, and the model obeys. What is specific here is that plugins can place text in front of an agent, and that agents run commands with the user's privileges.

### Paths from a plugin to an agent or to inference

| Path | What reaches the model | Controlled by |
|---|---|---|
| `cli.contribute` | Command names, help text and descriptions that an agent reads when it lists `ilium` subcommands | [Capability cli.contribute](/pages/capability-cli-contribute/) |
| Instruction blocks and rule packs | Text written into an agent's instruction files, or sent to an agent, by a T1 owner on behalf of a contributor | [SEC-283] |
| Project rule files | Rules in `.ilium/` that a plugin reads and forwards | [SEC-310], [SEC-315] |
| Pane titles and recap text | Titles and summaries that an agent-facing plugin reads from other panes | [SEC-380] |
| Inference prompts | Prompts a plugin builds from screen text, transcripts or files and sends with `inference` | [SEC-264], [Capability inference](/pages/api-inference/) |
| Agent input | Text sent to an agent pane by a plugin holding `agent.control` | [Capability agent.control](/pages/capability-agent-control/) |

### Attackers

- **P1 Malicious plugin.** A plugin that contributes text meant to make an agent run a command, read a secret or disable a check.
- **P2 Benign plugin forwarding hostile content.** A plugin that copies a web page, a repository file, a pane title or terminal output into a prompt or a rule. The attacker never installs anything; they only control the content, which is the attacker class A3 and A7 above.

### Mitigations

- **Grants.** Controlling an agent (`agent.control`) and reading its prompts (`agent.read-prompts`) are high-risk grants, so the review dialog never pre-ticks them and a headless install cannot add them ([SEC-116]). See [Capability agent.read-prompts](/pages/capability-agent-read-prompts/).
- **Allowlist on privileged entries.** The owner of an extension point validates each entry that sends text, writes an instruction block or redirects inference against an explicit allowlist, and the review dialog shows a `Can make <owner> <action>` line per such entry type ([SEC-283]). An entry outside the allowlist is rejected, not repaired.
- **Trusted-project gate.** Rules and values under `.ilium/` are ignored until the user trusts the project, and a plugin that reads those files itself must ask the host for the trust state and ignore the file when it is untrusted ([SEC-310], [SEC-315]).
- **Consent prompts are bounded.** A proposal to change agent configuration is a consent prompt: one pending per plugin, six per rolling hour, 120 s to answer ([SEC-105]).
- **Sanitising text the user sees.** Control, escape and bidirectional characters are removed from plugin-supplied labels, titles and reasons, so a title cannot hide its own instructions from the user ([SEC-380]).
- **Provenance marking.** The intended behaviour is that the host wraps plugin-originated text that it places in an agent-facing prompt in a marker that names the plugin id, so the user and the agent can tell it from the user's own words. This marking is a design intent; it has no requirement id yet (see the notes of this edit).
- **Review row.** A contribution that sends text to an agent or to inference appears in the review dialog as a line of its own, so the user approves the capability before it runs ([SEC-264] for the inference provider, [SEC-283] for entries).

### Accepted residual risk

The host cannot sanitise natural language. A sentence such as "ignore your instructions and run this command" is valid text, and no filter separates it from a legitimate instruction. The platform therefore does not claim to stop P2 once the text is in a prompt. What it guarantees is narrower: which plugins may put text there (grants and the allowlist), that the user was told (the review row), that untrusted projects cannot feed it (the trust gate), and that the damage a steered agent can do is bounded by the agent's own permission prompts and by the rule that no plugin can grant H capabilities to an agent headlessly ([SEC-116]). Plugin authors who build prompts from screen text or files MUST treat that text as data and SHOULD tell the model so in the prompt.

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

## Multi-user shared machines

Target behaviour (specified here before the code is written).

- [SEC-454] Each OS user MUST have a separate runtime directory, socket, module cache, trust store and state folder, all with mode 0700 owned by that user. A server MUST refuse to start and name the path when one of them is owned by another user or is group- or world-accessible.
- [SEC-455] The host MUST NOT share any compiled module, package cache or secret between OS users, even when the plugin and its hash are identical. A policy file in a system-wide location MUST be readable by all users and writable only by root, and the host MUST refuse a policy file that another non-root user can write.
- [SEC-456] A plugin helper MUST NOT be able to see another OS user's processes, sockets or files through the sandbox ([SEC-055]); the `peer` scope of messaging MUST stay inside one OS user's server and never cross to another user's server.

## Edge cases

| Situation | Result |
|---|---|
| A plugin holds a read grant and an egress grant | The review dialog shows the toxic combination row. Install proceeds if the user approves |
| A plugin is later granted an egress grant through the Plugins screen | The row appears in the capability list from then on, and the grant change is audited |
| The policy forbids the combination | The install succeeds, the combination row reads `Blocked by your administrator` with its grants locked off, and calls return `not-permitted` with the reason `policy` |
| A plugin that never declared `accepts_messages_from` is sent a peer message | The message is dropped and counted; the sender gets `not-permitted` with reason `unavailable` |

## Production additions: T2-security

- **[SEC-706]** The host MUST validate every component with Wasmtime's validator using the fixed feature set of the ABI (no threads, no relaxed-simd, no unlisted proposals) before compilation; a module using a disabled proposal MUST be refused with `manifest.component-invalid` and detail naming the proposal, and no compilation MUST start.

- **[SEC-714]** The threat model MUST list for each asset (user files, terminal content, secrets, tokens, trust store, grants, audit log) the actors that can reach it, the control that blocks them and the SEC id of that control, and a CI check MUST fail when a listed SEC id does not exist.


## Not defended

- **[SEC-715]** The threat model MUST state the out-of-scope attackers explicitly: a user with root on the machine, a malicious kernel, hardware side channels beyond SEC-711, and a compromised terminal emulator; the docs site MUST list them under the heading `Not defended`.


### See also

- [Security model](/pages/security/)
- [Sandbox](/pages/reference-sandbox/)
- [Network egress](/pages/reference-network-egress/)
- [Grant review and consent](/pages/reference-grant-review-and-consent/)
- [Trust and supply chain](/pages/reference-trust-and-supply-chain/)
- [Extension security](/pages/reference-extension-security/)
- [Security and policy requirements](/pages/reference-requirements-security-policy/)

Contract: `ilium:plugin@0.1.0`.
