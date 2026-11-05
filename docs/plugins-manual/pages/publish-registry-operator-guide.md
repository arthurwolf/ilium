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
| `timestamp.json` | Every metadata change, and on a timer. | At least every 12 hours (see [PUB-027](#metadata-lifetimes)). |
| `snapshot.json` | Every metadata change. | On every publish, yank, deprecate, unlist or takedown. |
| `index.json`, `plugins/<id>.json` | Publishes and state changes. | On change. |
| `revocations.json` | Takedowns and key revocations. | On change. |
| `blobs/<sha256>.iliumpkg` | A publish. Removed only by takedown. | On publish. |

**[PUB-026]** The registry MUST serve every file of the table over HTTPS with the path of the table, and MUST NOT serve a metadata document under a different name than the one its role uses. Test: fetch each path with `curl --fail`; each one returns `200` and the expected `signed.type` value.

### Metadata lifetimes

Every metadata document has an `expires` field, and a client refuses a document after that time ([PKG-416](/pages/reference-registry/#client-behaviour-on-each-failure)). The operator must choose lifetimes that let a normal outage pass without a refusal, and short enough that a withdrawn document stops being trusted soon.

**[PUB-027]** The timestamp document MUST expire 24 hours after it is signed, and the registry MUST re-sign it at least every 12 hours even when nothing changed. Proposed, owner to confirm: 24 hours is the timestamp lifetime.
**[PUB-028]** The snapshot document MUST expire no sooner than the timestamp document, and SHOULD expire 7 days after it is signed. Proposed, owner to confirm: 7 days.
**[PUB-029]** The root document MUST expire no sooner than 365 days after it is signed, and the operator MUST rotate it at least 30 days before its expiry. Test: read `root.json`, compute the time left, and fail the check when it is under 30 days.

### Daily checks

Run these checks once a day. Each one has a measurable pass condition.

| Check | Pass condition |
|---|---|
| `timestamp.json` `expires` | More than 6 hours away. |
| Root `expires` | More than 30 days away. |
| Publish failures in the last 24 hours | Below a threshold the operator writes down. Proposed, owner to confirm: 5 percent of publishes. |
| Responses with `registry.rate-limited` | Logged, and reviewed when a single publisher exceeds 20 publishes in one hour ([PKG-400](/pages/reference-registry/#publishing)). |
| Transparency log | Its newest entry is less than one hour old. |

**[PUB-030]** The registry MUST expose the expiry times of `timestamp.json` and `root.json` to the operator as a machine-readable status, and the operator MUST receive an alert when either is less than 6 hours or 30 days away, as [PUB-018](/pages/publish-mirror-runbook/#sync-schedule) requires for mirrors.
**[PUB-031]** The registry MUST NOT log a publish token, an `Authorization` header or a second-factor code in any log line ([PKG-392](/pages/reference-registry/#publishing)). Test: publish with a known token, then search the logs for that token; no match.

### Accounts and access

**[PUB-032]** Every publisher id MUST have at least two maintainers, and every maintainer MUST have a second factor enrolled ([SEC-723](/pages/reference-trust-and-supply-chain/)). A publisher id with one maintainer MUST show the warning `No second maintainer` on its registry page.
**[PUB-033]** A publish token MUST be issued to one publisher id and MUST expire 90 days after it is issued ([PKG-968](/pages/reference-registry/#publish-tokens)). The operator MUST run a report of tokens older than 90 days each month. Proposed, owner to confirm: the monthly report is the reminder; the token itself expires on its own.
**[PUB-034]** A token revocation MUST take effect within one hour of the publisher's request or of a leak report ([PKG-970](/pages/reference-registry/#publish-tokens)). Test: revoke a token, then publish with it within the hour; the publish is refused with `registry.auth`.

### Capacity and limits

The limits below come from the client and upload rules. The operator enforces them on the server.

| Limit | Value | Source |
|---|---|---|
| Package size | 300 MiB | [PKG-400](/pages/reference-registry/#publishing) |
| Publishes per publisher id | 20 per hour | [PKG-400](/pages/reference-registry/#publishing) |
| Maintainers per publisher id | At least 2 | [PUB-032](#accounts-and-access) |
| Token lifetime | 90 days | [PKG-968](/pages/reference-registry/#publish-tokens) |
| Pending review at level B | Proposed, owner to confirm: 5 working days from upload to a decision. | [PKG-401](/pages/reference-registry/#policy) |

**[PUB-035]** The registry MUST answer an upload that exceeds the size limit with `registry.size` before it stores any bytes of the body beyond the limit. Test: upload 301 MiB and check that the disk usage of the blob folder does not grow by more than the limit.
**[PUB-036]** The registry MUST answer an over-limit publisher with `registry.rate-limited` and a `Retry-After` header. The header MUST be a whole number of seconds.

### Takedowns and appeals

**[PUB-037]** A takedown MUST be approved by a second operator when the registry has more than one operator. Proposed, owner to confirm: the second approval is recorded in the transparency log with the first.
**[PUB-038]** The operator MUST answer an appeal within 14 days ([PKG-404](/pages/reference-registry/#policy)) and MUST record the decision and the reason in the transparency log. A reinstated version gets a new version number.

- **[PUB-043]** An appeal MUST NOT suspend the takedown. The `blocked_ids` entry MUST stay in `revocations.json`, and installed copies MUST stay `disabled-blocked`, until the decision is published. After a successful appeal the registry MUST remove an id-level `blocked_ids` entry so that the publisher can publish a new version under the rules of [PKG-404](/pages/reference-registry/#policy), and it MUST keep every version tombstone of [PKG-821](/pages/reference-registry/) (proposed, owner to confirm).
- **[PUB-044]** The operator MUST accept an appeal only from a maintainer of the publisher id, authenticated by the registry account with its second factor or by a signature of a trusted key of that publisher id. An appeal from anyone else MUST be refused and MUST NOT use up the single appeal allowed by [PKG-404](/pages/reference-registry/#policy). When the registry has more than one operator, the decision MUST be made by an operator who did not approve the takedown (proposed, owner to confirm).

The procedure for a takedown, in order:

1. Record the reason, the rule of the content policy and the time.
2. Add the version or the id to `blocked_ids` in `revocations.json`.
3. Remove the blob.
4. Sign `revocations.json`, `snapshot.json` and `timestamp.json`.
5. Notify every maintainer of the publisher id ([PUB-016](/pages/publish-yank-deprecate-takedown/#takedown)).
6. Check that a mirror sync and a client refresh see the change ([PUB-022](/pages/publish-mirror-runbook/#propagating-yanks-and-takedowns)).

### Incidents

**[PUB-039]** When a signing key or a publish token leaks, the operator MUST follow [Registry key compromise](/pages/reference-registry-key-compromise/) and MUST NOT wait for the publisher to act first. The first action is to revoke the key or the token, within the time of [PUB-034](#accounts-and-access).
**[PUB-040]** When a published version is found to carry malware, the operator MUST take it down under [PKG-403](/pages/reference-registry/#policy) within 24 hours of the report. Proposed, owner to confirm: 24 hours is the target for a confirmed malware report.

- **[PUB-045]** The registry MUST NOT include the identity, address or report text of an abuse reporter in `revocations.json`, `quarantine.json`, `policy.json`, any transparency log entry, any plugin page, or any notice to the maintainers of the reported plugin. Only the operator's private record may hold them (proposed, owner to confirm).
- **[PUB-046]** The abuse intake of PKG-705 MUST accept a report that names a plugin id, a version or a package `sha256`, and one rule id of `policy.json`. It MUST return a report id in the acknowledgement, and it MUST send the reporter the recorded decision (yank, takedown, quarantine, or none with a stated reason) with the index of its transparency log entry (proposed, owner to confirm).
- **[PUB-049]** During the 24-hour decision window of PKG-705 the operator MAY quarantine the reported range through `quarantine.json`. When the decision is `none`, the quarantine MUST be withdrawn (EXT-059) within 24 hours of that decision (proposed, owner to confirm). When the decision is a takedown, the quarantine entry MUST be withdrawn in the same metadata change that adds the `blocked_ids` entry (proposed, owner to confirm).

### Legal requests

Court orders, subpoenas, preservation requests and requests from government bodies are handled under [Legal requests](/pages/publish-legal-requests/). The operator keeps each request and its outcome in the private record described there.

### Review staff and quality audit

A level B badge ([PKG-683](/pages/reference-registry/#policy)) is only as trustworthy as the people who decide it. The rules below apply to every level B decision.

**[PUB-082]** A level B review MUST be recorded by a reviewer who has completed the review training named in the operator's private record and who has declared to the operator no financial or personal link to the publisher, and a refusal MUST be confirmed by a second reviewer before it takes effect.

**[PUB-083]** Each quarter the registry operator MUST re-review at least 5 percent of that quarter's level B decisions, rounded up to a whole decision, by a reviewer who was not involved in the original decision, and MUST state the agreement rate of those re-reviews in the transparency report.

### Backups and retention

Keep every published blob and every signed metadata document for as long as a lock file or a mirror might still need it. [Retention of published versions](/pages/reference-registry/#retention-of-published-versions-and-operator-backups) lists the rules. The operator's own backups of the registry follow the same rule: a backup holds signed files and never a publish token. The rule for a publisher's own backups is out of scope for this page.

**[PUB-041]** The operator MUST back up the blob folder and the metadata folder together, so that a restore has a `snapshot.json` that matches its files. Test: restore a backup to an empty host and run the client checks of [Publisher workflow and version states](/pages/publish-publisher-workflow/) against it; every hash matches.

### Account data and erasure

The registry holds the verified contact address of each maintainer ([PKG-093](/pages/reference-registry/#policy)), the recovery data of each account, the sanctions screening record ([PUB-063](/pages/governance/)) and the access logs ([USR-956](/pages/reference-requirements-user-admin/)). The transparency log holds key fingerprints and publisher ids only. Data that the Ilium project holds about users is covered by USR-958 and is not covered here.

**[PUB-078]** The registry operator MUST answer a written erasure request from a publisher account holder within 30 days, MUST erase the contact address, the display name and the recovery data of that account, and MUST keep the transparency log entries, which carry only key fingerprints and publisher ids.

**[PUB-079]** The registry operator MUST state in its private record the retention period of account data, of the access logs and of each backup, and MUST delete an erased account's data from every backup no later than the first restore cycle that follows the erasure request.

### Availability and outages

Installs and updates depend on the official registry. The objective and the outage duties below let administrators plan around it.

**[PUB-080]** The official registry operator MUST state a monthly availability objective of 99.5 percent for metadata and blob serving on the status page, and MUST post an outage notice on the status page within 1 hour of detecting an outage that lasts more than 15 minutes.

**[PUB-081]** During an outage the client MUST keep using its cached signed metadata and its installed packages, and MUST show the time of the last successful refresh in `ilium plugin registry status`.

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

## Production additions (round 2)

A registry that requires an SBOM cannot rely on the publisher's client to scan it, because client flags can be skipped by a modified or old client. The registry therefore matches every SBOM component against its own advisory database at publish time. A signed VEX statement is the only way for a publisher to override an unreviewed Critical advisory. The refusal gives the publisher what is needed to fix the package.

The registry holds the signed transparency log, the metadata and the blobs, and losing them breaks client trust. The operator keeps a second copy in a separate failure domain and proves each quarter that it can be restored. A restored log must match the lost one byte for byte, so checkpoints and consistency proofs issued earlier still verify.

- **[PRX-005]** A registry that requires an SBOM MUST, on its own side and regardless of client flags, match every SBOM component against its advisory database and MUST refuse the publish with `registry.known-vulnerable` when a component has an unreviewed Critical advisory, unless the publisher attaches a signed VEX statement marking the advisory `not_affected`; the refusal MUST name the component, the advisory id and the fixed version.
- **[PRX-006]** A registry operator MUST keep a copy of the signed transparency log, all metadata documents and all blobs in a second failure domain with a recovery point of at most 1 hour, MUST run a restore drill at least once per quarter, and a restored log MUST be byte-identical to the lost one so that every previously issued checkpoint and consistency proof still verifies.
