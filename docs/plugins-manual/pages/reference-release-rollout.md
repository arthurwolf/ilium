---
layout: page
title: Release rollout
permalink: /pages/reference-release-rollout/
---

## Release rollout

A **rollout** offers a new version of a plugin to a share of clients first, and widens the offer only when the publisher decides to. The share is a **percentage** from 0 to 100. A client inside the share is in the **cohort** for that version. A **halt** sets the percentage to 0, so the version is offered to no client that has not yet taken it.

A rollout needs a registry. A file install of a package is not limited by a rollout, because the user chooses that file directly (see [Registry reference](/pages/reference-registry/)).

A rollout is not a yank. A yank withdraws a version from every new install and leaves installed copies working (see the yank rule in the registry reference). A rollout limits the offer of a version to its cohort. Halting a rollout is the first response to a bad release, and yanking the version is the second.

### Terms

- **Rollout percentage**: an integer from 0 to 100 on one version entry. It is set by the publisher and carried in the signed metadata.
- **Staged version**: a version whose rollout percentage is below 100.
- **Cohort value**: the random value a client generates and keeps on its own machine. It decides whether the client is inside the cohort.
- **Offer**: the act of a client choosing a version for a new install or an update. A version that is listed in `index.json` but not chosen is not offered to that client.
- **Halt**: a rollout percentage of 0.
- **Widen**: raising the percentage of a staged version, up to 100.
- **Activate**: the step that makes an installed version the running one (see [Install, update, uninstall](/pages/reference-install-update-uninstall/)).

### You will need

- The publish, yank and deprecate rules: [Registry reference](/pages/reference-registry/).
- The request privacy rule that bounds what a client may send, in the same page.
- The publisher commands: [Plugin distribution commands](/pages/reference-plugin-distribution-cli/).

## What a client does with a rollout

The registry lists a staged version in `index.json` for every client, with its rollout percentage. The registry cannot tell two clients apart when it serves a static file tree ((see the static file rule in the registry reference)), so the choice of cohort is made by the client, on the machine, using the rules below.

- [PKG-686] A version with a rollout percentage below 100 MUST be offered only to clients whose cohort is inside that percentage. A client outside the cohort MUST NOT choose that version for a new install or an update. It MUST choose the version it would have chosen if the staged version did not exist. The registry MUST keep the staged version in `index.json` with its percentage. A version at 100 is offered to every client, the same as a version with no rollout.
- [PKG-687] A client MUST decide its cohort on the machine. It MUST generate one random value on first use, store that value locally, and compare it with the rollout percentage. The client MUST NOT send the value, the cohort or any value derived from them in any request, under the request privacy rule. The client MUST keep the same value across metadata refreshes, so that it does not move in and out of a rollout between checks.

A client that is outside the cohort sees no error. It keeps working with the version it has, and the staged version is simply not chosen for it.

Decision needed: the granularity of the cohort. The default is whole percentages, so the client compares its value with a whole number from 0 to 99 and the cohort is inside the percentage when the value is lower. Choose a finer granularity only if a publisher needs steps smaller than one per cent.

## Halting a rollout

A halt is the fastest publisher action on a bad release. It changes only the signed metadata, so it works without any change in the package. It does not delete the package file.

- [PKG-688] A rollout percentage of 0 MUST stop new offers of that version to every client at its next metadata refresh, including clients that are inside the cohort. A client that has started to install the version and has not yet activated it MUST NOT activate it once it has seen the halt. A client that has already activated the version keeps it running, in the same way as an installed copy after a yank. A version that must be removed from installed copies is handled by a takedown in the registry reference, not by a halt.

Decision needed: whether a client that has started an install checks for a halt again just before it activates the version. The default is yes, when the registry can be reached. When the registry cannot be reached, the client uses its cache as the offline rule of the registry reference allows, so a halt can reach an offline client only at its next refresh. Choose a stricter rule only if the publisher cannot accept that delay.

Decision needed: whether a client that already activated a halted version should be moved back to the previous version. The default is no, because a rollback changes an installed plugin without the user asking for it, and the same default applies to a yank. A takedown in the registry reference is the only route that removes a version from installed copies.

## Who changes a rollout

- [PKG-689] A change to a rollout percentage, including a halt, MUST be made by the publisher of the plugin with the registry token of the publish command, and MUST be published as a new signed version of `plugins/<id>.json` with a higher version number, so that rollback protection stops an older percentage from being served again. A change by a publisher who does not own the namespace MUST be refused with `registry.namespace-unowned`. A missing or refused token MUST be refused with `registry.auth`.

