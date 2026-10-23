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
