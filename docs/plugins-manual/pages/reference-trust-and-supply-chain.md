---
layout: page
title: Trust and supply chain
permalink: /pages/reference-trust-and-supply-chain/
---

## Trust and supply chain

This page defines how a package proves where it came from, how Ilium decides to trust a publisher, and how a package is stopped when it turns out to be bad. It also fixes the format of signatures, keys and fingerprints, so that two implementations agree and a test author can build fixtures. No registry is configured by default (decision D3 covers signing and trust; the optional registry is decision D27, see [Registry](/pages/reference-registry/)), so every rule here works on files the user already holds.

A **publisher** is the person or organisation that signs a package. A **key** is an Ed25519 key pair; the **public key** is shared, and the **private key** signs. A **fingerprint** is a short, fixed-size name for a public key. A **trust store** is the list of public keys the user trusts. **Pinning** means that the first key seen for a plugin id is remembered, and later versions must match. A **blocklist** is a list of packages, plugin ids or keys the host refuses.

### You will need

- The package layout in [Package format](/pages/reference-package-format/) and [Packages](/pages/packages/).
- The CLI in [Plugin CLI](/pages/reference-plugin-cli/).
- The attacker classes A1 and A2 in the [Threat model](/pages/reference-threat-model/).

## Signatures

**[SEC-140]** Canonicalisation. The signed bytes are the **canonical form of `manifest.json`** defined by RFC 8785 (JSON Canonicalization Scheme): UTF-8, object members sorted by code point of the name, no insignificant whitespace, numbers in the shortest form. A manifest with duplicate member names is invalid (`manifest.duplicate-key`). The manifest lists every other file with its SHA-256 ([SEC-153]), so signing the manifest signs the package.

**[SEC-141]** The `signature` file is a JSON document:

```json
{
  "version": 1,
  "algorithm": "ed25519",
  "key_id": "3f2a91c07b6d1e44a1b20c9e55d3f7a8b6c102e94d7798af13c56e0bd2f48a19",
  "signature": "<base64url, 64 bytes, no padding>"
}
```

`key_id` is the full fingerprint ([SEC-177]) of the signing public key. The `signature` is over the canonical manifest bytes prefixed with the ASCII string `ilium-plugin-manifest-v1\n`. A bad algorithm name or version gives `manifest.signature-invalid`.

**[SEC-177]** Fingerprint. The fingerprint is the SHA-256 of the raw 32-byte public key. It is shown as 64 lowercase hexadecimal digits grouped by 4 (`3f2a 91c0 …`). The short form is the first 8 digits and is shown only beside a full form or in `trust list`. Two keys never share a short form in one trust store: the store refuses to add a key whose short form collides (`trust.collision`).

**[SEC-178]** Public key file `publisher.pub` is a UTF-8 JSON document `{ "version": 1, "publisher": "Example Corp", "algorithm": "ed25519", "public_key": "<base64url, 32 bytes>" }`. The private key file `publisher.key` has mode 0600 and contains the 32-byte seed in base64url, wrapped by a passphrase (Argon2id and XChaCha20-Poly1305) unless `--no-passphrase` is given.

## The trust store

**[SEC-144]** The trust store ([SEC-112]) holds public keys the user trusts. A key may be trusted under one publisher name; one publisher name may have several keys. `ilium plugin trust remove <key_id>` removes a key. Installed plugins signed by it stop loading at the next activation and show `Publisher no longer trusted`; they are not uninstalled and their data stays. Trusting the key again reactivates them.

## Key pinning and rotation

**[SEC-142]** Key pinning. The first install of a plugin id records the signing key fingerprint (or `unsigned` for T3). An update of that id MUST be signed by the same key, or by a key that the old key has endorsed with a rotation statement. Otherwise the update is refused with `package.publisher-key-changed`, shown in the dialog in [Grant review and consent](/pages/reference-grant-review-and-consent/). The user can uninstall the plugin first, which discards the pin. A T3 package installed over a T2 plugin (or the reverse) is a tier change and resets all grants ([SEC-109]).

**[SEC-143]** Rotation statement. A rotation statement is a JSON file `rotation.json` in the new package:

```json
{
  "version": 1,
  "plugin_id": "example.notes",
  "old_key_id": "3f2a…",
  "new_key_id": "a1b2…",
  "issued": "2026-10-12T08:00:00Z",
  "signature": "<base64url, signature of the old key over the canonical form of this object without the signature member>"
}
```

The host accepts the new key for `plugin_id` only if the statement verifies under the pinned old key and `issued` is not in the future. The new key does not need to be in the trust store beforehand; the user is asked to trust it in the same dialog.

**[SEC-145]** Downgrade. A lower version of an installed id is refused unless `--allow-downgrade` is given. The downgrade is audited with both versions.

## Revocation and blocklists

