---
layout: page
title: CI test report formats
permalink: /pages/guide-ci-test-report-formats/
---

## You will need

This page specifies how the native JSON report of `ilium plugin test` becomes a JUnit XML file that a CI dashboard can show per case. It is written for pipeline authors who want per-case results in their dashboard and for tool authors who read the converted file. The harness writes only the native report, as [Plugin test harness CI run](/pages/reference-plugin-test-harness/) states, so every other format is derived from that one file.

You need the `ilium` command on the machine that runs the pipeline. Run the tests with `ilium plugin test --report <file>` to write the native report. Run the conformance suite with `ilium plugin test --conformance --report <file>` to write the conformance report. The converter is a separate command, `ilium plugin test-report`, and it reads a native report file that an earlier run wrote. The pipeline layout that calls these commands is described in [CI recipes](/pages/guide-ci-recipes/).

## The native report file

The native report is one JSON document with the schema value `ilium-test-report/1`. It holds the top-level fields `schema`, `complete`, `exit_code`, `plugin`, `side`, `harness_version`, `seed`, `component_sha256`, `cases` and `summary`. Each entry of `cases` carries a `name`, a `fixture`, a `status` of `pass`, `fail`, `trap` or `skipped`, a `flaky` flag and a `reason` when the status is not `pass`. The `summary` counts the passed, failed, trapped, skipped and flaky cases.

The converter reads this file and never changes it. The full field list and its meaning are defined on [Plugin test harness CI run](/pages/reference-plugin-test-harness/) and in the harness contract of [Test harness reference](/pages/reference-test-harness/).

## JUnit XML from the native report

Convert a native report with this command:

```
ilium plugin test-report report.json --format junit --out junit.xml
```

The output is a JUnit XML file with one `testcase` element for each case in the native report. A report with 12 cases therefore yields 12 `testcase` elements, and a parser that counts them gets 12. The `--out` option names the file to write. The JUnit file carries the per-case results only, and it has no field for the seed, the harness version or the component hash. Those values stay in the native report, which the pipeline should keep as the record of the run.

## Annotations for GitHub Actions and GitLab

This page defines only the JUnit format. The requirements on this page do not specify an annotation format for GitHub Actions or GitLab. A pipeline that wants inline annotations reads the JUnit file, or the native report, in its own step, and uses the CI system's own tooling for that step. The mapping rules below are the ones that a reader of the JUnit file can rely on.

## Mapping verdicts and flaky cases

Each native case status maps to one JUnit element. A case with status `fail` or `trap` becomes a `testcase` with a `failure` child. A case with status `skipped` becomes a `testcase` with a `skipped` child. A case that passes and has no flaky mark becomes a `testcase` with no child.

A case with `flaky: true` becomes a passing `testcase`. That element carries the property `<property name="flaky" value="true"/>` and has no `failure` child. A dashboard therefore counts a flaky case as passed, and it can still show the property to a reader who looks for flaky cases. A run with one fail, one skipped and one flaky case produces one `failure`, one `skipped` and one `testcase` carrying the flaky property.

## Edge cases

An unknown value of `--format` is refused. For example, `ilium plugin test-report report.json --format tap` exits with code 2 and prints `usage.invalid-value`.

An interrupted run still converts. When the user stops the run with Ctrl+C, the native report is written with exit code 130 and `complete` set to `false`. The converter still produces a well-formed JUnit file, and that file carries the testsuite property `<property name="complete" value="false"/>`. A dashboard can therefore tell a partial run from a finished one.

The conformance report uses the same command and the same formats. Pass its path to `ilium plugin test-report`, and use the same `--format junit` and `--out` options. Each `testcase` then takes the case id as its `name` attribute, so a row in the dashboard names the conformance case that it reports.

The JUnit file does not contain absolute paths, user names or wall-clock timestamps. Two machines with different user names and different home directories therefore produce byte-identical files from the same native report. A pipeline can therefore compare such files directly.

## Requirements

- **[PDT-033]** `ilium plugin test-report <report.json> --format junit --out <path>` MUST write a JUnit XML file with one `testcase` element per case in the native report.
- **[PDT-034]** A `fail` or `trap` status MUST map to a `failure` element, `skipped` to a `skipped` element, and a case with `flaky: true` MUST map to a passing testcase carrying a property `flaky` with value `true`.
- **[PDT-035]** The JUnit file MUST NOT contain absolute paths, user names or wall-clock timestamps, so output from different machines compares equal ([DEV-890]).
- **[PDT-036]** An unknown `--format` value MUST fail with exit code 2 and `usage.invalid-value`.
- **[PDT-037]** A native report from an interrupted run (exit 130) MUST still convert to a well-formed JUnit file carrying the testsuite property `complete` with value `false`.
- **[PDT-038]** The conformance suite report MUST convert with the same command and formats, using the case id as the `testcase` name.
