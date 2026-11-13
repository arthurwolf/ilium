---
layout: page
title: Audit records for grant decisions
permalink: /pages/reference-audit-grant-events/
---

## About this page

This page specifies every audit record that a grant decision writes to the audit log: grants given, revoked, expired or declined, install attempts refused by policy, and refused runtime calls. It lists the fields, actors, rate limits and query examples that let a security team reconstruct who changed or was denied a permission. Security teams and anyone who builds tools on top of the audit log need this page. For the general audit log, see [Audit log](/pages/reference-audit-log/).

## Event list

Every grant decision writes one or more records with a fixed event name. The `grant-given` and `grant-revoked` events record a grant that a plugin gained or lost. Revoking all grants of a plugin writes one `grant-revoked` record for each grant and one `revoke-all` summary record. The `grant-expired` event records a grant whose expiry time has passed, and it is written once per expiry. The `grant-declined` event records a row that a person declined in the review dialog during install. The `grant-policy-denied` event records a grant that policy refused at install time. The `grant-refused` event summarises runtime calls that a plugin made for a grant it does not hold.

## Record fields

The `grant-given` and `grant-revoked` records carry the actor kind under `actor.kind`, the grant name under `grant`, the state before the change under `previous_state`, and the grant epoch under `epoch`. A revoke from the command line records the invoking user identifier under `actor.uid`. The `grant-expired` record carries `plugin_id`, `grant`, `expired_at` (the stored expiry time) and `detected_at` (the time the host noticed the expiry). The `grant-declined` and `grant-policy-denied` records carry the grant name. The `revoke-all` and `grant-refused` summary records carry `count`, the number of revocations or refused calls that they stand for.

## Actors

The `actor.kind` field takes one of four values. The value `user-tui` means that a person changed the grant in the terminal interface. The value `cli` means that a command-line invocation changed the grant. The value `policy` means that a policy decision changed the grant. The value `expiry` means that the grant ended because its expiry time passed. For a `cli` change made in a non-interactive session, such as a script run from a shell without a controlling terminal, `actor.uid` gives the user identifier of the shell that ran the command.

## Rate limits and summarisation

Refused runtime calls can arrive in large bursts, so the host summarises them. For each plugin and grant that is not HIGH, the host writes at most one `grant-refused` record per minute, and that record carries `count` with the number of refused calls it stands for. HIGH grants keep one record for each refused call. Install-time records are not summarised: each install attempt that is refused by policy writes its own record. See [Errors and limits](/pages/reference-errors-and-limits/) for the limits that apply to calls themselves.

## Query examples

Use the `ilium plugin audit` command to read grant records. The command takes an event name with `--event`, a plugin identifier with `--plugin` and the `--json` flag. With `--json`, it prints one JSON Lines record per audit record, so the output can be read by a script line by line. To list the revocations of one plugin, run `ilium plugin audit --event grant-revoked --plugin example.sync --json`. To check that a single revoke wrote exactly one record, read the output of that command after the revoke. The `event` field of each line names the record type, so the same command works for any event in the event list above. For the grant commands that create these records, see [Plugin CLI](/pages/reference-plugin-cli/).

## Requirements

- **[PDY-013]** `grant-given` and `grant-revoked` records MUST carry `actor.kind` (one of `user-tui`, `cli`, `policy`, `expiry`), `grant`, `previous_state` and `epoch`.
- **[PDY-014]** Revoke all MUST write one `grant-revoked` record per grant plus exactly one `revoke-all` summary record carrying the count.
- **[PDY-015]** The audit catalogue ([SEC-222](/pages/reference-audit-log/)) MUST include `grant-expired` with `plugin_id`, `grant`, `expired_at` and `detected_at`, written once per expiry.
- **[PDY-016]** A grant refused by policy at install time MUST write one `grant-policy-denied` record per install attempt, naming the grant.
- **[PDY-017]** Declining a row in the review dialog MUST write one `grant-declined` record with the grant name.
- **[PDY-018]** Refused runtime calls for grants that are not HIGH MUST be summarised into at most one `grant-refused` record per plugin and grant per minute, carrying `count`; HIGH grants keep their per-call records.
- **[PDY-019]** A grant change made by the CLI in a non-interactive session MUST record the invoking uid in `actor.uid`.
- **[PDY-020]** Every grant audit record MUST be reachable with `ilium plugin audit --event <name> --plugin <id> --json`, printing one JSONL line per record.
