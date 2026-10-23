---
layout: page
title: CI recipes
permalink: /pages/guide-ci-recipes/
---

## CI recipes

This guide shows how to build, test, package and sign a plugin in a continuous integration (CI) pipeline. CI means a service that runs your commands on a clean machine after each push. The recipes use GitHub Actions and GitLab CI. The same commands work in any other system.

Terms:

- **Pipeline**: the list of CI steps.
- **Runner**: the machine that runs the steps.
- **Reproducible build**: two builds from the same source give the same package bytes.
- **Secret**: a value kept by the CI service and not stored in the repository.

## You will need

- A plugin project from [Project templates](/pages/reference-project-templates/), in a Git repository.
- The `ilium` binary on the runner. It is a single file. Download it in the first step and check its hash.
- For signing: a signing key made with `ilium plugin keygen`. See [Plugin command-line reference](/pages/reference-plugin-cli/).

## The pipeline in five steps

| Step | Command | Exit 0 means |
|---|---|---|
| 1. Check | `ilium plugin check --json` | The manifest and the component are valid |
| 2. Lint | `ilium plugin lint --deny warnings --json` | No warnings or errors |
| 3. Test | `ilium plugin test --json` | All fixtures pass |
| 4. Build | `ilium plugin build --release --json` | A release build in `dist/` |
| 5. Pack | `ilium plugin pack --json` | A package in `dist/` |

- **[DEV-760]** A CI pipeline SHOULD run the five steps in this order and stop at the first step that fails. The order puts the cheap steps first.
- **[DEV-761]** Every command MUST work with no terminal and no session. The commands in this table are all offline commands, so a runner needs no running Ilium.
- **[DEV-762]** With `--json` every command MUST print JSONL on standard output only. Progress text goes to standard error.
- **[DEV-763]** `NO_COLOR=1` MUST be set in CI. Commands MUST also disable colour when standard output is not a terminal.
- **[DEV-764]** The exit codes MUST be those of [Plugin command-line reference](/pages/reference-plugin-cli/). A CI step fails on any non-zero code. Code 3 (findings) from `lint` fails the pipeline when `--deny warnings` is set.

## GitHub Actions

`ilium plugin new --ci github` writes this file as `.github/workflows/plugin.yml`.

```yaml
name: plugin
on:
  push:
  pull_request:
env:
  NO_COLOR: "1"
  CARGO_TERM_COLOR: never
jobs:
  build:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - name: Install ilium
        run: |
          curl -fsSL -o ilium.tar.gz "$ILIUM_URL"
          echo "$ILIUM_SHA256  ilium.tar.gz" | sha256sum --check
          tar -xzf ilium.tar.gz -C "$RUNNER_TEMP"
          echo "$RUNNER_TEMP" >> "$GITHUB_PATH"
        env:
          ILIUM_URL: ${{ vars.ILIUM_URL }}
          ILIUM_SHA256: ${{ vars.ILIUM_SHA256 }}
      - name: Doctor
        run: ilium plugin doctor
      - name: Check
        run: ilium plugin check
      - name: Lint
        run: ilium plugin lint --deny warnings
      - name: Test
        run: ilium plugin test --coverage --min-coverage 70
      - name: Build
        run: ilium plugin build --release
      - name: Pack
        run: ilium plugin pack
      - uses: actions/upload-artifact@v4
        with:
          name: package
          path: dist/*.iliumpkg
```

- **[DEV-765]** The generated file MUST pin each third-party action to a major version tag and MUST read the `ilium` download URL and hash from repository variables, so no URL is hard coded.
- **[DEV-766]** The `Install ilium` step MUST verify the SHA-256 hash of the download before it runs the file.

## GitLab CI

`ilium plugin new --ci gitlab` writes this file as `.gitlab-ci.yml`.

