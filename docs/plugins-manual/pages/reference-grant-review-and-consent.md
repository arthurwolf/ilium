---
layout: page
title: Grant review and consent
permalink: /pages/reference-grant-review-and-consent/
---

## Grant review and consent

This page defines how a user decides what a plugin may do, how those decisions are stored, and how they are withdrawn. It replaces the three slightly different install descriptions in earlier pages with one pipeline. Mechanisms for enforcement are in the [Security model](/pages/security/). The attacker classes this page defends against are A1, A2 and A8 in the [Threat model](/pages/reference-threat-model/).

A **grant** is a standing permission that the user gives a plugin at install, for example `fs.read:project`. A **consent** is a one-off answer to a question asked at first use, for example whether a plugin may open `https://status.example.com`. The **review dialog** is the kernel-drawn screen that lists the grants a package asks for. The **authorization epoch** is the counter that changes when a grant changes ([Security model](/pages/security/)).

### You will need

- A package, local or signed. See [Packages](/pages/packages/).
- An attached Ilium client (the TUI). Decisions about grants and trust need one ([SEC-116]).
- The capability table in [Capability table](/pages/reference-capabilities/).

## The install pipeline

**[SEC-100]** `ilium plugin install`, the Plugins screen and `ilium plugin update` MUST run this one pipeline, in this order. The first failing step stops the install and writes one JSONL `error` line with the code. Nothing is written to the install directory before step 11.

| Step | Check | Failure code |
|---|---|---|
| 1 | Open the archive with the hardened reader | `package.path-invalid`, `package.path-duplicate`, `package.entry-symlink` or `package.entry-directory`, `package.zip-invalid` |
| 2 | Layout and API version; unknown manifest keys | `manifest.api-version`, `manifest.unknown-key` |
| 3 | Inventory: every file listed with length and SHA-256; asset size | `manifest.file-unlisted`, `manifest.file-missing`, `manifest.file-length`, `manifest.file-hash`, `manifest.asset-size` |
| 4 | Signature (T2) verifies; publisher name matches the key's record | `manifest.signature-invalid`, `manifest.publisher-mismatch` |
| 5 | Blocklist (release and policy) | `package.blocked` |
| 6 | Key trust and key pinning: an untrusted key is offered for trust; an installed id signed by another key is refused | `trust.declined`, `package.publisher-key-changed` |
| 7 | Policy: tier, id, publisher, source | `policy.tier-denied`, `policy.id-denied`, `policy.publisher-denied`, `policy.source-denied` |
| 8 | Grant strings parse and fit the tier; dependencies between grants | `manifest.grant-tier`, `manifest.grant-argument`, `manifest.grant-argument-missing`, `manifest.grant-wildcard`, `manifest.grant-scope`, `manifest.grant-dependency`, `manifest.limits-tier`, `manifest.secret-scope`, `manifest.replaces-tier` |
| 9 | Policy: replacement, then dependency resolution (package-format stage 10: `requires.plugins`, `requires.services`, host version, cycles). A grant denied by `grants.deny` is not checked here and does not fail the install; step 10 shows it locked off | `policy.replacement`, `dependency.unresolved`, `dependency.conflict`, `dependency.cycle`, `dependency.host-version`, `manifest.service-cycle` |
| 10 | Review dialog (see below) | user cancels: exit code 1, audit `install` with result `install.review-declined` |
| 11 | Commit: unpack into the install directory, write the grants, bump the epoch, write the audit line | `install.write-failed` |

**[SEC-123]** Declining to trust a key at SEC-100 step 6 refuses the install with `trust.declined`. The package is never downgraded to T3 silently. A user who wants it as a local package passes `--allow-unsigned` explicitly, which strips the signature view and gives a T3 install with the `dev` badge.

**[SEC-124]** `sessions.cross` in a T3 package is refused at SEC-100 step 8 (grants fit the tier; validation stage 9) with `manifest.grant-tier`. `sessions.cross` is for T1 and T2 only.

## The review dialog

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

