---
layout: page
title: Reproducible builds
permalink: /pages/guide-reproducible-builds/
---

## Reproducible builds

This guide defines the command that checks a build for reproducibility, the inputs an author must pin and the failure code on a mismatch. The CI recipe that uses it is in [CI recipes](/pages/guide-ci-recipes/). This page describes target behaviour; it does not claim the behaviour exists today.

Requirement ids of this page are in the range `DEV-892` to `DEV-893`. The complete list is in [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/).

### Rules

- **[DEV-892]** `ilium plugin verify-reproducible <dir>` MUST build the project twice in clean, separate build directories and compare the resulting component and package byte for byte. It MUST exit 0 when they are identical and exit 1 with `build.not-reproducible` when they differ, printing the first differing artifact and the offset.

- **[DEV-893]** A build is reproducible only with pinned inputs: a lockfile (`Cargo.lock` or `bun.lock`), the toolchain versions recorded in `dist/build.json`, and no network access during the build. `verify-reproducible` MUST fail with `build.unpinned-input` before building when a lockfile is missing or the recorded toolchain differs from the installed one.

### Failure behaviour

Each rule names its failure code. A failure prints a `diagnostic` record in JSONL or one line on standard error in text, and the process exit code follows [Plugin command-line reference](/pages/reference-plugin-cli/#exit-codes).

## See also

- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/)

API checked against `ilium:plugin@0.1.0`.