```yaml
stages: [verify, package]
variables:
  NO_COLOR: "1"
default:
  before_script:
    - curl -fsSL -o ilium.tar.gz "$ILIUM_URL"
    - echo "$ILIUM_SHA256  ilium.tar.gz" | sha256sum --check
    - tar -xzf ilium.tar.gz -C /usr/local/bin
verify:
  stage: verify
  image: rust:1.90
  script:
    - ilium plugin doctor
    - ilium plugin check
    - ilium plugin lint --deny warnings
    - ilium plugin test
package:
  stage: package
  image: rust:1.90
  script:
    - ilium plugin build --release
    - ilium plugin pack
  artifacts:
    paths: [dist/*.iliumpkg]
    expire_in: 30 days
```

A TypeScript project uses an image that has Bun 1.2 or later in place of `rust:1.90`.

## Reproducible builds

A package is reproducible when two builds from the same commit give the same bytes. Reproducibility lets a reviewer rebuild the package and compare hashes.

- **[DEV-770]** A release build MUST be reproducible on the same toolchain version and the same `ilium` version. `dist/build.json` records `toolchain` and `input_hash` so a reviewer can pick the same versions.
- **[DEV-771]** A build MUST NOT read the wall clock, the user name, the absolute path of the project or environment variables other than those listed in [Plugin command-line reference](/pages/reference-plugin-cli/). Package time stamps are fixed.
- **[DEV-772]** The lock file (`Cargo.lock` or `bun.lock`) MUST be committed, and CI MUST build with `--locked` (Rust) or `--frozen-lockfile` (Bun). `build` SHOULD pass the flag by itself when the lock file exists, and MUST fail with `toolchain.lockfile-missing` when it does not exist and `--release` is set without `--locked` (exit 4). When `--locked` is also given, the missing lock file is reported as `build.lockfile-missing` (exit 1, see DEV-053), so one missing file never yields two codes.
- **[DEV-773]** A reviewer check is `ilium plugin build --release && ilium plugin pack` followed by `sha256sum dist/*.iliumpkg`. Two runs MUST give the same hash.

## Signing in CI

A signed package proves who published it. Sign only release builds from a protected branch or tag.

```yaml
      - name: Sign
        if: startsWith(github.ref, 'refs/tags/v')
        run: |
          printf '%s' "$ILIUM_PLUGIN_SIGNING_KEY" > "$RUNNER_TEMP/key"
          chmod 600 "$RUNNER_TEMP/key"
          ilium plugin sign dist/*.iliumpkg --key "$RUNNER_TEMP/key"
          rm -f "$RUNNER_TEMP/key"
        env:
          ILIUM_PLUGIN_SIGNING_KEY: ${{ secrets.ILIUM_PLUGIN_SIGNING_KEY }}
```

- **[DEV-775]** The secret key MUST be stored as a CI secret and MUST NOT be written to the repository, the build log or an artifact. `ilium plugin sign` MUST read the key from `--key <file>` or from the environment variable `ILIUM_PLUGIN_SIGNING_KEY` and MUST NOT print it.
- **[DEV-776]** `pack` MUST NOT sign. Signing is a separate step so a pull request build never needs the key.
- **[DEV-777]** The pipeline SHOULD verify the signed package with `ilium plugin inspect dist/*.iliumpkg --verify` and compare the printed fingerprint with the fingerprint published for the plugin.
- **[DEV-778]** A forked pull request MUST NOT receive the secret. GitHub and GitLab do this by default for forks. Do not enable secrets for forks.

## Caching

Cache the Cargo registry (`~/.cargo/registry`) or the Bun cache (`~/.bun/install/cache`). Do not cache `dist/` or `.ilium/cache/` across commits that change the toolchain, because the build cache key includes the input hash and a stale cache only costs time but never gives a wrong result.

## A release workflow

Build and sign only on a version tag. The workflow below runs the five steps, signs, verifies and attaches the package to a release.

