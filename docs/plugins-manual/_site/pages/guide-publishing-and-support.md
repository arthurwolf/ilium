# Publishing and support

## Publishing and support

This guide covers the last mile of a plugin: preparing a release, signing it, giving it to users, writing a changelog, answering a bug report, and handling a lost key. It follows the order in which an author meets these tasks. The commands are the ones of the [Plugin command-line reference](/pages/reference-plugin-cli/), and the signing rules are in [Signing and trust](/pages/reference-signing-and-trust/).

Requirement ids of this page are in the range `AUT-300` to `AUT-399`. The complete list is in [Requirements for author guides](/pages/reference-requirements-author-guides/).

### You need

- A plugin that passes its tests. See [Testing plugins](/pages/testing/).
- A release build. A debug build is for development only.
- For a signed (T2) release, an Ed25519 publisher key made with `ilium plugin keygen`.

## What publishing means in version 0.1.0

In `ilium:plugin@0.1.0` the supported way to publish is to hand over files. A release is three things:

| File | What it is |
|---|---|
| `<id>-<version>.iliumpkg` | The package, a deterministic ZIP file |
| `publisher.pub` | The public key of the signer (signed releases only) |
| The SHA-256 of the package | Printed by `ilium plugin pack`, written in your release notes |

Users add the key once with `ilium plugin trust add publisher.pub` and install the package with `ilium plugin install`. A newer version of an installed id is an update.

The manual also specifies an optional registry, a set of signed static files that a client can read (see [Registry reference](/pages/reference-registry/)). It adds search, channels, yank and deprecate. The command `ilium plugin publish <file> --registry <url>` uploads a signed package to a registry (see [Distribution commands](/pages/reference-plugin-distribution-cli/)). The package file stays the base route that needs no registry, and an author MAY add a registry as a second route. [AUT-301]

## Before you release

Run these in the project folder, in this order. Each is offline.

```bash
ilium plugin check
ilium plugin lint --deny warnings
ilium plugin test
ilium plugin build --release
ilium plugin pack
ilium plugin verify dist/com.example.notes-1.4.0.iliumpkg
```

| Step | Catches |
|---|---|
| `check` | Mistakes that would stop an install or trap at run time. It reports the same codes as the installer. |
| `lint` | Weak spots such as an unused grant, a missing help topic or a debug build in a release |
| `test` | Behaviour against your fixtures |
| `build --release` | A release build without debug information |
| `pack` | The deterministic archive with the `files` inventory |
| `verify` | Offline replay of the install stages 1 to 7 on the finished file |

A release MUST pass `check` without errors and SHOULD pass `lint --deny warnings`. [AUT-302]

A release package MUST be built with `--release`, because a package that carries debug information or source maps is larger and fails `lint.debug-info-in-release`. [AUT-303]

The release checklist of [Quality checklist](/pages/guide-quality-checklist/) lists what each of these checks and what they cannot check.

### What goes into the package

| Item | Include | Why |
|---|---|---|
| Components (`*.wasm`) | Yes | The plugin |
| `manifest.json`, `files` inventory | Yes | Written by `pack` |
| Help topics, icons, assets | Yes | Declared in the manifest |
| `README.md`, `LICENSE` | Yes | The package page and the review dialog use them |
| `src/**` | Only with `--include-src` | Reviewers can read it. The host never loads it. |
| `.cwasm` precompiled files | Never | Refused with `package.precompiled-forbidden` |
| Test fixtures, build caches | No | They add size and no value |

A package MUST NOT contain a secret, a key or a token. The package is a file that anyone can open. [AUT-304]

## Names, ids and versions

The `id` is permanent. It is `[a-z0-9-]+(\.[a-z0-9-]+)+`, usually a reverse domain name that you control, such as `com.example.notes`. The prefix `ilium.` belongs to first-party plugins and is refused for others with `manifest.id-reserved`.

A signed package has one more rule: its id MUST start with the publisher id of the signing key and a dot, and an id that does not is refused with `manifest.id-publisher`. [AUT-305]

Pick the id once. A rename is a different plugin, and users lose grants, settings and storage. See [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/).

## Keys

A signed release needs a key pair. Make it once for each publisher.

```bash
ilium plugin keygen --publisher-id com.example --publisher "Example Corp" --out ~/keys/example
ilium plugin sign dist/com.example.notes-1.4.0.iliumpkg --key ~/keys/example/publisher.key
```

