---
layout: page
title: Contributing
permalink: /pages/contributing/
---

## Contributing to Ilium and its core plugins

This page is for people outside the Ilium project who want to report a bug, fix something or add something in the host repository or in a core plugin. The CI guide, [CI recipes](/pages/guide-ci-recipes/), covers plugin forks and plugin repositories. This page covers the host repository and the core plugins.

Terms:

- **Core plugin**: a bundled plugin whose id starts with `ilium.core.`. See [Core plugins](/pages/core-plugins/).
- **Contract or security change**: a change to the plugin contract `ilium:plugin@0.1.0`, to a grant, quota, signature or sandbox rule, or to a requirement row with a `SEC`, `CAP` or `API` prefix.
- **Maintainer**: a person listed in `MAINTAINERS` at the repository root.
- **Requirement row**: one row of a page listed in [Requirements index](/pages/reference-requirements/).
- **Snapshot**: the manual published with one host release ([Documentation versions](/pages/reference-documentation-versions/)).

## You will need

- A clone of the host repository and the `ilium` command line.
- `git` configured with your real name and email, because the sign-off line uses them.
- For a bug report: the version from `ilium --version --verbose`.

## Report a bug

Open an issue with the "Bug report" template. The template has these fields, and an issue without the first four is labelled `needs-info` and closed after 30 days without an answer:

| Field | What to write |
|---|---|
| Version | The output of `ilium --version --verbose` |
| Platform | Operating system, architecture and terminal |
| Steps | The shortest list of steps that shows the fault |
| Expected and actual | What you expected and what happened, with the error code if one was shown |
| Plugin involved | The plugin id and version, or "none". Try `ilium --safe-mode=third-party` first ([Safe mode and troubleshooting](/pages/user-safe-mode-and-troubleshooting/)); say whether the fault stays |
| Support bundle | The file from `ilium plugin support-bundle <id> --out <file>` when a plugin is involved |

The support bundle is one zip archive with exactly four entries, built under [USR-221], and writing it does not change plugin state ([USR-225]). It is built on your machine and goes nowhere until you attach it. Before you attach it, open the archive and check these points:

- Remove or replace home folder paths that hold your user name, host names, internal URLs and any token or key that a log line shows. Replace each with `[redacted]`. Do not delete a whole entry; a bundle with a missing entry is treated as incomplete.
- Do not attach `SECURITY.md`-class material: a bundle that shows a working exploit, a leaked secret or a sandbox escape is a security report, not a bug report. Send it to the contact in [Security response](/pages/reference-security-response/) ([SEC-320]) and do not open a public issue.
- Do not attach terminal screen content or agent conversations. They are not in the bundle and the project does not need them.

A maintainer who finds an unredacted secret in an attachment edits the attachment out of the issue and asks you to rotate the secret.

## Propose a change

| Kind of change | What to do first |
|---|---|
| Fix of a defect, a typo or a wrong error text | Open a pull request directly. Link the issue if one exists |
| Change of behaviour, a new setting, a new command, a new capability or a new extension point | Open an issue first and wait for a maintainer to label it `accepted`. A pull request for an unaccepted behaviour change is closed with the reason |
| Change that touches the contract, a grant or the sandbox | Open an issue first. The label is `contract`. It is reviewed under the two-maintainer rule below |
| Vulnerability fix | Do not open a pull request. See "What contributors must never do" |

A pull request has one purpose. A fix and a refactor are two pull requests.

## Sign-off

The project uses the Developer Certificate of Origin, version 1.1. You certify that you wrote the change or have the right to submit it under the project licence. You state this by adding a sign-off line to every commit:

`Signed-off-by: Your Name <your.email@example.org>`

`git commit -s` adds it. The name and email MUST match the commit author. The project does not ask for a contributor licence agreement. A pull request with a commit that lacks a valid line fails the `dco` check. Fix it with `git rebase --signoff <base>` and a force-push to your own branch. A commit made by a bot on your behalf needs your line, not the bot's.

## Checks a pull request must pass

All checks run on the pull request without secrets, so a forked pull request gets the same checks ([DEV-778]). No check signs anything ([DEV-776]).

| Check | Command | Passes when |
|---|---|---|
| Format | `cargo fmt --check` | No diff |
| Lint | `cargo clippy --workspace --all-targets` | No new warning. A warning is fixed, not silenced, unless the pull request records a reason next to the `#[allow]` |
| Unit and integration tests | `cargo test --workspace` | Every test passes |
| Requirement-row tests | The tests named in the Test column of the rows your change touches | Each row you changed or added has a test, and the test fails without your change |
| Fixture suite | The fixture and conformance suites ([Conformance suite](/pages/reference-conformance-suite/)) | Every case passes or is recorded as `skip` with a reason |
| Docs build | `docs/plugins-manual/build.sh` | The last JSONL line is a `result` object and the exit code is 0 |
| Changelog | A new file in `changelog.d/` | The file exists for a change that a user can see |
| DCO | Sign-off on each commit | Every commit has a valid line |

