---
layout: page
title: Rights-holder claims
permalink: /pages/publish-rights-claims/
---

## Rights-holder claims

A rights holder may say that a published plugin uses a work they own without permission. The work can be code, text, an image, a sound, a font or a trademark. This page covers what a claim must contain, how the registry operator answers it, how the publisher replies, when a version is quarantined or taken down, and what is published about the claim. It applies to copyright, trademark and patent claims alike. A claim about malware, hidden data collection or impersonation is a security or policy report instead, and follows [Yank, deprecate and takedown](/pages/publish-yank-deprecate-takedown/) and the abuse intake of [PKG-705](/pages/reference-registry/#policy).

The registry operator decides on a claim. The Ilium host does not judge whether a plugin infringes anything.

### You will need

A claim must come from the rights holder or from a person acting for them. Read the decision table before you answer a claim as a publisher, and the operator procedure before you handle one as an operator.

### What a claim contains

A claim is complete only when it names all of these:

- the plugin id, and the version or the package `sha256` (the value of `ilium plugin info <id>@<version>`);
- the work claimed and where it is in the package, for example a file path or a manifest field;
- the proof of ownership, such as a registration number, a URL of the original or a licence document;
- the legal name of the rights holder and a contact address;
- a statement that the claim is made in good faith.

The claims address is named in `policy.json`. It is separate from the abuse intake, so that a copyright claim and a security report never share a queue.

### Decision table

| Situation | Operator action | Publisher sees | Installed copies |
|---|---|---|---|
| The claim is incomplete. | Asks for the missing item. Starts no timetable. | Nothing until the claim is complete. | No change. |
| The claim is complete and names one version. | Acknowledges, then decides within the decision window. | The claim id and the decision when made. | No change while the claim is open. |
| The claim is open and the version is at risk. | Quarantines that version. | A notice naming the claim. | Keep running. Not offered for update. |
| The claim is withdrawn or rejected. | Withdraws the quarantine. | A notice of the decision. | Back to normal. |
| The claim is upheld. | Takes the version down, or the plugin when the whole id is affected. | A notice to every maintainer. | Moved to `disabled-blocked` at the next metadata refresh. |
| The publisher sends a counter-notice. | Forwards it. Applies the rule of the counter-notice section below. | A receipt with the time of forwarding. | As the decision requires. |

### Operator procedure

1. Check the claim is complete. If not, send the request for the missing item and stop the clock.
2. Acknowledge the claim with a claim id, and record it in the operator's private record.
3. Find the version named in the claim. Read the work and the claimed file or field in the package.
4. Quarantine the version if the claim is plausible and the version is still being offered. A quarantine is the least drastic step that stops new installs.
5. Decide, and send the decision to the claimant and to every maintainer of the publisher id. A decision on a claim against a version that is already taken down still goes to both parties.
6. Publish the decision in the transparency log, as [PUB-056](#what-is-published) requires.

### Counter-notice

**[PUB-053]** A publisher whose version is quarantined or taken down for a claim MAY send a counter-notice to the claims address, stating the version, why the publisher believes the claim is wrong, and a contact address. The operator MUST forward the counter-notice to the claimant within 1 business day and MUST tell the publisher the time of forwarding (proposed, owner to confirm).

**[PUB-054]** When the claimant does not show a court filing or a written agreement within 14 calendar days of the forwarding of a counter-notice, the operator MUST withdraw the quarantine of that version within 1 business day. The operator MUST NOT reverse a takedown on the counter-notice alone; a taken-down version comes back only as a new version under [PKG-404](/pages/reference-registry/#policy) (proposed, owner to confirm: 14 calendar days).

### Decision timing

**[PUB-050]** A rights holder MUST send a claim that names the plugin id, the version or the package `sha256`, the work claimed, the proof of ownership, the legal name and a contact address of the rights holder, and a statement of good faith. A claim without one of these items MUST be answered with a request for the missing item, and it MUST NOT start the decision window of [PUB-051](#decision-timing) (proposed, owner to confirm).

**[PUB-051]** The operator MUST acknowledge a complete claim within 1 business day with a claim id, and MUST decide within 10 business days whether to quarantine, take down or take no action. The decision MUST be sent to the claimant and to every maintainer of the publisher id (proposed, owner to confirm: 10 business days).

**[PUB-052]** While a complete claim is open, the operator MAY quarantine the version the claim names through `quarantine.json` ([EXT-057](/pages/reference-registry/)). The operator MUST NOT quarantine a version that the claim does not name.

### Repeat claims

**[PUB-055]** When three claims against one publisher id are upheld within 12 months, the operator MUST suspend that publisher id under [PKG-091](/pages/reference-registry/#policy) for 90 days, and MUST tell every maintainer of the publisher id the number of upheld claims and the date of the last one (proposed, owner to confirm: three upheld claims within 12 months, 90 days).

### What is published

**[PUB-056]** The registry MUST record every decision on a claim in the transparency log with the claim id, the plugin id, the version, the decision and the time. It MUST NOT publish the claimant's contact address, the text of a counter-notice or the identity of the person who filed the claim; those stay in the operator's private record (proposed, owner to confirm).

### Worked example: a font licence claim

A rights holder writes that version 1.2.0 of `com.example.typeset` bundles a font they publish without a licence. The claim names the plugin id, the version, the file `assets/display.ttf`, a registration number and a contact address, and it contains a statement of good faith. The operator acknowledges it the next business day and quarantines 1.2.0. The publisher replaces the font with a licensed one and ships 1.2.1. The operator takes no action on 1.2.1, because the claim names only 1.2.0. After 10 business days the operator decides the claim is upheld, takes 1.2.0 down and sends the decision to both parties. Installed copies of 1.2.0 move to `disabled-blocked` at the next refresh. Copies of 1.2.1 are not affected.

### Edge cases

- **A claim against a plugin with no published version left.** The operator still decides, and records the decision. The plugin page stays empty.
- **A claim that names a version which a later version has already replaced.** The operator decides on the named version. Its decision does not extend to later versions unless the claim names them.
- **Two claims against the same version.** Each has its own claim id and decision. A quarantine ends only when every open claim against that version is withdrawn or decided.
- **A claim about a trademark that the plugin id uses.** The name rules of [Governance](/pages/governance/) apply to the plugin id. A trademark claim about the id follows this page, and the operator decides whether the id must change.

### Open decisions

- The decision windows in [PUB-051](#decision-timing) and [PUB-054](#counter-notice), the counter-notice rule in [PUB-053](#counter-notice) and the repeat-claim rule in [PUB-055](#repeat-claims) are defaults, not a legal opinion. They need review by counsel for each jurisdiction in which the operator takes claims before the registry opens to publishers in that jurisdiction.

### See also

- [Yank, deprecate and takedown](/pages/publish-yank-deprecate-takedown/): the actions a takedown and a quarantine have on installed copies.
- [Registry operator guide](/pages/publish-registry-operator-guide/): the daily checks and the takedown and appeal procedure.
- [Governance](/pages/governance/): the moderation rules and the name rules.
