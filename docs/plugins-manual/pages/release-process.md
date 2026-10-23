---
layout: page
title: Release process
permalink: /pages/release-process/
---

## Release process

This page defines how a release of Ilium itself is made, supported and withdrawn. It is written for maintainers who cut releases and for operators who choose a release by its support horizon. It does not cover how a plugin is published; that is in [Publishing guide](/pages/guide-publishing/).

Requirement ids such as `PKG-953` are in the requirements pages listed in [Requirements index](/pages/reference-requirements/). Where this page states a value that no requirement fixes, it is marked "(proposed, owner to confirm)".

### Terms

- **Maintainer with release permission**: a person who holds the release permission on the Ilium release repository. Only this role pushes a release tag, creates a release branch, publishes a release, pauses it or withdraws it.
- **Release record**: the file kept for one release that holds the checklist results, the owner table review of DEV-958, the support dates and the evidence links of each step.
- **Minor release**: `vMAJOR.MINOR.0`. **Patch release**: `vMAJOR.MINOR.PATCH` with PATCH above 0. A patch release changes no WIT interface, limit, error code or manifest key (DEV-966).

### You will need

- The verification rules for the files a release ships: [Host release verification](/pages/reference-host-release-verification/).
- The channel, staged offer and pause rules: [Release rollout](/pages/reference-release-rollout/#host-release-channels-and-rollout).
- The support dates: [Toolchain support matrix](/pages/reference-toolchain-support-matrix/) and [ABI major versions](/pages/reference-abi-major-versions/).

## How a host release is cut

### Branch model

- Development happens on `master`. A minor release is cut from `master` at a commit whose checks are green. The maintainer creates a branch `release/vMAJOR.MINOR` from that commit (proposed, owner to confirm). The first release tag of the minor, `vMAJOR.MINOR.0`, is created on that branch.
- A patch release is prepared on the same `release/vMAJOR.MINOR` branch. A fix lands on `master` first and is then cherry-picked to every release branch whose minor is still inside its security-fix window (DEV-964, DEV-966). A fix that cannot be cherry-picked cleanly is written again on the branch and reviewed there.
- A release branch accepts only security fixes, data-corruption fixes, startup fixes, documentation corrections and fixes to the release files. It accepts no new feature, no interface change and no limit change.

### Tags

- A release tag has the form `vMAJOR.MINOR.PATCH`, for example `v0.1.0`. The installer resolves a channel to exactly one such tag (PKG-954). A tag that is pre-release only uses the suffix `-beta.N` or `-dev.N`, and its channel is `beta` or `dev` (PKG-953).
- Only a maintainer with release permission may push a tag. A tag is signed with the release key of SEC-426 and is never moved, deleted or re-pointed once pushed. A mistake is corrected by a new patch tag (and, if the old release is unsafe, by a pause or withdrawal below).

### The checklist

The maintainer works through this list in order and records the result and an evidence link for each item in the release record. A failed item blocks the release; the maintainer does not tag past it.

1. **Changelog.** Run `python3 tools/compile_changelog.py` to fold the `changelog.d/` fragments into `CHANGELOG.md`. Every user-visible change has a fragment. Evidence: the compiled diff.
2. **Owner table.** Every page of the owner table has a review record in the release record (DEV-958). A page without one blocks the release.
3. **Requirement-row tests.** Run the tests named in the Test column of every requirement row. A row marked `pending: test to be written` is listed in the release notes as untested, not as passing. Evidence: the test report.
4. **Fuzz and hostile-plugin suites.** Run the fuzz targets and the hostile-plugin suite of [Security testing](/pages/reference-security-testing/). Every fixed High or Critical vulnerability has a regression fixture whose name contains its advisory id; the checklist fails when one is missing (SEC-704). Evidence: the suite report and the advisory list.
5. **Screen-reader check.** A tester runs the consent dialog and one plugin settings screen with `[ui] screen_reader = true` on each shipped platform at 48 columns and passes every step (USR-951). Evidence: the reader and terminal emulator name per platform, which go into the release notes (USR-950).
6. **Third-party notices.** Check that `THIRD-PARTY-NOTICES` is regenerated from `Cargo.lock` and lists every shipped third-party crate, Wasmtime included (SEC-420). Evidence: the generated file hash.
7. **Interop matrix.** Run the matrix of host minor against SDK minor for every minor inside the window, at least the old plugin on the new host and the new SDK on the old host (ABI-064). Evidence: the matrix result.
8. **Manual snapshot.** Build the manual snapshot of this release. It describes exactly this release and is not edited afterwards (LIF-960). Evidence: the snapshot build log and its address.
9. **Archives, `SHA256SUMS` and attestations.** The release workflow builds the five archives, writes `SHA256SUMS` with one line per archive (SEC-428) and produces a build attestation for each archive. Evidence: the workflow run and `gh attestation verify` output for each archive.
10. **Release notes.** Write the notes. They name the channel and the starting rollout percentage, list every change to stored data (PKG-960), name the requirement rows and manual corrections the release changes, and name the screen reader per platform (USR-950).
11. **Support dates.** Enter the release date and the security-fix end date in the release record and the support matrix before the tag is created (DEV-964, DEV-994).

### What one release consists of

| File | Purpose |
|---|---|
| `ilium-linux-x86_64.tar.gz`, `ilium-linux-aarch64.tar.gz`, `ilium-windows-x86_64.zip`, `ilium-macos-aarch64.tar.gz`, `ilium-macos-x86_64.tar.gz` | The five release archives the installer expects (SEC-433) |
| `SHA256SUMS` | One SHA-256 line per archive (SEC-428) |
| Build attestations | One per archive, checked with `gh attestation verify` |
| `THIRD-PARTY-NOTICES` | Licence list of shipped crates (SEC-420) |
| Release notes | Channel, rollout, data changes, row and manual changes |
| Manual snapshot | The manual of this release (LIF-960) |

### Publishing and announcing

1. The maintainer pushes the signed tag. The release workflow builds the files above and attaches them to a release marked as a pre-release on the `beta` channel (PKG-955).
2. The maintainer runs the verification of [Host release verification](/pages/reference-host-release-verification/) against the published files, as a user would. A failure blocks the next step.
3. A release that is to become `stable` stays on `beta` for at least 7 days (PKG-955), then is promoted. It starts at a rollout percentage of at most 10 and is widened by hand with at least 24 hours between steps (PKG-956, PKG-957).
4. The announcement is the release notes page and the release entry. A major release is announced in the notes of a minor at least 6 months before it ships, with its full removal list (DEV-995).

### Withdrawing a release

- **Pause.** On a confirmed report of data corruption, a startup failure or a weakened security control, a maintainer with release permission sets the rollout percentage to 0 within 24 hours (PKG-958). The installer then refuses the tag before it downloads. The archives and `SHA256SUMS` are not deleted, replaced or re-signed (PKG-959), and installed hosts keep running.
- **Replace.** The fix ships as a new patch tag. The notes of the paused release record the pause with its date and a one-line reason (PKG-960).
- **Roll back.** An operator moves back by the steps of [Upgrading Ilium and rolling back the host](/pages/guide-operator-upgrades/).
- **Evidence kept.** The release record keeps the report, the pause time, the new tag and the notes link. Records are kept at least as long as the snapshot of the release (LIF-964).

## Release cadence, support window and long-term support

### Cadence

- A minor release is planned about every 3 months and a patch release as security or data-corruption fixes require, normally within the window of PKG-958 for a pause and within the project's advisory process for security fixes (proposed, owner to confirm).
- A planned date is not a promise. A release that fails the checklist waits; it is not tagged with a failing item.

### Long-term support

- One minor in each year is designated a long-term support minor in its release notes (proposed, owner to confirm). A long-term support minor receives security fixes and documentation corrections for 24 months after its release date instead of 12, and its snapshot stays online for at least 24 months after the next minor (LIF-964).
- Until the owner confirms the designation, no minor is called long-term support and the 12-month window applies to every minor.
- Plugin compatibility is unchanged by the designation: a host loads the current ABI minor and the previous one (DEV-964), so a long-term support host does not keep loading plugins built for an older ABI minor than that rule allows.

### One table for the three support rules

| Rule | Id | What it fixes | Value |
|---|---|---|---|
| Security-fix window | DEV-964, SEC-966 | Release date to security-fix end date of one minor | At least 12 months (proposed, owner to confirm) |
| Two-minor rule | SEC-323, SEC-966 | The latest minor and the one before it always receive security fixes | Applies even after the end date; the result is the longer of this and the window |
| Snapshot retention | LIF-964 | Manual snapshot of a minor stays online | At least 24 months after the next minor, and never while the minor is supported |
| Documentation corrections | LIF-973 | Which minors receive manual corrections | Stated on each release page; never ends before the retention of LIF-964 |

A minor is supported for the longer of the security window and the two-minor rule. The snapshot outlives both.

### End of life

When a minor passes its security-fix end date and is no longer one of the two latest:

- It receives no more fixes. `ilium version --json` reports `support: "end-of-life"` and the server writes one `warn` line `host.end-of-life` per start (DEV-965).
- Its row stays in the support matrix and installed plugins keep loading unchanged (DEV-962).
- Its snapshot stays online for the retention of LIF-964.
- `ilium plugin check --against` keeps accepting that version, so a plugin author can still check a plugin against it.

### Upgrading across several minors

- An operator who skips minors upgrades directly to the target release; no intermediate release is required unless the notes of a skipped minor say so (proposed, owner to confirm). The release notes of every minor list each change to stored data (PKG-960), and the operator reads all of them for the minors skipped.
- Data formats are read only when their format number is at most the host's own (LIF-304) and are migrated at first start of the new host. The operator keeps a copy of the previous binary and of the plugin state files first (LIF-950). If a skipped migration cannot be made, the host starts with the affected plugins disabled and logs `host.format-newer` or `migration.data-newer` (LIF-305, LIF-079).
- A plugin that became API-incompatible is kept installed in state `Incompatible` and is not loaded (USR-254). The operator installs a rebuilt version, or removes it; no data is deleted by the upgrade.
- Going back is a host rollback and is limited by the data rule of [Upgrading Ilium and rolling back the host](/pages/guide-operator-upgrades/#data-written-by-a-newer-host).
