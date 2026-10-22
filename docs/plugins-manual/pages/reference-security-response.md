---
layout: page
title: Security response
permalink: /pages/reference-security-response/
---

## Security response

This page says how to report a security problem in Ilium or in a plugin, what the Ilium project does with the report, how a bad plugin is stopped, and how a user starts Ilium with third-party code off. It is a process page: the dates and contact are commitments of the project, and a test cannot check them, but the safe-mode behaviour is testable.

### You will need

- For a report: a way to send encrypted email, and a description with steps to reproduce.
- For safe mode: the `ilium` command line.

## Reporting a vulnerability in Ilium

**[SEC-320]** The project publishes a security contact in `SECURITY.md` at the repository root and in `ilium --version --verbose`. Until the project publishes an address the contact is the placeholder `security@ilium.example` and a PGP key published next to it. Reports go there, not to public issues.

A report should say: the version (`ilium --version --verbose`), the platform, what you did, what you expected, what happened, whether a plugin was involved, and whether you have disclosed it elsewhere.

**[SEC-321]** The project acknowledges a report within 3 working days, gives a first assessment within 10 working days, and keeps the reporter informed at least every 14 days. The embargo is 90 days from the report, or until a fix is released, whichever is first. The reporter and the project may agree on a shorter or a longer embargo. The project does not take legal action against good-faith research that follows this process.

**[SEC-322]** A confirmed issue gets a CVE identifier, a security advisory in the repository (`GHSA` style) and an entry in the release notes. An advisory states: affected versions, fixed versions, severity, a description, a workaround if one exists, credit (if the reporter wants it), and the requirement ids concerned (for example `SEC-023`).

**[SEC-323]** Supported versions. The latest minor release of Ilium and the one before it receive security fixes.

**[SEC-324]** Severity. The project rates issues on a four-level scale:

| Level | Examples in this platform |
|---|---|
| Critical | A sandbox escape from a T2 or T3 plugin; a plugin that runs unconfined code without a grant; reading the HTTP API token from a plugin |
| High | A grant check bypass; a consent bypass; reading a file outside a granted scope; a signature bypass; unauthenticated use of the HTTP API |
| Medium | A data leak through an error message or event; a denial of service of the server by a plugin that quotas should stop; spoofing of a kernel dialog |
| Low | A side channel with a small bandwidth; a missing audit line; a wrong error code |

## Reporting a vulnerability in a plugin

**[SEC-325]** A plugin SHOULD set `security_contact` in its manifest ([SEC-268]). The Details view of the install dialog and `ilium plugin inspect` show it. A user who finds a flaw reports it to that contact and not to the Ilium project, unless the flaw is in the host.

**[SEC-326]** When a publisher does not answer a serious report within 30 days, the Ilium project MAY add the package, the plugin id with a version range, or the publisher key to the release blocklist ([SEC-146]) and publish an advisory. The project does not host plugins, so it has no other way to stop a plugin that is spread as files.

## Takedown

**[SEC-327]** When a blocklist entry matches an installed plugin, the plugin is disabled at the next load, its data is kept, the user sees `Blocked: <reason>`, and the audit log records `blocklist-hit` ([SEC-149]). The user can uninstall it. The user can turn it on again only with `ilium plugin enable <id> --accept-risk`, or by installing an update signed by another trusted key of the same publisher.

## Safe mode

Safe mode starts Ilium with all third-party code off, for the case where a plugin breaks the client so that the recovery page cannot be reached.

**[SEC-328]** `ilium --safe-mode` without a value is `--safe-mode=third-party`. The environment variable `ILIUM_SAFE_MODE` accepts `third-party`, `all-optional`, `1` (the same as `third-party`) and `0` (off); any other value exits 2 with `usage.invalid-value`. `--safe-plugins` is an alias of `--safe-mode=third-party`, and `--no-plugins` starts no plugin host. The flag wins over the variable. The flags start the server and client in safe mode. In safe mode: all T2 and T3 plugins are disabled; all core plugin replacements are off and the built-in plugins are restored; the admin policy still applies; no plugin is deleted and no grant, consent or data is changed. The flag is not stored: the next normal start returns to the previous state.

