---
layout: page
title: Collecting audit logs from a fleet
permalink: /pages/guide-audit-collection/
---

## Collecting audit logs from a fleet

This page specifies how a central collector receives Ilium audit records from many machines. It gives security teams a stable field mapping to common SIEM schemas, the delivery and ordering rules of syslog forwarding, the way a collector finds gaps and tampering with `seq` and the chain hash, a retention plan that matches the host-side limits, and a minimum alert set. Security engineers and fleet administrators who build or configure a collector need this page. The record format itself is defined in the [Audit log](/pages/reference-audit-log/) reference, and the host settings are described in [Administration](/pages/guide-administration/).

## Collector requirements

A collector keys every record by the machine id together with the `seq` value. The pair identifies one record on one machine. When a pair arrives a second time with a different record hash, the collector treats the repeat as tampering and raises a `tamper` alert that names the machine id and the `seq` value. A collector also checks the chain on every stream it receives. When `seq` skips a value, or when `prev_hash` does not equal the hash of the previous record the collector accepted, it raises a `gap` alert that gives the expected and the received `seq`.

A collector orders records by `seq` and never by `ts`. The host clock can move backwards, so `ts` is not a safe sort key. Records that arrive with a `ts` lower than the previous record, while `seq` increases, are stored in `seq` order and raise no reorder alert.

## Field mapping to common SIEM schemas

Each syslog message carries one audit record as one JSON object. The collector maps the fields of that object to the event schema of the SIEM in use. The fields that the mapping depends on are `ts` (the UTC time in ISO 8601 form with milliseconds, such as `2026-10-12T08:14:02.114Z`), `seq` (the record number on one machine), `prev_hash` (the hash of the previous record), `event` (the event name from the catalogue), `plugin_id`, `version` and `tier` when the event concerns a plugin, and `detail` (a free text field that can be shortened, as described below). Map `ts` to the event time of the SIEM. Keep the machine id, `seq` and `prev_hash` as fields of their own, because the gap and tamper checks need them after the event has been stored.

A record can exceed the size of one syslog message. Each syslog message is one JSON object of at most 8192 bytes. When a record is larger, the host shortens only `detail` and adds `truncated: true`. The collector must read that flag and must not treat a shortened `detail` as a complete value. Every other field of the record stays intact.

## Gap and reorder detection with seq and chain hash

The chain hash is the SHA-256 of the previous line, without its trailing newline. A collector can therefore recompute the hash of each record it accepts and compare it with the `prev_hash` of the next record. A match means the stream is intact up to that point. A mismatch, or a `seq` value that skips, is a gap. Drop one record from a forwarded stream and the reference collector raises a `gap` alert that shows the expected and the received `seq`.

The chain detects edits made by a party that cannot also rewrite the whole file. When a pair of machine id and `seq` arrives again with a different record hash, the record has been changed after the first copy was forwarded. The collector raises a `tamper` alert for that case. Forwarding over [syslog](/pages/guide-enterprise-deployment/) with `[audit] sink = "syslog"` or `"journald"` gives the collector the `seq` value and chain hash of each record, so it can find a missing or reordered record without reading the local file.

## Host retention versus central retention

The host keeps the audit log in the user's state folder. The file rotates at `audit.max_mib` (64 MiB by default), and a rotated file is deleted at server start once it is older than `audit.keep_days` (365 days by default). The total storage ceiling of 256 MiB can remove the oldest rotated files earlier than `keep_days` says, and the host then writes one record that states how many files it removed. Because of that, the host cannot be relied on to keep a long history.

The central store keeps the records for at least as long as the host setting. A collector must hold its records for at least the value of `audit.keep_days` set on the host, and 365 days is the default when no value is set. The collector configuration check fails when its retention is shorter. For example, a collector set to 30 days, checked against a host with `audit.keep_days = 365`, fails its check with `retention-below-host`. Set the central retention first, then confirm the value against the host policy.

## Time and clock handling

Use `ts` for display and for time-based search only. Use `seq` for order, for gap detection and for deduplication. A host clock can jump backwards, so two records can carry times in the wrong order while their `seq` values keep increasing. The collector stores such records in `seq` order and does not raise an alert for that. Do not sort, group or count events by `ts` alone when a correct sequence is needed.

## Recommended alerts

The minimum alert set has four events. Each of the events `policy-violation`, `signature-failure`, `blocklist-hit` and `integrity-failure` raises one alert for each record it appears in. Load the published alert rule file into the collector, then forward one record of each of the four events. The collector must raise four alerts, one per event. The collector also raises the `tamper` and `gap` alerts described above, which are not part of the four events and are raised by the chain checks instead.

## Requirements

- **[PDV-021]** A collector MUST key records by machine id plus `seq` and MUST treat a repeated pair arriving with a different record hash as tampering.
- **[PDV-022]** A collector MUST raise a gap alert when `seq` skips a value or when `prev_hash` does not equal the hash of the previous accepted record.
- **[PDV-023]** Central retention MUST be at least the host `audit.keep_days` value (default 365).
- **[PDV-024]** Each syslog message MUST be one JSON object of at most 8192 bytes, and a record exceeding that MUST be truncated in `detail` only, with `truncated: true` added.
- **[PDV-025]** The documented minimum alert set MUST include records with event `policy-violation`, `signature-failure`, `blocklist-hit` and `integrity-failure`.
- **[PDV-026]** Collectors MUST order records by `seq`, never by `ts`, because the host clock may move backwards.

## Production additions (round 2)

Operators with many machines need to know how much audit data to expect before they size syslog sinks, disks and collectors. The manual therefore publishes a measured upper bound for audit bytes per plugin-hour, taken on the reference workload.

The host also counts the audit bytes and records it writes and shows both counters in `ilium plugin metrics`. An operator compares these counters with the published bound to check the collector and disk budget.

- **[PRX-014]** The manual MUST state a measured upper bound for audit bytes per plugin-hour on the reference workload, and the host MUST expose `ilium_audit_bytes_total` and `ilium_audit_records_total` in `ilium plugin metrics` so an operator can verify the collector and disk budget against that bound.