**[SEC-102]** The review dialog MUST be drawn by the kernel (the client) only. It is modal over every plugin widget. No plugin widget can draw inside, over or in the reserved dialog border. No plugin input event reaches the dialog. The same holds for the consent prompts, breach notices, the recovery page and policy lock marks.

**[SEC-106]** Rows. Sections by side (Server, Client, Scene). A high-risk grant row starts unchecked and carries the text `HIGH` (not a symbol). Normal rows start checked. A row for a grant that the policy denies (`grants.deny`, see [SEC-198]) is locked off and reads `Blocked by your administrator`. Each row names the grant in the right column. The dialog also shows, as separate groups: **Adds to other plugins** (extension contributions), **Replaces** (a core plugin), **Receives data from other plugins**, and the **toxic combination** row ([SEC-004]) when the requested set holds, in the same side, at least one grant of the data-read set and at least one grant of the egress set, both enumerated in [SEC-006]. The toxic row is informational and has no checkbox of its own: it cannot be unchecked, and it disappears when the user unchecks either member grant.

**[SEC-103]** Protection against accidental approval. The default focus is on **Cancel**. The dialog ignores `Enter` and `Space` for 500 ms after it opens and while a key is auto-repeating (a second press of the same key within 80 ms of the previous one, without release). The dialog ignores input that arrived before it was drawn. A plugin cannot open the dialog while the user types in a terminal (see [SEC-377]). At most one review dialog is open at a time; others queue.

**[SEC-104]** Names. The publisher is shown as `Name (self-declared)` next to the verified fingerprint. Display names, plugin titles and publisher names shown in the dialog have control characters, bidirectional overrides and isolates, and zero-width joiners removed; and they are shortened with an ellipsis to 64 characters for display only. This shortening is a display rule and never refuses an install. The install-time name rules (3 to 40 characters, no kernel-name spoofing) are in [SEC-381](/pages/reference-extension-security/).

**[SEC-107]** Details (reachable with Tab) shows: full SHA-256 fingerprint in groups of 4 hex digits, package SHA-256, the tier, `security_contact` if present, `data_use` if present, each grant with its risk, each file's size, and the optional provenance entries ([Trust and supply chain](/pages/reference-trust-and-supply-chain/)).

**[SEC-121]** Accessibility. Every row has a text label that a screen reader announces as `<state>, <risk>, <title>, <grant name>`. The dialog never relies on colour alone.

**[SEC-122]** If rows exceed the height of the terminal, the list scrolls. A scroll indicator shows `n of m`. The footer stays visible.

### Trust dialog

<figure>
<pre class="term"><span class="t-hi">╭─ Trust this publisher? ──────────────────────╮</span>
│ Self-declared name: Example Corp             │
│ Key fingerprint (SHA-256):                   │
│ 3F2A 91C0 7B6D 1E44 A1B2 0C9E 55D3 F7A8      │
│ B6C1 02E9 4D77 98AF 13C5 6E0B D2F4 8A19      │
│                                              │
│ Compare this with the publisher over a       │
│ channel you trust before you continue.       │
│                                              │
│ <span class="t-hi"> Esc </span> Do not trust   <span class="t-hi"> Enter </span> Trust           │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Full fingerprint, two groups of 8 hex digits per line pair.</figcaption>
</figure>

The fingerprint is the SHA-256 of the raw public key, shown as 64 hex digits in 16 groups of 4. The short form `3F2A91C0` is the first 8 hex digits and appears only next to the full form or in `trust list`.

**Localisation of dialog text.** The review dialog, consent prompts and notices use the host's own wording for fixed text. Text supplied by the plugin (the grant reason, a toast body) is shown verbatim as data, cut with `…`, and is never translated by the host. Ids stay identifiers. See [Internationalisation](/pages/guide-internationalisation/) ([AUT-618] to [AUT-622]).

## Updates

