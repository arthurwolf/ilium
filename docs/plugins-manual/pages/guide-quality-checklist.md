---
layout: page
title: Quality checklist
permalink: /pages/guide-quality-checklist/
---

## Quality checklist

This guide is the review list an author runs before each release. Every item says what to check, which command checks it, and what no command can check. The rule codes are those of [Check and lint rules](/pages/reference-check-and-lint-rules/). Requirement ids of this page are in the range `AUT-400` to `AUT-499`.

### How to use the list

Work top to bottom. Sections 1 to 6 are run by a machine wherever a row names a command or test; a row marked `Review` in those sections is a manual check. Section 6 (Behaviour tests) is run with `test`. The last two sections need a person. A release is ready when every item is checked or has a written reason in `lint.toml` or in the release notes.

A release MUST have no `check` error and no unsuppressed `lint` error. [AUT-401]

A release SHOULD pass `ilium plugin lint --deny warnings`, and every suppression SHOULD carry a reason that a reviewer can read. [AUT-402]

## 1. Package and manifest

| Item | Command | Rule |
|---|---|---|
| Manifest is valid and complete | `ilium plugin check` | `manifest.*` codes |
| Components exist for every declared side | `check` | `component.missing`, `component.invalid` |
| Exports match the manifest | `check` | `manifest.exports-mismatch` |
| SDK version matches `api` | `check` | `manifest.api-version` |
| `description` is one paragraph of 280 characters or less (hard limit 500) | `lint` | `lint.description-missing` |
| `license` is an SPDX expression and `LICENSE` exists | `lint` | `lint.license-missing`, `lint.license-file-missing` |
| `unstable` is empty | `lint` | `lint.unstable-feature` |
| Package is below 100 MiB | `lint` | `lint.package-size-near-cap` |
| Release build, no debug information | `lint` | `lint.debug-info-in-release` |

A plugin MUST NOT list an `unstable` feature in a release unless the plugin is for development use, because the shape of that feature can change without a major version. [AUT-403]

## 2. Grants and privacy

| Item | Command | Rule |
|---|---|---|
| No grant without a use | `lint` | `lint.grant-unused` |
| Every grant is explained in the README | `lint` | `lint.grant-unexplained` |
| High-risk grants are needed | `lint` | `lint.high-risk-grants` |
| The plugin works when each optional grant is refused | Test | none |
| No secret or whole payload in a log line | Review | none |

Every capability a plugin requests MUST be exercised by a tested call path, no unused grant may remain in the manifest, and every grant MUST be explained in the README with the feature that needs it. [AUT-404]

A plugin MUST handle `not-permitted` for every call of an optional grant and MUST keep working with a reduced feature set. [AUT-405]

## 3. Resource use

| Item | Command | Rule |
|---|---|---|
| Limits are inside the tier ceiling | `lint` | `lint.limit-above-ceiling` |
| No `startup` activation without need | `lint` | `lint.activation-startup` |
| Contribution lists are not near their cap | `lint` | `lint.contribution-near-cap` |
| Handlers stay below deadlines | `dev --timings`, `stats` | none |
| Render stays below 10 ms for the largest realistic view | `profile` | none |
| Memory stays below 80 percent of `memory_mib` | `stats` | none |

A plugin SHOULD keep every measured use below 80 percent of its limit on the fixture data set named in the README (one stated size per limit, such as the largest list length or buffer size the plugin documents), because the limit is a trap, not a warning. See [Performance](/pages/performance/). [AUT-406]

## 4. Views and keys

| Item | Command | Rule |
|---|---|---|
| Every interactive widget has a `key` | `lint` | `lint.widget-missing-key` |
| Every declared view renders with an empty fixture | `lint` | `lint.render-trap` |
| Dedup keys use the allowed characters | `lint` | `lint.dedup-key-charset` |
| Icon names exist | `lint` | `lint.icon-name-unknown` |

A plugin MUST give every interactive widget a stable `key`. [AUT-407]

## 5. Documentation

