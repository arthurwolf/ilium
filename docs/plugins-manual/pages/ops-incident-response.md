---
layout: page
title: Operational incident response
permalink: /pages/ops-incident-response/
---

## Operational incident response

This page sets the severity scale and the response clock for operational incidents: a host defect that stops the server or loses data, a registry outage, or a fault that many installs share. A vulnerability in the host or in a plugin follows the disclosure clocks of [Security response](/pages/reference-security-response/) first. The operational effects of that vulnerability then use the scale below. The administrator's record of each incident is defined in the [Administrator failure runbook](/pages/guide-admin-failure-runbook/).

### You need

- The role names in the table below, and a named person for each role in your own change record.
- The [Administrator failure runbook](/pages/guide-admin-failure-runbook/), which maps each symptom to one level.

### Severity levels

| Level | Meaning | Examples | Acknowledgement time (proposed, owner to confirm) | Owner |
|---|---|---|---|---|
| Level 1, critical | User data was lost or corrupted, or a host defect stops the server on many installs | A crash loop on many installs; data loss caused by a host defect; a compromised signing key | 1 hour, at any time of day | Host on-call maintainer, who opens the incident |
| Level 2, high | A shared service is down, or one fault stops work for a whole site | Registry unavailable; crash loop on every machine of one site | 4 hours in working hours | Registry operator for registry faults; host maintainer for other faults |
| Level 3, medium | Behaviour is degraded, and a workaround exists | A plugin traps, is slow or fails an update; watchdog timeouts | 1 working day | Release manager, who triages the incident |
| Level 4, low | One machine or one plugin, with a workaround | A single machine's log question with no lasting effect | 3 working days | Support queue owner |

### Requirements

- **[OPS-230]** The project MUST publish a four-level severity scale for operational incidents, with an acknowledgement time and an owner for each level, as the table of this page sets them.

### Choosing a level

1. Start from the examples. Choose the highest level whose meaning matches the incident.
2. When two symptoms occur together, open one incident and take the highest level of the two.
3. When you cannot decide between two levels, take the higher one. Lower it later and write the reason in the incident record.

The acknowledgement clock starts when the incident is first noticed, which is the time recorded in the incident record.

### Edge cases

- **The owner does not acknowledge in time.** Pass the incident to the next role in the same level, then to the project lead. Record each hand-over with its time.
- **The owner is not named yet.** Until your change record names a person for a role, the role itself is the owner, and the incident record shows the role as "unknown" for the person.
- **The incident is a data-exposure case.** Use the clocks of the [Data-exposure incident](/pages/guide-admin-failure-runbook/#data-exposure-incident) section of the runbook in addition to the level.

### See also

- [Administrator failure runbook](/pages/guide-admin-failure-runbook/) for the triage table and the incident record.
- [Security response](/pages/reference-security-response/) for vulnerability reports.
- [Diagnostics and logs for operators](/pages/ops-diagnostics-and-logs/) for the logs that an incident needs.
- [Telemetry and plugin health](/pages/reference-host-telemetry/) for the host series that show a host fault.

## Production additions (round 3)

- **[PRY-038]** An incident record MUST contain severity, start time in UTC, affected plugin ids, the action taken and the closing time, and `ilium plugin alerts` MUST re-raise an unacknowledged severity-1 alert every 15 minutes until it is acknowledged.
