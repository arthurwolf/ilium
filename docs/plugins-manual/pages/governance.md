---
layout: page
title: Governance
permalink: /pages/governance/
---

## Governance

This page states who owns Ilium, under which licence it ships, who may do what, how the project behaves towards people and how its name may be used. Other pages cite "a maintainer with release permission" or "the owner"; this page defines those terms. Decisions about the design are recorded on [Design decisions](/pages/design-decisions/). Security reports follow [SECURITY.md](https://github.com/arthurwolf/ilium/blob/master/SECURITY.md) and the [security response](/pages/reference-security-response/) page.

### You will need

Nothing to install or grant. Read the licence section before you ship a plugin or fork, the roles section before you ask who can approve a change, and the name section before you publish a registry, a fork or a plugin that mentions Ilium.

## Licence of Ilium, its SDKs and its core plugins

Everything the Ilium repository ships is under one licence, the MIT licence, SPDX identifier `MIT`. The copyright line reads "Copyright (c) 2026 Arthur Wolf". The workspace manifest (`Cargo.toml`, `[workspace.package]`) sets `license = "MIT"` and every crate inherits it with `license.workspace = true`.

| Part | Licence (SPDX) | Where the text lives |
|---|---|---|
| Ilium host, the `ilium` command, `ilium-server` and `ilium-client` binaries | `MIT` | `LICENSE` at the repository root |
| The 30 core plugins under `plugins/core` | `MIT` | the same `LICENSE`; each core manifest carries `license = "MIT"` |
| Rust `ilium-plugin-sdk` crate (with `ilium-plugin-sdk-core` and `ilium-plugin-sdk-macros`) | `MIT` | the same `LICENSE`; the crate metadata inherits it |
| `@ilium/plugin-sdk` package | `MIT` | `LICENSE` inside the published package; its `package.json` declares the same SPDX id |
| WIT contract files under `wit/` | `MIT` | the same `LICENSE`; the files are generated from [reference-wit](/pages/reference-wit/) |

Every release archive contains `LICENSE` and `THIRD-PARTY-NOTICES` next to the binaries, so an installed copy always carries its own licence text. `ilium --version --verbose` prints the path of the notices file and reports `notices: missing` when it is absent ([SEC-423]).

**What this means for a plugin author.** The licence of the SDK does not restrict the licence of your plugin. You may link the SDK or generate bindings from the WIT files and then publish your plugin as open source under any licence, or as proprietary software, or keep it private. The only condition the MIT licence puts on you is that a copy of Ilium or of the SDK source you redistribute keeps the copyright line and permission notice. A built plugin component contains SDK code, so a package that bundles the SDK keeps that notice in its own `LICENSE` or notices file. `ilium plugin new --license <spdx-id>` writes your chosen `LICENSE` and sets the manifest `license` field; an id that is not an SPDX identifier fails with `new.license-unknown` before any file is written ([DEV-935]).

**Dependencies.** The licence of Ilium does not replace the licence of what it links. `THIRD-PARTY-NOTICES` lists every third-party crate in the shipped binaries and `.cwasm` components with its exact version, SPDX expression and full text ([SEC-420]). The list must equal the packages of `Cargo.lock` that reach a shipped target, and a difference fails the release check ([SEC-421]). A dependency whose licence is outside the repository allow list, or that declares none, fails the same check ([SEC-422]). The allow list holds permissive licences and the `Apache-2.0 WITH LLVM-exception` expression that Wasmtime uses. The notices file therefore describes dependencies only, and `LICENSE` describes Ilium itself.

**Failure modes.** A release whose archive lacks `LICENSE` or `THIRD-PARTY-NOTICES` fails the release check and is not published. A user who finds an installed copy without the notices file sees `notices: missing` and should reinstall from a verified archive ([Host release verification](/pages/reference-host-release-verification/)).

## Maintainers, roles and decision records

The manual uses four roles. A person can hold several.

| Role | May | May not |
|---|---|---|
| Maintainer with release permission | Cut a host release, set a rollout percentage, pause a release by setting it to 0 ([PKG-958]), hold one custodian share of the release key | Change a signed registry document or sign for the registry |
| Registry operator | Publish and re-sign registry metadata, mark a namespace abandoned and reclaim it ([PKG-703]), announce a root key change out of band ([PKG-692]), update the status page ([AUT-952]) | Sign a host release |
| Security team | Rate and answer reports, agree embargoes, sign and publish advisories, request an emergency disable | Publish a release alone |
| Owner of a requirement prefix | Confirm rows marked "proposed, owner to confirm", approve a change to a contract in that prefix | Merge a change without the review a maintainer gives |

**Gaining and losing a role.** A role is granted by the existing holders of that role, by agreement of at least two of them, and recorded as a decision (below). A role ends when the holder resigns, when two other holders remove it for inactivity of 12 months (proposed, owner to confirm) or for a breach of the code of conduct, or at once when a key of that holder is reported compromised. When a role ends, the holder's repository permissions and key shares are revoked in the same change, and the custodians follow [Release key compromise](/pages/reference-release-key-compromise/) or [Registry key compromise](/pages/reference-registry-key-compromise/) when a key is involved.

**Decision records.** A decision that changes a contract, a limit, a grant or a role is written as a numbered entry on [Design decisions](/pages/design-decisions/) (`D<n>`). The entry gives the date, the question, the alternatives considered, the decision, the reason and the effect on plugin authors, and names every requirement row and manual page it changes. The next free number is used; a number is never reused. A later entry may supersede an earlier one, and the earlier entry then says which. A decision that is not on that page is not a decision of the project.

**Continuity.** Every release key and every registry root key has at least two holders, and the release key is signed with a threshold of 2 of 3 custodians ([SEC-728]). Root keys stay offline ([PKG-690]). If a maintainer leaves, the remaining holders rotate any share the person held before the next release; a release is not cut while only one holder remains. If the project can no longer ship releases, it publishes a public notice on the repository, the status page and the release channel at least 90 days (proposed, owner to confirm) before the last release. The last release stays downloadable, its signatures stay verifiable ([SEC-428]), and installed copies keep working.

## Code of conduct and moderation

One code of conduct applies to the repository, the issue tracker, pull requests, the registry and any chat or forum the project runs.

**Expected.** Be respectful, assume good faith, give specific and kind review feedback, and credit other people's work.

**Prohibited.** Harassment, threats, discrimination, personal attacks, publishing another person's private information, sustained disruption of discussions, and impersonating a maintainer or the project.

**Reporting.** Send a report to the conduct address `conduct@ilium.example` (a placeholder until the project publishes a real address, as for the security contact in [SECURITY.md](https://github.com/arthurwolf/ilium/blob/master/SECURITY.md)). The report is confidential. It is read by at least two people who are not the subject of the report. The reporter receives an acknowledgement within 3 working days (proposed, owner to confirm) and a decision within 30 days.

**Actions.** In increasing order: a private warning, a public warning, a temporary ban of 1 to 12 months, a permanent ban, and, for abuse through a plugin or a namespace, removal of the namespace from the registry. Removal is signed into the registry metadata and announced in the transparency log, and the host then disables the plugin the way [PKG-711] describes, keeping its data. A reclaimed or removed id follows [PKG-703].

**Registry moderation.** Abusive plugin content (hateful text, harassment, doxxing) is reported to the same address, handled by the same people, and recorded in the same moderation record. Malware and security abuse go to the security team instead.

**Appeal.** A person who is warned or banned may appeal within 30 days to a maintainer who took no part in the decision. The appeal result is final and is recorded. The record is kept privately, and a public log lists only the action type and date, never the reporter.

## Use of the Ilium name and logo

The MIT licence covers the code, not the name or logo. The reserved id prefixes `ilium.`, `core.`, `local.`, `test.` and `example.` are refused at publish with `registry.id-reserved` ([PKG-387]). No key can be trusted for the publisher ids `ilium`, `core`, `local` or `test` ([PKG-220]), and a package starting with `ilium.` is refused with `manifest.id-reserved` even when signed ([PKG-234]). The rules below add the use of the name in text and artwork.

- A plugin, tool or registry may say "for Ilium" or "compatible with Ilium". It must not present itself as Ilium, use the logo as its own icon, or claim endorsement.
- A fork that changes behaviour must use another name and state its own `ilium:api-version`, so a plugin can tell which contract it runs under.
- A private or mirror registry may not call itself the official registry. It shows its own name and URL, and a client pins it by its own root key.
- A release not built and signed by the project is not called an official Ilium release.
- For any other use, such as a book, a talk or a product name, ask at the project address given on the repository page before you use the name or logo.

## See also

[Design decisions](/pages/design-decisions/), [Security response](/pages/reference-security-response/), [Registry](/pages/reference-registry/), [Signing and trust](/pages/reference-signing-and-trust/), [Host release verification](/pages/reference-host-release-verification/).