| Item | Command | Rule |
|---|---|---|
| `README.md` and `CHANGELOG.md` exist | `lint` | `lint.readme-missing`, `lint.changelog-missing` |
| Help topic for every command, pane kind, panel and settings page | `lint` | `lint.help-topic-missing`, `lint.settings-help-missing` |
| Help titles equal the manifest titles | `lint` | `lint.help-title-mismatch` |
| Help links resolve | `lint` | `lint.help-link-broken` |
| Topics are 20 KiB or less | `lint` | `lint.help-topic-too-large` |
| Extension points are documented | `lint` | `lint.extension-point-undocumented` |

See [Plugin documentation guide](/pages/guide-plugin-documentation/).

## 6. Behaviour (tests)

No lint rule can tell whether a plugin is correct. These tests are the minimum.

| Test | What it proves |
|---|---|
| Activate and deactivate | Both finish inside their deadlines and leave no state behind |
| Every command with empty and with large input | No trap, no timeout |
| Every `err` code the plugin can receive | The user sees a sentence, not a crash |
| Restart (`restart_with`) | State comes back from restore bytes or storage |
| Whole batch of events | Handlers do not assume one event per call |
| Upgrade from the previous release | `migrator` keeps old data. See [Data migration](/pages/guide-data-migration/). |

A plugin MUST have at least one test for activation, one for deactivation and one for each error code that its code handles. [AUT-408]

A plugin that changes the layout of stored data MUST test the upgrade from the previous release with a storage snapshot of that release. [AUT-409]

## 7. Accessibility and appearance

No command checks these. Use [Accessibility and theming](/pages/accessibility-and-theming/).

| Item | Check |
|---|---|
| Colour roles only | No hard-coded colours. Use the nine colour roles. |
| Meaning without colour | Every status has a word or a glyph, not only a colour |
| Reduced motion | Animation stops or slows when the host asks |
| Density | Layout works in compact and comfortable density |
| Icon mode | Every icon has a text fallback |
| Width | A view is usable at 40 columns |
| Wide text | Widths use display width, not byte or character counts. See [Internationalisation](/pages/guide-internationalisation/). |

A plugin MUST NOT use colour as the only carrier of meaning. [AUT-410]

## 8. Support and release

| Item | Check |
|---|---|
| Version raised, changelog entry written | Manual |
| New grants and `data_version` named in the release notes | Manual |
| Package passes `ilium plugin verify` | Command |
| Package installed once on a clean profile, log read | Manual |
| Key kept offline, package signed last | Manual |
| Contact address in the README | Manual |

See [Publishing and support](/pages/guide-publishing-and-support/).

## What the tools cannot check

| Gap | What to do |
|---|---|
| Correct behaviour | Tests (section 6) |
| Good wording of messages | Read them aloud |
| Accessibility | Section 7 by hand |
| Performance on a real session | `profile` and `stats` on a large project |
| Interaction with other plugins | Install the most common ones together |

## Suppressing a rule

A suppression lives in `lint.toml` with a rule, an optional `key_path` and a reason. A suppression without a reason is an error (`lint.suppression-reason-missing`). One that matches nothing is a warning (`lint.suppression-unused`). Prefer fixing over suppressing.

## Edge cases

| Situation | Result |
|---|---|
| No fixtures | Fixture-based rules print `lint.skipped` |
| Rule not known to an old CLI | `lint.suppression-unknown` on the suppression |
| Warning is wrong for the plugin | Suppress it with a reason |
| `check` passes, install fails | Look at the install code index in [Errors and limits reference](/pages/reference-errors-and-limits/) |

## See also

- [Check and lint rules](/pages/reference-check-and-lint-rules/)
- [Testing plugins](/pages/testing/)
- [Performance](/pages/performance/)
- [Concurrency for plugin authors](/pages/concurrency-for-plugin-authors/) for deadlines, slow work and job state
- [Security](/pages/security/)
- [CI recipes](/pages/guide-ci-recipes/) to run the list on every commit
- [Troubleshooting](/pages/guide-troubleshooting/)

API version checked: `ilium:plugin@0.1.0`.