```yaml
name: release
on:
  push:
    tags: ["v*"]
jobs:
  release:
    runs-on: ubuntu-24.04
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v4
      - name: Install ilium
        run: ./ci/install-ilium.sh
      - run: ilium plugin check
      - run: ilium plugin lint --deny warnings
      - run: cargo test --locked
      - run: ilium plugin test
      - run: ilium plugin build --release
      - run: ilium plugin pack
      - name: Sign
        run: ./ci/sign.sh
        env:
          ILIUM_PLUGIN_SIGNING_KEY: ${{ secrets.ILIUM_PLUGIN_SIGNING_KEY }}
      - run: ilium plugin inspect dist/*.iliumpkg --verify
      - name: Hash
        run: sha256sum dist/*.iliumpkg | tee dist/SHA256SUMS
      - uses: softprops/action-gh-release@v2
        with:
          files: |
            dist/*.iliumpkg
            dist/SHA256SUMS
```

- **[DEV-740]** A release pipeline SHOULD run the conformance suite in component mode before it packs: `ilium plugin test --conformance`.
- **[DEV-741]** The tag name SHOULD equal `v` plus the manifest `version`. A pipeline step MUST fail when they differ. A shell test is `test "v$(ilium plugin inspect --json . | jq -r .version)" = "$GITHUB_REF_NAME"`.
- **[DEV-742]** The release MUST publish the SHA-256 of the package next to the package, so a user can compare it.

## Reading a failure

Each step fails with an exit code and, with `--json`, an `error` record. The table shows what to look for.

| Step | Exit | Record to read | Typical cause |
|---|---|---|---|
| check | 3 | `check` record with a `rule` such as `manifest.*` or `component.*` | A manifest key is wrong |
| lint | 3 | `lint` with `severity` `warn` | A warning under `--deny warnings` |
| test | 3 | `test-case` with `status` `fail` or `trap` | An expectation file differs or the component trapped |
| build | 4 or 1 | `error` with code `toolchain.*` | Toolchain missing or a compile error |
| pack | 1 | `error` with `pack.stale-build` | The build is older than the sources |
| sign | 1 | `error` with `sign.*` | Wrong key or publisher |

```json
{"type":"error","code":"pack.stale-build","message":"dist/server.wasm is older than src/lib.rs","fix":"run ilium plugin build --release"}
{"type":"summary","duration_ms":38,"warnings":0,"errors":1}
{"type":"result","ok":false,"exit_code":1}
```

- **[DEV-743]** A CI log MUST be enough to find the cause without a terminal. Every `error` record MUST carry `code`, `message` and, when known, `fix`.

## Matrix builds

A plugin that supports two languages or two sides builds each in its own job so a failure names the part. The `build` command takes `--side`. A matrix over `side: [server, client]` runs `ilium plugin build --release --side ${{ matrix.side }}`, and a final job runs `pack` on the merged `dist/` folder. `pack` MUST refuse a `dist/` folder that misses a declared side with `pack.not-built`.

- **[DEV-744]** `pack` MUST fail with `pack.not-built` when a side that the manifest declares has no component in `dist/`.

## Edge cases

- **Offline runners.** Set `ILIUM_PLUGIN_OFFLINE=1`. `build` then fails with `toolchain.network-needed` instead of downloading.
- **Coverage tool missing.** `--coverage` exits 4. Install `cargo-llvm-cov` in the runner image.
- **Golden updates.** `ILIUM_PLUGIN_UPDATE_GOLDEN` is refused in CI (`harness.golden-update-in-ci`). Update goldens on a developer machine and commit the files.
- **Debug build in pack.** `pack` refuses a debug build. The Build step above uses `--release`.

## See also

- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [Test harness reference](/pages/reference-test-harness/)
- [Conformance suite](/pages/reference-conformance-suite/)
- [Packages](/pages/packages/)
- [Plugin documentation guide](/pages/guide-plugin-documentation/)

API checked against `ilium:plugin@0.1.0`.


### Dependency vulnerability audit

- [DEV-896] `ilium plugin check` and `ilium plugin publish` MUST scan the locked dependency set against an advisory database, MUST report each match as a `lint.dependency-vulnerable` diagnostic with the advisory id and fixed version, MUST fail `publish` at or above a configurable severity unless `--allow-advisory <id>` is given, and MUST run against a cached database without network access when `--offline` is set.

## Production additions: T5-devtools

