---
layout: page
title: ABI major versions
permalink: /pages/reference-abi-major-versions/
---

## ABI major versions

This page states what happens when the plugin contract moves from the `0.x` line to `1.0.0`. The rules on the [ABI evolution](/pages/reference-abi-evolution/) page apply inside each major line: a minor release adds only, keeps a window of the current minor and the previous one, and refuses what falls outside it. A major release is the one point where a change may break the rules of that page. This page says which 0.x minors stay loadable across that point, how a package is refused across it, and what a major release may change.

### Terms

- **Major boundary.** The release that moves the first number of `api` (the contract version `ilium:plugin@MAJOR.MINOR.PATCH`) from `0` to `1`.
- **Final 0.x minor.** The highest `0.N` minor released before `1.0.0`. The number is not fixed on this page; it is whatever the last 0.x release is.
- **Frozen copy and adapter.** The kept `wit/since_v0_N/` copy of a minor and the hand-written adapter the host reads it through, as defined on [ABI evolution](/pages/reference-abi-evolution/#4-the-window-and-the-refusal-codes).
- **Removal major.** The major release in which a marked feature or the retained 0.x minor stops working.

### What you need

- **The ABI evolution rules.** The window, the refusal codes and the removal notice are defined on [ABI evolution](/pages/reference-abi-evolution/). This page refers to them and does not repeat them.
- **The versioning rules for authors.** [Versioning](/pages/versioning/) explains the `api` key and the `ilium:api-version` section from the author side.

## 1. The version written at a major boundary

The three places that carry the contract version are the same as in every minor release: the manifest key `api`, the custom section `ilium:api-version` and the `@` suffix of the WIT package names. At `1.0.0` the three places hold `ilium:plugin@1.0.0`. The custom section of a `1.0.0` component is the three big-endian `u16` values `1`, `0` and `0`, so its bytes are `00 01 00 00 00 00`. A component whose section differs from its manifest `api` is refused with `manifest.api-version`, the same as in a minor release.

A component built for `1.0.0` and a component built for `0.x` are never the same binary. A plugin author rebuilds the component with the SDK of the new major to target `1.0.0`.

- [ABI-059] A component built for `ilium:plugin@1.0.0` MUST carry the custom section `ilium:api-version` with bytes `00 01 00 00 00 00`, and a section that differs from the manifest `api` MUST be refused with `manifest.api-version`.

## 2. The 0.x minor that stays loadable

A host that ships `1.0.0` keeps the final `0.x` minor loadable for a limited time. The host reads that minor through its frozen copy and an adapter, the same way it reads the previous minor inside the window.

- [ABI-055] A host that ships `ilium:plugin@1.0.0` MUST load a package whose `api` is the final `0.x` minor through that minor's frozen copy and adapter, and MUST NOT load a package whose `api` is any other `0.x` minor.

Decision needed: how long the final `0.x` minor stays loadable after `1.0.0` is released. The default I would use is until the next major release, under the same window rule that applies to minors, so a plugin author has one full major cycle to rebuild. The owner should confirm this period before the `1.0.0` release is announced.

Inputs are the `api` key of the manifest and the `ilium:api-version` section of the component. The output of the check is either a loaded package or one refusal code.

## 3. Refusal across the major boundary

A package whose `api` names a major version that the host does not load is refused at install, at stage 3, with `package.api-unsupported`. The refusal uses the same table and the same order as a minor refusal, so the host reports one code for each package.

- [ABI-056] A package whose `api` major differs from the host's major version and whose `api` is not the one retained under [ABI-055] MUST be refused with `package.api-unsupported`, and the message MUST name the `api` of the package and the versions the host supports.

A `1.0.0` package on a `0.x` host is refused with `package.api-unsupported` in the same way, because `1.0.0` is outside the window of every `0.x` host. A `0.x` package that is not the final minor is refused on a `1.0.0` host with the same code.

| Package `api` | Host `1.0.0` | Host `0.x` |
|---|---|---|
| `ilium:plugin@1.0.0` | loads | `package.api-unsupported` |
| final `0.x` minor | loads, one `deprecated` record | loads if inside the host window |
| other `0.x` minor | `package.api-unsupported` | loads if inside the host window |
| not a well-formed string | `manifest.schema` | `manifest.schema` |
| section differs from `api` | `manifest.api-version` | `manifest.api-version` |

Failure behaviour: the host reports the first failing check in the order of the table on [ABI evolution](/pages/reference-abi-evolution/#4-the-window-and-the-refusal-codes), so one package gets one code. A refused install changes nothing on disk.

## 4. Deprecation of the 0.x line

The retained `0.x` minor is a deprecated feature line. The host reports that state at load time, with the same record that a marked feature uses.

- [ABI-057] The host MUST log one `deprecated` record per plugin when it loads a package whose `api` is the final `0.x` minor, and the record MUST name the major version in which that `0.x` minor stops loading.

## 5. What a major release may change

A minor release may only add. A major release is the only release that may change the contract in ways a minor release may not. The table on [ABI evolution](/pages/reference-abi-evolution/) lists the fixed parts; a major release may change them only under the rules below.

- [ABI-058] A major release MUST NOT lower a limit or remove a feature unless the change was marked in an earlier release under [ABI-052] and [ABI-053], and the change MUST take effect in that major release only.

A limit may be lowered only in a major version. A removal follows the same notice rule as a minor removal, with the removal minor replaced by the removal major. The affected package refusals move to the codes of [ABI evolution](/pages/reference-abi-evolution/#4-the-window-and-the-refusal-codes) under [ABI-054].

Decision needed: how much notice a feature removal needs before a major release. The default I would use is the two-minor notice of [ABI-052], which means the feature is marked in a minor release at least two minors before the major release that removes it. A shorter notice would leave plugin authors less time to rebuild, so the default is the longer one.

## 6. Inside the 1.x line

After the boundary the rules of [ABI evolution](/pages/reference-abi-evolution/) apply again to `1.N` minors: a minor only adds, and the host loads the current minor and the previous one.

- [ABI-060] Inside the 1.x line a host MUST apply the window rule of ABI evolution (current minor and previous one), and a package whose `api` major differs from the host's major MUST NOT be loaded through that window.

## Limits

- Retained 0.x minors on a `1.0.0` host: exactly one (the final minor) [ABI-055].
- `deprecated` records: one per plugin per load [ABI-057].
- Notice for a removal: at least two minors before the removal major, as the default of the decision above; the number is not settled until the owner confirms it.
- A patch change moves no interface and never affects these rules.

## Example

A host at `1.0.0` whose final 0.x minor is `0.3.0` (illustrative numbers):

- `old.plugin` with `api = "ilium:plugin@0.3.0"` loads and logs one `deprecated` record naming the removal major.
- `older.plugin` with `api = "ilium:plugin@0.2.0"` is refused with `package.api-unsupported`; the message names `ilium:plugin@0.2.0` and the supported versions.
- `new.plugin` with `api = "ilium:plugin@1.0.0"` and section bytes `00 01 00 00 00 00` loads.

## 7. What an author sees

- A plugin built for `1.0.0` loads on a `1.0.0` host.
- A plugin built for the final `0.x` minor loads on a `1.0.0` host, logs one `deprecated` record at load, and is refused after the removal major.
- A plugin built for any other `0.x` minor is refused with `package.api-unsupported` on a `1.0.0` host.

## What this page does not cover

- The window, refusal table and removal notice inside one major line: [ABI evolution](/pages/reference-abi-evolution/).
- How authors write and check the `api` key: [Versioning](/pages/versioning/).
- Upgrade steps for plugin authors and operators: [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/).
- The full list of rules and tests: [Requirements for WIT and ABI](/pages/reference-requirements-wit-abi/).

## See also

- [ABI evolution](/pages/reference-abi-evolution/)
- [Versioning](/pages/versioning/)
- [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/)
- [Requirements for WIT and ABI](/pages/reference-requirements-wit-abi/)