**[SEC-329]** A persistent badge `SAFE` is shown in the status bar while safe mode is on. The Plugins screen lists the disabled plugins with `Off in safe mode`, and the user can uninstall them from there.

<figure>
<pre class="term"><span class="t-hi">╭─ Safe mode ──────────────────────────────────╮</span>
│ Third-party plugins and replacements are     │
│ off. Built-in plugins are restored.          │
│                                              │
│ example.pane-tools ……………………………………………………… off │
│ example.notes …………………………………………………………………… off │
│                                              │
│ <span class="t-hi"> Enter </span> Continue                             │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The badge SAFE stays in the status bar while safe mode is on.</figcaption>
</figure>

**[SEC-330]** The server writes `safe-mode` to the audit log at start. A project that holds `.ilium/` plugin values is treated as untrusted in safe mode.

## Publisher incident checklist

1. Stop distributing the affected package.
2. If the private key may be exposed: stop signing with it. If you still control it, sign a rotation statement for a new key ([SEC-143]) and ship a fixed version signed by the new key. If you no longer control it, ask the Ilium project for a blocklist entry for the key.
3. Ship a fixed version with a higher version number and a short description of what changed.
4. Tell users: uninstall the old version, trust the new fingerprint (compare it through a channel other than the package), and review the grants.
5. Publish a note with affected versions and the fix.

## Dependency advisories

**[SEC-331]** An advisory in Wasmtime or a codec that applies to the pinned version is handled as in [SEC-171]: a patched release within 14 days of the upstream fix (48 hours when exploited in the wild), a release-blocklist entry for the vulnerable runtime, and a warning in Ilium until the user updates.

## Publish rate limits and abuse reports

This section is target behaviour for the public plugin system. The registry protects itself and its users from floods of publications and downloads and from malicious packages. This section sets the limits and the abuse intake.

- [PKG-704] The registry MUST apply a per-publisher quota to publish requests and a per-client rate limit to metadata and download requests. A request over a limit MUST be refused with `registry.rate-limited` and a `retry-after` value, and a limit MUST NOT stop installs from an already cached package or from a file.
- [PKG-705] The registry MUST accept abuse reports for any published package through a documented intake. It MUST acknowledge a report within 1 business day, MUST decide on a report of active malware or a stolen key within 24 hours (yank or takedown, or a stated reason for none), and MUST record each decision in the transparency log.

## Coordinated disclosure policy

Target behaviour (specified here before the code is written).

- [SEC-439] The project MUST publish a `security.txt` (RFC 9116) at the documentation site's `/.well-known/security.txt` with the contact, the preferred language, the policy link and an expiry date no more than 12 months ahead. It MUST name the same contact as `SECURITY.md` ([SEC-320]).
- [SEC-440] The disclosure policy MUST state a safe harbor: the project MUST NOT start or support legal action against a reporter who acts in good faith, tests only their own installation, stops at proof of the issue and keeps the report private until the embargo of [SEC-321] ends.
- [SEC-441] A reporter who asks for it MUST be credited by name or handle in the advisory of [SEC-322]; a reporter who asks for anonymity MUST NOT be named. The project MAY shorten the embargo by agreement with the reporter, and MUST NOT lengthen it past the 90 days of [SEC-321] without the reporter's written consent.

## Edge cases

| Situation | Result |
|---|---|
| Safe mode and a `force_enabled` plugin from the policy | The plugin stays off if it is third-party; the policy shows `conflict: safe-mode` |
| Safe mode and a bundled T1 plugin | Runs normally |
| Report arrives for a plugin and the host together | Two advisories, one for each party |
| User runs `ilium --safe-mode` against a running server | The command starts a new session in safe mode and does not disturb the running one |

