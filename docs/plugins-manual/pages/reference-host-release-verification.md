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

## What the host reports about itself

The host does not re-check the release archive at start. The checks above run once, in the installer, before the files are installed. To see which build is running, use:

- `ilium --version --verbose`, which prints the version and the path of the third-party notices file, or `notices: missing` when that file is absent ([SEC-423](/pages/reference-trust-and-supply-chain/)).

Decision needed: whether each release should also publish a detached signature over `SHA256SUMS`, made with a release key that users trust in advance.

- Default I would use: keep the build attestation as the only signed proof for now. The releases already carry attestations from the build workflow, and the release key that signs the blocklist is not designed to sign release archives, so reusing it would widen its role.
- Option: add a detached signature file, such as `SHA256SUMS.sig`, made with a key that is separate from the blocklist release key, and an installer check for it. This lets a user verify the list without the GitHub CLI, at the cost of a second key to manage and rotate.

## Failures

| Symptom | Cause | What to do |
|---|---|---|
| `Archive SHA-256 mismatch` | The archive changed after publication, or the download was cut short | Download the archive again from the same release page. Do not run it. |
| `Malformed, duplicate or missing checksum record` | `SHA256SUMS` is incomplete or was edited | Download `SHA256SUMS` again. If the error repeats, report the release tag. |
| `gh attestation verify` reports no attestation | The file is not the one the release workflow built | Do not run the file. Report the release tag and the file name. |

### See also

- [Signing and trust](/pages/reference-signing-and-trust/) for plugin package signatures.
- [Trust and supply chain](/pages/reference-trust-and-supply-chain/) for the release notices and licence checks.
- [Install and update](/pages/reference-install-and-update/) for the installer steps.

Contract: `ilium:plugin@0.1.0`.
