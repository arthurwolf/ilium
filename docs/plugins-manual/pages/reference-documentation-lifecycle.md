---
layout: page
title: Documentation lifecycle
permalink: /pages/reference-documentation-lifecycle/
---

## Documentation lifecycle

Some pages in this manual are lookup tables that must match the code: the glossary, the error-code catalogue, the troubleshooting pages, the FAQ pages and the feature traceability table. A release that changes a code, a term or a feature changes these pages too. This page names who keeps each one true and when it is reviewed. The ids have the form `DEV-nnn`; see the [requirements for developer tooling](/pages/reference-requirements-dev-tooling/).

## You will need

- The pages listed in the table below, open in the manual folder.
- The version of the host or the plugin contract being released.

## Owners and review

The owners are roles, not people. The role holder is named in the release record of each release (see DEV-958). The roles and the review point are proposed, owner to confirm.

| Page | Owner (role) | Reviewed |
|---|---|---|
| [Glossary](/pages/glossary/) | Manual maintainer | Before each release tag (proposed, owner to confirm) |
| [Error codes and limits](/pages/reference-errors-and-limits/) and [Manifest schema and error catalogue](/pages/reference-manifest-schema/) | Host maintainer | Before each release tag (proposed, owner to confirm) |
| [Troubleshooting for authors](/pages/guide-troubleshooting/), [Troubleshooting plugins](/pages/user-troubleshooting/) and [Safe mode and recovery](/pages/user-safe-mode-and-troubleshooting/) | Support lead | Before each release tag (proposed, owner to confirm) |
| [Author FAQ](/pages/guide-faq/) and [Plugin FAQ](/pages/user-faq/) | Support lead | Before each release tag (proposed, owner to confirm) |
| [Feature traceability](/pages/reference-feature-traceability/) | Host maintainer | Before each release tag (proposed, owner to confirm) |

## Rules

- **[DEV-955]** Every page in the table above MUST have one owner role, recorded in the table. A page with no owner MUST NOT be released. Test: parse the table and assert that each row has a non-empty Owner cell.
- **[DEV-973]** Before each release tag, the owner of each page in the table MUST review that page against the code of the release and MUST correct every term, code, command, limit or feature that no longer matches. The review cadence is each release (proposed, owner to confirm). Test: the release checklist names each page of the table, with a reviewer, before the tag is created.
- **[DEV-957]** A release that changes or removes an error code, a manifest key, a CLI command, a limit or a feature listed in a page of the table MUST update every page of the table that names it in the same change. A removed error code MUST move to the table of retired codes in the [manifest schema](/pages/reference-manifest-schema/#error-catalogue) instead of being deleted. Test: for each code removed in the release, search the pages and assert that the only hit is the table of retired codes.
- **[DEV-958]** Each release record MUST name, for each page of the table, the owner role, the reviewer and the date of the review. A page without a record MUST block the release. Test: the release record is a file with one line per page in the table; a missing line fails the release check.
- **[DEV-959]** A troubleshooting page that names an error code MUST link that code to its entry in the error-code catalogue, and every such link MUST resolve to an anchor on the catalogue page. Test: run the link checker over the troubleshooting pages and assert that each code link resolves.

## See also

- [Requirements for developer tooling](/pages/reference-requirements-dev-tooling/)
- [Feature traceability](/pages/reference-feature-traceability/)
- [Keeping the kit current](/pages/agent-docs-maintenance/)
- [Glossary](/pages/glossary/)
