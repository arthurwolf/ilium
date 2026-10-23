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
| [PKG-424] | A policy refusal (`policy.*`, `package.blocked`) MUST exit 7. `registry.unreachable`, `registry.metadata-expired` and `install.download-failed` MUST exit 8. A declined prompt (`install.review-declined`, `trust.declined`) and `install.needs-input` are failures and MUST exit 1. Any other `manifest.*`, `package.*`, `trust.*`, `dependency.*` or `install.*` failure MUST exit 1, except `install.locked` (4) and the not-found codes (5). |
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

## rollout, unyank, unlist, owner and transfer commands

These are the publisher verbs that change what a registry offers. They complete [publish, yank, deprecate](#publish-yank-deprecate) and follow the rules of [Registry reference](/pages/reference-registry/) and [Release rollout](/pages/reference-release-rollout/). Each one needs a registry (`--registry <url>`, or the single configured one) and the publisher credentials in the table below.

| Command | Effect | Signing key | Token |
|---|---|---|---|
| `ilium plugin rollout <id>@<version> --percent <n>` | Sets the share of clients offered that version. `0` halts the rollout, `100` completes it. | None. The change is a new signed `plugins/<id>.json` made by the registry. | `ILIUM_REGISTRY_TOKEN` or `--token-file` |
| `ilium plugin yank <id>@<version> --reason <text> [--confirm-dependents]` | Withdraws a version. | A trusted key of the current publisher (`--key <file>` or `ILIUM_PUBLISHER_KEY`). | Same |
| `ilium plugin unyank <id>@<version>` | Reverses a yank. | Same as `yank`. | Same |
| `ilium plugin unlist <id>` and `relist <id>` | Removes the plugin from `index.json` and search, or puts it back. | Same as `yank`. | Same |
| `ilium plugin owner add <id> <publisher-id>` and `owner remove <id> <publisher-id>` | Adds or removes an owner of the namespace. | A key of an existing owner. | Same |
| `ilium plugin transfer <id> --to <publisher-id> --new-key <key_id>` | Moves the namespace to another publisher id. | A trusted key of the current owner signs the transfer statement. | Same |

### Arguments and flags

- `<n>` of `rollout` is a whole number from 0 to 100. A fraction, a negative number or a value above 100 fails with `usage.invalid-value` before any network access.
- `--reason` of `yank` is 1 to 500 Unicode scalar values and is required (`usage.missing-argument` when absent).
- `--confirm-dependents` of `yank` acknowledges the dependent count printed by the command (see PKG-823).
- `transfer` names the new publisher with `--to` and the new publisher key with `--new-key`. The statement names both and is signed locally before it is sent.
- The credentials come from the variables of [Environment variables](#environment-variables). A token is never a flag value ([PKG-442]).

### Result record

Each command prints one `result` record with `exit_code` and these fields, plus the common fields of [Plugin command line](/pages/reference-plugin-cli/#output-model).

| Command | Fields besides `exit_code` |
|---|---|
| `rollout` | `id`, `version`, `percent`, `previous_percent`, `metadata_version` (the new higher version of `plugins/<id>.json`) |
| `yank` | `id`, `version`, `yanked_at`, `yanked_by` (key fingerprint), `reason`, `dependents` (count) |
| `unyank` | `id`, `version`, `log_entry` (transparency log index) |
| `unlist`, `relist` | `id`, `listed` (`false` or `true`) |
| `owner add`, `owner remove` | `id`, `publisher_id`, `owners` (list after the change), `log_entry` |
| `transfer` | `id`, `from`, `to`, `new_key`, `log_entry` |

### Rules

| Id | Rule |
|---|---|
| [PKG-396] | `unyank` MUST reverse a yank of the named version. `yank` and `unyank` MUST NOT delete a blob ([PKG-443]). |
| [PKG-398] | `unlist` MUST remove the plugin from `index.json` and search and keep `plugins/<id>.json` and the blobs. `relist` MUST restore the plugin to the index. Both MUST be idempotent. |
| [PKG-689] | `rollout` MUST publish the new percentage as a signed `plugins/<id>.json` with a higher version number. A publisher outside the namespace MUST be refused with `registry.namespace-unowned`. A missing or refused token MUST be refused with `registry.auth`. |
| [PKG-800] | `yank` and `unyank` MUST be signed by a trusted key of the current publisher. Any other signer MUST be refused with `registry.not-owner` and the metadata MUST stay unchanged. |
| [PKG-801] | `yank` MUST record `yanked_at`, `yanked_by` and `reason`. Without `--reason` it MUST fail with `usage.missing-argument`. |
| [PKG-802] | `unyank` MUST clear the yanked fields, MUST append a transparency log entry, MUST fail with `registry.not-yanked` when the version is not yanked, and MUST fail with `registry.taken-down` for a version removed by takedown. |
| [PKG-819] | `owner add\|remove` MUST be signed by a key of an existing owner, MUST refuse removing the last owner with `registry.last-owner`, and MUST be logged. |
| [PKG-390] | `transfer` MUST publish a transfer statement signed by a trusted key of the current owner, naming the new publisher id and key. The registry records `transferred_from` in the plugin metadata. |
| [PKG-823] | `yank` MUST print the number of dependents and MUST require `--confirm-dependents` when it is above 0. Without the flag it MUST change nothing and exit 1. |

### Error codes and exit codes

| Code | When | Exit |
|---|---|---|
| `registry.not-owner` | The signing key is not a trusted key of the current owner. | 1 |
| `registry.not-yanked` | `unyank` of a version that is not yanked. | 1 |
| `registry.taken-down` | `unyank` of a version removed by takedown. | 1 |
| `registry.last-owner` | `owner remove` would leave no owner. | 1 |
| `registry.namespace-unowned` | The id is outside the namespace of the token. | 1 |
| `registry.auth` | The token is missing, expired or refused. | 1 |
| `usage.invalid-value` | A percentage outside 0 to 100, or a malformed id, version or publisher id. | 2 |
| `usage.missing-argument` | `--reason`, `--percent` or `--to` is absent. | 2 |
| `plugin.not-found` | The id or version is not in the registry. | 5 |
| `registry.unreachable` | The registry cannot be reached. | 8 |

Other failures follow [PKG-424]. A usage error is found before any key or network access ([PKG-425]).

### Failure modes

- A network failure after the request was sent leaves the outcome unknown. The command exits 8 and the next `info` shows the real state. Repeating `rollout`, `unlist`, `relist` or `unyank` is safe because each sets a state. A repeated `yank` of a yanked version keeps the first `yanked_at`.
- `rollout --percent 0` halts the offer at each client's next refresh. It does not remove a copy that is already active.
- A secret never appears in a record or log ([PKG-446]).

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
