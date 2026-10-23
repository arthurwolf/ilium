---
layout: page
title: Publishing guide
permalink: /pages/guide-publishing/
---

## Publishing guide

This guide shows how to take a finished plugin and publish it so that other people can install it. You will create a signing key, build a package that others can reproduce, sign it, check it and upload it to a registry. Support and issue handling are in [Publishing and support](/pages/guide-publishing-and-support/).

### You will need

- A plugin that passes `ilium plugin check` and `ilium plugin test`. See [Tutorial: package and share](/pages/tutorial-09-package-and-share/).
- The manifest rules: [Manifest reference](/pages/reference-manifest/).
- The key and signature formats: [Signing and trust](/pages/reference-signing-and-trust/).

Requirement ids such as `PKG-490` are in [Requirements: manifest and package](/pages/reference-requirements-manifest-package/).

## Choose an id and a version

Your id starts with a namespace that you own, such as `com.example.` or your registry handle. The registry checks this when you publish. See [Registry reference](/pages/reference-registry/#namespaces).

| Id | Rule |
|---|---|
| [PKG-490] | The plugin id MUST start with `<publisher_id>.` of the signing key (PKG-218). Otherwise signing fails with `manifest.id-publisher`. |
| [PKG-491] | Use SemVer. Raise the major number when you remove a command, a service function or a setting. See [Versions and dependencies](/pages/reference-dependency-resolution/#plugin-versions). |
| [PKG-492] | A version is immutable once published. A fix needs a new version (PKG-394). |

## Create a signing key

Run `ilium plugin keygen --publisher-id com.example --publisher "Example Labs" --out ~/keys/example`. Keep the private file offline and back it up. If you lose it, users must trust a new key. If someone steals it, follow the runbook in [Signing and trust](/pages/reference-signing-and-trust/#a-compromised-key-the-publisher-runbook).

| Id | Rule |
|---|---|
| [PKG-493] | The private key MUST NOT be stored in the plugin repository. For CI, put it in a secret store and pass it through `ILIUM_PUBLISHER_KEY` (PEM text) or `ILIUM_PLUGIN_SIGNING_KEY` (path of a key file). |
| [PKG-494] | Encrypt a key kept on disk. Pass the passphrase through `ILIUM_KEY_PASSPHRASE`, never on the command line. |

## Publisher key custody and signing in CI

A publisher key is the one secret that the whole trust of your plugins rests on. Everything the manual says about pinning, revocation and expiry (see [Signing and trust](/pages/reference-signing-and-trust/)) assumes the key stayed with you. This section turns the one-line advice of SEC-174 into a procedure.

**Generate and store**

1. Run `keygen` on a trusted machine that you control, not on a shared build server or a cloud shell.
2. Encrypt the key at rest (PKG-211). Give the passphrase through `--passphrase-file` or `ILIUM_KEY_PASSPHRASE`, never on the command line (PKG-494). An empty passphrase is refused.
3. Keep one key per publisher id. Do not copy the same key into two CI systems or to a colleague; use a second key with the same publisher id if two systems must sign, so that each can be revoked on its own.
4. Keep an offline backup of the encrypted key and of a prepared rotation statement `rotation.json` (PKG-238) in a place that is not the repository (PKG-493).

**Signing in CI**

- Store the key in the CI secret store and pass it as `ILIUM_PUBLISHER_KEY` (PEM text) or `ILIUM_PLUGIN_SIGNING_KEY` (path of a key file) (PKG-493, PKG-213).
- Bind the secret to the release branch or to release tags only. Jobs for pull requests and for forks MUST NOT see it. If a job without the secret reaches `sign`, the command fails with a missing-key error and no package is signed; make the job fail rather than fall back to an unsigned upload (`publish.unsigned`).
- Run the sign step only after a clean checkout and the final `pack` (PKG-505). Do not print the environment in the log; mask the secret value in the CI settings.
- Protect the release branch or tag so that only maintainers can create it; anyone who can push a release tag can make CI sign.

**Rotation and departures**

- Rotate when a maintainer with access to the key or to the CI secret leaves, and when the CLI warns about expiry. The registry refuses a publish signed with an expired key and the CLI warns 60 days before expiry (SEC-722).
- To rotate, sign `rotation.json` with the old key and publish the next version signed by the new key (PKG-238). Users that pinned the old key accept the new one through the statement.
- Registry accounts need a second factor, at least two maintainers per publisher id, and notify every maintainer on publish and key change (SEC-723). Check these notices after each rotation.

**Review level B**

A registry at review level B (PKG-401) reviews first publishes and packages that request high-risk grants. Such a publisher is expected to hold the key on a hardware token or in an encrypted file; a registry operator MAY refuse a publisher that signs with an unencrypted key kept in a CI variable only. Level B is not a statement that your key is safe; it is a statement that a person looked at the package.

**If the key leaks**

Act at once: revoke the key as `compromised` and follow the drill in [Release key compromise](/pages/reference-release-key-compromise/), [Registry key compromise](/pages/reference-registry-key-compromise/) and the runbook in [Signing and trust](/pages/reference-signing-and-trust/#a-compromised-key-the-publisher-runbook) (PKG-242 to PKG-247). Installed copies signed by that key become `disabled-blocked` until users take an update signed by another trusted key of your publisher id (USR-177).

| Id | Rule |
|---|---|
| [SEC-174] | A publisher SHOULD keep the private key encrypted, store a backup offline, and sign only on a clean build machine. |
| [PKG-493] | The private key MUST NOT be in the repository; CI uses a secret store. |
| [PKG-494] | A key on disk is encrypted and the passphrase is passed through `ILIUM_KEY_PASSPHRASE`. |

## Reproducible builds

A build is reproducible when two machines make the same package bytes from the same source. Reproducible builds let users and registries confirm that a package really comes from the source you published.

`ilium plugin pack` already writes a deterministic archive: fixed entry order, fixed timestamps, no extra fields (see [Package format](/pages/reference-package-format/#the-ilium-plugin-pack-algorithm)). The remaining source of difference is the WebAssembly component that your compiler writes.

| Id | Rule |
|---|---|
| [PKG-495] | Pin the toolchain version in the repository (`rust-toolchain.toml` or `package-lock.json`). |
| [PKG-496] | Build with the lock file frozen (`cargo build --locked`, `npm ci`). |
| [PKG-497] | Do not embed the build time, the user name or an absolute path. In Rust set `--remap-path-prefix`. |
| [PKG-498] | Write `build-info.json` with the toolchain name and version and the source commit. `pack` includes it and the registry shows it. |
| [PKG-499] | Verify: build twice in clean folders and compare `sha256sum` of the two `.iliumpkg` files. They MUST be equal. |

If the two hashes differ, unpack both with any ZIP tool and compare the entries one by one. The differing entry is almost always a `.wasm` file.

## SBOM

An SBOM is a list of every library that went into your component, with versions and licences. It helps users and administrators answer "does this plugin contain the vulnerable library?".

| Id | Rule |
|---|---|
| [PKG-500] | Put `sbom.json` in the project root. `pack` includes it as `sbom.json`. |
| [PKG-501] | The file SHOULD be CycloneDX 1.5 JSON or SPDX 2.3 JSON. The host reads it as opaque data and does not validate its content. |
| [PKG-502] | The file MUST be at most 4 MiB. A larger file fails with `package.size-limit`. |
| [PKG-503] | Generate it from the lock file, for example `cargo cyclonedx` or `npx @cyclonedx/cyclonedx-npm`. |
| [PKG-504] | A registry at review level B MAY require an SBOM (PKG-401). |
| [PKG-685] | To reach review level B, include `sbom.json`, a valid SPDX `license` in the manifest and a `LICENSE` or `NOTICE` file. Without them the registry refuses the publish with `registry.policy` (PKG-681) when it requires level B, and otherwise shows level A with `No SBOM` or `No licence notice` (PKG-682). The full list is in PKG-680. |

## Pack, sign, check

```
ilium plugin pack
ilium plugin sign dist/com.example.git-tools-1.4.2.iliumpkg --key ~/keys/example.key
ilium plugin verify dist/com.example.git-tools-1.4.2.iliumpkg
```

`verify` runs the install stages 1 to 7 without installing. A signed package has a `signature` entry that covers the exact bytes of `manifest.json`.

| Id | Rule |
|---|---|
| [PKG-505] | Sign only after the final `pack`. Changing any file after signing makes `manifest.file-hash` or `manifest.signature-invalid`. |
| [PKG-506] | Run `verify` and install the file on a clean machine before you upload. |

## Secret detection in pack and check

`pack` includes every file under `src/` and `assets/` (PKG-143), and a published version can never be changed (PKG-394). A secret that enters a package therefore stays public. AUT-304 and AUT-306 forbid secrets and keys in a package; `ilium plugin pack` and `ilium plugin check` enforce this with an automated scan over the same input set that `pack` would include.

**What the scan refuses**

- A PEM block whose header contains `PRIVATE KEY` (for example `BEGIN PRIVATE KEY`, `BEGIN ENCRYPTED PRIVATE KEY`, `BEGIN OPENSSH PRIVATE KEY`).
- A file named `publisher.key` or matching `*.key`, and a file named `.env` or `.env.*`.
- A text string that matches a common token pattern, such as a registry token, a cloud access key id with its secret, a code-host personal access token, or a bearer token with a long random value. The scan reads text files only and skips files larger than 4 MiB.

**What the user sees**

The command exits 1 with the code `pack.secret-detected`, the path inside the package, the line number for a string match, and the rule that matched. It never prints the secret itself. `pack` writes no `.iliumpkg` when the scan fails, and `check` reports the same finding without packing.

**False positives**

A test fixture or sample may look like a secret. Run `ilium plugin pack --allow-secret-path <path>` (or the same flag on `check`) for that exact path. The flag may be repeated, takes a path relative to the project root and no globs, and each allowed path is listed in the command output and in `build-info.json` so that a reviewer sees it. Do not allow a real key; remove it from the project instead and, if it was ever published or committed, revoke it.

**At upload**

The registry repeats the scan on every upload as part of its automated static analysis (SEC-726), after the package format stages and the other checks of PKG-811. It does not honour `--allow-secret-path` blindly: a path allowed by the publisher is shown to the reviewer, and a PEM private key block is refused regardless. A refused upload fails with `registry.policy` and detail `secret-detected`, leaves no blob and no metadata change, and does not use up the version number.

| Id | Rule |
|---|---|
| [PKG-143] | `pack` includes every file under `assets/`, `help/`, `sounds/`, `screenshots/` and `src/`; the scan covers this set. |
| [AUT-304] | A package MUST NOT contain a secret, a key or a token. |
| [AUT-306] | The secret key MUST NOT be committed, packaged or printed. |
| [SEC-726] | The registry runs automated static analysis on every upload. |

## Publish

```
ILIUM_REGISTRY_TOKEN=... ilium plugin publish dist/com.example.git-tools-1.4.2.iliumpkg --registry https://plugins.example.com
```

The server repeats the checks and, at review level B, a person looks at it. Your version appears in `index.json` after approval.

| Id | Rule |
|---|---|
| [PKG-507] | A publish limit of 20 per hour per publisher applies (PKG-400). |
| [PKG-508] | Start new plugins on the `beta` channel with `--channel beta`, then publish a later version to `stable` once beta users report no problems. |

## After publishing

- To withdraw a bad version, run `ilium plugin yank <id>@<version> --reason "<text>"`. Installed copies keep working.
- To retire a plugin, run `deprecate` with `--successor`.
- Never delete a blob. Takedown is the registry operator's decision (PKG-403).
- If your signing key leaks, revoke it as `compromised`. Every installed copy signed by that key enters `disabled-blocked` and users must accept the risk or take an update signed by another trusted key of your publisher id.
- A registry takedown (PKG-403) has the same effect on the named version. Ship a fixed version signed by a trusted key so that users can leave the blocked state.

| Id | Rule |
|---|---|
| [USR-177] | A publisher guide MUST state that revoking a key as `compromised` or a registry takedown blocks installed copies, that users leave the block with `enable --accept-risk` or with an update signed by another trusted key of the same publisher id, and that a blocked version cannot be installed from the store. |

## Edge cases

| Case | Result |
|---|---|
| Same version published twice | `registry.version-exists` on publish, exit 1. The same version with different content on install is `package.version-reused`. |
| Unsigned package | `publish.unsigned`, exit 1 |
| Namespace not yours | `registry.namespace-unowned` |
| Two builds differ | See Reproducible builds |
| Token expired | `registry.auth`, exit 1 |

## See also

- [Distribution commands](/pages/reference-plugin-distribution-cli/)
- [Package format](/pages/reference-package-format/)
- [Registry reference](/pages/reference-registry/)
- [Publishing and support](/pages/guide-publishing-and-support/)
