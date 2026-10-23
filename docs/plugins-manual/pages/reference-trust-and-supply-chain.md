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

## Which page defines the signature format

This page and [Signing and trust](/pages/reference-signing-and-trust/) describe the same signature in different words. A signer and a verifier built from different pages would refuse every package, so one page is normative. **[Signing and trust](/pages/reference-signing-and-trust/) is the single normative source for the wire format of the signature, the signed message and the rotation statement.** Where this page differs, the text there wins, and an implementation follows it.

| Question | Normative answer (Signing and trust) | Superseded wording on this page |
|---|---|---|
| What bytes are signed | The message `M` of PKG-201: `ilium-plugin-signature-v1`, `0x00`, the publisher id, `0x00`, then the 32 raw bytes of the SHA-256 of `manifest.json`. That SHA-256 is taken over the exact archived bytes of the `manifest.json` entry. The host does not parse, canonicalise or re-serialise the manifest first (PKG-202). | SEC-140: the RFC 8785 canonical form of `manifest.json`, with the prefix `ilium-plugin-manifest-v1\n` |
| Shape of the `signature` entry | `{ "format": "ilium-sig/1", "signatures": [ ... ] }` with 1 to 4 objects holding `algorithm`, `key_id`, `publisher_id`, `signature`, `signed_at`; the first is the publisher, a later one may be a registry countersignature; at most 16 KiB (PKG-204 to PKG-208) | SEC-141: a single object with `version`, `algorithm`, `key_id`, `signature` |
| Rotation statement | `rotation.json` of format `ilium-rot/1`, keyed by `publisher_id` and carrying the `new_key` (PKG-238) | SEC-143: a statement keyed by `plugin_id` with `new_key_id` |

Why exact bytes: signing the archived bytes removes the parser from the trust boundary. Duplicate member names, number forms and Unicode escapes cannot be read differently by the signer and the verifier, because neither reads the manifest before the hash. The duplicate-key refusal `manifest.duplicate-key` of SEC-140 still applies, but only after the signature has verified, when the manifest is parsed for use.

What stays valid on this page: SEC-153 (every other file is hashed in the manifest, so the signature covers the package), SEC-177 (the fingerprint and `key_id` are the SHA-256 of the raw public key) and the refusal codes `manifest.signature-invalid`, `package.signature-format` and `package.publisher-key-changed`. Treat the SEC-140, SEC-141 and SEC-143 wording above as superseded by PKG-200 to PKG-208 and PKG-238; the SEC-459 rule on a closed algorithm allow-list applies to both.