**[SEC-146]** Release blocklist. Each Ilium release ships a blocklist signed with the project's release key. It is verified with the key compiled into the binary. An administrator or user cannot edit it.

**[SEC-147]** Policy blocklist. The admin policy `[packages] blocklist_file` names a file in the same format, signed or unsigned. An unsigned policy blocklist is accepted only if the policy file itself is root-owned ([SEC-182]).

**[SEC-148]** Blocklist format:

```json
{
  "version": 1,
  "issued": "2026-10-12T00:00:00Z",
  "entries": [
    { "kind": "package-sha256", "value": "<64 hex>", "reason": "malware" },
    { "kind": "plugin", "id": "example.bad", "versions": ">=1.0.0 <1.4.2", "reason": "grant bypass" },
    { "kind": "publisher-key", "value": "<fingerprint>", "reason": "key compromise" },
    { "kind": "runtime", "advisory": "<advisory id>", "versions": "<Wasmtime version range>", "reason": "sandbox escape" }
  ]
}
```

A `runtime` entry names an advisory (`advisory`, a non-empty string), the affected runtime versions (`versions`, a semver range) and a `reason`. It matches when the running Wasmtime version, as recorded in the build metadata ([SEC-170]), lies in `versions`. It does not match a package or a plugin; its effect is the one in [SEC-171]. It comes only from the signed release blocklist, never from a policy or user blocklist.

**[SEC-149]** A package that matches an entry is refused at install with `package.blocked`, and an installed plugin that matches is disabled at the next load with a notice `Blocked: <reason>`. Its data is kept. The audit log records `blocklist-hit`. A blocked plugin cannot be enabled by `plugins.manage`, the Plugins screen or the CLI.

## Archive parsing

A `.iliumpkg` is a ZIP file. ZIP has many ambiguities that attackers use. The reader is strict.

**[SEC-150]** The archive reader MUST refuse, with these codes:

| Defect | Code |
|---|---|
| Entry name with `..` component, absolute path, a drive letter, a backslash, a NUL byte, or an empty component | `package.path-invalid` |
| Two entries with the same name, or two names equal after Unicode NFC normalisation and case folding | `package.path-duplicate` |
| An entry that is a symbolic link, device, FIFO or hard link (external attributes), or has a setuid or setgid bit | `package.entry-symlink` or `package.entry-directory` |
| ZIP64 records, multi-disk archives, encrypted entries, compression methods other than stored and deflate, data descriptors that disagree with headers, local header fields (name, size, CRC) that differ from the central directory, trailing data after the end record, overlapping entries | `package.zip-invalid` |

**[SEC-151]** Limits: at most 4,096 entries; entry names at most 255 bytes and paths at most 1,024 bytes; total uncompressed size at most 512 MiB; a compression ratio above 200 to 1 for any entry is refused. These limits apply before any file is written. Exceeding one gives `package.zip-invalid` with a detail naming the limit.

## Install location and integrity

**[SEC-152]** Packages are unpacked into the store of the user's data folder (`<data>/plugins/store/<id>/<version>/`; the data folder is `~/.local/share/ilium` on Linux, `~/Library/Application Support/ilium` on macOS and `%APPDATA%\ilium\data` on Windows, as in [Plugin files](/pages/reference-plugin-files/)). The directory is created 0700 and files get mode 0600, as in [Plugin files](/pages/reference-plugin-files/). A system-wide package directory, if used, is root-owned and read-only for the user ([Enterprise deployment](/pages/guide-enterprise-deployment/)).

**[SEC-153]** At each load the host re-hashes every component and asset against the SHA-256 recorded in the manifest before it hands the bytes to Wasmtime. A mismatch refuses the load with `manifest.file-hash`, disables the plugin with a notice `Files changed after install`, and writes an audit line `integrity-failure`. The host does not repair the files. Assets are checked at first read, and a mismatch returns `io-error` with reason `integrity`.

**[SEC-154]** `ilium plugin inspect <file>` prints, without installing and without running any plugin code: the manifest, each grant with its risk, the signer fingerprint and verification result, the tier it would install as, each file's size and hash, and the optional provenance entries. Output is JSONL: one `manifest` line, one `grant` line per grant, one `file` line per file, one `signature` line, one `summary` line.

**[SEC-156]** `ilium plugin verify <id or file>` re-checks an installed plugin (by id) or a package file (by path) against [SEC-153] and prints the result. A file is checked offline and without installing; a name that is not an installed id and not an existing file fails with `plugin.not-found` (exit 5).

**[SEC-155]** Optional provenance. A package MAY contain `sbom.spdx.json` (an SPDX 2.3 document of the guest dependencies) and `provenance.json` (an in-toto statement of how the package was built). Both are listed in `files`, hashed like every other file, and shown in Details. The host does not trust them; it displays them.