**[DEV-932]** The CI recipes MUST name the exact cache paths and cache key inputs per language: Rust `~/.cargo/registry` (proposed, owner to confirm), `~/.cargo/git`, `target/` keyed on `Cargo.lock` hash and toolchain version; TypeScript `~/.bun/install/cache` keyed on `bun.lock` hash. A recipe MUST NOT cache `dist/`, and a pack from a cold cache MUST be byte-identical to one from a warm cache.

**[DEV-933]** A CI recipe MUST show running `test` and `conformance` against the oldest and the newest host contract version the manifest `requires` range admits, using `ilium plugin test --host-contract <version>` (proposed, owner to confirm). An unsupported version MUST fail with `usage.host-contract-unavailable` and exit 2.

**[DEV-934]** The CI recipe MUST upload on every run, success or failure, these files: the JSONL log of each step, `dist/build.json`, `dist/coverage.json` when produced, the conformance report, and on release the package, its SHA-256 file and `publisher.pub`. It MUST NOT upload `publisher.key`, `dist/*.tmp` or the signing environment.

## Several plugins in one repository

The recipes above assume one plugin at the repository root. A team that ships a family of plugins keeps them in one repository and runs one pipeline per plugin. Commands that a flag below marks as "proposed" are not yet fixed by a requirement; the flag name may change before release.

### Layout

```
repo/
  Cargo.toml              # workspace root, one Cargo.lock
  Cargo.lock
  bun.lock                # one lock file for all TypeScript plugins
  package.json            # npm workspaces: ["plugins/*", "shared/*"]
  shared/
    plugin-common/        # shared Rust crate or npm package, not a plugin
  plugins/
    git-tools/            # one folder per plugin
      manifest.json
      dist/
    log-viewer/
      manifest.json
      dist/
```

- Each plugin folder holds its own `manifest.json` and its own `dist/`. Two plugins MUST NOT share a `dist/` folder.
- A shared crate or package holds code only. It has no `manifest.json`, so `ilium plugin` commands never treat it as a plugin.
- There is one lock file at the root for each ecosystem: `Cargo.lock` for the Cargo workspace and `bun.lock` for the npm workspace. CI builds with `--locked` (Rust) or `--frozen-lockfile` (Bun) as in [DEV-772]. A change to the lock file changes the inputs of every plugin that uses an affected dependency, so the change detection below treats the lock file as an input of all plugins.

### Per-plugin commands

Every project command accepts `--manifest-path <file>` ([DEV-902]). The project directory is then the parent of that file. Do not also give `<dir>`: the command exits 2 with `usage.conflicting-flags`. Paths in the output records are relative to the plugin folder, not to the runner's working directory, so a log from `plugins/git-tools` and one from `plugins/log-viewer` can be read side by side.

```sh
P=plugins/git-tools/manifest.json
ilium plugin check     --manifest-path "$P"
ilium plugin lint      --manifest-path "$P" --deny warnings
ilium plugin test      --manifest-path "$P"
ilium plugin build     --manifest-path "$P" --release
ilium plugin pack      --manifest-path "$P"
ilium plugin sign      --manifest-path "$P" plugins/git-tools/dist/*.iliumpkg --key "$KEY_FILE"
```

`pack` writes its package into the `dist/` folder of that plugin. `sign` takes the package path, so the key file and the package of one plugin are never mixed with those of another. Sign each plugin with the key that its manifest `publisher` names ([DEV-776] still holds: `pack` never signs).

### The `dist/` lock

Two commands that write the same `dist/` take the advisory lock `dist/.lock` ([DEV-901]). The second waits up to 30,000 ms and then fails with `build.locked` (exit 1), naming the PID that holds the lock. A lock whose PID is dead is broken automatically.

- The lock is per project, so jobs for different plugins run in parallel without waiting for each other.
- Two jobs for the same plugin on the same runner, for example a `build` and a `pack` started by two matrix entries, do wait. Give each plugin one job that runs the five steps in order, or give each its own checkout. Do not raise the wait to hide a double start.
- A matrix over runners (each job has its own checkout) never meets the lock.

### Change detection

