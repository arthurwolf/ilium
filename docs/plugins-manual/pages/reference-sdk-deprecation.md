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

### Who acts

- SDK maintainers add the mark, name the successor and write the changelog entry in the release that marks the item.
- Plugin authors act on the build warning or the editor mark. They move to the successor before the removal minor, because the item stops compiling after it.
- Operators take no action. An SDK deprecation changes only what plugin builds report, so there is nothing to configure on a host.

### See also

- [ABI evolution](/pages/reference-abi-evolution/)
- [Rust SDK](/pages/rust-sdk/)
- [TypeScript SDK](/pages/typescript-sdk/)
- [Versioning](/pages/versioning/)

Requirement ids for this page: [API-082], [API-083], [API-084]. See the [requirements index](/pages/reference-requirements/).
