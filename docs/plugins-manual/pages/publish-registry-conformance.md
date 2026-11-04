---
layout: page
title: Registry conformance
permalink: /pages/publish-registry-conformance/
---

## Registry conformance

The client works with any registry that follows [Registry reference](/pages/reference-registry/). A private registry or a mirror run by an organisation can pass its own tests and still break a rule that users depend on: a signature chain, the rollback check, or the countersignature rules. The conformance suite checks one registry URL against the rules, so that an operator can see which rule a registry breaks before its users do. The suite does not publish plugins and does not change the registry.

### You will need

- The base URL of the registry, for example `https://registry.example.org`, reachable over HTTPS.
- The root key id of that registry, the same value that a client pins with `ilium plugin registry add --root-key`.
- A test publisher account on the registry, only when the publish rules are to be checked. The suite then publishes one test package under the reserved test namespace of the registry, if the registry offers one. Without it, the suite reports the publish rules as `skip`.

### What the suite checks

The suite runs one check for each rule of the three sections below. Each check has the rule id of the registry reference, so a `fail` line names the rule that broke.

| Section of the registry reference | Rules checked |
|---|---|
| Metadata documents and their signatures | PKG-361 to PKG-368 |
| Publishing | PKG-392 to PKG-400, PKG-047, PKG-048, PKG-066, PKG-067 |
| Mirrors | PKG-408 to PKG-411, PKG-816 |

The suite reads `root.json`, `timestamp.json`, `snapshot.json`, `index.json` and the version entries of a sample of plugins, and checks each hash, length and version against `snapshot.json`. It checks the rollback rule by reading a stored copy of an older `timestamp.json` and asserting that the registry does not serve it again as current.

### Running the suite

The suite is run with `ilium plugin registry conformance <url>`. It prints one `result` line for each rule, with the rule id, the outcome and a short reason. The outcome is `pass`, `fail` or `skip`. A `skip` names the reason, such as a missing test account.

<pre class="term"><span class="t-prompt">$</span> ilium plugin registry conformance https://mirror.example.org
<span class="t-ok">result</span> PKG-361 pass
<span class="t-ok">result</span> PKG-366 pass
<span class="t-err">result</span> PKG-368 fail   length of plugins/notes.json differs
<span class="t-warn">result</span> PKG-392 skip   no test account given
</pre>

The exit status is `0` when every rule passes, `1` when any rule fails, and `2` when the suite cannot reach the registry.

<div class="note">
<p>A `skip` is not a pass. A registry that skips a rule has not shown that it follows the rule.</p>
</div>

### Claiming compatibility

A registry that passes the suite may say so on its page. The word to use is `Ilium-compatible`. The name describes the registry only. It does not make the registry an official registry, and a private or mirror registry still shows its own name and URL ([Governance](/pages/governance/)).

**[PKG-997]** (proposed, owner to confirm) The project MUST publish a conformance suite that runs against any registry URL. It MUST check every rule of the metadata, publish and mirror sections of [Registry reference](/pages/reference-registry/), and it MUST print one `result` line for each rule, with the rule id and the outcome `pass`, `fail` or `skip`.

**[PKG-998]** (proposed, owner to confirm) A registry that passes every rule of the suite MAY use the name `Ilium-compatible` on its page. A registry that fails any rule, or that has a `skip` line, MUST NOT use that name.

### Edge cases

- **The registry has no test namespace.** The publish rules are reported as `skip`. The registry is not `Ilium-compatible` until the publish rules are run.
- **The registry changes during a run.** The suite reads each document once and checks it against the `snapshot.json` it read first. A change during the run is reported as a `fail` on the next run, not as a pass.
- **The mirror is partial.** The suite checks the partial mirror against [PKG-409](/pages/reference-registry/#mirrors) and its own root, not against the root of the source registry.

### See also

- [Registry reference](/pages/reference-registry/): the rules that the suite checks.
- [Mirror runbook](/pages/publish-mirror-runbook/): how an operator keeps a mirror current.
- [Governance](/pages/governance/): the rule on the Ilium name and logo.
