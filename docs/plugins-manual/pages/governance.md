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

A plugin may change its licence in a later version. The rules for that change, including the consent of contributors and what installed copies do, are on [Changing the licence of a plugin](/pages/guide-publishing-and-support/#changing-the-licence-of-a-plugin).

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

**Registry moderation.** Abusive plugin content (hateful text, harassment, doxxing) is reported to the same address, handled by the same people, and recorded in the same moderation record. Malware and security abuse go to the security team instead. Copyright, trademark and patent claims by a rights holder follow [Rights-holder claims](/pages/publish-rights-claims/). Court orders and requests from government bodies follow [Legal requests](/pages/publish-legal-requests/). The content classes that users can hide are defined in [Registry policy](/pages/reference-registry/#content-classes).

**Appeal.** A person who is warned or banned may appeal within 30 days to a maintainer who took no part in the decision. The appeal result is final and is recorded. The record is kept privately, and a public log lists only the action type and date, never the reporter.

## Export control and sanctions

The registry accepts uploads from anyone who can create an account, and the Ilium host carries TLS and cryptographic code. Both facts bring legal obligations that depend on where the operator and the users are. This section states what the registry screens, how a refusal reads to the person refused, what the operator records, and what the project says about the encryption in its software. It is a statement of the project's rules, not legal advice. Counsel reviews it before the registry opens to publishers in a new jurisdiction.

### You will need

The sanctions lists the operator is bound by, written into `policy.json`, and the current export classification statement of the project. Read the account rules of [Registry operator guide](/pages/publish-registry-operator-guide/) before you change how accounts are created.

### Screening accounts

The operator screens each new publisher account when it is created. The operator screens it again before issuing a new token to an existing account, because a list can change between two uses. A match stops the account or the token. The requester gets a refusal with the code `registry.account-refused` and no token.

<div class="note">
<p>Proposed, owner to confirm: the screen uses the account holder's name and the declared address. It does not read the content of a plugin, and it does not run on downloads by users who never publish.</p>
</div>

**[PUB-061]** The registry operator MUST screen every new publisher account against the sanctions lists that bind the operator, at account creation and before the operator issues a new token to an existing account. The operator MUST refuse a match with `registry.account-refused` and MUST NOT issue a token to it (proposed, owner to confirm: the lists are named in `policy.json`).

**[PUB-062]** A refusal on sanctions grounds MUST NOT name the list, the matched entry or the country to the requester. The requester MUST see only `registry.account-refused` and the claims contact of `policy.json`, so that the operator can answer a mistaken match (proposed, owner to confirm).

**[PUB-063]** The operator MUST keep the screening result and the date of each screen in the operator's private record. The registry MUST NOT publish the result in `revocations.json`, `policy.json`, the transparency log or any plugin page (proposed, owner to confirm).

### Encryption classification

The Ilium host uses TLS for its registry and mirror connections, and it ships cryptographic code in its sandbox and its client. Export rules for such code depend on the algorithms and the libraries that carry them, so the project states them in writing.

**[PUB-064]** The project MUST keep a written export classification statement for every encryption function of the host, the WASM sandbox and the TLS client. The statement MUST name each library and each algorithm, MUST be updated in the same release as any change of a cryptographic dependency, and MUST be reviewed by counsel before a release that adds a cryptographic function (proposed, owner to confirm).

### Edge cases

- **A match on a common name.** The requester is refused, and the operator checks the match through the claims contact. A confirmed mistake is cleared in the operator's private record, and the account may then be created again.
- **A list changes after an account exists.** The account keeps its token until the operator's next screen, which runs before the next token is issued. A match then suspends the account under [Governance](/pages/governance/) with the same refusal text.
- **A user who downloads a plugin from a mirror.** The mirror carries no screening result. Mirror rules are in [Mirror runbook](/pages/publish-mirror-runbook/).

## Commercial plugins

An author may sell a plugin, or ask for a licence key before the plugin runs. The Ilium licence of the host does not forbid either, and the registry carries no payment or entitlement data. This section states who answers the buyer, what the package and the registry must not contain, and what the buyer sees. The rules of the registry for licence-key plugins are in [Registry reference](/pages/reference-registry/).

### You will need

The end-user terms you want buyers to accept, the refund address, and the licence key check if the plugin has one. Read [Publishing and support](/pages/guide-publishing-and-support/) for the support rules that apply to every plugin, paid or not.

### Who answers the buyer

The seller answers refunds, disputes and support for a paid plugin. The registry operator is not a party to the sale. It does not take payment, hold entitlements or mediate a refund ([PUB-071](#commercial-plugins)).

**[PUB-069]** A plugin MAY be sold, or licensed with a licence key, under any licence its author chooses. The registry MUST NOT refuse a publish because the plugin charges for use (proposed, owner to confirm).

**[PUB-070]** A plugin that charges for use MUST carry its end-user terms in its `LICENSE` file or in a terms file named in its README, and MUST state in its README the price model and the refund address (proposed, owner to confirm).

**[PUB-071]** The registry operator MUST NOT take payment, hold entitlement data or mediate a refund for a plugin. The plugin page of a paid plugin MUST show the seller's contact address, and that address is the place where refund and support requests go (proposed, owner to confirm).

**[PUB-072]** A licence key MUST NOT appear in the package, in a test fixture or in the release notes. The seller issues the key outside Ilium, and a plugin that checks a key MUST declare that check in its manifest, as the registry rules for licence-key plugins require (proposed, owner to confirm).

### What the buyer sees

The plugin page shows `Requires a licence key` when the manifest declares a key check, and it shows the seller's contact. The install dialog is not yet specified for end-user terms: the project has not decided whether it shows the terms text or only a link to it. Until it decides, the buyer reads the terms file named in the README before installing.

### Edge cases

- **A plugin that changes from free to paid.** The new version is published with the terms file and the README statement. Installed copies keep working, and the buyer's refund address is the one of the newest version.
- **A key that leaks.** The seller revokes it outside Ilium and issues a new one. The registry holds no key, so there is nothing to revoke there.
- **A buyer who asks the registry for a refund.** The registry answers that it does not hold payment data and gives the seller's contact from the plugin page.

## Use of the Ilium name and logo

The MIT licence covers the code, not the name or logo. The reserved id prefixes `ilium.`, `core.`, `local.`, `test.` and `example.` are refused at publish with `registry.id-reserved` ([PKG-387]). No key can be trusted for the publisher ids `ilium`, `core`, `local` or `test` ([PKG-220]), and a package starting with `ilium.` is refused with `manifest.id-reserved` even when signed ([PKG-234]). The rules below add the use of the name in text and artwork.

- A plugin, tool or registry may say "for Ilium" or "compatible with Ilium". It must not present itself as Ilium, use the logo as its own icon, or claim endorsement.
- A fork that changes behaviour must use another name and state its own `ilium:api-version`, so a plugin can tell which contract it runs under.
- A private or mirror registry may not call itself the official registry. It shows its own name and URL, and a client pins it by its own root key.
- A release not built and signed by the project is not called an official Ilium release.
- For any other use, such as a book, a talk or a product name, ask at the project address given on the repository page before you use the name or logo.

The registry checks this rule at publish for plugin ids and display names. A name that uses `ilium`, `official`, `core` or `verified` as a separate word is refused unless the publisher id is a `user.` account of the project maintainers. The rule and its appeal path are in [Registry namespaces](/pages/reference-registry/#namespaces).

## See also

[Design decisions](/pages/design-decisions/), [Security response](/pages/reference-security-response/), [Registry](/pages/reference-registry/), [Signing and trust](/pages/reference-signing-and-trust/), [Host release verification](/pages/reference-host-release-verification/).
