---
layout: page
title: SDK deprecation
permalink: /pages/reference-sdk-deprecation/
---

## SDK deprecation

This page states how the Rust SDK (`ilium-plugin-sdk`) and the TypeScript SDK (`@ilium/plugin-sdk`) retire a function, type, builder or wrapper: how the item is marked, how long it keeps working, which minor removes it and how the SDK changelog announces it. The features of the contract `ilium:plugin@0.1.0` are covered by [ABI evolution](/pages/reference-abi-evolution/). This page covers only the names that the SDKs add on top of the contract.

### What an author sees

An SDK deprecation shows at build time, not at load time. The SDK code is part of the plugin build, so the host never sees a deprecated SDK item and writes no `deprecated` record for it.

- **Rust:** a use of a deprecated item is a warning in the standard `rustc` output during the build.
- **TypeScript:** the editor shows the `@deprecated` tag, usually as a struck-through name. The TypeScript compiler does not fail on the tag.
- **Changelog:** the SDK changelog names the item, its successor and its removal minor.

After the removal minor, the item is gone. A plugin that still names it fails to compile (Rust) or to import (TypeScript) against that SDK version.

Decision needed: whether `ilium plugin build` should print its own summary of deprecated SDK items, on top of the `rustc` warnings and the editor marks. Default: no extra output, because the compiler and the editor already report the mark and a second report would repeat it. The project owner decides, since the choice changes build output that authors read.

### Marking an item

- [API-082] An SDK item that a release marks for removal MUST carry the Rust `#[deprecated(since = "<sdk version>", note = "...")]` attribute or the TypeScript `@deprecated` JSDoc tag in that same release, on the item itself. The note MUST name the successor and the removal minor, and an item with no successor MUST say so in the note. A changelog entry alone MUST NOT mark an item.

### Keeping a deprecated item working

- [API-083] A deprecated SDK item MUST keep its signature and its behaviour until the removal minor named in its mark. A patch release MUST NOT remove it, and a minor release MUST NOT remove it before two minors after the release that marked it.

Decision needed: whether the two-minor minimum should match the rule for WIT features in [ABI-052](/pages/reference-abi-evolution/#notice-and-removal), or use a different count. Default: two minors, so an SDK item and a WIT feature give authors the same warning time. The SDK has its own version number, separate from the contract (see [SDK versioning policy](/pages/rust-sdk/#sdk-versioning-policy)), so the project owner should confirm that the count is in SDK minors.

### Announcing in the changelog

- [API-084] Each SDK release MUST list every item it marks under a `Deprecated` heading of its changelog entry, and every item it removes under a `Removed` heading. Each entry MUST name the item, its successor and its removal minor. The headings use the Keep a Changelog section names.

### Definitions

- **SDK item:** a public function, type, builder, wrapper or export of `ilium-plugin-sdk` or `@ilium/plugin-sdk`. A WIT feature of the contract is not an SDK item.
- **Marking release:** the SDK release that adds the mark to an item.
- **Removal minor:** the SDK minor release named in the mark, in which the item no longer exists.
- **Successor:** the SDK item an author uses instead.

### Rules for the successor and the host

- [API-085] A release that marks an item MUST already ship its successor, so that an author can move in the same release. An item with no successor MUST say so in its note instead.
- [API-086] The release that removes an item MUST be the minor named in its mark, MUST NOT be a patch release, and MUST NOT change the signature or behaviour of any other item.
- [API-087] The host MUST NOT write a `deprecated` record, a log line or a refusal for an SDK deprecation. Only the `deprecated` record of [ABI-053](/pages/reference-abi-evolution/#notice-and-removal) for WIT features exists at load time.

### Inputs, outputs and failures

- Input: the SDK source of a release, with its marks and its changelog entry.
- Output: build warnings (Rust), editor marks (TypeScript) and the changelog headings `Deprecated` and `Removed`.
- Failure: a plugin that names a removed item fails to compile (Rust) or to import (TypeScript). The host error codes of the manual do not apply, because the failure happens before a package exists. A package built against an SDK that needs another contract version is refused with `manifest.api-version` (see [SDK versioning policy](/pages/rust-sdk/#sdk-versioning-policy)).

### Limits

- Minimum notice: two SDK minors between the marking release and the removal minor ([API-083]).
- Patch releases remove nothing ([API-083]).
- The manual states no other numeric limit for SDK deprecation.

### Example

A Rust item marked in SDK 0.4.0 for removal in 0.6.0:

```rust
#[deprecated(since = "0.4.0", note = "use `pane_open`; removed in 0.6.0")]
pub fn open_pane_legacy() { /* unchanged behaviour */ }
```

The matching changelog entry of 0.4.0:

```
### Deprecated
- `open_pane_legacy`: use `pane_open`; removed in 0.6.0.
```

The entry of 0.6.0 repeats the item under `### Removed`. The names here are illustrative, not items of the real SDK.

### What this page does not cover

- Retiring a WIT feature of the contract: see [ABI evolution](/pages/reference-abi-evolution/).
- SDK version numbers and the contract version: see [Versioning](/pages/versioning/).
- Host error codes: see [Error codes and limits](/pages/reference-errors-and-limits/).

### Who acts

- SDK maintainers add the mark, name the successor and write the changelog entry in the release that marks the item.
- Plugin authors act on the build warning or the editor mark. They move to the successor before the removal minor, because the item stops compiling after it.
- Operators take no action. An SDK deprecation changes only what plugin builds report, so there is nothing to configure on a host.

### See also

- [ABI evolution](/pages/reference-abi-evolution/)
- [Rust SDK](/pages/rust-sdk/)
- [TypeScript SDK](/pages/typescript-sdk/)
- [Versioning](/pages/versioning/)

Requirement ids for this page: [API-082], [API-083], [API-084], [API-085], [API-086], [API-087]. See the [requirements index](/pages/reference-requirements/).
