# Requirements index

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
| Host API | `API` | [Host API](/pages/reference-requirements-api-host/) | 27 |
| Capabilities | `CAP` | [Capabilities](/pages/reference-requirements-capabilities/) | 18 |
| Widgets | `WID` | [Widgets](/pages/reference-requirements-widgets/) | 25 |
| Pane primitives | `PRI` | [Pane primitives](/pages/reference-requirements-primitives/) | 21 |
| Events and intents | `EVT` | [Events and intents](/pages/reference-requirements-events-intents/) | 19 |
| Requests and outcomes | `RQO` | [Requests and outcomes](/pages/reference-requirements-requests-outcomes/) | 17 |
| Services and topics | `SVC` | [Services and topics](/pages/reference-requirements-services-topics/) | 24 |
| Extension points | `EXT` | [Extension points](/pages/reference-requirements-extension-points/) | 25 |
| Settings | `SET` | [Settings](/pages/reference-requirements-settings/) | 20 |
| Panes | `PAN` | [Panes](/pages/reference-requirements-panes/) | 21 |
| Rendering | `RND` | [Rendering](/pages/reference-requirements-rendering/) | 18 |

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
