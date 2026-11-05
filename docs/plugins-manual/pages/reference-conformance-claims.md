---
layout: page
title: Conformance claims and reports
permalink: /pages/reference-conformance-claims/
---

## Conformance claims and reports

This page specifies what a conformance claim states, how a signed report backs it, how a third party verifies the claim with `ilium plugin verify-conformance`, when a claim goes stale, and which claims are refused. It is for third-party SDK authors and alternative host authors who want to publish a statement that their build follows the contract `ilium:plugin@0.1.0`, and who want anyone to be able to check that statement against the suite. The suite itself is described in [Conformance suite](/pages/reference-conformance-suite/).

### You will need

- A build of the SDK or host under test, with its SHA-256 hash computed over the exact artefact you publish.
- The version of the conformance suite that you ran, in the form `MAJOR.MINOR`.
- A copy of the claim template, which contains only the wording that the governance page permits for the Ilium name and logo. See [Use of the Ilium name and logo](/pages/governance/#use-of-the-ilium-name-and-logo).
- The `ilium` command line tool, to run `ilium plugin verify-conformance`.

Because the `sdk` and `host` subjects are reserved for the Ilium source tree, check which subjects a normal install can run in [Conformance suite](/pages/reference-conformance-suite/) before you plan the run.

## What a claim states

A conformance claim is a file that names the contract `ilium:plugin@0.1.0`, the suite version it was checked against, and the subject that was tested. The claim also records how many cases passed, were skipped and failed, and it lists those counts by case id. A claim that leaves out any of these values is not valid.

A claim is refused when either of two conditions holds. The first is that any case it counts has a retired id, which is an id the suite no longer carries. The second is that any counted case has failed. A claim with a retired id fails verification with the code `conformance.retired-case`, and a claim with one failed case fails with `conformance.case-failed`. In both cases the exit code is non-zero.

The statement of the claim is limited to the wording of the claim template. A claim whose statement differs from the permitted wording fails the template check with `conformance.wording-invalid`. The claim must not use the Ilium name or logo beyond that wording.

## Producing a signed report

A report is the file that backs a claim. It must include the SHA-256 hash of the component or host build under test, in a field named `subject_sha256`. The signature on the report covers that hash, so the report is tied to exactly one build. A report without `subject_sha256` is malformed. When you remove that field from a report, `ilium plugin verify-conformance` exits with code 2 and names `subject_sha256` in its output.

## Verifying a claim

Run `ilium plugin verify-conformance <report>` to check a claim. Pass `--subject` with the path of the artefact to compare its hash with the hash in the report. The command reports the following failures:

| Failure | Exit code | Output names |
|---|---|---|
| A required field is missing, for example `suite_version` or `subject_sha256` | 2 | The missing field |
| The input is not JSON | 2 | The input as malformed |
| The subject hash does not match the artefact given with `--subject` | 3 | `conformance.hash-mismatch` |
| A counted case has a retired id | Non-zero | `conformance.retired-case` |
| A counted case failed | Non-zero | `conformance.case-failed` |

A missing `suite_version` gives exit code 2, and the output names `suite_version`. A report that is not JSON also gives exit code 2. A `--subject` that points at a different build gives exit code 3 with `conformance.hash-mismatch`.

## Expiry and re-run

A claim stays valid for the suite version it names, but it does not stay current forever. When a new suite minor version is released, the claim must be re-run and re-published within 90 days. If it is not, verification marks the claim as stale. For example, a claim made against suite 1.2 is marked stale when suite 1.3 was released 91 days earlier, and the output then carries `status: "stale"`.

Re-running the claim means running the suite again on the build under test. A changed build produces a new hash, and the new report must cover that hash.

## Edge cases

A claim that counts a retired case id is refused even when every other case passed. A single failed case refuses the whole claim, regardless of the number of passed cases. A report whose `subject_sha256` does not match the artefact given to `--subject` is refused with exit code 3, even when the claim itself is otherwise valid. A file that is not JSON is refused with exit code 2.

A claim whose statement uses other wording than the template permits is refused with `conformance.wording-invalid`, even if its counts are correct.

## Requirements

- **[PDT-039]** A conformance claim MUST name the contract `ilium:plugin@0.1.0`, the suite version, the subject, and the pass, skip and fail counts by case id.
- **[PDT-040]** A claim MUST be invalid if any counted case has a retired id or if any case failed.
- **[PDT-041]** A claim MUST be re-run and re-published within 90 days of a new suite minor, otherwise verification MUST mark it `stale`.
- **[PDT-042]** A report MUST include the SHA-256 of the component or host build under test, and the signed report MUST cover that hash.
- **[PDT-043]** `ilium plugin verify-conformance <report>` MUST exit 3 when the subject hash does not match the provided artefact and exit 2 on malformed input.
- **[PDT-044]** A claim MUST NOT use the Ilium name or logo beyond the wording the governance page permits, and the published claim template MUST contain only that wording.