You can run the first three on your machine. Compilation of the host uses the project's build service in CI, so a local failure to build on an unusual toolchain is not a reason to skip the check; say so in the pull request.

## Changing a core plugin

A core plugin has three records that must change together in one pull request:

1. The plugin page, `core-plugin-<name>.md`, which defines its surface and its stored data.
2. The row for that plugin in [Core replacement matrix](/pages/reference-core-replacement-matrix/): surface items, stored data to leave alone, CLI names, what is lost while off and the install error code. A change to any of these on the plugin page without the matching row change fails review, because a replacement package is checked against the matrix ([DEV-846] to [DEV-849]).
3. The requirement rows that cite the plugin, in the requirements pages listed in [Requirements index](/pages/reference-requirements/), with their tests.

A new surface item is additive and needs the matrix row. A removed or renamed surface item is a contract change and needs two maintainers. A core plugin that is replaceable stays replaceable; changing that is a contract change.

## Allocating a requirement id

1. Pick the prefix of the area ([Requirements index](/pages/reference-requirements/), "Prefixes"). Never invent a prefix.
2. Take the next unused number in the group of tens that fits the topic. If the group is full, take the first number of the next free group.
3. Never reuse a number, including the number of a removed row. A removed row leaves its number unused.
4. Define the id in exactly one requirements page. Other pages cite it in square brackets.
5. Write one keyword per row, and give each row a source page and a test that can fail.

Two pull requests that take the same number are resolved by the maintainer who merges second: the later pull request moves to the next free number and updates every citation. Ids of rows marked "pending: test to be written" still need the test before the next release.

## Review rules

| Change | Approvals needed | Who may not approve |
|---|---|---|
| Fix, docs, tests, wording | One maintainer | The author |
| Contract, grant, quota, signature, sandbox or security change | Two maintainers, at least one from the owners of the area | The author, and a maintainer from the author's own employer counts once |
| Change to a core replacement matrix row | Two maintainers | The author |

A maintainer approves only after the required checks are green. New commits after an approval on a two-maintainer change reset the approvals. A review comment marked "blocking" is resolved by the reviewer, not the author. A pull request without a reply from the author for 30 days is closed and can be reopened.

## What contributors must never do

- Do not edit a released snapshot of the manual. A snapshot describes exactly one host release ([LIF-965]). A correction goes to the current manual first, and a maintainer copies it into the snapshots that need it ([LIF-967]). If you find a wrong statement in a snapshot, open an issue that names the snapshot and the page.
- Do not put a security fix in a public pull request, a public branch or a public issue before the advisory is published ([SEC-321], [SEC-322]). Send the report privately. A maintainer prepares the fix in a private fork, and the advisory is published with the fix.
- Do not leave a High or Critical fix without a regression fixture. The fix is not accepted until the hostile-plugin suite has a fixture whose name contains the advisory id ([SEC-704]).
- Do not weaken a test, a quota or a gate to make a pull request green. Fix the cause.
- Do not add a build step that signs a package or reads a signing key in a pull request check ([DEV-776], [DEV-778]).
- Do not add telemetry, a network call at start-up or a new default capability grant without an accepted issue.
- Do not commit generated files, binaries, secrets or personal data.

## Failure modes

| What you see | Cause | What to do |
|---|---|---|
| `dco` check fails | A commit has no valid sign-off line | `git rebase --signoff <base>` and force-push your branch |
| Docs build prints an `error` line | Broken front matter, link or a missing page in the navigation | Read the message, fix the page, run `docs/plugins-manual/build.sh` again |
| Requirement-row test missing | A row was added or changed without a test | Write the test named in the row's Test column |
| Matrix and plugin page disagree | One of the two records changed alone | Change both in the same pull request |
| Issue closed as `needs-info` | The template fields were not filled in within 30 days | Open a new issue with the fields filled in |
| Pull request closed as unaccepted | A behaviour change had no accepted issue | Open the issue and wait for `accepted` |

Next steps: [CI recipes](/pages/guide-ci-recipes/) for plugin repositories, [Security response](/pages/reference-security-response/) for vulnerabilities, [Documentation versions](/pages/reference-documentation-versions/) for snapshots.
