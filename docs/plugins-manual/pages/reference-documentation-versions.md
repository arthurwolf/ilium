---
layout: page
title: Reference, documentation versions
permalink: /pages/reference-documentation-versions/
---

## Documentation versions

This page says which version of this manual describes which Ilium host release, how long an older manual stays online, how a page is corrected after a release, and what support a release promises for its documentation. A plugin author or an operator on an older host must be able to find the manual that matches the host they run. The contract version `ilium:plugin@0.1.0` identifies the plugin interface. It does not by itself identify a manual, so this page ties the two together.

## You will need

- The host version, from `ilium version --json` or `ilium --version --verbose`.
- The contract line at the top of a page, `Contract: ilium:plugin@0.1.0`, which names the contract the page describes.

## Which manual matches which host

- [LIF-960] Each host release MUST be published with one manual snapshot of its own. The snapshot MUST describe exactly the behaviour of that host release, and it MUST NOT be edited to describe a later release.
- [LIF-961] The snapshot of host release `X.Y.Z` MUST be served under the path `/docs/X.Y.Z/`, and the current manual MUST be served under `/docs/latest/`. Every page of a snapshot keeps its `permalink` relative to that path (proposed, owner to confirm).
- [LIF-962] Every page of a snapshot MUST show the host release it describes and the contract line it was written for, so that a reader who arrives by a search engine link can see the version without the site menu.
- [LIF-963] A page that describes a feature added in a later minor release than the snapshot it is in MUST say so with the minor version of that feature, so that a reader on an older host knows the feature is absent on their host. The page MUST NOT present such a feature as available in every version.

## Retention of older manuals

- [LIF-964] The snapshot of every released minor version MUST stay online for at least 24 months after the release of the next minor version (proposed, owner to confirm). A snapshot is never removed while its minor version still receives security fixes under [SEC-323].
- [LIF-965] A snapshot MUST NOT be replaced by the current manual, even when the current manual has a correction that the snapshot lacks. The correction goes to the snapshot as described under "Corrections after a release".
- [LIF-966] A snapshot that is removed MUST be replaced by a page that names the removed minor version, the host release that replaces it and the date of removal, so that a link from an older advisory or a forum post still lands on a clear answer.

## Corrections after a release

- [LIF-967] A correction to a page after a release MUST be made in the current manual. It MUST also be made in every snapshot that is still online and that makes the same wrong statement, when the wrong statement changes what a plugin or an operator does.
- [LIF-968] A correction that only fixes wording, spelling or a link MAY be made in the current manual alone. The release notes of the next patch release MUST NOT list such a correction.
- [LIF-969] Every correction that changes a rule, a number, a code name or a procedure MUST be listed in the release notes of the next patch release. The entry MUST name the page, the old statement and the new statement, and it MUST say which snapshots received the correction.
- [LIF-970] A correction to a documented number or rule MUST NOT change the contract version, the `api` string or the `ilium:api-version` section. Only the rules in [Reference, ABI evolution](/pages/reference-abi-evolution/) decide whether the contract changes. If a correction reveals a real change of behaviour, the host release that contains the change is the one whose snapshot is corrected.

## Fixed-in versions and advisories

- [LIF-971] The "fixed in" version of a security advisory MUST name a host release. The advisory MUST link to the snapshot of that release, and that snapshot MUST describe the fixed behaviour. A snapshot that does not describe the fixed behaviour is a documentation error under [LIF-967].
- [LIF-972] Where the documented behaviour of an older snapshot differs from the behaviour of a fixed release, the advisory MUST say which behaviour the affected snapshot describes, so a reader can tell whether their host is affected.

## Support statement

- [LIF-973] Each release page MUST state two things: the minor versions that receive documentation corrections, and the minor versions that receive security fixes under [SEC-323]. The documentation window MAY end before the security window, but it MUST NOT end before the minimum retention of [LIF-964].
- [LIF-974] A support answer about a plugin or a host MUST cite the snapshot of the host version that the user runs, not the current manual. A support answer that cites the current manual for an older host MUST say that the behaviour may differ.
- [LIF-975] A plugin author who publishes documentation for a plugin MUST state the minimum host minor version the plugin needs and the `api` string it was built for. The statement MUST be kept in the same place as the release notes that name the plugin version (see [Publishing and support](/pages/guide-publishing-and-support/)).

## See also

- [Versioning](/pages/versioning/)
- [ABI evolution](/pages/reference-abi-evolution/)
- [Publishing and support](/pages/guide-publishing-and-support/)
- [Security response](/pages/reference-security-response/)
- [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/)
