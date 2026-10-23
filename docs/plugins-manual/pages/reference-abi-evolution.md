---
layout: page
title: ABI evolution
permalink: /pages/reference-abi-evolution/
---

## ABI evolution

This page states how the plugin contract `ilium:plugin@0.1.0` may change after its first freeze, how a component records the version it was built for, and how the host decides whether it can load a component. The rules protect the binary layout of the contract (the canonical ABI), not only its names. The [WIT reference](/pages/reference-wit/) is the contract. [Versioning](/pages/versioning/) explains the author side. The [requirements page](/pages/reference-requirements-wit-abi/) lists each rule with its id and its test.

### What you need

- **The WIT reference.** Case names and orders on this page refer to the [WIT reference](/pages/reference-wit/).
- **A built component.** The `ilium:api-version` section is written by the SDK. You do not write it by hand. To inspect one, run `ilium plugin inspect <package>`.

## 1. What is versioned

There is exactly one contract version: `ilium:plugin@0.1.0`. It is an umbrella identity. It is not a WIT package. The WIT packages `ilium:types`, `ilium:host`, `ilium:primitives`, `ilium:exports`, `ilium:server`, `ilium:client` and `ilium:anim` all carry the version `@0.1.0` and change together [ABI-003].

The version appears in three places, and the host requires that they agree:

| Place | Form | Checked |
|---|---|---|
| Manifest key `api` | the string `ilium:plugin@0.1.0` | at install, stage 3 |
| Custom section `ilium:api-version` of each component | three big-endian `u16` values: major, minor, patch | at install and at load |
| Package names in the WIT imports of the component | `@0.1.0` | at link time |

The bytes of the custom section for `0.1.0` are `00 00 00 01 00 00`: major 0, minor 1, patch 0 [ABI-004]. A component with no section, with a section of another length, or with values that differ from the manifest is refused with `manifest.api-version`.

- [DEV-968] Each host release MUST publish the SHA-256 of every WIT package it ships in the release notes and in `ilium version --json` under `wit_hashes`, and `ilium plugin inspect` MUST report the same hashes for the component's embedded world.

## 2. What may change

After the first freeze of 0.1.0, a later version may only **add**. The canonical ABI makes the following parts of the contract binary-significant, so a change to one of them is a break:

| Part | Rule |
|---|---|
| Case order of a `variant`, `enum` or `flags` | Fixed. A case is only appended at the end [ABI-019]. |
| Fields of a `record` | Fixed. A field is never added, removed, reordered or retyped. A new optional value goes into a new record or a new function. |
| Parameters and results of a function | Fixed. A new capability is a new function. |
| Function and type names | Fixed. A name is never reused for another meaning. |
| Interface and world names | Fixed. A new interface is added beside the old one. |
| Required exports | `lifecycle` and `handler` (server and client), `lifecycle` and `renderer` (scene) never change. |
| Limits | A limit may be raised. It is lowered only in a major version. |

Why records are fixed: a record is flattened into a sequence of fields. A host that reads a field that an older plugin did not write would read the wrong bytes. Appending to a `variant` is safe, because an older plugin never produces the new case. Delivering a new case to an older plugin is not safe, so the host filters (section 5).

**What counts as "shape" in 0.1.0.** Several records carry optional fields in 0.1.0 itself, for example `ui-node`, `popup-anchor`, `send-input-spec`, `set-decoration-spec`, `rename-node-spec`, `notify-spec`, `set-virtual-rows-spec`, `choice-option` and `menu-entry`. They are part of the first freeze. They are listed as "shape" items in [Versioning](/pages/versioning/) and they are not open to later change.

- [DEV-967] Two host patch releases of the same minor MUST export byte-identical WIT packages; the release pipeline MUST fail when the WIT text hash of a patch release differs from that of its minor's first release.

## 3. Markers and unstable features

Every item of the contract has one of two markers:

| Marker | Meaning |
|---|---|
| `@since(version = 0.1.0)` | The item exists from that version and keeps its shape in later minor versions. In the WIT reference a stable item carries no marker, because every item of 0.1.0 has this one. |
| `@unstable(feature = <name>)` | The item may change or disappear inside 0.x. Only a plugin whose manifest `unstable` key lists the name may use it. |

Version 0.1.0 has **four** unstable features: `ui-calendar`, `plugin-services`, `gpu-kernels` and `websocket` [ABI-022]. In the WIT listing an unstable item has a `// @unstable(feature = <name>)` comment on the line before it. A comment is used because a gate on a single variant case is not part of the WIT text format.

