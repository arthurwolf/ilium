---
layout: page
title: Deprecating plugin features
permalink: /pages/guide-deprecating-plugin-features/
---

## Deprecating plugin features

This guide is for plugin authors who retire one of their own commands, settings or exported interfaces. It defines the notice period, the manifest marking and the migration warning in `check`. The rules for the command line itself are in [CLI output stability](/pages/reference-cli-output-stability/). This page describes target behaviour; it does not claim the behaviour exists today.

Requirement ids of this page are `DEV-885` to `DEV-887` and `DEV-969` to `DEV-972`. The complete list is in [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/).

### Rules

- **[DEV-885]** A plugin MUST mark a command, a setting or an exported interface as deprecated in `manifest.json` with a `deprecated` object that holds `since` (a plugin version), `remove_in` (a later plugin version) and `message`. `remove_in` MUST be at least two minor versions after `since`, or `check` fails with `lint.deprecation-too-short`.

- **[DEV-886]** The host MUST show the `message` of a deprecated command or setting to the user at first use in a session and MUST write a `warn` log line naming the plugin and the item. A caller of a deprecated exported interface MUST get the same `warn` line once per session per caller. Deprecation MUST NOT change the behaviour of the item.

- **[DEV-887]** `ilium plugin check` MUST report `lint.deprecated-removed` as an error when the current version is at or past `remove_in` and the item is still declared, and `lint.deprecated-used` as a warning when the plugin itself, or a dependency it declares, uses a deprecated item of another plugin. The warning MUST name the item and the `remove_in` version.

- **[DEV-969]** `ilium plugin deprecations [--json]` (proposed, owner to confirm) MUST list every host feature, SDK item flag and installed plugin item currently marked deprecated, with fields `kind`, `name`, `since`, `remove_in` and `owner`, and MUST exit 0 when the list is empty. Test: list with one deprecated item of each kind and assert every row; list with none and assert exit 0.

- **[DEV-970]** The `message` of a `deprecated` object MUST be 1 to 280 UTF-8 bytes (proposed, owner to confirm) without control characters and MUST be rendered as plain text; a violation MUST be refused with `manifest.deprecated-invalid`. Test: a message of 281 bytes and a message with a control character are each refused with that code.

- **[DEV-971]** The host MUST show a deprecation message at most once per plugin item per session and MUST write at most one `warn` log line per item per minute thereafter, counting suppressed lines in a `suppressed` field of the next line. Test: call a deprecated item 100 times within one minute and assert one `warn` line, then assert the next line after a minute has `suppressed` set to 99.

- **[DEV-972]** After the plugin version reaches `remove_in`, the host MUST NOT keep a still-declared item active: a load of such a package MUST be refused with `manifest.deprecated-expired`, naming the item. Test: load a package at its `remove_in` version that still declares the item, and assert the refusal.

### Failure behaviour

Each rule names its failure code. A failure prints a `diagnostic` record in JSONL or one line on standard error in text, and the process exit code follows [Plugin command-line reference](/pages/reference-plugin-cli/#exit-codes).

## See also

- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/)

API checked against `ilium:plugin@0.1.0`.
