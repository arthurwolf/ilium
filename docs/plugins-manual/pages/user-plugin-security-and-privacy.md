---
layout: page
title: Plugin security and privacy for users
permalink: /pages/user-plugin-security-and-privacy/
---

## Plugin security and privacy for users

This page is for people who use Ilium, not for people who write plugins. It says what Ilium and its plugins keep on your computer, what can leave it, how to read the install screen, and what to do if a plugin misbehaves. The technical rules behind it are in [Security model](/pages/security/).

### You will need

- Ilium with the plugin platform.
- To read the install screen, nothing else. To delete data, the Settings screen or the `ilium plugin` command.

## What Ilium collects

**[SEC-260]** Ilium and its bundled plugins send no telemetry, crash reports or usage data. If a future version collects anything, it will be opt-in, off by default, and listed on this page before it ships.

A third-party plugin can send data only to hosts you allowed in its install screen, and only through the rules in [Network egress](/pages/reference-network-egress/).

## What is stored on your computer

**[SEC-261]** The table lists every place the plugin platform writes. A file in this table is created by Ilium and is readable only by you.

| What | Where (Linux) | Retention | How to delete |
|---|---|---|---|
| Plugin files | `~/.local/share/ilium/plugins/` | Until uninstall | `ilium plugin uninstall <id>` |
| Plugin data and cache | Inside the plugin's own folder | Until uninstall or cache eviction | Uninstall, or the Data tab buttons Clear cache and Delete data |
| Storage buckets | The plugin's data folder | Per bucket policy | Clear data |
| Grants and consents | `~/.local/state/ilium/grants.json`, `consents.json` | Until you revoke or uninstall | Settings > Plugins |
| Trusted publisher keys | `~/.local/state/ilium/trust.json` | Until you remove them | `ilium plugin trust remove` |
| Secrets | OS keyring, or `~/.local/state/ilium/secrets.json` | Until you forget them | Settings > Plugins > Secrets > Forget |
| Plugin logs | Ilium's log folder | Rotates at 4 MiB; keeps the active file and four rotated files (`.log.1` to `.log.4`), oldest deleted on rotation, and rotated files older than 14 days deleted at server start; logs of an uninstalled plugin deleted 7 days after uninstall, or at once with `--purge` | `ilium plugin logs --clear <id>` |
| Audit log | `~/.local/state/ilium/audit/` | 365 days or 64 MiB | Delete the folder; the next start begins a new chain |
| Privacy log | `~/.local/state/ilium/privacy/inference.jsonl` | 90 days or 16 MiB | Settings > Privacy > Clear |
| Pane journal | Ilium state folder, per session | Newest entries only | Closes with the session |
| Session snapshots | `<project>/.ilium/sessions/` | Until you delete them | Delete the file |

## What can leave your computer

**[SEC-263]** Only the features in the inventory of [Network egress](/pages/reference-network-egress/) send data off the machine. The two that matter most:

- **AI model requests.** The default provider is the remote Kilo Gateway, with no key. Text a plugin sends to the AI model goes to that service. Choose a local provider in Settings > Inference if you do not want that.
- **Place lookups.** Place names go to a public geocoding service, after you agree once.

**[SEC-264]** The install screen of a plugin that asks for `inference` names the provider that is selected now and says `remote` or `local`.

### The privacy log

**[SEC-265]** Every request to an AI model made through Ilium is recorded in the privacy log, `inference.jsonl`, one JSON object per line: `ts` (UTC, ISO 8601), `plugin` (id), `purpose` (a short label the plugin gives), `provider`, `model`, `tokens_in`, `tokens_out`, `bytes_out`, `bytes_in`, `local` (true or false). It never contains the text. Retention is 90 days or 16 MiB, whichever is smaller. Settings > Privacy shows the log; `ilium privacy log [--since <time>] [--plugin <id>]` prints it as JSONL. The privacy log belongs to the kernel and a replacement of the inference plugin cannot disable it.

## Reading the install screen

- **HIGH** means the permission can change things outside the plugin or read sensitive data (running programs, reading scrollback, typing into terminals). A HIGH row starts off. Turn it on only if you need the feature it names.
- **This plugin can read your data and send data out** means the plugin asked for something that reads your terminal, files or agents, and for something that sends data to a server. Many useful plugins do. Check that you know the plugin and its publisher.
- **Details** (press Tab) shows the full key fingerprint, the file hashes and the publisher's contact.
- **Signed (T2)** means a publisher you trust signed it. **dev** means it is unsigned and runs from a local file.
- The publisher name is what the publisher wrote about itself. Only the key fingerprint is verified.

You can change your mind any time: Settings > Plugins > Capabilities revokes one permission, and **Revoke all** removes them all. If a plugin crashes Ilium or you doubt it, start Ilium with `ilium --safe-mode` to turn off all third-party plugins, then uninstall the one you doubt ([Security response](/pages/reference-security-response/)).

To report a bad plugin, use the contact in its Details. For a flaw in Ilium itself, follow [Security response](/pages/reference-security-response/).

## If you suspect a plugin is malicious

Act in this order. Steps 1 and 2 take under a minute and lose nothing; the later steps are destructive or send information, so do them after you know what happened.

