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
