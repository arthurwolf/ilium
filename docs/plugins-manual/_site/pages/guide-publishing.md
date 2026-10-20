# Publishing guide

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
| [PKG-490] | The first segment pair of the plugin id MUST match the publisher id in the signing key (PKG-218). Otherwise signing fails with `manifest.id-publisher`. |
| [PKG-491] | Use SemVer. Raise the major number when you remove a command, a service function or a setting. See [Versions and dependencies](/pages/reference-dependency-resolution/#plugin-versions). |
| [PKG-492] | A version is immutable once published. A fix needs a new version (PKG-394). |

## Create a signing key

Run `ilium plugin keygen --publisher-id com.example --publisher "Example Labs" --out ~/keys/example`. Keep the private file offline and back it up. If you lose it, users must trust a new key. If someone steals it, follow the runbook in [Signing and trust](/pages/reference-signing-and-trust/#a-compromised-key-the-publisher-runbook).

| Id | Rule |
|---|---|
| [PKG-493] | The private key MUST NOT be stored in the plugin repository. For CI, put it in a secret store and pass it through `ILIUM_PUBLISHER_KEY`. |
| [PKG-494] | Encrypt a key kept on disk. Pass the passphrase through `ILIUM_KEY_PASSPHRASE`, never on the command line. |

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

## Edge cases

| Case | Result |
|---|---|
| Same version published twice | `registry.version-exists`, exit 1 |
| Unsigned package | `publish.unsigned`, exit 1 |
| Namespace not yours | `registry.namespace-unowned` |
| Two builds differ | See Reproducible builds |
| Token expired | `registry.auth`, exit 1 |

## See also

- [Distribution commands](/pages/reference-plugin-distribution-cli/)
- [Package format](/pages/reference-package-format/)
- [Registry reference](/pages/reference-registry/)
- [Publishing and support](/pages/guide-publishing-and-support/)
