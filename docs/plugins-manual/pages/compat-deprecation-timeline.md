---
layout: page
title: Deprecation timeline and the window
permalink: /pages/compat-deprecation-timeline/
---

## Deprecation timeline and the window

A deprecated item keeps working until its removal minor. This page shows how the two-minor notice and the two-minor window of the host combine, so an author can work out which packages a removal breaks and when they must move. It adds the calendar that the rules on the owner pages do not spell out. The rules themselves stay on [SDK deprecation](/pages/reference-sdk-deprecation/) and [ABI evolution](/pages/reference-abi-evolution/#notice-and-removal).

## You will need

- [SDK deprecation](/pages/reference-sdk-deprecation/) for the marks, the changelog headings and the lint rule.
- [Load and refusal decisions](/pages/compat-load-decision-table/) for the window and the refusal codes.
- [SDK migration](/pages/guide-sdk-migration/) for the migration notes and the `upgrade-sdk` command.

## The rules in one place

- An item is marked deprecated in the minor `M`, the marking release. The marking release ships the successor too ([API-085]).
- The removal minor `R` must be at least `M + 2` ([ABI-052], [API-083]). A patch release never removes anything ([API-086]).
- A marked feature keeps its behaviour until `R` ([ABI-053]).
- The host window at minor `N` holds the minors `N - 1` and `N`, so a package built for minor `P` loads on hosts `P` and `P + 1` only ([ABI evolution, section 4](/pages/reference-abi-evolution/#4-the-window-and-the-refusal-codes)).
- **[CMP-000]** The removal minor of a marked item MUST be released at least 90 days after the release date of the marking minor, in addition to the two-minor rule of [ABI-052], and the release gate MUST block a removal whose marking release is younger than that (proposed, owner to confirm).

From these rules follows one fact that matters for planning: a removal at `R = M + 2` breaks only the packages built for `M + 1` that still use the item. A package built for `M` is already refused on host `M + 2`, because its minor has left the window. A package built for `M + 2` cannot use the item, because the removal already happened in its SDK.

- **[CMP-010]** The removal minor MUST be stated in the marking release notes, together with the successor, so that an author can compute the window of every affected package from the notes alone.

## Worked timeline

The example uses an item called `services.legacy-call`. It is marked deprecated in minor `0.2.0`, with the successor `services.call`. Its removal minor is `0.4.0`, the earliest minor the rule allows.

| Host | Window | Item `services.legacy-call` | Package built for `0.1.0` | Package built for `0.2.0` | Package built for `0.3.0` |
|---|---|---|---|---|---|
| `0.2.0` (marking) | `0.1`, `0.2` | Works, marked deprecated | Loads through the `0.1` adapter | Loads, with a `lint.sdk-deprecated` warning at build | Not built yet |
| `0.3.0` | `0.2`, `0.3` | Works, still marked | Refused, `package.api-unsupported` | Loads, the item still works | Loads, if it still uses the item |
| `0.4.0` (removal) | `0.3`, `0.4` | Removed | Refused, `package.api-unsupported` | Refused, `package.api-unsupported` | Refused by the removal check if it still uses the item; loads if it moved to `services.call` |

Read the table column by column. The `0.1.0` package is refused at `0.3.0`, which is before the removal, so the removal itself never reaches it. The `0.2.0` package is refused at the removal minor and would have been refused at `0.4.0` even without the removal. The `0.3.0` package is the only one that the removal can break, and it is the one that an author has to move in the `0.3.0` release.

- **[CMP-011]** A package built for `M + 1` that still uses an item removed at `M + 2` MUST be refused at install with a code of the ABI refusal table, and the removal MUST NOT change the code of any other package.

The code for that refusal is one of the codes of section 4 of [ABI evolution](/pages/reference-abi-evolution/#4-the-window-and-the-refusal-codes), chosen by the check that fails. Which code applies to a removed SDK item that a package still imports is not fixed on the owner page yet (proposed, owner to confirm: `manifest.import-not-requested`, because the removed item no longer appears in the component's requested imports).

## What an author sees in each release

| Release | What the author sees | Where it is written |
|---|---|---|
| Marking, `M` | A changelog entry under `Deprecated` that names the item, the successor and the removal minor. Rust builds show a deprecation warning. TypeScript editors show a mark. | Changelog of the SDK |
| Between `M` and `R` | `ilium plugin check` reports each use as `lint.sdk-deprecated`, a warning | [Check and lint rules](/pages/reference-check-and-lint-rules/) |
| Removal, `R` | The same finding becomes an error, and the build fails with `build.sdk-host-mismatch` when the manifest `api` is outside the host range | [SDK deprecation](/pages/reference-sdk-deprecation/) |

The host writes no record for an SDK deprecation ([API-087]). A `deprecated` record exists only for a WIT feature, once per plugin and feature at load time ([ABI-053]). Authors who look for warnings at run time will not find any for an SDK item, so they must read the build output.

## Unstable features follow a shorter rule

An unstable feature has no notice period of its own in the contract, because it may change in any release. The one rule that applies is [ABI-083]: a release that removes or graduates an unstable feature names it in its release notes one minor in advance, and the host writes one `deprecated` record per plugin that still lists the feature.

| Feature | Marked in | Removed or graduated in | Notice that the authors get |
|---|---|---|---|
| Any of `ui-calendar`, `plugin-services`, `gpu-kernels`, `websocket` | Release `N` notes | Minor `N + 1` at the earliest | One minor in advance, plus the `deprecated` record |

Graduation moves a feature from `unstable` to stable. A plugin that still lists the old name keeps loading after a graduation, because the manifest entry remains valid for one minor. Proposed, owner to confirm: after graduation the `unstable` entry is ignored with a `deprecated` record, and it becomes `manifest.unstable-unknown` only on the minor that removes the name.

- **[CMP-012]** A graduation or removal of an unstable feature MUST be named in the release notes one minor before it takes effect, and the host MUST write a `deprecated` record for each plugin that still lists the feature in that minor.

## Edge cases

- A deprecated item whose successor is missing has no timeline. The note must say so, and the item must not be removed on the usual two-minor clock ([API-085]).
- Proposed, owner to confirm: a patch release that changes the behaviour of a deprecated item without removing it names the item under `Changed` in its changelog.
- Proposed, owner to confirm: a marking release that is later withdrawn does not shorten the notice. The removal minor stays as stated, because authors may already have rebuilt against it.
- Two items marked in the same minor with different removal minors each keep their own calendar. The window does not change by item.
- A plugin that lists `requires` on another plugin's deprecated service is not covered by the SDK rules. The other plugin's contract and its major version decide that case ([AUT-213]).

- **[CMP-013]** A removal MUST NOT happen on a patch release and MUST NOT change the behaviour of any item other than the removed one ([API-086]).

## Example release note

A release note for the marking minor is short. It names the item, the successor, the removal minor and the affected window.

```
### Deprecated
- `services.legacy-call` is deprecated. Use `services.call`.
  Removal minor: 0.4.0. Packages built for 0.3 that still use it are refused on host 0.4.
```

- **[CMP-014]** A marking release note MUST contain the item name, the successor name, the removal minor and the packages it breaks, in that order, so that a reader can check the timeline without the source.

## Deprecation register

Each host and SDK release publishes a deprecation register. It is the one list of what is deprecated now, with the successor and the removal minor of each item, so that a plugin author or an operator can plan a removal without reading each changelog. The register is a page of this manual and a JSON file with the same rows. Each row gives the item, its kind (`sdk`, `cli`, `contract` or `unstable-feature`), its successor, its removal minor and the release that marked it. The `ilium plugin deprecations --json` command ([DEV-969](/pages/guide-deprecating-plugin-features/)) lists the rows that apply to the installed version.

- **[CMP-024]** Each host and SDK release MUST publish a deprecation register, as a page of the manual and as a JSON file with the same content, listing every item deprecated at that release with its kind (`sdk`, `cli`, `contract` or `unstable-feature`), its successor, its removal minor and the release that marked it.
- **[CMP-025]** The register MUST drop an item only in the release that removes it, and the release notes of that release MUST name the item exactly as the register does.

## Test

- Build a package for `M + 1` that uses the item, install it on host `R` and check that the refusal code is the one of the ABI table and that the package is not partly installed.
- Build a package for `M + 2` and check that it does not import the item.
- Run `ilium plugin check` on a package for `M` and check that each use is listed once as `lint.sdk-deprecated`.

## See also

- [SDK deprecation](/pages/reference-sdk-deprecation/) for the marks and the rules for the successor
- [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) for the rules the author follows when raising a version
- [Backwards-compatibility test matrix](/pages/guide-backwards-compatibility-test-matrix/) for the cell that runs a package for `M + 1` on host `R`
- [Host upgrade runbook](/pages/compat-host-upgrade-runbook/) for the operator side when a removal refuses a package
- [Two majors during a transition](/pages/api-services/#two-majors-during-a-transition) for a service name that keeps an old major for its window

## Open decisions

- The refusal code for a removed SDK item that a package still imports (see the proposal above).
- The fate of an unstable `unstable` entry after graduation (proposed, owner to confirm).

API version checked: `ilium:plugin@0.1.0`.
