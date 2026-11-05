---
layout: page
title: Proposing a change to the plugin API
permalink: /pages/governance-api-change-proposals/
---

## Proposing a change to the plugin API

This page specifies how a change to the plugin contract is proposed, reviewed, decided and recorded. The plugin contract is the set of WIT interfaces, capabilities, limits and manifest keys that plugins depend on. Plugin authors who want to contest a change, and maintainers who want to make one, both need this page, because a change that skips it is refused by the release checks.

## You will need

A GitHub account that can open a pull request against the Ilium repository, and the text of the change you want to make. You also need the numbers of the decision records you depend on, found on [Design decisions](/pages/design-decisions/), and the list of limits in [Errors and limits](/pages/errors-and-limits/). Read [Governance](/pages/governance/) first to learn who owns each requirement prefix, because that owner makes the final decision on a proposal.

## When a proposal is required

A proposal is required for any change to a WIT interface, to a capability, to a limit listed on [Errors and limits](/pages/errors-and-limits/), or to a manifest key. The proposal is numbered `P<n>`, using the next free number. It must be filed before the change is merged. A pull request that changes a WIT file must carry a `Proposal: P<n>` line in its description. Without that line, the repository check fails with `governance.proposal-missing`.

## Proposal contents

Every proposal states four things. It states the problem the change solves. It lists the alternatives that were considered, in a section named `Alternatives`. It describes the effect on existing packages. If the proposal deprecates anything, it also states the marking minor, which is the release that marks the item deprecated, and the removal minor, which is the release that removes it. A proposal file without the `Alternatives` section fails the proposal template check, and the check names `Alternatives`.

## Review stages and comment period

A proposal that contains a breaking change stays open for comment for at least 30 days. The owner of the affected requirement prefix may not decide on it before that period ends. If a proposal is opened today and someone tries to mark it `accepted` 20 days later, the template check fails with `governance.comment-period-open` and reports the days remaining.

## Outcomes and the decision record

Every proposal ends in exactly one of four outcomes: `accepted`, `rejected`, `withdrawn` or `superseded`. Any other value in the outcome field fails the template check, which lists the four allowed values. An accepted proposal must create a numbered `D<n>` entry on [Design decisions](/pages/design-decisions/) within 7 days. An accepted proposal that is older than 7 days without a `D<n>` reference fails with `governance.decision-missing`.

A rejected proposal records its reason in the proposal file. If someone later wants to reopen it, the proposal must carry a `Reopened because` section that cites new evidence. Changing a rejected proposal back to open without that section fails the template check with `governance.reopen-unjustified`.

## Effect on deprecation and support windows

A proposal never changes a contract in a patch release. An accepted proposal that changes a WIT interface can only take effect in a minor release. A patch release whose changelog references such a proposal fails the release check with `governance.contract-change-in-patch`.

When a proposal deprecates an item, its marking minor and removal minor are stated in the proposal, as described above. The calendar that connects those minors to the two-minor support window is worked out on [Deprecation timeline and the window](/pages/compat-deprecation-timeline/).

## Edge cases

A proposal that is withdrawn or superseded needs no decision record, because only an accepted proposal creates a `D<n>` entry. A rejected proposal stays closed unless new evidence is cited in a `Reopened because` section. A change made in a patch release is refused even when a proposal for it was accepted, so the change must wait for the next minor release.

## Requirements

- **[PDT-027]** A change to a WIT interface, a capability, a limit listed in Errors and limits, or a manifest key MUST be filed as a proposal numbered `P<n>` before any merge.
- **[PDT-028]** A proposal MUST state the problem, the alternatives considered, the effect on existing packages, and, if it deprecates anything, the marking minor and the removal minor.
- **[PDT-029]** A proposal containing a breaking change MUST stay open for comment at least 30 days before the owner of the affected requirement prefix decides.
- **[PDT-030]** The outcome MUST be exactly one of `accepted`, `rejected`, `withdrawn` or `superseded`, and an accepted proposal MUST create a `D<n>` entry on Design decisions within 7 days.
- **[PDT-031]** A proposal MUST NOT change a contract in a patch release.
- **[PDT-032]** A rejected proposal MUST record its reason and MUST NOT be reopened without new evidence cited in a `Reopened because` section.

## Production additions (round 3)

- **[PRY-043]** A proposal labelled `security-emergency` MAY shorten the comment period to 72 hours only if it names a CVE-style identifier, and the template check MUST fail with `governance.emergency-unjustified` when the label is present without one.
