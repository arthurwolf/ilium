---
layout: page
title: Host release verification
permalink: /pages/reference-host-release-verification/
---

## Host release verification

This page explains how to confirm that the Ilium host you downloaded is the release that was published. It covers the `SHA256SUMS` file that every release ships, the checks the installer makes, and the build attestations that tie an archive to the release build. It does not cover plugin packages. Those are checked with the package checksum and signature described in [Signing and trust](/pages/reference-signing-and-trust/).

A **release** is one tagged version of the host, for example `v0.1.0`. A **release archive** is the compressed file for one platform. **`SHA256SUMS`** is the text file published with the release. It holds one line per archive: a 64-character lowercase SHA-256 hash, two spaces, and the archive file name. An **attestation** is a signed record from the build system that names the files it built and the workflow that built them.

### You will need

- The download page for the release, which links `SHA256SUMS` and the build attestations.
- A SHA-256 tool on your system: `sha256sum`, `shasum -a 256` or `openssl dgst -sha256`.
- For attestation checks, the GitHub CLI (`gh`).
- The package checks for plugins: [Signing and trust](/pages/reference-signing-and-trust/).

## What a release ships

Every release publishes one `SHA256SUMS` file beside the archives. The file lists every archive of that release, one line each. The installer script reads the same file.

- [SEC-428] A release MUST publish a `SHA256SUMS` file that lists every release archive with its SHA-256 hash, one line per archive.
- [SEC-429] A user who checks a download by hand MUST compare the archive's SHA-256 hash with the line in `SHA256SUMS` that carries the same archive name, and MUST run the attestation check before running the archive.

## Terms and formats

- **Archive inventory**: the five archives the installer expects in `SHA256SUMS`: `ilium-linux-x86_64.tar.gz`, `ilium-linux-aarch64.tar.gz`, `ilium-windows-x86_64.zip`, `ilium-macos-aarch64.tar.gz` and `ilium-macos-x86_64.tar.gz` (`release/install.sh`, `checksum_archives`).
- **Record**: one line of `SHA256SUMS`. A valid record is exactly 66 characters plus the length of the file name: 64 lowercase hexadecimal characters, two spaces, then the name. The name starts with a letter or digit and holds only letters, digits, `.`, `_` and `-`.
- **Hash tool**: `sha256sum`, `shasum -a 256` or `openssl dgst -sha256`. The installer uses the first one it finds and stops with `A SHA-256 tool is required: sha256sum, shasum, or openssl` when none exists.

- [SEC-433] The installer MUST accept `SHA256SUMS` only when it holds exactly one valid record for each of the five archives of the archive inventory, with no other record, no duplicate name and no blank or extra line. Any other file MUST stop the install with `Malformed, duplicate or missing checksum record`.

## Inputs, outputs and limits

| Item | Value | Source |
|---|---|---|
| Inputs | The archive and `SHA256SUMS` of one release tag, fetched over HTTPS only | `release/install.sh` |
| Download limits | 15 seconds to connect, 300 seconds per download | `release/install.sh` |
| Latest fallback | None. A missing asset ends the install with `HTTPS download failed (network, rate limit, or missing release asset); no latest fallback` | `release/install.sh` |
| Free staging space | At least 64 MiB, or `At least 64 MiB of free staging space is required` | `release/install.sh` |
| Unpacked tar size | 1024 bytes to 2000000000 bytes, a multiple of 512, or `Unbounded or truncated tar archive` | `release/install.sh` |
| Output on success | The installed files. On failure the installer exits 1 and unpacks nothing | `release/install.sh` |

- [SEC-434] The installer MUST download the archive and `SHA256SUMS` over HTTPS from the same release tag, MUST NOT fall back to another release when a download fails, and MUST work in a private staging directory so that a failed check leaves no installed file.

## What the installer checks

The installer script downloads the archive and `SHA256SUMS` from the same release, then stops before it unpacks anything when the check fails. Each failure has a fixed message.

