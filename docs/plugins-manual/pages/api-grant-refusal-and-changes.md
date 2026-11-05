---
layout: page
title: Refusal results and grant-change notification
permalink: /pages/api-grant-refusal-and-changes/
---

## Refusal results and grant-change notification

This page specifies what a plugin observes when a capability call is refused and when its grant set changes. It covers the exact refusal result shape, the reasons a plugin can see, how a plugin reads its current grants, the `grant-set-changed` event, and the rules for retrying a refused call. Plugin authors need it to handle refusals without guessing, and host authors need it to build the refusal path. The summary of refusals for auditing is on the [audit log](/pages/reference-audit-log/) page.

## Refusal result shape

A call that the host refuses returns the `not-permitted` result. The result carries two fields, `grant` and `reason`. The `grant` field names the capability that was asked for, such as `fs.read`. For example, a call to `fs.read` from a plugin whose `fs.read` grant was revoked returns `not-permitted` with `grant: "fs.read"` and `reason: "revoked"`, and the result has no other fields.

The `reason` field takes exactly one of four values: `declined`, `revoked`, `not-granted` or `policy`. A plugin can branch on these four values.

## Reasons a plugin can see

Each reason tells the plugin why the call was refused, and each one leads to a different plugin behaviour.

- `declined` means the user refused the grant when the consent prompt was shown.
- `revoked` means the grant was held earlier and the user has since withdrawn it.
- `not-granted` means the grant is not in the set the host offered to the plugin.
- `policy` means a policy file, managed by an administrator, blocks the grant.

A refusal with reason `policy` never reveals which policy file, key or rule matched. The serialized result carries no file path, no key name and no rule id. For example, when `net.fetch` is denied for a plugin in the system policy file, the result carries `reason: "policy"` and nothing that names the file. A plugin can show the user that its capability is blocked by policy, but it cannot learn the policy source from the result.

A refused call returns within 50 ms. A refusal does not open a prompt unless the grant is defined with first-use consent. A harness that calls an un-granted capability without first-use consent 100 times sees every call return `not-permitted` in under 50 ms, and it records no prompts.

## Reading the current grant set

A plugin reads its current grant set with `grants.current`. The result lists only the grants that the plugin's manifest requests, each with its state. A plugin never sees a grant it was not offered, even when the user holds grants for other plugins. For example, a plugin whose manifest requests `fs.read` and `clipboard.read` gets at most those two names from `grants.current`, whatever other grants the user holds.

A plugin should call `grants.current` at start-up, and again when it receives a `grant-set-changed` event, so that its view of its own grants stays current.

## The grant-set-changed plugin event

The host tells a plugin about a change of its grant set with the plugin event `grant-set-changed`. The host delivers this event within 1 s of each epoch bump. The event carries three values and nothing else: the `added` grant names, the `removed` grant names and the new `epoch`. For example, when the user revokes `fs.read` with the command line while the plugin runs, the plugin's handler receives `removed: ["fs.read"]` and `added: []`, together with the new epoch, within 1 s.

This event is distinct from the `grants-changed` control record, which the server sends to the client. A plugin author must not confuse the two: the plugin reacts to `grant-set-changed`, and the control record is not a plugin event.

## Retry rules

A refusal with reason `declined`, `revoked` or `policy` is final until the host changes the grant set. The plugin must not retry such a call until it receives a `grant-set-changed` event that adds the grant. Until then, the host returns `not-permitted` without a prompt on every retry. For example, a call refused as `declined` that is retried ten times with no intervening grant event returns `not-permitted` with reason `declined` every time, and the harness records no prompts.

A plugin that retries anyway wastes its own calls and gains nothing, so it should wait for the event and then read `grants.current` before it tries again.

## Requirements

- **[PDY-007]** A refused call MUST return the `not-permitted` result with the fields `grant` and `reason`, and `reason` MUST be exactly one of `declined`, `revoked`, `not-granted` or `policy`.
- **[PDY-008]** A refusal with reason `policy` MUST NOT reveal which policy file, key or rule matched.
- **[PDY-009]** A refused call MUST return within 50 ms and MUST NOT open a prompt unless the grant is defined with first-use consent.
- **[PDY-010]** The host MUST deliver the plugin event `grant-set-changed` within 1 s of each epoch bump, carrying only the `added` and `removed` grant names and the new `epoch`; it is distinct from the server-to-client `grants-changed` control record.
- **[PDY-011]** A plugin MUST be able to read its current grant set through `grants.current`, and the result MUST list only grants its manifest requests, never grants it was not offered.
- **[PDY-012]** A plugin MUST NOT retry a call refused with reason `declined`, `revoked` or `policy` until it receives a `grant-set-changed` event that adds the grant, and the host MUST return `not-permitted` without a prompt on such retries.
