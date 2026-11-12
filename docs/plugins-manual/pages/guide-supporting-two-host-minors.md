---
layout: page
title: Support two host minors
permalink: /pages/guide-supporting-two-host-minors/
---

## Support two host minors

This guide takes one plugin from a `requires.host` range that admits a single host minor to a range that admits two. It uses the commands, the golden folder layout and the CI job that the manual defines on other pages, and it shows what each step should print. The steps are written for minors `0.1` and `0.2`. Replace them with the two minors that your release actually supports.

The plugin in this example is `com.example.review`. Its manifest starts with the range below.

```json
{
  "id": "com.example.review",
  "version": "1.0.0",
  "requires": { "host": ">=0.1.0 <0.2.0" }
}
```

A range over host minors uses the comparator syntax of [Dependency resolution](/pages/reference-dependency-resolution/), where space-separated comparators must all hold ([PKG-315]). The lower bound of `requires.host` is the oldest host release the plugin accepts. The upper bound stops the plugin from loading on a host minor it was never tested with.

The `<pre class="term">` blocks on this page show the target output of each command. The CLI must print these lines, and the CI job of [AUT-958](#requirement-statements) compares its output with them. Where a line differs, treat the page as the specification and report the difference.

### Step 1: confirm the single-minor build

Run the checks on the plugin as it is. Its manifest still names one minor, so the result should not change.

```text
ilium plugin check
ilium plugin lint --deny warnings
ilium plugin test
```

<pre class="term"><span class="t-prompt">$</span> ilium plugin check
<span class="t-ok">ok</span>   com.example.review 1.0.0 requires host &gt;=0.1.0 &lt;0.2.0
<span class="t-prompt">$</span> ilium plugin test
<span class="t-ok">ok</span>   3 cases passed, 0 failed
</pre>

Each command must exit with code 0. If `ilium plugin check` reports a `lint.host-range-below-api` warning, the lower bound is older than the first host release that ships the manifest `api` minor. Raise the lower bound before you go on.

### Step 2: widen the range to two minors

Change the range so that it admits minor `0.2` as well.

```json
{
  "requires": { "host": ">=0.1.0 <0.3.0" }
}
```

Run `ilium plugin check` and `ilium plugin lint --deny warnings` again. The lint run must report no finding. A `lint.host-range-above-api` warning means that the upper bound admits a host release whose window does not yet cover the manifest `api` minor. Narrow the upper bound or wait for that release, and do not raise the version of the plugin to hide the warning.

<div class="note">
<p>Do not widen the range past the minors that the golden files and the CI job cover. The range states what the plugin claims. Tests are the only evidence for that claim.</p>
</div>

### Step 3: lay out one golden folder per minor

Each minor has its own golden folder. The render of a view can differ between minors, so one folder would give false failures on one of them.

```text
tests/golden/0.1/    golden files rendered on host contract 0.1
tests/golden/0.2/    golden files rendered on host contract 0.2
```

Make the test code choose the folder with the contract version that the running host reports. The test reads it with `host.contract_version()` in Rust, or with `host.contractVersion()` in TypeScript, and then opens `tests/golden/<minor>/`. A test that reads the other folder fails, so the choice is checked on every run.

### Step 4: create and review the golden files for each minor

Create the golden files on a developer machine, one minor at a time. Do not run the update in CI, because CI refuses it ([DEV-503]).

```text
ilium plugin test --host-contract 0.1 --update
ilium plugin test --host-contract 0.2 --update
```

Review the changed files in version control. Every changed line must be expected. Then commit both folders together.

<pre class="term"><span class="t-prompt">$</span> ilium plugin test --host-contract 0.2 --update
<span class="t-ok">ok</span>   wrote tests/golden/0.2/ (contract 0.2)
<span class="t-warn">warn</span> review the diff before you commit it
</pre>

### Step 5: run the tests and the conformance suite for each minor

Run every minor on its own. A run that names `--host-contract` renders with the renderer of that contract, so a pass on one minor proves nothing about the other.

```text
ilium plugin test --host-contract 0.1
ilium plugin test --host-contract 0.2
ilium plugin test --conformance --host-contract 0.1
ilium plugin test --conformance --host-contract 0.2
```

Each command must exit with code 0. A conformance case with a `fail` verdict returns 3, and it is never quarantined ([DEV-961]). If `--host-contract` names a minor that the installed CLI does not embed, the run exits 2 with `usage.host-contract-unavailable` before any case runs. Install a CLI that embeds the minor, or drop the minor from the range.

### Step 6: run both minors in the examples repository

Your own CI runs the commands of Step 5 once for each minor in a matrix. Separately, the examples repository runs the same commands for every example ([DEV-174]), and the job that covers this plugin there runs the commands of Step 5 for each minor too. Both jobs must also run `ilium plugin check` and `ilium plugin lint --deny warnings`. The matrix pattern is described in [CI recipes](/pages/guide-ci-recipes/), and the recipe names the oldest and the newest contract that a plugin must pass ([DEV-193]).

### Step 7: publish only when both minors pass

Publish the new version when every job of the matrix is green. The range must not be wider than the minors that the matrix covered ([AUT-212]). If a later host minor is released, add its golden folder and its matrix row in the same change that widens the range.

## Requirement statements

- **[AUT-957]** The manual MUST contain one worked page that takes a plugin from a single-minor `requires` range to a two-minor range, with the commands, the golden directory layout per minor and the expected output of each step.

- **[AUT-958]** The worked page's steps MUST be run as a CI job in the examples repository, and the job MUST fail when the page's commands or expected output change.

## Production additions (round 10)

- **[PRR-009]** The system must publish the contract and host test bundle of every host minor on the beta channel at least 7 days before stable, accept a beta contract in ilium plugin test --host-contract and --conformance with every record labelled pre-release true, and make ilium plugin publish refuse a package whose requires range admits only a beta contract with an error code naming the beta (owner page is guide-supporting-two-host-minors, with release-process for the host side).