<figure>
<pre class="term"><span class="t-hi">╭─ Update example.pane-tools 2.1.0 ────────────╮</span>
│ Same signing key as 2.0.0.                   │
│                                              │
│ <span class="t-dim">New</span>                                          │
│ [ ] <span class="t-warn">HIGH</span> Read scrollback    terminal.history │
│ <span class="t-dim">Widened</span>                                      │
│ [ ] Send data to *.example.com …………………… http │
│ <span class="t-dim">Removed</span>                                      │
│ Show toasts ………………………………………………… notify.toast │
│                                              │
│ Unchanged grants keep their answers.         │
│                                              │
│ <span class="t-hi"> Esc </span> Cancel   <span class="t-hi"> Enter </span> Update                │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The update dialog lists only what changed.</figcaption>
</figure>

**[SEC-108]** An update dialog MUST show three groups. **New**: grants not held before; a new high-risk grant is unchecked and a new normal grant is checked. **Wider**: a grant whose argument now covers more than before; always unchecked. **Removed**: grants the new version no longer lists, shown for information. A widened grant replaces the old one, so the old grant string also appears under Removed. Unchanged grants keep their state and are not shown. Always-granted entries are never shown. Test: update from {`fs.read:data`, `http:a.example`} to {`fs.read:project`, `http:*.example`}; assert New=[`fs.read:project`], Wider=[`http:*.example`], Removed=[`http:a.example`].

Widening is defined for these real argument forms only. Any other argument change is Removed plus New.

| Family | Widened when |
|---|---|
| `commands.invoke:<arg>` | an exact id becomes `<plugin-id>/*`, or `<plugin-id>/*` becomes `*` |
| `http:<host>` | an exact host becomes `*.<domain>` that covers it. Adding a port is a new grant. `http:setting:<key>` is never widened |
| `fs.*:path:<template>` | the template is replaced by an ancestor template (`path:{home}/a/b` to `path:{home}/a`) |
| `fs.*:<scope>` | the scope becomes a broader named scope (`project` to `path:{home}/**`), or read becomes write |
| `process.run:<program>` | a program name becomes an absolute path: a new grant, not widening |
| `extensions.*:<point>` | a different point: Removed plus New |
| risk | a normal grant becomes high: New |

An answer the user gave to an unchanged grant is kept. New and widened grants start in their default state (widened: unchecked). A removed grant is dropped.

**[SEC-109]** A change of the publisher key that no rotation statement endorses is refused for an update ([PKG-237]); the user must uninstall the plugin first. A change of the tier (T2 to T3 or T3 to T2) resets all grants and consents of the plugin and shows the full dialog as for a first install. A key change that is endorsed by a rotation statement ([SEC-143]) shows the update dialog and a line `Signing key rotated`.

<figure>
<pre class="term"><span class="t-hi">╭─ Publisher key changed ──────────────────────╮</span>
│ example.pane-tools 2.2.0 is signed by a      │
│ different key than the installed 2.1.0.      │
│                                              │
│ Installed key  3F2A91C0 7B6D1E44             │
│ New key        A1B20C9E 55D3F7A8             │
│                                              │
│ The update is refused. To use this package,  │
│ uninstall the plugin first. Its grants and   │
│ data settings will be asked for again.       │
│                                              │
│ <span class="t-hi"> Enter </span> Close                                │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The kernel refuses an update that changes the signing key without an endorsement.</figcaption>
</figure>

## Consent prompts

**[SEC-105]** Consent prompts (origin for `open-external`, host from a setting, geocoding, bundled first start, agent configuration proposals) are limited. At most one prompt is pending per plugin. At most 6 prompts are shown per plugin in a rolling hour. The call that triggers a prompt waits for the answer, up to 120 s; then it returns `timed-out`, the prompt is closed and nothing is stored. A second consent-gated call from the same plugin while one prompt is pending returns `busy` with `retry-after-ms` = 1000 and shows no prompt. The 7th prompt in a rolling hour returns `rate-limited` with `retry-after-ms` = the time until the oldest prompt leaves the window. Test: trigger two consent calls at once and assert one prompt and one `busy`; trigger seven in an hour and assert `rate-limited` on the 7th. A prompt is a kernel dialog and cannot be answered by any event a plugin can cause.

**[SEC-110]** After a user declines the same target three times, the plugin gets `not-permitted` with reason `declined` for that target without a prompt, until the user resets it in Settings > Plugins > Consents or with `ilium plugin consents <id> --withdraw <target>`.