| Feature | Items it gates |
|---|---|
| `ui-calendar` | the `calendar` widget and `calendar-spec` |
| `plugin-services` | `services.call` between plugins, and the grants `services.use:<name>` and `services.provide:<name>` |
| `gpu-kernels` | `gpu.run`, `gpu.available` and the grant `gpu.kernels` |
| `websocket` | `http.open-websocket` and `http.send-websocket` |

The manifest checks run before any code loads:

| Code | When it fails |
|---|---|
| `manifest.unstable-unknown` | A name in `unstable` is not a feature of this host. |
| `manifest.unstable-tier` | A T2 or T3 package lists a feature outside development mode. |
| `manifest.unstable-missing` | The component imports an unstable item and the manifest does not list the feature. |

- [DEV-975] An unstable feature MUST either graduate or be removed within 6 minor releases (proposed, owner to confirm) of its introduction; the release notes of the sixth minor MUST name the decision, and a feature past the bound MUST be flagged by the release checklist as blocking.
- [DEV-977] `ilium plugin list --json` MUST include for each plugin the array `unstable_used` of feature names it lists, so an operator can find every plugin affected by a planned graduation or removal.
- [DEV-978] The host MUST refuse, with `manifest.unstable-tier`, a plugin that lists an unstable feature when installed from a registry flagged `production`, regardless of tier.

## 4. The window and the refusal codes

The host loads the **current minor version and the previous one**. The window counts minor numbers. Each released minor is kept as a frozen copy under `wit/since_v0_N/`, and the host reads the older minor through a hand-written adapter.

| Situation | Code | Stage |
|---|---|---|
| `api` is not a well-formed `ilium:plugin@X.Y.Z` string | `manifest.schema` (see [Manifest reference](/pages/reference-manifest/)) | manifest |
| `api` is well formed and outside the window | `package.api-unsupported` | install, stage 3 |
| `api` is inside the window, but the component section differs from it | `manifest.api-version` | install and load |
| The component imports an interface that no requested capability unlocks | `manifest.import-not-requested` | install |
| A manifest lists a grant that its API version does not have | `manifest.grant-unknown` | install |

There is one refusal for each situation. A single package never gets two codes: the host reports the first failing check in the order of the table [ABI-021].

<figure>
<pre class="term">
╭  ≡ ● · Install check─────────────────────────────╮
│ old-titler 1.2.0 ……… api ilium:plugin@0.1.0  ✓   │
│ future-x 1.0.0 …… api ilium:plugin@0.3.0  ✕      │
│   package.api-unsupported: host supports 0.1.0   │
╰──────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A plugin built for a newer minor than the window is refused.</figcaption>
</figure>

### Notice and removal

A host API, a WIT interface, a grant or a manifest field is never removed in a minor release that is still inside the window. Authors get warning before the window moves.

- [ABI-052] A feature that a release marks for removal MUST be listed in that release's notes with the minor version in which it stops working, and that minor MUST be at least two minors after the marking release.
- [ABI-053] A marked feature MUST keep working until the minor in its notice, and the host MUST log a `deprecated` record once per plugin and feature at load time, naming the feature and the removal minor.
- [ABI-054] Removing a feature MUST also move the affected package refusals to the codes of section 4, and the removal MUST NOT change any other code.

## 5. What an older plugin sees from a newer host

A host of minor N+1 runs plugins built for N. To keep them correct, the host adapts what it sends:

- **Events.** The host does not deliver an event case that is newer than the plugin's API. The plugin never sees it, and it is not counted in `events-dropped`.
- **Outcomes.** A newer outcome case is mapped to the nearest older one: `ok-epoch(n)` would be delivered to a plugin of an earlier API as `ok`.
- **Intents.** A plugin cannot send a newer case, because its bindings do not have it.
- **Reasons.** A newer `deactivation-reason` is delivered as `shutdown`.

[ABI-029] A host MUST NOT deliver an event, outcome or reason case to a plugin whose API version predates the case. It maps the case as described above, or it does not deliver it.

An SDK enum is non-exhaustive, so a plugin compiled for N still compiles when N+1 bindings add a case.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;

fn describe(outcome: &Outcome) -> &'static str {
    match outcome {
        Outcome::Ok => "applied",
        Outcome::OkNode(_) => "created a node",
        Outcome::Duplicate(_) => "already done",
        // A case that a later API adds lands here.
        _ => "other",
    }
}
```

**TypeScript**

```ts
import type { Outcome } from '@ilium/plugin-sdk';

