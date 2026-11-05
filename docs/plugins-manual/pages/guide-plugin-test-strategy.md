---
layout: page
title: Plan the tests of a production plugin
permalink: /pages/guide-plugin-test-strategy/
---

## Plan the tests of a production plugin

This page says what a production plugin must test before it is released, and how flaky and quarantined cases are treated. The harness, the fixtures, the golden folders and the conformance suite form one release gate, so every author meets the same rules. Plugin authors read it before they write the first fixture, and maintainers read it when they review a release.

## You will need

Before you start, you need a plugin project with a manifest that declares its commands, views, hooks and grants, a `tests/fixtures/` folder with one JSON fixture for each case, and a build that produces the packed component in `dist/`. The rules below use the `ilium plugin check --release` and `ilium plugin test` commands, so install a version of the Ilium command line tool that provides both.

## The test layers and what each proves

The fixtures drive the plugin through its public contract. Each fixture names one case, and the harness runs the cases in sorted fixture-name order. A fixture proves that one command, view or hook behaves as declared, or that a grant denial happens when the grant is absent.

The golden folders hold the expected output of the cases. The harness compares the live output with the golden files on every run. Golden files change only through a local run with `--update`. The continuous integration pipeline never runs with `--update`, so a golden file cannot change in the same step that tests it.

The conformance suite checks the packed component against the host contract. The release gate runs it with `ilium plugin test --conformance` on the packed component in `dist/`. A debug build does not satisfy this check. The full rules of the harness are in [Plugin test harness reference](/pages/reference-plugin-test-harness/), and the suite cases are in [Conformance suite reference](/pages/reference-conformance-suite/).

## Required cases per side and grant

Every contribution of the plugin needs a fixture. A release counts each declared command, view and hook, and it needs at least one fixture that covers each of them. When one contribution has no fixture, `ilium plugin check --release` prints a `lint.untested-contribution` warning that names the contribution. For example, if a plugin declares two commands and has a fixture for only one, the check prints one warning that names the other command.

Every grant that the plugin requests needs two fixtures. The first fixture runs with the grant held, so it proves that the feature works when the host allows it. The second fixture runs with the grant absent and expects the host to refuse the call with `capability.denied`, so it proves that the plugin does not depend on the grant silently. If a grant such as `fs.read` has only the granted fixture, `ilium plugin check --release` prints `lint.grant-denial-untested` and names `fs.read`.

## Handling flaky and quarantined cases

A quarantined case is a case that is temporarily left out of the pass or fail decision. Quarantine is for a known problem that the author is fixing, and it is not a way to hide a failure. Each quarantined case must carry an `expires` date that is at most 14 days ahead. A quarantine with a longer date fails with `test.quarantine-too-long` and exit code 2. After the expiry date passes, the run fails with `test.quarantine-expired`, so the case counts again until its quarantine entry is removed.

A flaky case is a case that fails on one attempt and passes on a later attempt. The run lists it as flaky with the number of attempts, and a retry never turns it into a plain pass. A case that is flagged flaky in three consecutive runs is a persistent problem. The release gate then fails with `test.flaky-persistent` and names the case, so a release cannot ship with a case that is known to be unstable. Fix the cause of the flakiness and do not rely on a retry.

## Release gate

The release gate is the command `ilium plugin check --release`. It runs the checks of this page in one pass and fails when any of them fails. The gate checks that every contribution has a fixture, that every grant has a granted and a denied fixture, that no quarantine has passed its expiry date, and that no case has been flaky in three consecutive runs. It also requires a packed build. If you build only a debug component, the gate fails with `release.conformance-not-packed`, because the conformance run must use the packed component in `dist/`.

A release that passes the gate is built from pinned inputs. For the rules on pinned inputs and reproducible builds, see [Reproducible builds](/pages/guide-reproducible-builds/).

## Edge cases

A plugin with no declared commands, views or hooks still needs its grants covered. A plugin with no grants still needs every contribution covered. A quarantined case still runs and its result still appears in the report, even though the pass or fail decision leaves it out. A case that is both quarantined and flaky follows the quarantine rules and is still listed as flaky in the report.

## Requirements

- **[PDT-045]** A release MUST have at least one fixture per declared command, view and hook, otherwise `ilium plugin check --release` warns with `lint.untested-contribution` naming each uncovered contribution.
- **[PDT-046]** Every requested grant MUST have one fixture that runs with the grant held and one that expects `capability.denied` with the grant absent.
- **[PDT-047]** A quarantined case ([DEV-960]) MUST carry an `expires` date at most 14 days ahead, and a run after that date MUST fail with `test.quarantine-expired`.
- **[PDT-048]** The release gate MUST run `ilium plugin test --conformance` against the packed component in dist/, not a debug build.
- **[PDT-049]** A case flagged flaky ([DEV-963]) in three consecutive runs MUST fail the release gate with `test.flaky-persistent`.