| Check | Failure message | Result |
|---|---|---|
| `SHA256SUMS` has a record for every archive of the release | `Malformed, duplicate or missing checksum record` | Nothing is unpacked. |
| The selected archive name has exactly one record | `Malformed, duplicate or missing checksum record` | Nothing is unpacked. |
| The record hash equals the SHA-256 of the downloaded archive | `Archive SHA-256 mismatch` | Nothing is unpacked. |
| The archive is a valid gzip file | `Malformed gzip archive or exhausted staging space` | Nothing is installed. |

- [SEC-430] The installer MUST compare the archive hash with `SHA256SUMS` before it decompresses or unpacks the archive, and MUST stop with `Archive SHA-256 mismatch` when the two hashes differ.

## Check a download by hand

Run these steps in the directory that holds the archive and the `SHA256SUMS` file you downloaded from the same release page.

1. Compute the hash of the archive. With `sha256sum`, run `sha256sum <archive-name>`.
2. Find the line in `SHA256SUMS` whose second field is the same archive name, and compare the first field with the hash from step 1. The two MUST be identical.
3. Check the build attestation of the archive with `gh attestation verify <archive-name> --repo arthurwolf/ilium`. The command MUST report a verified attestation for the release workflow of the repository before you run the archive.

A matching hash shows that the archive is the file listed in `SHA256SUMS`. It does not, by itself, show who built the file. The attestation check adds that proof.

Example of a good manual check (the hash shown is a placeholder):

```
$ sha256sum ilium-linux-x86_64.tar.gz
<64 hex characters>  ilium-linux-x86_64.tar.gz
$ grep ' ilium-linux-x86_64.tar.gz$' SHA256SUMS
<the same 64 hex characters>  ilium-linux-x86_64.tar.gz
$ gh attestation verify ilium-linux-x86_64.tar.gz --repo arthurwolf/ilium
```

- [SEC-435] A hash comparison MUST use the whole 64-character value and the exact archive name. A user who finds no line for the archive, or more than one, MUST treat the download as failed.

## Offline verification

This section applies to a site that cannot reach the download page or GitHub. The attestation check of [SEC-429] needs GitHub, so an offline site cannot run it itself. The hash check still works offline, once the files are copied in.

- [SEC-960] A site without GitHub access MUST run the attestation check of [SEC-429] on a connected machine, against the same archive bytes that the site will receive, and MUST NOT carry the archive into the site until that check reports a verified attestation. The hash of the archive MUST be computed again on the offline site.
- [SEC-961] A mirror of a host release MUST hold the five archives of the archive inventory and the `SHA256SUMS` of one release tag, with the file names and bytes unchanged. A mirror MUST NOT edit, rebuild or re-sign `SHA256SUMS`, and MUST NOT combine files from two release tags.
- [SEC-962] Each offline verification MUST leave a written record with the release tag, the archive name, the SHA-256 hash, the time of the check in `YYYY-MM-DD HH:mm:ss`, the machine that ran the attestation check and the result. A record without the attestation result MUST NOT be used as proof that the archive is genuine.

A matching hash on the offline site shows that the archive is the file listed in `SHA256SUMS`, as in [SEC-429]. It shows nothing about the builder, so the record of [SEC-962] is the only proof of origin that the site keeps.

Decision needed: whether each release should also publish an attestation bundle, a file that `gh attestation verify` can check without contacting GitHub. Default I would use: no bundle for now, because this page does not yet confirm that the GitHub CLI supports offline bundle checks in the version the project documents. Until the bundle or a detached signature exists, an offline site has no install path from a local archive: the installer of [SEC-434] downloads over HTTPS only, so this page does not define an offline install.

## What the host reports about itself

The host does not re-check the release archive at start. The checks above run once, in the installer, before the files are installed. To see which build is running, use:

- `ilium --version --verbose`, which prints the version and the path of the third-party notices file, or `notices: missing` when that file is absent ([SEC-423](/pages/reference-trust-and-supply-chain/)).

