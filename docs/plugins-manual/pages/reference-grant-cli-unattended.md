---
layout: page
title: Granting and revoking without a terminal
permalink: /pages/reference-grant-cli-unattended/
---

## Granting and revoking without a terminal

This page specifies how the `ilium plugin grant` and `ilium plugin grants` commands behave when no Ilium client is attached to the session. Operators who script fleets of machines need to know which operations work headless, which need the terminal user interface (TUI), what each command prints, which exit code it returns, and how an administrator's policy allow-list settles consent before any prompt appears. The grant model itself is described in [Grant review and consent](/pages/reference-grant-review-and-consent/).

## ilium plugin grants commands

The command `ilium plugin grant <id> <grant>` asks the host to grant one permission to one plugin. A high-risk grant, such as `shell.exec`, needs a person to approve it in a client. When no client is attached, the command refuses the grant unless the policy allow-list names both the plugin and the grant. For example, `ilium plugin grant example.sync shell.exec` on a headless machine with an empty allow-list exits with code 3 and prints the code `grant.needs-interactive`. The grant is not stored, so `ilium plugin grants example.sync --json` does not list `shell.exec` afterwards.

The command `ilium plugin grants <id> --revoke <grant>` removes one grant. It works without an attached client, because revoking a permission narrows what a plugin may do and needs no approval. The command exits 0 only after the authorization epoch has been incremented and that new value has been written to the grant store. An operator who reads the grant store file straight after a successful revoke sees the incremented epoch. The grant store is the `grants.json` file in the Ilium user state directory described in [Grant review and consent](/pages/reference-grant-review-and-consent/).

Revoking a grant that the plugin does not hold is a failure. The command exits 4 and prints the code `grant.not-held`. It changes nothing, and the epoch in the grant store stays at its previous value.

## JSONL output and exit codes

Every grant command prints exactly one JSONL result line. The line is a single JSON object with the four keys `type`, `plugin_id`, `grant` and `epoch`, and no other keys. For example, `ilium plugin grants example.sync --revoke fs.read --json` writes one line of that shape to standard output and nothing else. Scripts can therefore read one line per command without any further parsing rules.

The exit codes used by the unattended grant commands are these:

- **0**: the revoke succeeded and the epoch bump is in the grant store.
- **3**: a high-risk grant needs an attached client, and the output contains `grant.needs-interactive`.
- **4**: the revoke named a grant the plugin does not hold, and the output contains `grant.not-held`.

Scripts should check the exit code first and then read the code in the output. The codes are stable identifiers, so a script can match them as text. Other error codes are listed in [Errors and limits](/pages/reference-errors-and-limits/).

## What needs a TUI

Granting a high-risk permission needs a person who confirms it in an attached client. A headless server can therefore grant nothing that is high-risk unless the administrator has listed it in advance. Revoking, by contrast, works in every session, with or without a client.

A first-use consent is a one-off answer that a plugin asks for when it first tries an action, such as opening an external address. In a headless session no person can answer that question, so the consent is not left pending. The host resolves it as `declined` at once. The capability call returns `not-permitted` with the reason `declined`, and the audit log gains one `grant-declined` line whose `actor.kind` is `"policy"`. The audit record format is described in [Audit log](/pages/reference-audit-log/).

## Policy preseeding

An administrator can settle consent for a fleet before any headless session starts. The policy file's `preapproved` list names a plugin id and the high-risk grants that plugin may hold without a dialog. When a plugin and grant pair appears in that list, `ilium plugin grant` succeeds without an attached client, and the normal `needs-interactive` refusal does not apply. A grant that is not in the list keeps the refusal described above. The policy file format and the rules for the list are in [Admin policy](/pages/reference-admin-policy/).

Preseeding is the only way to grant a high-risk permission without a client. Scripts must not try to work around the refusal by editing the grant store, because the host does not read external edits until its next start and audits them as `store-external-edit`.

## Requirements

- **[PDY-021]** `ilium plugin grant <id> <grant>` for a HIGH grant without an attached client MUST fail with exit code 3 and the code `grant.needs-interactive`, unless a policy allow-list names that plugin and grant.
- **[PDY-022]** `ilium plugin grants <id> --revoke <grant>` MUST work without an attached client and MUST exit 0 only after the epoch bump has been written to the grant store.
- **[PDY-023]** Every grant command MUST print exactly one JSONL result line with the fields `type`, `plugin_id`, `grant` and `epoch`.
- **[PDY-024]** A first-use consent that would prompt in a headless session MUST resolve as `declined`, return `not-permitted` with reason `declined`, and write a `grant-declined` audit record with `actor.kind: "policy"`.
- **[PDY-025]** Revoking a grant the plugin does not hold MUST exit 4 with the code `grant.not-held` and MUST change nothing, including the epoch.