Build only the plugins that a commit touches. A plugin is touched when a changed path is under its folder, under a shared folder that it depends on, or is a root lock file or a root build file.

```yaml
jobs:
  changes:
    runs-on: ubuntu-24.04
    outputs:
      plugins: ${{ steps.detect.outputs.plugins }}
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - id: detect
        run: ./ci/changed-plugins.sh "${{ github.event.pull_request.base.sha || github.event.before }}" HEAD
```

`ci/changed-plugins.sh` runs `git diff --name-only <base> HEAD`, maps paths to plugin ids and writes a JSON array such as `["com.example.git-tools"]` to `$GITHUB_OUTPUT`. It MUST print every plugin when the base commit is unknown (first push of a branch, forced push) so that a missing base never skips a plugin. A tag build always builds the one plugin named by the tag and ignores change detection.

### Matrix keyed on plugin id

```yaml
  build:
    needs: changes
    if: needs.changes.outputs.plugins != '[]'
    strategy:
      fail-fast: false
      matrix:
        plugin: ${{ fromJSON(needs.changes.outputs.plugins) }}
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - run: ./ci/install-ilium.sh
      - run: ./ci/verify-plugin.sh "${{ matrix.plugin }}"
      - uses: actions/upload-artifact@v4
        with:
          name: package-${{ matrix.plugin }}
          path: plugins/${{ matrix.plugin }}/dist/*.iliumpkg
```

- The job name contains the plugin id, so a red job names the plugin that failed. `fail-fast: false` lets the other plugins finish, because one failing plugin must not hide the result of the others.
- `ci/verify-plugin.sh` resolves the folder from the id and runs the five steps of the pipeline with `--manifest-path`.
- The artifact name includes the id. Two matrix entries that upload the same artifact name fail the upload.
- A protected-branch rule requires the matrix jobs by their fixed prefix. When change detection skips a plugin, add a final job `all-plugins` with `if: always()` that fails if any matrix job failed and passes if all were skipped or green, and require only that job.

### Independent versions and tags

Each plugin has its own `version` in its manifest and its own release. The tag is `<id>-v<version>`, for example `com.example.git-tools-v1.4.2`. A tag that matches `*-v*` starts the release workflow for the one plugin that the tag names.

[DEV-741] adapts as follows. The step splits the tag at the last `-v`, takes the id and the version, finds the folder whose `manifest.json` has that `id`, and fails when they differ:

```sh
tag="$GITHUB_REF_NAME"
id="${tag%-v*}"
version="${tag##*-v}"
dir=$(./ci/plugin-dir.sh "$id")        # fails when no plugin has this id
actual=$(ilium plugin inspect --json --manifest-path "$dir/manifest.json" | jq -r '.version')
test "$actual" = "$version"
```

- The workflow fails with a clear message when no folder has that id, when two folders have it (a duplicate id is a repository error) or when the versions differ.
- A tag for plugin A never builds, signs or publishes plugin B.
- A shared-crate change that affects several plugins needs one version bump and one tag per affected plugin. The change-detection job lists them; the release owner tags each.

### Pack and sign per plugin

`pack` and `sign` run once for each plugin, inside that plugin's job. The signing key is a secret of the environment that the release job is bound to ([DEV-775], [DEV-778]). Use one environment per publisher. A plugin family under one publisher can share one key and one environment; a plugin under another publisher uses its own key and environment, so a job for plugin A cannot read the key of publisher B. The release attaches `<id>-<version>.iliumpkg` and its `SHA256SUMS` line for that plugin only ([DEV-742]).

### Failure modes

| Symptom | Code | Cause and fix |
|---|---|---|
| Second command waits then stops | `build.locked` (exit 1) | Two commands wrote the same `dist/`. Give the plugin one job |
| Command exits 2 | `usage.conflicting-flags` | Both `<dir>` and `--manifest-path` were given |
| Release builds the wrong plugin | none (script bug) | The tag split used the first `-v` instead of the last; ids may contain `-v` |
| Plugin skipped after a lock file change | none (script bug) | The detection script ignored root lock files |

## Publishing to a registry from CI

