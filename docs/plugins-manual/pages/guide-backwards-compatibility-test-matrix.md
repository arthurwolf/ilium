---
layout: page
title: Backwards-compatibility test matrix
permalink: /pages/guide-backwards-compatibility-test-matrix/
---

## Backwards-compatibility test matrix

The rules for keeping a plugin working across Ilium versions are in [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/). This page turns those rules into a test matrix. A plugin author runs it before a release, and a host CI runs the same cells to check that a host keeps the window it promises. It has two axes: the contract version of the plugin build against the host version, and the stored data version of the plugin against the migration code. Each cell names the expected result, the fixture it needs and the CI job that runs it.

Requirement ids of this page are in the range `DEV-840` to `DEV-845`. They are also listed in the [developer tooling requirements](/pages/reference-requirements-dev-tooling/). The window rule is in [Versioning](/pages/versioning/) and [ABI evolution](/pages/reference-abi-evolution/), and the data rules are in [Data migration](/pages/guide-data-migration/).

### You need

- A plugin with at least one shipped release, or a host build for a second minor version. Today only `ilium:plugin@0.1.0` is released, so the rows for N-1 and N+1 have no builds yet. See [Open decisions](#open-decisions).
- The fixture helpers of [Test harness](/pages/reference-test-harness/): `with_storage_snapshot` and `with_data_version`.
- The pipeline order of [CI recipes](/pages/guide-ci-recipes/).

### Names used in this page

- **N** is the current minor version of the contract that the host ships. **N-1** is the minor before it, and **N+1** is the minor after it.
- The window is the current minor and the previous one. A host at N loads plugins built for N-1 and N. See [Versioning](/pages/versioning/).

## Axis 1: plugin build against host version

A host loads a package when its `api` falls inside the window. The table gives the expected result for each pair. Patch versions do not change the window, so a build for `0.1.0` also loads on a host at `0.1.1` when the three fields of its `api` string agree (see [ABI evolution](/pages/reference-abi-evolution/)).

| Plugin built for | Host N-1 (window N-2 to N-1) | Host N (window N-1 to N) | Host N+1 (window N to N+1) |
|---|---|---|---|
| N-1 | Loads | Loads through the adapter of N-1 | Refused with `package.api-unsupported` |
| N | Refused with `package.api-unsupported` | Loads | Loads. Events newer than its API are not delivered |
| N+1 | Refused with `package.api-unsupported` | Refused with `package.api-unsupported` | Loads |

Notes on the cells:

- A host of minor N+1 does not deliver an event, outcome or reason case that is newer than the plugin's API. The plugin never sees it, and the case is not counted in `events-dropped`. A newer outcome is mapped to the nearest older one, and a newer `deactivation-reason` arrives as `shutdown`. See section 5 of [ABI evolution](/pages/reference-abi-evolution/).
- A plugin built for N+1 that runs on a host at N is refused with `package.api-unsupported`, and it does not load in part.
- Today the only released minor is 0.1.0, which has no previous version. Its row for N-1 and its column for N+1 are empty until a second minor exists.

## Axis 2: data version and migration

Each plugin that stores data has a `data_version` in its manifest. The cells below run on a storage snapshot of an older data version, and they check what the host does on upgrade, failure and rollback.

| Case | Input | Expected result | Source |
|---|---|---|---|
| Upgrade with a `migrator` | Snapshot at the previous `data_version`, build with a higher one | `migrate(from, to)` runs once per bucket, then the new version activates | [Data migration](/pages/guide-data-migration/) |
| Second run of `migrate` | The same snapshot, `migrate` called twice | The second call changes nothing | [LIF-106] in Data migration |
| `migrate` fails | An error, trap or deadline miss | The backup is restored and the old version stays | [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) |
| `data_version` rises with no `migrator` | Snapshot at the previous version | The host stamps the new number, drops the restore bytes, clears the `shared` bucket and logs `migration.skipped` | [Data migration](/pages/guide-data-migration/) |
| Restore bytes from another `data_version` | Build changes `data_version` between two runs | The bytes are dropped and `restore.dropped-data-version` is logged | [LIF-103] in Data migration |
| Rollback after a migration | Install a lower version whose supported data version is below the stored one | Refused with `migration.data-newer` | [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) |

### Fixtures and snapshots per data version

A snapshot is a JSON file of the form `{bucket: {key: base64}}`, written as pretty JSON with sorted keys and a trailing newline ([DEV-504] in [Test harness](/pages/reference-test-harness/)). The data-migration page uses one example, `tests/fixtures/notes-v1.json`, loaded with `with_storage_snapshot` and `with_data_version(1)`.

The compatibility page requires that data stay readable by the previous release for one release ([AUT-208]). A matrix therefore needs at least the snapshot of the previous `data_version`. The naming scheme beyond that example is an open decision.

## CI job per cell

Each cell runs in a CI job that follows the five-step pipeline of [CI recipes](/pages/guide-ci-recipes/). The matrix over host versions and data-version snapshots uses the same matrix mechanism as the side matrix in that page. A failing cell fails the pipeline, and the job names the cell.

| Cell kind | Job steps | What the job asserts |
|---|---|---|
| Load cell (`loads`) | Check, test with `--json`, then the fixtures of the cell | The package is active and not `refused-at-load` |
| Refusal cell (`package.api-unsupported`) | The same steps as a load cell | The refusal code is `package.api-unsupported`, and the package never runs |
| Migration cell | Test with `--json` on the snapshot of the cell | The result of Axis 2, read from `migration.skipped`, `restore.dropped-data-version` or `migration.data-newer` where it applies |

## Requirements

- **[DEV-840]** A plugin's compatibility matrix MUST hold one cell for each pair of a plugin built for N-1, N or N+1 and a host at N-1, N or N+1, and each cell MUST state the expected result given in the table of Axis 1.
- **[DEV-841]** A cell that expects a load MUST assert that the package is active and not `refused-at-load`. A cell that expects a refusal MUST assert the code `package.api-unsupported` and MUST assert that the plugin never ran.
- **[DEV-842]** A cell in which a host of N+1 delivers to a plugin of N MUST assert that the newer event, outcome or reason case never reaches the plugin. It MUST NOT assert on `events-dropped` for that case, because the host does not count it there.
- **[DEV-843]** For each release that stores data, the matrix MUST keep the storage snapshot of the previous `data_version`, and each migration cell MUST load it with `with_storage_snapshot` and `with_data_version`.
- **[DEV-844]** Each migration cell MUST call `migrate` twice on the same snapshot and compare the two results, and each rollback cell MUST expect `migration.data-newer` and fail when the rollback succeeds.
- **[DEV-845]** Each cell MUST run as a CI job in the order of the five pipeline steps, with `--json` on every command, and a job that exits with a non-zero code MUST fail the pipeline.

## Open decisions

1. **How a cell selects its host version.** The [plugin command-line reference](/pages/reference-plugin-cli/) lists no flag that names a host version for `ilium plugin test`. Choice A: install the older host build in each job, which needs the binary to be kept and found. Choice B: add a flag that selects a frozen contract snapshot, which is new CLI work. The consequence of A is that the matrix runs the real host; the consequence of B is that the matrix runs an adapter that is not the shipped binary.
2. **Where a refusal is observed.** [ABI evolution](/pages/reference-abi-evolution/) says the refusal happens at install, at stage 3. The pages do not state whether `ilium plugin test` or `ilium plugin check` reports the same code. The choice decides which step of the pipeline a refusal cell asserts in.
3. **Host builds for N-1 and N+1.** Only 0.1.0 is released, so the N-1 and N+1 rows have no build to run. The choice is whether the CI keeps a binary for each released minor or rebuilds old minors from source. Keeping binaries costs storage; rebuilding costs time and depends on the old toolchain.
4. **Fixture naming and how many snapshots to keep.** The data-migration page shows `notes-v1.json` only. Choice: keep one snapshot per shipped `data_version`, or only the previous one that [AUT-208] requires. Keeping all of them costs repository size and tests more upgrade paths; keeping only the previous one matches the rule but leaves older upgrades untested.
5. **How to assert that a newer case is not delivered.** [DEV-842] needs a visible signal. The pages do not name a record or log entry for a silent drop. Choice: the test plugin writes a marker for each event it receives. The consequence is that the marker is extra code in every fixture plugin.

## Edge cases

- **A plugin that never stores data.** Axis 2 does not apply. Run only the cells of Axis 1.
- **A patch-only release.** The window counts minors only. Run one load cell at the patch version, with the `api` string equal in its three fields, so that a mismatch shows as `manifest.api-version`.
- **An unstable feature.** A cell that lists an unstable feature which the newer host removed expects `manifest.unstable-unknown`. See [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/).

## See also

- [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) for the author rules that these cells test
- [Versioning](/pages/versioning/) for the window
- [ABI evolution](/pages/reference-abi-evolution/) for the adapters and the dropped cases
- [Data migration](/pages/guide-data-migration/) for the `migrator` and the backup
- [CI recipes](/pages/guide-ci-recipes/) for the pipeline
- [Conformance suite](/pages/reference-conformance-suite/) for the contract checks that run alongside the matrix

API version checked: `ilium:plugin@0.1.0`.

## Production additions: T9-compat-deprecation

- **[DEV-987]** `ilium plugin check --against <host-version>` (proposed, owner to confirm) MUST load the built package into a headless host of that version and report load result, refusal code and `deprecated` records as JSONL, for every minor inside the window with `--against window`.