export function describe(outcome: Outcome): string {
  switch (outcome.tag) {
    case 'ok': return 'applied';
    case 'ok-node': return 'created a node';
    case 'duplicate': return 'already done';
    default: return 'other'; // A case that a later API adds lands here.
  }
}
```

- [ABI-119] The host MUST provide the import `host-info` returning the exact `api-version`, host build id, enabled unstable features and every limit in effect, and its result MUST NOT change during one activation.

## 6. A change in practice

Suppose a later minor version, called N+1 here, adds an intent `pin-node(node-id)`. The authors do these steps:

1. Append `pin-node(node-id)` as case 43 of `intent`, after `dismiss-toast`. Never insert it in the middle.
2. Add the case to the frozen snapshot of N+1 and keep `wit/since_v0_1/` as it is.
3. Write the adapter that maps the 0.1 view of the host to the N+1 view.
4. Add the grant `tree.write` to the row of the intent in the [intent table](/pages/reference-events-and-intents/).
5. Update the case counts on the pages that state them, and the [requirements page](/pages/reference-requirements-wit-abi/).
6. Run the contract check: a diff of the case order of every `variant`, `enum` and `flags` type against `wit/since_v0_1/` must show only appended cases.

Results for the three kinds of plugin:

| Plugin | Host N+1 |
|---|---|
| Built for 0.1.0, installed on N+1 | Loads. It never sends `pin-node`. |
| Built for N+1, installed on N+1 | Loads. It may send `pin-node`. |
| Built for N+1, installed on 0.1.0 | Refused with `package.api-unsupported`. |

## 7. The cases that the first freeze fixes

The counts below are the sizes of the main variants and enums in 0.1.0. A later minor can only make them larger. The extension counts are subsets of those totals, not additions introduced in a later API version: the frozen WIT and case-order snapshot already include them in 0.1.0, including `ok-epoch`. Future additions require a separately recorded introduction version.

| Type | Cases in 0.1.0 | Included extension cases in the same freeze |
|---|---|---|
| `event` | 56 | 18 |
| `intent` | 42 | 23 |
| `outcome` | 7 | 1 (`ok-epoch`) |
| `widget` | 51 | 0 |
| `action-kind` | 17 | 0 |
| `action-value` | 12 | 0 |
| `deactivation-reason` | 6 | 0, with `crash-restart` reserved |
| `error-code` | 15 | 0 |

The event catalogue shows 35 rows for the base events, because three rows each describe two cases (`editor-opened` and `editor-closed`, `key-pressed` and `key-released`, `client-attached` and `client-detached`). The count of cases is 38 for the base events [ABI-002].

## 8. Edge cases

- **A patch release.** A patch version changes no interface, and the window counts minors only. A component built for `0.1.0` loads on a host at `0.1.1`: the `api` string is well formed and its minor is inside the window, so stage 3 accepts it, and the host uses the adapter of that minor. The string in the manifest and the `ilium:api-version` section of the component must still equal each other in all three fields (otherwise `manifest.api-version`), so a package states the patch it was built for.
- **A reserved case.** `crash-restart` exists in `deactivation-reason` so that the numbering never changes. The host never delivers it [ABI-028].
- **A removed feature.** An unstable feature may be removed. A package that lists it fails with `manifest.unstable-unknown` after the upgrade.
- **Names kept by users.** Pane kind ids, command ids, settings keys, decoration slots and topic names are stored in user snapshots and keep their spelling for ever.


## Cross-version interop matrix

Target behaviour of the production plugin system.

- [ABI-064] The project MUST publish and run a matrix of host minor against SDK minor, covering at least the old plugin on the new host and the new SDK on the old host, for every minor inside the window.
- [ABI-065] Each matrix cell MUST state its expected result: loads, loads with a `deprecated` record, or refused with the code of section 4.
- [ABI-066] A release MUST NOT ship while any matrix cell inside the window differs from its stated result.

## See also

- [WIT reference](/pages/reference-wit/)
- [Versioning](/pages/versioning/)
- [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/)
- [ABI major versions](/pages/reference-abi-major-versions/): the 0.x to 1.0.0 transition
- [Runtime model](/pages/reference-runtime-model/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [SDK deprecation](/pages/reference-sdk-deprecation/): the rules for SDK names that are not WIT features


### Graduating unstable features

- [ABI-083] A release that removes or graduates an unstable feature MUST name it in its release notes one minor version in advance; the host MUST write one `deprecated` record per plugin that still lists the feature, and after graduation a manifest that still lists it under `unstable` MUST load with that `deprecated` record instead of being refused with `manifest.unstable-unknown`.