`ilium plugin publish` uploads a signed package to a registry ([DEV-135], [PKG-392]). This section adds it to the release workflow after the verify step. Registry rules are in [Registry reference](/pages/reference-registry/) and the command is in [Distribution commands](/pages/reference-plugin-distribution-cli/).

### The step

```yaml
  publish:
    needs: release
    runs-on: ubuntu-24.04
    environment: registry-production
    if: github.event_name == 'push' && startsWith(github.ref, 'refs/tags/')
    steps:
      - uses: actions/download-artifact@v4
        with:
          name: package
          path: dist
      - run: ilium plugin inspect dist/*.iliumpkg --verify
      - name: Publish
        run: |
          printf '%s' "$ILIUM_REGISTRY_TOKEN" > "$RUNNER_TEMP/registry-token"
          chmod 600 "$RUNNER_TEMP/registry-token"
          ilium plugin publish dist/*.iliumpkg --registry "$REGISTRY_URL" --token-file "$RUNNER_TEMP/registry-token"
          rm -f "$RUNNER_TEMP/registry-token"
        env:
          ILIUM_REGISTRY_TOKEN: ${{ secrets.ILIUM_REGISTRY_TOKEN }}
          REGISTRY_URL: ${{ vars.REGISTRY_URL }}
```

- `publish` runs `verify` first and refuses an unsigned package with `publish.unsigned` ([PKG-441]). It never prints the credential.
- The credential comes from `ILIUM_REGISTRY_TOKEN` or from `--token-file`. It is never a flag value ([PKG-442]). A command line such as `--token abc` is a usage error, and a token on a command line would also reach the process list and the log.
- If both the variable and `--token-file` are set, the file wins. Delete the file at the end of the step; a protected runner is destroyed anyway.

### Protected environment

- The job is bound to a protected environment (`registry-production`). The environment holds the secret, allows only release tags or the protected branch, and can require a reviewer. A pull-request job, a branch job and any job outside the environment cannot read the token.
- A forked pull request MUST NOT receive the token ([DEV-778]). Do not use `pull_request_target` to run code from a fork with secrets, and do not enable secrets for forks.
- Publishing from a tag build only: a branch push or a pull request never calls `publish`.

### Token scope and rotation

- Use a token limited to one publisher id and to the upload permission. A token that can also change rollout or yank versions belongs to a person, not to CI.
- Rotate the token on a calendar date (every 90 days is a good default) and after any change of the people who can edit workflows or approve the environment. Replace the secret in the environment, run a `--dry-run` publish (below) and revoke the old token at the registry. A token found in a log is revoked at once, and the package versions it published since the leak are checked against the hashes in `SHA256SUMS`.
- A wrong or expired token gives an authentication error from the registry and a non-zero exit. The step fails; it does not retry with another credential.

### Reruns

A version is immutable ([PKG-394]). A publish of an existing `id@version` is refused with `registry.version-exists`, even when the content is identical, so a rerun of a job that already published fails at the registry. Treat the result in the workflow like this:

1. Read the `error` record from `publish --json`. If the code is not `registry.version-exists`, fail the step.
2. Fetch the published package hash from the registry index and compare it with `sha256sum` of `dist/*.iliumpkg`.
3. If the hashes are equal, the earlier run already published this exact package. Print a `notice` and exit 0.
4. If the hashes differ, fail. Somebody built a different package for the same version; the fix is a new version, never a replacement ([AUT-319]).

After a takedown, the version stays blocked ([PKG-821]): the rerun check in step 3 does not apply to a tombstoned version, so the workflow fails and the author publishes a new version.

### Dry run on pull requests

A pull-request job runs `ilium plugin publish dist/*.iliumpkg --registry "$REGISTRY_URL" --dry-run` (proposed flag, owner to confirm). The dry run checks the package, the version against the registry index and the publisher id, and sends nothing. It needs no token, so it can run for forks. If the registry needs a token even to read its index, the dry run checks the local rules only and states so in its summary record.

### Failure modes

