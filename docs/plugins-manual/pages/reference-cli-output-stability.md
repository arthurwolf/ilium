---
layout: page
title: CLI output stability
permalink: /pages/reference-cli-output-stability/
---

## CLI output stability

This page states which parts of the plugin command line are a contract, how a change to them is announced, and how a breaking change is versioned. Scripts and CI pipelines that parse `--json` records or test exit codes depend on those names and numbers, so they stay stable across releases. The rules apply to every command in [Plugin command-line reference](/pages/reference-plugin-cli/).

Text output is outside this contract. A script that reads text output gets no protection from this page, and it MUST use `--json` instead.

Requirement ids of this page are in the range `DEV-858` to `DEV-861`. The complete list is in [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/).

### What is covered

The contract has three parts:

- the `type` values of the JSONL records, and the fields that the [record catalogue](/pages/reference-plugin-cli/#the-record-catalogue) defines for each type;
- the exit codes in the [exit code table](/pages/reference-plugin-cli/#exit-codes);
- the flag names, and the meaning of each flag in the command reference.

Adding a field, a record type or a flag is not a breaking change. A consumer MUST ignore fields that it does not know, as the record rules of the command-line reference already require.

### Rules

- **[DEV-858]** A flag, field or record type that a release removes or renames MUST first be marked deprecated in the release notes. The marking release MUST name the release in which it stops working. Until that release, the item MUST keep working as before.

- **[DEV-859]** While a flag, field or record type is marked deprecated, every command that uses it MUST print a notice. In JSONL output the notice is one `warning` record with the code `deprecated`. In text output it is one line on standard error. The notice MUST name the item and the release in which it stops working, and it is printed once per invocation.

- **[DEV-860]** A breaking change MUST happen only in a release whose contract version has a new minor number, and MUST be listed in that release's notes. A breaking change is a removal or rename that [DEV-858] governs, or a change in the type or meaning of a field.

- **[DEV-861]** The number and the meaning of an exit code MUST NOT change in any release. A new exit code MUST be added to the exit code table and listed in the release notes, and no existing code MUST be given a new meaning.

### Decisions needed

Decision needed: how many minor releases must pass between the marking release and the release in which a deprecated item stops working. Default: two minor releases, the same gap that the plugin contract uses for its own deprecations (see [ABI evolution](/pages/reference-abi-evolution/)). Scripts and CI pipelines need at least one full upgrade cycle to move, and a shorter gap would break a pipeline on a single upgrade.

Decision needed: whether these rules apply before the contract reaches a stable 1.0. Default: yes, from the first release this page covers, with no exception for a 0.x version. The plugin contract is already named in the manifest and in the built component, so scripts that read its output already depend on it.

### What to do as a script author

- Read the release notes before you upgrade the command line.
- Treat a `deprecated` warning as a change to make before the release that it names.
- Check exit codes by number, and never by the text of a message.

## See also

- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [ABI evolution](/pages/reference-abi-evolution/)
- [Versioning](/pages/versioning/)
- [CI recipes](/pages/guide-ci-recipes/)
