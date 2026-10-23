---
layout: page
title: Requirements index
permalink: /pages/reference-requirements/
---

## Requirements index

This index lists the numbered requirements of the Ilium plugin manual. A requirement is one testable statement about what the host, the SDK or a plugin must do. The statements come from the contract pages, so a test author can derive a test from a row without reading the prose around it. Contract version: ilium:plugin@0.1.0.

### Meaning of a requirement

A requirement has one id, one statement, one source and one observable test. The statement uses one keyword.

| Keyword | Meaning |
|---|---|
| MUST, MUST NOT | The behaviour is mandatory. A test fails when it differs. |
| SHOULD, SHOULD NOT | The behaviour is expected. A test records a deviation as a warning. |
| MAY | The behaviour is allowed and optional. A test does not fail either way. |

A statement holds one keyword. A statement that needs two keywords is two rows.

### Id rules

- An id is a prefix of three capital letters, a hyphen and three digits: `[API-040]`.
- Ids are never reused. A removed requirement leaves its number unused.
- An id appears as a definition in exactly one requirements page. Other pages cite it in square brackets.
- Numbers inside a page are grouped in tens. The first group holds the general rules and later groups hold a topic each.

### Prefixes

| Prefix | Area | Page |
|---|---|---|
| `API` | Host API: errors, traps, restart rule, limits, quotas | [Host API](/pages/reference-requirements-api-host/) |
| `CAP` | Capabilities and grants | [Capabilities](/pages/reference-requirements-capabilities/) |
| `WID` | Widgets and the ui-tree | [Widgets](/pages/reference-requirements-widgets/) |
| `PRI` | Pane primitives: terminal, agent, editor, basic | [Pane primitives](/pages/reference-requirements-primitives/) |
| `EVT` | Events and intents | [Events and intents](/pages/reference-requirements-events-intents/) |
| `RQO` | Requests, submissions and outcomes | [Requests and outcomes](/pages/reference-requirements-requests-outcomes/) |
| `SVC` | Services and topics | [Services and topics](/pages/reference-requirements-services-topics/) |
| `EXT` | Extension points and hooks | [Extension points](/pages/reference-requirements-extension-points/) |
| `SET` | Settings | [Settings](/pages/reference-requirements-settings/) |
| `PAN` | Panes and their lifecycle | [Panes](/pages/reference-requirements-panes/) |
| `RND` | Rendering and scenes | [Rendering](/pages/reference-requirements-rendering/) |
| `LIF` | Plugin lifecycle and operations (other lane) | [Lifecycle and operations](/pages/reference-requirements-lifecycle-ops/) |
| `AUT` | Author guides (other lane) | [Author guides](/pages/reference-requirements-author-guides/) |
| `PKG` | Packages, install, signing and trust (other lane) | [Package format](/pages/reference-package-format/) |
| `SEC` | Security and sandbox (other lane) | [Threat model](/pages/reference-threat-model/) |
| `HST` | Host internals (other lane) | [Host architecture](/pages/reference-host-architecture/) |
| `DEV` | Developer tools (other lane) | [Check and lint rules](/pages/reference-check-and-lint-rules/) |
| `USR` | User documentation (other lane) | [Plugin policy](/pages/reference-plugin-policy/) |

### Row format

Every area page is one table with four columns.

| Column | Content |
|---|---|
| Id | The id in square brackets, such as `[RQO-004]`. |
| Requirement | One statement with one keyword. |
| Source | A link to the page and the anchor that states the behaviour. |
| Observable test | The channel in parentheses, the input, an arrow, and the expected result. |

### Observation channels

A test names the channel on which it looks. Each channel is something a test can read without help from the host internals.

| Channel | What a test reads |
|---|---|
| sync-return | The result of a host function call: ok or a plugin-error code. |
| completion | The request-result event for a submission or a request-id: the outcome. |
| install-error | The JSONL error lines and the exit code of `ilium plugin install` or `check`. |
| load-warning | A warning shown at load that does not stop the plugin. |
| log-line | A line of `ilium plugin logs` for the plugin. |
| trap | The end of an instance, shown as a log line with the plugin id, interface and function. |
| restart-schedule | The delay between a failure and the next instance, and the final disable. |
| event-delivery | The events a plugin receives in `on-events`, their order and batching. |
| screen-text | The text of the rendered screen, read from a terminal capture. |
| cli-exit | The exit code and output of an Ilium command. |

### Glossary

| Term | Meaning |
|---|---|
| Side | One of server, client or scene. A plugin runs once for each side it declares. |
| Grant | A capability string in the manifest that the user accepts. |
| Trap | A plugin bug that ends the instance. The plugin cannot catch it. |
| Failure | An event that counts toward the restart and disable rule. |
| Submission | One intent that a plugin returns from `on-events`. |
| Outcome | The result of a submission: ok, ok-node, ok-epoch, duplicate, refused, failed or cancelled. |
| Dedup key | A string of 1 to 128 characters that makes a submission safe to repeat for 10 minutes. |
| Extension code | A code such as `extension.vetoed`, carried as a base code plus a message prefix. |
| Tier | T1 (bundled), T2 or T3. The tier sets the ceilings of the limits. |

### Index

Each area page below lists its requirements with an id, a rule and a test.

