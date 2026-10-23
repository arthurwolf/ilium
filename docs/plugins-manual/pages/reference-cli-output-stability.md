---
layout: page
title: CLI output stability
permalink: /pages/reference-cli-output-stability/
---

## CLI output stability

This page states which parts of the plugin command line are a contract, how a change to them is announced, and how a breaking change is versioned. Scripts and CI pipelines that parse `--json` records or test exit codes depend on those names and numbers, so they stay stable across releases. The rules apply to every command in [Plugin command-line reference](/pages/reference-plugin-cli/).

Text output is outside this contract. A script that reads text output gets no protection from this page, and it MUST use `--json` instead.

Requirement ids of this page are in the range `DEV-858` to `DEV-864` and `DEV-891`. The complete list is in [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/).

### Terms

- **Contract**: the set of names and numbers in this page that a script may depend on. Everything else is outside it.
- **Item**: one flag, one record field, one record `type`, one value of an enumerated field, or one exit code.
- **Deprecated**: an item that a release note marks for removal or renaming, and that still works as before.
- **Breaking change**: a removal or rename governed by [DEV-858], or a change in the type or meaning of a field ([DEV-860]).
- **Marking release**: the release whose notes first mark an item deprecated.
- **Stop release**: the release that the marking release names as the one in which the item stops working.
- **Contract version**: the version `ilium:plugin@0.1.0` of the plugin contract. See [Versioning](/pages/versioning/) and [ABI evolution](/pages/reference-abi-evolution/).
- **Release notes**: the notes that ship with each release of the command line. Their location is not settled by any page (see the decisions below).

### What is covered

The contract has three parts:

- the `type` values of the JSONL records, and the fields that the [record catalogue](/pages/reference-plugin-cli/#the-record-catalogue) defines for each type;
- the exit codes in the [exit code table](/pages/reference-plugin-cli/#exit-codes);
- the flag names, and the meaning of each flag in the command reference.

The enumerated values are part of the contract too: the `step` values of `progress` ([DEV-021]), the `kind` values of `artifact` ([DEV-022]), the `severity` values ([DEV-023]), the `status` values ([DEV-024]) and the `side` values ([DEV-025]). The envelope is covered: one `summary` record, then one `result` record as the last line ([DEV-017]), and the `exit_code` of `result` equals the process exit code ([DEV-007]).

Adding a field, a record type or a flag is not a breaking change. A consumer MUST ignore fields that it does not know, as the record rules of the command-line reference already require.

### Rules

- **[DEV-858]** A flag, field or record type that a release removes or renames MUST first be marked deprecated in the release notes. The marking release MUST name the release in which it stops working. Until that release, the item MUST keep working as before.

- **[DEV-859]** While a flag, field or record type is marked deprecated, every command that uses it MUST print a notice. In JSONL output the notice is one `warning` record with the code `deprecated`. In text output it is one line on standard error. The notice MUST name the item and the release in which it stops working, and it is printed once per invocation.

- **[DEV-860]** A breaking change MUST happen only in a release whose contract version has a new minor number, and MUST be listed in that release's notes. A breaking change is a removal or rename that [DEV-858] governs, or a change in the type or meaning of a field.

- **[DEV-861]** The number and the meaning of an exit code MUST NOT change in any release. A new exit code MUST be added to the exit code table and listed in the release notes, and no existing code MUST be given a new meaning.

- **[DEV-862]** Removing or renaming a value of an enumerated field listed under "What is covered" MUST be treated as a breaking change under [DEV-858] and [DEV-860]. Adding a value is not breaking, and a consumer MUST treat a value it does not know as unknown, never as an error.

- **[DEV-863]** Each deprecation and each breaking change MUST appear in the release notes of its release as one entry that names the command, the item, the kind of change (`deprecated` or `breaking`) and the stop release.

- **[DEV-864]** The `deprecated` warning of [DEV-859] MUST use only the fields that the `warning` record defines (`code`, `message`, optional `file`, `line`, `column`), and the `code` MUST be exactly `deprecated`. It MUST NOT change the exit code, and it counts in the `warnings` field of `summary`.


### Compatibility check in the command line's own CI

The rule below is for the maintainers of the command line, so that the promises above hold in every release. It describes target behaviour.

- **[DEV-891]** The command line project MUST keep a recorded snapshot of every `--json` record shape, `type` value and exit code of its previous release, and its own CI MUST fail when a build changes any of them without a matching deprecation or breaking entry under [DEV-863].

### Inputs and outputs

- Input: a release of the command line, its release notes, and the pages of the command reference.
- Output on use of a deprecated item: one `warning` record in JSONL, or one line on standard error in text, as [DEV-859] states. Example:

```json
{"type":"warning","code":"deprecated","message":"Flag --old-name is deprecated and stops working in release 0.4.0; use --new-name."}
```

(The release number and the flag are illustrative. The example shows the shape, not a real deprecation.)

### Failure behaviour

- An item that has passed its stop release is no longer part of the contract. A flag that no longer exists fails as any unknown flag does: exit code 2, before any work starts ([DEV-010]).
- A deprecated item behaves exactly as before. The only change is the notice, and the exit code is not affected ([DEV-864]).
- An exit code is never reused with a new meaning ([DEV-861]). A new code, such as 7 and 8 from [Distribution commands](/pages/reference-plugin-distribution-cli/#exit-codes), is added to the table.

### Limits

- Gap between marking release and stop release: at least two minor releases by default (see the first decision below). [ABI-052] sets the same two-minor minimum for the plugin contract.
- Notices per invocation: one per deprecated item ([DEV-859]). No page sets a cap on the number of distinct items per invocation.

### Examples

A script reads the `result` record and the exit code:

```sh
ilium plugin check --json . | tail -n 1   # last line is the result record
test $? -eq 0
```

A script reacts to a deprecation by reading `warning` records whose `code` is `deprecated`, and fails its own lint step, not the build, so the team changes the call before the stop release.

### Decisions needed

Decision needed: how many minor releases must pass between the marking release and the release in which a deprecated item stops working. Default: two minor releases, the same gap that the plugin contract uses for its own deprecations (see [ABI evolution](/pages/reference-abi-evolution/)). Scripts and CI pipelines need at least one full upgrade cycle to move, and a shorter gap would break a pipeline on a single upgrade.

Decision needed: whether these rules apply before the contract reaches a stable 1.0. Default: yes, from the first release this page covers, with no exception for a 0.x version. The plugin contract is already named in the manifest and in the built component, so scripts that read its output already depend on it.

Decision needed: where the release notes live and in what form (file in the repository, release page, or both). Default: a `CHANGELOG.md` entry per release, which the project already keeps in Keep a Changelog form, with the entries of [DEV-863] under Changed and Removed. No page settles this.

### What this page does not cover

- The commands, flags, records and exit codes themselves: [Plugin command-line reference](/pages/reference-plugin-cli/).
- Versioning of the plugin contract and of the WIT packages: [ABI evolution](/pages/reference-abi-evolution/) and [Versioning](/pages/versioning/).
- Deprecation of SDK names: [SDK deprecation](/pages/reference-sdk-deprecation/).
- Text output, which is not a contract ([DEV-028]).

### What to do as a script author

- Read the release notes before you upgrade the command line.
- Treat a `deprecated` warning as a change to make before the release that it names.
- Ignore fields and enumerated values that you do not know.
- Check exit codes by number, and never by the text of a message.

## Production additions: T9-compat-deprecation

- **[DEV-998]** A CLI flag, field or record type marked deprecated under DEV-858 MUST keep working for at least two minor releases (proposed, owner to confirm) of the contract version, matching API-083 and ABI-052.


## Production additions: release notes

The rules below settle where the release notes of the command line live, which sections they use and when they are published. They adopt the default given in the decisions above; the owner confirms or replaces that default.

- **[DEV-950]** The release notes of each release of the command line MUST be the `CHANGELOG.md` entry of that release, under a heading `## <version> (<YYYY-MM-DD>)` whose date is the release date. The entry MUST use only the Keep a Changelog sections `Added`, `Changed`, `Removed`, `Fixed` and `Security`. Deprecations under [DEV-863] go under `Changed`, and breaking removals go under `Removed`. (proposed, owner to confirm)

- **[DEV-951]** The `CHANGELOG.md` entry MUST be committed in the same commit as the version change, and the release tag MUST NOT be created before that commit exists. The release pipeline MUST fail with no release artifact produced when the tag has no heading for its version. (proposed, owner to confirm)

- **[DEV-952]** The release notes MUST be published together with the release artifacts, never after them. A release page MAY repeat the notes, but the `CHANGELOG.md` entry is the record that a check reads. (proposed, owner to confirm)

- **[DEV-953]** A release that fixes a vulnerability in the command line MUST list the fix under `Security`, with the advisory identifier of [SEC-322] and the CVE identifier, when one is assigned ([SEC-702]). That entry MUST be published with the advisory and MUST NOT name the flaw before the advisory is public ([EXT-070]). (proposed, owner to confirm)

- **[DEV-954]** The release pipeline MUST fail when a `Removed` entry names an item that no earlier `CHANGELOG.md` entry marked deprecated with that item's stop release equal to the release being built. (proposed, owner to confirm)

## See also

- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [ABI evolution](/pages/reference-abi-evolution/)
- [Versioning](/pages/versioning/)
- [CI recipes](/pages/guide-ci-recipes/)