Decision needed: the command and the record names for setting a rollout. The repository does not define them yet. The default is a `rollout` option of the publisher command, in the form `ilium plugin rollout <id>@<version> --percent <n>`, because `yank` and `deprecate` already use the form `ilium plugin <verb>`. A value outside 0 to 100 is refused with `usage.invalid-value`. The default record field in `index.json` is `rollout_percent` on each version entry, next to `review_level`.

Decision needed: the signal the publisher uses to widen a rollout or to halt it. The client sends no data about its activations, so the registry cannot count successful activations. The default is that the publisher widens a rollout by hand, after checking the download counts in the registry operator's own server logs and the reports that users send through the contact on the plugin page. The registry does not halt a rollout automatically. An automatic halt would need activation data that clients never send.

Decision needed: whether a client shows a notice when a version is withheld from it because of its cohort. The default is no notice, the same as the silent skip of yanked versions.

## Publisher rollouts and operator stages: one cohort rule and settled record names

Two mechanisms limit who receives a new version. The **publisher rollout** of this page is a percentage in the signed registry metadata and is set by the publisher (PKG-686 to PKG-689). The **operator stage** of [Upgrading Ilium and rolling back the host](/pages/guide-operator-upgrades/) is a policy-driven staged rollout set by the person who runs many machines (LIF-306 to LIF-308). They answer different questions and they compose. This section fixes how, and settles the open decisions above into rules.

### One cohort function

Both mechanisms place a host with one function. The separate random cohort value of PKG-687 is dropped.

1. The host reads its installation key (kept beside `installed.json`, created once at the first start of the server).
2. For a publisher rollout the host hashes the text `<plugin id>@<version>:<installation key>` with SHA-256. For an operator stage it hashes `<rollout id>:<installation key>`, as in the staged update rollout section of the operator guide.
3. The host reads the first four bytes of the digest as a big-endian integer and takes it modulo 10,000. The result is the bucket, 0 to 9,999, in units of 0.01 %.
4. The host is inside a share when its bucket is lower than the share in units of 0.01 %. A publisher percentage `n` is the share `n * 100`.

The input text differs, so the two placements are independent: a host in the first operator stage is not therefore in the first publisher cohort. The bucket is stable across restarts and metadata refreshes, and widening only raises the cutoff, so a host never leaves a cohort that it has entered. The key and the bucket are never sent in any request, under the request privacy rule of the registry reference. Deleting the state directory creates a new key and can move the host.

### Order of the two gates

A host MUST pass both gates before it applies a version, and the gates are evaluated in this order:

1. **Publisher gate.** The host takes the candidate versions from the verified `index.json` and removes every staged version whose `rollout_percent` does not cover its bucket (PKG-686). The result is the version the host would choose without the staged version.
2. **Operator gate.** If a policy assigns the chosen update to an operator stage, the host applies it only when its operator bucket is below the current stage share (LIF-306). Otherwise it keeps the installed version and reports `held` in `ilium plugin list --json`.

A version withheld by the publisher gate is never shown to the operator gate, so an operator cannot widen a version beyond what the publisher offers. A host that passes the publisher gate and is held by the operator gate reports `held`. A host outside the publisher cohort reports nothing, because the version is not offered to it. An operator stage cannot raise a publisher percentage, and a publisher percentage cannot release a host from an operator hold.

The automatic halt differs by gate. The operator stage halts on its own when the plugin fails inside the failure window on a host of the current stage (LIF-307). A publisher rollout never halts on its own, because clients send no activation data (see the signal decision above).

### Settled rules

These rules replace the six "Decision needed" paragraphs above. The earlier wording stays as the record of the options.

