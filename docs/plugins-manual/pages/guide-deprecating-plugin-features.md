---
layout: page
title: Deprecating plugin features
permalink: /pages/guide-deprecating-plugin-features/
---

## Deprecating plugin features

This guide is for plugin authors who retire one of their own commands, settings or exported interfaces. It defines the notice period, the manifest marking and the migration warning in `check`. The rules for the command line itself are in [CLI output stability](/pages/reference-cli-output-stability/). This page describes target behaviour; it does not claim the behaviour exists today.

Requirement ids of this page are in the range `DEV-885` to `DEV-887`. The complete list is in [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/).

### Rules

- **[DEV-885]** A plugin MUST mark a command, a setting or an exported interface as deprecated in `manifest.json` with a `deprecated` object that holds `since` (a plugin version), `remove_in` (a later plugin version) and `message`. `remove_in` MUST be at least two minor versions after `since`, or `check` fails with `lint.deprecation-too-short`.

- **[DEV-886]** The host MUST show the `message` of a deprecated command or setting to the user at first use in a session and MUST write a `warn` log line naming the plugin and the item. A caller of a deprecated exported interface MUST get the same `warn` line once per session per caller. Deprecation MUST NOT change the behaviour of the item.

- **[DEV-887]** `ilium plugin check` MUST report `lint.deprecated-removed` as an error when the current version is at or past `remove_in` and the item is still declared, and `lint.deprecated-used` as a warning when the plugin itself, or a dependency it declares, uses a deprecated item of another plugin. The warning MUST name the item and the `remove_in` version.

### Failure behaviour

Each rule names its failure code. A failure prints a `diagnostic` record in JSONL or one line on standard error in text, and the process exit code follows [Plugin command-line reference](/pages/reference-plugin-cli/#exit-codes).

## See also

- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/)

API checked against `ilium:plugin@0.1.0`.