| Area | Prefix | Page | Rows |
|---|---|---|---|
| Host API | `API` | [Host API](/pages/reference-requirements-api-host/) | 29 |
| Capabilities | `CAP` | [Capabilities](/pages/reference-requirements-capabilities/) | 30 |
| Widgets | `WID` | [Widgets](/pages/reference-requirements-widgets/) | 30 |
| Pane primitives | `PRI` | [Pane primitives](/pages/reference-requirements-primitives/) | 21 |
| Events and intents | `EVT` | [Events and intents](/pages/reference-requirements-events-intents/) | 27 |
| Requests and outcomes | `RQO` | [Requests and outcomes](/pages/reference-requirements-requests-outcomes/) | 17 |
| Services and topics | `SVC` | [Services and topics](/pages/reference-requirements-services-topics/) | 25 |
| Extension points | `EXT` | [Extension points](/pages/reference-requirements-extension-points/) | 41 |
| Settings | `SET` | [Settings](/pages/reference-requirements-settings/) | 21 |
| Panes | `PAN` | [Panes](/pages/reference-requirements-panes/) | 21 |
| Rendering | `RND` | [Rendering](/pages/reference-requirements-rendering/) | 20 |

### How a proposed row becomes a confirmed requirement

Many rows carry the marker `(proposed, owner to confirm)` next to a number, a command or a whole statement, for example [SEC-101], [DEV-975] and [PKG-958]. The marker means the value is the manual's best current proposal and no owner has decided it yet. A proposed value is a design input, not a promise to plugin authors.

**Who the owner is.** Each prefix has one owner role: a named maintainer who is listed in the maintainer file of the repository for that prefix. The owner decides the values of every row under the prefix. `SEC` rows are owned by the security maintainer, `PKG` rows by the release maintainer, and the other prefixes by the maintainer of the matching host crate. One person can hold several roles; a decision on a row is never made by its author alone.

**How a confirmation is recorded.** Confirming a row is one change that does three things together:

1. The row loses the marker. The number stays if the owner accepts it, or changes if the owner decides otherwise.
2. The decision gets a `D<n>` entry in [Design decisions](/pages/design-decisions/) with the row id, the confirmed value, the old value if it changed, the reason and the date `YYYY-MM-DD`.
3. The row's test changes from `pending: test to be written` to a real test name, or the change is not merged.

A row without the marker and without a decision entry is an error in the manual and is fixed by adding the entry.

**The release gate.** No host release may ship a MUST or MUST NOT row that still carries the marker. The release checklist counts the markers per lane with `rg -c 'proposed, owner to confirm' pages/reference-requirements-*.md` and fails when a MUST row is still marked. SHOULD and MAY rows may ship marked; the release notes list them as provisional. `ilium plugin check` does not read the manual: it counts markers only in plugin packages it validates, so it cannot clear a host release. Plugin authors read the marker as "this value may still change before the host release that makes it final". A value without the marker is final for that release.

**How an owner changes a proposed value.** The row is rewritten in place and the marker stays until the confirmation change above. A confirmed row is never reused for a different meaning: if the meaning changes, the old id is retired (ids are never reused, see the id rules) and a new id is added.

**How a contract change after version 1 is proposed and approved.**

1. The author writes a proposal that names the affected ids, the old and new behaviour, the migration for existing plugins and the tests.
2. Two maintainers review it, and one of them is the owner of the affected prefix. The author cannot be a reviewer.
3. The change must be additive only, as defined in [Versioning](/pages/versioning/#the-rule-after-version-0.1.0). A change that removes or changes the meaning of a shipped item is refused; the route is a new item and an unstable-then-stable graduation.
4. The approved change gets a `D<n>` entry, and the rows change in the same merge.

**Worked example.** [DEV-975] proposes that an unstable feature graduates or is removed within 6 minor releases (proposed, owner to confirm). The release maintainer reviews release history, finds that a feature has stayed unstable for only four releases in practice, and decides on 4. The change removes the marker, rewrites the row to say 4 minor releases, adds the entry `D<n>` with old value 6, new value 4 and the date, and adds the checklist test for the fourth-minor-release flag. A plugin author who read 6 as a proposal is not broken, because the marker warned that the value could change; a plugin that used a feature past its fourth minor release now sees the checklist blocking flag.

### Coverage rule

Every behaviour that a page states with must, never, only or a fixed number MUST appear in one requirements row. Check the rule with these commands from the manual folder.

```
rg -o '^\| \[([A-Z]{3}-[0-9]{3})\]' -r '$1' pages/reference-requirements-*.md | sort | uniq -d
rg -o '\[(API|CAP|WID|PRI|EVT|RQO|SVC|EXT|SET|PAN|RND)-[0-9]{3}\]' pages --no-filename | sort -u > /tmp/cited.txt
```

The first command must print nothing. A printed id is defined twice. The second command lists every cited id. Each listed id must also be a row id in an area page.

### Reading order

- [Error codes and limits](/pages/reference-errors-and-limits/) for the numbers behind most rows.
- [Capabilities](/pages/reference-capabilities/) for the grant table.
- [Events and intents](/pages/reference-events-and-intents/) and [Requests and outcomes](/pages/requests-and-outcomes/) for the submission rules.
- [Lifecycle](/pages/lifecycle/) for the other lane requirements.

### See also

- [Reference](/pages/reference/)
- [Manifest reference](/pages/reference-manifest/)