`keygen` writes `publisher.key` (the secret, mode 0600) and `publisher.pub` (the public part). It refuses to overwrite either file with `trust.key-exists`. `sign` refuses a key file that other users can read with `sign.key-permissions`, and refuses a manifest whose `publisher` differs from the key with `sign.publisher-mismatch`.

| Rule | Reason |
|---|---|
| Keep `publisher.key` outside the project and outside version control | A key in a repository is a leaked key |
| Keep an offline copy | A lost key cannot be recovered |
| Sign last | A later `pack` writes a new archive without a signature |
| Sign in a clean environment, or give CI the key through `ILIUM_PLUGIN_SIGNING_KEY` (path of a key file) or `ILIUM_PUBLISHER_KEY` (PEM text) | The key never appears in a command line or a log |

The secret key MUST NOT be committed, packaged or printed. [AUT-306]

A release MUST be signed after the last `pack`, and the signed file MUST be the file that users receive. Signing twice with the same key gives the same bytes. [AUT-307]

### Show users the fingerprint

The key fingerprint is the SHA-256 of the raw public key. Users see its short form (the first 8 digits, for example `3f2a 91c0`) in the trust dialog. The author SHOULD publish the full fingerprint on a channel separate from the download, such as the project page, so that a user can compare the two. [AUT-308]

### Lost, leaked or changed keys

| Situation | What happens | What the author does |
|---|---|---|
| Key lost | No new signed release for that publisher id | Make a new key and a new publisher id, publish a new plugin id, and tell users |
| Key leaked | Anyone can sign as you | Stop shipping, tell users to run `ilium plugin trust revoke <key_id> --reason compromised`, publish under a new key |
| A known publisher id arrives with another key | Refused with `package.publisher-key-changed` | Users must remove the old key and add the new one on purpose |
| Registry in use | A key can also be revoked in the registry metadata | Publish the revocation there too |

A leaked key MUST be announced to users on the same channel as the releases, with the fingerprint of the key that replaces it. [AUT-309]

## Release notes and changelog

Keep a `CHANGELOG.md` in the project. Use one block per version, newest first, with the date in `YYYY-MM-DD` form and three kinds of entry.

```text
## 1.4.0 (2026-10-12)

Added
- Export to Markdown (command `export-notes`).

Changed
- The old command id `notes-export` still works and is hidden. It will go in 3.0.0.

Fixed
- A note with an empty title no longer traps the panel.
```

| Include | Why |
|---|---|
| New or changed grants | The update review dialog will show them |
| Raised `data_version` and what the migration does | Users with large data will wait |
| Renamed or removed names | See [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) |
| The SHA-256 of the package | So that users can check the file |
| The contract version (`api`) | So that users know whether their Ilium supports it |

Release notes MUST name every new grant and every raised `data_version`. [AUT-310]

A project SHOULD ship `README.md` and `CHANGELOG.md`, because `lint.readme-missing` and `lint.changelog-missing` report their absence. [AUT-311]

The in-product help of the plugin is a separate matter and is covered in [Plugin documentation guide](/pages/guide-plugin-documentation/).

## Distribution channels

| Channel | Good for | Notes |
|---|---|---|
| A project page with a download link, the fingerprint and the SHA-256 | Public plugins | Keep old versions available so that users can roll back |
| A shared folder or file host | A team | Send `publisher.pub` once, out of band |
| E-mail | One user | Attach the package and send the fingerprint another way |
| A registry, when the host supports it | Search, channels, updates | Adds `yank` and `deprecate` |

Wherever the file goes, the author SHOULD name the file `<id>-<version>.iliumpkg` and SHOULD NOT rename it. [AUT-312]

A user can check a file before installing it: `ilium plugin verify <file>` runs the install stages offline, and `ilium plugin inspect <file> --verify` recomputes the hashes.

## Platforms

T2 and T3 packages run only on Linux in version 0.1.0 and are refused on Windows and macOS with `package.platform-unsupported`. A release page MUST state that signed and unsigned third-party packages need Linux. [AUT-313]

## Supporting your users

A bug report is useful when it lets you see what the plugin did. The user has four tools, and you tell them which to use.