<figure>
<pre class="term"><span class="t-hi">╭─ Allow once more? ───────────────────────────╮</span>
│ Notes Panel wants to open this site again:   │
│                                              │
│ https://status.example.com                   │
│                                              │
│ You declined this site twice before.         │
│                                              │
│ <span class="t-hi"> Esc </span> Decline        <span class="t-hi"> Enter </span> Allow           │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. After three declines the prompt stops until the user resets it.</figcaption>
</figure>

**[SEC-111]** Bundled first start. A T1 plugin with H grants is activated without them. The kernel asks at first activation. Every row of the prompt starts unchecked, and focus starts on **Not now**. Enter with no row checked equals **Not now** and counts as one of the three asks. If the user answers **Not now**, the question comes back at the next server start. After three **Not now** answers (counted per plugin) the prompt stops and the grants stay off until the user enables them in Settings > Plugins > Capabilities.

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
<figcaption>Rendered at 48 columns. Illustrative. Bundled plugins start without their high-risk grants, and every row starts unchecked.</figcaption>
</figure>

## Where decisions are stored

**[SEC-112]** The grant store, consent store, trust store and project trust store are separate files in the Ilium user state directory (Linux `$XDG_STATE_HOME/ilium/`, macOS `~/Library/Application Support/Ilium/state/`, Windows `%LOCALAPPDATA%\Ilium\state\`). Each file is created with mode 0600 in a directory with mode 0700 and is owned by the user. No store lives in a project folder, and none is reachable by any plugin scope ([SEC-060]).

| File | Content |
|---|---|
| `grants.json` | `{ "version": 1, "plugins": { "<id>": { "epoch": n, "tier": "t2", "key": "<fingerprint>", "grants": { "server": [...], "client": [...] }, "installed": "<ISO 8601>" } } }` |
| `consents.json` | `{ "version": 1, "plugins": { "<id>": { "open-external": { "<origin>": "allow" or "deny:<count>" }, "http-hosts": {...}, "geocode": "allow" } } }` |
| `trust.json` | `{ "version": 1, "keys": [ { "fingerprint", "publisher", "added", "public_key" } ] }` |
| `project-trust.json` | `{ "version": 1, "projects": { "<canonical path>": { "trusted": true, "added": "<ISO 8601>" } } }` |

**[SEC-114]** Stores are written atomically (temporary sibling, fsync, rename) and carry a `version`. A file with a newer version than the server knows is not modified and the server reports `store.version`. A file that cannot be parsed is renamed to `<name>.corrupt-<YYYYMMDD-HHmmss>`, the server logs `grants.unreadable` (`trust.unreadable` for the trust store), starts with no entries from it, and draws a notice. T2 and T3 plugins then start as not installed with no grants, and T1 plugins start with only their normal grants; grants and consents are asked again (fail closed).

**[SEC-113]** The server re-reads a store file only at start. External edits made while the server runs are ignored until the next start. At start, entries that appear in a store but were not written by the server (the file hash differs from the hash the server recorded in the audit log at its last write) are audited as `store-external-edit`. External edits MUST NOT add a high-risk grant: an H grant found in a store without an audit record of a user decision is dropped. An administrator uses the policy file `preapproved` list instead ([Admin policy](/pages/reference-admin-policy/)).

## Who may approve

**[SEC-116]** A decision that adds a grant, trusts a key, installs a package with `--allow-unsigned` or marks a project trusted MUST be confirmed in a dialog in an attached TUI client of the same session, or by the exact-item flags of [SEC-117]. When `ilium plugin install` runs in a shell pane (the `ILIUM_PANE_ID` variable is set), the CLI asks the server to open the dialog in the client, and the dialog names the requesting pane: `Requested from pane "shell 3"`. With `--yes` the dialog is skipped for normal grants (see [PKG-429]). If no client is attached and `--yes` is not given, the CLI waits up to 60 s, prints `waiting for a client to approve`, and then fails with `install.review-required` (exit 1).

**[SEC-117]** No command-line flag grants a high-risk capability, trusts a key or approves a project without that confirmation, except the flag that names the exact item: `--grant <grant>` for one high-risk grant and `--trust <key_id>` for one key ([PKG-282], [PKG-429]). `--yes` alone never does. The only exception is a grant that the admin policy lists in `preapproved` for that plugin id, which requires no dialog. A prompt-injected agent that runs `ilium plugin install pkg --yes` gets only the normal grants.

## Revocation and uninstall

**[SEC-118]** The user can withdraw a decision at any time: one grant, one consent, one picked path, or all of a plugin's grants and consents (**Revoke all**). Effects are those in the [Security model](/pages/security/): in-flight calls return `revoked`, the epoch bumps, contributions are withdrawn.

**[SEC-120]** CLI. `ilium plugin consents <id>` prints consents as JSONL. `ilium plugin consents <id> --withdraw <target>` withdraws one. `ilium plugin grants <id> --revoke <grant>` revokes one grant. These two subcommands only revoke or withdraw and never grant; a grant from the command line comes only from the exact-item flags of [SEC-117] ([SEC-116]).

**[SEC-119]** Uninstall forgets the plugin's grants, consents and picked paths. Its data is kept unless `--purge` is passed ([PKG-294]); the cache is always removed. A reinstall starts with a fresh review dialog.

## Edge cases

| Situation | Result |
|---|---|
| The user holds Enter while the dialog opens | Ignored for 500 ms and while repeating; the dialog stays open |
| A plugin tries to open a second review dialog | Queued; the first stays on top |
| The terminal is smaller than the dialog minimum (40 by 12) | The dialog shows `Terminal too small` and Cancel only |
| Two clients attached | The dialog opens in the client that most recently had input; the other shows `Waiting for approval` |
| Grants store missing | All third-party plugins are treated as not installed and the user is told |
| Package re-signed with the same key, same version and same manifest | No change: `install.already-installed` warning, exit 0 ([LIF-072]) |
| Same version, different manifest SHA-256 | `install` refuses with `package.version-reused` ([PKG-274]) |

## Production additions: T2-security

- **[SEC-717]** A grant MAY carry an expiry. `ilium plugin grant --until <timestamp>` MUST store it, the host MUST treat an expired grant as absent at the next call, write the audit line `grant-expired`, and show the grant as `expired` in the Plugins screen for 30 days (proposed, owner to confirm).

- **[SEC-719]** The consent prompt MUST NOT be drawable, answerable or dismissible by any plugin, MUST rate-limit to at most one prompt per plugin per 30 s (proposed, owner to confirm), and MUST auto-deny after 120 s (proposed, owner to confirm) without an answer, recording `consent-timeout`.

- **[SEC-720]** When an update adds a capability or widens a scope, the host MUST keep the old version running with the old grants until the user answers, and a non-interactive session MUST deny the widening and keep the old version.


## Production additions: T7-users-privacy-a11y

- **[USR-318]** Each consent decision (grant, decline, revoke, key trust, link open, host confirm, replaces confirm) MUST be stored with plugin id, version, grant string, decision, UTC time `YYYY-MM-DD HH:mm:ss` and the surface (`dialog`, `cli`, `policy`), and `ilium plugin consents <id>` MUST list them oldest first.

- **[USR-319]** Every consent given through a dialog MUST be revocable from the Capabilities tab in at most 2 key presses from the plugin's Details page, and revoking MUST NOT require a reason, a confirmation for a normal grant, or a restart of Ilium.

- **[USR-321]** A review dialog MUST show the plain wording of a permission untruncated (wrapping or scrolling), MUST show how many rows are below the fold, and Confirm MUST stay disabled until the list has been scrolled to its end when more than one screen of rows exists.


### See also

- [Security model](/pages/security/)
- [Packages](/pages/packages/)
- [Trust and supply chain](/pages/reference-trust-and-supply-chain/)
- [Admin policy](/pages/reference-admin-policy/)
- [Audit log](/pages/reference-audit-log/)

Contract: `ilium:plugin@0.1.0`.