- [SEC-436] The host MUST NOT claim, in `ilium --version --verbose` or in any other output, that its own archive passed verification. Verification belongs to the installer and to the user, before the first run.

Decision needed: whether each release should also publish a detached signature over `SHA256SUMS`, made with a release key that users trust in advance.

- Default I would use: keep the build attestation as the only signed proof for now. The releases already carry attestations from the build workflow, and the release key that signs the blocklist is not designed to sign release archives, so reusing it would widen its role.
- Option: add a detached signature file, such as `SHA256SUMS.sig`, made with a key that is separate from the blocklist release key, and an installer check for it. This lets a user verify the list without the GitHub CLI, at the cost of a second key to manage and rotate.

## Failures

| Symptom | Cause | What to do |
|---|---|---|
| `Archive SHA-256 mismatch` | The archive changed after publication, or the download was cut short | Download the archive again from the same release page. Do not run it. |
| `Malformed, duplicate or missing checksum record` | `SHA256SUMS` is incomplete or was edited | Download `SHA256SUMS` again. If the error repeats, report the release tag. |
| `gh attestation verify` reports no attestation | The file is not the one the release workflow built | Do not run the file. Report the release tag and the file name. |

- [SEC-437] After any failed check the installer MUST exit with status 1, MUST print the fixed message of the failed check, and MUST NOT leave an unpacked or half-installed host.
- [SEC-438] An invalid gzip stream MUST stop the install with `Malformed gzip archive or exhausted staging space`. This check runs only after the hash check has passed.

## How a running host learns of a new release

The checks above run when you install. A host that is already running learns of a newer release through the update check described here. The check only tells the operator that a release exists. It never downloads, unpacks or installs a host.

**`channels.json`** is a release asset published beside `SHA256SUMS` and signed with the release key of [SEC-426]. It settles the open decision of [Release rollout](/pages/reference-release-rollout/) about where the channel and rollout percentage of a host release are stored. The installer of [PKG-954] reads the same file. For each channel (`stable`, `beta`, `dev`, [PKG-953]) it holds these fields:

| Field | Meaning |
|---|---|
| `tag` | The release tag, for example `v0.2.0`. |
| `version` | The version string of that release. |
| `rollout_percent` | 0 to 100. 0 means the release is paused ([PKG-958]). The cohort rule of [PKG-956] decides whether this host is offered it. |
| `minimum_supported_version` | The oldest host version the project still supports. A host older than this shows the notice as `unsupported`. |
| `notes_url` | The release notes page. |
| `archive_urls` | One HTTPS URL per archive of the archive inventory. |
| `sha256sums_url` | The HTTPS URL of the `SHA256SUMS` of that tag. |

### Setting, cadence and what is sent

- `[host] update_check = off|notify`. The default is `notify`. `off` makes no request. A policy file can lock the value ([Admin policy](/pages/reference-admin-policy/)), and a locked value cannot be changed from the settings page.
- The check runs at start and then every 24 hours while the host runs, each time after a random delay of up to 1 hour. A failed check backs off as in HST-693 and never blocks start.
- The request is an HTTPS GET of `channels.json` for the selected channel. It carries only a `User-Agent` of the form `ilium/<version>`. It sends no identifier, no cookie, no session name, no plugin list and no path.
- `--offline` makes no request and the notice shows nothing. On a connection that the operating system marks as metered, the check runs only at start and is skipped when the cached answer is under 24 hours old.
- The host verifies the signature of `channels.json` before it reads any field. A bad signature, a malformed file or a channel without an entry ends the check silently for the user and writes a log line with the reason. The host never uses a cached value that failed verification.

### What the operator sees

When the offered release is newer than the running version and this host is in the rollout cohort, the status bar shows `Update available: <version>` and `ilium version --json` reports `"update_available": {"version": ..., "tag": ..., "notes_url": ...}`. Otherwise the field is `null`. The notice does not replace the `Update: <advisory id>` badge of [SEC-968]. That badge comes from the advisories the running release was built with, which cannot know about later releases. The notice tells the operator to follow the install steps of this page and of [Install and update](/pages/reference-install-and-update/). The host does not install itself. A rollback target is kept as HST-513 requires.