| Ask the user for | Command | What it gives you |
|---|---|---|
| The plugin log | `ilium plugin logs <id> --tail 200` | Traps, restarts, deadline breaches, refusals |
| The state and failures | `ilium plugin status <id>` | Health and the failure window |
| Resource use | `ilium plugin stats <id>` | Use against each limit |
| A recorded trace | `ilium plugin trace <id> --last 100 --record trace.replay` | The events and calls, with payloads redacted |
| An environment check | `ilium plugin doctor` | Host and sandbox facts |
| The version and grants | `ilium plugin info <id>` | What is installed and what was granted |

Payloads in a normal trace are redacted and print as `<redacted N bytes>`. `--unredacted` works only for a plugin installed with `ilium plugin dev`, so an author MUST NOT ask a user for unredacted output from a normal install. [AUT-314]

A plugin that logs MUST NOT write secrets, tokens, file contents or whole payloads to the plugin log. A log line is 16 KiB at most, so write a count or an id. [AUT-315]

### A template for bug reports

Put this in your README, so that users send what you need:

```text
Plugin id and version:        (ilium plugin info <id>)
Ilium version:                (ilium --version)
Operating system:
What you did:
What you expected:
What happened:
Plugin log, last 100 lines:   (ilium plugin logs <id> --tail 100)
Status:                       (ilium plugin status <id>)
```

### Reproducing a report

With a replay file from `ilium plugin trace --record`, you can run `ilium plugin test --replay trace.replay` and see the same events. The fake host runs on a fake clock, so timing problems are reproduced with `advance_clock`. A report that cannot be reproduced SHOULD be turned into a fixture once you understand it, so that it never returns. [AUT-316]

### Triage

| Symptom in the report | First place to look |
|---|---|
| Plugin disabled after three failures | The first trap in the log. Later restarts repeat it. |
| `not-permitted` | The user declined or revoked a grant. Your code must work without it. |
| Install error with a `manifest.` or `package.` code | [Errors and limits reference](/pages/reference-errors-and-limits/), install code index |
| Works on your machine only | Sandbox paths, network allowlists, tier differences |
| Slow or stuck | `ilium plugin stats` and `ilium plugin profile` |

[Troubleshooting](/pages/guide-troubleshooting/) lists these by symptom for authors.

## Security reports

If someone reports a vulnerability, treat it as private until a fix is out. Publish a fixed version with a higher number, state in the release notes that it is a security fix, and tell users to update. If the vulnerability needs a grant to be exploited, say which grant. A plugin that cannot be fixed SHOULD be deprecated with a successor named, when a registry is in use. [AUT-317]

An author MUST publish a way to reach them, in the README and in the package description, before the first release. [AUT-318]

## Withdrawing a release

| Goal | How |
|---|---|
| A bad version must not be installed new | Publish a higher fixed version. With a registry, `yank` the bad one. Installed copies keep working. |
| The whole plugin is obsolete | With a registry, `deprecate` with a message and a successor |
| Users must stop using it at once | Tell them to run `ilium plugin disable <id>` or `uninstall <id>` |

A version number, once published, MUST NOT be reused for different content. The host refuses it with `package.version-reused`. [AUT-319]

## Edge cases and errors

| Situation | Result | Fix |
|---|---|---|
| Manifest `publisher` differs from the key | `sign.publisher-mismatch` at sign, `manifest.publisher-mismatch` at install | Use the same publisher string and publisher id |
| Package signed, then packed again | The signature is gone | Sign last |
| Key file readable by others | `sign.key-permissions` | `chmod 600` |
| Package renamed | Still installs. The id comes from the manifest. | Keep the standard name |
| Package built with `--debug` | `lint.debug-info-in-release` | Rebuild with `--release` |
| User on Windows or macOS | `package.platform-unsupported` | Say so on the release page |
| Same version, new content | `package.version-reused` | Raise the version |

## What the user sees

Before install, the review dialog lists the publisher, the tier, the signature state, the key fingerprint and every grant, one row each. A key that is not trusted is offered with `Trust publisher`. After install the plugin appears in Settings > Plugins with its version. See [Managing plugins](/pages/user-managing-plugins/).

## See also

- [Package and share](/pages/tutorial-09-package-and-share/) for the first release step by step
- [Signing and trust](/pages/reference-signing-and-trust/) for key and signature formats
- [Package format](/pages/reference-package-format/) for the archive
- [Quality checklist](/pages/guide-quality-checklist/) before each release
- [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) for version rules
- [Plugin documentation guide](/pages/guide-plugin-documentation/) for help topics and README

API version checked: `ilium:plugin@0.1.0`.