1. **Stop it now.** Run `ilium plugin kill <id> --reason <text>`, or use the kill action for the plugin in the Plugins screen (Settings > Plugins). Kill stops the plugin on the server and in every attached client, sets it to `disabled-by-admin`, cancels its pending requests as `cancelled`, and writes `plugin.killed` to the audit log with the reason and the time [HST-516, HST-517]. The server stops within 1 s and each attached client within 5 s. Kill differs from disable: a disabled plugin can be turned on again by anyone with the Space key or `ilium plugin enable`, while a killed plugin is refused at activation, also after a restart, until you run `ilium plugin unkill <id>`. Kill works in safe mode and with no network. It keeps the plugin's grants, settings and data, so the evidence in step 2 stays. If you do not know which plugin it is, start Ilium with `ilium --safe-mode` first ([Security response](/pages/reference-security-response/)), then kill the one you find.
2. **Check what it reached.** Run `ilium plugin audit --plugin <id> --since 7d` [SEC-227]. Read the `egress-host-first-use` records (hosts it contacted for the first time), the process-run records (programs it started) and the records of writes outside its own data folder ([Audit log](/pages/reference-audit-log/)). Run `ilium plugin audit --verify` to check that the log was not edited. The log is local and shows what the host saw; a plugin that was granted a wide capability can do more than one record shows.
3. **Rotate every secret it could read.** For each secret the plugin held or was granted, set a new value at the service that issued it first, then run `ilium plugin secret rotate <id> <name>` to replace the stored value [SEC-451]. The old value is overwritten in the secret store and the audit log records the rotation without the value. Rotating the stored value does not cancel the old one at the remote service; revoke it there.
4. **Report it.** Run `ilium plugin report <id>`. It prints the plugin's `contact` and `security_contact`, its version and its key fingerprint, and sends nothing by itself, even with the network on [USR-349]. Then send that text yourself to the plugin's `security_contact` and to the registry abuse contact. The registry acknowledges a report within 1 business day and decides on active malware or a stolen key within 24 hours (yank or takedown, or a stated reason for none), and records the decision in its transparency log [PKG-705]. A takedown or a `compromised` key revocation then shows the plugin as `Blocked` for everyone who installed it [USR-174].
5. **Deal with the publisher key if the publisher is the problem.** If you trust the key for other plugins, run `ilium plugin trust remove <key_id>` (add `--force` when plugins depend on it). The other plugins from that key stay installed and show `untrusted` [DEV-127]. Say in the report that the key may be stolen, so the registry can revoke it.
6. **Remove it.** Run `ilium plugin uninstall <id> --purge` to delete the plugin, its data and its logs at once. If you want to keep the evidence for the report, export the audit records first (`ilium plugin audit --plugin <id> > audit.jsonl`). To erase what an ordinary uninstall leaves behind (crash records, consent records), run `ilium plugin erase-data` for that plugin; it shows counts per data kind, asks for confirmation (the default is No) and reports what was deleted and what was kept [USR-246].

Failure modes:

| Situation | Result |
|---|---|
| `kill` or `unkill` names an id that is not installed | Exit code 5, `not found` |
| No session is running | Exit code 6; start Ilium, or start it in safe mode, and run the command again |
| A client is offline when you run `kill` | It learns the killed state before it activates the plugin when it reconnects |
| `unkill` on a plugin that is not killed | No change; the command says so |
| You `unkill` a plugin that a registry takedown or key revocation blocks | It stays `Blocked` (`registry.takedown` or `trust.key-revoked`) |

## Rules for plugin authors

**[SEC-266]** A plugin that sends telemetry MUST make it opt-in, through a setting that is visible in its settings page and that defaults to off.

**[SEC-267]** A plugin that collects or sends data other than what its function needs MUST declare it in the optional manifest key `data_use`, a list of objects `{ "what": string, "where": string, "purpose": string, "opt_in": bool }`, with at most 8 entries and 200 characters per field. The review dialog shows each entry under **Data this plugin collects**. The key is part of contract 0.1.0; a host that does not know it refuses the manifest with `manifest.unknown-key`, so the key is listed in the [Manifest reference](/pages/reference-manifest/).

**[SEC-268]** A plugin SHOULD set `security_contact` (an email address or an https URL, at most 200 characters) in its manifest; Details shows it.

## Edge cases

| Situation | Result |
|---|---|
| Local provider selected | `inference` rows say `local`; nothing leaves the machine |
| Offline switch on | AI requests and lookups fail with `offline` |
| Privacy log disk full | Oldest records are dropped first, a notice is shown |
| You clear the audit log | The next start begins a new hash chain, recorded in the first record |

### See also

- [Security model](/pages/security/)
- [Network egress](/pages/reference-network-egress/)
- [Secrets](/pages/reference-secrets/)
- [Audit log](/pages/reference-audit-log/)
- [Grant review and consent](/pages/reference-grant-review-and-consent/)

Contract: `ilium:plugin@0.1.0`.

## Production additions: T7-users-privacy-a11y

- [USR-339] Every window or dialog drawn by a T2 or T3 plugin MUST carry a host-drawn title line containing the plugin name and a tier badge that the plugin cannot change or cover, and a plugin MUST NOT draw into the row holding it.
