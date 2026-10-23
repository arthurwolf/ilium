---
layout: page
title: Support policy
permalink: /pages/support/
---

## Support policy for users, administrators and authors

This page states what support the Ilium project gives, what it does not give, and how to ask for it. It applies to users, administrators and plugin authors alike. Where the manual elsewhere says the host project "has no general support channel", this page is the full statement: support is community support only.

### You need

- The host version you run, from `ilium --version --verbose`.
- The output of `ilium plugin doctor` (see [Diagnostics](/pages/core-plugin-diagnostics/)).
- For a plugin failure, a support bundle from `ilium plugin support-bundle` (see below).

### What the project offers

The project offers community support through its public channels. Each channel has one purpose.

| Channel | Use it for | Do not use it for |
|---|---|---|
| Issue tracker | A reproducible defect in the host, in a core plugin or in this manual, and a concrete feature request | A vulnerability, a question about a third-party plugin, a request for private help |
| Discussions | How-to questions, design questions, sharing a recipe, asking whether something is a defect | A vulnerability, a defect report that already has a reproduction (file it in the tracker) |
| Security contact | A vulnerability in the host or the sandbox, routed as described under "Security reports" | Ordinary defects and questions |

The project does not state a response time. Nobody is obliged to answer, and a missing answer is not a decision. The project sells no support contract. Commercial support exists only if a named party offers it in its own words; that offer binds that party and not the project, and the project does not vouch for it.

### What a good report contains

A report that lacks these items is answered with a request for them, so include them the first time.

1. The host version from `ilium --version --verbose`, the platform, and the host API range.
2. What you did, what you expected and what happened, with the error code if one was shown (for example `manifest.unknown-key`).
3. The full output of `ilium plugin doctor`. It prints one line per check with `ok`, `warn` or `fail`, and it exits 0 only when no line is `fail` ([USR-337]).
4. For a plugin failure, a redacted support bundle (next section).
5. The manual snapshot you consulted, as the host release written on the snapshot's pages ([LIF-962]).

### Support bundles

`ilium plugin support-bundle <id>` builds the bundle. The default name is `ilium-support-<id>-<YYYYMMDD-HHmmss>.zip`, and `summary.txt` lists the Ilium version, plugin id, version, tier, state, platform, host API range, failure count, redaction counts and dropped log lines ([USR-335]). The host shows the redacted `summary.txt` and `log.txt` in a viewer, never uploads a bundle itself, and `--dry-run` prints the file names, sizes and redaction counts per rule without writing the zip ([USR-332]).

The data-handling rules are:

- A bundle is sensitive data. Read `log.txt` before it leaves your machine, and do not send a bundle that holds personal data the recipient is not entitled to receive ([USR-962]).
- Attach a bundle to a public tracker issue or discussion only after you have read it and found nothing you would not publish. If in doubt, describe the failure in text and offer the bundle privately.
- An administrator sends a bundle outside the organisation only to the plugin's `security_contact` or to the Ilium project contact, over a channel the organisation approves ([USR-960]).
- The project keeps a bundle only as long as it needs it to resolve the report, and an administrator keeps a copy no longer than 90 days after the incident is closed, deleting every copy including copies in tickets and mail ([USR-961]).

### Answers cite the matching snapshot

A support answer about a plugin or a host cites the manual snapshot of the host version the user runs, not the current manual. An answer that cites the current manual for an older host says that the behaviour may differ ([LIF-974]). If you cite a page yourself, cite the snapshot path for your release and see [Documentation versions](/pages/reference-documentation-versions/).

### Whose problem it is

| Symptom | Go to |
|---|---|
| The log or a trace shows Ilium itself, and not the plugin, caused the failure | The Ilium project issue tracker, with the items above ([AUT-954]) |
| A plugin misbehaves, is wrong, or is unmaintained | The plugin's own support route: the `homepage` and `repository` in its manifest and its README |
| A vulnerability in a plugin | The plugin's `security_contact` (see [Security response](/pages/reference-security-response/)) |
| A vulnerability in the host or the sandbox, including a sandbox escape from a plugin | The Ilium security contact, not a public issue |
| A question about a plugin you wrote | Authors answer this themselves; see [Publishing and support](/pages/guide-publishing-and-support/) |

The project does not triage defects in third-party plugins. A maintainer may close such an issue with a pointer to the plugin's own route. An author who publishes a plugin publishes its support window and its contacts, as described in [Publishing and support](/pages/guide-publishing-and-support/) ([AUT-950] requires a release to be tested before it is handed out).

### Security reports

A vulnerability is never reported in a public issue or in discussions. The project's contact is in `SECURITY.md` at the repository root and in `ilium --version --verbose`. The project acknowledges a report within 3 working days, gives a first assessment within 10 working days, and informs the reporter at least every 14 days ([SEC-321]). The full process, the severity scale and the embargo are in [Security response](/pages/reference-security-response/). A bundle sent with a security report follows the same data-handling rules as above.

### What the project does not do

- It does not guarantee an answer or a fix date.
- It does not debug a third-party plugin's code.
- It does not accept a support request that is a vulnerability report posted publicly; a maintainer may remove the details and point to the security contact.
- It does not take receipt of bundles by any channel other than the ones above.
