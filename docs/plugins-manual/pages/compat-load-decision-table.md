---
layout: page
title: Load and refusal decisions across host versions
permalink: /pages/compat-load-decision-table/
---

## Load and refusal decisions across host versions

A host decides, for each package and for each installed plugin, whether the package loads, is refused at install, or stays installed but inactive. This page gives the full decision order, the state an installed plugin moves to when a host upgrade leaves its `api` outside the window, and worked cases for three host versions. The codes themselves are defined on the owner pages linked below; this page shows the order in which they are checked and what the user sees at each step.

## You will need

- [Versioning](/pages/versioning/) for the `api` string, the `ilium:api-version` section and the marker rules.
- [ABI evolution](/pages/reference-abi-evolution/#4-the-window-and-the-refusal-codes) for the window and the first refusal table.
- [Install, update and uninstall](/pages/reference-install-and-update/) for the install dialog and the update path.

## Terms

- **Host version** `H`: the contract version the running Ilium ships, for example `ilium:plugin@0.2.0`.
- **Window**: the current minor and the previous minor of `H`, counted by minor number only. At `0.1.0` the window holds `0.1` alone, because there is no previous minor. At `0.2.x` it holds `0.1` and `0.2`. At `0.3.x` it holds `0.2` and `0.3`.
- **Package `api`**: the value of the `api` key in the manifest of a package.
- **Section version**: the value of the `ilium:api-version` custom section of each component, written as three big-endian `u16` values.

## The decision order

The host runs the checks below in this order and stops at the first failure. A package gets one code, never two. The first five rows are the ABI table of [ABI evolution](/pages/reference-abi-evolution/#4-the-window-and-the-refusal-codes), and the rest are the checks of [Versioning](/pages/versioning/#unstable-opt-in-checks) and of the package format.

| Step | Check | Code on failure | Stage |
|---|---|---|---|
| 1 | The `api` value is a well-formed `ilium:plugin@X.Y.Z` string | `manifest.schema` | manifest |
| 2 | The `api` value falls inside the window | `package.api-unsupported` | install, stage 3 |
| 3 | The section version of every component equals the `api` value, patch number included | `manifest.api-version` | install and load |
| 4 | Every name in `unstable` is an unstable feature of `H` | `manifest.unstable-unknown` | install |
| 5 | A T2 or T3 package that lists an unstable feature is installed in development mode | `manifest.unstable-tier` | install |
| 6 | Every unstable item the component imports has its feature in `unstable` | `manifest.unstable-missing` | install |
| 7 | Every grant in the manifest exists in the `api` version | `manifest.grant-unknown` | install |
| 8 | Every import of the component is unlocked by a requested capability | `manifest.import-not-requested` | install |
| 9 | The package version is not lower than the installed version, or `--allow-downgrade` is given | `package.downgrade` | install |

Steps 4 to 6 come after the ABI checks because they only make sense for a package whose contract version the host reads at all. The order between steps 4 and 9 is a decision of this page (proposed, owner to confirm), because the owner pages name the codes but not their relative order.

- **[CMP-001]** The host MUST run the checks of the decision order in the order of the table, and it MUST report the code of the first failing check only.
- **[CMP-002]** A refusal MUST leave the package out of the store and MUST leave the installed version, if any, active.

## Step 3 with patch numbers

The section version is compared with the `api` value exactly, including the patch number. A package built with `ilium:plugin@0.1.3` in its `api` key and `0.1.3` in its section passes step 3 on a host that carries the `0.1` snapshot. The host has one frozen snapshot per released minor, so it has to decide which patch value it accepts for that snapshot. The manual does not state this rule yet.

- Proposed, owner to confirm: a host accepts a package whose `api` names a patch value of a released minor inside the window only when that patch value is the one the host ships for that minor. Any other patch value fails at step 3 with `manifest.api-version`, even inside the window.

- **[CMP-003]** The host MUST refuse a package whose section version differs from its `api` value in any part, with `manifest.api-version`, before any code of the component loads.

## State of an installed plugin after a host upgrade

An installed plugin keeps its files, grants, settings and data when the host changes. Only its ability to load depends on the window, so an upgrade moves it between two states and never deletes it.

| State after the upgrade | When it applies | Files, grants, settings and data | What the user sees |
|---|---|---|---|
| Active | `api` is inside the new window and steps 3 to 8 pass | Unchanged | The plugin row shows its normal state |
| Refused, api outside window | `api` falls outside the new window (step 2) | Kept on disk, unchanged | The Plugins list and `ilium plugin list --json` show the refusal code for the plugin |
| Refused, section mismatch | The component section differs from `api` (step 3) | Kept on disk, unchanged | Same as above, with `manifest.api-version` |

- **[CMP-004]** A host upgrade that refuses an installed plugin MUST NOT uninstall it, change its files, its grants, its settings or its stored data.
- **[CMP-005]** An installed plugin refused after a host upgrade MUST load again, without any change to its files, when the host that accepts its `api` runs again. The rollback of the host is the way back; see [Host upgrade runbook](/pages/compat-host-upgrade-runbook/).

The refused state stays until the host is changed or the plugin is replaced. The manual does not yet say when the host re-runs the checks.

- Proposed, owner to confirm: the host re-runs steps 2 and 3 at each start, never in the background, and it writes one audit line per refused plugin per start.

## Worked cases

Three host versions are used below: `0.1.0`, `0.2.0` and `0.3.0`. The window of `0.2.0` is `0.1` and `0.2`.

| Package `api` | Host `0.1.0` | Host `0.2.0` | Host `0.3.0` |
|---|---|---|---|
| `ilium:plugin@0.1.0` | Loads | Loads through the `0.1` adapter | Refused, `package.api-unsupported` |
| `ilium:plugin@0.2.0` | Refused, `package.api-unsupported` | Loads | Loads |
| `ilium:plugin@0.3.0` | Refused, `package.api-unsupported` | Refused, `package.api-unsupported` | Loads |

A `0.1` package on a `0.2.0` host is inside the window and loads through the adapter. On a `0.3.0` host the same package falls outside the window, so it is refused. The `0.2` window is also the reason a `0.2` package is still accepted on `0.3.0`.

- **[CMP-006]** A package whose `api` is inside the window of the host MUST load through the adapter of its minor, and it MUST behave as it did on the host that released that minor for every item the adapter maps.

### Example: an install refused at the window edge

A user runs the install of a plugin built for `0.1.0` on a host at `0.3.0`.

<pre class="term"><span class="t-prompt">$</span> ilium plugin install notes-caller-0.1.0.iliumpkg
<span class="t-err">error</span> package.api-unsupported
<span class="t-dim">      Plugin notes-caller 0.1.0 needs a plugin API</span>
<span class="t-dim">      that this Ilium does not support.</span>
<span class="t-dim">      This Ilium supports</span>
<span class="t-dim">      ilium:plugin@0.2.0 and ilium:plugin@0.3.0.</span>
</pre>

Illustrative, not generated. The message names both ends of the window, as [Versioning](/pages/versioning/) requires. For the exit status of a refused install, see [CLI output stability](/pages/reference-cli-output-stability/).

- **[CMP-007]** A `package.api-unsupported` message MUST name the lowest and the highest contract version of the current window.

### Example: a plugin built for a newer minor than the host

A plugin built for `ilium:plugin@0.3.0` is installed on a host at `0.2.0`. Step 2 fails because `0.3` is above the window. The fix is on the author side: rebuild against the SDK for `0.2` or install a newer Ilium. The host does not guess at a downgrade of the `api` value.

## Edge cases

- A package with an `api` value of a minor that the host dropped long ago and a section that matches it fails at step 2. The section is never read first.
- A package that lists an unstable feature which the host renamed fails at step 4 with `manifest.unstable-unknown`, even when its `api` is inside the window.
- A package whose `api` is inside the window but which uses an item marked `@since` a newer minor fails at step 7 or in the import check at step 8, depending on whether the item is a grant or an interface.
- A dev-mode install of a T2 or T3 package follows the same order. Step 5 passes in development mode and fails outside it.

- **[CMP-008]** A package that fails at one step MUST NOT be partly installed: the store MUST hold no file of it after the refusal.

## Errors

| Code | Meaning | Action for the user |
|---|---|---|
| `manifest.schema` | The `api` value is not a contract version | Rebuild the package with a current SDK |
| `package.api-unsupported` | The `api` value is outside the window | Install a host in the window, or ask the author for a rebuild |
| `manifest.api-version` | The section and the `api` value differ | Rebuild the package; a mismatch is never a runtime fix |
| `manifest.unstable-unknown` | The unstable name is no longer known | Read the changelog and update the manifest |
| `manifest.unstable-tier` | A T2 or T3 package with an unstable feature outside development mode | Use `ilium plugin dev` or ask for a stable build |
| `manifest.unstable-missing` | The component imports an unstable item without the manifest entry | Add the feature to `unstable` in the author project |

- **[CMP-009]** Each refusal in the table above MUST have a line in the audit log that carries the code, the plugin id and the `api` value of the package.

## See also

- [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) for the author side of a host upgrade
- [Host upgrade runbook](/pages/compat-host-upgrade-runbook/) for the operator procedure after a refusal
- [Deprecation timeline](/pages/compat-deprecation-timeline/) for how removals line up with the window
- [Backwards-compatibility test matrix](/pages/guide-backwards-compatibility-test-matrix/) for the cells that test each row of the decision order

## Open decisions

- Patch values inside the window: whether the host ships one patch value per minor only (see Step 3 with patch numbers).
- Retry cadence of a refused plugin at start (proposed, owner to confirm, see the state section).
- Relative order of the unstable checks (steps 4 to 6) and the grant checks (step 7). Proposed, owner to confirm.

API version checked: `ilium:plugin@0.1.0`.
