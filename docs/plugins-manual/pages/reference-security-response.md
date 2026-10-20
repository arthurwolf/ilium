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

## Edge cases

| Situation | Result |
|---|---|
| Safe mode and a `force_enabled` plugin from the policy | The plugin stays off if it is third-party; the policy shows `conflict: safe-mode` |
| Safe mode and a bundled T1 plugin | Runs normally |
| Report arrives for a plugin and the host together | Two advisories, one for each party |
| User runs `ilium --safe-mode` against a running server | The command starts a new session in safe mode and does not disturb the running one |

### See also

- [Trust and supply chain](/pages/reference-trust-and-supply-chain/)
- [Lifecycle](/pages/lifecycle/)
- [User security and privacy](/pages/user-plugin-security-and-privacy/)
- [Audit log](/pages/reference-audit-log/)

Contract: `ilium:plugin@0.1.0`.
