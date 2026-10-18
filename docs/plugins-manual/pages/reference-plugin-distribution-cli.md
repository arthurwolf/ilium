---
layout: page
title: Distribution commands
permalink: /pages/reference-plugin-distribution-cli/
---

## Distribution commands

This page lists the `ilium plugin` commands that find, fetch, verify, publish and keep plugins: `install` from every source, `update`, `rollback`, `pin`, `channel`, `search`, `info`, `verify`, `lock`, `registry`, `mirror`, `export-bundle`, `publish`, `yank`, `deprecate` and the trust and signing commands. The build commands (`new`, `build`, `pack`, `check`, `lint`, `test`, `dev`) and the output model are in [Plugin command line](/pages/reference-plugin-cli/). The two pages share one output model and one exit-code table.

Requirement ids such as `PKG-423` can be cited by tests. See [Requirements: manifest and package](/pages/reference-requirements-manifest-package/).

### You will need

- The output model (records `progress`, `error`, `warning`, `plugin`, `summary`, `result`): [Plugin command line](/pages/reference-plugin-cli/#output-model).
- The meaning of each stage and code: [Package format](/pages/reference-package-format/), [Signing and trust](/pages/reference-signing-and-trust/), [Registry reference](/pages/reference-registry/), [Install and update](/pages/reference-install-and-update/).

## Exit codes

The table of [Plugin command line](/pages/reference-plugin-cli/#exit-codes) holds codes 0 to 6 and 130. This page adds two codes. All commands use the one table below.

| Code | Name | Meaning |
|---|---|---|
| 0 | ok | Done. Warnings do not change the code. |
| 1 | failed | A validation, signature, dependency or install step failed. |
| 2 | usage | A flag or argument is wrong. No work started. |
| 3 | findings | `check`, `lint` or `test` found problems. |
| 4 | environment | A tool is missing, or the install lock is held (`install.locked`). |
| 5 | not found | The plugin, version, key or publisher does not exist. |
| 6 | no session | No reachable session. |
| 7 | policy | The administrator policy refused the action. |
| 8 | network | A registry or URL could not be reached, or its metadata is stale. |
| 130 | interrupted | Ctrl+C. |

| Id | Rule |
|---|---|
| [PKG-423] | Every command on this page MUST exit with a code of this table, and the `exit_code` of its `result` record MUST equal the process exit code. |
| [PKG-424] | A policy refusal (`policy.*`, `package.blocked`) MUST exit 7. `registry.unreachable`, `registry.stale`, `registry.metadata-expired` and `install.download-failed` MUST exit 8. A declined prompt (`install.review-declined`, `trust.declined`) and `install.needs-input` are failures and MUST exit 1. Any other `manifest.*`, `package.*`, `trust.*`, `dependency.*` or `install.*` failure MUST exit 1, except `install.locked` (4) and the not-found codes (5). |
| [PKG-425] | A command MUST NOT start work and then exit 2. Usage errors are found before any file or network access. |

## install

`ilium plugin install <source> [--allow-unsigned] [--allow-downgrade] [--trust <key_id>] [--channel stable|beta|dev] [--yes] [--grant <grant>]... [--locked] [--dry-run] [--registry <url>] [--from-bundle <dir>] [--no-backup] [--unskip]`

`--no-backup` skips the pre-migration backup copy of an update (LIF-107). Without the backup a failed `migrate` cannot be undone and the host logs `migration.no-backup-failed`. The size cap of the backup is the key `migration_backup_mib` of the `[plugins]` table.

A source is a path to a `.iliumpkg` file, an `https://` URL, or a registry id with an optional range (`com.example.notes@^1.2`).

| Id | Rule |
|---|---|
| [PKG-426] | `install` MUST classify the source in this order: a URL (it has `://`): `https` is a download; an argument that is a file path (it contains `/` or `\`, starts with `.` or `~`, or ends in `.iliumpkg`), which fails with `usage.file-not-found` when nothing is at that path or the path is a directory; otherwise a registry id. A URL with the scheme `http` fails with `install.insecure-url`; a URL with any other scheme fails with `install.source-scheme`. An id with `--from-bundle` reads the bundle and never the network. |
| [PKG-427] | `--dry-run` MUST run stages 1 to 10 of [Package format](/pages/reference-package-format/#the-validation-order), print the review as records, change no file under the plugin root, and exit with the code the real install would give. |
| [PKG-428] | `--locked` MUST install exactly the versions and package hashes of `ilium-plugins.lock` and fail with `dependency.lock-mismatch` on any difference. It never contacts a registry for resolution. |
| [PKG-429] | `--yes` skips the review dialog as in [Plugin command line](/pages/reference-plugin-cli/#install-uninstall-enable-disable). `--yes` does not confirm a new high-risk grant or a new `replaces` entry; only `--grant <grant>` for that exact grant does. `--yes` MUST NOT trust a new key. Only `--trust <key_id>` does. |
| [PKG-430] | With no attached client and no `--yes`, `install` MUST fail with `install.review-required` and exit 1 (the missing decision is the grant review; `install.needs-input` stays for any other decision nobody can answer). It MUST NOT wait for input that cannot come. |

## update, rollback, pin, channel

| Command | Effect |
|---|---|
| `ilium plugin update <id>` | Resolves the newest version in the channel of the plugin, within pins and ranges, and installs it. |
| `ilium plugin update --all [--dry-run]` | Does this for each unpinned plugin and prints one result per plugin. |
| `ilium plugin rollback <id>` | Makes the one previous version on disk active again. |
| `ilium plugin pin <id>@<version>` and `unpin <id>` | Stops or resumes updates of that plugin. |
| `ilium plugin channel <id> <stable\|beta>` | Sets the channel for later updates. |

| Id | Rule |
|---|---|
| [PKG-431] | `update` of an id that is not installed MUST fail with `install.not-installed` and exit 5. When the newest allowed version equals the installed one, it MUST print a `plugin` record and exit 0. |
| [PKG-432] | `update --all` MUST continue after a failure of one plugin, print every failure, and exit 1 when at least one plugin failed. |
| [PKG-433] | `rollback` with no previous version on disk MUST fail with `install.no-previous` and exit 5. |
| [PKG-434] | `pin` and `channel` MUST change only `installed.json` and MUST be idempotent. `channel dev` is refused with `usage.invalid-value`: the dev channel exists only for `ilium plugin dev`. |

## search, info, verify, lock

| Command | Effect |
|---|---|
| `ilium plugin search [<text>] [--category <name>] [--limit <1..100>] [--offset <n>] [--registry <url>]` | Lists matches. |
| `ilium plugin info <id>[@<version>] [--json]` | Shows one plugin, installed or in a registry. |
| `ilium plugin verify <file or id>` | Runs stages 1 to 7 on a package file or an installed package, and prints the tier. |
| `ilium plugin lock` | Writes `ilium-plugins.lock` for the project from the installed set. |

| Id | Rule |
|---|---|
| [PKG-435] | `search` with no registry configured MUST fail with `registry.none-configured` and exit 1. It MUST NOT use the network when the only configured source is a bundle: it searches the bundle index. |
| [PKG-436] | `verify` MUST NOT change any file and MUST NOT run plugin code. It exits 0 when stages 1 to 7 pass, as in [PKG-156]. |
| [PKG-437] | `info` of an unknown id MUST fail with `plugin.not-found` and exit 5. Registry fields (`yanked`, `deprecated`, `successor`) MUST appear in the `plugin` record when the source is a registry. |

## registry and mirror

| Command | Effect |
|---|---|
| `ilium plugin registry add <url> --root-key <key_id>` | Adds a registry and pins its root key. |
| `ilium plugin registry list` and `registry remove <url>` | Lists or removes configured registries. |
| `ilium plugin mirror sync <dir> --registry <url> [--allow <id>...] [--channel stable\|beta]` | Copies signed metadata and the blobs of the allowed plugins to a folder. |
| `ilium plugin export-bundle <dir> <id>[@range]... [--with-deps]` | Writes an offline bundle. |

| Id | Rule |
|---|---|
| [PKG-438] | `registry add` MUST fail with `registry.root-key-missing` when `--root-key` is absent, with `registry.insecure-url` for a plain `http:` URL (PKG-371 allows `localhost` only when `allow_insecure_localhost = true`), and with `usage.invalid-value` for any other scheme that is not `https` (a `file://` folder is allowed for bundles and mirrors). |
| [PKG-439] | `mirror sync` MUST verify every document and blob before it writes them, MUST write the folder atomically per file, and MUST NOT change any signed byte. |
| [PKG-440] | `export-bundle` MUST fail with `dependency.unsatisfied` listing the missing id when `--with-deps` is absent and a dependency is not in the set. |

## publish, yank, deprecate

| Command | Effect |
|---|---|
| `ilium plugin publish <file> --registry <url> [--channel stable\|beta]` | Uploads a signed package. |
| `ilium plugin yank <id>@<version> --reason <text>` | Marks a version withdrawn. |
| `ilium plugin deprecate <id> --message <text> [--successor <id>]` | Marks a plugin deprecated. |

| Id | Rule |
|---|---|
| [PKG-441] | `publish` MUST run `verify` first and refuse an unsigned package (`publish.unsigned`) or a package whose version already exists in the registry (`registry.version-exists`). It MUST NOT print the upload credential. |
| [PKG-442] | `publish` reads its credential from the environment variable `ILIUM_REGISTRY_TOKEN` or from `--token-file`. It MUST NOT accept the token as a flag value. |
| [PKG-443] | `yank` and `deprecate` MUST NOT delete a blob. They change the signed metadata only (see [Registry reference](/pages/reference-registry/)). |

## keygen, sign and trust

The commands and their rules are in [Signing and trust](/pages/reference-signing-and-trust/). The form of the key files, the trust store and the fingerprint is defined there only. The key forms in [Plugin command line](/pages/reference-plugin-cli/#keygen-sign-and-trust) follow it.

| Id | Rule |
|---|---|
| [PKG-444] | `keygen`, `sign` and `trust` MUST use the file formats, the fingerprint and the codes of [Signing and trust](/pages/reference-signing-and-trust/). A page that shows another format is wrong. |
| [PKG-445] | `trust` failures exit 1, except `trust.not-found` (5). A declined prompt (`trust.declined`) exits 1, as [PKG-424] says. |

## Environment variables

| Variable | Meaning |
|---|---|
| `ILIUM_PUBLISHER_KEY` | PEM text of the publisher key, for CI. |
| `ILIUM_KEY_PASSPHRASE` | Passphrase of an encrypted key. |
| `ILIUM_TRUST_STORE` | Path of the trust store. |
| `ILIUM_REGISTRY_TOKEN` | Upload credential for `publish`. |

| Id | Rule |
|---|---|
| [PKG-446] | A secret variable MUST NOT appear in any record, log or error message. |

## Edge cases

| Case | Result |
|---|---|
| `install` of a file that does not exist | `usage.file-not-found`, exit 2. |
| `install` of a directory | `usage.file-not-found`, exit 2. |
| URL returns 404 | `install.download-failed`, exit 8. |
| Two installs at once | The second waits up to 30 s, then `install.locked`, exit 4. |
| `update --all` with no plugins | Prints `summary` with 0 plugins, exit 0. |

## See also

- [Plugin command line](/pages/reference-plugin-cli/)
- [Install and update](/pages/reference-install-and-update/)
- [Registry reference](/pages/reference-registry/)
- [Signing and trust](/pages/reference-signing-and-trust/)
- [Manifest schema and error catalogue](/pages/reference-manifest-schema/)
