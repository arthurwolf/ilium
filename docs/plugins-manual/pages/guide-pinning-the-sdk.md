---
layout: page
title: Pinning and updating the SDK
permalink: /pages/guide-pinning-the-sdk/
---

## Pinning and updating the SDK

This page specifies how a plugin project pins its plugin SDK, which lockfile it commits, which checks `ilium plugin check` and `ilium plugin build` run against that pin, and how an author moves the pin within a minor line and on to a new minor line. Plugin authors who write a Rust or TypeScript plugin need it, as do the people who maintain the scheduled jobs that propose SDK updates. The missing-lockfile failure is defined in [Reproducible builds](/pages/guide-reproducible-builds/), and this page does not repeat its rules.

## Why pin

The SDK version decides which host API minors a plugin can use and which SDK items its source can call. If a requirement admits any newer minor, the same source can produce different builds on different days, and a plugin can start to call an item that a later minor removed. A pin fixes the SDK line in the manifest and the lockfile, so every build resolves the same code and the checks in `ilium plugin check` test one known version.

## Rust: Cargo.toml requirement forms and Cargo.lock

The Rust templates declare the SDK in `Cargo.toml` as `ilium-plugin-sdk = "~0.4.2"`. A tilde requirement accepts 0.4.2 and any later patch release of the 0.4 line, and it refuses 0.5.0 and every later minor. The requirement forms `*`, `^0.4` and a bare major admit a higher minor, so do not use them.

Commit `Cargo.lock` next to `Cargo.toml`. The lockfile records the exact SDK version that the project built, and a release build needs it. See the [Rust SDK](/pages/rust-sdk/) page for the template layout.

## TypeScript: package.json range and bun.lock

The TypeScript templates declare the SDK in `package.json` with the range `"~0.4.2"`. As with Rust, the tilde range accepts later patch releases of 0.4 and refuses 0.5.0.

Commit `bun.lock` next to `package.json`. Do not replace the tilde range with a caret range or a wildcard. See the [TypeScript SDK](/pages/typescript-sdk/) page for the template layout.

## Checks the CLI makes on build

Several checks in the plugin CLI guard the pin. `ilium plugin check` warns with `lint.sdk-range-open` when a requirement admits a higher minor than the line the author intends. It fails with `lint.sdk-deprecated` as an error when the resolved SDK minor has reached the removal minor of an item the plugin still uses.

`ilium plugin check --release` fails with `build.unpinned-inputs` when the lockfile is missing. `ilium plugin build` fails with `build.sdk-line-mixed` when a plugin uses the Rust and the TypeScript SDK on different lines, and it fails with `build.sdk-host-mismatch` when the manifest `api` is outside the host range of the resolved SDK. The file `dist/build.json` records the resolved SDK version and the lowest and highest host API minors that SDK declares, so the range check can be read after a build. See [Plugin CLI](/pages/reference-plugin-cli/) and [Check and lint rules](/pages/reference-check-and-lint-rules/) for the full output format.

## Bumping within a line and across lines

A patch bump stays inside the pinned line. For example, moving the lock from 0.4.2 to 0.4.3 with `cargo update -p ilium-plugin-sdk` for Rust, or `bun update ilium-plugin-sdk` for TypeScript, does not change any exported signature. An author may accept patch updates automatically. After the update, run `ilium plugin build` and confirm that the exported WIT interface hash in `dist/build.json` is the same as before.

A minor bump is a different kind of change. Make it in its own commit, and re-run `ilium plugin check` before the commit is made. Before moving the pin to a new minor, look up the removal minor of every item the plugin uses in [SDK deprecation](/pages/reference-sdk-deprecation/), and replace each item whose removal minor is at or below the new minor. Then change both SDK requirements to the new line, for example `~0.5.0`, update the lockfiles, run `ilium plugin check` to confirm that no `lint.sdk-deprecated` error remains, and then run `ilium plugin build`. The pin is moved only after the items it would break have been replaced.

## Keeping the manifest api and the SDK range aligned

The manifest `api` must fall inside the lowest and highest host API minors that the resolved SDK declares. When the SDK line changes, update the manifest `api` to a minor inside the new range in the same commit as the pin. If the value is outside the range, `ilium plugin build` fails with `build.sdk-host-mismatch` and writes no package under `dist/`. The manifest format is described in [Manifest reference](/pages/reference-manifest/).

## CI recipe for scheduled bump PRs

A scheduled job can open pull requests for patch updates only. It updates the SDK entry inside the pinned line, runs `ilium plugin check --release` and `ilium plugin build`, and opens a pull request that changes the lockfile alone when both commands pass. If either command fails, the job reports the failure and opens no pull request. A minor bump is never opened by the scheduled job. An author makes it by hand, in its own commit, after replacing the items the new line removes.

## Requirements

- **[PDX-001]** A plugin project MUST commit its lockfile (`Cargo.lock` for Rust, `bun.lock` for TypeScript), and a release build or `ilium plugin check --release` without one MUST fail with `build.unpinned-inputs` as defined by [DEV-893].
- **[PDX-002]** The templates MUST pin the SDK to a tilde-style minor range (Rust `ilium-plugin-sdk = "~0.4.2"`, TypeScript `"~0.4.2"`), and `ilium plugin check` MUST warn with `lint.sdk-range-open` for any requirement that admits a higher minor, such as `*`, `^0.4` or a bare major.
- **[PDX-003]** `dist/build.json` MUST record the resolved SDK version together with the lowest and highest host API minors that SDK declares, and `ilium plugin build` MUST fail with `build.sdk-host-mismatch` when the manifest `api` is outside that range.
- **[PDX-004]** When one plugin uses both the Rust and the TypeScript SDK, they MUST resolve to the same SDK line (same major.minor), otherwise `ilium plugin build` MUST fail with `build.sdk-line-mixed`.
- **[PDX-005]** A patch bump within a pinned SDK line MUST NOT change any exported signature (per [API-083] and [API-086]), so an author MAY accept patch updates automatically, while a minor bump MUST be made in its own commit with `ilium plugin check` re-run.
- **[PDX-006]** `ilium plugin check` MUST fail with `lint.sdk-deprecated` as an error when the resolved SDK minor equals or exceeds the removal minor of an item the plugin still uses ([DEV-997]), so the pin is moved only after the item is replaced.
