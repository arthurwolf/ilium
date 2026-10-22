---
layout: page
title: Plugin privacy
permalink: /pages/user-privacy/
---

## Plugin privacy

This page answers three questions about plugins and your privacy. What can a plugin see? Where can it send what it sees? What does Ilium itself send about plugins? It also tells you where plugin data is stored on your machine and how to delete it.

Contract version: `ilium:plugin@0.1.0`. Requirement ids have the form `[USR-nnn]`, collected in [User and administrator requirements](/pages/reference-requirements-user-admin/).

<div class="note">
<p>Decision status. The platform is not implemented. The statements here are the specified behaviour. A plugin you install is software written by someone else. The permission system limits it, but it cannot make a dishonest plugin honest. Install only what you trust.</p>
</div>

### Contents

- The short version
- What a plugin can see
- Where a plugin can send data
- What Ilium itself sends
- What is stored on your machine
- Delete a plugin's data
- Crash records and pre-migration backups
- Core plugins and AI providers
- Errors and edge cases
- Requirements

### The short version

- A plugin sees only what you grant, plus its own folders. See [Permissions explained](/pages/user-permissions-explained/).
- A plugin can send data only to hosts you granted, over HTTPS. Plugins cannot open a raw network connection.
- Ilium does not report plugins, their ids or their use to anyone. It has no plugin telemetry.
- Everything about a plugin is stored in your own folders. You can delete it all.

- [USR-060] Ilium MUST NOT send the list of installed plugins, plugin use, plugin logs or plugin errors to any remote party. Test: run Ilium for one hour with five plugins and request logging, and assert that no request carries a plugin id.

### What a plugin can see

| A plugin can see | Only if |
|---|---|
| The names and structure of your project tree | you granted `tree.read` |
| Titles, sizes and focus of panes | you granted `panes.read` |
| The text in a terminal pane | you granted `terminal.read-screen` or `terminal.read-cells` |
| Older terminal lines | you granted `terminal.history` (HIGH) |
| What you typed to a coding agent | you granted `agent.read-prompts` (HIGH) |
| An agent's saved conversation | you granted `agent.transcript` or `fs.read:agent-sessions:<agent>` (HIGH) |
| Files in the project folder | you granted `fs.read:project` |
| Files in other folders | you granted a read of that specific folder |
| What you copied | you granted `clipboard.read` (HIGH) |
| Your voice | you granted `audio.capture` (HIGH) |
| Sound your computer plays | you granted `audio.loopback` (HIGH) |
| Your approximate location | you granted `location.observer` (HIGH) |
| Your key presses | you granted `keys.intercept` (HIGH) |
| Another plugin's data | never directly. Only through a service or a message that the other plugin chose to offer, and you granted |

A plugin never sees the process environment, your home folder as a whole, other users' files, or the memory of Ilium or of other plugins. It sees what Ilium passes to it and nothing else.

Terminal text may contain passwords, keys and private notes. Think twice about terminal permissions for a plugin whose job does not need them.

- [USR-061] A plugin MUST NOT read terminal text, history, agent prompts, transcripts, the clipboard, audio or location without the matching grant. Test: for each item call the function without the grant and assert `not-permitted`.

### Where a plugin can send data

A third-party plugin runs in a sandbox with no network of its own. The only ways data can leave are listed here.

| Way out | What guards it |
|---|---|
| `http:<host>` | You granted that exact host or pattern. Only HTTPS, except for the loopback hosts `localhost`, `127.0.0.1` and `[::1]` when the grant names them. A redirect to another host needs its own grant |
| `http:setting:<key>` | The host comes from a setting. Ilium asks you the first time for each new host |
| `inference` | The plugin uses the AI providers you set up. What it sends goes to the provider you chose, as described in [Inference providers](/pages/core-plugin-inference/) |
| `location.geocode` | A place name goes to an online lookup service, Photon, with Nominatim as fallback. Ilium asks the first time |
| `open-external` | Ilium asks before it opens each site in your browser. The browser then contacts the site |
| `process.run:<program>` | The program runs in the same sandbox, with no network |
| `notify.desktop`, `sound.play` | Local only |
| Messages to another plugin | Needs `messages.peer:<plugin-id>`. The other plugin may have its own grants for the network |

A plugin that holds both a read permission and a network permission can send what it reads to that host. The dialog does not warn you about the combination. Think about it yourself: a notes plugin that reads terminal text and can talk to `*.example.com` can pass that text to example.com.

