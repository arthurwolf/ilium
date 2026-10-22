---
layout: page
title: CLI network use and privacy
permalink: /pages/reference-cli-network-and-privacy/
---

## CLI network use and privacy

This page defines what the plugin command line may send over the network when it builds, publishes and updates, how to turn it off and what the offline mode guarantees. This page describes target behaviour; it does not claim the behaviour exists today.

Requirement ids of this page are in the range `DEV-894` to `DEV-895`. The complete list is in [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/).

### Rules

- **[DEV-894]** During `build`, `check`, `test` and `pack` the command line MUST make no network connection. During `publish`, `install` and `update` it MUST connect only to the registry named in the command or in the configuration, and MUST send only the package and the request fields that the registry protocol defines, with no analytics, usage data, project paths, user names or machine identifiers.

- **[DEV-895]** `--offline` and the environment variable `ILIUM_OFFLINE=1` MUST make every command that needs the network fail with exit code 1 and `net.offline` before any connection is attempted, and MUST make all other commands behave as without the flag. The command line MUST NOT include any telemetry that needs an opt-out; if it is ever added it MUST be off by default and enabled only by an explicit setting.

### Failure behaviour

Each rule names its failure code. A failure prints a `diagnostic` record in JSONL or one line on standard error in text, and the process exit code follows [Plugin command-line reference](/pages/reference-plugin-cli/#exit-codes).

## See also

- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/)

API checked against `ilium:plugin@0.1.0`.