| Symptom | Code | What to do |
|---|---|---|
| Package unsigned | `publish.unsigned` | Run the sign step before publish |
| Version already published | `registry.version-exists` | Apply the rerun rule above |
| No `--registry` | `usage.missing-argument` (exit 2) | Set the `REGISTRY_URL` variable |
| Token rejected | registry authentication error | Rotate the secret; do not retry |

## Performance regression gate

The manual sets hard deadlines: 20 ms for one `on-events` batch and 10 ms for one `render` call ([DEV-836]). A plugin can pass every test and still creep toward a deadline with each release. The gate compares each build with its last release and fails the pipeline before the drift reaches users. Test-level budgets are described by `assert_within_budget` ([DEV-622]) and the test targets by [DEV-851]; this gate covers the whole built component.

### What `bench` measures

`ilium plugin bench` calls the built component in the embedded test host with a real clock, 10 warm-up iterations and 100 measured iterations by default, and prints one `timing` record per export with `p50_ms`, `p99_ms` and `max_ms` ([DEV-922]). Each record is labelled `synthetic: true`, because the host is the test host and not the real session. It exits 3 when an export p99 exceeds its manifest `deadline_ms`.

The gate compares these metrics with the baseline, per export (`on-events`, `render`):

| Metric | Source | Compared |
|---|---|---|
| p50 | `timing.p50_ms` | Yes, typical cost |
| p99 | `timing.p99_ms` | Yes, tail cost. The tail is the value the deadline applies to |
| peak memory | `timing.peak_memory_bytes` (proposed field) | Yes, when the record has it |
| max | `timing.max_ms` | Recorded only. One slow outlier on a shared runner is noise |

The text of the audit names p95; `bench` prints p99, so the gate uses p99.

### The gate

```sh
ilium plugin build --release
ilium plugin bench --iterations 200 --baseline bench/baseline.json --max-regression 20
```

`--baseline <file>` and `--max-regression <percent>` are proposed flags, owner to confirm. Their behaviour:

- `bench/baseline.json` is a committed file with the `timing` records of the last release, the plugin version, the `ilium` version and the toolchain version.
- A metric regresses when it is more than `--max-regression` percent above its baseline value. A p99 that goes from 4.0 ms to 4.9 ms is +22.5 percent and fails at 20.
- The command prints one `bench.regression` record for each regressed metric (export, metric, baseline, measured, percent) and exits 3 ([DEV-764]: code 3 is findings). With no regression it exits 0. A missing baseline file is `bench.baseline-missing` (exit 1); the gate fails closed and never treats a missing file as a pass.
- A baseline taken with another `ilium` or toolchain version prints a `warning` record and still compares. Refresh the baseline in that case.

### Noisy runners

- Run `bench` on a dedicated runner or at least a runner with the same size for every run. Shared runners add scheduling noise.
- Raise `--iterations` to 200 or more, so p99 is built from enough samples; with 100 samples the p99 is the second highest value.
- Choose a tolerance above the run-to-run spread. Measure it by running `bench` 10 times on the baseline commit; a tolerance of 20 percent is a start, and a tolerance below the spread produces false failures that teach the team to ignore the gate.
- Retry a failed gate once. A metric that fails twice is a real regression.
- Time-based tolerance has a floor: ignore a regression under 0.2 ms absolute, since a 0.5 ms export that goes to 0.6 ms is +20 percent but harmless.

### Release builds only

The gate runs on `build --release` output. A debug build is slower by a large factor and `pack` refuses it. A pull-request job may run `bench` on the release build as a non-blocking report, and the tag job runs it as a blocking gate before `pack`.

### Refreshing the baseline

Refresh `bench/baseline.json` only in a commit of its own, after a release whose slower numbers were reviewed and accepted, or after a runner change. The commit message states the old and new numbers and the reason. The pipeline never writes the baseline by itself, because a job that refreshes it on a regression would hide that regression. A regression that is a deliberate trade (a new feature that costs 1 ms) is accepted by reviewing that commit, not by raising the tolerance.

### Failure modes