- [SEC-434] applies to any later install: HTTPS only, the same release tag, no fallback.
- Requirement IDs for the file format, the setting and the notice are not yet in a requirements file. The behaviour above is proposed, owner to confirm.

## Official installation channels and distribution packages

| Channel | Official | Signed by the project |
|---|---|---|
| Release archives with `release/install.sh` and `SHA256SUMS` | Yes | Hashes and attestation, as in [SEC-428] and [SEC-429] |
| A package repository or package-manager formula maintained by the project | Only when the release page lists it | Only as that listing states |
| A package built by a distribution, a company or a person | No | Not by the project |

The release page is the only list of official channels. A channel that it does not list is not official.

A third-party package MUST keep these things:

- `THIRD-PARTY-NOTICES` next to the binary, so that `ilium --version --verbose` prints its path and not `notices: missing` ([SEC-423]).
- The `.build.json` provenance file next to each binary ([PKG-301] describes the install layout).
- The core plugins inside the binary. A package must not remove or replace them.
- `[host] update_check = off`, set by the package, when the package manager updates the host, so that the notice does not point away from the package manager.

`ilium --version --verbose` prints a `channel:` line. Its value is `archive` for the installer, the name written by the package for a package, and `unknown` when the build file is missing. A package that does not write the name is shown as `unknown`.

To report a bug in a package the project did not build, first reproduce it with an official archive of the same version. If it reproduces, report it with the `ilium --version --verbose` output. If it does not, report it to the package maintainer. The project does not support a package it does not list.

## Code signing of host binaries on macOS and Windows

Platform signing is separate from the checks above. The state of each platform is:

| Platform | Signature | Verify with |
|---|---|---|
| macOS archives | Not signed or notarized by default (proposed, owner to confirm) | `codesign -dv --verbose=4 <file>` and `spctl -a -vv <file>` |
| Windows binaries | No Authenticode signature by default (proposed, owner to confirm) | `signtool verify /pa /v <file>`, or the Digital Signatures tab of the file properties |
| Linux archives | Not applicable | Not applicable |

Until a signature is published, Gatekeeper on macOS reports that the developer cannot be verified, and SmartScreen on Windows shows a warning for an unrecognised app. The supported way past either is to verify the archive by [Check a download by hand], then allow it: on macOS, remove the quarantine flag with `xattr -d com.apple.quarantine <file>` or use the Open Anyway button in System Settings, and on Windows choose More info and Run anyway. A managed fleet that blocks unsigned binaries MUST NOT be told to bypass its policy. It should wait for a signed release, or re-sign the binary with its own certificate and record that in its own inventory.

When the project starts to sign, the release notes name the signing identity, and a rotation or revocation of the certificate is announced in the release notes and in [Release key compromise](/pages/reference-release-key-compromise/). A valid platform signature shows who signed the file. It does not replace the `SHA256SUMS` check or the attestation check of [SEC-429], and a signed file that fails either still MUST NOT be run. Requirement IDs for platform signing are not yet written. The statements above are proposed, owner to confirm.

## What this page does not cover

- Plugin package checksums and signatures: [Signing and trust](/pages/reference-signing-and-trust/).
- Release notices, licences and the notices path: [Trust and supply chain](/pages/reference-trust-and-supply-chain/).
- Loss of the release key and the blocklist key rules: [Release key compromise](/pages/reference-release-key-compromise/).
- Installer steps, versions and updates: [Install and update](/pages/reference-install-and-update/).

### See also

- [Signing and trust](/pages/reference-signing-and-trust/) for plugin package signatures.
- [Trust and supply chain](/pages/reference-trust-and-supply-chain/) for the release notices and licence checks.
- [Install and update](/pages/reference-install-and-update/) for the installer steps.

Contract: `ilium:plugin@0.1.0`.