- [USR-102] A T2 or T3 plugin MUST have no network access except through `http:` grants and the providers named above. Test: from a test plugin, try a raw socket and a request to an ungranted host, and assert failure.
- [USR-103] Ilium MUST refuse a request to a host that the plugin has no grant for, including a redirect target. Test: redirect from a granted host to an ungranted host and assert `not-permitted`.

### What Ilium itself sends

Ilium does not contact any server on its own to check, list, download or update plugins. The only exception is a registry you set up yourself: the store then fetches that registry's metadata when you open it (see [the plugin store](/pages/user-plugin-store/)), and nothing else is fetched. Installing a plugin does not use the network. Reading a package does not use the network. Verifying a signature does not use the network, because the keys you trust are stored on your machine.

Ilium's own features that use the network are unchanged: for example, AI providers you configure. Core plugins contact the same services as before. Their page in Settings lists them. See [Inference providers](/pages/core-plugin-inference/).

- [USR-062] Install, verify, update, uninstall and list of local packages MUST NOT use the network; only the store's registry fetch from a registry the user configured (the refresh sequence of PKG-370 in [the registry reference](/pages/reference-registry/)) may, and never with no registry configured (PKG-532). Test: run each in a network namespace without interfaces and assert success.

### What is stored on your machine

| What | Where | Who can read it |
|---|---|---|
| Installed packages | your plugin store | you |
| Your permissions and answers | `grants.json` and `consents.json`, one file each for all plugins | you |
| Plugin settings | your `config.toml` and the project's `.ilium` folder | you, and anyone who gets those files |
| Plugin data and notes | one folder and database per plugin | you and that plugin |
| Plugin logs | one file per plugin | you |
| Crash records | the server state folder `<state>`, never a plugin's folder | you |
| Pre-migration backups | `<data>/plugins/backups/<id>/<version>-<utc timestamp>/`, copies of the plugin's data and storage buckets | you |
| Audit log | always on; nothing to enable | you and the administrator |

A project's `.ilium` folder may hold plugin settings and the plugin's project storage. If you share or commit that folder, you share them. Permissions are never stored there. The exact paths are in [Plugin files on disk](/pages/reference-plugin-files/).

Plugin logs are plain text. Plugins are told not to write secrets there, but Ilium cannot check. Read a log before you share it.

### Delete a plugin's data

| I want to delete | Do this |
|---|---|
| Everything of one plugin | `ilium plugin uninstall <id> --purge` |
| A plugin's settings only | Recovery page > Forget settings |
| A plugin's logs | `--purge` |
| A plugin's pre-migration backups | `ilium plugin uninstall <id> --purge` |
| A plugin's crash records | Decision needed, see [Crash records and pre-migration backups](#crash-records-and-pre-migration-backups) |
| One project's plugin data | Delete the plugin's files in the project's `.ilium` folder, or use `--purge` while the project is open |
| My trusted publisher keys | `ilium plugin trust remove <key_id>` |
| Everything | Remove the plugin folders listed in [Plugin files on disk](/pages/reference-plugin-files/) with Ilium stopped |

`--purge` also records a purge note in Ilium's data folder (`<data>/plugins/purged.json`, holding the plugin id and the purge time). The next time you open a project that holds data of that plugin, Ilium deletes the plugin's project and session data and then removes that project from the note. A project you never reopen keeps its data, and the note stays until every known project has been cleaned. A plugin that was uninstalled without `--purge` leaves its data inert: no other plugin can read it, and the same plugin reads it again if you reinstall.

### Crash records and pre-migration backups

Two kinds of plugin data are kept outside the plugin's own folder. Both appear in the tables above.

Crash records are written to the server state folder `<state>` when a plugin helper crashes, is killed or runs out of memory. Each record holds the time in UTC, the plugin id and version, the side (`t1`, `t2` or `t3`), the reason (`crash`, `kill` or `out-of-memory`), the exit status and the name of the export that was running. A record holds no payloads, stack memory or file contents. Run `ilium plugin support-bundle <id> --out <file>` to copy the records into the bundle's `crashes.txt` before you remove anything.

- [USR-236] A crash record MUST be removed once it is more than 30 days old. When more than 200 records exist, the oldest records MUST be removed first until 200 remain. Test: doc check in ilium/tests/plugin_user_flows that this section and the storage table give the 30-day and 200-record limits.

