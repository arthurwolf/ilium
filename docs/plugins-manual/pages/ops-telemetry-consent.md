---
layout: page
title: Telemetry consent for operators
permalink: /pages/ops-telemetry-consent/
---

## Telemetry consent for operators

This page tells an operator what telemetry consent means on a managed machine, how a consent is recorded and withdrawn, and what the host does and does not check. The user-facing promise is on [Plugin privacy](/pages/user-plugin-security-and-privacy/) ([SEC-260] and [SEC-266]). The host counters are on [Telemetry and plugin health](/pages/reference-host-telemetry/). The consent stores of the user are on [Grant review and consent](/pages/reference-grant-review-and-consent/).

Two different things carry the word telemetry. Keep them apart when you answer an auditor:

- **Host telemetry** is the counters, histograms and spans that the host keeps in the process and in the local log. It never leaves the machine ([HST-350]). It needs no consent.
- **Plugin telemetry** is data that a plugin itself sends to a network address, using its own `http` grant. Only this kind needs consent. The host cannot see its content, so the host controls it only through the grant and the consent record.

### You need

- The policy file of the site, if the machine is managed ([Enterprise deployment](/pages/guide-enterprise-deployment/)).
- Read access to the user's state folder, to check the consent record.
- The audit log ([Audit log](/pages/reference-audit-log/)) for the consent history.

### Requirements

- **[OPS-101]** No plugin MUST send telemetry, and the host MUST NOT record a telemetry consent, until the user sets the consent in the plugin's settings page. A new install MUST start in the state `not-asked`, which means no telemetry. Test: install a fixture plugin that declares telemetry, and assert that no network connection is opened and that its consent state is `not-asked`.
- **[OPS-102]** The telemetry consent of one plugin MUST have one of three states: `not-asked` (the default), `granted` and `declined`. A plugin that is removed and installed again MUST start in `not-asked`. (Proposed, owner to confirm: the three state names.) Test: grant, uninstall, install again, and read the state.
- **[OPS-103]** The consent MUST be stored in the consent store of the user, under the key `telemetry` of the plugin id, in the same file as the other consents ([grants and consents](/pages/reference-grant-review-and-consent/)). It MUST NOT be stored in the plugin's own data folder, so that a plugin cannot change it. (Proposed, owner to confirm: the key name `telemetry`.) Test: write a grant, read the file, and assert that the key exists in the store and not in the bucket.
- **[OPS-104]** Each change of the state MUST write one audit record of kind `consent-given` or `consent-withdrawn` ([SEC-222], [reference-audit-log](/pages/reference-audit-log/)). The record MUST name the plugin id, the new state, the actor and the time. It MUST NOT contain any telemetry content. Test: change the state three times and assert three records in the audit chain with the stated fields.
- **[OPS-105]** Withdrawing a consent MUST stop plugin telemetry at the next send after the change. The host MUST pass the new state to the plugin, and the plugin MUST NOT be able to read a stale value after the change. Data that the plugin already delivered before the withdrawal is outside the host's control and MUST be described as such in the plugin's own privacy text. Test: grant, send, withdraw, send again, and assert that the second send does not reach the fixture endpoint.
- **[OPS-106]** An administrator policy MAY restrict telemetry, for example by turning off all plugin telemetry on a managed machine. An administrator policy MUST NOT set a consent to `granted` and MUST NOT read the consent records of a user ([USR-344]). Test: set the policy to block telemetry, then grant a consent, and assert that the send still does not happen; then check that the policy file holds no consent value.
- **[OPS-107]** Accepting the install review of a plugin MUST NOT set the telemetry consent, even when the review lists network grants. Test: accept a review with an `http` grant to the telemetry host and assert the state stays `not-asked`.
- **[OPS-108]** The consent state MUST be shown on the plugin's settings page and in `ilium plugin grants <id>`, with the words `not asked`, `granted` or `declined`. Test: read both views in each of the three states.

### Worked example

A managed machine runs a plugin `example.stats` that sends anonymous usage counts. The administrator wants to know whether anything was sent.

<pre class="term"><span class="t-prompt">$</span> ilium plugin grants example.stats
http       <span class="t-dim">Connect to stats.example.org</span>   allowed
telemetry  <span class="t-dim">Plugin telemetry</span>               <span class="t-warn">not asked</span>
<span class="t-prompt">$</span> ilium plugin audit --plugin example.stats
2026-11-01 09:12:40 install
2026-11-01 09:14:02 grant    http stats.example.org
</pre>

The consent is `not asked`, so the plugin must not send. No `consent-given` record exists. The administrator has the proof without reading any telemetry content. The audit lines show the install and the grant; the audit command prints plugin ids and events only ([USR-344]).

The telemetry line of `ilium plugin grants` and the `--plugin` filter of `ilium plugin audit` are proposed, owner to confirm (see the decisions below). Until they exist, read the consent store file and filter the audit file by plugin id.

### Edge cases

- **The user has declined and the plugin keeps sending.** That is a plugin fault. Disable the plugin ([plugin disable](/pages/reference-plugin-cli/)) and report it through [Security response](/pages/reference-security-response/).
- **The policy blocks telemetry and the user has granted it.** The policy wins. The state stays `granted` in the user's store, and the host sends nothing (OPS-106).
- **The audit file is damaged.** The consent state is still read from the consent store. The damaged-record rule of [SEC-956](/pages/reference-audit-log/) applies to the history.

### Decisions for the owner to confirm

- Whether the three states of OPS-102 are shown to the user as `not asked`, `granted` and `declined`, or as on and off.
- Whether a policy key to block all plugin telemetry is added, and its name. Until then, SEC-457 (disable all third-party plugins) is the only control.
- Whether `ilium plugin audit --plugin <id>` is the command an operator uses to read the consent history, or whether a dedicated command is added.

### See also

- [Plugin privacy](/pages/user-plugin-security-and-privacy/) for the user statement.
- [Diagnostics and logs for operators](/pages/ops-diagnostics-and-logs/) for the logs, which are local and not consent-gated.
- [Crash isolation, health checks and the kill switch](/pages/ops-crash-isolation-health-and-kill-switch/).