## Reporting a vulnerability in the host or sandbox

The general intake above covers all of Ilium. A flaw in the plugin host or in the sandbox is more serious, because it breaks the guarantee that every plugin relies on, so it has its own handling.

- **[HST-520]** A report about the plugin host, the Wasmtime configuration or the sandbox helper MUST be rated at least High, and a sandbox escape MUST be rated Critical. The intake MUST be the security contact in `SECURITY.md`, and the project MUST acknowledge such a report within 2 working days.
- **[HST-521]** For a confirmed host or sandbox flaw the embargo MUST be 90 days from the report or until a fix is released, whichever is first. A fix MUST be released within 30 days for a Critical flaw and within 60 days for a High flaw, and the advisory MUST name the affected tiers and say whether the flaw can be reached without a grant. The project MAY shorten the embargo when the flaw is exploited in the wild.

## Emergency disable of an installed plugin version

The blocklist ([SEC-146]) stops a plugin at the next load. These requirements add the target behaviour for a registry-pushed emergency disable that reaches hosts which already run the plugin.

- [EXT-057] The host MUST poll the signed emergency-disable feed of each configured registry at least every 6 hours and MUST verify it with the registry key before use. An entry that fails verification MUST be ignored and logged as `quarantine.feed-invalid`.
- [EXT-058] An entry that matches an installed plugin version MUST put that version in quarantine within 60 s of the next poll: running instances MUST be stopped, the package MUST NOT be instantiated again, its data MUST be kept, and the user MUST see `Quarantined: <reason>`.
- [EXT-059] A quarantine MUST be lifted only by a newer signed feed entry that withdraws it or by an upgrade to a version the feed does not list. A user or an administrator MUST NOT be able to lift it by editing local files. When the feed is unreachable, existing quarantines MUST stay in force and the audit log MUST record `quarantine.feed-unreachable`.
- [EXT-069] The security contact in `SECURITY.md` MUST accept reports about the host and about plugin sandbox escapes through at least one encrypted channel, and a report MUST receive a tracking identifier with the acknowledgement defined in [SEC-321].
- [EXT-070] During an embargo the project MUST NOT publish the details of an unfixed issue. When a fix ships, the advisory MUST state which plugin tiers are affected and the date the reporter was told, and a reporter who asks for it MUST be credited.

### See also

- [Trust and supply chain](/pages/reference-trust-and-supply-chain/)
- [Lifecycle](/pages/lifecycle/)
- [User security and privacy](/pages/user-plugin-security-and-privacy/)
- [Audit log](/pages/reference-audit-log/)

Contract: `ilium:plugin@0.1.0`.

## Production additions: T2-security

- **[SEC-700]** A security advisory MUST be published as a machine-readable JSON document (OSV schema 1.6 (proposed, owner to confirm)) beside the human page, and MUST contain `id`, `aliases` (CVE if assigned), `affected` ranges by plugin id or `ilium-host` version, `severity` (CVSS v4 vector and score), `fixed` version, `published` and `credits`.
- **[SEC-702]** The project MUST request a CVE id through a CNA for every confirmed High or Critical host, sandbox or registry flaw before the advisory is published, and the advisory MUST list that id in `aliases` (SEC-700).
- **[SEC-703]** An advisory for a third-party plugin MUST be sent to the publisher privately first; if the publisher does not acknowledge within 14 days the registry MAY quarantine the version through the emergency feed (EXT-057) and MUST publish the advisory 90 days after the first contact, whether or not a fix exists.
- **[SEC-729]** For an active exploit the project MUST be able to ship an emergency-disable entry (EXT-057) within 4 hours (proposed, owner to confirm) of confirmation, MUST publish an incident status page, and MUST write a public post-mortem within 30 days (proposed, owner to confirm) that lists timeline, root cause and the SEC ids changed.