- **Record name.** Each version entry of `plugins/<id>.json` and `index.json` MAY carry `rollout_percent`, an integer from 0 to 100, next to `review_level`. An entry without the field has no rollout and counts as 100. A client MUST treat a value that is not an integer from 0 to 100 as invalid metadata and refuse it with `registry.signature-invalid` behaviour for that file, using no percentage from it.
- **Command.** The publisher sets the percentage with `ilium plugin rollout <id>@<version> --percent <n>`. A value outside 0 to 100, or not a whole number, is refused with `usage.invalid-value`. The command acts on the registry only; the operator forms `ilium plugin rollout resume <id>` and `ilium plugin rollout cancel <id>` (LIF-308) take an id without `@<version>` and no `--percent`, and a mixed form is refused with `usage.invalid-value`. Errors `registry.namespace-unowned` and `registry.auth` follow PKG-689.
- **Granularity.** The publisher percentage is a whole percentage. The operator stage keeps steps of 0.01 %. Both use the same 10,000-bucket function, so no second granularity exists in the client.
- **Re-check before activation.** Immediately before it activates a version that has a `rollout_percent` below 100, a client MUST refresh `timestamp.json` and re-evaluate the publisher gate when the registry can be reached. If the version is now halted or no longer covers the bucket, the client MUST NOT activate it (PKG-688) and keeps the downloaded package for a later offer. When the registry cannot be reached, the client uses its cache under the offline rule of the registry reference and activates.
- **No notice.** A client MUST NOT show a notice, warning or log entry above debug level for a version withheld by its cohort. Only the operator hold is visible, as `held`.
- **No automatic move-back.** After a halt, a host that already activated the version keeps it. No client moves a host to the previous version because of a halt. Only a takedown under the registry reference removes a version from installed copies, and an operator who wants the previous version follows the rollback steps of the operator guide.

### Failure behaviour of the combination

| Situation | Result |
|---|---|
| Host outside the publisher cohort | No error, no notice. The host chooses the version it would have chosen without the staged version. |
| Host inside the publisher cohort, outside the current operator stage | The update is not applied. `ilium plugin list --json` shows `held`. |
| Publisher halts while an operator stage is open | The publisher gate stops the offer at the next refresh and at the pre-activation re-check. The operator stage has nothing to apply; its share is unchanged. |
| Operator halts (`rollout.halt`) while the publisher percentage is below 100 | Both stop widening independently. Resuming the operator stage does not change the publisher percentage. |
| Registry unreachable at the pre-activation re-check | The cached metadata is used under the offline rule. A halt reaches the host at its next successful refresh. |
| Installation key missing or unreadable | The host creates a new key, which can move its bucket. It does not fail an install. |

## Inputs and outputs

| Item | Value | Where it lives |
|---|---|---|
| Rollout percentage | Integer 0 to 100 | The version entry in the signed metadata (field name: see the decision on record names) |
| Cohort value | One random value per client, never sent | Local storage of the client |
| Staged version in `index.json` | Listed for every client with its percentage | Registry files |
| Offer decision | The version the client chooses for install or update | Computed on the client at each refresh |

The registry serves the same files to every client. All per-client behaviour is computed on the client, so a mirror or a bundle that copies the files also carries the percentages ([Registry reference](/pages/reference-registry/)).

## Failure behaviour

| Situation | Result |
|---|---|
| A client is outside the cohort of a staged version | No error. The client chooses the version it would have chosen without the staged version. |
| The rollout is halted | No error. New offers stop at the next metadata refresh. See PKG-688. |
| A publisher who does not own the namespace changes a rollout | Refused with `registry.namespace-unowned`. |
| The token is missing or refused | Refused with `registry.auth`. |
| A change is published with a version number that is not higher | Refused by clients with `registry.rollback` ([Registry reference](/pages/reference-registry/), PKG-366). |
| The signed metadata fails verification or is past its `expires` time | `registry.signature-invalid` or `registry.metadata-expired`. The client uses no percentage from that metadata. |
| A percentage outside 0 to 100 is given to the publisher command | `usage.invalid-value` (default of the decision on the command). |

## Limits

| Limit | Value | Source |
|---|---|---|
| Percentage range | 0 to 100, whole numbers by default | This page, decision on granularity |
| `plugins/<id>.json` size | 1 MiB | PKG-369 in the [Registry reference](/pages/reference-registry/) |
| `index.json` size | 32 MiB | PKG-369 |
| `timestamp.json` lifetime | Within 1 day of issue | PKG-367 |
| `snapshot.json` lifetime | Within 7 days of issue (SHOULD) | PKG-367 |

The speed at which a halt reaches clients is bounded by the metadata refresh. Because `timestamp.json` expires within 1 day, an online client that keeps refreshing sees a halt within the lifetime of its `timestamp.json`. How often a client refreshes is not set by this page; the registry reference governs it.

- [PKG-694] A halt and every other rollout change MUST change only the signed metadata. The package blob of the version MUST stay in place, and a halt MUST NOT delete it.
- [PKG-695] A rollout MUST NOT limit a file install, because the user chooses the file directly. A client outside the cohort MUST NOT show an error for the staged version.

## Examples