**[SEC-176]** T1 bundled components. The hashes of the embedded `.cwasm` files are compiled into the binary. The loader verifies each before use, and a mismatch is a fatal startup error for that plugin (`bundled.integrity`); the plugin is disabled and the built-in fallback, if any, is used.

## Dependencies

**[SEC-170]** The Wasmtime version is pinned exactly in the release. The build records it in the build metadata (`ilium --version --verbose` prints it). `Cargo.lock` is committed and the build uses `--locked`.

**[SEC-171]** Advisory response. The project monitors the Wasmtime advisory feed and the advisories of the codecs Ilium uses (image, audio, inflate, video). For a sandbox-escape or memory-safety advisory that applies to the pinned version, a patched release MUST be published within 14 days of the upstream fix for high severity and 48 hours when the issue is exploited in the wild. While a vulnerable version runs, the host adds the advisory id to its release blocklist as a `runtime` entry, and T2 and T3 plugins refuse to load with `sandbox.unavailable` and reason `runtime-vulnerable` when the user's policy sets `[packages] block_vulnerable_runtime = true`. By default the host warns at start and in Plugins.

**[SEC-172]** Codec dependencies are pinned the same way and fuzzed ([Security testing](/pages/reference-security-testing/)).

**[SEC-173]** SDK provenance. The Rust crate `ilium-plugin-sdk` and the npm package `@ilium/plugin-sdk` are released from the Ilium repository by CI, with signed tags. The npm package is published with provenance (`npm publish --provenance`). Templates created by `ilium plugin new` pin the exact SDK version and commit the lock file. A plugin build SHOULD use `--locked` (Rust) and `npm ci` (TypeScript).

### Licence attribution

**[SEC-420]** Release attribution file. Every Ilium release MUST ship a `THIRD-PARTY-NOTICES` file next to the binaries. It lists every third-party crate linked into the shipped binaries and `.cwasm` components (Wasmtime and its Cranelift, WASI and component-model crates included) with its name, exact version, SPDX licence expression and the full licence text. The file is generated from `Cargo.lock` by CI, never written by hand.

**[SEC-421]** Notice completeness. The list in `THIRD-PARTY-NOTICES` MUST equal the set of packages in `Cargo.lock` that reach a shipped target. A crate that is in the build and absent from the file, or in the file and absent from the build, fails the release check.

**[SEC-422]** Licence policy. A dependency whose licence is outside the allow list of the repository (`deny.toml`, permissive licences and the Apache-2.0 WITH LLVM-exception expression Wasmtime uses) fails the release check. A dependency with no declared licence also fails.

**[SEC-423]** Runtime visibility. `ilium --version --verbose` prints the path of the notices file and the Wasmtime licence expression. An installed binary without the notices file next to it prints `notices: missing`.

## Advice for publishers

**[SEC-174]** A publisher SHOULD keep the private key encrypted, store a backup offline, and sign only on a clean build machine. On suspected compromise the publisher should: stop signing; publish a rotation statement if the old key is still under their control (see [Security response](/pages/reference-security-response/)); ask the Ilium project for a blocklist entry for the key; ship a fixed version signed by a new key; and tell users to uninstall and trust the new key.

## Worked example

```text
ilium plugin keygen --publisher-id com.example --publisher "Example Corp" --out ~/keys/example
ilium plugin sign ./dist/example.notes-1.2.0.iliumpkg --key ~/keys/example/publisher.key
ilium plugin inspect ./dist/example.notes-1.2.0.iliumpkg
ilium plugin trust add ~/keys/example/publisher.pub
ilium plugin install ./dist/example.notes-1.2.0.iliumpkg
```

`inspect` prints the signer fingerprint in full. The user compares it with a value published by the author before running `trust add`. A later `example.notes-1.3.0` signed by another key fails with `package.publisher-key-changed` unless it carries a rotation statement signed by the first key.

## Edge cases

| Situation | Result |
|---|---|
| Two keys with the same short fingerprint | `trust add` refuses the second with `trust.collision` |
| Rotation statement with a future `issued` date | Ignored; the update is refused |
| Archive has a file not listed in `files` | `manifest.file-unlisted` |
| Package matches a blocklist entry and a policy allow entry | Blocked: deny wins |
| Install directory edited by hand | Next load fails with `manifest.file-hash` |
| Blocklist file malformed | The release blocklist is ignored with an audit line; the last good copy is used if one exists |

### See also

- [Packages](/pages/packages/)
- [Manifest reference](/pages/reference-manifest/)
- [Grant review and consent](/pages/reference-grant-review-and-consent/)
- [Admin policy](/pages/reference-admin-policy/)
- [Security response](/pages/reference-security-response/)
- [Errors and limits](/pages/reference-errors-and-limits/)

Contract: `ilium:plugin@0.1.0`.