The test vector to build fixtures from is the one in [Signing and trust](/pages/reference-signing-and-trust/#test-vector): seed `000102...1f`, publisher id `example`, the 402-byte minimal manifest, `key_id` `56475aa75463474c0285df5dbf2bcab73da651358839e9b77481b2eab107708c`, and the signature `8efe7370...a6507`. It is stated there once and is not repeated here, so it cannot drift. A verifier that reproduces it, and refuses the negative vectors PKG-248 to PKG-258, conforms.

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

## Blocklist and emergency-disable refresh for hosts with no registry

No registry is configured by default (decision D27). A host that installs from files would otherwise never receive a revocation between releases, and the stale warning of EXT-072 would show for ever after 7 days. So the project runs a **revocation feed**, and every host polls it unless policy turns it off.

- **Location and key.** The feed URL is compiled into the binary and printed by `ilium --version --verbose` as `revocation_feed`. The feed is signed with a dedicated feed key whose public half is also compiled in; it is not the release key, so a leaked feed key cannot sign a release. The feed key has its own compromise rule: the compromise is announced under [Release key compromise](/pages/reference-release-key-compromise/), a new feed key ships in the next release, and per SEC-425 a feed signed by any key other than the compiled-in one is refused, the last good copy is kept and an audit line is written.
- **Content.** The feed is a blocklist in the format of SEC-148 (`package-sha256`, `plugin`, `publisher-key` and `runtime` entries), plus the emergency-disable entries that EXT-057 and EXT-058 define for a registry feed. Entries are matched and enforced as in SEC-149 and EXT-058.
- **Polling.** The host fetches the feed at start and then every 6 hours plus a random jitter of up to 30 minutes, the same period EXT-057 sets for a registry. A fetch uses HTTPS with certificate validation, a 10 s timeout and a 1 MiB size cap, and sends a conditional request (`If-None-Match`) so an unchanged feed costs one small reply. A failed fetch is retried with back-off from 5 minutes up to the normal period and never blocks start.
- **Privacy.** The request carries only the path and the standard headers with `User-Agent: ilium/<version>`, exactly as PKG-376 requires of a registry client: no identifier, no list of installed plugins, no install count. The project reads only its own server logs.
- **Policy.** `[packages] revocation_feed = false` turns the poll off. `[packages] revocation_feed_url = "<https url or file path>"` points the host at an internal mirror of the signed feed, which is verified with the same compiled-in key; it can only redirect the source, never the key. Air-gapped fleets mirror the file and distribute it with `blocklist_file` or the mirror URL. See [Admin policy](/pages/reference-admin-policy/).
- **Runtime advisories.** A `runtime` entry of SEC-171 reaches installed binaries through this feed between releases. The feed is the channel the host uses to "add" the entry to the release blocklist: the entry is held beside the release blocklist, is not written into the binary, and is dropped when a release that fixes the Wasmtime version no longer matches it. SEC-729 (an emergency-disable entry within 4 hours of confirmation) is met by publishing to this feed, which hosts pick up within one poll period.
- **Merging.** The effective blocklist is the union of the release blocklist, the feed, the policy blocklist (SEC-147) and the emergency-disable feed of each configured registry (PKG-407). A match in any source blocks; deny wins, as in the edge-case table. A source that fails verification is ignored on its own and does not affect the others.
- **Staleness.** The 7 days of EXT-072 count from the last successful verified refresh of the feed. A stale feed is kept and enforced, and the user sees `Revocation data is stale`. A host with the feed turned off by policy has no refresh source: it enforces the release blocklist and the policy blocklist, and shows `Revocation data is stale` only for a policy blocklist file or mirror that has not been updated for 7 days. It does not show the warning for the release blocklist, which is current as of the release (its age is the release age; see SEC-967 for the advisory check). Such a host receives no revocations between releases, and the Plugins screen says `Revocation feed: off by policy`.

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

**[SEC-153]** At each load the host re-hashes every component and asset against the SHA-256 recorded in the manifest before it hands the bytes to Wasmtime. A mismatch refuses the load with `manifest.file-hash`, disables the plugin with a notice `Files changed after install`, and writes an audit line `integrity-failure`. The host does not repair the files. Assets are checked at first read, and a mismatch returns `io` with the detail `integrity`.

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

## Dependency advisory scanning for the host release

SEC-171 names the Wasmtime feed and four codec families. The rule extends to the whole `Cargo.lock` of the host, so the host meets the bar that DEV-896 sets for publishers.

- Every CI run and every release job runs an advisory scan of `Cargo.lock` against the RustSec database with `cargo audit` or `cargo deny check advisories`. An advisory with no recorded review fails the job.
- A new or upgraded dependency needs a review record (`cargo vet` audit or an equivalent note in the pull request) before it merges. The same `deny.toml` that enforces SEC-422 holds the advisory policy.
- The response times of SEC-171 apply to any crate reachable from plugin input: HTTP, TLS, archive, JSON, image, audio and video crates, not only Wasmtime and the codecs.
- An advisory accepted without a fix is listed with its reason in the release notes and appears in the `advisories` of `ilium version --json` (SEC-967) when it affects the shipped version.
- The scan database is refreshed on each run, so a release cannot ship on a stale database.

## Software bill of materials and provenance of a host release

- Every host release ships a machine-readable SBOM in CycloneDX JSON next to `SHA256SUMS` (SEC-428), named `ilium-<version>.cdx.json`, and listed in `SHA256SUMS`. It is generated from the same `Cargo.lock` as `THIRD-PARTY-NOTICES` (SEC-420) and covers the binaries and the bundled core plugin components (SEC-176). The set of components in the SBOM equals the set in `THIRD-PARTY-NOTICES` (SEC-421); a difference fails the release check.
- The build attestation names the SBOM and the archives by SHA-256, so a procurement team verifies in order: the archive hash against `SHA256SUMS` ([Host release verification](/pages/reference-host-release-verification/)), the attestation over those hashes, then the SBOM hash named in the attestation.
- Host builds are reproducible: two builds from the same tag with the pinned toolchain, `--locked` (SEC-170) and no network produce byte-identical archives, as SEC-727 requires. The SBOM format is the one SEC-727 names. Plugin packages may carry their own SBOM (SEC-155, PKG-680) and are verified with DEV-892.

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

## Transparency log

This section is target behaviour for the public plugin system. The registry keeps an append-only log of every published release and every signing event, so that a client or an auditor can detect a registry that shows different histories to different users (a split view) or that silently signs a release again.

- [PKG-700] The registry MUST publish an append-only transparency log of every release publication, yank, takedown and key rotation. Each entry MUST carry the plugin id, the version, the package digest, the signing key fingerprint and a timestamp, and the log head MUST be signed with the registry signing key. Removing or rewriting an entry MUST be impossible without changing the signed head.
- [PKG-701] A client MUST check an inclusion proof for a package against the log head before it installs or updates it, and MUST keep the last log head it has seen. A head that is not an extension of the stored head, or a package with no valid proof, MUST be refused with `trust.log-inconsistent`, and the client MUST keep the evidence for the audit log.

## Edge cases

| Situation | Result |
|---|---|
| Two keys with the same short fingerprint | `trust add` refuses the second with `trust.collision` |
| Rotation statement with a future `issued` date | Ignored; the update is refused |
| Archive has a file not listed in `files` | `manifest.file-unlisted` |
| Package matches a blocklist entry and a policy allow entry | Blocked: deny wins |
| Install directory edited by hand | Next load fails with `manifest.file-hash` |
| Blocklist file malformed | The release blocklist is ignored with an audit line; the last good copy is used if one exists |

## Publisher ownership transfer

A plugin id belongs to one publisher key. This section says how an id moves to another publisher and how an abandoned id is recovered.

- [LIF-309] A plugin id MUST move to another publisher only through a transfer statement signed by the current publisher key and countersigned by the receiving key. A host MUST accept the new key for that id only when both signatures verify, and MUST otherwise keep the pinned key.
- [LIF-310] After a transfer the existing grants and the plugin data MUST stay unchanged, but the host MUST show the user the new publisher key fingerprint once and MUST ask for consent again before the next update from the new key is applied.
- [LIF-311] An abandoned id MAY be recovered only through the registry with a recovery request that a human reviews. A recovered id MUST NOT inherit the old pin, so every host MUST treat the first package under the new key as a new install.

## Clock skew and time-based trust

Signatures, certificates and revocation lists carry times. The system clock can jump or be wrong.

- [EXT-071] The host MUST verify a package signature against the signing time recorded in the signature and not against the current clock. A clock set before that time or far in the future MUST NOT make a valid signature fail or an expired key valid.
- [EXT-072] A revocation list or blocklist MUST be treated as stale after 7 days without a successful refresh. A stale list MUST NOT be dropped; the host MUST keep enforcing it and MUST show `Revocation data is stale`. A list whose own issue time is later than the clock plus 24 h MUST be rejected as `trust.clock-skew`.
- [EXT-073] Durable timers of plugins MUST be driven by the monotonic clock for their duration and MUST NOT fire in a burst after a wall-clock jump. A timer set for a wall-clock time MUST fire once, when that time is reached, and MUST be logged as `timer.clock-jump` when the clock moved while it waited.

### See also

- [Packages](/pages/packages/)
- [Manifest reference](/pages/reference-manifest/)
- [Grant review and consent](/pages/reference-grant-review-and-consent/)
- [Admin policy](/pages/reference-admin-policy/)
- [Host release verification](/pages/reference-host-release-verification/) for checking the downloaded host release.
- [Security response](/pages/reference-security-response/)
- [Release key compromise](/pages/reference-release-key-compromise/)
- [Errors and limits](/pages/reference-errors-and-limits/)

Contract: `ilium:plugin@0.1.0`.

## Production additions: T2-security

- [SEC-725] A project-level or private registry entry MUST NOT shadow a public id silently: when two configured registries serve the same plugin id, the host MUST use the registry the plugin was installed from and MUST refuse an update from the other with `registry.source-changed` unless the user passes `--switch-registry`.
- [SEC-726] The registry MUST run automated static analysis on every upload (denied imports, oversize components, known malicious digests) and MUST hold a version that matches a rule in state `pending-review` for at most 72 h (proposed, owner to confirm), during which clients MUST NOT receive it in metadata.
- [SEC-727] Release builds of the host and of every first-party plugin MUST be reproducible: two builds from the same tag on different machines MUST produce byte-identical artifacts, and the release job MUST publish the SBOM (CycloneDX) and the SLSA provenance attestation beside the artifact.
