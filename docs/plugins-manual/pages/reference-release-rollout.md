---
layout: page
title: Release rollout
permalink: /pages/reference-release-rollout/
---

## Release rollout

A **rollout** offers a new version of a plugin to a share of clients first, and widens the offer only when the publisher decides to. The share is a **percentage** from 0 to 100. A client inside the share is in the **cohort** for that version. A **halt** sets the percentage to 0, so the version is offered to no client that has not yet taken it.

A rollout needs a registry. A file install of a package is not limited by a rollout, because the user chooses that file directly (see [Registry reference](/pages/reference-registry/)).

A rollout is not a yank. A yank withdraws a version from every new install and leaves installed copies working (see the yank rule in the registry reference). A rollout limits the offer of a version to its cohort. Halting a rollout is the first response to a bad release, and yanking the version is the second.

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

## See also

- [Registry reference](/pages/reference-registry/)
- [Distribution commands](/pages/reference-plugin-distribution-cli/)
- [Publishing guide](/pages/guide-publishing/)
- [Versions and dependencies](/pages/reference-dependency-resolution/)
