---
layout: page
title: SDK migration
permalink: /pages/guide-sdk-migration/
---

## SDK migration

This guide is for a plugin author whose shipped plugin must move to a new major version of the SDK (`ilium-plugin-sdk` for Rust, `@ilium/plugin-sdk` for TypeScript) or to a new major version of the plugin contract (`ilium:plugin`). A minor or patch release needs no migration: removals follow the notice rules in [SDK deprecation](/pages/reference-sdk-deprecation/) and [ABI evolution](/pages/reference-abi-evolution/), and `ilium plugin upgrade-sdk` handles them without a note. A major release is the only place where removed items, renamed items, changed limits and manifest key changes take effect together ([ABI-058](/pages/reference-abi-major-versions/)).

### Migrating a plugin across an SDK major or a contract major

#### Migration notes

A migration note is one page of the manual per major version, written by the maintainers when the major is released. There is one note per SDK major and one per contract major. The note is part of the manual snapshot of the release that introduces the major, so it is read at the URL `/pages/migration-<kind>-<major>/` of that snapshot, where `<kind>` is `sdk` or `contract` and `<major>` is the new major number (for example `migration-sdk-2`). The `lint.sdk-deprecated` marks and the `Deprecated` and `Removed` changelog headings of [API-084](/pages/reference-sdk-deprecation/) are the inputs the maintainers use to write it; the note does not replace them.

A note MUST contain these parts, each under its own heading, so that an author and the CLI can find them:

| Heading | Content |
| --- | --- |
| `Removed and renamed` | One row per removed or renamed item: the old name, the successor (or the words `no successor`), the minor that marked it and the code change needed. Every item under `Removed` in the changelogs of the major's pre-releases appears here. |
| `Manifest changes` | Every manifest key that is renamed, removed, added as required, or whose accepted values changed, with the old and new form and the error code an unchanged manifest produces. |
| `New limits` | Each limit that is lowered or added, with the old value, the new value and the code of the refusal or trap ([ABI-058](/pages/reference-abi-major-versions/)). |
| `Data version` | Whether stored data of a plugin built for the old major still reads under the new one, whether the plugin must ship a migration step, and the rule for a rollback (see [Data migration](/pages/guide-data-migration/)). |
| `Host window` | The host majors that load a build made with the new SDK, and the date the previous major leaves the window. |

A major host release is announced at least 6 months before its release date, with the list of its removals and limit changes, and that list does not grow afterwards ([DEV-995]). The migration note is the author-facing form of that list, so an item absent from the announcement is absent from the note.

#### What `upgrade-sdk` does with a note

`ilium plugin upgrade-sdk [<dir>] [--to <version>] [--dry-run]` updates the SDK dependency and the pinned WIT version, prints each edit as a `diff` record, and is a no-op with exit 0 when the project is current ([DEV-912]).

- **Within a major:** the command needs no note.
- **Across a major:** the command looks for the note of every major between the current version and `--to`. A jump over two majors needs both notes. If any note is missing from the manual snapshot the CLI knows, the command changes nothing and fails with `upgrade.no-path` (exit 1), naming the missing note. It also fails with `upgrade.no-path` when `--to` is older than the current version ([DEV-912]).
- **With a note:** the command applies the dependency and WIT edits, then prints a `warning` record pointing to the note and listing the items of its `Removed and renamed` table that the source still uses. It does not edit source code and does not edit the manifest.
- **`--dry-run`:** prints the same `diff` records and the same list, and writes nothing.

`ilium plugin lint --fix` applies only rewrites that cannot change behaviour: a rename in the `Removed and renamed` table marked `mechanical` (the old name replaced by the successor with the same signature), and a manifest key rename from the `Manifest changes` table. Every other row is reported with its `lint.sdk-deprecated` or `manifest.*` code and left to the author. `--fix` prints each rewrite as a `diff` record and refuses to run on a working tree with uncommitted changes in the files it would edit.

Decision needed: whether `lint --fix` is a flag of `lint` or a separate subcommand, and whether the `mechanical` marking is part of the note format or a separate machine-readable file shipped with the SDK. Default: a flag of `lint`, with the marking in the note's table plus a JSON file beside it, so the CLI never parses prose. The project owner decides, because the choice fixes the format that every later note must follow.

#### Step list

1. **Branch.** Create a branch of the plugin source. Do not upgrade on the branch that builds the shipped release.
2. **Read the notes.** Read the migration note of each major you cross, and the changelog `Removed` headings of the same versions.
3. **Upgrade.** Run `ilium plugin upgrade-sdk --to <version> --dry-run`, then run it without `--dry-run`.
4. **Fix the source.** Run `ilium plugin lint --fix`, then fix each remaining `lint.sdk-deprecated` finding by hand. At the removal minor the finding is an error ([DEV-997]).
5. **Build.** Run `ilium plugin build`. It fails with `build.sdk-host-mismatch` when the manifest `api` falls outside the host range of the SDK release ([DEV-996]).
6. **Run the compatibility matrix.** Run the matrix for the plugin built for N-1, N and N+1 against a host at N-1, N and N+1. It has one cell per pair, and each cell states its expected result ([DEV-840]; see [Backwards-compatibility test matrix](/pages/guide-backwards-compatibility-test-matrix/)). A cell that differs from its expected result blocks the release.
7. **Bump `api`.** Set the manifest `api` to the new contract version only after step 6 passes. The host refuses a value outside its window with `package.api-unsupported` ([PKG-013](/pages/reference-manifest/)).
8. **Bump the plugin version.** Raise the plugin `version` as the notes' `Data version` part requires.
9. **Publish.** Publish as described in [Publishing](/pages/guide-publishing/). Keep the previous branch for the next step.

#### Keeping a release for the previous host major

While two host majors are both in their window, one release usually cannot serve both. Publish two lines from two branches: the old line, built with the old SDK and `api`, and the new line, built with the new SDK and `api`. Each package declares the `api` it was built for, and the host loads the package whose `api` it supports. The old line takes only fixes; it receives no feature from the new line. When the previous host major leaves the window (the date in the note's `Host window` part), the old line stops loading on updated hosts: publish its last release with a `Changed` changelog entry that says so, then stop publishing it.

#### Failure modes

| Situation | Result |
| --- | --- |
| The note is missing from the CLI's manual snapshot | `upgrade.no-path`, exit 1, nothing changed. Update the CLI, or wait for the snapshot that carries the note. |
| `--to` is older than the current version | `upgrade.no-path`, exit 1. |
| The source uses a removed item after upgrading | A compile error (Rust) or an import error (TypeScript); `lint --fix` fixes only `mechanical` rows. |
| The manifest `api` is outside the host's window | `package.api-unsupported` at install. |
| A matrix cell differs from its expected result | The release is blocked until the cell is fixed or its expectation is corrected with a reason. |

### See also

- [SDK deprecation](/pages/reference-sdk-deprecation/)
- [ABI and major versions](/pages/reference-abi-major-versions/)
- [Backwards-compatibility test matrix](/pages/guide-backwards-compatibility-test-matrix/)
- [Data migration](/pages/guide-data-migration/)
- [Plugin CLI reference](/pages/reference-plugin-cli/)
- [Versioning](/pages/versioning/)

Requirement ids for this page: [DEV-912], [DEV-995], [DEV-996], [DEV-997], [DEV-840], [API-084], [ABI-058], [PKG-013]. See the [requirements index](/pages/reference-requirements/).