Pre-migration backups are copies of a plugin's data and storage buckets, made before an update migrates them, so that a failed migration can be undone. They are kept in `<data>/plugins/backups/<id>/`. Set `migration_backup_mib = 0` in the `[plugins]` table of `config.toml`, or pass `--no-backup` to the update, and Ilium makes no copy. Without a copy, a failed migration cannot be undone.

- [USR-237] After each committed update, Ilium MUST keep only the newest pre-migration backup of that plugin and delete the older ones. `ilium plugin uninstall <id> --purge` MUST delete every pre-migration backup of that plugin. Test: doc check in ilium/tests/plugin_user_flows that the backup rule and the purge row of the delete table name the newest-copy rule and `--purge`.
- [USR-238] `ilium plugin uninstall <id>` without `--purge` MUST keep the pre-migration backups of that plugin, so that a reinstall finds them. Test: doc check in ilium/tests/plugin_user_flows that this rule matches the uninstall rules of the install and uninstall reference.

Decision needed: should `uninstall --purge` also delete that plugin's crash records? The records name the plugin, but the manual names no command that removes them before the 30-day limit, so a full erase of one plugin leaves them behind. Default: yes. `--purge` is the command for erasing everything about one plugin, so it should delete that plugin's crash records and leave the records of other plugins alone.

### Core plugins and AI providers

Core plugins are made by the Ilium project and follow the same permission rules for high-risk abilities. Some of them send data to services you configure: the Inference plugin sends prompts to the AI provider you pick, the Voice plugin may use a speech service you set up (audio is sent only when that provider is remote, stays in memory, and transcripts are dropped when the voice session ends; see [Voice data and privacy](/pages/core-plugin-voice/#data-and-privacy)), and the Cost plugin may read agents' usage files. These behave as documented in the user documentation. A third-party plugin that asks for `inference` uses the same providers and the same privacy controls: the provider you choose for each purpose, and the AI admission gate, which checks every request against the `purpose` its caller named and records it in the privacy log (see [Inference](/pages/capability-inference/#risk-and-privacy-rules)). No separate setting exists for file names, paths or terminal text: a plugin sees them only through its own read grants, and sends them only if it puts them in a request.

### Errors and edge cases

| Situation | What happens |
|---|---|
| A plugin tries a host without a grant | The call returns `not-permitted`. The log gets `refused http:<host>` |
| You revoke a network grant during a request | The request completes as `failed(revoked)` |
| The audit log | Always on. Install, update, grant, consent, trust and block events are recorded. Plugin data and settings values are not. See [Audit log](/pages/reference-audit-log/) |
| You share a project folder | Plugin settings and project storage in `.ilium` go with it. Permissions do not |
| A plugin writes a secret to its log | Ilium cannot detect it. The log rotates at 4 MiB, keeps four rotated files, deletes rotated files older than 14 days at server start, and the logs of an uninstalled plugin are deleted 7 days after uninstall; `--purge` deletes them at once |

## Personal data export and erasure requirements

- [USR-245] The host MUST let a user export all plugin data held about them with `ilium plugin export-data`, covering plugin settings, project storage references, grants, plugin logs, audit records and crash records, as readable files in one folder. Test: Run the export for a profile with each data kind and assert one folder with a file per kind.
- [USR-246] The host MUST let a user erase the data of one plugin or of all plugins with `ilium plugin erase-data`, after showing counts per data kind and asking for confirmation, and MUST report what was deleted and what was kept. Test: Run erase for one plugin and assert its settings, grants, logs and crash records are gone, others are untouched and the report lists both.
- [USR-247] The export and erase commands MUST work without network access and MUST NOT send any data to a plugin or a registry. Test: Run both with networking blocked and assert success and no outbound request.
- [USR-248] Erasure MUST NOT silently drop audit records that policy requires to keep; the host MUST name them as kept, with the retention period that keeps them, and MUST delete them when that period ends. Test: Set a retention policy, erase, and assert the audit records remain and are named; advance the clock and assert they are deleted.

### See also

- [Plugins for users](/pages/user-plugins-overview/)
- [Permissions explained](/pages/user-permissions-explained/)
- [Plugin files on disk](/pages/reference-plugin-files/)
- [Security](/pages/security/)
- [Plugin FAQ](/pages/user-faq/)

API version checked: `ilium:plugin@0.1.0`.