A publisher stages version `1.3.0` of `example.notes` at 10 per cent:

```
ilium plugin rollout example.notes@1.3.0 --percent 10
```

The command above is the default of a decision still open, not a settled command. After it, `plugins/example.notes.json` is re-signed with a higher `version`, and the entry for `1.3.0` carries `"rollout_percent": 10` (default record name, also open). A client whose cohort value is below 10 per cent chooses `1.3.0`. Every other client chooses `1.2.0`, the version it would have chosen without `1.3.0`, and sees no message.

The publisher finds a defect and halts the rollout with `--percent 0`. At each client's next refresh the offer of `1.3.0` stops. A client that downloaded `1.3.0` but did not activate it does not activate it. A client that already runs `1.3.0` keeps running it. If `1.3.0` must also be withdrawn from new exact-version installs, the publisher yanks it ([Registry reference](/pages/reference-registry/), PKG-396). If it must leave installed copies, the operator uses a takedown (PKG-403).

## Host release channels and rollout

This section covers a new Ilium host release: the archive and `SHA256SUMS` of one release tag. The plugin rules above apply to a host release only where this section names them.

### Channels

- [PKG-953] A host release MUST be published on exactly one channel, `stable`, `beta` or `dev`, with the same names as the plugin channels of [Install and update](/pages/reference-install-and-update/) (PKG-265). A host release on the `beta` or `dev` channel MUST NOT be offered to a user who did not select that channel.
- [PKG-954] The installer of [SEC-434] MUST resolve the selected channel to one release tag before it runs the checks of [SEC-430] to [SEC-438]. It MUST NOT fall back to another channel or another tag when that tag is missing or paused under [PKG-958].
- [PKG-955] A release that is to become `stable` MUST first be published on `beta` and MUST stay on `beta` for at least 7 days (proposed, owner to confirm) before it is promoted.

### Staged offer

- [PKG-956] A new `stable` host release MUST start with a rollout percentage of at most 10 (proposed, owner to confirm). The percentage has the meaning of [PKG-686] and the cohort rule of [PKG-687] applies. A client outside the cohort MUST be offered the release it would have been offered without the new one, and MUST NOT see an error.
- [PKG-957] A publisher MUST wait at least 24 hours (proposed, owner to confirm) between two widening steps of a staged `stable` host release. The publisher widens by hand and does not widen a release that a halt under [PKG-958] has paused.

### Pausing a host release

- [PKG-958] A host release MUST be paused by setting its rollout percentage to 0. A pause MUST be made by a maintainer with release permission on the Ilium release repository, and MUST start within 24 hours (proposed, owner to confirm) of a confirmed report that the release corrupts stored data, prevents startup or weakens a security control.
- [PKG-959] A pause MUST NOT delete, replace or re-sign the archives or `SHA256SUMS` of its tag, and MUST NOT change any installed host or installed plugin. A pause MUST NOT be applied to a mirror by editing its files, which remains under [SEC-961]. Installed hosts keep running; the operator moves back to an earlier release only by the steps of [Upgrading Ilium and rolling back the host](/pages/guide-operator-upgrades/).

Decision needed: where the channel and rollout percentage of a host release are stored, and how the installer reads them before it downloads an archive. The release pages do not define a file for this. Default: a `channels.json` asset of the release, signed with the release key of [SEC-426], read by the installer before it fetches the archive. Consequence: an installer that cannot read the file must stop with a fixed message and must not use a cached value.

### Release notes

- [PKG-960] Each `stable` host release MUST publish release notes before it is offered. The notes MUST name its channel and its starting rollout percentage, MUST list every change to stored data, and MUST record a pause under [PKG-958] with its date and a one-line reason.

## What this page does not cover

- Signing, expiry, rollback protection and the static file rule: [Registry reference](/pages/reference-registry/).
- The publisher commands and exit codes: [Plugin distribution commands](/pages/reference-plugin-distribution-cli/).
- Installed states, activation and rollback of an installed plugin: [Install, update, uninstall](/pages/reference-install-update-uninstall/).
- Version choice, channels and pins: [Versions and dependencies](/pages/reference-dependency-resolution/).
- How an author prepares a release: [Publishing guide](/pages/guide-publishing/).

## See also

- [Registry reference](/pages/reference-registry/)
- [Distribution commands](/pages/reference-plugin-distribution-cli/)
- [Publishing guide](/pages/guide-publishing/)
- [Versions and dependencies](/pages/reference-dependency-resolution/)
