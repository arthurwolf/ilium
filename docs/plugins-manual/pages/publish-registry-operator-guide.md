---
layout: page
title: Registry operator guide
permalink: /pages/publish-registry-operator-guide/
---

## Registry operator guide

This guide is for the person who runs a plugin registry: the static files, the upload endpoint, the signing keys and the daily checks. It is the operator's entry point. The file layout, the metadata fields and the client rules are in [Registry reference](/pages/reference-registry/). The publisher's view is in [Publisher workflow and version states](/pages/publish-publisher-workflow/), and the mirror view is in [Mirror runbook](/pages/publish-mirror-runbook/).

### You will need

- A host that serves HTTPS with a certificate that clients trust, or an internal CA bundle that the administrators of the clients will set ([PKG-987](/pages/reference-registry/#mirrors)).
- Offline or hardware-held keys for the root role, and online keys for the timestamp, snapshot and targets roles. Key custody is in [Publisher key custody and signing in CI](/pages/guide-publishing/#publisher-key-custody-and-signing-in-ci) for publishers and in [Release key compromise](/pages/reference-release-key-compromise/) for the project's own release key.
- A list of the maintainers of every publisher id, with a second factor enrolled for each ([SEC-723](/pages/reference-trust-and-supply-chain/)).
- A place to publish `policy.json`, which states the content policy and the appeal address ([PKG-402](/pages/reference-registry/#policy), [PKG-404](/pages/reference-registry/#policy)).

### What the registry serves

| Path | Changed by | How often |
|---|---|---|
| `root.json` | Key rotation only. | Rarely. |
| `timestamp.json` | Every metadata change, and on a timer. | At least every 12 hours (see [PUBL-027](#metadata-lifetimes)). |
| `snapshot.json` | Every metadata change. | On every publish, yank, deprecate, unlist or takedown. |
| `index.json`, `plugins/<id>.json` | Publishes and state changes. | On change. |
| `revocations.json` | Takedowns and key revocations. | On change. |
| `blobs/<sha256>.iliumpkg` | A publish. Removed only by takedown. | On publish. |

**[PUBL-026]** The registry MUST serve every file of the table over HTTPS with the path of the table, and MUST NOT serve a metadata document under a different name than the one its role uses. Test: fetch each path with `curl --fail`; each one returns `200` and the expected `signed.type` value.

### Metadata lifetimes

Every metadata document has an `expires` field, and a client refuses a document after that time ([PKG-416](/pages/reference-registry/#client-behaviour-on-each-failure)). The operator must choose lifetimes that let a normal outage pass without a refusal, and short enough that a withdrawn document stops being trusted soon.

**[PUBL-027]** The timestamp document MUST expire 24 hours after it is signed, and the registry MUST re-sign it at least every 12 hours even when nothing changed. Proposed, owner to confirm: 24 hours is the timestamp lifetime.
**[PUBL-028]** The snapshot document MUST expire no sooner than the timestamp document, and SHOULD expire 7 days after it is signed. Proposed, owner to confirm: 7 days.
**[PUBL-029]** The root document MUST expire no sooner than 365 days after it is signed, and the operator MUST rotate it at least 30 days before its expiry. Test: read `root.json`, compute the time left, and fail the check when it is under 30 days.

### Daily checks

Run these checks once a day. Each one has a measurable pass condition.

| Check | Pass condition |
|---|---|
| `timestamp.json` `expires` | More than 6 hours away. |
| Root `expires` | More than 30 days away. |
| Publish failures in the last 24 hours | Below a threshold the operator writes down. Proposed, owner to confirm: 5 percent of publishes. |
| Responses with `registry.rate-limited` | Logged, and reviewed when a single publisher exceeds 20 publishes in one hour ([PKG-400](/pages/reference-registry/#publishing)). |
| Transparency log | Its newest entry is less than one hour old. |

**[PUBL-030]** The registry MUST expose the expiry times of `timestamp.json` and `root.json` to the operator as a machine-readable status, and the operator MUST receive an alert when either is less than 6 hours or 30 days away, as [PUBL-018](/pages/publish-mirror-runbook/#sync-schedule) requires for mirrors.
**[PUBL-031]** The registry MUST NOT log a publish token, an `Authorization` header or a second-factor code in any log line ([PKG-392](/pages/reference-registry/#publishing)). Test: publish with a known token, then search the logs for that token; no match.

### Accounts and access

**[PUBL-032]** Every publisher id MUST have at least two maintainers, and every maintainer MUST have a second factor enrolled ([SEC-723](/pages/reference-trust-and-supply-chain/)). A publisher id with one maintainer MUST show the warning `No second maintainer` on its registry page.
**[PUBL-033]** A publish token MUST be issued to one publisher id and MUST expire 90 days after it is issued ([PKG-968](/pages/reference-registry/#publish-tokens)). The operator MUST run a report of tokens older than 90 days each month. Proposed, owner to confirm: the monthly report is the reminder; the token itself expires on its own.
**[PUBL-034]** A token revocation MUST take effect within one hour of the publisher's request or of a leak report ([PKG-970](/pages/reference-registry/#publish-tokens)). Test: revoke a token, then publish with it within the hour; the publish is refused with `registry.auth`.

### Capacity and limits

The limits below come from the client and upload rules. The operator enforces them on the server.

| Limit | Value | Source |
|---|---|---|
| Package size | 300 MiB | [PKG-400](/pages/reference-registry/#publishing) |
| Publishes per publisher id | 20 per hour | [PKG-400](/pages/reference-registry/#publishing) |
| Maintainers per publisher id | At least 2 | [PUBL-032](#accounts-and-access) |
| Token lifetime | 90 days | [PKG-968](/pages/reference-registry/#publish-tokens) |
| Pending review at level B | Proposed, owner to confirm: 5 working days from upload to a decision. | [PKG-401](/pages/reference-registry/#policy) |

**[PUBL-035]** The registry MUST answer an upload that exceeds the size limit with `registry.size` before it stores any bytes of the body beyond the limit. Test: upload 301 MiB and check that the disk usage of the blob folder does not grow by more than the limit.
**[PUBL-036]** The registry MUST answer an over-limit publisher with `registry.rate-limited` and a `Retry-After` header. The header MUST be a whole number of seconds.

### Takedowns and appeals

**[PUBL-037]** A takedown MUST be approved by a second operator when the registry has more than one operator. Proposed, owner to confirm: the second approval is recorded in the transparency log with the first.
**[PUBL-038]** The operator MUST answer an appeal within 14 days ([PKG-404](/pages/reference-registry/#policy)) and MUST record the decision and the reason in the transparency log. A reinstated version gets a new version number.

The procedure for a takedown, in order:

1. Record the reason, the rule of the content policy and the time.
2. Add the version or the id to `blocked_ids` in `revocations.json`.
3. Remove the blob.
4. Sign `revocations.json`, `snapshot.json` and `timestamp.json`.
5. Notify every maintainer of the publisher id ([PUBL-016](/pages/publish-yank-deprecate-takedown/#takedown)).
6. Check that a mirror sync and a client refresh see the change ([PUBL-022](/pages/publish-mirror-runbook/#propagating-yanks-and-takedowns)).

### Incidents

**[PUBL-039]** When a signing key or a publish token leaks, the operator MUST follow [Registry key compromise](/pages/reference-registry-key-compromise/) and MUST NOT wait for the publisher to act first. The first action is to revoke the key or the token, within the time of [PUBL-034](#accounts-and-access).
**[PUBL-040]** When a published version is found to carry malware, the operator MUST take it down under [PKG-403](/pages/reference-registry/#policy) within 24 hours of the report. Proposed, owner to confirm: 24 hours is the target for a confirmed malware report.

### Backups and retention

Keep every published blob and every signed metadata document for as long as a lock file or a mirror might still need it. [Retention of published versions](/pages/reference-registry/#retention-of-published-versions-and-operator-backups) lists the rules. The operator's own backups of the registry follow the same rule: a backup holds signed files and never a publish token. The rule for a publisher's own backups is out of scope for this page.

**[PUBL-041]** The operator MUST back up the blob folder and the metadata folder together, so that a restore has a `snapshot.json` that matches its files. Test: restore a backup to an empty host and run the client checks of [Publisher workflow and version states](/pages/publish-publisher-workflow/) against it; every hash matches.

### Worked example: a weekly check

<pre class="term"><span class="t-prompt">$</span> curl --fail -s https://registry.example.org/timestamp.json | head -c 120
<span class="t-ok">ok</span>   timestamp.json: type=timestamp version=4822 expires=2026-11-04 12:00:00
<span class="t-prompt">$</span> curl --fail -s https://registry.example.org/root.json | head -c 120
<span class="t-ok">ok</span>   root.json: type=root version=3 expires=2027-10-12 00:00:00
<span class="t-warn">warn</span> root expiry in 343 days, rotation due 2027-09-12
</pre>

The timestamp is fresh. The root has time left. The operator writes the result in the weekly log and takes no action.

### Edge cases

- **The registry is down at the time of a takedown.** The takedown is recorded when the registry is back. The record time is the time of the decision, not the time the registry recovered.
- **A publisher asks to delete a version.** The registry answers that deletion is not available ([PKG-399](/pages/reference-registry/#publishing)). The publisher can yank the version, and the operator takes it down only under the content policy.
- **A client uses a very old cache.** The client refuses the cache when its `expires` time has passed ([PKG-416](/pages/reference-registry/#client-behaviour-on-each-failure)). The operator cannot force a client to update.

### See also

- [Registry reference](/pages/reference-registry/): metadata layout, client error codes and the policy.
- [Signing and trust](/pages/reference-signing-and-trust/): key roles, trust tiers and revocation.
- [Yank, deprecate and takedown](/pages/publish-yank-deprecate-takedown/): the publisher-side actions that the operator sees in the log.