| Symptom | Record | Meaning |
|---|---|---|
| Export over its deadline | `timing` and exit 3 | A hard failure whatever the baseline says ([DEV-922]) |
| Export over the baseline tolerance | `bench.regression`, exit 3 | The build is slower than the last release |
| Baseline missing | `bench.baseline-missing`, exit 1 | Commit the file |
| Pass with very small margins | none | Tolerance too wide; narrow it |

## Build provenance in the release workflow

Provenance is the record of how a package was built. A reader who has the package, the record and the source can check the claim. This section adds the records to the release workflow. The rules are in [Trust and supply chain](/pages/reference-trust-and-supply-chain/) ([SEC-155]) and [Reproducible release builds](/pages/guide-reproducible-builds/) ([SEC-727]).

### What the release produces

- `provenance.json`: an in-toto statement of how the package was built (optional in a package, [SEC-155]).
- An SBOM: a list of the dependencies. `pack --sbom` writes `dist/<id>-<version>.sbom.json` in CycloneDX 1.5 JSON with every locked dependency, its version and its checksum, and prints an `artifact` record of kind `sbom` ([DEV-940]). The SBOM is reproducible. The package format also allows `sbom.spdx.json` (SPDX 2.3) as a listed file ([SEC-155]); the two formats are not converted by `pack`, so a release that must carry the SPDX file generates it with a separate tool and passes it to `pack` as a package file (proposed behaviour, owner to confirm).

`ilium plugin pack --sbom --provenance` (the `--provenance` flag is proposed, owner to confirm) writes both records and lists them in the package `files` map, hashed like every other file. As an alternative, a workflow can create the statement with an attestation action and pass the file to `pack`.

```yaml
      - run: ilium plugin build --release
      - run: ilium plugin pack --sbom --provenance
      - run: ilium plugin sign dist/*.iliumpkg --key "$RUNNER_TEMP/key"
      - run: ilium plugin inspect dist/*.iliumpkg --verify
```

Records are part of the package, so they are covered by the package signature. Signing happens after `pack`; adding a record after signing breaks the signature.

### What the statement must name

| Field | Value | Source |
|---|---|---|
| Subject | package file name and SHA-256 | the packed `.iliumpkg` |
| Source | repository URL and the commit hash | `git rev-parse HEAD` |
| Tag | the release tag | `GITHUB_REF_NAME` or `CI_COMMIT_TAG` |
| Toolchain | compiler, `ilium` and SDK versions, and `input_hash` | `dist/build.json` ([DEV-770]) |
| Builder | the workflow, the runner image and the run URL | CI service |
| Parameters | the build flags (`--release`, `--locked`) | the workflow |

A statement with a missing field, or whose subject hash is not the hash of the package it ships in, is refused by `inspect --verify` with a failed check, and a registry may refuse the upload.

### How readers use it

- `ilium plugin inspect dist/*.iliumpkg --verify` checks that each listed record is present and hashed correctly, and checks the signature. The host displays the records in Details and does not trust them ([SEC-155]); they inform the user and the reviewer, and never replace the signature or the grant review.
- A registry may show the statement beside the version. A user who wants more than the signature compares the statement with a rebuild.

### Reproducible rebuild

A reviewer rebuilds from the named commit with the named toolchain versions and `--locked` ([DEV-772]), packs with the same flags and compares hashes with the statement ([DEV-773]). The rebuilt package MUST give the same SHA-256 as the statement's subject, and the rebuilt SBOM the same bytes as the published SBOM ([DEV-940]). A mismatch means the statement is wrong or the build is not reproducible. The reviewer reports it to the publisher, and the first-party release rule of [SEC-727] applies to Ilium's own plugins: the release job refuses to sign when two builds differ (`build.not-reproducible`).

### Failure modes

| Symptom | Cause | Fix |
|---|---|---|
| Hash differs on rebuild | Another toolchain or a changed lock file | Rebuild with the versions in `dist/build.json` |
| Statement lists a different commit | Package built from a dirty checkout | Build from a clean checkout of the tag |
| Records appear unsigned | Added after `sign` | Run `pack` with the flags, then `sign` |
| SBOM lacks a dependency | Lock file not committed | Commit it ([DEV-772]) |
