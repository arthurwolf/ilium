---
layout: page
title: Registry shutdown, operator handover and URL migration
permalink: /pages/publish-registry-shutdown-and-migration/
---

## Registry shutdown, operator handover and URL migration

This page specifies what happens to clients and mirrors when a whole plugin registry ends service, changes operator or moves to a new URL. It covers the signed sunset notice, the read-only freeze, root key continuity on handover, the warnings a client shows, the behaviour after the last metadata expires, and the behaviour of mirrors. Registry operators, mirror operators and anyone who maintains a client build need this page; moving a single namespace is covered by [PKG-999] instead.

### You will need

- The signing keys of the current root role, at the threshold that the root document requires. Handover and URL moves are signed with these keys, so they must be available before the change starts.
- Write access to `snapshot.json` and `root.json` on the registry host, and the timestamp signing key for the final timestamp.
- The current pinned root version of each client, which `ilium plugin registry doctor` reports.
- Read access to the registry for mirror operators, who need the upstream URL and the root key id that the mirror pins.

For the day-to-day duties of an operator, read the [Registry operator guide](/pages/publish-registry-operator-guide/). For the file layout and the roles, read the [Registry reference](/pages/reference-registry/).

## Announcing an end of service in root.json and a signed notice document

An operator that ends service publishes a signed `sunset` notice in `snapshot.json`. The notice carries the date on which the registry ends, in the field `sunset.ends_at`. The notice must be published at least 180 days before the last signed `timestamp.json` expires, so that every client has time to see it before the metadata stops verifying. The operator checks this with `ilium plugin registry doctor`. A snapshot whose `sunset.ends_at` falls less than 180 days before the final timestamp expiry fails the check and the doctor reports `registry.sunset-too-short`.

The notice is a signed document, so a client accepts it only after it verifies the snapshot signature. A notice that is not signed by the current keys has no effect. The operator publishes the notice as early as the plan allows, and never later than the 180-day limit.

## Read-only freeze period

During the freeze the registry keeps serving its existing metadata and its packages, and it serves them read-only. It refuses every write: a publish, a yank and an owner change each fail with the code `registry.read-only`. Existing packages stay installable, so users who depend on a plugin can still install it while the freeze lasts.

The operator enters the freeze when the registry stops accepting changes. An operator that tests the freeze runs `ilium plugin publish` against the frozen registry. The command exits with a non-zero code and its output contains `registry.read-only`. The same registry still serves `ilium plugin install` for a package that already exists.

## Handover to a new operator and root key continuity

A handover to a new operator is a new version of `root.json`. The new version must be signed by the threshold of the previous root keys. The keys of the new operator are only trusted after the old keys have endorsed them in that signed version, so the chain of trust from the pinned root stays unbroken.

A client that receives a new root without that signature refuses it. The doctor reports `registry.root-invalid`, and the client keeps the root version it had pinned. For example, if a registry serves `root.json` version 3 signed only by the new keys, a client that pinned version 2 stays on version 2, and `ilium plugin registry doctor` reports `registry.root-invalid`.

## Client behaviour and warnings

When a client has verified a sunset notice, it shows the warning `registry ending on <date>` in two places: the output of `ilium plugin list` and the output of `ilium plugin registry doctor`. The date is the value of `sunset.ends_at`. The warning is informational. It never blocks an install, an update or a removal, so a user can keep working with the registry until it ends.

Once the last signed `timestamp.json` expires, the client reports `registry.metadata-expired`. From that point the client refuses new installs and updates from that registry. Installed plugins keep running, and the client does not delete them. For example, after the clock passes the final timestamp expiry, `ilium plugin update` exits non-zero with `registry.metadata-expired`, while `ilium plugin list` still shows the installed plugin as `enabled`.

### URL migration

A registry that moves to a new URL publishes the change as a `url` field in a new version of `root.json`. The new version is signed by the current root. A client never takes a new URL from an HTTP redirect. When a client receives a 301 response from the old URL, it keeps the old pinned URL and the doctor reports `registry.redirect-ignored`.

A signed `url` change is a different case. The client shows a prompt, and it re-pins the new URL only after the user answers `y`. Any other answer keeps the old pinned URL.

## Mirrors during and after shutdown

A mirror keeps serving its last verified snapshot after the upstream registry shuts down. It does not stop serving, and it does not delete the packages in its last snapshot. The mirror marks its state as `frozen` in its sync report, so that operators and clients can see that no new metadata will arrive.

The operator checks this with `ilium plugin mirror sync --json` against the shut-down upstream. The output line carries `status: "frozen"`, and the mirror still serves the packages from its last snapshot. The sync procedure itself is in the [Mirror runbook](/pages/publish-mirror-runbook/).

## Worked example

A registry ends service on 2027-06-30, and its final timestamp expires at 12:00:00 on that day. The operator sets `sunset.ends_at` to 2027-01-01, which is exactly 180 days before the final expiry, so the check passes. A user who lists plugins then sees the warning:

<pre class="term"><span class="t-prompt">$</span> ilium plugin list
<span class="t-warn">warn</span> registry ending on 2027-01-01  registry.example.org
<span class="t-ok">ok</span>   demo-plugin 0.1.0  enabled
</pre>

The warning does not stop the user. The install below still succeeds:

<pre class="term"><span class="t-prompt">$</span> ilium plugin install demo-plugin
<span class="t-ok">ok</span>   demo-plugin 0.1.0 installed
</pre>

## Edge cases

A client whose clock is wrong can report `registry.metadata-expired` too early or too late. The client decides expiry from the clock on its own machine, so a user with a clock far from the real time should fix the clock before drawing conclusions from the expiry error.

A root version that fails the signature check is never accepted, even when the registry serves it on the same URL as the valid version.

A mirror that is still syncing when the upstream shuts down keeps its last verified snapshot. It never replaces that snapshot with unverified data.

A user who sees `registry.redirect-ignored` after a host change should check the signed `root.json` for a `url` field. A redirect without that field is never a reason to move a pinned registry.

## Requirements

- **[PDW-001]** An operator ending service MUST publish a signed `sunset` notice in `snapshot.json` at least 180 days before the last signed `timestamp.json` expires.
- **[PDW-002]** During the freeze the registry MUST serve existing metadata and packages read-only and MUST refuse publish, yank and owner changes with `registry.read-only`.
- **[PDW-003]** Once a sunset notice is verified, a client MUST show `registry ending on <date>` in `ilium plugin list` and `ilium plugin registry doctor`, and MUST NOT block install, update or removal.
- **[PDW-004]** A handover to a new operator MUST be published as a `root.json` version signed by the threshold of the previous root keys, and a client MUST refuse a new root not so signed with `registry.root-invalid`.
- **[PDW-005]** After the last timestamp expires, a client MUST report `registry.metadata-expired`, refuse new installs and updates from that registry, and keep installed plugins running without deleting them.
- **[PDW-006]** A URL move MUST be a `url` field in a `root.json` version signed by the current root, and the client MUST re-pin the new URL only after the user confirms in a prompt, never from an HTTP redirect.
- **[PDW-007]** A mirror MUST keep serving the last verified snapshot after the upstream shutdown and MUST label it `frozen` in its sync report.
