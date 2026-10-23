---
layout: page
title: Reproducible builds
permalink: /pages/guide-reproducible-builds/
---

## Reproducible builds

This guide defines the command that checks a build for reproducibility, the inputs an author must pin and the failure code on a mismatch. The CI recipe that uses it is in [CI recipes](/pages/guide-ci-recipes/). This page describes target behaviour; it does not claim the behaviour exists today.

Requirement ids of this page are in the range `DEV-892` to `DEV-893`. The complete list is in [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/).

### Rules

- **[DEV-892]** `ilium plugin verify-reproducible <dir>` MUST build the project twice in clean, separate build directories and compare the resulting component and package byte for byte. It MUST exit 0 when they are identical and exit 1 with `build.not-reproducible` when they differ, printing the first differing artifact and the offset.

- **[DEV-893]** A build is reproducible only with pinned inputs: a lockfile (`Cargo.lock` or `bun.lock`), the toolchain versions recorded in `dist/build.json`, and no network access during the build. `verify-reproducible` MUST fail with `build.unpinned-input` before building when a lockfile is missing or the recorded toolchain differs from the installed one.

### Failure behaviour

Each rule names its failure code. A failure prints a `diagnostic` record in JSONL or one line on standard error in text, and the process exit code follows [Plugin command-line reference](/pages/reference-plugin-cli/#exit-codes).

### Release pipeline integrity

[SEC-727] makes release builds reproducible and [SEC-728] holds the signing key in hardware. Neither protects the path between a commit and the artefact that is signed. This section states the controls on that path for the host and for every first-party plugin. These controls are target behaviour of the release process, not of the `ilium` binary, and they add no new requirement id: each is a mechanical reading of [SEC-727], [SEC-728] and [SEC-173], and the release checklist MUST cite this section.

- **Signed tags from a protected branch.** A release tag MUST be a signed tag, and the signature MUST verify against the maintainer keys listed in the repository. A tag MUST be created only on a commit of the protected release branch. A commit reaches that branch only through a change reviewed and approved by a second maintainer who is not the author. Direct pushes and force pushes to the branch are refused by the server. A tag that fails any of these checks stops the release job before it builds; the job prints `release.tag-not-trusted` and exits 1.
- **Ephemeral, isolated runners.** Each release job MUST run on a fresh runner that is destroyed when the job ends, so no file, cache or credential survives to a later job. Release runners MUST NOT run pull-request builds, MUST NOT share a cache with them and MUST NOT be reachable from workflows triggered by forks.
- **Pinned third-party actions.** Every third-party action in a release workflow MUST be pinned by its full 40-character commit hash, with the tag name only in a comment. The generated CI file of [CI recipes](/pages/guide-ci-recipes/) pins a major version tag under DEV-765. A major tag can move, so that rule is not enough for the release workflow of the host itself: the release workflow MUST use hash pins, and a lint step MUST fail the job with `release.action-not-pinned` when it finds a `uses:` line without a hash.
- **Signing only after reproduction.** The signing step MUST NOT start until the reproducibility check of [SEC-727] has passed: the same tag was built on two separate machines and every artefact is byte-identical. If any artefact differs, the job exits 1 with `build.not-reproducible`, the first differing artefact and offset are printed as `ilium plugin verify-reproducible` prints them, and nothing is signed or published.
- **Listed secrets.** The release job MUST have exactly these secrets and no others: the request token for the hardware signing service of [SEC-728], and the publishing token that can upload to the release location. The signing key itself is never a CI secret. Neither token is available to pull-request jobs, and each is scoped to one repository and rotated after any maintainer change. The list is kept in the release checklist; adding a secret needs the same second-maintainer review as a workflow change.
- **Provenance records the build.** The SLSA provenance attestation of [SEC-727] MUST record the builder identity (the release workflow and runner image), the source repository and commit hash, the tag, the build parameters and the hashes of the pinned actions. The attestation is published beside the artefact and the user checks it as the chain on [Host release verification](/pages/reference-host-release-verification/) describes, together with the `SHA256SUMS` file of [SEC-428]. A mismatch there is a failed check and ends with exit 1 and no installed files, as [SEC-437] requires.

#### Failure behaviour

| Failure | Code | Result |
| --- | --- | --- |
| Tag unsigned, signed by an unknown key or not on the protected branch | `release.tag-not-trusted` | Job stops before building; nothing is published. |
| Action without a commit hash pin | `release.action-not-pinned` | Job stops before building. |
| Two-machine builds differ | `build.not-reproducible` | Nothing is signed or published. |

A failed release leaves no partial release visible to users. The maintainers fix the cause and cut a new tag; a published tag is never moved.

## See also

- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/)

API checked against `ilium:plugin@0.1.0`.
